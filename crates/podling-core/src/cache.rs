//! Content-addressed, on-disk cache for stage outputs.
//!
//! A stage's output is stored under a key hashed from everything that could
//! change it: the stage id, the stage's version, its input, and its config.
//! Same key ⇒ same output, so a cached value can be reused without re-running
//! the stage.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use podling_types::{ContentHash, SCHEMA_VERSION};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CacheKey(ContentHash);

impl CacheKey {
    pub fn new(
        stage_id: &str,
        stage_version: u32,
        input: &impl Serialize,
        config: &impl Serialize,
    ) -> Result<Self> {
        let input = canonical_json(input)?;
        let config = canonical_json(config)?;
        Ok(Self(ContentHash::of_parts(&[
            stage_id.as_bytes(),
            &stage_version.to_le_bytes(),
            &input,
            &config,
        ])))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for CacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// JSON with object keys sorted, so logically equal values hash equally.
///
/// Relies on `serde_json::Value` using a sorted map, which holds as long as
/// serde_json's `preserve_order` feature is off (see the workspace manifest).
fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value)?;
    Ok(serde_json::to_vec(&value)?)
}

#[derive(Serialize)]
struct EntryRef<'a, T> {
    schema_version: u32,
    stage: &'a str,
    value: &'a T,
}

#[derive(Deserialize)]
struct Entry<T> {
    value: T,
}

#[derive(Deserialize)]
struct EntryHeader {
    schema_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub entries: u64,
    pub bytes: u64,
    /// Binary outputs (audio) in the blob store, counted apart from entries.
    pub blobs: u64,
    pub blob_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct DiskCache {
    root: PathBuf,
}

impl DiskCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The blob store kept beside the entries, in `<root>/blobs`.
    pub fn blobs(&self) -> BlobStore {
        BlobStore::new(self.root.join(BLOBS_DIR))
    }

    fn path_for(&self, key: &CacheKey) -> PathBuf {
        let hex = key.as_str();
        self.root.join(&hex[..2]).join(format!("{hex}.json"))
    }

    /// Returns the cached value, or `None` on a miss. An entry that can't be
    /// read as `T`, or that was written under another schema version, is a
    /// miss (logged as a warning), never an error.
    pub fn get<T: DeserializeOwned>(&self, key: &CacheKey) -> Result<Option<T>> {
        let path = self.path_for(key);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(CoreError::io(path, err)),
        };
        match serde_json::from_slice::<EntryHeader>(&bytes) {
            Ok(header) if header.schema_version != SCHEMA_VERSION => {
                tracing::warn!(%key, found = header.schema_version, expected = SCHEMA_VERSION,
                    "cache entry has another schema version; treating as a miss");
                return Ok(None);
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(%key, %err, "unreadable cache entry; treating as a miss");
                return Ok(None);
            }
        }
        match serde_json::from_slice::<Entry<T>>(&bytes) {
            Ok(entry) => Ok(Some(entry.value)),
            Err(err) => {
                tracing::warn!(%key, %err, "cache entry does not match the expected type; treating as a miss");
                Ok(None)
            }
        }
    }

    /// Stores a value. The write is atomic: the entry is written to a
    /// temporary file in the same directory and renamed into place, so a
    /// reader never sees a partial file.
    pub fn put<T: Serialize>(&self, key: &CacheKey, stage: &str, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec(&EntryRef {
            schema_version: SCHEMA_VERSION,
            stage,
            value,
        })?;
        write_atomic(&self.path_for(key), &bytes)
    }

    /// Entries, and the blob store's contents counted separately.
    pub fn stats(&self) -> Result<CacheStats> {
        let (entries, bytes) = count_sharded(&self.root, JSON)?;
        let (blobs, blob_bytes) = count_sharded(self.blobs().root(), WAV)?;
        Ok(CacheStats {
            entries,
            bytes,
            blobs,
            blob_bytes,
        })
    }

    /// Deletes every cached entry and blob, then any directories left empty.
    ///
    /// Safety: only files shaped like cache entries (`<2 hex>/<64 hex>.json`,
    /// and `blobs/<2 hex>/<64 hex>.wav`, plus leftover temp files in those
    /// shards) are removed. Anything else is left alone with a warning, so
    /// pointing `--cache-dir` at the wrong directory cannot delete it.
    pub fn clear(&self) -> Result<()> {
        let blobs = self.blobs();
        for entry in read_dir_if_exists(&self.root)? {
            if entry.is_dir() && entry == blobs.root() {
                blobs.clear()?;
            } else if entry.is_dir() && is_shard_dir(&entry) {
                clear_shard(&entry, JSON)?;
            } else {
                tracing::warn!(path = %entry.display(), "not a cache entry; leaving it");
            }
        }
        remove_dir_if_empty(&self.root)
    }
}

