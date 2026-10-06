---
slug: natural-episode-speech
goal: Names the TTS mispronounces can be respelt (user pronounce.toml + episode [tts.pronounce]) without changing what the speech check or quotes compare; the check folds UK/US spellings and per-name heard-as variants; self-designed voices carry a recorded licence and provenance.
idea: docs/ideas/natural-episode-speech.md
classification: in-scope   # docs/ideas/natural-episode-speech.md "Recommendations" 1-5 and "Open questions" 2 ("both halves, smallest version"); docs/architecture/speech.rules.md ARCH-SPEECH-01..17
tracker_rows: [TRACKER#natural-episode-speech/nes-types, TRACKER#natural-episode-speech/nes-core, TRACKER#natural-episode-speech/nes-voice-design, TRACKER#natural-episode-speech/nes-docs]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(config:never)
coverage:
  contract:      1.1-1.6 (EpisodeSpec/TtsConfig schema, SCHEMA_VERSION 7→8), 2.1-2.7 (SpokenTurn.say_as, WireTurn text, Check::new/Verification/synthesize_script signatures; consumers in CONSUMERS)
  data:          N/A(no persistence/migration; chunk cache keys change only through say_as, ARCH-SPEECH-11; no stage VERSION bump)
  config:        1.1 ([tts.pronounce]), 2.4 (user-level pronounce.toml beside the sidecars.toml in use)
  security:      1.2, 2.4 (lexicon and pronounce.toml are strings only, validated at deserialisation; never a program), 2.9 (generated-licence voice fails closed without provenance)
  tests:         1.2, 1.3, 1.4, 2.1, 2.2, 2.3, 2.4, 2.7, 2.8, 2.9, 2.10, 2.11, 3.3
  observability: 2.6 (tracing::debug! of applied lexicon names per chunk, ARCH-SPEECH-12)
  interface:     2.4 (no new CLI flag; --sidecars /x/sidecars.toml implies /x/pronounce.toml); 3.2 (offline voice_design CLI)
  docs:          4.1 (docs/architecture.md "Episode audio"), 4.2 (sidecars/tts/README.md), 4.3-4.4 (examples/tunguska), 3.4 (scripts/voice_design/README.md)
  rollback:      git revert of the merge commits; SCHEMA_VERSION 8 cache entries read as foreign misses after revert and are rebuilt; episodes without [tts.pronounce] need no edit
units:
  - id: 1
    scope_id: nes-types
    project: .
    depends_on: []
    module: crates/podling-types lexicon, generated licence, provenance type
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/src/schema.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-core/src/plugin/mod.rs
      docs:
        - docs/architecture/speech.rules.md
      write:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/snapshots/schema_snapshot__episode.snap
        - crates/podling-core/src/plugin/mod.rs
    arch: [ARCH-SPEECH-07, ARCH-SPEECH-08, ARCH-SPEECH-13, ARCH-SPEECH-14]
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer, api-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: nes-core
    project: .
    depends_on: [nes-types]
    module: crates/podling-core say_as, wire text, lexicon merge, heard variants, UK/US fold, provenance check
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/plugin/tts.rs
        - crates/podling-core/src/plugin/sidecar_tts.rs
        - crates/podling-core/src/plugin/sidecar.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/stages/verify_audio.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/src/lib.rs
        - crates/podling-core/src/error.rs
        - crates/podling-core/tests/audio_e2e.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/sidecar_tts.rs
        - crates/podling-core/tests/fixtures/fake_sidecar.py
        - crates/podling-types/src/episode.rs
      docs:
        - docs/architecture/speech.rules.md
      write:
        - crates/podling-core/src/lexicon.rs
        - crates/podling-core/src/lib.rs
        - crates/podling-core/src/plugin/tts.rs
        - crates/podling-core/src/plugin/sidecar_tts.rs
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/stages/verify_audio.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/audio_e2e.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/sidecar_tts.rs
    arch: [ARCH-SPEECH-01, ARCH-SPEECH-02, ARCH-SPEECH-03, ARCH-SPEECH-04, ARCH-SPEECH-05, ARCH-SPEECH-06, ARCH-SPEECH-09, ARCH-SPEECH-10, ARCH-SPEECH-11, ARCH-SPEECH-12, ARCH-SPEECH-14, ARCH-SPEECH-16, ARCH-SPEECH-17]
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer, security-auditor],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: nes-voice-design
    project: .
    depends_on: [nes-types]
    module: scripts/voice_design offline design-then-clone tool
    language: python
    security: normal
    scope:
      read:
        - scripts/tts_bakeoff/pyproject.toml
        - scripts/tts_bakeoff/word_test.py
        - scripts/tts_bakeoff/README.md
        - crates/podling-types/src/episode.rs
      docs:
        - docs/architecture/speech.rules.md
      write:
        - scripts/voice_design/pyproject.toml
        - scripts/voice_design/uv.lock
        - scripts/voice_design/design.py
        - scripts/voice_design/README.md
        - scripts/voice_design/tests/test_design.py
    arch: [ARCH-SPEECH-13, ARCH-SPEECH-14, ARCH-SPEECH-15, ARCH-SPEECH-17]
    tooling: { implementer: implementer, gates: [code-reviewer, dependency-auditor],
               skills: [language-aware-planning], guards: [uv run pytest], mcp: [] }
  - id: 4
    scope_id: nes-docs
    project: .
    depends_on: [nes-core, nes-voice-design]
    module: docs, sidecar README, tunguska example
    language: markdown
    security: normal
    scope:
      read:
        - crates/podling-types/src/episode.rs
        - crates/podling-core/src/lexicon.rs
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/stages/verify_audio.rs
        - scripts/voice_design/README.md
      docs:
        - docs/architecture/speech.rules.md
        - docs/architecture.md
        - sidecars/tts/README.md
      write:
        - docs/architecture.md
        - sidecars/tts/README.md
        - examples/tunguska/episode-tts.toml
        - examples/tunguska/voices/README.md
        - docs/plans/natural-episode-speech.md
    arch: [ARCH-SPEECH-03, ARCH-SPEECH-06, ARCH-SPEECH-13, ARCH-SPEECH-15]
    tooling: { implementer: implementer, gates: [docs-curator],
               skills: [], guards: [cargo test], mcp: [] }
