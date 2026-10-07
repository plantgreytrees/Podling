//! `SidecarTts` against real worker processes on `127.0.0.1`:
//! - `tests/fixtures/fake_sidecar.py`, a stub that misbehaves on demand, for
//!   the failure paths and the exact request body;
//! - the real `sidecars/tts` worker on its `fake` backend (stdlib only), so
//!   the two sides of protocol v1 are checked against each other.
//!
//! Both need `python3` (3.11+); without it the tests print a note and pass.
//! Process checks read `/proc`, so they are Linux-only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use podling_core::audio::{Pcm, WavFormat};
use podling_core::plugin::{
    ChunkContext, ChunkRequest, SidecarProfile, SidecarTts, SpokenTurn, TtsProvider,
    synthesize_checked,
};
use podling_core::{CoreError, ProviderFailure};
use podling_types::{Emotion, EpisodeSpec, SpeakerId, VoiceRef};
use serde_json::{Value, json};

const PROFILES: &str = "/test/sidecars.toml";

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

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_sidecar.py")
}

/// The marker a test's worker carries on its command line: the test's tag
/// and this run's pid, so concurrent runs of the suite (other worktrees'
/// gates) never find each other's workers in `/proc`.
fn needle(tag: &str) -> String {
    format!("podling-test-{}-{tag}", std::process::id())
}

/// A unique tag per test, so a test can find its own worker in `/proc`.
fn stub(mode: &str, tag: &str) -> SidecarProfile {
    SidecarProfile {
        program: "python3".into(),
        args: vec![
            fixture().display().to_string(),
            "--mode".into(),
            mode.into(),
            "--tag".into(),
            needle(tag),
        ],
    }
}

fn start(profile: &SidecarProfile) -> Result<SidecarTts, CoreError> {
    SidecarTts::start_with_timeout(
        "stub",
        profile,
        Path::new(PROFILES),
        Duration::from_secs(20),
    )
}

/// Pids of live (not zombie) processes whose command line contains `tag`.
fn running(tag: &str) -> Vec<u32> {
    let needle = needle(tag);
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse().ok()) else {
            continue;
        };
        if alive(pid)
            && std::fs::read(entry.path().join("cmdline"))
                .is_ok_and(|c| String::from_utf8_lossy(&c).contains(&needle))
        {
            pids.push(pid);
        }
    }
    pids
}

/// A reaped process has no `/proc` entry; a zombie is dead too.
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        !stat
            .rsplit_once(')')
            .is_some_and(|(_, rest)| rest.starts_with(" Z"))
    })
}

#[test]
fn another_runs_worker_is_not_counted() {
    if !python_ok() {
        return;
    }
    // A worker of another run of this suite (another worktree's gate, say),
    // tagged the way that run would tag it, with its own pid.
    let foreign = format!("podling-test-{}-decoy", std::process::id() + 1);
    let mut decoy = Command::new("python3")
        .args([
            "-c",
            "import time; time.sleep(30)",
            "podling-test-decoy",
            &foreign,
        ])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !alive(decoy.id()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let seen = running("decoy");
    decoy.kill().unwrap();
    decoy.wait().unwrap();
    assert!(
        seen.is_empty(),
        "another run's worker was counted: {seen:?}"
    );
}

fn turn(speaker: &str, text: &str, emotion: Emotion) -> SpokenTurn {
    SpokenTurn::plain(SpeakerId(speaker.into()), text, emotion)
}

/// Voice clips for `ada` and `bo`, written into `dir`.
fn voices(dir: &Path) -> BTreeMap<SpeakerId, VoiceRef> {
    ["ada", "bo"]
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let path = dir.join(format!("{id}.wav"));
            let pcm = Pcm::new(24_000, vec![0.05 * (i + 1) as f32; 2400]);
            std::fs::write(&path, pcm.to_wav(WavFormat::Float32).unwrap()).unwrap();
            let voice = VoiceRef::new(path, format!("I am {id}."), "CC0-1.0").unwrap();
            (SpeakerId((*id).into()), voice)
        })
        .collect()
}

fn provider_message(err: CoreError) -> String {
    match err {
        CoreError::Provider { message, .. } => message,
        other => panic!("expected a provider error, got {other:?}"),
    }
}

