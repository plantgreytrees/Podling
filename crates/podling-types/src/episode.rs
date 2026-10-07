//! The episode specification: the TOML file a user writes to request an episode.
//!
//! Contains no secrets. Plugins that need credentials will take the *name* of
//! an environment variable, never the value.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::claim::PerMille;
use crate::ids::SpeakerId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EpisodeSpec {
    pub title: String,
    pub topic: String,
    #[serde(default)]
    pub mode: Mode,
    pub target_minutes: u16,
    pub sources: Vec<SourceSpec>,
    pub llm: LlmConfig,
    /// Embeds claims and source sentences so the NLI stages can pick which
    /// pairs to compare. Needs `nli` too; without both, claims merge only by
    /// exact wording and every piece of evidence is the claim's own extraction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<EmbeddingConfig>,
    /// Judges whether one text entails or contradicts another. Needs
    /// `embedding` too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nli: Option<NliConfig>,
    #[serde(default)]
    pub analysers: Vec<AnalyserConfig>,
    /// The speakers, each with a pinned voice. Required by `[tts]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cast: Vec<CastMember>,
    /// Turns the script into audio. Needs `[[cast]]` and `[asr]` too; without
    /// it, the run stops at the script.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tts: Option<TtsConfig>,
    /// Transcribes each synthesised chunk to check it against the script.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asr: Option<AsrConfig>,
    /// Gaps between turns and an optional compressed copy. Needs `[tts]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mix: Option<MixConfig>,
}

/// What kind of episode to make. Only non-fiction exists today; fiction modes
/// are planned, so matches on this enum must stay open-ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Mode {
    #[default]
    NonFiction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceSpec {
    /// Every `.md` / `.txt` file directly under `root`. A relative `root` is
    /// resolved against the episode file's directory.
    LocalFiles {
        root: PathBuf,
        independence_group: String,
    },
}

// Variants are written `Name {}` rather than `Name`: serde only enforces
// `deny_unknown_fields` on struct variants of an internally tagged enum, so a
// unit variant would silently accept stray keys.

