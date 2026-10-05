//! The audio half of a run: the fixture episode with `[[cast]]`, `[tts]` and
//! `[asr]`, offline with the fake TTS, and once through a real (stub) worker
//! process to show it is gone when the run returns.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use podling_core::audio::{Pcm, WavFormat};
use podling_core::stages::assemble::{TARGET_LUFS, measure};
use podling_core::{CoreError, DiskCache, RunReport, pipeline};
use podling_types::{AudioManifest, BeatKind, Envelope, EpisodeSpec, NonverbalAt, Pace, Script};
use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

const CAST: &str = r#"
[[cast]]
id = "host"
name = "Mara"
role = "host"
voice = { reference = "voices/host.wav", transcript = "Hello and welcome.", licence = "CC0-1.0" }

[[cast]]
id = "guest"
name = "Tomas"
role = "co-host"
voice = { reference = "voices/guest.wav", transcript = "Glad to be here.", licence = "CC0-1.0" }

[asr]
kind = "fake"
"#;

/// The fixture episode plus a cast and `tts`, with its sources and two
/// made-up voice clips copied into `dir`.
fn audio_episode(dir: &Path, tts: &str) -> EpisodeSpec {
    for group in ["eyewitness", "expedition"] {
        fs::create_dir_all(dir.join(group)).unwrap();
        for entry in fs::read_dir(fixtures().join(group)).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, dir.join(group).join(path.file_name().unwrap())).unwrap();
        }
    }
    fs::create_dir_all(dir.join("voices")).unwrap();
    for (name, pitch) in [("host", 180.0), ("guest", 120.0)] {
        let samples = (0..16_000)
            .map(|i| 0.2 * (i as f32 * pitch * std::f32::consts::TAU / 16_000.0).sin())
            .collect();
        let wav = Pcm::new(16_000, samples).to_wav(WavFormat::Int16).unwrap();
        fs::write(dir.join("voices").join(format!("{name}.wav")), wav).unwrap();
    }
    let base = fs::read_to_string(fixtures().join("episode.toml")).unwrap();
    toml::from_str(&format!("{base}{CAST}\n[tts]\n{tts}\n")).unwrap()
}

fn synth(report: &RunReport) -> Vec<bool> {
    report
        .stages
        .iter()
        .filter(|s| s.id == "synthesize_chunk")
        .map(|s| s.cache_hit)
        .collect()
}

fn manifest(out: &Path) -> AudioManifest {
    let json: Value =
        serde_json::from_str(&fs::read_to_string(out.join("audio.json")).unwrap()).unwrap();
    assert_eq!(json["kind"], "audio");
    serde_json::from_value(json["body"].clone()).unwrap()
}

#[test]
fn the_fake_episode_becomes_a_loudness_normalised_wav() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = audio_episode(tmp.path(), "kind = \"fake\"");
    let out = tmp.path().join("out");
    let report = pipeline::run(&spec, tmp.path(), None, &out).unwrap();

    let wav = out.join("episode.wav");
    assert_eq!(report.audio.as_deref(), Some(wav.as_path()));
    let pcm = Pcm::read_wav(&wav).unwrap();
    assert_eq!(pcm.rate, 48_000);
    // Measured from the 16-bit file on disk, not taken from the manifest.
    let loudness = measure(&pcm).unwrap();
    assert!(
        (loudness.integrated_lufs - TARGET_LUFS).abs() <= 0.5,
        "{loudness:?}"
    );

    let manifest = manifest(&out);
    let script: Value =
        serde_json::from_str(&fs::read_to_string(out.join("script.json")).unwrap()).unwrap();
    let turns = script["body"]["turns"].as_array().unwrap().len();
    assert_eq!(manifest.chunks.len(), turns, "one chunk per turn");
    for (i, chunk) in manifest.chunks.iter().enumerate() {
        assert_eq!(chunk.turns.indices(), i..i + 1);
    }
    assert_eq!(manifest.episode.path, Path::new("episode.wav"));
    assert!((manifest.episode.integrated_lufs - TARGET_LUFS).abs() <= 0.5);
    let licences: Vec<(&str, &str)> = manifest
        .voices
        .iter()
        .map(|v| (v.speaker.0.as_str(), v.licence.as_str()))
        .collect();
    assert_eq!(licences, [("host", "CC0-1.0"), ("guest", "CC0-1.0")]);
    // The declared cast is the script's cast.
    assert_eq!(script["body"]["cast"][0]["name"], "Mara");
    // Without a cache the audio waits beside the artifacts.
    assert!(out.join(".blobs").is_dir());
}

