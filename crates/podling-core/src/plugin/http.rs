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
//!
//! Local by default: a `base_url` whose host is not `localhost` or a loopback
//! or private IP address is refused unless its section declares
//! `data_policy = "zero_retention"`. Every HTTP provider is built here, so
//! no request reaches a hosted server the episode did not declare.

use std::fmt;
use std::io::Read;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::{Duration, Instant};

use podling_types::DataPolicy;

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
    /// The section's declared `data_policy`; a hosted `base_url` needs
    /// [`DataPolicy::ZeroRetention`]. `None` for a transport that must stay
    /// local, like the TTS sidecar's.
    pub data_policy: Option<DataPolicy>,
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
        let reach = Reach::of(&base_url);
        if reach == Reach::Hosted && config.data_policy != Some(DataPolicy::ZeroRetention) {
            return Err(config_error(format!(
                "{section}.base_url {base_url:?} is not a local server (host {:?}), and \
                 Podling sends source text only to localhost or a private address by \
                 default; if the endpoint neither trains on nor retains requests, declare \
                 {section}.data_policy = \"zero_retention\", otherwise use a local server",
                host(&base_url)
            )));
        }
        // Never the key, and the policy only as declared.
        tracing::info!(
            section,
            reach = reach.as_str(),
            data_policy = match config.data_policy {
                Some(DataPolicy::ZeroRetention) => "zero_retention",
                None => "none",
            },
            "HTTP provider endpoint"
        );
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
        // Wider than `Reach::Hosted`: a private LAN address is local for the
        // policy, but a key sent to it over plain http still crosses a network.
        if api_key.is_some() && !base_url.starts_with("https://") && !is_loopback(host(&base_url)) {
            tracing::warn!(
                %base_url,
                "the API key will be sent over plain http to a non-loopback host"
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
        self.request(path, Some(payload))
    }

    /// `GET {base_url}{path}`, with the same retries, size cap and redaction
    /// as [`Transport::post_json`].
    pub fn get_json(&self, path: &str) -> Result<Posted> {
        self.request(path, None)
    }

    /// A POST when there is a payload, a GET otherwise.
    fn request(&self, path: &str, payload: Option<&[u8]>) -> Result<Posted> {
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
    fn send_once(
        &self,
        endpoint: &str,
        payload: Option<&[u8]>,
    ) -> std::result::Result<String, Failure> {
        let bearer = self.api_key.as_ref().map(|key| format!("Bearer {}", key.0));
        let sent = match payload {
            Some(payload) => {
                let mut req = self.agent.post(endpoint).content_type("application/json");
                if let Some(bearer) = &bearer {
                    req = req.header("Authorization", bearer);
                }
                req.send(payload)
            }
            None => {
                let mut req = self.agent.get(endpoint);
                if let Some(bearer) = &bearer {
                    req = req.header("Authorization", bearer);
                }
                req.call()
            }
        };
        let mut response =
            sent.map_err(|err| Failure::fatal(self.describe_transport_error(&err)))?;

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

/// Where a `base_url` points: this machine or a private network, or anywhere
/// else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    Local,
    Hosted,
}

impl Reach {
    /// Local only for `localhost` and loopback or private IP literals
    /// (127.0.0.0/8, ::1, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16,
    /// fc00::/7). Any other name is hosted, even one that resolves to a
    /// private address (`localhost.example.com`, `127.0.0.1.nip.io`): a name
    /// can point anywhere, and the check never resolves it.
    pub(crate) fn of(base_url: &str) -> Self {
        let host = host(base_url);
        let local = if host.eq_ignore_ascii_case("localhost") {
            true
        } else if let Ok(v4) = host.parse::<Ipv4Addr>() {
            v4.is_loopback() || v4.is_private()
        } else if let Ok(v6) = host.parse::<Ipv6Addr>() {
            // fc00::/7 is the unique local range.
            v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00
        } else {
            false
        };
        if local { Self::Local } else { Self::Hosted }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Hosted => "hosted",
        }
    }
}

/// The host of a URL that [`validate_base_url`] accepted, without its port or
/// an IPv6 literal's brackets.
fn host(base_url: &str) -> &str {
    let rest = base_url.split("://").nth(1).unwrap_or_default();
    let authority = rest.split('/').next().unwrap_or_default();
    if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    }
}

