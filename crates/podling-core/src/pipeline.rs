//! Runs an episode end to end: sources → documents → chunks → claims →
//! (grounding, clusters and stances, when `[embedding]` and `[nli]` are set) → ledger →
//! script → analysis → (audio, when `[tts]` is set), writing each artifact to the
//! output directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use podling_types::{
    ArtifactKind, AsrConfig, AudioManifest, Document, DocumentId, Envelope, EpisodeAudio,
    EpisodeSpec, Finding, PerMille, Script, SourceRef, Speaker, TtsConfig,
};
use serde::Serialize;

use crate::audio::{Pcm, WavFormat};
use crate::cache::{BlobStore, DiskCache};
use crate::error::{CoreError, Result};
use crate::plugin::{
    AsrProvider, LlmProvider, build_analysers, build_asr, build_grounding, build_llm,
    build_sources, build_tts, check_audio, default_profiles_path, load_profile,
};
use crate::stage::{GroundingCounts, RunReport, cached};
use crate::stages::assemble::{SAMPLE_RATE, assemble};
use crate::stages::{
    Analyse, AnalyseInput, BuildLedger, ChunkDocuments, ClaimInput, ClusterClaims, ExtractClaims,
    GroundClaims, GroundInput, Ingest, ScoreStances, ScriptInput, StanceInput, Takes, Verification,
    Voices, WriteScript, synthesize_script, verify_audio,
};

/// The episode's audio file, in the output directory.
pub const EPISODE_WAV: &str = "episode.wav";

/// Runs `spec`. Relative source paths resolve against `base_dir` (normally
/// the episode file's directory). With `cache` set to `None` every stage runs.
///
/// Sources are always re-read: fetching is cheap, and ingest's cache key
/// covers the documents' text, so an edited source invalidates ingest and
/// everything downstream of it.
///
/// A `[tts] sidecar` profile is looked up in the user-level `sidecars.toml`
/// ([`default_profiles_path`]).
pub fn run(
    spec: &EpisodeSpec,
    base_dir: &Path,
    cache: Option<&DiskCache>,
    out_dir: &Path,
) -> Result<RunReport> {
    let llm = build_llm(&spec.llm)?;
    run_inner(spec, llm.as_ref(), None, None, base_dir, cache, out_dir)
}

/// As [`run`], with sidecar profiles read from `sidecars` instead of the
/// user-level default (the CLI's `--sidecars`).
pub fn run_with_sidecars(
    spec: &EpisodeSpec,
    sidecars: &Path,
    base_dir: &Path,
    cache: Option<&DiskCache>,
    out_dir: &Path,
) -> Result<RunReport> {
    let llm = build_llm(&spec.llm)?;
    run_inner(
        spec,
        llm.as_ref(),
        Some(sidecars),
        None,
        base_dir,
        cache,
        out_dir,
    )
}

/// As [`run`], but with the LLM provider supplied instead of built from
/// `spec.llm`. The seam for tests and for callers that build their own.
pub fn run_with_llm(
    spec: &EpisodeSpec,
    llm: &dyn LlmProvider,
    base_dir: &Path,
    cache: Option<&DiskCache>,
    out_dir: &Path,
) -> Result<RunReport> {
    run_inner(spec, llm, None, None, base_dir, cache, out_dir)
}

/// As [`run_with_llm`], with the speech recogniser supplied too instead of
/// built from `spec.asr` (which must still be set). The seam for tests that
/// script what the recogniser hears.
pub fn run_with_asr(
    spec: &EpisodeSpec,
    llm: &dyn LlmProvider,
    asr: Box<dyn AsrProvider>,
    base_dir: &Path,
    cache: Option<&DiskCache>,
    out_dir: &Path,
) -> Result<RunReport> {
    run_inner(spec, llm, None, Some(asr), base_dir, cache, out_dir)
}

