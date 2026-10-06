//! The adjudicator's verdicts on Contested claims.
//!
//! A verdict never changes a claim's status, which stays whatever `classify`
//! says. It records which side the sources favour, why, and which pieces of
//! the claim's evidence it rests on. Evidence is cited by reference, never
//! quoted: the explanation may hold no quotation marks at all.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::claim::Stance;
use crate::document::TextSpan;
use crate::ids::{ChunkId, ClaimId};

/// Longest explanation, in characters: a sentence or two, not an essay.
pub const MAX_EXPLANATION_CHARS: usize = 600;

/// The characters an explanation may not contain, so it can't quote a source.
const QUOTE_MARKS: [char; 3] = ['"', '\u{201C}', '\u{201D}'];

/// Which side of a Contested claim the sources favour.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Favours {
    /// The evidence that supports the claim is the stronger.
    Supporting,
    /// The evidence that contradicts the claim is the stronger.
    Contradicting,
    /// The sources don't settle it, or the adjudicator gave no usable answer.
    Unresolved,
}

/// One piece of a claim's evidence, named by what identifies it: its chunk,
/// its stance and, for NLI evidence, the premise span it was judged on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceRef {
    pub chunk: ChunkId,
    pub stance: Stance,
    /// The `EvidenceBasis::Nli` premise span, in the chunk's document. Absent
    /// for evidence without one (extracted from the chunk, or merged).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub premise: Option<TextSpan>,
}

/// Why a verdict was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidVerdict {
    #[error("the explanation is empty")]
    EmptyExplanation,
    #[error("the explanation is {0} characters; the limit is {MAX_EXPLANATION_CHARS}")]
    LongExplanation(usize),
    #[error("the explanation contains a quotation mark; refer to sources, never quote them")]
    QuotedExplanation,
    #[error("a verdict must cite at least one piece of evidence")]
    NoCitations,
    #[error("a verdict favouring the {0:?} side must cite evidence from that side")]
    UncitedSide(Favours),
    #[error("a fallback verdict must be unresolved")]
    DecidedFallback,
    #[error("claim {0} has more than one verdict")]
    DuplicateClaim(String),
}

/// The adjudicator's reading of one Contested claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawVerdict")]
pub struct Verdict {
    claim: ClaimId,
    favours: Favours,
    explanation: String,
    cites: Vec<EvidenceRef>,
    /// Set when the adjudicator's replies were rejected and this verdict was
    /// written in its place: the last rejection reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fallback: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct RawVerdict {
    claim: ClaimId,
    favours: Favours,
    explanation: String,
    cites: Vec<EvidenceRef>,
    #[serde(default)]
    fallback: Option<String>,
}

impl TryFrom<RawVerdict> for Verdict {
    type Error = InvalidVerdict;

    fn try_from(raw: RawVerdict) -> Result<Self, Self::Error> {
        Self::new(
            raw.claim,
            raw.favours,
            raw.explanation,
            raw.cites,
            raw.fallback,
        )
    }
}

impl Verdict {
    /// Checks the verdict's invariants; `cites` is sorted and de-duplicated.
    pub fn new(
        claim: ClaimId,
        favours: Favours,
        explanation: impl Into<String>,
        mut cites: Vec<EvidenceRef>,
        fallback: Option<String>,
    ) -> Result<Self, InvalidVerdict> {
        let explanation = explanation.into();
        if explanation.trim().is_empty() {
            return Err(InvalidVerdict::EmptyExplanation);
        }
        let chars = explanation.chars().count();
        if chars > MAX_EXPLANATION_CHARS {
            return Err(InvalidVerdict::LongExplanation(chars));
        }
        if explanation.contains(QUOTE_MARKS) {
            return Err(InvalidVerdict::QuotedExplanation);
        }
        cites.sort();
        cites.dedup();
        if cites.is_empty() {
            return Err(InvalidVerdict::NoCitations);
        }
        let needed = match favours {
            Favours::Supporting => Some(Stance::Supports),
            Favours::Contradicting => Some(Stance::Contradicts),
            Favours::Unresolved => None,
        };
        if let Some(stance) = needed
            && !cites.iter().any(|c| c.stance == stance)
        {
            return Err(InvalidVerdict::UncitedSide(favours));
        }
        if fallback.is_some() && favours != Favours::Unresolved {
            return Err(InvalidVerdict::DecidedFallback);
        }
        Ok(Self {
            claim,
            favours,
            explanation,
            cites,
            fallback,
        })
    }

    pub fn claim(&self) -> &ClaimId {
        &self.claim
    }

    pub fn favours(&self) -> Favours {
        self.favours
    }

    pub fn explanation(&self) -> &str {
        &self.explanation
    }

    pub fn cites(&self) -> &[EvidenceRef] {
        &self.cites
    }

    pub fn fallback(&self) -> Option<&str> {
        self.fallback.as_deref()
    }
}

/// Every verdict of a run, at most one per claim, ordered by claim id. Written
/// as a bare JSON array.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "Vec<Verdict>", into = "Vec<Verdict>")]
pub struct Verdicts(Vec<Verdict>);

