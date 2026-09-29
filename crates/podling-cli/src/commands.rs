//! One function per subcommand. Each adds human context to core errors.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use podling_core::{DiskCache, pipeline};
use podling_types::{EpisodeSpec, schema};

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
