//! Normalises fetched documents so formatting noise doesn't change ids.

use std::collections::BTreeSet;

use podling_types::Document;

use crate::error::Result;
use crate::stage::Stage;

/// Strips a byte-order mark, converts CRLF/CR line endings to LF, and drops
/// documents whose text duplicates an earlier one in the same independence
/// group (the same file saved twice is not extra evidence).
pub struct Ingest;

impl Stage for Ingest {
    const ID: &'static str = "ingest";
    const VERSION: u32 = 1;
    type Input = Vec<Document>;
    type Output = Vec<Document>;

    fn run(&self, input: &Vec<Document>) -> Result<Vec<Document>> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for doc in input {
            let text = normalise(doc.text());
            let group = doc.source().independence_group.clone();
            if seen.insert((group, text.clone())) {
                out.push(Document::new(doc.source().clone(), doc.title(), text));
            } else {
                tracing::info!(locator = %doc.source().locator, "dropping duplicate document");
            }
        }
        Ok(out)
    }
}

fn normalise(text: &str) -> String {
    text.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use podling_types::SourceRef;

    fn doc(locator: &str, group: &str, text: &str) -> Document {
        let source = SourceRef {
            connector: "t".into(),
            locator: locator.into(),
            independence_group: group.into(),
        };
        Document::new(source, locator, text)
    }

    #[test]
    fn normalises_and_dedupes_within_a_group() {
        let input = vec![
            doc("a", "g1", "\u{feff}Line one.\r\nLine two."),
            doc("b", "g1", "Line one.\nLine two."),
            doc("c", "g2", "Line one.\nLine two."),
        ];
        let out = Ingest.run(&input).unwrap();
        let summary: Vec<_> = out
            .iter()
            .map(|d| (d.source().locator.as_str(), d.text()))
            .collect();
        assert_eq!(
            summary,
            vec![("a", "Line one.\nLine two."), ("c", "Line one.\nLine two.")]
        );
    }
}
