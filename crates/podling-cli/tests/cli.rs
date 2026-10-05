//! Drives the real `podling` binary against the Tunguska example.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};

const STAGES: [&str; 6] = [
    "ingest",
    "chunk",
    "extract_claims",
    "ledger",
    "script",
    "analyse",
];

/// The stages that run only with `[embedding]` and `[nli]` configured.
const GROUNDING_STAGES: [&str; 3] = ["ground_claims", "cluster_claims", "score_stances"];

/// Runs once per chunk with `[tts]` configured, as the example has.
const SYNTH_STAGE: &str = "synthesize_chunk";
const TRANSCRIBE_STAGE: &str = "transcribe_chunk";

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tunguska/episode.toml")
}

fn podling(cache: &Path) -> Command {
    let mut cmd = Command::cargo_bin("podling").unwrap();
    cmd.arg("--cache-dir").arg(cache);
    cmd
}

/// `(stage, "hit"|"miss")` rows from the `run` output table.
fn stage_rows(stdout: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let (id, cache) = (cols.next()?, cols.next()?);
            (STAGES.contains(&id)
                || GROUNDING_STAGES.contains(&id)
                || id == SYNTH_STAGE
                || id == TRANSCRIBE_STAGE)
                .then(|| (id.to_owned(), cache.to_owned()))
        })
        .collect()
}

#[test]
fn every_example_episode_parses() {
    let dir = example().parent().unwrap().to_owned();
    for name in ["episode.toml", "episode-ollama.toml"] {
        let text = fs::read_to_string(dir.join(name)).unwrap();
        let spec: podling_types::EpisodeSpec =
            toml::from_str(&text).unwrap_or_else(|err| panic!("{name}: {err}"));
        // The Ollama episode shows the grounding stages; the offline one
        // stays without them, so it needs no downloads.
        assert_eq!(
            spec.embedding.is_some() && spec.nli.is_some(),
            name == "episode-ollama.toml",
            "{name}"
        );
    }
}

#[test]
fn help_lists_the_commands() {
    Command::cargo_bin("podling")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("schema")
                .and(predicate::str::contains("run"))
                .and(predicate::str::contains("cache")),
        );
}

#[test]
fn second_run_of_the_example_is_all_cache_hits() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let out = tmp.path().join("out");
    let run = || {
        podling(&cache)
            .args(["run", "--episode"])
            .arg(example())
            .arg("--out")
            .arg(&out)
            .assert()
            .success()
    };

    let first = run();
    let stdout = String::from_utf8_lossy(&first.get_output().stdout).into_owned();
    let first = stage_rows(stdout.as_bytes());
    let chunks = first.iter().filter(|(id, _)| id == SYNTH_STAGE).count();
    assert!(chunks > 0, "the example has [tts]: {stdout}");
    // Each chunk is synthesised, then transcribed to check it.
    assert_eq!(first.len(), STAGES.len() + 2 * chunks);
    assert!(first.iter().all(|(_, c)| c == "miss"), "{first:?}");
    let wav = out.join("episode.wav");
    assert!(
        stdout.contains(&format!("episode audio: {}", wav.display())),
        "{stdout}"
    );
    assert!(wav.is_file());

    let second = stage_rows(&run().get_output().stdout);
    let expected: Vec<_> = STAGES
        .iter()
        .copied()
        .chain(std::iter::repeat_n([SYNTH_STAGE, TRANSCRIBE_STAGE], chunks).flatten())
        .map(|s| (s.to_string(), "hit".to_string()))
        .collect();
    assert_eq!(second, expected);
    assert!(out.join("script.json").is_file());

    podling(&cache)
        .args(["cache", "stats"])
        .assert()
        .success()
        .stdout(
            predicate::str::starts_with(format!("{} entries,", STAGES.len() + 2 * chunks))
                .and(predicate::str::contains(format!("; {chunks} audio blobs"))),
        );
    podling(&cache).args(["cache", "clear"]).assert().success();
    podling(&cache)
        .args(["cache", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "0 entries, 0 bytes; 0 audio blobs",
        ));
}

