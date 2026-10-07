//! Checks every claim against the other independence groups' sources with an
//! NLI model, adding `Supports` or `Contradicts` evidence.
//!
//! Without this stage a claim only has evidence from the chunks it was
//! extracted from, so two sources stating one fact in different words stay
//! SingleSource, and nothing can ever contradict anything. Here each claim is
//! read against source sentences from groups that don't already back it:
//! a sentence that entails it is support from that group, one that
//! contradicts it is a contradiction. The ledger's `classify` then turns the
//! evidence into a status exactly as before.
//!
//! Cost is bounded: embeddings pick at most [`RETRIEVE_K`] premise windows per
//! claim, so a run makes at most `RETRIEVE_K × claims` NLI calls, however
//! large the sources.

use std::collections::{BTreeMap, BTreeSet};

use podling_types::{
    Chunk, Claim, DocumentId, Evidence, EvidenceBasis, PerMille, SourceRef, Stance,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::windows::windows;
use crate::error::{CoreError, Result};
use crate::plugin::{
    EmbeddingProvider, NliPair, NliProvider, cosine, embed_checked, score_checked,
};
use crate::stage::Stage;
use crate::text::{numbers, sentences};

/// Premise windows retrieved per claim (the cost bound).
pub const RETRIEVE_K: usize = 4;
/// A window less similar to the claim than this isn't worth an NLI call.
pub const MIN_RETRIEVAL_PM: u16 = 300;
/// Entailment at or above this makes the window's chunk support the claim.
pub const SUPPORT_ENTAIL_PM: u16 = 800;
/// Contradiction at or above this, from a window at least
/// [`MIN_CONTRADICT_SIMILARITY_PM`] similar to the claim, makes the chunk
/// contradict it. Set high because NLI models over-call contradiction on
/// sentences that merely share a topic: DeBERTa gives 0.903 to "Kulik
/// reached the site in 1927" against "No impact crater was found".
pub const CONTRADICT_PM: u16 = 950;
/// A contradiction only counts between texts about the same thing.
pub const MIN_CONTRADICT_SIMILARITY_PM: u16 = 600;
// Re-exported so the constant keeps its public path here; it lives with the
// window code both NLI stages share.
pub use super::windows::{MAX_WINDOW_SENTENCES, MAX_WINDOW_WORDS};

#[derive(Debug, Clone, Serialize)]
pub struct StanceInput {
    /// Claims with their evidence so far.
    pub claims: Vec<Claim>,
    pub chunks: Vec<Chunk>,
    /// Where each chunk's document came from, for its independence group.
    pub sources: BTreeMap<DocumentId, SourceRef>,
}

pub struct ScoreStances<'a> {
    pub embedder: &'a dyn EmbeddingProvider,
    pub nli: &'a dyn NliProvider,
}

