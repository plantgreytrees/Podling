//! Joins the synthesised chunks into one episode at podcast loudness.
//!
//! Minimal for now: each chunk loses the model's own leading and trailing
//! silence, chunks are joined with a fixed gap, resampled to 48 kHz and the
//! whole is normalised to −16 LUFS (EBU R128, the podcast standard). Gaps
//! from the script's pacing, per-chunk loudness matching and a true-peak
//! limiter come later.

use ebur128::{EbuR128, Mode};

use crate::audio::Pcm;
use crate::error::{CoreError, Result};

/// The episode file's sample rate.
pub const SAMPLE_RATE: u32 = 48_000;
/// Integrated loudness the episode is normalised to, in LUFS.
pub const TARGET_LUFS: f64 = -16.0;
/// Silence between two chunks.
const GAP_MS: u32 = 300;
/// Below this amplitude a sample counts as silence when trimming (−50 dBFS).
const SILENCE: f32 = 0.003_162;

const STAGE: &str = "assemble";

/// A finished episode and how loud it measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembled {
    pub pcm: Pcm,
    pub loudness: Loudness,
}

/// EBU R128 measurements of a mono signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Integrated loudness, in LUFS.
    pub integrated_lufs: f64,
    /// True peak, in dBTP.
    pub true_peak_dbtp: f64,
}

/// Trims, joins, resamples and normalises `chunks`, in playback order.
pub fn assemble(chunks: &[Pcm]) -> Result<Assembled> {
    let gap = vec![0.0; (SAMPLE_RATE * GAP_MS / 1000) as usize];
    let mut samples = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let trimmed = trim_silence(chunk);
        if trimmed.is_empty() {
            tracing::warn!(chunk = i, "chunk is silent after trimming");
            continue;
        }
        if !samples.is_empty() {
            samples.extend_from_slice(&gap);
        }
        samples.extend(trimmed.resample(SAMPLE_RATE)?.samples);
    }
    let joined = Pcm::new(SAMPLE_RATE, samples);
    let before = measure(&joined)?;
    let gain = 10f64.powf((TARGET_LUFS - before.integrated_lufs) / 20.0) as f32;
    let pcm = Pcm::new(
        SAMPLE_RATE,
        joined.samples.iter().map(|s| s * gain).collect(),
    );
    let loudness = measure(&pcm)?;
    tracing::info!(
        seconds = pcm.seconds(),
        measured_lufs = before.integrated_lufs,
        integrated_lufs = loudness.integrated_lufs,
        true_peak_dbtp = loudness.true_peak_dbtp,
        "episode assembled"
    );
    Ok(Assembled { pcm, loudness })
}

/// `pcm` without its leading and trailing silence.
fn trim_silence(pcm: &Pcm) -> Pcm {
    let loud = |s: &f32| s.abs() > SILENCE;
    let start = pcm.samples.iter().position(loud);
    let end = pcm.samples.iter().rposition(loud);
    let samples = match (start, end) {
        (Some(start), Some(end)) => pcm.samples[start..=end].to_vec(),
        _ => Vec::new(),
    };
    Pcm::new(pcm.rate, samples)
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

    fn tone(rate: u32, seconds: f32, amplitude: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n)
            .map(|i| amplitude * (i as f32 * 220.0 * std::f32::consts::TAU / rate as f32).sin())
            .collect()
    }

    #[test]
    fn trims_only_the_silent_ends() {
        let mut samples = vec![0.0; 100];
        samples.extend([0.5, 0.0, -0.5]);
        samples.extend(vec![0.001; 50]);
        let trimmed = trim_silence(&Pcm::new(24_000, samples));
        assert_eq!(trimmed.samples, vec![0.5, 0.0, -0.5]);
        assert!(trim_silence(&Pcm::new(24_000, vec![0.0; 10])).is_empty());
    }

    #[test]
    fn joins_with_gaps_at_48_khz_and_normalises_to_the_target() {
        let quiet = Pcm::new(24_000, tone(24_000, 2.0, 0.05));
        let loud = Pcm::new(24_000, tone(24_000, 2.0, 0.6));
        let episode = assemble(&[quiet, loud]).unwrap();

        assert_eq!(episode.pcm.rate, SAMPLE_RATE);
        // Two 2 s chunks (each loses a sample or so at its silent ends) and
        // one 300 ms gap.
        let expected = 2 * 2 * SAMPLE_RATE as usize + 14_400;
        assert!(
            episode.pcm.len().abs_diff(expected) < 10,
            "{} samples",
            episode.pcm.len()
        );
        assert!((episode.loudness.integrated_lufs - TARGET_LUFS).abs() < 0.5);
        // Measured again from the samples, not just reported.
        let again = measure(&episode.pcm).unwrap();
        assert!((again.integrated_lufs - TARGET_LUFS).abs() < 0.5);
        assert!(again.true_peak_dbtp < 0.0);
    }

    #[test]
    fn a_silent_episode_is_an_error() {
        let err = assemble(&[Pcm::new(24_000, vec![0.0; 48_000])]).unwrap_err();
        assert!(err.to_string().contains("assemble"), "{err}");
    }
}
