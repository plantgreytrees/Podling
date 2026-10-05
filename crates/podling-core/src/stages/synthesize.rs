//! Speaks the script, one chunk at a time, each chunk cached on its own.
//!
//! A chunk is one turn for now (the beat-aware planner comes later). Each
//! chunk runs through [`cached`] as a [`SynthesizeChunk`] stage, so a
//! 60-minute episode is many small cache entries: editing one turn
//! re-synthesises one chunk, and a crash loses only the chunk in flight. The
//! audio itself goes in the [`BlobStore`]; the cache entry names it by hash,
//! and an entry whose blob has gone is a miss.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::Path;
use std::time::Instant;

use podling_types::{
    CastMember, ChunkRecord, ContentHash, PerMille, Script, SpeakerId, TurnRange, VoiceCredit,
    VoiceRef,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::audio::{Pcm, WavFormat};
use crate::cache::{BlobStore, DiskCache};
use crate::error::{CoreError, Result};
use crate::plugin::{ChunkRequest, SpokenTurn, TtsProvider, synthesize_checked};
use crate::stage::{RunReport, Stage, cached};

/// The cast's voices: clips resolved against the episode directory, and the
/// hash of each clip's bytes, which is what a chunk's cache key holds.
#[derive(Debug, Clone)]
pub struct Voices {
    refs: BTreeMap<SpeakerId, VoiceRef>,
    keys: BTreeMap<SpeakerId, VoiceKey>,
    credits: Vec<VoiceCredit>,
}

/// What of a voice can change the audio: the clip's bytes and what is said
/// in it. Not the path, so moving a clip invalidates nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VoiceKey {
    pub clip: ContentHash,
    pub transcript: String,
}

impl Voices {
    /// Reads every cast member's reference clip. A missing clip is a
    /// [`CoreError::Config`] naming the speaker and the path, raised before
    /// any model has run.
    pub fn resolve(cast: &[CastMember], base_dir: &Path) -> Result<Self> {
        let mut voices = Self {
            refs: BTreeMap::new(),
            keys: BTreeMap::new(),
            credits: Vec::new(),
        };
        for member in cast {
            let voice = &member.voice;
            let path = base_dir.join(voice.reference());
            let bytes = fs::read(&path).map_err(|err| CoreError::Config {
                message: format!(
                    "cannot read the voice clip of speaker {:?} at {}: {err}",
                    member.id.0,
                    path.display()
                ),
            })?;
            let resolved = VoiceRef::new(&path, voice.transcript(), voice.licence())
                .expect("checked when the episode was parsed");
            voices.keys.insert(
                member.id.clone(),
                VoiceKey {
                    clip: ContentHash::of_parts(&[&bytes]),
                    transcript: voice.transcript().to_owned(),
                },
            );
            voices.refs.insert(member.id.clone(), resolved);
            voices.credits.push(VoiceCredit {
                speaker: member.id.clone(),
                reference: voice.reference().to_owned(),
                licence: voice.licence().to_owned(),
            });
        }
        Ok(voices)
    }

    /// Who the voices came from, for the audio manifest.
    pub fn credits(&self) -> &[VoiceCredit] {
        &self.credits
    }
}

/// What a chunk's audio depends on, apart from the take: the chunk id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChunkSpec {
    pub turns: Vec<SpokenTurn>,
    /// Only the voices of the speakers in `turns`, so changing someone
    /// else's voice leaves this chunk alone.
    pub voices: BTreeMap<SpeakerId, VoiceKey>,
}

impl ChunkSpec {
    pub fn id(&self) -> Result<ContentHash> {
        Ok(ContentHash::of_parts(&[&serde_json::to_vec(self)?]))
    }
}

/// The input of one [`SynthesizeChunk`] run, and so its cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChunkInput {
    pub chunk: ChunkSpec,
    /// Which attempt, from 0. Each take gets its own seed.
    pub take: u8,
}

impl ChunkInput {
    /// `seed = first 8 bytes of BLAKE3(chunk id ‖ take)`: derived, never
    /// random, so a rerun reproduces every take and finds it cached.
    pub fn seed(&self) -> Result<u64> {
        let id = self.chunk.id()?;
        let hash = ContentHash::of_parts(&[id.as_str().as_bytes(), &[self.take]]);
        let hex = &hash.as_str()[..16];
        Ok(u64::from_str_radix(hex, 16).expect("a content hash is hex"))
    }
}

/// What a chunk run leaves in the cache: a reference to its audio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkResult {
    /// The chunk's WAV (32-bit float, the model's native rate).
    pub blob: ContentHash,
    pub seed: u64,
    /// Where each turn is in the audio, in samples, when the backend said.
    pub turn_spans: Option<Vec<Range<usize>>>,
}

/// Synthesises one chunk with the TTS provider and stores its audio.
///
/// [`Stage::run`] takes `&self`, but [`TtsProvider::synthesize`] needs
/// `&mut self`. A `RefCell` bridges the two: it moves the "only one user at
/// a time" check from compile time to run time (`borrow_mut` panics if the
/// provider is already borrowed, which a single-threaded loop never does).
/// This is *interior mutability*.
pub struct SynthesizeChunk<'a> {
    tts: RefCell<&'a mut dyn TtsProvider>,
    fingerprint: Value,
    voices: &'a BTreeMap<SpeakerId, VoiceRef>,
    blobs: &'a BlobStore,
}

