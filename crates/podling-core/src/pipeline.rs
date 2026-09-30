//! Runs an episode end to end: sources → documents → chunks → claims →
//! (stances, when `[embedding]` and `[nli]` are set) → ledger → script →
//! analysis, writing each artifact to the output directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use podling_types::{ArtifactKind, Document, DocumentId, Envelope, EpisodeSpec, SourceRef};
use serde::Serialize;

use crate::cache::DiskCache;
use crate::error::{CoreError, Result};
use crate::plugin::{LlmProvider, build_analysers, build_grounding, build_llm, build_sources};
use crate::stage::{RunReport, cached};
use crate::stages::{
    Analyse, AnalyseInput, BuildLedger, ChunkDocuments, ClaimInput, ExtractClaims, Ingest,
    ScoreStances, ScriptInput, StanceInput, WriteScript,
};

/// Runs `spec`. Relative source paths resolve against `base_dir` (normally
/// the episode file's directory). With `cache` set to `None` every stage runs.
///
/// Sources are always re-read: fetching is cheap, and ingest's cache key
/// covers the documents' text, so an edited source invalidates ingest and
/// everything downstream of it.
pub fn run(
    spec: &EpisodeSpec,
    base_dir: &Path,
    cache: Option<&DiskCache>,
    out_dir: &Path,
) -> Result<RunReport> {
    let llm = build_llm(&spec.llm)?;
    run_with_llm(spec, llm.as_ref(), base_dir, cache, out_dir)
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
    let analysers = build_analysers(&spec.analysers);
    // Built before any stage runs, so a bad `[embedding]`/`[nli]` section
    // fails the run before the LLM has spent any time on it.
    let grounding = build_grounding(spec, base_dir)?;
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
        let stance_input = StanceInput {
            claims,
            chunks: claim_input.chunks.clone(),
            sources: claim_input.sources.clone(),
        };
        let stage = ScoreStances {
            embedder: grounding.embedder.as_ref(),
            nli: grounding.nli.as_ref(),
        };
        claims = cached(&stage, &stance_input, cache, &mut report)?;
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
    };
    let script = cached(&WriteScript { llm }, &script_input, cache, &mut report)?;

    let analyse_input = AnalyseInput {
        script,
        documents: script_input.documents,
    };
    let analysis = cached(
        &Analyse {
            analysers: &analysers,
        },
        &analyse_input,
        cache,
        &mut report,
    )?;
    report.error_findings = analysis.error_count();

    fs::create_dir_all(out_dir).map_err(|err| CoreError::io(out_dir, err))?;
    write(out_dir, ArtifactKind::Episode, spec)?;
    write(out_dir, ArtifactKind::Documents, &analyse_input.documents)?;
    write(out_dir, ArtifactKind::Chunks, &script_input.chunks)?;
    write(out_dir, ArtifactKind::Claims, &claims)?;
    write(out_dir, ArtifactKind::Ledger, &script_input.ledger)?;
    write(out_dir, ArtifactKind::Script, &analyse_input.script)?;
    write(out_dir, ArtifactKind::Analysis, &analysis)?;
    Ok(report)
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
