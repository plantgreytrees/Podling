//! Premise windows: short runs of consecutive sentences of a chunk, sized for
//! an NLI model's input. Shared by the stages that judge claims against
//! source text (`ground_claims`, `score_stances`).

use std::ops::Range;

use podling_types::{Chunk, TextSpan};

use crate::text::sentences;

/// Longest premise window, in consecutive sentences.
pub const MAX_WINDOW_SENTENCES: usize = 2;

/// Longest premise window, in words. DeBERTa reads at most 512 tokens for the
/// premise and the claim together and cuts the rest off, so a longer window
/// would hide its own end from the model. An English word is one to two
/// tokens, which leaves room for the claim and the names prefix.
pub const MAX_WINDOW_WORDS: usize = 120;

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
///
/// No window is longer than [`MAX_WINDOW_WORDS`]. A run of sentences over the
/// cap is skipped, since each of its sentences is a window of its own. A
/// single sentence over the cap (a long list with no full stops, say) is
/// split into overlapping slices instead, so every part of it is still seen.
pub(crate) fn windows(chunks: &[Chunk]) -> Vec<Window<'_>> {
    let mut out = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let text = chunk.text();
        let offset = chunk.span().start();
        let mut push = |range: Range<usize>| {
            let span = TextSpan::new(offset + range.start, offset + range.end)
                .expect("a sentence range is ordered");
            out.push(Window {
                chunk: i,
                text: &text[range],
                span,
            });
        };
        let sentences = sentences(text);
        for first in 0..sentences.len() {
            for len in 1..=MAX_WINDOW_SENTENCES {
                let Some(last) = sentences.get(first + len - 1) else {
                    break;
                };
                let range: Range<usize> = sentences[first].start..last.end;
                let words = words(&text[range.clone()]);
                if words.len() <= MAX_WINDOW_WORDS {
                    push(range);
                } else if len == 1 {
                    for slice in slices(&words) {
                        push(range.start + slice.start..range.start + slice.end);
                    }
                }
            }
        }
    }
    out
}

/// Byte ranges of the whitespace-separated words of `text`.
fn words(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (start, c.is_whitespace()) {
            (None, false) => start = Some(i),
            (Some(s), true) => {
                out.push(s..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(s..text.len());
    }
    out
}

/// Byte ranges of [`MAX_WINDOW_WORDS`]-word slices over `words`, each starting
/// half a slice after the one before, the last ending at the last word. A fact
/// up to half a slice long lies wholly inside at least one of them.
fn slices(words: &[Range<usize>]) -> Vec<Range<usize>> {
    let stride = MAX_WINDOW_WORDS / 2;
    let last_start = words.len().saturating_sub(MAX_WINDOW_WORDS);
    // `step_by` yields 0, stride, 2 × stride, … below `last_start`; the final
    // slice is then pinned to the end, so the tail is never left out.
    let mut starts: Vec<usize> = (0..last_start).step_by(stride).collect();
    starts.push(last_start);
    starts
        .into_iter()
        .map(|s| words[s].start..words[s + MAX_WINDOW_WORDS - 1].end)
        .collect()
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

    #[test]
    fn a_sentence_over_the_word_cap_is_split_into_overlapping_slices() {
        // 300 words with no full stop, then a short sentence.
        let list: Vec<String> = (1..=300).map(|n| format!("w{n}")).collect();
        let text = format!("{}. Short one.", list.join(" "));
        let source = SourceRef {
            connector: "t".into(),
            locator: "a".into(),
            independence_group: "g".into(),
        };
        let doc = Document::new(source, "a", text.clone());
        let span = TextSpan::new(0, text.len()).unwrap();
        let chunks = vec![Chunk::from_document(&doc, span, vec![]).unwrap()];

        let got = windows(&chunks);
        let texts: Vec<&str> = got.iter().map(|w| w.text).collect();
        // Slices start at words 1, 61, 121 and 181 (the last pinned to the
        // end); the two-sentence window is over the cap and skipped.
        let firsts: Vec<&str> = texts.iter().map(|t| t.split(' ').next().unwrap()).collect();
        assert_eq!(firsts, ["w1", "w61", "w121", "w181", "Short"]);
        assert_eq!(texts.last(), Some(&"Short one."));
        assert!(texts[3].ends_with("w300."));
        for w in &got {
            assert!(w.text.split_whitespace().count() <= MAX_WINDOW_WORDS);
            assert_eq!(doc.slice(w.span), Some(w.text));
        }
        // Every word is in some window.
        for word in &list {
            let seen = texts.iter().any(|t| {
                t.split_whitespace()
                    .any(|w| w.trim_end_matches('.') == word)
            });
            assert!(seen, "{word} is in no window");
        }
    }
}