impl Stage for ScoreStances<'_> {
    const ID: &'static str = "score_stances";
    // 3: a number-against-number contradiction needs the premise's number to
    // share the claim's subject.
    // 4: only a premise naming the subject in a numberless sentence is
    // refused; a reworded subject keeps the model's call
    // (`number_is_about_the_subject`).
    // 5: a numbered sentence opening with a pronoun also names the sentence
    // before it, so a contradiction told by pronoun isn't refused.
    // 6: the pronoun may sit anywhere in the sentence ("In 1931 he got
    // there.", "His arrival came in 1931."), not only first.
    // 7: a number-against-number contradiction must also hold with claim and
    // premise swapped (`StanceEvidence::reverse_contradiction`).
    // 8: the reverse check also reads the whole window when one of its
    // sentences has no number (`reverse_hypotheses`), and replaces the
    // subject rule of 3-6: a number about something else fails it too.
    const VERSION: u32 = 8;
    type Input = StanceInput;
    type Output = Vec<Claim>;

    fn config_fingerprint(&self) -> Value {
        json!({
            "embedding": self.embedder.fingerprint(),
            "nli": self.nli.fingerprint(),
            "retrieve_k": RETRIEVE_K,
            "min_retrieval_pm": MIN_RETRIEVAL_PM,
            "support_entail_pm": SUPPORT_ENTAIL_PM,
            "contradict_pm": CONTRADICT_PM,
            "min_contradict_similarity_pm": MIN_CONTRADICT_SIMILARITY_PM,
            "max_window_sentences": MAX_WINDOW_SENTENCES,
            "max_window_words": MAX_WINDOW_WORDS,
        })
    }

    fn run(&self, input: &StanceInput) -> Result<Vec<Claim>> {
        let groups = groups(input)?;
        let windows = windows(&input.chunks);
        let claim_texts: Vec<&str> = input.claims.iter().map(Claim::text).collect();
        let window_texts: Vec<&str> = windows.iter().map(|w| w.text).collect();
        let texts: Vec<&str> = claim_texts.iter().chain(&window_texts).copied().collect();
        let vectors = embed_checked(self.embedder, Self::ID, &texts)?;
        // `embed_checked` guarantees one vector per text (or none at all when
        // there are no texts), so this split can't panic.
        let (claim_vectors, window_vectors) =
            vectors.split_at(claim_texts.len().min(vectors.len()));

        // Pick the windows each claim is checked against, then judge every
        // pair in one call so a real model can batch them.
        let mut candidates: Vec<Candidate> = Vec::new();
        for (c, claim) in input.claims.iter().enumerate() {
            let backed: BTreeSet<&str> = claim
                .evidence()
                .iter()
                .map(|e| e.independence_group.as_str())
                .collect();
            let mut ranked: Vec<(usize, PerMille)> = windows
                .iter()
                .enumerate()
                .filter(|(_, w)| !backed.contains(groups[w.chunk]))
                .map(|(i, _)| {
                    let similarity = cosine(&claim_vectors[c], &window_vectors[i]);
                    (i, PerMille::from_probability(similarity))
                })
                .filter(|(_, similarity)| similarity.get() >= MIN_RETRIEVAL_PM)
                .collect();
            // Most similar first; ties by window order, so the choice is
            // deterministic.
            ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            candidates.extend(
                ranked
                    .into_iter()
                    .take(RETRIEVE_K)
                    .map(|(window, similarity)| Candidate {
                        claim: c,
                        window,
                        similarity,
                    }),
            );
        }
        let pairs: Vec<NliPair<'_>> = candidates
            .iter()
            .map(|c| NliPair {
                premise: windows[c.window].text,
                hypothesis: input.claims[c.claim].text(),
            })
            .collect();
        let scores = score_checked(self.nli, Self::ID, &pairs)?;
        let mut judgements: Vec<Judged> = candidates
            .iter()
            .zip(&scores)
            .map(|(candidate, score)| Judged {
                window: candidate.window,
                similarity: candidate.similarity,
                entailment: PerMille::from_probability(score.entailment),
                contradiction: PerMille::from_probability(score.contradiction),
                reverse_contradiction: None,
            })
            .collect();

        // A number-against-number contradiction must also hold the other way
        // round, the claim as premise against each of the window's numbered
        // sentences. Only candidates that would contradict if it did are
        // scored, again in one call. Asking `decide` with the best possible
        // reverse score keeps the rule in one place.
        let mut reverse_pairs: Vec<NliPair<'_>> = Vec::new();
        let mut reverse_of: Vec<(usize, usize)> = Vec::new(); // (judgement, pair count)
        for (j, (candidate, judged)) in candidates.iter().zip(&judgements).enumerate() {
            let claim = input.claims[candidate.claim].text();
            let premise = windows[candidate.window].text;
            let hypotheses = reverse_hypotheses(premise);
            let would_contradict = Judged {
                reverse_contradiction: Some(PerMille::from_probability(1.0)),
                ..*judged
            }
            .stance(claim, premise)
                == Some(Stance::Contradicts);
            if numbers(claim).is_empty() || hypotheses.is_empty() || !would_contradict {
                continue;
            }
            reverse_of.push((j, hypotheses.len()));
            reverse_pairs.extend(hypotheses.into_iter().map(|hypothesis| NliPair {
                premise: claim,
                hypothesis,
            }));
        }
        let reverse_scores = score_checked(self.nli, Self::ID, &reverse_pairs)?;
        let mut next = reverse_scores.iter();
        for (j, count) in reverse_of {
            judgements[j].reverse_contradiction = next
                .by_ref()
                .take(count)
                .map(|s| PerMille::from_probability(s.contradiction))
                .max();
        }

        // Keep, per claim and chunk, the one window that decides: the best
        // entailment if any window supports the claim, otherwise the best
        // qualifying contradiction. A chunk is never both for and against.
        let mut decided: BTreeMap<(usize, usize), (Stance, Judged)> = BTreeMap::new();
        for (candidate, judged) in candidates.iter().zip(judgements) {
            let premise = windows[candidate.window].text;
            let Some(stance) = judged.stance(input.claims[candidate.claim].text(), premise) else {
                continue;
            };
            let key = (candidate.claim, windows[candidate.window].chunk);
            let better = match decided.get(&key) {
                None => true,
                Some((held, current)) => match (stance, held) {
                    (Stance::Supports, Stance::Contradicts) => true,
                    (Stance::Contradicts, Stance::Supports) => false,
                    (Stance::Supports, _) => judged.entailment > current.entailment,
                    (Stance::Contradicts, _) => judged.contradiction > current.contradiction,
                },
            };
            if better {
                decided.insert(key, (stance, judged));
            }
        }

        let mut claims = input.claims.clone();
        let (mut supports, mut contradicts) = (0usize, 0usize);
        for ((c, _), (stance, judged)) in decided {
            let window = &windows[judged.window];
            let chunk = &input.chunks[window.chunk];
            let source = &input.sources[chunk.document()];
            match stance {
                Stance::Supports => supports += 1,
                Stance::Contradicts => contradicts += 1,
            }
            claims[c].add_evidence(Evidence {
                chunk: chunk.id().clone(),
                source: source.id(),
                independence_group: source.independence_group.clone(),
                stance,
                basis: Some(EvidenceBasis::Nli {
                    premise: window.span,
                    similarity_pm: judged.similarity,
                    entailment_pm: judged.entailment,
                    contradiction_pm: judged.contradiction,
                }),
            });
        }
        tracing::info!(
            claims = claims.len(),
            windows = windows.len(),
            pairs = pairs.len(),
            reverse_pairs = reverse_pairs.len(),
            supports,
            contradicts,
            "stances scored"
        );
        Ok(claims)
    }
}

