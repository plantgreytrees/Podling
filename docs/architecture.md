# Architecture

> **Status:** current as of 2026-10-01. It covers Phase 1 (core contracts), the
> `/scrutinise` fixes, Phase 2 (the OpenAI-compatible LLM provider) and its
> scrutinise fixes (unreferenced-quotation check, claim grounding, script input
> size warning), `{{quote:N}}` placeholders in script turns, claim grounding that
> can name things from the title and headings, and Phase 3 (embeddings and NLI
> in the claim ledger).

This document describes the state after Phase 3. Where the design
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
 [cluster_claims] ► Vec<Claim>     paraphrases merged: mutual NLI entailment, equal numbers
 [score_stances] ─► Vec<Claim>     other groups' sentences checked by NLI: Supports / Contradicts
 ledger ─────────► Ledger          classify(): status from distinct independence groups
 script ─────────► Script          LLM sees each claim's id, text and status, and numbered
                                   sentences; returns QuoteRef { chunk, sentence }
                                   and writes {{quote:N}} in the turn text where the quote goes;
                                   Quote::from_document copies the words and the stage fills the
                                   placeholder in; every citation must name a claim in the ledger
 analyse ────────► AnalysisReport  opt-in analysers, e.g. quote_verifier
```

[`pipeline::run`](../crates/podling-core/src/pipeline.rs) calls the stages in
order. The two bracketed ones run only when the episode has both `[embedding]`
and `[nli]` (see [Grounding with embeddings and NLI](#grounding-with-embeddings-and-nli));
without them the run has the same six stages, cache keys and artifact bodies as
before Phase 3. Each call goes through
[`stage::cached`](../crates/podling-core/src/stage.rs), which opens a
`tracing` span `stage{id, version}`, logs `cache_hit` and `elapsed_ms`, and
adds a `StageRecord` to the `RunReport`. Every artifact is written as
`<out>/<kind>.json` inside an `Envelope { schema_version, kind, body }`.

Provider output is never trusted. The following are `InvalidProviderOutput` errors:
- malformed JSON;
- a quote that names an unknown chunk or a sentence the chunk doesn't have
  (or that doesn't resolve in its document);
- a citation of a claim id that isn't in the ledger;
- a turn whose text has a `{{quote:N}}` with no quote reference N, a quote
  reference with no `{{quote:N}}`, or a malformed placeholder;
- a turn whose text puts three or more words in quotation marks itself: the
  model never types quoted words, so every quotation must come from a
  placeholder;
- a turn that, after the placeholders are filled in, doesn't speak a quote it
  references word for word, or quotes words no reference covers. This check
  can't fail after a successful fill-in, and it stays as an independent
  guard (`QuoteVerifier` re-checks both afterwards);
- a claim the chunk doesn't state (see the grounding check below);
- from an embedding or NLI provider, a result count that differs from the
  input count, vectors of zero or differing length, or a non-finite number
  ([`embed_checked`](../crates/podling-core/src/plugin/embedding.rs),
  [`score_checked`](../crates/podling-core/src/plugin/nli.rs)).

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

## The five bump rules

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
5. **You changed an embedding or NLI provider's behaviour.** Rule 3 applies to
   them too: `EmbeddingProvider::fingerprint()` and `NliProvider::fingerprint()`
   are in both grounding stages' cache keys. The fakes carry a `version` field to
   bump; `OpenAiEmbeddings` has the base URL, model and a request version;
   `CrossEncoderNli` has a BLAKE3 hash of its three model files and a version.
   The stages' thresholds are in their fingerprints as well, so changing one
   invalidates the cache on its own.

## Plugins

Plugins are trait objects, because the episode file chooses them at run time.
The factories in [`plugin/mod.rs`](../crates/podling-core/src/plugin/mod.rs)
are plain `match`es. Adding a plugin takes two steps: add a variant to the
config enum in [`episode.rs`](../crates/podling-types/src/episode.rs), then
add one arm to the factory.

| Kind | Trait | Implementations |
|---|---|---|
| Provider (LLM) | `LlmProvider` | `FakeLlm`: deterministic, offline. `OpenAiCompat`: any OpenAI chat-completions server (see below). |
| Provider (embeddings) | `EmbeddingProvider` | `FakeEmbedding`: BLAKE3-hashed bag of content words, 256 dimensions, so cosine measures shared words. `OpenAiEmbeddings`: `POST {base_url}/embeddings` (Ollama `nomic-embed-text`), 64 texts per request, on the same transport as `OpenAiCompat`. |
| Provider (NLI) | `NliProvider` | `FakeNli`: one-way word containment, plus a changed number read as contradiction. `CrossEncoderNli`: `cross-encoder/nli-deberta-v3-base` (Apache-2.0) run natively with candle on the CPU, loaded from a local directory on first use. |
| Source connector | `SourceConnector` | `LocalFilesConnector`: `.md`/`.txt` in one directory, symlinks confined to the root, 10 MiB cap. The locator is `<root as written in the episode>/<file name>`, so same-named files in different roots get distinct ids. |
| Analyser | `Analyser` | `QuoteVerifier`: every quote matches its source span, the turn speaks it verbatim, and no other quoted span of three or more words appears in a turn |

Deferred to later phases:
- Non-OpenAI-compatible LLM protocols, streaming, token budgeting. Until then
  `WriteScript` logs a warning when its input is over 24 KiB, because a small server
  context window truncates it silently; raise the server's context (for Ollama,
  `OLLAMA_CONTEXT_LENGTH`).
- TTS and ASR provider traits.
- MCP source connectors.
- PDF ingestion (Docling / pdfium).
- A Contested-claim adjudicator: the next phase.
- Replacing the lexical grounding check in `extract_claims` with an NLI check
  (see the grounding check below).

## The OpenAI-compatible provider

The key, URL, limits and retry rules below live in one shared `Transport`
([`plugin/http.rs`](../crates/podling-core/src/plugin/http.rs)), which the
embeddings client uses too; its config errors name its own section
(`embedding.base_url`, `embedding.api_key_env`).

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
| Errors | `CoreError::Provider` carries a `ProviderFailure` kind (`Http(status)`, `Unreachable`, `TimedOut`, `Other`); `CoreError::provider()` finds it and the failing plugin through stage wrappers. The CLI picks its fix hint from the kind, never from the message wording, and takes the URL, model and key variable from `[embedding]` when the plugin is `open_ai_compat_embeddings`, otherwise from `[llm]`. |
| Observability | One `tracing` span per request with the model, elapsed ms, attempts and token usage. Never the key or the prompt text. |
| TLS | rustls with the bundled web PKI roots, no OpenSSL. The added licences are permissive (Apache-2.0/MIT/ISC/BSD-3/CDLA-Permissive-2.0). |

**Sentence-addressed quotes.** For the script, each chunk is shown to the model as
numbered sentences (`text::sentences`, counting from 0), and the model answers with
`QuoteRef { chunk, sentence }`. Models count sentences far more reliably than bytes.
The stage looks the chunk up, takes the sentence's span, and calls
`Quote::from_document`. The invariant is unchanged: the model points and the code copies.

The same holds for the spoken text. In a turn's `text` the model writes `{{quote:N}}`
where the N-th entry of that turn's `quotes` (counting from 0) is spoken, and
`fill_quote_placeholders` ([`text.rs`](../crates/podling-core/src/text.rs)) replaces it with
the copied sentence in curly quotation marks. An 8B model paraphrased a sentence it had
just been shown, even when the retry quoted it back, so asking the model to retype a quote
was the flaw. Details:
- The typed-quotation check runs on the model's raw text, before filling in, so a quotation
  in the result can only have come from a placeholder.
- Straight or curly marks the model puts around a placeholder are dropped, so they are not
  doubled.
- The filled-in text is built in one pass. A source sentence that itself contains
  `{{quote:0}}` is inserted as it is and never filled in again.
- Curly marks are used because `“` closes only on `”`: a straight `"` inside a source
  sentence can't pair with a mark elsewhere in the turn.
