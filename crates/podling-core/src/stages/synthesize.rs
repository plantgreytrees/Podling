//! Speaks the script, one chunk at a time, each chunk cached on its own.
//!
//! [`plan_chunks`] decides the chunks: whole beats for a dialogue model, one
//! turn for a per-turn model. Each chunk runs through [`cached`] as a
//! [`SynthesizeChunk`] stage, so a 60-minute episode is many small cache
//! entries: editing one turn re-synthesises its chunk (and, for a model that
//! listens to context, the chunk after it), and a crash loses only the chunk
//! in flight. The audio itself goes in the [`BlobStore`]; the cache entry
//! names it by hash, and an entry whose blob has gone is a miss.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Instant;

use podling_types::{
    CastMember, ChunkRecord, ContentHash, NonverbalAt, PerMille, Script, SpeakerId, VoiceCredit,
    VoiceRef,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::audio::{Pcm, WavFormat};
use crate::cache::{BlobStore, DiskCache};
use crate::error::{CoreError, Result};
use crate::plugin::{ChunkContext, ChunkRequest, SpokenTurn, TtsProvider, synthesize_checked};
use crate::stage::{RunReport, Stage, cached};
use crate::stages::plan_chunks::{Piece, PlannedChunk, plan_chunks};

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

    /// The keys of everyone who speaks or makes a sound in `turns`.
    fn keys_for<'t>(
        &self,
        turns: impl IntoIterator<Item = &'t SpokenTurn>,
    ) -> Result<BTreeMap<SpeakerId, VoiceKey>> {
        let mut keys = BTreeMap::new();
        for turn in turns {
            let sounds = turn.nonverbal.iter().map(|n| &n.by);
            for speaker in std::iter::once(&turn.speaker).chain(sounds) {
                let key = self.keys.get(speaker).ok_or_else(|| CoreError::Config {
                    message: format!(
                        "speaker {:?} has no voice: add it to [[cast]] with a reference clip",
                        speaker.0
                    ),
                })?;
                keys.insert(speaker.clone(), key.clone());
            }
        }
        Ok(keys)
    }
}

/// What a chunk's audio depends on, apart from the take: the chunk id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChunkSpec {
    pub turns: Vec<SpokenTurn>,
    /// Only the voices of the speakers in `turns` and `context`, so changing
    /// someone else's voice leaves this chunk alone.
    pub voices: BTreeMap<SpeakerId, VoiceKey>,
    /// What the model hears first; `None` for a model that doesn't listen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextSpec>,
}

/// A chunk's context, keyed by what was *said*, not by its audio.
///
/// The context's audio is the output of earlier chunks. Keying on it would
/// chain every chunk to the one before: one edit would change a chunk's
/// audio, so the next chunk's key, so its audio, and so on to the end of the
/// episode. Keyed on the words, an edit re-synthesises at most the edited
/// chunk and the one after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextSpec {
    /// The end of the previous chunk: its last beat, or the part of that
    /// beat in it.
    pub turns: Vec<SpokenTurn>,
    /// Earlier turns that a turn in this chunk calls back to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callbacks: Vec<SpokenTurn>,
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
    /// Where the context's audio is. `#[serde(skip)]` leaves it out of the
    /// cache key: [`ContextSpec`] says why, and paths never belong in one.
    #[serde(skip)]
    pub context_audio: ContextAudio,
}

/// Where the audio of a chunk's [`ContextSpec`] is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextAudio {
    pub previous: Option<Clip>,
    pub callbacks: Vec<Clip>,
}

