//! `SidecarTts`: a [`TtsProvider`] backed by a model worker speaking the
//! sidecar protocol (`sidecars/tts`, protocol v1) over loopback HTTP.
//!
//! Audio never travels in HTTP bodies. Podling copies the voice clips into
//! the worker's run directory (the worker refuses paths outside it), names an
//! output file there, and reads the WAV the worker wrote.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use podling_types::{ContentHash, Emotion, Nonverbal, SpeakerId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::http::{Transport, TransportConfig};
use super::sidecar::{PROTOCOL, STARTUP_TIMEOUT, Sidecar, SidecarProfile};
use super::tts::{ChunkAudio, ChunkRequest, SpokenTurn, TtsCapabilities, TtsProvider};
use crate::audio::Pcm;
use crate::error::{CoreError, ProviderFailure, Result};

const API: &str = "/v1/podling";

/// A synthesis call can include loading the model and generating two
/// minutes of speech.
const REQUEST_TIMEOUT_SECS: u64 = 900;

const STAGE: &str = "synthesize";

#[derive(Debug, Deserialize)]
struct Health {
    protocol: u32,
    backend: String,
    model: String,
    weights: String,
    /// The worker adapter's version: how it turns a request into model calls.
    /// A worker from before the field existed reports none and reads as 0.
    #[serde(default)]
    adapter: u32,
    capabilities: WireCapabilities,
}

#[derive(Debug, Deserialize)]
struct WireCapabilities {
    multi_speaker: bool,
    max_chunk_secs: u32,
    max_speakers: u8,
    native_sample_rate: u32,
    /// Whether the model listens to context. A worker that doesn't say is
    /// taken not to.
    #[serde(default)]
    context: bool,
}

#[derive(Serialize)]
struct WireTurn<'a> {
    speaker: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    emotion: Option<Emotion>,
    /// Already in protocol v1's shape: `{kind, by, at, text?}`.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    nonverbal: &'a [Nonverbal],
}

#[derive(Serialize)]
struct WireVoice<'a> {
    reference: &'a Path,
    transcript: &'a str,
}

#[derive(Serialize)]
struct WireContext<'a> {
    turns: Vec<WireTurn<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio: Option<PathBuf>,
    callbacks: Vec<PathBuf>,
}

#[derive(Serialize)]
struct WireRequest<'a> {
    turns: Vec<WireTurn<'a>>,
    voices: BTreeMap<&'a str, WireVoice<'a>>,
    seed: u64,
    out_path: &'a Path,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<WireContext<'a>>,
}

#[derive(Debug, Deserialize)]
struct WireReply {
    sample_rate: u32,
    samples: usize,
    turn_spans: Option<Vec<(usize, usize)>>,
    #[serde(default)]
    dropped: Vec<Value>,
    /// Sounds rendered as clips of their own. Podling only sends sounds the
    /// speaker makes in line, so a worker has no reason to return any.
    #[serde(default)]
    clips: Vec<Value>,
}

/// A TTS provider that owns a running worker.
///
/// Fields are dropped in declaration order, so `sidecar` (whose `Drop`
/// stops the process) goes before `_run_dir` (whose `Drop` deletes the
/// directory the process was writing into).
#[derive(Debug)]
pub struct SidecarTts {
    sidecar: Sidecar,
    /// Held only for its `Drop`, which deletes the directory.
    _run_dir: TempDir,
    /// `run_dir` with symlinks resolved: the worker's own view of it.
    root: PathBuf,
    transport: Transport,
    health: Health,
    capabilities: TtsCapabilities,
    /// Files already copied into the run directory, by source path.
    staged: BTreeMap<PathBuf, PathBuf>,
    calls: u64,
}

impl SidecarTts {
    /// Starts profile `name` and checks its `/health`. `profiles` is the file
    /// the profile came from, for messages.
    pub fn start(name: &str, profile: &SidecarProfile, profiles: &Path) -> Result<Self> {
        Self::start_with_timeout(name, profile, profiles, STARTUP_TIMEOUT)
    }

