//! Joins the synthesised chunks into one episode at podcast loudness.
//!
//! In order:
//! 1. Each chunk is resampled to 48 kHz and its loudness matched to the
//!    median chunk's, so a take that came out quieter doesn't stand out.
//! 2. Each chunk is cut into its pieces by the turn spans (the backend's, or
//!    the recogniser's), and each piece loses the model's own silence at
//!    its ends.
//! 3. The pieces are laid end to end with the silence the script's pacing
//!    asks for before each turn. An interrupting turn instead starts before
//!    the one it cuts into has finished, with an equal-power crossfade.
//! 4. Sounds made over (or around) someone else's turn are mixed in on a
//!    second track, 6 dB down.
//! 5. The episode is normalised to −16 LUFS integrated (EBU R128, the podcast
//!    standard) and a limiter holds the true peak to −1 dBTP. The limiter
//!    acts only around the peaks; everything else keeps the one gain.

use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_2;
use std::ops::Range;

use ebur128::{EbuR128, Mode};
use podling_types::{Gaps, NonverbalAt, Pace, Script};

use crate::audio::Pcm;
use crate::error::{CoreError, Result};
use crate::stages::plan_chunks::Piece;
use crate::stages::synthesize::{OverlayClip, SynthesizedChunk};

/// The episode file's sample rate.
pub const SAMPLE_RATE: u32 = 48_000;
/// Integrated loudness the episode is normalised to, in LUFS.
pub const TARGET_LUFS: f64 = -16.0;
/// The highest true peak allowed, in dBTP.
pub const PEAK_CEILING_DBTP: f64 = -1.0;
/// Below this amplitude a sample counts as silence when trimming (−50 dBFS).
const SILENCE: f32 = 0.003_162;
/// Sounds on the second track are mixed 6 dB below their own level.
const OVERLAY_GAIN_DB: f64 = -6.0;
/// The limiter starts this far below the true-peak ceiling: it limits sample
/// values, and the peak between samples can be a little higher.
const LIMITER_HEADROOM_DB: f64 = 0.5;
/// The limiter's gain ramps down over two of these before a peak and back up
/// over two after it.
const LIMITER_HALF_WINDOW_MS: u32 = 5;
/// Limiter passes per round before giving up on the ceiling (each
/// re-measures).
const LIMITER_PASSES: usize = 4;
/// Rounds of gain-then-limit before settling for what was reached.
const NORMALISE_ROUNDS: usize = 6;
/// Close enough to the target loudness to stop, in LU.
const LOUDNESS_TOLERANCE_LU: f64 = 0.2;

const STAGE: &str = "assemble";

/// A finished episode and how loud it measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembled {
    pub pcm: Pcm,
    pub loudness: Loudness,
    /// Where each chunk ended up in `pcm`, in samples; `None` for a chunk
    /// that was silent and left out.
    pub chunks: Vec<Option<Range<usize>>>,
}

/// EBU R128 measurements of a mono signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Integrated loudness, in LUFS.
    pub integrated_lufs: f64,
    /// True peak, in dBTP.
    pub true_peak_dbtp: f64,
}

/// Puts `chunks` (in playback order) and the `overlays` together into one
/// normalised episode, with the silences in `gaps`.
pub fn assemble(
    script: &Script,
    chunks: &[SynthesizedChunk],
    overlays: &[OverlayClip],
    gaps: &Gaps,
) -> Result<Assembled> {
    let mut pcms = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        pcms.push(chunk.pcm.resample(SAMPLE_RATE)?);
    }
    match_loudness(&mut pcms);

    // An hour at 48 kHz is ~700 MB of samples, so each copy is let go as
    // soon as the next one is made: a chunk once it is cut into blocks, the
    // blocks once they are laid out.
    let mut blocks = Vec::new();
    for (index, (chunk, pcm)) in chunks.iter().zip(pcms).enumerate() {
        blocks.extend(blocks_of(script, index, chunk, &pcm));
    }
    let mut timeline = lay_out(&blocks, script.turns().len(), chunks.len(), gaps);
    drop(blocks);
    for overlay in overlays {
        let pcm = overlay.pcm.resample(SAMPLE_RATE)?;
        let placed = timeline.turns.get(overlay.turn).cloned().flatten();
        let Some(turn) = placed else {
            tracing::warn!(
                turn = overlay.turn,
                "a sound's turn is silent; sound left out"
            );
            continue;
        };
        mix_in(
            &mut timeline.samples,
            &turn,
            overlay.at,
            &trim_silence(&pcm),
        );
    }

    let (pcm, loudness) = normalise(Pcm::new(SAMPLE_RATE, timeline.samples))?;
    Ok(Assembled {
        pcm,
        loudness,
        chunks: timeline.chunks,
    })
}

