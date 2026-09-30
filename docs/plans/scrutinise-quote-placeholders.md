---
slug: scrutinise-quote-placeholders
goal: A model can't slip an invented quotation past the script stage with a stray quotation mark, and naming the episode's topic no longer makes an invented claim look grounded.
classification: in-scope   # /scrutinise of quote-placeholders (range c549c54..8fcf3b5), findings F1–F3; F4 logged as risk
tracker_rows: [TRACKER#scrutinise-quote-placeholders/1, TRACKER#scrutinise-quote-placeholders/2, TRACKER#scrutinise-quote-placeholders/3]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(root-only agent mode blocks non-strategist Task; plan-strategist re-derived both fixes independently and added the stray-`”` case and the inches message)
coverage:
  contract:      1.1 (WriteScript::VERSION 5→6), 2.1 (ExtractClaims::VERSION 4→5); no exported type changes (`is_grounded` and `PlaceholderError` are private / pub(crate))
  data:          N/A(no persistence; version bumps invalidate cached script and extract_claims outputs)
  config:        N/A(no new keys)
  security:      1.1, 1.2, 2.1 (fail-closed validation of model output; grounding guard tightened)
  tests:         1.3, 2.2, 3.1, 3.2
  observability: N/A(complete_validated already logs each rejection reason)
  interface:     N/A(no CLI or artifact shape change)
  docs:          1.4, 2.3
  rollback:      git revert; the bumps only invalidate cache entries
units:
  - id: 1
    scope_id: quote-mark-balance
    project: .
    depends_on: []
    module: crates/podling-core/src/text.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/text.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/analyser.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-core/src/text.rs
        - crates/podling-core/src/stages/script.rs
        - docs/architecture.md
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor],
               skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: grounding-share
    project: .
    depends_on: []
    module: crates/podling-core/src/stages/extract_claims.rs
    language: rust
    security: high
    scope:
      read: [crates/podling-core/src/stages/extract_claims.rs]
      docs: [docs/architecture.md]
      write:
        - crates/podling-core/src/stages/extract_claims.rs
        - docs/architecture.md
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor],
               skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: live-recheck
    project: .
    depends_on: [1, 2]
    module: examples/tunguska
    language: rust
    security: normal
    scope:
      read: [examples/tunguska/episode-ollama.toml]
      docs: []
      write: [docs/plans/scrutinise-quote-placeholders.md]
    tooling: { implementer: implementer, gates: [], skills: [], guards: [cargo test], mcp: [] }
---

# Plan: scrutinise fixes for quote placeholders

## Outcome
A model can't slip an invented quotation past the script stage with a stray quotation mark, and naming the episode's topic no longer makes an invented claim look grounded.

## Scope Steps (executable core)

Every command runs with `PATH="$HOME/.cargo/bin:$PATH"`.

### Step 1 — quote-mark-balance (., rust, high)
Tooling: implementer implementer · gates code-reviewer, security-auditor · guards cargo fmt/clippy/test
Depends on: none

- [x] 1.1 In `fill_quote_placeholders` (`crates/podling-core/src/text.rs`), after the typed-quotation check, scan the model's own text (the parts, with each placeholder as one token) with the same state machine as `quotations`. Reject when a `"` or `“` is still open at the end, or when a `”` appears while nothing is open. The rejection is a new `PlaceholderError::Unclosed(String)` holding the mark and up to 40 following characters. Its message says to remove the mark, that quotes come only from `{{quote:N}}`, and to write inches as a word. Leave `quotations` unchanged (QuoteVerifier uses it). Bump `WriteScript::VERSION` 5 → 6 with a `// 6:` line in `stages/script.rs` → accept: `grep -n "VERSION: u32 = 6" crates/podling-core/src/stages/script.rs` matches.
- [x] 1.2 (F3) Make the placeholder token in that scan the placeholder's own text (`{{quote:N}}`) instead of `_`, so a `Typed` span shows the model what it wrote. Cap the span echoed by `Typed`, `Malformed` and `Unclosed` at 80 characters, with `…` when cut → accept: a unit test that `"{{quote:0}} and {{quote:1}}"` gives a `Typed` span containing `{{quote:0}}`, and a 200-character typed span is echoed as at most 81 characters.
- [x] 1.3 Tests. Unit tests in `text.rs`:
  - `He said "oops. {{quote:0}} Then “every tree caught fire at once” ended.` → `Unclosed`
  - a straight-quote version, `He said “oops. {{quote:0}} Then "every tree caught fire at once" ended.` → `Unclosed`
  - `a 5" shell {{quote:0}}` → `Unclosed`
  - `fell” {{quote:0}}` → `Unclosed`
  - `{{quote:0}}”` → `Unclosed`
  - `“{{quote:0}}”` → Ok (absorbed)
  - a quote whose text is `He said “hello` inserted by `{{quote:0}}` → Ok

  A stage test in `script.rs`: the first case through `WriteScript::run` is `InvalidProviderOutput` with `turn 0` in the message, after 2 calls → accept: `cargo test -p podling-core` passes.
- [x] 1.4 In `docs/architecture.md`, "Sentence-addressed quotes", add that the model's text must not contain a stray or unclosed quotation mark. One would hide the quotation check, because `quotations` stops at an unmatched opener. Note that `'…'`, `«…»` and `„…“` are not scanned → accept: the section mentions "unclosed".

### Step 2 — grounding-share (., rust, high)
Tooling: implementer implementer · gates code-reviewer, security-auditor · guards cargo fmt/clippy/test
Depends on: none