- A placeholder may appear more than once, and gives the same words each time.
- The model's own text must have no stray or unclosed quotation mark: an opening `"` or `“`
  that never closes, or a `”` with nothing open. `quotations` finds nothing after an
  unmatched opener, so one stray mark would hide a typed quotation from this check and from
  `QuoteVerifier`. A mark inside a source sentence is allowed, since only the model's words
  are checked. Single quotes and `«…»`/`„…“` are not scanned.
- Model text the rejection repeats back is capped at 80 characters, since it goes into the
  retry's instructions.

**Prompt injection.** Source text can't be sanitised, so it is contained instead.
The instructions call ledger and source text untrusted data, and that text reaches the
model only inside the JSON input, never in the instructions. Whatever the model
returns is checked afterwards. Quotes come from the source, citations must be ledger
ids, and `QuoteVerifier` re-checks every quote. It also reports an error for any
quoted span (straight or curly marks, three or more words) in a turn that none of the
turn's quote refs covers, so invented words can't pass as a quotation by skipping
the ref. A successful injection can therefore change wording, but it can't invent a
quote or a citation without the run reporting it.

**Grounding check (a stopgap).** A claim becomes evidence that its chunk supports it,
so an invented claim would otherwise look SingleSource or even Corroborated. Claim
extraction therefore rejects a reply containing a claim whose content words (three or
more characters, or any number, minus stop words) are less than 60% present in the
chunk, or whose numbers aren't all present. A claim may also use words from its document's
title and the chunk's heading path (`Document::title`, `Chunk::heading_path`), because
extraction rule 2 has the model replace references such as "the site" with names, and a
name can appear only in a heading. Those title and heading words don't count toward the
share, though: the 60% is taken over the claim's other content words, and a claim made only
of title or heading words is rejected. Otherwise an invented claim that names the topic
would get those matches for free. A number may come from the chunk, the title or a heading. The model
itself is still shown only the chunk text. The model gets one retry with the reason,
then the run fails naming the chunk. The check is lexical. It catches invention and
knowledge pulled from the model's memory, and tolerates paraphrase. It does not catch
a subtle distortion made with the passage's own words. The NLI provider could (does
the chunk entail the claim?), and wiring it into this check is a follow-up.

