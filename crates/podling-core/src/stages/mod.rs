//! The six pipeline stages, in the order [`crate::pipeline::run`] calls them.

pub mod analyse;
pub mod chunk;
pub mod cluster_claims;
pub mod extract_claims;
pub mod ingest;
pub mod ledger;
pub mod score_stances;
pub mod script;
pub mod windows;

pub use analyse::{Analyse, AnalyseInput};
pub use chunk::ChunkDocuments;
pub use cluster_claims::ClusterClaims;
pub use extract_claims::{ClaimInput, ExtractClaims};
pub use ingest::Ingest;
pub use ledger::BuildLedger;
pub use score_stances::{ScoreStances, StanceInput};
pub use script::{ScriptInput, WriteScript};
