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

use podling_types::{DataPolicy, LlmConfig};
use serde_json::{Value, json};

use super::http::{Transport, TransportConfig, config_error};
use super::llm::{Completion, CompletionRequest, LlmProvider};
use super::ollama::OllamaUnload;
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
    /// The episode's declared policy, in the fingerprint when set.
    data_policy: Option<DataPolicy>,
    /// Set by `unload_after = true`.
    unload: Option<OllamaUnload>,
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
            unload_after,
            data_policy,
        } = config
        else {
            return Err(config_error("not an open_ai_compat configuration"));
        };

        // `&env`: a reference to a closure is itself callable, so both
        // transports can borrow the one lookup instead of each taking it.
        let transport = Transport::new(
            &TransportConfig {
                section: "llm",
                plugin: PLUGIN,
                base_url,
                api_key_env: api_key_env.as_deref(),
                timeout_secs: *timeout_secs,
            },
            &env,
        )?;
        let unload = unload_after
            .then(|| {
                OllamaUnload::new("llm", PLUGIN, base_url, model, api_key_env.as_deref(), &env)
            })
            .transpose()?;
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
            data_policy: *data_policy,
            unload,
        })
    }

    /// Base delay before the first retry; it doubles for the second. Tests
    /// set this to nearly nothing.
    pub fn with_retry_backoff(mut self, backoff: Duration) -> Self {
        self.transport.set_retry_backoff(backoff);
        if let Some(unload) = &mut self.unload {
            unload.set_retry_backoff(backoff);
        }
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
        if let Some(cap) = self.output_cap(request) {
            body["max_tokens"] = json!(cap.tokens());
        }
        body
    }

    /// The lower of the episode's cap and the request's, whichever are set.
    /// On a tie the stage's cap is the one named: raising the episode's would
    /// change nothing.
    fn output_cap(&self, request: &CompletionRequest) -> Option<OutputCap> {
        match (self.max_output_tokens, request.max_tokens) {
            (Some(episode), Some(stage)) if episode < stage => Some(OutputCap::Episode(episode)),
            (_, Some(stage)) => Some(OutputCap::Stage(stage)),
            (Some(episode), None) => Some(OutputCap::Episode(episode)),
            (None, None) => None,
        }
    }
}

/// Which setting limited a reply's length, so a cut-off reply can name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputCap {
    /// `llm.max_output_tokens` in the episode.
    Episode(u32),
    /// The stage's own `CompletionRequest::max_tokens`, which no setting raises.
    Stage(u32),
}

impl OutputCap {
    fn tokens(self) -> u32 {
        match self {
            Self::Episode(n) | Self::Stage(n) => n,
        }
    }
}

/// The message for a reply that stopped at the token limit.
fn cut_off_message(cap: Option<OutputCap>, completion_tokens: Option<u64>) -> String {
    let used = completion_tokens.map_or("?".into(), |n| n.to_string());
    match cap {
        Some(OutputCap::Stage(n)) => format!(
            "the model's output was cut off at this stage's cap of {n} tokens ({used} generated)"
        ),
        Some(OutputCap::Episode(n)) => format!(
            "the model's output was cut off at llm.max_output_tokens = {n} ({used} generated); \
             raise it if replies need more room"
        ),
        None => {
            format!("the model's output was cut off at the server's token limit ({used} tokens)")
        }
    }
}

impl LlmProvider for OpenAiCompat {
    fn id(&self) -> &str {
        PLUGIN
    }