const BLOBS_DIR: &str = "blobs";
const JSON: &str = ".json";
const WAV: &str = ".wav";

/// Binary outputs (chunk audio), stored under the BLAKE3 hash of their own
/// bytes: `<root>/<2 hex>/<64 hex>.wav`.
///
/// Content addressing makes a blob immutable: the same bytes always land at
/// the same path, so writing one twice is a no-op, and a reader can check
/// what it got against the name. A JSON cache entry refers to a blob by its
/// hash; a stage that finds the blob gone treats the entry as a miss.
#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.as_str();
        self.root.join(&hex[..2]).join(format!("{hex}{WAV}"))
    }

    /// Stores `bytes` and returns their hash. Atomic, like [`DiskCache::put`].
    pub fn put(&self, bytes: &[u8]) -> Result<ContentHash> {
        let hash = hash_of(bytes);
        let path = self.path_for(&hash);
        // Skip the write only when an intact copy is there already, so a
        // damaged blob is repaired by the next put.
        if self.get(&hash)?.is_none() {
            write_atomic(&path, bytes)?;
        }
        Ok(hash)
    }

    /// The blob's bytes, or `None` when it is missing or its bytes no longer
    /// match its name (logged; a damaged blob is a miss, never an error).
    pub fn get(&self, hash: &ContentHash) -> Result<Option<Vec<u8>>> {
        let path = self.path_for(hash);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(CoreError::io(path, err)),
        };
        if hash_of(&bytes) != *hash {
            tracing::warn!(%hash, "blob does not match its hash; treating as a miss");
            return Ok(None);
        }
        Ok(Some(bytes))
    }

    /// Deletes every blob (and leftover temp file), leaving foreign files
    /// with a warning, then any directories left empty.
    pub fn clear(&self) -> Result<()> {
        for entry in read_dir_if_exists(&self.root)? {
            if entry.is_dir() && is_shard_dir(&entry) {
                clear_shard(&entry, WAV)?;
            } else {
                tracing::warn!(path = %entry.display(), "not a blob; leaving it");
            }
        }
        remove_dir_if_empty(&self.root)
    }
}

fn hash_of(bytes: &[u8]) -> ContentHash {
    blake3::hash(bytes)
        .to_hex()
        .parse()
        .expect("BLAKE3 hex is a valid content hash")
}

/// Writes through a temporary file in the same directory and renames it into
/// place, so a reader never sees a partial file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().expect("cache paths have a shard directory");
    fs::create_dir_all(dir).map_err(|err| CoreError::io(dir, err))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|err| CoreError::io(dir, err))?;
    tmp.write_all(bytes)
        .and_then(|()| tmp.as_file().sync_all())
        .map_err(|err| CoreError::io(tmp.path(), err))?;
    tmp.persist(path)
        .map_err(|err| CoreError::io(path, err.error))?;
    Ok(())
}

/// Number and total size of `<2 hex>/<64 hex><ext>` files under `root`.
fn count_sharded(root: &Path, ext: &str) -> Result<(u64, u64)> {
    let (mut count, mut bytes) = (0, 0);
    for shard in read_dir_if_exists(root)? {
        if !(shard.is_dir() && is_shard_dir(&shard)) {
            continue;
        }
        for entry in read_dir_if_exists(&shard)? {
            if is_entry_file(&entry, ext) {
                count += 1;
                bytes += fs::metadata(&entry)
                    .map_err(|err| CoreError::io(&entry, err))?
                    .len();
            }
        }
    }
    Ok((count, bytes))
}

