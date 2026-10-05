//! A compressed copy of the episode, made by an external `ffmpeg`.
//!
//! Encoders for Opus and MP3 are C libraries; running the `ffmpeg` the user
//! already has keeps them out of Podling's build and licence. It is started
//! with an argument vector, never a shell, and the episode file can only
//! choose the format: the program is always `ffmpeg` from `PATH`, and the
//! output is always `episode.<format>` in the output directory.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use podling_types::Encode;

use crate::error::{CoreError, ProviderFailure, Result};

/// How long a message from a failed `ffmpeg` may be, from its end.
const STDERR_TAIL: usize = 2_000;

/// What to run and where its output goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoder {
    /// Normally `ffmpeg`; tests point it at a program that isn't there.
    pub program: OsString,
    pub format: Encode,
}

impl Encoder {
    pub fn new(format: Encode) -> Self {
        Self {
            program: "ffmpeg".into(),
            format,
        }
    }

    /// `episode.opus` or `episode.mp3`, in `out_dir`.
    pub fn output_path(&self, out_dir: &Path) -> PathBuf {
        out_dir.join(format!("episode.{}", self.format.extension()))
    }

    /// The arguments: quiet, never prompting or reading stdin, mono, the
    /// codec's settings, and the container named outright (the temporary
    /// output's extension says nothing). Both paths go through ffmpeg's
    /// `file:` protocol, so a path that starts with `-` or looks like a URL
    /// is still just a file.
    pub fn args(&self, input: &Path, output: &Path) -> Vec<OsString> {
        let mut args: Vec<OsString> =
            ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"]
                .into_iter()
                .map(OsString::from)
                .collect();
        args.push(file_url(input));
        let codec: &[&str] = match self.format {
            Encode::Opus => &["-ac", "1", "-c:a", "libopus", "-b:a", "64k", "-f", "opus"],
            Encode::Mp3 => &["-ac", "1", "-c:a", "libmp3lame", "-q:a", "4", "-f", "mp3"],
        };
        args.extend(codec.iter().map(OsString::from));
        args.push(file_url(output));
        args
    }

    /// Encodes `wav` into [`Self::output_path`]. ffmpeg writes a temporary
    /// file that is renamed into place only when it succeeds, so a failed or
    /// interrupted encode never leaves a half file behind under the real
    /// name.
    pub fn encode(&self, wav: &Path, out_dir: &Path) -> Result<PathBuf> {
        let output = self.output_path(out_dir);
        let partial = out_dir.join(format!(".episode.{}.part", self.format.extension()));
        let run = Command::new(&self.program)
            .args(self.args(wav, &partial))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output();
        let program = self.program.to_string_lossy();
        let run = match run {
            Ok(run) => run,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return Err(CoreError::Config {
                    message: format!(
                        "[mix] encode = \"{}\" needs `{program}` on PATH, and it isn't there; \
                         install ffmpeg or remove `encode` (episode.wav is written either way)",
                        self.format.extension()
                    ),
                });
            }
            Err(err) => return Err(CoreError::io(Path::new(&*program), err)),
        };
        if !run.status.success() {
            let _ = fs::remove_file(&partial);
            let stderr = String::from_utf8_lossy(&run.stderr);
            let tail =
                &stderr[stderr.floor_char_boundary(stderr.len().saturating_sub(STDERR_TAIL))..];
            return Err(CoreError::Provider {
                plugin: "ffmpeg".into(),
                kind: ProviderFailure::Other,
                message: format!(
                    "encoding {} failed ({}): {}",
                    output.display(),
                    run.status,
                    tail.trim()
                ),
            });
        }
        fs::rename(&partial, &output).map_err(|err| CoreError::io(&output, err))?;
        Ok(output)
    }
}

fn file_url(path: &Path) -> OsString {
    let mut url = OsString::from("file:");
    url.push(path);
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arguments_are_fixed_but_for_the_paths() {
        let encoder = Encoder::new(Encode::Opus);
        let args = encoder.args(Path::new("-x/episode.wav"), Path::new("out/e.part"));
        let args: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(
            args,
            [
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-y",
                "-i",
                "file:-x/episode.wav",
                "-ac",
                "1",
                "-c:a",
                "libopus",
                "-b:a",
                "64k",
                "-f",
                "opus",
                "file:out/e.part",
            ]
        );
        let mp3 = Encoder::new(Encode::Mp3);
        let args = mp3.args(Path::new("a.wav"), Path::new("b"));
        assert!(args.iter().any(|a| a == "libmp3lame"));
        assert_eq!(
            mp3.output_path(Path::new("out")),
            Path::new("out/episode.mp3")
        );
    }

    #[test]
    fn a_missing_ffmpeg_is_a_readable_error() {
        let dir = tempfile::tempdir().unwrap();
        let encoder = Encoder {
            program: "podling-no-such-ffmpeg".into(),
            format: Encode::Opus,
        };
        let err = encoder
            .encode(&dir.path().join("episode.wav"), dir.path())
            .unwrap_err();
        let CoreError::Config { message } = err else {
            panic!("expected a Config error, got {err:?}");
        };
        assert!(
            message.contains("needs `podling-no-such-ffmpeg` on PATH"),
            "{message}"
        );
        assert!(!encoder.output_path(dir.path()).exists());
    }

    #[test]
    fn a_failing_ffmpeg_leaves_no_file_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        // `false` exists everywhere a shell does, takes any arguments and
        // fails.
        let encoder = Encoder {
            program: "false".into(),
            format: Encode::Mp3,
        };
        let err = encoder
            .encode(&dir.path().join("episode.wav"), dir.path())
            .unwrap_err();
        assert!(err.to_string().contains("ffmpeg"), "{err}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// Runs the real ffmpeg when it is installed.
    #[test]
    #[ignore = "needs ffmpeg on PATH"]
    fn ffmpeg_encodes_a_wav() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("episode.wav");
        let tone: Vec<f32> = (0..48_000).map(|i| 0.3 * (i as f32 * 0.05).sin()).collect();
        let bytes = crate::audio::Pcm::new(48_000, tone)
            .to_wav(crate::audio::WavFormat::Int16)
            .unwrap();
        fs::write(&wav, bytes).unwrap();
        for format in [Encode::Opus, Encode::Mp3] {
            let out = Encoder::new(format).encode(&wav, dir.path()).unwrap();
            assert!(
                fs::metadata(&out).unwrap().len() > 1000,
                "{}",
                out.display()
            );
        }
    }
}
