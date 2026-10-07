//! Measures a written script against its ledger: how long it is for its
//! target, how much of the ledger it cites, and whether it cites anything the
//! ledger doesn't hold.
//!
//! Pure functions only (no I/O, no provider), so the script stage can log a
//! script's word ratio and the live evaluation harness can compare prompts
//! with the same numbers.

use std::collections::BTreeSet;

use podling_types::{ClaimId, ClaimStatus, Ledger, Script, Verdicts};

/// Spoken words per minute the script prompt asks for.
pub const WORDS_PER_MINUTE: usize = 150;

/// What one script delivers, measured against the ledger it was written from.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptMetrics {
    /// Whitespace-separated words in every turn's text, quotes included.
    pub words: usize,
    /// `words / (WORDS_PER_MINUTE × target_minutes)`; 0.0 for a zero target.
    pub word_ratio: f64,
    pub turns: usize,
    pub quotes: usize,
    /// Citations over all turns, repeats counted.
    pub citations: usize,
    /// Distinct ledger claims cited.
    pub distinct_cited: usize,
    /// Ledger claims the script may use: every status but Unsupported.
    pub usable_claims: usize,
    /// `distinct_cited / usable_claims`; 0.0 when nothing is usable.
    pub coverage: f64,
    /// Ledger claims with an adjudicator's verdict.
    pub judged_contested: usize,
    /// Of those, the ones some turn cites.
    pub judged_contested_cited: usize,
    /// Citations (repeats counted) naming a claim the ledger doesn't hold.
    pub unknown_citations: usize,
}

impl ScriptMetrics {
    pub fn of(script: &Script, ledger: &Ledger, verdicts: &Verdicts, target_minutes: u16) -> Self {
        let turns = script.turns();
        let in_ledger: BTreeSet<&ClaimId> = ledger.entries().iter().map(|e| e.claim.id()).collect();
        let cited = cited_claims(script);

        let words = turns
            .iter()
            .map(|t| t.text.split_whitespace().count())
            .sum();
        let usable_claims = ledger
            .entries()
            .iter()
            .filter(|e| e.status != ClaimStatus::Unsupported)
            .count();
        let distinct_cited = cited.iter().filter(|id| in_ledger.contains(id)).count();
        let judged: Vec<&ClaimId> = verdicts
            .as_slice()
            .iter()
            .map(|v| v.claim())
            .filter(|id| in_ledger.contains(id))
            .collect();

        Self {
            words,
            word_ratio: ratio(words, WORDS_PER_MINUTE * usize::from(target_minutes)),
            turns: turns.len(),
            quotes: turns.iter().map(|t| t.quotes.len()).sum(),
            citations: turns.iter().map(|t| t.citations.len()).sum(),
            distinct_cited,
            usable_claims,
            coverage: ratio(distinct_cited, usable_claims),
            judged_contested: judged.len(),
            judged_contested_cited: judged.iter().filter(|id| cited.contains(**id)).count(),
            unknown_citations: turns
                .iter()
                .flat_map(|t| &t.citations)
                .filter(|id| !in_ledger.contains(id))
                .count(),
        }
    }
}

/// Every claim id some turn cites, once each.
pub fn cited_claims(script: &Script) -> BTreeSet<ClaimId> {
    script
        .turns()
        .iter()
        .flat_map(|t| t.citations.iter().cloned())
        .collect()
}

/// How far two scripts' cited claims agree: for comparing two `topic`
/// angles on one ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct Overlap {
    pub shared: usize,
    pub only_a: usize,
    pub only_b: usize,
    /// `shared / (shared + only_a + only_b)`; 1.0 for two empty sets, which
    /// are identical.
    pub jaccard: f64,
}

pub fn cited_claim_overlap(a: &BTreeSet<ClaimId>, b: &BTreeSet<ClaimId>) -> Overlap {
    let shared = a.intersection(b).count();
    let only_a = a.len() - shared;
    let only_b = b.len() - shared;
    let union = shared + only_a + only_b;
    Overlap {
        shared,
        only_a,
        only_b,
        jaccard: if union == 0 {
            1.0
        } else {
            ratio(shared, union)
        },
    }
}