/// Removes the `<64 hex><ext>` and temp files in one shard, warns about the
/// rest, and removes the shard if that left it empty.
fn clear_shard(shard: &Path, ext: &str) -> Result<()> {
    for file in read_dir_if_exists(shard)? {
        if is_entry_file(&file, ext) || is_temp_file(&file) {
            fs::remove_file(&file).map_err(|err| CoreError::io(&file, err))?;
        } else {
            tracing::warn!(path = %file.display(), "not a cache entry; leaving it");
        }
    }
    remove_dir_if_empty(shard)
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_shard_dir(path: &Path) -> bool {
    is_lower_hex(file_name(path), 2)
}

fn is_entry_file(path: &Path, ext: &str) -> bool {
    path.is_file()
        && file_name(path)
            .strip_suffix(ext)
            .is_some_and(|stem| is_lower_hex(stem, 64))
}

/// `tempfile::NamedTempFile`'s default names start with `.tmp`.
fn is_temp_file(path: &Path) -> bool {
    path.is_file() && file_name(path).starts_with(".tmp")
}

fn remove_dir_if_empty(dir: &Path) -> Result<()> {
    match fs::remove_dir(dir) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        // Not empty: something we deliberately left behind.
        Err(_) if fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some()) => Ok(()),
        Err(err) => Err(CoreError::io(dir, err)),
    }
}