fn run_inner(
    spec: &EpisodeSpec,
    llm: &dyn LlmProvider,
    sidecars: Option<&Path>,
    asr: Option<Box<dyn AsrProvider>>,
    base_dir: &Path,
    cache: Option<&DiskCache>,
    out_dir: &Path,
) -> Result<RunReport> {
    let analysers = build_analysers(&spec.analysers);
    // Built before any stage runs, so a bad `[embedding]`/`[nli]` section
    // fails the run before the LLM has spent any time on it. The audio
    // sections, the voice clips, the sidecar profile and the speech
    // recogniser's files are checked now for the same reason; the TTS model
    // itself starts only after the script.
    let grounding = build_grounding(spec, base_dir)?;
    check_audio(spec)?;
    let audio = match &spec.tts {
        Some(tts) => Some(AudioPlan::check(spec, tts, sidecars, asr, base_dir)?),
        None => None,
    };
    let mut report = RunReport::default();

    let mut fetched = Vec::new();
    for connector in build_sources(&spec.sources, base_dir) {
        let documents = connector.fetch()?;
        tracing::info!(
            connector = connector.id(),
            count = documents.len(),
            "fetched documents"
        );
        fetched.extend(documents);
    }

    let documents = cached(&Ingest, &fetched, cache, &mut report)?;
    let chunks = cached(&ChunkDocuments::default(), &documents, cache, &mut report)?;

    let claim_input = ClaimInput {
        sources: source_map(&documents)?,
        titles: documents
            .iter()
            .map(|d| (d.id().clone(), d.title().to_owned()))
            .collect(),
        chunks,
    };
    let mut claims = cached(&ExtractClaims { llm }, &claim_input, cache, &mut report)?;
    if let Some(grounding) = grounding {
        let ground_input = GroundInput {
            claims,
            chunks: claim_input.chunks.clone(),
            titles: claim_input.titles.clone(),
        };
        let grounded = cached(
            &GroundClaims {
                embedder: grounding.embedder.as_ref(),
                nli: grounding.nli.as_ref(),
            },
            &ground_input,
            cache,
            &mut report,
        )?;
        // Logged here rather than in the stage, so a cache hit reports its
        // rejections too.
        for rejection in &grounded.rejected {
            tracing::info!(
                claim = %rejection.text,
                chunk = %rejection.chunk,
                entailment_pm = rejection.entailment_pm.get(),
                "claim not entailed by its chunk; evidence dropped"
            );
        }
        report.grounding = Some(GroundingCounts {
            dropped_claims: grounded.dropped_claims(),
            rejected_evidence: grounded.rejected.len(),
        });
        let merged = cached(
            &ClusterClaims {
                embedder: grounding.embedder.as_ref(),
                nli: grounding.nli.as_ref(),
            },
            &grounded.claims,
            cache,
            &mut report,
        )?;
        let stance_input = StanceInput {
            claims: merged,
            chunks: claim_input.chunks.clone(),
            sources: claim_input.sources.clone(),
        };
        let stage = ScoreStances {
            embedder: grounding.embedder.as_ref(),
            nli: grounding.nli.as_ref(),
        };
        claims = cached(&stage, &stance_input, cache, &mut report)?;
        release("embedding", grounding.embedder.release());
        // `grounding` was moved into this block, so it is dropped here: any
        // model it loaded is freed before the script stage needs the memory.
    }
    let ledger = cached(&BuildLedger, &claims, cache, &mut report)?;

    let script_input = ScriptInput {
        topic: spec.topic.clone(),
        target_minutes: spec.target_minutes,
        ledger,
        chunks: claim_input.chunks,
        documents,
        cast: spec
            .cast
            .iter()
            .map(|member| Speaker {
                id: member.id.clone(),
                name: member.name.clone(),
                role: member.role.clone(),
            })
            .collect(),
        audio: spec.tts.is_some(),
    };
    let script = cached(&WriteScript { llm }, &script_input, cache, &mut report)?;
    // The LLM's last stage: free its GPU memory for the TTS model.
    release("llm", llm.release());

    let analyse_input = AnalyseInput {
        script,
        documents: script_input.documents,
    };
    let mut analysis = cached(
        &Analyse {
            analysers: &analysers,
        },
        &analyse_input,
        cache,
        &mut report,
    )?;
    report.error_findings = analysis.error_count();

    // The text artifacts are written before any audio is made, so a failure
    // in synthesis still leaves the script to read.
    fs::create_dir_all(out_dir).map_err(|err| CoreError::io(out_dir, err))?;
    write(out_dir, ArtifactKind::Episode, spec)?;
    write(out_dir, ArtifactKind::Documents, &analyse_input.documents)?;
    write(out_dir, ArtifactKind::Chunks, &script_input.chunks)?;
    write(out_dir, ArtifactKind::Claims, &claims)?;
    write(out_dir, ArtifactKind::Ledger, &script_input.ledger)?;
    write(out_dir, ArtifactKind::Script, &analyse_input.script)?;
    write(out_dir, ArtifactKind::Analysis, &analysis)?;

    if let Some(mut audio) = audio {
        let (path, findings) = audio.make(&analyse_input.script, cache, out_dir, &mut report)?;
        report.audio = Some(path);
        // A chunk that never passed speech recognition is reported like a
        // misquote: an `Error` finding, the run still complete.
        if !findings.is_empty() {
            analysis.findings.extend(findings);
            report.error_findings = analysis.error_count();
            write(out_dir, ArtifactKind::Analysis, &analysis)?;
        }
    }
    Ok(report)
}

/// Logs a failed unload instead of failing the run: the episode can still be
/// made, and if the GPU really is short, the TTS start-up error says so.
fn release(section: &str, result: Result<()>) {
    if let Err(err) = result {
        tracing::warn!(section, error = %err, "could not unload the model; the GPU may still hold it");
    }
}

/// Everything the audio stages need, checked before any model runs.
struct AudioPlan<'a> {
    tts: &'a TtsConfig,
    /// Where the sidecar profile was found; unused by the fake.
    profiles: PathBuf,
    voices: Voices,
    /// Checked and fingerprinted, not loaded: Whisper loads on its first
    /// transcript, so a fully cached run never loads it.
    asr: Box<dyn AsrProvider>,
    takes: Takes,
}