---

# Plan: natural episode speech

## Outcome
Names the TTS gets wrong can be respelt through a user-level `pronounce.toml` and the episode's `[tts.pronounce]`. The speech check and quotes still compare the original words. The check also folds British/American spellings and per-name heard-as variants. A self-designed voice must carry `LicenseRef-Podling-Generated` and a provenance file.

## Scope Steps (executable core)

### Step 1 — nes-types (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer, api-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: none
- [ ] 1.1 Add `Pronunciation` (`say: String`, `heard: Vec<String>`) and `Lexicon` to `episode.rs`. `Lexicon` is a `BTreeMap<String, Pronunciation>` newtype with `is_empty` and `iter`. Both are `Serialize`, `Deserialize` and `JsonSchema`. `TtsConfig::Sidecar` gains `#[serde(default, skip_serializing_if = "Lexicon::is_empty")] pronounce: Lexicon` → accept: the existing roundtrip tests pass.
- [ ] 1.2 Deserialise an entry untagged, as `Name = "say"` or `Name = { say = "...", heard = ["..."] }`; `heard` defaults to empty, and the table form denies unknown fields. Reject an empty or whitespace-only name, `say`, or `heard` item at deserialisation (ARCH-SPEECH-08) → accept: roundtrip tests parse both shapes and fail on each of the three empties.
- [ ] 1.3 Add `pub const GENERATED_VOICE_LICENCE = "LicenseRef-Podling-Generated"` to `VOICE_LICENCES`, which becomes `[&str; 4]`. Add no other ids (ARCH-SPEECH-13) → accept: a test asserts exactly those four ids; a `VoiceRef` with the generated licence parses.
- [ ] 1.4 Add `VoiceProvenance` (`model`, `weights_commit`, `design_prompt`, `tool_version`: non-empty `String`; `seed: u64`), with `deny_unknown_fields`, rejecting empty strings. Add `pub fn provenance_path(clip: &Path) -> PathBuf`, which appends `.provenance.json` to the full clip file name (`voices/host.wav` → `voices/host.wav.provenance.json`). Re-export both from `lib.rs`. It is not an artifact, so it has no schema export → accept: unit tests parse a full file; reject a missing field, an unknown field and an empty string; and test `provenance_path`.
- [ ] 1.5 Bump `SCHEMA_VERSION` 7→8 (`envelope.rs:8`) and its pin (`tests/schema_snapshot.rs:19`). Refresh `schema_snapshot__episode.snap` → accept: the snapshot diff adds only the optional `pronounce` property and its definitions (ARCH-SPEECH-07).
- [ ] 1.6 Add `pronounce: Lexicon::default()` to the struct-literal consumers (`crates/podling-types/tests/roundtrip.rs:209`, `crates/podling-core/src/plugin/mod.rs:329`) → accept: `cargo test --quiet` builds and passes for the workspace.

