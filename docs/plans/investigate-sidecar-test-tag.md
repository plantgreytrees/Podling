---
slug: investigate-sidecar-test-tag
goal: The sidecar TTS tests pass when several copies of the suite run at once, so a pre-merge gate no longer fails at random.
classification: in-scope   # /investigate of the flaky sidecar_tts tests seen in the stance-precision pre-merge gate
tracker_rows: [TRACKER#investigate-sidecar-test-tag/1]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)   # one test file, no contract change
  plan_strategist: skipped(trivial)
coverage:
  contract:      N/A(test helpers only)
  data:          N/A(no persistence)
  config:        N/A(no tunable)
  security:      N/A(no authn/input/secret surface)
  tests:         1.1, 1.2, 1.3
  observability: N/A(test-only)
  interface:     N/A(no CLI/UI change)
  docs:          1.4 (plan Report only; no doc describes the test tags)
  rollback:      git revert
units:
  - id: 1
    scope_id: investigate-sidecar-test-tag
    project: .
    depends_on: []
    module: crates/podling-core sidecar_tts test tags
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/tests/sidecar_tts.rs
        - crates/podling-core/tests/fixtures/fake_sidecar.py
        - docs/plans/investigate-sidecar-test-tag.md
      docs:
        - docs/plans/investigate-sidecar-test-tag.md
      write:
        - crates/podling-core/tests/sidecar_tts.rs
        - docs/plans/investigate-sidecar-test-tag.md
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: investigate-sidecar-test-tag

## Outcome
Each run of the `sidecar_tts` test binary tags its workers with its own pid, so `running()`
never counts another run's workers. Two copies run at once both pass.

## Scope Steps (executable core)

### Unit 1 — investigate-sidecar-test-tag
- [x] 1.1 Regression test first: start a decoy process (`python3 -c` sleep) whose command
  line holds `podling-test-decoy`, the tag another run would use, and assert
  `running("decoy")` is empty; kill and reap the decoy afterwards.
  accept: fails on the old code (decoy counted), passes after 1.2.
- [x] 1.2 Root cause: one helper `needle(tag)` = `podling-test-<pid>-<tag>`
  (`std::process::id()`), used by `stub()` (`sidecar_tts.rs:42-53`) and `running()`
  (`:65-80`). Fixes the latent `running("ok"/"hang"/"proto")` checks (`:188, :233, :243`) too.
  accept: no other `podling-test-` literal left in the file.
- [x] 1.3 Two concurrent runs of the `sidecar_tts` binary (filter `child`, the reproduction)
  both pass; `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace` exit 0.
  accept: outcomes recorded in Report.
- [x] 1.4 Fill Report below.

## Report
- 1.1: `another_runs_worker_is_not_counted` failed on the old code ("another run's worker
  was counted: [pid]") and passes after 1.2.
- 1.2: `needle(tag)` = `podling-test-<pid>-<tag>`, shared by `stub()` and `running()`; the
  only other `podling-test-` literals are the decoy's.
- 1.3: two concurrent runs of the fixed binary: full suite 13/13 both; filter `child` three
  more pairs, 2/2 each (before the fix: 0/2 each). `cargo fmt --all --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo test --workspace` exit 0.

## Verification background
- Reproduction (2026-10-07, before the fix): two copies of the binary started together with
  filter `child` both fail 2/2: `a_child_of_the_worker_that_ignores_sigterm_is_killed_too`
  "left: 3 right: 2" (`:273`) and "the worker's child is gone too: [pid]" (`:277`);
  `a_child_left_behind_by_a_dead_worker_is_killed` "left: 3 / 2 right: 1" (`:297`).
- Not a leak: afterwards no `podling-test-*` process survives (`pgrep -af podling-test-`
  matched only its own shell). Product code (`src/plugin/sidecar*.rs`) is not involved.
- Sidecar stop design (memory): descendants are killed via pidfd, not process groups.

CONSUMERS:
- none (test-private helpers `stub`, `running`)
