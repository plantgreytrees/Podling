//! Whisper speech recognition, run natively on the CPU with candle.
//!
//! The model is a local Hugging Face snapshot, e.g.
//! `hf download openai/whisper-base.en --local-dir <dir>` (MIT). Only
//! `config.json`, `tokenizer.json`, `model.safetensors` and
//! `preprocessor_config.json` (which holds the mel filter bank) are read;
//! `pytorch_model.bin` is never touched, as unpickling it can run code.
//!
//! Long audio is decoded the way Whisper itself does it: one 30 s window at
//! a time, each picking up where the last complete segment ended, with no
//! earlier text fed back as a prompt. A window whose output is too
//! repetitive (a loop such as "The size of the graphs." five times) or too
//! unlikely is decoded again at a higher temperature. Without that guard
//! the verifier would fail good audio and regenerate it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use candle_core::{Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::audio::pcm_to_mel;
use candle_transformers::models::whisper::model::Whisper;
use candle_transformers::models::whisper::{self as consts, Config};
use serde::Deserialize;
use serde_json::{Value, json};
use tokenizers::Tokenizer;

use super::asr::{AsrProvider, AsrRequest, Segment, Transcript};
use crate::audio::Pcm;
use crate::error::{CoreError, ProviderFailure, Result};

const PLUGIN: &str = "whisper";

/// The files a model directory must contain.
pub const MODEL_FILES: [&str; 4] = [
    "config.json",
    "tokenizer.json",
    "model.safetensors",
    "preprocessor_config.json",
];

/// Temperatures tried in turn while a window's output looks wrong.
pub const TEMPERATURES: [f64; 6] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];
/// Above this, a window's tokens are too repetitive: zlib packs them this
/// many times smaller. Measured on the token ids, as transformers does
/// (Whisper's own 2.4 is for text).
pub const COMPRESSION_RATIO_THRESHOLD: f64 = 1.35;
/// Below this mean log-probability per token, a window's output is too
/// unlikely to trust.
pub const LOGPROB_THRESHOLD: f64 = -1.0;

/// Bump when decoding changes in a way the weights don't show.
const WHISPER_VERSION: u32 = 1;
/// Seconds per timestamp token.
const SECONDS_PER_STEP: f64 = 0.02;
/// Mel frames per timestamp step: frames are 10 ms apart.
const FRAMES_PER_STEP: usize = 2;
/// Seconds per mel frame.
const SECONDS_PER_FRAME: f64 = consts::HOP_LENGTH as f64 / consts::SAMPLE_RATE as f64;
/// A window's first timestamp is at most this many steps (1 s) in.
const MAX_INITIAL_TIMESTAMP: usize = 50;
/// Audio left over shorter than this (0.1 s) is not decoded.
const MIN_WINDOW_FRAMES: usize = 10;

/// The special tokens decoding needs.
#[derive(Debug, Clone)]
struct Tokens {
    eot: u32,
    no_timestamps: u32,
    /// `<|0.00|>`; every token from here up is a timestamp.
    timestamp_begin: u32,
    /// What every window's decode starts from.
    prompt: Vec<u32>,
}

/// The `preprocessor_config.json` fields used here.
#[derive(Deserialize)]
struct Preprocessor {
    /// `num_mel_bins` rows of `N_FFT / 2 + 1` weights.
    mel_filters: Vec<Vec<f32>>,
}

/// A loaded model, ready to decode.
pub struct WhisperModel {
    model: Whisper,
    tokenizer: Tokenizer,
    filters: Vec<f32>,
    tokens: Tokens,
    suppress: Vec<u32>,
    begin_suppress: Vec<u32>,
}

/// One window's decode.
#[derive(Debug, Clone)]
struct Decoded {
    /// Sampled tokens, without the prompt or the end-of-text token.
    tokens: Vec<u32>,
    avg_logprob: f64,
}

