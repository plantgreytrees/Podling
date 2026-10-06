---
slug: scrutinise-nli-extraction-grounding
goal: A faithful claim about the end of a very long source sentence is no longer dropped by ground_claims, and the CLI cache test covers the new stage.
classification: in-scope   # findings of /craftsman:scrutinise over f12b046..be10c28
tracker_rows: [TRACKER#scrutinise-nli-extraction-grounding/1]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)
coverage:
  contract:      N/A(Window stays crate-private; no exported type changes)
  data:          1.1.2 (both NLI stages bump VERSION, so old cache entries are never read)
  config:        N/A(no episode keys)
  security:      N/A(no new I/O)
  tests:         1.1.3, 1.1.4
  observability: N/A(unchanged)
  interface:     N/A(unchanged)
  docs:          1.1.5
  rollback:      git revert of the merge
units:
  - id: 1
    scope_id: window-word-cap
    project: .
    depends_on: []
    module: crates/podling-core/src/stages, crates/podling-cli/tests
    language: rust
    security: normal
    scope:
      read: [crates/podling-core/src/text.rs]
      docs: [docs/architecture.md]
      write: [crates/podling-core/src/stages/windows.rs, crates/podling-core/src/stages/ground_claims.rs, crates/podling-core/src/stages/score_stances.rs, crates/podling-cli/tests/cli.rs, docs/architecture.md]
    tooling: { implementer: implementer, gates: [code-reviewer], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Scrutinise: NLI grounding in extraction

## Findings

**F1, Correctness (major).** `windows()` (`crates/podling-core/src/stages/windows.rs:31`)
puts no length cap on a window. A source with a long unpunctuated run, such as a list
or a table flattened to text, is one "sentence" of up to the chunker's 800 words.
`CrossEncoderNli` truncates the pair at 512 tokens (longest side first), so the end of
the premise never reaches the model. A probe with a 120-item list scored a faithful
claim about item 1 at 0.965 and about item 117 at 0.617, so `ground_claims` rejects
true evidence and can drop a true claim. `score_stances` misses support the same way.

**F2, Test-coverage (minor).** `GROUNDING_STAGES` (`crates/podling-cli/tests/cli.rs:24`)
lists only `cluster_claims` and `score_stances`, so `stage_rows` filters out
`ground_claims`. `a_grounded_run_caches_the_new_stages_too` passes without ever seeing
the new stage's cache row.

## Tasks

- [x] 1.1.1 Add `MAX_WINDOW_WORDS` to `windows.rs`. A window of at most that many words
  is kept as it is. A two-sentence window over the cap is skipped, since its
  sentences appear as windows of their own. A single sentence over the cap becomes
  overlapping slices of `MAX_WINDOW_WORDS` words, half a slice apart, with the last
  slice ending at the sentence's end. Every span still slices the document.
  accept: unit test, a 300-word sentence gives slices of ≤ cap words covering every word, spans slice the document.
- [x] 1.1.2 Add `max_window_words` to the fingerprints of `GroundClaims` and
  `ScoreStances`, and bump both `VERSION`s to 2.
  accept: `cargo test -p podling-core` passes; the no-NLI keys test (`NO_NLI_KEYS`) still passes.
- [x] 1.1.3 Add `ground_claims` to `GROUNDING_STAGES`, and to the expected list in
  `a_grounded_run_caches_the_new_stages_too`.
  accept: that test asserts 9 stages, `ground_claims` among them.
- [x] 1.1.4 `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` pass.
- [x] 1.1.5 `docs/architecture.md` states the word cap where it describes premise windows.

## Verification background

CONSUMERS:
- `windows::windows` → `ground_claims.rs` (`run`), `score_stances.rs` (`run`). Both
  get the cap; both bump `VERSION`. `cluster_claims` embeds claims only.
- No-NLI runs never call either stage, so their keys and artifacts are unchanged.