/// `part / whole`, or 0.0 when `whole` is 0.
fn ratio(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use podling_types::{
        Chunk, Claim, Document, Emotion, Evidence, EvidenceRef, Favours, Pace, Quote, SourceRef,
        Speaker, SpeakerId, Stance, TextSpan, Turn, Verdict,
    };

    fn doc_and_chunk() -> (Document, Chunk) {
        let source = SourceRef {
            connector: "t".into(),
            locator: "a".into(),
            independence_group: "g".into(),
        };
        let doc = Document::new(source, "A", "The sky split in two.");
        let chunk = Chunk::from_document(&doc, TextSpan::new(0, doc.text().len()).unwrap(), vec![])
            .unwrap();
        (doc, chunk)
    }

    /// A claim with one supporting piece of evidence: SingleSource.
    fn supported(text: &str) -> Claim {
        let (doc, chunk) = doc_and_chunk();
        let mut claim = Claim::new(text);
        claim.add_evidence(Evidence {
            chunk: chunk.id().clone(),
            source: doc.source().id(),
            independence_group: doc.source().independence_group.clone(),
            stance: Stance::Supports,
            basis: None,
        });
        claim
    }

    fn turn(text: &str, citations: &[&ClaimId]) -> Turn {
        Turn {
            speaker: SpeakerId("host".into()),
            text: text.into(),
            emotion: Emotion::Neutral,
            citations: citations.iter().map(|id| (*id).clone()).collect(),
            quotes: Vec::new(),
            pace: Pace::Normal,
            nonverbal: Vec::new(),
            callback_to: None,
        }
    }

    fn script(turns: Vec<Turn>) -> Script {
        let host = Speaker {
            id: SpeakerId("host".into()),
            name: "Ada".into(),
            role: "host".into(),
        };
        Script::new(vec![host], turns).unwrap()
    }

    fn verdict_on(claim: &ClaimId) -> Verdicts {
        let (_, chunk) = doc_and_chunk();
        let cite = EvidenceRef {
            chunk: chunk.id().clone(),
            stance: Stance::Supports,
            premise: None,
        };
        let verdict = Verdict::new(
            claim.clone(),
            Favours::Unresolved,
            "They differ.",
            vec![cite],
            None,
        )
        .unwrap();
        Verdicts::new(vec![verdict]).unwrap()
    }

    #[test]
    fn counts_words_against_150_a_minute() {
        let words = vec!["word"; 300].join(" ");
        let s = script(vec![turn(&words, &[]), turn("and  three\nmore", &[])]);
        let m = ScriptMetrics::of(&s, &Ledger::default(), &Verdicts::default(), 2);
        assert_eq!(m.words, 303);
        assert!((m.word_ratio - 1.01).abs() < 1e-9, "{}", m.word_ratio);
        assert_eq!(m.turns, 2);
    }

    #[test]
    fn a_zero_minute_target_gives_a_zero_ratio() {
        let s = script(vec![turn("Hello there.", &[])]);
        let m = ScriptMetrics::of(&s, &Ledger::default(), &Verdicts::default(), 0);
        assert_eq!(m.word_ratio, 0.0);
        assert_eq!(m.coverage, 0.0);
    }

    #[test]
    fn repeated_citations_count_once_toward_coverage() {
        let a = supported("The sky split in two.");
        let a_id = a.id().clone();
        let ledger = Ledger::from_claims([a, supported("Trees fell.")]);
        let s = script(vec![
            turn("One.", &[&a_id]),
            turn("Two.", &[&a_id]),
            turn("Three.", &[&a_id]),
        ]);
        let m = ScriptMetrics::of(&s, &ledger, &Verdicts::default(), 5);
        assert_eq!(m.citations, 3);
        assert_eq!(m.distinct_cited, 1);
        assert_eq!(m.usable_claims, 2);
        assert_eq!(m.coverage, 0.5);
    }

    #[test]
    fn unsupported_claims_are_not_usable() {
        let used = supported("The sky split in two.");
        let used_id = used.id().clone();
        let ledger = Ledger::from_claims([used, Claim::new("Nobody saw it.")]);
        let s = script(vec![turn("It split.", &[&used_id])]);
        let m = ScriptMetrics::of(&s, &ledger, &Verdicts::default(), 5);
        assert_eq!(m.usable_claims, 1);
        assert_eq!(m.coverage, 1.0);
    }

    #[test]
    fn counts_quotes() {
        let (doc, _) = doc_and_chunk();
        let quote = Quote::from_document(&doc, TextSpan::new(0, 21).unwrap()).unwrap();
        let mut quoting = turn("\"The sky split in two.\" he said.", &[]);
        quoting.quotes.push(quote);
        let s = script(vec![quoting, turn("No quote.", &[])]);
        let m = ScriptMetrics::of(&s, &Ledger::default(), &Verdicts::default(), 5);
        assert_eq!(m.quotes, 1);
    }

    #[test]
    fn a_judged_claim_counts_as_cited_only_when_a_turn_cites_it() {
        let judged = supported("The blast was heard far away.");
        let judged_id = judged.id().clone();
        let other = supported("The sky split in two.");
        let other_id = other.id().clone();
        let ledger = Ledger::from_claims([judged, other]);
        let verdicts = verdict_on(&judged_id);

        let silent = script(vec![turn("The sky split.", &[&other_id])]);
        let m = ScriptMetrics::of(&silent, &ledger, &verdicts, 5);
        assert_eq!((m.judged_contested, m.judged_contested_cited), (1, 0));

        let cited = script(vec![turn("They disagree.", &[&judged_id])]);
        let m = ScriptMetrics::of(&cited, &ledger, &verdicts, 5);
        assert_eq!((m.judged_contested, m.judged_contested_cited), (1, 1));
    }

    #[test]
    fn counts_citations_the_ledger_does_not_hold() {
        let known = supported("The sky split in two.");
        let known_id = known.id().clone();
        let stranger = Claim::id_for("Not in the ledger.");
        let ledger = Ledger::from_claims([known]);
        let s = script(vec![
            turn("One.", &[&known_id, &stranger]),
            turn("Two.", &[&stranger]),
        ]);
        let m = ScriptMetrics::of(&s, &ledger, &Verdicts::default(), 5);
        assert_eq!(m.unknown_citations, 2);
        assert_eq!(m.distinct_cited, 1);
    }

    #[test]
    fn overlap_of_identical_disjoint_partial_and_empty_sets() {
        let set =
            |ts: &[&str]| -> BTreeSet<ClaimId> { ts.iter().map(|t| Claim::id_for(t)).collect() };

        let same = cited_claim_overlap(&set(&["a", "b"]), &set(&["a", "b"]));
        assert_eq!((same.shared, same.only_a, same.only_b), (2, 0, 0));
        assert_eq!(same.jaccard, 1.0);

        let apart = cited_claim_overlap(&set(&["a"]), &set(&["b"]));
        assert_eq!((apart.shared, apart.only_a, apart.only_b), (0, 1, 1));
        assert_eq!(apart.jaccard, 0.0);

        let partial = cited_claim_overlap(&set(&["a", "b", "c"]), &set(&["b", "c", "d"]));
        assert_eq!((partial.shared, partial.only_a, partial.only_b), (2, 1, 1));
        assert_eq!(partial.jaccard, 0.5);

        let empty = cited_claim_overlap(&set(&[]), &set(&[]));
        assert_eq!(empty.jaccard, 1.0);
    }

    #[test]
    fn cited_claims_lists_each_claim_once() {
        let a = Claim::id_for("a");
        let s = script(vec![turn("x", &[&a]), turn("y", &[&a])]);
        assert_eq!(cited_claims(&s).len(), 1);
    }
}