#[test]
fn synthesises_through_the_stub_and_stops_it_on_drop() {
    if !python_ok() {
        return;
    }
    let clips = tempfile::tempdir().unwrap();
    let voices = voices(clips.path());
    let mut tts = start(&stub("ok", "ok")).unwrap();
    assert_eq!(
        tts.fingerprint(),
        json!({ "id": "sidecar", "protocol": 1, "backend": "stub",
                "model": "stub-model", "weights": "abc123", "adapter": 1 })
    );
    assert!(!tts.capabilities().multi_speaker);

    let mut respelt = turn("bo", "Over Siberia.", Emotion::Neutral);
    respelt.say_as = Some("Over Sigh-beer-ia.".into());
    let turns = [
        turn("ada", "The sky split in two.", Emotion::Excited),
        respelt,
    ];
    let previous = [turn("bo", "Where was this?", Emotion::Curious)];
    let previous_audio = clips.path().join("ada.wav");
    let request = ChunkRequest {
        turns: &turns,
        voices: &voices,
        context: Some(ChunkContext {
            turns: &previous,
            audio: Some(&previous_audio),
            callbacks: &[],
        }),
        seed: 1908,
    };
    let audio = synthesize_checked(&mut tts, "synthesize", &request).unwrap();
    assert_eq!((audio.pcm.rate, audio.pcm.len()), (24_000, 4800));
    assert_eq!(audio.turn_spans, Some(vec![0..2400, 2400..4800]));

    // What the worker was sent: only paths inside its run directory.
    let run_dir = tts.run_dir().to_owned();
    let sent: Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("last_request.json")).unwrap())
            .unwrap();
    assert_eq!(sent["seed"], 1908);
    assert_eq!(sent["turns"][0]["emotion"], "excited");
    assert!(
        sent["turns"][1].get("emotion").is_none(),
        "neutral is not sent"
    );
    assert_eq!(sent["context"]["turns"][0]["text"], "Where was this?");
    // A respelling is the wire text; the worker never sees the field.
    assert_eq!(sent["turns"][0]["text"], "The sky split in two.");
    assert_eq!(sent["turns"][1]["text"], "Over Sigh-beer-ia.");
    assert!(sent["turns"][1].get("say_as").is_none());
    for path in [
        &sent["voices"]["ada"]["reference"],
        &sent["voices"]["bo"]["reference"],
        &sent["context"]["audio"],
        &sent["out_path"],
    ] {
        let path = Path::new(path.as_str().unwrap());
        assert!(
            path.starts_with(&run_dir),
            "{} is outside the run dir",
            path.display()
        );
    }
    assert_eq!(sent["voices"]["bo"]["transcript"], "I am bo.");
    let out = Path::new(sent["out_path"].as_str().unwrap());
    assert!(!out.exists(), "the output WAV is read and removed");

    let pid = tts.pid();
    assert!(alive(pid));
    drop(tts);
    assert!(
        !alive(pid),
        "the worker is gone once the provider is dropped"
    );
    assert!(running("ok").is_empty());
    assert!(!run_dir.exists(), "the run directory is removed");
}

#[test]
fn a_worker_that_crashes_at_start_gets_a_gpu_hint() {
    if !python_ok() {
        return;
    }
    let message = provider_message(start(&stub("crash", "crash")).unwrap_err());
    for expected in [
        "exited before it was ready",
        "exit code 3",
        "CUDA out of memory",
        "ollama stop",
        PROFILES,
    ] {
        assert!(
            message.contains(expected),
            "missing {expected:?} in {message}"
        );
    }
}

#[test]
fn a_worker_that_never_gets_ready_is_stopped() {
    if !python_ok() {
        return;
    }
    let profile = stub("hang", "hang");
    let started = Instant::now();
    let err = SidecarTts::start_with_timeout(
        "stub",
        &profile,
        Path::new(PROFILES),
        Duration::from_secs(1),
    )
    .unwrap_err();
    let message = provider_message(err);
    assert!(
        message.contains("did not report ready within 1 s"),
        "{message}"
    );
    assert!(message.contains("[sidecars.stub]"), "{message}");
    assert!(started.elapsed() < Duration::from_secs(8));
    assert!(running("hang").is_empty(), "the hung worker was stopped");
}

#[test]
fn a_worker_on_another_protocol_is_refused() {
    if !python_ok() {
        return;
    }
    let message = provider_message(start(&stub("wrong-protocol", "proto")).unwrap_err());
    assert!(message.contains("protocol 2"), "{message}");
    assert!(running("proto").is_empty());
}

#[test]
fn a_worker_that_ignores_sigterm_is_killed() {
    if !python_ok() {
        return;
    }
    let tts = start(&stub("stubborn", "stubborn")).unwrap();
    let pid = tts.pid();
    let started = Instant::now();
    drop(tts);
    assert!(!alive(pid));
    assert!(
        started.elapsed() >= Duration::from_secs(4),
        "SIGTERM was ignored, so the kill came after the grace period"
    );
}

#[test]
fn a_child_of_the_worker_that_ignores_sigterm_is_killed_too() {
    if !python_ok() {
        return;
    }
    let tts = start(&stub("orphan", "orphan")).unwrap();
    let pid = tts.pid();
    let deadline = Instant::now() + Duration::from_secs(5);
    while running("orphan").len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(running("orphan").len(), 2, "the worker and its child run");

    drop(tts);
    assert!(!alive(pid));
    assert!(
        running("orphan").is_empty(),
        "the worker's child is gone too: {:?}",
        running("orphan")
    );
}