impl WhisperModel {
    /// Loads the model from `model_dir`. Every problem with the directory is
    /// a [`CoreError::Config`] that names it.
    pub fn load(model_dir: &Path) -> Result<Self> {
        check_model_dir(model_dir)?;
        let bad = |what: &str, err: &dyn std::fmt::Display| {
            config_error(format!("{}: {what}: {err}", model_dir.display()))
        };
        let read = |name: &str| {
            let path = model_dir.join(name);
            fs::read_to_string(&path).map_err(|err| CoreError::io(path, err))
        };

        let config_text = read("config.json")?;
        let config: Config =
            serde_json::from_str(&config_text).map_err(|err| bad("config.json", &err))?;
        let raw: Value =
            serde_json::from_str(&config_text).map_err(|err| bad("config.json", &err))?;

        let preprocessor: Preprocessor = serde_json::from_str(&read("preprocessor_config.json")?)
            .map_err(|err| bad("preprocessor_config.json", &err))?;
        let bins = consts::N_FFT / 2 + 1;
        if preprocessor.mel_filters.len() != config.num_mel_bins
            || preprocessor.mel_filters.iter().any(|row| row.len() != bins)
        {
            return Err(bad(
                "preprocessor_config.json",
                &format!("mel_filters must be {} rows of {bins}", config.num_mel_bins),
            ));
        }
        let filters = preprocessor.mel_filters.concat();

        let tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|err| bad("tokenizer.json", &err))?;
        let id = |token: &str| {
            tokenizer
                .token_to_id(token)
                .ok_or_else(|| bad("tokenizer.json", &format!("no {token} token")))
        };
        let sot = id(consts::SOT_TOKEN)?;
        let no_timestamps = id(consts::NO_TIMESTAMPS_TOKEN)?;
        // A multilingual model (its vocabulary has the language tokens) is
        // told the language and the task; an English-only one needs neither.
        let prompt = if config.vocab_size >= 51_865 {
            vec![sot, id("<|en|>")?, id(consts::TRANSCRIBE_TOKEN)?]
        } else {
            vec![sot]
        };
        let tokens = Tokens {
            eot: id(consts::EOT_TOKEN)?,
            no_timestamps,
            timestamp_begin: tokenizer
                .token_to_id("<|0.00|>")
                .unwrap_or(no_timestamps + 1),
            prompt,
        };
        let begin_suppress = match raw.get("begin_suppress_tokens") {
            Some(list) => serde_json::from_value(list.clone())
                .map_err(|err| bad("config.json begin_suppress_tokens", &err))?,
            // A space and end-of-text can't start a transcript.
            None => vec![220, tokens.eot],
        };

        // Read into memory rather than mapped: mapping is an `unsafe fn` and
        // the workspace forbids `unsafe` (see `CrossEncoder::load`).
        let weights_path = model_dir.join("model.safetensors");
        let weights = fs::read(&weights_path).map_err(|err| CoreError::io(&weights_path, err))?;
        let vb = VarBuilder::from_buffered_safetensors(weights, consts::DTYPE, &Device::Cpu)
            .map_err(|err| bad("model.safetensors", &err))?;
        let suppress = config.suppress_tokens.clone();
        let model = Whisper::load(&vb, config).map_err(|err| bad("model.safetensors", &err))?;

