//! Plugin contracts and the factories that build plugins from episode config.
//!
//! Plugins are trait objects because the episode file chooses them at run
//! time. The factories are plain `match`es: adding a plugin means adding an
//! enum variant in `podling-types` and one arm here.

pub mod analyser;
pub mod llm;
pub mod source;

use std::path::Path;

use podling_types::{AnalyserConfig, LlmConfig, SourceSpec};

pub use analyser::{Analyser, QuoteVerifier};
pub use llm::{
    ClaimDraft, Completion, CompletionRequest, DraftTurn, FakeLlm, LlmProvider, LlmTask, QuoteRef,
    ScriptDraft,
};
pub use source::{LocalFilesConnector, SourceConnector};

pub fn build_llm(config: &LlmConfig) -> Box<dyn LlmProvider> {
    match config {
        LlmConfig::Fake {} => Box::new(FakeLlm),
    }
}

/// Builds source connectors. Relative paths resolve against `base_dir`,
/// normally the directory containing the episode file.
pub fn build_sources(specs: &[SourceSpec], base_dir: &Path) -> Vec<Box<dyn SourceConnector>> {
    specs
        .iter()
        .map(|spec| -> Box<dyn SourceConnector> {
            match spec {
                SourceSpec::LocalFiles {
                    root,
                    independence_group,
                } => Box::new(
                    LocalFilesConnector::new(base_dir.join(root), independence_group.clone())
                        .with_label(root),
                ),
            }
        })
        .collect()
}

pub fn build_analysers(configs: &[AnalyserConfig]) -> Vec<Box<dyn Analyser>> {
    configs
        .iter()
        .map(|config| -> Box<dyn Analyser> {
            match config {
                AnalyserConfig::QuoteVerifier {} => Box::new(QuoteVerifier),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use podling_types::EpisodeSpec;

    #[test]
    fn builds_every_plugin_from_an_episode() {
        let spec: EpisodeSpec = toml::from_str(
            r#"
            title = "T"
            topic = "T"
            target_minutes = 5
            llm = { kind = "fake" }
            sources = [{ kind = "local_files", root = "src", independence_group = "g" }]
            analysers = [{ kind = "quote_verifier" }]
            "#,
        )
        .unwrap();
        assert_eq!(build_llm(&spec.llm).id(), "fake");
        let sources = build_sources(&spec.sources, Path::new("/episodes"));
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id(), "local_files");
        let analysers = build_analysers(&spec.analysers);
        assert_eq!(analysers[0].id(), "quote_verifier");
    }
}
