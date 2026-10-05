//! Plugin contracts and the factories that build plugins from episode config.
//!
//! Plugins are trait objects because the episode file chooses them at run
//! time. The factories are plain `match`es: adding a plugin means adding an
//! enum variant in `podling-types` and one arm here.

pub mod analyser;
pub mod asr;
pub mod cross_encoder;
pub mod embedding;
mod http;
pub mod llm;
pub mod nli;
mod ollama;
pub mod openai;
pub mod openai_embeddings;
pub mod sidecar;
pub mod sidecar_tts;
pub mod source;
pub mod tts;
pub mod whisper;

use std::collections::BTreeSet;
use std::path::Path;

use podling_types::{
    AnalyserConfig, AsrConfig, EmbeddingConfig, EpisodeSpec, LlmConfig, NliConfig, SourceSpec,
    TtsConfig,
};

use crate::error::{CoreError, Result};

pub use analyser::{Analyser, QuoteVerifier, UncitedFigures};
pub use asr::{AsrProvider, AsrRequest, FakeAsr, Segment, Transcript, transcribe_checked};
pub use cross_encoder::CrossEncoderNli;
pub use embedding::{EmbeddingProvider, FakeEmbedding, cosine, embed_checked};
pub use llm::{
    ClaimDraft, ClaimsDraft, Completion, CompletionRequest, DraftTurn, FakeLlm, LedgerClaim,
    LlmProvider, LlmTask, NumberedSentence, PROMPT_VERSION, QuoteRef, ScriptDraft, SourceText,
    complete_validated,
};
pub use nli::{FakeNli, NliPair, NliProvider, NliScores, score_checked};
pub use openai::OpenAiCompat;
pub use openai_embeddings::OpenAiEmbeddings;
pub use sidecar::{Sidecar, SidecarProfile, default_profiles_path, load_profile};
pub use sidecar_tts::SidecarTts;
pub use source::{LocalFilesConnector, SourceConnector};
pub use tts::{
    ChunkAudio, ChunkContext, ChunkRequest, FakeTts, SpokenTurn, TtsCapabilities, TtsProvider,
    synthesize_checked,
};
pub use whisper::CandleWhisper;

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

/// Checks the audio sections: `[[cast]]` ids are unique, and `[tts]` comes
/// with a `[[cast]]` (the pinned voices) and an `[asr]` (to verify the audio).
/// Like [`build_grounding`], a half configuration is reported before any
/// stage runs.
pub fn check_audio(spec: &EpisodeSpec) -> Result<()> {
    let config = |message: &str| CoreError::Config {
        message: message.into(),
    };
    let mut ids = BTreeSet::new();
    for member in &spec.cast {
        if !ids.insert(&member.id) {
            return Err(CoreError::Config {
                message: format!("[[cast]] lists speaker {:?} twice", member.id.0),
            });
        }
    }
    match (&spec.tts, &spec.asr) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(config("[asr] checks synthesised audio, so it needs [tts]")),
        (Some(_), None) => Err(config(
            "[tts] needs [asr] to check the audio against the script",
        )),
        (Some(_), Some(_)) if spec.cast.is_empty() => Err(config(
            "[tts] needs [[cast]]: one entry per speaker, each with a voice",
        )),
        (Some(TtsConfig::Sidecar { takes: 0, .. }), Some(_)) => {
            Err(config("[tts] takes must be at least 1"))
        }
        (Some(_), Some(_)) => Ok(()),
    }
}

/// Builds the TTS provider. A sidecar profile is read from `profiles` (the
/// user-level `sidecars.toml`, never the episode file) and its worker is
/// started here, so call this just before synthesis and drop the provider
/// right after: dropping it stops the worker and frees the GPU.
pub fn build_tts(config: &TtsConfig, profiles: &Path) -> Result<Box<dyn TtsProvider>> {
    match config {
        TtsConfig::Fake {} => Ok(Box::new(FakeTts::default())),
        TtsConfig::Sidecar { sidecar, .. } => {
            let profile = load_profile(profiles, sidecar)?;
            Ok(Box::new(SidecarTts::start(sidecar, &profile, profiles)?))
        }
    }
}

