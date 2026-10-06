//! JSON Schema export: the language-neutral contract for non-Rust tools
//! (Python workers, a future UI) that read or write Podling artifacts.

use schemars::{JsonSchema, Schema, schema_for};

use crate::analysis::AnalysisReport;
use crate::audio::AudioManifest;
use crate::claim::Claim;
use crate::document::{Chunk, Document};
use crate::envelope::{ArtifactKind, Envelope};
use crate::episode::EpisodeSpec;
use crate::ledger::Ledger;
use crate::script::Script;
use crate::verdict::Verdicts;

/// The schema of each artifact kind as written to disk: the episode spec is
/// plain TOML; every other artifact is wrapped in an [`Envelope`].
pub fn all() -> Vec<(ArtifactKind, Schema)> {
    ArtifactKind::ALL
        .into_iter()
        .map(|kind| (kind, of(kind)))
        .collect()
}

pub fn of(kind: ArtifactKind) -> Schema {
    fn enveloped<T: JsonSchema>() -> Schema {
        schema_for!(Envelope<T>)
    }
    match kind {
        ArtifactKind::Episode => schema_for!(EpisodeSpec),
        ArtifactKind::Documents => enveloped::<Vec<Document>>(),
        ArtifactKind::Chunks => enveloped::<Vec<Chunk>>(),
        ArtifactKind::Claims => enveloped::<Vec<Claim>>(),
        ArtifactKind::Ledger => enveloped::<Ledger>(),
        ArtifactKind::Verdicts => enveloped::<Verdicts>(),
        ArtifactKind::Script => enveloped::<Script>(),
        ArtifactKind::Analysis => enveloped::<AnalysisReport>(),
        ArtifactKind::Audio => enveloped::<AudioManifest>(),
    }
}