fn read_dir_if_exists(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(CoreError::io(dir, err)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        paths.push(entry.map_err(|err| CoreError::io(dir, err))?.path());
    }
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(input: &serde_json::Value) -> CacheKey {
        CacheKey::new("stage", 1, input, &json!({})).unwrap()
    }

    #[test]
    fn key_ignores_field_order_but_not_values() {
        #[derive(Serialize)]
        struct Ba {
            b: u8,
            a: u8,
        }
        let from_struct = CacheKey::new("s", 1, &Ba { b: 2, a: 1 }, &()).unwrap();
        let from_json = CacheKey::new("s", 1, &json!({"a": 1, "b": 2}), &()).unwrap();
        assert_eq!(from_struct, from_json, "canonical JSON must sort keys");
        assert_ne!(
            from_json,
            CacheKey::new("s", 1, &json!({"a": 1, "b": 3}), &()).unwrap()
        );
    }

    #[test]
    fn key_changes_with_stage_version_and_config() {
        let input = json!({"x": 1});
        let base = CacheKey::new("s", 1, &input, &json!({"m": "a"})).unwrap();
        assert_ne!(
            base,
            CacheKey::new("t", 1, &input, &json!({"m": "a"})).unwrap()
        );
        assert_ne!(
            base,
            CacheKey::new("s", 2, &input, &json!({"m": "a"})).unwrap()
        );
        assert_ne!(
            base,
            CacheKey::new("s", 1, &input, &json!({"m": "b"})).unwrap()
        );
    }

    #[test]
    fn put_then_get_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let k = key(&json!(1));
        assert_eq!(cache.get::<Vec<String>>(&k).unwrap(), None);
        cache.put(&k, "stage", &vec!["a".to_string()]).unwrap();
        assert_eq!(
            cache.get::<Vec<String>>(&k).unwrap(),
            Some(vec!["a".to_string()])
        );
        // No temporary files are left behind next to the entry.
        let shard = cache.path_for(&k).parent().unwrap().to_owned();
        assert_eq!(fs::read_dir(shard).unwrap().count(), 1);
    }

    #[test]
    fn corrupt_or_foreign_entries_are_misses() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let k = key(&json!(2));
        cache.put(&k, "stage", &42u32).unwrap();
        let path = cache.path_for(&k);

        fs::write(&path, b"{\"schema_version\": 1, \"val").unwrap();
        assert_eq!(cache.get::<u32>(&k).unwrap(), None, "truncated");

        fs::write(
            &path,
            br#"{"schema_version": 999, "stage": "stage", "value": 42}"#,
        )
        .unwrap();
        assert_eq!(cache.get::<u32>(&k).unwrap(), None, "other schema version");

        fs::write(
            &path,
            br#"{"schema_version": 1, "stage": "stage", "value": "text"}"#,
        )
        .unwrap();
        assert_eq!(cache.get::<u32>(&k).unwrap(), None, "wrong type");
    }

    #[test]
    fn stats_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().join("cache"));
        assert_eq!(cache.stats().unwrap(), CacheStats::default());
        cache.put(&key(&json!(1)), "s", &1u8).unwrap();
        cache.put(&key(&json!(2)), "s", &2u8).unwrap();
        let stats = cache.stats().unwrap();
        assert_eq!(stats.entries, 2);
        assert!(stats.bytes > 0);
        cache.clear().unwrap();
        assert_eq!(cache.stats().unwrap().entries, 0);
        assert!(!dir.path().join("cache").exists(), "empty dirs are removed");
        cache.clear().unwrap();
    }

    #[test]
    fn clear_never_deletes_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        // As if the user ran `podling --cache-dir . cache clear` in a project.
        let cache = DiskCache::new(dir.path());
        let k = key(&json!(1));
        cache.put(&k, "s", &1u8).unwrap();
        let shard = dir.path().join(&k.as_str()[..2]);
        fs::write(dir.path().join("Cargo.toml"), "precious").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "precious").unwrap();
        fs::write(shard.join("notes.txt"), "precious").unwrap();
        fs::write(shard.join(".tmpABC123"), "partial write").unwrap();

        cache.clear().unwrap();

        assert_eq!(cache.stats().unwrap().entries, 0);
        assert!(!shard.join(".tmpABC123").exists());
        for kept in ["Cargo.toml", "src/main.rs"] {
            assert!(dir.path().join(kept).exists(), "{kept} was deleted");
        }
        assert!(shard.join("notes.txt").exists());
    }

    #[test]
    fn blobs_are_content_addressed() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = DiskCache::new(dir.path()).blobs();
        let hash = blobs.put(b"RIFF audio").unwrap();
        assert_eq!(hash.as_str(), blake3::hash(b"RIFF audio").to_hex().as_str());
        assert_eq!(blobs.put(b"RIFF audio").unwrap(), hash, "idempotent");
        let path = blobs.path_for(&hash);
        assert!(path.starts_with(dir.path().join("blobs")));
        assert!(path.to_str().unwrap().ends_with(".wav"));
        assert_eq!(
            blobs.get(&hash).unwrap().as_deref(),
            Some(&b"RIFF audio"[..])
        );

        fs::write(&path, b"tampered").unwrap();
        assert_eq!(blobs.get(&hash).unwrap(), None, "a damaged blob is a miss");
        blobs.put(b"RIFF audio").unwrap();
        assert!(blobs.get(&hash).unwrap().is_some(), "a put repairs it");
        fs::remove_file(&path).unwrap();
        assert_eq!(blobs.get(&hash).unwrap(), None, "a deleted blob is a miss");
    }

    #[test]
    fn stats_and_clear_cover_blobs_but_never_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().join("cache"));
        cache.put(&key(&json!(1)), "s", &1u8).unwrap();
        let blobs = cache.blobs();
        let hash = blobs.put(&[0u8; 100]).unwrap();
        blobs.put(&[1u8; 50]).unwrap();
        let stats = cache.stats().unwrap();
        assert_eq!((stats.entries, stats.blobs, stats.blob_bytes), (1, 2, 150));

        let shard = blobs.path_for(&hash).parent().unwrap().to_owned();
        fs::write(shard.join("keep.wav"), "precious").unwrap();
        fs::write(shard.join(".tmpXYZ"), "partial write").unwrap();
        fs::write(blobs.root().join("README"), "precious").unwrap();

        cache.clear().unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!((stats.entries, stats.blobs), (0, 0));
        assert!(!shard.join(".tmpXYZ").exists());
        assert!(shard.join("keep.wav").exists());
        assert!(blobs.root().join("README").exists());
    }
}
