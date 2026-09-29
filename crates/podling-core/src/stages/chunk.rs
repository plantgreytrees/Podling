//! Splits documents into chunks along their Markdown structure.

use podling_types::{Chunk, Document, TextSpan};
use serde_json::{Value, json};

use crate::error::Result;
use crate::stage::Stage;

/// Cuts each document at its Markdown headings, then splits long sections at
/// paragraph breaks once they reach `max_words`. Heading lines themselves are
/// not chunk text; they are recorded in each chunk's `heading_path`.
pub struct ChunkDocuments {
    pub max_words: usize,
}

impl Default for ChunkDocuments {
    fn default() -> Self {
        Self { max_words: 800 }
    }
}

impl Stage for ChunkDocuments {
    const ID: &'static str = "chunk";
    const VERSION: u32 = 1;
    type Input = Vec<Document>;
    type Output = Vec<Chunk>;

    fn config_fingerprint(&self) -> Value {
        json!({ "max_words": self.max_words })
    }

    fn run(&self, input: &Vec<Document>) -> Result<Vec<Chunk>> {
        Ok(input.iter().flat_map(|doc| self.chunk(doc)).collect())
    }
}

/// A chunk under construction: byte range and running word count.
struct Open {
    start: usize,
    end: usize,
    words: usize,
}

impl ChunkDocuments {
    fn chunk(&self, doc: &Document) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut headings: Vec<(usize, String)> = Vec::new();
        let mut open: Option<Open> = None;

        let flush =
            |open: &mut Option<Open>, headings: &[(usize, String)], chunks: &mut Vec<Chunk>| {
                if let Some(o) = open.take() {
                    let path = headings.iter().map(|(_, title)| title.clone()).collect();
                    let span = TextSpan::new(o.start, o.end).expect("start <= end by construction");
                    if let Some(chunk) = Chunk::from_document(doc, span, path) {
                        chunks.push(chunk);
                    }
                }
            };

        let mut offset = 0;
        for raw_line in doc.text().split_inclusive('\n') {
            let line_start = offset;
            offset += raw_line.len();
            let line = raw_line.trim_end();

            if let Some((level, title)) = heading(line) {
                flush(&mut open, &headings, &mut chunks);
                headings.retain(|(l, _)| *l < level);
                headings.push((level, title.to_owned()));
                continue;
            }
            if line.trim().is_empty() {
                if open.as_ref().is_some_and(|o| o.words >= self.max_words) {
                    flush(&mut open, &headings, &mut chunks);
                }
                continue;
            }

            let lead = line.len() - line.trim_start().len();
            let words = line.split_whitespace().count();
            match open.as_mut() {
                Some(o) => {
                    o.end = line_start + line.len();
                    o.words += words;
                }
                None => {
                    open = Some(Open {
                        start: line_start + lead,
                        end: line_start + line.len(),
                        words,
                    });
                }
            }
        }
        flush(&mut open, &headings, &mut chunks);
        chunks
    }
}

/// `(level, title)` for an ATX heading such as `## Aftermath`.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    rest.starts_with(' ').then(|| (level, rest.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use podling_types::SourceRef;

    fn doc(text: &str) -> Document {
        let source = SourceRef {
            connector: "t".into(),
            locator: "a".into(),
            independence_group: "g".into(),
        };
        Document::new(source, "A", text)
    }

    #[test]
    fn splits_by_heading_and_tracks_the_path() {
        let d = doc(
            "# Event\n\nIntro text.\n\n## Witnesses\n\nFirst account.\nSecond line.\n\n# Aftermath\n\nTrees fell.\n",
        );
        let chunks = ChunkDocuments::default().run(&vec![d.clone()]).unwrap();
        let summary: Vec<_> = chunks
            .iter()
            .map(|c| (c.heading_path().join(" > "), c.text()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Event".into(), "Intro text."),
                ("Event > Witnesses".into(), "First account.\nSecond line."),
                ("Aftermath".into(), "Trees fell."),
            ]
        );
        for chunk in &chunks {
            assert_eq!(d.slice(chunk.span()), Some(chunk.text()), "span invariant");
        }
    }

    #[test]
    fn splits_long_sections_at_paragraph_breaks() {
        let d = doc("one two three\n\nfour five\n\nsix\n");
        let chunks = ChunkDocuments { max_words: 3 }.run(&vec![d]).unwrap();
        let texts: Vec<_> = chunks.iter().map(|c| c.text()).collect();
        assert_eq!(texts, vec!["one two three", "four five\n\nsix"]);
    }

    #[test]
    fn ignores_hashtags_and_handles_crlf_free_utf8() {
        let d = doc("#not-a-heading café\n");
        let chunks = ChunkDocuments::default().run(&vec![d]).unwrap();
        assert_eq!(chunks[0].text(), "#not-a-heading café");
        assert!(chunks[0].heading_path().is_empty());
    }
}
