//! The candle Whisper against Hugging Face transformers.
//!
//! `tests/fixtures/whisper/reference.json` holds what
//! `scripts/whisper_reference.py` got from transformers (long-form decoding,
//! the same temperature fallback) for two bake-off clips. The clips and the
//! weights are not in the repository, so this test is ignored by default:
//!
//!   hf download openai/whisper-base.en
//!   PODLING_WHISPER_MODEL_DIR=<snapshot dir> \
//!     cargo test --release -p podling-core --test whisper_parity -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::time::Instant;

use podling_core::audio::Pcm;
use podling_core::plugin::{AsrProvider, AsrRequest, CandleWhisper, transcribe_checked};
use podling_core::stages::verify_audio::{wer_pm, words};
use serde::Deserialize;

/// Largest word error rate allowed between the two transcripts, per mille.
/// Greedy decoding on different kernels can pick a different token where two
/// are almost equally likely; anything past a word or two is a real bug.
const TOLERANCE_PM: u16 = 30;

#[derive(Deserialize)]
struct Reference {
    clips: Vec<Clip>,
}

#[derive(Deserialize)]
struct Clip {
    /// Relative to the repository root.
    path: PathBuf,
    text: String,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn reference() -> Reference {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/whisper/reference.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
#[ignore = "needs the Whisper model and bake-off clips; set PODLING_WHISPER_MODEL_DIR"]
fn parity_with_transformers() {
    let Ok(dir) = std::env::var("PODLING_WHISPER_MODEL_DIR") else {
        // Fail, don't skip: an ignored test that returns early reports "ok".
        panic!("set PODLING_WHISPER_MODEL_DIR to a local openai/whisper-base.en snapshot");
    };
    let mut whisper = CandleWhisper::new(&PathBuf::from(dir)).unwrap();
    for clip in reference().clips {
        let path = repo_root().join(&clip.path);
        let pcm = Pcm::read_wav(&path).unwrap_or_else(|err| {
            panic!(
                "{}: {err}; make it with scripts/tts_bakeoff",
                path.display()
            )
        });
        let request = AsrRequest {
            pcm: &pcm,
            expected: &[],
        };
        let started = Instant::now();
        let heard = transcribe_checked(&mut whisper, "parity", &request).unwrap();
        let elapsed = started.elapsed().as_secs_f64();

        let rate = wer_pm(&words(&clip.text), &words(&heard.text()));
        println!(
            "{}: {:.1} s of audio in {elapsed:.1} s (RTF {:.2}), {} segments, {}‰ apart",
            clip.path.display(),
            pcm.seconds(),
            elapsed / pcm.seconds(),
            heard.segments.len(),
            rate.get()
        );
        assert!(
            rate.get() <= TOLERANCE_PM,
            "{}‰ apart\n transformers: {}\n candle:       {}",
            rate.get(),
            clip.text,
            heard.text()
        );
    }
    assert_eq!(whisper.id(), "whisper");
}
