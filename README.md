# Podling

Podling generates story-driven podcast episodes that stay grounded in their
sources, in the spirit of NotebookLM's audio overviews. It works in narrative
non-fiction first, and fiction modes come later.

Podling doesn't hand the story to an LLM. It turns the sources into a claim ledger.
Every claim records which independent sources support it, and gets a
status: *Corroborated*, *SingleSource*, *Contested* or *Unsupported*. The script
is written from that ledger. Quotes are exact spans copied from the sources,
never text the model typed.

## Status

This is **Phase 1: core contracts**. The pipeline runs end to end offline
with a deterministic fake LLM:

```
sources → documents → chunks → claims → ledger → script → analysis
```

Every stage is cached, and every artifact is a versioned JSON file with an exported JSON
Schema. Real LLM providers, text-to-speech, PDF ingestion and MCP source
connectors come in later phases. See [docs/architecture.md](docs/architecture.md).

## Prerequisites

Install a Rust toolchain with [rustup](https://rustup.rs). `rust-toolchain.toml` pins the
stable channel with `clippy` and `rustfmt`.

## Build and test

```sh
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Usage

Run the example episode. Its two sources are an eyewitness account and an
expedition report about the 1908 Tunguska event:

```sh
cargo run -p podling-cli -- run --episode examples/tunguska/episode.toml
```

It prints one row per stage showing whether the cache was hit, then writes
`episode`, `documents`, `chunks`, `claims`, `ledger`, `script` and
`analysis` JSON files to `.podling/out`. Run it again and every stage is a
cache hit.

| Command | What it does |
|---|---|
| `podling run --episode <file.toml> [--out <dir>] [--no-cache]` | Run an episode. Exits non-zero if an analyser reports an error. |
| `podling schema export --out <dir>` | Write `<kind>.schema.json` for every artifact kind. |
| `podling cache stats` / `podling cache clear` | Inspect or empty the stage cache. |

Global flags: `--verbose` logs stage progress to stderr (`RUST_LOG` takes
precedence), and `--cache-dir <dir>` sets the cache location (default `.podling/cache`).

To use the binary directly, run `cargo install --path crates/podling-cli`, or call
`target/debug/podling` after a build.

### Writing an episode

```toml
title = "The Tunguska event"
topic = "the 1908 Tunguska explosion"
target_minutes = 5
llm = { kind = "fake" }
sources = [
  # Every .md/.txt file directly in `root` (relative to this file).
  { kind = "local_files", root = "sources/eyewitness", independence_group = "eyewitness" },
  { kind = "local_files", root = "sources/expedition", independence_group = "expedition" },
]
analysers = [{ kind = "quote_verifier" }]
```

An **independence group** names where a source's information came from.
Five articles rewritten from one wire report belong in one group, so
together they count as a single source. Podling rejects unknown keys, and
episode files never hold secrets.
