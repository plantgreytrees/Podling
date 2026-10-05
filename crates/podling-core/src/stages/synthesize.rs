//! Speaks the script, one chunk at a time, each chunk cached on its own.
//!
//! [`plan_chunks`] decides the chunks: whole beats for a dialogue model, one
//! turn for a per-turn model. Each chunk runs through [`cached`] as a
//! [`SynthesizeChunk`] stage, so a 60-minute episode is many small cache
//! entries: editing one turn re-synthesises its chunk (and, for a model that
//! listens to context, the chunk after it), and a crash loses only the chunk
//! in flight. The audio itself goes in the [`BlobStore`]; the cache entry
//! names it by hash, and an entry whose blob has gone is a miss.
//!
//! Each take is then transcribed ([`TranscribeChunk`], cached the same way)
//! and checked against the script; a take that fails is made again.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Instant;

use podling_types::{
    BeatKind, CastMember, ChunkRecord, ContentHash, Emotion, NonverbalAt, NonverbalKind, PerMille,
    Script, SpeakerId, VoiceCredit, VoiceRef,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::audio::{Pcm, WavFormat};
use crate::cache::{BlobStore, DiskCache};
use crate::error::{CoreError, Result};
use crate::plugin::{
    AsrProvider, ChunkContext, ChunkRequest, SpokenTurn, Transcript, TtsProvider,
    synthesize_checked,
};
use crate::stage::{RunReport, Stage, cached};
use crate::stages::plan_chunks::{Piece, PlannedChunk, plan_chunks};
use crate::stages::verify_audio::{Check, TranscribeChunk, TranscribeInput, spans_from};

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
    /// What the chunk speaks: which turn, or part of one, each piece is.
    pub pieces: Vec<Piece>,
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

/// How hard to try for a chunk that passes its checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Takes {
    /// Takes made of a chunk with banter in it; the best passing one wins.
    /// Other chunks get one.
    pub banter: u8,
    /// Extra takes, one at a time, for a chunk none of whose takes passed.
    pub max_retries: u8,
    /// A take passes at this word error rate or below (and with every quote
    /// heard).
    pub max_wer_pm: PerMille,
}

/// The recogniser that checks each take, and how many takes to try.
pub struct Verification<'a> {
    pub asr: &'a mut dyn AsrProvider,
    pub takes: Takes,
}

/// One take of a chunk, synthesised and checked.
struct Take {
    take: u8,
    result: ChunkResult,
    pcm: Pcm,
    transcript: Transcript,
    check: Check,
    passed: bool,
    /// Words a second.
    rate: f64,
}

impl Take {
    /// Passing beats failing, then a lower word error rate, then a speaking
    /// rate closer to `median` (the speaker's so far). On a full tie the
    /// earlier take stays.
    fn better_than(&self, other: &Take, median: Option<f64>) -> bool {
        if self.passed != other.passed {
            return self.passed;
        }
        let (mine, theirs) = (self.check.wer_pm.get(), other.check.wer_pm.get());
        if mine != theirs {
            return mine < theirs;
        }
        median.is_some_and(|m| (self.rate - m).abs() < (other.rate - m).abs())
    }
}

