---
slug: scrutinise-stance-whole-window
goal: The stance code's comments describe the VERSION 8 reverse check, and its tests pin t03's recovery and drive a whole-window reverse pair through the stage.
classification: in-scope   # /scrutinise stance-whole-window findings, 2026-10-07
tracker_rows: [TRACKER#scrutinise-stance-whole-window/1]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)   # comments and tests in two files, one crate, no contract change
  plan_strategist: skipped(trivial)
coverage:
  contract:      N/A(no pub signature or podling-types change)
  data:          N/A(scores.json unchanged; no VERSION bump)
  config:        N/A(no tunable)
  security:      N/A(no authn/input/secret surface)
  tests:         1.3, 1.4
  observability: N/A(no runtime change)
  interface:     N/A(no CLI/UI change)
  docs:          1.1, 1.2 (code comments); architecture.md only if a cite moves
  rollback:      git revert
units:
  - id: 1
    scope_id: scrutinise-stance-whole-window
    project: .
    depends_on: []
    module: crates/podling-core stance comments and tests
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - docs/plans/scrutinise-stance-whole-window.md
        - docs/plans/stance-whole-window.md
        - docs/architecture.md
      docs:
        - docs/plans/scrutinise-stance-whole-window.md
        - docs/architecture.md
      write:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - docs/plans/scrutinise-stance-whole-window.md
        - docs/architecture.md
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: scrutinise-stance-whole-window

## Outcome
Fixes for the four Suggestion findings of `/scrutinise stance-whole-window` (isolated
scrutineer, range 92dc295..c7c86ec, merged as 7958af0; 0 Critical, 0 Warning; all four
acceptance criteria MET). Comments and tests only: no rule, threshold, VERSION or
`scores.json` change.

## Scope Steps (executable core)

### Unit 1 — scrutinise-stance-whole-window
- [x] 1.1 `score_stances.rs` comment in `run` (finding 1, :163-165): the claim is read back
  against `reverse_hypotheses(window)`, i.e. each numbered sentence plus the whole window
  when it mixes numbered and numberless sentences.
  accept: no "numbered sentences" wording in `run` that excludes the whole window.
- [x] 1.2 `stance_precision.rs` (finding 2): the `Scored::reverse_contradiction_pm` doc
  (:85-87) and the `score_the_stance_pairs` doc (:288-291) name `reverse_hypotheses`; the
  :132 comment says "similarity floor", not "same-subject requirement".
  accept: `grep -n "numbered sentences\|numbered window\|same-subject"` finds none of the three.
- [x] 1.3 Stage test (finding 3): a scripted NLI that records batches and gives
  contradiction 1.0 forward for the numbered window, and in reverse only when the
  hypothesis is the whole window "Kulik never reached the site. The expedition set off in
  1927."; the claim "Kulik reached the site in 1927." comes out Contested, and the
  recorded reverse batch holds (claim, whole window).
  accept: the test fails if `reverse_hypotheses` stops adding the whole window (shown by
  temporarily dropping the push), passes otherwise; the four pre-existing stage tests unchanged.
- [x] 1.4 Precision pin (finding 4): `Counts` records missed ids; the report prints them;
  the pin adds them, contradicts missed exactly "c18" and supports missed its five ids.
  accept: `stance_precision_report` passes with the missed ids pinned.
- [x] 1.5 Gates: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace` exit 0. Fill Report.
  accept: outcomes recorded in Report.

## Report
- 1.1: the comment in `run` names `reverse_hypotheses`: numbered sentences, plus the
  whole window when it also has a sentence without a number.
- 1.2: the `reverse_contradiction_pm` and `score_the_stance_pairs` docs name
  `reverse_hypotheses`; the :132 comment says "similarity floor". The grep for
  "numbered sentences|numbered window|same-subject" in `stance_precision.rs` finds nothing.
- 1.3: `the_stage_reads_a_mixed_window_back_whole`: the claim is Contested, and the reverse
  batch is exactly [(claim, "The expedition set off in 1927."), (claim, whole window)].
  With `hypotheses.push(premise)` disabled it fails at the Contested assertion; restored,
  it passes. The four pre-existing stage tests are unchanged.
- 1.4: `Counts.missed_ids` is printed after the false positives and pinned:
  supports `(14, 0, 5, "", "s04 s05 s06 s12 s16")`, contradicts
  `(27, 2, 1, "n35 n36", "c18")`.
- 1.5: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` exit 0. No VERSION, threshold or `scores.json` change.

## Verification background
- Findings: scrutineer report of 2026-10-07 (this session), each cite confirmed at
  `score_stances.rs:163-165`, `stance_precision.rs:85-87, :132, :259-261, :288-291`.
- Existing two-way stage tests (`the_stage_needs_the_contradiction_both_ways`,
  `reverse_scores_go_to_their_own_candidate`) use only all-numbered windows.
- Pinned figures: contradicts (27, 2, 1, "n35 n36"), supports (14, 0, 5, "").

CONSUMERS:
- `Counts` / `Report` are test-private to `tests/stance_precision.rs`; no other consumer.
- No pub item changes.
