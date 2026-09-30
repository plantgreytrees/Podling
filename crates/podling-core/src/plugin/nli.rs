//! Natural-language-inference providers: does a premise entail a hypothesis,
//! contradict it, or neither?
//!
//! This is what turns "two sources say the same thing in different words"
//! and "a source says the opposite" into evidence. The ledger's status still
//! comes only from that evidence, through `classify`: the NLI model supplies
//! scores, and fixed thresholds in the stages turn scores into stances.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::error::{CoreError, Result};
use crate::text::{content_words, is_number};

/// One pair to judge: does `premise` entail `hypothesis`?
///
/// The `'a` is a *lifetime parameter*: the pair borrows its two strings
/// instead of owning copies, and the compiler checks that the texts it points
/// into (the chunks and claims a stage holds) outlive the pair. A stage can
/// build thousands of pairs over the same few texts without copying any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NliPair<'a> {
    pub premise: &'a str,
    pub hypothesis: &'a str,
}

/// Class probabilities for one pair; they sum to about 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NliScores {
    pub entailment: f32,
    pub neutral: f32,
    pub contradiction: f32,
}

/// A plugin that judges premise/hypothesis pairs. Used as a trait object,
/// like [`EmbeddingProvider`](super::EmbeddingProvider).
pub trait NliProvider {
    fn id(&self) -> &str;

    /// Everything that can change the scores (model weights, version). Part
    /// of every cache key that depends on this provider.
    fn fingerprint(&self) -> Value;

    /// Scores for each pair, in the same order.
    fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>>;
}

/// Calls `provider` and checks one finite result per pair; anything else is
/// an [`CoreError::InvalidProviderOutput`] of `stage`. No pairs, no call.
pub fn score_checked(
    provider: &dyn NliProvider,
    stage: &'static str,
    pairs: &[NliPair<'_>],
) -> Result<Vec<NliScores>> {
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    let scores = provider.score(pairs)?;
    let invalid = |message: String| CoreError::InvalidProviderOutput { stage, message };
    if scores.len() != pairs.len() {
        return Err(invalid(format!(
            "NLI provider {} returned {} results for {} pairs",
            provider.id(),
            scores.len(),
            pairs.len()
        )));
    }
    if scores.iter().any(|s| {
        ![s.entailment, s.neutral, s.contradiction]
            .iter()
            .all(|x| x.is_finite())
    }) {
        return Err(invalid(format!(
            "NLI provider {} returned a non-finite score",
            provider.id()
        )));
    }
    Ok(scores)
}

/// Share of the non-number words two texts must have in common before the
/// fake calls a differing number a contradiction (rather than a different
/// subject).
const FAKE_SAME_SUBJECT: f32 = 0.6;

/// A deterministic, offline stand-in for an NLI model, working on content
/// words (see [`content_words`]):
/// - **contradiction** (1.0) when the hypothesis has a number the premise
///   lacks, the premise has a number of its own, and at least 60% of the
///   hypothesis's other words are in the premise: "happened in 1907" against
///   "happened in 1908";
/// - otherwise **entailment** is the share of the hypothesis's content words
///   found in the premise, and the rest is neutral.
///
/// Containment is one-way on purpose, like real entailment: "trees fell in
/// the forest" entails "trees fell", not the reverse. That keeps the
/// merge stage's both-directions check meaningful in tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeNli;

impl FakeNli {
    fn judge(pair: NliPair<'_>) -> NliScores {
        let premise = content_words(pair.premise);
        let hypothesis = content_words(pair.hypothesis);
        if hypothesis.is_empty() {
            return NliScores {
                entailment: 0.0,
                neutral: 1.0,
                contradiction: 0.0,
            };
        }
        let (numbers, words): (BTreeSet<&String>, BTreeSet<&String>) =
            hypothesis.iter().partition(|w| is_number(w));
        let premise_has_numbers = premise.iter().any(|w| is_number(w));
        let changed_number = premise_has_numbers && numbers.iter().any(|n| !premise.contains(*n));
        let same_subject = words.is_empty()
            || words.iter().filter(|w| premise.contains(**w)).count() as f32
                >= FAKE_SAME_SUBJECT * words.len() as f32;
        if changed_number && same_subject {
            return NliScores {
                entailment: 0.0,
                neutral: 0.0,
                contradiction: 1.0,
            };
        }
        let entailment = hypothesis.iter().filter(|w| premise.contains(*w)).count() as f32
            / hypothesis.len() as f32;
        NliScores {
            entailment,
            neutral: 1.0 - entailment,
            contradiction: 0.0,
        }
    }
}

impl NliProvider for FakeNli {
    fn id(&self) -> &str {
        "fake"
    }

    fn fingerprint(&self) -> Value {
        // Bump when the fake's behaviour changes.
        json!({ "provider": "fake", "version": 1 })
    }

    fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>> {
        Ok(pairs.iter().map(|p| Self::judge(*p)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judge(premise: &str, hypothesis: &str) -> NliScores {
        FakeNli
            .score(&[NliPair {
                premise,
                hypothesis,
            }])
            .unwrap()[0]
    }

    #[test]
    fn fake_entailment_is_one_way_containment() {
        let long = "Trees fell across the whole Tunguska forest.";
        let short = "Trees fell across the forest.";
        assert_eq!(judge(long, short).entailment, 1.0);
        assert!(judge(short, long).entailment < 0.8);
    }

    #[test]
    fn fake_calls_a_changed_number_a_contradiction() {
        let s = judge(
            "The explosion happened in June 1908.",
            "The explosion happened in June 1907.",
        );
        assert_eq!((s.contradiction, s.entailment), (1.0, 0.0));
        // A different subject with a different number is merely unrelated.
        let s = judge(
            "Kulik reached the site in 1927.",
            "The explosion happened in 1908.",
        );
        assert_eq!(s.contradiction, 0.0);
        // A premise without numbers can't contradict one.
        let s = judge(
            "The explosion happened in June.",
            "The explosion happened in June 1907.",
        );
        assert_eq!(s.contradiction, 0.0);
    }

    #[test]
    fn fake_scores_a_reordered_paraphrase_as_entailed_both_ways() {
        let a = "In June 1908 an explosion flattened about 80 million trees.";
        let b = "About 80 million trees were flattened by an explosion in June 1908.";
        assert_eq!(judge(a, b).entailment, 1.0);
        assert_eq!(judge(b, a).entailment, 1.0);
    }

    struct Short;
    impl NliProvider for Short {
        fn id(&self) -> &str {
            "short"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn score(&self, _: &[NliPair<'_>]) -> Result<Vec<NliScores>> {
            Ok(vec![])
        }
    }

    #[test]
    fn score_checked_rejects_a_missing_result() {
        let pair = NliPair {
            premise: "a",
            hypothesis: "b",
        };
        let err = score_checked(&Short, "s", &[pair]).unwrap_err();
        assert!(matches!(
            err,
            CoreError::InvalidProviderOutput { stage: "s", .. }
        ));
        assert!(score_checked(&Short, "s", &[]).unwrap().is_empty());
    }
}
