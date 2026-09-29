//! Source documents and the chunks cut from them.

use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{ChunkId, ContentHash, DocumentId, IdMismatch, SourceId};

/// Where a document came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceRef {
    /// The connector that fetched it, e.g. `local_files`.
    pub connector: String,
    /// Connector-specific location, e.g. a relative path or URL.
    pub locator: String,
    /// Sources in the same group are not independent evidence (for example,
    /// several articles rewriting one wire report).
    pub independence_group: String,
}

impl SourceRef {
    pub fn id(&self) -> SourceId {
        SourceId::new(ContentHash::of_parts(&[
            self.connector.as_bytes(),
            self.locator.as_bytes(),
        ]))
    }
}

/// A fetched source text. Its id is derived from the source and the text, so
/// editing the text produces a new id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawDocument")]
pub struct Document {
    id: DocumentId,
    source: SourceRef,
    title: String,
    text: String,
}

#[derive(Deserialize, JsonSchema)]
struct RawDocument {
    id: DocumentId,
    source: SourceRef,
    title: String,
    text: String,
}

impl TryFrom<RawDocument> for Document {
    type Error = IdMismatch;

    fn try_from(raw: RawDocument) -> Result<Self, Self::Error> {
        let doc = Self::new(raw.source, raw.title, raw.text);
        if doc.id != raw.id {
            return Err(IdMismatch {
                kind: "document",
                stored: raw.id.to_string(),
                expected: doc.id.to_string(),
            });
        }
        Ok(doc)
    }
}

impl Document {
    pub fn new(source: SourceRef, title: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let id = DocumentId::new(ContentHash::of_parts(&[
            source.connector.as_bytes(),
            source.locator.as_bytes(),
            text.as_bytes(),
        ]));
        Self {
            id,
            source,
            title: title.into(),
            text,
        }
    }

    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    pub fn source(&self) -> &SourceRef {
        &self.source
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text at `span`, or `None` if the span is out of bounds or splits a
    /// UTF-8 character.
    pub fn slice(&self, span: TextSpan) -> Option<&str> {
        self.text.get(span.range())
    }
}

/// A half-open byte range `[start, end)` into a document's text.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct TextSpan {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid span: start {start} is after end {end}")]
pub struct InvalidSpan {
    pub start: usize,
    pub end: usize,
}

impl TextSpan {
    pub fn new(start: usize, end: usize) -> Result<Self, InvalidSpan> {
        if start > end {
            return Err(InvalidSpan { start, end });
        }
        Ok(Self { start, end })
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// A contiguous piece of a document. Its text is always exactly the document
/// text at its span, because the only constructor copies it from there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Chunk {
    id: ChunkId,
    document: DocumentId,
    span: TextSpan,
    heading_path: Vec<String>,
    text: String,
}

impl Chunk {
    /// Cuts a chunk from `doc`. Returns `None` if `span` is out of bounds or
    /// not on UTF-8 character boundaries.
    pub fn from_document(
        doc: &Document,
        span: TextSpan,
        heading_path: Vec<String>,
    ) -> Option<Self> {
        let text = doc.slice(span)?.to_owned();
        let id = ChunkId::new(ContentHash::of_parts(&[
            doc.id().hash().as_str().as_bytes(),
            &(span.start() as u64).to_le_bytes(),
            &(span.end() as u64).to_le_bytes(),
        ]));
        Some(Self {
            id,
            document: doc.id().clone(),
            span,
            heading_path,
            text,
        })
    }

    pub fn id(&self) -> &ChunkId {
        &self.id
    }

    pub fn document(&self) -> &DocumentId {
        &self.document
    }

    pub fn span(&self) -> TextSpan {
        self.span
    }

    /// Headings enclosing this chunk, outermost first.
    pub fn heading_path(&self) -> &[String] {
        &self.heading_path
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> SourceRef {
        SourceRef {
            connector: "local_files".into(),
            locator: "a.md".into(),
            independence_group: "g1".into(),
        }
    }

    #[test]
    fn document_id_is_stable_and_content_sensitive() {
        let a = Document::new(source(), "A", "same text");
        let b = Document::new(source(), "A (retitled)", "same text");
        let c = Document::new(source(), "A", "different text");
        assert_eq!(a.id(), b.id());
        assert_ne!(a.id(), c.id());
    }

    #[test]
    fn deserialisation_rejects_tampered_text() {
        let mut json = serde_json::to_value(Document::new(source(), "A", "original")).unwrap();
        assert!(serde_json::from_value::<Document>(json.clone()).is_ok());
        json["text"] = "edited".into();
        assert!(serde_json::from_value::<Document>(json).is_err());
    }

    #[test]
    fn span_rejects_reversed_bounds() {
        assert!(TextSpan::new(3, 2).is_err());
        assert!(TextSpan::new(2, 2).unwrap().is_empty());
    }

    #[test]
    fn chunk_text_matches_document_slice() {
        let doc = Document::new(source(), "A", "héllo world");
        let chunk = Chunk::from_document(&doc, TextSpan::new(0, 6).unwrap(), vec![]).unwrap();
        assert_eq!(chunk.text(), "héllo");
        // Byte 2 is inside the two-byte "é".
        assert!(Chunk::from_document(&doc, TextSpan::new(0, 2).unwrap(), vec![]).is_none());
        assert!(Chunk::from_document(&doc, TextSpan::new(0, 99).unwrap(), vec![]).is_none());
    }
}