impl Verdicts {
    pub fn new(mut verdicts: Vec<Verdict>) -> Result<Self, InvalidVerdict> {
        verdicts.sort_by(|a, b| a.claim.cmp(&b.claim));
        if let Some(pair) = verdicts.windows(2).find(|w| w[0].claim == w[1].claim) {
            return Err(InvalidVerdict::DuplicateClaim(pair[0].claim.to_string()));
        }
        Ok(Self(verdicts))
    }

    pub fn as_slice(&self) -> &[Verdict] {
        &self.0
    }

    /// The verdict on `claim`, if it has one.
    pub fn get(&self, claim: &ClaimId) -> Option<&Verdict> {
        self.0
            .binary_search_by(|v| v.claim.cmp(claim))
            .ok()
            .map(|i| &self.0[i])
    }
}

impl TryFrom<Vec<Verdict>> for Verdicts {
    type Error = InvalidVerdict;

    fn try_from(verdicts: Vec<Verdict>) -> Result<Self, Self::Error> {
        Self::new(verdicts)
    }
}

impl From<Verdicts> for Vec<Verdict> {
    fn from(verdicts: Verdicts) -> Self {
        verdicts.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::Claim;
    use crate::ids::ContentHash;

    fn chunk(name: &str) -> ChunkId {
        ChunkId::new(ContentHash::of_parts(&[name.as_bytes()]))
    }

    fn cite(name: &str, stance: Stance) -> EvidenceRef {
        EvidenceRef {
            chunk: chunk(name),
            stance,
            premise: None,
        }
    }

    fn both_sides() -> Vec<EvidenceRef> {
        vec![
            cite("b", Stance::Contradicts),
            cite("a", Stance::Supports),
            cite("a", Stance::Supports),
        ]
    }

    fn verdict(
        favours: Favours,
        explanation: &str,
        cites: Vec<EvidenceRef>,
    ) -> Result<Verdict, InvalidVerdict> {
        Verdict::new(Claim::id_for("x"), favours, explanation, cites, None)
    }

    #[test]
    fn a_valid_verdict_sorts_its_citations_and_round_trips() {
        let v = verdict(Favours::Supporting, "The survey is later.", both_sides()).unwrap();
        assert_eq!(v.cites().len(), 2, "duplicates removed");
        assert!(v.cites().windows(2).all(|w| w[0] < w[1]));
        let json = serde_json::to_value(&v).unwrap();
        assert!(json.get("fallback").is_none());
        assert_eq!(serde_json::from_value::<Verdict>(json).unwrap(), v);
    }

    #[test]
    fn invalid_verdicts_are_rejected() {
        use InvalidVerdict::*;
        let cases = [
            (
                verdict(Favours::Unresolved, "  ", both_sides()),
                EmptyExplanation,
            ),
            (
                verdict(Favours::Unresolved, &"x".repeat(601), both_sides()),
                LongExplanation(601),
            ),
            (
                verdict(
                    Favours::Unresolved,
                    "It said \u{201C}no\u{201D}.",
                    both_sides(),
                ),
                QuotedExplanation,
            ),
            (verdict(Favours::Unresolved, "Why.", vec![]), NoCitations),
            (
                verdict(
                    Favours::Contradicting,
                    "Why.",
                    vec![cite("a", Stance::Supports)],
                ),
                UncitedSide(Favours::Contradicting),
            ),
            (
                verdict(
                    Favours::Supporting,
                    "Why.",
                    vec![cite("b", Stance::Contradicts)],
                ),
                UncitedSide(Favours::Supporting),
            ),
            (
                Verdict::new(
                    Claim::id_for("x"),
                    Favours::Supporting,
                    "Why.",
                    both_sides(),
                    Some("rejected".into()),
                ),
                DecidedFallback,
            ),
        ];
        for (got, want) in cases {
            assert_eq!(got.unwrap_err(), want);
        }
    }

    #[test]
    fn deserialisation_runs_the_same_checks() {
        let v = verdict(Favours::Unresolved, "Why.", both_sides()).unwrap();
        let mut json = serde_json::to_value(&v).unwrap();
        json["explanation"] = "He said \"so\".".into();
        assert!(serde_json::from_value::<Verdict>(json).is_err());
    }

    #[test]
    fn verdicts_are_one_per_claim_and_ordered() {
        let make = |text: &str| {
            Verdict::new(
                Claim::id_for(text),
                Favours::Unresolved,
                "Why.",
                both_sides(),
                None,
            )
            .unwrap()
        };
        let verdicts = Verdicts::new(vec![make("b"), make("a")]).unwrap();
        let ids: Vec<_> = verdicts.as_slice().iter().map(Verdict::claim).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
        assert!(verdicts.get(&Claim::id_for("a")).is_some());
        assert!(verdicts.get(&Claim::id_for("c")).is_none());

        assert!(matches!(
            Verdicts::new(vec![make("a"), make("a")]),
            Err(InvalidVerdict::DuplicateClaim(_))
        ));
        assert_eq!(serde_json::to_string(&Verdicts::default()).unwrap(), "[]");
    }
}