// Not `Eq`: `temperature` is an `f32`, which has no total equality (NaN).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LlmConfig {
    /// Deterministic offline stand-in, for development and tests.
    Fake {},
    /// Any server that speaks the OpenAI chat-completions protocol: Ollama,
    /// llama.cpp `llama-server`, vLLM, LM Studio, OpenAI itself.
    ///
    /// There is deliberately no field for the key itself, only the name of the
    /// environment variable that holds it, so an episode file stays free of
    /// secrets and `deny_unknown_fields` rejects a pasted `api_key = "..."`.
    OpenAiCompat {
        /// Base URL up to and including the version segment, e.g.
        /// `http://localhost:11434/v1`. Must be `http://` or `https://`.
        base_url: String,
        /// Model name as the server knows it, e.g. `llama3.1:8b`.
        model: String,
        /// Name of the environment variable holding the API key. Leave unset
        /// for local servers that need none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key_env: Option<String>,
        /// Sampling temperature; the server's default when unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temperature: Option<f32>,
        /// Per-request timeout in seconds; the provider's default when unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_secs: Option<u64>,
        /// Cap on generated tokens per request; the server's default when unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_output_tokens: Option<u32>,
        /// Ask Ollama to unload the model once the script is written, so the
        /// GPU is free for text-to-speech. Ollama only: `base_url` must end
        /// in `/v1`.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        unload_after: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmbeddingConfig {
    /// Deterministic offline stand-in: a hashed bag of words.
    Fake {},
    /// Any server that speaks the OpenAI embeddings protocol
    /// (`POST {base_url}/embeddings`), e.g. Ollama with `nomic-embed-text`.
    OpenAiCompat {
        /// Base URL up to and including the version segment, e.g.
        /// `http://localhost:11434/v1`. Must be `http://` or `https://`.
        base_url: String,
        /// Model name as the server knows it, e.g. `nomic-embed-text`.
        model: String,
        /// Name of the environment variable holding the API key, never the
        /// key itself. Leave unset for local servers that need none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key_env: Option<String>,
        /// Per-request timeout in seconds; the provider's default when unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_secs: Option<u64>,
        /// Ask Ollama to unload the model once grounding is done, so the GPU
        /// is free for the next model. Ollama only: `base_url` must end in
        /// `/v1`.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        unload_after: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NliConfig {
    /// Deterministic offline stand-in: word containment, with a changed
    /// number counted as a contradiction.
    Fake {},
    /// A local DeBERTa-v3 NLI cross-encoder, run on the CPU, e.g.
    /// `cross-encoder/nli-deberta-v3-base`.
    CrossEncoder {
        /// Directory holding the model's `config.json`, `tokenizer.json` and
        /// `model.safetensors`. A relative path is resolved against the
        /// episode file's directory.
        model_dir: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnalyserConfig {
    /// Checks every quote against its source, word for word.
    QuoteVerifier {},
    /// Warns on a turn that states a number or a year but cites no claim:
    /// the likeliest place for banter to slip in an unsourced fact.
    UncitedFigures {},
}

/// One speaker in `[[cast]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CastMember {
    pub id: SpeakerId,
    pub name: String,
    /// Free-form role, e.g. `host` or `narrator`.
    pub role: String,
    pub voice: VoiceRef,
}

/// A reference clip that pins a speaker's voice; every chunk is conditioned
/// on it.
///
/// The licence is required because voice clips are where non-commercial
/// terms sneak in, and it is copied into the audio manifest. Deserialisation
/// runs the same checks as [`VoiceRef::new`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawVoiceRef")]
pub struct VoiceRef {
    reference: PathBuf,
    transcript: String,
    licence: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawVoiceRef {
    /// The clip (WAV). A relative path is resolved against the episode
    /// file's directory.
    reference: PathBuf,
    /// Exactly what is said in the clip; voice-cloning models need it.
    transcript: String,
    /// SPDX identifier of the clip's licence, e.g. `CC0-1.0` or `CC-BY-4.0`.
    licence: String,
}

/// The licence of a voice clip made by the user's own voice-design tool
/// (`scripts/voice_design/`) rather than recorded from a person. Such a clip
/// is accepted only with a provenance file beside it ([`provenance_path`]).
pub const GENERATED_VOICE_LICENCE: &str = "LicenseRef-Podling-Generated";

/// The SPDX ids a voice clip may carry: public domain or attribution only, so
/// a cloned voice never brings non-commercial or share-alike terms into an
/// episode, or the user's own [`GENERATED_VOICE_LICENCE`]. Matched exactly,
/// so the error can name the id to write.
pub const VOICE_LICENCES: [&str; 4] =
    ["CC0-1.0", "CC-BY-3.0", "CC-BY-4.0", GENERATED_VOICE_LICENCE];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VoiceRefError {
    #[error("voice {0:?} has no licence: name the clip's licence, e.g. \"CC0-1.0\"")]
    MissingLicence(PathBuf),
    #[error(
        "voice {reference:?} has licence {licence:?}, which is not allowed: \
         use a clip under one of {VOICE_LICENCES:?} (non-commercial and unknown \
         licences are refused)"
    )]
    LicenceNotAllowed { reference: PathBuf, licence: String },
    #[error("voice {0:?} has no transcript of its reference clip")]
    MissingTranscript(PathBuf),
}

impl VoiceRef {
    pub fn new(
        reference: impl Into<PathBuf>,
        transcript: impl Into<String>,
        licence: impl Into<String>,
    ) -> Result<Self, VoiceRefError> {
        let (reference, transcript, licence) =
            (reference.into(), transcript.into(), licence.into());
        if licence.trim().is_empty() {
            return Err(VoiceRefError::MissingLicence(reference));
        }
        if !VOICE_LICENCES.contains(&licence.trim()) {
            return Err(VoiceRefError::LicenceNotAllowed { reference, licence });
        }
        if transcript.trim().is_empty() {
            return Err(VoiceRefError::MissingTranscript(reference));
        }
        Ok(Self {
            reference,
            transcript,
            licence,
        })
    }

    pub fn reference(&self) -> &Path {
        &self.reference
    }

    pub fn transcript(&self) -> &str {
        &self.transcript
    }

    pub fn licence(&self) -> &str {
        &self.licence
    }
}

impl TryFrom<RawVoiceRef> for VoiceRef {
    type Error = VoiceRefError;

    fn try_from(raw: RawVoiceRef) -> Result<Self, Self::Error> {
        Self::new(raw.reference, raw.transcript, raw.licence)
    }
}

/// Where the provenance of a generated voice clip lives: beside the clip,
/// its whole file name followed by `.provenance.json`
/// (`voices/host.wav` → `voices/host.wav.provenance.json`).
pub fn provenance_path(clip: &Path) -> PathBuf {
    let mut name = clip.as_os_str().to_owned();
    name.push(".provenance.json");
    PathBuf::from(name)
}

/// How a [`GENERATED_VOICE_LICENCE`] clip was made, written by the
/// voice-design tool next to the clip. Enough to make the clip again, and to
/// show it was designed rather than copied from a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawVoiceProvenance")]
pub struct VoiceProvenance {
    model: String,
    weights_commit: String,
    design_prompt: String,
    seed: u64,
    tool_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVoiceProvenance {
    /// The voice-design model, e.g. `Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign`.
    model: String,
    /// The model weights' revision (a Hugging Face commit).
    weights_commit: String,
    /// The description the voice was designed from.
    design_prompt: String,
    seed: u64,
    /// The version of the tool that wrote the clip.
    tool_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("voice provenance has an empty {0:?}")]
pub struct EmptyProvenanceField(&'static str);

impl VoiceProvenance {
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn weights_commit(&self) -> &str {
        &self.weights_commit
    }

    pub fn design_prompt(&self) -> &str {
        &self.design_prompt
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn tool_version(&self) -> &str {
        &self.tool_version
    }
}

impl TryFrom<RawVoiceProvenance> for VoiceProvenance {
    type Error = EmptyProvenanceField;

    fn try_from(raw: RawVoiceProvenance) -> Result<Self, Self::Error> {
        let fields = [
            ("model", &raw.model),
            ("weights_commit", &raw.weights_commit),
            ("design_prompt", &raw.design_prompt),
            ("tool_version", &raw.tool_version),
        ];
        if let Some((name, _)) = fields.iter().find(|(_, value)| value.trim().is_empty()) {
            return Err(EmptyProvenanceField(name));
        }
        Ok(Self {
            model: raw.model,
            weights_commit: raw.weights_commit,
            design_prompt: raw.design_prompt,
            seed: raw.seed,
            tool_version: raw.tool_version,
        })
    }
}

/// How one name is said: the respelling the TTS model is given instead of
/// the name, and what speech recognition may write when it hears it.
///
/// Respell names the model gets wrong, never common words (respelling one
/// made it worse in the word test). A respelling is one unhyphenated word
/// per word of the name: a hyphen splits the word when spoken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawPronunciation")]
pub struct Pronunciation {
    say: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    heard: Vec<String>,
}

/// An entry is either just the respelling, `Kulik = "Koolick"`, or a table
/// that also lists what speech recognition writes for the name,
/// `Kulik = { say = "Koolick", heard = ["Koolik"] }`.
///
/// `Deserialize` is written by hand (below) rather than `untagged`, so a
/// table's own error, such as ``missing field `say` ``, reaches the user
/// instead of "did not match any variant". The schema stays untagged.
#[derive(JsonSchema)]
#[schemars(untagged)]
enum RawPronunciation {
    /// The respelling only.
    Say(String),
    Full(PronunciationTable),
}

impl<'de> Deserialize<'de> for RawPronunciation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Shape;

        impl<'de> serde::de::Visitor<'de> for Shape {
            type Value = RawPronunciation;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a respelling string or a table with `say` and `heard`")
            }

            fn visit_str<E: serde::de::Error>(self, say: &str) -> Result<Self::Value, E> {
                Ok(RawPronunciation::Say(say.to_owned()))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                let table = PronunciationTable::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(RawPronunciation::Full(table))
            }
        }

        deserializer.deserialize_any(Shape)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PronunciationTable {
    /// The respelling the TTS model is given.
    say: String,
    /// What speech recognition may write for the name; each is read as the
    /// name when the audio is checked.
    #[serde(default)]
    heard: Vec<String>,
}

/// A pronunciation entry with nothing in it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LexiconError {
    #[error("a pronunciation entry has an empty name")]
    EmptyName,
    /// A padded name would never match a whole word, so it is refused rather
    /// than trimmed: the padding is usually a typo worth seeing.
    #[error("pronunciation name {0:?} has leading or trailing whitespace")]
    PaddedName(String),
    #[error("a pronunciation has an empty `say`")]
    EmptySay,
    #[error("a pronunciation has an empty `heard` entry")]
    EmptyHeard,
}

impl Pronunciation {
    pub fn new(say: impl Into<String>, heard: Vec<String>) -> Result<Self, LexiconError> {
        let say = say.into();
        if say.trim().is_empty() {
            return Err(LexiconError::EmptySay);
        }
        if heard.iter().any(|h| h.trim().is_empty()) {
            return Err(LexiconError::EmptyHeard);
        }
        Ok(Self { say, heard })
    }

    /// What the TTS model is given in place of the name.
    pub fn say(&self) -> &str {
        &self.say
    }

    /// What speech recognition may write for the name.
    pub fn heard(&self) -> &[String] {
        &self.heard
    }
}

impl TryFrom<RawPronunciation> for Pronunciation {
    type Error = LexiconError;

    fn try_from(raw: RawPronunciation) -> Result<Self, Self::Error> {
        match raw {
            RawPronunciation::Say(say) => Self::new(say, Vec::new()),
            RawPronunciation::Full(table) => Self::new(table.say, table.heard),
        }
    }
}

/// Names and how to say them, e.g. `[tts.pronounce]` in an episode.
/// Names are matched as whole words, case-sensitively.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "BTreeMap<String, Pronunciation>")]
pub struct Lexicon(BTreeMap<String, Pronunciation>);

impl Lexicon {
    pub fn new(entries: BTreeMap<String, Pronunciation>) -> Result<Self, LexiconError> {
        for name in entries.keys() {
            if name.trim().is_empty() {
                return Err(LexiconError::EmptyName);
            }
            if name != name.trim() {
                return Err(LexiconError::PaddedName(name.clone()));
            }
        }
        Ok(Self(entries))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, name: &str) -> Option<&Pronunciation> {
        self.0.get(name)
    }

    /// Every entry, by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Pronunciation)> {
        self.0.iter().map(|(name, p)| (name.as_str(), p))
    }

    /// `self` with every entry of `over` added, `over` winning per name.
    pub fn overlaid(&self, over: &Lexicon) -> Lexicon {
        let mut entries = self.0.clone();
        entries.extend(over.0.iter().map(|(n, p)| (n.clone(), p.clone())));
        Self(entries)
    }
}

impl TryFrom<BTreeMap<String, Pronunciation>> for Lexicon {
    type Error = LexiconError;

    fn try_from(entries: BTreeMap<String, Pronunciation>) -> Result<Self, Self::Error> {
        Self::new(entries)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TtsConfig {
    /// Deterministic offline stand-in: a sine tone per speaker.
    Fake {},
    /// A model worker started from a profile in the user-level
    /// `~/.config/podling/sidecars.toml`. The episode names the profile
    /// only, so a shared episode file can never choose a program to run.
    Sidecar {
        /// Profile name in `sidecars.toml`, e.g. `qwen3-tts`.
        sidecar: String,
        /// Takes made of each banter beat; the best passing one wins.
        #[serde(default = "default_takes")]
        takes: u8,
        /// Extra attempts for a chunk that fails verification.
        #[serde(default = "default_max_retries")]
        max_retries: u8,
        /// Names the model mispronounces, each with a respelling, as
        /// `[tts.pronounce]`. Merged over the user-level `pronounce.toml`
        /// beside `sidecars.toml`; this list wins per name. The speech check
        /// and the quotes keep the original words.
        #[serde(default, skip_serializing_if = "Lexicon::is_empty")]
        pronounce: Lexicon,
    },
}

fn default_takes() -> u8 {
    2
}

fn default_max_retries() -> u8 {
    2
}

/// How the synthesised chunks are put together into one episode file.
/// Applies to every TTS backend, so it is a section of its own rather than
/// part of `[tts]` (whose fields depend on its `kind`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixConfig {
    /// Silence before a turn, by the turn's pace.
    #[serde(default)]
    pub gaps_ms: Gaps,
    /// Also writes the episode in a compressed format, through `ffmpeg`
    /// (which must be on `PATH`). `episode.wav` is always written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode: Option<Encode>,
}

/// Milliseconds of silence before a turn, by its pace; `interrupt` is how far
/// an interrupting turn overlaps the one before it. Each key is optional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Gaps {
    #[serde(default = "Gaps::default_quick")]
    pub quick: u16,
    #[serde(default = "Gaps::default_normal")]
    pub normal: u16,
    #[serde(default = "Gaps::default_beat")]
    pub beat: u16,
    #[serde(default = "Gaps::default_long_pause")]
    pub long_pause: u16,
    #[serde(default = "Gaps::default_interrupt")]
    pub interrupt: u16,
}

impl Gaps {
    fn default_quick() -> u16 {
        120
    }
    fn default_normal() -> u16 {
        300
    }
    fn default_beat() -> u16 {
        600
    }
    fn default_long_pause() -> u16 {
        1000
    }
    fn default_interrupt() -> u16 {
        150
    }
}

impl Default for Gaps {
    fn default() -> Self {
        Self {
            quick: Self::default_quick(),
            normal: Self::default_normal(),
            beat: Self::default_beat(),
            long_pause: Self::default_long_pause(),
            interrupt: Self::default_interrupt(),
        }
    }
}

/// A compressed format for the episode, beside `episode.wav`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Encode {
    /// Ogg Opus, 64 kbit/s: small, and what podcast apps play natively.
    Opus,
    /// MP3, VBR around 130 kbit/s: plays everywhere.
    Mp3,
}

impl Encode {
    /// The file extension, which is also ffmpeg's name for the container.
    pub fn extension(self) -> &'static str {
        match self {
            Encode::Opus => "opus",
            Encode::Mp3 => "mp3",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AsrConfig {
    /// Deterministic offline stand-in that hears exactly what was meant.
    Fake {},
    /// Whisper run on the CPU, e.g. `openai/whisper-base.en`.
    Whisper {
        /// Directory holding the model's `config.json`, `tokenizer.json` and
        /// `model.safetensors`. A relative path is resolved against the
        /// episode file's directory.
        model_dir: PathBuf,
        /// A chunk passes when its word error rate is at most this, in
        /// thousandths (80 = 8%).
        #[serde(default = "default_max_wer_pm")]
        max_wer_pm: PerMille,
    },
}

fn default_max_wer_pm() -> PerMille {
    PerMille::new(80).expect("80 is within 0..=1000")
}
