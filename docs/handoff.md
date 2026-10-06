# Hand-off: next goal

> Written 2026-10-01 at the end of Phase 3 (embeddings and NLI in the claim
> ledger); rewritten 2026-10-06 when Phase 4 (the Contested-claim adjudicator)
> was finished. NLI grounding in extraction and Phase 5 (episode audio) were
> merged before it.

## Goal

**Fewer spurious Contested claims. With `[nli]` set, two sources should be
counted as disagreeing only when they make incompatible statements about the
same thing, not when they only share a topic. Each spurious Contested claim
costs an adjudicator call and can put a false disagreement into the script.**

The goal was picked from the Phase 4 live runs. It was not asked for, so check
it against your own priorities first. Other open candidates are listed under
"Other open threads".

Run it through the full craftsman loop:

```
/craftsman:plan stance-precision
/craftsman:orchestrate stance-precision
# live check (see "Acceptance")
/craftsman:scrutinise stance-precision
/craftsman:orchestrate scrutinise-stance-precision   # only if scrutinise finds anything
/craftsman:sync-docs
# merge the branch into main
```

## Where Phase 4 leaves it

- `score_stances` (`crates/podling-core/src/stages/score_stances.rs`) scores
  each claim against premise windows from other source groups. A window counts
  as a contradiction when its NLI contradiction is at least `CONTRADICT_PM`
  (950 ‰) and the embeddings' similarity is at least
  `MIN_CONTRADICT_SIMILARITY_PM` (600 ‰). Any one contradicting group makes the
  claim Contested in the ledger's `classify`.
- The live Titanic runs (`docs/plans/phase4-adjudicator.md`, "Live results")
  show contradictions that are not disagreements. One was "lifeboats for
  1,176" against "712 saved": both are numbers about the lifeboats, but they
  count different things. The adjudicator cannot make such a claim
  un-Contested, because status stays deterministic. At best it falls back to
  `Unresolved`, and the script may still mention it.
- The adjudicator (`crates/podling-core/src/stages/adjudicate.rs`) makes one
  LLM call per Contested claim, capped at `MAX_VERDICT_TOKENS`. Fewer spurious
  Contested claims means fewer calls and fewer fallbacks.

## Design constraints

- `ClaimStatus` stays deterministic: any new check is a rule or a model score
  with a fixed threshold, not an LLM judgement.
- A threshold or rule change goes into `score_stances`' `config_fingerprint`
  and bumps its `Stage::VERSION`. Runs without `[nli]` stay byte-identical.
- Licensing as in `.claude/CLAUDE.md`: no non-commercial, AGPL or
  revenue-capped weights or dependencies.
- Check the measurements before changing a threshold. The `nli-extraction-grounding`
  and `phase3-nli-ledger` plans record the scores they relied on.

## Acceptance (suggested)

- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
      and `cargo test --workspace` pass.
- [ ] Offline: a pair that only shares a topic (two different counts about the
      same lifeboats) is not Contested, and the 1908-vs-1907 pair in
      `crates/podling-core/tests/fixtures/contradiction/` still is.
- [ ] Runs without `[nli]` give byte-identical artifact bodies.
- [ ] Live, from a cold cache, on `examples/titanic`: the real disagreement
      (706 vs 712 saved) is still Contested with a verdict, and fewer Contested claims are spurious
      than in Phase 4's runs. `examples/tunguska` stays consistent (no
      Contested claims).

## Other open threads

- **llama3.1:8b reliability on Titanic.** Of seven cold runs, five passed.
  Two failed: one mangled a claim id on all three script attempts, and in one
  extraction grounding rejected the paraphrase "Sunday evening". See
  "Live results" in the Phase 4 plan.
- **Adjudicator fallbacks.** The model often leaves out a supporting cite, so
  the verdict falls back to `Unresolved`.
- **Speech (Phase 5).** Serious mispronunciations (proper nouns such as
  "Kulik") and flat delivery. The Qwen 1.7B Base model is voice-clone only,
  and its adapter drops each turn's `emotion`.
- **Grounding gaps** (`docs/plans/nli-extraction-grounding.md`). DeBERTa lets
  through a dropped hedge ("my shirt almost burned" → "the shirt burned"), and
  a number that occurs elsewhere in the chunk. The adjudicator does not catch
  these, since only Contested claims reach it.
- **`text::sentences`** does not split after `."`, so a chunk with quoted
  speech can come out as one long sentence. The script can still quote
  the quotation by `part`.

## Before you start

- Start from `main`. The remote `origin` is `github.com/plantgreytrees/Podling`.
- `cargo` is at `~/.cargo/bin`, which isn't on the default PATH.
- Ollama: the Docker container `infra_docker_compose-ollama-1` (port 11434)
  has no GPU and runs at well under 1 token/s. The Phase 4 and 5 live runs
  used a native Ollama on the GPU at 127.0.0.1:11435
  (`OLLAMA_CONTEXT_LENGTH=16384`), with `llama3.1:8b` and `nomic-embed-text`.
  `examples/*/episode-ollama.toml` names 11434, so for a live run copy it and
  change `base_url`.
- The NLI model is in `~/.cache/podling-models/nli-deberta-v3-base`. The live
  examples expect it at `examples/<name>/models/nli-deberta-v3-base`
  (gitignored); a symlink is enough.

## Out of scope

- Token budgeting. `WriteScript` only warns over 24 KiB.
- MCP source connectors, PDF ingestion.
- Parallel synthesis, and the Dia2 dialogue adapter.

## Where things stand

| Plan | State |
|---|---|
| [phase2-llm-provider](plans/phase2-llm-provider.md) | complete |
| [quote-placeholders](plans/quote-placeholders.md) | complete; two cold-cache llama3.1:8b runs pass with 0 errors |
| [phase3-nli-ledger](plans/phase3-nli-ledger.md) | complete; live runs recorded in the plan ("Live results") |
| [nli-extraction-grounding](plans/nli-extraction-grounding.md) | complete; live runs recorded in the plan ("Live results") |
| [scrutinise-nli-extraction-grounding](plans/scrutinise-nli-extraction-grounding.md) | complete; premise windows capped at 120 words |
| [phase5-tts-audio](plans/phase5-tts-audio.md) | complete; live runs and listening passes in the plan |
| [scrutinise-phase5-tts-audio](plans/scrutinise-phase5-tts-audio.md) | every Warning fixed; units 5, 6 and 8 are PENDING Suggestions |
| [phase4-adjudicator](plans/phase4-adjudicator.md) | complete; two cold Titanic runs and one Tunguska run pass ("Live results") |
| [scrutinise-phase4-adjudicator](plans/scrutinise-phase4-adjudicator.md) | complete; the fallback reason is capped |
