//! Runs an episode end to end: sources → documents → chunks → claims →
//! ledger → script → analysis, writing each artifact to the output directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use podling_types::{ArtifactKind, Envelope, EpisodeSpec, Severity};
use serde::Serialize;

use crate::cache::DiskCache;
use crate::error::{CoreError, Result};
use crate::plugin::{build_analysers, build_llm, build_sources};
use crate::stage::{RunReport, cached};
use crate::stages::{
    Analyse, AnalyseInput, BuildLedger, ChunkDocuments, ClaimInput, ExtractClaims, Ingest,
    ScriptInput, WriteScript,
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
    let llm = build_llm(&spec.llm);
    let analysers = build_analysers(&spec.analysers);
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
        sources: documents
            .iter()
            .map(|d| (d.id().clone(), d.source().clone()))
            .collect::<BTreeMap<_, _>>(),
        chunks,
    };
    let claims = cached(
        &ExtractClaims { llm: llm.as_ref() },
        &claim_input,
        cache,
        &mut report,
    )?;
    let ledger = cached(&BuildLedger, &claims, cache, &mut report)?;

    let script_input = ScriptInput {
        topic: spec.topic.clone(),
        target_minutes: spec.target_minutes,
        ledger,
        chunks: claim_input.chunks,
        documents,
    };
    let script = cached(
        &WriteScript { llm: llm.as_ref() },
        &script_input,
        cache,
        &mut report,
    )?;

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
    report.error_findings = analysis
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();

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

fn write<T: Serialize>(out_dir: &Path, kind: ArtifactKind, body: &T) -> Result<()> {
    let path = out_dir.join(format!("{}.json", kind.as_str()));
    let json = serde_json::to_vec_pretty(&Envelope::new(kind, body))?;
    fs::write(&path, json).map_err(|err| CoreError::io(&path, err))
}
