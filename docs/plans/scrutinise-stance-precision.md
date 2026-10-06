---
slug: scrutinise-stance-precision
goal: The number-subject gate is measured on reworded and incidental-word numeric pairs too, so its precision gain is not bought with unseen recall loss.
classification: in-scope   # fixes from /scrutinise stance-precision (range d5a78af..b3632c1); docs/plans/stance-precision.md
tracker_rows: [TRACKER#scrutinise-stance-precision/1, TRACKER#scrutinise-stance-precision/2, TRACKER#scrutinise-stance-precision/3]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)
coverage:
  contract:      N/A(no podling-types change; `decide`/`StanceEvidence` keep their signatures, consumers in CONSUMERS)
  data:          N/A(no persistence; stage cache keyed by VERSION, bumped in 1.6 only if the rule changes)
  config:        N/A(the gate has no tunable value; nothing joins config_fingerprint)
  security:      N/A(no authn/input/secret surface; fixtures are committed text)
  tests:         1.1, 1.2, 1.3, 1.5, 2.1, 2.3, 2.4, 3.1, 3.3, 3.4
  observability: 1.4, 2.2, 3.2 (before/after report printed)
  interface:     N/A(no CLI/UI change)
  docs:          1.7, 2.5, 3.6 (docs/architecture.md score_stances), 2.6, 3.5 (text.rs doc), 1.8, 2.7, 3.7 (this plan's Reports)
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
  - id: 2
    scope_id: stance-pronoun
    project: .
    depends_on: [1]
    module: crates/podling-core stance gate pronoun windows + docs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
      docs:
        - docs/architecture.md
        - docs/plans/scrutinise-stance-precision.md
      write:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - docs/architecture.md
        - docs/plans/scrutinise-stance-precision.md
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: stance-pronoun-anywhere
    project: .
    depends_on: [2]
    module: crates/podling-core stance gate fronted/possessive pronouns + docs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
      docs:
        - docs/architecture.md
        - docs/plans/scrutinise-stance-precision.md
      write:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/text.rs
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
- [x] 1.1 W2: `stance_precision_report` also asserts after-recall ≥ before-recall per stance.
  accept: a rule that drops every Contradicts but one correct one fails the test (checked by
  temporarily pointing `after` at such a rule, then reverted).
- [x] 1.2 S2: `Scored` records `text_hash` (blake3 hex of claim + "\n" + premise);
  `Report::of` asserts each matches its pair, message "scores.json is stale".
  accept: editing a pair's premise without re-scoring fails the report test.
- [x] 1.3 W1/S1 pairs: add labelled pairs to `pairs.json` — ≥4 numeric contradicts with a
  reworded/plural subject (e.g. blast/occurred/1907; "explosions"; "trees … 80 million" vs
  "8 million fir trees"), ≥3 numeric neither pairs whose only shared word is incidental
  (e.g. "Kulik's expedition reached the site in June 1927." vs the June 1908 claim), and a
  window whose subject sentence restates the claim's number while another sentence holds
  an unrelated number. Re-score the whole set with the live models (module docs command).
  accept: `the_pair_set_is_well_formed` passes; scores.json fingerprint/embedding unchanged.
- [x] 1.4 Print the report on the extended set for the current rule (VERSION 3) and for
  VERSION 2; record both in the Report section below.
  accept: report printed with `-- --nocapture report`.
- [x] 1.5 Gate change only if 1.4 shows a W1 miss or an S1 false positive: candidate rules
  (a) the subject sentence must hold a number the claim does not state; (b) month/unit
  words leave the subject; (c) shared-prefix match (≥5 chars) for the subject. Report each
  candidate's contradicts TP/FP/FN; pick the one with the best precision at ≥ the current
  recall, else keep the rule and record why. S3 cases join
  `a_number_about_something_else_does_not_contradict` (or a sibling test) either way.
  accept: chosen rule's figures printed before/after; pre-existing tests unchanged.
- [x] 1.6 If the rule changed: `Stage::VERSION` 3 → 4 with a one-line reason; `decide_v2`
  stays the baseline; nothing joins `config_fingerprint` unless a tunable value is added.
  accept: `cargo test --workspace` green.
- [x] 1.7 S4 + figures: `docs/architecture.md` score_stances "Measured precision" paragraph
  gives the extended pair count and figures and the `-- --nocapture report` command.
  accept: doc figures equal the printed report.
- [x] 1.8 Fill the Report section below.

### Unit 2 — stance-pronoun (round 2: /scrutinise range d64f503..a35bc90, 0 C / 1 W / 3 S)
Findings: W1 `score_stances.rs:261-276` a window naming the subject in a numberless sentence
and referring back by pronoun in the numbered one ("Leonid Kulik led the first expedition to
the site. He got there in 1931.") is refused — "there" is a stop word (`text.rs:29-33`); no
such pair is measured. S1 `:274-275` one incidental shared word passes, unpinned. S2
`docs/architecture.md:393-394` says the test fails on any precision/recall drop; it guards
only against `decide_v2`. S3 `text.rs:9-11` `sentences` doc names only the fake provider
and tests.

- [x] 2.1 W1 pairs: add to `pairs.json` c22 ("Kulik reached the site in 1927." / "Leonid
  Kulik led the first expedition to the site. He got there in 1931."), c23 ("The explosion
  happened in June 1908." / "The explosion was enormous. It occurred in 1907."), c24 ("About
  80 million trees were flattened." / "The trees fell in a butterfly-shaped pattern. They
  numbered roughly 8 million."), and pronoun neithers whose pronoun refers to something
  other than the claim's subject: n34 ("The explosion happened in June 1908." / "The
  explosion was witnessed by Evenki herders. They returned to the area in 1921."), n35
  ("Kulik reached the site in 1927." / "Kulik studied meteorites in Petrograd. It became
  Leningrad in 1924."). Re-score live (module docs command).
  accept: `the_pair_set_is_well_formed` passes; old pairs' scores unchanged; fingerprint and
  embedding unchanged.
- [x] 2.2 Print the report for VERSION 4 on the extended set; record it in Report (round 2).
  accept: printed with `-- --nocapture report`.
- [x] 2.3 Only if 2.2 shows a contradicts miss among c22–c24: candidate (d) a numbered
  sentence opening with a pronoun (he/she/it/they/this) continues the previous sentence's
  subject. Report its contradicts TP/FP/FN (n34/n35 measure the precision cost); adopt it
  only at ≥ VERSION 4 precision and ≥ recall, else keep VERSION 4 and record the trade-off.
  Unit tests for c22-shaped (and n34-shaped if adopted) windows join a sibling of
  `a_number_about_something_else_does_not_contradict`. If adopted: VERSION 4 → 5 with a
  one-line reason; no threshold moved.
  accept: chosen rule's figures printed; pre-existing tests unchanged; `cargo test --workspace` green.
- [x] 2.4 S1: unit test pinning that one incidental shared word in the numbered sentence
  keeps the model's call (n29 shape: "Kulik's expedition reached the site in June 1927."
  vs "The explosion happened in June 1908." with contradiction ≥ CONTRADICT_PM → Contradicts).
  accept: test passes and names the trade-off in a comment.
- [x] 2.5 S2: `docs/architecture.md` "Measured precision" says the test fails if a rule
  change lowers precision or recall *relative to the VERSION 2 rule*; pair count and figures
  updated to the 2.2/2.3 report; the pronoun trade-off recorded next to the rule.
  accept: doc figures equal the printed report.
- [x] 2.6 S3: `text.rs` `sentences` doc names the stance gate as a consumer and the
  abbreviation limitation ("Dr. Kulik" splits).
  accept: `cargo doc`-visible comment updated; no behaviour change.
- [x] 2.7 Fill "Report (round 2)" below.

### Unit 3 — stance-pronoun-anywhere (round 3: /scrutinise range d64f503..5ef59fa, 0 C / 1 W / 2 S)
Findings: W1 `score_stances.rs:297-303` `opens_with_pronoun` checks only the first word
(used at :280-282), so a fronted adverbial ("…to the site. In 1931 he got there.") or a
possessive opener ("His arrival came in 1931."; "his" is a stop word, `text.rs:30-34`) is
refused; no such pair is measured. S1 `text.rs:30-34,41` STOP_WORDS/`content_words`/
`sentences` feed the stance gate but `ScoreStances::fingerprint` doesn't cover them. S2
`score_stances.rs:534-607` the gate is tested through `decide` only, not `ScoreStances::run`.

- [ ] 3.1 W1 pairs: add to `pairs.json` c25 ("Kulik reached the site in 1927." / "Leonid
  Kulik led the first expedition to the site. In 1931 he got there."), c26 ("Kulik reached
  the site in 1927." / "Leonid Kulik led the first expedition to the site. His arrival came
  in 1931."), and same-shape neithers whose pronoun means another noun: n36 ("Kulik reached
  the site in 1927." / "Kulik studied meteorites in Petrograd. In 1924 it became
  Leningrad."), n37 ("The explosion happened in June 1908." / "The explosion was witnessed by
  Evenki herders. Their village was moved in 1921."). Re-score live.
  accept: `the_pair_set_is_well_formed` passes; old pairs' scores unchanged.
- [ ] 3.2 Print the report for VERSION 5 on the extended set; record it in Report (round 3).
  accept: printed with `-- --nocapture report`.
- [ ] 3.3 Only if 3.2 shows a contradicts miss among c25–c26: candidate (e) a numbered
  sentence carries the previous sentence's subject when any of its words is a pronoun or
  possessive (he/she/it/they/this/these/his/her/its/their/him/them). Report its contradicts
  TP/FP/FN (n36/n37 measure the cost); adopt it only at ≥ VERSION 2 precision and recall
  (the guard in `stance_precision_report`), else keep VERSION 5 and record the trade-off.
  If adopted: VERSION 5 → 6 with a one-line reason; unit cases for c25/c26 shapes beside
  `a_pronoun_carries_the_subject_into_the_numbered_sentence`; no threshold moved.
  accept: chosen rule's figures printed; pre-existing tests unchanged; `cargo test --workspace` green.
- [ ] 3.4 S2: one `ScoreStances::run`-level test with the module's fake NLI returning
  contradiction ≥ CONTRADICT_PM for a claim and an off-subject numbered window (n21 shape),
  asserting that claim gets no Contradicts evidence from that chunk.
  accept: test passes and fails if `Judged::stance` gets claim/premise swapped or the gate removed.
- [ ] 3.5 S1: doc comments on `STOP_WORDS`, `content_words` and `sentences` in `text.rs` say
  the stance gate depends on them, so a change must bump `ScoreStances` VERSION.
  accept: comments present; no behaviour change.
- [ ] 3.6 `docs/architecture.md` score_stances: pair count and figures equal the 3.2/3.3
  report; rule bullet describes the pronoun carry-over as adopted.
  accept: doc figures equal the printed report.
- [ ] 3.7 Fill "Report (round 3)" below.

## Report
Pair set: 62 → 73 pairs (c17–c21 reworded/plural-subject numeric contradictions; n29–n33
incidental-word and window numeric neithers; s19 a window restating the claim's number
beside an unrelated one). Re-scored live (DeBERTa NLI, nomic-embed-text); the 62 old
pairs' scores came back identical.

`cargo test -p podling-core --test stance_precision -- --nocapture report`:

```
before: VERSION 2 rule on the labelled pair set
  stance       TP  FP  FN  precision  recall  false positives
  supports     14   0   5     100.0%   73.7%
  contradicts  21   2   0      91.3%  100.0%  n21 n33
VERSION 3 (number's own sentence must share a word), same set:
  contradicts  17   0   4     100.0%   81.0%   -> W1 confirmed; the new recall check failed it
after: score_stances::decide (VERSION 4) on the labelled pair set
  supports     14   0   5     100.0%   73.7%
  contradicts  21   0   0     100.0%  100.0%
```

What separates the false positives from the misses: n21 and n33 name the claim's subject
("explosion") in a sentence without a number and put their year in a sentence about
something else; c17–c21 share no word with the claim anywhere, so word overlap cannot
judge them. VERSION 4 refuses Contradicts only when some sentence names the subject and
no number-holding sentence does.

| Candidate (plan 1.5) | Contradicts TP/FP/FN | Outcome |
|---|---|---|
| (a) subject sentence must hold a number the claim doesn't state | ≤17 / 0 / ≥4 | adds a condition to VERSION 3, so it can't recover c17–c21 |
| (b) month/unit words leave the subject | ≤17 / 0 / ≥4 | n29 ("June" only) is no false positive here (the model is below 0.950), and it can't recover the misses |
| (c) ≥5-char shared prefix | ≤18 / 0 / ≥3 | only "expedition(s)" (c19) is matched; blast/explosion, trees/trunks are not |
| subject named only in a numberless sentence (chosen) | 21 / 0 / 0 | precision and recall both 100% |

No threshold moved. S1 (one incidental word such as "june" passes the gate) remains
possible in principle; n29 shows the model doesn't over-call it on this set.

## Report (round 2)
Pair set: 73 → 78 pairs (c22–c24 pronoun-reference numeric contradictions; n34–n35 windows
whose pronoun means another noun). Re-scored live; the 73 old pairs' scores, the NLI
fingerprint and the embedding model came back unchanged (scores.json diff is insertions only).
New scores (similarity / contradiction): c22 845/1000, c23 860/995, c24 799/999,
n34 698/963, n35 701/998.

`cargo test -p podling-core --test stance_precision -- --nocapture report`:

```
before: VERSION 2 rule on the labelled pair set
  contradicts  24   4   0      85.7%  100.0%  n21 n33 n34 n35
VERSION 4 (no pronoun carry-over), same set:
  contradicts  22   0   2     100.0%   91.7%   -> W1 confirmed (c22, c23 refused); recall check failed it
after: score_stances::decide (VERSION 5)
  supports     14   0   5     100.0%   73.7%
  contradicts  24   2   0      92.3%  100.0%  n34 n35
```

c24 already passed VERSION 4 (its numbered sentence shares "million" with the claim). c22 and
c23 were refused because the pronoun sentence shares no word. Candidate (d), adopted as
VERSION 5: a sentence opening with he/she/it/they/this/these also names the previous
sentence's subject words. It recovers c22 and c23 and also lets n34 ("They" = herders) and n35
("It" = Petrograd) through. Word overlap cannot resolve which noun a pronoun means, so no word
rule separates c22/c23 from n34/n35.

Deviation from 2.3 as written ("adopt only at ≥ VERSION 4 precision"): VERSION 4 fails the
recall guard (91.7% < VERSION 2's 100%), and keeping it would mean weakening that test, which
the constraints forbid. VERSION 5 holds recall at 100% and still raises contradicts precision
over VERSION 2 (85.7% → 92.3%). A false Contradicts sends the claim to the adjudicator; a
dropped one silently leaves it Corroborated/SingleSource. Supports unchanged; no threshold
moved.

## Report (round 3)
_Filled by 3.7._

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
