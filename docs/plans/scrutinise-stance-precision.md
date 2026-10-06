---
slug: scrutinise-stance-precision
goal: The number-subject gate is measured on reworded and incidental-word numeric pairs too, so its precision gain is not bought with unseen recall loss.
classification: in-scope   # fixes from /scrutinise stance-precision (range d5a78af..b3632c1); docs/plans/stance-precision.md
tracker_rows: [TRACKER#scrutinise-stance-precision/1]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)
coverage:
  contract:      N/A(no podling-types change; `decide`/`StanceEvidence` keep their signatures, consumers in CONSUMERS)
  data:          N/A(no persistence; stage cache keyed by VERSION, bumped in 1.6 only if the rule changes)
  config:        N/A(the gate has no tunable value; nothing joins config_fingerprint)
  security:      N/A(no authn/input/secret surface; fixtures are committed text)
  tests:         1.1, 1.2, 1.3, 1.5
  observability: 1.4 (before/after report printed)
  interface:     N/A(no CLI/UI change)
  docs:          1.7 (docs/architecture.md score_stances), 1.8 (this plan's Report)
  rollback:      git revert; a VERSION bump makes cached entries foreign misses
units:
  - id: 1
    scope_id: stance-subject
    project: .
    depends_on: []
    module: crates/podling-core stance gate + eval
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - crates/podling-core/Cargo.toml
      docs:
        - docs/architecture.md
        - docs/plans/stance-precision.md
      write:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - docs/architecture.md
        - docs/plans/scrutinise-stance-precision.md
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: scrutinise stance-precision

## Outcome
The number-subject gate from stance-precision is measured on the inputs the scrutineer
showed it was never tested on (a reworded subject, an incidental shared word), and the
report fails the build when a rule change loses recall.

## Findings (from /scrutinise stance-precision, isolated, 0 Critical / 2 Warning / 4 Suggestion)
- W1 Correctness — `score_stances.rs:259-280` `numbers_share_the_subject` matches exact
  lower-cased words (`text.rs:38` `content_words`, no stemming); "The blast occurred in
  1907." vs "The explosion happened in June 1908." fails the gate → a real contradiction
  dropped. Every numeric contradicts pair (c01 c02 c03 c05 c10 c15) reuses the claim's words.
- W2 Test-coverage — `stance_precision.rs:230-239` asserts precision never falls, not recall.
- S1 Correctness — `score_stances.rs:276-279` any number-holding sentence sharing any word
  (e.g. only "june") passes, including the sentence restating the claim's own number.
- S2 Test-coverage — `stance_precision.rs:159-163` the stale check compares ids only.
- S3 Test-coverage — `score_stances.rs:504-523` no "claim has no number" case, no "shared
  subject only in a numberless sentence" case.
- S4 Cross-unit — `docs/architecture.md` says the report prints on every `cargo test`;
  libtest captures `eprintln!` unless `--nocapture`.

## Scope Steps (executable core)

### Unit 1 — stance-subject
- [ ] 1.1 W2: `stance_precision_report` also asserts after-recall ≥ before-recall per stance.
  accept: a rule that drops every Contradicts but one correct one fails the test (checked by
  temporarily pointing `after` at such a rule, then reverted).
- [ ] 1.2 S2: `Scored` records `text_hash` (blake3 hex of claim + "\n" + premise);
  `Report::of` asserts each matches its pair, message "scores.json is stale".
  accept: editing a pair's premise without re-scoring fails the report test.
- [ ] 1.3 W1/S1 pairs: add labelled pairs to `pairs.json` — ≥4 numeric contradicts with a
  reworded/plural subject (e.g. blast/occurred/1907; "explosions"; "trees … 80 million" vs
  "8 million fir trees"), ≥3 numeric neither pairs whose only shared word is incidental
  (e.g. "Kulik's expedition reached the site in June 1927." vs the June 1908 claim), and a
  window whose subject sentence restates the claim's number while another sentence holds
  an unrelated number. Re-score the whole set with the live models (module docs command).
  accept: `the_pair_set_is_well_formed` passes; scores.json fingerprint/embedding unchanged.
- [ ] 1.4 Print the report on the extended set for the current rule (VERSION 3) and for
  VERSION 2; record both in the Report section below.
  accept: report printed with `-- --nocapture report`.
- [ ] 1.5 Gate change only if 1.4 shows a W1 miss or an S1 false positive: candidate rules
  (a) the subject sentence must hold a number the claim does not state; (b) month/unit
  words leave the subject; (c) shared-prefix match (≥5 chars) for the subject. Report each
  candidate's contradicts TP/FP/FN; pick the one with the best precision at ≥ the current
  recall, else keep the rule and record why. S3 cases join
  `a_number_about_something_else_does_not_contradict` (or a sibling test) either way.
  accept: chosen rule's figures printed before/after; pre-existing tests unchanged.
- [ ] 1.6 If the rule changed: `Stage::VERSION` 3 → 4 with a one-line reason; `decide_v2`
  stays the baseline; nothing joins `config_fingerprint` unless a tunable value is added.
  accept: `cargo test --workspace` green.
- [ ] 1.7 S4 + figures: `docs/architecture.md` score_stances "Measured precision" paragraph
  gives the extended pair count and figures and the `-- --nocapture report` command.
  accept: doc figures equal the printed report.
- [ ] 1.8 Fill the Report section below.

## Report
(filled by 1.4/1.5)

## Verification background
- Gate: `crates/podling-core/src/stages/score_stances.rs` `numbers_share_the_subject` (~259),
  `decide` (~233), VERSION (~68). Helpers `content_words`/`is_number`/`numbers`/`sentences`
  in `crates/podling-core/src/text.rs`.
- Re-score: `PODLING_NLI_MODEL_DIR=~/.cache/podling-models/nli-deberta-v3-base
  PODLING_LIVE_EMBED_URL=http://localhost:11434/v1 PODLING_LIVE_EMBED_MODEL=nomic-embed-text
  cargo test -p podling-core --test stance_precision -- --ignored score_the_stance_pairs`
  (Ollama container on :11434 has nomic-embed-text, checked 2026-10-06).
- `blake3` is already a podling-core dependency (`crates/podling-core/Cargo.toml:11`).

CONSUMERS:
- `score_stances::decide` → `Judged::stance` (score_stances.rs), `tests/stance_precision.rs`
- `score_stances::StanceEvidence` → same two
- podling-types: unchanged
