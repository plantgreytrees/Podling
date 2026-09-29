# Architecture

> **Status:** current as of 2026-09-30. It covers Phase 1 (core contracts), the
> `/scrutinise` fixes and Phase 2 (the OpenAI-compatible LLM provider). Synced
> through commit `5059b6f` (the Phase 2 code); the CLI hints and these docs
> landed in the commit after it.

This document describes the state after Phase 2. Where the design
is heading is recorded in [`.claude/CLAUDE.md`](../.claude/CLAUDE.md).

## Crates

| Crate | Role |
|---|---|
| [`podling-types`](../crates/podling-types/src/lib.rs) | Artifact types (the data contract), content-hash ids, and JSON Schema export. It has no I/O. |
| [`podling-core`](../crates/podling-core/src/lib.rs) | Plugin traits, the content-addressed cache, the stages, and the pipeline. |
| [`podling-cli`](../crates/podling-cli/src/main.rs) | The `podling` binary: argument parsing, logging setup, and error reporting. |

The dependencies run one way: `cli → core → types`. Everything is synchronous,
including the HTTP provider (`ureq`, blocking). tokio waits for something that
needs concurrency, such as parallel TTS or streaming.

## Artifact flow

```
 EpisodeSpec (TOML)
      │
      ▼
 SourceConnector::fetch ── always re-read (cheap); the cache key covers the text
      │ Vec<Document>
      ▼
 ingest ─────────► Vec<Document>   BOM/CRLF normalised; duplicates within a group dropped
 chunk ──────────► Vec<Chunk>      split at Markdown headings (not inside code fences), then paragraphs (~800 words)
 extract_claims ─► Vec<Claim>      one LLM call per chunk; merged by ClaimId + Evidence
 ledger ─────────► Ledger          classify(): status from distinct independence groups
 script ─────────► Script          LLM sees numbered sentences, returns QuoteRef { chunk, sentence };
                                   Quote::from_document copies the words; every citation
                                   must name a claim in the ledger
 analyse ────────► AnalysisReport  opt-in analysers, e.g. quote_verifier
```

[`pipeline::run`](../crates/podling-core/src/pipeline.rs) calls the six
stages in order. Each call goes through
[`stage::cached`](../crates/podling-core/src/stage.rs), which opens a
`tracing` span `stage{id, version}`, logs `cache_hit` and `elapsed_ms`, and
adds a `StageRecord` to the `RunReport`. Every artifact is written as
`<out>/<kind>.json` inside an `Envelope { schema_version, kind, body }`.

