//! Premise windows: short runs of consecutive sentences of a chunk, sized for
//! an NLI model's input. Shared by the stages that judge claims against
//! source text (`ground_claims`, `score_stances`).

use std::ops::Range;

use podling_types::{Chunk, TextSpan};

use crate::text::sentences;

/// Longest premise window, in consecutive sentences.
pub const MAX_WINDOW_SENTENCES: usize = 2;

/// A span of one to [`MAX_WINDOW_SENTENCES`] consecutive sentences of a chunk:
/// short enough for an NLI model's input, long enough to hold a fact that
/// spans a sentence break.
///
/// `pub(crate)`: visible to every module of this crate, but not part of its
/// public API. The window is a detail of how stages call the NLI model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Window<'a> {
    /// Index into the chunk slice the windows were made from.
    pub chunk: usize,
    pub text: &'a str,
    /// The window's span in the chunk's *document*, as evidence records it.
    pub span: TextSpan,
}

/// Every premise window of every chunk, in chunk order, then by first
/// sentence, then by length.
pub(crate) fn windows(chunks: &[Chunk]) -> Vec<Window<'_>> {
    let mut out = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let text = chunk.text();
        let sentences = sentences(text);
        for first in 0..sentences.len() {
            for len in 1..=MAX_WINDOW_SENTENCES {
                let Some(last) = sentences.get(first + len - 1) else {
                    break;
                };
                let range: Range<usize> = sentences[first].start..last.end;
                let offset = chunk.span().start();
                let span = TextSpan::new(offset + range.start, offset + range.end)
                    .expect("a sentence range is ordered");
                out.push(Window {
                    chunk: i,
                    text: &text[range],
                    span,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use podling_types::{Document, SourceRef};

    #[test]
    fn windows_are_ordered_and_their_spans_slice_the_document() {
        let text = "One fell. Two fell. Three fell.";
        let source = SourceRef {
            connector: "t".into(),
            locator: "a".into(),
            independence_group: "g".into(),
        };
        // A heading first, so chunk and document offsets differ.
        let doc = Document::new(source, "a", format!("# A\n\n{text}"));
        let start = doc.text().len() - text.len();
        let span = TextSpan::new(start, doc.text().len()).unwrap();
        let chunks = vec![Chunk::from_document(&doc, span, vec![]).unwrap()];

        let got = windows(&chunks);
        let texts: Vec<&str> = got.iter().map(|w| w.text).collect();
        assert_eq!(
            texts,
            [
                "One fell.",
                "One fell. Two fell.",
                "Two fell.",
                "Two fell. Three fell.",
                "Three fell.",
            ]
        );
        for w in &got {
            assert_eq!(w.chunk, 0);
            assert_eq!(doc.slice(w.span), Some(w.text));
        }
    }
}