/// Scales each measurable chunk to the median chunk's integrated loudness.
/// A chunk too short or quiet to measure is left as it is.
fn match_loudness(pcms: &mut [Pcm]) {
    let measured: Vec<Option<f64>> = pcms.iter().map(integrated_lufs).collect();
    let mut known: Vec<f64> = measured.iter().flatten().copied().collect();
    known.sort_by(f64::total_cmp);
    let Some(&median) = known.get(known.len() / 2) else {
        return;
    };
    for (pcm, lufs) in pcms.iter_mut().zip(measured) {
        if let Some(lufs) = lufs {
            let gain = db_to_gain(median - lufs);
            pcm.samples.iter_mut().for_each(|s| *s *= gain);
        }
    }
    tracing::debug!(
        median_lufs = median,
        chunks = known.len(),
        "chunk loudness matched"
    );
}

/// A stretch of speech to place: one piece of a turn, or a whole chunk when
/// there are no spans to cut it by.
#[derive(Debug, Clone, PartialEq)]
struct Block {
    samples: Vec<f32>,
    /// Every turn the block speaks some of.
    turns: Range<usize>,
    chunk: usize,
    /// The silence before it: the turn's pace where the turn starts, normal
    /// pace where a long turn carries on from the chunk before.
    pace: Pace,
}

/// `pcm` (the chunk at 48 kHz) cut into its pieces, each trimmed.
fn blocks_of(script: &Script, index: usize, chunk: &SynthesizedChunk, pcm: &Pcm) -> Vec<Block> {
    let pace_of = |piece: &Piece| {
        if piece.starts_turn() {
            script.turns()[piece.turn].pace
        } else {
            Pace::Normal
        }
    };
    let scale = |at: usize| {
        let at = at as u64 * u64::from(SAMPLE_RATE) / u64::from(chunk.pcm.rate.max(1));
        (at as usize).min(pcm.len())
    };
    let spans = chunk
        .turn_spans
        .as_ref()
        .filter(|spans| spans.len() == chunk.pieces.len());
    let blocks: Vec<Block> = match spans {
        Some(spans) => chunk
            .pieces
            .iter()
            .zip(spans)
            .map(|(piece, span)| Block {
                samples: trim(
                    &pcm.samples[scale(span.start)..scale(span.end).max(scale(span.start))],
                ),
                turns: piece.turn..piece.turn + 1,
                chunk: index,
                pace: pace_of(piece),
            })
            .collect(),
        None => {
            let first = chunk.pieces.first().expect("a chunk is never empty");
            let last = chunk.pieces.last().expect("a chunk is never empty");
            vec![Block {
                samples: trim(&pcm.samples),
                turns: first.turn..last.turn + 1,
                chunk: index,
                pace: pace_of(first),
            }]
        }
    };
    blocks
        .into_iter()
        .filter(|block| {
            let silent = block.samples.is_empty();
            if silent {
                tracing::warn!(chunk = index, turns = ?block.turns, "piece is silent after trimming");
            }
            !silent
        })
        .collect()
}

/// The blocks laid end to end, and where each turn and chunk ended up.
#[derive(Debug, Clone, PartialEq)]
struct Timeline {
    samples: Vec<f32>,
    turns: Vec<Option<Range<usize>>>,
    chunks: Vec<Option<Range<usize>>>,
}