    /// [`SidecarTts::start`] with another startup timeout (tests use short ones).
    pub fn start_with_timeout(
        name: &str,
        profile: &SidecarProfile,
        profiles: &Path,
        timeout: Duration,
    ) -> Result<Self> {
        let run_dir = tempfile::Builder::new()
            .prefix("podling-tts-")
            .tempdir()
            .map_err(|err| CoreError::io(std::env::temp_dir(), err))?;
        // Resolve symlinks (e.g. a /tmp link), so the paths we send match the
        // worker's own view of its run directory.
        let root = run_dir
            .path()
            .canonicalize()
            .map_err(|err| CoreError::io(run_dir.path(), err))?;
        for sub in ["voices", "out"] {
            let dir = root.join(sub);
            std::fs::create_dir(&dir).map_err(|err| CoreError::io(&dir, err))?;
        }
        let mut sidecar = Sidecar::spawn(name, profile, profiles, &root, timeout)?;

        let base_url = format!("{}{API}", sidecar.base_url());
        let transport = Transport::new(
            &TransportConfig {
                section: "tts",
                plugin: "sidecar_tts",
                base_url: &base_url,
                api_key_env: None,
                timeout_secs: Some(REQUEST_TIMEOUT_SECS),
                // No policy: the worker is always on this machine, and a
                // hosted URL here is refused.
                data_policy: None,
            },
            |_| None,
        )?;
        let health = match transport.get_json("/health") {
            Ok(reply) => parse_health(&reply.body)?,
            Err(err) => return Err(sidecar.failure(&format!("did not answer /health: {err}"))),
        };
        let wire = &health.capabilities;
        let capabilities = TtsCapabilities {
            multi_speaker: wire.multi_speaker,
            max_chunk_secs: wire.max_chunk_secs,
            max_speakers: wire.max_speakers,
            native_sample_rate: wire.native_sample_rate,
            context: wire.context,
        };
        tracing::info!(
            sidecar = name,
            backend = %health.backend,
            model = %health.model,
            weights = %health.weights,
            adapter = health.adapter,
            "TTS sidecar healthy"
        );
        Ok(Self {
            sidecar,
            _run_dir: run_dir,
            root,
            transport,
            health,
            capabilities,
            staged: BTreeMap::new(),
            calls: 0,
        })
    }

    /// The worker's process id, for checks that it has gone.
    pub fn pid(&self) -> u32 {
        self.sidecar.pid()
    }

    /// The directory the worker may read and write.
    pub fn run_dir(&self) -> &Path {
        &self.root
    }

    /// Copies `source` into the run directory, once, under a name taken from
    /// its content, and returns the copy's path.
    fn stage(&mut self, source: &Path) -> Result<PathBuf> {
        if let Some(copy) = self.staged.get(source) {
            return Ok(copy.clone());
        }
        let bytes = std::fs::read(source).map_err(|err| CoreError::io(source, err))?;
        let name = format!("{}.wav", ContentHash::of_parts(&[&bytes]).as_str());
        let copy = self.root.join("voices").join(name);
        std::fs::write(&copy, &bytes).map_err(|err| CoreError::io(&copy, err))?;
        self.staged.insert(source.to_owned(), copy.clone());
        Ok(copy)
    }

    /// Adds a hint to a failed request: a dead worker's log, or advice for
    /// a GPU that is out of memory.
    fn explain(&mut self, err: CoreError) -> CoreError {
        let CoreError::Provider {
            plugin,
            kind,
            message,
        } = err
        else {
            return err;
        };
        let wait = match kind {
            ProviderFailure::Unreachable => Duration::from_millis(500),
            _ => Duration::ZERO,
        };
        if self.sidecar.exits_within(wait) {
            return self
                .sidecar
                .failure(&format!("died during a request: {message}"));
        }
        let message = match kind {
            ProviderFailure::Http(500 | 503) => {
                format!("{message}. {}", self.sidecar.hint(&message))
            }
            _ => message,
        };
        CoreError::Provider {
            plugin,
            kind,
            message,
        }
    }

    fn invalid(message: String) -> CoreError {
        CoreError::InvalidProviderOutput {
            stage: STAGE,
            message,
        }
    }
}

