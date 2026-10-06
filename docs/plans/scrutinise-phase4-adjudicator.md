---
goal: A rejected adjudicator reply leaves a bounded reason in verdicts.json, and a reply cut off at a token cap names the cap that cut it.
status: COMPLETE
coverage:
  correctness: 1.1, 1.2, 2.1, 2.2
  tests: 1.3, 2.1, 2.2, 2.4, 2.5
  caching: 1.3 (Adjudicate::VERSION bump); round 2 N/A(no stored output of a successful run changes; only error and log text)
  contracts: N/A(Verdict, QuoteRef, PlaceholderError and ProviderFailure are unchanged)
  security: N/A(no auth, secrets or new I/O; the bound limits what model text is persisted)
  docs: 2.1 (docs/architecture.md if it quotes the cut-off message)
  migration: N/A(no persisted store beyond the content-addressed cache)
  observability: 2.3
blind_rederivation: skipped(round 2: decomposition fixed by the verified scrutineer findings; one unit, no contract change)
steps:
  - id: fallback-reason-cap
    scope_id: fallback-reason-cap
    project: .
    depends_on: []
    scope:
      read: [crates/podling-core/src/plugin/llm.rs, crates/podling-core/src/plugin/mod.rs, crates/podling-core/src/stages/adjudicate.rs]
      docs: [docs/plans/scrutinise-phase4-adjudicator.md]
      write: [crates/podling-core/src/plugin/llm.rs, crates/podling-core/src/plugin/mod.rs, crates/podling-core/src/stages/adjudicate.rs, docs/plans/TRACKER.md, docs/plans/scrutinise-phase4-adjudicator.md]
  - id: live-fix-polish
    scope_id: live-fix-polish
    project: .
    depends_on: []
    scope:
      read: [crates/podling-core/src/plugin/openai.rs, crates/podling-core/src/plugin/llm.rs, crates/podling-core/src/stages/script.rs, crates/podling-core/src/stages/adjudicate.rs, crates/podling-core/src/text.rs, crates/podling-core/tests/openai_provider.rs]
      docs: [docs/plans/scrutinise-phase4-adjudicator.md, docs/architecture.md]
      write: [crates/podling-core/src/plugin/openai.rs, crates/podling-core/src/plugin/llm.rs, crates/podling-core/src/stages/script.rs, crates/podling-core/src/stages/adjudicate.rs, crates/podling-core/src/text.rs, crates/podling-core/tests/openai_provider.rs, docs/architecture.md, docs/plans/scrutinise-phase4-adjudicator.md]
---

# Scrutinise fixes: phase4-adjudicator

Source: `/craftsman:scrutinise phase4-adjudicator` over `f144672..feat/p4-live-and-docs`, one
verified finding.

## Scope Steps (executable core)

### Step 1 — fallback-reason-cap (., rust, normal)
Tooling: implementer · gates code-reviewer · checks cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace

- [x] 1.1 Name the 500-character cap in `plugin/llm.rs` (for example a `pub fn reason_excerpt(reason: &str) -> String` with its constant), and use it for `complete_validated`'s retry excerpt and for the reason `Adjudicate` stores in `fallback`. When it truncates, it ends with `…`. → accept: one definition of the cap; the retry prompt is unchanged for reasons under 500 chars.
- [x] 1.2 Correct the comment at `adjudicate.rs:202`: the reason may echo part of the model's reply, and it is bounded. Bump `Adjudicate::VERSION` 1→2 with a version note. → accept: the comment matches the code.
- [x] 1.3 Unit test: a provider whose reply has a 2000-character unknown `favours` value, on both attempts, gives a fallback verdict whose `fallback` is at most the cap plus the marker, and the stage makes two calls. → accept: passes; the workspace gates pass.

### Step 2 — live-fix-polish (., rust, normal)
Source: the second `/craftsman:scrutinise`, isolated, over `1dcb270..93f0d2a` (the live fixes). 0 Critical, 1 Warning, 4 Suggestions; none disputed.
Tooling: implementer · gates code-reviewer · checks cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace

