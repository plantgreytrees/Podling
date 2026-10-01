//! A provider for any server that speaks the OpenAI chat-completions protocol
//! (`POST {base_url}/chat/completions`): Ollama, llama.cpp `llama-server`,
//! vLLM, LM Studio, OpenAI itself.
//!
//! The HTTP side (key, retries, size cap, redaction) is the shared
//! [`Transport`]; this file builds the request and reads the reply. The key
//! never appears in a `Debug` print, an error, a log line or the
//! [`fingerprint`].
//!
//! [`fingerprint`]: LlmProvider::fingerprint

use std::time::{Duration, Instant};

use podling_types::LlmConfig;
use serde_json::{Value, json};

use super::http::{Transport, TransportConfig, config_error};
use super::llm::{Completion, CompletionRequest, LlmProvider};
use crate::error::{ProviderFailure, Result};

pub use super::http::MAX_RESPONSE_BYTES;

/// Bump when the way a request is built changes (message layout,
/// `response_format`, …). Part of the fingerprint, so it invalidates the cache.
pub const PROMPT_VERSION: u32 = 1;

const PLUGIN: &str = "open_ai_compat";

#[derive(Debug)]
pub struct OpenAiCompat {
    transport: Transport,
    model: String,
    temperature: Option<f32>,
    max_output_tokens: Option<u32>,
}

impl OpenAiCompat {
    /// Builds the provider from an episode's `open_ai_compat` config, reading
    /// the key from the process environment.
    pub fn from_config(config: &LlmConfig) -> Result<Self> {
        Self::from_config_with_env(config, |name| std::env::var(name).ok())
    }

    /// As [`from_config`](Self::from_config), with the environment lookup
    /// injected. Mutating the real environment is `unsafe` in Rust 2024 (and
    /// this workspace forbids `unsafe`), so tests pass a closure instead.
    pub fn from_config_with_env(
        config: &LlmConfig,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let LlmConfig::OpenAiCompat {
            base_url,
            model,
            api_key_env,
            temperature,
            timeout_secs,
            max_output_tokens,
        } = config
        else {
            return Err(config_error("not an open_ai_compat configuration"));
        };

        let transport = Transport::new(
            &TransportConfig {
                section: "llm",
                plugin: PLUGIN,
                base_url,
                api_key_env: api_key_env.as_deref(),
                timeout_secs: *timeout_secs,
            },
            env,
        )?;
        if model.trim().is_empty() {
            return Err(config_error("llm.model must not be empty"));
        }
        if let Some(t) = temperature
            && !(t.is_finite() && *t >= 0.0)
        {
            return Err(config_error(
                "llm.temperature must be a finite number of at least 0",
            ));
        }

        Ok(Self {
            transport,
            model: model.clone(),
            temperature: *temperature,
            max_output_tokens: *max_output_tokens,
        })
    }

    /// Base delay before the first retry; it doubles for the second. Tests
    /// set this to nearly nothing.
    pub fn with_retry_backoff(mut self, backoff: Duration) -> Self {
        self.transport.set_retry_backoff(backoff);
        self
    }

    fn request_body(&self, request: &CompletionRequest) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": request.instructions },
                { "role": "user", "content": request.input.to_string() },
            ],
            "response_format": { "type": "json_object" },
        });
        if let Some(t) = self.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(n) = self.max_output_tokens {
            body["max_tokens"] = json!(n);
        }
        body
    }
}

impl LlmProvider for OpenAiCompat {
    fn id(&self) -> &str {
        PLUGIN
    }

    /// Never includes the key: rotating it must not invalidate the cache.
    fn fingerprint(&self) -> Value {
        json!({
            "provider": PLUGIN,
            "base_url": self.transport.base_url(),
            "model": self.model,
            "temperature": self.temperature,
            "max_output_tokens": self.max_output_tokens,
            "prompt_version": PROMPT_VERSION,
        })
    }

    fn complete(&self, request: &CompletionRequest) -> Result<Completion> {
        let payload = serde_json::to_vec(&self.request_body(request))?;
        let span = tracing::info_span!(
            "llm_request",
            provider = PLUGIN,
            model = %self.model,
            task = ?request.task,
        );
        let _entered = span.enter();
        let started = Instant::now();

        let posted = self.transport.post_json("/chat/completions", &payload)?;
        let reply = parse_reply(&posted.body)
            .map_err(|message| self.transport.error(ProviderFailure::Other, message))?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            attempts = posted.attempts,
            prompt_tokens = reply.prompt_tokens,
            completion_tokens = reply.completion_tokens,
            "llm request done"
        );
        Ok(Completion {
            text: reply.content,
        })
    }
}

