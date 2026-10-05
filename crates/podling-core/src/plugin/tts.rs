//! Text-to-speech providers: turn a chunk of script into audio.
//!
//! One request shape covers both kinds of model. A dialogue model takes
//! several speakers' turns at once; a per-turn model (Qwen3-TTS) is simply
//! sent one turn per chunk, which the chunk planner decides from
//! [`TtsCapabilities::multi_speaker`].

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

use podling_types::{Emotion, Nonverbal, SpeakerId, VoiceRef};
use serde::Serialize;
use serde_json::{Value, json};

use crate::audio::Pcm;
use crate::error::{CoreError, Result};

/// What a backend can do; the chunk planner reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TtsCapabilities {
    /// `false`: the planner sends one turn per chunk.
    pub multi_speaker: bool,
    /// Longest chunk the model handles well, e.g. 120 for Qwen3-TTS.
    pub max_chunk_secs: u32,
    pub max_speakers: u8,
    /// The rate the model produces; Podling resamples afterwards.
    pub native_sample_rate: u32,
    /// The model listens to a chunk's [`ChunkContext`]. When it doesn't, no
    /// context is sent and none goes into a chunk's cache key, so editing a
    /// turn leaves the next chunk alone.
    pub context: bool,
}

/// One turn (or part of one) as it will be spoken: quotes already filled
/// into `text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpokenTurn {
    pub speaker: SpeakerId,
    pub text: String,
    pub emotion: Emotion,
    /// Sounds the speaker makes just before or after the words, which the
    /// backend renders in the same voice (as its own tags, or not at all and
    /// reported as dropped). Sounds over the turn, or by someone else, are
    /// placed by the assembler instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nonverbal: Vec<Nonverbal>,
}

impl SpokenTurn {
    /// `text` spoken by `speaker`, with no sounds around it.
    pub fn plain(speaker: SpeakerId, text: impl Into<String>, emotion: Emotion) -> Self {
        Self {
            speaker,
            text: text.into(),
            emotion,
            nonverbal: Vec::new(),
        }
    }
}

/// What came just before a chunk. Conditioning only: none of it is spoken
/// in the output.
///
/// The `'a` lifetime says this struct only *borrows* what it points at, so
/// the compiler makes sure the turns and files it names live at least as
/// long as the request does.
#[derive(Debug, Clone, Copy)]
pub struct ChunkContext<'a> {
    /// The previous beat's turns.
    pub turns: &'a [SpokenTurn],
    /// The previous beat's audio, a WAV file.
    pub audio: Option<&'a Path>,
    /// Earlier clips a turn calls back to, WAV files.
    pub callbacks: &'a [&'a Path],
}

/// One chunk to synthesise.
#[derive(Debug, Clone, Copy)]
pub struct ChunkRequest<'a> {
    pub turns: &'a [SpokenTurn],
    /// The pinned reference clip of every speaker in `turns`, sent with every
    /// chunk so the voice never drifts. Paths are absolute (resolved against
    /// the episode directory by the caller).
    pub voices: &'a BTreeMap<SpeakerId, VoiceRef>,
    pub context: Option<ChunkContext<'a>>,
    pub seed: u64,
}

/// A chunk's audio.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkAudio {
    /// At the model's native rate; context is not included.
    pub pcm: Pcm,
    /// Where each turn starts and ends in `pcm`, in samples. Per-turn
    /// backends always fill this; a dialogue backend may return `None`, and
    /// speech recognition fills the spans in later.
    pub turn_spans: Option<Vec<Range<usize>>>,
}

/// A plugin that speaks script turns. Used as a trait object.
///
/// `synthesize` takes `&mut self`, unlike the other providers' `&self`: a
/// provider may own a running model worker and change its state. `&mut`
/// means "exclusive", so the borrow checker guarantees no other code uses
/// the provider at the same time, with no lock needed.
pub trait TtsProvider {
    fn id(&self) -> &str;

