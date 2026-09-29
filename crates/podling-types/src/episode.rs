//! The episode specification: the TOML file a user writes to request an episode.
//!
//! Contains no secrets. Plugins that need credentials will take the *name* of
//! an environment variable, never the value.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EpisodeSpec {
    pub title: String,
    pub topic: String,
    #[serde(default)]
    pub mode: Mode,
    pub target_minutes: u16,
    pub sources: Vec<SourceSpec>,
    pub llm: LlmConfig,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LlmConfig {
    /// Deterministic offline stand-in, for development and tests.
    Fake {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnalyserConfig {
    /// Checks every quote against its source, word for word.
    QuoteVerifier {},
}
