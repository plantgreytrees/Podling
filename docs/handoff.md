# Hand-off: next goal

> Written 2026-10-04 at the end of Phase 4 (the Contested-claim adjudicator),
> which is merged into `main`.

## Goal

**NLI grounding in extraction. With `[nli]` set, a claim the model extracts
is kept only if its chunk entails it, instead of only if the chunk shares its
content words. Without `[nli]`, extraction stays exactly as it is.**

Run it through the full craftsman loop:

```
/craftsman:plan nli-grounding
/craftsman:orchestrate nli-grounding
# live check (see "Acceptance")
/craftsman:scrutinise nli-grounding
/craftsman:orchestrate scrutinise-nli-grounding   # only if scrutinise finds anything
/craftsman:sync-docs
# merge the branch into main
```

## Where Phase 4 leaves it

- `extract_claims` checks each drafted claim with `is_grounded`
  (`crates/podling-core/src/stages/extract_claims.rs`). The check is lexical:
  every content word of the claim must appear in the chunk, or in the
  document's title and headings. A distortion built from the chunk's own
  words passes ("the expedition found a crater" from a chunk saying "the
  expedition found no crater").
- A claim that fails is a rejection reason inside `complete_validated`, so
  the model gets one retry with the reason, and a second failure fails the
  stage.
- `build_grounding` in `crates/podling-core/src/pipeline.rs` builds the NLI
  provider before any stage runs; the weights load on the first `score`. Today
  `cluster_claims`, after extraction, is the first user, and the provider is
  dropped after `score_stances` to free memory for the script.
- The adjudicator (`crates/podling-core/src/stages/adjudicate.rs`) writes
  `verdicts.json`: one LLM call per Contested claim, none without `[nli]`. See
  `docs/architecture.md`, "Adjudicating Contested claims".

## Design constraints

- Without `[nli]`, artifact bodies and cache keys stay byte-identical (the
  golden test `no_nli_config_writes_todays_artifacts` in
  `crates/podling-core/tests/pipeline.rs` guards this), so the NLI check joins
  the stage's config fingerprint only when `[nli]` is set.
- Keep the lexical check when `[nli]` is absent. With `[nli]`, decide whether
  NLI replaces it or runs after it, and say why in the plan. Entailment of a
  short claim by a long chunk needs premise windows, as `score_stances` uses.
- The threshold is a fixed number in the fingerprint, compared against a
  rounded `PerMille`, like the grounding stages' thresholds.
- Bump `ExtractClaims::VERSION`. If the rejection reason the model sees
  changes, `PROMPT_VERSION` is in that stage's cache key too.
- The `.claude/CLAUDE.md` licensing rules apply to any new dependency.

## Acceptance (suggested)

- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
      and `cargo test --workspace` pass.
- [ ] Offline, with `FakeNli`: a distorted claim built from the chunk's own
      words is rejected with `[nli]` and accepted without it.
- [ ] Runs without `[nli]` give byte-identical artifact bodies.
- [ ] Live, from a cold cache, on `examples/tunguska` and `examples/titanic`:
      exit 0 with no `error` in `analysis.json`.

## Before you start

- Ollama runs in the docker container `infra_docker_compose-ollama-1` on port
  11434, with `llama3.1:8b` and `nomic-embed-text`. The container belongs to
  another project's compose stack. It runs **CPU only** with a **4096-token
  context**, so a script takes several minutes, and a request that overruns
  the context fails in odd ways (in Phase 4, a chunk id cited as a claim).
  Other background jobs may share the server, and it serves one request at a
  time. Check `pgrep -af podling` and run when it's free.
- `cargo` is at `~/.cargo/bin`, which isn't on the default PATH.
- The NLI model is in `~/.cache/podling-models/nli-deberta-v3-base`. Each live
  example expects it at `examples/<name>/models/nli-deberta-v3-base`
  (gitignored); a symlink is enough.

## Out of scope

- Token budgeting. `WriteScript` only warns over 24 KiB.
- TTS, MCP source connectors, PDF ingestion.
- Tuning the stance thresholds. Phase 4's first live Titanic run, on longer
  excerpts, showed NLI calling contradiction between sentences that only share
  a topic; that is a separate plan if it matters.

## Where things stand

| Plan | State |
|---|---|
| [phase2-llm-provider](plans/phase2-llm-provider.md) | complete |
| [quote-placeholders](plans/quote-placeholders.md) | complete; two cold-cache llama3.1:8b runs pass with 0 errors |
| [phase3-nli-ledger](plans/phase3-nli-ledger.md) | complete; live runs recorded in the plan ("Live results") |
| [phase4-adjudicator](plans/phase4-adjudicator.md) | complete; live runs recorded in the plan ("Live results") |
