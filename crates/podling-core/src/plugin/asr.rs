//! Speech recognition: hear what a chunk of synthesised audio actually says,
//! so a mumbled line or a skipped quote is caught before the episode ships.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::audio::Pcm;
use crate::error::{CoreError, Result};

/// A stretch of recognised speech, timed in seconds from the start of the
/// audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// What a recogniser heard, in order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Transcript {
    pub segments: Vec<Segment>,
}

impl Transcript {
    /// Every segment's text, joined by spaces.
    pub fn text(&self) -> String {
        let texts: Vec<&str> = self.segments.iter().map(|s| s.text.trim()).collect();
        texts.join(" ")
    }
}

/// One chunk to transcribe.
#[derive(Debug, Clone, Copy)]
pub struct AsrRequest<'a> {
    pub pcm: &'a Pcm,
    /// What the audio is meant to say, turn by turn. Only a fake may read
    /// it: a real recogniser that did would hear what it was told.
    pub expected: &'a [String],
}

/// A plugin that transcribes audio. Used as a trait object.
///
/// `transcribe` takes `&mut self`, as [`TtsProvider::synthesize`] does: a
/// real recogniser loads its model on first use and keeps decoder state
/// between calls.
///
/// [`TtsProvider::synthesize`]: super::TtsProvider::synthesize
pub trait AsrProvider {
    fn id(&self) -> &str;

    /// Everything that can change a transcript (model, weights, decoding
    /// rules). Part of every transcript's cache key.
    fn fingerprint(&self) -> Value;

    fn transcribe(&mut self, request: &AsrRequest<'_>) -> Result<Transcript>;
}

/// Calls `provider` and checks what came back: finite times, each segment
/// starting no later than it ends, segments in order, and none past the end
/// of the audio. Bad output is an [`CoreError::InvalidProviderOutput`] of
/// `stage`.
pub fn transcribe_checked(
    provider: &mut dyn AsrProvider,
    stage: &'static str,
    request: &AsrRequest<'_>,
) -> Result<Transcript> {
    let transcript = provider.transcribe(request)?;
    let id = provider.id();
    let invalid = |message: String| CoreError::InvalidProviderOutput {
        stage,
        message: format!("ASR provider {id} {message}"),
    };
    // Whisper times in steps of 20 ms; allow one step past the end.
    let duration = request.pcm.seconds() + 0.02;
    let mut previous_start = 0.0;
    for (i, segment) in transcript.segments.iter().enumerate() {
        let (start, end) = (segment.start, segment.end);
        if !start.is_finite() || !end.is_finite() {
            return Err(invalid(format!(
                "returned a non-finite time in segment {i}"
            )));
        }
        if start < previous_start || start > end || start < 0.0 {
            return Err(invalid(format!(
                "returned segment {i} at {start}..{end} s, out of order"
            )));
        }
        if end > duration {
            return Err(invalid(format!(
                "returned segment {i} ending at {end} s, past the end of the audio ({duration:.2} s)"
            )));
        }
        previous_start = start;
    }
    Ok(transcript)
}

/// A deterministic, offline stand-in that hears exactly what was meant: one
/// segment per expected turn, timed by its share of the words.
///
/// [`FakeAsr::mishearing_first`] scripts failures: the first `n` calls hear
/// nothing, so tests can drive the regenerate-and-retry path.
#[derive(Debug, Clone, Default)]
pub struct FakeAsr {
    mishear: u32,
}

impl FakeAsr {
    pub fn mishearing_first(calls: u32) -> Self {
        Self { mishear: calls }
    }
}

impl AsrProvider for FakeAsr {
    fn id(&self) -> &str {
        "fake"
    }

    fn fingerprint(&self) -> Value {
        json!({ "id": "fake", "version": 1 })
    }

    fn transcribe(&mut self, request: &AsrRequest<'_>) -> Result<Transcript> {
        if self.mishear > 0 {
            self.mishear -= 1;
            return Ok(Transcript::default());
        }
        let words = |text: &String| text.split_whitespace().count();
        let total: usize = request.expected.iter().map(words).sum();
        let seconds = request.pcm.seconds();
        let mut done = 0;
        let segments = request
            .expected
            .iter()
            .map(|text| {
                let start = seconds * done as f64 / total.max(1) as f64;
                done += words(text);
                Segment {
                    text: text.clone(),
                    start,
                    end: seconds * done as f64 / total.max(1) as f64,
                }
            })
            .collect();
        Ok(Transcript { segments })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request<'a>(pcm: &'a Pcm, expected: &'a [String]) -> AsrRequest<'a> {
        AsrRequest { pcm, expected }
    }

    #[test]
    fn the_fake_hears_each_turn_timed_by_its_words() {
        let pcm = Pcm::new(16_000, vec![0.0; 16_000 * 4]);
        let expected = ["One two three.".to_owned(), "Four.".to_owned()];
        let heard =
            transcribe_checked(&mut FakeAsr::default(), "t", &request(&pcm, &expected)).unwrap();
        assert_eq!(heard.text(), "One two three. Four.");
        let times: Vec<(f64, f64)> = heard.segments.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(times, [(0.0, 3.0), (3.0, 4.0)]);
    }

    #[test]
    fn a_scripted_fake_mishears_then_hears() {
        let pcm = Pcm::new(16_000, vec![0.0; 16_000]);
        let expected = ["Hello.".to_owned()];
        let mut fake = FakeAsr::mishearing_first(1);
        let first = fake.transcribe(&request(&pcm, &expected)).unwrap();
        assert_eq!(first.text(), "");
        let second = fake.transcribe(&request(&pcm, &expected)).unwrap();
        assert_eq!(second.text(), "Hello.");
    }

    /// Returns a canned transcript.
    struct Canned(Transcript);

    impl AsrProvider for Canned {
        fn id(&self) -> &str {
            "canned"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn transcribe(&mut self, _: &AsrRequest<'_>) -> Result<Transcript> {
            Ok(self.0.clone())
        }
    }

    fn rejection(segments: &[(f64, f64)]) -> String {
        let pcm = Pcm::new(16_000, vec![0.0; 16_000 * 2]);
        let transcript = Transcript {
            segments: segments
                .iter()
                .map(|&(start, end)| Segment {
                    text: "x".into(),
                    start,
                    end,
                })
                .collect(),
        };
        transcribe_checked(&mut Canned(transcript), "t", &request(&pcm, &[]))
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn bad_timings_are_rejected() {
        assert!(rejection(&[(0.0, f64::NAN)]).contains("non-finite"));
        assert!(rejection(&[(1.0, 0.5)]).contains("out of order"));
        assert!(rejection(&[(1.0, 1.5), (0.5, 1.0)]).contains("out of order"));
        assert!(rejection(&[(0.0, 2.5)]).contains("past the end"));
    }
}