- [x] 2.1 (F2, Warning) In `plugin/openai.rs`, compute the effective output cap once, with a helper used by both `request_body` and `complete`. The helper should say whether the episode's `llm.max_output_tokens` or the stage's request cap bound. Then make the `CutOff` message name that cap and its value, e.g. "…cut off at the stage's cap of 512 tokens" or "…cut off at llm.max_output_tokens = 256; raise it". If `docs/architecture.md` quotes the old wording, update it. → accept: two `tests/openai_provider.rs` tests check the message: one where the stage cap binds, one where the episode cap binds. Advice to raise `llm.max_output_tokens` appears only when the episode cap bound.
- [x] 2.2 (F3, Suggestion) In `stages/script.rs`, make `quoted_part_hint` match a listed quotation by prefix when the typed echo ends in `…` (it was cut at `MAX_ECHO_CHARS`); keep exact matching otherwise. `PlaceholderError` is unchanged. → accept: a unit test with a quotation longer than `MAX_ECHO_CHARS` gets the `part` hint.
- [x] 2.3 (F4, Suggestion) In `plugin/openai.rs`, emit a `tracing::warn!` before returning `CutOff`, with the same fields as the success `info!` (elapsed_ms, attempts, prompt_tokens, completion_tokens). → accept: code-reviewer confirms; the gates pass.
- [x] 2.4 (F5, Suggestion) Add a `build_script` test on `lookout()` whose turn quotes `{"source": 0, "sentence": 1, "part": 0}`. → accept: the turn text contains “Iceberg right ahead.” and the quote's span is the inner quotation's span in the chunk.
- [x] 2.5 (F6, Suggestion) Add tests with an LLM that cuts off on every call. → accept: `complete_validated` returns `InvalidProviderOutput` containing "after 2 attempts" after exactly 2 calls, and `Adjudicate::run` returns an `Unresolved` fallback verdict, not an error.

Design note, out of scope: no episode setting can raise the per-stage caps (`MAX_VERDICT_TOKENS`, `MAX_CLAIMS_TOKENS`, `MAX_SCRIPT_TOKENS`). A model that counts thinking tokens in `completion_tokens` would hit them. Add an override if such a model is ever used.

No `Stage::VERSION` bump in round 2. A successful run's stored output does not change; only error, hint and log text change.

## Verification background
- **F1 (Correctness, Minor).** `adjudicate.rs:200-204` turns `InvalidProviderOutput { message }` into `fallback(claim, message)` verbatim. `build_verdict` passes serde_json errors through, and an unknown enum variant's message quotes the model's string whole ("unknown variant `…`"). The comment says the reason "names ids and numbers, never source text", which isn't guaranteed. `complete_validated` already caps the same reason at 500 chars for the retry prompt (`llm.rs:221`).
- The claim id in a mismatch message is a validated `ContentHash` (`crates/podling-types/src/ids.rs`), so it is not a vector.

CONSUMERS:
- `complete_validated` retry excerpt: `crates/podling-core/src/plugin/llm.rs:221` (only site).
- `Verdict.fallback`: `crates/podling-core/src/stages/adjudicate.rs:278-294` (`fallback`), tests in the same file; `crates/podling-core/tests/pipeline.rs` asserts `fallback() == None` only.

CONSUMERS (round 2):
- The `CutOff` message: built only at `crates/podling-core/src/plugin/openai.rs:169-179`. `crates/podling-core/tests/openai_provider.rs:301-315` asserts "2048 tokens". `complete_validated_with` (`crates/podling-core/src/plugin/llm.rs:315`) appends it to a rejection reason. The CLI hint path never sees it (commands.rs maps `CutOff => None`).
- `request_body`'s cap: `crates/podling-core/src/plugin/openai.rs:114-134`. Unit test `the_lower_of_the_episode_and_request_token_caps_is_sent`.
- `quoted_part_hint`: one caller, `crates/podling-core/src/stages/script.rs:240`. `PlaceholderError::Typed` is built at `crates/podling-core/src/text.rs:275` from `echo` (`text.rs:130`).

## Risks
None beyond cache invalidation of stored verdicts, which the version bump makes explicit.
