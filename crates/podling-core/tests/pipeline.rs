//! End-to-end pipeline runs over two fixture sources in different
//! independence groups that share one sentence.

use std::fs;
use std::path::{Path, PathBuf};

use podling_core::{DiskCache, RunReport, pipeline};
use podling_types::{Chunk, ClaimStatus, Document, EpisodeSpec, Ledger};
use serde_json::Value;

const STAGES: [&str; 6] = [
    "ingest",
    "chunk",
    "extract_claims",
    "ledger",
    "script",
    "analyse",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn spec(base_dir: &Path) -> EpisodeSpec {
    toml::from_str(&fs::read_to_string(base_dir.join("episode.toml")).unwrap()).unwrap()
}

fn hits(report: &RunReport) -> Vec<(&str, bool)> {
    report
        .stages
        .iter()
        .map(|s| (s.id.as_str(), s.cache_hit))
        .collect()
}

/// Copies the fixture tree so a test can edit it.
fn copy_fixtures(to: &Path) {
    for group in ["eyewitness", "expedition"] {
        fs::create_dir_all(to.join(group)).unwrap();
        for entry in fs::read_dir(fixtures().join(group)).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, to.join(group).join(path.file_name().unwrap())).unwrap();
        }
    }
    fs::copy(fixtures().join("episode.toml"), to.join("episode.toml")).unwrap();
}

#[test]
fn first_run_misses_and_second_run_hits_every_stage() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = DiskCache::new(tmp.path().join("cache"));
    let out = tmp.path().join("out");
    let spec = spec(&fixtures());

    let first = pipeline::run(&spec, &fixtures(), Some(&cache), &out).unwrap();
    assert_eq!(hits(&first), STAGES.map(|id| (id, false)).to_vec());
    assert_eq!(first.error_findings, 0);

    let second = pipeline::run(&spec, &fixtures(), Some(&cache), &out).unwrap();
    assert_eq!(hits(&second), STAGES.map(|id| (id, true)).to_vec());
}

#[test]
fn writes_every_artifact_in_a_versioned_envelope() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    pipeline::run(&spec(&fixtures()), &fixtures(), None, &out).unwrap();

    for kind in podling_types::ArtifactKind::ALL {
        let path = out.join(format!("{}.json", kind.as_str()));
        let json: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            json["schema_version"],
            podling_types::SCHEMA_VERSION,
            "{path:?}"
        );
        assert_eq!(json["kind"], kind.as_str());
    }
}

fn read_body<T: serde::de::DeserializeOwned>(out: &Path, kind: &str) -> T {
    let json: Value =
        serde_json::from_str(&fs::read_to_string(out.join(format!("{kind}.json"))).unwrap())
            .unwrap();
    serde_json::from_value(json["body"].clone()).unwrap()
}

#[test]
fn every_chunk_is_an_exact_slice_of_its_document() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    pipeline::run(&spec(&fixtures()), &fixtures(), None, &out).unwrap();

    let documents: Vec<Document> = read_body(&out, "documents");
    let chunks: Vec<Chunk> = read_body(&out, "chunks");
    assert!(chunks.len() >= 2);
    for chunk in &chunks {
        let doc = documents
            .iter()
            .find(|d| d.id() == chunk.document())
            .unwrap();
        assert_eq!(doc.slice(chunk.span()), Some(chunk.text()));
    }
}

#[test]
fn ledger_corroborates_the_shared_sentence_only() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    pipeline::run(&spec(&fixtures()), &fixtures(), None, &out).unwrap();

    let json: Value =
        serde_json::from_str(&fs::read_to_string(out.join("ledger.json")).unwrap()).unwrap();
    let ledger: Ledger = serde_json::from_value(json["body"].clone()).unwrap();
    let status_of = |text: &str| {
        ledger
            .entries()
            .iter()
            .find(|e| e.claim.text() == text)
            .map(|e| e.status.clone())
            .unwrap_or_else(|| panic!("no claim {text:?}"))
    };

    let shared =
        "In June 1908 an explosion flattened about 80 million trees over the Tunguska forest.";
    assert_eq!(
        status_of(shared),
        ClaimStatus::Corroborated {
            groups: vec!["expedition".into(), "eyewitness".into()]
        }
    );
    assert_eq!(
        status_of("No impact crater was found."),
        ClaimStatus::SingleSource {
            group: "expedition".into()
        }
    );
}

#[test]
fn editing_a_source_invalidates_ingest_and_everything_downstream() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("episode");
    copy_fixtures(&base);
    let cache = DiskCache::new(tmp.path().join("cache"));
    let out = tmp.path().join("out");
    let spec = spec(&base);

    pipeline::run(&spec, &base, Some(&cache), &out).unwrap();
    let report_path = base.join("expedition/report.md");
    let edited =
        fs::read_to_string(&report_path).unwrap() + "\nThe trees pointed away from the centre.\n";
    fs::write(&report_path, edited).unwrap();

    let after = pipeline::run(&spec, &base, Some(&cache), &out).unwrap();
    assert_eq!(hits(&after), STAGES.map(|id| (id, false)).to_vec());
}

#[test]
fn a_missing_source_directory_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("episode");
    copy_fixtures(&base);
    fs::remove_dir_all(base.join("expedition")).unwrap();

    let err = pipeline::run(&spec(&base), &base, None, &tmp.path().join("out")).unwrap_err();
    assert!(err.to_string().contains("expedition"), "{err}");
}

/// Writes an episode with one `local_files` source per `(root, group)`.
fn episode_with_sources(base: &Path, sources: &[(&str, &str)]) -> EpisodeSpec {
    let list: Vec<String> = sources
        .iter()
        .map(|(root, group)| {
            format!(
                r#"{{ kind = "local_files", root = "{root}", independence_group = "{group}" }}"#
            )
        })
        .collect();
    let toml = format!(
        "title = \"t\"\ntopic = \"t\"\ntarget_minutes = 1\nllm = {{ kind = \"fake\" }}\nsources = [{}]\n",
        list.join(", ")
    );
    fs::write(base.join("episode.toml"), toml).unwrap();
    spec(base)
}

/// Regression: same-named, identical files under two roots used to share a
/// document id, so one group's evidence overwrote the other's.
#[test]
fn identical_files_in_two_groups_are_corroborated() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    for root in ["a", "b"] {
        fs::create_dir(base.join(root)).unwrap();
        fs::write(base.join(root).join("notes.md"), "Shared fact here.\n").unwrap();
    }
    let spec = episode_with_sources(base, &[("a", "ga"), ("b", "gb")]);
    let out = base.join("out");
    pipeline::run(&spec, base, None, &out).unwrap();

    let ledger: Ledger = read_body(&out, "ledger");
    assert_eq!(
        ledger.entries()[0].status,
        ClaimStatus::Corroborated {
            groups: vec!["ga".into(), "gb".into()]
        }
    );
}

#[test]
fn one_document_claimed_by_two_groups_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();
    fs::create_dir(base.join("a")).unwrap();
    fs::write(base.join("a/notes.md"), "Shared fact here.\n").unwrap();
    let spec = episode_with_sources(base, &[("a", "ga"), ("a", "gb")]);

    let err = pipeline::run(&spec, base, None, &base.join("out")).unwrap_err();
    assert!(err.to_string().contains("a/notes.md"), "{err}");
}
