//! The HTTP transport shared by the OpenAI-compatible providers (chat
//! completions and embeddings): URL validation, the API key, retries, the
//! response size cap and redaction.
//!
//! Blocking HTTP via `ureq`: the pipeline calls a model one request at a
//! time, so nothing here needs an async runtime.
//!
//! The API key is read from the environment once, at construction. It is only
//! ever sent as an `Authorization: Bearer` header, and never appears in a
//! `Debug` print, an error, a log line or a fingerprint.

use std::fmt;
use std::io::Read;
use std::time::{Duration, Instant};

use crate::error::{CoreError, ProviderFailure, Result};

/// A response body larger than this is rejected while it is being read.
pub const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

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

/// What a provider's config section says about reaching its server.
pub(crate) struct TransportConfig<'a> {
    /// The episode-file section, for messages: `llm`, `embedding`.
    pub section: &'static str,
    /// The `plugin` named in a [`CoreError::Provider`].
    pub plugin: &'static str,
    pub base_url: &'a str,
    pub api_key_env: Option<&'a str>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct Transport {
    agent: ureq::Agent,
    plugin: &'static str,
    base_url: String,
    api_key: Option<ApiKey>,
    retry_backoff: Duration,
}

/// A 2xx body and how many requests it took.
pub(crate) struct Posted {
    pub body: String,
    pub attempts: u32,
}

impl Transport {
    /// Validates the config and reads the key through `env` (the process
    /// environment in production, a closure in tests).
    pub fn new(config: &TransportConfig<'_>, env: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let section = config.section;
        let base_url = validate_base_url(section, config.base_url)?;
        let timeout = match config.timeout_secs {
            Some(0) => {
                return Err(config_error(format!(
                    "{section}.timeout_secs must be at least 1"
                )));
            }
            Some(secs) => Duration::from_secs(secs),
            None => DEFAULT_TIMEOUT,
        };
        let api_key = config
            .api_key_env
            .map(|name| read_key(section, name, &env))
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
            plugin: config.plugin,
            base_url,
            api_key,
            retry_backoff: DEFAULT_RETRY_BACKOFF,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Base delay before the first retry; it doubles for the second.
    pub fn set_retry_backoff(&mut self, backoff: Duration) {
        self.retry_backoff = backoff;
    }

    /// `POST {base_url}{path}` with a JSON body, retrying a 429 or 5xx. Runs
    /// inside the caller's span, so its log lines say which request it was.
    pub fn post_json(&self, path: &str, payload: &[u8]) -> Result<Posted> {
        let endpoint = format!("{}{path}", self.base_url);
        let started = Instant::now();
        let mut attempt = 0;
        loop {
            match self.send_once(&endpoint, payload) {
                Ok(body) => {
                    return Ok(Posted {
                        body,
                        attempts: attempt + 1,
                    });
                }
                Err(failure) if failure.retryable && attempt < MAX_RETRIES => {
                    tracing::warn!(attempt, reason = %failure.message, "retrying request");
                    std::thread::sleep(self.retry_backoff * 2u32.pow(attempt));
                    attempt += 1;
                }
                Err(failure) => {
                    // Not a warning: the error below is the report, and the
                    // caller shows it. Logging it too would print it twice.
                    tracing::info!(
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        attempts = attempt + 1,
                        "request failed"
                    );
                    return Err(self.error(failure.kind, failure.message));
                }
            }
        }
    }

    /// One HTTP exchange. `Ok` is the raw 2xx body.
    fn send_once(&self, endpoint: &str, payload: &[u8]) -> std::result::Result<String, Failure> {
        let mut req = self.agent.post(endpoint).content_type("application/json");
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
            kind: ProviderFailure::Http(status),
            message,
        })
    }

    fn describe_transport_error(&self, err: &ureq::Error) -> (ProviderFailure, String) {
        match err {
            ureq::Error::Timeout(_) => (
                ProviderFailure::TimedOut,
                format!("request to {} timed out", self.base_url),
            ),
            ureq::Error::BodyExceedsLimit(_) => (
                ProviderFailure::Other,
                format!(
                    "response from {} is larger than {} MiB",
                    self.base_url,
                    MAX_RESPONSE_BYTES / (1024 * 1024)
                ),
            ),
            other => (
                ProviderFailure::Unreachable,
                self.redact(format!("request to {} failed: {other}", self.base_url)),
            ),
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
    pub fn redact(&self, text: String) -> String {
        match &self.api_key {
            Some(key) => text.replace(&key.0, "[redacted]"),
            None => text,
        }
    }

    pub fn error(&self, kind: ProviderFailure, message: impl Into<String>) -> CoreError {
        CoreError::Provider {
            plugin: self.plugin.into(),
            kind,
            message: message.into(),
        }
    }
}

/// A failed attempt; `retryable` is true for a 429 or 5xx.
struct Failure {
    retryable: bool,
    kind: ProviderFailure,
    message: String,
}

impl Failure {
    fn fatal((kind, message): (ProviderFailure, String)) -> Self {
        Self {
            retryable: false,
            kind,
            message,
        }
    }
}

pub(crate) fn config_error(message: impl Into<String>) -> CoreError {
    CoreError::Config {
        message: message.into(),
    }
}

fn read_key(section: &str, name: &str, env: &impl Fn(&str) -> Option<String>) -> Result<ApiKey> {
    let value = env(name).map(|v| v.trim().to_owned()).unwrap_or_default();
    if value.is_empty() {
        return Err(config_error(format!(
            "environment variable {name} (named by {section}.api_key_env) is not set or is empty"
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
/// rejected: an endpoint path is appended to it, and secrets belong in the
/// environment.
fn validate_base_url(section: &str, raw: &str) -> Result<String> {
    let url = raw.trim().trim_end_matches('/');
    let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    else {
        return Err(config_error(format!(
            "{section}.base_url must start with http:// or https://, got {raw:?}"
        )));
    };
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(config_error(format!(
            "{section}.base_url {raw:?} has no host"
        )));
    }
    if authority.contains('@') {
        return Err(config_error(format!(
            "{section}.base_url must not contain credentials; use {section}.api_key_env"
        )));
    }
    if url.contains(['?', '#']) || url.contains(char::is_whitespace) {
        return Err(config_error(format!(
            "{section}.base_url {raw:?} must not contain a query, a fragment or whitespace"
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
