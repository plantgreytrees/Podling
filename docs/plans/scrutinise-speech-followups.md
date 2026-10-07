---
slug: scrutinise-speech-followups
goal: The generated-voice provenance tests again cover an empty field, and a test proves design.py records the hash of the clip it wrote.
idea: docs/ideas/natural-episode-speech.md
classification: in-scope   # /scrutinise speech-followups (range 177d60e..2b7bd68), Suggestions 3 and 4
tracker_rows: [TRACKER#scrutinise-speech-followups/ssf-tests]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)
coverage:
  contract:      N/A(test-only; no API/type change)
  data:          N/A(no persistence)
  config:        N/A
  security:      N/A(no new surface; restores coverage of the fail-closed provenance check)
  tests:         1.1, 1.2
  observability: N/A
  interface:     N/A
  docs:          N/A(no behaviour change)
  rollback:      git revert of the merge commit
units:
  - id: 1
    scope_id: ssf-tests
    project: .
    depends_on: []
    module: provenance tests (Rust + voice_design)
    language: rust, python
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/synthesize.rs
        - scripts/voice_design/design.py
        - scripts/voice_design/tests/test_design.py
      docs:
        - docs/architecture/speech.rules.md
      write:
        - crates/podling-core/src/stages/synthesize.rs
        - scripts/voice_design/tests/test_design.py
    arch: [ARCH-SPEECH-19, ARCH-SPEECH-15]
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [], guards: [cargo fmt, cargo clippy, cargo test, uv run pytest], mcp: [] }
---

# Plan: scrutinise-speech-followups

## Outcome
The generated-voice provenance tests again cover an empty field, and a test proves design.py records the hash of the clip it wrote.

## Scope Steps (executable core)

### Step 1 — ssf-tests (., rust+python, normal)
Tooling: implementer · gates code-reviewer · guards cargo fmt/clippy/test, uv run pytest
Depends on: none
- [ ] 1.1 In `a_generated_voice_with_bad_provenance_is_refused` (synthesize.rs tests), build the empty-model case from `provenance_for(<the clip's real hash>)` with `"model": ""`, and assert the message names `model` as well as the path → accept: the test fails if `ProvenanceError::Empty` is not reached (the message has "\"model\"").
- [ ] 1.2 Add a pytest that runs `design.main` with `snapshot`, `free_mib` and `generate` monkeypatched (no GPU or model) and asserts the written provenance JSON's `clip_blake3` equals the blake3 of the written clip's bytes → accept: `uv run --directory scripts/voice_design pytest -q` exits 0 with the new test.

## Sequencing
One unit.

## Verification background
- The empty-model case lacks `clip_blake3`, so it fails as a missing field — `crates/podling-core/src/stages/synthesize.rs:1122-1133`
- `main` hashes `args.out` after writing it — `scripts/voice_design/design.py:199-200`
- `main` calls `snapshot`, `free_mib`, `generate` — `scripts/voice_design/design.py:177-185`

CONSUMERS: none (test-only).

Disputed findings (no task): Suggestion 1, the `SCHEMA_VERSION` 8→9 bump, is kept because the user's goal for speech-followups asks for it explicitly. Suggestion 2, `blake3` 1.0.10 being new, is kept because it is pinned by sha256 in `scripts/voice_design/uv.lock`, its licence `CC0-1.0 OR Apache-2.0` was read from the installed METADATA, and the dependency audit is recorded in the speech-followups unit 2 tracker evidence.

## Risk & rollback
Tests only. Revert the merge commit.

## Out of scope
Any change to production code.