impl<'a> SynthesizeChunk<'a> {
    pub fn new(tts: &'a mut dyn TtsProvider, voices: &'a Voices, blobs: &'a BlobStore) -> Self {
        Self {
            fingerprint: tts.fingerprint(),
            tts: RefCell::new(tts),
            voices: &voices.refs,
            blobs,
        }
    }
}

impl Stage for SynthesizeChunk<'_> {
    const ID: &'static str = "synthesize_chunk";
    const VERSION: u32 = 1;
    type Input = ChunkInput;
    type Output = ChunkResult;

    fn config_fingerprint(&self) -> Value {
        json!({ "tts": self.fingerprint })
    }

    fn run(&self, input: &ChunkInput) -> Result<ChunkResult> {
        let seed = input.seed()?;
        let request = ChunkRequest {
            turns: &input.chunk.turns,
            voices: self.voices,
            context: None,
            seed,
        };
        let started = Instant::now();
        let mut tts = self.tts.borrow_mut();
        let audio = synthesize_checked(&mut **tts, Self::ID, &request)?;
        let elapsed = started.elapsed().as_secs_f64();
        let seconds = audio.pcm.seconds();
        tracing::info!(
            provider = tts.id(),
            turns = input.chunk.turns.len(),
            take = input.take,
            seconds,
            elapsed_ms = (elapsed * 1000.0) as u64,
            rtf = elapsed / seconds,
            "chunk synthesised"
        );
        let blob = self.blobs.put(&audio.pcm.to_wav(WavFormat::Float32)?)?;
        Ok(ChunkResult {
            blob,
            seed,
            turn_spans: audio.turn_spans,
        })
    }

    fn is_reusable(&self, output: &ChunkResult) -> bool {
        matches!(self.blobs.get(&output.blob), Ok(Some(_)))
    }
}

/// One chunk of the episode: its manifest entry and its audio.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthesizedChunk {
    pub record: ChunkRecord,
    pub pcm: Pcm,
}

