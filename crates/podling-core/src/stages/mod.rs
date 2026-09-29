//! The six pipeline stages, in the order [`crate::pipeline::run`] calls them.

pub mod analyse;
pub mod chunk;
pub mod extract_claims;
pub mod ingest;
pub mod ledger;
pub mod script;

pub use analyse::{Analyse, AnalyseInput};
pub use chunk::ChunkDocuments;
pub use extract_claims::{ClaimInput, ExtractClaims};
pub use ingest::Ingest;
pub use ledger::BuildLedger;
pub use script::{ScriptInput, WriteScript};
