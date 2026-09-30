//! Atomic claims extracted from sources, with the evidence behind them.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

use crate::document::TextSpan;
use crate::ids::{ChunkId, ClaimId, ContentHash, IdMismatch, SourceId};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Supports,
    Contradicts,
}

/// One chunk's position on a claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    pub chunk: ChunkId,
    pub source: SourceId,
    pub independence_group: String,
    pub stance: Stance,
    /// How this evidence was established, when it wasn't simply extracted from
    /// the chunk in the claim's own words. Absent for plain extraction, so
    /// artifacts written without the NLI stages are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<EvidenceBasis>,
}

/// The audit trail behind a piece of evidence that exact wording alone didn't
/// establish. The stored scores are the ones that decided it, so a status can
/// be checked by hand against the thresholds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceBasis {
    /// The chunk stated the claim in other words, and the two wordings were
    /// merged because each entails the other.
    Merged {
        /// The chunk's own wording of the claim.
        wording: String,
        /// The weaker of the two entailment directions.
        entailment_pm: PerMille,
    },
    /// An NLI model read `premise` (a span of the chunk's document) and judged
    /// that it supports or contradicts the claim.
    Nli {
        premise: TextSpan,
        /// How close the premise was to the claim when it was retrieved.
        similarity_pm: PerMille,
        entailment_pm: PerMille,
        contradiction_pm: PerMille,
    },
}

/// A score from 0 to 1 in thousandths: 0 to 1000.
///
/// An integer rather than an `f32`, because floats have neither total
/// equality nor total order (`NaN != NaN`), and `Evidence` needs both: claims
/// keep their evidence sorted and de-duplicated. Integers also serialise
/// identically everywhere, which the content-hash cache relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct PerMille(u16);

// Written by hand: derived, the schema would describe the inner `u16`
// (0 to 65535), not the range deserialisation enforces.
impl JsonSchema for PerMille {
    fn schema_name() -> Cow<'static, str> {
        "PerMille".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A score from 0 to 1 in thousandths.",
            "type": "integer",
            "minimum": 0,
            "maximum": PerMille::MAX,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0} is not a per-mille score (0 to 1000)")]
pub struct InvalidPerMille(pub u16);

impl PerMille {
    pub const MAX: u16 = 1000;

    pub fn new(value: u16) -> Result<Self, InvalidPerMille> {
        if value > Self::MAX {
            return Err(InvalidPerMille(value));
        }
        Ok(Self(value))
    }

    /// Rounds a probability to the nearest thousandth. Out-of-range values
    /// are clamped and `NaN` becomes 0, so a misbehaving model can't produce
    /// an invalid score.
    pub fn from_probability(p: f32) -> Self {
        if p.is_nan() {
            return Self(0);
        }
        Self((p.clamp(0.0, 1.0) * 1000.0).round() as u16)
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

// `#[serde(try_from = "u16")]` deserialises a plain number, then runs this
// check, so a stored score over 1000 is rejected like any malformed input.
impl TryFrom<u16> for PerMille {
    type Error = InvalidPerMille;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PerMille> for u16 {
    fn from(value: PerMille) -> Self {
        value.0
    }
}

/// A single checkable statement. Claims whose text differs only in case or
/// whitespace share an id, so the same fact from two sources merges into one
/// claim with two pieces of evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawClaim")]
pub struct Claim {
    id: ClaimId,
    text: String,
    evidence: Vec<Evidence>,
}

#[derive(Deserialize, JsonSchema)]
struct RawClaim {
    id: ClaimId,
    text: String,
    evidence: Vec<Evidence>,
}

impl TryFrom<RawClaim> for Claim {
    type Error = IdMismatch;

    fn try_from(raw: RawClaim) -> Result<Self, Self::Error> {
        let mut claim = Self::new(raw.text);
        if claim.id != raw.id {
            return Err(IdMismatch {
                kind: "claim",
                stored: raw.id.to_string(),
                expected: claim.id.to_string(),
            });
        }
        for evidence in raw.evidence {
            claim.add_evidence(evidence);
        }
        Ok(claim)
    }
}

impl Claim {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            id: Self::id_for(&text),
            text,
            evidence: Vec::new(),
        }
    }

    /// The id a claim with this text would have.
    pub fn id_for(text: &str) -> ClaimId {
        ClaimId::new(ContentHash::of_parts(&[normalise(text).as_bytes()]))
    }

    pub fn id(&self) -> &ClaimId {
        &self.id
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn evidence(&self) -> &[Evidence] {
        &self.evidence
    }

    /// Adds evidence, ignoring exact duplicates so repeated merges are
    /// idempotent.
    pub fn add_evidence(&mut self, evidence: Evidence) {
        if !self.evidence.contains(&evidence) {
            self.evidence.push(evidence);
            self.evidence.sort();
        }
    }
}

fn normalise(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_mille_scores_stay_in_range() {
        assert_eq!(PerMille::from_probability(0.9876).get(), 988);
        assert_eq!(PerMille::from_probability(1.5).get(), 1000);
        assert_eq!(PerMille::from_probability(-0.1).get(), 0);
        assert_eq!(PerMille::from_probability(f32::NAN).get(), 0);
        assert_eq!(PerMille::new(1001), Err(InvalidPerMille(1001)));

        let basis: EvidenceBasis = serde_json::from_value(serde_json::json!({
            "kind": "merged", "wording": "w", "entailment_pm": 1000
        }))
        .unwrap();
        assert_eq!(serde_json::to_value(&basis).unwrap()["entailment_pm"], 1000);
        assert!(
            serde_json::from_value::<EvidenceBasis>(serde_json::json!({
                "kind": "merged", "wording": "w", "entailment_pm": 1001
            }))
            .is_err()
        );
    }

    #[test]
    fn case_and_whitespace_do_not_change_identity() {
        assert_eq!(
            Claim::new("The Sky  is blue").id(),
            Claim::new(" the sky is\tblue ").id()
        );
        assert_ne!(
            Claim::new("The sky is blue").id(),
            Claim::new("The sky is green").id()
        );
    }
}
