//! Audio in memory: mono `f32` samples at a known rate, WAV encoding and
//! decoding (`hound`), and resampling (`rubato`).
//!
//! Everything is mono: speech for a podcast gains nothing from stereo, and a
//! TTS model produces one channel anyway. Multi-channel input is averaged
//! down on read.

use std::io::Cursor;
use std::path::Path;

use rubato::audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Resampler};

use crate::error::{CoreError, Result};

/// Mono audio: `samples` at `rate` Hz, nominally in `-1.0..=1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub samples: Vec<f32>,
}

/// How a WAV stores its samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavFormat {
    /// 32-bit IEEE float: lossless for [`Pcm`], used for cached chunks.
    Float32,
    /// 16-bit integer: what players expect from a finished episode.
    Int16,
}

impl Pcm {
    pub fn new(rate: u32, samples: Vec<f32>) -> Self {
        Self { rate, samples }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Length in seconds (0 for a rate of 0).
    pub fn seconds(&self) -> f64 {
        if self.rate == 0 {
            return 0.0;
        }
        self.samples.len() as f64 / f64::from(self.rate)
    }

    /// Encodes as a mono WAV file in memory.
    pub fn to_wav(&self, format: WavFormat) -> Result<Vec<u8>> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.rate,
            bits_per_sample: match format {
                WavFormat::Float32 => 32,
                WavFormat::Int16 => 16,
            },
            sample_format: match format {
                WavFormat::Float32 => hound::SampleFormat::Float,
                WavFormat::Int16 => hound::SampleFormat::Int,
            },
        };
        let mut bytes = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut bytes, spec).map_err(wav_error)?;
        for &sample in &self.samples {
            match format {
                WavFormat::Float32 => writer.write_sample(sample),
                // `as` saturates for floats, so a sample past ±1 clips
                // instead of wrapping around.
                WavFormat::Int16 => {
                    writer.write_sample((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
                }
            }
            .map_err(wav_error)?;
        }
        writer.finalize().map_err(wav_error)?;
        Ok(bytes.into_inner())
    }

    /// Decodes a WAV file held in memory: float or 8/16/24/32-bit integer
    /// samples, any channel count (averaged to mono).
    pub fn from_wav(bytes: &[u8]) -> Result<Self> {
        let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(wav_error)?;
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .collect::<std::result::Result<_, _>>()
                .map_err(wav_error)?,
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1_i64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .into_samples::<i32>()
                    .map(|s| s.map(|s| s as f32 * scale))
                    .collect::<std::result::Result<_, _>>()
                    .map_err(wav_error)?
            }
        };
        let channels = usize::from(spec.channels.max(1));
        let samples = if channels == 1 {
            samples
        } else {
            samples
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                .collect()
        };
        Ok(Self::new(spec.sample_rate, samples))
    }

    /// Reads a WAV file from disk.
    pub fn read_wav(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|err| CoreError::io(path, err))?;
        Self::from_wav(&bytes).map_err(|err| match err {
            CoreError::Source { message, .. } => CoreError::Source {
                path: path.to_owned(),
                message,
            },
            other => other,
        })
    }

    /// Resamples to `rate` Hz. The output length is the input length times
    /// the rate ratio, rounded up; the resampler's own delay is trimmed.
    pub fn resample(&self, rate: u32) -> Result<Self> {
        if rate == self.rate || self.samples.is_empty() {
            return Ok(Self::new(rate, self.samples.clone()));
        }
        let invalid = |message: String| CoreError::Config { message };
        if self.rate == 0 || rate == 0 {
            return Err(invalid(format!(
                "cannot resample {} Hz audio to {rate} Hz",
                self.rate
            )));
        }
        let mut resampler = Fft::<f32>::new(
            self.rate as usize,
            rate as usize,
            RESAMPLE_CHUNK,
            1,
            FixedSync::Input,
        )
        .map_err(|err| invalid(format!("resampler {} Hz → {rate} Hz: {err}", self.rate)))?;
        let input = InterleavedOwned::new_from(self.samples.clone(), 1, self.samples.len())
            .map_err(|err| invalid(format!("resampler input: {err}")))?;
        let output = resampler
            .process_all(&input, self.samples.len(), None)
            .map_err(|err| invalid(format!("resampling {} Hz → {rate} Hz: {err}", self.rate)))?;
        Ok(Self::new(rate, output.take_data()))
    }
}

/// Frames per resampler call; any size works, this one is quick for speech.
const RESAMPLE_CHUNK: usize = 1024;

/// A malformed WAV is bad input data, reported like an unreadable source.
fn wav_error(err: hound::Error) -> CoreError {
    CoreError::Source {
        path: "<wav>".into(),
        message: format!("invalid WAV: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f32) -> Pcm {
        let n = (rate as f32 * seconds) as usize;
        let samples = (0..n)
            .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
            .collect();
        Pcm::new(rate, samples)
    }

    #[test]
    fn float_wav_roundtrips_exactly() {
        let pcm = tone(24_000, 0.5);
        let back = Pcm::from_wav(&pcm.to_wav(WavFormat::Float32).unwrap()).unwrap();
        assert_eq!(back, pcm);
    }

    #[test]
    fn int16_wav_roundtrips_within_one_step_and_clips() {
        let mut pcm = tone(48_000, 0.1);
        pcm.samples.push(3.0);
        pcm.samples.push(-3.0);
        let back = Pcm::from_wav(&pcm.to_wav(WavFormat::Int16).unwrap()).unwrap();
        assert_eq!((back.rate, back.len()), (pcm.rate, pcm.len()));
        for (a, b) in pcm.samples.iter().zip(&back.samples) {
            assert!(
                (a.clamp(-1.0, 1.0) - b).abs() < 1.0 / 16_000.0,
                "{a} vs {b}"
            );
        }
    }

    #[test]
    fn stereo_is_averaged_to_mono() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 8000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut bytes = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
        for s in [0.2_f32, 0.4, -0.5, 0.5] {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
        let pcm = Pcm::from_wav(bytes.get_ref()).unwrap();
        assert_eq!(pcm.samples.len(), 2);
        assert!((pcm.samples[0] - 0.3).abs() < 1e-6 && pcm.samples[1].abs() < 1e-6);
    }

    #[test]
    fn garbage_is_an_error() {
        let err = Pcm::from_wav(b"RIFF....not a wav").unwrap_err().to_string();
        assert!(err.contains("<wav>"), "{err}");
    }

    #[test]
    fn resampling_24k_to_48k_doubles_the_length() {
        for seconds in [0.01, 0.5, 3.0] {
            let pcm = tone(24_000, seconds);
            let up = pcm.resample(48_000).unwrap();
            assert_eq!(up.rate, 48_000);
            assert!(
                up.len().abs_diff(2 * pcm.len()) <= 1,
                "{} → {}",
                pcm.len(),
                up.len()
            );
        }
    }

    #[test]
    fn resampling_keeps_the_signal() {
        let pcm = tone(24_000, 1.0);
        let up = pcm.resample(48_000).unwrap();
        // Same tone at twice the rate: compare away from the edges.
        let expected = tone(48_000, 1.0);
        let worst = (4800..43_200)
            .map(|i| (up.samples[i] - expected.samples[i]).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < 0.01, "max deviation {worst}");
    }
}