/// Synthesises `script` chunk by chunk, as [`plan_chunks`] cuts it for `tts`,
/// and checks each take with speech recognition.
///
/// A take that fails is made again with a new seed, up to
/// [`Takes::max_retries`] more times; a chunk with banter gets
/// [`Takes::banter`] takes from the start and keeps the best. A chunk that
/// never passes keeps its best take and is marked unverified. Both stages
/// are cached per take, so a rerun makes no TTS or ASR calls at all.
pub fn synthesize_script(
    script: &Script,
    voices: &Voices,
    tts: &mut dyn TtsProvider,
    verification: Verification<'_>,
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
    let Verification { asr, takes } = verification;
    let synthesize = SynthesizeChunk::new(tts, voices, blobs);
    let transcribe = TranscribeChunk::new(asr, blobs);
    let beats = script.beats();
    // Each speaker's speaking rate in the takes kept so far.
    let mut rates: BTreeMap<SpeakerId, Vec<f64>> = BTreeMap::new();
    let mut chunks: Vec<SynthesizedChunk> = Vec::with_capacity(plan.len());
    for planned in &plan {
        let turns = spoken(script, &planned.pieces);
        let (context, context_audio) = if capabilities.context {
            context_for(script, planned, &plan, &chunks)
        } else {
            (None, ContextAudio::default())
        };
        let context_turns = context.iter().flat_map(|c| &c.turns);
        let chunk = ChunkSpec {
            voices: voices.keys_for(turns.iter().chain(context_turns))?,
            turns,
            context,
        };
        // What is said, own backchannels included: that is what the ASR hears.
        let expected: Vec<String> = chunk.turns.iter().map(SpokenTurn::said).collect();
        let words: usize = expected.iter().map(|t| t.split_whitespace().count()).sum();
        let quotes = quotes_in(script, &planned.pieces);
        let banter = planned
            .pieces
            .iter()
            .any(|p| beats[p.beat].kind == BeatKind::Banter);
        let first = if banter { takes.banter.max(1) } else { 1 };
        let speaker = single_speaker(&chunk.turns);
        let median = speaker.and_then(|s| median(rates.get(s)?));

        let mut best: Option<Take> = None;
        for take in 0..first.saturating_add(takes.max_retries) {
            if take >= first && best.as_ref().is_some_and(|b| b.passed) {
                break;
            }
            let input = ChunkInput {
                chunk: chunk.clone(),
                take,
                context_audio: context_audio.clone(),
            };
            let result = cached(&synthesize, &input, cache, report)?;
            let bytes =
                blobs
                    .get(&result.blob)?
                    .ok_or_else(|| CoreError::InvalidProviderOutput {
                        stage: SynthesizeChunk::ID,
                        message: format!("audio blob {} vanished during the run", result.blob),
                    })?;
            let pcm = Pcm::from_wav(&bytes)?;
            let heard = TranscribeInput {
                blob: result.blob.clone(),
                expected: expected.clone(),
            };
            let transcript = cached(&transcribe, &heard, cache, report)?;
            let check = Check::new(&expected, &quotes, &transcript);
            let passed = check.passes(takes.max_wer_pm);
            tracing::info!(
                turns = ?planned.turns().indices(),
                take,
                wer_pm = check.wer_pm.get(),
                quote_misses = check.quote_misses.len(),
                passed,
                "chunk checked"
            );
            let candidate = Take {
                take,
                rate: words as f64 / pcm.seconds().max(f64::EPSILON),
                result,
                pcm,
                transcript,
                check,
                passed,
            };
            if best
                .as_ref()
                .is_none_or(|b| candidate.better_than(b, median))
            {
                best = Some(candidate);
            }
        }
        let best = best.expect("every chunk gets at least one take");
        if !best.passed {
            tracing::warn!(
                turns = ?planned.turns().indices(),
                takes = first.saturating_add(takes.max_retries),
                wer_pm = best.check.wer_pm.get(),
                quote_misses = ?best.check.quote_misses,
                "chunk failed speech recognition on every take; keeping its best"
            );
        }
        if let Some(speaker) = speaker {
            rates.entry(speaker.clone()).or_default().push(best.rate);
        }
        // A backend that gave no turn spans gets them from the transcript.
        let turn_spans = best
            .result
            .turn_spans
            .clone()
            .or_else(|| spans_from(&best.transcript, &expected, &best.pcm));
        chunks.push(SynthesizedChunk {
            record: ChunkRecord {
                id: chunk.id()?,
                turns: planned.turns(),
                blob: best.result.blob,
                seed: best.result.seed,
                take: best.take,
                wer_pm: best.check.wer_pm,
                quote_misses: best.check.quote_misses,
                verified: best.passed,
            },
            pcm: best.pcm,
            pieces: planned.pieces.clone(),
            turn_spans,
        });
    }
    Ok(chunks)
}

/// A sound made over, before or after a turn by someone other than the turn's
/// speaker, rendered on its own for the assembler's second track.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayClip {
    /// The turn it goes with.
    pub turn: usize,
    pub at: NonverbalAt,
    pub pcm: Pcm,
}