        Ok(Self {
            model,
            tokenizer,
            filters,
            tokens,
            suppress,
            begin_suppress,
        })
    }

    /// Transcribes `pcm`, any rate, mono.
    pub fn transcribe(&mut self, pcm: &Pcm) -> Result<Vec<Segment>> {
        let candle = |err: candle_core::Error| inference_error(err.to_string());
        let audio = if pcm.rate == consts::SAMPLE_RATE as u32 {
            pcm.clone()
        } else {
            pcm.resample(consts::SAMPLE_RATE as u32)?
        };
        let duration = audio.seconds();
        let content_frames = audio.len() / consts::HOP_LENGTH;
        // 30 s of silence after the audio, so every window has a full 30 s
        // of mel frames, as Whisper was trained on.
        let mut samples = audio.samples;
        samples.resize(samples.len() + consts::N_SAMPLES, 0.0);
        let mel = pcm_to_mel(&self.model.config, &samples, &self.filters);
        let n_mels = self.model.config.num_mel_bins;
        let frames = mel.len() / n_mels;
        let mel = Tensor::from_vec(mel, (1, n_mels, frames), &Device::Cpu).map_err(candle)?;

        let mut segments = Vec::new();
        let mut seek = 0;
        let mut window = 0u64;
        while seek + MIN_WINDOW_FRAMES <= content_frames {
            let size = (content_frames - seek).min(consts::N_FRAMES);
            let input = mel.narrow(2, seek, consts::N_FRAMES).map_err(candle)?;
            let features = self.model.encoder.forward(&input, true).map_err(candle)?;
            let decoded = self.decode_with_fallback(&features, window)?;
            let (parts, advance) =
                split_segments(&decoded.tokens, self.tokens.timestamp_begin, size);
            let offset = seek as f64 * SECONDS_PER_FRAME;
            for part in parts {
                let text = self
                    .tokenizer
                    .decode(&part.text, true)
                    .map_err(|err| inference_error(format!("decoding tokens: {err}")))?;
                let start = (offset + part.start).min(duration);
                let text = text.trim();
                if text.is_empty() || start >= duration {
                    continue;
                }
                segments.push(Segment {
                    text: text.to_owned(),
                    start,
                    end: (offset + part.end).clamp(start, duration),
                });
            }
            seek += advance;
            window += 1;
        }
        Ok(segments)
    }

    /// Decodes one window at each temperature in turn until the output
    /// passes [`needs_fallback`]; the last try is kept whatever it is.
    fn decode_with_fallback(&mut self, features: &Tensor, window: u64) -> Result<Decoded> {
        let mut decoded = None;
        for (i, &temperature) in TEMPERATURES.iter().enumerate() {
            let mut rng = SplitMix64(window.wrapping_mul(31).wrapping_add(i as u64));
            let attempt = self.decode(features, temperature, &mut rng)?;
            if !needs_fallback(&attempt.tokens, attempt.avg_logprob) {
                return Ok(attempt);
            }
            tracing::debug!(
                window,
                temperature,
                avg_logprob = attempt.avg_logprob,
                compression_ratio = compression_ratio(&attempt.tokens),
                "Whisper window looks wrong; decoding again hotter"
            );
            decoded = Some(attempt);
        }
        Ok(decoded.expect("TEMPERATURES is not empty"))
    }

    /// Greedy decoding at temperature 0, sampling above it.
    fn decode(
        &mut self,
        features: &Tensor,
        temperature: f64,
        rng: &mut SplitMix64,
    ) -> Result<Decoded> {
        let candle = |err: candle_core::Error| inference_error(err.to_string());
        let mut tokens = self.tokens.prompt.clone();
        let mut sampled: Vec<u32> = Vec::new();
        let mut sum_logprob = 0.0;
        let max_tokens = self.model.config.max_target_positions / 2;
        for step in 0..max_tokens {
            let input = Tensor::new(tokens.as_slice(), &Device::Cpu)
                .and_then(|t| t.unsqueeze(0))
                .map_err(candle)?;
            // The decoder caches the audio features' keys and values; the
            // first step of each decode refreshes them.
            let hidden = self
                .model
                .decoder
                .forward(&input, features, step == 0)
                .map_err(candle)?;
            let last = hidden.dim(1).map_err(candle)? - 1;
            let mut logits: Vec<f32> = hidden
                .i((..1, last..))
                .and_then(|h| self.model.decoder.final_linear(&h))
                .and_then(|l| l.i((0, 0)))
                .and_then(|l| l.to_vec1())
                .map_err(candle)?;
            self.apply_rules(&mut logits, &sampled);
            let log_probs = log_softmax(&logits);
            let next = if temperature > 0.0 {
                sample(&logits, temperature, rng)
            } else {
                argmax(&logits)
            };
            if next == self.tokens.eot {
                break;
            }
            sum_logprob += f64::from(log_probs[next as usize]);
            tokens.push(next);
            sampled.push(next);
        }
        Ok(Decoded {
            avg_logprob: sum_logprob / (sampled.len() + 1) as f64,
            tokens: sampled,
        })
    }

    /// Masks tokens that can't come next: the model's suppressed tokens,
    /// and Whisper's timestamp rules (timestamps come in pairs around each
    /// segment's text, never go backwards, start within the first second,
    /// and win when they are likelier together than any single word).
    fn apply_rules(&self, logits: &mut [f32], sampled: &[u32]) {
        let never = f32::NEG_INFINITY;
        let mut mask = |token: u32| {
            if let Some(logit) = logits.get_mut(token as usize) {
                *logit = never;
            }
        };
        self.suppress.iter().for_each(|&t| mask(t));
        mask(self.tokens.no_timestamps);
        if sampled.is_empty() {
            self.begin_suppress.iter().for_each(|&t| mask(t));
        }

        let begin = (self.tokens.timestamp_begin as usize).min(logits.len());
        let eot = (self.tokens.eot as usize).min(begin);
        let is_timestamp = |t: u32| t >= self.tokens.timestamp_begin;
        let last = sampled.last().is_some_and(|&t| is_timestamp(t));
        let penultimate = sampled.len() < 2 || is_timestamp(sampled[sampled.len() - 2]);
        if last {
            if penultimate {
                // A segment just closed: text (or the end) comes next.
                logits[begin..].fill(never);
            } else {
                // A segment's text just ended: close it.
                logits[..eot].fill(never);
            }
        }
        if let Some(&previous) = sampled.iter().rev().find(|&&t| is_timestamp(t)) {
            // Never backwards, and each segment at least one step long.
            let floor = if last && !penultimate {
                previous
            } else {
                previous + 1
            };
            let floor = (floor as usize).min(logits.len());
            logits[begin..floor].fill(never);
        }
        if sampled.is_empty() {
            logits[..begin].fill(never);
            let latest = (begin + MAX_INITIAL_TIMESTAMP + 1).min(logits.len());
            logits[latest..].fill(never);
        }

        let log_probs = log_softmax(logits);
        let timestamps = log_sum_exp(&log_probs[begin..]);
        let best_text = log_probs[..begin].iter().copied().fold(never, f32::max);
        if timestamps > best_text {
            logits[..begin].fill(never);
        }
    }
}

