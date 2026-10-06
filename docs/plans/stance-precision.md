---
slug: stance-precision
goal: score_stances stops over-calling Supports/Contradicts, so a Contested or Corroborated status rests on evidence measured as precise, not on topic overlap.
classification: in-scope   # .claude/CLAUDE.md "Grounding" (NLI-scored, deterministic status); docs/plans/phase3-nli-ledger.md "Risk & rollback" ("Live results are recorded so the thresholds can be tuned by evidence")
tracker_rows: [TRACKER#stance-precision/1, TRACKER#stance-precision/2]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(config:never)
coverage:
  contract:      N/A(no podling-types change; `decide` is a new pub fn in podling-core only, consumers listed in CONSUMERS)
  data:          N/A(no persistence/migration; stage cache invalidated by VERSION bump 2.4)
  config:        2.3 (new rule constant(s) in config_fingerprint)
  security:      N/A(no authn/input/secret surface; fixtures are committed text)
  tests:         1.2, 1.4, 1.5, 2.2
  observability: 1.4 (the report prints a precision/recall table); existing `stances scored` info log unchanged
  interface:     N/A(no CLI/UI change)
  docs:          2.5 (docs/architecture.md score_stances section), 2.6 (this plan's "Report" section)
  rollback:      git revert; VERSION 3→2 makes cached v3 entries foreign misses
units:
  - id: 1
    scope_id: stance-eval
    project: .
    depends_on: []
    module: crates/podling-core stance evaluation
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/src/plugin/nli.rs
        - crates/podling-core/src/plugin/embedding.rs
        - crates/podling-core/tests/ground_claims_live.rs
        - crates/podling-core/tests/openai_embeddings.rs
      docs:
        - docs/architecture.md
        - docs/plans/phase3-nli-ledger.md
      write:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/pairs.json
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: stance-rule
    project: .
    depends_on: [stance-eval]
    module: crates/podling-core score_stances rule
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/tests/stance_precision.rs
        - crates/podling-core/tests/fixtures/stance_pairs/scores.json
        - crates/podling-core/tests/pipeline.rs
      docs:
        - docs/architecture.md
      write:
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/tests/stance_precision.rs
        - docs/architecture.md
        - docs/plans/stance-precision.md
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: stance precision

## Outcome
score_stances stops over-calling Supports/Contradicts, so a Contested or Corroborated
status rests on evidence measured as precise, not on topic overlap.

## Scope Steps (executable core)

### Step 1 — stance-eval (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer · skills language-aware-planning · guards fmt/clippy/test
Depends on: none
- [x] 1.1 In `score_stances.rs`, extract the decision into `pub fn decide(pair: &StanceEvidence) -> Option<Stance>` (a small `pub struct StanceEvidence<'a> { claim: &'a str, premise: &'a str, similarity, entailment, contradiction: PerMille }`, so a text-based gate can be added later without changing the signature); `Judged::stance()` delegates to it. No behaviour change, VERSION stays 2 → accept: every existing `score_stances` test passes unchanged; `git diff` shows no test edits
- [x] 1.2 Write `tests/fixtures/stance_pairs/pairs.json`: ≥ 40 hand-labelled `{ id, claim, premise, label: "supports"|"contradicts"|"neither", note }` pairs, at least 10 per label, in the Tunguska domain the examples use; must include "Kulik reached the site in 1927" / "No impact crater was found" (neither), the 1907-vs-1908 pair (contradicts), the 80-million-trees paraphrase (supports), and topic-overlap distractors (same subject, different fact) → accept: an offline test parses it and checks ids are unique, labels valid, per-label counts ≥ 10
- [x] 1.3 Add an `#[ignore]` test `score_the_stance_pairs` in `tests/stance_precision.rs` that needs `PODLING_NLI_MODEL_DIR`, `PODLING_LIVE_EMBED_URL`, `PODLING_LIVE_EMBED_MODEL` (panics if unset, never skips), scores every pair with `CrossEncoderNli` (premise → claim, as the stage does) and `OpenAiEmbeddings` cosine, and writes `scores.json` `{ nli: <fingerprint>, embedding: <model>, pairs: [{ id, similarity_pm, entailment_pm, contradiction_pm }] }` → accept: run once live; `scores.json` committed with one row per pair
- [x] 1.4 Add a non-ignored test `stance_precision_report` that joins `pairs.json` and `scores.json`, runs `decide` on each, and prints a table: per stance (supports, contradicts) TP/FP/FN, precision, recall, plus each false positive's id → accept: `cargo test -p podling-core --test stance_precision -- --nocapture` prints the table; scores.json ids match pairs.json exactly (asserted)
- [x] 1.5 Record the "before" figures in this plan's "Report" section → accept: the section lists precision/recall for both stances under the VERSION 2 rule, and every FP id

### Step 2 — stance-rule (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer · skills language-aware-planning · guards fmt/clippy/test
Depends on: stance-eval
- [x] 2.1 From the "before" false positives only, choose the rule change (threshold move and/or a same-subject gate: e.g. a contradiction needs the claim and premise to share at least one non-number content word, `text::content_words` minus `text::is_number`); write the reasoning, the alternatives tried and their figures into "Report" → accept: each change names the FP ids it removes and the TPs it costs
- [x] 2.2 Make `stance_precision_report` print before (VERSION 2 rule, kept as a private `decide_v2` in the test file reproducing the old constants) and after (`decide`) side by side, and assert, for each stance, after-precision ≥ before-precision, after-precision strictly higher for at least one stance, and the Kulik/crater pair is not `Contradicts` under `decide` → accept: test prints both tables and passes
- [x] 2.3 Apply the change in `decide`; every new constant or flag goes into `config_fingerprint` → accept: all existing score_stances tests and `tests/pipeline.rs` contradiction/paraphrase tests pass unchanged
- [x] 2.4 Bump `ScoreStances::VERSION` 2 → 3 → accept: grep shows `VERSION: u32 = 3`; no podling-types file in `git diff main --stat`
- [x] 2.5 Update `docs/architecture.md` "score_stances" (thresholds / gate, and the measured precision with a link to this plan) → accept: the documented rule matches `decide`
- [x] 2.6 Fill this plan's "Report" with the after figures → accept: before and after both printed there

## Sequencing
Measure before changing: step 1 adds the instrument and records today's figures with no behaviour change; step 2 changes the rule only where the recorded figures justify it.

## Report

**Pair set.** 62 hand-labelled pairs in `crates/podling-core/tests/fixtures/stance_pairs/pairs.json`:
18 supports, 16 contradicts, 28 neither. 48 have a one-sentence premise; 14 (s17–s18, c15–c16,
n19–n28) have a two-sentence premise window, the shape `score_stances` actually scores
(`MAX_WINDOW_SENTENCES = 2`). Scored 2026-10-06 by `cross-encoder/nli-deberta-v3-base`
(weights BLAKE3 `fc98f663…`) and `nomic-embed-text` (Ollama), raw scores in `scores.json`.

**Before (VERSION 2 rule: entail ≥ 800 → Supports; contradiction ≥ 950 and similarity ≥ 600 → Contradicts):**

| stance | TP | FP | FN | precision | recall | false positives |
|---|---|---|---|---|---|---|
| supports | 13 | 0 | 5 | 100.0% | 72.2% | |
| contradicts | 16 | 1 | 0 | 94.1% | 100.0% | n21 |

Observations:
- **n21** is the topic-overlap false positive: claim "The explosion happened in June 1908." against
  the window "The explosion was heard hundreds of kilometres away. Kulik's expedition reached the
  site in 1927." — similarity 691, contradiction 991. The window shares the subject in one sentence
  and carries a different event's year in the other.
- The Kulik/crater pair does not reproduce sentence-to-sentence (n01 contradiction 4, n02 466) nor in
  the window shapes tried (n19 1, n20 1). The 0.903 recorded in phase3-nli-ledger was a different
  premise; the same failure family shows up as n21.
- Four "neither" pairs get contradiction ≥ 980 and are stopped only by the similarity gate: n06 (489),
  n08 (556), n12 (490), n16 (**594**, 6 per mille under the gate). n28 is at contradiction 939, 11
  under `CONTRADICT_PM`. The lowest-similarity true contradictions are c16 (650) and c04 (663).
- Supports has no false positive; its five misses (s04, s05, s06, s12, s16) are recall, not precision.

**Rule change (2.1), chosen from the one false positive, n21.** n21's contradiction comes
from a number in a sentence about something else: the window's only number (1927) sits
in "Kulik's expedition reached the site in 1927.", which shares no content word with the
claim; the sentence that does share the subject ("The explosion was heard…") has no
number. Every true number-against-number contradiction (c01, c02, c03, c05, c10, c15 and
the rest) puts its number in a sentence that also names the claim's subject.

Chosen: the **number-subject gate** (`numbers_share_the_subject` in `decide`). When claim
and premise both hold a number, `Contradicts` additionally needs some premise sentence
holding a number to share a non-number content word (`text::content_words` minus
`text::is_number`) with the claim. Pairs where either text has no number are untouched.
Removes FP n21; costs no TP. It has no tunable value, so it adds nothing to
`config_fingerprint`; the rule change is keyed by `VERSION` 2 → 3.

Alternatives tried on the same scores:

| change | contradicts TP | FP | precision | recall | why not |
|---|---|---|---|---|---|
| none (VERSION 2) | 16 | 1 | 94.1% | 100.0% | n21 |
| `MIN_CONTRADICT_SIMILARITY_PM` 600 → 700 | 14 | 0 | 100.0% | 87.5% | drops c04 (663) and c16 (650) |
| `CONTRADICT_PM` 950 → 995 | 16 | 0 | 100.0% | 100.0% | n21 is 991 vs the lowest TP 998: a 7 ‰ margin that fits this set's noise, not the cause |
| content-word overlap ratio (whole window) | — | — | — | — | n21 and c09 both share 1/3 of the claim's words; no cut separates them |
| **number-subject gate** | **16** | **0** | **100.0%** | **100.0%** | chosen |

**After (VERSION 3, `decide`), printed by `cargo test -p podling-core --test stance_precision -- --nocapture report`:**

```
before: VERSION 2 rule on the labelled pair set
  stance       TP  FP  FN  precision  recall  false positives
  supports     13   0   5     100.0%   72.2%
  contradicts  16   1   0      94.1%  100.0%  n21
after: score_stances::decide on the labelled pair set
  stance       TP  FP  FN  precision  recall  false positives
  supports     13   0   5     100.0%   72.2%
  contradicts  16   0   0     100.0%  100.0%
```

Supports is unchanged (no threshold moved). Contradicts precision 94.1% → 100.0% at
unchanged recall. On 62 pairs this says the gate fits the set, not that it generalises.

## Verification background
- Decision rule today: entail ≥ 800 → Supports; else contradiction ≥ 950 and similarity ≥ 600 → Contradicts — `crates/podling-core/src/stages/score_stances.rs` `Judged::stance` (~line 217)
- Thresholds are in the fingerprint — `score_stances.rs` `config_fingerprint`
- Kulik/crater 0.903 contradiction recorded — `docs/plans/phase3-nli-ledger.md:288`; `docs/architecture.md:356-359`
- Live-test conventions (panic when env unset) — `crates/podling-core/tests/ground_claims_live.rs:17-22`, `tests/openai_embeddings.rs:170-180`
- `content_words` / `is_number` — `crates/podling-core/src/text.rs:38`, `:50`
- NLI weights at `~/.cache/podling-models/nli-deberta-v3-base`; Ollama `nomic-embed-text` at `localhost:11434` (checked 2026-10-06)
- Strategist: Option B (score once with the real models, commit raw scores, evaluate offline every test run) over a 4-unit layer split or an example binary (no CI enforcement)

CONSUMERS:
- `score_stances::decide` / `StanceEvidence` (new, pub in podling-core): `score_stances.rs` `Judged::stance`, `tests/stance_precision.rs`. No existing consumer changes.
- `ScoreStances` public API unchanged: `crates/podling-core/src/pipeline.rs`, `stages/mod.rs:33`.

## Risk & rollback
- A ~40-pair set is small: any change is stated as fitting this set, not as general.
- Committed scores go stale if the model or embedder changes; `scores.json` records both, and regenerating is one ignored test.
- Rollback: git revert; the VERSION bump makes v3 cache entries misses after revert.

## Out of scope
Retrieval (RETRIEVE_K, MIN_RETRIEVAL_PM), cluster_claims and ground_claims thresholds, an LLM in the stance decision, podling-types changes, docs/architecture/speech*.
