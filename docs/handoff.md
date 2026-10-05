# Hand-off: next goal

> Written 2026-10-01 at the end of Phase 3 (embeddings and NLI in the claim
> ledger); updated 2026-10-04 after NLI grounding in extraction, and 2026-10-05
> after Phase 5 (episode audio), which was built before Phase 4, and
> 2026-10-06 after its `/scrutinise` fixes. Phase 5 is on
> the branch `worktree-phase5-tts-plan` until it is merged into `main`.

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

## Where Phase 5 leaves it

With `[tts]`, a run ends in `episode.wav`: Qwen3-TTS in a sidecar worker speaks
each turn in a cloned voice, Whisper (CPU) checks every chunk, and the assembler
paces, mixes and normalises the episode (see `docs/architecture.md`, "Episode
audio"). Two things are open:
- **Listening.** Whether per-turn banter sounds natural, and how the seams and
  backchannels sound, needs a human ear (plan tasks 1.4 and 10.3). If per-turn
  banter is rejected, the Dia2 dialogue adapter is the planned fallback.
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
- [ ] Offline, with fake providers: the contradiction fixture produces one
      verdict per Contested claim, and the script mentions the disagreement.
- [ ] Without Contested claims, no adjudicator call is made, and runs without
      `[nli]` give byte-identical artifact bodies.
- [ ] Live, twice from a cold cache, on an example with a real contradiction:
      the run exits 0 with no `error` in `analysis.json`.

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
- The worktrees `.claude/worktrees/phase1-core-contracts` and
  `.claude/worktrees/phase2-llm-provider` are fully merged into `main`. Remove
  them with `git worktree remove <path>`.

## Out of scope

- Token budgeting. `WriteScript` only warns over 24 KiB.
- MCP source connectors, PDF ingestion.
- Parallel synthesis, and the Dia2 dialogue adapter (unless the listening pass
  rejects per-turn banter).

## Where things stand

| Plan | State |
|---|---|
| [phase2-llm-provider](plans/phase2-llm-provider.md) | complete |
| [quote-placeholders](plans/quote-placeholders.md) | complete; two cold-cache llama3.1:8b runs pass with 0 errors |
| [phase3-nli-ledger](plans/phase3-nli-ledger.md) | complete; live runs recorded in the plan ("Live results") |
| [nli-extraction-grounding](plans/nli-extraction-grounding.md) | complete; live runs recorded in the plan ("Live results") |
| [scrutinise-nli-extraction-grounding](plans/scrutinise-nli-extraction-grounding.md) | complete; premise windows capped at 120 words |
| [phase5-tts-audio](plans/phase5-tts-audio.md) | Phase 5, episode audio: all units merged on `worktree-phase5-tts-plan`; live runs in the plan ("Live results"); listening passes (1.4, 10.3) pending |
| [scrutinise-phase5-tts-audio](plans/scrutinise-phase5-tts-audio.md) | three `/scrutinise` rounds; every Warning fixed (units 1–4, 7), the last round clean; units 5, 6 and 8 are PENDING Suggestions |
