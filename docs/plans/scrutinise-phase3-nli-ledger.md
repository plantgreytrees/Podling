---
slug: scrutinise-phase3-nli-ledger
craftsman_version: 2.1.0
goal: The script stage's new compact ledger view is versioned as the bump rules require, and a test keeps claim evidence out of the script request.
classification: in-scope   # /scrutinise of phase3-nli-ledger (range 9d791cd..99b8f80), findings F1–F2
tracker_rows: [TRACKER#scrutinise-phase3-nli-ledger/1]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)
coverage:
  contract:      1.1 (PROMPT_VERSION 2→3); no exported type changes
  data:          N/A(no persistence; the bump invalidates cached extract_claims and script outputs)
  config:        N/A(no new keys)
  security:      N/A(no auth, secrets or external I/O touched)
  tests:         1.2, 1.3
  observability: N/A(no new failure path)
  interface:     N/A(no CLI or artifact shape change)
  docs:          N/A(docs/architecture.md rule 4 already states the rule being applied)
  rollback:      git revert; the bump only invalidates cache entries
units:
  - id: 1
    scope_id: script-prompt-version
    project: .
    depends_on: []
    module: crates/podling-core
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/stages/script.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/stages/script.rs
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Scrutinise fixes: phase3-nli-ledger

## Unit 1 — script-prompt-version

- [x] 1.1 Bump `PROMPT_VERSION` in `crates/podling-core/src/plugin/llm.rs:16` to 3, adding the history line
      "3: the script request's ledger lists each claim's id, text and status (`LedgerClaim`), without evidence."
      → accept: `cargo test --workspace` passes; `no_nli_config_writes_todays_artifacts` (golden) still passes, since the constant is only in cache keys
- [x] 1.2 Add a unit test in `crates/podling-core/src/stages/script.rs` (reuse the `Recording` LLM at `script.rs:633`) that runs `WriteScript` on a ledger with evidence and asserts every item of the request's `input["ledger"]` has exactly the keys `id`, `text`, `status`
      → accept: the test passes, and fails if `LedgerClaim` gains an `evidence` field (check by adding one temporarily)
- [x] 1.3 Final gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
      → accept: all pass

## Verification background

**F1 (Correctness, minor).** Unit 6 of phase3-nli-ledger changed the script request's
`ledger` from full ledger entries to `LedgerClaim { id, text, status }`
(`crates/podling-core/src/stages/script.rs:72`, `crates/podling-core/src/plugin/llm.rs:69`)
and bumped `WriteScript::VERSION` to 7, but left `PROMPT_VERSION` at 2.
`docs/architecture.md` "The five bump rules", rule 4: "You changed a prompt or the shape
of an LLM input. Bump `PROMPT_VERSION`." Caching was not stale (the stage version bump
already invalidates script outputs), so this is conformance, not a live bug.

**F2 (Test coverage).** Nothing pins the request shape. The replay LLM in
`crates/podling-core/tests/pipeline.rs:288` parses `Vec<LedgerClaim>`, which would still
succeed with an extra `evidence` field. The failure this guards against (llama3.1:8b
citing a chunk id from merged evidence as a claim id) only showed in a live run.

CONSUMERS:
- `PROMPT_VERSION` (`crates/podling-core/src/plugin/llm.rs:16`) →
  `crates/podling-core/src/stages/extract_claims.rs:92` (config fingerprint),
  `crates/podling-core/src/stages/script.rs:61` (config fingerprint),
  re-exported at `crates/podling-core/src/plugin/mod.rs:30`.
  `crates/podling-core/src/plugin/openai.rs:25` is a separate, unrelated constant.
  No test asserts the value; CLI cache tests start from an empty cache.

## Execution notes

Merged at `c58315d`. For 1.2 the mutation check put the old request back (`"ledger": input.ledger.entries()`, full entries with evidence) rather than adding a field to `LedgerClaim`; the test failed as expected, then the code was restored. Suite on the merged base: 195 passed, 0 failed.
