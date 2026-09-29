---
slug: scrutinise-phase2-llm-provider
goal: A model can no longer smuggle an invented quotation or an ungrounded claim into an episode unnoticed, and the live-model test and oversized-input failures say plainly what to do.
classification: in-scope   # findings on merged Phase 2 range 162204c..2a621f8; F5 deferred (PENDING follow-up)
tracker_rows: [TRACKER#scrutinise-phase2-llm-provider/1, TRACKER#scrutinise-phase2-llm-provider/2, TRACKER#scrutinise-phase2-llm-provider/3, TRACKER#scrutinise-phase2-llm-provider/4, TRACKER#scrutinise-phase2-llm-provider/5]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(root-only agent mode; decomposition came pre-derived from /scrutinise, self-checked against the consumer grep)
coverage:
  contract:      1.3, 2.4 | N/A for units 3-4 (no shared type change). Stage::VERSION bumps are the cache contract; no exported type or schema changes.
  data:          N/A(no persistence/migration; stale cache entries are invalidated by the VERSION bumps in 1.3 and 2.4)
  config:        N/A(no new episode-file keys; the size threshold in 4.1 is a constant)
  security:      1.1, 2.1 (fabricated quote / ungrounded claim, both fail closed). Units 3-4 N/A(no auth/secret surface; 4.1 logs sizes only, never prompt text)
  tests:         1.2, 1.4, 2.2, 2.3, 3.1, 4.2
  observability: 4.1 (warn on oversized input). Units 1-3 N/A(errors surface as findings / InvalidProviderOutput)
  interface:     3.1, 3.2 (CLI test message and README). Units 1, 2, 4 N/A(no new CLI flag or output shape)
  docs:          1.5, 2.5, 3.2, 4.3
  rollback:      git revert per unit; VERSION bumps only invalidate cache, never corrupt it
units:
  - id: 1
    scope_id: quote-verifier
    project: .
    depends_on: []
    module: crates/podling-core/src/plugin/analyser.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/plugin/analyser.rs
        - crates/podling-core/src/stages/analyse.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-types/src/quote.rs
        - crates/podling-types/src/script.rs
      docs: [docs/architecture.md, .claude/CLAUDE.md]
      write:
        - crates/podling-core/src/plugin/analyser.rs
        - crates/podling-core/src/stages/analyse.rs
        - crates/podling-core/tests/pipeline.rs
        - docs/architecture.md
    tooling: { implementer: implementer, gates: [security-auditor], skills: [rust], guards: [cargo fmt, cargo clippy -D warnings, cargo test], mcp: [] }
  - id: 2
    scope_id: claim-grounding
    project: .
    depends_on: [1]
    module: crates/podling-core/src/stages/extract_claims.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/stages/extract_claims.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/llm/extract_claims.json
      docs: [docs/architecture.md, .claude/CLAUDE.md]
      write:
        - crates/podling-core/src/stages/extract_claims.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/llm/extract_claims.json
        - docs/architecture.md
    tooling: { implementer: implementer, gates: [security-auditor], skills: [rust], guards: [cargo fmt, cargo clippy -D warnings, cargo test], mcp: [] }
  - id: 3
    scope_id: live-test-guard
    project: .
    depends_on: [2]
    module: crates/podling-cli/tests/cli.rs
    language: rust
    security: normal
    scope:
      read: [crates/podling-cli/tests/cli.rs]
      docs: [README.md]
      write: [crates/podling-cli/tests/cli.rs, README.md]
    tooling: { implementer: implementer, gates: [], skills: [rust], guards: [cargo fmt, cargo clippy -D warnings, cargo test], mcp: [] }
  - id: 4
    scope_id: script-input-size
    project: .
    depends_on: [3]
    module: crates/podling-core/src/stages/script.rs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/openai.rs
      docs: [README.md, docs/architecture.md]
      write:
        - crates/podling-core/src/stages/script.rs
        - README.md
        - docs/architecture.md
    tooling: { implementer: implementer, gates: [], skills: [rust], guards: [cargo fmt, cargo clippy -D warnings, cargo test], mcp: [] }
---

# Plan: scrutinise phase2-llm-provider

## Outcome
A model can no longer put an invented quotation into a script, or an ungrounded claim into the ledger, without Podling flagging or rejecting it; `cargo test -- --ignored` no longer reports a pass for a live test that never ran; and an oversized script input warns instead of failing as confusing invalid JSON.

## Scope Steps (executable core)

### Step 1 — quote-verifier (., rust, high)
Tooling: implementer implementer · gates security-auditor · skills rust · guards fmt, clippy, test
Depends on: none
- [x] 1.1 Add to `QuoteVerifier::analyse` (`analyser.rs:33-56`) a per-turn scan for quoted spans in `turn.text`, straight (`"…"`) and curly (`“…”`), of at least 3 words; each span that is not equal to one of that turn's `quote.text()` values is an `Error` finding naming the turn and the span. Do not count these spans in `checked`. → accept: a turn whose text contains `He said "the sky was entirely on fire"` with no quote ref yields one Error finding; a turn whose quoted span equals its referenced quote yields none.
- [x] 1.2 Add unit tests in `analyser.rs`: unreferenced straight-quote span flagged; unreferenced curly-quote span flagged; span under 3 words (a scare quote such as `"so-called"`) not flagged; a referenced quote still passes; an unbalanced quote mark does not panic or flag. → accept: `cargo test -p podling-core analyser` passes and the new tests fail if 1.1 is reverted.
- [x] 1.3 Bump `Analyse::VERSION` 1→2 (`stages/analyse.rs:23`), because analyser logic changed (bump rule 2, `docs/architecture.md:101`). → accept: a run on a warm cache re-executes the analyse stage only.
- [x] 1.4 Add a pipeline test in `tests/pipeline.rs` with a canned provider (reuse the `Replay` pattern) whose script turn puts a fabricated 3+ word quotation in `turn.text` with no quote ref. → accept: the run reports `error_findings >= 1` and the finding names the fabricated span.
- [x] 1.5 Update the prompt-injection paragraph (`docs/architecture.md:157-162`) and the quote paragraph (`:180-182`): `QuoteVerifier` also flags any quoted span in a turn that no quote ref covers. → accept: the doc claim "can't invent a quote" matches what the analyser enforces; only lines about quotes changed.

### Step 2 — claim-grounding (., rust, high)
Tooling: implementer implementer · gates security-auditor · skills rust · guards fmt, clippy, test
Depends on: quote-verifier (both edit `docs/architecture.md` and `tests/pipeline.rs`)
- [x] 2.1 Add a pure helper, `is_grounded(claim: &str, chunk_text: &str) -> bool`, in `extract_claims.rs` (move to `plugin/llm.rs` only if another caller needs it): lower-case, split on non-alphanumerics, drop stop-words and tokens under 3 chars, then require at least 60% of the claim's content words to occur in the chunk's word set. A claim with no content words is not grounded. The 60% threshold is a named `const`. → accept: unit tests below pass; the helper is pure, with no I/O.
- [x] 2.2 Call it inside the `complete_validated` closure (`extract_claims.rs:67-70`): after parsing `ClaimsDraft`, return `Err("chunk <id>: claim not grounded in the text: <claim>")` for the first ungrounded non-empty claim. `complete_validated` then retries once with the reason and finally raises `InvalidProviderOutput` naming the chunk. → accept: a canned provider that always returns an invented claim makes `run` fail with `InvalidProviderOutput` mentioning the chunk id after exactly two calls.
- [x] 2.3 Add tests: fabricated claim rejected; a paraphrase with reordered words and a different tense accepted; a claim quoting the chunk accepted; empty or stop-word-only claim rejected; the FakeLlm output and the Tunguska replay fixture (`tests/fixtures/llm/extract_claims.json`) still pass without edits. If the fixture needs an edit, stop and re-scope. → accept: `cargo test --workspace` passes, including `replayed_model_output_gives_the_same_ledger_statuses_and_verbatim_quotes`.
- [x] 2.4 Bump `ExtractClaims::VERSION` 2→3 (`extract_claims.rs:41`). The instructions text may gain one line saying "only claims stated in the text", which changes the fingerprint too; if so, bump `PROMPT_VERSION` (`llm.rs:13`) 1→2 as well. → accept: `changing_the_model_invalidates_the_llm_stages_only` still passes.
- [x] 2.5 Document the check in `docs/architecture.md`, in the "Known limit" paragraph (`:164-166`) and the "Provider output is never trusted" list (`:49-53`), as a lexical stopgap until the NLI provider: it catches invention, not subtle distortion. → accept: the doc states the threshold, the stopgap status and that NLI is the real fix.

### Step 3 — live-test-guard (., rust, normal)
Tooling: implementer implementer · gates none · skills rust · guards fmt, clippy, test
Depends on: claim-grounding (README edit ordering)
- [x] 3.1 In `live_run_against_a_real_server` (`cli.rs:410-417`), replace the early `return` with a `panic!` whose message names `PODLING_LIVE_LLM_URL` and `PODLING_LIVE_LLM_MODEL` and gives an example command. The test stays `#[ignore]`, so a default `cargo test` is unchanged. → accept: `cargo test -p podling-cli -- --ignored live` with the vars unset fails with that message; `cargo test --workspace` still passes.
- [x] 3.2 Update `README.md:146-151` to say that the ignored test fails unless both variables are set, and that `PODLING_LIVE_LLM_KEY_ENV` is optional. → accept: the README command matches the test's actual behaviour.

### Step 4 — script-input-size (., rust, normal)
Tooling: implementer implementer · gates none · skills rust · guards fmt, clippy, test
Depends on: live-test-guard (README edit ordering)
- [x] 4.1 In `WriteScript::run` (`script.rs:58-73`), after building the input `json!`, compute its serialised byte length; if it exceeds a named `const LARGE_INPUT_BYTES` (start at 24 KiB, roughly 6k tokens, under Ollama's usual 4k-8k default context once instructions are counted), emit `tracing::warn!` with the byte size and a hint to raise the server context (`OLLAMA_CONTEXT_LENGTH`). Log sizes only, never text. → accept: the warn line fires for a large input and does not fire for the Tunguska example.
- [x] 4.2 Test the threshold by extracting the size check into a small pure fn (`fn is_large(input: &Value) -> bool`) and unit-testing both sides. → accept: `cargo test -p podling-core script` passes; the run still succeeds for oversized input, since this is a warning, not a limit.
- [x] 4.3 Document in `README.md` (after the "What the model can and can't do" paragraph, `:136-141`) and `docs/architecture.md` (deferred list, `:126-131`): the script call sends the whole ledger plus every chunk's sentences, a small context window truncates it silently, so raise it (e.g. `OLLAMA_CONTEXT_LENGTH=16384`) and expect a warning; token budgeting is still deferred. → accept: both docs mention the warning and the setting; the "Deferred" list still names token budgeting.

## Sequencing
1 → 2 → 3 → 4. Units 1 and 2 are the security-relevant fixes and go first; 1 and 2 share `docs/architecture.md` and `tests/pipeline.rs`, and 3 and 4 share `README.md`, so they run in series, not parallel. No CHANGELOG task: the repo has no `CHANGELOG.md` (the executor confirms before skipping).

## Verification background   (citations — for the reviewer, not the executor)
- QuoteVerifier only iterates `turn.quotes`, so unreferenced quotation marks pass — `crates/podling-core/src/plugin/analyser.rs:33-56`
- The script instructions rely on the model not quoting outside refs (rule 3) — `crates/podling-core/src/stages/script.rs:17`
- Claim evidence is added for any non-empty model claim with no grounding check — `crates/podling-core/src/stages/extract_claims.rs:67-88`
- The validator closure only parses JSON — `extract_claims.rs:67-70`
- The live test returns early when unset — `crates/podling-cli/tests/cli.rs:411-417`
- The script input is unbounded — `crates/podling-core/src/stages/script.rs:58-73`
- Bump rules — `docs/architecture.md:94-110`; the analyse stage `VERSION` is 1 — `crates/podling-core/src/stages/analyse.rs:23`

CONSUMERS:
- `QuoteVerifier` (behaviour change, no signature change) →
  - constructed in `crates/podling-core/src/plugin/mod.rs:60` (factory), `crates/podling-core/src/stages/analyse.rs:74` (test)
  - configured by `AnalyserConfig::QuoteVerifier {}` in `crates/podling-types/src/episode.rs:88` (unchanged)
  - exercised in `crates/podling-cli/tests/cli.rs:268` (episodes with `quote_verifier`) and `analyser.rs:114-143` (unit tests)
  - risk: the CLI success tests run FakeLlm scripts through the verifier, so 1.1 must not flag FakeLlm turns; the acceptance run is the full workspace suite.
- `ExtractClaims` (behaviour + VERSION) → `pipeline.rs` (calls the stage), `tests/pipeline.rs` (`Replay` provider, fixtures `tests/fixtures/llm/extract_claims.json`), `FakeLlm::extract_claims` in `llm.rs:151` (returns whole sentences, so it passes the check by construction).
- `PROMPT_VERSION` (only if 2.4 changes the instructions) → both LLM stages' `config_fingerprint` (`extract_claims.rs:49`, `script.rs:54`).
- No exported type, JSON Schema or `SCHEMA_VERSION` change, so no schema snapshot update and no CLI contract change.

## Risk & rollback
- 1.1 could flag legitimate scare quotes; the 3-word minimum and the exact-equality rule limit this, and the finding is an analyser Error so the run exits non-zero. Revert unit 1 if it proves noisy.
- 2.x could reject good paraphrases from a real model; the retry-with-reason gives one recovery, and the threshold is one `const`. Tune it against a real run after the live test works. Revert unit 2 if it blocks valid runs.
- VERSION bumps only invalidate cached stage outputs.

## Out of scope
- F5 (PENDING follow-up, unit 5): replace the substring matching in `crates/podling-cli/src/commands.rs:67-95` (`provider_hint`) with a typed provider error (status/kind) on `CoreError`. Deferred because two tests pin both ends of the wording today; promote it when a third message is added.
- Semantic claim clustering and NLI grounding (a later phase; 2.x is a stopgap).
- Token budgeting or chunked script generation (4.x only warns).
- Running the live test against a real model (needs a model pulled into the Ollama container; a user action).