struct Candidate {
    claim: usize,
    window: usize,
    similarity: PerMille,
}

#[derive(Clone, Copy)]
struct Judged {
    window: usize,
    similarity: PerMille,
    entailment: PerMille,
    contradiction: PerMille,
    reverse_contradiction: Option<PerMille>,
}

/// Everything one premise window's stance on a claim is decided from: the two
/// texts and the scores the models gave them. Public so the stance rule can
/// be measured on a labelled pair set (`tests/stance_precision.rs`) without
/// running the whole stage.
#[derive(Debug, Clone, Copy)]
pub struct StanceEvidence<'a> {
    pub claim: &'a str,
    pub premise: &'a str,
    /// Embedding cosine of the claim and the premise.
    pub similarity: PerMille,
    pub entailment: PerMille,
    pub contradiction: PerMille,
    /// The highest contradiction of the claim, read as the premise, against
    /// each of [`reverse_hypotheses`]`(premise)`; `None` when not scored.
    /// Only a number-against-number contradiction needs it.
    pub reverse_contradiction: Option<PerMille>,
}

/// The stance a premise establishes on a claim, if any. Compared on the
/// rounded scores, so the stored numbers are the ones that decided.
pub fn decide(evidence: &StanceEvidence<'_>) -> Option<Stance> {
    if evidence.entailment.get() >= SUPPORT_ENTAIL_PM {
        Some(Stance::Supports)
    } else if evidence.contradiction.get() >= CONTRADICT_PM
        && evidence.similarity.get() >= MIN_CONTRADICT_SIMILARITY_PM
        && holds_both_ways(evidence)
    {
        Some(Stance::Contradicts)
    } else {
        None
    }
}