/// Whether `host` (as [`host`] returns it) never leaves this machine:
/// `localhost` in any case, 127.0.0.0/8 or `::1`. Like [`Reach::of`], a name
/// is never resolved, so `localhost.example.com` is not loopback.
fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<Ipv4Addr>().is_ok_and(|v4| v4.is_loopback())
        || host.parse::<Ipv6Addr>().is_ok_and(|v6| v6.is_loopback())
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::*;

    fn config<'a>(
        section: &'static str,
        base_url: &'a str,
        data_policy: Option<DataPolicy>,
    ) -> TransportConfig<'a> {
        TransportConfig {
            section,
            plugin: "test",
            base_url,
            api_key_env: None,
            timeout_secs: None,
            data_policy,
        }
    }

    fn message(err: CoreError) -> String {
        match err {
            CoreError::Config { message } => message,
            other => panic!("expected a Config error, got {other:?}"),
        }
    }

    #[test]
    fn only_localhost_and_loopback_or_private_addresses_are_local() {
        for local in [
            "http://localhost:11434/v1",
            "http://127.5.0.1/v1",
            "http://[::1]:8080/v1",
            "http://10.0.0.5/v1",
            "http://192.168.1.2:11434/v1",
            "http://172.16.0.1/v1",
            "http://[fd00::1]/v1",
        ] {
            assert_eq!(Reach::of(local), Reach::Local, "{local}");
        }
        for hosted in [
            "http://localhost.example.com/v1",
            "http://127.0.0.1.nip.io/v1",
            "http://192.169.0.1/v1",
            "http://172.32.0.1/v1",
            "https://api.together.xyz/v1",
            "http://[2001:db8::1]/v1",
            "http://h/v1",
        ] {
            assert_eq!(Reach::of(hosted), Reach::Hosted, "{hosted}");
        }
    }

    #[test]
    fn only_localhost_and_loopback_addresses_are_loopback() {
        assert_eq!(host("http://[::1]:8080/v1"), "::1");
        for loopback in ["localhost", "LOCALHOST", "127.0.0.1", "127.5.0.1", "::1"] {
            assert!(is_loopback(loopback), "{loopback}");
        }
        // Private LAN addresses are local to `Reach`, but not loopback.
        for other in [
            "10.0.0.5",
            "172.16.0.1",
            "192.168.1.2",
            "fd00::1",
            "localhost.example.com",
            "127.0.0.1.nip.io",
            "api.together.xyz",
        ] {
            assert!(!is_loopback(other), "{other}");
        }
    }

    #[test]
    fn a_hosted_url_without_a_data_policy_is_refused_naming_the_section_and_the_fix() {
        let err = Transport::new(&config("llm", "https://api.together.xyz/v1", None), |_| {
            None
        })
        .unwrap_err();
        let message = message(err);
        assert!(message.contains("llm.base_url"), "{message}");
        assert!(message.contains("api.together.xyz"), "{message}");
        assert!(
            message.contains("llm.data_policy = \"zero_retention\""),
            "{message}"
        );
    }

    #[test]
    fn a_declared_hosted_url_and_an_undeclared_local_one_are_accepted() {
        let declared = config(
            "embedding",
            "https://api.together.xyz/v1",
            Some(DataPolicy::ZeroRetention),
        );
        assert!(Transport::new(&declared, |_| None).is_ok());
        let local = config("embedding", "http://localhost:11434/v1", None);
        assert!(Transport::new(&local, |_| None).is_ok());
    }

    #[test]
    fn a_sidecar_style_transport_with_no_policy_must_be_local() {
        let hosted = config("tts", "http://tts.example:9000/podling/v1", None);
        assert!(message(Transport::new(&hosted, |_| None).unwrap_err()).contains("tts.base_url"));
        let loopback = config("tts", "http://127.0.0.1:9000/podling/v1", None);
        assert!(Transport::new(&loopback, |_| None).is_ok());
    }

    #[test]
    fn credentials_in_the_base_url_are_rejected() {
        let err = Transport::new(
            &config(
                "llm",
                "https://user:pw@api.together.xyz/v1",
                Some(DataPolicy::ZeroRetention),
            ),
            |_| None,
        )
        .unwrap_err();
        assert!(message(err).contains("must not contain credentials"));
    }

    /// Answers every request with a redirect to itself and counts them.
    fn redirecting_server() -> (String, Arc<Mutex<usize>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(0));
        let counter = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                // Read the request head up to its blank line.
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                *counter.lock().unwrap() += 1;
                let reply = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{addr}/elsewhere\r\n\
                     Content-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (format!("http://{addr}/v1"), seen)
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let (base_url, seen) = redirecting_server();
        let transport = Transport::new(&config("llm", &base_url, None), |_| None).unwrap();
        let Err(err) = transport.get_json("/models") else {
            panic!("a 302 must not be followed to a 2xx");
        };
        assert!(
            matches!(
                err,
                CoreError::Provider {
                    kind: ProviderFailure::Http(302),
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(*seen.lock().unwrap(), 1);
    }
}
