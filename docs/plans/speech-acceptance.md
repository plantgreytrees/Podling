---
slug: speech-acceptance
goal: A live Tunguska run with and without the pronunciation list shows whether WER and retries got worse, and the user rates the changed chunks blind.
idea: docs/ideas/natural-episode-speech.md
classification: in-scope   # the idea's open acceptance, recommendation 6 (natural-episode-speech.md:169-173) and kill criterion (natural-episode-speech.md:128)
tracker_rows: [TRACKER#speech-acceptance/sa-tool, TRACKER#speech-acceptance/sa-live, TRACKER#speech-acceptance/sa-listen]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)   # one crate's example + docs; no shared contract, migration or security surface
coverage:
  contract:      N/A(reads `AudioManifest` and `Script` as they are; no type, schema or CLI change)
  data:          N/A(no persistence; the pack is written to a user directory outside the repository)
  config:        2.1 (per-arm episode copies in the job tmp dir; nothing committed)
  security:      1.4 (the pack refuses a non-empty directory and never deletes; local files only)
  tests:         1.5
  observability: N/A(a one-off tool that prints its report)
  interface:     1.1-1.4 (the example's arguments and report)
  docs:          2.3, 3.2
  rollback:      git revert of the merge; delete ~/podling-listening/lexicon-ab/
units:
  - id: 1
    scope_id: sa-tool
    project: .
    depends_on: []
    module: podling-cli lexicon A/B example
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-cli/Cargo.toml
        - crates/podling-types/src/audio.rs
        - crates/podling-types/src/script.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-core/src/cache.rs
        - Cargo.toml
      docs:
        - docs/ideas/natural-episode-speech.md
      write:
        - crates/podling-cli/Cargo.toml
        - crates/podling-cli/examples/lexicon_ab.rs
    arch: []
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer],
               skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: sa-live
    project: .
    depends_on: [sa-tool]
    module: live lexicon A/B run
    language: rust
    security: normal
    scope:
      read:
        - examples/tunguska/episode-tts.toml
        - crates/podling-cli/examples/lexicon_ab.rs
      docs:
        - docs/plans/phase5-tts-audio.md
        - docs/plans/speech-acceptance.md
      write:
        - docs/plans/phase5-tts-audio.md
        - docs/plans/speech-acceptance.md
    arch: []
    tooling: { implementer: implementer, gates: [],
               skills: [], guards: [cargo test], mcp: [] }
  - id: 3
    scope_id: sa-listen
    project: .
    depends_on: [sa-live]
    module: blind listening (user)
    language: none
    security: normal
    scope:
      read: []
      docs:
        - docs/plans/phase5-tts-audio.md
        - docs/plans/speech-acceptance.md
      write:
        - docs/plans/phase5-tts-audio.md
        - docs/plans/speech-acceptance.md
    arch: []
    tooling: { implementer: implementer, gates: [],
               skills: [], guards: [], mcp: [] }
---

# Plan: speech acceptance (lexicon A/B)

## Outcome
A live Tunguska run with and without the pronunciation list shows whether WER and retries got worse, and the user rates the changed chunks blind.

## Scope Steps (executable core)

### Step 1 — sa-tool (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer · guards cargo fmt/clippy/test
Depends on: none

A Cargo example, `cargo run -p podling-cli --example lexicon_ab -- --off <out-a> --on <out-b> [--cache-dir <dir> --pack <dir>]`. Not a `podling` subcommand: it is a one-off acceptance check, so it adds nothing to the shipped binary. No new dependency.
- [ ] 1.1 Add `[[example]] name = "lexicon_ab"` with `test = true` to `crates/podling-cli/Cargo.toml`, and write the example's argument parsing (clap derive, as in `src/main.rs`) and loading of `<out>/audio.json` for both arms through the artifact envelope (`schema_version`, `kind`, `body`; `crates/podling-types/src/envelope.rs`) → accept: a missing `audio.json`, or one whose `kind` is not `audio`, exits non-zero naming the path.
- [ ] 1.2 Pair the two manifests' chunks by `turns`; refuse when the chunk counts or any pair's `turns` differ (the arms then had different scripts or chunking), naming the first mismatch. A pair is *affected* when its `id`s differ → accept: unit tests for a clean pairing, a count mismatch, a `turns` mismatch, and the affected set.
- [ ] 1.3 Report per arm, for all chunks and for the affected ones: chunk count, mean `wer_pm`, chunks with `take > 0`, unverified chunks; then a table of the affected pairs (turns, each arm's take, `wer_pm`, verified). Verdict: **inconclusive** (exit 2) when no pair is affected; **fail** (exit 1) when the on-arm's mean WER over all chunks is above 10 ‰ or its `take > 0` count is above the off-arm's; else **pass** (exit 0). Print the caveats with the verdict: one deterministic sample per arm; the on-arm's WER is scored with its `heard` variants; `take > 0` also counts banter picks → accept: unit tests for pass, each fail cause, and inconclusive.
- [ ] 1.4 With `--pack <dir> --cache-dir <dir>`: refuse a non-empty `<dir>`; for each affected pair, copy both blobs (`<cache>/blobs/<2 hex>/<64 hex>.wav`) to `<dir>/NN-X.wav` and `<dir>/NN-Y.wav`, which arm is X decided at random (`std::collections::hash_map::RandomState`, no new dependency); write `key.json` (NN → which of X/Y is the on-arm) and `ratings.md` (one row per NN: the turns' text from the off-arm's `script.json`, "prefer X / Y / same", "names said right in X / Y", notes). Print neither the key nor which file is which arm → accept: a unit test that a pack's file names and `ratings.md` never contain "on"/"off" arm labels, and that a non-empty directory is refused.
- [ ] 1.5 Keep the logic (pairing, summary, verdict, pack planning) in plain functions over `AudioManifest` values, tested in the example's `#[cfg(test)]` module with hand-built `ChunkRecord`s → accept: `cargo test --quiet` exits 0 and runs the example's tests; `cargo clippy --all-targets -- -D warnings` is clean.

### Step 2 — sa-live (., rust, normal)
Tooling: implementer · guards cargo test
Depends on: sa-tool

Needs the GPU sidecar and an LLM (see the live-run notes in `phase5-tts-audio.md` "Live results").
- [x] 2.1 In the job tmp dir, copy `examples/tunguska/episode-tts.toml` twice, identical except that the on-copy uncomments `[tts.pronounce]` (`Kulik`, `Vanavara`); only the LLM base URL and timeouts may differ from the committed example, and identically in both → accept: `diff` of the two copies shows only the pronounce lines.
- [ ] 2.2 Run the off-copy, then the on-copy, with one shared `--cache-dir` and separate `--out` directories → accept: both write `audio.json` (exit 1 from an `Error` finding is fine); the off-arm's `script.json` names Kulik or Vanavara. If it names neither, set `target_minutes = 30` in both copies and rerun both, at most twice; still neither → record the A/B as inconclusive and stop the unit.
- [ ] 2.3 Run `lexicon_ab --off … --on …` and add "Lexicon A/B (speech-acceptance, <date>)" to `phase5-tts-audio.md` "Live results": the setup, the report (all and affected), the verdict and its caveats → accept: the section exists with the numbers and n affected.
- [ ] 2.4 Build the pack with `--pack ~/podling-listening/lexicon-ab/ --cache-dir <shared cache>` → accept: one X/Y pair per affected chunk, `key.json` and `ratings.md` exist; the key has not been read in this session.

**Stopped 2026-10-07, inconclusive.** The off arm's script named neither name at 10 minutes or at
30 minutes twice (the second with a new shared cache). Only the off arm was run, since the on arm
shares its script. The script stage writes one turn at version 12, a regression: see
`phase5-tts-audio.md` "Lexicon A/B". 2.2–2.4 and step 3 wait for a fix to the script stage, then
run unchanged.

### Step 3 — sa-listen (., user, normal)
Depends on: sa-live

**The user's step.** The agent cannot do or tick 3.1; the row stays open until the user's ratings arrive in a user message.
- [ ] 3.1 (user) Listen to each X/Y pair in `~/podling-listening/lexicon-ab/` and fill in `ratings.md` → accept: the user says the ratings are done.
- [ ] 3.2 Only then read `key.json`, un-blind the ratings, and record "Listening (the user, <date>)" under the A/B section in `phase5-tts-audio.md`, quoting the user → accept: the section states, per pair, whether the on-arm was preferred and whether the names were said right.

## Sequencing
Tool first (tested without a GPU), then the live arms, then the user's blind pass. The key is read only after the ratings exist, so the agent cannot anchor the user.

## Verification background
- Kill criterion: "Mean WER rises above 10 ‰, or the retry rate rises" — `docs/ideas/natural-episode-speech.md:128`; acceptance asks for a blind A/B and no worse WER or retry rate — `docs/ideas/natural-episode-speech.md:169-173`
- `ChunkRecord` keeps only the kept take: `id`, `turns`, `blob`, `seed`, `take`, `wer_pm`, `verified` — `crates/podling-types/src/audio.rs:95-113`
- The take > 0 count is the retry measure the earlier live run used ("6 chunks used a take after 0") — `docs/plans/phase5-tts-audio.md:549`
- The seed is derived from the chunk id and take, and the id hashes the turns' `say_as`; so the lexicon changes the seed of exactly the chunks it touches, and a rerun of one arm repeats itself — `crates/podling-core/src/stages/synthesize.rs:172-203`, `:235-239`; `crates/podling-core/src/plugin/tts.rs:41-46`
- `heard` variants remap the transcript before WER — `crates/podling-core/src/stages/verify_audio.rs:408-419`
- The script stage asks only whether `[tts]` exists — `crates/podling-core/src/stages/script.rs:60`
- The Tunguska lexicon is commented out — `examples/tunguska/episode-tts.toml:85-87`
- Blob layout `blobs/<2 hex>/<64 hex>.wav` — `crates/podling-core/src/cache.rs:100`, `:168`
- No compare or report command exists — `crates/podling-cli/src/main.rs:27-72`
- The 30-minute live run heard "Kulik" as "Koolik" on every take — `docs/plans/phase5-tts-audio.md:557-559`

CONSUMERS: none (no shared contract changes; the example only reads `AudioManifest` and `Script`).

## Risk & rollback
- One deterministic sample per arm; only the affected chunks differ, so n may be small. The report states n and the caveats rather than claiming significance.
- The LLM may write a script without Kulik or Vanavara (2.2 handles it).
- Revert the merge; the pack lives outside the repository.

## Out of scope
A `podling` subcommand; storing every take or transcripts in `audio.json`; re-scoring the on-arm without `heard` variants; changing the idea doc's status (the idea's owner); the model swap, the banter prompt and the voice marketplace.