Provider output is never trusted. The following are `InvalidProviderOutput` errors:
- malformed JSON;
- a quote that names an unknown chunk or a sentence the chunk doesn't have
  (or that doesn't resolve in its document);
- a citation of a claim id that isn't in the ledger.

Both LLM stages call the model through
[`complete_validated`](../crates/podling-core/src/plugin/llm.rs). If a reply fails
those checks, the model is asked once more with the reason appended to the
instructions. A second failure is the error, so a stage makes at most two calls per
chunk or script. Transport failures are not retried there, because the provider
has its own policy (below).

`pipeline::run` also fails closed if two fetched documents share an id but
come from different sources. Without that check, their evidence would merge silently.

## Cache key

[`CacheKey::new`](../crates/podling-core/src/cache.rs) computes:

```
key = BLAKE3( len‖stage_id, len‖stage_version (u32 LE),
              len‖canonical_json(input), len‖canonical_json(config_fingerprint) )
```

Each part is length-prefixed, so different splits can never collide. Canonical JSON
means `serde_json::to_value` followed by serialisation, so object keys come out sorted.
Two rules keep that sorting in place. Do not enable serde_json's `preserve_order`
feature. Do not use `HashMap`/`HashSet` either, which
[`clippy.toml`](../clippy.toml) enforces with `disallowed-types`.

Entries are stored at `<cache>/<first two hex chars>/<key>.json` as an
`Envelope`. They are written through a temp file plus rename, so writes are atomic. An entry
that is corrupt or was written under another `SCHEMA_VERSION` is a **miss**
with a warning, never an error.

`podling cache clear` deletes only files shaped like cache entries
(`<2 hex>/<64 hex>.json` and leftover `.tmp*` files in those shards), then any
directories that are left empty. Anything else stays, with a warning. Pointing
`--cache-dir` at the wrong directory therefore cannot destroy it.

The cache is keyed by content, which gives *early cutoff*. If a stage
re-runs but produces identical output, the downstream stages see the same input
and are still cache hits.

## The three bump rules

1. **You changed an artifact's fields.** The `insta` schema snapshot test in
   [`crates/podling-types/tests/schema_snapshot.rs`](../crates/podling-types/tests/schema_snapshot.rs)
   fails. Review the diff, accept it with `cargo insta accept` (or
   `INSTA_UPDATE=always cargo test`), and bump
   [`SCHEMA_VERSION`](../crates/podling-types/src/envelope.rs).
2. **You changed a stage's logic.** Bump that stage's `Stage::VERSION`. Otherwise
   the cache keeps serving the old outputs.
3. **You changed a provider's behaviour.** Make sure `LlmProvider::fingerprint()`
   changes, for example the model name or a version field. Stages include the
   fingerprint in their config, so the cache invalidates. `OpenAiCompat`'s
   fingerprint holds the base URL, model, temperature, max output tokens and a
   request-layout version, and never the API key, so rotating a key keeps the cache.
4. **You changed a prompt or the shape of an LLM input.** Bump
   [`PROMPT_VERSION`](../crates/podling-core/src/plugin/llm.rs). Both LLM stages
   put it in their config fingerprint next to the instruction text.

## Plugins

Plugins are trait objects, because the episode file chooses them at run time.
The factories in [`plugin/mod.rs`](../crates/podling-core/src/plugin/mod.rs)
are plain `match`es. Adding a plugin takes two steps: add a variant to the
config enum in [`episode.rs`](../crates/podling-types/src/episode.rs), then
add one arm to the factory.

| Kind | Trait | Implementations |
|---|---|---|
| Provider (LLM) | `LlmProvider` | `FakeLlm`: deterministic, offline. `OpenAiCompat`: any OpenAI chat-completions server (see below). |
| Source connector | `SourceConnector` | `LocalFilesConnector`: `.md`/`.txt` in one directory, symlinks confined to the root, 10 MiB cap. The locator is `<root as written in the episode>/<file name>`, so same-named files in different roots get distinct ids. |
| Analyser | `Analyser` | `QuoteVerifier`: every quote matches its source span, and the turn speaks it verbatim |

Deferred to later phases:
- Non-OpenAI-compatible LLM protocols, streaming, token budgeting.
- TTS, embedding, NLI and ASR provider traits.
- MCP source connectors.
- PDF ingestion (Docling / pdfium).
- A Contested-claim adjudicator.

## The OpenAI-compatible provider

[`OpenAiCompat`](../crates/podling-core/src/plugin/openai.rs) sends
`POST {base_url}/chat/completions`, which covers Ollama, llama.cpp
`llama-server`, vLLM, LM Studio and OpenAI. The instructions go in the `system` message
and the task input, as JSON, in the `user` message. It asks for
`response_format: json_object`. JSON mode only guarantees a JSON *object*, so every
LLM task replies with an object (claim extraction returns `{ "claims": [...] }`).

| Concern | Behaviour |
|---|---|
| Key | Read once from the variable named by `api_key_env`. Named but unset or empty is a `Config` error before any request. Sent only as `Authorization: Bearer`. Its `Debug` prints `[redacted]`, and error excerpts and logs never contain it. |
| URL | Must be `http://` or `https://`, with no credentials, query or fragment. A key over plain `http` to a non-local host logs a warning. Redirects are off, so the header can't follow one to another host. |
| Limits | Per-request timeout (default 120 s); response bodies over 4 MiB are rejected while being read; an error body is quoted up to 512 bytes. |
| Retries | A 429 or 5xx is retried twice (0.5 s, then 1 s). A timeout, a 4xx or a transport error is not. Separately, `complete_validated` re-asks once when a reply fails validation. |
| Observability | One `tracing` span per request with the model, elapsed ms, attempts and token usage. Never the key or the prompt text. |
| TLS | rustls with the bundled web PKI roots, no OpenSSL. The added licences are permissive (Apache-2.0/MIT/ISC/BSD-3/CDLA-Permissive-2.0). |

**Sentence-addressed quotes.** For the script, each chunk is shown to the model as
numbered sentences (`text::sentences`, counting from 0), and the model answers with
`QuoteRef { chunk, sentence }`. Models count sentences far more reliably than bytes.
The stage looks the chunk up, takes the sentence's span, and calls
`Quote::from_document`. The invariant is unchanged: the model points and the code copies.

**Prompt injection.** Source text can't be sanitised, so it is contained instead.
The instructions call ledger and source text untrusted data, and that text reaches the
model only inside the JSON input, never in the instructions. Whatever the model
returns is checked afterwards. Quotes come from the source, citations must be ledger
ids, and `QuoteVerifier` re-checks every quote. A successful injection can therefore
change wording, but it can't invent a quote or a citation.

**Known limit.** Claims merge by exact normalised text. A model that paraphrases one
fact from two sources will not corroborate it. Semantic clustering belongs with the
NLI provider, which is a later phase.

## Why a claim ledger, not debating agents

Agents that "argue it out" produce outcomes that depend on prompts and
sampling, and you can't audit them afterwards. The ledger makes trust
**deterministic and inspectable**:
- A claim's status is a pure function of its evidence, via
  [`classify`](../crates/podling-types/src/ledger.rs).
- It counts distinct *independence groups*, not documents. Syndicated copies of one report
  therefore never look like corroboration.
- An LLM is needed only where judgement really is required: extracting
  claims, writing the script, and (later) adjudicating Contested claims.

The same reasoning applies to quotes. The model can only *point* at a sentence (`QuoteRef`), and
[`Quote::from_document`](../crates/podling-types/src/quote.rs) copies the words
from the source. A model therefore cannot put words in a source's mouth.
