//! The pipeline stages, and the premise windows the NLI stages share.
//! [`crate::pipeline::run`] calls the stages in this order: ingest, chunk,
//! extract_claims, then (with `[embedding]` and `[nli]`) ground_claims,
//! cluster_claims and score_stances, then ledger, adjudicate (which calls the
//! LLM only for Contested claims), script and analyse, then (with `[tts]`)
//! the chunks are planned, synthesize_chunk and transcribe_chunk run once per
//! take of each chunk until one passes, the sounds one speaker makes over
//! another's turn are synthesised on their own, and the episode is assembled.

pub mod adjudicate;
pub mod analyse;
pub mod assemble;
pub mod chunk;
pub mod cluster_claims;
pub mod extract_claims;
pub mod ground_claims;
pub mod ingest;
pub mod ledger;
pub mod plan_chunks;
pub mod score_stances;
pub mod script;
pub mod synthesize;
pub mod verify_audio;
pub mod windows;

pub use adjudicate::{Adjudicate, AdjudicateInput};
pub use analyse::{Analyse, AnalyseInput};
pub use chunk::ChunkDocuments;
pub use cluster_claims::ClusterClaims;
pub use extract_claims::{ClaimInput, ExtractClaims};
pub use ground_claims::{GroundClaims, GroundInput, Grounded, Rejection};
pub use ingest::Ingest;
pub use ledger::BuildLedger;
pub use plan_chunks::{Piece, PlannedChunk, plan_chunks};
pub use score_stances::{ScoreStances, StanceInput};
pub use script::{ScriptInput, WriteScript};
pub use synthesize::{
    ChunkInput, ChunkResult, ChunkSpec, ContextAudio, ContextSpec, OverlayClip, SynthesizeChunk,
    SynthesizedChunk, Takes, Verification, Voices, synthesize_overlays, synthesize_script,
};
pub use verify_audio::{Check, TranscribeChunk, TranscribeInput};
