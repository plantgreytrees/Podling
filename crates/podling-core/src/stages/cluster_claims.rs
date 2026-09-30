//! Merges claims that state the same fact in different words.
//!
//! The extraction stage keys claims by their exact text, so "an explosion
//! flattened 80 million trees" and "80 million trees were flattened by an
//! explosion" arrive as two claims, each backed by one source. Here they
//! become one claim carrying both sources' evidence, which is what lets the
//! ledger call it Corroborated.
//!
//! Embeddings only nominate candidates; a pair merges only when the NLI
//! model finds that each entails the other, their numbers match, and every
//! other member of the cluster agrees too. Similarity alone never merges.

use std::collections::{BTreeMap, BTreeSet};

use podling_types::{Claim, EvidenceBasis, PerMille};
use serde_json::{Value, json};

use crate::error::Result;
use crate::plugin::{
    EmbeddingProvider, NliPair, NliProvider, cosine, embed_checked, score_checked,
};
use crate::stage::Stage;
use crate::text::numbers;

/// Two claims less similar than this are never compared.
pub const MERGE_CANDIDATE_PM: u16 = 800;
/// Candidates per claim, most similar first (the cost bound).
pub const MAX_MERGE_CANDIDATES: usize = 8;
/// Entailment each way must reach this for two claims to merge.
pub const MERGE_ENTAIL_PM: u16 = 900;

pub struct ClusterClaims<'a> {
    pub embedder: &'a dyn EmbeddingProvider,
    pub nli: &'a dyn NliProvider,
}

impl Stage for ClusterClaims<'_> {
    const ID: &'static str = "cluster_claims";
    const VERSION: u32 = 1;
    type Input = Vec<Claim>;
    type Output = Vec<Claim>;

    fn config_fingerprint(&self) -> Value {
        json!({
            "embedding": self.embedder.fingerprint(),
            "nli": self.nli.fingerprint(),
            "merge_candidate_pm": MERGE_CANDIDATE_PM,
            "max_merge_candidates": MAX_MERGE_CANDIDATES,
            "merge_entail_pm": MERGE_ENTAIL_PM,
            "number_veto": true,
        })
    }

    fn run(&self, claims: &Vec<Claim>) -> Result<Vec<Claim>> {
        cluster(claims, self.embedder, self.nli, NumberVeto::On)
    }
}

/// Whether claims with different numbers are kept apart before NLI is asked.
/// Always `On` in the stage; `Off` lets a test show that the NLI check alone
/// also refuses to merge "1907" with "1908".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumberVeto {
    On,
    #[cfg_attr(not(test), allow(dead_code))]
    Off,
}

