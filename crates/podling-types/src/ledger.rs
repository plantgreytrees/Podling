//! The claim ledger: every claim with a deterministic trust status.
//!
//! Status comes from counting *independence groups*, not documents, so five
//! articles copied from one wire report count as one source.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::claim::{Claim, Stance};

/// Group names are sorted and de-duplicated in every variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ClaimStatus {
    /// Supported by at least two independent groups; nobody contradicts it.
    Corroborated { groups: Vec<String> },
    /// Supported by exactly one independent group; nobody contradicts it.
    SingleSource { group: String },
    /// At least one group contradicts it. `supporting` may be empty.
    Contested {
        supporting: Vec<String>,
        contradicting: Vec<String>,
    },
    /// No evidence either way.
    Unsupported,
}

/// Classifies a claim from its evidence. Pure: same claim, same status.
pub fn classify(claim: &Claim) -> ClaimStatus {
    let groups_with = |stance: Stance| -> Vec<String> {
        let set: BTreeSet<&str> = claim
            .evidence()
            .iter()
            .filter(|e| e.stance == stance)
            .map(|e| e.independence_group.as_str())
            .collect();
        set.into_iter().map(str::to_owned).collect()
    };
    let supporting = groups_with(Stance::Supports);
    let contradicting = groups_with(Stance::Contradicts);

    if !contradicting.is_empty() {
        return ClaimStatus::Contested {
            supporting,
            contradicting,
        };
    }
    match supporting.len() {
        0 => ClaimStatus::Unsupported,
        1 => ClaimStatus::SingleSource {
            group: supporting.into_iter().next().unwrap_or_default(),
        },
        _ => ClaimStatus::Corroborated { groups: supporting },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LedgerEntry {
    pub claim: Claim,
    pub status: ClaimStatus,
}

/// Every entry's status is `classify(&entry.claim)`; deserialisation rejects a
/// ledger whose stored status disagrees with its evidence.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawLedger")]
pub struct Ledger {
    entries: Vec<LedgerEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("ledger status for claim {claim} does not match its evidence")]
pub struct StatusMismatch {
    pub claim: String,
}

#[derive(Deserialize, JsonSchema)]
struct RawLedger {
    entries: Vec<LedgerEntry>,
}

impl TryFrom<RawLedger> for Ledger {
    type Error = StatusMismatch;

    fn try_from(raw: RawLedger) -> Result<Self, Self::Error> {
        if let Some(bad) = raw.entries.iter().find(|e| classify(&e.claim) != e.status) {
            return Err(StatusMismatch {
                claim: bad.claim.id().to_string(),
            });
        }
        Ok(Self::from_claims(raw.entries.into_iter().map(|e| e.claim)))
    }
}

impl Ledger {
    /// Classifies every claim; entries are ordered by claim id so the ledger is
    /// deterministic regardless of input order.
    pub fn from_claims(claims: impl IntoIterator<Item = Claim>) -> Self {
        let mut entries: Vec<LedgerEntry> = claims
            .into_iter()
            .map(|claim| {
                let status = classify(&claim);
                LedgerEntry { claim, status }
            })
            .collect();
        entries.sort_by(|a, b| a.claim.id().cmp(b.claim.id()));
        Self { entries }
    }

    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::Evidence;
    use crate::ids::{ChunkId, ContentHash, SourceId};

    fn evidence(group: &str, doc: &str, stance: Stance) -> Evidence {
        Evidence {
            chunk: ChunkId::new(ContentHash::of_parts(&[doc.as_bytes()])),
            source: SourceId::new(ContentHash::of_parts(&[doc.as_bytes()])),
            independence_group: group.into(),
            stance,
        }
    }

    fn claim_with(evidence: &[Evidence]) -> Claim {
        let mut claim = Claim::new("The meteor exploded in 1908.");
        for e in evidence {
            claim.add_evidence(e.clone());
        }
        claim
    }

    #[test]
    fn classifies_every_status() {
        use Stance::{Contradicts, Supports};
        let cases = [
            (vec![], ClaimStatus::Unsupported),
            (
                vec![evidence("wire", "a", Supports)],
                ClaimStatus::SingleSource {
                    group: "wire".into(),
                },
            ),
            (
                // Two documents, same group: still one independent source.
                vec![
                    evidence("wire", "a", Supports),
                    evidence("wire", "b", Supports),
                ],
                ClaimStatus::SingleSource {
                    group: "wire".into(),
                },
            ),
            (
                vec![
                    evidence("wire", "a", Supports),
                    evidence("archive", "b", Supports),
                ],
                ClaimStatus::Corroborated {
                    groups: vec!["archive".into(), "wire".into()],
                },
            ),
            (
                vec![
                    evidence("wire", "a", Supports),
                    evidence("archive", "b", Contradicts),
                ],
                ClaimStatus::Contested {
                    supporting: vec!["wire".into()],
                    contradicting: vec!["archive".into()],
                },
            ),
        ];
        for (evidence, expected) in cases {
            assert_eq!(classify(&claim_with(&evidence)), expected);
        }
    }

    #[test]
    fn deserialisation_rejects_tampered_status_and_ids() {
        let claim = claim_with(&[evidence("wire", "a", Stance::Supports)]);
        let mut json = serde_json::to_value(Ledger::from_claims([claim])).unwrap();
        assert!(serde_json::from_value::<Ledger>(json.clone()).is_ok());

        let mut bad_status = json.clone();
        bad_status["entries"][0]["status"] =
            serde_json::json!({"status": "corroborated", "groups": ["wire", "x"]});
        assert!(serde_json::from_value::<Ledger>(bad_status).is_err());

        json["entries"][0]["claim"]["text"] = "Something else entirely.".into();
        assert!(
            serde_json::from_value::<Ledger>(json).is_err(),
            "claim id no longer matches text"
        );
    }

    #[test]
    fn ledger_order_is_independent_of_input_order() {
        let a = Claim::new("alpha");
        let b = Claim::new("beta");
        assert_eq!(
            Ledger::from_claims([a.clone(), b.clone()]),
            Ledger::from_claims([b, a])
        );
    }
}
