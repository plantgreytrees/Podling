---
slug: quote-placeholders
goal: A real local model (llama3.1:8b on Ollama) turns the Tunguska example into a passing episode, because the model marks where a quote goes with `{{quote:N}}` instead of retyping it, and claim grounding accepts names taken from the title or headings.
classification: in-scope   # docs/handoff.md "Fixes" 1 and 2; .claude/CLAUDE.md "an LLM may select a quote but never write one"
tracker_rows: [TRACKER#quote-placeholders/1, TRACKER#quote-placeholders/2, TRACKER#quote-placeholders/3]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(root-only agent mode: agent-mode-guard blocks non-strategist Task; the plan-strategist pass found 3 consumers the handoff omits, folded in as 1.6–1.8)
coverage:
  contract:      1.2 (PROMPT_VERSION 1→2; DraftTurn.text now carries placeholders), 1.3 (WriteScript::VERSION 4→5), 1.6 (FakeLlm fingerprint 2→3), 1.7, 1.8, 2.1 (ClaimInput gains `titles`), 2.2 (pipeline.rs builds it), 2.4 (ExtractClaims::VERSION 3→4)
  data:          N/A(no persistence/migration; the version bumps invalidate cached `extract_claims` and `script` outputs)
  config:        N/A(no new episode keys or env vars)
  security:      1.1, 1.4 (model output validated before it becomes a Script; single-pass substitution so a source sentence containing `{{quote:N}}` can't re-expand; fails closed after one retry)
  tests:         1.1, 1.5, 1.6, 1.7, 1.8, 2.3, 3.1, 3.2, 3.3
  observability: N/A(complete_validated already logs each rejection reason at warn; no new path)
  interface:     N/A(no CLI flag or artifact shape change; Script/Turn types unchanged)
  docs:          1.9, 2.5
  rollback:      git revert of the branch; bumped versions only invalidate cache entries, nothing persisted needs migrating
units:
  - id: 1
    scope_id: quote-placeholders
    project: .
    depends_on: []
    module: crates/podling-core/src/stages/script.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/plugin/analyser.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/llm/write_script.json
        - crates/podling-cli/tests/cli.rs
      docs: [docs/handoff.md, docs/architecture.md, README.md]
      write:
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/llm/write_script.json
        - crates/podling-cli/tests/cli.rs
        - docs/architecture.md
        - README.md
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: heading-grounding
    project: .
    depends_on: []
    module: crates/podling-core/src/stages/extract_claims.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/stages/extract_claims.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-types/src/document.rs
        - crates/podling-core/tests/pipeline.rs
      docs: [docs/handoff.md, docs/architecture.md]
      write:
        - crates/podling-core/src/stages/extract_claims.rs
        - crates/podling-core/src/pipeline.rs
        - docs/architecture.md
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: live-acceptance
    project: .
    depends_on: [1, 2]
    module: examples/tunguska
    language: rust
    security: normal
    scope:
      read:
        - examples/tunguska/episode-ollama.toml
        - crates/podling-cli/tests/cli.rs
      docs: [docs/handoff.md]
      write: [docs/plans/quote-placeholders.md]
    tooling: { implementer: implementer, gates: [], skills: [], guards: [cargo test], mcp: [] }
---

# Plan: quote placeholders and heading-aware grounding

## Outcome
`podling run --episode examples/tunguska/episode-ollama.toml` against llama3.1:8b exits 0 with no analyser `error` finding, twice in a row from a cold cache. The model never types quoted words: it writes `{{quote:N}}` and the code puts the source sentence there.

## Scope Steps (executable core)

Every command runs with `PATH="$HOME/.cargo/bin:$PATH"` (cargo isn't on the default PATH; `docs/handoff.md:96`).

### Step 1 — quote-placeholders (., rust, high)
Tooling: implementer implementer · gates code-reviewer, security-auditor, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: none

Design decisions (fixed here, so the executor doesn't have to choose):
- **Syntax.** `{{quote:N}}`, where N counts from 0 over the turn's own `quotes` array.
- **Wrapper.** Substitute the sentence as `“<sentence>”`, with curly quotes. `quotations()` closes `“` only on `”` (`text.rs:39-44`). A straight `"` inside a source sentence therefore can't pair with a mark in the model's text.
- **Absorb typed marks.** If the model wraps a placeholder in marks (`"{{quote:0}}"` or `“{{quote:0}}”`), drop those marks before any check, so the output never has double marks.
- **Check order.** (a) parse the draft and `resolve` the refs. (b) Run `quotations()` on the raw text, with the absorbed marks removed. Any span is model-typed, so reject it with a message telling the model to use `{{quote:N}}`. (c) Parse the placeholders. Reject an index with no ref, a ref with no placeholder, and a malformed or unclosed `{{quote` token, each with its own message. (d) Substitute in a single pass over the raw text; never rescan inserted text. (e) Run `check_quotes_are_spoken` on the result, unchanged, as the independent check.
- **Duplicates.** A placeholder may be used more than once. It's harmless, since the text inserted is the same verbatim sentence each time.

- [x] 1.1 Add a `pub(crate)` substitution helper in `crates/podling-core/src/text.rs`, plus a small error enum whose variants carry the index or token. Inputs: raw text and the resolved quote texts. It absorbs typed marks, validates the placeholders as in (b)–(c), and substitutes as in (d). Unit tests: happy path; duplicate use; absorbed straight and curly marks; unknown index; unused quote; malformed `{{quote:x}}` and unclosed `{{quote:0`; a source sentence that itself contains `{{quote:0}}` is inserted literally and not re-expanded; a source sentence containing a straight `"` → accept: `cargo test -p podling-core text::` passes.
- [x] 1.2 In `crates/podling-core/src/plugin/llm.rs`, bump `PROMPT_VERSION` 1 → 2 and extend its doc comment with why ("turn text carries `{{quote:N}}` placeholders"). Update the `DraftTurn` doc to say `text` holds placeholders, not quoted words → accept: `grep -n "PROMPT_VERSION: u32 = 2" crates/podling-core/src/plugin/llm.rs` matches.
- [x] 1.3 In `crates/podling-core/src/stages/script.rs`, rewrite INSTRUCTIONS rule 3. The model writes `{{quote:N}}` in `text` where the N-th entry of `quotes` is spoken. It never types quoted words or quotation marks, and every entry in `quotes` needs exactly that placeholder. Update the reply-shape example to show a placeholder. Bump `WriteScript::VERSION` 4 → 5 with a `// 5:` comment line → accept: `grep -n "VERSION: u32 = 5" crates/podling-core/src/stages/script.rs` matches and INSTRUCTIONS contains `{{quote:`.
- [x] 1.4 In `build_script` (`script.rs:122-155`), after `resolve`, call the 1.1 helper and map its errors to `turn {i}: …` strings that tell the model the exact fix. Put the substituted text in `Turn::text`, then call `check_quotes_are_spoken` on it, unchanged → accept: `cargo build -p podling-core` passes. `check_quotes_are_spoken` and `QuoteVerifier` are still present and unchanged.
- [x] 1.5 Update the stage tests in `script.rs`:
  - `Speaking` texts use placeholders (`VERBATIM` becomes `A witness said: {{quote:0}}`).
  - The retry test proves a first reply that is missing its placeholder is corrected on the second call.
  - Add one `WriteScript::run` test per rejection: an index with no ref, a ref with no placeholder, a model-typed quotation outside a placeholder, and a malformed placeholder. Each asserts `InvalidProviderOutput` with `turn 0` and the reason.
  - Add a test where the finished turn text contains `“The sky split in two.”` → accept: `cargo test -p podling-core stages::script` passes.
- [x] 1.6 Make `FakeLlm::opening` (`llm.rs:217-244`) emit `… It begins with this: {{quote:0}}` and drop the typed words. Bump the `FakeLlm` fingerprint version 2 → 3 (`llm.rs:262`). Update `write_script_quotes_the_first_sentence_by_reference` to assert that `text` contains `{{quote:0}}` and no `"` → accept: `cargo test -p podling-core plugin::llm` passes.
- [x] 1.7 Replace the typed quotations in `crates/podling-core/tests/fixtures/llm/write_script.json` (turns 0 and 2) with `{{quote:0}}`. In `tests/pipeline.rs:280`, narrow the Replay assert to `{{claim:` and `{{chunk:`, so the placeholder the model is meant to write isn't counted as an unfilled fixture token → accept: `cargo test -p podling-core --test pipeline` passes, including `replayed_model_output_gives_the_same_ledger_statuses_and_verbatim_quotes`.
- [x] 1.8 Make `tiny_model` in `crates/podling-cli/tests/cli.rs:248-256` write `"The first source says: {{quote:0}}"` → accept: `cargo test -p podling-cli` passes (non-ignored tests).
- [x] 1.9 Docs. In `docs/architecture.md`:
  - "Sentence-addressed quotes" (~line 159): add that the turn text marks each quote with `{{quote:N}}`, and the stage substitutes the sentence in curly quotes.
  - The `InvalidProviderOutput` list (~line 54): add the three placeholder rejections.
  - The artifact-flow diagram (~line 36): mention placeholders.
  - Line ~201: widen "can only point" to cover the spoken text.

  In the README, "What the model can and can't do" (~line 136): the model marks where a quote goes and never types it → accept: `grep -n "{{quote:N}}" docs/architecture.md README.md` matches in both files.

### Step 2 — heading-grounding (., rust, high)
Tooling: implementer implementer · gates code-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: none (it shares no file with Step 1, so it can run in either order)

- [x] 2.1 Add `pub titles: BTreeMap<DocumentId, String>` to `ClaimInput` (`extract_claims.rs:74-78`), with a doc comment: "the title of each chunk's document; its words count as grounding". Change `is_grounded(claim, chunk_text)` to take the reference text as `&[&str]` (chunk text, document title, then each heading of `heading_path`) and build `have` from all of them. The number-must-appear rule and the 60% share don't change. In `run`, pass the chunk's title, or no title when the map has none, plus `chunk.heading_path()` → accept: `cargo build -p podling-core` passes.
- [x] 2.2 In `crates/podling-core/src/pipeline.rs:62`, fill `titles` from `documents` (`d.id().clone()` → `d.title().to_owned()`) → accept: `cargo build --workspace` passes.
- [x] 2.3 Tests in `extract_claims.rs`:
  - A chunk with the heading path `["Tunguska event"]` and the text "It happened in 1908." The claim "The Tunguska event happened in 1908." (content words tunguska, event, happened, 1908: 2/4 without the heading, 4/4 with it) is accepted through `ExtractClaims::run` with one call.
  - The same claim against the same chunk with no heading is rejected by `is_grounded`. This proves the heading is what grounds it.
  - The document title "Kulik expedition" and the chunk "He arrived in 1927." make the claim "The Kulik expedition arrived in 1927." accepted (2/4 without the title, 4/4 with it). Without the title, it is rejected.
  - An invented claim is still rejected when a heading is present: the heading doesn't ground words it doesn't contain.
  - `an_invented_claim_is_rejected_after_one_retry` still passes unchanged.
  - Adjust the test `input()` helper to fill `titles` → accept: `cargo test -p podling-core stages::extract_claims` passes.
- [x] 2.4 Bump `ExtractClaims::VERSION` 3 → 4 with a `// 4:` comment ("grounding also sees the document title and heading path") → accept: `grep -n "VERSION: u32 = 4" crates/podling-core/src/stages/extract_claims.rs` matches.
- [x] 2.5 In `docs/architecture.md`, "Grounding check (a stopgap)" (~line 175): the word set includes the document title and the chunk's heading path. Also say that the model still sees only the chunk text → accept: the section mentions "heading".

### Step 3 — live-acceptance (., rust, normal)
Depends on: 1 (quote-placeholders), 2 (heading-grounding)

- [ ] 3.1 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` → accept: exits 0.
- [ ] 3.2 Live run, twice, each time with a new empty `--cache-dir` (`mktemp -d`): `cargo run -p podling-cli -- run --episode examples/tunguska/episode-ollama.toml --cache-dir <empty> --out <dir>`. Ollama runs in the container `infra_docker_compose-ollama-1` on port 11434 → accept: both runs exit 0, and `jq '[.body.findings[] | select(.severity=="error")] | length' <out>/analysis.json` prints `0` both times.
- [ ] 3.3 `PODLING_LIVE_LLM_URL=http://localhost:11434/v1 PODLING_LIVE_LLM_MODEL=llama3.1:8b cargo test -p podling-cli -- --ignored live` → accept: passes. If a live run fails, record the exact error in "Risk & rollback" below. Don't loosen a check to make it pass; hand the failure to `/craftsman:investigate`.

## Sequencing
Steps 1 and 2 are independent (no shared file, separate stage versions). Do Step 1 first because it removes the known live blocker (`docs/handoff.md:31-34`). Within Step 1, write the helper and its tests first (1.1), then the prompt/version changes, the stage wiring and tests, then fixtures and mocks (1.6–1.8, which the version bump and new rule break), and docs last. Step 3 needs both.

## Verification background   (citations — for the reviewer, not the executor)
- Live failure: llama3.1:8b paraphrases the sentence it references, even on retry. `docs/handoff.md:27-34`.
- Invariant being extended: "an LLM may select a quote but never write one". `.claude/CLAUDE.md` (Decisions, Quotes).
- `build_script` resolves the refs and then checks that each quote is spoken. `crates/podling-core/src/stages/script.rs:138-145`.
- `check_quotes_are_spoken` rejects an unspoken quote and an uncovered quotation. `script.rs:161-187`. `QuoteVerifier` repeats both checks. `crates/podling-core/src/plugin/analyser.rs:40-72`.
- `quotations()` pairs straight marks with straight marks and `“` with `”`, and ignores spans under 3 words. `crates/podling-core/src/text.rs:27-52`.
- `ClaimInput` has no title data today. `extract_claims.rs:74-78`. `Document::title` is at `crates/podling-types/src/document.rs:90`, `Chunk::heading_path` at `document.rs:197`.
- `is_grounded` compares against the chunk text only. `extract_claims.rs:55-71`, called at `:121`.
- The extraction request shows the model only `chunk_text`. `extract_claims.rs:113`.
- There are two separate `PROMPT_VERSION` consts. Only `plugin/llm.rs:13` (stage prompts) changes. `plugin/openai.rs:26` is request layout and stays at 1.
- Cache-bump rules are at `docs/architecture.md:105-114`.
- The Tunguska sources contain no quote marks (`grep -rE '["“”]' examples/tunguska/sources` is empty), so embedded-mark handling is covered by unit tests only.

CONSUMERS:
- `PROMPT_VERSION` (llm.rs:13) → `stages/script.rs:55`, `stages/extract_claims.rs:97` (both config fingerprints; they pick up the value, no edit needed); re-export `plugin/mod.rs:21`.
- `DraftTurn.text` semantics (llm.rs:86-96) → `stages/script.rs` `build_script` (1.4); `FakeLlm::opening` llm.rs:231 (1.6); `llm.rs:343` test (1.6); `tests/fixtures/llm/write_script.json` (1.7), via the Replay provider in `tests/pipeline.rs:230-285`, whose `{{` assert is at `:280` (1.7); `crates/podling-cli/tests/cli.rs:231-257` `tiny_model` (1.8); `plugin/openai.rs` passes the text through untouched (no edit).
- `WriteScript::VERSION` (script.rs:47) → `stage::cached` cache key only.
- `FakeLlm::fingerprint` (llm.rs:262) → the config fingerprints of both LLM stages. The bump invalidates cached fake runs.
- `ClaimInput` (extract_claims.rs:74) → re-export `stages/mod.rs:12`; the constructor at `pipeline.rs:62` (2.2); test helper `extract_claims.rs:177` (2.3). No other constructor (`grep -rn "ClaimInput {"`).
- `is_grounded` (extract_claims.rs:55) → `extract_claims.rs:121` and the tests at `:314-321`. Private, so no other consumer.
- `ExtractClaims::VERSION` (extract_claims.rs:89) → cache key only.

## Risk & rollback
- **The live model may still misbehave.** llama3.1:8b may leave out the placeholder, write `{{quote:1}}` with one ref, or type the sentence anyway. Each is now a targeted rejection with one retry, and the error messages must name the exact fix. If both runs of 3.2 aren't clean, that's a finding for `/craftsman:investigate`. It is not a reason to relax a check.
- **Grounding can still reject a paraphrase that involves no heading.** Fix 2 doesn't address that (`docs/handoff.md:66-70` scopes it to headings/title).
- **Wider grounding admits slightly more.** A claim can now lean on title and heading words. The number rule and the 60% share are unchanged, and headings are source text, so they aren't model-invented.
- **Rollback.** Revert the branch. The version bumps only invalidate cache entries.

## Out of scope
- Showing the title or headings to the extraction model (a prompt change beyond the handoff).
- Semantic claim clustering (needs NLI).
- Token budgeting (`docs/handoff.md:100-105`).
- Merging `worktree-phase2-llm-provider` into `main`. This branch is stacked on it. The handoff (`docs/handoff.md:92-95`) and the final merge step belong to the user.
- Removing the leftover `phase1-core-contracts` worktree (a user housekeeping step, `docs/handoff.md:98-100`).