/// When the claim and the premise both state a number, whether the
/// contradiction also holds read the other way round. NLI models over-call a
/// differing count that only shares a topic one way: "The vessel was provided
/// with lifeboats for 1,176 persons." is "contradicted" by "From these boats
/// he took on board 712 persons" (0.997), but the claim doesn't contradict
/// that sentence (0.010), while "706 persons were saved." does both ways
/// (0.997 / 0.995). A number about something else fails the same way: "The
/// explosion happened in June 1908." against "... Kulik's expedition reached
/// the site in 1927." (the 1927 dates the expedition). The cost: a count the
/// model reads as a subset ("8 million fir trunks" of "80 million trees") is
/// no longer a contradiction. Number filtering rests on the model alone here:
/// no word-overlap rule backs it.
fn holds_both_ways(evidence: &StanceEvidence<'_>) -> bool {
    if numbers(evidence.claim).is_empty() || numbers(evidence.premise).is_empty() {
        return true;
    }
    evidence
        .reverse_contradiction
        .is_some_and(|r| r.get() >= CONTRADICT_PM)
}

/// The hypotheses the claim is read against for
/// [`StanceEvidence::reverse_contradiction`]: each sentence of `premise` that
/// holds a number, then the whole window when it also has a sentence without
/// one. The numbered sentences come one by one so the claim is judged against
/// the numbered statement itself; the whole window lets a contradiction the
/// window states without a number ("Kulik never reached the site. The
/// expedition set off in 1927.") be found both ways. Empty when no sentence
/// holds a number.
pub fn reverse_hypotheses(premise: &str) -> Vec<&str> {
    let (numbered, numberless): (Vec<&str>, Vec<&str>) = sentences(premise)
        .into_iter()
        .map(|range| &premise[range])
        .partition(|sentence| !numbers(sentence).is_empty());
    let mut hypotheses = numbered;
    if !hypotheses.is_empty() && !numberless.is_empty() {
        hypotheses.push(premise);
    }
    hypotheses
}

impl Judged {
    /// The stance this judgement of `premise` against `claim` establishes.
    fn stance(&self, claim: &str, premise: &str) -> Option<Stance> {
        decide(&StanceEvidence {
            claim,
            premise,
            similarity: self.similarity,
            entailment: self.entailment,
            contradiction: self.contradiction,
            reverse_contradiction: self.reverse_contradiction,
        })
    }
}

