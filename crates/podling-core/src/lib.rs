//! Podling pipeline, content cache, and plugin contracts.

pub mod cache;
pub mod error;
pub mod pipeline;
pub mod plugin;
pub mod stage;
pub mod stages;
pub mod text;

pub use cache::{CacheKey, CacheStats, DiskCache};
pub use error::{CoreError, Result};
pub use stage::{RunReport, Stage, StageRecord, cached};
