---
slug: speech-followups
goal: A padded lexicon name or a say-less pronounce table fails with its own clear error, a generated voice's provenance must match its clip's blake3 hash, and an e2e test proves a user pronounce.toml reaches the TTS request with the episode entry winning.
idea: docs/ideas/natural-episode-speech.md
classification: in-scope   # follow-ups to docs/ideas/natural-episode-speech.md; docs/architecture/speech.rules.md ARCH-SPEECH-18, ARCH-SPEECH-19 (amended d6c5d36)
tracker_rows: [TRACKER#speech-followups/sf-lexicon, TRACKER#speech-followups/sf-provenance, TRACKER#speech-followups/sf-e2e]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(config:never)
coverage:
  contract:      2.1-2.4 (VoiceProvenance gains clip_blake3; Python Provenance dataclass; SCHEMA_VERSION 8→9), 1.1-1.3 (LexiconError gains PaddedName; consumers in CONSUMERS)
  data:          N/A(no persistence/migration; provenance.json is a sidecar file of the clip, not a cached artifact; no stage VERSION bump, ARCH-SPEECH-11)
  config:        1.1-1.2 (episode [tts.pronounce] and user pronounce.toml parse stricter), 3.1 (stub --capture arg, test-only)
  security:      2.3 (generated-licence voice fails closed on a hash mismatch), 2.5 (blake3 PyPI dep through dependency-auditor)
  tests:         1.3, 2.4, 2.6, 3.2
  observability: N/A(errors are config errors naming the file and the field; no new runtime path)
  interface:     2.5 (design.py prints nothing new; writes clip_blake3)
  docs:          2.7 (scripts/voice_design/README.md, examples/tunguska/voices/README.md, docs/architecture.md:608)
  rollback:      git revert of the merge commits; SCHEMA_VERSION 9 cache entries read as foreign misses after revert and are rebuilt; a provenance.json with clip_blake3 is rejected by the reverted code (deny_unknown_fields), so re-run design.py or delete the field
units:
  - id: 1
    scope_id: sf-lexicon
    project: .
    depends_on: []
    module: podling-types lexicon parsing
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-types/tests/snapshots/schema_snapshot__episode.snap
        - crates/podling-core/src/lexicon.rs
      docs:
        - docs/architecture/speech.rules.md
        - docs/architecture/privacy.rules.md
      write:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
    arch: [ARCH-SPEECH-18, ARCH-SPEECH-07, ARCH-PRIVACY-03]
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: sf-provenance
    project: .
    depends_on: [sf-lexicon]
    module: voice provenance clip hash (Rust + voice_design)
    language: rust, python
    security: normal
    scope:
      read:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/tests/pipeline.rs
        - scripts/voice_design/design.py
        - scripts/voice_design/pyproject.toml
        - scripts/voice_design/README.md
        - scripts/voice_design/tests/test_design.py
        - examples/tunguska/voices/README.md
        - docs/architecture.md
      docs:
        - docs/architecture/speech.rules.md
        - docs/architecture/privacy.rules.md
      write:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-types/tests/snapshots/schema_snapshot__episode.snap
        - crates/podling-core/src/stages/synthesize.rs
        - scripts/voice_design/design.py
        - scripts/voice_design/pyproject.toml
        - scripts/voice_design/uv.lock
        - scripts/voice_design/README.md
        - scripts/voice_design/tests/test_design.py
        - examples/tunguska/voices/README.md
        - docs/architecture.md
    arch: [ARCH-SPEECH-19, ARCH-SPEECH-15, ARCH-SPEECH-11, ARCH-SPEECH-03, ARCH-PRIVACY-03]
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer, dependency-auditor],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test, uv run pytest], mcp: [] }
  - id: 3
    scope_id: sf-e2e
    project: .
    depends_on: [sf-lexicon, sf-provenance]
    module: podling-core e2e pronounce test
    language: rust, python
    security: normal
    scope:
      read:
        - crates/podling-core/tests/audio_e2e.rs
        - crates/podling-core/tests/fixtures/fake_sidecar.py
        - crates/podling-core/tests/fixtures/episode.toml
        - crates/podling-core/tests/fixtures/llm/write_script.json
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/src/lexicon.rs
      docs:
        - docs/architecture/speech.rules.md
      write:
        - crates/podling-core/tests/audio_e2e.rs
        - crates/podling-core/tests/fixtures/fake_sidecar.py
    arch: [ARCH-SPEECH-06, ARCH-SPEECH-03]
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: speech-followups

