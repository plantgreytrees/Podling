//! Analysers: opt-in checks that read a finished script and report findings.

use podling_types::{Document, Finding, Script, Severity, Turn};

use crate::text::quotations;

pub trait Analyser {
    fn id(&self) -> &str;

    fn analyse(&self, script: &Script, documents: &[Document]) -> Vec<Finding>;
}

/// Checks that every quote is word-for-word what its source says, that the
/// turn actually speaks those words rather than a paraphrase, and that the turn
/// puts no other words in quotation marks: a quoted span the turn's quote refs
/// don't cover is text a model invented, however it is dressed up.
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
            for span in quotations(&turn.text) {
                if !turn.quotes.iter().any(|q| q.text().contains(span)) {
                    findings.push(self.error(
                        i,
                        format!("turn quotes words no quote ref covers: {span:?}"),
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

/// Warns on a turn that states a figure (any word with a digit: a year, a
/// count, a distance) but cites no claim. Banter is told to add no facts;
/// this catches the ones it adds anyway. Digits inside a quote don't count:
/// the quote is the source's own words.
#[derive(Debug, Clone, Copy, Default)]
pub struct UncitedFigures;

impl UncitedFigures {
    /// The first word of `turn` outside its quotes that contains a digit.
    fn figure(turn: &Turn) -> Option<String> {
        let mut text = turn.text.clone();
        for quote in &turn.quotes {
            text = text.replace(quote.text(), " ");
        }
        text.split_whitespace()
            .find(|word| word.chars().any(|c| c.is_ascii_digit()))
            .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()).to_owned())
    }
}

impl Analyser for UncitedFigures {
    fn id(&self) -> &str {
        "uncited_figures"
    }

    fn analyse(&self, script: &Script, _documents: &[Document]) -> Vec<Finding> {
        let mut findings: Vec<Finding> = script
            .turns()
            .iter()
            .enumerate()
            .filter(|(_, turn)| turn.citations.is_empty())
            .filter_map(|(i, turn)| {
                let figure = Self::figure(turn)?;
                Some(Finding {
                    analyser: self.id().to_owned(),
                    severity: Severity::Warning,
                    turn: Some(i),
                    message: format!("turn states {figure:?} but cites no claim"),
                })
            })
            .collect();
        if findings.is_empty() {
            findings.push(Finding {
                analyser: self.id().to_owned(),
                severity: Severity::Info,
                turn: None,
                message: "every turn that states a figure cites a claim".into(),
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use podling_types::{
        ClaimId, Emotion, Pace, Quote, SourceRef, Speaker, SpeakerId, TextSpan, Turn,
    };

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
            vec![turn(text, vec![quote], vec![])],
        )
        .unwrap()
    }

    fn turn(text: &str, quotes: Vec<Quote>, citations: Vec<ClaimId>) -> Turn {
        Turn {
            speaker: SpeakerId("h".into()),
            text: text.into(),
            emotion: Emotion::Neutral,
            citations,
            quotes,
            pace: Pace::Normal,
            nonverbal: vec![],
            callback_to: None,
        }
    }

    fn script_of(turns: Vec<Turn>) -> Script {
        Script::new(
            vec![Speaker {
                id: SpeakerId("h".into()),
                name: "H".into(),
                role: "host".into(),
            }],
            turns,
        )
        .unwrap()
    }

    #[test]
    fn an_uncited_year_is_a_warning_on_its_turn() {
        let script = script_of(vec![
            turn("It was a hot summer.", vec![], vec![]),
            turn("Back in 1908, mind you!", vec![], vec![]),
        ]);
        let findings = UncitedFigures.analyse(&script, &[]);
        assert_eq!(severities(&findings), vec![Severity::Warning]);
        assert_eq!(findings[0].turn, Some(1));
        assert!(findings[0].message.contains("\"1908\""), "{findings:?}");
    }

    #[test]
    fn cited_figures_and_quoted_figures_pass() {
        let d = Document::new(
            SourceRef {
                connector: "t".into(),
                locator: "a".into(),
                independence_group: "g".into(),
            },
            "A",
            "It fell in 1908.",
        );
        let quote = Quote::from_document(&d, TextSpan::new(0, 16).unwrap()).unwrap();
        let script = script_of(vec![
            turn(
                "It flattened 2,000 square kilometres.",
                vec![],
                vec![podling_types::Claim::id_for("x")],
            ),
            turn("The report: \"It fell in 1908.\"", vec![quote], vec![]),
            turn("No figures here, just one wild story.", vec![], vec![]),
        ]);
        let findings = UncitedFigures.analyse(&script, &[d]);
        assert_eq!(severities(&findings), vec![Severity::Info]);
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

    fn quote() -> Quote {
        Quote::from_document(&doc(), TextSpan::new(0, 15).unwrap()).unwrap()
    }

    #[test]
    fn flags_an_unreferenced_straight_quotation() {
        let text = "\"The sky burned.\" Then he said \"the whole forest was on fire\".";
        let findings = QuoteVerifier.analyse(&script_with(text, quote()), &[doc()]);
        assert_eq!(severities(&findings), vec![Severity::Error]);
        assert!(findings[0].message.contains("the whole forest was on fire"));
    }

    #[test]
    fn flags_an_unreferenced_curly_quotation() {
        let text = "\u{201C}The sky burned.\u{201D} And \u{201C}nothing survived at all\u{201D}.";
        let findings = QuoteVerifier.analyse(&script_with(text, quote()), &[doc()]);
        assert_eq!(severities(&findings), vec![Severity::Error]);
    }

    #[test]
    fn ignores_scare_quotes_and_unbalanced_marks() {
        let text = "\"The sky burned.\" A \"so-called\" event, said \"he";
        let findings = QuoteVerifier.analyse(&script_with(text, quote()), &[doc()]);
        assert_eq!(severities(&findings), vec![Severity::Info]);
    }

    #[test]
    fn accepts_a_part_of_a_referenced_quote() {
        // "The sky burned" is still the source's own words.
        let text = "\"The sky burned.\" Again: \"The sky burned\"";
        let findings = QuoteVerifier.analyse(&script_with(text, quote()), &[doc()]);
        assert_eq!(severities(&findings), vec![Severity::Info]);
    }
}