fn parse_health(body: &str) -> Result<Health> {
    let health: Health = serde_json::from_str(body).map_err(|err| {
        SidecarTts::invalid(format!("the sidecar's /health reply is malformed: {err}"))
    })?;
    if health.protocol != PROTOCOL {
        return Err(SidecarTts::invalid(format!(
            "the sidecar speaks protocol {}, but this Podling speaks {PROTOCOL}",
            health.protocol
        )));
    }
    let caps = &health.capabilities;
    if caps.max_chunk_secs == 0 || caps.max_speakers == 0 || caps.native_sample_rate == 0 {
        return Err(SidecarTts::invalid(format!(
            "the sidecar reports impossible capabilities: {caps:?}"
        )));
    }
    Ok(health)
}

fn wire_turns(turns: &[SpokenTurn]) -> Vec<WireTurn<'_>> {
    turns
        .iter()
        .map(|turn| WireTurn {
            speaker: &turn.speaker.0,
            // The model says the respelt names; nothing else sees them.
            text: turn.say_as.as_deref().unwrap_or(&turn.text),
            // Neutral is the default delivery; only a real hint is sent.
            emotion: (turn.emotion != Emotion::Neutral).then_some(turn.emotion),
            nonverbal: &turn.nonverbal,
        })
        .collect()
}

impl TtsProvider for SidecarTts {
    fn id(&self) -> &str {
        "sidecar"
    }

    fn fingerprint(&self) -> Value {
        fingerprint(&self.health)
    }

    fn capabilities(&self) -> &TtsCapabilities {
        &self.capabilities
    }

    fn synthesize(&mut self, request: &ChunkRequest<'_>) -> Result<ChunkAudio> {
        let started = Instant::now();
        // Only the voices this chunk (and its context) uses, sounds included.
        let context_turns = request.context.map(|c| c.turns).unwrap_or_default();
        let speakers = request.turns.iter().chain(context_turns).flat_map(|turn| {
            std::iter::once(&turn.speaker).chain(turn.nonverbal.iter().map(|n| &n.by))
        });
        let mut voices = BTreeMap::new();
        for speaker in speakers {
            if voices.contains_key(speaker.0.as_str()) {
                continue;
            }
            let voice = request
                .voices
                .get(speaker)
                .ok_or_else(|| missing_voice(speaker))?;
            voices.insert(speaker.0.as_str(), (self.stage(voice.reference())?, voice));
        }
        let context = match request.context {
            None => None,
            Some(context) => Some(WireContext {
                turns: wire_turns(context.turns),
                audio: context.audio.map(|p| self.stage(p)).transpose()?,
                callbacks: context
                    .callbacks
                    .iter()
                    .map(|p| self.stage(p))
                    .collect::<Result<_>>()?,
            }),
        };

        self.calls += 1;
        let out_path = self
            .root
            .join("out")
            .join(format!("chunk-{}.wav", self.calls));
        let body = serde_json::to_vec(&WireRequest {
            turns: wire_turns(request.turns),
            voices: voices
                .iter()
                .map(|(id, (copy, voice))| {
                    (
                        *id,
                        WireVoice {
                            reference: copy,
                            transcript: voice.transcript(),
                        },
                    )
                })
                .collect(),
            seed: request.seed,
            out_path: &out_path,
            context,
        })?;

        let reply = match self.transport.post_json("/synthesize", &body) {
            Ok(reply) => reply,
            Err(err) => return Err(self.explain(err)),
        };
        let reply: WireReply = serde_json::from_str(&reply.body).map_err(|err| {
            Self::invalid(format!(
                "the sidecar's /synthesize reply is malformed: {err}"
            ))
        })?;
        let pcm = Pcm::read_wav(&out_path);
        // The file was only a way to move the audio; it is in memory now.
        let _ = std::fs::remove_file(&out_path);
        let pcm = pcm?;
        if pcm.rate != reply.sample_rate || pcm.len() != reply.samples {
            return Err(Self::invalid(format!(
                "the sidecar reported {} samples at {} Hz but wrote {} at {} Hz",
                reply.samples,
                reply.sample_rate,
                pcm.len(),
                pcm.rate
            )));
        }
        if !reply.dropped.is_empty() {
            tracing::info!(
                dropped = %serde_json::Value::Array(reply.dropped),
                "the TTS backend cannot express these; they were left out"
            );
        }
        if !reply.clips.is_empty() {
            tracing::warn!(
                clips = %serde_json::Value::Array(reply.clips),
                "the TTS backend rendered sounds as separate clips, which are not placed yet"
            );
        }
        let seconds = pcm.seconds();
        let elapsed = started.elapsed().as_secs_f64();
        tracing::info!(
            seconds = format!("{seconds:.1}"),
            elapsed_ms = (elapsed * 1000.0) as u64,
            rtf = format!("{:.2}", elapsed / seconds.max(f64::EPSILON)),
            "synthesised a chunk"
        );
        Ok(ChunkAudio {
            pcm,
            turn_spans: reply
                .turn_spans
                .map(|spans| spans.into_iter().map(|(start, end)| start..end).collect()),
        })
    }
}

