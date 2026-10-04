//! The pipeline stages, and the premise windows the NLI stages share.
//! [`crate::pipeline::run`] calls the stages in this order: ingest, chunk,
//! extract_claims, then (with `[embedding]` and `[nli]`) ground_claims,
//! cluster_claims and score_stances, then ledger, script and analyse.

pub mod analyse;
pub mod chunk;
pub mod cluster_claims;
pub mod extract_claims;
pub mod ground_claims;
pub mod ingest;
pub mod ledger;
pub mod score_stances;
pub mod script;
pub mod windows;

pub use analyse::{Analyse, AnalyseInput};
pub use chunk::ChunkDocuments;
pub use cluster_claims::ClusterClaims;
pub use extract_claims::{ClaimInput, ExtractClaims};
pub use ground_claims::{GroundClaims, GroundInput, Grounded, Rejection};
pub use ingest::Ingest;
pub use ledger::BuildLedger;
pub use score_stances::{ScoreStances, StanceInput};
pub use script::{ScriptInput, WriteScript};
