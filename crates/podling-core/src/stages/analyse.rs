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