    /// Everything that can change the audio (backend, model, weights,
    /// protocol). Part of every chunk's cache key.
    fn fingerprint(&self) -> Value;

    fn capabilities(&self) -> &TtsCapabilities;

    fn synthesize(&mut self, request: &ChunkRequest<'_>) -> Result<ChunkAudio>;
}

/// Checks the request (every speaker has a voice), calls `provider`, and
/// checks what came back: non-empty, finite samples at a real rate, and, when
/// there are spans, one per turn, in order, non-overlapping, inside the
/// audio. A per-turn backend must return spans. Bad output is an
/// [`CoreError::InvalidProviderOutput`] of `stage`.
pub fn synthesize_checked(
    provider: &mut dyn TtsProvider,
    stage: &'static str,
    request: &ChunkRequest<'_>,
) -> Result<ChunkAudio> {
    if request.turns.is_empty() {
        return Err(CoreError::InvalidProviderOutput {
            stage,
            message: "a chunk needs at least one turn".into(),
        });
    }
    let context_turns = request.context.map(|c| c.turns).unwrap_or_default();
    for speaker in
        request.turns.iter().chain(context_turns).flat_map(|turn| {
            std::iter::once(&turn.speaker).chain(turn.nonverbal.iter().map(|n| &n.by))
        })
    {
        if !request.voices.contains_key(speaker) {
            return Err(CoreError::Config {
                message: format!(
                    "speaker {:?} has no voice: add it to [[cast]] with a reference clip",
                    speaker.0
                ),
            });
        }
    }

    let audio = provider.synthesize(request)?;
    let id = provider.id();
    let invalid = |message: String| CoreError::InvalidProviderOutput {
        stage,
        message: format!("TTS provider {id} {message}"),
    };
    let pcm = &audio.pcm;
    if pcm.rate == 0 {
        return Err(invalid("returned audio with a sample rate of 0".into()));
    }
    if pcm.is_empty() {
        return Err(invalid("returned no audio".into()));
    }
    if let Some(i) = pcm.samples.iter().position(|s| !s.is_finite()) {
        return Err(invalid(format!("returned a non-finite sample at {i}")));
    }
    match &audio.turn_spans {
        None if !provider.capabilities().multi_speaker => {
            return Err(invalid(
                "is a per-turn backend but returned no turn spans".into(),
            ));
        }
        None => {}
        Some(spans) => check_spans(spans, request.turns.len(), pcm.len()).map_err(invalid)?,
    }
    Ok(audio)
}

fn check_spans(
    spans: &[Range<usize>],
    turns: usize,
    samples: usize,
) -> std::result::Result<(), String> {
    if spans.len() != turns {
        return Err(format!(
            "returned {} turn spans for {turns} turns",
            spans.len()
        ));
    }
    let mut previous_end = 0;
    for (turn, span) in spans.iter().enumerate() {
        if span.start >= span.end {
            return Err(format!("returned an empty span for turn {turn}: {span:?}"));
        }
        if span.start < previous_end {
            return Err(format!(
                "returned a span for turn {turn} ({span:?}) overlapping the turn before"
            ));
        }
        if span.end > samples {
            return Err(format!(
                "returned a span for turn {turn} ({span:?}) past the end of the audio ({samples} samples)"
            ));
        }
        previous_end = span.end;
    }
    Ok(())
}

/// A deterministic, offline stand-in: a sine tone per turn, its pitch picked
/// by the speaker and its length by the word count (a quarter second a word).
/// The seed shifts the phase, so two seeds give different samples, as two
/// takes of a real model would.
#[derive(Debug, Clone)]
pub struct FakeTts {
    capabilities: TtsCapabilities,
}

const FAKE_RATE: u32 = 24_000;
const FAKE_SECONDS_PER_WORD: f32 = 0.25;

/// Behaves like a per-turn model (Qwen3-TTS): one turn per chunk, and no
/// use for context.
impl Default for FakeTts {
    fn default() -> Self {
        Self {
            capabilities: TtsCapabilities {
                multi_speaker: false,
                max_chunk_secs: 120,
                max_speakers: 8,
                native_sample_rate: FAKE_RATE,
                context: false,
            },
        }
    }
}

impl FakeTts {
    /// Behaves like a dialogue model: whole beats per chunk, conditioned on
    /// the chunk before. The audio is the same either way; only how the
    /// script is chunked and keyed differs.
    pub fn dialogue() -> Self {
        let mut fake = Self::default();
        fake.capabilities.multi_speaker = true;
        fake.capabilities.context = true;
        fake
    }