fn cluster(
    claims: &[Claim],
    embedder: &dyn EmbeddingProvider,
    nli: &dyn NliProvider,
    veto: NumberVeto,
) -> Result<Vec<Claim>> {
    // Work in `ClaimId` order, so the result doesn't depend on input order.
    let mut claims: Vec<&Claim> = claims.iter().collect();
    claims.sort_by(|a, b| a.id().cmp(b.id()));
    let texts: Vec<&str> = claims.iter().map(|c| c.text()).collect();
    let vectors = embed_checked(embedder, ClusterClaims::ID, &texts)?;

    // Candidate pairs `(i, j)` with `i < j`: each claim nominates its most
    // similar partners. A `BTreeSet` drops the pairs both sides nominate.
    let mut candidates: BTreeSet<(usize, usize)> = BTreeSet::new();
    for i in 0..claims.len() {
        let mut ranked: Vec<(usize, PerMille)> = (0..claims.len())
            .filter(|&j| j != i)
            .map(|j| {
                (
                    j,
                    PerMille::from_probability(cosine(&vectors[i], &vectors[j])),
                )
            })
            .filter(|(_, similarity)| similarity.get() >= MERGE_CANDIDATE_PM)
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        candidates.extend(
            ranked
                .into_iter()
                .take(MAX_MERGE_CANDIDATES)
                .map(|(j, _)| (i.min(j), i.max(j))),
        );
    }
    if veto == NumberVeto::On {
        // "Happened in 1907" and "happened in 1908" read as near-paraphrases
        // to an embedding and sometimes to NLI; a date or count that differs
        // is a different fact, so such pairs never reach the model.
        candidates.retain(|&(i, j)| numbers(texts[i]) == numbers(texts[j]));
    }

    // Judge each candidate both ways in one batch: premise i, hypothesis j,
    // then the reverse.
    let pairs: Vec<NliPair<'_>> = candidates
        .iter()
        .flat_map(|&(i, j)| {
            [
                NliPair {
                    premise: texts[i],
                    hypothesis: texts[j],
                },
                NliPair {
                    premise: texts[j],
                    hypothesis: texts[i],
                },
            ]
        })
        .collect();
    let scores = score_checked(nli, ClusterClaims::ID, &pairs)?;
    // `as_chunks::<2>()` views the scores as `[NliScores; 2]` arrays, one per
    // candidate, matching the pairs pushed above; an array pattern then names
    // both directions. The weaker one is the pair's mutual entailment.
    let (both_ways, _) = scores.as_chunks::<2>();
    let equivalent: BTreeMap<(usize, usize), PerMille> = candidates
        .iter()
        .zip(both_ways)
        .map(|(&pair, [forward, backward])| {
            let forward = PerMille::from_probability(forward.entailment);
            let backward = PerMille::from_probability(backward.entailment);
            (pair, forward.min(backward))
        })
        .filter(|(_, mutual)| mutual.get() >= MERGE_ENTAIL_PM)
        .collect();
    let mutual = |a: usize, b: usize| equivalent.get(&(a.min(b), a.max(b))).copied();

    // Complete linkage: a claim joins a cluster only if it is equivalent to
    // *every* member, so A≈B and B≈C can't chain A to an unrelated C. Each
    // cluster is led by its lowest-id claim, whose wording it keeps.
    let mut assigned = vec![false; claims.len()];
    let mut merged = Vec::new();
    let mut merges = 0usize;
    for lead in 0..claims.len() {
        if assigned[lead] {
            continue;
        }
        assigned[lead] = true;
        let mut members = vec![lead];
        for (other, taken) in assigned.iter_mut().enumerate().skip(lead + 1) {
            if !*taken && members.iter().all(|&m| mutual(m, other).is_some()) {
                *taken = true;
                members.push(other);
            }
        }

        let mut claim = claims[lead].clone();
        for &other in &members[1..] {
            let entailment_pm = mutual(lead, other).expect("members are equivalent to the lead");
            for evidence in claims[other].evidence() {
                let mut evidence = evidence.clone();
                evidence.basis = Some(EvidenceBasis::Merged {
                    wording: texts[other].to_owned(),
                    entailment_pm,
                });
                claim.add_evidence(evidence);
            }
            merges += 1;
        }
        merged.push(claim);
    }
    tracing::info!(
        claims = claims.len(),
        pairs = pairs.len(),
        merges,
        "claims clustered"
    );
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{FakeEmbedding, FakeNli, NliScores};
    use podling_types::{ChunkId, ContentHash, Evidence, SourceId, Stance};
    use std::cell::Cell;

    fn claim(text: &str, group: &str) -> Claim {
        let hash = || ContentHash::of_parts(&[group.as_bytes()]);
        let mut claim = Claim::new(text);
        claim.add_evidence(Evidence {
            chunk: ChunkId::new(hash()),
            source: SourceId::new(hash()),
            independence_group: group.into(),
            stance: Stance::Supports,
            basis: None,
        });
        claim
    }

    fn run(claims: &[Claim], nli: &dyn NliProvider) -> Vec<Claim> {
        ClusterClaims {
            embedder: &FakeEmbedding,
            nli,
        }
        .run(&claims.to_vec())
        .unwrap()
    }

    const A: &str = "In June 1908 an explosion flattened about 80 million trees.";
    const B: &str = "About 80 million trees were flattened by an explosion in June 1908.";

    #[test]
    fn a_paraphrase_merges_into_the_lowest_id_wording() {
        let claims = run(&[claim(A, "eyewitness"), claim(B, "expedition")], &FakeNli);
        assert_eq!(claims.len(), 1);
        // Which wording leads depends only on the two texts' hashes.
        let (lead, other, other_group) = if Claim::id_for(A) < Claim::id_for(B) {
            (A, B, "expedition")
        } else {
            (B, A, "eyewitness")
        };
        assert_eq!(claims[0].text(), lead);
        for evidence in claims[0].evidence() {
            let expected =
                (evidence.independence_group == other_group).then(|| EvidenceBasis::Merged {
                    wording: other.into(),
                    entailment_pm: PerMille::new(PerMille::MAX).unwrap(),
                });
            assert_eq!(evidence.basis, expected);
        }
    }

    #[test]
    fn output_is_independent_of_input_order() {
        let c = claim("No impact crater was ever found at the site.", "expedition");
        let forward = run(&[claim(A, "a"), claim(B, "b"), c.clone()], &FakeNli);
        let backward = run(&[c, claim(B, "b"), claim(A, "a")], &FakeNli);
        assert_eq!(forward, backward);
        assert_eq!(forward.len(), 2);
    }

    /// Entails only the pairs listed, by `(premise, hypothesis)`, and counts
    /// the pairs it is asked about.
    struct Scripted(Vec<(&'static str, &'static str)>, Cell<usize>);
    impl NliProvider for Scripted {
        fn id(&self) -> &str {
            "scripted"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>> {
            self.1.set(self.1.get() + pairs.len());
            Ok(pairs
                .iter()
                .map(|p| {
                    let e = if self.0.contains(&(p.premise, p.hypothesis)) {
                        1.0
                    } else {
                        0.0
                    };
                    NliScores {
                        entailment: e,
                        neutral: 1.0 - e,
                        contradiction: 0.0,
                    }
                })
                .collect())
        }
    }

    #[test]
    fn entailment_one_way_does_not_merge() {
        let nli = Scripted(vec![(A, B)], Cell::new(0));
        let claims = run(&[claim(A, "a"), claim(B, "b")], &nli);
        assert_eq!(nli.1.get(), 2, "the pair is judged both ways");
        assert_eq!(claims.len(), 2);
    }

    #[test]
    fn similarity_alone_never_merges() {
        let nli = Scripted(vec![], Cell::new(0));
        assert_eq!(run(&[claim(A, "a"), claim(B, "b")], &nli).len(), 2);
    }

    // Three wordings that share most words, so each pair is a candidate.
    const X: &str = "Explosion flattened trees across the whole Tunguska forest region";
    const Y: &str = "Explosion flattened trees across the whole Tunguska forest area";
    const Z: &str = "Explosion flattened trees across the whole Tunguska forest zone";

    #[test]
    fn linkage_is_complete_not_chained() {
        // X≈Y and Y≈Z, but not X≈Z: no cluster may hold both X and Z.
        let both = |a, b| [(a, b), (b, a)];
        let script = [both(X, Y), both(Y, Z)].concat();
        let claims = run(
            &[claim(X, "a"), claim(Y, "b"), claim(Z, "c")],
            &Scripted(script, Cell::new(0)),
        );
        assert_eq!(claims.len(), 2);
        for c in &claims {
            let wordings: BTreeSet<&str> = c
                .evidence()
                .iter()
                .map(|e| match &e.basis {
                    Some(EvidenceBasis::Merged { wording, .. }) => wording.as_str(),
                    _ => c.text(),
                })
                .collect();
            assert!(
                !(wordings.contains(X) && wordings.contains(Z)),
                "{wordings:?}"
            );
        }
    }

    const Y1908: &str = "In June 1908 an explosion over Siberia flattened about 80 million trees.";
    const Y1907: &str = "In June 1907 an explosion over Siberia flattened about 80 million trees.";

    #[test]
    fn a_changed_year_is_never_merged() {
        let claims = [claim(Y1908, "a"), claim(Y1907, "b")];
        let vectors = FakeEmbedding.embed(&[Y1908, Y1907]).unwrap();
        let similarity = PerMille::from_probability(cosine(&vectors[0], &vectors[1]));
        assert!(
            similarity.get() >= MERGE_CANDIDATE_PM,
            "the near miss must be a merge candidate, or the test proves nothing"
        );

        // With the veto, the pair never reaches the model.
        let nli = Scripted(vec![], Cell::new(0));
        assert_eq!(
            cluster(&claims, &FakeEmbedding, &nli, NumberVeto::On)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(nli.1.get(), 0);

        // Without it, the NLI check alone keeps them apart.
        let merged = cluster(&claims, &FakeEmbedding, &FakeNli, NumberVeto::Off).unwrap();
        assert_eq!(merged.len(), 2);
    }
}