#[test]
fn a_child_left_behind_by_a_dead_worker_is_killed() {
    if !python_ok() {
        return;
    }
    // Like `uv run` crashing while the model process it started runs on.
    let tts = start(&stub("orphan-exit", "dead-parent")).unwrap();
    let pid = tts.pid();
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(pid), "the worker exited on its own");
    assert_eq!(
        running("dead-parent").len(),
        1,
        "its child runs on without it"
    );

    drop(tts);
    assert!(
        running("dead-parent").is_empty(),
        "the child is gone too: {:?}",
        running("dead-parent")
    );
}

fn synthesize_once(tts: &mut SidecarTts, dir: &Path) -> Result<(), CoreError> {
    let voices = voices(dir);
    let turns = [turn("ada", "Hello there.", Emotion::Neutral)];
    let request = ChunkRequest {
        turns: &turns,
        voices: &voices,
        context: None,
        seed: 1,
    };
    synthesize_checked(tts, "synthesize", &request).map(|_| ())
}

#[test]
fn a_busy_gpu_gets_the_ollama_hint() {
    if !python_ok() {
        return;
    }
    let clips = tempfile::tempdir().unwrap();
    let mut tts = start(&stub("busy", "busy")).unwrap();
    let err = synthesize_once(&mut tts, clips.path()).unwrap_err();
    assert_eq!(err.provider_failure(), Some(ProviderFailure::Http(503)));
    let message = provider_message(err);
    assert!(message.contains("900 MiB"), "{message}");
    assert!(message.contains("ollama stop"), "{message}");
}

#[test]
fn a_worker_that_dies_mid_request_is_reported_with_its_log() {
    if !python_ok() {
        return;
    }
    let clips = tempfile::tempdir().unwrap();
    let mut tts = start(&stub("die", "die")).unwrap();
    let message = provider_message(synthesize_once(&mut tts, clips.path()).unwrap_err());
    assert!(message.contains("died during a request"), "{message}");
    assert!(message.contains("exit code 9"), "{message}");
    assert!(message.contains("Segmentation fault"), "{message}");
}

#[test]
fn a_reply_that_disagrees_with_the_wav_is_invalid_output() {
    if !python_ok() {
        return;
    }
    let clips = tempfile::tempdir().unwrap();
    let mut tts = start(&stub("liar", "liar")).unwrap();
    match synthesize_once(&mut tts, clips.path()) {
        Err(CoreError::InvalidProviderOutput { message, .. }) => {
            assert!(message.contains("reported 2401 samples"), "{message}");
        }
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[test]
fn round_trip_with_the_real_worker_on_its_fake_backend() {
    if !python_ok() {
        return;
    }
    let sidecar_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sidecars/tts");
    let profile = SidecarProfile {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            "import sys; sys.path.insert(0, sys.argv.pop(1)); \
             from podling_tts.server import main; main()"
                .into(),
            sidecar_dir.canonicalize().unwrap().display().to_string(),
            "--backend".into(),
            "fake".into(),
            // Quiet: the worker logs every request at INFO.
            "--log-level".into(),
            "WARNING".into(),
        ],
    };
    let clips = tempfile::tempdir().unwrap();
    let voices = voices(clips.path());
    let mut tts = start(&profile).unwrap();
    assert_eq!(tts.fingerprint()["backend"], "fake");
    assert_eq!(tts.fingerprint()["protocol"], 1);

    let turns = [
        turn("ada", "one two three four", Emotion::Amused),
        turn("bo", "five six", Emotion::Neutral),
    ];
    let request = ChunkRequest {
        turns: &turns,
        voices: &voices,
        context: None,
        seed: 42,
    };
    let audio = synthesize_checked(&mut tts, "synthesize", &request).unwrap();
    // The fake backend speaks a quarter second a word at 24 kHz.
    assert_eq!(audio.pcm.rate, 24_000);
    assert_eq!(audio.turn_spans, Some(vec![0..24_000, 24_000..36_000]));
    assert_eq!(audio.pcm.len(), 36_000);
    let again = synthesize_checked(&mut tts, "synthesize", &request).unwrap();
    assert_eq!(audio, again, "same seed, same audio");

    let pid = tts.pid();
    drop(tts);
    assert!(!alive(pid));
}

#[test]
fn an_episode_file_cannot_name_a_program() {
    let episode = |tts: &str| {
        toml::from_str::<EpisodeSpec>(&format!(
            "title = \"T\"\ntopic = \"T\"\ntarget_minutes = 5\nllm = {{ kind = \"fake\" }}\n\
             sources = []\n{tts}"
        ))
    };
    episode("tts = { kind = \"sidecar\", sidecar = \"qwen\" }").unwrap();
    for smuggled in [
        "tts = { kind = \"sidecar\", sidecar = \"qwen\", program = \"/bin/sh\" }",
        "tts = { kind = \"sidecar\", sidecar = \"qwen\", args = [\"-c\", \"rm -rf ~\"] }",
        "tts = { kind = \"sidecar\", sidecar = \"qwen\", command = \"sh -c x\" }",
    ] {
        let err = episode(smuggled).unwrap_err().to_string();
        assert!(err.contains("unknown field"), "{smuggled}: {err}");
    }
}
