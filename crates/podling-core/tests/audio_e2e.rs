//! The audio half of a run: the fixture episode with `[[cast]]`, `[tts]` and
//! `[asr]`, offline with the fake TTS, and once through a real (stub) worker
//! process to show it is gone when the run returns.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use podling_core::audio::{Pcm, WavFormat};
use podling_core::plugin::{FakeAsr, FakeLlm, FakeTts};
use podling_core::stages::assemble::{TARGET_LUFS, measure};
use podling_core::stages::{Takes, Verification, Voices, synthesize_script};
use podling_core::{CoreError, DiskCache, RunReport, pipeline};
use podling_types::{
    AnalysisReport, AudioManifest, Beat, BeatKind, CastMember, Emotion, Envelope, EpisodeSpec,
    NonverbalAt, Pace, PerMille, Script, Severity, Speaker, SpeakerId, Turn, TurnRange, VoiceRef,
};
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

/// Whether each run of stage `id` was a cache hit, in order.
fn hits(report: &RunReport, id: &str) -> Vec<bool> {
    report
        .stages
        .iter()
        .filter(|s| s.id == id)
        .map(|s| s.cache_hit)
        .collect()
}

fn synth(report: &RunReport) -> Vec<bool> {
    hits(report, "synthesize_chunk")
}

fn transcribed(report: &RunReport) -> Vec<bool> {
    hits(report, "transcribe_chunk")
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
    assert!(loudness.true_peak_dbtp <= -1.0, "{loudness:?}");

    let manifest = manifest(&out);
    assert!(manifest.episode.true_peak_dbtp <= -1.0);
    assert_eq!(manifest.episode.encoded, None);
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

/// A chunk the recogniser mishears once is regenerated, and its second take
/// is the one kept.
#[test]
fn a_misheard_chunk_is_regenerated_and_its_second_take_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = audio_episode(tmp.path(), "kind = \"fake\"");
    let out = tmp.path().join("out");
    let asr = Box::new(FakeAsr::mishearing_first(1));
    let report = pipeline::run_with_asr(&spec, &FakeLlm, asr, tmp.path(), None, &out).unwrap();

    let manifest = manifest(&out);
    let chunks = manifest.chunks.len();
    // One regeneration, and the script's one "mm-hm" over another turn
    // (made on its own, never transcribed).
    assert_eq!(synth(&report).len(), chunks + 2, "one regeneration");
    assert_eq!(transcribed(&report).len(), chunks + 1);
    let first = &manifest.chunks[0];
    assert_eq!((first.take, first.verified), (1, true), "take 2 was kept");
    assert!(
        manifest.chunks[1..]
            .iter()
            .all(|c| c.take == 0 && c.verified)
    );
    assert_eq!(report.error_findings, 0);
}

/// A chunk that never passes is kept, so the run completes, and is reported
/// as an `Error` finding naming its turns.
#[test]
fn a_chunk_that_never_passes_becomes_an_error_finding() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = audio_episode(tmp.path(), "kind = \"fake\"");
    let out = tmp.path().join("out");
    let asr = Box::new(FakeAsr::mishearing_first(u32::MAX));
    let report = pipeline::run_with_asr(&spec, &FakeLlm, asr, tmp.path(), None, &out).unwrap();

    let manifest = manifest(&out);
    assert!(manifest.chunks.iter().all(|c| !c.verified));
    assert!(out.join("episode.wav").is_file(), "the run still completes");
    let text = fs::read_to_string(out.join("analysis.json")).unwrap();
    let analysis: Envelope<AnalysisReport> = serde_json::from_str(&text).unwrap();
    let unverified: Vec<_> = analysis
        .body
        .findings
        .iter()
        .filter(|f| f.analyser == "verify_audio")
        .collect();
    assert_eq!(unverified.len(), manifest.chunks.len());
    assert!(unverified.iter().all(|f| f.severity == Severity::Error));
    assert!(
        unverified[0].message.contains("every take"),
        "{unverified:?}"
    );
    assert_eq!(report.error_findings, analysis.body.error_count());
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