fn lay_out(blocks: &[Block], turns: usize, chunks: usize, gaps: &Gaps) -> Timeline {
    let mut timeline = Timeline {
        samples: Vec::new(),
        turns: vec![None; turns],
        chunks: vec![None; chunks],
    };
    let out = &mut timeline.samples;
    let mut previous_len = 0;
    for block in blocks {
        let start = if out.is_empty() {
            0
        } else if block.pace == Pace::Interrupt {
            let overlap = samples_of(gaps.interrupt)
                .min(previous_len)
                .min(block.samples.len());
            crossfade(out, &block.samples[..overlap]);
            out.len() - overlap
        } else {
            out.resize(out.len() + samples_of(gap_ms(gaps, block.pace)), 0.0);
            out.len()
        };
        let overlap = out.len() - start;
        out.extend_from_slice(&block.samples[overlap..]);
        let placed = start..out.len();
        for turn in block.turns.clone() {
            widen(&mut timeline.turns[turn], &placed);
        }
        widen(&mut timeline.chunks[block.chunk], &placed);
        previous_len = block.samples.len();
    }
    timeline
}

/// The silence before a turn at `pace` (an interrupt has none: it overlaps).
fn gap_ms(gaps: &Gaps, pace: Pace) -> u16 {
    match pace {
        Pace::Quick => gaps.quick,
        Pace::Normal => gaps.normal,
        Pace::Beat => gaps.beat,
        Pace::LongPause => gaps.long_pause,
        Pace::Interrupt => 0,
    }
}

fn samples_of(ms: u16) -> usize {
    (u64::from(SAMPLE_RATE) * u64::from(ms) / 1000) as usize
}

/// `range` grown to cover `more` too.
fn widen(range: &mut Option<Range<usize>>, more: &Range<usize>) {
    *range = Some(match range.take() {
        Some(r) => r.start.min(more.start)..r.end.max(more.end),
        None => more.clone(),
    });
}

/// Fades the last `head.len()` samples of `out` down while `head` fades up,
/// along a quarter sine and cosine: their squares add to one, so the sum of
/// two unrelated voices keeps a steady loudness through the overlap.
fn crossfade(out: &mut [f32], head: &[f32]) {
    let tail = out.len() - head.len();
    for (i, (old, new)) in out[tail..].iter_mut().zip(head).enumerate() {
        let angle = (i as f32 + 0.5) / head.len() as f32 * FRAC_PI_2;
        *old = *old * angle.cos() + new * angle.sin();
    }
}

/// Adds `clip`, 6 dB down, at its place in `turn`: centred in it for a sound
/// over the turn, ending as the turn starts for one before it, and starting
/// as it ends for one after. The track grows if the clip runs past its end.
fn mix_in(track: &mut Vec<f32>, turn: &Range<usize>, at: NonverbalAt, clip: &[f32]) {
    let start = match at {
        NonverbalAt::Over => turn.start + turn.len().saturating_sub(clip.len()) / 2,
        NonverbalAt::Before => turn.start.saturating_sub(clip.len()),
        NonverbalAt::After => turn.end,
    };
    let end = start + clip.len();
    if track.len() < end {
        track.resize(end, 0.0);
    }
    let gain = db_to_gain(OVERLAY_GAIN_DB);
    for (out, sample) in track[start..end].iter_mut().zip(clip) {
        *out += sample * gain;
    }
}

