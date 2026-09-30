//! The episode specification: the TOML file a user writes to request an episode.
//!
//! Contains no secrets. Plugins that need credentials will take the *name* of
//! an environment variable, never the value.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EpisodeSpec {
    pub title: String,
    pub topic: String,
    #[serde(default)]
    pub mode: Mode,
    pub target_minutes: u16,
    pub sources: Vec<SourceSpec>,
    pub llm: LlmConfig,
    /// Embeds claims and source sentences so the NLI stages can pick which
    /// pairs to compare. Needs `nli` too; without both, claims merge only by
    /// exact wording and every piece of evidence is the claim's own extraction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<EmbeddingConfig>,
    /// Judges whether one text entails or contradicts another. Needs
    /// `embedding` too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nli: Option<NliConfig>,
    #[serde(default)]
    pub analysers: Vec<AnalyserConfig>,
}

/// What kind of episode to make. Only non-fiction exists today; fiction modes
/// are planned, so matches on this enum must stay open-ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Mode {
    #[default]
    NonFiction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceSpec {
    /// Every `.md` / `.txt` file directly under `root`. A relative `root` is
    /// resolved against the episode file's directory.
    LocalFiles {
        root: PathBuf,
        independence_group: String,
    },
}

// Variants are written `Name {}` rather than `Name`: serde only enforces
// `deny_unknown_fields` on struct variants of an internally tagged enum, so a
// unit variant would silently accept stray keys.

// Not `Eq`: `temperature` is an `f32`, which has no total equality (NaN).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LlmConfig {
    /// Deterministic offline stand-in, for development and tests.
    Fake {},
    /// Any server that speaks the OpenAI chat-completions protocol: Ollama,
    /// llama.cpp `llama-server`, vLLM, LM Studio, OpenAI itself.
    ///
    /// There is deliberately no field for the key itself, only the name of the
    /// environment variable that holds it, so an episode file stays free of
    /// secrets and `deny_unknown_fields` rejects a pasted `api_key = "..."`.
    OpenAiCompat {
        /// Base URL up to and including the version segment, e.g.
        /// `http://localhost:11434/v1`. Must be `http://` or `https://`.
        base_url: String,
        /// Model name as the server knows it, e.g. `llama3.1:8b`.
        model: String,
        /// Name of the environment variable holding the API key. Leave unset
        /// for local servers that need none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key_env: Option<String>,
        /// Sampling temperature; the server's default when unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temperature: Option<f32>,
        /// Per-request timeout in seconds; the provider's default when unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_secs: Option<u64>,
        /// Cap on generated tokens per request; the server's default when unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_output_tokens: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmbeddingConfig {
    /// Deterministic offline stand-in: a hashed bag of words.
    Fake {},
    /// Any server that speaks the OpenAI embeddings protocol
    /// (`POST {base_url}/embeddings`), e.g. Ollama with `nomic-embed-text`.
    OpenAiCompat {
        /// Base URL up to and including the version segment, e.g.
        /// `http://localhost:11434/v1`. Must be `http://` or `https://`.
        base_url: String,
        /// Model name as the server knows it, e.g. `nomic-embed-text`.
        model: String,
        /// Name of the environment variable holding the API key, never the
        /// key itself. Leave unset for local servers that need none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key_env: Option<String>,
        /// Per-request timeout in seconds; the provider's default when unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_secs: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NliConfig {
    /// Deterministic offline stand-in: word containment, with a changed
    /// number counted as a contradiction.
    Fake {},
    /// A local DeBERTa-v3 NLI cross-encoder, run on the CPU, e.g.
    /// `cross-encoder/nli-deberta-v3-base`.
    CrossEncoder {
        /// Directory holding the model's `config.json`, `tokenizer.json` and
        /// `model.safetensors`. A relative path is resolved against the
        /// episode file's directory.
        model_dir: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnalyserConfig {
    /// Checks every quote against its source, word for word.
    QuoteVerifier {},
}
