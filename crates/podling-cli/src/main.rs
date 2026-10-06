//! `podling`: generate source-grounded podcast episodes.

mod commands;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "podling", version, about = "Source-grounded podcast generator")]
struct Cli {
    /// Log stage progress (RUST_LOG, when set, takes precedence).
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Where stage outputs are cached.
    #[arg(long, global = true, default_value = ".podling/cache")]
    cache_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Work with the artifact JSON Schemas.
    #[command(subcommand)]
    Schema(SchemaCommand),

    /// Run an episode through the whole pipeline.
    Run {
        /// The episode file (TOML).
        #[arg(long)]
        episode: PathBuf,

        /// Where to write the artifacts.
        #[arg(long, default_value = ".podling/out")]
        out: PathBuf,

        /// Run every stage without reading or writing the cache.
        #[arg(long)]
        no_cache: bool,

        /// The TTS worker profiles; defaults to
        /// `~/.config/podling/sidecars.toml`. Never read from the episode.
        #[arg(long)]
        sidecars: Option<PathBuf>,
    },

    /// Inspect or empty the stage cache.
    #[command(subcommand)]
    Cache(CacheCommand),
}

#[derive(Debug, Subcommand)]
enum SchemaCommand {
    /// Write `<kind>.schema.json` for every artifact kind.
    Export {
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// Show the number of entries and their total size.
    Stats,
    /// Delete every cached entry.
    Clear,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let result = match cli.command {
        Command::Schema(SchemaCommand::Export { out }) => commands::export_schemas(&out),
        Command::Run {
            episode,
            out,
            no_cache,
            sidecars,
        } => commands::run(
            &episode,
            &out,
            (!no_cache).then_some(cli.cache_dir.as_path()),
            sidecars.as_deref(),
        ),
        Command::Cache(CacheCommand::Stats) => commands::cache_stats(&cli.cache_dir),
        Command::Cache(CacheCommand::Clear) => commands::cache_clear(&cli.cache_dir),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // `{:#}` prints the whole context chain on one line.
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// Logs go to stderr so stdout stays clean for results.
fn init_tracing(verbose: bool) {
    let default = if verbose { "info" } else { "warn" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
