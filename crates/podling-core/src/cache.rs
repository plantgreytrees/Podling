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
        let path = self.path_for(key);
        let dir = path.parent().unwrap_or(&self.root);
        fs::create_dir_all(dir).map_err(|err| CoreError::io(dir, err))?;

        let bytes = serde_json::to_vec(&EntryRef {
            schema_version: SCHEMA_VERSION,
            stage,
            value,
        })?;
        let mut tmp =
            tempfile::NamedTempFile::new_in(dir).map_err(|err| CoreError::io(dir, err))?;
        tmp.write_all(&bytes)
            .and_then(|()| tmp.as_file().sync_all())
            .map_err(|err| CoreError::io(tmp.path(), err))?;
        tmp.persist(&path)
            .map_err(|err| CoreError::io(&path, err.error))?;
        Ok(())
    }

    pub fn stats(&self) -> Result<CacheStats> {
        let mut stats = CacheStats::default();
        for shard in read_dir_if_exists(&self.root)? {
            if !shard.is_dir() {
                continue;
            }
            for entry in read_dir_if_exists(&shard)? {
                if entry.extension().is_some_and(|ext| ext == "json") {
                    let len = fs::metadata(&entry)
                        .map_err(|err| CoreError::io(&entry, err))?
                        .len();
                    stats.entries += 1;
                    stats.bytes += len;
                }
            }
        }
        Ok(stats)
    }

    /// Deletes every cached entry.
    pub fn clear(&self) -> Result<()> {
        match fs::remove_dir_all(&self.root) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(CoreError::io(&self.root, err)),
        }
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
        cache.clear().unwrap();
    }
}
