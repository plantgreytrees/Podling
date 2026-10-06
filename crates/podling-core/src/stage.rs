//! The stage abstraction and the caching wrapper every stage runs through.

use std::time::Instant;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::{CacheKey, DiskCache};
use crate::error::{CoreError, Result};

/// One step of the pipeline: a deterministic function from input to output.
///
/// Its output is cached under a key built from `ID`, `VERSION`, the input and
/// [`Stage::config_fingerprint`]. **Bump `VERSION` whenever the stage's logic
/// changes**, or stale cached outputs will be reused.
pub trait Stage {
    const ID: &'static str;
    const VERSION: u32;
    type Input: Serialize;
    type Output: Serialize + DeserializeOwned;

    /// Settings and providers that affect the output (e.g. the LLM's
    /// fingerprint). Defaults to none.
    fn config_fingerprint(&self) -> Value {
        Value::Null
    }

    fn run(&self, input: &Self::Input) -> Result<Self::Output>;

    /// Whether a cached output can still be used. Defaults to yes; a stage
    /// whose output points at something stored elsewhere (an audio blob)
    /// says no when that has gone, and the stage runs again.
    fn is_reusable(&self, _output: &Self::Output) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageRecord {
    pub id: String,
    pub version: u32,
    pub key: String,
    pub cache_hit: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunReport {
    pub stages: Vec<StageRecord>,
    /// Number of `Error` findings in the analysis report.
    pub error_findings: usize,
    /// What `ground_claims` dropped; `None` when it didn't run (no
    /// `[embedding]` and `[nli]`). Filled in on a cache hit too.
    ///
    /// `#[serde(default)]` lets a report serialised before this field existed
    /// still deserialise: a missing field becomes `None` instead of an error.
    #[serde(default)]
    pub grounding: Option<GroundingCounts>,
    /// The episode's audio file; `None` without `[tts]`.
    #[serde(default)]
    pub audio: Option<std::path::PathBuf>,
}

/// Counts from the `ground_claims` stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GroundingCounts {
    /// Claims dropped because no chunk they came from entails them.
    pub dropped_claims: usize,
    /// Pieces of extraction evidence dropped, including those of claims that
    /// kept other evidence.
    pub rejected_evidence: usize,
}

/// Runs `stage`, reusing a cached output when one exists. With `cache` set to
/// `None` the stage always runs and nothing is stored.
pub fn cached<S: Stage>(
    stage: &S,
    input: &S::Input,
    cache: Option<&DiskCache>,
    report: &mut RunReport,
) -> Result<S::Output> {
    let span = tracing::info_span!("stage", id = S::ID, version = S::VERSION);
    let _entered = span.enter();
    let started = Instant::now();
    let wrap = |err: CoreError| CoreError::Stage {
        stage: S::ID,
        source: Box::new(err),
    };

    let key = CacheKey::new(S::ID, S::VERSION, input, &stage.config_fingerprint()).map_err(wrap)?;
    let hit = match cache {
        Some(cache) => cache.get::<S::Output>(&key).map_err(wrap)?,
        None => None,
    };
    let hit = hit.filter(|output| {
        let usable = stage.is_reusable(output);
        if !usable {
            tracing::warn!(key = %key, "cached output refers to data that is gone; running again");
        }
        usable
    });
    let cache_hit = hit.is_some();
    let output = match hit {
        Some(output) => output,
        None => {
            let output = stage.run(input).map_err(wrap)?;
            if let Some(cache) = cache {
                cache.put(&key, S::ID, &output).map_err(wrap)?;
            }
            output
        }
    };

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(cache_hit, elapsed_ms, key = %key, "stage finished");
    report.stages.push(StageRecord {
        id: S::ID.to_owned(),
        version: S::VERSION,
        key: key.to_string(),
        cache_hit,
        elapsed_ms,
    });
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct Counting<const V: u32> {
        runs: Cell<u32>,
    }

    impl<const V: u32> Stage for Counting<V> {
        const ID: &'static str = "counting";
        const VERSION: u32 = V;
        type Input = String;
        type Output = usize;

        fn run(&self, input: &String) -> Result<usize> {
            self.runs.set(self.runs.get() + 1);
            Ok(input.len())
        }
    }

    #[test]
    fn second_call_is_a_cache_hit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let stage = Counting::<1> { runs: Cell::new(0) };
        let mut report = RunReport::default();
        let input = "hello".to_string();

        assert_eq!(
            cached(&stage, &input, Some(&cache), &mut report).unwrap(),
            5
        );
        assert_eq!(
            cached(&stage, &input, Some(&cache), &mut report).unwrap(),
            5
        );
        assert_eq!(stage.runs.get(), 1);
        let hits: Vec<bool> = report.stages.iter().map(|s| s.cache_hit).collect();
        assert_eq!(hits, vec![false, true]);
    }

    #[test]
    fn version_bump_invalidates() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let mut report = RunReport::default();
        let input = "hello".to_string();
        cached(
            &Counting::<1> { runs: Cell::new(0) },
            &input,
            Some(&cache),
            &mut report,
        )
        .unwrap();

        let bumped = Counting::<2> { runs: Cell::new(0) };
        cached(&bumped, &input, Some(&cache), &mut report).unwrap();
        assert_eq!(
            bumped.runs.get(),
            1,
            "a new version must not reuse the old output"
        );
    }

    #[test]
    fn no_cache_always_runs() {
        let stage = Counting::<1> { runs: Cell::new(0) };
        let mut report = RunReport::default();
        cached(&stage, &"x".to_string(), None, &mut report).unwrap();
        cached(&stage, &"x".to_string(), None, &mut report).unwrap();
        assert_eq!(stage.runs.get(), 2);
    }

    /// Reuses nothing: every cached output is stale.
    struct Picky {
        runs: Cell<u32>,
    }

    impl Stage for Picky {
        const ID: &'static str = "picky";
        const VERSION: u32 = 1;
        type Input = String;
        type Output = usize;

        fn run(&self, input: &String) -> Result<usize> {
            self.runs.set(self.runs.get() + 1);
            Ok(input.len())
        }

        fn is_reusable(&self, _output: &usize) -> bool {
            false
        }
    }

    #[test]
    fn an_unusable_cached_output_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let stage = Picky { runs: Cell::new(0) };
        let mut report = RunReport::default();
        for _ in 0..2 {
            cached(&stage, &"x".to_string(), Some(&cache), &mut report).unwrap();
        }
        assert_eq!(stage.runs.get(), 2);
        assert!(report.stages.iter().all(|s| !s.cache_hit));
    }
}
