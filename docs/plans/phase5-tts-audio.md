---
slug: phase5-tts-audio
goal: "`podling run` turns the script into one listenable, broadcast-loud episode file (30–60 min) with consistent voices, natural banter timing and every verbatim quote checked by ear-equivalent ASR, on an 8 GB GPU with one model loaded at a time."
classification: in-scope   # .claude/CLAUDE.md "Modularity" (TTS + ASR providers), "Hardware target"; docs/architecture.md "Deferred to later phases: TTS and ASR provider traits"
tracker_rows: [TRACKER#phase5-tts-audio/1, TRACKER#phase5-tts-audio/2, TRACKER#phase5-tts-audio/3, TRACKER#phase5-tts-audio/4, TRACKER#phase5-tts-audio/5, TRACKER#phase5-tts-audio/6, TRACKER#phase5-tts-audio/7, TRACKER#phase5-tts-audio/8, TRACKER#phase5-tts-audio/9, TRACKER#phase5-tts-audio/10]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: "skipped(root-only agent mode: agent-mode-guard blocks non-strategist Task; the plan-strategist pass supplied the independent decomposition, see \"Decomposition\")"
coverage:
  contract:      "2.1–2.6 (EpisodeSpec cast/tts/asr, AudioManifest, ArtifactKind::Audio, SCHEMA_VERSION), 4.1 (TtsProvider + ChunkRequest), 5.2 (WriteScript honours a declared cast), 5.3 (LlmProvider::release), 6.1–6.4 (Beat/Pace/Nonverbal/callback_to on Script + ScriptDraft, PROMPT_VERSION), 8.1 (AsrProvider), 3.2 (sidecar HTTP protocol, versioned)"
  data:          "4.5–4.6 (content-addressed audio blob store beside the JSON cache; `cache clear`/`stats` learn its shape). N/A for databases: none exist"
  config:        "2.1–2.3 ([[cast]] with pinned voices, [tts], [asr] episode sections), 4.3 (user-level sidecar profiles, never in the episode file), 5.3 ([llm] unload_after for Ollama), 10.1 (examples/tunguska/episode-tts.toml)"
  security:      "4.3–4.4 (sidecar program comes from user config, argv only, no shell, 127.0.0.1 only, killed on Drop), 3.3 (sidecar binds loopback, validates paths under a run dir), 4.2 (sidecar responses size-capped and validated), 2.2 (voice licence recorded per clip), 1.4 + 3.5 + 8.6 (dependency and weight licence audit)"
  tests:         "2.6, 3.4, 4.7, 4.8, 5.6, 5.7, 6.5, 6.6, 7.5, 7.6, 8.7, 8.8, 9.6, 9.7, 10.2–10.3"
  observability: "4.4 (sidecar spawn/ready/exit spans with pid and elapsed_ms), 5.4 (per-chunk synth span: seconds of audio, RTF), 8.5 (per-chunk verify log: WER, quote misses, retries, take chosen), 9.5 (episode LUFS / true peak / duration logged and in the manifest)"
  interface:     "5.5 (CLI prints the episode audio path; readable Config error naming a missing voice or sidecar profile), 4.4 (OOM / sidecar-not-ready hint naming `ollama stop` and the profile file)"
  docs:          "10.4 (architecture.md), 10.5 (README), 10.6 (handoff.md), 3.6 (sidecars/tts/README.md)"
  rollback:      "git revert of the branch. Without [tts] the pipeline runs today's stages and writes today's artifacts plus the schema_version bump (tests 5.7, 6.6); the sidecar is a separate directory with no Rust dependency on it"
units:
  - id: 1
    scope_id: tts-bakeoff
    project: .
    depends_on: []
    module: scripts/tts_bakeoff
    language: python
    security: "normal"
    scope:
      read: [examples/tunguska/episode-ollama.toml, crates/podling-types/src/script.rs]
      docs: [.claude/CLAUDE.md, docs/plans/phase5-tts-audio.md]
      write:
        - scripts/tts_bakeoff/pyproject.toml
        - scripts/tts_bakeoff/bakeoff.py
        - scripts/tts_bakeoff/README.md
        - scripts/tts_bakeoff/.gitignore
        - scripts/tts_bakeoff/uv.lock              # scope correction: `uv sync` writes it
        - scripts/tts_bakeoff/fixtures/            # scope correction: the 96-word example sources give a <1 min script
        - docs/plans/phase5-tts-audio.md
    tooling: { implementer: implementer, gates: [dependency-auditor], skills: [], guards: [], mcp: [] }
  - id: 2
    scope_id: audio-contracts
    project: .
    depends_on: [1]
    module: crates/podling-types
    language: rust
    security: "normal"
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
        # widened in execution: the [tts] companion check sits next to build_grounding,
        # and the core/CLI tests enumerate ArtifactKind
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-cli/tests/cli.rs
    tooling: { implementer: implementer, gates: [code-reviewer, api-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: tts-sidecar
    project: .
    depends_on: [1, 2]
    module: sidecars/tts
    language: python
    security: "high"
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
    security: "high"
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
    security: "normal"
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
    security: "normal"
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
    security: "normal"
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
    security: "normal"
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
    security: "normal"
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
    security: "normal"
    scope:
      read: [examples/tunguska/episode-ollama.toml, crates/podling-types/src/episode.rs, sidecars/tts/README.md, crates/podling-types/src/script.rs]
      docs: [docs/architecture.md, README.md, docs/handoff.md, docs/plans/phase5-tts-audio.md]
      write:
        # 10.7 (live-run fix): beats marked on turns, derived by the stage
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/tests/pipeline.rs
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
    pub max_chunk_secs: u32,     // e.g. Qwen3-TTS 120, Dia2 90 (see Spike results)
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
  model's own syntax (text tags for Dia2/MOSS-TTSD; Qwen3-TTS voice clones take no style
  instruction, see Spike results) **in the adapter**, so the Rust side never learns model-specific
  tags.

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

## Spike results (unit 1, 2026-10-05)

**Decision: Qwen3-TTS 1.7B Base is the default backend (per-turn), Dia2-1B the fallback (dialogue);
Whisper `base.en` verifies.** MOSS-TTSD does not fit the card.

Measured by `scripts/tts_bakeoff` on `fixtures/tunguska-10min.json` (51 turns, two hosts, 8 quotes),
RTX 5060 8 GB, voices LibriTTS-R 4446/1089 (CC-BY-4.0). Ollama had no model on the GPU in any run
(`ollama_at_start`/`_end` in each `results.json`). The CPU was shared with another job (load
average 18–51), so CPU timings are pessimistic; GPU timings are not affected.

| | Qwen3-TTS 1.7B Base | Dia2-1B | MOSS-TTSD v1.0 (8B, NF4) |
|---|---|---|---|
| Fits 8 GB | yes | yes | **no**: ~5 GB resident + a 1.2 GiB bf16 matrix mid-load exceeds what the desktop (~1.2 GB) leaves; its 1.77B fp32 audio tokenizer (~7 GB) has to run on the CPU |
| Peak VRAM, own process (device incl. desktop) | 5,824 MiB (7,417) | 5,768 MiB (7,278) | — |
| RTF (synth s / audio s) | **0.63** | 1.28 | — |
| WER `base.en` / `small.en` (mean) | **0.009** / 0.027 | 0.032 / 0.024 | — |
| Quotes heard `base.en` / `small.en` | 7/8 / 7/8 | 7/7 / 6/7 | — |
| Speaker similarity (ECAPA) mean / min | **0.76 / 0.43** | 0.54 / 0.06 | — |
| CPU RTF of Whisper `base.en` / `small.en` (loaded CPU) | 1.0 / 2.0 | 1.6 / 3.5 | — |
| Chunks (cap) | 5 (120 s) | 6 (90 s) | 0 |
| Licences | weights + tokenizer Apache-2.0; `qwen-tts` Apache-2.0 | weights Apache-2.0, Mimi CC-BY-4.0 | Apache-2.0 |

Every remaining quote miss is a Whisper deletion, not a speech error: the other Whisper size heard
the same audio correctly in each case.

**Why Qwen.** It is twice as fast, has the lowest WER and the best voice match, and it synthesises one
turn per call, so turn spans are exact and the two voices cannot be swapped. Dia2 swapped them for a
whole chunk: in chunk 4 every turn matched the *other* host (e.g. 0.05 vs 0.50). What Qwen gives up
is cross-turn prosody, nonverbal tags, and style control: `generate_voice_clone` takes no
instruction, so emotion comes from the wording alone. Whether per-turn banter sounds natural is the
open question for the listening pass (1.4).

**Why `base.en`.** After the scorer fix below, it is as accurate as `small.en` (0.009–0.032 vs
0.024–0.027 mean WER) at half the CPU time.

**Design changes, and the units that follow them:**
1. **Per-turn default.** `qwen` reports `multi_speaker: false`, `max_chunk_secs: 120`. The planner
   sends one turn per chunk (7.1 already says so), so turn spans are exact and 8.4's ASR spans apply
   only to the fallback. Unit 3.4 implements the Qwen adapter. The Dia2 adapter is the documented
   fallback, built only if the listening pass rejects per-turn banter.
2. **Emotion and nonverbals under Qwen.** The adapter renders `Backchannel { text }` as its own short
   call (for 9.3's second track). It drops `Laugh`/`Chuckle`/`Sigh` and emotion, which it cannot
   express, and lists them in its response so the manifest can say so. There is no style instruction
   (this corrects "a style instruction for Qwen3-TTS" above).
3. **Dia2, if built.** Its chunk cap is 90 s, not 120, because the 2-minute context also holds both
   voice prefixes. Mimi decodes on the CPU: a GPU decode beside the generation cache overflowed 8 GB.
   Prefix word timings are computed once per voice and stored, never with `whisper-timestamped`
   (AGPL-3.0). Each turn is checked against both reference voices to catch a swap, and a swapped
   chunk is regenerated.
4. **Whisper must guard against loops (8.2).** The transformers *chunked* long-form pipeline looped
   on clean audio ("The size of the graphs." ×5), scoring good chunks at WER 0.33–0.89. Sequential
   30 s windows with Whisper's temperature fallback fixed it (0.885 → 0.005). That fallback retries a
   window that is too repetitive (compression ratio > 1.35) or too unlikely (mean log-prob < −1.0),
   with no previous-text prompt. `CandleWhisper` must do the same, or a verifier would regenerate
   good chunks.
5. **"Peak VRAM ≤ 7.0 GB" means the Podling-owned processes** (sidecar), measured per process. The
   desktop holds ~1.2 GB of the 7.6 GB card, which is why MOSS fails. 4.4's OOM hint should also
   mention the desktop.

## Live results (unit 10, 2026-10-05)

`examples/tunguska/episode-tts.toml` with llama3.1:8b, `nomic-embed-text`, NLI on the CPU, Qwen3-TTS
1.7B Base in the sidecar, Whisper `base.en` on the CPU, voices LibriTTS-R 4446/1089 (CC-BY-4.0).
RTX 5060, 8,151 MiB as the driver reports it. VRAM was sampled once a second per process with
`nvidia-smi`.

**Where Ollama ran.** The docker Ollama container has no GPU, and its VM was swapping (0.36 tokens/s):
the script stage timed out after 1,800 s. The runs below use a native Ollama 0.35.1 on the GPU
(29 tokens/s, runner process 5,410 MiB). That is the stricter test of the VRAM rule, because the LLM
does compete with the sidecar for the card. Only the base URL differed from the committed example.

**10.2: 10-minute episode, cold cache.** Exit 0 in 247 s; the script stage took 108 s (one attempt
rejected for citing a claim not in the ledger).

| | |
|---|---|
| Script | 18 turns, 216 words, 18 beats (narration and banter alternating), 8 `quick` paces, 0 quotes |
| LLM unload → sidecar | llama unloaded, then the sidecar spawned (ready in 232 ms) and exited with 143 (SIGTERM) after 122.9 s |
| Peak VRAM, sidecar process | **5,120 MiB** (device 6,389 of 8,151 during TTS; the llama runner was absent throughout) |
| Peak VRAM, device, whole run | 6,888 MiB, during the LLM stage, with no sidecar running |
| Verification | 18/18 chunks verified, mean WER 3.1 ‰, 0 quote misses; 6 chunks used a take after 0 (one passed on take 1 after 100 ‰, one on take 2 after 91 ‰ twice) |
| `episode.wav` | 82.3 s, −16.17 LUFS, −1.45 dBTP, PCM s16le 48 kHz mono; `episode.opus` decodes (82.3 s) |
| Findings | info (quote verifier), warning (`uncited_figures`: "1908") |

**10.3: 30-minute episode, then one edited turn.** The first try was cut off at the 6,000-token
output cap while the model looped. The second (cold for the script) ran in 137 s and wrote 10 turns,
167 words and 2 quotes: 10 chunks, 9 verified, `episode.wav` 60.6 s at −16.0 LUFS and −1.00 dBTP,
sidecar peak 5,356 MiB (device 6,640 during TTS, 6,694 overall during the LLM stage). It exited 1 by
design: one `Error` finding, because Whisper heard "Leonid Kulik reached the site in 1927." as
"Leonid **Koolik**…" on every take (best 72 ‰). That is a proper noun spelt by ear, not a speech error;
a per-episode vocabulary for the scorer would fix it, and is not built.

Editing turn 2 of the cached script ("What else do we have?" → "What else is there?") and rerunning
took 12 s. Every stage up to the script hit the cache; `synthesize_chunk` had 2 misses and 16 hits,
and `transcribe_chunk` the same. The 2 misses are the two takes of the one edited chunk (2..3), so
exactly one chunk was re-synthesised; the sidecar ran for 11.5 s.

**What a "30-minute" target gives.** About 1 minute. The script length is capped by the grounded
ledger (9 usable claims from the Tunguska sources), not by `target_minutes`. Audio rule 9 now forbids
repeating a point, which is what the model did when asked to fill 30 minutes. Longer episodes need
more sources, not a longer target.

**Listening (the user, 2026-10-06).** Closes the listening part of 1.4 and 10.3. Copies of the
files are in `~/podling-listening/` (outside the repository).
- *Bake-off (1.4):* Qwen and Dia2 sound very similar, near identical. Dia2 is slightly better, but
  the difference is practically unnoticeable. **Decision: keep per-turn Qwen; the Dia2 dialogue
  adapter (change 3) is not built.**
- *Live episodes (10.3):* both are fine: the seams, turn-taking and loudness raised no complaint.
- *Two problems, for a follow-up:*
  - **Serious mispronunciations.** Which words are not yet recorded; proper nouns and Russian
    names ("Kulik", "Tunguska") are the likely ones, matching the ASR's "Koolik".
  - **Flat delivery.** The voices are not bad but lack personality in how they speak. One cause is
    known: the 1.7B Base model is voice-clone only, and the adapter drops each turn's `emotion`
    (`sidecars/tts/podling_tts/backends/qwen.py:160-161`), so the script's emotions never reach
    the model.

## Scope Steps (executable core)

### Step 1 — tts-bakeoff (., python, normal)
Tooling: implementer · gates dependency-auditor
Depends on: none
- [x] 1.1 Write `scripts/tts_bakeoff/pyproject.toml` (uv; Python 3.11) with only commercial-safe deps; record each dep's licence in README → accept: `uv sync` succeeds; README table lists every direct dep + licence; none is AGPL/NC.
- [x] 1.2 Write `scripts/tts_bakeoff/bakeoff.py`: reads a Podling `script.json`, packs turns into ≤120 s chunks, runs one backend per process (MOSS-TTSD NF4, Dia2-1B, Qwen3-TTS-1.7B), pinned reference clips, writes chunk WAVs + `results.json` → accept: `python bakeoff.py --backend <b> --script <path>` writes ≥5 chunk WAVs for each backend on the Tunguska script. *Done for Qwen (5) and Dia2 (6). MOSS-TTSD cannot load on the 8 GB card after four fixes; that failure is its recorded result (Spike results). The script is the hand-written `fixtures/tunguska-10min.json`, because the example sources are too short for a 10-minute grounded script.*
- [x] 1.3 Measure per backend: peak VRAM (`torch.cuda.max_memory_allocated` and `nvidia-smi` polling), RTF, speaker similarity of each chunk to its reference (ECAPA cosine, Apache-2.0 model), WER and quote hits with Whisper `base.en` and `small.en` on CPU (incl. CPU RTF) → accept: `results.json` has every metric for every backend; Ollama model unloaded during runs (recorded). *Every backend that produced audio; Ollama's GPU usage was 0 throughout (`ollama_at_start`/`_end`).*
- [x] 1.4 Listening pass: rate seams (1–5) and banter timing on chunks 1, 3, 5; confirm weight licences on the model cards → accept: notes recorded per backend. *Licences confirmed (README table). The listening needs a human: `scripts/tts_bakeoff/out/{qwen,dia2}/chunk_00{1,3}.wav` plus `qwen/chunk_004.wav` and `dia2/chunk_005.wav`. In particular, does per-turn Qwen banter sound natural? If not, the Dia2 fallback (change 3) gets built.* **Done 2026-10-06:** Qwen and Dia2 near identical (Dia2 marginally better); Qwen kept, no Dia2 adapter. See "Live results".
- [x] 1.5 Record "Spike results" in this plan: the default backend (must fit ≤ 7.0 GB peak, RTF ≤ 2.0, licence clean), a fallback, Whisper size, and any change to the design above → accept: section exists with numbers and a one-line decision; downstream units' text updated if the decision changes them.

### Step 2 — audio-contracts (., rust, normal)
Tooling: implementer · gates code-reviewer, api-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: tts-bakeoff
- [x] 2.1 Add `EpisodeSpec.cast: Vec<CastMember { id, name, role, voice: VoiceRef }>` (`#[serde(default)]`) → accept: an episode without `[[cast]]` parses unchanged; duplicate ids rejected. *(`episode_without_audio_sections_is_unchanged`, `cast_ids_are_unique`; absent sections are not serialised, so `episode.json` keeps its shape.)*
- [x] 2.2 Add `VoiceRef { reference: PathBuf, transcript: String, licence: String }` with `deny_unknown_fields`; empty licence rejected → accept: unit test for the empty-licence error. *(`a_voice_needs_a_licence_and_a_transcript`; an empty transcript is rejected too, since cloning needs it.)*
- [x] 2.3 Add `TtsConfig { Fake {}, Sidecar { sidecar: String, takes: u8, max_retries: u8 } }` and `AsrConfig { Fake {}, Whisper { model_dir: PathBuf, max_wer_pm: PerMille } }` (`PerMille` from Phase 3 keeps the config `Eq` and rejects > 1000); `[tts]` requires `[[cast]]` and `[asr]` (checked at build, a `Config` error) → accept: tests for each missing-companion error message. *(`plugin::check_audio`, called by the pipeline next to `build_grounding` before any stage; also rejects `[asr]` without `[tts]` and `takes = 0`. Test `audio_sections_come_together`. Scope widened to `plugin/mod.rs` and `pipeline.rs` for this.)*
- [x] 2.4 Add `crates/podling-types/src/audio.rs` with `TurnRange` (validated `try_from`), `AudioManifest { sample_rate, chunks: Vec<ChunkRecord>, episode: EpisodeAudio { path, duration_ms, integrated_lufs, true_peak_dbtp }, voices: Vec<VoiceCredit> }`, `ChunkRecord { id, turns: TurnRange, blob: ContentHash, seed, take, wer_pm: PerMille, quote_misses, verified }` → accept: roundtrip test. *(`artifacts_roundtrip`, `turn_ranges_are_never_empty`. Loudness is `f64`, so the manifest is `PartialEq` only; it is an output, never a cache key.)*
- [x] 2.5 Add `ArtifactKind::Audio` (`audio.json`), register in `schema.rs`, bump `SCHEMA_VERSION` 3→4 → accept: `ArtifactKind::ALL` has 8 entries; schema export writes `audio.schema.json`. *(`schema_version_is_pinned`; CLI `schema_export_writes_one_parseable_file_per_kind` lists `audio`.)*
- [x] 2.6 Accept the new insta snapshots and refresh golden fixtures → accept: `cargo test -p podling-types` and `cargo test -p podling-core` pass. *(Golden fixtures need no refresh: the test already normalises `schema_version`. It now also asserts no `audio.json` is written without `[tts]`; the core tests also gained `audio.json` exclusion and the CLI test, so scope widened to `crates/podling-core/tests/pipeline.rs` and `crates/podling-cli/tests/cli.rs`.)*

### Step 3 — tts-sidecar (., python, high)
Tooling: implementer · gates code-reviewer, security-auditor, dependency-auditor
Depends on: tts-bakeoff, audio-contracts
- [x] 3.1 Create `sidecars/tts` (uv project, `podling_tts` package) with a stdlib-or-minimal HTTP server → accept: `uv run podling-tts --port 0 --backend fake` answers `/health`. *(stdlib `http.server`, no runtime deps for `fake`; `podling-tts` prints one `{"listening": "127.0.0.1:<port>", "protocol": 1}` line on stdout; `test_the_command_prints_where_it_listens_and_answers_health`.)*
- [x] 3.2 Implement protocol v1 in `protocol.py` (`/health`, `/synthesize`, `/unload`), request validated with explicit types; protocol version in `/health` → accept: malformed request → 400 with a reason; unknown field → 400. *(`protocol.py` parses every body field by field; `test_malformed_requests_are_400_with_a_reason` (14 cases incl. unknown top-level and nested fields), `test_missing_field_is_400`, `test_a_non_json_body_is_400`. Also 403 on a foreign `Host` (DNS rebinding), 415 on non-JSON content type (forces a CORS preflight), 413 over 1 MiB, 503 for backend errors such as low VRAM.)*
- [x] 3.3 Bind to `127.0.0.1` only; refuse `out_path`/reference paths outside the `--run-dir` given at start (resolve symlinks first) → accept: test with `../` and a symlink escape both → 400. *(`confine()` resolves with symlinks followed and requires the path strictly inside the resolved run dir; `test_parent_dir_escape_is_400`, `test_symlink_escape_is_400` (file and directory symlinks), `test_an_existing_output_symlink_is_refused`. Outputs are written temp + rename. The worker also exits when its parent dies, so an orphan cannot hold the GPU.)*
- [x] 3.4 Implement the spike winner's adapter in `backends/`: Qwen3-TTS 1.7B Base, per turn (`multi_speaker: false`, `max_chunk_secs: 120`). It needs pinned references, a seed and an exact turn span; renders `Backchannel` as its own call; and drops `Laugh`/`Chuckle`/`Sigh`/emotion, listing them in the response. Add a `fake` backend, and a free-VRAM check at load with a readable error (Spike results, changes 1–2) → accept: `pytest` passes for `fake`; a live call with the winner writes a WAV. *(`backends/qwen.py`: one `generate_voice_clone` call per turn with per-turn seeds, exact sample spans, backchannels as clips beside `out_path`, `dropped` lists emotion/laugh/chuckle/sigh; refuses to load below 6,200 MiB free with an `ollama stop` hint. `backends/fake.py`: sine tones. Live, 2026-10-05: two turns plus a backchannel gave an 8.3 s WAV, warm RTF 0.63, worker process 4,752 MiB, same seed gave the same samples, GPU back to desktop-only after exit. The worker never downloads weights; `weights` in `/health` is the snapshot commit.)*
- [x] 3.5 Audit deps and weights licences → accept: README licence table; no AGPL/NC/revenue-capped entries. *(README tables; full `qwen` environment scanned: no AGPL/NC/revenue-capped package. Noted: `soxr` LGPL-2.1+ (dynamic use), MPL-2.0 in `certifi`/`tqdm`/`orjson`, NVIDIA CUDA runtime wheels via torch; `torchaudio` is pinned to the cu128 index like torch, or its CUDA 13 build fails to load.)*
- [x] 3.6 Write `sidecars/tts/README.md`: install, model download, the `sidecars.toml` profile snippet → accept: steps reproduce a running sidecar on a fresh clone. *(Reproduced from a clean copy of the tracked files: `uv sync --frozen`, `uv run pytest` (36 passed), `uv run podling-tts --port 0 --backend fake` prints its listening line. Interface fixed here for unit 4: profiles are `[sidecars.<name>] program = …, args = [...]`; Podling appends `--port 0 --run-dir <dir>` and reads the listening line; voice clips are copied into the run dir, since the worker refuses paths outside it.)*

### Step 4 — tts-provider (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor, dependency-auditor, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: audio-contracts
- [x] 4.1 Add `plugin/tts.rs`: `TtsProvider`, `TtsCapabilities`, `ChunkRequest`, `ChunkContext`, `ChunkAudio`, `synthesize_checked` (rejects empty/non-finite PCM, spans that overlap or exceed length, wrong span count) and `FakeTts` → accept: unit tests for each rejection. *(Tests `empty_and_non_finite_audio_are_rejected`, `bad_spans_are_rejected` (count, empty, overlap, past the end), `a_per_turn_backend_must_return_spans`, `a_speaker_without_a_voice_is_a_config_error`, `fake_tts_is_deterministic_with_exact_spans`. `SpokenTurn { speaker, text, emotion }` for now; unit 6 adds nonverbals. `ChunkContext { turns, audio, callbacks }` names WAV files, as protocol v1 does.)*
- [x] 4.2 Add `crates/podling-core/src/audio.rs`: `Pcm { rate, samples: Vec<f32> }`, WAV read/write via `hound`, resample via `rubato` → accept: write→read roundtrip equal; 24 kHz→48 kHz doubles length ±1. *(`Pcm::to_wav`/`from_wav` (float 32 for blobs, int 16 for the episode; any channel count averaged to mono), `Pcm::resample` with rubato 5's `Fft`. Tests `float_wav_roundtrips_exactly`, `resampling_24k_to_48k_doubles_the_length` (0.01 s, 0.5 s, 3 s), `resampling_keeps_the_signal`. The real worker's float WAV, `fact` chunk and all, reads back in the round-trip test.)*
- [x] 4.3 Add `plugin/sidecar.rs`: load profiles from the user config path (never the episode), `Command` with argv only, `--run-dir` and port passed, `/health` wait with timeout and protocol check (interface as fixed in 3.6: `--port 0`, read the stdout listening line; copy voice clips into the run dir) → accept: a profile missing from the file is a `Config` error naming the file; no code path builds a shell string. *(`load_profile` reads `[sidecars.<name>] program/args` (`deny_unknown_fields`) from `default_profiles_path` (`$XDG_CONFIG_HOME`, else `~/.config`). The only `Command::new` runs the profile's program with `.arg()`s, `--port 0 --run-dir <dir>` appended. The listening line must say `127.0.0.1:<port>` and protocol 1; `/health` is then checked over HTTP. Tests `a_missing_profile_or_file_names_the_file`, `malformed_profiles_are_rejected`, `the_listening_line_must_be_loopback_and_our_protocol`, `an_episode_file_cannot_name_a_program`.)*
- [x] 4.4 Kill and reap the child in `Drop`; log spawn/ready/exit spans with pid and elapsed_ms; map startup failure / OOM text to a hint naming `ollama stop <model>` and the profile → accept: test with a fake sidecar binary shows the process is gone after drop. *(`impl Drop for Sidecar`: SIGTERM (rustix, safe API), 5 s grace, then SIGKILL; always reaped. SIGTERM first because the README profile wraps the worker in `uv run`, which forwards it (checked live: the worker was gone 0.02 s after uv got it); a SIGKILL to uv would orphan the worker. Logs `sidecar spawned`/`ready`/`exited` with pid, port, elapsed_ms and status. The worker's stderr goes to `<run-dir>/sidecar.log`, whose tail errors quote, with an `ollama stop` hint when it mentions memory. Tests against the stub `tests/fixtures/fake_sidecar.py`: `synthesises_through_the_stub_and_stops_it_on_drop` (pid gone, run dir removed), `a_worker_that_never_gets_ready_is_stopped`, `a_worker_that_ignores_sigterm_is_killed`, `a_worker_that_crashes_at_start_gets_a_gpu_hint`, `a_worker_that_dies_mid_request_is_reported_with_its_log`.)*
- [x] 4.5 Add `plugin/sidecar_tts.rs`: `SidecarTts` over `plugin/http.rs`, fingerprint = protocol + backend + model + weights hash from `/health` → accept: integration test against a stub server (like `tests/openai_provider.rs`). *(`http.rs` gained `get_json`; scope widened for it. Voices and context clips are copied into a private temp run dir under content-hash names; the output WAV is read, checked against the reply, then deleted. Fingerprint `{id, protocol, backend, model, weights}`. `tests/sidecar_tts.rs` runs against the stub and against the real `sidecars/tts` worker on its `fake` backend (`round_trip_with_the_real_worker_on_its_fake_backend`), plus `a_busy_gpu_gets_the_ollama_hint` (503) and `a_reply_that_disagrees_with_the_wav_is_invalid_output`. They need `python3` 3.11+, else they print a note and pass.)*
- [x] 4.6 Add a content-addressed `BlobStore` beside `DiskCache` in `cache.rs` (`<cache>/blobs/<2hex>/<blake3>.wav`, temp + rename) and teach `cache stats`/`cache clear` its shape → accept: clear removes blobs and still leaves foreign files with a warning. *(`get` verifies the hash: a damaged blob is a miss, and the next `put` repairs it. `DiskCache::blobs()`; `stats` counts blobs apart; `clear` empties them; CLI `cache stats` prints both. Tests `blobs_are_content_addressed`, `stats_and_clear_cover_blobs_but_never_foreign_files`.)*
- [x] 4.7 Add `build_tts` to `plugin/mod.rs` → accept: factory test builds `FakeTts` from an episode. *(`build_tts(config, profiles)`; test `builds_the_tts_provider_from_an_episode`, which also shows a missing profiles file is a `Config` error naming it.)*
- [x] 4.8 Dependency audit for `hound`, `rubato` → accept: both MIT/Apache; recorded in the unit's review. *(hound 3.5.1 Apache-2.0; rubato 5.0.1 MIT OR Apache-2.0, which pulls audioadapter, audioadapter-buffers, audioadapter-sample, rustfft, primal-check, strength_reduce, transpose, num-integer (MIT OR Apache-2.0), realfft and windowfunctions (MIT), visibility (Zlib OR MIT OR Apache-2.0) and audio-codec-algorithms (0BSD OR Apache-2.0). rustix (already in the tree through tempfile) is Apache-2.0 OR MIT. All pure Rust, no build scripts. `toml` moved from a dev- to a normal dependency of `podling-core`.)*

### Step 5 — audio-e2e (., rust, normal) — first listenable episode
Tooling: implementer · gates code-reviewer, idiom-reviewer, performance-reviewer, dependency-auditor · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: tts-sidecar, tts-provider
- [x] 5.1 Add `stages/synthesize.rs`: one chunk per turn (planner comes in 7), a `SynthesizeChunk` stage run through `cached()` per chunk with derived seeds, audio in the `BlobStore`, pinned voices → accept: second run with a warm cache makes zero TTS calls; deleting a blob makes exactly that chunk re-synthesise. *(`SynthesizeChunk` holds the provider in a `RefCell`: the stage trait's `run` takes `&self`, `synthesize` needs `&mut self`. Cache key: the chunk's spoken turns, each of its speakers' clip BLAKE3 and transcript (not the path), and the take; fingerprint = the TTS fingerprint. Seed = first 8 bytes of BLAKE3(chunk id ‖ take). A new `Stage::is_reusable` hook (default true) lets `cached()` treat an entry whose blob is missing or damaged as a miss. Tests: `stages::synthesize` `a_warm_cache_makes_no_tts_calls_and_a_lost_blob_redoes_one_chunk`; `tests/audio_e2e.rs` `a_warm_rerun_makes_no_tts_calls_and_a_lost_blob_redoes_one_chunk` (whole pipeline: the warm rerun is all hits, a deleted blob re-synthesises exactly chunk 1, and the episode comes back byte-identical); `stage::an_unusable_cached_output_is_a_miss`.)*
- [x] 5.2 Make `WriteScript` honour a declared `[[cast]]` (prompt lists it; a draft with another speaker is rejected and retried); bump `PROMPT_VERSION` and the stage `VERSION` → accept: test with `FakeLlm` returning an unknown speaker fails after 2 attempts. *(`ScriptInput.cast`, left out of the key when empty; rule 5 in the instructions; `check_declared_cast` rejects a turn by anyone else, and the declared cast replaces the model's. `WriteScript::VERSION` 8, `PROMPT_VERSION` 4, `FakeLlm` version 4 (it uses a declared cast). Tests `a_speaker_outside_the_declared_cast_fails_after_two_attempts`, `the_declared_cast_replaces_the_one_the_model_wrote`. `NO_NLI_KEYS`: only `extract_claims` and `script` moved.)*
- [x] 5.3 Add `release()` (default no-op) to `LlmProvider` and `EmbeddingProvider` (both models live in Ollama). With `unload_after = true` in `[llm]`/`[embedding]`, `OpenAiCompat`/`OpenAiEmbeddings` POST Ollama's native `{base without /v1}/api/generate` with `keep_alive: 0`. The pipeline calls both before building the TTS provider → accept: stub-server test sees one unload request per flagged provider; without the flag, none. *(Shared `plugin/ollama.rs` `OllamaUnload`. Scope widened: `podling-types` gained `unload_after` on both OpenAI-compatible configs, an optional field inside the still-unreleased schema 4; snapshot accepted. With the flag, a `base_url` not ending in `/v1` is a config error. Checked live first: `/api/generate {model, keep_alive: 0}` also unloads the embedding-only `nomic-embed-text` (`done_reason: "unload"`). The pipeline releases the embedder when grounding ends and the LLM after the script stage, both before the TTS provider is built; a failed unload is a warning. Tests `plugin::ollama` `a_flagged_provider_sends_one_unload_request`, `without_the_flag_release_sends_nothing`, `the_flag_needs_an_ollama_base_url`.)*
- [x] 5.4 Add `stages/assemble.rs` (minimal): trim leading/trailing silence, fixed 300 ms gap, resample to 48 kHz, normalise to −16 LUFS with `ebur128`, write `episode.wav`; per-chunk synth span logs seconds and RTF → accept: on the fake episode the measured integrated loudness is −16 ± 0.5 LUFS. *(`assemble`: trim at −50 dBFS, 300 ms gaps, rubato to 48 kHz, one gain to −16 LUFS, then measured again; `measure` (ebur128 integrated + true peak) is public for tests. The chunk log has seconds, elapsed_ms and RTF. `ebur128` 0.1.10 is MIT; its deps dasp_frame/dasp_sample (MIT OR Apache-2.0), bitflags 1 and smallvec (MIT/Apache); its build.rs does nothing unless the C test feature is on. Tests `joins_with_gaps_at_48_khz_and_normalises_to_the_target`, and `tests/audio_e2e.rs` `the_fake_episode_becomes_a_loudness_normalised_wav`, which measures the 16-bit `episode.wav` read back from disk.)*
- [x] 5.5 Wire the stages into `pipeline.rs` after `analyse` when `[tts]` is set; write `audio.json`; CLI prints the episode path → accept: CLI test shows the path line. *(Text artifacts are now written before any audio, so a failed synthesis still leaves the script. Voices and the sidecar profile are checked before any stage runs (`a_missing_voice_clip_fails_before_any_stage_runs`, `an_unknown_sidecar_profile_fails_before_any_stage_runs`). The provider is built just before synthesis and dropped before assembly (`no_worker_is_left_running_once_the_run_returns`: the stub's pid is gone when the run returns). `RunReport.audio`; `pipeline::run_with_sidecars` and CLI `--sidecars` (the design's flag). CLI test `second_run_of_the_example_is_all_cache_hits` checks the `episode audio:` line and exact entry and blob counts.)*
- [x] 5.6 Add `[[cast]]`, `[tts] fake`, `[asr] fake` to `examples/tunguska/episode.toml` → accept: `podling run` on it writes `episode.wav` offline. *(Two plain tones made for the example, `voices/tone-*.wav`, CC0-1.0. The CLI test runs it offline and finds `episode.wav`.)*
- [x] 5.7 Test: an episode without `[tts]` produces the same artifacts as before (bar schema version) → accept: test passes. *(The golden test `no_nli_config_writes_todays_artifacts` still compares every artifact byte for byte with files written before Phase 5; `without_tts_no_audio_is_made` adds: no audio stage, only the text artifacts written, no blobs. Noted in passing: `audio.json` marks every chunk `verified: false` with `wer_pm` 1000 until unit 8's verification fills them in.)*

### Step 6 — script-beats (., rust, normal)
Tooling: implementer · gates code-reviewer, api-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: audio-e2e
- [x] 6.1 Add `Beat`, `BeatKind`, `Pace`, `Nonverbal`, `NonverbalAt` and `Turn.pace/nonverbal/callback_to`, `Script.beats` with validation in `Script::new` → accept: unit tests: gap in beats, overlap, forward `callback_to`, unknown `by` each rejected; old script JSON still parses (one beat per turn implied). *(`Script::with_beats(cast, turns, beats)`; `Script::new` = no beats. `Script::beats() -> Cow<[Beat]>` returns the stored beats, or one `Narration` beat per turn. `Nonverbal { kind: NonverbalKind (flattened, tagged by "kind"), by, at: Before | After | Over }`. Every new field is left out of the JSON at its default, so a script without them is written byte for byte as before. Errors `BeatGap`, `BeatOverlap`, `BeatsEnd`, `ForwardCallback`, `UnknownNonverbalSpeaker`. Tests `beats_must_cover_every_turn_in_order`, `a_callback_must_point_back`, `a_nonverbal_sound_is_made_by_a_cast_member`, `an_old_script_parses_with_one_beat_per_turn_and_is_written_unchanged`, `the_new_fields_roundtrip`.)*
- [x] 6.2 Mirror them in `ScriptDraft`/`DraftTurn` and the script prompt (beats, pacing, nonverbals, callbacks; "banter adds no new facts") → accept: `PROMPT_VERSION` bumped. *(`PROMPT_VERSION` 5. The directions are a separate `AUDIO_RULES` text (rules 6–8), sent only when `ScriptInput.audio` is set (the episode has `[tts]`), with `"audio": true` in the request input. A text-only episode is asked exactly what it was before (`a_text_only_script_is_asked_for_nothing_new`), and any directions the model volunteers there are dropped unchecked (`directions_a_text_only_script_did_not_ask_for_are_dropped`). Banter turns go through the same citation and quote checks as every turn.)*
- [x] 6.3 Carry beats through `build_script`; bump `WriteScript::VERSION` → accept: fake-LLM test produces a script with beats. *(`WriteScript::VERSION` 9. `FakeLlm` version 5 writes, for an audio request, quote-reading / banter / transition beats, a quick reply with an `over` backchannel, and a long-pause sign-off calling back to turn 0. Tests `a_script_for_audio_asks_for_beats_and_the_fake_writes_them`, `beats_that_leave_a_turn_out_are_rejected_with_the_reason`. `NO_NLI_KEYS`: only `extract_claims` and `script` moved again.)*
- [x] 6.4 Bump `SCHEMA_VERSION` 4→5 and accept snapshots → accept: schema test passes. *(The `script` and `episode` snapshots changed; `schema_version_is_pinned` says 5.)*
- [x] 6.5 Add opt-in analyser `uncited_figures` (Warning on a turn with a number/year and no citation) → accept: unit tests for a hit and a miss. *(`AnalyserConfig::UncitedFigures {}`. A figure is any word with a digit, outside the turn's quotes. Tests `an_uncited_year_is_a_warning_on_its_turn`, `cited_figures_and_quoted_figures_pass`.)*
- [x] 6.6 Pipeline test with beats on the fake episode → accept: passes. *(`tests/audio_e2e.rs` `a_spoken_episode_has_beats_in_its_script`. The golden test `no_nli_config_writes_todays_artifacts` still passes unchanged.)*

### Step 7 — beat-chunker (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer, performance-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: audio-e2e, script-beats
- [x] 7.1 Add `stages/plan_chunks.rs`: pack whole beats into chunks of 60–`max_chunk_secs` s using a words-per-minute estimate per speaker; per-turn backends get one turn per chunk → accept: unit tests: no beat split; every turn in exactly one chunk. *(`plan_chunks(script, capabilities) -> Vec<PlannedChunk>`; a chunk is a list of `Piece { turn, beat, text: Range }`. Whole beats are added until the chunk reaches `TARGET_SECS` (60) or the next beat would pass `max_chunk_secs` or `max_speakers`. One rate for every speaker, `WORDS_PER_MINUTE` = 150: nothing measures a speaker's own rate before unit 8. Tests `whole_beats_are_packed_into_chunks_of_about_a_minute`, `a_per_turn_backend_gets_one_turn_per_chunk`, `too_many_voices_for_one_chunk_cut_the_beat`, each checking that every byte of speech is in exactly one piece, in order.)*
- [x] 7.2 Split a beat longer than the limit only at a sentence end inside its longest turn → accept: test with a 200 s beat. *(Changed in execution: a long beat is cut **between its turns** first (about a minute at a time), and only a turn that is itself past the limit is cut at sentence ends, into parts of about equal length, never inside one of its quotes. Cutting between turns keeps every ordinary turn whole, which unit 8's per-chunk quote check and the turn spans rely on. Tests `a_200_second_beat_is_cut_between_turns_and_its_long_turn_at_sentence_ends` (a 12 s + 160 s + 28 s beat: the 160 s turn becomes two 80 s halves), `a_cut_never_falls_inside_a_quote`.)*
- [x] 7.3 Add context: the previous beat (text + its cached blob) as `ChunkContext`; callbacks add the referenced turn's cached clip → accept: test that context changes the cache key and never appears in output length. *(Context = the previous chunk's last beat, as text and as a `Clip` of its audio (cut by the turn spans), plus a clip of each turn called back to that is neither in this chunk nor in the context. The key holds the context's **words** (`ContextSpec`), not its audio: keying on audio would chain every chunk to the one before, so one edit would re-synthesise the rest of the episode. Clips are cut and stored only when the chunk actually runs, so a warm rerun does no audio work for context. `TtsCapabilities.context` (new; `/health` field `context`, default false) says whether a backend listens; when it doesn't, no context is sent or keyed. Tests `context_changes_the_key_but_is_never_in_the_audio`, `a_callback_brings_the_earlier_turns_audio`.)*
- [x] 7.4 Extend the sidecar protocol/adapters for context and callback clips (protocol v1 fields already reserved), and pass `Before`/`After` nonverbals to the TTS as the backend's tags (`SpokenTurn` gains them; `Over` sounds are unit 9's second track) → accept: sidecar tests pass. *(`SpokenTurn.nonverbal` carries the speaker's own `Before`/`After` sounds, on the piece that starts or ends the turn; `Over` sounds and sounds by someone else are left to unit 9. The wire turn sends them in `podling_types::Nonverbal`'s own JSON shape, which is already protocol v1's. Workers report `capabilities.context` (both backends: false) and list an ignored context as `{"kind": "context"}` in `dropped`; a per-turn backend says the speaker's own before/after backchannel in line, as words, and still drops laughs, chuckles and sighs. Protocol stays v1 (the new `/health` field is optional). 37 pytest cases pass, including `test_the_speakers_own_backchannel_before_or_after_is_said_in_line`; Rust `neutral_emotion_and_no_sounds_are_not_sent`, `a_worker_that_does_not_mention_context_does_not_get_it`.)*
- [x] 7.5 `synthesize` consumes the plan; per-chunk cache key includes context hashes → accept: editing one turn re-synthesises exactly one chunk (and the next chunk only if its context changed). *(`synthesize_script` runs `plan_chunks`; `ChunkSpec` gains `context: Option<ContextSpec>`; `SynthesizeChunk::VERSION` 2. `FakeTts::default()` now behaves like Qwen (per-turn, no context) and `FakeTts::dialogue()` like a dialogue model. Test `editing_one_turn_redoes_its_chunk_and_the_next_only_if_it_listens`: cache hits `[true, false, false, true]` for the dialogue fake, `[true, false, true, true]` for the per-turn one.)*
- [x] 7.6 Pipeline test on a fake 30-minute script → accept: chunk count and cache behaviour as expected; run time with `FakeTts` < 10 s. *(`tests/audio_e2e.rs` `a_thirty_minute_script_is_chunked_by_beat_and_cached_per_chunk`: 90 beats of 20 s give 30 chunks, each ending on a beat, every turn in exactly one; a warm rerun has 0 misses; one edit has 2. 3.4 s in a debug build. The first version took 10.2 s: hound's per-sample WAV path is generic, so it is compiled unoptimised in podling-core. Float32 mono now encodes and decodes in one pass, byte for byte what hound writes (`the_float_fast_path_agrees_with_hound_both_ways`).)*

### Step 8 — asr-verify (., rust, normal)
Tooling: implementer · gates code-reviewer, dependency-auditor, performance-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: audio-e2e
- [x] 8.1 Add `plugin/asr.rs`: `AsrProvider`, `Transcript`, `transcribe_checked`, `FakeAsr` (returns the request's text, or a scripted miss for tests) → accept: unit tests. *(`plugin/asr.rs`: `AsrProvider::transcribe(&mut self)`, `Segment`/`Transcript`, `transcribe_checked` rejects non-finite, out-of-order or past-the-end segments; `FakeAsr::mishearing_first(n)` hears nothing for n calls. Tests: `the_fake_hears_each_turn_timed_by_its_words`, `a_scripted_fake_mishears_then_hears`, `bad_timings_are_rejected`.)*
- [x] 8.2 Add `plugin/whisper.rs`: `CandleWhisper` (candle-transformers Whisper `base.en` by default, CPU, safetensors only, weights BLAKE3 in fingerprint, loaded on first use). For audio over 30 s it decodes sequential windows with temperature fallback: it retries a window whose compression ratio is > 1.35 or whose mean log-prob is < −1.0, at temperatures 0.2…1.0, with no previous-text prompt (Spike results, change 4) → accept: parity test against a reference transcript fixture, gated like `cross_encoder_parity.rs`; a unit test shows a repetitive window triggers the fallback. *(`plugin/whisper.rs`: candle-transformers 0.11, CPU, lazy load; fingerprint is BLAKE3 of config/tokenizer/weights/preprocessor plus decoding constants. Sequential 30 s windows with openai seek and timestamp rules, temperatures 0…1.0, fallback on compression ratio > 1.35 or mean log-prob < −1.0, seeded sampling, no previous-text prompt. `a_repetitive_window_triggers_the_fallback`. `tests/whisper_parity.rs` (ignored, `PODLING_WHISPER_MODEL_DIR`) against `fixtures/whisper/reference.json` from `scripts/whisper_reference.py`: 2026-10-05, both bake-off clips 7‰ from transformers (tolerance 30‰), CPU RTF 0.24.)*
- [x] 8.3 Add `stages/verify_audio.rs` text normalisation (case, punctuation, numbers ↔ words) and WER → accept: unit tests incl. "1908" vs "nineteen oh eight". *(`words` lower-cases, drops punctuation and apostrophes, reads %/&/decimals, strips ordinals, and folds spelt-out numbers and years to digits; `wer_pm` rounds up. Tests include "1908" vs "nineteen oh eight".)*
- [x] 8.4 Verify each chunk right after synthesis; fill missing `turn_spans` from segment timestamps snapped to silence → accept: dialogue-backend fake without spans gets spans. *(Each take is transcribed by the `transcribe_chunk` stage right after `synthesize_chunk`. **Deviation:** transcription is its own cached stage keyed by the blob hash, the expected text and the ASR fingerprint, not a `verification` field on `ChunkResult`, so changing the ASR model or WER limit never re-synthesises audio. `spans_from` aligns transcript words to turns and snaps each boundary to the quietest 20 ms within ±300 ms when that is below half the median energy. `a_backend_without_spans_gets_them_from_the_transcript`: a span-less dialogue fake gets spans within one sample of the true ones.)*
- [x] 8.5 Regenerate failures with a new seed up to `max_retries`; banter beats get `takes`, best passing take wins; log WER, misses, retries, take; still failing → `Error` finding + `verified: false` → accept: tests for pass, retry-then-pass, give-up. *(Retries with a seed derived from the take; banter chunks get `takes.banter` takes; best = passed, then lowest WER, then speaking rate nearest the speaker's median, then earliest. A `chunk checked` log per take (turns, take, WER, quote misses, passed) and a warning on give-up. Unverified chunks become `verify_audio` `Error` findings appended to `analysis.json`. Tests: `a_misheard_take_is_made_again_and_the_passing_one_kept`, `a_chunk_that_never_passes_keeps_its_best_take_unverified`, `banter_gets_several_takes_even_when_the_first_passes`, `the_best_take_passes_then_is_heard_best_then_keeps_the_speakers_pace`, `unverified_chunks_become_error_findings`.)*
- [x] 8.6 Dependency/weights licence check (Whisper weights MIT) → accept: recorded. *(2026-10-05: `openai/whisper-base.en` weights are Apache-2.0 per the Hugging Face model card (the plan's "MIT" is the upstream code's licence); both are commercial-safe. New crate `miniz_oxide` 0.8.9: MIT OR Zlib OR Apache-2.0, `#![forbid(unsafe_code)]`, one dependency (`adler2`, 0BSD OR MIT OR Apache-2.0), used only for Whisper's compression ratio. `scripts/whisper_reference.py` is a dev-only script (transformers, Apache-2.0).)*
- [x] 8.7 Quote check: a quote missing from the transcript fails the chunk even when WER passes → accept: test. *(`a_missing_quote_fails_the_chunk_even_when_the_wer_passes`: one misheard word inside the quote is 39‰, under the 80‰ limit, and still fails.)*
- [x] 8.8 Pipeline test with `FakeAsr` scripted to fail once → accept: one regeneration, manifest shows take 2. *(`tests/audio_e2e.rs`: `a_misheard_chunk_is_regenerated_and_its_second_take_kept` (`pipeline::run_with_asr` with `FakeAsr::mishearing_first(1)`: chunks + 1 TTS calls, the first chunk's record has `take: 1`) and `a_chunk_that_never_passes_becomes_an_error_finding`.)*

### Step 9 — full-assembler (., rust, normal)
Tooling: implementer · gates code-reviewer, security-auditor, performance-reviewer, idiom-reviewer · skills language-aware-planning · guards cargo fmt/clippy/test
Depends on: beat-chunker, asr-verify
- [x] 9.1 Gaps from `Pace` (Quick 120 ms, Normal 300 ms, Beat 600 ms, LongPause 1000 ms; configurable) using turn spans → accept: unit test measures gaps ±10 ms. *(`stages/assemble.rs`: each chunk is cut into its pieces by the turn spans, each piece is trimmed of the model's silence, and the turn's pace sets the silence before it. A long turn carried over from the previous chunk gets the normal gap; a chunk with no spans is placed as one block. **Deviation:** the gaps are configured in a new optional `[mix]` section (`gaps_ms = { quick, normal, beat, long_pause, interrupt }`, each key optional), not in `[tts]`: `TtsConfig` is tagged per backend, and the mix applies to all of them. `[mix]` without `[tts]` is a `Config` error. Tests: `each_pace_puts_its_gap_before_the_turn` (all four paces ±10 ms, measured from the samples, and a configured 900 ms beat), `tests/assemble.rs::turns_are_paced_trimmed_and_normalised`.)*
- [x] 9.2 `Interrupt`: start the turn 150 ms early with an equal-power crossfade → accept: test of overlap length and no clipping. *(Quarter-sine/cosine crossfade over `gaps_ms.interrupt` (150 ms), never longer than either turn. Tests: `an_interrupt_overlaps_the_turn_before_by_150_ms` (exactly 7 200 samples; peak ≤ 0.3·√2 for two in-phase 0.3 tones), `an_interrupt_never_overlaps_more_than_the_turn_before`, `the_crossfade_keeps_equal_power`, `tests/assemble.rs::an_interrupt_cuts_in_and_nothing_clips` (0.95-amplitude turns: every sample ≤ 1.0, true peak ≤ −1 dBTP, 2.85 s for two 1.5 s turns).)*
- [x] 9.3 Second track: `Nonverbal::Over` clips mixed at their offset, ducked −6 dB → accept: test that the main track is unchanged outside the overlay. *(Unit 7 sends the TTS only the speaker's own `Before`/`After` sounds. `Over` sounds, and `Before`/`After` sounds by someone else, reach no backend yet: synthesise each backchannel as its own cached one-turn chunk in that speaker's voice (a sound the backend cannot say is dropped, and logged), then place it here.)* *(Done as noted. `synthesize_overlays` turns each backchannel that plays over a turn, or that someone other than the turn's speaker says, into a one-turn `synthesize_chunk` in that speaker's voice. It is cached, and the same response by the same speaker is made once per run. ASR does not check it: two words carry no facts and are below what Whisper hears reliably. A laugh, chuckle or sigh in those places has no words to send, and the protocol has no sound-only request, so it is left out with a warning. The assembler mixes each clip 6 dB down: centred in the turn (`Over`), ending as the turn starts (`Before`), or starting as it ends (`After`). Tests: `an_overlay_changes_the_track_only_where_it_plays` (identical outside the clip, clip × 0.5 inside), `tests/assemble.rs::a_sound_over_a_turn_goes_on_the_second_track`. The fake script's one "mm-hm" adds one `synthesize_chunk` run in `audio_e2e` and in the CLI test.)*
- [x] 9.4 Per-chunk loudness match (to the median chunk) before episode normalisation → accept: chunk loudness spread ≤ 1 LU on the fake episode. *(Each chunk, at 48 kHz, is gained to the median chunk's integrated loudness; a chunk too short or quiet to measure is left alone. `Assembled.chunks` says where each chunk landed. Tests: `chunks_are_matched_to_the_median_loudness`, `tests/assemble.rs::chunks_of_different_loudness_come_out_level` (chunks 18 dB apart come out within 1 LU, measured from the episode).)*
- [x] 9.5 Episode normalisation to −16 LUFS integrated / −1 dBTP true peak (limiter on the peak only); values logged and in the manifest → accept: measured values within ±0.5 LU / ≤ −1.0 dBTP. *(Gain to −16 LUFS, then a look-ahead limiter acting only around samples over a −1.5 dBFS ceiling. The limiter takes the sliding minimum of the gain each sample needs and smooths the cut with a moving average, so its gain is exactly 1 away from peaks. Any true-peak overshoot lowers the ceiling, and the gain-then-limit round repeats until the loudness is within 0.2 LU of the target (at most 6 rounds). Logged as `episode assembled` (measured and final LUFS and dBTP, limiter passes) and recorded in `audio.json` as `episode.integrated_lufs` and `true_peak_dbtp`. Tests: `loud_audio_is_normalised_and_its_true_peak_limited` (bursts 25 dB over the bed), `the_limiter_holds_peaks_to_the_ceiling_and_leaves_the_rest`, `the_sliding_minimum_matches_a_direct_one`. `audio_e2e::the_fake_episode_becomes_a_loudness_normalised_wav` measures the 16-bit `episode.wav` on disk: −16 ± 0.5 LUFS, ≤ −1.0 dBTP.)*
- [x] 9.6 Optional `encode.rs`: `[tts] encode = "opus" | "mp3"` runs `ffmpeg` via argv (no shell), output path fixed under `--out` → accept: missing ffmpeg → readable error; test asserts argv construction. *(`crates/podling-core/src/encode.rs`, set by `[mix] encode` (see 9.1). The program is always `ffmpeg` from `PATH`, run by argv with stdin closed. Both paths go through ffmpeg's `file:` protocol, so a path starting with `-` or shaped like a URL is still a file. ffmpeg writes `.episode.<ext>.part`, which is renamed to `episode.opus` or `episode.mp3` on success and removed on failure. The result is recorded as `episode.encoded` in `audio.json` (SCHEMA_VERSION 6). Tests: `the_arguments_are_fixed_but_for_the_paths`, `a_missing_ffmpeg_is_a_readable_error` (a Config error naming the program and the fix), `a_failing_ffmpeg_leaves_no_file_and_says_why`. The ignored test `ffmpeg_encodes_a_wav` passed with the local ffmpeg (libopus, libmp3lame) on 2026-10-05.)*
- [x] 9.7 Integration test `tests/assemble.rs` on synthetic PCM → accept: passes. *(Four tests: pacing and trimming with and without spans; an interrupt without clipping; a sound over a turn changing only its own 0.3 s; chunk loudness spread.)*

### Step 10 — live-and-docs (., markdown, normal)
Tooling: implementer · gates docs-curator · guards cargo test
Depends on: full-assembler
- [x] 10.1 Add `examples/tunguska/episode-tts.toml` (Ollama + sidecar profile + CC0 voices) and `voices/README.md` with the download commands and licences (clips gitignored) → accept: file parses; README lists each clip's licence. *(LibriTTS-R test-clean 4446/1089 clips, CC-BY-4.0, not CC0: the bake-off voices, credited in `voices/README.md`, whose curl|tar command was run and gave byte-identical clips. `every_example_episode_parses` loads the file, runs `check_audio`, and requires each cast licence to be CC0-1.0 or CC-BY-4.0 and named in the README.)*
- [x] 10.2 Live run: 10-minute Tunguska episode, cold cache → accept: `episode.wav` plays; all chunks verified; peak VRAM of the sidecar process ≤ 7.0 GB (device total recorded beside it); numbers recorded under "Live results". *(Exit 0; 18/18 chunks verified; sidecar peak 5,120 MiB, device 6,389 of 8,151 during TTS, with llama unloaded first; `episode.wav` decodes at −16.17 LUFS, −1.45 dBTP. See "Live results".)*
- [x] 10.3 Live run: 30-minute episode, then edit one turn and rerun → accept: one chunk re-synthesised; listening notes on seams and banter recorded. *(Machine part done: one edited turn re-synthesised exactly one chunk (2 takes), every upstream stage cached. The 30-minute target gives about 1 minute, capped by the ledger. Listening notes recorded under "Live results": the episodes are fine; mispronunciations and flat delivery are follow-ups.)*
- [x] 10.4 Update `docs/architecture.md`: pipeline, TTS/ASR rows in the plugin table, sidecar protocol and lifecycle, blob cache, bump rule for the TTS fingerprint, remove "TTS and ASR provider traits" from Deferred → accept: docs-curator passes. *(Artifact flow, plugin table (TTS, ASR, `UncitedFigures`), new "Episode audio" section, TTS/ASR fingerprints under bump rule 5, Deferred updated. Docs-curator check done in root-only mode: every relative link resolves and every claim was checked against the code.)*
- [x] 10.5 Update README config table (`[[cast]]`, `[tts]`, `[asr]`, `sidecars.toml`) → accept: every new key documented. *("Episode audio" section: `[[cast]]`, `tts.*`, `asr.*`, `mix.*`, `unload_after`, `sidecars.toml`, `--sidecars`.)*
- [x] 10.6 Update `docs/handoff.md` "Where things stand" and "Out of scope" → accept: Phase 5 row present. *("Where Phase 5 leaves it", Out of scope, and a phase5-tts-audio row in "Where things stand".)*
- [x] 10.7 (added by the live run) Beats that an 8B model can write → accept: the live script stage passes without a beat error. *(llama3.1:8b, asked for `beats` as `{start, end}` index ranges, wrote inclusive ends ("beat 1 starts at turn 2, so turns 1..2 are in no beat") on both attempts, or looped on turns and never reached the list. The model now marks `beat` on the turn that begins each beat and `beats_from_marks` derives contiguous ranges, so coverage holds by construction; an unmarked first turn opens a narration beat, and no marks at all leaves the beats implied. Audio rule 9 asks for about 150 words a minute and no repeated points, and one example audio turn shows the fields. `PROMPT_VERSION` 6, `script` 10, fake LLM 6; the exported `Script` is unchanged, and the ledger and analyse keys in `without_nli_the_cache_keys_are_unchanged` did not move, so a text-only script is byte-identical. Five fake-audio samples: no beat errors; two failed on the existing citation and placeholder checks.)*

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
