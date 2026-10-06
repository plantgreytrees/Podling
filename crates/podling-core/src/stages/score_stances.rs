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
use crate::text::{content_words, is_number, numbers, sentences};

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
    // share the claim's subject (`numbers_share_the_subject`).
    const VERSION: u32 = 3;
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

        // Keep, per claim and chunk, the one window that decides: the best
        // entailment if any window supports the claim, otherwise the best
        // qualifying contradiction. A chunk is never both for and against.
        let mut decided: BTreeMap<(usize, usize), (Stance, Judged)> = BTreeMap::new();
        for (candidate, score) in candidates.iter().zip(&scores) {
            let judged = Judged {
                window: candidate.window,
                similarity: candidate.similarity,
                entailment: PerMille::from_probability(score.entailment),
                contradiction: PerMille::from_probability(score.contradiction),
            };
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

struct Judged {
    window: usize,
    similarity: PerMille,
    entailment: PerMille,
    contradiction: PerMille,
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
}

/// The stance a premise establishes on a claim, if any. Compared on the
/// rounded scores, so the stored numbers are the ones that decided.
pub fn decide(evidence: &StanceEvidence<'_>) -> Option<Stance> {
    if evidence.entailment.get() >= SUPPORT_ENTAIL_PM {
        Some(Stance::Supports)
    } else if evidence.contradiction.get() >= CONTRADICT_PM
        && evidence.similarity.get() >= MIN_CONTRADICT_SIMILARITY_PM
        && numbers_share_the_subject(evidence.claim, evidence.premise)
    {
        Some(Stance::Contradicts)
    } else {
        None
    }
}

/// When the claim and the premise both state a number, whether some premise
/// sentence holding a number shares a word (not a number) with the claim.
/// Without one, the premise's number is about something else: in "The
/// explosion was heard far away. Kulik's expedition reached the site in
/// 1927." against "The explosion happened in June 1908.", the model reads
/// 1927 as a contradicting date, but it dates the expedition, not the
/// explosion. True when either text has no number, so the rule only judges
/// number-against-number contradictions.
fn numbers_share_the_subject(claim: &str, premise: &str) -> bool {
    if numbers(claim).is_empty() || numbers(premise).is_empty() {
        return true;
    }
    let subject = |text: &str| -> BTreeSet<String> {
        content_words(text)
            .into_iter()
            .filter(|w| !is_number(w))
            .collect()
    };
    let claim_subject = subject(claim);
    sentences(premise).into_iter().any(|range| {
        let sentence = &premise[range];
        !numbers(sentence).is_empty() && !subject(sentence).is_disjoint(&claim_subject)
    })
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
    fn a_number_about_something_else_does_not_contradict() {
        let evidence = |premise| StanceEvidence {
            claim: "The explosion happened in June 1908.",
            premise,
            similarity: PerMille::from_probability(0.7),
            entailment: PerMille::from_probability(0.0),
            contradiction: PerMille::from_probability(0.99),
        };
        // The window's only year dates the expedition, not the explosion.
        let other_subject =
            "The explosion was heard far away. Kulik's expedition reached the site in 1927.";
        assert_eq!(decide(&evidence(other_subject)), None);
        // The same year beside the claim's subject does contradict it.
        let same_subject = "The explosion was heard far away. The explosion happened in 1927.";
        assert_eq!(decide(&evidence(same_subject)), Some(Stance::Contradicts));
        // Without a number in the premise the gate doesn't apply.
        let no_number = "Kulik's expedition found no crater.";
        assert_eq!(decide(&evidence(no_number)), Some(Stance::Contradicts));
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
