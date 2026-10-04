---
goal: A rejected adjudicator reply leaves a bounded reason in verdicts.json, never the model's whole reply.
status: COMPLETE
coverage:
  correctness: 1.1, 1.2
  tests: 1.3
  caching: 1.3 (Adjudicate::VERSION bump)
  contracts: N/A(the Verdict type and schema are unchanged; only the fallback string's content is bounded)
  security: N/A(no auth, secrets or new I/O; the bound limits what model text is persisted)
  docs: N/A(docs/architecture.md describes `fallback` as "the rejection reason", which stays true)
  migration: N/A(no persisted store beyond the content-addressed cache, which the VERSION bump invalidates)
  observability: N/A(the existing warn! line is kept)
blind_rederivation: skipped(trivial)
steps:
  - id: fallback-reason-cap
    scope_id: fallback-reason-cap
    project: .
    depends_on: []
    scope:
      read: [crates/podling-core/src/plugin/llm.rs, crates/podling-core/src/plugin/mod.rs, crates/podling-core/src/stages/adjudicate.rs]
      docs: [docs/plans/scrutinise-phase4-adjudicator.md]
      write: [crates/podling-core/src/plugin/llm.rs, crates/podling-core/src/plugin/mod.rs, crates/podling-core/src/stages/adjudicate.rs, docs/plans/TRACKER.md, docs/plans/scrutinise-phase4-adjudicator.md]
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

## Verification background
- **F1 (Correctness, Minor).** `adjudicate.rs:200-204` turns `InvalidProviderOutput { message }` into `fallback(claim, message)` verbatim. `build_verdict` passes serde_json errors through, and an unknown enum variant's message quotes the model's string whole ("unknown variant `…`"). The comment says the reason "names ids and numbers, never source text", which isn't guaranteed. `complete_validated` already caps the same reason at 500 chars for the retry prompt (`llm.rs:221`).
- The claim id in a mismatch message is a validated `ContentHash` (`crates/podling-types/src/ids.rs`), so it is not a vector.

CONSUMERS:
- `complete_validated` retry excerpt: `crates/podling-core/src/plugin/llm.rs:221` (only site).
- `Verdict.fallback`: `crates/podling-core/src/stages/adjudicate.rs:278-294` (`fallback`), tests in the same file; `crates/podling-core/tests/pipeline.rs` asserts `fallback() == None` only.

## Risks
None beyond cache invalidation of stored verdicts, which the version bump makes explicit.