/// Builds the speech recogniser. A relative `model_dir` resolves against
/// `base_dir`. Whisper is only checked and fingerprinted here; it loads on
/// its first transcript, on the CPU.
pub fn build_asr(config: &AsrConfig, base_dir: &Path) -> Result<Box<dyn AsrProvider>> {
    match config {
        AsrConfig::Fake {} => Ok(Box::new(FakeAsr::default())),
        AsrConfig::Whisper { model_dir, .. } => {
            Ok(Box::new(CandleWhisper::new(&base_dir.join(model_dir))?))
        }
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
                AnalyserConfig::UncitedFigures {} => Box::new(UncitedFigures),
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
            analysers = [{ kind = "quote_verifier" }, { kind = "uncited_figures" }]
            "#,
        )
        .unwrap();
        assert_eq!(build_llm(&spec.llm).unwrap().id(), "fake");
        let sources = build_sources(&spec.sources, Path::new("/episodes"));
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id(), "local_files");
        let analysers = build_analysers(&spec.analysers);
        assert_eq!(analysers[0].id(), "quote_verifier");
        assert_eq!(analysers[1].id(), "uncited_figures");
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

    const CAST: &str = r#"
        [[cast]]
        id = "ada"
        name = "Ada"
        role = "host"
        voice = { reference = "voices/ada.wav", transcript = "Hello.", licence = "CC0-1.0" }
    "#;

    fn audio_error(extra: &str) -> String {
        match check_audio(&episode(extra)) {
            Err(CoreError::Config { message }) => message,
            other => panic!("expected a Config error for {extra:?}, got {other:?}"),
        }
    }

    #[test]
    fn audio_sections_come_together() {
        check_audio(&episode("")).unwrap();
        // A cast alone is fine: it pins the script's speakers.
        check_audio(&episode(CAST)).unwrap();
        let full = format!("tts = {{ kind = \"fake\" }}\nasr = {{ kind = \"fake\" }}\n{CAST}");
        check_audio(&episode(&full)).unwrap();

        let message = audio_error("tts = { kind = \"fake\" }\nasr = { kind = \"fake\" }");
        assert!(message.contains("[[cast]]"), "{message}");
        let message = audio_error(&format!("tts = {{ kind = \"fake\" }}\n{CAST}"));
        assert!(message.contains("needs [asr]"), "{message}");
        let message = audio_error(&format!("asr = {{ kind = \"fake\" }}\n{CAST}"));
        assert!(message.contains("needs [tts]"), "{message}");
        let message = audio_error(&format!(
            "tts = {{ kind = \"sidecar\", sidecar = \"qwen\", takes = 0 }}\n\
             asr = {{ kind = \"fake\" }}\n{CAST}"
        ));
        assert!(message.contains("takes"), "{message}");
    }

    #[test]
    fn builds_the_tts_provider_from_an_episode() {
        let full = format!("tts = {{ kind = \"fake\" }}\nasr = {{ kind = \"fake\" }}\n{CAST}");
        let spec = episode(&full);
        let profiles = Path::new("/no/sidecars.toml");
        let tts = build_tts(spec.tts.as_ref().unwrap(), profiles).unwrap();
        assert_eq!(tts.id(), "fake");
        assert!(tts.capabilities().native_sample_rate > 0);

        // A sidecar profile comes from the profiles file, which must exist.
        let sidecar = TtsConfig::Sidecar {
            sidecar: "qwen".into(),
            takes: 2,
            max_retries: 2,
        };
        let Err(CoreError::Config { message }) = build_tts(&sidecar, profiles) else {
            panic!("a missing profiles file must be a Config error");
        };
        assert!(message.contains("/no/sidecars.toml"), "{message}");
    }

    #[test]
    fn cast_ids_are_unique() {
        let message = audio_error(&format!("{CAST}\n{CAST}"));
        assert!(message.contains("\"ada\" twice"), "{message}");
    }
}
