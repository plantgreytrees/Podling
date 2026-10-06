//! One function per subcommand. Each adds human context to core errors.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use podling_core::plugin::openai_embeddings::PLUGIN as EMBEDDINGS_PLUGIN;
use podling_core::{CoreError, DiskCache, ProviderFailure, pipeline};
use podling_types::{EmbeddingConfig, EpisodeSpec, LlmConfig, schema};

pub fn export_schemas(out: &Path) -> Result<()> {
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    for (kind, schema) in schema::all() {
        let path = out.join(format!("{}.schema.json", kind.as_str()));
        let json = serde_json::to_string_pretty(&schema)?;
        fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

pub fn run(
    episode: &Path,
    out: &Path,
    cache_dir: Option<&Path>,
    sidecars: Option<&Path>,
) -> Result<()> {
    let text = fs::read_to_string(episode)
        .with_context(|| format!("reading episode file {}", episode.display()))?;
    let spec: EpisodeSpec =
        toml::from_str(&text).with_context(|| format!("parsing {}", episode.display()))?;
    let base_dir = episode.parent().unwrap_or(Path::new("."));
    let cache = cache_dir.map(DiskCache::new);

    let report = match sidecars {
        Some(profiles) => {
            pipeline::run_with_sidecars(&spec, profiles, base_dir, cache.as_ref(), out)
        }
        None => pipeline::run(&spec, base_dir, cache.as_ref(), out),
    };
    let report = report
        .map_err(|err| explain(&spec, &err))
        .with_context(|| format!("running episode {:?}", spec.title))?;

    println!("{:<16} {:<5} {:>8}", "stage", "cache", "ms");
    for stage in &report.stages {
        let cache = if stage.cache_hit { "hit" } else { "miss" };
        println!("{:<16} {:<5} {:>8}", stage.id, cache, stage.elapsed_ms);
    }
    // `if let` runs the block only for `Some`, binding what's inside: no line
    // at all for an episode without `[embedding]` and `[nli]`.
    if let Some(grounding) = report.grounding {
        println!(
            "grounding: {} claim(s) dropped, {} evidence item(s) rejected (run with -v to see them)",
            grounding.dropped_claims, grounding.rejected_evidence
        );
    }
    println!("artifacts written to {}", out.display());
    if let Some(audio) = &report.audio {
        println!("episode audio: {}", audio.display());
    }

    if report.error_findings > 0 {
        bail!(
            "analysis reported {} error finding(s); see {}",
            report.error_findings,
            out.join("analysis.json").display()
        );
    }
    Ok(())
}

/// One line for a core error: its whole cause chain, plus what to fix when
/// an HTTP provider failed in a way the user can act on. The core's messages
/// never contain the API key, so neither does this.
fn explain(spec: &EpisodeSpec, err: &CoreError) -> anyhow::Error {
    let mut message = err.to_string();
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    match err
        .provider()
        .and_then(|(plugin, kind)| provider_hint(spec, plugin, kind))
    {
        Some(hint) => anyhow!("{message} ({hint})"),
        None => anyhow!(message),
    }
}

/// The server settings of the provider that failed: `[embedding]` for the
/// embeddings client, `[llm]` otherwise.
fn server_of<'a>(
    spec: &'a EpisodeSpec,
    plugin: &str,
) -> Option<(&'static str, &'a str, &'a str, Option<&'a str>)> {
    if plugin == EMBEDDINGS_PLUGIN {
        match spec.embedding.as_ref()? {
            EmbeddingConfig::OpenAiCompat {
                base_url,
                model,
                api_key_env,
                ..
            } => Some(("embedding", base_url, model, api_key_env.as_deref())),
            EmbeddingConfig::Fake {} => None,
        }
    } else {
        match &spec.llm {
            LlmConfig::OpenAiCompat {
                base_url,
                model,
                api_key_env,
                ..
            } => Some(("llm", base_url, model, api_key_env.as_deref())),
            LlmConfig::Fake {} => None,
        }
    }
}

/// What to fix for a provider failure the user can act on.
fn provider_hint(spec: &EpisodeSpec, plugin: &str, kind: ProviderFailure) -> Option<String> {
    let (section, base_url, model, api_key_env) = server_of(spec, plugin)?;
    match kind {
        ProviderFailure::Http(404) => Some(format!(
            "check that the server has a model called {model:?} (for Ollama: `ollama pull {model}`) and that base_url ends in /v1"
        )),
        ProviderFailure::Http(401 | 403) => Some(match api_key_env {
            Some(var) => format!("check that ${var} holds a valid key for {base_url}"),
            None => format!(
                "{base_url} wants a key: set {section}.api_key_env to the name of the variable that holds it"
            ),
        }),
        ProviderFailure::Unreachable | ProviderFailure::TimedOut => Some(format!(
            "is the server at {base_url} running and reachable?"
        )),
        // A cut-off reply is retried as a rejection and never reaches here as is.
        ProviderFailure::Http(_) | ProviderFailure::CutOff | ProviderFailure::Other => None,
    }
}

pub fn cache_stats(cache_dir: &Path) -> Result<()> {
    let stats = DiskCache::new(cache_dir)
        .stats()
        .with_context(|| format!("reading cache {}", cache_dir.display()))?;
    println!(
        "{} entries, {} bytes; {} audio blobs, {} bytes; in {}",
        stats.entries,
        stats.bytes,
        stats.blobs,
        stats.blob_bytes,
        cache_dir.display()
    );
    Ok(())
}

pub fn cache_clear(cache_dir: &Path) -> Result<()> {
    DiskCache::new(cache_dir)
        .clear()
        .with_context(|| format!("clearing cache {}", cache_dir.display()))?;
    println!("cleared {}", cache_dir.display());
    Ok(())
}
