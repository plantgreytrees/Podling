//! Podling pipeline, content cache, and plugin contracts.

pub mod cache;
pub mod error;
pub mod plugin;
pub mod text;

pub use cache::{CacheKey, CacheStats, DiskCache};
pub use error::{CoreError, Result};