/// Renders the sounds [`spoken`] leaves out of the chunks: those over a turn,
/// and those made by someone other than the turn's speaker.
///
/// Each backchannel ("mm-hm") becomes a one-turn chunk of its own in that
/// speaker's voice, cached like any other take, so a rerun makes it no
/// more. It is not checked by speech recognition: a two-word response is
/// below what a recogniser hears reliably, and it carries no facts. A laugh,
/// chuckle or sigh has no words to send, and the protocol has no way to ask
/// for a sound alone, so it is left out and logged.
pub fn synthesize_overlays(
    script: &Script,
    voices: &Voices,
    tts: &mut dyn TtsProvider,
    blobs: &BlobStore,
    cache: Option<&DiskCache>,
    report: &mut RunReport,
) -> Result<Vec<OverlayClip>> {
    let synthesize = SynthesizeChunk::new(tts, voices, blobs);
    // The same response by the same speaker is made once per run.
    let mut made: BTreeMap<(SpeakerId, String), Pcm> = BTreeMap::new();
    let mut clips = Vec::new();
    for (index, turn) in script.turns().iter().enumerate() {
        let apart = turn
            .nonverbal
            .iter()
            .filter(|n| n.at == NonverbalAt::Over || n.by != turn.speaker);
        for sound in apart {
            let NonverbalKind::Backchannel { text } = &sound.kind else {
                tracing::warn!(
                    turn = index,
                    by = %sound.by.0,
                    kind = ?sound.kind,
                    at = ?sound.at,
                    "a sound with no words can only be made by its speaker, before or after \
                     their own turn; left out"
                );
                continue;
            };
            let key = (sound.by.clone(), text.clone());
            let pcm = match made.get(&key) {
                Some(pcm) => pcm.clone(),
                None => {
                    let spoken = SpokenTurn::plain(sound.by.clone(), text, Emotion::Neutral);
                    let input = ChunkInput {
                        chunk: ChunkSpec {
                            voices: voices.keys_for([&spoken])?,
                            turns: vec![spoken],
                            context: None,
                        },
                        take: 0,
                        context_audio: ContextAudio::default(),
                    };
                    let result = cached(&synthesize, &input, cache, report)?;
                    let bytes = blobs.get(&result.blob)?.ok_or_else(|| {
                        CoreError::InvalidProviderOutput {
                            stage: SynthesizeChunk::ID,
                            message: format!("audio blob {} vanished during the run", result.blob),
                        }
                    })?;
                    let pcm = Pcm::from_wav(&bytes)?;
                    made.insert(key, pcm.clone());
                    pcm
                }
            };
            clips.push(OverlayClip {
                turn: index,
                at: sound.at,
                pcm,
            });
        }
    }
    Ok(clips)
}

/// The speaker of every turn in `turns`, when there is only one.
fn single_speaker(turns: &[SpokenTurn]) -> Option<&SpeakerId> {
    let first = &turns.first()?.speaker;
    turns.iter().all(|t| &t.speaker == first).then_some(first)
}

fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len() / 2).copied()
}

