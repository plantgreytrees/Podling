---
slug: phase5-tts-audio
goal: "`podling run` turns the script into one listenable, broadcast-loud episode file (30–60 min) with consistent voices, natural banter timing and every verbatim quote checked by ear-equivalent ASR, on an 8 GB GPU with one model loaded at a time."
classification: in-scope   # .claude/CLAUDE.md "Modularity" (TTS + ASR providers), "Hardware target"; docs/architecture.md "Deferred to later phases: TTS and ASR provider traits"
tracker_rows: [TRACKER#phase5-tts-audio/1, TRACKER#phase5-tts-audio/2, TRACKER#phase5-tts-audio/3, TRACKER#phase5-tts-audio/4, TRACKER#phase5-tts-audio/5, TRACKER#phase5-tts-audio/6, TRACKER#phase5-tts-audio/7, TRACKER#phase5-tts-audio/8, TRACKER#phase5-tts-audio/9, TRACKER#phase5-tts-audio/10]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(root-only agent mode: agent-mode-guard blocks non-strategist Task; the plan-strategist pass supplied the independent decomposition, see "Decomposition")
coverage:
  contract:      2.1–2.6 (EpisodeSpec cast/tts/asr, AudioManifest, ArtifactKind::Audio, SCHEMA_VERSION), 4.1 (TtsProvider + ChunkRequest), 5.2 (WriteScript honours a declared cast), 5.3 (LlmProvider::release), 6.1–6.4 (Beat/Pace/Nonverbal/callback_to on Script + ScriptDraft, PROMPT_VERSION), 8.1 (AsrProvider), 3.2 (sidecar HTTP protocol, versioned)
  data:          4.5–4.6 (content-addressed audio blob store beside the JSON cache; `cache clear`/`stats` learn its shape). N/A for databases: none exist
  config:        2.1–2.3 ([[cast]] with pinned voices, [tts], [asr] episode sections), 4.3 (user-level sidecar profiles, never in the episode file), 5.3 ([llm] unload_after for Ollama), 10.1 (examples/tunguska/episode-tts.toml)
  security:      4.3–4.4 (sidecar program comes from user config, argv only, no shell, 127.0.0.1 only, killed on Drop), 3.3 (sidecar binds loopback, validates paths under a run dir), 4.2 (sidecar responses size-capped and validated), 2.2 (voice licence recorded per clip), 1.4 + 3.5 + 8.6 (dependency and weight licence audit)
  tests:         2.6, 3.4, 4.7, 4.8, 5.6, 5.7, 6.5, 6.6, 7.5, 7.6, 8.7, 8.8, 9.6, 9.7, 10.2–10.3
  observability: 4.4 (sidecar spawn/ready/exit spans with pid and elapsed_ms), 5.4 (per-chunk synth span: seconds of audio, RTF), 8.5 (per-chunk verify log: WER, quote misses, retries, take chosen), 9.5 (episode LUFS / true peak / duration logged and in the manifest)
  interface:     5.5 (CLI prints the episode audio path; readable Config error naming a missing voice or sidecar profile), 4.4 (OOM / sidecar-not-ready hint naming `ollama stop` and the profile file)
  docs:          10.4 (architecture.md), 10.5 (README), 10.6 (handoff.md), 3.6 (sidecars/tts/README.md)
  rollback:      git revert of the branch. Without [tts] the pipeline runs today's stages and writes today's artifacts plus the schema_version bump (tests 5.7, 6.6); the sidecar is a separate directory with no Rust dependency on it
units:
  - id: 1
    scope_id: tts-bakeoff
    project: .
    depends_on: []
    module: scripts/tts_bakeoff
    language: python
    security: normal
    scope:
      read: [examples/tunguska/episode-ollama.toml, crates/podling-types/src/script.rs]
      docs: [.claude/CLAUDE.md, docs/plans/phase5-tts-audio.md]
      write:
        - scripts/tts_bakeoff/pyproject.toml
        - scripts/tts_bakeoff/bakeoff.py
        - scripts/tts_bakeoff/README.md
        - scripts/tts_bakeoff/.gitignore
        - docs/plans/phase5-tts-audio.md
    tooling: { implementer: implementer, gates: [dependency-auditor], skills: [], guards: [], mcp: [] }
  - id: 2
    scope_id: audio-contracts
    project: .
    depends_on: [1]
    module: crates/podling-types
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/schema.rs
        - crates/podling-types/src/script.rs
        - crates/podling-types/src/ids.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/audio.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/schema.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-types/tests/snapshots/
        - crates/podling-core/tests/fixtures/golden/
    tooling: { implementer: implementer, gates: [code-reviewer, api-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: tts-sidecar
    project: .
    depends_on: [1, 2]
    module: sidecars/tts
    language: python
    security: high
    scope:
      read: [scripts/tts_bakeoff/bakeoff.py, crates/podling-types/src/audio.rs]
      docs: [docs/plans/phase5-tts-audio.md]
      write:
        - sidecars/tts/pyproject.toml
        - sidecars/tts/uv.lock
        - sidecars/tts/podling_tts/__init__.py
        - sidecars/tts/podling_tts/server.py
        - sidecars/tts/podling_tts/protocol.py
        - sidecars/tts/podling_tts/backends/
        - sidecars/tts/tests/
        - sidecars/tts/README.md
        - .gitignore
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor, dependency-auditor],
               skills: [], guards: [], mcp: [] }
  - id: 4
    scope_id: tts-provider
    project: .
    depends_on: [2]
    module: crates/podling-core/src/plugin
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/plugin/http.rs
        - crates/podling-core/src/plugin/nli.rs
        - crates/podling-core/src/plugin/openai.rs
        - crates/podling-core/src/cache.rs
        - crates/podling-core/src/error.rs
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/audio.rs
        - crates/podling-cli/src/commands.rs
      docs: [docs/architecture.md]
      write:
        - Cargo.toml
        - Cargo.lock
        - crates/podling-core/Cargo.toml
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/plugin/tts.rs
        - crates/podling-core/src/plugin/sidecar.rs
        - crates/podling-core/src/plugin/sidecar_tts.rs
        - crates/podling-core/src/audio.rs
        - crates/podling-core/src/cache.rs
        - crates/podling-core/src/error.rs
        - crates/podling-core/src/lib.rs
        - crates/podling-core/tests/sidecar_tts.rs
        - crates/podling-cli/src/commands.rs
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor, dependency-auditor, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 5
    scope_id: audio-e2e
    project: .
    depends_on: [3, 4]
    module: crates/podling-core/src/stages
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/src/stage.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/plugin/openai.rs
        - crates/podling-core/src/plugin/tts.rs
        - crates/podling-core/src/audio.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-cli/src/commands.rs
        - crates/podling-cli/tests/cli.rs
      docs: [docs/architecture.md]
      write:
        - Cargo.toml
        - Cargo.lock
        - crates/podling-core/Cargo.toml
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/stages/assemble.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/plugin/openai.rs
        - crates/podling-core/src/audio.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/tts/
        - crates/podling-cli/src/commands.rs
        - crates/podling-cli/tests/cli.rs
        - examples/tunguska/episode.toml
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer, performance-reviewer, dependency-auditor],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 6
    scope_id: script-beats
    project: .
    depends_on: [5]
    module: crates/podling-types/src/script.rs, crates/podling-core/src/stages/script.rs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-types/src/script.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/plugin/analyser.rs
        - crates/podling-core/tests/pipeline.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-types/src/script.rs
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/snapshots/
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/plugin/analyser.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/golden/
    tooling: { implementer: implementer, gates: [code-reviewer, api-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 7
    scope_id: beat-chunker
    project: .
    depends_on: [5, 6]
    module: crates/podling-core/src/stages/plan_chunks.rs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-types/src/script.rs
        - crates/podling-types/src/audio.rs
        - crates/podling-core/src/plugin/tts.rs
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/cache.rs
        - crates/podling-core/src/text.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-core/src/stages/plan_chunks.rs
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/plugin/tts.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-types/src/audio.rs
        - crates/podling-types/tests/snapshots/
        - sidecars/tts/podling_tts/protocol.py
        - sidecars/tts/podling_tts/backends/
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer, performance-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 8
    scope_id: asr-verify
    project: .
    depends_on: [5]
    module: crates/podling-core/src/plugin/whisper.rs, crates/podling-core/src/stages/verify_audio.rs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/plugin/cross_encoder.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/src/audio.rs
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/audio.rs
        - crates/podling-types/src/analysis.rs
      docs: [docs/architecture.md]
      write:
        - Cargo.toml
        - Cargo.lock
        - crates/podling-core/Cargo.toml
        - crates/podling-core/src/plugin/asr.rs
        - crates/podling-core/src/plugin/whisper.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/stages/verify_audio.rs
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/whisper_parity.rs
        - crates/podling-types/src/audio.rs
        - crates/podling-types/tests/snapshots/
    tooling: { implementer: implementer, gates: [code-reviewer, dependency-auditor, performance-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 9
    scope_id: full-assembler
    project: .
    depends_on: [7, 8]
    module: crates/podling-core/src/stages/assemble.rs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/assemble.rs
        - crates/podling-core/src/audio.rs
        - crates/podling-types/src/script.rs
        - crates/podling-types/src/audio.rs
        - crates/podling-types/src/episode.rs
      docs: [docs/architecture.md]
      write:
        - Cargo.toml
        - Cargo.lock
        - crates/podling-core/Cargo.toml
        - crates/podling-core/src/stages/assemble.rs
        - crates/podling-core/src/audio.rs
        - crates/podling-core/src/encode.rs
        - crates/podling-core/src/lib.rs
        - crates/podling-core/tests/assemble.rs
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/audio.rs
        - crates/podling-types/tests/snapshots/
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor, performance-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 10
    scope_id: live-and-docs
    project: .
    depends_on: [9]
    module: examples/tunguska, docs
    language: markdown
    security: normal
    scope:
      read: [examples/tunguska/episode-ollama.toml, crates/podling-types/src/episode.rs, sidecars/tts/README.md]
      docs: [docs/architecture.md, README.md, docs/handoff.md, docs/plans/phase5-tts-audio.md]
      write:
        - examples/tunguska/episode-tts.toml
        - examples/tunguska/voices/README.md
        - .gitignore
        - crates/podling-cli/tests/cli.rs
        - docs/architecture.md
        - README.md
        - docs/handoff.md
        - docs/plans/phase5-tts-audio.md
    tooling: { implementer: implementer, gates: [docs-curator], skills: [], guards: [cargo test], mcp: [] }
---

# Plan: Phase 5 — text-to-speech and episode audio

## Outcome
`podling run` turns the script into one listenable, broadcast-loud episode file (30–60 min)
with consistent voices, natural banter timing and every verbatim quote checked by speech
recognition, on an 8 GB GPU with one model loaded at a time.

## Design (decided here; the executor follows it)

### Principles: "fantastic but lightweight"

| Lever | Decision |
|---|---|
| VRAM | One GPU model at a time. The TTS model lives in a **sidecar process that Podling spawns and kills**; ending the process is the only reliable way to free VRAM. The LLM is unloaded before synthesis starts (5.3). ASR runs **on the CPU** in Rust (candle Whisper), so it never competes for the card and can check each chunk straight after it is made, without reloading anything. |
| Memory per chunk | Chunks of 60–150 s. Generation memory grows with context, so a short window keeps the peak flat whatever the episode length. |
| Compute | Everything is cached by content hash: one blob per chunk take. Editing one turn re-synthesises one chunk. Only chunks that fail verification are regenerated, and only banter beats get more than one take. |
| Voice consistency | Every chunk is conditioned on the same pinned reference clip per speaker. A chunk is **never** conditioned on the previous chunk's output alone, so errors can't compound. |
| Seams | Chunks are cut only between beats (never inside a setup→punchline→reaction). The previous beat is passed as context and is not part of the output. The model's own leading/trailing silence is trimmed and the assembler inserts scripted gaps. |
| Loudness | Per-chunk loudness match, then episode normalisation to **−16 LUFS integrated, −1 dBTP true peak** (podcast standard), measured with `ebur128` (EBU R128). |
| Format | Internal: 48 kHz mono `f32` (resampled from the model's native rate with `rubato`). Output: `episode.wav` (16-bit PCM, `hound`) plus optional Opus/MP3 through an external `ffmpeg` (argv, no shell). |

### One TTS trait for both backend types

The strategist pass (see "Decomposition") rejected two traits. A per-turn model is a dialogue
model called with one turn, so one chunk-shaped request covers both:

```rust
/// What a backend can do; the chunk planner reads it.
pub struct TtsCapabilities {
    pub multi_speaker: bool,     // false → the planner sends one turn per chunk
    pub max_chunk_secs: u32,     // e.g. Dia2 ≈ 120
    pub max_speakers: u8,
    pub native_sample_rate: u32,
}

pub struct ChunkRequest<'a> {
    pub turns: &'a [SpokenTurn],          // text with quotes filled in, speaker, emotion, nonverbals
    pub voices: &'a BTreeMap<SpeakerId, VoiceRef>, // pinned reference clip + transcript, every chunk
    pub context: Option<ChunkContext<'a>>, // previous beat (audio blob + text), callback clips: conditioning only
    pub seed: u64,
}

pub struct ChunkAudio {
    pub pcm: Pcm,                         // native rate, mono f32; context NOT included
    /// Where each turn starts/ends in `pcm`. Per-turn backends always fill this;
    /// dialogue backends may return `None`, and ASR timestamps fill it later (8.4).
    pub turn_spans: Option<Vec<Range<usize>>>,
}

pub trait TtsProvider {
    fn id(&self) -> &str;
    fn fingerprint(&self) -> Value;       // backend, model, weights hash, protocol version
    fn capabilities(&self) -> &TtsCapabilities;
    fn synthesize(&mut self, request: &ChunkRequest<'_>) -> Result<ChunkAudio>;
}
```

`&mut self` (unlike the other providers' `&self`) because a sidecar provider owns a child
process it may start lazily: the borrow checker then guarantees nothing else uses it
concurrently. The pipeline builds the TTS provider just before synthesis and drops it right
after, as Phase 3 does with `Grounding`, so `Drop` ends the sidecar and frees the card before
assembly. `FakeTts` makes deterministic sine tones (pitch per speaker, length per word),
so the whole audio path is testable offline.

### The sidecar: a Python model worker behind local HTTP

- **Protocol** (`/v1/podling`, versioned): `GET /health` → `{protocol, backend, model, capabilities}`;
  `POST /synthesize` → JSON request naming reference clips and an `out_path` **inside a run
  directory Podling created**; the worker writes a WAV there and returns `{sample_rate, turn_spans?}`.
  Audio moves through files, not HTTP bodies, so no large payloads and the existing size caps in
  `plugin/http.rs` stay small. `POST /unload` frees the model without killing the process.
- **Who starts it.** The episode file names a sidecar *profile* (`[tts] sidecar = "dia2"`); the
  program and argv come from the **user-level** `~/.config/podling/sidecars.toml` (or `--sidecars`).
  Episode files are meant to be shareable, and a shared file must never be able to start a process.
  Spawned with `std::process::Command` (argv, no shell), bound to `127.0.0.1` on a port Podling
  picks, waited on `/health` with a timeout, killed and reaped in `Drop`.
- **Backends inside the worker** are adapters chosen by the spike (unit 1): the winner is
  implemented first, plus at most one fallback. Emotion and nonverbal events are mapped to each
  model's own syntax (text tags for Dia2/MOSS-TTSD, a style instruction for Qwen3-TTS) **in the
  adapter**, so the Rust side never learns model-specific tags.

### Voices are pinned by the episode, not invented by the LLM

Today the LLM returns the cast (`ScriptDraft.cast`, `plugin/llm.rs:113`). A pinned voice needs a
known speaker id, so the episode gains an optional `[[cast]]`; when present, `WriteScript` is told
the cast and rejects a draft that uses any other speaker. Each member has a `voice`:
`{ reference = "voices/host.wav", transcript = "...", licence = "CC0-1.0" }`. The licence string is
required and copied into the audio manifest: voice clips are where non-commercial terms sneak in
(Kyutai's Expresso/EARS voices are CC-BY-NC).

### Banter that survives chunking (script contract additions)

| Addition | Shape | Used by |
|---|---|---|
| `Script.beats` | `Vec<Beat { kind: BeatKind, turns: TurnRange }>` (`TurnRange { start, end }`, `try_from` rejects `start >= end`), contiguous, covering every turn; `BeatKind = Narration \| Banter \| QuoteReading \| Transition` | chunk planner (never splits a beat), best-of-N (banter only) |
| `Turn.pace` | `Pace = Quick \| Normal \| Beat \| LongPause \| Interrupt` (gap *before* the turn) | assembler gaps; `Interrupt` = negative gap + crossfade |
| `Turn.nonverbal` | `Vec<Nonverbal { kind: Laugh {} \| Chuckle {} \| Sigh {} \| Backchannel { text }, by: SpeakerId, at: NonverbalAt }>` (struct variants: serde's internally tagged enums can't hold tuple variants) (`Inline` before/after the words, or `Over` = on the second track while this turn plays) | TTS adapter tags; assembler second track |
| `Turn.callback_to` | `Option<usize>` (an earlier turn index) | chunk planner adds that turn's cached clip as context |

All are `#[serde(default)]`, so old scripts deserialise. Validation (in `Script::new`, so
deserialisation runs it too): beats contiguous and covering; `callback_to < own index`; `by` in the
cast. **Grounding guardrail:** banter turns are validated exactly like every other turn (citations
must name ledger claims, quotes must be verbatim); the prompt says banter adds no new facts, and the
new opt-in analyser `uncited_figures` warns on any turn that states a number or a year with no
citation.

### Verification: speech recognition, then regenerate only failures

`AsrProvider::transcribe(&Pcm) -> Transcript { segments: Vec<{ text, start, end }> }`.
`CandleWhisper` runs Whisper (MIT weights; `base.en` or `small.en`, decided by the spike) natively
on the CPU with candle, loaded once per stage like `CrossEncoderNli`. A chunk **passes** when (a)
normalised word error rate against the chunk text ≤ `max_wer_pm` (default 80 ‰), and (b) every quote
in the chunk appears in the transcript verbatim after normalisation (case, punctuation, numbers
spelled out). A failed chunk is regenerated with a new seed up to `max_retries` (default 2); still
failing → an `Error` finding in the analysis report (the run completes; the episode is marked
unverified, mirroring how the analysis findings work today). Banter beats get `takes` (default 2)
and the best passing take wins (lowest WER, then speech rate closest to the speaker's median).
Segment timestamps fill `turn_spans` for dialogue chunks, snapped to the nearest silence.

### Seeds, caching and errors

- **Seeds are derived, never random:** `seed = first 8 bytes of BLAKE3(chunk cache key ‖ attempt)`.
  A rerun reproduces every take, so the take that passed is a cache hit.
- **Per-chunk caching goes through the existing `cached()`**, using a `SynthesizeChunk` stage (input:
  the chunk, its voices' file hashes, its context hashes, the attempt; output:
  `ChunkResult { blob, turn_spans, verification }`). Audio goes in a separate `BlobStore`
  (`<cache>/blobs/` when caching, `<out>/.blobs/` otherwise). **A cached `ChunkResult` whose blob
  is missing is a miss**, so a cache clear can't leave dangling references.
- **Error model:** infrastructure failures are `CoreError` (`Config` for a missing profile, voice
  or companion section; `Provider` for sidecar transport and startup; `InvalidProviderOutput` for
  bad PCM, spans or protocol). A chunk that fails *verification* is not an error. It becomes an
  `Error` finding in the analysis report with `verified: false`, the same way quote findings work
  today, so a 60-minute run never dies at minute 59 over one line.

### Pipeline

```
… script → analyse → [plan_chunks] → [synthesize (+ verify per chunk)] → [assemble]
```

The bracketed stages run only when the episode has `[tts]`. Without it, nothing changes except the
schema version. `synthesize` caches **per chunk take** (key: chunk text, voices' file hashes,
context hashes, seed, TTS fingerprint), not per stage, so a 60-minute episode is ~30 independent
cache entries and a crash at minute 40 loses one chunk.

## Scope Steps (executable core)

### Step 1 — tts-bakeoff (., python, normal)
Tooling: implementer · gates dependency-auditor
Depends on: none
- [ ] 1.1 Write `scripts/tts_bakeoff/pyproject.toml` (uv; Python 3.11) with only commercial-safe deps; record each dep's licence in README → accept: `uv sync` succeeds; README table lists every direct dep + licence; none is AGPL/NC.
- [ ] 1.2 Write `scripts/tts_bakeoff/bakeoff.py`: reads a Podling `script.json`, packs turns into ≤120 s chunks, runs one backend per process (MOSS-TTSD NF4, Dia2-1B, Qwen3-TTS-1.7B), pinned reference clips, writes chunk WAVs + `results.json` → accept: `python bakeoff.py --backend <b> --script <path>` writes ≥5 chunk WAVs for each backend on the Tunguska script.
- [ ] 1.3 Measure per backend: peak VRAM (`torch.cuda.max_memory_allocated` and `nvidia-smi` polling), RTF, speaker similarity of each chunk to its reference (ECAPA cosine, Apache-2.0 model), WER and quote hits with Whisper `base.en` and `small.en` on CPU (incl. CPU RTF) → accept: `results.json` has every metric for every backend; Ollama model unloaded during runs (recorded).
- [ ] 1.4 Listening pass: rate seams (1–5) and banter timing on chunks 1, 3, 5; confirm weight licences on the model cards → accept: notes recorded per backend.
- [ ] 1.5 Record "Spike results" in this plan: the default backend (must fit ≤ 7.0 GB peak, RTF ≤ 2.0, licence clean), a fallback, Whisper size, and any change to the design above → accept: section exists with numbers and a one-line decision; downstream units' text updated if the decision changes them.

### Step 2 — audio-contracts (., rust, normal)
Tooling: implementer · gates code-reviewer, api-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: tts-bakeoff
- [ ] 2.1 Add `EpisodeSpec.cast: Vec<CastMember { id, name, role, voice: VoiceRef }>` (`#[serde(default)]`) → accept: an episode without `[[cast]]` parses unchanged; duplicate ids rejected.
- [ ] 2.2 Add `VoiceRef { reference: PathBuf, transcript: String, licence: String }` with `deny_unknown_fields`; empty licence rejected → accept: unit test for the empty-licence error.
- [ ] 2.3 Add `TtsConfig { Fake {}, Sidecar { sidecar: String, takes: u8, max_retries: u8 } }` and `AsrConfig { Fake {}, Whisper { model_dir: PathBuf, max_wer_pm: PerMille } }` (`PerMille` from Phase 3 keeps the config `Eq` and rejects > 1000); `[tts]` requires `[[cast]]` and `[asr]` (checked at build, a `Config` error) → accept: tests for each missing-companion error message.
- [ ] 2.4 Add `crates/podling-types/src/audio.rs` with `TurnRange` (validated `try_from`), `AudioManifest { sample_rate, chunks: Vec<ChunkRecord>, episode: EpisodeAudio { path, duration_ms, integrated_lufs, true_peak_dbtp }, voices: Vec<VoiceCredit> }`, `ChunkRecord { id, turns: TurnRange, blob: ContentHash, seed, take, wer_pm: PerMille, quote_misses, verified }` → accept: roundtrip test.
- [ ] 2.5 Add `ArtifactKind::Audio` (`audio.json`), register in `schema.rs`, bump `SCHEMA_VERSION` 3→4 → accept: `ArtifactKind::ALL` has 8 entries; schema export writes `audio.schema.json`.
- [ ] 2.6 Accept the new insta snapshots and refresh golden fixtures → accept: `cargo test -p podling-types` and `cargo test -p podling-core` pass.

### Step 3 — tts-sidecar (., python, high)
Tooling: implementer · gates code-reviewer, security-auditor, dependency-auditor
Depends on: tts-bakeoff, audio-contracts
- [ ] 3.1 Create `sidecars/tts` (uv project, `podling_tts` package) with a stdlib-or-minimal HTTP server → accept: `uv run podling-tts --port 0 --backend fake` answers `/health`.
- [ ] 3.2 Implement protocol v1 in `protocol.py` (`/health`, `/synthesize`, `/unload`), request validated with explicit types; protocol version in `/health` → accept: malformed request → 400 with a reason; unknown field → 400.
- [ ] 3.3 Bind to `127.0.0.1` only; refuse `out_path`/reference paths outside the `--run-dir` given at start (resolve symlinks first) → accept: test with `../` and a symlink escape both → 400.
- [ ] 3.4 Implement the spike winner's adapter in `backends/` (emotion + nonverbal → model syntax, pinned references, context conditioning, seed, turn spans when available) and a `fake` backend; free VRAM check at load with a readable error → accept: `pytest` passes for `fake`; a live call with the winner writes a WAV.
- [ ] 3.5 Audit deps and weights licences → accept: README licence table; no AGPL/NC/revenue-capped entries.
- [ ] 3.6 Write `sidecars/tts/README.md`: install, model download, the `sidecars.toml` profile snippet → accept: steps reproduce a running sidecar on a fresh clone.

### Step 4 — tts-provider (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor, dependency-auditor, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: audio-contracts
- [ ] 4.1 Add `plugin/tts.rs`: `TtsProvider`, `TtsCapabilities`, `ChunkRequest`, `ChunkContext`, `ChunkAudio`, `synthesize_checked` (rejects empty/non-finite PCM, spans that overlap or exceed length, wrong span count) and `FakeTts` → accept: unit tests for each rejection.
- [ ] 4.2 Add `crates/podling-core/src/audio.rs`: `Pcm { rate, samples: Vec<f32> }`, WAV read/write via `hound`, resample via `rubato` → accept: write→read roundtrip equal; 24 kHz→48 kHz doubles length ±1.
- [ ] 4.3 Add `plugin/sidecar.rs`: load profiles from the user config path (never the episode), `Command` with argv only, `--run-dir` and port passed, `/health` wait with timeout and protocol check → accept: a profile missing from the file is a `Config` error naming the file; no code path builds a shell string.
- [ ] 4.4 Kill and reap the child in `Drop`; log spawn/ready/exit spans with pid and elapsed_ms; map startup failure / OOM text to a hint naming `ollama stop <model>` and the profile → accept: test with a fake sidecar binary shows the process is gone after drop.
- [ ] 4.5 Add `plugin/sidecar_tts.rs`: `SidecarTts` over `plugin/http.rs`, fingerprint = protocol + backend + model + weights hash from `/health` → accept: integration test against a stub server (like `tests/openai_provider.rs`).
- [ ] 4.6 Add a content-addressed `BlobStore` beside `DiskCache` in `cache.rs` (`<cache>/blobs/<2hex>/<blake3>.wav`, temp + rename) and teach `cache stats`/`cache clear` its shape → accept: clear removes blobs and still leaves foreign files with a warning.
- [ ] 4.7 Add `build_tts` to `plugin/mod.rs` → accept: factory test builds `FakeTts` from an episode.
- [ ] 4.8 Dependency audit for `hound`, `rubato` → accept: both MIT/Apache; recorded in the unit's review.

### Step 5 — audio-e2e (., rust, normal) — first listenable episode
Tooling: implementer · gates code-reviewer, idiom-reviewer, performance-reviewer, dependency-auditor · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: tts-sidecar, tts-provider
- [ ] 5.1 Add `stages/synthesize.rs`: one chunk per turn (planner comes in 7), a `SynthesizeChunk` stage run through `cached()` per chunk with derived seeds, audio in the `BlobStore`, pinned voices → accept: second run with a warm cache makes zero TTS calls; deleting a blob makes exactly that chunk re-synthesise.
- [ ] 5.2 Make `WriteScript` honour a declared `[[cast]]` (prompt lists it; a draft with another speaker is rejected and retried); bump `PROMPT_VERSION` and the stage `VERSION` → accept: test with `FakeLlm` returning an unknown speaker fails after 2 attempts.
- [ ] 5.3 Add `release()` (default no-op) to `LlmProvider` and `EmbeddingProvider` (both models live in Ollama). With `unload_after = true` in `[llm]`/`[embedding]`, `OpenAiCompat`/`OpenAiEmbeddings` POST Ollama's native `{base without /v1}/api/generate` with `keep_alive: 0`. The pipeline calls both before building the TTS provider → accept: stub-server test sees one unload request per flagged provider; without the flag, none.
- [ ] 5.4 Add `stages/assemble.rs` (minimal): trim leading/trailing silence, fixed 300 ms gap, resample to 48 kHz, normalise to −16 LUFS with `ebur128`, write `episode.wav`; per-chunk synth span logs seconds and RTF → accept: on the fake episode the measured integrated loudness is −16 ± 0.5 LUFS.
- [ ] 5.5 Wire the stages into `pipeline.rs` after `analyse` when `[tts]` is set; write `audio.json`; CLI prints the episode path → accept: CLI test shows the path line.
- [ ] 5.6 Add `[[cast]]`, `[tts] fake`, `[asr] fake` to `examples/tunguska/episode.toml` → accept: `podling run` on it writes `episode.wav` offline.
- [ ] 5.7 Test: an episode without `[tts]` produces the same artifacts as before (bar schema version) → accept: test passes.

### Step 6 — script-beats (., rust, normal)
Tooling: implementer · gates code-reviewer, api-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: audio-e2e
- [ ] 6.1 Add `Beat`, `BeatKind`, `Pace`, `Nonverbal`, `NonverbalAt` and `Turn.pace/nonverbal/callback_to`, `Script.beats` with validation in `Script::new` → accept: unit tests: gap in beats, overlap, forward `callback_to`, unknown `by` each rejected; old script JSON still parses (one beat per turn implied).
- [ ] 6.2 Mirror them in `ScriptDraft`/`DraftTurn` and the script prompt (beats, pacing, nonverbals, callbacks; "banter adds no new facts") → accept: `PROMPT_VERSION` bumped.
- [ ] 6.3 Carry beats through `build_script`; bump `WriteScript::VERSION` → accept: fake-LLM test produces a script with beats.
- [ ] 6.4 Bump `SCHEMA_VERSION` 4→5 and accept snapshots → accept: schema test passes.
- [ ] 6.5 Add opt-in analyser `uncited_figures` (Warning on a turn with a number/year and no citation) → accept: unit tests for a hit and a miss.
- [ ] 6.6 Pipeline test with beats on the fake episode → accept: passes.

### Step 7 — beat-chunker (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer, performance-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: audio-e2e, script-beats
- [ ] 7.1 Add `stages/plan_chunks.rs`: pack whole beats into chunks of 60–`max_chunk_secs` s using a words-per-minute estimate per speaker; per-turn backends get one turn per chunk → accept: unit tests: no beat split; every turn in exactly one chunk.
- [ ] 7.2 Split a beat longer than the limit only at a sentence end inside its longest turn → accept: test with a 200 s beat.
- [ ] 7.3 Add context: the previous beat (text + its cached blob) as `ChunkContext`; callbacks add the referenced turn's cached clip → accept: test that context changes the cache key and never appears in output length.
- [ ] 7.4 Extend the sidecar protocol/adapters for context and callback clips (protocol v1 fields already reserved) → accept: sidecar tests pass.
- [ ] 7.5 `synthesize` consumes the plan; per-chunk cache key includes context hashes → accept: editing one turn re-synthesises exactly one chunk (and the next chunk only if its context changed).
- [ ] 7.6 Pipeline test on a fake 30-minute script → accept: chunk count and cache behaviour as expected; run time with `FakeTts` < 10 s.

### Step 8 — asr-verify (., rust, normal)
Tooling: implementer · gates code-reviewer, dependency-auditor, performance-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: audio-e2e
- [ ] 8.1 Add `plugin/asr.rs`: `AsrProvider`, `Transcript`, `transcribe_checked`, `FakeAsr` (returns the request's text, or a scripted miss for tests) → accept: unit tests.
- [ ] 8.2 Add `plugin/whisper.rs`: `CandleWhisper` (candle-transformers Whisper, CPU, safetensors only, weights BLAKE3 in fingerprint, loaded on first use) → accept: parity test against a reference transcript fixture, gated like `cross_encoder_parity.rs`.
- [ ] 8.3 Add `stages/verify_audio.rs` text normalisation (case, punctuation, numbers ↔ words) and WER → accept: unit tests incl. "1908" vs "nineteen oh eight".
- [ ] 8.4 Verify each chunk right after synthesis; fill missing `turn_spans` from segment timestamps snapped to silence → accept: dialogue-backend fake without spans gets spans.
- [ ] 8.5 Regenerate failures with a new seed up to `max_retries`; banter beats get `takes`, best passing take wins; log WER, misses, retries, take; still failing → `Error` finding + `verified: false` → accept: tests for pass, retry-then-pass, give-up.
- [ ] 8.6 Dependency/weights licence check (Whisper weights MIT) → accept: recorded.
- [ ] 8.7 Quote check: a quote missing from the transcript fails the chunk even when WER passes → accept: test.
- [ ] 8.8 Pipeline test with `FakeAsr` scripted to fail once → accept: one regeneration, manifest shows take 2.

### Step 9 — full-assembler (., rust, normal)
Tooling: implementer · gates code-reviewer, security-auditor, performance-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: beat-chunker, asr-verify
- [ ] 9.1 Gaps from `Pace` (Quick 120 ms, Normal 300 ms, Beat 600 ms, LongPause 1000 ms; configurable) using turn spans → accept: unit test measures gaps ±10 ms.
- [ ] 9.2 `Interrupt`: start the turn 150 ms early with an equal-power crossfade → accept: test of overlap length and no clipping.
- [ ] 9.3 Second track: `Nonverbal::Over` clips mixed at their offset, ducked −6 dB → accept: test that the main track is unchanged outside the overlay.
- [ ] 9.4 Per-chunk loudness match (to the median chunk) before episode normalisation → accept: chunk loudness spread ≤ 1 LU on the fake episode.
- [ ] 9.5 Episode normalisation to −16 LUFS integrated / −1 dBTP true peak (limiter on the peak only); values logged and in the manifest → accept: measured values within ±0.5 LU / ≤ −1.0 dBTP.
- [ ] 9.6 Optional `encode.rs`: `[tts] encode = "opus" | "mp3"` runs `ffmpeg` via argv (no shell), output path fixed under `--out` → accept: missing ffmpeg → readable error; test asserts argv construction.
- [ ] 9.7 Integration test `tests/assemble.rs` on synthetic PCM → accept: passes.

### Step 10 — live-and-docs (., markdown, normal)
Tooling: implementer · gates docs-curator · guards cargo test
Depends on: full-assembler
- [ ] 10.1 Add `examples/tunguska/episode-tts.toml` (Ollama + sidecar profile + CC0 voices) and `voices/README.md` with the download commands and licences (clips gitignored) → accept: file parses; README lists each clip's licence.
- [ ] 10.2 Live run: 10-minute Tunguska episode, cold cache → accept: `episode.wav` plays; all chunks verified; peak VRAM ≤ 7.0 GB; numbers recorded under "Live results".
- [ ] 10.3 Live run: 30-minute episode, then edit one turn and rerun → accept: one chunk re-synthesised; listening notes on seams and banter recorded.
- [ ] 10.4 Update `docs/architecture.md`: pipeline, TTS/ASR rows in the plugin table, sidecar protocol and lifecycle, blob cache, bump rule for the TTS fingerprint, remove "TTS and ASR provider traits" from Deferred → accept: docs-curator passes.
- [ ] 10.5 Update README config table (`[[cast]]`, `[tts]`, `[asr]`, `sidecars.toml`) → accept: every new key documented.
- [ ] 10.6 Update `docs/handoff.md` "Where things stand" and "Out of scope" → accept: Phase 5 row present.

## Acceptance criteria (consolidated for `/orchestrate`)

Plan review (root-only, plan-reviewer brief): **VERDICT revise → applied**. Changes: `TurnRange`
and `PerMille` instead of `Range`/`f32` (illegal states); struct-variant `Backchannel` (serde
tagging); derived seeds and the missing-blob-is-a-miss rule (failure modes); per-chunk `cached()`
through `SynthesizeChunk` with a separate `BlobStore` (consistency with `stage.rs`); dropped
`nonverbal_tags` (abstraction budget); explicit error model; build-late/drop-early TTS provider
(resource lifecycle).

These are not yet in `.craftsman/acceptance.md`: the stop gate enforces that file for the session
that writes it, so `/orchestrate` loads these together with the per-task `accept:` checks above.

- [ ] An episode without `[tts]` produces the same artifact bodies as before Phase 5 (only `schema_version` differs).
- [ ] Rerunning an unchanged episode with a warm cache makes zero TTS and zero ASR calls; editing one turn re-synthesises only the chunks whose text or context changed.
- [ ] No chunk boundary falls inside a beat, and every turn is in exactly one chunk.
- [ ] Once the synthesize stage returns, no sidecar process is left running (checked by pid), and no episode-file field can cause a process to start.
- [ ] Every quote in the script appears verbatim (normalised) in its chunk's transcript, or the analysis report has an `Error` finding for that chunk.
- [ ] `episode.wav` measures −16 ± 0.5 LUFS integrated and ≤ −1.0 dBTP true peak.
- [ ] A live 10-minute Tunguska episode peaks at ≤ 7.0 GB VRAM, with the LLM unloaded first.

## Sequencing
1 (spike) decides the backend before any trait is frozen. 2 (types) and then 3 ∥ 4 (Python sidecar
and Rust provider meet at the protocol). 5 is the **first listenable episode** — judge seams and
voices against real audio before building quality layers on guesses. Then 6 → 7 (beats feed the
chunker; both touch the script stage, so not parallel) and 8 in parallel with 6–7 (depends only on 5).
9 needs spans from 8 and beats from 7. 10 last.

## Decomposition
`plan-strategist` compared three decompositions: by layer (types → traits → stages → script),
thin vertical slice then quality layers, and by backend (separate dialogue and per-turn paths).
**Chosen: vertical slice.** Layering puts the riskiest unknowns (cross-chunk drift, seam
audibility, 8 GB fit) last and fixes the trait before the spike can shape it; per-backend paths
double the surface for a choice the spike settles. A single `ChunkRequest` covers both backend
types (per-turn = one turn per chunk). Its other calls adopted here: the sidecar is killed to
free VRAM, audio never goes in JSON, and the assembler is pure Rust with ffmpeg only for encoding.

## Verification background   (citations — for the reviewer, not the executor)
- Python only as model-worker sidecars; Dia2 / MOSS-TTSD named — `.claude/CLAUDE.md` "Language".
- One model at a time, unloaded between stages; 8 GB — `.claude/CLAUDE.md` "Hardware target".
- No NC / AGPL / revenue-capped weights — `.claude/CLAUDE.md` "Licensing".
- TTS/ASR traits deferred — `docs/architecture.md:163`; tokio reserved for parallel TTS — `docs/architecture.md:21-23` (this plan stays synchronous: one GPU model, sequential chunks).
- Plugin pattern (config enum + factory arm, `Name {}` variants) — `crates/podling-core/src/plugin/mod.rs:1-5`, `crates/podling-types/src/episode.rs:53-55`.
- Cache is JSON-only, `<2hex>/<key>.json` — `crates/podling-core/src/cache.rs` (`path_for`), `docs/architecture.md:88-115`.
- Five bump rules — `docs/architecture.md:117-140`.
- The LLM chooses the cast today — `crates/podling-core/src/plugin/llm.rs:111-115`.
- `Emotion` already exists as a TTS hint — `crates/podling-types/src/script.rs:19-30`.
- Models dropped before the next stage — `crates/podling-core/src/pipeline.rs` (`grounding` moved into its block).
- Phase 3 put NLI on the CPU to keep the GPU for Ollama, so the LLM is still resident when TTS starts — `docs/plans/phase3-nli-ledger.md` "NLI on CPU".

CONSUMERS:
- `Script` / `Turn` / `Speaker` (`crates/podling-types/src/script.rs`): `crates/podling-core/src/stages/script.rs:5,129-164`, `crates/podling-core/src/plugin/analyser.rs:3,10,36,88-110`, `crates/podling-core/tests/pipeline.rs:11,355,457`, `crates/podling-types/tests/roundtrip.rs:64-92`, `crates/podling-types/src/schema.rs:12,33`, `crates/podling-types/src/lib.rs:28`.
- `ScriptDraft` / `DraftTurn` (`crates/podling-core/src/plugin/llm.rs:111-130`): `crates/podling-core/src/stages/script.rs`, `crates/podling-core/src/plugin/mod.rs` (re-export), `crates/podling-core/tests/pipeline.rs:286,407`.
- `EpisodeSpec` (`crates/podling-types/src/episode.rs:12`): `crates/podling-core/src/pipeline.rs`, `crates/podling-core/src/plugin/mod.rs` (factories + tests), `crates/podling-cli/src/commands.rs:22,54,101`, all `episode.toml` fixtures and examples.
- `ArtifactKind` (`crates/podling-types/src/envelope.rs:16-46`): `crates/podling-types/src/schema.rs:33`, `crates/podling-core/src/pipeline.rs` (`write`), CLI `schema export`.
- `LlmProvider` (gains `release`, default no-op): `FakeLlm`, `OpenAiCompat`, test doubles in `crates/podling-core/tests/pipeline.rs`.
- `DiskCache` (gains blob store): `crates/podling-core/src/stage.rs` (`cached`), `crates/podling-cli/src/commands.rs:120,133`.
- Sidecar protocol v1 (process boundary): `sidecars/tts/podling_tts/protocol.py` ↔ `crates/podling-core/src/plugin/sidecar_tts.rs`; versioned via `/health`.

## Risk & rollback
- **No backend fits 8 GB with acceptable quality.** The spike gates this first; the fallback is the per-turn path (Qwen3-TTS 0.6B or MOSS-TTS Q4 GGUF, ≤ 5 GB) with the same trait.
- **Ollama keeps the LLM resident.** 5.3 unloads it; 4.4 turns an OOM into a hint naming `ollama stop`.
- **Dialogue backends give no turn spans.** ASR timestamps fill them (8.4); until then the assembler treats a chunk as one block.
- **CPU Whisper is slow for 60 min of audio.** The spike measures `base.en` vs `small.en`; verification can be set to quotes-and-banter-only if needed (config, not a code change).
- **Voice clip licences.** Required `licence` field, copied to the manifest; example uses CC0 voices only.
- **Process spawning.** Profiles only from user-level config, argv only, loopback only, killed on Drop.
- Rollback: `git revert`. Without `[tts]` the pipeline is today's plus the schema bumps; `sidecars/` has no Rust dependency.

## Out of scope
- Streaming / real-time playback, and tokio (sequential chunks on one GPU gain nothing from it).
- The generic OpenAI `/v1/audio/speech` backend (no reference clips or context in that protocol) — a later per-turn adapter.
- Music beds, intros/outros, chapter markers, multi-language episodes, a separate quote-reader voice.
- Fine-tuning voices; the Contested-claim adjudicator (separate phase).