    fn pitch(speaker: &SpeakerId) -> f32 {
        let hash = blake3::hash(speaker.0.as_bytes());
        120.0 + f32::from(hash.as_bytes()[0]) * (200.0 / 255.0)
    }
}

impl TtsProvider for FakeTts {
    fn id(&self) -> &str {
        "fake"
    }

    fn fingerprint(&self) -> Value {
        json!({ "id": "fake", "version": 1 })
    }

    fn capabilities(&self) -> &TtsCapabilities {
        &self.capabilities
    }

    fn synthesize(&mut self, request: &ChunkRequest<'_>) -> Result<ChunkAudio> {
        let rate = FAKE_RATE as f32;
        let phase = (request.seed % 360) as f32 * (std::f32::consts::TAU / 360.0);
        let mut samples = Vec::new();
        let mut spans = Vec::with_capacity(request.turns.len());
        for turn in request.turns {
            let words = turn.text.split_whitespace().count().max(1);
            let n = (words as f32 * FAKE_SECONDS_PER_WORD * rate) as usize;
            let step = std::f32::consts::TAU * Self::pitch(&turn.speaker) / rate;
            let start = samples.len();
            samples.extend((0..n).map(|i| 0.3 * (phase + step * i as f32).sin()));
            spans.push(start..samples.len());
        }
        Ok(ChunkAudio {
            pcm: Pcm::new(FAKE_RATE, samples),
            turn_spans: Some(spans),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(speaker: &str, text: &str) -> SpokenTurn {
        SpokenTurn::plain(SpeakerId(speaker.into()), text, Emotion::Neutral)
    }

    fn voices(ids: &[&str]) -> BTreeMap<SpeakerId, VoiceRef> {
        ids.iter()
            .map(|id| {
                let voice = VoiceRef::new(format!("/v/{id}.wav"), "Hello.", "CC0-1.0").unwrap();
                (SpeakerId((*id).into()), voice)
            })
            .collect()
    }

    /// Returns whatever it is told to, to exercise the checks.
    struct Canned {
        audio: ChunkAudio,
        capabilities: TtsCapabilities,
    }

    impl Canned {
        fn new(samples: Vec<f32>, spans: Option<Vec<Range<usize>>>) -> Self {
            Self {
                audio: ChunkAudio {
                    pcm: Pcm::new(24_000, samples),
                    turn_spans: spans,
                },
                capabilities: FakeTts::dialogue().capabilities,
            }
        }
    }

    impl TtsProvider for Canned {
        fn id(&self) -> &str {
            "canned"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn capabilities(&self) -> &TtsCapabilities {
            &self.capabilities
        }
        fn synthesize(&mut self, _: &ChunkRequest<'_>) -> Result<ChunkAudio> {
            Ok(self.audio.clone())
        }
    }

    fn check(provider: &mut dyn TtsProvider, turns: &[SpokenTurn]) -> Result<ChunkAudio> {
        let voices = voices(&["ada", "bo"]);
        let request = ChunkRequest {
            turns,
            voices: &voices,
            context: None,
            seed: 7,
        };
        synthesize_checked(provider, "synthesize", &request)
    }

    fn rejection(mut provider: Canned) -> String {
        let turns = [turn("ada", "one two"), turn("bo", "three")];
        match check(&mut provider, &turns) {
            Err(CoreError::InvalidProviderOutput { stage, message }) => {
                assert_eq!(stage, "synthesize");
                message
            }
            other => panic!("expected invalid output, got {other:?}"),
        }
    }

    #[test]
    fn fake_tts_is_deterministic_with_exact_spans() {
        let turns = [turn("ada", "one two three four"), turn("bo", "five six")];
        let mut fake = FakeTts::default();
        let audio = check(&mut fake, &turns).unwrap();
        assert_eq!(audio.pcm.rate, 24_000);
        assert_eq!(audio.turn_spans, Some(vec![0..24_000, 24_000..36_000]));
        assert_eq!(audio, check(&mut fake, &turns).unwrap());

        let voices = voices(&["ada", "bo"]);
        let other_seed = ChunkRequest {
            turns: &turns,
            voices: &voices,
            context: None,
            seed: 8,
        };
        assert_ne!(audio, fake.synthesize(&other_seed).unwrap());
        assert_ne!(
            FakeTts::pitch(&turns[0].speaker),
            FakeTts::pitch(&turns[1].speaker)
        );
    }

    #[test]
    fn empty_and_non_finite_audio_are_rejected() {
        assert!(rejection(Canned::new(vec![], None)).contains("no audio"));
        let message = rejection(Canned::new(vec![0.0, f32::NAN], None));
        assert!(message.contains("non-finite sample at 1"), "{message}");
        let mut zero_rate = Canned::new(vec![0.0; 4], None);
        zero_rate.audio.pcm.rate = 0;
        assert!(rejection(zero_rate).contains("sample rate of 0"));
    }

    #[test]
    fn bad_spans_are_rejected() {
        let samples = vec![0.0; 10];
        let cases = [
            (vec![0..3, 3..6, 6..9], "3 turn spans for 2 turns"),
            (vec![0..5, 5..5], "empty span for turn 1"),
            (vec![0..6, 5..10], "overlapping"),
            (vec![0..5, 5..11], "past the end"),
        ];
        for (spans, expected) in cases {
            let message = rejection(Canned::new(samples.clone(), Some(spans)));
            assert!(message.contains(expected), "{message}");
        }
        // Gaps between turns are fine.
        let turns = [turn("ada", "a"), turn("bo", "b")];
        check(&mut Canned::new(samples, Some(vec![0..3, 6..10])), &turns).unwrap();
    }

    #[test]
    fn a_per_turn_backend_must_return_spans() {
        let mut provider = Canned::new(vec![0.0; 10], None);
        let turns = [turn("ada", "a"), turn("bo", "b")];
        check(&mut provider, &turns).expect("a dialogue backend may omit spans");
        provider.capabilities.multi_speaker = false;
        assert!(rejection(provider).contains("no turn spans"));
    }

    #[test]
    fn a_speaker_without_a_voice_is_a_config_error() {
        let turns = [turn("ada", "hi"), turn("cy", "hello")];
        let Err(CoreError::Config { message }) = check(&mut FakeTts::default(), &turns) else {
            panic!("expected a Config error");
        };
        assert!(message.contains("\"cy\""), "{message}");

        let mut laughing = turn("ada", "hi");
        laughing.nonverbal.push(Nonverbal {
            kind: podling_types::NonverbalKind::Laugh {},
            by: SpeakerId("dee".into()),
            at: podling_types::NonverbalAt::Before,
        });
        let Err(CoreError::Config { message }) = check(&mut FakeTts::default(), &[laughing]) else {
            panic!("a sound needs its maker's voice too");
        };
        assert!(message.contains("\"dee\""), "{message}");
        assert!(matches!(
            check(&mut FakeTts::default(), &[]),
            Err(CoreError::InvalidProviderOutput { .. })
        ));
    }
}
