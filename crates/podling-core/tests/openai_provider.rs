//! `OpenAiCompat` against a tiny in-process HTTP server. Everything runs
//! offline on `127.0.0.1`, so it is safe in CI.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use podling_core::plugin::{CompletionRequest, LlmProvider, LlmTask, OpenAiCompat};
use podling_core::{CoreError, ProviderFailure};
use podling_types::LlmConfig;
use serde_json::{Value, json};

const KEY: &str = "sk-test-key-123";

/// One canned response.
struct Reply {
    status: u16,
    body: String,
    /// Wait this long before answering.
    delay: Duration,
}

impl Reply {
    fn ok(content: &str) -> Self {
        let body = json!({
            "choices": [{ "message": { "content": content }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 11, "completion_tokens": 3 },
        });
        Self::status(200, body.to_string())
    }

    fn status(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            delay: Duration::ZERO,
        }
    }

    fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

struct Recorded {
    /// Lower-cased header names.
    headers: Vec<(String, String)>,
    body: Value,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

struct MockServer {
    base_url: String,
    seen: Arc<Mutex<Vec<Recorded>>>,
}

impl MockServer {
    /// Serves `replies` in order, one per connection, then stops listening.
    fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        thread::spawn(move || {
            for reply in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                serve(stream, reply, &log);
            }
        });
        Self { base_url, seen }
    }

    fn request_count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn request(&self, index: usize) -> Recorded {
        let seen = self.seen.lock().unwrap();
        let r = &seen[index];
        Recorded {
            headers: r.headers.clone(),
            body: r.body.clone(),
        }
    }
}

fn serve(stream: TcpStream, reply: Reply, log: &Mutex<Vec<Recorded>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    assert!(
        request_line.starts_with("POST /v1/chat/completions "),
        "{request_line}"
    );

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.to_ascii_lowercase();
            let value = value.trim().to_owned();
            if name == "content-length" {
                content_length = value.parse().unwrap();
            }
            headers.push((name, value));
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).unwrap();
    log.lock().unwrap().push(Recorded {
        headers,
        body: serde_json::from_slice(&body).unwrap(),
    });

    thread::sleep(reply.delay);
    let head = format!(
        "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.status,
        reply.body.len()
    );
    let mut stream = stream;
    // The client may already have hung up (timeout, size cap); that is fine.
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(reply.body.as_bytes());
}

fn provider(server: &MockServer, key: Option<&str>, timeout_secs: Option<u64>) -> OpenAiCompat {
    let config = LlmConfig::OpenAiCompat {
        base_url: server.base_url.clone(),
        model: "test-model".into(),
        api_key_env: key.map(|_| "PODLING_TEST_KEY".into()),
        temperature: Some(0.5),
        timeout_secs,
        max_output_tokens: Some(256),
        unload_after: false,
    };
    OpenAiCompat::from_config_with_env(&config, |_| key.map(Into::into))
        .unwrap()
        .with_retry_backoff(Duration::from_millis(1))
}

fn request() -> CompletionRequest {
    CompletionRequest {
        task: LlmTask::ExtractClaims,
        instructions: "Extract claims.".into(),
        input: json!({ "chunk_text": "Trees fell." }),
        max_tokens: None,
    }
}

fn provider_error(err: CoreError) -> (ProviderFailure, String) {
    match err {
        CoreError::Provider {
            plugin,
            kind,
            message,
        } => {
            assert_eq!(plugin, "open_ai_compat");
            (kind, message)
        }
        other => panic!("expected a Provider error, got {other:?}"),
    }
}