### Step 2 — nes-core (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer, security-auditor · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: nes-types
- [ ] 2.1 Add `SpokenTurn.say_as: Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`. `plain()` sets `None`, and `said()` still reads only `text` (ARCH-SPEECH-01/04) → accept: the existing `said_puts_the_speakers_own_backchannels_in_line` passes; a new test shows `said()` returns the original word while `say_as` is set.
- [ ] 2.2 `wire_turns` builds `WireTurn.text` from `turn.say_as.as_deref().unwrap_or(&turn.text)`, and `say_as` is read nowhere else (ARCH-SPEECH-02). Nothing changes under `sidecars/tts/podling_tts/` (ARCH-SPEECH-03) → accept: a `tests/sidecar_tts.rs` test with the fake sidecar shows the turn text in `last_request.json` carrying the respelling; `git diff --stat main -- sidecars/tts/podling_tts` is empty.
- [ ] 2.3 Add a new `src/lexicon.rs` with three pieces:
  - `Respellings::new(&Lexicon)` with `respell(&str) -> Option<String>`. It matches whole words, case-sensitively, longest name first, and returns `None` when nothing matched (ARCH-SPEECH-05).
  - `HeardVariants::new(&Lexicon)` with `apply(Vec<String>) -> Vec<String>`. It replaces each run of `words(variant)` in the transcript words with `words(name)` (ARCH-SPEECH-09).
  - `merge(user, episode) -> Lexicon`, where the episode wins per name (ARCH-SPEECH-06).

  → accept: unit tests cover a word boundary ("Kulik" is not matched inside "Kuliks"), case ("kulik" untouched), overlap order (a two-word name before its one-word prefix), no match → `None`, a multi-word heard variant, and a merge where both lexicons define a name.
- [ ] 2.4 In `lexicon.rs`, add `user_path(profiles: &Path) -> PathBuf`, which is `pronounce.toml` beside the sidecars file in use. Add `load_user(path) -> Result<Lexicon>`: the file has a `[pronounce]` table and `deny_unknown_fields`. A missing file is an empty lexicon; an unreadable or invalid file is a `CoreError::Config` naming it. `pipeline.rs` `AudioPlan::check` merges it with `TtsConfig::Sidecar.pronounce` for `kind = "sidecar"` only; the fake gets an empty lexicon → accept: tests for a missing file (empty), a malformed file (a Config error naming the path) and the pipeline merge.
- [ ] 2.5 `spoken()` takes `&Respellings` and sets `say_as` from the piece's text only, never from backchannel text. It is the only writer of `say_as`, used by `synthesize_script` and `context_for`. `synthesize_script` gains the respellings, and `Verification` gains `heard: &HeardVariants`. Update every caller listed in CONSUMERS → accept: `cargo test --quiet` passes.
- [ ] 2.6 In `synthesize_script`, after `spoken()`, add a `tracing::debug!` naming the chunk's turns and the lexicon names applied (ARCH-SPEECH-12) → accept: the call is present and names both.
- [ ] 2.7 `Check::new(expected, quotes, transcript, heard: &HeardVariants)` applies `heard` to the transcript words only, before WER and quote matching. Respellings never reach it (ARCH-SPEECH-04/09). Update `synthesize.rs:464` and the tests at `verify_audio.rs:621,626` → accept: a test where the transcript says the heard variant (e.g. "Koolik") against the reference name "Kulik" scores 0 WER, and scores more than 0 without the variant.
- [ ] 2.8 Fold British to American spellings in `words()` with a built-in explicit word-pair `const` table, applied to every token on both sides. It covers the singular and plural of kilometre, metre, centimetre, millimetre, centre, litre, theatre, colour, honour, favour, neighbour, harbour and programme, plus labour, behaviour, grey, defence, analyse(d), organise(d), realise(d), recognise(d), travelled, travelling, jewellery, catalogue and aluminium. No suffix rules and no new crate (ARCH-SPEECH-10) → accept: a test where "kilometres" vs "kilometers" scores 0 WER, and `words("acre genre")` returns `["acre","genre"]`.
- [ ] 2.9 `Voices::resolve` rejects a voice with `GENERATED_VOICE_LICENCE` when `provenance_path(base_dir.join(reference))` is missing or does not parse as `VoiceProvenance`. The `CoreError::Config` names the speaker and the expected path. Provenance stays out of `VoiceKey` and `audio.json` (ARCH-SPEECH-14) → accept: tests with the file (Ok), without it (Err naming the path) and with a malformed file (Err).
- [ ] 2.10 In `tests/audio_e2e.rs`, modelled on `a_thirty_minute_script_is_chunked_by_beat_and_cached_per_chunk`, test three things:
  - editing one name's respelling re-synthesises only the chunks whose turns contain it (with a context-free fake TTS as in that test, since context conditioning also keys the next chunk);
  - a warm rerun then makes 0 synthesis and 0 ASR calls;
  - an empty lexicon gives the same chunk keys as no lexicon.

  → accept: the tests pass.