/// Gains `pcm` to the target loudness, then limits its true peak.
fn normalise(mut pcm: Pcm) -> Result<(Pcm, Loudness)> {
    let before = measure(&pcm)?;
    let mut loudness = before;
    let half = (SAMPLE_RATE * LIMITER_HALF_WINDOW_MS / 1000) as usize;
    let mut ceiling_db = PEAK_CEILING_DBTP - LIMITER_HEADROOM_DB;
    let mut passes = 0;
    // Limiting takes some loudness away with the peaks, so the episode is
    // gained and limited again until it settles on the target. Speech
    // settles in one round; a few loud bursts can take more.
    for _ in 0..NORMALISE_ROUNDS {
        let gain = db_to_gain(TARGET_LUFS - loudness.integrated_lufs);
        pcm.samples.iter_mut().for_each(|s| *s *= gain);
        loudness = measure(&pcm)?;
        let mut round_passes = 0;
        while loudness.true_peak_dbtp > PEAK_CEILING_DBTP && round_passes < LIMITER_PASSES {
            limit(&mut pcm.samples, db_to_gain(ceiling_db), half);
            loudness = measure(&pcm)?;
            // The peak between samples overshot: aim lower next time.
            ceiling_db -= (loudness.true_peak_dbtp - PEAK_CEILING_DBTP).max(0.0) + 0.1;
            round_passes += 1;
        }
        passes += round_passes;
        if (loudness.integrated_lufs - TARGET_LUFS).abs() <= LOUDNESS_TOLERANCE_LU {
            break;
        }
    }
    if loudness.true_peak_dbtp > PEAK_CEILING_DBTP
        || (loudness.integrated_lufs - TARGET_LUFS).abs() > LOUDNESS_TOLERANCE_LU
    {
        tracing::warn!(
            integrated_lufs = loudness.integrated_lufs,
            true_peak_dbtp = loudness.true_peak_dbtp,
            "the episode could not be brought to -16 LUFS within a -1 dBTP peak"
        );
    }
    tracing::info!(
        seconds = pcm.seconds(),
        measured_lufs = before.integrated_lufs,
        measured_true_peak_dbtp = before.true_peak_dbtp,
        limiter_passes = passes,
        integrated_lufs = loudness.integrated_lufs,
        true_peak_dbtp = loudness.true_peak_dbtp,
        "episode assembled"
    );
    Ok((pcm, loudness))
}

/// Lowers the gain around every sample louder than `ceiling` so that none
/// is. What each sample needs is `ceiling / |x|` (1 when under it); the
/// gain is the smallest need within ±`half` samples, then averaged over
/// ±`half`. At a peak, every value in that average is at most the peak's
/// own need, so the peak lands at or under the ceiling. Away from one,
/// both windows hold only 1s and the gain is exactly 1, so the limiter
/// works region by region and leaves the rest alone.
fn limit(samples: &mut [f32], ceiling: f32, half: usize) {
    let over: Vec<usize> = (0..samples.len())
        .filter(|&i| samples[i].abs() > ceiling)
        .collect();
    // Peaks closer than this share one region; regions further apart
    // cannot touch each other's gain.
    let reach = 4 * half;
    let mut first = 0;
    while first < over.len() {
        let mut last = first;
        while last + 1 < over.len() && over[last + 1] - over[last] <= reach {
            last += 1;
        }
        let region = over[first].saturating_sub(reach)..(over[last] + reach + 1).min(samples.len());
        let need: Vec<f32> = samples[region.clone()]
            .iter()
            .map(|x| {
                if x.abs() > ceiling {
                    ceiling / x.abs()
                } else {
                    1.0
                }
            })
            .collect();
        // Averaged as a cut (1 − gain), which is exactly 0 wherever the
        // window holds no peak: an averaged gain could round to 0.99999994
        // and touch samples the limiter has no business changing.
        let cut: Vec<f32> = sliding_min(&need, half).iter().map(|g| 1.0 - g).collect();
        for (sample, cut) in samples[region].iter_mut().zip(sliding_mean(&cut, half)) {
            *sample *= 1.0 - cut;
        }
        first = last + 1;
    }
}

/// The minimum of `values` within ±`half` of each index (fewer at the ends),
/// in one pass: `window` holds indices whose values increase from front to
/// back, so the front is always the minimum.
fn sliding_min(values: &[f32], half: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(values.len());
    let mut window: VecDeque<usize> = VecDeque::new();
    let mut next = 0;
    for i in 0..values.len() {
        while next < values.len() && next <= i + half {
            while window.back().is_some_and(|&j| values[j] >= values[next]) {
                window.pop_back();
            }
            window.push_back(next);
            next += 1;
        }
        while window.front().is_some_and(|&j| j + half < i) {
            window.pop_front();
        }
        out.push(values[*window.front().expect("holds index i at least")]);
    }
    out
}

