---
slug: story-driven-script
goal: Scripts follow a story arc (claims in source order, an arc prompt, topic as the angle) measured before and after on the local model, and an HTTP provider is local-only unless its section declares data_policy = "zero_retention".
idea: docs/ideas/story-driven-script.md
classification: in-scope   # docs/ideas/story-driven-script.md "Recommendations" 1, 2, 5 (topic), 7 + "Data privacy"; rules ARCH-STORY-01..11, ARCH-PRIVACY-01..08 (decided 2026-10-07)
tracker_rows: [TRACKER#story-driven-script/1, TRACKER#story-driven-script/2, TRACKER#story-driven-script/3, TRACKER#story-driven-script/4, TRACKER#story-driven-script/5, TRACKER#story-driven-script/6]
guards:
  plan_review: revise→applied (self-review per plan-reviewer brief, root-only mode)
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: Explore (diffed; see "Blind re-derivation")
  plan_strategist: option B (two tracks; story track ordered by evidence)
coverage:
  contract:      5.1, 5.2 (LlmConfig/EmbeddingConfig gain `data_policy`; every literal/destructure in CONSUMERS updated) · 3.1 (script request ledger order; `LedgerClaim` shape unchanged)
  data:          N/A(no persistence beyond the file cache; script key moves via SCRIPT_PROMPT_VERSION + WriteScript::VERSION 13; schema_version is header-only — cache.rs:121)
  config:        5.1 (`data_policy` in [llm]/[embedding]), 6.6 (examples/titanic/episode-together.toml), 2.1 (harness env vars)
  security:      6.1-6.5 (hosted base_url refused unless zero_retention; local = localhost/private IP literal only; redirects off; key never logged or fingerprinted)
  tests:         1.2, 2.1, 2.2, 3.1, 3.6, 3.7, 5.3, 5.4, 6.1-6.5, 6.7
  observability: 3.4 (debug claim order, info word ratio — ARCH-STORY-11), 6.3 (info section/local|hosted/policy — ARCH-PRIVACY-07)
  interface:     6.2 (refusal message names section + fix), 6.8 (README hosted-provider docs)
  docs:          3.8, 6.8, unit 4 tables
  rollback:      git revert per unit; prompt revert = script cache invalidation only; data_policy is optional (old episode files still parse)
units:
  - id: 1
    scope_id: sds-metrics
    project: .
    depends_on: []
    module: podling-core script_metrics
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/lib.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-types/src/script.rs
        - crates/podling-types/src/ledger.rs
        - docs/plans/story-driven-script.md
      docs:
        - docs/architecture/story.rules.md
        - docs/plans/story-driven-script.md
      write:
        - crates/podling-core/src/script_metrics.rs
        - crates/podling-core/src/lib.rs
    arch: [ARCH-STORY-06]
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: sds-harness-baseline
    project: .
    depends_on: [sds-metrics]
    module: podling-core script_eval_live + baseline table
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/script_metrics.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/ground_claims_live.rs
        - crates/podling-core/Cargo.toml
        - examples/titanic/episode-ollama.toml
        - examples/tunguska/episode-ollama.toml
        - docs/plans/story-driven-script.md
      docs:
        - docs/architecture/story.rules.md
        - docs/plans/story-driven-script.md
      write:
        - crates/podling-core/tests/script_eval_live.rs
        - docs/plans/story-driven-script.md
    arch: [ARCH-STORY-07, ARCH-STORY-08]
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: sds-arc-script
    project: .
    depends_on: [sds-harness-baseline]
    module: podling-core script stage (order, arc prompt, SCRIPT_PROMPT_VERSION)
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/stages/extract_claims.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/script_metrics.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/golden/script.json
        - crates/podling-core/tests/fixtures/golden/analysis.json
        - crates/podling-core/tests/fixtures/golden/ledger.json
        - crates/podling-core/tests/fixtures/llm/write_script.json
        - crates/podling-core/tests/audio_e2e.rs
        - README.md
        - docs/architecture.md
        - docs/plans/story-driven-script.md
      docs:
        - docs/architecture/story.rules.md
        - docs/architecture/speech.rules.md
        - docs/plans/story-driven-script.md
      write:
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/golden/script.json
        - crates/podling-core/tests/fixtures/golden/analysis.json
        - README.md
        - docs/architecture.md
    arch: [ARCH-STORY-01, ARCH-STORY-02, ARCH-STORY-03, ARCH-STORY-04, ARCH-STORY-05, ARCH-STORY-11, ARCH-SPEECH-04]   # SPEECH-04: quotes stay the original words; SPEECH-16 is superseded by STORY-02
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 4
    scope_id: sds-arc-measure
    project: .
    depends_on: [sds-arc-script, sds-transport-policy]
    module: arc table, topic overlap, ARCH-STORY-08 verdict, hosted table
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/tests/script_eval_live.rs
        - crates/podling-core/src/script_metrics.rs
        - examples/titanic/episode-together.toml
        - docs/plans/story-driven-script.md
      docs:
        - docs/architecture/story.rules.md
        - docs/architecture/privacy.rules.md
        - docs/plans/story-driven-script.md
      write:
        - docs/plans/story-driven-script.md
        # re-scoped: unit 2 merged this file before `cargo fmt` (edition 2024
        # import order); the one-line fmt fix lands here
        - crates/podling-core/tests/script_eval_live.rs
    arch: [ARCH-STORY-05, ARCH-STORY-08]
    tooling: { implementer: implementer, gates: [],
               skills: [], guards: [], mcp: [] }
  - id: 5
    scope_id: sds-data-policy-types
    project: .
    depends_on: []
    module: podling-types DataPolicy field + consumers + fingerprints
    language: rust
    security: high
    scope:
      read:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-types/tests/snapshots/schema_snapshot__episode.snap
        - crates/podling-core/src/plugin/openai.rs
        - crates/podling-core/src/plugin/openai_embeddings.rs
        - crates/podling-core/src/plugin/ollama.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/openai_embeddings.rs
        - crates/podling-core/tests/openai_provider.rs
        - crates/podling-cli/src/commands.rs
        - docs/plans/story-driven-script.md
      docs:
        - docs/architecture/privacy.rules.md
        - docs/architecture/speech.rules.md
        - docs/plans/story-driven-script.md
      write:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-types/tests/snapshots/schema_snapshot__episode.snap
        - crates/podling-core/src/plugin/openai.rs
        - crates/podling-core/src/plugin/openai_embeddings.rs
        - crates/podling-core/src/plugin/ollama.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/openai_embeddings.rs
        - crates/podling-core/tests/openai_provider.rs
    arch: [ARCH-PRIVACY-03, ARCH-PRIVACY-05, ARCH-SPEECH-07]
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 6
    scope_id: sds-transport-policy
    project: .
    depends_on: [sds-data-policy-types]
    module: podling-core http Transport local/hosted policy
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/plugin/http.rs
        - crates/podling-core/src/plugin/openai.rs
        - crates/podling-core/src/plugin/openai_embeddings.rs
        - crates/podling-core/src/plugin/ollama.rs
        - crates/podling-core/src/plugin/sidecar_tts.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/openai_provider.rs
        - crates/podling-core/tests/openai_embeddings.rs
        - crates/podling-core/src/plugin/sidecar.rs
        - crates/podling-types/src/episode.rs
        - examples/titanic/episode-ollama.toml
        - examples/tunguska/episode-ollama.toml
        - README.md
        - docs/architecture.md
        - docs/plans/story-driven-script.md
      docs:
        - docs/architecture/privacy.rules.md
        - docs/architecture/speech.rules.md
        - docs/plans/story-driven-script.md
      write:
        - crates/podling-core/src/plugin/http.rs
        - crates/podling-core/src/plugin/openai.rs
        - crates/podling-core/src/plugin/openai_embeddings.rs
        - crates/podling-core/src/plugin/ollama.rs
        - crates/podling-core/src/plugin/sidecar_tts.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/openai_provider.rs
        - crates/podling-core/tests/openai_embeddings.rs
        - examples/titanic/episode-together.toml
        - examples/tunguska/episode-ollama.toml
        - README.md
        - docs/architecture.md
    arch: [ARCH-PRIVACY-01, ARCH-PRIVACY-02, ARCH-PRIVACY-04, ARCH-PRIVACY-05, ARCH-PRIVACY-06, ARCH-PRIVACY-07, ARCH-PRIVACY-08, ARCH-SPEECH-03]
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: story-driven-script

## Outcome
Scripts follow a story arc — claims in source order, an arc prompt, `topic` as the angle — with
the change measured before and after on the local model; and an HTTP provider is local-only
unless its section declares `data_policy = "zero_retention"`.

No act writer: `act_plan.rs` is not built here (ARCH-STORY-08 gates it on the arc table, unit 4).

## Scope Steps (executable core)

Two independent tracks. **Story** (1 → 2 → 3 → 4) is ordered by evidence: the baseline must be
measured on today's unchanged request before unit 3 touches it. **Privacy** (5 → 6) changes
neither the script request nor any `FakeLlm` reply, so it runs beside it; unit 4 waits for 6
only for the hosted example.

### Step 1 — sds-metrics (., rust, normal)
Tooling: implementer · gates code-reviewer · guards cargo fmt/clippy/test
Depends on: none
- [ ] 1.1 Add `crates/podling-core/src/script_metrics.rs` (+ `pub mod script_metrics;` in `lib.rs`): a pure `ScriptMetrics::of(script: &Script, ledger: &Ledger, verdicts: &Verdicts, target_minutes: u16) -> ScriptMetrics` with `words`, `word_ratio` = words / (150 × target_minutes), `turns`, `quotes`, `citations`, `distinct_cited`, `usable_claims` (non-Unsupported), `coverage` = distinct_cited / usable, `judged_contested`, `judged_contested_cited`, `unknown_citations` (cited ids not in the ledger); plus `cited_claim_overlap(a: &BTreeSet<ClaimId>, b: &BTreeSet<ClaimId>) -> Overlap { shared, only_a, only_b, jaccard }`. Counts are `usize`, ratios `f64`; `word_ratio` is 0.0 when `target_minutes` is 0, `coverage` 0.0 when nothing is usable, and two empty sets have `jaccard` 1.0 (identical). No I/O, no provider → accept: `grep -E "LlmProvider|std::fs|std::io|DiskCache" crates/podling-core/src/script_metrics.rs` is empty.
- [ ] 1.2 Unit tests in the module over fixed scripts: word count and ratio (incl. `target_minutes` = 0 → ratio 0, no panic), quote/citation counts, distinct vs repeated citations, Unsupported excluded from `usable_claims`, a judged Contested claim cited vs not, an unknown citation counted, overlap of identical / disjoint / partial sets → accept: `cargo test -p podling-core script_metrics` passes in the default run (no `#[ignore]`).

### Step 2 — sds-harness-baseline (., rust, normal)
Tooling: implementer · gates code-reviewer · guards cargo fmt/clippy/test
Depends on: sds-metrics
- [ ] 2.1 Add `crates/podling-core/tests/script_eval_live.rs`: `#[ignore]` test `script_eval` that **panics** (never returns early) when `PODLING_SCRIPT_EVAL_RUN` (a `podling run --out` dir) is unset; loads its `ledger`, `verdicts`, `chunks`, `documents` and `episode` artifacts; builds the LLM from `PODLING_SCRIPT_EVAL_EPISODE` (an episode toml; optional, default the run's own `episode.json` `[llm]`) via `plugin::build_llm`; wraps it in an attempt-counting `LlmProvider` that also counts "which is not in the ledger" rejections (from retry instructions and the final error); calls `WriteScript::run` directly `PODLING_SCRIPT_EVAL_N` times (default 5), `topic` from `PODLING_SCRIPT_EVAL_TOPIC` or the episode; records a run whose `WriteScript::run` returns an error (rejected after 3 attempts, timeout, unreachable) as a failed row with the error's kind, so the N-run table always completes; prints a per-run row (ok/fail, attempts, unknown-citation rejections, words, word ratio, turns, quotes, citations, coverage, judged Contested cited) and a summary (eventual pass rate, first-try pass rate, mean attempts, mean word ratio over passing runs, total unknown-citation rejections) built from `script_metrics` → accept: no `DiskCache` in the file; `cargo test -p podling-core --test script_eval_live -- --ignored` without the env var fails with the panic message.
- [ ] 2.2 Second `#[ignore]` test `topic_overlap` in the same file: needs `PODLING_SCRIPT_EVAL_RUN` and `PODLING_SCRIPT_EVAL_TOPICS="<topic A>|<topic B>"` (panics without either), writes `PODLING_SCRIPT_EVAL_N` scripts (default 1) per topic on the same ledger, prints each topic's distinct cited claim set and `cited_claim_overlap` (shared / only A / only B / Jaccard) → accept: test exists, panics without env, compiles under `cargo test --no-run`.
- [ ] 2.3 Produce saved runs on the local Ollama with today's code: `podling run --episode <tmp copy of examples/{titanic,tunguska}/episode-ollama.toml> --out $CLAUDE_JOB_DIR/tmp/runs/{titanic,tunguska}` (NLI weights symlinked, large `timeout_secs`) → accept: each out dir holds `ledger.json`, `verdicts.json`, `chunks.json`, `documents.json`, `episode.json`.
- [ ] 2.4 Run `script_eval` with N=5 on each saved run **before any unit-3 change**, and record the **Baseline table** below (commit sha, model llama3.1:8b, per-run rows + summary for Titanic and Tunguska) → accept: the section holds real numbers and the commit it ran on.

### Step 3 — sds-arc-script (., rust, normal)
Tooling: implementer · gates code-reviewer · guards cargo fmt/clippy/test
Depends on: sds-harness-baseline (2.4 recorded)
- [ ] 3.1 Order the request's `ledger` view by first appearance in `ScriptInput.chunks` (lowest index of any evidence chunk, ties by claim id; a claim with no evidence chunk in `chunks` goes last, by id) in a function in `script.rs`; `LedgerClaim::from_ledger` and the `Ledger` artifact keep claim-id order (ARCH-STORY-01) → accept: unit test with claims whose id order differs from source order sees the request in source order; golden `ledger.json` byte-identical.
- [ ] 3.2 Add `pub const SCRIPT_PROMPT_VERSION: u32 = 1` (history comment) in `llm.rs`, re-exported from `plugin/mod.rs`; `WriteScript::config_fingerprint` names `"script_prompt_version": SCRIPT_PROMPT_VERSION` and no longer `PROMPT_VERSION`; the `PROMPT_VERSION` doc says it keys `extract_claims` only (ARCH-STORY-02) → accept: `script.rs` names only `SCRIPT_PROMPT_VERSION`; the `NO_NLI_KEYS` `extract_claims` entry is unchanged.
- [ ] 3.3 Arc prompt: add to `INSTRUCTIONS` the story shape — a cold open (a concrete scene or quote), one through-line taken from `topic` (the episode's angle), anecdote followed by reflection, the judged Contested claims as the turning point told as a dispute (never settled), a closing reflection; in `AUDIO_RULES` rule 9 replace "covering every usable claim" with "the claims that serve the through-line", keeping the length target. Rules 1–5 text unchanged; no `EpisodeSpec` field (ARCH-STORY-04, ARCH-STORY-05) → accept: `git diff` shows no change to rules 1–5; `check_judged_claims_are_cited` still called in `build_script`.
- [ ] 3.4 Logging (ARCH-STORY-11): `tracing::debug!` of the request's claim id order; `tracing::info!` of each accepted script's word ratio (from `script_metrics`) → accept: both calls present in `script.rs`.
- [ ] 3.5 `WriteScript::VERSION` 12 → 13 with a history comment (ARCH-STORY-03). `FakeLlm`'s reply to a given request is unchanged, so its fingerprint version stays 8 (say so in the commit) → accept: version line and comment in the same commit as 3.1/3.3.
- [ ] 3.6 Move the `NO_NLI_KEYS` `script` key in `tests/pipeline.rs` (and the `analyse` key only if the script bytes change; `extract_claims` never), updating the pin's history comment; if the reorder changes the fake script, regenerate `tests/fixtures/golden/script.json` (and `analysis.json` if it follows) and add the reason to the golden test's doc comment, test code unchanged. `audio_e2e.rs` indexes `FakeLlm` turns (`turns[1]`): it must pass unchanged — if it would not, stop and report rather than edit it → accept: `cargo test --quiet` passes; `audio_e2e.rs` has no diff; the `tests/pipeline.rs` diff is only key pins and doc comments.
- [ ] 3.7 Existing grounding tests unchanged and passing → accept: `git diff origin/main -- crates/podling-core/src/stages/{ground_claims,score_stances,cluster_claims,extract_claims,adjudicate,ledger}.rs crates/podling-core/tests/{stance_precision,ground_claims_live}.rs` is empty; no existing `#[test]` body in `script.rs` changed (only added tests); `cargo test --quiet` exit 0.
- [ ] 3.8 `docs/architecture.md` script stage (artifact flow, the bump rules, "the script sees a compact ledger"): request order, arc prompt, `topic` as the angle, `SCRIPT_PROMPT_VERSION` (and `PROMPT_VERSION` = extraction only); README: `topic` is the episode's angle → accept: both docs name each.

### Step 4 — sds-arc-measure (., docs only)
Depends on: sds-arc-script, sds-transport-policy
- [x] 4.1 Run `script_eval` N=5 on the same two saved runs with the unit-3 code; record the **Arc table** below in the baseline's shape → accept: section holds real numbers and the commit.
- [x] 4.2 Run `topic_overlap` on the Titanic run with `"how the 1912 Titanic inquiries disagreed|the ship's construction, its lifeboats and the engineering failure"`; record the overlap → accept: "Topic overlap" section filled.
- [x] 4.3 Apply ARCH-STORY-08: arc vs baseline on mean word ratio < 0.70, eventual pass rate below baseline, more unknown-citation rejections than baseline; record the verdict → accept: "ARCH-STORY-08 verdict" section filled; if the gate trips, stop with BLOCKED ON USER (whether to build the act writer); no `act_plan.rs` in the tree either way.
- [x] 4.4 Hosted table: if `TOGETHER_API_KEY` is set, run `script_eval` with `PODLING_SCRIPT_EVAL_EPISODE=examples/titanic/episode-together.toml` on baseline and arc code and record it; else record "pending: TOGETHER_API_KEY not set" → accept: the "Hosted table" section says one or the other.

### Step 5 — sds-data-policy-types (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor · guards cargo fmt/clippy/test
Depends on: none
- [ ] 5.1 `episode.rs`: `pub enum DataPolicy { ZeroRetention }` (snake_case → `zero_retention`, `JsonSchema`, doc comment: declare it only for an endpoint that neither trains on nor retains inputs); `data_policy: Option<DataPolicy>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` on `LlmConfig::OpenAiCompat` and `EmbeddingConfig::OpenAiCompat`; export from `lib.rs`; `SCHEMA_VERSION` 8 → 9 and its pin in `schema_snapshot.rs` (ARCH-PRIVACY-03, ARCH-SPEECH-07) → accept: the episode snapshot diff shows only the new optional field and enum; version incremented.
- [ ] 5.2 Update every consumer in CONSUMERS (literals add `data_policy: None` or the tested value; full destructures add the field) → accept: `cargo build --workspace --all-targets` passes.
- [ ] 5.3 Roundtrip tests: `data_policy = "zero_retention"` parses in both sections; an unknown value is rejected; absent → `None`, re-serialised without the key → accept: tests in `roundtrip.rs` pass.
- [ ] 5.4 Fingerprints: `OpenAiCompat` and `OpenAiEmbeddings` include `data_policy`, never the key (ARCH-PRIVACY-05) → accept: tests assert the field changes the fingerprint and the key never appears in it.

### Step 6 — sds-transport-policy (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor · guards cargo fmt/clippy/test
Depends on: sds-data-policy-types
- [ ] 6.1 `http.rs`: host classification — local only for `localhost` and loopback/private IP literals (127.0.0.0/8, ::1, 10/8, 172.16/12, 192.168/16, fc00::/7), parsed with `std::net` from the URL `validate_base_url` already accepted; everything else hosted (ARCH-PRIVACY-01). The plain-http API-key warning uses the same classification, replacing `is_loopback`, so there is one notion of local → accept: unit tests for `localhost`, `127.5.0.1`, `[::1]`, `10.0.0.5`, `192.168.1.2`, `[fd00::1]` (local) and `localhost.example.com`, `127.0.0.1.nip.io`, `192.169.0.1`, `172.32.0.1`, `api.together.xyz` (hosted).
- [ ] 6.2 `TransportConfig` gains `data_policy: Option<DataPolicy>`; `Transport::new` refuses a hosted URL without `ZeroRetention` with a `CoreError::Config` naming `<section>.base_url`, the host and the fix; `ureq::Agent` built only here (ARCH-PRIVACY-02) → accept: tests for refused, declared and local; `grep -rn "ureq::Agent" crates/podling-core/src` shows only `http.rs`.
- [ ] 6.3 `tracing::info!` once in `Transport::new`: section, `local`/`hosted`, declared policy, never the key (ARCH-PRIVACY-07); `max_redirects(0)` and the credentials-in-URL rejection kept, each with a test (ARCH-PRIVACY-06) → accept: the log call and both tests present.
- [ ] 6.4 Plumbing: `openai.rs` and `openai_embeddings.rs` pass their section's `data_policy`; `OllamaUnload::new` takes the policy and gets its section's; `sidecar_tts.rs` passes `None` (ARCH-PRIVACY-04) → accept: a test that a hosted sidecar-style URL with no policy is refused by `Transport::new`; an unload transport test with a declared policy.
- [ ] 6.5 Refused before any stage: test in `tests/pipeline.rs` that `pipeline::run` with `[llm] base_url = "https://api.together.xyz/v1"` and no `data_policy` returns a config error and writes no artifact and no cache entry; same for a hosted `[embedding]` → accept: tests pass.
- [ ] 6.6 Add `examples/titanic/episode-together.toml`: `episode-ollama.toml` with `[llm]` on `https://api.together.xyz/v1`, an open-weight model, `api_key_env = "TOGETHER_API_KEY"`, `data_policy = "zero_retention"`, and a comment on Together's zero-retention setting; `[embedding]` stays local → accept: a test parses it as `EpisodeSpec` with `data_policy = Some(ZeroRetention)`.
- [ ] 6.7 Fix unit tests whose dummy hosts become hosted (`http://h/v1`, `http://x/v1`, `https://api.example/v1`, …); confirm `sidecar.rs` hands out a loopback `base_url`, by using `127.0.0.1` or declaring the policy, no assertion weakened → accept: `cargo test --quiet` exit 0.
- [ ] 6.8 ARCH-PRIVACY-08: Podling sends no host-specific privacy control today, so nothing to translate; `openai.rs` `PROMPT_VERSION` unchanged; say so in the commit. README: the hosted example, the `[llm]` and `[embedding]` key tables and the Privacy paragraph show `data_policy = "zero_retention"`, the local-by-default rule and the refusal; `docs/architecture.md` provider/transport section and fingerprint list name the policy; the tunguska example's hosted comment updated → accept: README names `data_policy`; no new privacy field beside it.

## Sequencing
Story: 1 → 2 (baseline recorded on today's request) → 3 → 4. Privacy: 5 → 6. Unit 4 also needs 6
for the hosted example. The tracks share only `tests/pipeline.rs` (3.6 moves the script key pin,
6.5 adds a test); whichever lands second merges on top of the first.

## Baseline table
Today's prompt (`WriteScript::VERSION` 12, `PROMPT_VERSION` 8), measured 2026-10-07 at commit
`ce8e757` with `script_eval` (N=5 per episode). Model llama3.1:8b (Q4_K_M, digest `46e0c10c…`,
the same weights as the Ollama container) on a native GPU Ollama at `127.0.0.1:11435`
(`OLLAMA_CONTEXT_LENGTH=16384`), temperature 0.2. Saved runs: `podling run` of
`examples/{titanic,tunguska}/episode-ollama.toml` (local `[llm]`/`[embedding]`, NLI on) at the
same commit. Titanic ledger: 10 claims (9 SingleSource, 1 Contested with a verdict). Tunguska
ledger: 7 claims (6 SingleSource, 1 Corroborated), no verdicts. Target 5 minutes (750 words).

**Titanic** — topic "how the 1912 Titanic inquiries disagreed"

| run | ok | attempts | unknown-citation rejections | secs | words | word ratio | turns | quotes | citations | coverage | judged cited |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | no | 3 | 0 | 128 | — | — | — | — | — | — | — (turn 3: source 2 has 1 sentences, so sentence 1 does not exist) |
| 2 | no | 3 | 0 | 96 | — | — | — | — | — | — | — (turn 3: the text puts "Iceberg right ahead." in quotation marks) |
| 3 | yes | 2 | 0 | 91 | 280 | 0.37 | 10 | 1 | 10 | 9/10 (0.90) | 1/1 |
| 4 | yes | 2 | 0 | 69 | 131 | 0.17 | 7 | 1 | 7 | 7/10 (0.70) | 1/1 |
| 5 | yes | 3 | 0 | 128 | 245 | 0.33 | 11 | 1 | 11 | 9/10 (0.90) | 1/1 |

Summary: eventual pass **3/5**, first-try pass 0/5, mean attempts 2.60, mean word ratio
**0.29** (passing runs), mean quotes 1.0, mean coverage 0.83, unknown-citation rejections **0**,
judged Contested cited 3/3.

**Tunguska** — topic "the 1908 Tunguska explosion"

| run | ok | attempts | unknown-citation rejections | secs | words | word ratio | turns | quotes | citations | coverage | judged cited |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | yes | 1 | 0 | 14 | 37 | 0.05 | 2 | 0 | 2 | 2/7 (0.29) | 0/0 |
| 2 | no | 3 | 0 | 94 | — | — | — | — | — | — | — (turn 2: source 1 has 1 sentences, so sentence 1 does not exist) |
| 3 | yes | 2 | 0 | 84 | 115 | 0.15 | 6 | 3 | 7 | 7/7 (1.00) | 0/0 |
| 4 | yes | 1 | 0 | 11 | 37 | 0.05 | 2 | 0 | 2 | 2/7 (0.29) | 0/0 |
| 5 | yes | 1 | 0 | 10 | 37 | 0.05 | 2 | 0 | 2 | 2/7 (0.29) | 0/0 |

Summary: eventual pass **4/5**, first-try pass 3/5, mean attempts 1.60, mean word ratio
**0.08** (passing runs), mean quotes 0.8, mean coverage 0.46, unknown-citation rejections **0**,
judged Contested cited 0/0.

Note for ARCH-STORY-08: today's prompt is already far below the 0.70 word-ratio line on both
episodes (the Tunguska example has no `[tts]`, so it gets no length rule at all — rule 9 is in
`AUDIO_RULES` only).

## Arc table
The arc prompt (`WriteScript::VERSION` 13, `SCRIPT_PROMPT_VERSION` 1, claims in source order),
measured 2026-10-07 at commit `a13b3d0` (unit 3 merged on top of origin/main) with `script_eval`
(N=5 per episode), on the same saved runs, model, server and settings as the baseline.

**Titanic** — topic "how the 1912 Titanic inquiries disagreed"

| run | ok | attempts | unknown-citation rejections | secs | words | word ratio | turns | quotes | citations | coverage | judged cited |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | no | 3 | 0 | 134 | — | — | — | — | — | — | — (turn 2: the text puts "Iceberg right ahead." in quotation marks) |
| 2 | yes | 3 | 0 | 124 | 124 | 0.17 | 6 | 2 | 6 | 3/10 (0.30) | 1/1 |
| 3 | no | 3 | 0 | 37 | — | — | — | — | — | — | — (no turn cites the contested claim) |
| 4 | no | 3 | 0 | 479 | — | — | — | — | — | — | — (output cut off at the stage's token cap of 819…) |
| 5 | yes | 1 | 0 | 45 | 416 | 0.55 | 8 | 9 | 10 | 8/10 (0.80) | 1/1 |

Summary: eventual pass **2/5**, first-try pass 1/5, mean attempts 2.60, mean word ratio
**0.36** (passing runs), mean quotes 5.5, mean coverage 0.55, unknown-citation rejections **0**,
judged Contested cited 2/2.

**Tunguska** — topic "the 1908 Tunguska explosion"

| run | ok | attempts | unknown-citation rejections | secs | words | word ratio | turns | quotes | citations | coverage | judged cited |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | no | 3 | 0 | 95 | — | — | — | — | — | — | — (turn 4: uses {{quote:0}} but the turn has 0 quote references) |
| 2 | yes | 2 | 0 | 41 | 34 | 0.05 | 2 | 1 | 2 | 2/7 (0.29) | 0/0 |
| 3 | yes | 3 | 0 | 64 | 55 | 0.07 | 2 | 0 | 2 | 2/7 (0.29) | 0/0 |
| 4 | yes | 3 | 0 | 462 | 36 | 0.05 | 2 | 1 | 2 | 2/7 (0.29) | 0/0 |
| 5 | no | 3 | 0 | 73 | — | — | — | — | — | — | — (turn 5: sentence 0 of source 1 has 0 quoted parts) |

Summary: eventual pass **3/5**, first-try pass 0/5, mean attempts 2.80, mean word ratio
**0.06** (passing runs), mean quotes 0.7, mean coverage 0.29, unknown-citation rejections **0**,
judged Contested cited 0/0.

Baseline → arc: Titanic eventual pass 3/5 → 2/5, word ratio 0.29 → 0.36, quotes 1.0 → 5.5,
coverage 0.83 → 0.55; Tunguska eventual pass 4/5 → 3/5, word ratio 0.08 → 0.06, coverage
0.46 → 0.29. N=5 at temperature 0.2 is small: a one-run difference in pass rate is within noise,
but it is the gate the rule names.

## Topic overlap
`topic_overlap` on the saved Titanic run (10 ledger claims), arc prompt at `a13b3d0`, same model
and server. A first pass at 1 script per topic got no accepted script for either topic (both
failed validation after 3 attempts), so its overlap was empty; recorded here is the rerun at
`PODLING_SCRIPT_EVAL_N=3` scripts per topic (union of claims cited by the accepted scripts).

| topic | scripts accepted | distinct claims cited |
|---|---|---|
| A: how the 1912 Titanic inquiries disagreed | 2/3 (ratios 0.30, 0.32) | 10 |
| B: the ship's construction, its lifeboats and the engineering failure | 1/3 (ratio 1.16) | 5 |

Shared **5**, only A **5**, only B **0**, Jaccard **0.50**. The topic does steer selection: the
construction angle cited half the ledger, all of it also cited under the inquiry angle; the
inquiry angle cited everything. Small sample (3 accepted scripts in all).

## ARCH-STORY-08 verdict
**Gate tripped** (docs/architecture/story.rules.md:18), on two of its three conditions, on both
episodes:

| condition | Titanic | Tunguska | trips |
|---|---|---|---|
| arc mean word ratio < 0.70 | 0.36 | 0.06 | yes |
| arc eventual pass rate below baseline | 2/5 < 3/5 | 3/5 < 4/5 | yes |
| more unknown-citation rejections than baseline | 0 vs 0 | 0 vs 0 | no |

The rule therefore permits starting the act writer (ARCH-STORY-09/10); whether to build it is
the user's decision, so this plan stops here with BLOCKED ON USER. No `act_plan.rs` exists in
the tree. Observation for that decision: the arc prompt alone did not lengthen scripts (the 8B
model still writes 2-turn Tunguska scripts with no `[tts]` length rule), and its extra
instructions added failure modes (truncation at the token cap, a missed contested claim).

## Hosted table
Pending: `TOGETHER_API_KEY` not set in this environment, so the hosted run of
`examples/titanic/episode-together.toml` was not measured. To record it: set the key (and
Together's no-training / no-retention settings), then run `script_eval` with
`PODLING_SCRIPT_EVAL_EPISODE=examples/titanic/episode-together.toml` on the baseline and arc
commits.

## Notes from execution
- Unit 5 added `data_policy: None,` to `crates/podling-core/tests/stance_precision.rs` — a
  one-line consumer compile fix outside its listed write scope (noted in its commit).
- Unit 4 carries `cargo fmt`'s import-order fix to `tests/script_eval_live.rs`, which unit 2
  merged unformatted (re-scoped above).
- `audio_e2e::the_user_and_episode_lexicons_reach_the_worker_the_episode_winning` failed once
  ("Peer disconnected") under full-suite load during unit 6; it passed 3/3 alone and on the
  next full run. Treated as a load flake, not changed.

## Verification background   (citations — for the reviewer, not the executor)
- Script prompt, `PROMPT_VERSION` in the script fingerprint — `crates/podling-core/src/stages/script.rs:23-58`, `:102-107`.
- `PROMPT_VERSION` also keys `extract_claims` — `crates/podling-core/src/stages/extract_claims.rs:98`.
- The ledger view is built in ledger order — `crates/podling-core/src/plugin/llm.rs:131-149`.
- Retry instructions list each rejection — `crates/podling-core/src/plugin/llm.rs:305-348`.
- `FakeLlm` writes one turn per ledger entry in request order — `crates/podling-core/src/plugin/llm.rs:408-415`; fingerprint version 8 — `:544-554`.
- Cache pins — `crates/podling-core/tests/pipeline.rs:624-681`; golden artifacts — `:93-123`.
- `SCHEMA_VERSION` is in the cache header, not the key — `crates/podling-core/src/cache.rs:121`.
- Providers are built before any stage (`build_llm`, `build_grounding`) — `crates/podling-core/src/pipeline.rs:52,65,118`.
- Every HTTP provider goes through `Transport::new` — `crates/podling-core/src/plugin/http.rs:67`; callers `openai.rs:69`, `openai_embeddings.rs:61`, `ollama.rs:43`, `sidecar_tts.rs:155`.
- Today's loopback check, to be replaced — `crates/podling-core/src/plugin/http.rs:341`.
- Live-test pattern (panic, never skip) — `crates/podling-core/tests/ground_claims_live.rs:17-22`.
- A saved run's artifacts are `<kind>.json` in `--out` — `crates/podling-core/src/pipeline.rs:449`.

CONSUMERS:
- `LlmConfig::OpenAiCompat` (gains `data_policy`): `crates/podling-core/src/plugin/openai.rs:54` (destructure), `:271,365,379,383,400` (tests); `crates/podling-core/src/plugin/mod.rs:61` (`{ .. }`, unaffected); `crates/podling-core/src/plugin/ollama.rs:132` (test); `crates/podling-core/tests/openai_provider.rs:147`; `crates/podling-types/tests/roundtrip.rs:484,504`; `crates/podling-cli/src/commands.rs:111` (`..`, unaffected).
- `EmbeddingConfig::OpenAiCompat`: `crates/podling-core/src/plugin/openai_embeddings.rs:49` (destructure), `:200` (test); `crates/podling-core/src/plugin/mod.rs:158` (`{ .. }`); `crates/podling-core/src/plugin/ollama.rs:147` (test); `crates/podling-core/tests/openai_embeddings.rs:83,181`; `crates/podling-core/tests/stance_precision.rs:304`; `crates/podling-types/tests/roundtrip.rs:542`; `crates/podling-cli/src/commands.rs:101` (`..`).
- `TransportConfig` (gains `data_policy`): `openai.rs:69`, `openai_embeddings.rs:61`, `ollama.rs:43`, `sidecar_tts.rs:155`.
- `OllamaUnload::new` (gains the policy): `openai.rs:81`, `openai_embeddings.rs:76`, `ollama.rs` tests.
- Script request ledger order: `FakeLlm::write_script` (`llm.rs:375`) reads whatever order it gets → `tests/fixtures/golden/script.json`.
- `SCRIPT_PROMPT_VERSION`: new; read by `WriteScript::config_fingerprint` only.

## Blind re-derivation
A fresh read-only Explore pass listed the touched files from the raw request alone. Added from
its list: `golden/analysis.json` and the `analyse` key pin (follow the script), `audio_e2e.rs`
(indexes `FakeLlm` turns; must stay unchanged), `fixtures/llm/write_script.json` (read),
`sidecar.rs` (loopback `base_url`, read), `https://api.example/v1` test hosts, the
`docs/architecture.md` bump-rule/transport sections, and the README `[embedding]` table and
`topic`-as-angle note. Recorded, not planned: `crates/podling-cli` needs no change (`..`
destructures; the config error already names the fix); `docs/handoff.md` and the idea's `status:`
belong to `/sync-docs`; a CLI-level refusal test duplicates 6.5.

## Risk & rollback
- The arc prompt may lower the pass rate on llama3.1:8b: the tables measure it, and ARCH-STORY-08 decides.
- Refusing hosted URLs breaks an episode pointing at a hosted server without `data_policy`: intended (local by default); the error names the fix.
- Only `localhost` and IP literals count as local: a LAN hostname (`ollama.lan`) is hosted and needs an IP or a declared policy — documented in the README.
- Live measurement: the Ollama container runs llama3.1:8b on the CPU at ~0.3 tok/s (2026-10-07), too slow for 20+ scripts; units 2 and 4 use a native GPU Ollama on 127.0.0.1:11435 (same model and quantisation), recorded in each table.
- Rollback: `git revert` per unit; reverting the prompt only invalidates script cache entries.

## Out of scope
- The act writer / `act_plan.rs` (ARCH-STORY-08; ARCH-STORY-09/10 apply only if it is built).
- An `EpisodeSpec` angle/focus field (ARCH-STORY-05).
- New Titanic angle sources, a writer-model bake-off, fiction (separate idea).
- An OpenRouter `provider` routing object (ARCH-PRIVACY-08 applies once Podling supports a host's control).