/// Synthesises `script`, one chunk per turn, in order.
pub fn synthesize_script(
    script: &Script,
    voices: &Voices,
    tts: &mut dyn TtsProvider,
    blobs: &BlobStore,
    cache: Option<&DiskCache>,
    report: &mut RunReport,
) -> Result<Vec<SynthesizedChunk>> {
    let stage = SynthesizeChunk::new(tts, voices, blobs);
    let mut chunks = Vec::with_capacity(script.turns().len());
    for (i, turn) in script.turns().iter().enumerate() {
        let spoken = SpokenTurn {
            speaker: turn.speaker.clone(),
            text: turn.text.clone(),
            emotion: turn.emotion,
        };
        let key = voices
            .keys
            .get(&turn.speaker)
            .ok_or_else(|| CoreError::Config {
                message: format!(
                    "speaker {:?} has no voice: add it to [[cast]] with a reference clip",
                    turn.speaker.0
                ),
            })?;
        let input = ChunkInput {
            chunk: ChunkSpec {
                turns: vec![spoken],
                voices: BTreeMap::from([(turn.speaker.clone(), key.clone())]),
            },
            take: 0,
        };
        let result = cached(&stage, &input, cache, report)?;
        let bytes = blobs
            .get(&result.blob)?
            .ok_or_else(|| CoreError::InvalidProviderOutput {
                stage: SynthesizeChunk::ID,
                message: format!("audio blob {} vanished during the run", result.blob),
            })?;
        chunks.push(SynthesizedChunk {
            record: ChunkRecord {
                id: input.chunk.id()?,
                turns: TurnRange::new(i, i + 1).expect("i < i + 1"),
                blob: result.blob,
                seed: result.seed,
                take: input.take,
                // Not checked yet: speech-recognition verification fills
                // these in. Until then a chunk counts as unverified.
                wer_pm: PerMille::new(1000).expect("1000 is within range"),
                quote_misses: Vec::new(),
                verified: false,
            },
            pcm: Pcm::from_wav(&bytes)?,
        });
    }
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use podling_types::{Emotion, Pace, Speaker, Turn};

    use super::*;
    use crate::plugin::FakeTts;

    fn cast_member(dir: &Path, id: &str, clip: &[u8]) -> CastMember {
        fs::write(dir.join(format!("{id}.wav")), clip).unwrap();
        CastMember {
            id: SpeakerId(id.into()),
            name: id.into(),
            role: "host".into(),
            voice: VoiceRef::new(format!("{id}.wav"), "Hello there.", "CC0-1.0").unwrap(),
        }
    }

    fn script(texts: &[(&str, &str)]) -> Script {
        let mut ids: Vec<&str> = texts.iter().map(|(s, _)| *s).collect();
        ids.sort_unstable();
        ids.dedup();
        let cast = ids
            .iter()
            .map(|id| Speaker {
                id: SpeakerId((*id).into()),
                name: (*id).into(),
                role: "host".into(),
            })
            .collect();
        let turns = texts
            .iter()
            .map(|(speaker, text)| Turn {
                speaker: SpeakerId((*speaker).into()),
                text: (*text).into(),
                emotion: Emotion::Neutral,
                citations: vec![],
                quotes: vec![],
                pace: Pace::Normal,
                nonverbal: vec![],
                callback_to: None,
            })
            .collect();
        Script::new(cast, turns).unwrap()
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        voices: Voices,
        cache: DiskCache,
        blobs: BlobStore,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let cast = [
            cast_member(dir.path(), "ada", b"clip a"),
            cast_member(dir.path(), "ben", b"clip b"),
        ];
        let voices = Voices::resolve(&cast, dir.path()).unwrap();
        let cache = DiskCache::new(dir.path().join("cache"));
        let blobs = cache.blobs();
        Fixture {
            _dir: dir,
            voices,
            cache,
            blobs,
        }
    }

    fn misses(report: &RunReport) -> usize {
        report.stages.iter().filter(|s| !s.cache_hit).count()
    }

    #[test]
    fn a_warm_cache_makes_no_tts_calls_and_a_lost_blob_redoes_one_chunk() {
        let f = fixture();
        let script = script(&[
            ("ada", "One two three."),
            ("ben", "Four five."),
            ("ada", "Six."),
        ]);
        let run = |report: &mut RunReport| {
            synthesize_script(
                &script,
                &f.voices,
                &mut FakeTts::default(),
                &f.blobs,
                Some(&f.cache),
                report,
            )
            .unwrap()
        };

        let mut cold = RunReport::default();
        let first = run(&mut cold);
        assert_eq!((first.len(), misses(&cold)), (3, 3));

        let mut warm = RunReport::default();
        let second = run(&mut warm);
        assert_eq!(misses(&warm), 0, "a warm cache makes no TTS calls");
        assert_eq!(first, second);

        fs::remove_file(f.blobs.path_for(&first[1].record.blob)).unwrap();
        let mut lost = RunReport::default();
        let third = run(&mut lost);
        let hits: Vec<bool> = lost.stages.iter().map(|s| s.cache_hit).collect();
        assert_eq!(hits, [true, false, true], "only the lost chunk runs again");
        assert_eq!(third, first, "and comes back the same: the seed is derived");
    }

    #[test]
    fn records_point_at_their_turn_and_audio() {
        let f = fixture();
        let script = script(&[("ada", "One two three four."), ("ben", "Five.")]);
        let mut report = RunReport::default();
        let chunks = synthesize_script(
            &script,
            &f.voices,
            &mut FakeTts::default(),
            &f.blobs,
            None,
            &mut report,
        )
        .unwrap();
        assert_eq!(chunks[0].record.turns, TurnRange::new(0, 1).unwrap());
        assert_eq!(chunks[1].record.turns, TurnRange::new(1, 2).unwrap());
        // FakeTts speaks a quarter-second per word at 24 kHz.
        assert_eq!(chunks[0].pcm.len(), 4 * 6_000);
        assert!(!chunks[0].record.verified);
        assert!(f.blobs.get(&chunks[1].record.blob).unwrap().is_some());
    }

    #[test]
    fn the_key_holds_the_clip_bytes_not_the_path() {
        let a = fixture();
        let b = fixture();
        assert_eq!(
            a.voices.keys, b.voices.keys,
            "same bytes in another directory"
        );

        let dir = tempfile::tempdir().unwrap();
        let other = Voices::resolve(
            &[cast_member(dir.path(), "ada", b"another clip")],
            dir.path(),
        )
        .unwrap();
        let ada = SpeakerId("ada".into());
        assert_ne!(a.voices.keys[&ada], other.keys[&ada]);
    }

    #[test]
    fn seeds_differ_per_take_and_per_chunk() {
        let f = fixture();
        let spec = |text: &str| ChunkSpec {
            turns: vec![SpokenTurn {
                speaker: SpeakerId("ada".into()),
                text: text.into(),
                emotion: Emotion::Neutral,
            }],
            voices: f.voices.keys.clone(),
        };
        let seed = |text, take| {
            ChunkInput {
                chunk: spec(text),
                take,
            }
            .seed()
            .unwrap()
        };
        assert_eq!(seed("Hi.", 0), seed("Hi.", 0));
        assert_ne!(seed("Hi.", 0), seed("Hi.", 1));
        assert_ne!(seed("Hi.", 0), seed("Bye.", 0));
    }

    #[test]
    fn a_missing_voice_clip_is_a_config_error_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let member = CastMember {
            id: SpeakerId("ada".into()),
            name: "Ada".into(),
            role: "host".into(),
            voice: VoiceRef::new("voices/nowhere.wav", "Hi.", "CC0-1.0").unwrap(),
        };
        let err = Voices::resolve(&[member], dir.path()).unwrap_err();
        let CoreError::Config { message } = err else {
            panic!("expected a Config error, got {err:?}");
        };
        assert!(
            message.contains("\"ada\"") && message.contains("nowhere.wav"),
            "{message}"
        );
    }
}