/// The mean of `values` within ±`half` of each index (fewer at the ends).
fn sliding_mean(values: &[f32], half: usize) -> Vec<f32> {
    let mut prefix = Vec::with_capacity(values.len() + 1);
    prefix.push(0.0_f64);
    for &v in values {
        prefix.push(prefix[prefix.len() - 1] + f64::from(v));
    }
    (0..values.len())
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(values.len());
            ((prefix[hi] - prefix[lo]) / (hi - lo) as f64) as f32
        })
        .collect()
}

fn db_to_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// `samples` without their leading and trailing silence.
fn trim(samples: &[f32]) -> Vec<f32> {
    let loud = |s: &f32| s.abs() > SILENCE;
    match (
        samples.iter().position(loud),
        samples.iter().rposition(loud),
    ) {
        (Some(start), Some(end)) => samples[start..=end].to_vec(),
        _ => Vec::new(),
    }
}

fn trim_silence(pcm: &Pcm) -> Vec<f32> {
    trim(&pcm.samples)
}

/// Integrated loudness, or `None` for audio too quiet or short (under
/// 400 ms) to measure.
fn integrated_lufs(pcm: &Pcm) -> Option<f64> {
    let mut meter = EbuR128::new(1, pcm.rate, Mode::I).ok()?;
    meter.add_frames_f32(&pcm.samples).ok()?;
    meter.loudness_global().ok().filter(|l| l.is_finite())
}