#[test]
fn success_sends_the_documented_request_and_returns_the_content() {
    let server = MockServer::start(vec![Reply::ok(r#"[{"text":"Trees fell."}]"#)]);
    let completion = provider(&server, Some(KEY), None)
        .complete(&request())
        .unwrap();
    assert_eq!(completion.text, r#"[{"text":"Trees fell."}]"#);

    let sent = server.request(0);
    assert_eq!(sent.header("authorization"), Some("Bearer sk-test-key-123"));
    assert_eq!(sent.body["model"], "test-model");
    assert_eq!(sent.body["temperature"], 0.5);
    assert_eq!(sent.body["max_tokens"], 256);
    assert_eq!(sent.body["response_format"]["type"], "json_object");
    assert_eq!(sent.body["messages"][0]["role"], "system");
    assert_eq!(sent.body["messages"][0]["content"], "Extract claims.");
    assert_eq!(sent.body["messages"][1]["role"], "user");
    let user: Value =
        serde_json::from_str(sent.body["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(user, json!({ "chunk_text": "Trees fell." }));
}

#[test]
fn no_authorization_header_without_a_configured_key() {
    let server = MockServer::start(vec![Reply::ok("{}")]);
    provider(&server, None, None).complete(&request()).unwrap();
    assert_eq!(server.request(0).header("authorization"), None);
}

#[test]
fn a_401_is_readable_and_never_echoes_the_key() {
    // OpenAI's own 401 body quotes (part of) the key it was given.
    let body = format!(r#"{{"error":{{"message":"Incorrect API key provided: {KEY}."}}}}"#);
    let server = MockServer::start(vec![Reply::status(401, body)]);
    let (kind, message) = provider_error(
        provider(&server, Some(KEY), None)
            .complete(&request())
            .unwrap_err(),
    );
    assert!(message.contains("401"), "{message}");
    assert!(message.contains("Incorrect API key"), "{message}");
    assert!(!message.contains(KEY), "{message}");
    assert_eq!(kind, ProviderFailure::Http(401));
    assert_eq!(server.request_count(), 1, "a 4xx must not be retried");
}

#[test]
fn a_500_twice_then_a_200_succeeds() {
    let server = MockServer::start(vec![
        Reply::status(500, "boom"),
        Reply::status(503, "still down"),
        Reply::ok("{}"),
    ]);
    let completion = provider(&server, None, None).complete(&request()).unwrap();
    assert_eq!(completion.text, "{}");
    assert_eq!(server.request_count(), 3);
}

#[test]
fn a_429_is_retried_but_only_twice() {
    let server = MockServer::start(vec![
        Reply::status(429, "slow down"),
        Reply::status(429, "slow down"),
        Reply::status(429, "slow down"),
        Reply::ok("{}"), // never reached
    ]);
    let (kind, message) = provider_error(
        provider(&server, None, None)
            .complete(&request())
            .unwrap_err(),
    );
    assert!(message.contains("429"), "{message}");
    assert_eq!(kind, ProviderFailure::Http(429));
    assert_eq!(server.request_count(), 3);
}

#[test]
fn a_slow_server_times_out() {
    let server = MockServer::start(vec![
        Reply::ok("{}").delayed(Duration::from_secs(4)),
        Reply::ok("{}"),
    ]);
    let (kind, message) = provider_error(
        provider(&server, None, Some(1))
            .complete(&request())
            .unwrap_err(),
    );
    assert!(message.contains("timed out"), "{message}");
    assert_eq!(kind, ProviderFailure::TimedOut);
    assert_eq!(server.request_count(), 1, "a timeout must not be retried");
}

#[test]
fn an_oversized_body_is_rejected() {
    let filler = "x".repeat(5 * 1024 * 1024);
    let server = MockServer::start(vec![Reply::status(200, filler)]);
    let (kind, message) = provider_error(
        provider(&server, None, None)
            .complete(&request())
            .unwrap_err(),
    );
    assert!(message.contains("larger than 4 MiB"), "{message}");
    assert_eq!(kind, ProviderFailure::Other);
}

#[test]
fn a_body_that_is_not_a_chat_completion_is_a_shape_error() {
    let server = MockServer::start(vec![Reply::status(200, r#"{"unexpected":true}"#)]);
    let (kind, message) = provider_error(
        provider(&server, None, None)
            .complete(&request())
            .unwrap_err(),
    );
    assert!(message.contains("choices[0].message.content"), "{message}");
    assert_eq!(kind, ProviderFailure::Other);
}

#[test]
fn a_reply_stopped_at_the_token_limit_is_cut_off() {
    let body = json!({
        "choices": [{ "message": { "content": "{\"claims\": [" }, "finish_reason": "length" }],
        "usage": { "prompt_tokens": 11, "completion_tokens": 2048 },
    });
    let server = MockServer::start(vec![Reply::status(200, body.to_string())]);
    let (kind, message) = provider_error(
        provider(&server, None, None)
            .complete(&request())
            .unwrap_err(),
    );
    assert_eq!(kind, ProviderFailure::CutOff);
    assert!(message.contains("2048 tokens"), "{message}");
}

#[test]
fn an_unreachable_server_names_the_url() {
    let server = MockServer::start(vec![]);
    let url = server.base_url.clone();
    // Nothing accepts on this port once the listener thread has returned.
    thread::sleep(Duration::from_millis(50));
    let (kind, message) = provider_error(
        provider(&server, None, Some(2))
            .complete(&request())
            .unwrap_err(),
    );
    assert!(message.contains(&url), "{message}");
    assert_eq!(kind, ProviderFailure::Unreachable);
}
