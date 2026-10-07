//! Embeddings from any server that speaks the OpenAI embeddings protocol
//! (`POST {base_url}/embeddings`), such as Ollama with `nomic-embed-text`.
//!
//! Same [`Transport`] as the chat provider, so the same rules hold: the key
//! comes from the environment and is never logged, 429/5xx are retried,
//! redirects are refused and bodies are capped.

use std::time::{Duration, Instant};

use podling_types::{DataPolicy, EmbeddingConfig};
use serde::Deserialize;
use serde_json::{Value, json};

use super::embedding::EmbeddingProvider;
use super::http::{Transport, TransportConfig, config_error};
use super::ollama::OllamaUnload;
use crate::error::{ProviderFailure, Result};

/// Texts per request. Keeps each body well under the response cap (64
/// vectors of 768 floats is about 1 MiB of JSON) and a slow batch short.
pub const EMBED_BATCH: usize = 64;

/// Bump when the request changes shape; part of the fingerprint.
const REQUEST_VERSION: u32 = 1;

/// The `plugin` in errors; distinct from the chat provider's so the CLI can
/// tell which server to suggest checking.
pub const PLUGIN: &str = "open_ai_compat_embeddings";

#[derive(Debug)]
pub struct OpenAiEmbeddings {
    transport: Transport,
    model: String,
    /// The episode's declared policy, in the fingerprint when set.
    data_policy: Option<DataPolicy>,
    /// Set by `unload_after = true`.
    unload: Option<OllamaUnload>,
}

impl OpenAiEmbeddings {
    pub fn from_config(config: &EmbeddingConfig) -> Result<Self> {
        Self::from_config_with_env(config, |name| std::env::var(name).ok())
    }

    /// As [`from_config`](Self::from_config), with the environment lookup
    /// injected for tests.
    pub fn from_config_with_env(
        config: &EmbeddingConfig,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let EmbeddingConfig::OpenAiCompat {
            base_url,
            model,
            api_key_env,
            timeout_secs,
            unload_after,
            data_policy,
        } = config
        else {
            return Err(config_error(
                "not an open_ai_compat embedding configuration",
            ));
        };
        let transport = Transport::new(
            &TransportConfig {
                section: "embedding",
                plugin: PLUGIN,
                base_url,
                api_key_env: api_key_env.as_deref(),
                timeout_secs: *timeout_secs,
            },
            &env,
        )?;
        if model.trim().is_empty() {
            return Err(config_error("embedding.model must not be empty"));
        }
        let unload = unload_after
            .then(|| {
                OllamaUnload::new(
                    "embedding",
                    PLUGIN,
                    base_url,
                    model,
                    api_key_env.as_deref(),
                    &env,
                )
            })
            .transpose()?;
        Ok(Self {
            transport,
            model: model.clone(),
            data_policy: *data_policy,
            unload,
        })
    }

    /// Base delay before the first retry; tests set it to nearly nothing.
    pub fn with_retry_backoff(mut self, backoff: Duration) -> Self {
        self.transport.set_retry_backoff(backoff);
        if let Some(unload) = &mut self.unload {
            unload.set_retry_backoff(backoff);
        }
        self
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let payload = serde_json::to_vec(&json!({ "model": self.model, "input": texts }))?;
        let span = tracing::info_span!(
            "embedding_request",
            provider = PLUGIN,
            model = %self.model,
            count = texts.len(),
        );
        let _entered = span.enter();
        let started = Instant::now();

        let posted = self.transport.post_json("/embeddings", &payload)?;
        let vectors = parse_embeddings(&posted.body, texts.len())
            .map_err(|message| self.transport.error(ProviderFailure::Other, message))?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            attempts = posted.attempts,
            "embedding request done"
        );
        Ok(vectors)
    }
}

impl EmbeddingProvider for OpenAiEmbeddings {
    fn id(&self) -> &str {
        "open_ai_compat"
    }

    /// Never includes the key: rotating it must not invalidate the cache.
    /// A declared `data_policy` is included; an undeclared one adds nothing,
    /// so a local episode keeps its cache keys.
    fn fingerprint(&self) -> Value {
        let mut fingerprint = json!({
            "provider": "open_ai_compat",
            "base_url": self.transport.base_url(),
            "model": self.model,
            "request_version": REQUEST_VERSION,
        });
        if let Some(policy) = self.data_policy {
            fingerprint["data_policy"] = json!(policy);
        }
        fingerprint
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(EMBED_BATCH) {
            vectors.extend(self.embed_batch(batch)?);
        }
        Ok(vectors)
    }

    fn release(&self) -> Result<()> {
        self.unload.as_ref().map_or(Ok(()), OllamaUnload::release)
    }
}

#[derive(Deserialize)]
struct Response {
    data: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    index: usize,
    embedding: Vec<f32>,
}