    /// Never includes the key: rotating it must not invalidate the cache.
    /// A declared `data_policy` is included; an undeclared one adds nothing,
    /// so a local episode keeps its cache keys.
    fn fingerprint(&self) -> Value {
        let mut fingerprint = json!({
            "provider": PLUGIN,
            "base_url": self.transport.base_url(),
            "model": self.model,
            "temperature": self.temperature,
            "max_output_tokens": self.max_output_tokens,
            "prompt_version": PROMPT_VERSION,
        });
        if let Some(policy) = self.data_policy {
            fingerprint["data_policy"] = json!(policy);
        }
        fingerprint
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
        if reply.cut_off {
            tracing::warn!(
                elapsed_ms = started.elapsed().as_millis() as u64,
                attempts = posted.attempts,
                prompt_tokens = reply.prompt_tokens,
                completion_tokens = reply.completion_tokens,
                "llm reply cut off at the token limit"
            );
            return Err(self.transport.error(
                ProviderFailure::CutOff,
                cut_off_message(self.output_cap(request), reply.completion_tokens),
            ));
        }
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

    fn release(&self) -> Result<()> {
        self.unload.as_ref().map_or(Ok(()), OllamaUnload::release)
    }
}

struct Reply {
    content: String,
    /// `finish_reason` was `length`: the reply stopped at the token limit.
    cut_off: bool,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

/// Extracts `choices[0].message.content`, whether it was cut off, and the
/// token usage.
fn parse_reply(raw: &str) -> std::result::Result<Reply, String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|_| "the response body is not valid JSON".to_string())?;
    let choice = &value["choices"][0];
    let content = choice["message"]["content"]
        .as_str()
        .ok_or("the response has no choices[0].message.content text")?;
    Ok(Reply {
        content: content.to_owned(),
        cut_off: choice["finish_reason"] == "length",
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
            unload_after: false,
            data_policy: None,
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
    fn a_declared_data_policy_is_in_the_fingerprint_and_the_key_never_is() {
        let key = "sk-super-secret-value";
        let build = |policy: Option<DataPolicy>| {
            let mut cfg = config("https://api.example/v1", Some("K"));
            if let LlmConfig::OpenAiCompat { data_policy, .. } = &mut cfg {
                *data_policy = policy;
            }
            OpenAiCompat::from_config_with_env(&cfg, |_| Some(key.into()))
                .unwrap()
                .fingerprint()
        };
        let declared = build(Some(DataPolicy::ZeroRetention));
        let undeclared = build(None);
        assert_eq!(declared["data_policy"], "zero_retention");
        assert!(undeclared.get("data_policy").is_none(), "{undeclared}");
        assert_ne!(declared, undeclared);
        for fingerprint in [declared, undeclared] {
            assert!(!fingerprint.to_string().contains(key), "{fingerprint}");
        }
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
    fn the_lower_of_the_episode_and_request_token_caps_is_sent() {
        use crate::plugin::LlmTask;
        let with_cap = |episode: Option<u32>| {
            let mut cfg = config("http://h/v1", None);
            if let LlmConfig::OpenAiCompat {
                max_output_tokens, ..
            } = &mut cfg
            {
                *max_output_tokens = episode;
            }
            OpenAiCompat::from_config_with_env(&cfg, no_env).unwrap()
        };
        let request = |cap: Option<u32>| CompletionRequest {
            task: LlmTask::AdjudicateClaim,
            instructions: String::new(),
            input: json!({}),
            max_tokens: cap,
        };
        let sent =
            |episode, req| with_cap(episode).request_body(&request(req))["max_tokens"].clone();
        assert_eq!(sent(Some(4096), Some(512)), json!(512));
        assert_eq!(sent(Some(256), Some(512)), json!(256));
        assert_eq!(sent(None, Some(512)), json!(512));
        assert_eq!(sent(Some(4096), None), json!(4096));
        assert_eq!(sent(None, None), Value::Null);

        let named = |episode, req| with_cap(episode).output_cap(&request(req));
        assert_eq!(named(Some(4096), Some(512)), Some(OutputCap::Stage(512)));
        assert_eq!(named(Some(256), Some(512)), Some(OutputCap::Episode(256)));
        assert_eq!(named(Some(512), Some(512)), Some(OutputCap::Stage(512)));
        assert_eq!(named(Some(4096), None), Some(OutputCap::Episode(4096)));
    }

    #[test]
    fn a_cut_off_names_the_cap_that_cut_it() {
        let stage = cut_off_message(Some(OutputCap::Stage(512)), Some(512));
        assert!(stage.contains("this stage's cap of 512 tokens"), "{stage}");
        assert!(!stage.contains("max_output_tokens"), "{stage}");

        let episode = cut_off_message(Some(OutputCap::Episode(256)), Some(256));
        assert!(episode.contains("llm.max_output_tokens = 256"), "{episode}");
        assert!(episode.contains("raise it"), "{episode}");

        let server = cut_off_message(None, None);
        assert!(
            server.contains("server's token limit (? tokens)"),
            "{server}"
        );
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
        assert!(!reply.cut_off);
        let cut =
            parse_reply(r#"{"choices":[{"message":{"content":"{"},"finish_reason":"length"}]}"#)
                .unwrap();
        assert!(cut.cut_off);
    }
}