/// Some or all of an earlier chunk's audio. Only cut out and written to a
/// file when the chunk is actually synthesised, so a cache hit costs nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    /// The earlier chunk's audio in the blob store.
    pub blob: ContentHash,
    /// Which of its samples; `None` for all of them.
    pub samples: Option<Range<usize>>,
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

    /// A WAV file holding `clip`: the earlier chunk's own blob when the clip
    /// is all of it, otherwise the cut-out samples, stored as a blob too.
    fn file_of(&self, clip: &Clip) -> Result<PathBuf> {
        let Some(samples) = &clip.samples else {
            return Ok(self.blobs.path_for(&clip.blob));
        };
        let gone = || CoreError::InvalidProviderOutput {
            stage: Self::ID,
            message: format!("context audio {} vanished during the run", clip.blob),
        };
        let pcm = Pcm::from_wav(&self.blobs.get(&clip.blob)?.ok_or_else(gone)?)?;
        let cut = pcm.samples.get(samples.clone()).ok_or_else(gone)?;
        let wav = Pcm::new(pcm.rate, cut.to_vec()).to_wav(WavFormat::Float32)?;
        Ok(self.blobs.path_for(&self.blobs.put(&wav)?))
    }
}

impl Stage for SynthesizeChunk<'_> {
    const ID: &'static str = "synthesize_chunk";
    /// 2: a chunk is a planned run of pieces with context, not one turn.
    const VERSION: u32 = 2;
    type Input = ChunkInput;
    type Output = ChunkResult;

    fn config_fingerprint(&self) -> Value {
        json!({ "tts": self.fingerprint })
    }

    fn run(&self, input: &ChunkInput) -> Result<ChunkResult> {
        let seed = input.seed()?;
        let audio = &input.context_audio;
        let previous = audio
            .previous
            .as_ref()
            .map(|c| self.file_of(c))
            .transpose()?;
        let callbacks = audio
            .callbacks
            .iter()
            .map(|c| self.file_of(c))
            .collect::<Result<Vec<_>>>()?;
        let callbacks: Vec<&Path> = callbacks.iter().map(PathBuf::as_path).collect();
        let request = ChunkRequest {
            turns: &input.chunk.turns,
            voices: self.voices,
            context: input.chunk.context.as_ref().map(|context| ChunkContext {
                turns: &context.turns,
                audio: previous.as_deref(),
                callbacks: &callbacks,
            }),
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
    /// Where each of the chunk's pieces is in `pcm`, when the backend said.
    pub turn_spans: Option<Vec<Range<usize>>>,
}

impl SynthesizedChunk {
    /// The audio of pieces `pieces` (indices into the chunk's pieces); all
    /// of the chunk when that is what they are, or when the backend gave no
    /// spans to cut by.
    fn clip(&self, pieces: Range<usize>) -> Clip {
        let samples = self.turn_spans.as_ref().and_then(|spans| {
            let all = pieces == (0..spans.len());
            (!all).then(|| spans[pieces.start].start..spans[pieces.end - 1].end)
        });
        Clip {
            blob: self.record.blob.clone(),
            samples,
        }
    }
}

/// Synthesises `script` chunk by chunk, as [`plan_chunks`] cuts it for `tts`.
pub fn synthesize_script(
    script: &Script,
    voices: &Voices,
    tts: &mut dyn TtsProvider,
    blobs: &BlobStore,
    cache: Option<&DiskCache>,
    report: &mut RunReport,
) -> Result<Vec<SynthesizedChunk>> {
    let capabilities = tts.capabilities().clone();
    let plan = plan_chunks(script, &capabilities);
    tracing::info!(
        chunks = plan.len(),
        multi_speaker = capabilities.multi_speaker,
        "chunks planned"
    );
    let stage = SynthesizeChunk::new(tts, voices, blobs);
    let mut chunks: Vec<SynthesizedChunk> = Vec::with_capacity(plan.len());
    for planned in &plan {
        let turns = spoken(script, &planned.pieces);
        let (context, context_audio) = if capabilities.context {
            context_for(script, planned, &plan, &chunks)
        } else {
            (None, ContextAudio::default())
        };
        let context_turns = context.iter().flat_map(|c| &c.turns);
        let input = ChunkInput {
            chunk: ChunkSpec {
                voices: voices.keys_for(turns.iter().chain(context_turns))?,
                turns,
                context,
            },
            take: 0,
            context_audio,
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
                turns: planned.turns(),
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
            turn_spans: result.turn_spans,
        });
    }
    Ok(chunks)
}

