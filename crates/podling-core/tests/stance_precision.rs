//! How precise is the stance rule? A labelled pair set
//! (`fixtures/stance_pairs/pairs.json`) is scored once by the real models and
//! the raw scores are committed (`scores.json`), so the report below runs on
//! every `cargo test` without the weights. A rule change is justified only by
//! the figures it prints.
//!
//! To score the pairs again (after changing the pair set, the NLI model or
//! the embedder):
//!
//!   PODLING_NLI_MODEL_DIR=<dir> PODLING_LIVE_EMBED_URL=http://localhost:11434/v1 \
//!   PODLING_LIVE_EMBED_MODEL=nomic-embed-text \
//!   cargo test -p podling-core --test stance_precision -- --ignored score_the_stance_pairs
//!
//! and print the report with
//!
//!   cargo test -p podling-core --test stance_precision -- --nocapture report

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use podling_core::plugin::{
    CrossEncoderNli, EmbeddingProvider, NliPair, NliProvider, OpenAiEmbeddings, cosine,
};
use podling_core::stage::Stage;
use podling_core::stages::score_stances::{
    ScoreStances, StanceEvidence, decide, reverse_hypotheses,
};
use podling_core::text::numbers;
use podling_types::{EmbeddingConfig, PerMille, Stance};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a human says the premise does to the claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Label {
    Supports,
    Contradicts,
    Neither,
}

