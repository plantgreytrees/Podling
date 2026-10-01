//! `OpenAiEmbeddings` against a tiny in-process HTTP server on `127.0.0.1`,
//! so it runs offline. The live test at the bottom needs a real server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use podling_core::plugin::{EmbeddingProvider, OpenAiEmbeddings, cosine};
use podling_core::{CoreError, ProviderFailure};
use podling_types::EmbeddingConfig;
use serde_json::{Value, json};

const KEY: &str = "sk-embed-key-456";

/// What the server saw: the `Authorization` header and the JSON body.
type Seen = Arc<Mutex<Vec<(Option<String>, Value)>>>;

/// Serves `replies` in order, one per connection, on `POST /v1/embeddings`.
fn mock_server(replies: Vec<(u16, String)>) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen = Seen::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for (status, body) in replies {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            serve(stream, status, &body, &log);
        }
    });
    (base_url, seen)
}

fn serve(stream: TcpStream, status: u16, reply: &str, log: &Seen) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.starts_with("POST /v1/embeddings "), "{line}");
    let (mut auth, mut length) = (None, 0usize);
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.to_ascii_lowercase().as_str() {
                "authorization" => auth = Some(value.trim().to_owned()),
                "content-length" => length = value.trim().parse().unwrap(),
                _ => {}
            }
        }
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).unwrap();
    log.lock()
        .unwrap()
        .push((auth, serde_json::from_slice(&body).unwrap()));

    let mut stream = stream;
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(reply.as_bytes());
}

/// An embeddings reply listing `vectors` under the given indices.
fn reply(items: &[(usize, Vec<f32>)]) -> (u16, String) {
    let data: Vec<Value> = items
        .iter()
        .map(|(index, v)| json!({ "object": "embedding", "index": index, "embedding": v }))
        .collect();
    (200, json!({ "object": "list", "data": data }).to_string())
}

fn provider(base_url: &str, key: Option<&str>) -> OpenAiEmbeddings {
    let config = EmbeddingConfig::OpenAiCompat {
        base_url: base_url.into(),
        model: "embed-model".into(),
        api_key_env: key.map(|_| "PODLING_TEST_KEY".into()),
        timeout_secs: None,
    };
    OpenAiEmbeddings::from_config_with_env(&config, |_| key.map(Into::into))
        .unwrap()
        .with_retry_backoff(Duration::from_millis(1))
}

#[test]
fn sends_the_model_and_texts_and_returns_vectors_in_request_order() {
    // The server answers out of order; `index` puts them back.
    let (base_url, seen) = mock_server(vec![reply(&[(1, vec![0.0, 1.0]), (0, vec![1.0, 0.0])])]);
    let vectors = provider(&base_url, Some(KEY))
        .embed(&["first", "second"])
        .unwrap();
    assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);

    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].0.as_deref(), Some("Bearer sk-embed-key-456"));
    assert_eq!(
        seen[0].1,
        json!({ "model": "embed-model", "input": ["first", "second"] })
    );
}

#[test]
fn more_than_one_batch_of_texts_is_split_across_requests() {
    let batch = podling_core::plugin::openai_embeddings::EMBED_BATCH;
    let texts: Vec<String> = (0..batch + 1).map(|i| format!("text {i}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let full: Vec<(usize, Vec<f32>)> = (0..batch).map(|i| (i, vec![1.0])).collect();
    let (base_url, seen) = mock_server(vec![reply(&full), reply(&[(0, vec![1.0])])]);

    let vectors = provider(&base_url, None).embed(&refs).unwrap();
    assert_eq!(vectors.len(), batch + 1);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].1["input"], json!([format!("text {batch}")]));
}

#[test]
fn a_5xx_is_retried() {
    let (base_url, seen) = mock_server(vec![(503, "busy".into()), reply(&[(0, vec![1.0])])]);
    assert_eq!(
        provider(&base_url, None).embed(&["t"]).unwrap(),
        vec![vec![1.0]]
    );
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn a_401_is_readable_and_never_echoes_the_key() {
    let body = format!(r#"{{"error":{{"message":"Incorrect API key provided: {KEY}."}}}}"#);
    let (base_url, seen) = mock_server(vec![(401, body)]);
    let err = provider(&base_url, Some(KEY)).embed(&["t"]).unwrap_err();
    let CoreError::Provider {
        plugin,
        kind,
        message,
    } = err
    else {
        panic!("expected a Provider error, got {err:?}");
    };
    assert_eq!(plugin, "open_ai_compat_embeddings");
    assert_eq!(kind, ProviderFailure::Http(401));
    assert!(message.contains("401"), "{message}");
    assert!(!message.contains(KEY), "{message}");
    assert_eq!(seen.lock().unwrap().len(), 1, "a 4xx must not be retried");
}

#[test]
fn a_reply_with_a_missing_vector_is_an_error() {
    let (base_url, _) = mock_server(vec![reply(&[(0, vec![1.0])])]);
    let err = provider(&base_url, None).embed(&["a", "b"]).unwrap_err();
    assert!(err.to_string().contains("index 1"), "{err}");
}

/// Embeds a paraphrase pair and an unrelated sentence with a real server:
///
/// ```text
/// PODLING_LIVE_EMBED_URL=http://localhost:11434/v1 PODLING_LIVE_EMBED_MODEL=nomic-embed-text \
///   cargo test -p podling-core --test openai_embeddings -- --ignored live
/// ```
#[test]
#[ignore = "needs a real server: set PODLING_LIVE_EMBED_URL and PODLING_LIVE_EMBED_MODEL"]
fn live_embeddings() {
    let (Ok(url), Ok(model)) = (
        std::env::var("PODLING_LIVE_EMBED_URL"),
        std::env::var("PODLING_LIVE_EMBED_MODEL"),
    ) else {
        // Fail, don't skip: an ignored test that returns early reports "ok"
        // for a run that tested nothing.
        panic!("set PODLING_LIVE_EMBED_URL and PODLING_LIVE_EMBED_MODEL to run this test");
    };
    let provider = OpenAiEmbeddings::from_config(&EmbeddingConfig::OpenAiCompat {
        base_url: url,
        model,
        api_key_env: None,
        timeout_secs: None,
    })
    .unwrap();
    let v = provider
        .embed(&[
            "In June 1908 an explosion flattened about 80 million trees.",
            "About 80 million trees were flattened by an explosion in June 1908.",
            "The recipe needs two eggs and a cup of flour.",
        ])
        .unwrap();
    let (paraphrase, unrelated) = (cosine(&v[0], &v[1]), cosine(&v[0], &v[2]));
    eprintln!("paraphrase cosine {paraphrase:.3}, unrelated cosine {unrelated:.3}");
    assert!(paraphrase > unrelated, "{paraphrase} vs {unrelated}");
}