/// The independence group of each chunk, by chunk index. A chunk whose
/// document has no source is an error.
fn groups(input: &StanceInput) -> Result<Vec<&str>> {
    input
        .chunks
        .iter()
        .map(|chunk| {
            input
                .sources
                .get(chunk.document())
                .map(|source| source.independence_group.as_str())
                .ok_or_else(|| CoreError::InvalidProviderOutput {
                    stage: ScoreStances::ID,
                    message: format!("chunk {} belongs to an unknown document", chunk.id()),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{FakeEmbedding, FakeNli, NliScores};
    use crate::text::sentences;
    use podling_types::{ClaimStatus, Document, TextSpan, classify};

    fn chunk_in(group: &str, text: &str) -> (Chunk, SourceRef) {
        let source = SourceRef {
            connector: "t".into(),
            locator: group.into(),
            independence_group: group.into(),
        };
        // A heading before the text, so chunk offsets differ from document
        // offsets and the premise span must add them up.
        let doc = Document::new(source.clone(), group, format!("# {group}\n\n{text}"));
        let start = doc.text().len() - text.len();
        let span = TextSpan::new(start, doc.text().len()).unwrap();
        (Chunk::from_document(&doc, span, vec![]).unwrap(), source)
    }

    /// Claims extracted as every sentence of their chunk, like `FakeLlm`.
    fn input(chunks: &[(&str, &str)]) -> StanceInput {
        let built: Vec<(Chunk, SourceRef)> = chunks.iter().map(|(g, t)| chunk_in(g, t)).collect();
        let mut claims = Vec::new();
        for (chunk, source) in &built {
            for range in sentences(chunk.text()) {
                let mut claim = Claim::new(&chunk.text()[range]);
                claim.add_evidence(Evidence {
                    chunk: chunk.id().clone(),
                    source: source.id(),
                    independence_group: source.independence_group.clone(),
                    stance: Stance::Supports,
                    basis: None,
                });
                claims.push(claim);
            }
        }
        StanceInput {
            claims,
            sources: built
                .iter()
                .map(|(c, s)| (c.document().clone(), s.clone()))
                .collect(),
            chunks: built.into_iter().map(|(c, _)| c).collect(),
        }
    }

    fn run(nli: &dyn NliProvider, input: &StanceInput) -> Vec<Claim> {
        ScoreStances {
            embedder: &FakeEmbedding,
            nli,
        }
        .run(input)
        .unwrap()
    }

    fn status_of<'c>(claims: &'c [Claim], text: &str) -> (ClaimStatus, &'c Claim) {
        let claim = claims.iter().find(|c| c.text() == text).unwrap();
        (classify(claim), claim)
    }

    #[test]
    fn a_paraphrase_in_another_group_supports_the_claim() {
        let a = "In June 1908 an explosion flattened about 80 million trees.";
        let b = "About 80 million trees were flattened by an explosion in June 1908.";
        let input = input(&[("eyewitness", a), ("expedition", b)]);
        let claims = run(&FakeNli, &input);
        let (status, claim) = status_of(&claims, a);
        assert_eq!(
            status,
            ClaimStatus::Corroborated {
                groups: vec!["expedition".into(), "eyewitness".into()]
            }
        );
        let added = claim
            .evidence()
            .iter()
            .find(|e| e.independence_group == "expedition")
            .unwrap();
        let Some(EvidenceBasis::Nli {
            premise,
            entailment_pm,
            ..
        }) = &added.basis
        else {
            panic!("NLI evidence must carry its scores");
        };
        assert_eq!(entailment_pm.get(), 1000);
        // The premise is a span of the expedition document, holding b.
        let doc = Document::new(
            input.sources[input.chunks[1].document()].clone(),
            "expedition",
            format!("# expedition\n\n{b}"),
        );
        assert_eq!(doc.slice(*premise), Some(b));
    }

    #[test]
    fn a_changed_year_in_another_group_contests_the_claim() {
        let input = input(&[
            ("eyewitness", "The explosion happened in June 1908."),
            ("expedition", "The explosion happened in June 1907."),
        ]);
        let claims = run(&FakeNli, &input);
        for text in [
            "The explosion happened in June 1908.",
            "The explosion happened in June 1907.",
        ] {
            let (status, _) = status_of(&claims, text);
            assert!(
                matches!(status, ClaimStatus::Contested { .. }),
                "{text}: {status:?}"
            );
        }
    }

    /// Returns the same scores for every pair and counts the pairs.
    struct Constant(NliScores, std::cell::Cell<usize>);
    impl NliProvider for Constant {
        fn id(&self) -> &str {
            "constant"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>> {
            self.1.set(self.1.get() + pairs.len());
            Ok(vec![self.0; pairs.len()])
        }
    }

    fn constant(entailment: f32, contradiction: f32) -> Constant {
        Constant(
            NliScores {
                entailment,
                neutral: 1.0 - entailment - contradiction,
                contradiction,
            },
            Default::default(),
        )
    }

    const A: &str = "The blast flattened trees near the river.";
    const B: &str =
        "Trees near the river were flattened. The blast was loud. Trees burned near the river.";

    #[test]
    fn thresholds_decide_the_stance() {
        let input = input(&[("a", A), ("b", B)]);
        let supported = run(&constant(0.85, 0.0), &input);
        assert!(matches!(
            status_of(&supported, A).0,
            ClaimStatus::Corroborated { .. }
        ));
        let contested = run(&constant(0.0, 0.97), &input);
        assert!(matches!(
            status_of(&contested, A).0,
            ClaimStatus::Contested { .. }
        ));
        // Just under both thresholds: no new evidence at all.
        let neither = run(&constant(0.79, 0.94), &input);
        assert_eq!(neither, input.claims);
    }

    #[test]
    fn a_contradiction_needs_a_premise_about_the_same_thing() {
        // Every pair "contradicts", but the other group's only sentence
        // shares no words with the claim.
        let input = input(&[("a", A), ("b", "Kulik reached the camp in autumn.")]);
        let claims = run(&constant(0.0, 1.0), &input);
        assert_eq!(claims, input.claims);
    }

    #[test]
    fn each_claim_is_checked_against_at_most_k_windows() {
        let long = "Trees fell by the river. Trees fell by the lake. Trees fell by the hill. \
                    Trees fell by the road. Trees fell by the camp. Trees fell by the farm.";
        let input = input(&[("a", "Trees fell."), ("b", long)]);
        let nli = constant(0.0, 0.0);
        run(&nli, &input);
        // Claims from `b` see `a`'s one window; the claim from `a` sees at
        // most K of `b`'s eleven windows.
        let from_b = sentences(long).len();
        assert!(nli.1.get() <= RETRIEVE_K + from_b, "{} pairs", nli.1.get());
    }

    #[test]
    fn one_chunk_gives_at_most_one_piece_of_evidence_per_claim() {
        let input = input(&[("a", A), ("b", B)]);
        let claims = run(&constant(0.9, 0.0), &input);
        let from_b = status_of(&claims, A)
            .1
            .evidence()
            .iter()
            .filter(|e| e.independence_group == "b")
            .count();
        assert_eq!(from_b, 1);
    }

    #[test]
    fn a_number_about_something_else_fails_the_reverse_check() {
        // Scores from the real NLI model (`stance_pairs` n21 and t03).
        let pm = |p| PerMille::from_probability(p);
        let evidence = |claim, premise, reverse| StanceEvidence {
            claim,
            premise,
            similarity: pm(0.7),
            entailment: pm(0.0),
            contradiction: pm(0.99),
            reverse_contradiction: Some(pm(reverse)),
        };
        // The window's only year dates the expedition, not the explosion:
        // read the other way the claim doesn't contradict it.
        let other_subject = "The explosion was heard hundreds of kilometres away. Kulik's \
                             expedition reached the site in 1927.";
        assert_eq!(
            decide(&evidence(
                "The explosion happened in June 1908.",
                other_subject,
                0.080
            )),
            None
        );
        // A contradiction stated in the sentence without the number holds
        // both ways once the whole window is read.
        let numberless = "Kulik never reached the site. The expedition set off in 1927.";
        assert_eq!(
            decide(&evidence(
                "Kulik reached the site in 1927.",
                numberless,
                1.0
            )),
            Some(Stance::Contradicts)
        );
    }

    #[test]
    fn a_numeric_contradiction_must_hold_both_ways() {
        let evidence = |claim, reverse| StanceEvidence {
            claim,
            premise: "From these boats he took on board 712 persons, one of them died shortly \
                      afterwards.",
            similarity: PerMille::from_probability(0.7),
            entailment: PerMille::from_probability(0.0),
            contradiction: PerMille::from_probability(0.997),
            reverse_contradiction: reverse,
        };
        let pm = |p| Some(PerMille::from_probability(p));
        // The lifeboat capacity only shares a topic with the count saved:
        // the model calls it a contradiction one way only.
        let capacity = "The vessel was provided with lifeboats for 1,176 persons.";
        assert_eq!(decide(&evidence(capacity, pm(0.010))), None);
        // Not scored the other way: refused, never assumed.
        assert_eq!(decide(&evidence(capacity, None)), None);
        // Just under the threshold the other way is not enough.
        assert_eq!(decide(&evidence(capacity, pm(0.949))), None);
        // A real disagreement on the count holds both ways.
        let saved = "706 persons were saved.";
        assert_eq!(
            decide(&evidence(saved, pm(0.995))),
            Some(Stance::Contradicts)
        );
        assert_eq!(
            decide(&evidence(saved, pm(0.95))),
            Some(Stance::Contradicts)
        );
    }

    #[test]
    fn a_contradiction_without_numbers_ignores_the_reverse_score() {
        let evidence = |claim, premise| StanceEvidence {
            claim,
            premise,
            similarity: PerMille::from_probability(0.7),
            entailment: PerMille::from_probability(0.0),
            contradiction: PerMille::from_probability(0.99),
            reverse_contradiction: None,
        };
        // Neither text holds a number.
        assert_eq!(
            decide(&evidence(
                "No impact crater was found at the site.",
                "Kulik found a large crater at the site."
            )),
            Some(Stance::Contradicts)
        );
        // Only the premise holds one.
        assert_eq!(
            decide(&evidence(
                "No impact crater was found at the site.",
                "Kulik found a crater at the site in 1927."
            )),
            Some(Stance::Contradicts)
        );
    }

    #[test]
    fn reverse_hypotheses_are_the_numbered_sentences_and_a_mixed_window() {
        // A sentence without a number: the whole window is read too.
        let mixed = "Kulik never reached the site. The expedition set off in 1927.";
        assert_eq!(
            reverse_hypotheses(mixed),
            vec!["The expedition set off in 1927.", mixed]
        );
        // Every sentence numbered: just the sentences.
        assert_eq!(
            reverse_hypotheses("706 persons were saved. 712 were taken on board."),
            vec!["706 persons were saved.", "712 were taken on board."]
        );
        // No number anywhere: nothing to read back.
        assert!(reverse_hypotheses("The ship sank. Rescue came at dawn.").is_empty());
    }

    #[test]
    fn the_stage_needs_the_contradiction_both_ways() {
        const CAPACITY: &str = "The boats took on board 1176 persons.";
        const TAKEN: &str = "The boats took on board 712 persons.";
        /// Contradiction 1.0 whenever CAPACITY is the hypothesis, and `back`
        /// for every other pair: with `back` 0 the contradiction is one-way.
        struct OneWay {
            back: f32,
            pairs: std::cell::Cell<usize>,
        }
        impl NliProvider for OneWay {
            fn id(&self) -> &str {
                "one-way"
            }
            fn fingerprint(&self) -> Value {
                Value::Null
            }
            fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>> {
                self.pairs.set(self.pairs.get() + pairs.len());
                Ok(pairs
                    .iter()
                    .map(|p| {
                        let c = if p.hypothesis == CAPACITY {
                            1.0
                        } else {
                            self.back
                        };
                        NliScores {
                            entailment: 0.0,
                            neutral: 1.0 - c,
                            contradiction: c,
                        }
                    })
                    .collect())
            }
        }
        // Similar enough that only the reverse check can refuse it.
        let v = FakeEmbedding.embed(&[CAPACITY, TAKEN]).unwrap();
        let similarity = PerMille::from_probability(cosine(&v[0], &v[1]));
        assert!(
            similarity.get() >= MIN_CONTRADICT_SIMILARITY_PM,
            "{similarity:?}"
        );
        let input = input(&[("a", CAPACITY), ("b", TAKEN)]);

        let one_way = OneWay {
            back: 0.0,
            pairs: Default::default(),
        };
        let claims = run(&one_way, &input);
        assert_eq!(claims, input.claims, "a one-way contradiction adds nothing");
        // Two forward pairs, then one reverse pair for CAPACITY, the only
        // forward contradiction.
        assert_eq!(one_way.pairs.get(), 3);

        let two_way = OneWay {
            back: 1.0,
            pairs: Default::default(),
        };
        let claims = run(&two_way, &input);
        for text in [CAPACITY, TAKEN] {
            let (status, _) = status_of(&claims, text);
            assert!(
                matches!(status, ClaimStatus::Contested { .. }),
                "{text}: {status:?}"
            );
        }
        // Two forward pairs, then one reverse pair for each claim.
        assert_eq!(two_way.pairs.get(), 4);
    }

    #[test]
    fn reverse_scores_go_to_their_own_candidate() {
        const X: &str = "The boats took on board 712 persons.";
        const Y: &str = "The boats took on board 20 persons.";
        const S1: &str = "The boats took on board 706 persons.";
        const S2: &str = "The boats took on board 705 persons.";
        /// Forward: X and Y are contradicted by every window holding S1.
        /// Reverse: only X against S2 contradicts. Records each batch.
        #[derive(Default)]
        struct Scripted(std::cell::RefCell<Vec<Vec<(String, String)>>>);
        impl NliProvider for Scripted {
            fn id(&self) -> &str {
                "scripted"
            }
            fn fingerprint(&self) -> Value {
                Value::Null
            }
            fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>> {
                self.0.borrow_mut().push(
                    pairs
                        .iter()
                        .map(|p| (p.premise.to_owned(), p.hypothesis.to_owned()))
                        .collect(),
                );
                Ok(pairs
                    .iter()
                    .map(|p| {
                        let forward = [X, Y].contains(&p.hypothesis) && p.premise.contains(S1);
                        let reverse = (p.premise, p.hypothesis) == (X, S2);
                        let c = if forward || reverse { 1.0 } else { 0.0 };
                        NliScores {
                            entailment: 0.0,
                            neutral: 1.0 - c,
                            contradiction: c,
                        }
                    })
                    .collect())
            }
        }
        // Group b's windows are S1, "S1 S2" (two numbered sentences) and S2.
        let window = format!("{S1} {S2}");
        let mut input = input(&[("a", X), ("y", Y), ("b", &window)]);
        input.claims.retain(|c| [X, Y].contains(&c.text()));
        for claim in [X, Y] {
            for premise in [S1, window.as_str()] {
                let v = FakeEmbedding.embed(&[claim, premise]).unwrap();
                let similarity = PerMille::from_probability(cosine(&v[0], &v[1]));
                assert!(
                    similarity.get() >= MIN_CONTRADICT_SIMILARITY_PM,
                    "{claim} / {premise}: {similarity:?}"
                );
            }
        }

        let nli = Scripted::default();
        let claims = run(&nli, &input);
        // X holds only through the two-sentence window, whose reverse score
        // is the higher of its two sentences; Y's reverse scores are all low.
        assert!(matches!(
            status_of(&claims, X).0,
            ClaimStatus::Contested { .. }
        ));
        assert!(matches!(
            status_of(&claims, Y).0,
            ClaimStatus::SingleSource { .. }
        ));
        // Two batches: forward, then the reverse pairs of the four
        // contradicting candidates (each claim against S1 and "S1 S2").
        let batches = nli.0.into_inner();
        assert_eq!(batches.len(), 2);
        let mut reverse = batches[1].clone();
        reverse.sort();
        let pair = |p: &str, h: &str| (p.to_owned(), h.to_owned());
        assert_eq!(
            reverse,
            vec![
                pair(Y, S2),
                pair(Y, S1),
                pair(Y, S1),
                pair(X, S2),
                pair(X, S1),
                pair(X, S1),
            ]
        );
    }

    #[test]
    fn a_claims_own_group_is_never_checked() {
        let input = input(&[("a", A), ("a", B)]);
        let nli = constant(1.0, 0.0);
        let claims = run(&nli, &input);
        assert_eq!(nli.1.get(), 0);
        assert_eq!(claims, input.claims);
    }
}