/// A segment of one window, before its tokens are turned into text.
#[derive(Debug, Clone, PartialEq)]
struct Part {
    /// Seconds from the window's start.
    start: f64,
    end: f64,
    text: Vec<u32>,
}

/// Cuts a window's tokens into timed segments and says how many mel frames
/// to move on: to the start of an unfinished last segment, which the next
/// window decodes again whole, or past the window when every segment closed.
/// `size` is the window's length in frames (30 s, or less at the end).
fn split_segments(tokens: &[u32], timestamp_begin: u32, size: usize) -> (Vec<Part>, usize) {
    let is_timestamp = |t: u32| t >= timestamp_begin;
    let seconds = |t: u32| f64::from(t.saturating_sub(timestamp_begin)) * SECONDS_PER_STEP;
    let text = |tokens: &[u32]| -> Vec<u32> {
        tokens
            .iter()
            .copied()
            .filter(|&t| !is_timestamp(t))
            .collect()
    };
    let n = tokens.len();
    let single_ending = n >= 2 && !is_timestamp(tokens[n - 2]) && is_timestamp(tokens[n - 1]);
    // Where one segment's closing timestamp meets the next one's opening.
    let mut cuts: Vec<usize> = (1..n)
        .filter(|&i| is_timestamp(tokens[i - 1]) && is_timestamp(tokens[i]))
        .collect();

    if cuts.is_empty() {
        let mut end = size as f64 * SECONDS_PER_FRAME;
        if let Some(&last) = tokens.iter().rev().find(|&&t| is_timestamp(t))
            && last != timestamp_begin
        {
            end = seconds(last);
        }
        let part = Part {
            start: 0.0,
            end,
            text: text(tokens),
        };
        return (vec![part], size);
    }

    if single_ending {
        cuts.push(n);
    }
    let mut parts = Vec::with_capacity(cuts.len());
    let mut from = 0;
    for &to in &cuts {
        let slice = &tokens[from..to];
        let start = slice
            .first()
            .copied()
            .filter(|&t| is_timestamp(t))
            .map_or(0.0, seconds);
        let end = slice
            .last()
            .copied()
            .filter(|&t| is_timestamp(t))
            .map_or(start, seconds);
        parts.push(Part {
            start,
            end: end.max(start),
            text: text(slice),
        });
        from = to;
    }
    let advance = if single_ending {
        size
    } else {
        tokens[from - 1].saturating_sub(timestamp_begin) as usize * FRAMES_PER_STEP
    };
    (
        parts,
        if advance == 0 {
            size
        } else {
            advance.min(size)
        },
    )
}

/// Whether a window must be decoded again hotter: its tokens are too
/// repetitive, or too unlikely.
pub fn needs_fallback(tokens: &[u32], avg_logprob: f64) -> bool {
    compression_ratio(tokens) > COMPRESSION_RATIO_THRESHOLD || avg_logprob < LOGPROB_THRESHOLD
}

/// How many times smaller zlib packs the tokens (as 2-byte little-endian
/// ids, as transformers measures it). A loop compresses well.
pub fn compression_ratio(tokens: &[u32]) -> f64 {
    if tokens.is_empty() {
        return 0.0;
    }
    let bytes: Vec<u8> = tokens
        .iter()
        .flat_map(|&t| u16::try_from(t).unwrap_or(u16::MAX).to_le_bytes())
        .collect();
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 6);
    bytes.len() as f64 / packed.len() as f64
}

