//! Plugin contracts and the factories that build plugins from episode config.
//!
//! Plugins are trait objects because the episode file chooses them at run
//! time. The factories are plain `match`es: adding a plugin means adding an
//! enum variant in `podling-types` and one arm here.

pub mod analyser;
pub mod cross_encoder;
pub mod embedding;
mod http;
pub mod llm;
pub mod nli;
pub mod openai;
pub mod openai_embeddings;
pub mod source;

use std::path::Path;

use podling_types::{
    AnalyserConfig, EmbeddingConfig, EpisodeSpec, LlmConfig, NliConfig, SourceSpec,
};

use crate::error::{CoreError, Result};

pub use analyser::{Analyser, QuoteVerifier};
pub use cross_encoder::CrossEncoderNli;
pub use embedding::{EmbeddingProvider, FakeEmbedding, cosine, embed_checked};
pub use llm::{
    ADJUDICATE_PROMPT_VERSION, AdjudicationClaim, AdjudicationEvidence, ClaimDraft, ClaimsDraft,
    Completion, CompletionRequest, DraftTurn, FakeLlm, LedgerClaim, LedgerVerdict, LlmProvider,
    LlmTask, MAX_REASON_CHARS, NumberedSentence, PROMPT_VERSION, QuoteRef, ScriptDraft, SourceText,
    VerdictDraft, complete_validated, reason_excerpt,
};
pub use nli::{FakeNli, NliPair, NliProvider, NliScores, score_checked};
pub use openai::OpenAiCompat;
pub use openai_embeddings::OpenAiEmbeddings;
pub use source::{LocalFilesConnector, SourceConnector};

/// Builds the LLM provider. Fallible because a real provider reads its
/// configuration (a key from the environment, a URL) at construction.
pub fn build_llm(config: &LlmConfig) -> Result<Box<dyn LlmProvider>> {
    match config {
        LlmConfig::Fake {} => Ok(Box::new(FakeLlm)),
        LlmConfig::OpenAiCompat { .. } => Ok(Box::new(OpenAiCompat::from_config(config)?)),
    }
}

/// The two providers the NLI stages need. An episode has both or neither.
///
/// Owned (`Box`), so dropping a `Grounding` frees both providers, and with
/// them any model they loaded: the pipeline drops it before the script stage.
pub struct Grounding {
    pub embedder: Box<dyn EmbeddingProvider>,
    pub nli: Box<dyn NliProvider>,
}

/// Builds the NLI stages' providers from `[embedding]` and `[nli]`: `None`
/// when the episode has neither, an error when it has only one. Embeddings
/// alone can't judge anything, and NLI alone has no cheap way to pick which
/// pairs to judge, so half a configuration is a mistake worth reporting.
pub fn build_grounding(spec: &EpisodeSpec, base_dir: &Path) -> Result<Option<Grounding>> {
    match (&spec.embedding, &spec.nli) {
        (None, None) => Ok(None),
        (Some(embedding), Some(nli)) => Ok(Some(Grounding {
            embedder: build_embedder(embedding)?,
            nli: build_nli(nli, base_dir)?,
        })),
        _ => Err(CoreError::Config {
            message: "[embedding] and [nli] work together: set both, or neither".into(),
        }),
    }
}

/// Builds the embedding provider.
pub fn build_embedder(config: &EmbeddingConfig) -> Result<Box<dyn EmbeddingProvider>> {
    match config {
        EmbeddingConfig::Fake {} => Ok(Box::new(FakeEmbedding)),
        EmbeddingConfig::OpenAiCompat { .. } => {
            Ok(Box::new(OpenAiEmbeddings::from_config(config)?))
        }
    }
}

/// Builds the NLI provider. A relative `model_dir` resolves against
/// `base_dir`, normally the directory containing the episode file.
/// The real model is not loaded here, only checked and fingerprinted.
pub fn build_nli(config: &NliConfig, base_dir: &Path) -> Result<Box<dyn NliProvider>> {
    match config {
        NliConfig::Fake {} => Ok(Box::new(FakeNli)),
        // `join` keeps an absolute `model_dir` as it is.
        NliConfig::CrossEncoder { model_dir } => {
            Ok(Box::new(CrossEncoderNli::new(&base_dir.join(model_dir))?))
        }
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
        assert_eq!(build_llm(&spec.llm).unwrap().id(), "fake");
        let sources = build_sources(&spec.sources, Path::new("/episodes"));
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id(), "local_files");
        let analysers = build_analysers(&spec.analysers);
        assert_eq!(analysers[0].id(), "quote_verifier");
        assert!(
            build_grounding(&spec, Path::new("/episodes"))
                .unwrap()
                .is_none()
        );
    }

    fn episode(extra: &str) -> EpisodeSpec {
        toml::from_str(&format!(
            "title = \"T\"\ntopic = \"T\"\ntarget_minutes = 5\nllm = {{ kind = \"fake\" }}\n\
             sources = []\n{extra}"
        ))
        .unwrap()
    }

    #[test]
    fn grounding_needs_both_embedding_and_nli() {
        let both = episode("embedding = { kind = \"fake\" }\nnli = { kind = \"fake\" }");
        let grounding = build_grounding(&both, Path::new(".")).unwrap().unwrap();
        assert_eq!(
            (grounding.embedder.id(), grounding.nli.id()),
            ("fake", "fake")
        );

        for half in [
            "embedding = { kind = \"fake\" }",
            "nli = { kind = \"fake\" }",
        ] {
            let Err(CoreError::Config { message }) =
                build_grounding(&episode(half), Path::new("."))
            else {
                panic!("half a configuration must be a Config error: {half}");
            };
            assert!(
                message.contains("[embedding]") && message.contains("[nli]"),
                "{message}"
            );
        }
    }
}
