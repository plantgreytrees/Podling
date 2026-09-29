//! Analysers: opt-in checks that read a finished script and report findings.

use podling_types::{Document, Finding, Script, Severity};

pub trait Analyser {
    fn id(&self) -> &str;

    fn analyse(&self, script: &Script, documents: &[Document]) -> Vec<Finding>;
}

/// Checks that every quote is word-for-word what its source says, and that the
/// turn actually speaks those words rather than a paraphrase.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuoteVerifier;

impl QuoteVerifier {
    fn error(&self, turn: usize, message: String) -> Finding {
        Finding {
            analyser: self.id().to_owned(),
            severity: Severity::Error,
            turn: Some(turn),
            message,
        }
    }
}

impl Analyser for QuoteVerifier {
    fn id(&self) -> &str {
        "quote_verifier"
    }

    fn analyse(&self, script: &Script, documents: &[Document]) -> Vec<Finding> {
        let mut findings = Vec::new();
        let mut checked = 0;
        for (i, turn) in script.turns().iter().enumerate() {
            for quote in &turn.quotes {
                checked += 1;
                let Some(doc) = documents.iter().find(|d| d.id() == quote.document()) else {
                    findings.push(self.error(
                        i,
                        format!("quote cites unknown document {}", quote.document()),
                    ));
                    continue;
                };
                if doc.slice(quote.span()) != Some(quote.text()) {
                    findings.push(self.error(
                        i,
                        format!(
                            "quote {:?} does not match the source text at its span",
                            quote.text()
                        ),
                    ));
                } else if !turn.text.contains(quote.text()) {
                    findings.push(self.error(
                        i,
                        format!("turn does not speak the quote verbatim: {:?}", quote.text()),
                    ));
                }
            }
        }
        if findings.is_empty() {
            findings.push(Finding {
                analyser: self.id().to_owned(),
                severity: Severity::Info,
                turn: None,
                message: format!("{checked} quote(s) verified against their sources"),
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use podling_types::{Emotion, Quote, SourceRef, Speaker, SpeakerId, TextSpan, Turn};

    fn doc() -> Document {
        let source = SourceRef {
            connector: "t".into(),
            locator: "a".into(),
            independence_group: "g".into(),
        };
        Document::new(source, "A", "The sky burned. Trees fell.")
    }

    fn script_with(text: &str, quote: Quote) -> Script {
        Script::new(
            vec![Speaker {
                id: SpeakerId("h".into()),
                name: "H".into(),
                role: "host".into(),
            }],
            vec![Turn {
                speaker: SpeakerId("h".into()),
                text: text.into(),
                emotion: Emotion::Neutral,
                citations: vec![],
                quotes: vec![quote],
            }],
        )
        .unwrap()
    }

    fn severities(findings: &[Finding]) -> Vec<Severity> {
        findings.iter().map(|f| f.severity).collect()
    }

    #[test]
    fn accepts_a_verbatim_quote() {
        let d = doc();
        let quote = Quote::from_document(&d, TextSpan::new(0, 15).unwrap()).unwrap();
        let findings =
            QuoteVerifier.analyse(&script_with("They said: \"The sky burned.\"", quote), &[d]);
        assert_eq!(severities(&findings), vec![Severity::Info]);
    }

    #[test]
    fn flags_tampered_quote_text() {
        let d = doc();
        let genuine = Quote::from_document(&d, TextSpan::new(0, 15).unwrap()).unwrap();
        // Same length, so it passes deserialisation's shape check.
        let mut json = serde_json::to_value(&genuine).unwrap();
        json["text"] = "The sky BURNED.".into();
        let tampered: Quote = serde_json::from_value(json).unwrap();

        let findings = QuoteVerifier.analyse(&script_with("\"The sky BURNED.\"", tampered), &[d]);
        assert_eq!(severities(&findings), vec![Severity::Error]);
    }

    #[test]
    fn flags_paraphrased_turn_text() {
        let d = doc();
        let quote = Quote::from_document(&d, TextSpan::new(0, 15).unwrap()).unwrap();
        let findings =
            QuoteVerifier.analyse(&script_with("Apparently the sky was on fire.", quote), &[d]);
        assert_eq!(severities(&findings), vec![Severity::Error]);
    }

    #[test]
    fn flags_unknown_document() {
        let quote = Quote::from_document(&doc(), TextSpan::new(0, 15).unwrap()).unwrap();
        let findings = QuoteVerifier.analyse(&script_with("\"The sky burned.\"", quote), &[]);
        assert_eq!(severities(&findings), vec![Severity::Error]);
    }
}
