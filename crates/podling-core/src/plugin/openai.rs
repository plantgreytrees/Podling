//! A provider for any server that speaks the OpenAI chat-completions protocol
//! (`POST {base_url}/chat/completions`): Ollama, llama.cpp `llama-server`,
//! vLLM, LM Studio, OpenAI itself.
//!
//! Blocking HTTP via `ureq`: the pipeline calls the model one chunk at a time,
//! so nothing here needs an async runtime.
//!
//! The API key is read from the environment once, at construction. It is only
//! ever sent as an `Authorization: Bearer` header, and never appears in a
//! `Debug` print, an error, a log line or the [`fingerprint`].
//!
//! [`fingerprint`]: LlmProvider::fingerprint

use std::fmt;
use std::io::Read;
use std::time::{Duration, Instant};

use podling_types::LlmConfig;
use serde_json::{Value, json};

use super::llm::{Completion, CompletionRequest, LlmProvider};
use crate::error::{CoreError, Result};

/// Bump when the way a request is built changes (message layout,
/// `response_format`, …). Part of the fingerprint, so it invalidates the cache.
pub const PROMPT_VERSION: u32 = 1;

/// A response body larger than this is rejected while it is being read.
pub const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

const PLUGIN: &str = "open_ai_compat";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_RETRY_BACKOFF: Duration = Duration::from_millis(500);
/// A 429 or 5xx is retried this many times (so at most three requests).
const MAX_RETRIES: u32 = 2;
/// How much of an error response body is quoted in an error message.
const ERROR_EXCERPT_BYTES: u64 = 512;

/// The API key. Its `Debug` output is fixed so that a stray `{:?}` on any
/// struct holding it cannot leak it.
struct ApiKey(String);

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Debug)]
pub struct OpenAiCompat {
    agent: ureq::Agent,
    /// `{base_url}/chat/completions`
    endpoint: String,
    base_url: String,
    model: String,
    api_key: Option<ApiKey>,
    temperature: Option<f32>,
    max_output_tokens: Option<u32>,
    retry_backoff: Duration,
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

        let base_url = validate_base_url(base_url)?;
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
        let timeout = match timeout_secs {
            Some(0) => return Err(config_error("llm.timeout_secs must be at least 1")),
            Some(secs) => Duration::from_secs(*secs),
            None => DEFAULT_TIMEOUT,
        };
        let api_key = api_key_env
            .as_deref()
            .map(|name| read_key(name, &env))
            .transpose()?;
        if api_key.is_some() && !base_url.starts_with("https://") && !is_loopback(&base_url) {
            tracing::warn!(
                %base_url,
                "the API key will be sent over plain http to a non-local host"
            );
        }

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            // Errors are classified from the status code, with the body.
            .http_status_as_error(false)
            // A redirect could carry the Authorization header to another host.
            .max_redirects(0)
            .build()
            .into();

