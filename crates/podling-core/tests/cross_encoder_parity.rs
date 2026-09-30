//! The candle cross-encoder against Hugging Face transformers.
//!
//! `tests/fixtures/nli/reference_logits.json` holds the probabilities that
//! `scripts/nli_reference.py` got from transformers for a few fixed pairs.
//! This test needs the model weights, so it is ignored by default:
//!
//!   hf download cross-encoder/nli-deberta-v3-base --local-dir <dir>
//!   PODLING_NLI_MODEL_DIR=<dir> cargo test -p podling-core -- --ignored parity

use std::path::{Path, PathBuf};
use std::time::Instant;

use podling_core::plugin::cross_encoder::CrossEncoder;
use serde::Deserialize;

/// Largest difference allowed between a candle and a transformers probability.
const TOLERANCE: f32 = 0.001;

#[derive(Deserialize)]
struct Reference {
    pairs: Vec<Pair>,
}

#[derive(Deserialize)]
struct Pair {
    premise: String,
    hypothesis: String,
    entailment: f32,
    neutral: f32,
    contradiction: f32,
}

fn reference() -> Reference {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nli/reference_logits.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn argmax(scores: [f32; 3]) -> usize {
    (0..3)
        .max_by(|&a, &b| scores[a].total_cmp(&scores[b]))
        .unwrap_or(0)
}

#[test]
#[ignore = "needs the NLI model; set PODLING_NLI_MODEL_DIR"]
fn parity_with_transformers() {
    let Ok(dir) = std::env::var("PODLING_NLI_MODEL_DIR") else {
        // Fail, don't skip: an ignored test that returns early reports "ok".
        panic!("set PODLING_NLI_MODEL_DIR to a local cross-encoder/nli-deberta-v3-base snapshot");
    };
    let encoder = CrossEncoder::load(&PathBuf::from(dir)).unwrap();
    let reference = reference();
    let pairs: Vec<(&str, &str)> = reference
        .pairs
        .iter()
        .map(|p| (p.premise.as_str(), p.hypothesis.as_str()))
        .collect();

    // Once as one padded batch, then pair by pair: padding must change nothing.
    let started = Instant::now();
    let batched = encoder.scores(&pairs).unwrap();
    let elapsed = started.elapsed();
    eprintln!(
        "{} pairs in {elapsed:?} ({:?} per pair)",
        pairs.len(),
        elapsed / pairs.len() as u32
    );
    let mut worst = 0f32;
    for (i, pair) in pairs.iter().enumerate() {
        let alone = encoder.scores(&[*pair]).unwrap()[0];
        for (got, reference) in [
            (batched[i], &reference.pairs[i]),
            (alone, &reference.pairs[i]),
        ] {
            let got = [got.entailment, got.neutral, got.contradiction];
            let want = [
                reference.entailment,
                reference.neutral,
                reference.contradiction,
            ];
            for (g, w) in got.iter().zip(want) {
                worst = worst.max((g - w).abs());
                assert!(
                    (g - w).abs() <= TOLERANCE,
                    "pair {i} ({:?}): got {got:?}, transformers {want:?}",
                    reference.hypothesis
                );
            }
            assert_eq!(argmax(got), argmax(want), "pair {i}");
        }
    }
    eprintln!("largest difference from transformers: {worst:.6}");
}
