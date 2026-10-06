# Hand-off: next goal

> Written 2026-10-01 at the end of Phase 3 (embeddings and NLI in the claim
> ledger); updated 2026-10-04 after NLI grounding in extraction, 2026-10-05
> after Phase 5 (episode audio), 2026-10-06 after its `/scrutinise` fixes, and
> 2026-10-06 when Phase 4 (the Contested-claim adjudicator) was merged with it.

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

## Where Phase 5 leaves it

With `[tts]`, a run ends in `episode.wav`: Qwen3-TTS in a sidecar worker speaks
each turn in a cloned voice, Whisper (CPU) checks every chunk, and the assembler
paces, mixes and normalises the episode (see `docs/architecture.md`, "Episode
audio"). Two things are open:
- **Listening (done 2026-10-06).** Per-turn Qwen and Dia2 sound near identical,
  so Qwen stays and the Dia2 adapter is not built; the live episodes are fine.
  Two problems remain for a follow-up: serious mispronunciations (proper nouns
  such as "Kulik") and flat delivery. The Qwen 1.7B Base model is voice-clone
  only, and its adapter drops each turn's `emotion`.
- **The adjudicator and audio.** An adjudicator that adds script turns needs
  nothing new from the audio stages: they read the script only.

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

## Known limits of NLI grounding in extraction

The `ground_claims` stage (`crates/podling-core/src/stages/ground_claims.rs`) drops a
claim its own chunk doesn't entail, which catches distortions made from the chunk's
own words ("led" for "joined"). DeBERTa still lets two kinds through. Both were
measured on the Tunguska sources (`docs/plans/nli-extraction-grounding.md`):
- a dropped hedge: "the eyewitness's shirt burned" against "my shirt almost
  burned" scored 0.994;
- a figure moved within the chunk: "an explosion in 1927 flattened trees" scored
  0.966, because 1927 appears elsewhere in the chunk's heading. The lexical
  exact-number gate only checks that a number occurs in the chunk, not where.

The adjudicator would not catch these either, since only Contested claims reach it.

## Acceptance (suggested)

- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
      and `cargo test --workspace` pass.
- [ ] Offline, with `FakeNli`: a distorted claim built from the chunk's own
      words is rejected with `[nli]` and accepted without it.
- [ ] Runs without `[nli]` give byte-identical artifact bodies.
- [ ] Live, from a cold cache, on `examples/tunguska` and `examples/titanic`:
      exit 0 with no `error` in `analysis.json`.

## Before you start

- Start from `main`. The remote `origin` is `github.com/plantgreytrees/Podling`.
- Ollama runs in the docker container `infra_docker_compose-ollama-1` on port
  11434, with `llama3.1:8b` and `nomic-embed-text` pulled. `cargo` is at
  `~/.cargo/bin`, which isn't on the default PATH. The container has no GPU,
  and when its VM swaps it runs at well under 1 token/s; the Phase 5 live runs
  used a native Ollama on the GPU instead (see that plan's "Live results").
- The NLI model is in `~/.cache/podling-models/nli-deberta-v3-base`. The live
  example expects it at `examples/tunguska/models/nli-deberta-v3-base`
  (gitignored); a symlink is enough.

## Out of scope

- Token budgeting. `WriteScript` only warns over 24 KiB.
- MCP source connectors, PDF ingestion.
- Parallel synthesis, and the Dia2 dialogue adapter (unless the listening pass
  rejects per-turn banter).
- Tuning the stance thresholds. Phase 4's first live Titanic run, on longer
  excerpts, showed NLI calling contradiction between sentences that only share
  a topic; that is a separate plan if it matters.

## Where things stand

| Plan | State |
|---|---|
| [phase2-llm-provider](plans/phase2-llm-provider.md) | complete |
| [quote-placeholders](plans/quote-placeholders.md) | complete; two cold-cache llama3.1:8b runs pass with 0 errors |
| [phase3-nli-ledger](plans/phase3-nli-ledger.md) | complete; live runs recorded in the plan ("Live results") |
| [nli-extraction-grounding](plans/nli-extraction-grounding.md) | complete; live runs recorded in the plan ("Live results") |
| [scrutinise-nli-extraction-grounding](plans/scrutinise-nli-extraction-grounding.md) | complete; premise windows capped at 120 words |
| [phase5-tts-audio](plans/phase5-tts-audio.md) | Phase 5, episode audio: all units merged on `worktree-phase5-tts-plan`; live runs in the plan ("Live results"); listening passes done: Qwen kept, mispronunciations and flat delivery are follow-ups |
| [scrutinise-phase5-tts-audio](plans/scrutinise-phase5-tts-audio.md) | three `/scrutinise` rounds; every Warning fixed (units 1–4, 7), the last round clean; units 5, 6 and 8 are PENDING Suggestions |
| [phase4-adjudicator](plans/phase4-adjudicator.md) | complete; live runs recorded in the plan ("Live results") |
