//! Source connectors: where documents come from.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use podling_types::{Document, SourceRef};

use crate::error::{CoreError, Result};

pub trait SourceConnector {
    fn id(&self) -> &str;

    /// Fetches every document this connector provides, in a stable order.
    fn fetch(&self) -> Result<Vec<Document>>;
}

/// Files larger than this are skipped; source material is prose, not data dumps.
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// Reads `.md` and `.txt` files directly under a directory.
///
/// Security: every entry is canonicalised and must resolve inside the
/// canonical root, so a symlink cannot smuggle in files from elsewhere.
#[derive(Debug, Clone)]
pub struct LocalFilesConnector {
    root: PathBuf,
    independence_group: String,
}

impl LocalFilesConnector {
    pub fn new(root: impl Into<PathBuf>, independence_group: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            independence_group: independence_group.into(),
        }
    }

    fn read_document(&self, path: &Path, root: &Path) -> Result<Option<Document>> {
        let canonical = match path.canonicalize() {
            Ok(canonical) => canonical,
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "skipping unreadable entry");
                return Ok(None);
            }
        };
        if !canonical.starts_with(root) {
            tracing::warn!(path = %path.display(), target = %canonical.display(),
                "skipping entry that resolves outside the source root");
            return Ok(None);
        }
        if !canonical.is_file() {
            return Ok(None);
        }
        // Read through a size-limited handle rather than checking metadata
        // first, so a file that grows between check and read is still capped.
        let file = fs::File::open(&canonical).map_err(|err| CoreError::io(&canonical, err))?;
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| CoreError::io(&canonical, err))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            tracing::warn!(path = %path.display(), limit = MAX_FILE_BYTES, "skipping oversized file");
            return Ok(None);
        }

        let text = String::from_utf8(bytes).map_err(|_| CoreError::Source {
            path: path.to_owned(),
            message: "file is not valid UTF-8".into(),
        })?;
        let locator = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = markdown_title(&text).unwrap_or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let source = SourceRef {
            connector: self.id().to_owned(),
            locator,
            independence_group: self.independence_group.clone(),
        };
        Ok(Some(Document::new(source, title, text)))
    }
}

impl SourceConnector for LocalFilesConnector {
    fn id(&self) -> &str {
        "local_files"
    }

    fn fetch(&self) -> Result<Vec<Document>> {
        let root = self.root.canonicalize().map_err(|err| CoreError::Source {
            path: self.root.clone(),
            message: format!("cannot open source directory: {err}"),
        })?;
        let mut paths = Vec::new();
        for entry in fs::read_dir(&root).map_err(|err| CoreError::io(&root, err))? {
            let path = entry.map_err(|err| CoreError::io(&root, err))?.path();
            let wanted = path
                .extension()
                .is_some_and(|ext| ext == "md" || ext == "txt");
            if wanted {
                paths.push(path);
            }
        }
        paths.sort();

        let mut documents = Vec::new();
        for path in paths {
            if let Some(doc) = self.read_document(&path, &root)? {
                documents.push(doc);
            }
        }
        Ok(documents)
    }
}

/// The text of the first `# ` heading, if the file starts with one.
fn markdown_title(text: &str) -> Option<String> {
    let first = text.lines().find(|line| !line.trim().is_empty())?;
    first
        .strip_prefix("# ")
        .map(|title| title.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_markdown_and_text_in_sorted_order() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), "Plain text.").unwrap();
        fs::write(dir.path().join("a.md"), "# Heading\n\nBody.").unwrap();
        fs::write(dir.path().join("ignored.pdf"), "binary").unwrap();

        let docs = LocalFilesConnector::new(dir.path(), "g").fetch().unwrap();
        let summary: Vec<_> = docs
            .iter()
            .map(|d| (d.source().locator.as_str(), d.title()))
            .collect();
        assert_eq!(summary, vec![("a.md", "Heading"), ("b.txt", "b")]);
        assert!(docs.iter().all(|d| d.source().connector == "local_files"));
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinks_that_escape_the_root() {
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.md");
        fs::write(&secret, "Do not read me.").unwrap();

        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(&secret, dir.path().join("escape.md")).unwrap();
        fs::write(dir.path().join("inside.md"), "Fine.").unwrap();
        std::os::unix::fs::symlink(dir.path().join("inside.md"), dir.path().join("alias.md"))
            .unwrap();

        let docs = LocalFilesConnector::new(dir.path(), "g").fetch().unwrap();
        let locators: Vec<_> = docs.iter().map(|d| d.source().locator.as_str()).collect();
        assert_eq!(locators, vec!["alias.md", "inside.md"]);
    }

    #[test]
    fn skips_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let big = fs::File::create(dir.path().join("big.txt")).unwrap();
        big.set_len(MAX_FILE_BYTES + 1).unwrap();
        fs::write(dir.path().join("small.txt"), "Small.").unwrap();

        let docs = LocalFilesConnector::new(dir.path(), "g").fetch().unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].source().locator, "small.txt");
    }

    #[test]
    fn non_utf8_is_an_error_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("latin1.txt"), [0x63, 0x61, 0x66, 0xe9]).unwrap();
        let err = LocalFilesConnector::new(dir.path(), "g")
            .fetch()
            .unwrap_err();
        assert!(err.to_string().contains("latin1.txt"), "{err}");
    }

    #[test]
    fn missing_root_is_a_source_error() {
        let err = LocalFilesConnector::new("/definitely/not/here", "g")
            .fetch()
            .unwrap_err();
        assert!(matches!(err, CoreError::Source { .. }));
    }
}
