//! One function per subcommand. Each adds human context to core errors.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use podling_core::{CoreError, DiskCache, pipeline};
use podling_types::{EpisodeSpec, LlmConfig, schema};

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

pub fn run(episode: &Path, out: &Path, cache_dir: Option<&Path>) -> Result<()> {
    let text = fs::read_to_string(episode)
        .with_context(|| format!("reading episode file {}", episode.display()))?;
    let spec: EpisodeSpec =
        toml::from_str(&text).with_context(|| format!("parsing {}", episode.display()))?;
    let base_dir = episode.parent().unwrap_or(Path::new("."));
    let cache = cache_dir.map(DiskCache::new);

    let report = pipeline::run(&spec, base_dir, cache.as_ref(), out)
        .map_err(|err| explain(&spec, &err))
        .with_context(|| format!("running episode {:?}", spec.title))?;

    println!("{:<16} {:<5} {:>8}", "stage", "cache", "ms");
    for stage in &report.stages {
        let cache = if stage.cache_hit { "hit" } else { "miss" };
        println!("{:<16} {:<5} {:>8}", stage.id, cache, stage.elapsed_ms);
    }
    println!("artifacts written to {}", out.display());

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
/// the LLM provider failed in a way the user can act on. The core's messages
/// never contain the API key, so neither does this.
fn explain(spec: &EpisodeSpec, err: &CoreError) -> anyhow::Error {
    let mut message = err.to_string();
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    match provider_hint(&spec.llm, &message) {
        Some(hint) => anyhow!("{message} ({hint})"),
        None => anyhow!(message),
    }
}

/// Matches the wording of `OpenAiCompat`'s error messages ("answered HTTP
/// 401", "request to <url> failed" / "timed out"); the CLI tests pin both ends.
fn provider_hint(llm: &LlmConfig, message: &str) -> Option<String> {
    let LlmConfig::OpenAiCompat {
        base_url,
        model,
        api_key_env,
        ..
    } = llm
    else {
        return None;
    };
    if message.contains("answered HTTP 404") {
        return Some(format!(
            "check that the server has a model called {model:?} (for Ollama: `ollama pull {model}`) and that base_url ends in /v1"
        ));
    }
    if message.contains("answered HTTP 401") || message.contains("answered HTTP 403") {
        return Some(match api_key_env {
            Some(var) => format!("check that ${var} holds a valid key for {base_url}"),
            None => format!(
                "{base_url} wants a key: set llm.api_key_env to the name of the variable that holds it"
            ),
        });
    }
    let unreachable = message.contains("request to") && message.contains("failed");
    (unreachable || message.contains("timed out"))
        .then(|| format!("is the server at {base_url} running and reachable?"))
}

pub fn cache_stats(cache_dir: &Path) -> Result<()> {
    let stats = DiskCache::new(cache_dir)
        .stats()
        .with_context(|| format!("reading cache {}", cache_dir.display()))?;
    println!(
        "{} entries, {} bytes in {}",
        stats.entries,
        stats.bytes,
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