## Outcome
A padded lexicon name or a say-less pronounce table fails with its own clear error, a generated voice's provenance must match its clip's blake3 hash, and an e2e test proves a user pronounce.toml reaches the TTS request with the episode entry winning.

## Scope Steps (executable core)

### Step 1 — sf-lexicon (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: none
- [ ] 1.1 In `Lexicon::new` (episode.rs), keep `EmptyName` for an all-whitespace name and add `LexiconError::PaddedName(String)` for `name != name.trim()`, its message quoting the name and saying "leading or trailing whitespace" → accept: `Lexicon::new` with `" Kulik"` returns `Err(PaddedName(" Kulik".into()))`; `" "` still returns `EmptyName`.
- [ ] 1.2 Replace `#[serde(untagged)]` on `RawPronunciation` with a hand-written `Deserialize` (a `Visitor` whose `visit_str` gives `Say` and whose `visit_map` deserialises `PronunciationTable` through `serde::de::value::MapAccessDeserializer`), keeping the JSON schema via `#[schemars(untagged)]` → accept: a table without `say` fails with an error containing `` `say` ``; an unknown key still fails; both shapes still parse; the episode schema snapshot is byte-identical.
- [ ] 1.3 Add roundtrip tests in `crates/podling-types/tests/roundtrip.rs`: TOML `" Kulik" = "Koolick"`, `"Kulik " = {...}` and `Kulik = { heard = ["Koolik"] }` fail, each asserting its own message → accept: `cargo test -p podling-types` passes.

### Step 2 — sf-provenance (., rust+python, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer, dependency-auditor · skills language-aware-planning · guards cargo fmt/clippy/test, uv run pytest
Depends on: sf-lexicon
- [ ] 2.1 Add `clip_blake3` to `VoiceProvenance` as a `blake3::Hash` (`RawVoiceProvenance` holds the string; `TryFrom` parses it with `Hash::from_hex` after requiring lowercase, so a bad hash cannot be represented; serialised back as lowercase hex) with an accessor; turn `EmptyProvenanceField` into a `ProvenanceError` enum (`Empty(&'static str)`, `BadClipHash`) and update the `lib.rs` re-export → accept: roundtrip tests parse a full file with the hash, and reject a missing, uppercase or short hash.
- [ ] 2.2 Bump `SCHEMA_VERSION` 8→9 (envelope.rs) and the assertion in `schema_snapshot.rs`; run `cargo insta` / the snapshot test and accept only a diff (if any) limited to the touched types → accept: `cargo test -p podling-types` passes.
- [ ] 2.3 In `check_provenance` (synthesize.rs:137), after parsing, hash the clip's bytes with `blake3::hash` and compare it to `clip_blake3()` (`blake3::Hash` equality is constant-time); on mismatch return `CoreError::Config` naming the speaker, the clip, and "does not match" → accept: unit tests: no provenance file → refused; mismatched hash → refused with "does not match"; matching hash → accepted.
- [ ] 2.4 In `design.py`, add `clip_blake3: str` to `Provenance` and compute it from the written clip's bytes with `blake3.blake3(data).hexdigest()` in a pure function `clip_hash(path)` → accept: `test_provenance_has_exactly_the_rust_fields` passes; a new test checks that `clip_hash` equals the known blake3 of fixed bytes (the same vector as a Rust test).
- [ ] 2.5 Add `blake3` to `scripts/voice_design/pyproject.toml`, `uv lock`, add it to the README licence table; run dependency-auditor on it → accept: auditor reports no blocking finding; licence is commercial-safe.
- [ ] 2.6 Keep the lazy-import test green: `blake3` is imported where it is used, or is shown light enough → accept: `uv run --directory scripts/voice_design pytest -q` exits 0.
- [ ] 2.7 Add `clip_blake3` to the provenance field lists in `scripts/voice_design/README.md:33`, `examples/tunguska/voices/README.md:55`, `docs/architecture.md:608` → accept: `grep -n clip_blake3` hits each file.