/// With `[tts]` the script is written for audio: it has beats covering every
/// turn, pace, a backchannel and a callback, and they survive the round trip
/// through `script.json`.
#[test]
fn a_spoken_episode_has_beats_in_its_script() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = audio_episode(tmp.path(), "kind = \"fake\"");
    let out = tmp.path().join("out");
    pipeline::run(&spec, tmp.path(), None, &out).unwrap();

    let text = fs::read_to_string(out.join("script.json")).unwrap();
    let envelope: Envelope<Script> = serde_json::from_str(&text).unwrap();
    let script = envelope.body;
    let beats = script.beats();
    let kinds: Vec<BeatKind> = beats.iter().map(|b| b.kind).collect();
    assert_eq!(kinds.first(), Some(&BeatKind::QuoteReading));
    assert_eq!(kinds.last(), Some(&BeatKind::Transition));
    assert!(kinds.contains(&BeatKind::Banter), "{kinds:?}");
    let raw: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        raw["body"]["beats"].as_array().unwrap().len(),
        beats.len(),
        "the beats are stored, not implied"
    );
    let turns = script.turns();
    assert_eq!(beats.last().unwrap().turns.end(), turns.len());
    assert_eq!(turns[1].pace, Pace::Quick);
    assert_eq!(turns[1].nonverbal[0].at, NonverbalAt::Over);
    assert_eq!(turns.last().unwrap().callback_to, Some(0));
}

#[test]
fn a_warm_rerun_makes_no_tts_calls_and_a_lost_blob_redoes_one_chunk() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = audio_episode(tmp.path(), "kind = \"fake\"");
    let cache = DiskCache::new(tmp.path().join("cache"));
    let out = tmp.path().join("out");
    let run = || pipeline::run(&spec, tmp.path(), Some(&cache), &out).unwrap();

    let cold = synth(&run());
    assert!(cold.len() > 2 && cold.iter().all(|hit| !hit), "{cold:?}");
    let first = fs::read(out.join("episode.wav")).unwrap();

    let warm = run();
    assert!(
        synth(&warm).iter().all(|hit| *hit),
        "a warm rerun calls no TTS"
    );
    assert!(warm.stages.iter().all(|s| s.cache_hit));
    assert_eq!(fs::read(out.join("episode.wav")).unwrap(), first);

    let lost = &manifest(&out).chunks[1];
    fs::remove_file(cache.blobs().path_for(&lost.blob)).unwrap();
    let again = synth(&run());
    let misses: Vec<usize> = again
        .iter()
        .enumerate()
        .filter(|(_, hit)| !**hit)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(misses, [1], "only the chunk whose blob was lost");
    assert_eq!(
        fs::read(out.join("episode.wav")).unwrap(),
        first,
        "the derived seed reproduces the lost take"
    );
}

#[test]
fn a_missing_voice_clip_fails_before_any_stage_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = audio_episode(tmp.path(), "kind = \"fake\"");
    fs::remove_file(tmp.path().join("voices/guest.wav")).unwrap();
    let err = pipeline::run(&spec, tmp.path(), None, &tmp.path().join("out")).unwrap_err();
    assert!(
        matches!(&err, CoreError::Config { message } if message.contains("guest.wav")),
        "{err}"
    );
    assert!(!tmp.path().join("out").exists(), "nothing ran");
}

#[test]
fn an_unknown_sidecar_profile_fails_before_any_stage_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = audio_episode(tmp.path(), "kind = \"sidecar\"\nsidecar = \"nope\"");
    let profiles = tmp.path().join("sidecars.toml");
    fs::write(
        &profiles,
        "[sidecars.stub]\nprogram = \"python3\"\nargs = []\n",
    )
    .unwrap();
    let err =
        pipeline::run_with_sidecars(&spec, &profiles, tmp.path(), None, &tmp.path().join("out"))
            .unwrap_err();
    assert!(
        matches!(&err, CoreError::Config { message } if message.contains("[sidecars.nope]")),
        "{err}"
    );
}

fn python_ok() -> bool {
    let ok = Command::new("python3")
        .args(["-c", "import sys; sys.exit(sys.version_info < (3, 11))"])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("skipping: python3 3.11+ is not available");
    }
    ok
}

/// Pids of live processes whose command line contains `tag` (Linux `/proc`).
fn processes_tagged(tag: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    for entry in fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse().ok()) else {
            continue;
        };
        let cmdline = fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let state = fs::read_to_string(entry.path().join("stat")).unwrap_or_default();
        // A zombie (state Z) has exited; only its exit status is left.
        let zombie = state
            .rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'));
        if !zombie && String::from_utf8_lossy(&cmdline).contains(tag) {
            pids.push(pid);
        }
    }
    pids
}

#[test]
fn no_worker_is_left_running_once_the_run_returns() {
    if !python_ok() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let spec = audio_episode(tmp.path(), "kind = \"sidecar\"\nsidecar = \"stub\"");
    let tag = format!("podling-e2e-{}", std::process::id());
    let profiles = tmp.path().join("sidecars.toml");
    let stub = fixtures().join("fake_sidecar.py");
    fs::write(
        &profiles,
        format!(
            "[sidecars.stub]\nprogram = \"python3\"\nargs = [{:?}, \"--tag\", {tag:?}]\n",
            stub.display().to_string()
        ),
    )
    .unwrap();
    let out = tmp.path().join("out");

    let report = pipeline::run_with_sidecars(&spec, &profiles, tmp.path(), None, &out).unwrap();
    assert!(synth(&report).iter().all(|hit| !hit));
    assert!(out.join("episode.wav").is_file());
    assert_eq!(
        processes_tagged(&tag),
        Vec::<u32>::new(),
        "the worker is gone"
    );
}
