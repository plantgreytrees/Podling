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

This is **Phase 5: episode audio**. The pipeline runs end to end, either offline
with a deterministic fake LLM or with any OpenAI-compatible model (Ollama,
llama.cpp, vLLM, LM Studio, OpenAI), and, with `[tts]`, ends in a spoken
`episode.wav`:

```
sources → documents → chunks → claims → [ground → cluster → stances] → ledger → script → analysis
        → [chunks of speech → synthesise → check with speech recognition → assemble]
```

With the optional `[embedding]` and `[nli]` sections, the same fact worded differently
by two independent sources becomes one Corroborated claim, and a source that
contradicts a claim makes it Contested. Before that, a claim is kept only where its
own source passage entails it, so a distortion made from the passage's own words
("Kulik led the expedition" when he joined it) is dropped and counted. A local NLI
model decides all three, not exact text matching (see
[Grounding with embeddings and NLI](#grounding-with-embeddings-and-nli)).

With `[tts]`, a local text-to-speech model speaks the script in voices cloned from
short reference clips, Whisper checks every chunk against the script (every quote
word for word), and the episode is assembled at podcast loudness (see
[Episode audio](#episode-audio)).

Every stage is cached, and every artifact is a versioned JSON file with an exported JSON
Schema. PDF ingestion, MCP source connectors and an LLM adjudicator for
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
| `podling run --episode <file.toml> [--out <dir>] [--no-cache] [--sidecars <file>]` | Run an episode. Exits non-zero if an analyser reports an error. `--sidecars` overrides where TTS worker profiles are read from. |
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
| `unload_after` | Optional, default `false`. Ollama only: free the model as soon as its last stage is done, so the GPU is empty for text-to-speech. |

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
| `embedding.base_url`, `embedding.model`, `embedding.api_key_env`, `embedding.timeout_secs`, `embedding.unload_after` | As for `[llm]`. |
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

### Episode audio

`examples/tunguska/episode-tts.toml` goes on from the script to a spoken episode,
entirely on one machine; an 8 GB GPU is enough because only one model is on it at
a time. Its header lists what to fetch first. In short:

1. **The TTS worker.** A small Python program that holds the voice model
   (Qwen3-TTS 1.7B, Apache-2.0); install it as
   [`sidecars/tts/README.md`](sidecars/tts/README.md) says.
2. **A worker profile** in `~/.config/podling/sidecars.toml` (or pass
   `--sidecars <file>`). The episode file only *names* a profile; it can never
   choose a program, so a shared episode file cannot start a process:

   ```toml
   [sidecars.qwen]
   program = "/home/you/.local/bin/uv"     # argv, never a shell string
   args = ["run", "--project", "/path/to/Podling/sidecars/tts", "--extra", "qwen",
           "podling-tts", "--backend", "qwen"]
   ```
3. **Whisper weights** (`openai/whisper-base.en`, MIT) for the checks:
   `hf download openai/whisper-base.en --local-dir examples/tunguska/models/whisper-base.en`.
4. **Voice clips** with a licence that allows your use; see
   [`examples/tunguska/voices/README.md`](examples/tunguska/voices/README.md).
5. **`ffmpeg`** on `PATH`, only for an Opus or MP3 copy.

```sh
cargo run --release -p podling-cli -- run --episode examples/tunguska/episode-tts.toml
```

The run writes `episode.wav` (48 kHz, −16 LUFS, peaks at most −1 dBTP), the optional
`episode.opus`/`episode.mp3`, and `audio.json`, which lists every chunk with its seed,
take and check result, and every voice with its licence. Each chunk is cached on its
own, so editing one turn re-synthesises one chunk. A chunk that still fails its check
after `max_retries` becomes an `Error` finding in `analysis.json`; the episode is still
assembled.

| Key | Meaning |
|---|---|
| `[[cast]]` `id`, `name`, `role` | One entry per speaker. With a cast, the script may use only these speaker ids. |
| `[[cast]]` `voice = { reference, transcript, licence }` | The reference clip (WAV, relative to the episode file), exactly what is said in it, and its SPDX licence (e.g. `CC0-1.0`, `CC-BY-4.0`). All three are required; the licence is copied into `audio.json`. |
| `tts.kind` | `sidecar` (a worker process) or `fake` (sine tones, offline, for tests). Needs `[asr]` and `[[cast]]`. |
| `tts.sidecar` | The profile name in `sidecars.toml`. |
| `tts.takes` | Default 2, at least 1. Takes per banter beat; the best one that passes is kept. Other beats get one. |
| `tts.max_retries` | Default 2. Further attempts, each with a new seed, for a chunk that fails its check. |
| `asr.kind` | `whisper` (on the CPU) or `fake`. |
| `asr.model_dir` | Directory with Whisper's `config.json`, `tokenizer.json`, `model.safetensors` and `preprocessor_config.json`. |
| `asr.max_wer_pm` | Default 80 (8%). The highest word error rate, in thousandths, a chunk may have and pass. Every quote must also be heard word for word. |
| `mix.gaps_ms` | Optional silences in ms before a turn, by its pace: `quick` 120, `normal` 300, `beat` 600, `long_pause` 1000, and `interrupt` 150 (an overlap, crossfaded). Set any of them, e.g. `gaps_ms = { beat = 700 }`. `[mix]` needs `[tts]`. |
| `mix.encode` | Optional `opus` (64 kb/s) or `mp3`: a copy of `episode.wav` made with `ffmpeg`. |

**Licences.** A cloned voice carries its clip's terms, and many voice datasets are
non-commercial (Kyutai's Expresso and EARS voices are CC-BY-NC), so check each clip
before you use it. The worker, its libraries and the default weights are
Apache-2.0, MIT or BSD; the list is in `sidecars/tts/README.md`.

**Privacy.** The voice model and Whisper run locally; no audio or text leaves the
machine. The worker listens on `127.0.0.1` only and is stopped when synthesis ends.