**The script sees a compact ledger.** The script request carries each claim's id,
text and status only
([`LedgerClaim`](../crates/podling-core/src/plugin/llm.rs)), never its evidence. An
8B model cited a chunk id from a merged claim's evidence as a claim, twice.

## Grounding with embeddings and NLI

Extraction keys claims by their exact text, so two sources stating one fact in
different words give two SingleSource claims, and nothing can contradict anything.
Two optional stages fix that. Both are pure producers of *evidence*: status still
comes only from `classify()`, and no LLM is involved.

**Evidence audit.** `Evidence` has an optional `basis`
([`claim.rs`](../crates/podling-types/src/claim.rs)). It is absent for plain
extraction, so artifacts written without these stages are unchanged:
- `Merged { wording, entailment_pm }`: this evidence came from a claim worded
  `wording`, merged in because the two entail each other.
- `Nli { premise, similarity_pm, entailment_pm, contradiction_pm }`: `premise` is
  the span of the chunk's document that was judged against the claim.

Scores are `PerMille` integers (0–1000), rounded once. That keeps `Evidence: Eq + Ord`
and canonical JSON, and every threshold is compared against the rounded number, so
the stored score is the one that decided.

**[`cluster_claims`](../crates/podling-core/src/stages/cluster_claims.rs)** merges paraphrases:
1. Each claim nominates up to 8 partners with embedding cosine ≥ 0.80. Similarity
   only nominates; it never merges.
2. The number veto: a pair whose number sets differ ("1907" vs "1908") is dropped
   before the model sees it.
3. NLI entailment must be ≥ 0.900 **in both directions**. A claim that adds detail
   entails the shorter one, but not the reverse, so it stays separate (and the
   stance stage can still corroborate it).
4. Linkage is complete: a claim joins a cluster only if it is equivalent to every
   member, so A≈B and B≈C never chain A to C.

A cluster keeps the wording of its lowest-`ClaimId` member. Work runs in `ClaimId`
order, so the result doesn't depend on input order.

**[`score_stances`](../crates/podling-core/src/stages/score_stances.rs)** reads each claim
against other groups' sources. Premises are windows of one or two consecutive sentences,
well under DeBERTa's 512 tokens. For each claim, only windows from groups that have no
evidence on it yet are candidates; the 4 most similar, at cosine ≥ 0.30, are scored:
- entailment ≥ 0.800: `Supports`;
- else contradiction ≥ 0.950 **and** cosine ≥ 0.60: `Contradicts`. NLI models over-call
  contradiction between sentences that merely share a topic (0.903 for "Kulik reached
  the site in 1927" against "No impact crater was found"), hence the high bar and the
  same-subject requirement;
- else nothing.

A chunk gives a claim at most one piece of evidence, from its most decisive window, and
entailment wins over contradiction, so a chunk is never both for and against.

**Cost bound.** At most 4 NLI pairs per claim for stances, plus 2 × 8 per claim for
clustering, and one embedding per claim and per window. Both stages are cached by
content, so a rerun costs nothing, and a fully cached run never loads the NLI model.

**Resources.** The NLI model runs on the CPU (about 55–60 ms per pair), so the 8 GB GPU
stays free for the LLM and the embedder. `CrossEncoderNli` checks and fingerprints its
files when built, and loads the weights on the first `score` (a `OnceCell`). The pipeline
drops both providers after `score_stances`, before the script stage. Weights load from
`model.safetensors` only; `pytorch_model.bin`, a pickle, is never read.

**Config.** `[embedding]` and `[nli]` are both set or both absent; one without the other
is a `Config` error before any stage runs. A relative `model_dir` resolves against the
episode file's directory, and a missing model file is one error line naming the
`hf download` command.

## Why a claim ledger, not debating agents

Agents that "argue it out" produce outcomes that depend on prompts and
sampling, and you can't audit them afterwards. The ledger makes trust
**deterministic and inspectable**:
- A claim's status is a pure function of its evidence, via
  [`classify`](../crates/podling-types/src/ledger.rs).
- It counts distinct *independence groups*, not documents. Syndicated copies of one report
  therefore never look like corroboration.
- An LLM is needed only where judgement really is required: extracting
  claims, writing the script, and (later) adjudicating Contested claims. The NLI
  model is not a judge of status either: it produces scored evidence, and fixed
  thresholds turn scores into stances.

The same reasoning applies to quotes. The model can only *point* at a sentence (`QuoteRef`) and
mark where it goes in the spoken text (`{{quote:N}}`).
[`Quote::from_document`](../crates/podling-types/src/quote.rs) copies the words
from the source, and the script stage puts them in the text. A model therefore cannot put words
in a source's mouth: the script stage rejects quoted words it typed itself, and
`QuoteVerifier` flags them if they get through anyway.
