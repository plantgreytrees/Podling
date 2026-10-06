//! Keeps a claim only where the NLI model finds that its own chunk entails it.
//!
//! Extraction's lexical check (`extract_claims::is_grounded`) catches a claim
//! the passage never states, but not a distortion built from the passage's
//! own words: "Kulik led the expedition" from "Kulik joined the expedition"
//! shares most of its words with the chunk. This stage asks the NLI model the
//! real question, "does the chunk say this?", for every piece of extraction
//! evidence, and drops the evidence where the answer is no. A claim left with
//! no evidence is dropped. Every rejection is kept in the output, so it can
//! be counted and inspected even when the stage's result comes from the cache.
//!
//! It runs only when `[embedding]` and `[nli]` are set; without them the
//! pipeline skips it and nothing else changes. Status stays deterministic:
//! fixed thresholds decide, and `classify` still turns evidence into status.
//!
//! The premise is not the whole chunk. A chunk at the chunker's 800-word cap
//! is about 1000–1600 DeBERTa tokens, over its 512-token limit, and even a
//! short chunk fails: DeBERTa gave a faithful paraphrase of one sentence of a
//! four-sentence chunk 0.000 entailment and 1.000 contradiction, against
//! 0.998 for the sentence alone. So each claim is judged against the best
//! of its chunk's short sentence windows (see [`super::windows`]), each
//! prefixed with the document title and the chunk's headings, because
//! extraction has the model name things ("the site" → "the Tunguska site")
//! that only a heading names.
//!
//! Source text reaches the NLI model only as premise data. It is never an
//! instruction to anything, so a source can't steer which claims survive
//! except by what it states.

use std::collections::{BTreeMap, BTreeSet};

use podling_types::{Chunk, ChunkId, Claim, ClaimId, DocumentId, PerMille, TextSpan};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::windows::{MAX_WINDOW_SENTENCES, MAX_WINDOW_WORDS, windows};
use crate::error::{CoreError, Result};
use crate::plugin::{
    EmbeddingProvider, NliPair, NliProvider, cosine, embed_checked, score_checked,
};
use crate::stage::Stage;

/// Windows of a claim's chunk scored per claim (the cost bound): the ones
/// most similar to the claim by embedding. A run makes at most
/// `GROUND_K × evidence` NLI calls, however long the chunks.
pub const GROUND_K: usize = 4;

/// Entailment at or above this, from the best window, keeps a piece of
/// evidence. The same value as `score_stances::SUPPORT_ENTAIL_PM` but its own
/// constant, so either can be tuned alone. On the Tunguska sources DeBERTa
/// gave faithful claims 0.971 or more and distortions 0.003 or less
/// (`docs/plans/nli-extraction-grounding.md`), so 800 sits in a wide gap.
pub const GROUND_ENTAIL_PM: u16 = 800;

#[derive(Debug, Clone, Serialize)]
pub struct GroundInput {
    /// Claims as extraction left them, each with its extraction evidence.
    pub claims: Vec<Claim>,
    pub chunks: Vec<Chunk>,
    /// The title of each chunk's document, the first part of every premise.
    pub titles: BTreeMap<DocumentId, String>,
}

/// The stage's output: the claims that kept at least one piece of evidence,
/// and every piece of evidence that was dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grounded {
    pub claims: Vec<Claim>,
    pub rejected: Vec<Rejection>,
}

impl Grounded {
    /// Claims dropped entirely: every piece of their evidence was rejected.
    pub fn dropped_claims(&self) -> usize {
        let kept: BTreeSet<&ClaimId> = self.claims.iter().map(Claim::id).collect();
        let rejected: BTreeSet<&ClaimId> = self.rejected.iter().map(|r| &r.claim).collect();
        rejected.difference(&kept).count()
    }
}

/// One piece of extraction evidence the NLI model did not find entailed: the
/// claim, the chunk it was extracted from, and the best score any window of
/// that chunk got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rejection {
    pub claim: ClaimId,
    pub text: String,
    pub chunk: ChunkId,
    pub entailment_pm: PerMille,
    /// The best window's span in the chunk's document; `None` when the chunk
    /// has no sentence to judge against.
    pub premise: Option<TextSpan>,
}

pub struct GroundClaims<'a> {
    pub embedder: &'a dyn EmbeddingProvider,
    pub nli: &'a dyn NliProvider,
}

/// One NLI call to make: a piece of evidence and a window of its chunk.
struct Candidate {
    claim: usize,
    evidence: usize,
    window: usize,
}

