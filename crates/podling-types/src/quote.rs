//! Verbatim quotes.
//!
//! A language model may *point at* a span to quote, but it can never write the
//! quoted words itself: the only way to build a [`Quote`] from text is
//! [`Quote::from_document`], which copies the words out of the source.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::document::{Document, TextSpan};
use crate::ids::DocumentId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawQuote")]
pub struct Quote {
    document: DocumentId,
    span: TextSpan,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuoteError {
    #[error("quote span {start}..{end} is outside the document or splits a character")]
    OutOfBounds { start: usize, end: usize },
    #[error("quote is empty")]
    Empty,
    #[error("quote text is {text_len} bytes but its span covers {span_len}")]
    LengthMismatch { text_len: usize, span_len: usize },
}

impl Quote {
    /// Builds a quote by copying `doc`'s text at `span`.
    pub fn from_document(doc: &Document, span: TextSpan) -> Result<Self, QuoteError> {
        if span.is_empty() {
            return Err(QuoteError::Empty);
        }
        let text = doc.slice(span).ok_or(QuoteError::OutOfBounds {
            start: span.start(),
            end: span.end(),
        })?;
        Ok(Self {
            document: doc.id().clone(),
            span,
            text: text.to_owned(),
        })
    }

    pub fn document(&self) -> &DocumentId {
        &self.document
    }

    pub fn span(&self) -> TextSpan {
        self.span
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// The unchecked wire shape. Deserialising can only check the quote's shape;
/// whether the words really appear in the source is checked against the
/// document by the `QuoteVerifier` analyser.
#[derive(Deserialize, JsonSchema)]
struct RawQuote {
    document: DocumentId,
    span: TextSpan,
    text: String,
}

impl TryFrom<RawQuote> for Quote {
    type Error = QuoteError;

    fn try_from(raw: RawQuote) -> Result<Self, Self::Error> {
        if raw.text.is_empty() {
            return Err(QuoteError::Empty);
        }
        if raw.text.len() != raw.span.len() {
            return Err(QuoteError::LengthMismatch {
                text_len: raw.text.len(),
                span_len: raw.span.len(),
            });
        }
        Ok(Self {
            document: raw.document,
            span: raw.span,
            text: raw.text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::SourceRef;

    fn doc() -> Document {
        let source = SourceRef {
            connector: "local_files".into(),
            locator: "a.md".into(),
            independence_group: "g".into(),
        };
        Document::new(source, "A", "Café crowds gathered.")
    }

    #[test]
    fn copies_text_from_the_document() {
        let quote = Quote::from_document(&doc(), TextSpan::new(0, 5).unwrap()).unwrap();
        assert_eq!(quote.text(), "Café");
    }

    #[test]
    fn rejects_bad_spans() {
        let d = doc();
        assert_eq!(
            Quote::from_document(&d, TextSpan::new(0, 999).unwrap()),
            Err(QuoteError::OutOfBounds { start: 0, end: 999 })
        );
        // Byte 4 is inside the two-byte "é".
        assert!(matches!(
            Quote::from_document(&d, TextSpan::new(0, 4).unwrap()),
            Err(QuoteError::OutOfBounds { .. })
        ));
        assert_eq!(
            Quote::from_document(&d, TextSpan::new(3, 3).unwrap()),
            Err(QuoteError::Empty)
        );
    }

    #[test]
    fn deserialisation_checks_shape() {
        let quote = Quote::from_document(&doc(), TextSpan::new(0, 5).unwrap()).unwrap();
        let mut json = serde_json::to_value(&quote).unwrap();
        assert_eq!(
            serde_json::from_value::<Quote>(json.clone()).unwrap(),
            quote
        );
        json["text"] = "Cafe, and more".into();
        assert!(serde_json::from_value::<Quote>(json).is_err());
    }
}