- [ ] 2.11 Keep the no-`[tts]` invariant: `tests/pipeline.rs` `no_nli_config_writes_todays_artifacts` passes unchanged, apart from `schema_version`. No stage `VERSION`, `ADAPTER_VERSION` or `PROMPT_VERSION` changes (ARCH-SPEECH-11/16), and `MODEL` in `qwen.py` stays the same (ARCH-SPEECH-17) → accept: the test passes; `git diff main -- crates sidecars | grep -E "^[+-].*(const VERSION|PROMPT_VERSION|ADAPTER_VERSION|^.MODEL)"` is empty.

### Step 3 — nes-voice-design (., python, normal)
Tooling: implementer · gates code-reviewer, dependency-auditor · skills language-aware-planning · guards uv run pytest
Depends on: nes-types
- [ ] 3.1 Add `scripts/voice_design/pyproject.toml` as a `uv` project (`package = false`) on Python 3.11. Depend on `qwen-tts==0.1.1` (Apache-2.0), `torch`/`torchaudio` from the cu128 index and `soundfile`, with `pytest` in a dev group. Lock it, and run dependency-auditor on every direct dependency (ARCH-SPEECH-15) → accept: the auditor reports no non-commercial, AGPL or revenue-capped licence; `uv lock` succeeds.
- [ ] 3.2 Add `scripts/voice_design/design.py`, with `--description`, `--text`, `--seed` and `--out <clip.wav>`:
  - It generates the clip with Qwen3-TTS VoiceDesign, one GPU model, freed on exit.
  - It writes the clip, its transcript, and `<clip>.wav.provenance.json` with `model`, `weights_commit` (the resolved HF revision), `design_prompt`, `seed` and `tool_version`, exactly `VoiceProvenance`'s fields.
  - It prints a ready `voice = {..., licence = "LicenseRef-Podling-Generated"}` line.
  - It is not a sidecar, and the pipeline and `sidecars.toml` never reference it.

  → accept: `grep -rn voice_design crates sidecars` is empty.
- [ ] 3.3 Add `tests/test_design.py`, which tests the GPU-free parts: the provenance key set equals the Rust fields, the provenance path appends to the clip name, and the arguments are validated → accept: `uv run --directory scripts/voice_design pytest -q` passes.
- [ ] 3.4 Add `scripts/voice_design/README.md`, covering usage, the licence policy (clips stay local and uncommitted until the voice-marketplace idea decides otherwise) and each direct dependency's licence → accept: the README names every direct dependency's licence.

### Step 4 — nes-docs (., markdown, normal)
Tooling: implementer · gates docs-curator · guards cargo test
Depends on: nes-core, nes-voice-design
- [ ] 4.1 Document in `docs/architecture.md` "Episode audio" the lexicon (user file and episode, the episode winning, names only, `say_as` on the wire only), heard variants, the UK/US fold and the generated-voice provenance → accept: docs-curator finds every cited path resolves.
- [ ] 4.2 Add a "Pronunciation" section to `sidecars/tts/README.md`: `pronounce.toml` beside `sidecars.toml`, an example, and that the worker never sees the lexicon → accept: the section exists and matches `lexicon.rs`.
- [ ] 4.3 Add a commented `[tts.pronounce]` example to `examples/tunguska/episode-tts.toml`: names only, with single unhyphenated respellings, per the word test → accept: the episode still parses (`cargo test --quiet`, and it loads with the CLI).
- [ ] 4.4 Add self-designed voices to `examples/tunguska/voices/README.md`: the generated licence, the provenance file and `scripts/voice_design/` → accept: the section exists.

