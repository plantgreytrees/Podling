//! Unloading a model from Ollama, so the GPU is free for the next one.
//!
//! The OpenAI-compatible API has no "unload" call. Ollama's native API does:
//! `POST /api/generate` with no prompt and `keep_alive: 0` frees the model
//! straight away, and works for embedding-only models too (checked against
//! Ollama with `nomic-embed-text`, which answers `done_reason: "unload"`).

use podling_types::DataPolicy;
use serde_json::json;

use super::http::{Transport, TransportConfig, config_error};
use crate::error::Result;

/// Sends the unload request for one model. Built only when the episode sets
/// `unload_after = true`.
#[derive(Debug)]
pub struct OllamaUnload {
    /// Points at the server root: `base_url` without its `/v1`.
    transport: Transport,
    model: String,
}

impl OllamaUnload {
    /// `base_url` is the provider's OpenAI-compatible base, which for Ollama
    /// ends in `/v1`; anything else is a config error, since only Ollama
    /// understands the request.
    pub fn new(
        section: &'static str,
        plugin: &'static str,
        base_url: &str,
        model: &str,
        api_key_env: Option<&str>,
        // The section's own `data_policy`: the unload request goes to the same
        // server, so the same rule applies to it.
        data_policy: Option<DataPolicy>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let root = base_url
            .trim_end_matches('/')
            .strip_suffix("/v1")
            .ok_or_else(|| {
                config_error(format!(
                    "{section}.unload_after only works with Ollama, whose base_url ends in /v1 \
                     (e.g. http://localhost:11434/v1); got {base_url:?}"
                ))
            })?;
        let transport = Transport::new(
            &TransportConfig {
                section,
                plugin,
                base_url: root,
                api_key_env,
                timeout_secs: None,
                data_policy,
            },
            env,
        )?;
        Ok(Self {
            transport,
            model: model.to_owned(),
        })
    }

    pub fn set_retry_backoff(&mut self, backoff: std::time::Duration) {
        self.transport.set_retry_backoff(backoff);
    }

    pub fn release(&self) -> Result<()> {
        let payload = serde_json::to_vec(&json!({ "model": self.model, "keep_alive": 0 }))?;
        self.transport.post_json("/api/generate", &payload)?;
        tracing::info!(model = %self.model, "asked Ollama to unload the model");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use podling_types::{EmbeddingConfig, LlmConfig};
    use serde_json::Value;

    use crate::error::CoreError;
    use crate::plugin::{EmbeddingProvider, LlmProvider, OpenAiCompat, OpenAiEmbeddings};

    /// Request lines and JSON bodies the server saw.
    type Seen = Arc<Mutex<Vec<(String, Value)>>>;

    /// Answers every request with Ollama's unload reply until the test ends.
    fn ollama() -> (String, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let seen = Seen::default();
        let log = Arc::clone(&seen);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                log.lock().unwrap().push((
                    request_line.trim_end().to_owned(),
                    serde_json::from_slice(&body).unwrap(),
                ));
                let reply = r#"{"done":true,"done_reason":"unload"}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            }
        });
        (base_url, seen)
    }

    fn llm(base_url: &str, unload_after: bool) -> crate::error::Result<OpenAiCompat> {
        OpenAiCompat::from_config_with_env(
            &LlmConfig::OpenAiCompat {
                base_url: base_url.into(),
                model: "llama3.1:8b".into(),
                api_key_env: None,
                temperature: None,
                timeout_secs: None,
                max_output_tokens: None,
                unload_after,
                data_policy: None,
            },
            |_| None,
        )
    }

    fn embedder(base_url: &str, unload_after: bool) -> OpenAiEmbeddings {
        OpenAiEmbeddings::from_config_with_env(
            &EmbeddingConfig::OpenAiCompat {
                base_url: base_url.into(),
                model: "nomic-embed-text".into(),
                api_key_env: None,
                timeout_secs: None,
                unload_after,
                data_policy: None,
            },
            |_| None,
        )
        .unwrap()
    }

    #[test]
    fn a_flagged_provider_sends_one_unload_request() {
        let (base_url, seen) = ollama();
        llm(&base_url, true).unwrap().release().unwrap();
        embedder(&base_url, true).release().unwrap();

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "{seen:?}");
        for ((line, body), model) in seen.iter().zip(["llama3.1:8b", "nomic-embed-text"]) {
            assert!(line.starts_with("POST /api/generate "), "{line}");
            assert_eq!(
                body,
                &serde_json::json!({ "model": model, "keep_alive": 0 })
            );
        }
    }

    #[test]
    fn without_the_flag_release_sends_nothing() {
        let (base_url, seen) = ollama();
        llm(&base_url, false).unwrap().release().unwrap();
        embedder(&base_url, false).release().unwrap();
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn the_flag_needs_an_ollama_base_url() {
        let err = llm("http://localhost:8080", true).unwrap_err();
        let CoreError::Config { message } = err else {
            panic!("expected a Config error, got {err:?}");
        };
        assert!(message.contains("llm.unload_after"), "{message}");
        // Unflagged, any base URL is fine.
        assert!(llm("http://localhost:8080", false).is_ok());
    }

    #[test]
    fn the_unload_transport_follows_its_sections_data_policy() {
        use podling_types::DataPolicy;

        use super::OllamaUnload;

        let unload = |policy| {
            OllamaUnload::new(
                "llm",
                "open_ai_compat",
                "https://ollama.example/v1",
                "m",
                None,
                policy,
                |_| None,
            )
        };
        let Err(CoreError::Config { message }) = unload(None) else {
            panic!("a hosted unload URL with no policy must be refused");
        };
        assert!(message.contains("llm.data_policy"), "{message}");
        assert!(unload(Some(DataPolicy::ZeroRetention)).is_ok());
    }
}
