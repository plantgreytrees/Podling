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
        - crates/podling-core/src/text.rs
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
        - crates/podling-core/src/text.rs
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

Second decision (user, 2026-10-07, after 1.3's first re-score): the gate failed because
the subject rule (`number_is_about_the_subject`, VERSIONs 3–7) also refuses t03. A probe
without it gave 27/2/1, so the user chose "drop the subject rule": VERSION 8 is the
whole-window two-way check alone. `text.rs` joined the scope for its doc comments.

## Scope Steps (executable core)

### Unit 1 — stance-whole-window
- [x] 1.1 `score_stances.rs`: `reverse_hypotheses` returns the numbered sentences, then the
  whole window when it has ≥ 2 sentences and at least one has no number; docs on it and on
  `holds_both_ways` updated; VERSION 7 → 8 with a one-line reason. No threshold changed.
  accept: `cargo test -p podling-core --lib score_stances` green.
- [x] 1.2 Unit test: `reverse_hypotheses` on a window with a numberless sentence returns the
  numbered sentence and the whole window; on a window of numbered sentences only, just those.
  The four pre-existing stage tests unchanged.
  accept: tests named for each case.
- [x] 1.3 Live re-score (`score_the_stance_pairs`); forward scores unchanged. Gate: t03 is a TP
  and the false positives stay exactly n35 n36. Pass → pin the new figures. Fail → revert
  1.1/1.2, record the figures in Report, keep option 1.
  accept: report printed and recorded in Report.
- [x] 1.4 Live cold runs (gate passed only): Titanic keeps "706 persons were saved." Contested
  and the lifeboat claim not; Tunguska exit 0 with 0 Contested.
  accept: outcomes recorded in Report.
- [x] 1.5 `docs/architecture.md` score_stances rule and figures; `docs/handoff.md` status.
  accept: doc figures equal the printed report.
- [x] 1.6 Fill Report below.

## Report
- 1.1: `reverse_hypotheses` adds the whole window when it mixes numbered and numberless
  sentences. The subject rule and `refers_back` are removed (second decision). VERSION 8.
  No threshold changed.
- 1.2: `reverse_hypotheses_are_the_numbered_sentences_and_a_mixed_window`;
  `a_number_about_something_else_fails_the_reverse_check` (n21 at 0.080 back → None; t03 at
  1.000 back → Contradicts) replaces the five subject-rule tests (`a_number_about_something_else_does_not_contradict`,
  `a_claim_without_a_number_skips_the_subject_rule`, `a_pronoun_carries_the_subject_into_the_numbered_sentence`,
  `one_shared_word_keeps_the_models_call`, `the_stage_refuses_a_number_about_something_else`),
  whose rule no longer exists. The four pre-existing stage tests are unchanged and pass.
- 1.3: live re-score (DeBERTa v3 base, nomic-embed-text); forward scores unchanged.
  Reverse: t03 0.002 → 1.000, n21 0.080, n33 0.021, n34 0.002, n37 0.004.
  First pass (with the subject rule): contradicts 26/2/2, unchanged, so the gate failed.
  Without the subject rule:
  ```
  before: VERSION 2   contradicts 28 TP 7 FP 0 FN  80.0% / 100.0%  n21 n33 n34 n35 n36 n37 t02
  after:  VERSION 8   contradicts 27 TP 2 FP 1 FN  93.1% /  96.4%  n35 n36
                      supports    14 TP 0 FP 5 FN 100.0% /  73.7%
  ```
  Gate passed (t03 TP, FPs exactly n35 n36); pinned in `stance_precision_report`.
- 1.4: cold runs (llama3.1:8b, GPU Ollama). Titanic exit 0: "706 persons were saved."
  Contested (the only Contested claim); the lifeboat claim single_source. Tunguska: the first
  run failed at `script` (llama cited a sentence that doesn't exist; stances already
  scored, `reverse_pairs=0`, 0 contradicts); the rerun on the same cache exited 0 with
  7 claims, 0 Contested (1 corroborated, 6 single_source).
- 1.5: `docs/architecture.md` score_stances rule and figures, `docs/handoff.md` status.
- Gates: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace` exit 0.

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
