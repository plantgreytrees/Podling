//! Atomic claims extracted from sources, with the evidence behind them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
