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

This is **Phase 3: embeddings and NLI in the claim ledger**. The pipeline runs end
to end, either offline with a deterministic fake LLM or with any OpenAI-compatible
model (Ollama, llama.cpp, vLLM, LM Studio, OpenAI):

```
sources → documents → chunks → claims → [ground → cluster → stances] → ledger → script → analysis
```

With the optional `[embedding]` and `[nli]` sections, the same fact worded differently
by two independent sources becomes one Corroborated claim, and a source that
contradicts a claim makes it Contested. Before that, a claim is kept only where its
own source passage entails it, so a distortion made from the passage's own words
("Kulik led the expedition" when he joined it) is dropped and counted. A local NLI
model decides all three, not exact text matching (see
[Grounding with embeddings and NLI](#grounding-with-embeddings-and-nli)).

Every stage is cached, and every artifact is a versioned JSON file with an exported JSON
Schema. Text-to-speech, PDF ingestion, MCP source connectors and an LLM adjudicator for
Contested claims come in later phases. See [docs/architecture.md](docs/architecture.md).

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

**What the model can and can't do.** It picks which sentences to quote by number and
writes `{{quote:N}}` in a turn's text where each one goes. Podling copies the words
from the source and puts them there, so the model never types a quotation. A citation
of a claim that is not in the ledger, a quote of a sentence that doesn't exist, a
placeholder with no quote behind it, or quoted words the model typed itself, is rejected. The model gets one
retry with the reason, and then the run fails with an error naming the stage. A
small local model may produce invalid JSON often, so pick an instruct model that
handles JSON well. A rate-limited (429) or failing (5xx) server is retried twice.

Failures print one line naming what to fix: an unset key variable, an
unreachable server, a 401 (which variable to check) or a 404 (the model name).

### Grounding with embeddings and NLI

Without these sections, claims only merge when their text matches exactly, so two
sources saying "some 80 million trees were flattened" and "about 80 million trees
were knocked down" give two SingleSource claims. Add both sections to fix that
(`examples/tunguska/episode-ollama.toml` has them):

```toml
[embedding]
kind = "open_ai_compat"
base_url = "http://localhost:11434/v1"
model = "nomic-embed-text"

[nli]
kind = "cross_encoder"
model_dir = "models/nli-deberta-v3-base"   # relative to the episode file
```

Fetch the two models once:

```sh
ollama pull nomic-embed-text
hf download cross-encoder/nli-deberta-v3-base \
  --local-dir examples/tunguska/models/nli-deberta-v3-base
```

Two stages then run between extraction and the ledger. `cluster_claims` merges two
claims when the NLI model finds that each entails the other and their numbers are
equal ("in 1907" never merges with "in 1908"); embedding similarity only picks which
pairs to check. `score_stances` reads each claim against the most similar sentences
of the *other* independence groups: one that entails it adds support, one that
clearly contradicts it adds a contradiction, and the ledger turns that into
Contested. Each such piece of evidence records the source span and the scores that
decided it. The claim's status still comes from fixed rules, never from an LLM.

| Key | Meaning |
|---|---|
| `embedding.kind` | `open_ai_compat` (any server with `POST /v1/embeddings`) or `fake` (offline, for tests). |
| `embedding.base_url`, `embedding.model`, `embedding.api_key_env`, `embedding.timeout_secs` | As for `[llm]`. |
| `nli.kind` | `cross_encoder` or `fake`. |
| `nli.model_dir` | Directory with `config.json`, `tokenizer.json` and `model.safetensors`. |

Set both sections or neither; one alone is a config error. The NLI model
(`cross-encoder/nli-deberta-v3-base`, Apache-2.0, about 0.7 GB) runs on the CPU, at
roughly 60 ms per sentence pair, and is loaded only when a stage actually runs, so a
cached rerun never loads it. It is dropped before the script is written.

**Privacy.** The embedding server receives the text of every claim and every source
sentence. A local server keeps it on your machine. The NLI model is always local.

**Context window.** The script call sends the whole claim ledger plus every chunk's
numbered sentences. A server with a small context window (Ollama defaults to a few
thousand tokens) silently cuts that off, and the model then answers with invalid
JSON. Podling logs a warning when the request is large (over about 24 KiB). Raise
the server's context, for example `OLLAMA_CONTEXT_LENGTH=16384 ollama serve`.
Token budgeting is not built yet.

To run the test that talks to a real server (it is `#[ignore]`d, so a normal
`cargo test` skips it). It fails unless both variables are set, and
`PODLING_LIVE_LLM_KEY_ENV` may name a variable that holds an API key:

```sh
PODLING_LIVE_LLM_URL=http://localhost:11434/v1 PODLING_LIVE_LLM_MODEL=llama3.1:8b \
  cargo test -p podling-cli -- --ignored live
```
