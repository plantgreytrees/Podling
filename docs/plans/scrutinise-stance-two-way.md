---
slug: scrutinise-stance-two-way
goal: The two-way contradiction check's recall cost on a numberless contradicting sentence is measured and pinned, and its reverse batching is covered by stage tests.
classification: in-scope   # /scrutinise stance-two-way round 1: 0 Critical, 0 Warning, 3 Suggestion
tracker_rows: [TRACKER#scrutinise-stance-two-way/1]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)   # tests, one fixture pair and one doc line; no contract change
  plan_strategist: skipped(trivial)
coverage:
  contract:      N/A(no public API change)
  data:          N/A(no persistence; no VERSION bump, the rule is unchanged)
  config:        N/A(no tunable)
  security:      N/A(no authn/input/secret surface)
  tests:         1.1, 1.2, 1.3
  observability: N/A(no new log)
  interface:     N/A(no CLI/UI change)
  docs:          1.4
  rollback:      git revert
units:
  - id: 1
    scope_id: scrutinise-stance-two-way
    project: .
    depends_on: []
    module: crates/podling-core stance two-way tests + measured cost
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - docs/architecture.md
        - docs/plans/scrutinise-stance-two-way.md
      docs:
        - docs/architecture.md
        - docs/plans/scrutinise-stance-two-way.md
      write:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - docs/architecture.md
        - docs/plans/scrutinise-stance-two-way.md
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: scrutinise-stance-two-way

## Outcome
The cost the two-way check may carry beyond c18 is measured: a contradiction stated in a
numberless sentence of a numbered window (S1) is in the labelled pair set with live scores,
and the report pins whether VERSION 7 keeps or misses it. The reverse batch in `run` is
covered for several candidates with different hypothesis counts (S2), and both halves of
the existing two-way stage test pin their pair counts (S3). The rule and thresholds are
unchanged: if the new pair is missed, that is documented, not fixed (changing the rule
needs a user decision).

## Scope Steps (executable core)

### Unit 1 — scrutinise-stance-two-way
- [ ] 1.1 S1: `pairs.json` gains t03, contradicts: claim "Kulik reached the site in 1927.",
  premise "Kulik never reached the site. The expedition set off in 1927."; re-score live
  (`score_the_stance_pairs`); update the pinned figures in `stance_precision_report` to
  whatever the rule now gives (t03 as TP or as a miss), with the before/after printed.
  accept: report printed; scores of the 84 earlier pairs unchanged.
- [ ] 1.2 S2: stage test in `score_stances.rs`: two claims needing reverse scores in one
  run, one window with 2 numbered sentences and one with 1, and a fake NLI giving a high
  reverse contradiction to one specific (premise, hypothesis) pair only; asserts which
  claims are Contested and the exact NLI pair count.
  accept: test passes; it would fail if reverse scores were assigned to the wrong candidate.
- [ ] 1.3 S3: `the_stage_needs_the_contradiction_both_ways` two-way half asserts 4 pairs
  (2 forward + 1 reverse for each of the two contradicting claims).
  accept: assertion present and green.
- [ ] 1.4 `docs/architecture.md` score_stances: Measured precision uses the new pair count
  and figures; if t03 is missed, the rule bullet names the limit (a contradiction stated in
  a numberless sentence of a numbered window is read against the numbered sentences only).
  accept: doc figures equal the printed report.
- [ ] 1.5 Fill Report below.

## Report
_Filled by 1.5._

## Verification background
- Findings: /scrutinise stance-two-way round 1 (isolated scrutineer, range
  c518010..1fcc319): S1 `score_stances.rs:313-331`, S2 `score_stances.rs:166-193`, S3
  `score_stances.rs:877-891`. All acceptance criteria of stance-two-way MET.
- `holds_both_ways` tests numbers on the whole window; `reverse_hypotheses` returns only its
  numbered sentences (`score_stances.rs:313-331`).
- S3 count: input a = CAPACITY, b = TAKEN; forward pairs 2; with `back` 1.0 both forward
  pairs contradict, each gets one reverse pair (its window has one sentence): 4.

CONSUMERS:
- none changed (test-only additions; `StanceEvidence`, `decide`, `reverse_hypotheses` unchanged)