#[test]
fn no_cache_runs_every_stage() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    for _ in 0..2 {
        let assert = podling(&cache)
            .args(["run", "--no-cache", "--episode"])
            .arg(example())
            .arg("--out")
            .arg(tmp.path().join("out"))
            .assert()
            .success();
        let rows = stage_rows(&assert.get_output().stdout);
        assert!(rows.iter().all(|(_, c)| c == "miss"), "{rows:?}");
    }
    assert!(!cache.exists(), "--no-cache must not write the cache");
}

#[test]
fn schema_export_writes_one_parseable_file_per_kind() {
    let tmp = tempfile::tempdir().unwrap();
    podling(&tmp.path().join("cache"))
        .args(["schema", "export", "--out"])
        .arg(tmp.path().join("schemas"))
        .assert()
        .success();

    let mut names: Vec<String> = fs::read_dir(tmp.path().join("schemas"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "analysis",
            "audio",
            "chunks",
            "claims",
            "documents",
            "episode",
            "ledger",
            "script"
        ]
        .map(|k| format!("{k}.schema.json"))
    );
    for name in names {
        let text = fs::read_to_string(tmp.path().join("schemas").join(name)).unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap();
    }
}

#[test]
fn missing_episode_file_is_a_readable_error() {
    let tmp = tempfile::tempdir().unwrap();
    podling(&tmp.path().join("cache"))
        .args(["run", "--episode", "does-not-exist.toml"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "error: reading episode file does-not-exist.toml",
        ));
}

// --- An OpenAI-compatible provider, against a mock server ------------------

/// What the mock server was asked: the `Authorization` header and the body.
type Seen = Arc<Mutex<Vec<(Option<String>, Value)>>>;

/// Serves `POST /v1/chat/completions` on 127.0.0.1 until the test process
/// exits; `handler` maps a request body to `(status, response body)`.
fn mock_server(
    handler: impl Fn(&Value) -> (u16, String) + Send + Sync + 'static,
) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen = Seen::default();
    let (log, handler) = (Arc::clone(&seen), Arc::new(handler));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (log, handler) = (Arc::clone(&log), Arc::clone(&handler));
            thread::spawn(move || serve(stream, &log, handler.as_ref()));
        }
    });
    (base_url, seen)
}

fn serve(stream: TcpStream, log: &Seen, handler: &dyn Fn(&Value) -> (u16, String)) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
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
    let body: Value = serde_json::from_slice(&body).unwrap();
    let (status, reply) = handler(&body);
    log.lock().unwrap().push((auth, body));

    let mut stream = stream;
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(reply.as_bytes());
}

