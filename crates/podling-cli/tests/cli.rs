//! Drives the real `podling` binary against the Tunguska example.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

const STAGES: [&str; 6] = [
    "ingest",
    "chunk",
    "extract_claims",
    "ledger",
    "script",
    "analyse",
];

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
            STAGES
                .contains(&id)
                .then(|| (id.to_owned(), cache.to_owned()))
        })
        .collect()
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

    let first = stage_rows(&run().get_output().stdout);
    assert_eq!(first.len(), 6);
    assert!(first.iter().all(|(_, c)| c == "miss"), "{first:?}");

    let second = stage_rows(&run().get_output().stdout);
    let expected: Vec<_> = STAGES
        .iter()
        .map(|s| (s.to_string(), "hit".to_string()))
        .collect();
    assert_eq!(second, expected);
    assert!(out.join("script.json").is_file());

    podling(&cache)
        .args(["cache", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("6 entries"));
    podling(&cache).args(["cache", "clear"]).assert().success();
    podling(&cache)
        .args(["cache", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 entries"));
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
