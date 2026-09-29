//! Runs the configured analysers over the finished script.

use podling_types::{AnalysisReport, Document, Script};
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::Result;
use crate::plugin::Analyser;
use crate::stage::Stage;

#[derive(Debug, Clone, Serialize)]
pub struct AnalyseInput {
    pub script: Script,
    pub documents: Vec<Document>,
}

pub struct Analyse<'a> {
    pub analysers: &'a [Box<dyn Analyser>],
}

impl Stage for Analyse<'_> {
    const ID: &'static str = "analyse";
    const VERSION: u32 = 1;
    type Input = AnalyseInput;
    type Output = AnalysisReport;

    fn config_fingerprint(&self) -> Value {
        let ids: Vec<&str> = self.analysers.iter().map(|a| a.id()).collect();
        json!({ "analysers": ids })
    }

    fn run(&self, input: &AnalyseInput) -> Result<AnalysisReport> {
        let findings = self
            .analysers
            .iter()
            .flat_map(|a| a.analyse(&input.script, &input.documents))
            .collect();
        Ok(AnalysisReport { findings })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::QuoteVerifier;
    use podling_types::{Emotion, Quote, SourceRef, Speaker, SpeakerId, TextSpan, Turn};

    #[test]
    fn a_paraphrased_quote_counts_as_one_error() {
        let source = SourceRef {
            connector: "t".into(),
            locator: "a".into(),
            independence_group: "g".into(),
        };
        let doc = Document::new(source, "A", "The sky burned.");
        let quote = Quote::from_document(&doc, TextSpan::new(0, 15).unwrap()).unwrap();
        let host = SpeakerId("h".into());
        let script = Script::new(
            vec![Speaker {
                id: host.clone(),
                name: "H".into(),
                role: "host".into(),
            }],
            vec![Turn {
                speaker: host,
                text: "Apparently the sky was on fire.".into(),
                emotion: Emotion::Neutral,
                citations: vec![],
                quotes: vec![quote],
            }],
        )
        .unwrap();

        let analysers: Vec<Box<dyn Analyser>> = vec![Box::new(QuoteVerifier)];
        let report = Analyse {
            analysers: &analysers,
        }
        .run(&AnalyseInput {
            script,
            documents: vec![doc],
        })
        .unwrap();
        assert_eq!(report.error_count(), 1);
    }
}
