# Hand-off: next goal

> Written 2026-10-01 at the end of Phase 3 (embeddings and NLI in the claim
> ledger). The branch `worktree-phase3-nli-ledger` holds everything below; merge
> it into `main` before starting (see "Before you start").

## Goal

**Phase 4: a Contested-claim adjudicator. When two independent sources disagree,
the episode says so and explains the disagreement from the sources, instead of
repeating one side or both without comment.**

Run it through the full craftsman loop:

```
/craftsman:plan phase4-adjudicator
/craftsman:orchestrate phase4-adjudicator
# live check (see "Acceptance")
/craftsman:scrutinise phase4-adjudicator
/craftsman:orchestrate scrutinise-phase4-adjudicator   # only if scrutinise finds anything
/craftsman:sync-docs
# merge the branch into main
```

## Where Phase 3 leaves it

`score_stances` adds `Contradicts` evidence when a sentence from another
independence group contradicts a claim (NLI contradiction ≥ 0.950 and embedding
similarity ≥ 0.60), and `classify()` turns that into `Contested`. Each piece of
evidence stores the premise span and the scores that decided it
(`EvidenceBasis::Nli` in `crates/podling-types/src/claim.rs`). Nothing reads a
Contested claim yet except the script model, which sees only its id, text and
status (`LedgerClaim` in `crates/podling-core/src/plugin/llm.rs`).

## Design constraints

- `.claude/CLAUDE.md`: "only Contested claims go to an LLM adjudicator". Status
  stays deterministic: the adjudicator must not change a claim's status. It
  writes a new artifact (for example, a verdict per Contested claim: which side
  the sources favour, or "unresolved", plus a short explanation citing both
  premise spans) that the script stage reads.
- The adjudicator quotes nothing itself. It cites evidence by reference; quoted
  words still come only from source spans.
- A new stage bumps its own `Stage::VERSION`; a new artifact kind updates the
  schema snapshot and `SCHEMA_VERSION`. With no Contested claims, the stage makes
  no LLM call, and artifacts of runs without `[nli]` stay byte-identical.
- The live Tunguska sources contain no contradiction, so the live check needs a
  second example (for example, two sources that disagree on a date or a count).
  The offline fixture `crates/podling-core/tests/fixtures/contradiction/` is a
  starting point.

## Follow-up worth planning separately

**NLI grounding in extraction.** `is_grounded` in
`crates/podling-core/src/stages/extract_claims.rs` checks a claim against its
chunk by content words. It misses a distortion built from the chunk's own words.
With `[nli]` set, "does the chunk entail the claim?" is the better test. Keep the
lexical check when `[nli]` is absent, so no-config output doesn't change.

## Acceptance (suggested)

- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
      and `cargo test --workspace` pass.
- [ ] Offline, with fake providers: the contradiction fixture produces one
      verdict per Contested claim, and the script mentions the disagreement.
- [ ] Without Contested claims, no adjudicator call is made, and runs without
      `[nli]` give byte-identical artifact bodies.
- [ ] Live, twice from a cold cache, on an example with a real contradiction:
      the run exits 0 with no `error` in `analysis.json`.

## Before you start

- Merge this branch: `git merge --no-ff worktree-phase3-nli-ledger` from the
  main checkout. There is no git remote, so there is nothing to push.
- Ollama runs in the docker container `infra_docker_compose-ollama-1` on port
  11434, with `llama3.1:8b` and `nomic-embed-text` pulled. `cargo` is at
  `~/.cargo/bin`, which isn't on the default PATH.
- The NLI model is in `~/.cache/podling-models/nli-deberta-v3-base`. The live
  example expects it at `examples/tunguska/models/nli-deberta-v3-base`
  (gitignored); a symlink is enough.
- The worktrees `.claude/worktrees/phase1-core-contracts` and
  `.claude/worktrees/phase2-llm-provider` are fully merged into `main`. Remove
  them with `git worktree remove <path>`.

## Out of scope

- Token budgeting. `WriteScript` only warns over 24 KiB.
- TTS, MCP source connectors, PDF ingestion.

## Where things stand

| Plan | State |
|---|---|
| [phase2-llm-provider](plans/phase2-llm-provider.md) | complete |
| [quote-placeholders](plans/quote-placeholders.md) | complete; two cold-cache llama3.1:8b runs pass with 0 errors |
| [phase3-nli-ledger](plans/phase3-nli-ledger.md) | complete; live runs recorded in the plan ("Live results") |