impl Stage for GroundClaims<'_> {
    const ID: &'static str = "ground_claims";
    const VERSION: u32 = 2;
    type Input = GroundInput;
    type Output = Grounded;

    fn config_fingerprint(&self) -> Value {
        json!({
            "embedding": self.embedder.fingerprint(),
            "nli": self.nli.fingerprint(),
            "ground_k": GROUND_K,
            "ground_entail_pm": GROUND_ENTAIL_PM,
            "max_window_sentences": MAX_WINDOW_SENTENCES,
            "max_window_words": MAX_WINDOW_WORDS,
        })
    }

    fn run(&self, input: &GroundInput) -> Result<Grounded> {
        let chunk_index: BTreeMap<&ChunkId, usize> = input
            .chunks
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id(), i))
            .collect();
        let windows = windows(&input.chunks);
        let mut by_chunk: Vec<Vec<usize>> = vec![Vec::new(); input.chunks.len()];
        for (i, w) in windows.iter().enumerate() {
            by_chunk[w.chunk].push(i);
        }

        let claim_texts: Vec<&str> = input.claims.iter().map(Claim::text).collect();
        let window_texts: Vec<&str> = windows.iter().map(|w| w.text).collect();
        let texts: Vec<&str> = claim_texts.iter().chain(&window_texts).copied().collect();
        let vectors = embed_checked(self.embedder, Self::ID, &texts)?;
        // One vector per text, or none at all when there are no texts.
        let (claim_vectors, window_vectors) =
            vectors.split_at(claim_texts.len().min(vectors.len()));

        // For each piece of evidence, the GROUND_K windows of its chunk most
        // similar to the claim. Similarity only picks what to judge; the NLI
        // score alone decides.
        let mut candidates: Vec<Candidate> = Vec::new();
        for (c, claim) in input.claims.iter().enumerate() {
            for (e, evidence) in claim.evidence().iter().enumerate() {
                // `get` returns `Option<&usize>`; the `&chunk` pattern matches
                // through the reference and binds the `usize` itself (a copy),
                // so `chunk` doesn't keep the map borrowed.
                let &chunk = chunk_index.get(&evidence.chunk).ok_or_else(|| {
                    CoreError::InvalidProviderOutput {
                        stage: Self::ID,
                        message: format!(
                            "claim {} cites chunk {}, which is not in the input",
                            claim.id(),
                            evidence.chunk
                        ),
                    }
                })?;
                let mut ranked: Vec<(usize, f32)> = by_chunk[chunk]
                    .iter()
                    .map(|&w| (w, cosine(&claim_vectors[c], &window_vectors[w])))
                    .collect();
                // Most similar first; ties by window order, so the choice is
                // deterministic. `total_cmp` orders every f32, NaN included,
                // which plain `<` on floats can't.
                ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                candidates.extend(
                    ranked
                        .into_iter()
                        .take(GROUND_K)
                        .map(|(window, _)| Candidate {
                            claim: c,
                            evidence: e,
                            window,
                        }),
                );
            }
        }

        let names: Vec<String> = input
            .chunks
            .iter()
            .map(|chunk| {
                let title = input.titles.get(chunk.document()).map(String::as_str);
                names_prefix(
                    title
                        .into_iter()
                        .chain(chunk.heading_path().iter().map(String::as_str)),
                )
            })
            .collect();
        let premises: Vec<String> = candidates
            .iter()
            .map(|c| {
                let window = &windows[c.window];
                format!("{}{}", names[window.chunk], window.text)
            })
            .collect();
        let pairs: Vec<NliPair<'_>> = candidates
            .iter()
            .zip(&premises)
            .map(|(c, premise)| NliPair {
                premise,
                hypothesis: input.claims[c.claim].text(),
            })
            .collect();
        let scores = score_checked(self.nli, Self::ID, &pairs)?;

        // The best window per piece of evidence. Candidates arrive most
        // similar first, so on equal scores the more similar window wins.
        let mut best: BTreeMap<(usize, usize), (PerMille, usize)> = BTreeMap::new();
        for (candidate, score) in candidates.iter().zip(&scores) {
            let entailment = PerMille::from_probability(score.entailment);
            let key = (candidate.claim, candidate.evidence);
            if best.get(&key).is_none_or(|(held, _)| entailment > *held) {
                best.insert(key, (entailment, candidate.window));
            }
        }

        let mut claims = Vec::new();
        let mut rejected = Vec::new();
        for (c, claim) in input.claims.iter().enumerate() {
            let mut kept = Claim::new(claim.text());
            for (e, evidence) in claim.evidence().iter().enumerate() {
                // No entry: the chunk has no sentence, so nothing entails it.
                let (entailment, window) = match best.get(&(c, e)) {
                    Some(&(pm, w)) => (pm, Some(w)),
                    None => (PerMille::from_probability(0.0), None),
                };
                if entailment.get() >= GROUND_ENTAIL_PM {
                    kept.add_evidence(evidence.clone());
                } else {
                    rejected.push(Rejection {
                        claim: claim.id().clone(),
                        text: claim.text().to_owned(),
                        chunk: evidence.chunk.clone(),
                        entailment_pm: entailment,
                        premise: window.map(|w| windows[w].span),
                    });
                }
            }
            if !kept.evidence().is_empty() {
                claims.push(kept);
            }
        }

        let grounded = Grounded { claims, rejected };
        tracing::info!(
            claims = input.claims.len(),
            kept = grounded.claims.len(),
            dropped_claims = grounded.dropped_claims(),
            rejected_evidence = grounded.rejected.len(),
            pairs = pairs.len(),
            "claims grounded"
        );
        Ok(grounded)
    }
}

