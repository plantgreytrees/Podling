//! The episode's audio: which chunks were synthesised, how each was checked,
//! and how loud the finished file is.

use std::ops::Range;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::claim::PerMille;
use crate::ids::{ContentHash, SpeakerId};

/// A non-empty run of script turns, `start..end` (end exclusive).
///
/// Not a `std::ops::Range`: a range may be empty or backwards, and serde
/// would accept either. Deserialisation runs the same check as
/// [`TurnRange::new`], so an empty range can't be built at all.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "RawTurnRange")]
pub struct TurnRange {
    start: usize,
    end: usize,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawTurnRange {
    /// Index of the first turn.
    start: usize,
    /// One past the index of the last turn.
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("turn range {start}..{end} is empty: start must be below end")]
pub struct EmptyTurnRange {
    pub start: usize,
    pub end: usize,
}

impl TurnRange {
    pub fn new(start: usize, end: usize) -> Result<Self, EmptyTurnRange> {
        if start >= end {
            return Err(EmptyTurnRange { start, end });
        }
        Ok(Self { start, end })
    }

    pub fn start(self) -> usize {
        self.start
    }

    pub fn end(self) -> usize {
        self.end
    }

    pub fn contains(self, turn: usize) -> bool {
        (self.start..self.end).contains(&turn)
    }

    /// The same turns as a slice index, e.g. `&script.turns()[range.indices()]`.
    pub fn indices(self) -> Range<usize> {
        self.start..self.end
    }
}

impl TryFrom<RawTurnRange> for TurnRange {
    type Error = EmptyTurnRange;

    fn try_from(raw: RawTurnRange) -> Result<Self, Self::Error> {
        Self::new(raw.start, raw.end)
    }
}

/// The `audio.json` artifact.
///
/// Not `Eq`: loudness is measured as `f64`, which has no total equality
/// (`NaN`). The manifest is a stage output, never a cache key, so floats are
/// safe here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioManifest {
    /// Sample rate of the episode file, in Hz.
    pub sample_rate: u32,
    /// In playback order.
    pub chunks: Vec<ChunkRecord>,
    pub episode: EpisodeAudio,
    /// Where each voice came from, and under which licence.
    pub voices: Vec<VoiceCredit>,
}

/// One synthesised chunk: the take that was kept, and how it was checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChunkRecord {
    /// Cache key of the chunk: its text, voices and context.
    pub id: ContentHash,
    pub turns: TurnRange,
    /// The audio in the blob store.
    pub blob: ContentHash,
    /// Derived from `id` and the attempt, so a rerun reproduces the take.
    pub seed: u64,
    /// Which attempt this is, from 0.
    pub take: u8,
    /// Word error rate of the transcript against the chunk text.
    pub wer_pm: PerMille,
    /// Quotes missing from the transcript, verbatim from the script.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quote_misses: Vec<String>,
    /// Passed speech-recognition checks. A chunk that never passed is kept
    /// (the run completes) and reported as an `Error` finding.
    pub verified: bool,
}

/// The finished episode file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EpisodeAudio {
    /// Relative to the output directory, e.g. `episode.wav`.
    pub path: PathBuf,
    pub duration_ms: u64,
    /// EBU R128 integrated loudness, in LUFS (target −16).
    pub integrated_lufs: f64,
    /// True peak in dBTP (at most −1).
    pub true_peak_dbtp: f64,
}

/// Credit for one voice: the clip it was cloned from and its licence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceCredit {
    pub speaker: SpeakerId,
    pub reference: PathBuf,
    /// SPDX identifier, copied from the episode's `[[cast]]`.
    pub licence: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_ranges_are_never_empty() {
        let range = TurnRange::new(2, 5).unwrap();
        assert_eq!(range.indices(), 2..5);
        assert_eq!((range.start(), range.end()), (2, 5));
        assert!(range.contains(4) && !range.contains(5));
        assert_eq!(
            TurnRange::new(3, 3),
            Err(EmptyTurnRange { start: 3, end: 3 })
        );
        assert!(TurnRange::new(4, 1).is_err());

        let stored: TurnRange =
            serde_json::from_value(serde_json::json!({ "start": 0, "end": 1 })).unwrap();
        assert_eq!(stored, TurnRange::new(0, 1).unwrap());
        let err = serde_json::from_value::<TurnRange>(serde_json::json!({ "start": 1, "end": 1 }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty"), "{err}");
    }
}