fn log_sum_exp(values: &[f32]) -> f32 {
    let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if max == f32::NEG_INFINITY {
        return max;
    }
    max + values.iter().map(|v| (v - max).exp()).sum::<f32>().ln()
}

fn log_softmax(logits: &[f32]) -> Vec<f32> {
    let total = log_sum_exp(logits);
    logits.iter().map(|l| l - total).collect()
}

fn argmax(values: &[f32]) -> u32 {
    let mut best = 0;
    for (i, v) in values.iter().enumerate() {
        if *v > values[best] {
            best = i;
        }
    }
    best as u32
}

/// Draws a token from `softmax(logits / temperature)`.
fn sample(logits: &[f32], temperature: f64, rng: &mut SplitMix64) -> u32 {
    let scaled: Vec<f64> = logits.iter().map(|&l| f64::from(l) / temperature).collect();
    let max = scaled.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let weights: Vec<f64> = scaled.iter().map(|l| (l - max).exp()).collect();
    let mut target = rng.next_f64() * weights.iter().sum::<f64>();
    for (i, w) in weights.iter().enumerate() {
        target -= w;
        if target <= 0.0 && *w > 0.0 {
            return i as u32;
        }
    }
    argmax(logits)
}

/// A tiny seeded random number generator (SplitMix64), so sampling at a
/// higher temperature is the same on every run, and cached transcripts stay
/// true to what a rerun would hear.
struct SplitMix64(u64);

impl SplitMix64 {
    /// Uniform in `[0, 1)`.
    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Fails, naming the download command, unless `model_dir` has every file in
/// [`MODEL_FILES`].
pub fn check_model_dir(model_dir: &Path) -> Result<()> {
    let missing: Vec<&str> = MODEL_FILES
        .into_iter()
        .filter(|name| !model_dir.join(name).is_file())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(config_error(format!(
        "Whisper model directory {} is missing {}; download the model with \
         `hf download openai/whisper-base.en --local-dir {}`",
        model_dir.display(),
        missing.join(", "),
        model_dir.display()
    )))
}

fn config_error(message: impl Into<String>) -> CoreError {
    CoreError::Config {
        message: message.into(),
    }
}

fn inference_error(message: impl Into<String>) -> CoreError {
    CoreError::Provider {
        plugin: PLUGIN.into(),
        kind: ProviderFailure::Other,
        message: message.into(),
    }
}

/// The [`AsrProvider`] over a local Whisper model, on the CPU so the GPU
/// stays free for the TTS model.
///
/// Construction is cheap: it checks the files and fingerprints their bytes.
/// The model loads on the first [`transcribe`] call, so a run whose
/// transcripts are all cached never loads it.
///
/// [`transcribe`]: AsrProvider::transcribe
pub struct CandleWhisper {
    model_dir: PathBuf,
    weights: String,
    model: Option<WhisperModel>,
}

impl CandleWhisper {
    pub fn new(model_dir: &Path) -> Result<Self> {
        check_model_dir(model_dir)?;
        let mut combined = blake3::Hasher::new();
        for name in MODEL_FILES {
            let path = model_dir.join(name);
            let mut file = fs::File::open(&path).map_err(|err| CoreError::io(&path, err))?;
            let mut hasher = blake3::Hasher::new();
            hasher
                .update_reader(&mut file)
                .map_err(|err| CoreError::io(&path, err))?;
            combined.update(hasher.finalize().as_bytes());
        }
        Ok(Self {
            model_dir: model_dir.to_owned(),
            weights: combined.finalize().to_hex().to_string(),
            model: None,
        })
    }

    /// The loaded model, loading it on first use.
    fn model(&mut self) -> Result<&mut WhisperModel> {
        // `take` moves the model out (leaving `None`), and `insert` puts it
        // back and hands out a `&mut` to it: one path for both cases, with
        // no "is it loaded?" check the borrow checker would have to follow.
        let model = match self.model.take() {
            Some(model) => model,
            None => {
                let started = Instant::now();
                let model = WhisperModel::load(&self.model_dir)?;
                tracing::info!(
                    model_dir = %self.model_dir.display(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Whisper model loaded"
                );
                model
            }
        };
        Ok(self.model.insert(model))
    }
}

impl AsrProvider for CandleWhisper {
    fn id(&self) -> &str {
        PLUGIN
    }

