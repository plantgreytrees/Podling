---
slug: stance-whole-window
goal: A contradiction a window states in a sentence without a number (t03) holds both ways again, without letting the topic-only false contradictions back in.
classification: in-scope   # user decision 2026-10-07: option 2, try before merging (option B)
tracker_rows: [TRACKER#stance-whole-window/1]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)   # one function's output widens; same files as stance-two-way
  plan_strategist: skipped(options already weighed with the user)
coverage:
  contract:      N/A(no podling-types change; `reverse_hypotheses` keeps its signature)
  data:          N/A(no persistence; stage cache keyed by VERSION 7 -> 8)
  config:        N/A(no new tunable)
  security:      N/A(no authn/input/secret surface)
  tests:         1.2, 1.3
  observability: N/A(reverse pair count already logged)
  interface:     N/A(no CLI/UI change)
  docs:          1.5
  rollback:      git revert; or keep option 1 (documented limit) if 1.3's gate fails
units:
  - id: 1
    scope_id: stance-whole-window
    project: .
    depends_on: []
    module: crates/podling-core stance reverse check reads the whole window too
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - docs/architecture.md
        - docs/handoff.md
        - docs/plans/stance-whole-window.md
      docs:
        - docs/architecture.md
        - docs/handoff.md
        - docs/plans/stance-whole-window.md
      write:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - docs/architecture.md
        - docs/handoff.md
        - docs/plans/stance-whole-window.md
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: stance-whole-window

## Outcome
`reverse_hypotheses` also returns the whole window when it has more than one sentence and one
of them holds no number, so a contradiction stated in that sentence ("Kulik never reached the
site.") can be found both ways. Kept only if the live re-score shows t03 recovered and no new
false contradiction; otherwise reverted to the documented limit (option 1).

## Decision (user, 2026-10-07)
Option 2 (whole window as an extra reverse hypothesis), tried before merging the branch
(option B). Option 1 is the fallback.

## Scope Steps (executable core)

### Unit 1 — stance-whole-window
- [ ] 1.1 `score_stances.rs`: `reverse_hypotheses` returns the numbered sentences, then the
  whole window when it has ≥ 2 sentences and at least one has no number; docs on it and on
  `holds_both_ways` updated; VERSION 7 → 8 with a one-line reason. No threshold changed.
  accept: `cargo test -p podling-core --lib score_stances` green.
- [ ] 1.2 Unit test: `reverse_hypotheses` on a window with a numberless sentence returns the
  numbered sentence and the whole window; on a window of numbered sentences only, just those.
  The four pre-existing stage tests unchanged.
  accept: tests named for each case.
- [ ] 1.3 Live re-score (`score_the_stance_pairs`); forward scores unchanged. Gate: t03 is a TP
  and the false positives stay exactly n35 n36. Pass → pin the new figures. Fail → revert
  1.1/1.2, record the figures in Report, keep option 1.
  accept: report printed and recorded in Report.
- [ ] 1.4 Live cold runs (gate passed only): Titanic keeps "706 persons were saved." Contested
  and the lifeboat claim not; Tunguska exit 0 with 0 Contested.
  accept: outcomes recorded in Report.
- [ ] 1.5 `docs/architecture.md` score_stances rule and figures; `docs/handoff.md` status.
  accept: doc figures equal the printed report.
- [ ] 1.6 Fill Report below.

## Report
_Filled by 1.6._

## Verification background
- t03 (VERSION 7): similarity 883, forward 1000, reverse 2 (numbered sentence only);
  `docs/plans/scrutinise-stance-two-way.md` Report.
- First probe (`docs/plans/stance-two-way.md` request): reverse against the whole window gave
  n21 0.003, n33 0.002, n34 0.001, n37 0.002, so the whole window is expected not to re-admit
  them; unmeasured for t03.
- `FakeNli` rarely calls a whole window a contradiction (≥ 60% of the hypothesis's words must
  be in the premise), so the stage tests' outcomes come from the numbered sentences as before.

CONSUMERS:
- `score_stances::reverse_hypotheses` → `run` (score_stances.rs), `tests/stance_precision.rs`
  (`score_the_stance_pairs`)
