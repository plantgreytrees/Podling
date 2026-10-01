//! Versioned wrapper written around every artifact on disk.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Bump when any artifact's serialised shape changes (the schema snapshot
/// test fails first, as a reminder). Readers treat other versions as foreign.
pub const SCHEMA_VERSION: u32 = 3;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Episode,
    Documents,
    Chunks,
    Claims,
    Ledger,
    Script,
    Analysis,
}

impl ArtifactKind {
    pub const ALL: [ArtifactKind; 7] = [
        Self::Episode,
        Self::Documents,
        Self::Chunks,
        Self::Claims,
        Self::Ledger,
        Self::Script,
        Self::Analysis,
    ];

    /// File-name stem, e.g. `ledger` for `ledger.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Episode => "episode",
            Self::Documents => "documents",
            Self::Chunks => "chunks",
            Self::Claims => "claims",
            Self::Ledger => "ledger",
            Self::Script => "script",
            Self::Analysis => "analysis",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Envelope<T> {
    pub schema_version: u32,
    pub kind: ArtifactKind,
    pub body: T,
}

impl<T> Envelope<T> {
    pub fn new(kind: ArtifactKind, body: T) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            kind,
            body,
        }
    }
}