### Step 3 — sf-e2e (., rust+python, normal)
Tooling: implementer · gates code-reviewer · guards cargo fmt/clippy/test
Depends on: sf-lexicon, sf-provenance
- [ ] 3.1 Add `--capture <path>` to `crates/podling-core/tests/fixtures/fake_sidecar.py`: every request body is also appended as one JSON line to that path (it outlives the worker's run dir, which is deleted on drop), beside the existing `last_request.json` → accept: existing `sidecar_tts.rs` tests still pass.
- [ ] 3.2 Add a `run_with_sidecars` test in `audio_e2e.rs` (guarded by `python_ok()`): `pronounce.toml` beside `sidecars.toml` respells `Tunguska` and `June` (both whole words in the fixture script's turns, `tests/fixtures/llm/write_script.json`); the episode's `[tts.pronounce]` respells `Tunguska` differently; the stub runs with `--capture` → accept: some wire turn's `text` contains the episode respelling, some contains the user-only respelling, none contains the user respelling of the shared name.
- [ ] 3.3 Tighten `a_bad_user_lexicon_beside_the_profiles_fails_before_any_stage_runs` to also assert the message names `say` → accept: test passes.

## Sequencing
sf-lexicon → sf-provenance (both edit episode.rs, so they run one after the other) → sf-e2e, which runs last against the final types.

## Verification background
- `RawPronunciation` is `#[serde(untagged)]` — `crates/podling-types/src/episode.rs:380`
- `Lexicon::new` checks only an empty name — `crates/podling-types/src/episode.rs:448-453`
- `VoiceProvenance` / `RawVoiceProvenance` with `deny_unknown_fields` — `crates/podling-types/src/episode.rs:291-315`
- `check_provenance` parses but never ties the file to the clip — `crates/podling-core/src/stages/synthesize.rs:137-155`
- workspace already depends on blake3 — `Cargo.toml:20`, `crates/podling-core/Cargo.toml:11`
- Python/Rust field-set test — `scripts/voice_design/tests/test_design.py:21`
- `SCHEMA_VERSION = 8` asserted — `crates/podling-types/tests/schema_snapshot.rs:19`
- SidecarTts run dir is a `TempDir` dropped with the worker — `crates/podling-core/src/plugin/sidecar_tts.rs:107-113,138`
- User lexicon merged in the pipeline — `crates/podling-core/src/pipeline.rs:337`

CONSUMERS:
- `LexiconError` → `crates/podling-types/tests/roundtrip.rs:380`, `crates/podling-core/src/lexicon.rs:53` (maps the parse error to a config error naming the file)
- `VoiceProvenance` → `crates/podling-core/src/stages/synthesize.rs:23,140`, `crates/podling-types/tests/roundtrip.rs:395-412`, `scripts/voice_design/tests/test_design.py:23` (parses `RawVoiceProvenance` fields)
- `EmptyProvenanceField` → `crates/podling-types/src/lib.rs:26` (re-export only)
- provenance field list (prose) → `scripts/voice_design/README.md:33`, `examples/tunguska/voices/README.md:55`, `docs/architecture.md:608`
- `SCHEMA_VERSION` → `crates/podling-types/tests/schema_snapshot.rs:19`, `crates/podling-core/tests/pipeline.rs:87,107` (read the constant, so they need no edit)

## Risk & rollback
A provenance file written before this change is rejected (unknown or missing `clip_blake3`); re-run design.py. The repo has no committed provenance files. The SCHEMA_VERSION bump makes every cached artifact miss once. Revert the merge commits to roll back.

## Out of scope
No change under `sidecars/tts/podling_tts/` (ARCH-SPEECH-03); no `SynthesizeChunk`/`TranscribeChunk` VERSION, `ADAPTER_VERSION` or `PROMPT_VERSION` bump; no trimming of names (rejected instead); no other lexicon or provenance fields.