fn completion(content: &Value) -> (u16, String) {
    let body = json!({
        "choices": [{ "message": { "content": content.to_string() }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 1 },
    });
    (200, body.to_string())
}

/// A stand-in model: every chunk is one claim, and the script cites the first
/// claim and quotes the first sentence of the first source.
fn tiny_model(request: &Value) -> (u16, String) {
    let instructions = request["messages"][0]["content"].as_str().unwrap();
    let input: Value =
        serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
    if instructions.contains("extract factual claims") {
        let text = input["chunk_text"].as_str().unwrap().trim();
        return completion(&json!({ "claims": [{ "text": text }] }));
    }
    let claim = &input["ledger"][0]["id"];
    let source = input["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| !s["sentences"].as_array().unwrap().is_empty())
        .unwrap();
    let first = &source["sentences"][0];
    completion(&json!({
        "cast": [{ "id": "host", "name": "Ada", "role": "host" }],
        "turns": [{
            "speaker": "host",
            "text": "The first source says: {{quote:0}}",
            "emotion": "neutral",
            "citations": [claim],
            "quotes": [{ "chunk": source["chunk"], "sentence": first["sentence"] }],
        }],
    }))
}

/// The Tunguska sources with the `llm` table filled in by the caller.
fn episode_with_llm(dir: &Path, llm: &str) -> PathBuf {
    let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tunguska/sources");
    let root = |name: &str| format!("{:?}", sources.join(name).display().to_string());
    let text = format!(
        "title = \"Tunguska\"\ntopic = \"the 1908 explosion\"\ntarget_minutes = 5\n\
         sources = [\n  {{ kind = \"local_files\", root = {}, independence_group = \"eyewitness\" }},\n  \
         {{ kind = \"local_files\", root = {}, independence_group = \"expedition\" }},\n]\n\
         analysers = [{{ kind = \"quote_verifier\" }}]\n\n[llm]\n{llm}\n",
        root("eyewitness"),
        root("expedition"),
    );
    let path = dir.join("episode.toml");
    fs::write(&path, text).unwrap();
    path
}

fn compat_llm(base_url: &str, api_key_env: Option<&str>) -> String {
    let key = api_key_env
        .map(|var| format!("api_key_env = \"{var}\"\n"))
        .unwrap_or_default();
    format!("kind = \"open_ai_compat\"\nbase_url = \"{base_url}\"\nmodel = \"tiny\"\n{key}")
}

/// stderr must be exactly one `error:` line: no stack dump, no panic.
fn stderr_of(assert: &assert_cmd::assert::Assert) -> String {
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("error: "), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    stderr
}

#[test]
fn a_run_against_an_openai_compatible_server_writes_verbatim_quotes() {
    let (base_url, seen) = mock_server(tiny_model);
    let tmp = tempfile::tempdir().unwrap();
    let episode = episode_with_llm(tmp.path(), &compat_llm(&base_url, Some("PODLING_TEST_KEY")));
    let out = tmp.path().join("out");

    let assert = podling(&tmp.path().join("cache"))
        .env("PODLING_TEST_KEY", "sk-cli-test-key")
        .args(["run", "--episode"])
        .arg(&episode)
        .arg("--out")
        .arg(&out)
        .assert()
        .success();
    let printed = String::from_utf8_lossy(&assert.get_output().stdout).into_owned()
        + &String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(!printed.contains("sk-cli-test-key"), "{printed}");

    // The key went out only as a bearer header, on every request.
    let seen = seen.lock().unwrap();
    assert!(
        seen.len() >= 3,
        "expected several requests, got {}",
        seen.len()
    );
    assert!(
        seen.iter()
            .all(|(auth, _)| auth.as_deref() == Some("Bearer sk-cli-test-key"))
    );

    // The quote is copied from the source, not typed by the "model".
    let script: Value =
        serde_json::from_str(&fs::read_to_string(out.join("script.json")).unwrap()).unwrap();
    let quote = &script["body"]["turns"][0]["quotes"][0];
    let document = fs::read_to_string(out.join("documents.json")).unwrap();
    assert!(document.contains(quote["text"].as_str().unwrap()));
    assert!(!quote["text"].as_str().unwrap().is_empty());
}

#[test]
fn an_unset_key_variable_is_one_readable_line() {
    let tmp = tempfile::tempdir().unwrap();
    let episode = episode_with_llm(
        tmp.path(),
        &compat_llm("http://127.0.0.1:1/v1", Some("PODLING_UNSET_TEST_KEY")),
    );
    let assert = podling(&tmp.path().join("cache"))
        .env_remove("PODLING_UNSET_TEST_KEY")
        .args(["run", "--episode"])
        .arg(&episode)
        .assert()
        .failure();
    let stderr = stderr_of(&assert);
    assert!(stderr.contains("PODLING_UNSET_TEST_KEY"), "{stderr}");
    assert!(stderr.contains("not set"), "{stderr}");
}

#[test]
fn an_unreachable_server_is_one_readable_line() {
    let tmp = tempfile::tempdir().unwrap();
    // Nothing listens on port 1.
    let episode = episode_with_llm(tmp.path(), &compat_llm("http://127.0.0.1:1/v1", None));
    let assert = podling(&tmp.path().join("cache"))
        .args(["run", "--episode"])
        .arg(&episode)
        .assert()
        .failure();
    let stderr = stderr_of(&assert);
    assert!(stderr.contains("http://127.0.0.1:1/v1"), "{stderr}");
    assert!(stderr.contains("running and reachable"), "{stderr}");
}

#[test]
fn a_404_suggests_checking_the_model_name() {
    let (base_url, _) = mock_server(|_| (404, r#"{"error":{"message":"model not found"}}"#.into()));
    let tmp = tempfile::tempdir().unwrap();
    let episode = episode_with_llm(tmp.path(), &compat_llm(&base_url, None));
    let assert = podling(&tmp.path().join("cache"))
        .args(["run", "--episode"])
        .arg(&episode)
        .assert()
        .failure();
    let stderr = stderr_of(&assert);
    assert!(stderr.contains("HTTP 404"), "{stderr}");
    assert!(stderr.contains("a model called \"tiny\""), "{stderr}");
}

#[test]
fn a_401_names_the_key_variable_and_never_prints_the_key() {
    let key = "sk-cli-401-key";
    let echo = format!(r#"{{"error":{{"message":"Incorrect API key provided: {key}"}}}}"#);
    let (base_url, _) = mock_server(move |_| (401, echo.clone()));
    let tmp = tempfile::tempdir().unwrap();
    let episode = episode_with_llm(tmp.path(), &compat_llm(&base_url, Some("PODLING_TEST_KEY")));
    let assert = podling(&tmp.path().join("cache"))
        .env("PODLING_TEST_KEY", key)
        .args(["run", "--episode"])
        .arg(&episode)
        .assert()
        .failure();
    let stderr = stderr_of(&assert);
    assert!(stderr.contains("HTTP 401"), "{stderr}");
    assert!(stderr.contains("$PODLING_TEST_KEY"), "{stderr}");
    assert!(!stderr.contains(key), "{stderr}");
}

/// Runs the Tunguska example against a real server:
///
/// ```text
/// PODLING_LIVE_LLM_URL=http://localhost:11434/v1 PODLING_LIVE_LLM_MODEL=llama3.1:8b \
///   cargo test -p podling-cli -- --ignored live
/// ```
///
/// `PODLING_LIVE_LLM_KEY_ENV` may name a variable that holds an API key.
#[test]
#[ignore = "needs a real server: set PODLING_LIVE_LLM_URL and PODLING_LIVE_LLM_MODEL"]
fn live_run_against_a_real_server() {
    let (Ok(url), Ok(model)) = (
        std::env::var("PODLING_LIVE_LLM_URL"),
        std::env::var("PODLING_LIVE_LLM_MODEL"),
    ) else {
        // Fail, don't skip: an ignored test that returns early reports "ok"
        // for a run that tested nothing.
        panic!(
            "set PODLING_LIVE_LLM_URL and PODLING_LIVE_LLM_MODEL to run this test, e.g. \
             PODLING_LIVE_LLM_URL=http://localhost:11434/v1 PODLING_LIVE_LLM_MODEL=llama3.1:8b \
             cargo test -p podling-cli -- --ignored live"
        );
    };
    let key_env = std::env::var("PODLING_LIVE_LLM_KEY_ENV").ok();
    let mut llm = compat_llm(&url, key_env.as_deref());
    llm = llm.replace("model = \"tiny\"", &format!("model = \"{model}\""));
    // A local 8B model needs 80 to 120 s to write the script; the default
    // timeout is 120 s.
    llm.push_str("timeout_secs = 300\n");

    let tmp = tempfile::tempdir().unwrap();
    let episode = episode_with_llm(tmp.path(), &llm);
    let out = tmp.path().join("out");
    podling(&tmp.path().join("cache"))
        .args(["run", "--episode"])
        .arg(&episode)
        .arg("--out")
        .arg(&out)
        .assert()
        .success();
    assert!(out.join("script.json").is_file());
}

#[test]
fn unknown_episode_key_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let episode = tmp.path().join("episode.toml");
    let text = fs::read_to_string(example()).unwrap() + "\napi_key = \"sk-nope\"\n";
    fs::write(&episode, text).unwrap();
    podling(&tmp.path().join("cache"))
        .args(["run", "--episode"])
        .arg(&episode)
        .assert()
        .failure()
        .stderr(predicate::str::contains("api_key"));
}

#[test]
fn a_grounded_run_caches_the_new_stages_too() {
    let tmp = tempfile::tempdir().unwrap();
    // `[embedding]` and `[nli]` follow the `[llm]` table the helper writes.
    let episode = episode_with_llm(
        tmp.path(),
        "kind = \"fake\"\n\n[embedding]\nkind = \"fake\"\n\n[nli]\nkind = \"fake\"",
    );
    let cache = tmp.path().join("cache");
    let run = || {
        let assert = podling(&cache)
            .args(["run", "--episode"])
            .arg(&episode)
            .arg("--out")
            .arg(tmp.path().join("out"))
            .assert()
            .success();
        stage_rows(&assert.get_output().stdout)
    };

    let first = run();
    let ids: Vec<&str> = first.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "ingest",
            "chunk",
            "extract_claims",
            "ground_claims",
            "cluster_claims",
            "score_stances",
            "ledger",
            "script",
            "analyse",
        ]
    );
    assert!(first.iter().all(|(_, c)| c == "miss"), "{first:?}");
    let second = run();
    assert_eq!(second.len(), 9);
    assert!(second.iter().all(|(_, c)| c == "hit"), "{second:?}");
}

#[test]
fn an_embedding_401_names_the_embedding_key_variable() {
    let key = "sk-embed-401-key";
    let echo = format!(r#"{{"error":{{"message":"Incorrect API key provided: {key}"}}}}"#);
    let (base_url, _) = mock_server(move |_| (401, echo.clone()));
    let tmp = tempfile::tempdir().unwrap();
    let episode = episode_with_llm(
        tmp.path(),
        &format!(
            "kind = \"fake\"\n\n[embedding]\nkind = \"open_ai_compat\"\nbase_url = \"{base_url}\"\n\
             model = \"embed\"\napi_key_env = \"PODLING_EMBED_KEY\"\n\n[nli]\nkind = \"fake\""
        ),
    );
    let assert = podling(&tmp.path().join("cache"))
        .env("PODLING_EMBED_KEY", key)
        .args(["run", "--episode"])
        .arg(&episode)
        .assert()
        .failure();
    let stderr = stderr_of(&assert);
    assert!(stderr.contains("HTTP 401"), "{stderr}");
    assert!(stderr.contains("$PODLING_EMBED_KEY"), "{stderr}");
    assert!(!stderr.contains(key), "{stderr}");
}

#[test]
fn a_missing_nli_model_is_one_line_naming_the_download_command() {
    let tmp = tempfile::tempdir().unwrap();
    let episode = episode_with_llm(
        tmp.path(),
        "kind = \"fake\"\n\n[embedding]\nkind = \"fake\"\n\n\
         [nli]\nkind = \"cross_encoder\"\nmodel_dir = \"models/missing\"",
    );
    let assert = podling(&tmp.path().join("cache"))
        .args(["run", "--episode"])
        .arg(&episode)
        .assert()
        .failure();
    let stderr = stderr_of(&assert);
    assert!(
        stderr.contains("hf download cross-encoder/nli-deberta-v3-base"),
        "{stderr}"
    );
    // Resolved against the episode's directory, not the working directory.
    assert!(
        stderr.contains(&tmp.path().join("models/missing").display().to_string()),
        "{stderr}"
    );
}
