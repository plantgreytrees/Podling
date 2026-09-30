# Hand-off: next goal

> Written 2026-09-30 at the end of Phase 2 and its scrutinise round. The branch
> `worktree-phase2-llm-provider` holds everything below; merge it into `main`
> before starting (see "Before you start").

## Goal

**A real local model (llama3.1:8b on Ollama) turns the Tunguska example into a
passing episode: `podling run --episode examples/tunguska/episode-ollama.toml`
exits 0 with zero analyser errors, two runs in a row from a cold cache.**

Run it through the full craftsman loop:

```
/craftsman:plan quote-placeholders          # from the "Fixes" section below
/craftsman:orchestrate quote-placeholders
# live check (see "Acceptance")
/craftsman:scrutinise quote-placeholders
/craftsman:orchestrate scrutinise-quote-placeholders   # only if scrutinise finds anything
/craftsman:sync-docs
# merge the branch into main
```

## Why the goal isn't met yet

Two live runs with llama3.1:8b (2026-09-30) got through claim extraction but
failed at the script stage:

```
stage script got invalid output from its provider: turn 1 must speak quote 0 word
for word, exactly as "The heat was so strong that my shirt almost burned." ... (after 2 attempts)
```

The model picks the right sentence (`QuoteRef { chunk, sentence }`), then
paraphrases it in the turn's text. It does so again even when the retry quotes
the exact sentence back. An 8B model can't be relied on to retype text, so
asking it to is the flaw, not the retry.

## Fixes

1. **Quote placeholders (the main fix).** The model writes `{{quote:N}}` in a
   turn's `text` where the N-th quote of that turn goes. The code replaces it
   with the resolved sentence in quotation marks. The model never types quoted
   words, which is the invariant in `.claude/CLAUDE.md` ("an LLM may select a
   quote but never write one") applied to the spoken text too.
   - `crates/podling-core/src/stages/script.rs`: INSTRUCTIONS rule 3; in
     `build_script`, substitute the placeholders after `resolve`. Reject a
     placeholder with no matching quote, a quote with no placeholder, and (as
     now) quoted words outside a placeholder. Bump `WriteScript::VERSION` to 5.
   - `crates/podling-core/src/plugin/llm.rs`: bump `PROMPT_VERSION` to 2;
     make `FakeLlm::write_script` emit placeholders.
   - `crates/podling-core/tests/fixtures/llm/write_script.json`: use
     placeholders in the turn text.
   - Keep `check_quotes_are_spoken` and `QuoteVerifier`. After substitution
     they should always pass, and they stay the independent check.
   - Docs: `docs/architecture.md` "Sentence-addressed quotes", README "What the
     model can and can't do".
2. **Grounding sees headings and title.** `is_grounded` in
   `crates/podling-core/src/stages/extract_claims.rs` compares a claim with the
   chunk text only. Extraction rule 2 asks the model to replace references
   such as "the site" with names, which may come from the document title or
   the chunk's heading path (`Document::title`, `Chunk::heading_path`). Include
   both in the word set the claim is checked against, and add a test with a
   claim that names a term found only in a heading. Bump `ExtractClaims::VERSION` to 4.

## Acceptance

- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
      and `cargo test --workspace` pass.
- [ ] A turn's text contains no model-typed quotation. Every quotation in a
      finished script comes from a placeholder substitution. There is a unit test
      for each rejection case in fix 1.
- [ ] A claim naming a term from a heading only is accepted; an invented claim is
      still rejected.
- [ ] Live, twice from a cold cache (`--cache-dir` pointing at an empty directory):
      `cargo run -p podling-cli -- run --episode examples/tunguska/episode-ollama.toml`
      exits 0 and `analysis.json` has no `error` finding.
- [ ] `PODLING_LIVE_LLM_URL=http://localhost:11434/v1 PODLING_LIVE_LLM_MODEL=llama3.1:8b cargo test -p podling-cli -- --ignored live` passes.

## Before you start

- Merge this branch: `git merge --no-ff worktree-phase2-llm-provider` from the
  main checkout. There is no git remote, so there is nothing to push.
- Ollama runs in the docker container `infra_docker_compose-ollama-1` on port
  11434, and `llama3.1:8b` is already pulled. `cargo` is at `~/.cargo/bin`, which
  isn't on the default PATH.
- A leftover worktree, `.claude/worktrees/phase1-core-contracts`, sits at
  `main`'s old commit from an earlier session. Remove it with
  `git worktree remove .claude/worktrees/phase1-core-contracts` once you've
  confirmed it has nothing unmerged.

## Out of scope

- Semantic claim clustering. A fact two sources word differently stays two
  SingleSource claims (seen live with "80 million trees"). That needs the NLI
  provider, a later phase.
- Token budgeting. `WriteScript` only warns over 24 KiB.

## Where things stand

| Plan | State |
|---|---|
| [phase2-llm-provider](plans/phase2-llm-provider.md) | complete |
| [scrutinise-phase2-llm-provider](plans/scrutinise-phase2-llm-provider.md) | complete (5 units, including typed `ProviderFailure`) |
| [script-verbatim-retry](plans/script-verbatim-retry.md) | complete; works, but not enough on its own for an 8B model (see above) |
| [quote-placeholders](plans/quote-placeholders.md) | complete; two cold-cache llama3.1:8b runs pass with 0 errors |
| [scrutinise-quote-placeholders](plans/scrutinise-quote-placeholders.md) | complete (stray quotation marks rejected; title/heading words don't count toward grounding) |
