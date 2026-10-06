---
slug: script-verbatim-retry
craftsman_version: 2.1.0
goal: A model that paraphrases a quote, or puts its own words in quotation marks, is told exactly what to fix and retried once, instead of the episode failing at analysis.
classification: in-scope   # follow-up from the live llama3.1:8b run after scrutinise-phase2-llm-provider (2 quote_verifier errors)
tracker_rows: [TRACKER#script-verbatim-retry/1]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial: one crate, no exported type or schema change; root-only agent mode)
coverage:
  contract:      1.4 (WriteScript::VERSION bump; no exported type, schema or CLI change)
  data:          N/A(no persistence/migration; the VERSION bump invalidates cached scripts)
  config:        N/A(no new episode keys)
  security:      1.2 (model output validated before it becomes a Script; fails closed after one retry)
  tests:         1.1, 1.3
  observability: N/A(complete_validated already logs retries; the rejection reason is in the error)
  interface:     N/A(no CLI flag or output shape change)
  docs:          1.5
  rollback:      git revert; the VERSION bump only invalidates cached scripts
units:
  - id: 1
    scope_id: script-verbatim-retry
    project: .
    depends_on: []
    module: crates/podling-core/src/stages/script.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/analyser.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/tests/pipeline.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/analyser.rs
        - crates/podling-core/src/text.rs
        - docs/architecture.md
    tooling: { implementer: implementer, gates: [security-auditor], skills: [rust], guards: [cargo fmt, cargo clippy -D warnings, cargo test], mcp: [] }
---

# Plan: script-verbatim-retry

## Outcome
A model that paraphrases a quote, or puts its own words in quotation marks, is told exactly what to fix and retried once, instead of the episode failing at analysis.

## Scope Steps (executable core)

### Step 1 — script-verbatim-retry (., rust, high)
Tooling: implementer implementer · gates security-auditor · skills rust · guards fmt, clippy, test
Depends on: none
- [x] 1.1 Move `quoted_spans` and `MIN_QUOTED_WORDS` (`plugin/analyser.rs:20-40`) into `text.rs` as `pub(crate) fn quotations(text: &str) -> Vec<&str>`, which returns only spans of `MIN_QUOTED_WORDS`+ words. `QuoteVerifier` calls it; the span tests move with it. → accept: the analyser tests are unchanged and pass; there is one span detector in the crate.
- [x] 1.2 In `build_script` (`stages/script.rs:121-155`), after resolving a turn's quotes, return `Err` if (a) a resolved quote's text is not contained in the turn's text: `turn {i} must speak quote {n} word for word: "<quote text>"`; or (b) a `quotations(text)` span is not contained in any of the turn's quote texts: `turn {i} puts "<span>" in quotation marks, but no quote reference covers it; add the reference or drop the marks`. → accept: `complete_validated` feeds the reason back and retries once.
- [x] 1.3 Tests in `script.rs`: a scripted provider that paraphrases once, then speaks the quote verbatim, succeeds in exactly 2 calls; one that always paraphrases fails with `InvalidProviderOutput` whose message names the turn and the quote text; an unreferenced quotation fails the same way; the FakeLlm script and the Tunguska replay (`tests/pipeline.rs`) still pass. → accept: `cargo test --workspace` is green.
- [x] 1.4 Bump `WriteScript::VERSION` 3→4 (`script.rs:46`), with a one-line comment. INSTRUCTIONS are unchanged, so `PROMPT_VERSION` stays 1. → accept: `changing_the_model_invalidates_the_llm_stages_only` passes.
- [x] 1.5 `docs/architecture.md`: add to the "Provider output is never trusted" list: a turn that doesn't speak its quote verbatim, or that quotes words no reference covers. Note that `QuoteVerifier` still re-checks independently afterwards. → accept: the list matches `build_script`.

## Sequencing
Single step: 1.1 (shared helper) → 1.2 (use it) → 1.3 tests → 1.4 bump → 1.5 docs. No CHANGELOG (none in repo).

## Verification background   (citations — for the reviewer, not the executor)
- The live run on `examples/tunguska/episode-ollama.toml` with llama3.1:8b gave two `turn does not speak the quote verbatim` errors at analysis (turns 2 and 3).
- `build_script` validates citations and resolves quotes, but never compares quote text to turn text — `stages/script.rs:121-155`.
- `complete_validated` re-asks once with the reason appended — `plugin/llm.rs:113`.
- Span detection now lives in `plugin/analyser.rs:20-40`.

CONSUMERS:
- `quoted_spans` / `MIN_QUOTED_WORDS` (private, moving to `text::quotations`) → `plugin/analyser.rs:88-90` only.
- `build_script` (private) → `WriteScript::run` via `complete_validated`, `stages/script.rs:70`.
- `WriteScript::VERSION` → cache key only.
- Execution correction: `tests/pipeline.rs` `a_quotation_no_quote_ref_covers_is_an_analysis_error` (added by scrutinise-phase2-llm-provider 1.4) depended on the script stage *accepting* an unreferenced quotation. It is now `a_quotation_no_quote_ref_covers_fails_the_script_stage`, and the file was added to the unit's write scope.

## Risk & rollback
- A model that can't quote verbatim now fails at the script stage (after one retry) rather than at analysis. The run already failed in that case, so the net change is one more chance plus a clearer error. Revert the commit to undo.

## Out of scope
- Auto-repairing the turn text (e.g. splicing the quote in); the model must fix its own wording.
- Semantic claim clustering (the paraphrase corroboration gap), a later NLI phase.