## Sequencing
1. nes-types first, so one `SCHEMA_VERSION` bump covers every type change.
2. nes-core and nes-voice-design next. Both need only the type names and share no files.
3. nes-docs last, so it describes merged code.

## Verification background (citations — for the reviewer, not the executor)
- Speech recognition is checked against `said()`: `crates/podling-core/src/plugin/tts.rs:61-68` and `crates/podling-core/src/stages/synthesize.rs:425`.
- `spoken()` builds every `SpokenTurn`, for synthesis and for context: `synthesize.rs:642`, called at `:416`, `:687` and `:721`.
- The chunk key includes `ChunkSpec.turns`, the serialised `SpokenTurn`s: `synthesize.rs:124-135`. `SynthesizeChunk::VERSION = 2` is at `:248`. A `None` `say_as` is skipped by serde, so the key is unchanged.
- The wire turn's text: `crates/podling-core/src/plugin/sidecar_tts.rs:57-66` and `:281-292`.
- `words()`: `crates/podling-core/src/stages/verify_audio.rs:29`. `Check::new`: `:326`.
- The sidecars file is located at `crates/podling-core/src/plugin/sidecar.rs:59`, or by the CLI's `--sidecars` (`crates/podling-cli/src/main.rs:47`).
- Voice clips are resolved against the episode directory in `Voices::resolve`, `synthesize.rs:60-90`. Rule 14 cites `episode.rs:187` for `VoiceRef`; its text ("where references are resolved against the episode directory") puts the check where `base_dir` is known.
- Whisper writes American spellings, and respelling common words makes them worse: `docs/ideas/natural-episode-speech.md`, "Word test".

CONSUMERS:
- `TtsConfig::Sidecar` gains `pronounce`.
  - Pattern matches with `..` need no change: `crates/podling-core/src/pipeline.rs:314,325` and `crates/podling-core/src/plugin/mod.rs:121,135`.
  - Struct literals need the new field: `crates/podling-types/tests/roundtrip.rs:209` and `crates/podling-core/src/plugin/mod.rs:329`.
- `SpokenTurn` gains `say_as`. Its only struct literal is `synthesize.rs:652`. Callers of `SpokenTurn::plain` are unaffected (`tests/sidecar_tts.rs:91`, `plugin/tts.rs:317`).
- `Check::new` gains `heard`: `synthesize.rs:464` and `verify_audio.rs:621,626`.
- `Verification` gains `heard`: `pipeline.rs:362`, `tests/audio_e2e.rs:404` and `synthesize.rs:819`.
- `synthesize_script` gains the respellings: `pipeline.rs:366`, `tests/audio_e2e.rs:412` and `synthesize.rs:834,874,933,1067,1237`.
- `VOICE_LICENCES`: `episode.rs:216,235` and `tests/roundtrip.rs:256`.
- Wire: the worker reads `WireTurn.text` as `turn["text"]` under `sidecars/tts/podling_tts`. The shape is unchanged, so nothing is edited there (ARCH-SPEECH-03).

## Risk & rollback
- **Respellings leaking into transcripts or quotes.** `say_as` is written only in `spoken()` and read only in `wire_turns`; tests 2.1, 2.2 and 2.7 cover this.
- **The UK/US table folding a real distinction.** It is an explicit word list with no suffix rules, and the acre/genre test guards it.
- **Rollback.** `git revert`. Cache entries at schema version 8 become foreign misses and are rebuilt.

## Out of scope
- Text normalisation (recommendation 2).
- A clip per emotion, and the banter prompt change (recommendation 7, which would need a `PROMPT_VERSION` bump).
- A model swap (recommendation 8).
- The voice marketplace.
- Recording applied names in `audio.json`.
- Mapping heard variants inside `spans_from`, which still aligns by edit distance.