/// Measures integrated loudness and true peak. Silence (or audio too short
/// to measure, under 400 ms) is an error: there is nothing to normalise.
pub fn measure(pcm: &Pcm) -> Result<Loudness> {
    let failed = |message: String| CoreError::InvalidProviderOutput {
        stage: STAGE,
        message,
    };
    let mut meter = EbuR128::new(1, pcm.rate, Mode::I | Mode::TRUE_PEAK)
        .map_err(|err| failed(format!("cannot measure loudness: {err}")))?;
    meter
        .add_frames_f32(&pcm.samples)
        .map_err(|err| failed(format!("cannot measure loudness: {err}")))?;
    let integrated_lufs = meter
        .loudness_global()
        .map_err(|err| failed(format!("cannot measure loudness: {err}")))?;
    if !integrated_lufs.is_finite() {
        return Err(failed(
            "the episode is silent (or under 400 ms), so it has no loudness to normalise".into(),
        ));
    }
    let peak = meter
        .true_peak(0)
        .map_err(|err| failed(format!("cannot measure the true peak: {err}")))?;
    Ok(Loudness {
        integrated_lufs,
        true_peak_dbtp: 20.0 * peak.log10(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone that never crosses zero at a sample (its phase is offset), so
    /// trimming keeps all of it.
    fn tone(seconds: f32, amplitude: f32, hz: f32) -> Vec<f32> {
        let n = (SAMPLE_RATE as f32 * seconds) as usize;
        (0..n)
            .map(|i| {
                let phase = (i as f32 + 0.25) * hz * std::f32::consts::TAU / SAMPLE_RATE as f32;
                amplitude * phase.sin().signum() * phase.sin().abs().max(0.01)
            })
            .collect()
    }

    fn block(turn: usize, seconds: f32, pace: Pace) -> Block {
        Block {
            samples: tone(seconds, 0.3, 220.0),
            turns: turn..turn + 1,
            chunk: 0,
            pace,
        }
    }

    fn ms(samples: usize) -> f64 {
        samples as f64 * 1000.0 / f64::from(SAMPLE_RATE)
    }

    #[test]
    fn trims_only_the_silent_ends() {
        let mut samples = vec![0.0; 100];
        samples.extend([0.5, 0.0, -0.5]);
        samples.extend(vec![0.001; 50]);
        assert_eq!(trim(&samples), vec![0.5, 0.0, -0.5]);
        assert!(trim(&[0.0; 10]).is_empty());
    }

    #[test]
    fn each_pace_puts_its_gap_before_the_turn() {
        let gaps = Gaps::default();
        for (pace, want) in [
            (Pace::Quick, 120.0),
            (Pace::Normal, 300.0),
            (Pace::Beat, 600.0),
            (Pace::LongPause, 1000.0),
        ] {
            let blocks = [block(0, 0.5, Pace::Normal), block(1, 0.5, pace)];
            let timeline = lay_out(&blocks, 2, 1, &gaps);
            let first = timeline.turns[0].clone().unwrap();
            let second = timeline.turns[1].clone().unwrap();
            let gap = ms(second.start - first.end);
            assert!((gap - want).abs() <= 10.0, "{pace:?}: {gap} ms");
            // Measured from the samples too: the silence between the turns.
            let silent = timeline.samples[first.end..second.start]
                .iter()
                .all(|&s| s == 0.0);
            assert!(silent, "{pace:?}");
        }
        // Configurable.
        let slow = Gaps {
            beat: 900,
            ..Gaps::default()
        };
        let timeline = lay_out(
            &[block(0, 0.5, Pace::Normal), block(1, 0.5, Pace::Beat)],
            2,
            1,
            &slow,
        );
        let (a, b) = (
            timeline.turns[0].clone().unwrap(),
            timeline.turns[1].clone().unwrap(),
        );
        assert!((ms(b.start - a.end) - 900.0).abs() <= 10.0);
    }

    #[test]
    fn an_interrupt_overlaps_the_turn_before_by_150_ms() {
        let blocks = [block(0, 1.0, Pace::Normal), block(1, 1.0, Pace::Interrupt)];
        let timeline = lay_out(&blocks, 2, 1, &Gaps::default());
        let (a, b) = (
            timeline.turns[0].clone().unwrap(),
            timeline.turns[1].clone().unwrap(),
        );
        assert_eq!(ms(a.end - b.start), 150.0);
        assert_eq!(
            timeline.samples.len(),
            2 * SAMPLE_RATE as usize - samples_of(150)
        );
        // Two in-phase tones at 0.3 through an equal-power crossfade peak
        // at 0.3·√2: louder for a moment, nowhere near clipping.
        let peak = timeline.samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(peak <= 0.3 * 2f32.sqrt() + 1e-4, "{peak}");
    }

    #[test]
    fn an_interrupt_never_overlaps_more_than_the_turn_before() {
        let blocks = [block(0, 0.05, Pace::Normal), block(1, 1.0, Pace::Interrupt)];
        let timeline = lay_out(&blocks, 2, 1, &Gaps::default());
        let b = timeline.turns[1].clone().unwrap();
        assert_eq!(b.start, 0);
        assert_eq!(timeline.samples.len(), SAMPLE_RATE as usize);
    }

    #[test]
    fn the_crossfade_keeps_equal_power() {
        let mut out = vec![1.0; 100];
        crossfade(&mut out, &[1.0; 100]);
        // cos + sin of the same angle: 1 at the ends, √2 in the middle.
        assert!((out[0] - 1.0).abs() < 0.02, "{}", out[0]);
        assert!((out[50] - 2f32.sqrt()).abs() < 0.01, "{}", out[50]);
        assert!((out[99] - 1.0).abs() < 0.02, "{}", out[99]);
    }

    #[test]
    fn an_overlay_changes_the_track_only_where_it_plays() {
        let main = tone(2.0, 0.3, 220.0);
        let turn = 0..main.len();
        let clip = tone(0.4, 0.5, 660.0);
        let mut mixed = main.clone();
        mix_in(&mut mixed, &turn, NonverbalAt::Over, &clip);
        assert_eq!(mixed.len(), main.len());
        // Centred in the turn.
        let start = (main.len() - clip.len()) / 2;
        let end = start + clip.len();
        assert_eq!(mixed[..start], main[..start]);
        assert_eq!(mixed[end..], main[end..]);
        // Inside, the clip is 6 dB down.
        let gain = db_to_gain(-6.0);
        for i in [start, start + 1000, end - 1] {
            let added = mixed[i] - main[i];
            assert!((added - clip[i - start] * gain).abs() < 1e-6);
        }

        // Before ends as the turn starts; after starts as it ends and grows
        // the track.
        let mut before = vec![0.0; 48_000];
        mix_in(&mut before, &(24_000..48_000), NonverbalAt::Before, &clip);
        assert_eq!(before[24_000..], vec![0.0; 24_000][..]);
        assert!(before[24_000 - 1] != 0.0);
        let mut after = vec![0.0; 48_000];
        mix_in(&mut after, &(0..48_000), NonverbalAt::After, &clip);
        assert_eq!(after.len(), 48_000 + clip.len());
        assert!(after[..48_000].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn chunks_are_matched_to_the_median_loudness() {
        let mut pcms: Vec<Pcm> = [0.05, 0.2, 0.8]
            .into_iter()
            .map(|a| Pcm::new(SAMPLE_RATE, tone(2.0, a, 440.0)))
            .collect();
        // Too short to measure: left alone.
        pcms.push(Pcm::new(SAMPLE_RATE, tone(0.1, 0.9, 440.0)));
        match_loudness(&mut pcms);
        let levels: Vec<f64> = pcms[..3]
            .iter()
            .map(|p| integrated_lufs(p).unwrap())
            .collect();
        let spread = levels.iter().fold(f64::MIN, |m, &l| m.max(l))
            - levels.iter().fold(f64::MAX, |m, &l| m.min(l));
        assert!(spread <= 1.0, "{levels:?}");
        assert_eq!(pcms[3].samples, tone(0.1, 0.9, 440.0));
    }

    #[test]
    fn the_limiter_holds_peaks_to_the_ceiling_and_leaves_the_rest() {
        let mut samples = tone(1.0, 0.3, 220.0);
        for (i, s) in samples.iter_mut().enumerate().skip(24_000).take(200) {
            *s = if i % 2 == 0 { 0.95 } else { -0.95 };
        }
        let original = samples.clone();
        let half = 240;
        limit(&mut samples, 0.5, half);
        let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(peak <= 0.5 + 1e-5, "{peak}");
        // Untouched more than two windows away from the burst.
        assert_eq!(samples[..24_000 - 2 * half], original[..24_000 - 2 * half]);
        assert_eq!(samples[24_200 + 2 * half..], original[24_200 + 2 * half..]);
    }

    #[test]
    fn the_sliding_minimum_matches_a_direct_one() {
        let values: Vec<f32> = (0..200).map(|i| ((i * 37) % 101) as f32).collect();
        for half in [0, 1, 5, 30] {
            let direct: Vec<f32> = (0..values.len())
                .map(|i| {
                    let lo = i.saturating_sub(half);
                    let hi = (i + half + 1).min(values.len());
                    values[lo..hi].iter().copied().fold(f32::MAX, f32::min)
                })
                .collect();
            assert_eq!(sliding_min(&values, half), direct, "half {half}");
        }
    }

    #[test]
    fn loud_audio_is_normalised_and_its_true_peak_limited() {
        // Speech-like: quiet with sharp loud bursts, so gaining it to −16
        // LUFS pushes the bursts far past −1 dBTP.
        let mut samples = tone(6.0, 0.05, 180.0);
        for burst in 0..6 {
            let at = burst * SAMPLE_RATE as usize + 10_000;
            for s in &mut samples[at..at + 400] {
                *s *= 18.0;
            }
        }
        let (pcm, loudness) = normalise(Pcm::new(SAMPLE_RATE, samples)).unwrap();
        let again = measure(&pcm).unwrap();
        assert_eq!(again, loudness);
        assert!(
            (again.integrated_lufs - TARGET_LUFS).abs() <= 0.5,
            "{again:?}"
        );
        assert!(again.true_peak_dbtp <= PEAK_CEILING_DBTP, "{again:?}");
    }

    #[test]
    fn a_silent_episode_is_an_error() {
        let err = normalise(Pcm::new(SAMPLE_RATE, vec![0.0; 48_000])).unwrap_err();
        assert!(err.to_string().contains("assemble"), "{err}");
    }
}