- [x] 2.1 Change `is_grounded` in `crates/podling-core/src/stages/extract_claims.rs` to `is_grounded(claim, chunk_text, names: &[&str])`, where `names` holds the document title and the chunk's headings. Let C be the chunk text's content words, T the content words of `names`, W the claim's content words, and N = W \ T. The claim is grounded only when N is non-empty, every number in W is in C ∪ T, and |N ∩ C| ≥ 0.6·|N|. Update the call in `run` and the doc comment. Bump `ExtractClaims::VERSION` 4 → 5 with a `// 5:` line → accept: `grep -n "VERSION: u32 = 5" crates/podling-core/src/stages/extract_claims.rs` matches.
- [x] 2.2 Tests:
  - These stay accepted:
    - `a_claim_naming_a_term_found_only_in_a_heading_is_accepted`
    - `a_claim_naming_a_term_found_only_in_the_title_is_accepted`
    - `a_paraphrase_of_the_passage_is_accepted`
    - "The Tunguska blast flattened trees." against that same sentence with title "The Tunguska event"
  - These are rejected:
    - "The Tunguska event was caused by a comet impact." against "No impact crater was found near the site." with title "The Tunguska event"
    - "The Tunguska event." (only title words)
    - "Tunguska happened yesterday" against "It happened in 1908." with heading "Tunguska event"
    - "The Tunguska event happened in 1907." (the number)
  - Rewrite `grounding_draws_on_every_piece_of_context` for the new signature → accept: `cargo test -p podling-core stages::extract_claims` passes.
- [x] 2.3 In `docs/architecture.md`, "Grounding check": title and heading words can name the subject, but they don't count toward the 60% share. The rest of the claim must be in the chunk text, and a claim made only of title or heading words is rejected → accept: the section says title and heading words "don't count toward" the share.

### Step 3 — live-recheck (., rust, normal)
Depends on: 1, 2

- [x] 3.1 `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` → accept: exits 0.
- [x] 3.2 Two live runs of `target/debug/podling run --episode examples/tunguska/episode-ollama.toml`, each with a new empty `--cache-dir`, plus `PODLING_LIVE_LLM_URL=http://localhost:11434/v1 PODLING_LIVE_LLM_MODEL=llama3.1:8b cargo test -p podling-cli -- --ignored live` → accept: all exit 0 and each `analysis.json` has 0 `error` findings. The model varies from run to run. If the tighter grounding rejects a real claim, record the claim and the scores here and hand the case to `/craftsman:investigate`. Don't loosen the rule.

**Live results (2026-09-30, llama3.1:8b).** The workspace passes fmt, clippy and all 147 tests. Recheck run A had a cold cache, exited 0 with 0 errors, and verified 2 quotes; the ledger has 9 claims, so the tighter grounding rejected nothing the model extracted. Recheck run B also had a cold cache and exited 0 with 0 errors. Its first script reply left out a `{{quote:0}}`, and the retry dropped the reference, so it verified 0 quotes. The script stage took 213 s in run A and 278 s in run B. Run B made two requests at about 140 s each, under the 300 s per-request timeout. The ignored live test passes. Results vary from run to run.

## Sequencing
Steps 1 and 2 share only `docs/architecture.md` (different sections) and can run in either order. Step 3 runs last.

## Verification background
- `quotations` opens on `"`/`“`, and an unmatched opener swallows the rest of the text. `crates/podling-core/src/text.rs:33-52`. The existing test asserts that unbalanced text yields nothing (`text.rs` tests, `finds_straight_and_curly_quotations_but_not_scare_quotes`).
- The typed check in `fill_quote_placeholders` runs `quotations` on `bare`. `check_quotes_are_spoken` (`stages/script.rs`) and `QuoteVerifier` (`plugin/analyser.rs:64`) use the same function, so a stray opener blinds all three. /scrutinise verified this by transcription: the control text is caught, and the text with a stray `"` yields no span.
- A source-side imbalance (`said” then "the fire`) can desync `quotations` on filled text and wrongly reject a valid turn. It fails closed and is rare, so it's recorded, not fixed.
- The grounding share: the comet claim scores 1/5 on chunk text alone and 3/5 with title words in the union (verified). Under the new rule it scores 1/3.

CONSUMERS:
- `fill_quote_placeholders` / `PlaceholderError` (text.rs, pub(crate)) → only `stages/script.rs` `build_script`. The error is formatted into the `turn {i}: …` string there.
- `is_grounded` (extract_claims.rs, private) → `ExtractClaims::run` and that file's tests.
- `WriteScript::VERSION`, `ExtractClaims::VERSION` → cache keys only.

## Risk & rollback
- **Stricter grounding can reject more paraphrases.** With few non-name words, 60% means 1 of 1, 2 of 2, 2 of 3. INSTRUCTIONS rule 3 already asks for the passage's wording. Step 3.2 checks the real model.
- **Model prose like `5"` is rejected.** The retry message says to write inches as a word.
- **F4 (follow-up, not driven here).** A source containing the literal `{{quote:` may end up in a turn's text through a claim. The script stage then rejects it as Malformed or Unknown. It fails closed; escaping it is future work.
- **Not scanned:** `'…'`, `«…»`, `„…“` quotation styles.
- **Rollback:** `git revert`. Only cache entries are invalidated.

## Out of scope
- Making `quotations` or QuoteVerifier report unmatched marks. That would false-positive on source-side imbalance.
- NLI-based grounding.