impl Label {
    /// The stance the stage should emit for this pair.
    fn stance(self) -> Option<Stance> {
        match self {
            Label::Supports => Some(Stance::Supports),
            Label::Contradicts => Some(Stance::Contradicts),
            Label::Neither => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    id: String,
    label: Label,
    claim: String,
    premise: String,
    #[allow(dead_code)] // for the reader of the fixture
    note: String,
}

/// The models' raw scores for every pair, recorded by `score_the_stance_pairs`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scores {
    /// The NLI provider's fingerprint (model files hash and version).
    nli: Value,
    /// The embedding model's name.
    embedding: String,
    pairs: Vec<Scored>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scored {
    id: String,
    /// `text_hash` of the pair as scored, so an edited pair can't keep stale
    /// scores under its old id.
    text_hash: String,
    similarity_pm: PerMille,
    entailment_pm: PerMille,
    contradiction_pm: PerMille,
    /// The claim (as premise) against each of `reverse_hypotheses(premise)`
    /// (as hypotheses), the highest contradiction; absent unless both texts
    /// hold a number. See `StanceEvidence::reverse_contradiction`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reverse_contradiction_pm: Option<PerMille>,
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stance_pairs")
        .join(name)
}

fn pairs() -> Vec<Pair> {
    let text = std::fs::read_to_string(fixture("pairs.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// What the scores depend on: the claim and the premise.
fn text_hash(pair: &Pair) -> String {
    blake3::hash(format!("{}\n{}", pair.claim, pair.premise).as_bytes())
        .to_hex()
        .to_string()
}

fn scores() -> Scores {
    let text = std::fs::read_to_string(fixture("scores.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_pair_set_is_well_formed() {
    let pairs = pairs();
    let ids: BTreeSet<&str> = pairs.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids.len(), pairs.len(), "pair ids must be unique");
    assert!(pairs.len() >= 40, "{} pairs", pairs.len());
    for label in [Label::Supports, Label::Contradicts, Label::Neither] {
        let n = pairs.iter().filter(|p| p.label == label).count();
        assert!(n >= 10, "{label:?}: only {n} pairs");
    }
    for p in &pairs {
        assert!(
            !p.claim.trim().is_empty() && !p.premise.trim().is_empty(),
            "{}",
            p.id
        );
    }
    // The pair that motivated the similarity floor must stay in.
    assert!(
        pairs
            .iter()
            .any(|p| p.claim == "Kulik reached the site in 1927."
                && p.premise == "No impact crater was found."
                && p.label == Label::Neither)
    );
}

/// Counts for one stance: emitted and right, emitted and wrong, missed, with
/// the ids of the wrong and the missed pairs.
#[derive(Debug, Default)]
struct Counts {
    tp: usize,
    fp: usize,
    missed: usize,
    false_positives: Vec<String>,
    missed_ids: Vec<String>,
}

impl Counts {
    fn precision(&self) -> Option<f64> {
        let emitted = self.tp + self.fp;
        (emitted > 0).then(|| self.tp as f64 / emitted as f64)
    }

    fn recall(&self) -> Option<f64> {
        let expected = self.tp + self.missed;
        (expected > 0).then(|| self.tp as f64 / expected as f64)
    }
}

/// Per-stance counts of `rule` over the scored pair set.
struct Report {
    supports: Counts,
    contradicts: Counts,
}

impl Report {
    fn of(rule: impl Fn(&StanceEvidence<'_>) -> Option<Stance>) -> Report {
        let (pairs, scores) = (pairs(), scores());
        let pair_ids: Vec<&str> = pairs.iter().map(|p| p.id.as_str()).collect();
        let score_ids: Vec<&str> = scores.pairs.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            pair_ids, score_ids,
            "scores.json is stale: run score_the_stance_pairs"
        );

        let mut report = Report {
            supports: Counts::default(),
            contradicts: Counts::default(),
        };
        for (pair, scored) in pairs.iter().zip(&scores.pairs) {
            assert_eq!(
                scored.text_hash,
                text_hash(pair),
                "scores.json is stale for {}: run score_the_stance_pairs",
                pair.id
            );
            let got = rule(&evidence(pair, scored));
            let want = pair.label.stance();
            for (stance, counts) in [
                (Stance::Supports, &mut report.supports),
                (Stance::Contradicts, &mut report.contradicts),
            ] {
                match (got == Some(stance), want == Some(stance)) {
                    (true, true) => counts.tp += 1,
                    (true, false) => {
                        counts.fp += 1;
                        counts.false_positives.push(pair.id.clone());
                    }
                    (false, true) => {
                        counts.missed += 1;
                        counts.missed_ids.push(pair.id.clone());
                    }
                    (false, false) => {}
                }
            }
        }
        report
    }

    fn print(&self, title: &str) {
        let pct = |x: Option<f64>| x.map_or("n/a".to_owned(), |x| format!("{:.1}%", x * 100.0));
        eprintln!("{title}");
        eprintln!("  stance       TP  FP  FN  precision  recall  false positives / missed");
        for (name, c) in [
            ("supports", &self.supports),
            ("contradicts", &self.contradicts),
        ] {
            eprintln!(
                "  {name:<11} {:>3} {:>3} {:>3}  {:>9}  {:>6}  {} / {}",
                c.tp,
                c.fp,
                c.missed,
                pct(c.precision()),
                pct(c.recall()),
                c.false_positives.join(" "),
                c.missed_ids.join(" ")
            );
        }
    }
}

/// The VERSION 2 rule, kept so every run prints what the change bought.
fn decide_v2(evidence: &StanceEvidence<'_>) -> Option<Stance> {
    if evidence.entailment.get() >= 800 {
        Some(Stance::Supports)
    } else if evidence.contradiction.get() >= 950 && evidence.similarity.get() >= 600 {
        Some(Stance::Contradicts)
    } else {
        None
    }
}

#[test]
fn stance_precision_report() {
    let before = Report::of(decide_v2);
    let after = Report::of(decide);
    before.print("before: VERSION 2 rule on the labelled pair set");
    after.print(&format!(
        "after: VERSION {} rule (score_stances::decide) on the labelled pair set",
        <ScoreStances<'_> as Stage>::VERSION
    ));

    // The current rule's figures, pinned exactly: a rule change that moves
    // any of them, for better or worse, must update them here on purpose,
    // with the printed before/after as its justification. Against VERSION 2,
    // the two-way check trades c18 (a count the model reads as a subset, see
    // `holds_both_ways`) for n21 n33 n34 n37 t02. VERSION 8 reads the whole
    // window the other way too, which recovers t03 (the contradiction sits in
    // a sentence without a number) once the subject rule is gone.
    let pinned = |c: &Counts| {
        (
            c.tp,
            c.fp,
            c.missed,
            c.false_positives.join(" "),
            c.missed_ids.join(" "),
        )
    };
    let pin = |tp, fp, missed, fps: &str, misses: &str| {
        (tp, fp, missed, fps.to_owned(), misses.to_owned())
    };
    assert_eq!(
        pinned(&after.supports),
        pin(14, 0, 5, "", "s04 s05 s06 s12 s16")
    );
    assert_eq!(pinned(&after.contradicts), pin(27, 2, 1, "n35 n36", "c18"));

    // The off-subject windows the model over-calls get no stance at all.
    for (pair, scored) in pairs().iter().zip(&scores().pairs) {
        if !["n21", "n33"].contains(&pair.id.as_str()) {
            continue;
        }
        assert!(
            decide_v2(&evidence(pair, scored)) == Some(Stance::Contradicts),
            "{}: the model no longer over-calls it; pick another pair",
            pair.id
        );
        assert_eq!(decide(&evidence(pair, scored)), None, "{}", pair.id);
    }
}

fn evidence<'a>(pair: &'a Pair, scored: &Scored) -> StanceEvidence<'a> {
    StanceEvidence {
        claim: &pair.claim,
        premise: &pair.premise,
        similarity: scored.similarity_pm,
        entailment: scored.entailment_pm,
        contradiction: scored.contradiction_pm,
        reverse_contradiction: scored.reverse_contradiction_pm,
    }
}

/// Scores every pair with the real NLI model and embedder, the way the stage
/// does (premise = source window, hypothesis = claim, and for two numbered
/// texts the reverse: premise = claim, hypothesis = each of
/// `reverse_hypotheses(window)`), and rewrites `scores.json`.
#[test]
#[ignore = "needs the NLI model and an embedding server; see the module docs"]
fn score_the_stance_pairs() {
    let (Ok(dir), Ok(url), Ok(model)) = (
        std::env::var("PODLING_NLI_MODEL_DIR"),
        std::env::var("PODLING_LIVE_EMBED_URL"),
        std::env::var("PODLING_LIVE_EMBED_MODEL"),
    ) else {
        // Fail, don't skip: an ignored test that returns early reports "ok".
        panic!("set PODLING_NLI_MODEL_DIR, PODLING_LIVE_EMBED_URL and PODLING_LIVE_EMBED_MODEL");
    };
    let nli = CrossEncoderNli::new(&PathBuf::from(dir)).unwrap();
    let embedder = OpenAiEmbeddings::from_config(&EmbeddingConfig::OpenAiCompat {
        base_url: url,
        model: model.clone(),
        api_key_env: None,
        timeout_secs: None,
        unload_after: false,
    })
    .unwrap();

    let pairs = pairs();
    let nli_pairs: Vec<NliPair<'_>> = pairs
        .iter()
        .map(|p| NliPair {
            premise: &p.premise,
            hypothesis: &p.claim,
        })
        .collect();
    let nli_scores = nli.score(&nli_pairs).unwrap();
    let reverse: Vec<Option<PerMille>> = pairs
        .iter()
        .map(|p| {
            let hypotheses = reverse_hypotheses(&p.premise);
            if numbers(&p.claim).is_empty() || hypotheses.is_empty() {
                return None;
            }
            let reverse_pairs: Vec<NliPair<'_>> = hypotheses
                .into_iter()
                .map(|hypothesis| NliPair {
                    premise: &p.claim,
                    hypothesis,
                })
                .collect();
            nli.score(&reverse_pairs)
                .unwrap()
                .iter()
                .map(|s| PerMille::from_probability(s.contradiction))
                .max()
        })
        .collect();
    let texts: Vec<&str> = pairs
        .iter()
        .flat_map(|p| [p.claim.as_str(), p.premise.as_str()])
        .collect();
    let vectors = embedder.embed(&texts).unwrap();
    assert_eq!(nli_scores.len(), pairs.len());
    assert_eq!(vectors.len(), texts.len());

    let scored = pairs
        .iter()
        .zip(&nli_scores)
        .zip(vectors.chunks(2))
        .zip(reverse)
        .map(|(((p, s), v), reverse_contradiction_pm)| Scored {
            id: p.id.clone(),
            text_hash: text_hash(p),
            similarity_pm: PerMille::from_probability(cosine(&v[0], &v[1])),
            entailment_pm: PerMille::from_probability(s.entailment),
            contradiction_pm: PerMille::from_probability(s.contradiction),
            reverse_contradiction_pm,
        })
        .collect();
    let scores = Scores {
        nli: nli.fingerprint(),
        embedding: model,
        pairs: scored,
    };
    let json = serde_json::to_string_pretty(&scores).unwrap();
    std::fs::write(fixture("scores.json"), json + "\n").unwrap();
}