/// A 30-minute script at 150 words a minute: 90 banter beats, each two
/// turns of 25 words (20 s a beat). `edit` changes one turn's words.
fn half_hour_script(edit: Option<usize>) -> Script {
    let speaker = |id: &str| Speaker {
        id: SpeakerId(id.into()),
        name: id.into(),
        role: "host".into(),
    };
    let turns = (0..180)
        .map(|t| {
            let edited = if edit == Some(t) { "x" } else { "" };
            let text: Vec<String> = (0..25).map(|i| format!("t{t}{edited}w{i}")).collect();
            Turn {
                speaker: SpeakerId(if t % 2 == 0 { "host" } else { "guest" }.into()),
                text: format!("{}.", text.join(" ")),
                emotion: Emotion::Neutral,
                citations: vec![],
                quotes: vec![],
                pace: Pace::Normal,
                nonverbal: vec![],
                callback_to: None,
            }
        })
        .collect();
    let beats = (0..90)
        .map(|b| Beat {
            kind: BeatKind::Banter,
            turns: TurnRange::new(2 * b, 2 * b + 2).unwrap(),
        })
        .collect();
    Script::with_beats(vec![speaker("host"), speaker("guest")], turns, beats).unwrap()
}

#[test]
fn a_thirty_minute_script_is_chunked_by_beat_and_cached_per_chunk() {
    let started = Instant::now();
    let tmp = tempfile::tempdir().unwrap();
    let cast: Vec<CastMember> = ["host", "guest"]
        .iter()
        .map(|id| {
            fs::write(tmp.path().join(format!("{id}.wav")), id.as_bytes()).unwrap();
            CastMember {
                id: SpeakerId((*id).into()),
                name: (*id).into(),
                role: "host".into(),
                voice: VoiceRef::new(format!("{id}.wav"), "Hello.", "CC0-1.0").unwrap(),
            }
        })
        .collect();
    let voices = Voices::resolve(&cast, tmp.path()).unwrap();
    let cache = DiskCache::new(tmp.path().join("cache"));
    let blobs = cache.blobs();
    let run = |script: &Script| {
        let mut report = RunReport::default();
        // One take a chunk, so the counts below are chunks.
        let verification = Verification {
            asr: &mut FakeAsr::default(),
            takes: Takes {
                banter: 1,
                max_retries: 2,
                max_wer_pm: PerMille::new(80).unwrap(),
            },
        };
        let chunks = synthesize_script(
            script,
            &voices,
            &mut FakeTts::dialogue(),
            verification,
            &blobs,
            Some(&cache),
            &mut report,
        )
        .unwrap();
        let count = |hits: Vec<bool>| hits.iter().filter(|hit| !**hit).count();
        (chunks, (count(synth(&report)), count(transcribed(&report))))
    };

    let (chunks, misses) = run(&half_hour_script(None));
    // Three 20-second beats make a minute: 30 chunks, each ending on a beat.
    assert_eq!((chunks.len(), misses), (30, (30, 30)));
    assert!(chunks.iter().all(|c| c.record.verified));
    let mut next = 0;
    for chunk in &chunks {
        let turns = chunk.record.turns;
        assert_eq!(turns.start(), next, "every turn in exactly one chunk");
        assert_eq!(turns.end() % 2, 0, "no chunk ends inside a beat");
        next = turns.end();
    }
    assert_eq!(next, 180);

    let (_, misses) = run(&half_hour_script(None));
    assert_eq!(misses, (0, 0), "a warm rerun makes no TTS or ASR calls");
    let (_, misses) = run(&half_hour_script(Some(100)));
    assert_eq!(
        misses,
        (2, 2),
        "the edited chunk, and the next one, which hears it"
    );

    let elapsed = started.elapsed();
    assert!(elapsed.as_secs_f64() < 10.0, "took {elapsed:?}");
}