/// `pieces` as the TTS will speak them. A sound the speaker makes before
/// their words goes with the piece that starts the turn, one after with the
/// piece that ends it; sounds over the turn, or by someone else, are left to
/// the assembler.
fn spoken(script: &Script, pieces: &[Piece]) -> Vec<SpokenTurn> {
    pieces
        .iter()
        .map(|piece| {
            let turn = &script.turns()[piece.turn];
            let in_line = |at: NonverbalAt| match at {
                NonverbalAt::Before => piece.starts_turn(),
                NonverbalAt::After => piece.ends_turn(script),
                NonverbalAt::Over => false,
            };
            SpokenTurn {
                speaker: turn.speaker.clone(),
                text: piece.text(script).to_owned(),
                emotion: turn.emotion,
                nonverbal: turn
                    .nonverbal
                    .iter()
                    .filter(|n| n.by == turn.speaker && in_line(n.at))
                    .cloned()
                    .collect(),
            }
        })
        .collect()
}

/// The context of `planned`: the last beat of the chunk before it, and the
/// earlier turns its turns call back to, with where their audio is in the
/// chunks already made.
fn context_for(
    script: &Script,
    planned: &PlannedChunk,
    plan: &[PlannedChunk],
    done: &[SynthesizedChunk],
) -> (Option<ContextSpec>, ContextAudio) {
    let previous = done.len().checked_sub(1).and_then(|i| plan.get(i));
    let (Some(previous), Some(made)) = (previous, done.last()) else {
        return (None, ContextAudio::default());
    };
    let last_beat = previous.pieces.last().expect("a chunk is never empty").beat;
    let from = previous
        .pieces
        .iter()
        .rposition(|p| p.beat != last_beat)
        .map_or(0, |i| i + 1);
    let mut spec = ContextSpec {
        turns: spoken(script, &previous.pieces[from..]),
        callbacks: Vec::new(),
    };
    let mut audio = ContextAudio {
        previous: Some(made.clip(from..previous.pieces.len())),
        callbacks: Vec::new(),
    };

    // A turn in this chunk, or already in the context, needs no clip.
    let heard = |t: usize| {
        planned.turns().contains(t) || previous.pieces[from..].iter().any(|p| p.turn == t)
    };
    let mut targets: Vec<usize> = planned
        .pieces
        .iter()
        .filter_map(|p| script.turns()[p.turn].callback_to)
        .filter(|&t| !heard(t))
        .collect();
    targets.sort_unstable();
    targets.dedup();
    for target in targets {
        // The first chunk with any of the turn: its pieces there.
        let Some(c) = plan[..done.len()]
            .iter()
            .position(|chunk| chunk.pieces.iter().any(|p| p.turn == target))
        else {
            continue;
        };
        let pieces = &plan[c].pieces;
        let first = pieces.iter().position(|p| p.turn == target).expect("found");
        let last = pieces
            .iter()
            .rposition(|p| p.turn == target)
            .expect("found");
        spec.callbacks.extend(spoken(script, &pieces[first..=last]));
        audio.callbacks.push(done[c].clip(first..last + 1));
    }
    (Some(spec), audio)
}
#[cfg(test)]
mod tests {
    use podling_types::{Emotion, Pace, Speaker, Turn, TurnRange};

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
            turns: vec![SpokenTurn::plain(
                SpeakerId("ada".into()),
                text,
                Emotion::Neutral,
            )],
            voices: f.voices.keys.clone(),
            context: None,
        };
        let seed = |text, take| {
            ChunkInput {
                chunk: spec(text),
                take,
                context_audio: ContextAudio::default(),
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

    /// `n` distinct words, `w0 w1 …`, ten to a sentence.
    fn words(prefix: &str, n: usize) -> String {
        let words: Vec<String> = (0..n)
            .map(|i| {
                let end = if i % 10 == 9 || i + 1 == n { "." } else { "" };
                format!("{prefix}{i}{end}")
            })
            .collect();
        words.join(" ")
    }

    /// What a provider was asked, per call.
    #[derive(Debug, Default)]
    struct Asked {
        context_turns: Vec<String>,
        callbacks: usize,
        heard_audio: bool,
    }

    /// FakeTts that notes each request's context, and checks the context
    /// files exist when it is called.
    struct Recording {
        inner: FakeTts,
        asked: Vec<Asked>,
    }

    impl TtsProvider for Recording {
        fn id(&self) -> &str {
            "recording"
        }
        fn fingerprint(&self) -> Value {
            self.inner.fingerprint()
        }
        fn capabilities(&self) -> &crate::plugin::TtsCapabilities {
            self.inner.capabilities()
        }
        fn synthesize(&mut self, request: &ChunkRequest<'_>) -> Result<crate::plugin::ChunkAudio> {
            let mut asked = Asked::default();
            if let Some(context) = request.context {
                asked.context_turns = context.turns.iter().map(|t| t.text.clone()).collect();
                asked.callbacks = context.callbacks.len();
                asked.heard_audio = context.audio.is_some_and(Path::is_file);
                assert!(context.callbacks.iter().all(|p| p.is_file()));
            }
            self.asked.push(asked);
            self.inner.synthesize(request)
        }
    }

    fn run(
        f: &Fixture,
        script: &Script,
        tts: &mut dyn TtsProvider,
    ) -> (Vec<SynthesizedChunk>, Vec<bool>) {
        let mut report = RunReport::default();
        let chunks = synthesize_script(
            script,
            &f.voices,
            tts,
            &f.blobs,
            Some(&f.cache),
            &mut report,
        )
        .unwrap();
        (chunks, report.stages.iter().map(|s| s.cache_hit).collect())
    }

    /// Four turns of a minute each: a dialogue model gets a chunk per turn.
    fn four_minutes(edit: Option<usize>) -> Script {
        let texts: Vec<(String, String)> = (0..4)
            .map(|t| {
                let edited = if edit == Some(t) { "x" } else { "" };
                let who = if t % 2 == 0 { "ada" } else { "ben" };
                (who.to_owned(), words(&format!("t{t}{edited}w"), 150))
            })
            .collect();
        let borrowed: Vec<(&str, &str)> = texts
            .iter()
            .map(|(s, t)| (s.as_str(), t.as_str()))
            .collect();
        script(&borrowed)
    }

    #[test]
    fn context_changes_the_key_but_is_never_in_the_audio() {
        let f = fixture();
        let script = four_minutes(None);
        let mut recording = Recording {
            inner: FakeTts::dialogue(),
            asked: Vec::new(),
        };
        let (chunks, _) = run(&f, &script, &mut recording);
        assert_eq!(chunks.len(), 4);

        // The first chunk has nothing before it; each later one hears the
        // chunk before, text and audio.
        assert!(recording.asked[0].context_turns.is_empty());
        assert!(!recording.asked[0].heard_audio);
        for (i, asked) in recording.asked.iter().enumerate().skip(1) {
            assert_eq!(asked.context_turns, [script.turns()[i - 1].text.clone()]);
            assert!(asked.heard_audio, "chunk {i}");
        }
        // 150 words at a quarter second each: the context adds no samples.
        for chunk in &chunks {
            assert_eq!(chunk.pcm.len(), 150 * 6_000);
        }

        // The same turn with and without context is a different chunk.
        let (per_turn, _) = run(&f, &script, &mut FakeTts::default());
        assert_eq!(
            per_turn[0].record.id, chunks[0].record.id,
            "no context either way"
        );
        assert_ne!(per_turn[1].record.id, chunks[1].record.id);
        assert_eq!(
            per_turn[1].pcm.len(),
            chunks[1].pcm.len(),
            "same words, same length"
        );
    }

    #[test]
    fn editing_one_turn_redoes_its_chunk_and_the_next_only_if_it_listens() {
        let f = fixture();
        let (_, cold) = run(&f, &four_minutes(None), &mut FakeTts::dialogue());
        assert_eq!(cold, [false; 4]);
        let (_, edited) = run(&f, &four_minutes(Some(1)), &mut FakeTts::dialogue());
        assert_eq!(edited, [true, false, false, true], "chunk 2 heard turn 1");

        let f = fixture();
        run(&f, &four_minutes(None), &mut FakeTts::default());
        let (_, edited) = run(&f, &four_minutes(Some(1)), &mut FakeTts::default());
        assert_eq!(
            edited,
            [true, false, true, true],
            "a per-turn model hears nothing"
        );
    }

    #[test]
    fn a_callback_brings_the_earlier_turns_audio() {
        let f = fixture();
        let mut script = four_minutes(None);
        let mut turns = script.turns().to_vec();
        turns[3].callback_to = Some(0);
        script = Script::new(script.cast().to_vec(), turns).unwrap();
        let mut recording = Recording {
            inner: FakeTts::dialogue(),
            asked: Vec::new(),
        };
        run(&f, &script, &mut recording);
        let callbacks: Vec<usize> = recording.asked.iter().map(|a| a.callbacks).collect();
        assert_eq!(callbacks, [0, 0, 0, 1]);

        // Calling back to the turn just before is already in the context;
        // only a turn in an earlier chunk adds a clip.
        let mut turns = script.turns().to_vec();
        turns[3].callback_to = Some(2);
        let near = Script::new(script.cast().to_vec(), turns).unwrap();
        let mut recording = Recording {
            inner: FakeTts::dialogue(),
            asked: Vec::new(),
        };
        run(&f, &near, &mut recording);
        let callbacks: Vec<usize> = recording.asked.iter().map(|a| a.callbacks).collect();
        assert_eq!(callbacks, [0], "only chunk 3 changed, and it added no clip");
    }

    #[test]
    fn only_the_speakers_own_sounds_before_or_after_go_to_the_tts() {
        let ada = SpeakerId("ada".into());
        let ben = SpeakerId("ben".into());
        let sound = |kind, by: &SpeakerId, at| podling_types::Nonverbal {
            kind,
            by: by.clone(),
            at,
        };
        let mut turn = script(&[("ada", "Hi."), ("ben", "Hey.")]).turns()[0].clone();
        turn.text = words("w", 400);
        turn.nonverbal = vec![
            sound(
                podling_types::NonverbalKind::Laugh {},
                &ada,
                NonverbalAt::Before,
            ),
            sound(
                podling_types::NonverbalKind::Sigh {},
                &ada,
                NonverbalAt::After,
            ),
            sound(
                podling_types::NonverbalKind::Chuckle {},
                &ben,
                NonverbalAt::Before,
            ),
            sound(
                podling_types::NonverbalKind::Backchannel {
                    text: "Mm-hm.".into(),
                },
                &ben,
                NonverbalAt::Over,
            ),
        ];
        let cast = script(&[("ada", "Hi."), ("ben", "Hey.")]).cast().to_vec();
        let script = Script::new(cast, vec![turn]).unwrap();
        let plan = plan_chunks(&script, FakeTts::default().capabilities());
        assert_eq!(plan.len(), 2, "400 words is cut in two");
        let first = spoken(&script, &plan[0].pieces);
        let last = spoken(&script, &plan[1].pieces);
        let kinds =
            |t: &SpokenTurn| -> Vec<NonverbalAt> { t.nonverbal.iter().map(|n| n.at).collect() };
        assert_eq!(kinds(&first[0]), [NonverbalAt::Before]);
        assert_eq!(kinds(&last[0]), [NonverbalAt::After]);
    }
}
