# Architecture

> **Status:** current as of 2026-10-07. It covers Phase 1 (core contracts), the
> `/scrutinise` fixes, Phase 2 (the OpenAI-compatible LLM provider) and its
> scrutinise fixes (unreferenced-quotation check, claim grounding, script input
> size warning), `{{quote:N}}` placeholders in script turns, claim grounding that
> can name things from the title and headings, Phase 3 (embeddings and NLI
> in the claim ledger), Phase 4 (the Contested-claim adjudicator) and Phase 5
> (episode audio: TTS, speech-recognition checks, assembly) with its
> `/scrutinise` fixes (backchannel-aware speech checks, a weights- and
> adapter-aware TTS cache key, a voice licence allow-list, and stopping the
> sidecar's whole process tree). Phase 5 was built before Phase 4. It also
> covers stance precision (the two-way check on numeric contradictions in
> `score_stances`, VERSION 8, measured on a labelled pair set).

This document describes the state after Phases 4 and 5. Where the design
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
 [ground_claims] ─► Grounded       evidence its own chunk doesn't entail (NLI) dropped and counted
 [cluster_claims] ► Vec<Claim>     paraphrases merged: mutual NLI entailment, equal numbers
 [score_stances] ─► Vec<Claim>     other groups' sentences checked by NLI: Supports / Contradicts
 ledger ─────────► Ledger          classify(): status from distinct independence groups
 adjudicate ─────► Verdicts        one LLM call per Contested claim: which side the sources
                                   favour, or Unresolved; none when nothing is Contested
 script ─────────► Script          LLM sees each claim's id, text, status and verdict, and
                                   numbered sources of numbered sentences; returns
                                   QuoteRef { source, sentence } and writes {{quote:N}} in the turn text where the quote goes;
                                   Quote::from_document copies the words and the stage fills the
                                   placeholder in; every citation must name a claim in the ledger
 analyse ────────► AnalysisReport  opt-in analysers, e.g. quote_verifier
 ─ only with [tts] ─────────────────────────────────────────────────────────────
 plan_chunks ────► Vec<PlannedChunk>  whole beats (dialogue model) or one turn each (per-turn model)
 synthesize_chunk ► ChunkResult       one cache entry per chunk take; audio in the blob store
 transcribe_chunk ► Transcript        Whisper on the CPU; WER and verbatim quotes decide the take
 assemble ───────► episode.wav        gaps, crossfades, overlays, −16 LUFS, −1 dBTP; audio.json
                                      lists every chunk, its check and each voice's licence
```

[`pipeline::run`](../crates/podling-core/src/pipeline.rs) calls the stages in
order. The three bracketed ones run only when the episode has both `[embedding]`
and `[nli]` (see [Grounding with embeddings and NLI](#grounding-with-embeddings-and-nli));
without them nothing can be Contested, so `adjudicate` makes no call and writes an
empty `verdicts.json`, and the other artifacts' bodies are byte for byte what they were
before Phase 3 (only the envelope's `schema_version` has moved on). The test
`without_nli_the_cache_keys_are_unchanged`
([`tests/pipeline.rs`](../crates/podling-core/tests/pipeline.rs)) pins the stages'
cache keys, so only a deliberate bump moves one. Each call goes through
[`stage::cached`](../crates/podling-core/src/stage.rs), which opens a
`tracing` span `stage{id, version}`, logs `cache_hit` and `elapsed_ms`, and
adds a `StageRecord` to the `RunReport`. Every artifact is written as
`<out>/<kind>.json` inside an `Envelope { schema_version, kind, body }`.
The audio stages run only when the episode has `[tts]` (see
[Episode audio](#episode-audio)); without it every text artifact is the same
as before Phase 5 apart from `schema_version`.

Provider output is never trusted. The following are `InvalidProviderOutput` errors:
- malformed JSON;
- a quote that names a source number past the end of `sources` or a sentence
  the source doesn't have (or that doesn't resolve in its document);
- a citation of a claim id that isn't in the ledger;
- a script in which no turn cites a Contested claim that has a verdict (the
  message names the claim, so the retry can add it);
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

All three LLM stages call the model through
[`complete_validated`](../crates/podling-core/src/plugin/llm.rs). If a reply fails
those checks, the model is asked again with every rejection so far listed after the
instructions, each cut to 500 characters (`reason_excerpt`), since a reason can quote
part of the reply. Extraction and adjudication get two attempts (`DEFAULT_ATTEMPTS`)
per chunk or Contested claim; the script gets three (`SCRIPT_ATTEMPTS`), because
live, llama3.1:8b often fixed the rejected mistake on a retry and made a new one.
The last failure is the error. Every request is also capped through
`CompletionRequest::max_tokens` (`MAX_CLAIMS_TOKENS` 2048, `MAX_SCRIPT_TOKENS` 8192,
`MAX_VERDICT_TOKENS` 512; the provider sends the lower of that and the episode's
`max_output_tokens`): live, llama3.1:8b in JSON mode sometimes never stops, and a
cut-off reply is just another rejection. The adjudicator alone turns that error into a
verdict instead of failing (see [Adjudicating Contested claims](#adjudicating-contested-claims)). Transport failures are not retried there, because the provider
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
   [`PROMPT_VERSION`](../crates/podling-core/src/plugin/llm.rs). `extract_claims`
   and `script` put it in their config fingerprint next to the instruction text.
   The adjudicator has its own `ADJUDICATE_PROMPT_VERSION`, so changing its prompt
   doesn't re-run claim extraction.
5. **You changed an embedding or NLI provider's behaviour.** Rule 3 applies to
   them too: `EmbeddingProvider::fingerprint()` and `NliProvider::fingerprint()`
   are in all three grounding stages' cache keys (`ground_claims`, `cluster_claims`,
   `score_stances`). The fakes carry a `version` field to
   bump; `OpenAiEmbeddings` has the base URL, model and a request version;
   `CrossEncoderNli` has a BLAKE3 hash of its three model files and a version.
   The stages' thresholds are in their fingerprints as well, so changing one
   invalidates the cache on its own. The same goes for **TTS and ASR**:
   `TtsProvider::fingerprint()` is the `synthesize_chunk` config, and
   `AsrProvider::fingerprint()` the `transcribe_chunk` config. `SidecarTts`
   takes protocol, backend, model, weights and adapter version from the
   worker's `/health` (never the profile name, so renaming a profile keeps the
   cache). `weights` is the hub snapshot's commit, or a `sha256:` over a
   `--model-dir`'s weight and config files; change what a worker adapter asks
   the model to say and bump its `ADAPTER_VERSION`. `CandleWhisper` has a BLAKE3 hash of its
   weights, its decoding thresholds and a version. The fakes carry a `version`.

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
| Provider (TTS) | `TtsProvider` (`&mut self`) | `FakeTts`: sine tones, a pitch per speaker and a length per word, so the audio path runs offline. `SidecarTts`: a Python worker process (Qwen3-TTS 1.7B Base by default) started from a user-level profile; see [Episode audio](#episode-audio). |
| Provider (ASR) | `AsrProvider` (`&mut self`) | `FakeAsr`: hears exactly what the script says; tests can make it mishear the first n calls. `CandleWhisper`: `openai/whisper-base.en` (MIT) with candle on the CPU, 30 s windows decoded in turn with the temperature fallback. |
| Analyser | `Analyser` | `QuoteVerifier`: every quote matches its source span, the turn speaks it verbatim, and no other quoted span of three or more words appears in a turn. `UncitedFigures`: warns on a turn that states a number or a year with no citation. |

Deferred to later phases:
- Non-OpenAI-compatible LLM protocols, streaming, token budgeting. Until then
  `WriteScript` logs a warning when its input is over 24 KiB, because a small server
  context window truncates it silently; raise the server's context (for Ollama,
  `OLLAMA_CONTEXT_LENGTH`).
- The Dia2 dialogue adapter in the TTS worker. Not needed for now: in the
  2026-10-06 listening pass it sounded near identical to per-turn Qwen (the
  planner and ASR turn spans already handle a multi-speaker backend).
- Pronunciation hints and expressive delivery: the Qwen 1.7B Base model is
  voice-clone only, so the adapter drops each turn's `emotion`.
- Synthesising chunks in parallel.
- MCP source connectors.
- PDF ingestion (Docling / pdfium).

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
| Retries | A 429 or 5xx is retried twice (0.5 s, then 1 s). A timeout, a 4xx or a transport error is not. Separately, `complete_validated` re-asks when a reply fails validation (once, or twice for the script). |
| Errors | `CoreError::Provider` carries a `ProviderFailure` kind (`Http(status)`, `Unreachable`, `TimedOut`, `CutOff`, `Other`; the LLM stages retry a `CutOff` reply as a rejection); `CoreError::provider()` finds it and the failing plugin through stage wrappers. The CLI picks its fix hint from the kind, never from the message wording, and takes the URL, model and key variable from `[embedding]` when the plugin is `open_ai_compat_embeddings`, otherwise from `[llm]`. |
| Observability | One `tracing` span per request with the model, elapsed ms, attempts and token usage. Never the key or the prompt text. |
| TLS | rustls with the bundled web PKI roots, no OpenSSL. The added licences are permissive (Apache-2.0/MIT/ISC/BSD-3/CDLA-Permissive-2.0). |

**Sentence-addressed quotes.** For the script, each chunk is shown to the model as
a numbered source (`source`, its position in the chunk list) of numbered sentences
(`text::sentences`), both counting from 0, and the model answers with
`QuoteRef { source, sentence }`. Models count sentences far more reliably than bytes.
The model sees no chunk id: llama3.1:8b, shown chunk ids, cited one as a claim.
A sentence also lists the quotations inside it (`quoted`, the spans
`text::quotation_ranges` finds), and a reference may add `part` to quote only one
of them: live, the model kept typing a lookout's words that sit inside a longer
sentence, since it had no way to point at them. A typed quotation that is one of
these parts is rejected with the reference to use instead. `text::sentences` does
not end a sentence at `."`, so a chunk with quoted speech can be one long sentence.
The stage takes the chunk at that position, takes the sentence's span, and calls
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
a subtle distortion made with the passage's own words. With `[embedding]` and `[nli]`
set, the `ground_claims` stage catches those: it keeps a claim only where the NLI model
finds that its own chunk entails it (see
[Grounding with embeddings and NLI](#grounding-with-embeddings-and-nli)). The lexical
check stays in front of it either way, unchanged, as the exact-number gate.

**The script sees a compact ledger.** The script request carries each claim's id,
text and status only
([`LedgerClaim`](../crates/podling-core/src/plugin/llm.rs)), never its evidence. An
8B model cited a chunk id from a merged claim's evidence as a claim, twice.

## Grounding with embeddings and NLI

Extraction keys claims by their exact text, so two sources stating one fact in
different words give two SingleSource claims, and nothing can contradict anything.
Its lexical grounding check also misses a distortion made from the chunk's own words.
Three optional stages fix that. They only add or remove *evidence*: status still
comes only from `classify()`, and no LLM is involved.

**[`ground_claims`](../crates/podling-core/src/stages/ground_claims.rs)** runs first, on
extraction's output. For each piece of evidence it asks whether the claim's own chunk
entails the claim, and drops the evidence if not. A claim left with no evidence is
dropped. This catches "Kulik led the expedition" from a chunk saying he *joined* it,
which passes the lexical check because it shares most of its words with the chunk.
- The premise is the best window of one or two consecutive sentences of the chunk,
  not the whole chunk. A chunk at the 800-word cap is about 1000–1600 DeBERTa tokens,
  over its 512, and even a short chunk fails: a faithful paraphrase of one sentence of
  a four-sentence chunk scored 0.000 entailment against the chunk and 0.998 against
  the sentence. The 4 windows most similar to the claim by embedding are scored.
- No window is longer than 120 words (`MAX_WINDOW_WORDS` in
  [`windows.rs`](../crates/podling-core/src/stages/windows.rs)). A longer sentence, such
  as a list with no full stops, is split into overlapping 120-word slices. Without that,
  DeBERTa cut the premise off at 512 tokens, and a faithful claim about item 117 of a
  long list scored 0.617 where one about item 1 scored 0.965.
- Each premise starts with the document title and the chunk's headings
  (`"<title>. <heading>. <window>"`), because extraction rule 2 has the model name
  what a heading names. Without that prefix such claims scored 0.000; with it, 0.997.
- Entailment ≥ 0.800 (`GROUND_ENTAIL_PM`, separate from the stance stage's equal
  `SUPPORT_ENTAIL_PM`) keeps the evidence. On the Tunguska sources faithful claims
  scored 0.971 or more and distortions 0.003 or less.
- A rejection is dropped and counted, never retried or fatal. Each one, with its
  claim, chunk, best score and premise span, is part of the stage's cached output.
  The pipeline logs them, also on a cache hit, and `RunReport::grounding` carries
  the counts, which the CLI prints as `grounding: N claim(s) dropped, M evidence item(s) rejected`.
- Known misses: a dropped hedge ("my shirt almost burned" → "the shirt burned", 0.994)
  and a figure moved within the chunk (0.966). The lexical check stays in front as the
  exact-number gate and rejects claims made only of title or heading words, which NLI
  scores as entailed (0.996).

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
at most 120 words, well under DeBERTa's 512 tokens. For each claim, only windows from groups that have no
evidence on it yet are candidates; the 4 most similar, at cosine ≥ 0.30, are scored:
- entailment ≥ 0.800: `Supports`;
- else contradiction ≥ 0.950 **and** cosine ≥ 0.60: `Contradicts`. NLI models over-call
  contradiction between sentences that merely share a topic (0.991 for the 1927
  expedition window below), hence the high bar and the similarity floor. When claim and
  premise both hold a number, the contradiction must also hold **both ways**: the claim,
  read as the premise, must contradict at ≥ 0.950 one of the window's numbered
  sentences or, when the window also has a sentence without a number, the whole window
  (`reverse_hypotheses`; scored only for candidates that would otherwise contradict, in
  one extra batch). NLI models over-call a number that only shares a topic one way:
  "The vessel was provided with lifeboats for 1,176 persons." against "From these boats
  he took on board 712 persons" scores 0.997 forward but 0.010 back, while "706 persons
  were saved." scores 0.997 / 0.995. A number about something else fails the same way:
  in "The explosion was heard hundreds of kilometres away. Kulik's expedition reached the
  site in 1927." the 1927 dates the expedition, and "The explosion happened in June
  1908." scores 0.991 forward but 0.080 back. Reading the whole window lets a
  contradiction stated without a number count: "Kulik reached the site in 1927."
  against "Kulik never reached the site. The expedition set off in 1927." scores 1.000
  both ways. The cost: a count the model reads as a subset ("80 million trees" against
  "8 million fir trunks", 0.000 back) is no longer a contradiction. Number filtering
  rests on the NLI model alone. VERSIONs 3–7 also refused a premise that named the
  claim's subject only in sentences without a number (word overlap, with a pronoun
  carry-over); VERSION 8 drops that rule, because the two-way check refuses the same
  pairs and the subject rule also refused t03;
- else nothing.

**Measured precision.** `tests/stance_precision.rs` runs the rule on 85 hand-labelled
pairs scored once by the real models, both ways (`tests/fixtures/stance_pairs/`); the
test pins the current rule's counts and false-positive ids exactly, so any rule change
must update them on purpose, and
`cargo test -p podling-core --test stance_precision -- --nocapture report` prints the
before/after table. VERSION 8: supports precision 100% / recall 73.7%, contradicts
93.1% / 96.4%. The false positives are n35 and n36 ("Kulik reached the site in 1927."
against "Kulik studied meteorites in Petrograd. It became Leningrad in 1924."), which
the model calls a contradiction both ways (≥ 0.997 forward, 0.998 back). The miss is c18, the subset
count above. The VERSION 2 rule (thresholds only) gives contradicts 80.0% / 100%, with
seven false positives. VERSION 7 (the subject rule plus a two-way check against the
numbered sentences only) had 92.9% / 92.9%, also missing t03. VERSION 6 (the subject
rule with no two-way check) had 86.7% / 100% on the first 82 pairs, with false positives
n34 n35 n36 n37. A set this small shows the rule fits it, not that it generalises; see
[the plan's report](plans/stance-whole-window.md#report).

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

## Adjudicating Contested claims

A Contested claim has evidence from at least one independence group against it.
[`adjudicate`](../crates/podling-core/src/stages/adjudicate.rs) asks the LLM which
side the sources favour, so the script can explain the disagreement instead of
only reporting it. It writes one [`Verdict`](../crates/podling-types/src/verdict.rs)
per Contested claim to `verdicts.json`:
- `favours`: `supporting`, `contradicting` or `unresolved`. The prompt makes
  `unresolved` the default and forbids outside knowledge.
- `explanation`: one or two plain sentences, at most 600 characters, with no
  quotation marks, so quoted words still come only from source spans.
- `cites`: the evidence the verdict rests on, as `EvidenceRef { chunk, stance,
  premise }`. The model sees the evidence numbered, with each passage (the NLI
  premise, the merged wording, or else the chunk) and its source's title, and
  cites by number. The stage checks every number and requires a cite from each
  side the evidence has.
- `fallback`: set only when the stage gave up on the model (below).

**Status is untouched.** The verdict sits next to the ledger; `ClaimStatus` still
comes only from `classify()`, and a claim the sources favour is still Contested.

**Cost bound.** One request per Contested claim, plus at most one retry when the
reply is rejected. Each reply is capped at `MAX_VERDICT_TOKENS` (512, or the
episode's `max_output_tokens` if lower) through `CompletionRequest::max_tokens`:
llama3.1:8b in JSON mode once kept writing past 13,000 tokens, and a cut-off reply
takes the usual retry and fallback. With no Contested claims (every run without `[nli]`) there is no
request at all. Only the Contested claims and their passages are in the cache key,
so editing anything else leaves the verdicts cached.

**Fallback.** A reply that is still rejected after the retry (bad JSON, an unknown
number, a missing side, a quotation mark, a favoured side with no cite) becomes an
`Unresolved` verdict citing the first piece of evidence on each side, with the
rejection reason in `fallback`, cut to 500 characters. The stage never picks a side the model didn't
argue. A transport failure (the server down, a timeout) fails the stage instead,
so it is never cached as a verdict.

**In the script.** The script request's ledger entry for a judged claim carries
`verdict: { favours, explanation }`, without the evidence references (whose chunk
ids a small model once mistook for claim ids). The prompt says to give both
accounts, say which side the sources favour or that it is unresolved, explain
why, and never state either side as settled. Every judged claim must be cited by
some turn; a script that leaves one out is rejected naming it (a live script
once dropped both of its judged claims).

## Episode audio

With `[tts]` (which needs `[asr]` and `[[cast]]`; `check_audio` in
[`plugin/mod.rs`](../crates/podling-core/src/plugin/mod.rs) rejects any other
mix), `pipeline::run` goes on after the analysis report. Everything here is
checked before the first stage runs: the voice clips, the sidecar profile, the
Whisper files and `[mix]`. The model processes start only when they are needed.

**One GPU model at a time.** The 8 GB card cannot hold the LLM and the TTS
model together, so:
1. With `unload_after = true` on `[llm]`/`[embedding]`, Podling asks Ollama to
   free each model straight after its last stage
   ([`plugin/ollama.rs`](../crates/podling-core/src/plugin/ollama.rs);
   Ollama's native `keep_alive: 0`). A failed unload is a warning.
2. The TTS provider is built just before synthesis and dropped just after, in
   one block of `AudioPlan::make`
   ([`pipeline.rs`](../crates/podling-core/src/pipeline.rs)). Dropping a
   `SidecarTts` stops its worker process, the only reliable way to give the GPU
   memory back.
3. Whisper runs on the CPU, so it can check each chunk straight after it is made
   without competing for the card.

**The sidecar.** [`plugin/sidecar.rs`](../crates/podling-core/src/plugin/sidecar.rs)
starts a worker; [`plugin/sidecar_tts.rs`](../crates/podling-core/src/plugin/sidecar_tts.rs)
speaks to it.
- *What runs.* The episode's `[tts] sidecar = "qwen"` names a profile. The program
  and its arguments come only from the user-level `sidecars.toml`
  (`$XDG_CONFIG_HOME/podling/` or `~/.config/podling/`, or `--sidecars`), never
  from the episode file, which is meant to be shareable. It runs from an argv
  (`std::process::Command`, no shell), with `--port 0 --run-dir <dir>` appended.
- *Start-up.* The worker binds `127.0.0.1` and prints one JSON line,
  `{"listening": "127.0.0.1:<port>", "protocol": 1}`; Podling waits up to 60 s
  for it, then checks `GET /v1/podling/health` (protocol, backend, model,
  weights, adapter, capabilities). Stderr goes to `<run dir>/sidecar.log`, whose tail
  every start-up or transport error quotes.
- *Requests.* `POST /v1/podling/synthesize` names the reference clips and an
  output path inside the run directory Podling created; the worker writes a WAV
  there and answers with its sample rate and turn spans. Audio moves through
  files, so the HTTP bodies stay small. The full protocol is in
  [`sidecars/tts/README.md`](../sidecars/tts/README.md).
- *Shutdown.* `uv run` starts the model as a child, so stopping only the
  worker could leave the GPU held. When the worker reports ready, Podling pins
  each of its descendants with a pidfd, which keeps naming that process even
  after its parent dies and a pid is reused (`Descendants` in `sidecar.rs`).
  `Drop` adds a fresh scan when the worker still runs, sends SIGTERM to the
  worker and every pinned process, waits up to 5 s (liveness is read from the
  pidfds), then kills and reaps what is left; this also happens when the
  worker has already died and left a child behind. The worker stays in
  Podling's process group, so Ctrl-C still reaches it. A process started after
  ready by a worker that then dies, or one that detaches before the ready
  scan, is caught only by the worker's own parent watch
  ([`server.py`](../sidecars/tts/podling_tts/server.py) `watch_parent`). The
  run directory is deleted after it.

**Chunks.** [`plan_chunks`](../crates/podling-core/src/stages/plan_chunks.rs)
reads the backend's `TtsCapabilities`. A per-turn model (`multi_speaker: false`,
Qwen) gets one turn per chunk; a dialogue model gets whole beats up to
`max_chunk_secs`, never splitting a beat. Every chunk is conditioned on the
pinned reference clip of each speaker, never on the previous chunk's output
alone, so errors can't compound.

**Checks and takes.** [`synthesize_script`](../crates/podling-core/src/stages/synthesize.rs)
makes each chunk, and [`verify_audio`](../crates/podling-core/src/stages/verify_audio.rs)
transcribes it. A take passes when its word error rate against what the chunk says is
at most `max_wer_pm` (the turn text plus any backchannel its own speaker says
in line before or after it, `SpokenTurn::said` in
[`plugin/tts.rs`](../crates/podling-core/src/plugin/tts.rs)) and every quote is heard word for word (after
normalising case, punctuation and numbers). Banter beats get `takes` takes and
keep the best passing one. A failing chunk is made again with a new seed up to
`max_retries` times; if it still fails it becomes an `Error` finding in the
analysis report and the episode is still assembled, as with a misquote. A seed
is derived, never random (`BLAKE3(chunk ‖ take)`), so a rerun asks for the same
takes and finds them cached. Backchannels said over another speaker's turn are
synthesised as separate short clips for the second track (`synthesize_overlays`).

**Blob cache.** Each take is one `synthesize_chunk` entry; its key covers the
turns, the voices' file hashes, any context audio, the take and the TTS
fingerprint, so editing one turn re-synthesises one chunk. The entry holds a
`ChunkResult` that names its audio by BLAKE3 hash; the bytes live in a
content-addressed `BlobStore` (`<cache>/blobs/`, or `<out>/.blobs/` with
`--no-cache`). An entry whose blob is missing is a **miss**, so clearing blobs
can never leave a dangling reference. Transcripts are cached the same way
(`transcribe_chunk`, keyed by the blob, the expected text and the ASR fingerprint).

**Assembly.** [`assemble`](../crates/podling-core/src/stages/assemble.rs)
resamples each chunk to 48 kHz mono (`rubato`), matches every chunk's loudness
to the median, trims each turn's silence and lays the turns out with the gap
its `pace` asks for (`[mix] gaps_ms`; `Interrupt` overlaps with an equal-power
crossfade). Overlays go on a second track at −6 dB. The episode is then
normalised to −16 LUFS integrated with a −1 dBTP true-peak limiter, measured
with `ebur128`, and written as 16-bit `episode.wav`. `[mix] encode = "opus"`
or `"mp3"` adds a copy made by `ffmpeg` (argv, no shell; a missing `ffmpeg` is a
`Config` error). `audio.json` records every chunk's seed, take and check, the
episode's loudness and length, and each voice's licence. A clip's licence
must be one of `VOICE_LICENCES` (`CC0-1.0`, `CC-BY-3.0`, `CC-BY-4.0`;
[`episode.rs`](../crates/podling-types/src/episode.rs)); any other is refused
when the episode is read.

## Why a claim ledger, not debating agents

Agents that "argue it out" produce outcomes that depend on prompts and
sampling, and you can't audit them afterwards. The ledger makes trust
**deterministic and inspectable**:
- A claim's status is a pure function of its evidence, via
  [`classify`](../crates/podling-types/src/ledger.rs).
- It counts distinct *independence groups*, not documents. Syndicated copies of one report
  therefore never look like corroboration.
- An LLM is needed only where judgement really is required: extracting
  claims, writing the script, and adjudicating Contested claims, where it adds a
  verdict but never changes the status. The NLI
  model is not a judge of status either: it produces scored evidence, and fixed
  thresholds turn scores into stances.

The same reasoning applies to quotes. The model can only *point* at a sentence (`QuoteRef`) and
mark where it goes in the spoken text (`{{quote:N}}`).
[`Quote::from_document`](../crates/podling-types/src/quote.rs) copies the words
from the source, and the script stage puts them in the text. A model therefore cannot put words
in a source's mouth: the script stage rejects quoted words it typed itself, and
`QuoteVerifier` flags them if they get through anyway.
