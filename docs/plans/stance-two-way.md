---
slug: stance-two-way
goal: A number-against-number contradiction counts only when it also holds read the other way round, so a shared-topic count (the Titanic lifeboat capacity against the 712 taken on board) is no longer Contested while the real 706-vs-712 disagreement stays Contested.
classification: in-scope   # follow-up to docs/plans/stance-precision.md and scrutinise-stance-precision.md; user decision 2026-10-07
tracker_rows: [TRACKER#stance-two-way/1]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)   # one module; StanceEvidence's only consumer is tests/stance_precision.rs
  plan_strategist: skipped(decomposition fixed by the user's choice between probed options)
coverage:
  contract:      N/A(no podling-types change; `StanceEvidence` gains a field, consumers in CONSUMERS)
  data:          N/A(no persistence; stage cache keyed by VERSION 6 -> 7)
  config:        N/A(no new tunable; the reverse check reuses CONTRADICT_PM, already in config_fingerprint)
  security:      N/A(no authn/input/secret surface)
  tests:         1.2, 1.3, 1.4, 1.5, 1.6
  observability: 1.1 (reverse pairs in the "stances scored" log), 1.6 (before/after report printed)
  interface:     N/A(no CLI/UI change)
  docs:          1.8 (docs/architecture.md score_stances), 1.9 (docs/handoff.md)
  rollback:      git revert; VERSION bump makes cached entries foreign misses
units:
  - id: 1
    scope_id: stance-two-way
    project: .
    depends_on: []
    module: crates/podling-core stance gate two-way check + eval + docs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/plugin/nli.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - docs/architecture.md
        - docs/handoff.md
        - docs/plans/stance-two-way.md
      docs:
        - docs/architecture.md
        - docs/handoff.md
        - docs/plans/stance-two-way.md
      write:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - docs/architecture.md
        - docs/handoff.md
        - docs/plans/stance-two-way.md
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: stance-two-way

## Outcome
With `[nli]` set, a number-against-number contradiction needs the NLI model to find the
contradiction in both directions. Live on `examples/titanic`, "The vessel was provided
with lifeboats for 1,176 persons." is no longer Contested by "From these boats he took on
board 712 persons, one of them died shortly afterwards.", while "706 persons were saved."
still is. Runs without `[nli]` are unchanged.

## Decision (user, 2026-10-07)
Adopt the two-way check; accept losing c18 ("80 million trees" vs "8 million fir
trunks", reverse contradiction 0.000: the model reads fir trunks as a subset of trees);
pin the new figures exactly in `stance_precision_report` instead of the VERSION 2 floor.
That is a deliberate, approved change to that test.

## Scope Steps (executable core)

### Unit 1 — stance-two-way
- [ ] 1.1 `score_stances.rs`: `pub fn reverse_hypotheses(premise) -> Vec<&str>` returns the
  window's sentences that hold a number. `StanceEvidence` gains
  `reverse_contradiction: Option<PerMille>` — the highest contradiction of the claim (as
  premise) against those sentences (as hypotheses), `None` when not scored. `decide`: when
  claim and premise both hold numbers, Contradicts also needs `reverse_contradiction ≥
  CONTRADICT_PM` (a `None` there refuses). `run`: after the forward batch, a candidate needs
  reverse pairs when `decide` with `reverse_contradiction: Some(max)` gives Contradicts and
  both texts hold numbers (so `decide` stays the one source of the rule); score all those
  reverse pairs in one `score_checked` call, then `decide` with the real value; log the
  reverse pair count. VERSION 6 → 7 with a
  one-line reason. The number-subject gate stays.
  accept: `cargo test -p podling-core --lib score_stances` green; no threshold value changed.
- [ ] 1.2 Unit tests: `decide` refuses a numeric contradiction with low or missing reverse
  contradiction and keeps it at ≥ CONTRADICT_PM; a non-numeric contradiction ignores the
  reverse field. Existing `decide` tests built in this feature series pass the reverse
  value they need (they predate it); the pre-existing stage tests (paraphrase supports,
  changed-year contests, one evidence per chunk, own group never checked) stay unchanged
  and green.
  accept: tests named for each case; `git diff` shows no edit to the four pre-existing tests.
- [ ] 1.3 A stage-level test with a fake NLI whose contradiction is one-way (forward high,
  reverse low) for a numbered window: the claim gets no Contradicts evidence; a two-way one
  still contests.
  accept: test passes and asserts the window's similarity clears MIN_CONTRADICT_SIMILARITY_PM.
- [ ] 1.4 `pairs.json`: add t01 contradicts ("706 persons were saved." / "From these boats he
  took on board 712 persons, one of them died shortly afterwards.") and t02 neither ("The
  vessel was provided with lifeboats for 1,176 persons." / same premise). `Scored` gains
  `reverse_contradiction_pm` (max over `reverse_hypotheses`, absent when the pair has no
  numbers on both sides); `score_the_stance_pairs` fills it; re-score live.
  accept: `the_pair_set_is_well_formed` passes; forward scores of the 82 earlier pairs unchanged.
- [ ] 1.5 Round-4 suggestion: replace the vacuous Kulik/crater assertion with
  `decide(..) == None` for n21 and n33 (the off-subject window pairs).
  accept: assertion fails if the subject gate and the reverse check are both removed.
- [ ] 1.6 `stance_precision_report`: prints VERSION 2 (before) and current (after); asserts
  the current contradicts and supports TP/FP/FN and FP id sets exactly, so any rule change
  must update them on purpose.
  accept: report printed with `-- --nocapture report`; figures recorded in Report below.
- [ ] 1.7 Live acceptance (cold `--cache-dir`, llama3.1:8b on the GPU Ollama
  127.0.0.1:11435, NLI weights in `~/.cache/podling-models/nli-deberta-v3-base`): a Titanic
  run reaches the ledger with "706 persons were saved." (or the extracted 706 claim)
  Contested and the lifeboat-capacity claim not Contested; one Tunguska run exits 0 with 0
  Contested. A script-stage failure from llama's known quoting flake is recorded, and the
  run repeated until one exits 0.
  accept: run outcomes recorded in Report.
- [ ] 1.8 `docs/architecture.md` score_stances: the rule bullet describes the two-way check
  and its cost (c18); "Measured precision" gives the new pair count and figures, and says
  the test pins the current figures exactly.
  accept: doc figures equal the printed report.
- [ ] 1.9 `docs/handoff.md` status: the lifeboat case is addressed and the live check run;
  remaining limits named (n35/n36 pronoun cases).
  accept: written.
- [ ] 1.10 Fill Report below.

## Report
_Filled by 1.10._

## Verification background
- Probe (real DeBERTa, forward contradiction / reverse contradiction, reverse = max over the
  window's sentences): T706 0.997/0.995, T1176 0.997/0.010; contradicts c01 c02 c03 c05 c10
  c15 c17 c19 c20 c21 c22 c23 c24 c25 c26 reverse ≥ 0.991; c18 0.000; neithers n21 0.080,
  n33 0.021, n34 0.002, n37 0.004, n29 0.664 (refused), n35 0.999, n36 0.999 (kept).
- Rejected: counterfactual number swap (put the premise's number in the claim, require
  entailment): T1176 entailment 0.995, and it loses c17 c18 c20 c21 c23 c24.
- Why per numbered sentence, not the whole window: `FakeNli` (`plugin/nli.rs:88-131`) calls
  a contradiction only when ≥ 60% of the hypothesis's words are in the premise, so a whole
  multi-sentence window as hypothesis would refuse the stage tests' real contradictions;
  the real model gives the same verdicts either way on every probed pair.
- Live baseline on main `e37244c` (cold caches): Titanic run 1 failed at the script stage
  (typed "Iceberg right ahead."); run 2 exit 0, Contested = {"706 persons were saved.",
  "The vessel was provided with lifeboats for 1,176 persons."}, verdicts 1 contradicting /
  1 unresolved; Tunguska exit 0, 9 claims, 0 Contested.
- Gate: `number_is_about_the_subject` and `decide` in `score_stances.rs`; thresholds
  `CONTRADICT_PM` 950, `MIN_CONTRADICT_SIMILARITY_PM` 600.

CONSUMERS:
- `score_stances::StanceEvidence` → `Judged::stance` (score_stances.rs), `tests/stance_precision.rs:24,161,181,229,271`
- `score_stances::decide` → same
- podling-types: unchanged
