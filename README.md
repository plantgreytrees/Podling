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

This is **Phase 2: a real LLM provider**. The pipeline runs end to end, either
offline with a deterministic fake LLM or with any OpenAI-compatible model
(Ollama, llama.cpp, vLLM, LM Studio, OpenAI):

```
sources → documents → chunks → claims → ledger → script → analysis
```

Every stage is cached, and every artifact is a versioned JSON file with an exported JSON
Schema. Text-to-speech, PDF ingestion, MCP source connectors and semantic
claim clustering come in later phases. See [docs/architecture.md](docs/architecture.md).

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

### Using a real model

Replace the `llm` line with an `open_ai_compat` table. This is
[`examples/tunguska/episode-ollama.toml`](examples/tunguska/episode-ollama.toml):

```toml
[llm]
kind = "open_ai_compat"
base_url = "http://localhost:11434/v1"   # Ollama; llama-server uses http://localhost:8080/v1
model = "llama3.1:8b"
temperature = 0.2
```

For a local model, start the server, pull a model and run the episode:

```sh
ollama pull llama3.1:8b
cargo run -p podling-cli -- run --episode examples/tunguska/episode-ollama.toml
```

For OpenAI or any hosted provider, name the environment variable that holds
your key, and set that variable in your shell:

```toml
[llm]
kind = "open_ai_compat"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"
```

```sh
export OPENAI_API_KEY=...   # never put the key in the episode file
```

| Key | Meaning |
|---|---|
| `base_url` | Up to and including `/v1`. Must start with `http://` or `https://`, with no credentials in it. |
| `model` | The model name as the server knows it. |
| `api_key_env` | Optional. The *name* of the variable holding the key. If it is named but unset or empty, the run stops before any request. |
| `temperature`, `max_output_tokens` | Optional. The server's defaults apply when left out. |
| `timeout_secs` | Optional, default 120. |

**Privacy.** With a hosted provider, the text of your sources (chunk by chunk,
then the claim ledger and numbered source sentences for the script) is sent to
that provider. With a local server on `localhost` nothing leaves your machine, so
that is the recommended default. The key is sent only as an `Authorization:
Bearer` header. It is never written to the cache, the artifacts, a log line or an error
message. Cached outputs derived from your sources are stored under `--cache-dir`.

**What the model can and can't do.** It picks which sentences to quote by number, and
Podling copies the words from the source. A citation of a claim that is not in the
ledger, or a quote of a sentence that doesn't exist, is rejected. The model gets one
retry with the reason, and then the run fails with an error naming the stage. A
small local model may produce invalid JSON often, so pick an instruct model that
handles JSON well. A rate-limited (429) or failing (5xx) server is retried twice.

Failures print one line naming what to fix: an unset key variable, an
unreachable server, a 401 (which variable to check) or a 404 (the model name).

To run the test that talks to a real server (skipped by default):

```sh
PODLING_LIVE_LLM_URL=http://localhost:11434/v1 PODLING_LIVE_LLM_MODEL=llama3.1:8b \
  cargo test -p podling-cli -- --ignored live
```