/// `"<name>. <name>. "` for the non-empty names, each ending in exactly one
/// sentence mark, ready to put in front of a window. Empty for no names.
fn names_prefix<'n>(names: impl Iterator<Item = &'n str>) -> String {
    let mut out = String::new();
    for name in names.map(str::trim).filter(|n| !n.is_empty()) {
        out.push_str(name);
        if !name.ends_with(['.', '!', '?']) {
            out.push('.');
        }
        out.push(' ');
    }
    out
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::plugin::{FakeEmbedding, FakeNli, NliScores};
    use podling_types::{Document, Evidence, SourceRef, Stance};

    /// A chunk spanning the whole of a document `locator` holding `text`,
    /// under `headings`.
    fn chunk(locator: &str, headings: &[&str], text: &str) -> Chunk {
        let doc = Document::new(source(locator), locator, text);
        let span = TextSpan::new(0, text.len()).unwrap();
        let headings = headings.iter().map(|h| (*h).to_owned()).collect();
        Chunk::from_document(&doc, span, headings).unwrap()
    }

    fn source(locator: &str) -> SourceRef {
        SourceRef {
            connector: "t".into(),
            locator: locator.into(),
            independence_group: locator.into(),
        }
    }

    /// `text` as a claim extracted from each of `chunks`.
    fn claim(text: &str, chunks: &[&Chunk]) -> Claim {
        let mut claim = Claim::new(text);
        // The source doesn't matter to grounding, only the chunk does.
        let source = source("g");
        for chunk in chunks {
            claim.add_evidence(Evidence {
                chunk: chunk.id().clone(),
                source: source.id(),
                independence_group: source.independence_group.clone(),
                stance: Stance::Supports,
                basis: None,
            });
        }
        claim
    }

    fn input(claims: Vec<Claim>, chunks: Vec<Chunk>) -> GroundInput {
        GroundInput {
            claims,
            chunks,
            titles: BTreeMap::new(),
        }
    }

    fn ground(nli: &dyn NliProvider, input: &GroundInput) -> Grounded {
        GroundClaims {
            embedder: &FakeEmbedding,
            nli,
        }
        .run(input)
        .unwrap()
    }

    fn texts(grounded: &Grounded) -> Vec<&str> {
        grounded.claims.iter().map(Claim::text).collect()
    }

    #[test]
    fn a_distortion_in_the_chunks_own_words_is_dropped() {
        let joined = chunk("a", &[], "Kulik joined the expedition.");
        let input = input(
            vec![
                // The lexical check passes it: two of its three content words
                // are in the chunk. FakeNli gives it 2/3, under 800.
                claim("Kulik led the expedition.", &[&joined]),
                claim("Kulik joined the expedition.", &[&joined]),
            ],
            vec![joined.clone()],
        );
        let grounded = ground(&FakeNli, &input);
        assert_eq!(texts(&grounded), ["Kulik joined the expedition."]);
        assert_eq!(grounded.dropped_claims(), 1);
        // A slice pattern: matches only a slice of exactly one element and
        // binds it, so "exactly one rejection" and "take it" are one step.
        let [rejection] = grounded.rejected.as_slice() else {
            panic!("expected one rejection: {:?}", grounded.rejected);
        };
        assert_eq!(rejection.text, "Kulik led the expedition.");
        assert_eq!(&rejection.chunk, joined.id());
        assert_eq!(rejection.entailment_pm.get(), 667);
        assert_eq!(
            rejection.premise,
            Some(TextSpan::new(0, "Kulik joined the expedition.".len()).unwrap())
        );
    }

    #[test]
    fn a_merged_claim_keeps_only_the_chunks_that_entail_it() {
        let a = chunk("a", &[], "Kulik joined the expedition.");
        let b = chunk("b", &[], "Trees fell over the forest.");
        let input = input(
            vec![claim("Kulik joined the expedition.", &[&a, &b])],
            vec![a.clone(), b.clone()],
        );
        let grounded = ground(&FakeNli, &input);
        let [kept] = grounded.claims.as_slice() else {
            panic!("the claim must survive");
        };
        let chunks: Vec<&ChunkId> = kept.evidence().iter().map(|e| &e.chunk).collect();
        assert_eq!(chunks, [a.id()]);
        assert_eq!(grounded.rejected.len(), 1);
        assert_eq!(&grounded.rejected[0].chunk, b.id());
        assert_eq!(grounded.dropped_claims(), 0);
    }

    /// Scores every pair with FakeNli and keeps the premises it was given.
    #[derive(Default)]
    struct Recording(RefCell<Vec<String>>);
    impl NliProvider for Recording {
        fn id(&self) -> &str {
            "recording"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>> {
            self.0
                .borrow_mut()
                .extend(pairs.iter().map(|p| p.premise.to_owned()));
            FakeNli.score(pairs)
        }
    }

    #[test]
    fn the_title_and_headings_name_what_the_chunk_refers_to() {
        let claim_text = "The Tunguska event happened in 1908.";
        let under = chunk("a", &["Tunguska event"], "It happened in 1908.");
        let mut with_names = input(vec![claim(claim_text, &[&under])], vec![under.clone()]);
        with_names
            .titles
            .insert(under.document().clone(), "Siberia, 1908.".into());
        let nli = Recording::default();
        let grounded = ground(&nli, &with_names);
        assert_eq!(texts(&grounded), [claim_text]);
        assert_eq!(
            nli.0.borrow().as_slice(),
            ["Siberia, 1908. Tunguska event. It happened in 1908."]
        );

        // Without the heading, the chunk alone doesn't say what "it" is.
        let bare = chunk("a", &[], "It happened in 1908.");
        let grounded = ground(
            &FakeNli,
            &input(vec![claim(claim_text, &[&bare])], vec![bare.clone()]),
        );
        assert!(grounded.claims.is_empty());
        assert_eq!(grounded.rejected[0].entailment_pm.get(), 500);
    }

    /// The same entailment for every pair; counts the pairs.
    struct Constant(f32, Cell<usize>);
    impl NliProvider for Constant {
        fn id(&self) -> &str {
            "constant"
        }
        fn fingerprint(&self) -> Value {
            json!({ "constant": self.0 })
        }
        fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>> {
            self.1.set(self.1.get() + pairs.len());
            Ok(vec![
                NliScores {
                    entailment: self.0,
                    neutral: 1.0 - self.0,
                    contradiction: 0.0,
                };
                pairs.len()
            ])
        }
    }

    #[test]
    fn the_threshold_is_inclusive() {
        let a = chunk("a", &[], "Trees fell.");
        let input = input(vec![claim("Trees fell.", &[&a])], vec![a.clone()]);
        let kept = ground(&Constant(0.80, Cell::new(0)), &input);
        assert_eq!(kept.claims.len(), 1);
        let dropped = ground(&Constant(0.799, Cell::new(0)), &input);
        assert!(dropped.claims.is_empty());
        assert_eq!(dropped.rejected[0].entailment_pm.get(), 799);
    }

    #[test]
    fn each_piece_of_evidence_is_judged_against_at_most_k_windows() {
        let long = chunk(
            "a",
            &[],
            "Trees fell by the river. Trees fell by the lake. Trees fell by the hill. \
             Trees fell by the road. Trees fell by the camp. Trees fell by the farm.",
        );
        let input = input(vec![claim("Trees fell.", &[&long])], vec![long.clone()]);
        let nli = Constant(1.0, Cell::new(0));
        ground(&nli, &input);
        assert_eq!(nli.1.get(), GROUND_K);
    }

    #[test]
    fn the_cache_key_follows_the_nli_provider() {
        let fingerprint = |nli: &dyn NliProvider| {
            GroundClaims {
                embedder: &FakeEmbedding,
                nli,
            }
            .config_fingerprint()
        };
        let base = fingerprint(&FakeNli);
        assert_ne!(base, fingerprint(&Constant(1.0, Cell::new(0))));
        assert_eq!(base["ground_entail_pm"], GROUND_ENTAIL_PM);
        assert_eq!(base["ground_k"], GROUND_K);
    }

    #[test]
    fn evidence_from_an_unknown_chunk_is_an_error() {
        let a = chunk("a", &[], "Trees fell.");
        let input = input(vec![claim("Trees fell.", &[&a])], vec![]);
        let err = GroundClaims {
            embedder: &FakeEmbedding,
            nli: &FakeNli,
        }
        .run(&input)
        .unwrap_err();
        assert!(matches!(
            err,
            CoreError::InvalidProviderOutput {
                stage: "ground_claims",
                ..
            }
        ));
    }

    #[test]
    fn names_end_in_one_sentence_mark() {
        assert_eq!(
            names_prefix(["A", " B. ", "", "C?"].into_iter()),
            "A. B. C? "
        );
        assert_eq!(names_prefix(std::iter::empty()), "");
    }
}