fn missing_voice(speaker: &SpeakerId) -> CoreError {
    CoreError::Config {
        message: format!(
            "speaker {:?} has no voice: add it to [[cast]] with a reference clip",
            speaker.0
        ),
    }
}

/// Protocol, backend, model, weights snapshot and adapter version, all from
/// `/health`. The profile name is left out: renaming a profile changes no audio.
fn fingerprint(health: &Health) -> Value {
    json!({
        "id": "sidecar",
        "protocol": health.protocol,
        "backend": health.backend,
        "model": health.model,
        "weights": health.weights,
        "adapter": health.adapter,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health(protocol: u32, rate: u32) -> String {
        json!({
            "protocol": protocol, "backend": "fake", "model": "m", "weights": "abc",
            "loaded": false,
            "capabilities": { "multi_speaker": false, "max_chunk_secs": 120,
                              "max_speakers": 8, "native_sample_rate": rate },
        })
        .to_string()
    }

    #[test]
    fn the_adapter_version_is_part_of_the_fingerprint() {
        let mut health: Value = serde_json::from_str(&health(1, 24_000)).unwrap();
        let old = fingerprint(&parse_health(&health.to_string()).unwrap());
        assert_eq!(
            old["adapter"], 0,
            "a worker that reports no adapter reads as 0"
        );
        health["adapter"] = json!(1);
        let new = fingerprint(&parse_health(&health.to_string()).unwrap());
        assert_eq!(new["adapter"], 1);
        assert_ne!(old, new);
    }

    #[test]
    fn health_must_match_our_protocol() {
        parse_health(&health(1, 24_000)).unwrap();
        let err = parse_health(&health(2, 24_000)).unwrap_err().to_string();
        assert!(err.contains("invalid output"), "{err}");
        assert!(parse_health(&health(1, 0)).is_err());
        assert!(parse_health("{}").is_err());
    }

    #[test]
    fn neutral_emotion_and_no_sounds_are_not_sent() {
        let a = SpeakerId("a".into());
        let mut wow = SpokenTurn::plain(a.clone(), "wow", Emotion::Excited);
        wow.nonverbal.push(Nonverbal {
            kind: podling_types::NonverbalKind::Laugh {},
            by: a.clone(),
            at: podling_types::NonverbalAt::After,
        });
        let turns = [SpokenTurn::plain(a, "hi", Emotion::Neutral), wow];
        let wire = serde_json::to_value(wire_turns(&turns)).unwrap();
        assert_eq!(
            wire,
            json!([
                { "speaker": "a", "text": "hi" },
                { "speaker": "a", "text": "wow", "emotion": "excited",
                  "nonverbal": [{ "kind": "laugh", "by": "a", "at": "after" }] },
            ])
        );
    }

    #[test]
    fn a_worker_that_does_not_mention_context_does_not_get_it() {
        assert!(
            !parse_health(&health(1, 24_000))
                .unwrap()
                .capabilities
                .context
        );
    }
}