        Ok(Self {
            agent,
            endpoint: format!("{base_url}/chat/completions"),
            base_url,
            model: model.clone(),
            api_key,
            temperature: *temperature,
            max_output_tokens: *max_output_tokens,
            retry_backoff: DEFAULT_RETRY_BACKOFF,
        })
    }

    /// Base delay before the first retry; it doubles for the second. Tests
    /// set this to nearly nothing.
    pub fn with_retry_backoff(mut self, backoff: Duration) -> Self {
        self.retry_backoff = backoff;
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

    /// One HTTP exchange. `Ok` is the raw 2xx body.
    fn send_once(&self, payload: &[u8]) -> std::result::Result<String, Failure> {
        let mut req = self
            .agent
            .post(&self.endpoint)
            .content_type("application/json");
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key.0));
        }
        let mut response = req
            .send(payload)
            .map_err(|err| Failure::fatal(self.describe_transport_error(&err)))?;

        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return response
                .body_mut()
                .with_config()
                .limit(MAX_RESPONSE_BYTES)
                .read_to_string()
                .map_err(|err| Failure::fatal(self.describe_transport_error(&err)));
        }

        let mut excerpt = Vec::new();
        // Best effort: the status line already says what went wrong.
        let _ = response
            .body_mut()
            .as_reader()
            .take(ERROR_EXCERPT_BYTES)
            .read_to_end(&mut excerpt);
        let message = format!(
            "{} answered HTTP {status}{}",
            self.base_url,
            self.excerpt_suffix(&excerpt)
        );
        Err(Failure {
            retryable: status == 429 || status >= 500,
            message,
        })
    }

    fn describe_transport_error(&self, err: &ureq::Error) -> String {
        match err {
            ureq::Error::Timeout(_) => format!("request to {} timed out", self.base_url),
            ureq::Error::BodyExceedsLimit(_) => format!(
                "response from {} is larger than {} MiB",
                self.base_url,
                MAX_RESPONSE_BYTES / (1024 * 1024)
            ),
            other => self.redact(format!("request to {} failed: {other}", self.base_url)),
        }
    }

    /// `: <body excerpt>` for an error message, or nothing when the body is empty.
    fn excerpt_suffix(&self, raw: &[u8]) -> String {
        let text = String::from_utf8_lossy(raw);
        let flat = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace(|c: char| c.is_control(), "");
        if flat.is_empty() {
            String::new()
        } else {
            format!(": {}", self.redact(flat))
        }
    }

    /// Servers sometimes echo the offending key in a 401 body.
    fn redact(&self, text: String) -> String {
        match &self.api_key {
            Some(key) => text.replace(&key.0, "[redacted]"),
            None => text,
        }
    }

    fn error(&self, message: impl Into<String>) -> CoreError {
        CoreError::Provider {
            plugin: PLUGIN.into(),
            message: message.into(),
        }
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
            "base_url": self.base_url,
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

        let mut attempt = 0;
        let raw = loop {
            match self.send_once(&payload) {
                Ok(raw) => break raw,
                Err(failure) if failure.retryable && attempt < MAX_RETRIES => {
                    tracing::warn!(attempt, reason = %failure.message, "retrying llm request");
                    std::thread::sleep(self.retry_backoff * 2u32.pow(attempt));
                    attempt += 1;
                }
                Err(failure) => {
                    // Not a warning: the error below is the report, and the
                    // caller shows it. Logging it too would print it twice.
                    tracing::info!(
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        attempts = attempt + 1,
                        "llm request failed"
                    );
                    return Err(self.error(failure.message));
                }
            }
        };

        let reply = parse_reply(&raw).map_err(|message| self.error(message))?;
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            attempts = attempt + 1,
            prompt_tokens = reply.prompt_tokens,
            completion_tokens = reply.completion_tokens,
            "llm request done"
        );
        Ok(Completion {
            text: reply.content,
        })
    }
}

/// A failed attempt; `retryable` is true for a 429 or 5xx.
struct Failure {
    retryable: bool,
    message: String,
}

impl Failure {
    fn fatal(message: String) -> Self {
        Self {
            retryable: false,
            message,
        }
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

fn config_error(message: impl Into<String>) -> CoreError {
    CoreError::Config {
        message: message.into(),
    }
}

fn read_key(name: &str, env: &impl Fn(&str) -> Option<String>) -> Result<ApiKey> {
    let value = env(name).map(|v| v.trim().to_owned()).unwrap_or_default();
    if value.is_empty() {
        return Err(config_error(format!(
            "environment variable {name} (named by llm.api_key_env) is not set or is empty"
        )));
    }
    // A control character would be a header-injection vector.
    if value.chars().any(char::is_control) {
        return Err(config_error(format!(
            "environment variable {name} holds a value that cannot be an API key"
        )));
    }
    Ok(ApiKey(value))
}

/// Accepts `http://host[:port][/path]` and `https://…`, and returns it without
/// a trailing slash. A URL that carries credentials, a query or a fragment is
/// rejected: `/chat/completions` is appended to it, and secrets belong in the
/// environment.
fn validate_base_url(raw: &str) -> Result<String> {
    let url = raw.trim().trim_end_matches('/');
    let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    else {
        return Err(config_error(format!(
            "llm.base_url must start with http:// or https://, got {raw:?}"
        )));
    };
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(config_error(format!("llm.base_url {raw:?} has no host")));
    }
    if authority.contains('@') {
        return Err(config_error(
            "llm.base_url must not contain credentials; use llm.api_key_env",
        ));
    }
    if url.contains(['?', '#']) || url.contains(char::is_whitespace) {
        return Err(config_error(format!(
            "llm.base_url {raw:?} must not contain a query, a fragment or whitespace"
        )));
    }
    Ok(url.to_owned())
}

fn is_loopback(base_url: &str) -> bool {
    let rest = base_url.split("://").nth(1).unwrap_or_default();
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!provider.redact(format!("bad key {key}")).contains(key));
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