struct Reply {
    content: String,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

/// Extracts `choices[0].message.content` and the token usage.
fn parse_reply(raw: &str) -> std::result::Result<Reply, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|_| "the response body is not valid JSON".to_string())?;
    let choice = &value["choices"][0];
    let content = choice["message"]["content"]
        .as_str()
        .ok_or("the response has no choices[0].message.content text")?;
    if choice["finish_reason"] == "length" {
        return Err(
            "the model's output was cut off at the token limit; raise llm.max_output_tokens"
                .to_string(),
        );
    }
    Ok(Reply {
        content: content.to_owned(),
        prompt_tokens: value["usage"]["prompt_tokens"].as_u64(),
        completion_tokens: value["usage"]["completion_tokens"].as_u64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;

    fn config(base_url: &str, api_key_env: Option<&str>) -> LlmConfig {
        LlmConfig::OpenAiCompat {
            base_url: base_url.into(),
            model: "m".into(),
            api_key_env: api_key_env.map(Into::into),
            temperature: Some(0.2),
            timeout_secs: None,
            max_output_tokens: Some(512),
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn message(err: CoreError) -> String {
        match err {
            CoreError::Config { message } => message,
            other => panic!("expected a Config error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_a_base_url_that_is_not_http_or_https() {
        for bad in [
            "ftp://host/v1",
            "localhost:11434/v1",
            "file:///etc/passwd",
            "",
        ] {
            let err = OpenAiCompat::from_config_with_env(&config(bad, None), no_env).unwrap_err();
            assert!(message(err).contains("http://"), "{bad}");
        }
    }

    #[test]
    fn rejects_credentials_queries_and_missing_hosts_in_the_url() {
        for bad in [
            "http://user:pw@host/v1",
            "https://host/v1?key=1",
            "https://host/v1#frag",
            "http:///v1",
        ] {
            assert!(
                OpenAiCompat::from_config_with_env(&config(bad, None), no_env).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_named_but_unset_or_empty_key_variable_fails_closed() {
        let unset =
            OpenAiCompat::from_config_with_env(&config("http://h/v1", Some("MY_KEY")), no_env)
                .unwrap_err();
        assert!(message(unset).contains("MY_KEY"));

        let empty =
            OpenAiCompat::from_config_with_env(&config("http://h/v1", Some("MY_KEY")), |_| {
                Some("  ".into())
            })
            .unwrap_err();
        assert!(message(empty).contains("MY_KEY"));
    }

    #[test]
    fn no_key_variable_means_no_key_and_is_fine() {
        assert!(OpenAiCompat::from_config_with_env(&config("http://h/v1/", None), no_env).is_ok());
    }

    #[test]
    fn debug_and_fingerprint_never_contain_the_key() {
        let key = "sk-super-secret-value";
        let provider = OpenAiCompat::from_config_with_env(
            &config("https://api.example/v1", Some("MY_KEY")),
            |_| Some(key.into()),
        )
        .unwrap();
        let debug = format!("{provider:?}");
        assert!(!debug.contains(key), "{debug}");
        assert!(debug.contains("[redacted]"), "{debug}");
        assert!(!provider.fingerprint().to_string().contains(key));
        assert!(
            !provider
                .transport
                .redact(format!("bad key {key}"))
                .contains(key)
        );
    }

    #[test]
    fn fingerprint_changes_with_model_but_not_with_the_key() {
        let build = |model: &str, key: &str| {
            let mut cfg = config("https://api.example/v1", Some("K"));
            if let LlmConfig::OpenAiCompat { model: m, .. } = &mut cfg {
                *m = model.into();
            }
            OpenAiCompat::from_config_with_env(&cfg, |_| Some(key.into()))
                .unwrap()
                .fingerprint()
        };
        assert_eq!(build("a", "key-1"), build("a", "key-2"));
        assert_ne!(build("a", "key-1"), build("b", "key-1"));
    }

    #[test]
    fn rejects_bad_numbers() {
        let mut cfg = config("http://h/v1", None);
        if let LlmConfig::OpenAiCompat { temperature, .. } = &mut cfg {
            *temperature = Some(f32::NAN);
        }
        assert!(OpenAiCompat::from_config_with_env(&cfg, no_env).is_err());
        if let LlmConfig::OpenAiCompat {
            temperature,
            timeout_secs,
            ..
        } = &mut cfg
        {
            *temperature = None;
            *timeout_secs = Some(0);
        }
        assert!(OpenAiCompat::from_config_with_env(&cfg, no_env).is_err());
    }

    #[test]
    fn parse_reply_reads_content_and_usage() {
        let reply = parse_reply(
            r#"{"choices":[{"message":{"content":"{}"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":7,"completion_tokens":2}}"#,
        )
        .unwrap();
        assert_eq!(reply.content, "{}");
        assert_eq!(reply.prompt_tokens, Some(7));
        assert!(parse_reply("nope").is_err());
        assert!(parse_reply(r#"{"choices":[]}"#).is_err());
        assert!(
            parse_reply(r#"{"choices":[{"message":{"content":"{"},"finish_reason":"length"}]}"#)
                .is_err()
        );
    }
}