/// Reads `data[*].embedding`, put back in request order by `index` (the
/// protocol doesn't promise response order). Anything but exactly one
/// non-empty, finite vector per input, all of one length, is an error.
fn parse_embeddings(raw: &str, expected: usize) -> std::result::Result<Vec<Vec<f32>>, String> {
    let response: Response = serde_json::from_str(raw)
        .map_err(|_| "the response is not an embeddings list (data[].embedding)".to_string())?;
    let mut slots: Vec<Option<Vec<f32>>> = vec![None; expected];
    for item in response.data {
        let slot = slots
            .get_mut(item.index)
            .ok_or_else(|| format!("the response has an embedding for index {}", item.index))?;
        if slot.replace(item.embedding).is_some() {
            return Err(format!("the response repeats index {}", item.index));
        }
    }
    let vectors: Vec<Vec<f32>> = slots
        .into_iter()
        .enumerate()
        .map(|(i, v)| v.ok_or_else(|| format!("the response has no embedding for index {i}")))
        .collect::<std::result::Result<_, _>>()?;
    let dims = vectors.first().map_or(0, Vec::len);
    if vectors.iter().any(|v| v.is_empty() || v.len() != dims) {
        return Err("the response has empty or different-length embeddings".into());
    }
    if vectors.iter().flatten().any(|x| !x.is_finite()) {
        return Err("the response has a non-finite embedding value".into());
    }
    Ok(vectors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;

    fn config(base_url: &str, model: &str) -> EmbeddingConfig {
        EmbeddingConfig::OpenAiCompat {
            base_url: base_url.into(),
            model: model.into(),
            api_key_env: Some("K".into()),
            timeout_secs: None,
            unload_after: false,
            data_policy: None,
        }
    }

    fn build(base_url: &str, model: &str) -> Result<OpenAiEmbeddings> {
        OpenAiEmbeddings::from_config_with_env(&config(base_url, model), |_| Some("sk-x".into()))
    }

    #[test]
    fn config_errors_name_the_embedding_section() {
        for (url, model) in [("ftp://h/v1", "m"), ("http://h/v1", " ")] {
            let Err(CoreError::Config { message }) = build(url, model) else {
                panic!("{url} {model:?} must be rejected");
            };
            assert!(message.starts_with("embedding."), "{message}");
        }
        let unset = OpenAiEmbeddings::from_config_with_env(&config("http://h/v1", "m"), |_| None)
            .unwrap_err();
        assert!(
            unset.to_string().contains("embedding.api_key_env"),
            "{unset}"
        );
    }

    #[test]
    fn fingerprint_has_the_model_but_never_the_key() {
        let fp = build("http://h/v1/", "nomic").unwrap().fingerprint();
        assert_eq!(fp["base_url"], "http://h/v1");
        assert_eq!(fp["model"], "nomic");
        assert!(!fp.to_string().contains("sk-x"));
    }

    #[test]
    fn a_declared_data_policy_is_in_the_fingerprint_and_the_key_never_is() {
        let mut cfg = config("http://h/v1/", "nomic");
        if let EmbeddingConfig::OpenAiCompat { data_policy, .. } = &mut cfg {
            *data_policy = Some(DataPolicy::ZeroRetention);
        }
        let declared = OpenAiEmbeddings::from_config_with_env(&cfg, |_| Some("sk-x".into()))
            .unwrap()
            .fingerprint();
        let undeclared = build("http://h/v1/", "nomic").unwrap().fingerprint();
        assert_eq!(declared["data_policy"], "zero_retention");
        assert!(undeclared.get("data_policy").is_none(), "{undeclared}");
        assert_ne!(declared, undeclared);
        assert!(!declared.to_string().contains("sk-x"), "{declared}");
    }

    #[test]
    fn parse_reorders_by_index() {
        let raw =
            r#"{"data":[{"index":1,"embedding":[0.0,1.0]},{"index":0,"embedding":[1.0,0.0]}]}"#;
        assert_eq!(
            parse_embeddings(raw, 2).unwrap(),
            vec![vec![1.0, 0.0], vec![0.0, 1.0]]
        );
    }

    #[test]
    fn parse_rejects_malformed_lists() {
        for raw in [
            r#"{"data":[{"index":0,"embedding":[1.0]}]}"#, // one for two
            r#"{"data":[{"index":0,"embedding":[1.0]},{"index":0,"embedding":[1.0]}]}"#, // repeated
            r#"{"data":[{"index":0,"embedding":[1.0]},{"index":2,"embedding":[1.0]}]}"#, // out of range
            r#"{"data":[{"index":0,"embedding":[1.0]},{"index":1,"embedding":[1.0,2.0]}]}"#, // ragged
            r#"{"data":[{"index":0,"embedding":[]},{"index":1,"embedding":[]}]}"#, // empty
            r#"{"error":"nope"}"#,
        ] {
            assert!(parse_embeddings(raw, 2).is_err(), "{raw}");
        }
    }
}
