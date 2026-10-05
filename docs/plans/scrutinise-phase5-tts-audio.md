---
slug: scrutinise-phase5-tts-audio
goal: "Fix the four Warnings /scrutinise found in Phase 5 audio: backchannels pass speech-recognition, the TTS cache key follows the weights, voice clips must be CC0/CC-BY, and the TTS sidecar's whole process tree is stopped."
classification: in-scope   # /scrutinise phase5-tts-audio round 1 (range 773ee71..e234ed8); hard rules in docs/plans/phase5-tts-audio.md and .claude/CLAUDE.md "Licensing", "Hardware target"
tracker_rows: [TRACKER#scrutinise-phase5-tts-audio/1, TRACKER#scrutinise-phase5-tts-audio/2, TRACKER#scrutinise-phase5-tts-audio/3, TRACKER#scrutinise-phase5-tts-audio/4, TRACKER#scrutinise-phase5-tts-audio/5, TRACKER#scrutinise-phase5-tts-audio/6]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: "skipped(root-only agent mode; the isolated scrutineer's report is the independent derivation, and the plan-strategist pass chose the decomposition — Option C, one unit per Warning)"
coverage:
  contract:      "2.3 (/health gains `adapter`, read with a serde default, so no protocol bump), 3.1 (new VoiceRefError variant; VoiceRef's serde shape and the episode schema are unchanged)"
  data:          "N/A(no persistence change; cache keys change on their own for chunks with own-speaker backchannels (1.2) and for every sidecar chunk once (2.3), which is the intended invalidation)"
  config:        "3.1 (episode [[cast]] voice licence is now restricted to an allow-list)"
  security:      "3.1 (licence allow-list, fail closed), 4.1–4.2 (the sidecar runs in its own process group; stop signals only that group, never Podling's)"
  tests:         "1.3, 2.4, 2.5, 3.2, 4.3, 4.4"
  observability: "4.2 (the existing 'sidecar ignored SIGTERM; killing it' warn also covers the group), 2.3 (adapter logged with the health line)"
  interface:     "3.1 (the error names the licence and lists the allowed ones)"
  docs:          "2.6 (sidecars/tts/README.md /health text), 3.3 (examples/tunguska/voices/README.md allow-list line)"
  rollback:      "git revert of each unit's merge commit; the units are independent apart from a shared test fixture"
units:
  - id: 1
    scope_id: asr-expected-text
    project: .
    depends_on: []
    module: crates/podling-core/src/stages/synthesize.rs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/plugin/tts.rs
        - crates/podling-core/src/stages/verify_audio.rs
        - sidecars/tts/podling_tts/backends/__init__.py
      docs: [docs/plans/scrutinise-phase5-tts-audio.md]
      write:
        - crates/podling-core/src/stages/synthesize.rs
        - crates/podling-core/src/plugin/tts.rs
        - sidecars/tts/podling_tts/backends/__init__.py
    tooling: { implementer: implementer, gates: [code-reviewer], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: tts-weights-fingerprint
    project: .
    depends_on: [4]   # both edit crates/podling-core/tests/fixtures/fake_sidecar.py; the safety fix lands first
    module: sidecars/tts, crates/podling-core/src/plugin/sidecar_tts.rs
    language: python, rust
    security: normal
    scope:
      read:
        - sidecars/tts/podling_tts/backends/qwen.py
        - sidecars/tts/podling_tts/backends/fake.py
        - sidecars/tts/podling_tts/backends/__init__.py
        - sidecars/tts/podling_tts/server.py
        - sidecars/tts/tests/test_server.py
        - sidecars/tts/README.md
        - crates/podling-core/src/plugin/sidecar_tts.rs
        - crates/podling-core/src/plugin/whisper.rs
        - crates/podling-core/tests/fixtures/fake_sidecar.py
      docs: [docs/plans/scrutinise-phase5-tts-audio.md]
      write:
        - sidecars/tts/podling_tts/backends/qwen.py
        - sidecars/tts/podling_tts/backends/__init__.py
        - sidecars/tts/podling_tts/server.py
        - sidecars/tts/tests/test_server.py
        - sidecars/tts/README.md
        - crates/podling-core/src/plugin/sidecar_tts.rs
        - crates/podling-core/tests/fixtures/fake_sidecar.py
    tooling: { implementer: implementer, gates: [code-reviewer], skills: [], guards: [cargo fmt, cargo clippy, cargo test, uv run pytest], mcp: [] }
  - id: 3
    scope_id: voice-licence-allowlist
    project: .
    depends_on: []
    module: crates/podling-types/src/episode.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/tests/roundtrip.rs
        - examples/tunguska/episode.toml
        - examples/tunguska/episode-tts.toml
        - examples/tunguska/voices/README.md
      docs: [docs/plans/scrutinise-phase5-tts-audio.md]
      write:
        - crates/podling-types/src/episode.rs
        - crates/podling-types/tests/roundtrip.rs
        - examples/tunguska/voices/README.md
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 4
    scope_id: sidecar-process-group
    project: .
    depends_on: []
    module: crates/podling-core/src/plugin/sidecar.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/plugin/sidecar.rs
        - crates/podling-core/tests/sidecar_tts.rs
        - crates/podling-core/tests/fixtures/fake_sidecar.py
        - crates/podling-core/Cargo.toml
        - sidecars/tts/podling_tts/server.py
      docs: [docs/plans/scrutinise-phase5-tts-audio.md]
      write:
        - crates/podling-core/src/plugin/sidecar.rs
        - crates/podling-core/tests/sidecar_tts.rs
        - crates/podling-core/tests/fixtures/fake_sidecar.py
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 5
    scope_id: audio-cleanups
    project: .
    depends_on: [1, 2, 3, 4]
    module: crates/podling-core/src/pipeline.rs, crates/podling-core/src/stages/script.rs, crates/podling-types/src/audio.rs
    language: rust
    security: normal
    scope:
      read: [crates/podling-core/src/pipeline.rs, crates/podling-core/src/stages/script.rs, crates/podling-core/src/plugin/sidecar_tts.rs, crates/podling-types/src/audio.rs, crates/podling-types/src/episode.rs]
      docs: [docs/plans/scrutinise-phase5-tts-audio.md]
      write: [crates/podling-core/src/pipeline.rs, crates/podling-core/src/stages/script.rs, crates/podling-core/src/plugin/sidecar_tts.rs, crates/podling-types/src/audio.rs, crates/podling-types/src/episode.rs, crates/podling-types/tests/snapshots/schema_snapshot__audio.snap]
    tooling: { implementer: implementer, gates: [code-reviewer], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 6
    scope_id: audio-dedupe
    project: .
    depends_on: [5]
    module: crates/podling-core/src/plugin/tts.rs, crates/podling-core/src/plugin/sidecar_tts.rs, crates/podling-core/src/stages/synthesize.rs, crates/podling-core/src/pipeline.rs
    language: rust
    security: normal
    scope:
      read: [crates/podling-core/src/plugin/tts.rs, crates/podling-core/src/plugin/sidecar_tts.rs, crates/podling-core/src/stages/synthesize.rs, crates/podling-core/src/pipeline.rs]
      docs: [docs/plans/scrutinise-phase5-tts-audio.md]
      write: [crates/podling-core/src/plugin/tts.rs, crates/podling-core/src/plugin/sidecar_tts.rs, crates/podling-core/src/stages/synthesize.rs, crates/podling-core/src/pipeline.rs]
    tooling: { implementer: implementer, gates: [code-reviewer], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: fix the Phase 5 audio review findings (round 1)

## Outcome
A turn with its speaker's own "Mm-hm" passes the speech check, the TTS cache key changes when the TTS weights or adapter change, an episode cannot clone a voice from a non-commercial clip, and stopping the TTS sidecar frees the GPU even when it runs behind `uv run`.

Units 1–4 fix the four Warnings and are driven now. Units 5–6 are the Suggestions, registered PENDING as follow-ups and not driven in this round.

## Scope Steps (executable core)

### Step 1 — asr-expected-text (., rust, normal)
Tooling: implementer · gates code-reviewer · guards cargo fmt, clippy, test
Depends on: none
- [ ] 1.1 Add `SpokenTurn::said(&self) -> String` in `crates/podling-core/src/plugin/tts.rs`. It returns the words the TTS will say: the texts of the speaker's own `Backchannel` sounds at `Before`, then `text`, then those at `After`, joined by single spaces. Laughs, chuckles and sighs have no words and add nothing; a backchannel's text is words such as "Mm-hm.", never a tag. This mirrors `in_line()` in `sidecars/tts/podling_tts/backends/__init__.py:112`. Add a comment on each side naming the other. → accept: a unit test in tts.rs: `Backchannel{text:"Mm-hm."}` by the speaker at Before plus text "Right, so." gives "Mm-hm. Right, so."; a Chuckle, or a backchannel by someone else, adds nothing.
- [ ] 1.2 In `crates/podling-core/src/stages/synthesize.rs:428`, build `expected` from `t.said()` instead of `t.text`, so the WER, `words` (speaking rate) and `spans_from` all see what was said. Keep `quotes_in` as is: a quote is matched inside the turn text, which is still part of `said()`. → accept: `cargo test -p podling-core` passes; no stage VERSION changes, because `TranscribeInput.expected` is already in the transcribe key (`verify_audio.rs:483-489`) and synthesis keys are unchanged.
- [ ] 1.3 Add a test in synthesize.rs: a turn with `Backchannel{by: speaker, at: Before, text: "Mm-hm."}`, and a canned ASR that returns the text the TTS spoke (built in the test as `"Mm-hm. " + text`, not from `expected`). The chunk must verify on take 0 with no Error finding. → accept: the test fails on the pre-fix code (shown in the unit summary) and passes after.

### Step 2 — tts-weights-fingerprint (., python + rust, normal)
Tooling: implementer · gates code-reviewer · guards cargo fmt, clippy, test; uv run pytest
Depends on: 4
- [ ] 2.1 In `sidecars/tts/podling_tts/backends/qwen.py:57`, make `weights()` return the snapshot's commit (its directory name) only for a hub snapshot. For `--model-dir`, return `sha256:<hex>` over the sorted weight and config files (`*.safetensors`, `*.json`, file name + bytes). Compute it once, lazily, and cache it on the instance so `/health` stays cheap. → accept: a pytest on a temp model dir: two dirs with the same name and different bytes give different `weights()`; the same bytes give the same value.
- [ ] 2.2 Add `ADAPTER_VERSION = 1` in `sidecars/tts/podling_tts/backends/__init__.py`, documented as "bump when a backend's handling of a request changes (e.g. `in_line`)". Report it as `"adapter"` in `/health` (`server.py:51`). → accept: the `test_server.py` health test asserts `adapter`.
- [ ] 2.3 In `crates/podling-core/src/plugin/sidecar_tts.rs`, add `#[serde(default)] adapter: u32` to `Health` (`:32`), log it on the healthy line (`:175`), and include it in `fingerprint()` (`:296`). The default keeps an older worker parseable, so `PROTOCOL` stays 1. → accept: the existing `health_must_match_our_protocol` test still passes; a new test shows two healths that differ only in `adapter` give different fingerprints.
- [ ] 2.4 Report `"adapter": 1` from `crates/podling-core/tests/fixtures/fake_sidecar.py`'s `/health`. → accept: `cargo test -p podling-core --test sidecar_tts` passes.
- [ ] 2.5 Run `uv run pytest -q` in `sidecars/tts`. → accept: all pass, including 2.1 and 2.2.
- [ ] 2.6 Update `sidecars/tts/README.md:70`. `weights` is the hub commit, or a content hash of a local `--model-dir`; `adapter` is the adapter version. Both feed Podling's cache key, and the first run after this change re-synthesises once. → accept: the README describes both fields.

### Step 3 — voice-licence-allowlist (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor · guards cargo fmt, clippy, test
Depends on: none
- [ ] 3.1 In `crates/podling-types/src/episode.rs:213-232`, add `pub const VOICE_LICENCES: [&str; 3] = ["CC0-1.0", "CC-BY-3.0", "CC-BY-4.0"]` and a `VoiceRefError::LicenceNotAllowed { reference, licence }` variant. Its message names the licence and lists the allowed SPDX ids, and says that non-commercial or unknown licences are refused. `VoiceRef::new` checks the trimmed licence against the list after the empty check. The match is strict and exact (no normalising of "cc0" or "CC-BY 4.0"): the error tells the user the exact id to write. → accept: `VoiceRef::new("v.wav", "Hi.", "CC-BY-NC-4.0")` is `Err(LicenceNotAllowed{..})`; "CC-BY-4.0" and "CC0-1.0" are `Ok`.
- [ ] 3.2 Add tests to `crates/podling-types/tests/roundtrip.rs` next to `a_voice_needs_a_licence_and_a_transcript` (`:208`). The direct constructor rejects CC-BY-NC-4.0 and "proprietary". The TOML path (`AUDIO.replace(...)`) errors with a message naming the licence. → accept: `cargo test -p podling-types` passes; `cargo test --workspace` passes (examples use CC0-1.0 / CC-BY-4.0: `examples/tunguska/episode.toml:29,35`, `episode-tts.toml:64,70`).
- [ ] 3.3 State in `examples/tunguska/voices/README.md:5` that Podling refuses any other licence (the allow-list). → accept: the README line names the three ids.

### Step 4 — sidecar-process-group (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor · guards cargo fmt, clippy, test
Depends on: none
- [ ] 4.1 In `crates/podling-core/src/plugin/sidecar.rs:139`, under `#[cfg(unix)]`, call `std::os::unix::process::CommandExt::process_group(0)` before `.spawn()`, so the worker leads a new group (pgid = child pid) and Podling's own group is never signalled. → accept: compiles on unix; non-unix is unchanged.
- [ ] 4.2 Make `terminate` (`:335`) send TERM to the group (`rustix::process::kill_process_group(Pid::from_child(child), Signal::TERM)`). In `stop` (`:286-311`), after the grace period, send KILL to the group, then `child.kill()` + `wait()`. When the direct child has already exited, or exits within the grace period, still send the group a final KILL (ignoring ESRCH) before returning, so a grandchild left behind by `uv` is not orphaned holding the GPU. Signal the group before reaping the direct child: until it is reaped, its pid (= the pgid) cannot be reused. → accept: the existing `a_worker_that_ignores_sigterm_is_killed` and `synthesises_through_the_stub_and_stops_it_on_drop` tests pass.
- [ ] 4.3 Add a `--mode orphan` to `crates/podling-core/tests/fixtures/fake_sidecar.py`. It starts a grandchild (`subprocess.Popen([sys.executable, "-c", <ignore SIGTERM; sleep 60>, "<tag>"])`) that ignores SIGTERM, then serves normally. → accept: the fixture runs on its own.
- [ ] 4.4 Add `the_whole_worker_group_is_stopped` to `crates/podling-core/tests/sidecar_tts.rs`. Start the orphan-mode stub, assert the grandchild is running via `running(tag)` (`:65`), drop the provider, and assert `running(tag)` is empty. → accept: fails on the pre-fix code, passes after.
- [ ] 4.5 In the unit summary, state the known limits. The worker leaves the terminal's foreground group, so Ctrl-C reaches only Podling, whose `Drop` stops the worker. A SIGKILLed Podling still relies on the worker's `watch_parent` fallback (`sidecars/tts/podling_tts/server.py`). → accept: the summary records both, with the `watch_parent` line cited.

### Step 5 — audio-cleanups (PENDING follow-up, not driven this round)
- [ ] 5.1 S2: delete any stale `audio.json` / `episode.*` in `out_dir` before writing the text artifacts (`crates/podling-core/src/pipeline.rs:247-256`). → accept: a test running with `[tts]` then without leaves no audio.json.
- [ ] 5.2 S4: remove the files in `reply.clips` along with `out_path` (`crates/podling-core/src/plugin/sidecar_tts.rs:376`). → accept: the run dir holds no clip WAVs after a call.
- [ ] 5.3 S5: add script rule 5 only when `input.cast` is non-empty (`crates/podling-core/src/stages/script.rs:28`), and bump PROMPT_VERSION. → accept: a text-only prompt equals the pre-Phase-5 instructions.
- [ ] 5.4 S6: export one `DEFAULT_MAX_WER_PM` from podling-types (`episode.rs:384`) and use it in `pipeline.rs:283`. → accept: there is a single definition.
- [ ] 5.5 S8: reword the `ChunkRecord.id` doc to "content id of the chunk spec" (`crates/podling-types/src/audio.rs:96`) and refresh the schema snapshot. → accept: the snapshot test passes.

### Step 6 — audio-dedupe (PENDING follow-up, not driven this round)
- [ ] 6.1 S7: write the "speaker plus the makers of its sounds" iteration and the missing-voice error once (e.g. `SpokenTurn::voices_needed()`), used by tts.rs `synthesize_checked`, sidecar_tts.rs `missing_voice`, and synthesize.rs `Voices::keys_for`. → accept: one definition; the tests pass.
- [ ] 6.2 S1: carry the voice clip bytes, or hash, from `Voices::resolve` (`synthesize.rs:69`) into `SidecarTts::stage` (`sidecar_tts.rs:204`) instead of re-reading the path. → accept: editing a clip mid-run cannot desync the audio from its key.
- [ ] 6.3 S3: with `--no-cache`, put blobs in a TempDir, or clear `<out>/.blobs` after assembly (`pipeline.rs:352`). → accept: there is no `.blobs` after a `--no-cache` run.

## Sequencing
Run the units in the order 3 → 1 → 4 → 2.
- **3, licence:** first, because it is the hard rule.
- **1, speech check:** a correctness fix.
- **4, process group:** the VRAM safety fix.
- **2, weights fingerprint:** after 4, because both edit `fake_sidecar.py`. Running 2 last keeps the cache-key change from holding back the safety fix.

Units 1, 3 and 4 touch separate files. Units 5–6 stay PENDING.

## Decomposition
plan-strategist Option C: one unit per Warning, since each is a different failure mode (ASR accuracy, cache correctness, licence policy, process lifecycle). That keeps each review and revert separate. The only coupling is the shared test fixture, and ordering handles it.

Rejected:
- **Option A (all four in parallel):** a fixture collision.
- **Option B (two units by layer):** bundles unrelated fixes, so a cache-key mistake would block the VRAM fix.

## Verification background
- `expected` is the turn text only — `crates/podling-core/src/stages/synthesize.rs:428`; `spoken()` attaches own Before/After nonverbals — `synthesize.rs:641-664`; the worker speaks only own backchannels in line, with their text — `sidecars/tts/podling_tts/backends/__init__.py:112-128`.
- `TranscribeInput { blob, expected }` is the transcribe cache input — `synthesize.rs:458-461`; `TranscribeChunk` VERSION 1, fingerprint = asr only — `verify_audio.rs:483-490`.
- `weights()` = `self.path.name` — `sidecars/tts/podling_tts/backends/qwen.py:57-59`; `--model-dir` flag — `server.py:197`; `/health` payload — `server.py:51-60`; the Rust `Health` struct — `sidecar_tts.rs:32-38`; fingerprint — `sidecar_tts.rs:296-303`; README promise — `sidecars/tts/README.md:70`. Whisper's byte-hash precedent — `crates/podling-core/src/plugin/whisper.rs:577-581`.
- Licence check is non-empty only — `crates/podling-types/src/episode.rs:221`; hard rule "every voice clip records its licence (CC0/CC-BY only)" — the Phase 5 goal; voices README — `examples/tunguska/voices/README.md:5`. `VoiceCredit.licence` in audio.json is a plain `String` (`crates/podling-types/src/audio.rs:137`), not re-validated, so existing artifacts still parse.
- Spawn without a process group — `crates/podling-core/src/plugin/sidecar.rs:139-150`; TERM to the pid only — `sidecar.rs:335-341`; KILL to the child only — `sidecar.rs:306`; rustix with "process" is already a unix dependency — `Cargo.toml:49`, `crates/podling-core/Cargo.toml:28-29`; the test helper `running(tag)` scans /proc — `crates/podling-core/tests/sidecar_tts.rs:65-89`.

CONSUMERS:
- `SpokenTurn` (new method only; no field change): synthesize.rs, tts.rs, sidecar_tts.rs serialise it. The wire JSON is unchanged.
- `/health` gains `adapter`: the Rust `Health` (sidecar_tts.rs:32) reads it with a serde default; `crates/podling-core/tests/fixtures/fake_sidecar.py:64-80` and `sidecars/tts/tests/test_server.py` produce or assert it.
- `VoiceRef::new` / `TryFrom<RawVoiceRef>` (episode.rs:213-252): callers are `synthesize.rs:76` (re-resolves an already-valid licence), and tests in tts.rs:299, synthesize.rs:738,945, audio_e2e.rs:30,36,394, sidecar_tts.rs:104, plugin/mod.rs:276, roundtrip.rs:147-224, cli.rs:77. All use CC0-1.0 or CC-BY-4.0.
- `VoiceRefError` is re-exported at `crates/podling-types/src/lib.rs:26`; adding a variant breaks no exhaustive match outside episode.rs.
- `Sidecar::spawn`/`stop`: one owner, `SidecarTts` (sidecar_tts.rs); the behaviour change is internal.

## Risk & rollback
- Unit 2 changes every sidecar TTS cache key once: cached audio is re-synthesised on the next run. This is intended and stated in the unit summary and the README.
- Unit 3 can refuse an existing user episode whose clip has another licence. The error names the allow-list, and that is the point of the hard rule.
- Unit 4 detaches the worker from the terminal's Ctrl-C. Podling's `Drop` still stops it, and SIGKILL of Podling falls back to `watch_parent` (task 4.5).
- Each unit reverts with `git revert` of its merge commit.

## Out of scope
- Plan tasks 1.4 and 10.3 of phase5-tts-audio (human listening passes) stay with the user.
- Unit 5 and 6 Suggestions are registered PENDING and are not driven in this round.
