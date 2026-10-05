//! Artifact types shared by every Podling stage and plugin.
//!
//! Types that carry invariants keep their fields private and are built through
//! a checked constructor; deserialisation runs the same checks wherever the
//! invariant can be verified without extra context.

pub mod analysis;
pub mod audio;
pub mod claim;
pub mod document;
pub mod envelope;
pub mod episode;
pub mod ids;
pub mod ledger;
pub mod quote;
pub mod schema;
pub mod script;

pub use analysis::{AnalysisReport, Finding, Severity};
pub use audio::{AudioManifest, ChunkRecord, EmptyTurnRange, EpisodeAudio, TurnRange, VoiceCredit};
pub use claim::{Claim, Evidence, EvidenceBasis, InvalidPerMille, PerMille, Stance};
pub use document::{Chunk, Document, SourceRef, TextSpan};
pub use envelope::{ArtifactKind, Envelope, SCHEMA_VERSION};
pub use episode::{
    AnalyserConfig, AsrConfig, CastMember, EmbeddingConfig, EpisodeSpec, LlmConfig, Mode,
    NliConfig, SourceSpec, TtsConfig, VoiceRef, VoiceRefError,
};
pub use ids::{ChunkId, ClaimId, ContentHash, DocumentId, SourceId, SpeakerId};
pub use ledger::{ClaimStatus, Ledger, LedgerEntry, classify};
pub use quote::{Quote, QuoteError};
pub use script::{
    Beat, BeatKind, Emotion, Nonverbal, NonverbalAt, NonverbalKind, Pace, Script, ScriptError,
    Speaker, Turn,
};