    /// The weights' hash, not the directory, plus the decoding rules.
    fn fingerprint(&self) -> Value {
        json!({
            "provider": PLUGIN,
            "weights_blake3": self.weights,
            "temperatures": TEMPERATURES,
            "compression_ratio_threshold": COMPRESSION_RATIO_THRESHOLD,
            "logprob_threshold": LOGPROB_THRESHOLD,
            "version": WHISPER_VERSION,
        })
    }

    fn transcribe(&mut self, request: &AsrRequest<'_>) -> Result<Transcript> {
        let started = Instant::now();
        let segments = self.model()?.transcribe(request.pcm)?;
        let elapsed = started.elapsed().as_secs_f64();
        tracing::info!(
            seconds = request.pcm.seconds(),
            segments = segments.len(),
            elapsed_ms = (elapsed * 1000.0) as u64,
            rtf = elapsed / request.pcm.seconds().max(f64::EPSILON),
            "chunk transcribed"
        );
        Ok(Transcript { segments })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repetitive_window_triggers_the_fallback() {
        // "The size of the graphs." five times over, as token ids.
        let phrase = [464, 2546, 286, 262, 28770, 13];
        let looped: Vec<u32> = phrase.iter().copied().cycle().take(30).collect();
        assert!(compression_ratio(&looped) > COMPRESSION_RATIO_THRESHOLD);
        assert!(needs_fallback(&looped, -0.1));

        let varied: Vec<u32> = (0..30).map(|i| 300 + i * 997 % 50_000).collect();
        assert!(compression_ratio(&varied) < COMPRESSION_RATIO_THRESHOLD);
        assert!(!needs_fallback(&varied, -0.3));
        assert!(needs_fallback(&varied, -1.5), "too unlikely");
    }

    const TS: u32 = 50_363;

    #[test]
    fn closed_segments_move_past_the_window() {
        // <0.00> a b <2.00><2.00> c <4.00>
        let tokens = [TS, 1, 2, TS + 100, TS + 100, 3, TS + 200];
        let (parts, advance) = split_segments(&tokens, TS, 3000);
        assert_eq!(parts.len(), 2);
        assert_eq!(
            (parts[0].start, parts[0].end, &parts[0].text),
            (0.0, 2.0, &vec![1, 2])
        );
        assert_eq!(
            (parts[1].start, parts[1].end, &parts[1].text),
            (2.0, 4.0, &vec![3])
        );
        assert_eq!(advance, 3000);
    }

    #[test]
    fn an_unfinished_segment_is_decoded_again_in_the_next_window() {
        // <0.00> a <2.00><2.00> b <3.00><3.00> c (cut off by the window's end)
        let tokens = [TS, 1, TS + 100, TS + 100, 2, TS + 150, TS + 150, 3];
        let (parts, advance) = split_segments(&tokens, TS, 3000);
        assert_eq!(parts.len(), 2, "c is dropped");
        assert_eq!(parts[1].text, [2]);
        // Back to 3.00 s: 150 steps of two frames.
        assert_eq!(advance, 300);
    }

    #[test]
    fn without_timestamp_pairs_the_window_is_one_segment() {
        let (parts, advance) = split_segments(&[TS, 1, 2, TS + 120], TS, 1500);
        assert_eq!(
            parts,
            [Part {
                start: 0.0,
                end: 2.4,
                text: vec![1, 2]
            }]
        );
        assert_eq!(advance, 1500);
    }

    #[test]
    fn sampling_is_seeded() {
        let logits = [0.0f32, 1.0, 2.0, 0.5];
        let draw = |seed| {
            let mut rng = SplitMix64(seed);
            (0..20)
                .map(|_| sample(&logits, 1.0, &mut rng))
                .collect::<Vec<_>>()
        };
        assert_eq!(draw(7), draw(7));
        assert_ne!(draw(7), draw(8));
        let mut rng = SplitMix64(1);
        assert_eq!(sample(&[0.0, f32::NEG_INFINITY], 0.5, &mut rng), 0);
    }

    #[test]
    fn a_missing_file_names_the_download() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), "{}").unwrap();
        let err = CandleWhisper::new(dir.path()).err().unwrap().to_string();
        assert!(
            err.contains("model.safetensors") && err.contains("hf download openai/whisper-base.en"),
            "{err}"
        );
    }
}