/// Whisper's default limit, used for the fake recogniser too.
const DEFAULT_MAX_WER_PM: u16 = 80;

impl<'a> AudioPlan<'a> {
    fn check(
        spec: &EpisodeSpec,
        tts: &'a TtsConfig,
        sidecars: Option<&Path>,
        asr: Option<Box<dyn AsrProvider>>,
        base_dir: &Path,
    ) -> Result<Self> {
        let asr_config = spec
            .asr
            .as_ref()
            .expect("check_audio requires [asr] with [tts]");
        let asr = match asr {
            Some(asr) => asr,
            None => build_asr(asr_config, base_dir)?,
        };
        let max_wer_pm = match asr_config {
            AsrConfig::Fake {} => PerMille::new(DEFAULT_MAX_WER_PM).expect("within 0..=1000"),
            AsrConfig::Whisper { max_wer_pm, .. } => *max_wer_pm,
        };
        // The fake's takes don't differ in quality, so one is enough.
        let (banter, max_retries) = match tts {
            TtsConfig::Fake {} => (1, 2),
            TtsConfig::Sidecar {
                takes, max_retries, ..
            } => (*takes, *max_retries),
        };
        let takes = Takes {
            banter,
            max_retries,
            max_wer_pm,
        };
        let profiles = match tts {
            TtsConfig::Fake {} => PathBuf::new(),
            TtsConfig::Sidecar { sidecar, .. } => {
                let path = match sidecars {
                    Some(path) => path.to_owned(),
                    None => default_profiles_path(|name| std::env::var(name).ok())?,
                };
                load_profile(&path, sidecar)?;
                path
            }
        };
        Ok(Self {
            tts,
            profiles,
            voices: Voices::resolve(&spec.cast, base_dir)?,
            asr,
            takes,
        })
    }

    /// Synthesises, checks and assembles the episode; writes `episode.wav`
    /// and `audio.json`, and returns the WAV's path and an `Error` finding
    /// for each chunk that failed its checks.
    fn make(
        &mut self,
        script: &Script,
        cache: Option<&DiskCache>,
        out_dir: &Path,
        report: &mut RunReport,
    ) -> Result<(PathBuf, Vec<Finding>)> {
        // With no cache, the audio still needs somewhere to live between
        // synthesis and assembly; it stays beside the artifacts.
        let blobs = match cache {
            Some(cache) => cache.blobs(),
            None => BlobStore::new(out_dir.join(".blobs")),
        };
        let chunks = {
            let mut tts = build_tts(self.tts, &self.profiles)?;
            let verification = Verification {
                asr: self.asr.as_mut(),
                takes: self.takes,
            };
            synthesize_script(
                script,
                &self.voices,
                tts.as_mut(),
                verification,
                &blobs,
                cache,
                report,
            )?
            // `tts` is dropped at the end of this block: a sidecar worker is
            // stopped and the GPU freed before assembly starts.
        };
        let (records, pcms): (Vec<_>, Vec<Pcm>) =
            chunks.into_iter().map(|c| (c.record, c.pcm)).unzip();
        let findings = verify_audio::findings(&records, self.takes.max_wer_pm);
        let episode = assemble(&pcms)?;

        let path = out_dir.join(EPISODE_WAV);
        let wav = episode.pcm.to_wav(WavFormat::Int16)?;
        fs::write(&path, wav).map_err(|err| CoreError::io(&path, err))?;
        let manifest = AudioManifest {
            sample_rate: SAMPLE_RATE,
            chunks: records,
            episode: EpisodeAudio {
                path: EPISODE_WAV.into(),
                duration_ms: (episode.pcm.seconds() * 1000.0).round() as u64,
                integrated_lufs: episode.loudness.integrated_lufs,
                true_peak_dbtp: episode.loudness.true_peak_dbtp,
            },
            voices: self.voices.credits().to_vec(),
        };
        write(out_dir, ArtifactKind::Audio, &manifest)?;
        Ok((path, findings))
    }
}

/// Maps each document to its source. Two documents with one id but different
/// sources would silently merge their evidence, so that is an error.
fn source_map(documents: &[Document]) -> Result<BTreeMap<DocumentId, SourceRef>> {
    let mut map = BTreeMap::new();
    for doc in documents {
        if let Some(existing) = map.insert(doc.id().clone(), doc.source().clone())
            && existing != *doc.source()
        {
            return Err(CoreError::Source {
                path: doc.source().locator.clone().into(),
                message: format!(
                    "document id {} is shared with {}; give the sources distinct locators",
                    doc.id(),
                    existing.locator
                ),
            });
        }
    }
    Ok(map)
}

fn write<T: Serialize>(out_dir: &Path, kind: ArtifactKind, body: &T) -> Result<()> {
    let path = out_dir.join(format!("{}.json", kind.as_str()));
    let json = serde_json::to_vec_pretty(&Envelope::new(kind, body))?;
    fs::write(&path, json).map_err(|err| CoreError::io(&path, err))
}
