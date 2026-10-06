//! Embedding providers: each text becomes a vector, and texts that mean
//! similar things get vectors that point in similar directions.
//!
//! Embeddings only ever *choose candidates* (which claims might be the same
//! fact, which source sentences might bear on a claim). They never decide
//! anything on their own: an NLI model judges every candidate pair.

use serde_json::{Value, json};

use crate::error::{CoreError, Result};
use crate::text::content_words;

/// A plugin that turns texts into vectors.
///
/// Like [`LlmProvider`](super::LlmProvider), it is used as a trait object
/// (`&dyn EmbeddingProvider`): the episode file picks the implementation at
/// run time, so the stages can't know the concrete type at compile time.
/// Every method takes `&self`, which is what makes the trait *object safe*
/// (usable behind `dyn`).
pub trait EmbeddingProvider {
    fn id(&self) -> &str;

    /// Everything that can change the vectors (model, server, version). Part
    /// of every cache key that depends on this provider.
    fn fingerprint(&self) -> Value;

    /// One vector per text, in the same order.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

    /// Frees whatever the model holds on the GPU; a no-op by default, as for
    /// [`LlmProvider::release`](super::LlmProvider::release).
    fn release(&self) -> Result<()> {
        Ok(())
    }
}

/// Calls `provider` and checks what came back: one vector per text, all of
/// one non-zero length, every value finite. A provider that breaks that is an
/// [`CoreError::InvalidProviderOutput`] of `stage`. No texts, no call.
pub fn embed_checked(
    provider: &dyn EmbeddingProvider,
    stage: &'static str,
    texts: &[&str],
) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let vectors = provider.embed(texts)?;
    let invalid = |message: String| CoreError::InvalidProviderOutput { stage, message };
    if vectors.len() != texts.len() {
        return Err(invalid(format!(
            "embedding provider {} returned {} vectors for {} texts",
            provider.id(),
            vectors.len(),
            texts.len()
        )));
    }
    let dims = vectors[0].len();
    if dims == 0 || vectors.iter().any(|v| v.len() != dims) {
        return Err(invalid(format!(
            "embedding provider {} returned vectors of different or zero length",
            provider.id()
        )));
    }
    if vectors.iter().flatten().any(|x| !x.is_finite()) {
        return Err(invalid(format!(
            "embedding provider {} returned a non-finite value",
            provider.id()
        )));
    }
    Ok(vectors)
}

/// Cosine similarity: 1 for vectors pointing the same way, 0 for unrelated
/// ones. 0 when either vector is all zeros or the lengths differ.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    let (na, nb) = (norm(a), norm(b));
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na * nb)
}

/// Length of [`FakeEmbedding`]'s vectors.
pub const FAKE_DIMENSIONS: usize = 256;

/// A deterministic, offline stand-in for an embedding model.
///
/// Each content word (see [`content_words`]) is hashed with BLAKE3 into one of
/// [`FAKE_DIMENSIONS`] slots; the counts are then scaled to length 1. So the
/// cosine of two texts measures how many content words they share, whatever
/// the word order: "an explosion flattened the trees" and "the trees were
/// flattened by an explosion" come out identical. BLAKE3 rather than std's
/// `DefaultHasher`, whose output may change between Rust releases.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeEmbedding;

impl FakeEmbedding {
    fn vector(text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; FAKE_DIMENSIONS];
        for word in content_words(text) {
            let hash = blake3::hash(word.as_bytes());
            let bytes = hash.as_bytes();
            let slot = usize::from(u16::from_le_bytes([bytes[0], bytes[1]])) % FAKE_DIMENSIONS;
            v[slot] += 1.0;
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            v.iter_mut().for_each(|x| *x /= norm);
        }
        v
    }
}

impl EmbeddingProvider for FakeEmbedding {
    fn id(&self) -> &str {
        "fake"
    }

    fn fingerprint(&self) -> Value {
        // Bump when the fake's behaviour changes.
        json!({ "provider": "fake", "version": 1 })
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| Self::vector(t)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cos(a: &str, b: &str) -> f32 {
        let v = FakeEmbedding.embed(&[a, b]).unwrap();
        cosine(&v[0], &v[1])
    }

    #[test]
    fn fake_embeddings_are_deterministic_and_order_blind() {
        let text = "In June 1908 an explosion flattened about 80 million trees.";
        assert_eq!(
            FakeEmbedding.embed(&[text]).unwrap(),
            FakeEmbedding.embed(&[text]).unwrap()
        );
        let paraphrase = "About 80 million trees were flattened by an explosion in June 1908.";
        assert!(cos(text, paraphrase) >= 0.95, "{}", cos(text, paraphrase));
        assert!(cos(text, "No impact crater was found.") < 0.3);
    }

    #[test]
    fn cosine_handles_degenerate_vectors() {
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 0.0]), 0.0);
        assert!((cosine(&[1.0, 1.0], &[2.0, 2.0]) - 1.0).abs() < 1e-6);
    }

    struct Broken(Vec<Vec<f32>>);
    impl EmbeddingProvider for Broken {
        fn id(&self) -> &str {
            "broken"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn embed(&self, _: &[&str]) -> Result<Vec<Vec<f32>>> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn embed_checked_rejects_malformed_output() {
        for vectors in [
            vec![vec![1.0]],                 // one vector for two texts
            vec![vec![1.0], vec![1.0, 2.0]], // ragged
            vec![vec![], vec![]],            // empty
            vec![vec![1.0], vec![f32::NAN]], // not finite
        ] {
            let err = embed_checked(&Broken(vectors), "s", &["a", "b"]).unwrap_err();
            assert!(
                matches!(err, CoreError::InvalidProviderOutput { stage: "s", .. }),
                "{err}"
            );
        }
        assert!(embed_checked(&Broken(vec![]), "s", &[]).unwrap().is_empty());
    }
}
