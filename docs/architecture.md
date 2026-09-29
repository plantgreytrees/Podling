# Architecture

This document describes the state after Phase 1 (core contracts). Where the design
is heading is recorded in [`.claude/CLAUDE.md`](../.claude/CLAUDE.md).

## Crates

| Crate | Role |
|---|---|
| [`podling-types`](../crates/podling-types/src/lib.rs) | Artifact types (the data contract), content-hash ids, and JSON Schema export. It has no I/O. |
| [`podling-core`](../crates/podling-core/src/lib.rs) | Plugin traits, the content-addressed cache, the stages, and the pipeline. |
| [`podling-cli`](../crates/podling-cli/src/main.rs) | The `podling` binary: argument parsing, logging setup, and error reporting. |

The dependencies run one way: `cli → core → types`. Phase 1 is synchronous.
tokio arrives with the first network-bound provider.

## Artifact flow

```
 EpisodeSpec (TOML)
      │
      ▼
 SourceConnector::fetch ── always re-read (cheap); the cache key covers the text
      │ Vec<Document>
      ▼
 ingest ─────────► Vec<Document>   BOM/CRLF normalised; duplicates within a group dropped
 chunk ──────────► Vec<Chunk>      split at Markdown headings, then paragraphs (~800 words)
 extract_claims ─► Vec<Claim>      one LLM call per chunk; merged by ClaimId + Evidence
 ledger ─────────► Ledger          classify(): status from distinct independence groups
 script ─────────► Script          LLM returns QuoteRefs → Quote::from_document copies words
 analyse ────────► AnalysisReport  opt-in analysers, e.g. quote_verifier
```

[`pipeline::run`](../crates/podling-core/src/pipeline.rs) calls the six
stages in order. Each call goes through
[`stage::cached`](../crates/podling-core/src/stage.rs), which opens a
`tracing` span `stage{id, version}`, logs `cache_hit` and `elapsed_ms`, and
adds a `StageRecord` to the `RunReport`. Every artifact is written as
`<out>/<kind>.json` inside an `Envelope { schema_version, kind, body }`.

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
   fingerprint in their config, so the cache invalidates.

## Plugins

Plugins are trait objects, because the episode file chooses them at run time.
The factories in [`plugin/mod.rs`](../crates/podling-core/src/plugin/mod.rs)
are plain `match`es. Adding a plugin takes two steps: add a variant to the
config enum in [`episode.rs`](../crates/podling-types/src/episode.rs), then
add one arm to the factory.

| Kind | Trait | Phase 1 implementations |
|---|---|---|
| Provider (LLM) | `LlmProvider` | `FakeLlm`: deterministic, offline |
| Source connector | `SourceConnector` | `LocalFilesConnector`: `.md`/`.txt` in one directory, symlinks confined to the root, 10 MiB cap |
| Analyser | `Analyser` | `QuoteVerifier`: every quote matches its source span, and the turn speaks it verbatim |

Deferred to later phases:
- OpenAI-compatible HTTP LLM providers.
- TTS, embedding, NLI and ASR provider traits.
- MCP source connectors.
- PDF ingestion (Docling / pdfium).
- A Contested-claim adjudicator.

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

The same reasoning applies to quotes. The model can only *point* at a span (`QuoteRef`), and
[`Quote::from_document`](../crates/podling-types/src/quote.rs) copies the words
from the source. A model therefore cannot put words in a source's mouth.