/// The quotes spoken in `pieces`, verbatim, each once.
fn quotes_in(script: &Script, pieces: &[Piece]) -> Vec<String> {
    let mut quotes: Vec<String> = Vec::new();
    for piece in pieces {
        let text = piece.text(script);
        for quote in &script.turns()[piece.turn].quotes {
            if text.contains(quote.text()) && !quotes.iter().any(|q| q == quote.text()) {
                quotes.push(quote.text().to_owned());
            }
        }
    }
    quotes
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
    use podling_types::{Beat, Emotion, Nonverbal, Pace, Speaker, Turn, TurnRange};

    use super::*;
    use crate::plugin::{AsrRequest, FakeAsr, FakeTts, Segment};

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

    /// Whether each TTS call was a cache hit, in order.
    fn synth_hits(report: &RunReport) -> Vec<bool> {
        report
            .stages
            .iter()
            .filter(|s| s.id == SynthesizeChunk::ID)
            .map(|s| s.cache_hit)
            .collect()
    }

    fn misses(report: &RunReport) -> usize {
        report.stages.iter().filter(|s| !s.cache_hit).count()
    }

    fn takes() -> Takes {
        Takes {
            banter: 2,
            max_retries: 2,
            max_wer_pm: PerMille::new(80).unwrap(),
        }
    }

    /// Checked by `asr` with the default takes.
    fn checked(asr: &mut dyn AsrProvider) -> Verification<'_> {
        Verification {
            asr,
            takes: takes(),
        }
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
                checked(&mut FakeAsr::default()),
                &f.blobs,
                Some(&f.cache),
                report,
            )
            .unwrap()
        };

        let mut cold = RunReport::default();
        let first = run(&mut cold);
        // A TTS call and an ASR call per chunk.
        assert_eq!((first.len(), misses(&cold)), (3, 6));

        let mut warm = RunReport::default();
        let second = run(&mut warm);
        assert_eq!(misses(&warm), 0, "a warm cache makes no TTS or ASR calls");
        assert_eq!(first, second);

        fs::remove_file(f.blobs.path_for(&first[1].record.blob)).unwrap();
        let mut lost = RunReport::default();
        let third = run(&mut lost);
        let hits = synth_hits(&lost);
        assert_eq!(hits, [true, false, true], "only the lost chunk runs again");
        assert_eq!(
            misses(&lost),
            1,
            "and the same audio needs no new transcript"
        );
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
            checked(&mut FakeAsr::default()),
            &f.blobs,
            None,
            &mut report,
        )
        .unwrap();
        assert_eq!(chunks[0].record.turns, TurnRange::new(0, 1).unwrap());
        assert_eq!(chunks[1].record.turns, TurnRange::new(1, 2).unwrap());
        // FakeTts speaks a quarter-second per word at 24 kHz.
        assert_eq!(chunks[0].pcm.len(), 4 * 6_000);
        let record = &chunks[0].record;
        assert!(record.verified);
        assert_eq!((record.take, record.wer_pm.get()), (0, 0));
        assert!(f.blobs.get(&chunks[1].record.blob).unwrap().is_some());
    }

    /// Hears the same words whatever it is sent, as a real recogniser hears
    /// only the audio: never reads `expected`.
    struct Hears(&'static str);

    impl AsrProvider for Hears {
        fn id(&self) -> &str {
            "hears"
        }

        fn fingerprint(&self) -> Value {
            json!({ "hears": self.0 })
        }

        fn transcribe(&mut self, request: &AsrRequest<'_>) -> Result<Transcript> {
            Ok(Transcript {
                segments: vec![Segment {
                    text: self.0.into(),
                    start: 0.0,
                    end: request.pcm.seconds(),
                }],
            })
        }
    }

    #[test]
    fn a_turn_with_its_speakers_own_backchannel_verifies_first_time() {
        let f = fixture();
        let mut script = script(&[("ada", "One two three four five.")]);
        let mut turns = script.turns().to_vec();
        turns[0].nonverbal = vec![Nonverbal {
            kind: NonverbalKind::Backchannel {
                text: "Mm-hm.".into(),
            },
            by: SpeakerId("ada".into()),
            at: NonverbalAt::Before,
        }];
        script = Script::new(script.cast().to_vec(), turns).unwrap();

        // The TTS says the backchannel in line, so that is what is heard.
        let chunks = synthesize_script(
            &script,
            &f.voices,
            &mut FakeTts::default(),
            checked(&mut Hears("Mm-hm. One two three four five.")),
            &f.blobs,
            None,
            &mut RunReport::default(),
        )
        .unwrap();
        let record = &chunks[0].record;
        assert!(record.verified, "wer {} ‰", record.wer_pm.get());
        assert_eq!((record.take, record.wer_pm.get()), (0, 0));
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
            checked(&mut FakeAsr::default()),
            &f.blobs,
            Some(&f.cache),
            &mut report,
        )
        .unwrap();
        (chunks, synth_hits(&report))
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

    /// Runs `script` through `tts`, checked by `asr`; the chunks and the
    /// number of TTS calls.
    fn verified(
        f: &Fixture,
        script: &Script,
        tts: &mut dyn TtsProvider,
        asr: &mut dyn AsrProvider,
    ) -> (Vec<SynthesizedChunk>, usize) {
        let mut report = RunReport::default();
        let chunks = synthesize_script(
            script,
            &f.voices,
            tts,
            checked(asr),
            &f.blobs,
            None,
            &mut report,
        )
        .unwrap();
        (chunks, synth_hits(&report).len())
    }

    #[test]
    fn a_misheard_take_is_made_again_and_the_passing_one_kept() {
        let f = fixture();
        let script = script(&[("ada", "One two three."), ("ben", "Four five.")]);
        let (chunks, calls) = verified(
            &f,
            &script,
            &mut FakeTts::default(),
            &mut FakeAsr::mishearing_first(1),
        );
        assert_eq!(calls, 3, "one retry");
        let first = &chunks[0].record;
        assert_eq!(
            (first.take, first.verified, first.wer_pm.get()),
            (1, true, 0)
        );
        assert_ne!(first.seed, chunks[1].record.seed);
        assert_eq!(
            (chunks[1].record.take, chunks[1].record.verified),
            (0, true)
        );
    }

    #[test]
    fn a_chunk_that_never_passes_keeps_its_best_take_unverified() {
        let f = fixture();
        let script = script(&[("ada", "One two three.")]);
        let (chunks, calls) = verified(
            &f,
            &script,
            &mut FakeTts::default(),
            &mut FakeAsr::mishearing_first(u32::MAX),
        );
        assert_eq!(calls, 3, "the first take and two retries");
        let record = &chunks[0].record;
        assert!(!record.verified);
        assert_eq!(
            (record.take, record.wer_pm.get()),
            (0, 1000),
            "a tie keeps the first"
        );
        assert!(
            f.blobs.get(&record.blob).unwrap().is_some(),
            "its audio is kept"
        );
    }

    #[test]
    fn banter_gets_several_takes_even_when_the_first_passes() {
        let f = fixture();
        let plain = script(&[("ada", "Right."), ("ben", "Sure."), ("ada", "Then.")]);
        let beats = vec![
            Beat {
                kind: BeatKind::Banter,
                turns: TurnRange::new(0, 2).unwrap(),
            },
            Beat {
                kind: BeatKind::Narration,
                turns: TurnRange::new(2, 3).unwrap(),
            },
        ];
        let script =
            Script::with_beats(plain.cast().to_vec(), plain.turns().to_vec(), beats).unwrap();
        let (chunks, calls) = verified(
            &f,
            &script,
            &mut FakeTts::default(),
            &mut FakeAsr::default(),
        );
        assert_eq!(
            calls,
            2 + 2 + 1,
            "two takes per banter chunk, one otherwise"
        );
        assert!(chunks.iter().all(|c| c.record.verified));
    }

    fn take(passed: bool, wer: u16, rate: f64) -> Take {
        Take {
            take: 0,
            result: ChunkResult {
                blob: ContentHash::of_parts(&[b"t"]),
                seed: 0,
                turn_spans: None,
            },
            pcm: Pcm::new(24_000, vec![]),
            transcript: Transcript::default(),
            check: Check {
                wer_pm: PerMille::new(wer).unwrap(),
                quote_misses: vec![],
            },
            passed,
            rate,
        }
    }

    #[test]
    fn the_best_take_passes_then_is_heard_best_then_keeps_the_speakers_pace() {
        let median = Some(3.0);
        assert!(take(true, 60, 9.0).better_than(&take(false, 0, 3.0), median));
        assert!(take(true, 10, 9.0).better_than(&take(true, 20, 3.0), median));
        assert!(take(true, 10, 3.2).better_than(&take(true, 10, 2.5), median));
        assert!(!take(true, 10, 2.5).better_than(&take(true, 10, 3.2), median));
        assert!(
            !take(true, 10, 3.2).better_than(&take(true, 10, 2.5), None),
            "no median: a tie"
        );
    }

    /// A dialogue backend that says nothing about where each turn is.
    struct Spanless(FakeTts);

    impl TtsProvider for Spanless {
        fn id(&self) -> &str {
            "spanless"
        }
        fn fingerprint(&self) -> Value {
            self.0.fingerprint()
        }
        fn capabilities(&self) -> &crate::plugin::TtsCapabilities {
            self.0.capabilities()
        }
        fn synthesize(&mut self, request: &ChunkRequest<'_>) -> Result<crate::plugin::ChunkAudio> {
            let mut audio = self.0.synthesize(request)?;
            audio.turn_spans = None;
            Ok(audio)
        }
    }

    #[test]
    fn a_backend_without_spans_gets_them_from_the_transcript() {
        let f = fixture();
        let script = script(&[
            ("ada", "One two three."),
            ("ben", "Four five."),
            ("ada", "Six."),
        ]);
        let (with, _) = verified(
            &f,
            &script,
            &mut FakeTts::dialogue(),
            &mut FakeAsr::default(),
        );
        let (without, _) = verified(
            &f,
            &script,
            &mut Spanless(FakeTts::dialogue()),
            &mut FakeAsr::default(),
        );
        assert_eq!(without.len(), 1, "one dialogue chunk");
        let told = with[0].turn_spans.clone().unwrap();
        let heard = without[0]
            .turn_spans
            .clone()
            .expect("spans from the transcript");
        assert_eq!(heard.len(), told.len());
        for (heard, told) in heard.iter().zip(&told) {
            assert!(
                heard.start.abs_diff(told.start) <= 1,
                "{heard:?} vs {told:?}"
            );
            assert!(heard.end.abs_diff(told.end) <= 1, "{heard:?} vs {told:?}");
        }
    }
}
