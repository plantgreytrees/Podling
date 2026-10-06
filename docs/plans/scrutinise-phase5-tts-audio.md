---
slug: scrutinise-phase5-tts-audio
goal: "Fix the four Warnings /scrutinise found in Phase 5 audio: backchannels pass speech-recognition, the TTS cache key follows the weights, voice clips must be CC0/CC-BY, and the TTS sidecar's whole process tree is stopped, even after the worker itself has died (round 2)."
classification: in-scope   # /scrutinise phase5-tts-audio round 1 (range 773ee71..e234ed8) , round 2 (range e234ed8..4a75676) and round 3 (range 4a75676..ea75403: 0 Critical, 0 Warning, 4 Suggestions folded into unit 8); hard rules in docs/plans/phase5-tts-audio.md and .claude/CLAUDE.md "Licensing", "Hardware target"
tracker_rows: [TRACKER#scrutinise-phase5-tts-audio/1, TRACKER#scrutinise-phase5-tts-audio/2, TRACKER#scrutinise-phase5-tts-audio/3, TRACKER#scrutinise-phase5-tts-audio/4, TRACKER#scrutinise-phase5-tts-audio/5, TRACKER#scrutinise-phase5-tts-audio/6, TRACKER#scrutinise-phase5-tts-audio/7, TRACKER#scrutinise-phase5-tts-audio/8]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: "skipped(root-only agent mode; the isolated scrutineer's report is the independent derivation, and the plan-strategist pass chose the decomposition — Option C, one unit per Warning; round 2 has one Warning in one file, fixed in one unit)"
coverage:
  contract:      "2.3 (/health gains `adapter`, read with a serde default, so no protocol bump), 3.1 (new VoiceRefError variant; VoiceRef's serde shape and the episode schema are unchanged)"
  data:          "N/A(no persistence change; cache keys change on their own for chunks with own-speaker backchannels (1.2) and for every sidecar chunk once (2.3), which is the intended invalidation)"
  config:        "3.1 (episode [[cast]] voice licence is now restricted to an allow-list)"
  security:      "3.1 (licence allow-list, fail closed), 4.1–4.2 as shipped (pidfd-pinned descendants, never Podling's group), 7.1–7.3 (descendants pinned at ready time are stopped on every path; signals and liveness only through pidfds)"
  tests:         "1.3, 2.4, 2.5, 3.2, 4.3, 4.4, 7.4, 7.5"
  observability: "4.2 (the existing 'sidecar ignored SIGTERM; killing it' warn also covers the group), 2.3 (adapter logged with the health line)"
  interface:     "3.1 (the error names the licence and lists the allowed ones)"
  docs:          "2.6 (sidecars/tts/README.md /health text), 3.3 (examples/tunguska/voices/README.md allow-list line), 7.6 (Descendants limits) and the Step 4 "as shipped" note; docs/architecture.md stale lines (S-R2-8) go to /sync-docs"
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
  - id: 7
    scope_id: sidecar-orphan-after-exit
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
        - Cargo.toml
        - Cargo.lock
        - sidecars/tts/podling_tts/server.py
      docs: [docs/plans/scrutinise-phase5-tts-audio.md]
      write:
        - crates/podling-core/src/plugin/sidecar.rs
        - crates/podling-core/tests/sidecar_tts.rs
        - crates/podling-core/tests/fixtures/fake_sidecar.py
        - Cargo.toml
        - Cargo.lock
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 8
    scope_id: sidecar-hardening
    project: .
    depends_on: [7]
    module: crates/podling-core/src/plugin/sidecar.rs, sidecars/tts, crates/podling-types/src/episode.rs
    language: rust, python
    security: normal
    scope:
      read: [crates/podling-core/src/plugin/sidecar.rs, crates/podling-core/src/plugin/sidecar_tts.rs, crates/podling-core/tests/sidecar_tts.rs, crates/podling-core/tests/fixtures/fake_sidecar.py, sidecars/tts/podling_tts/backends/qwen.py, sidecars/tts/podling_tts/server.py, sidecars/tts/podling_tts/backends/__init__.py, crates/podling-core/src/plugin/tts.rs, crates/podling-types/src/episode.rs]
      docs: [docs/plans/scrutinise-phase5-tts-audio.md]
      write: [crates/podling-core/src/plugin/sidecar.rs, crates/podling-core/src/plugin/sidecar_tts.rs, crates/podling-core/tests/sidecar_tts.rs, crates/podling-core/tests/fixtures/fake_sidecar.py, sidecars/tts/podling_tts/backends/qwen.py, sidecars/tts/podling_tts/server.py, sidecars/tts/tests/test_server.py, crates/podling-core/src/plugin/tts.rs, crates/podling-types/src/episode.rs, crates/podling-types/tests/roundtrip.rs, crates/podling-types/tests/snapshots/schema_snapshot__episode.snap]
    tooling: { implementer: implementer, gates: [code-reviewer], skills: [], guards: [cargo fmt, cargo clippy, cargo test, uv run pytest], mcp: [] }
---

# Plan: fix the Phase 5 audio review findings (round 1)

## Outcome
A turn with its speaker's own "Mm-hm" passes the speech check, the TTS cache key changes when the TTS weights or adapter change, an episode cannot clone a voice from a non-commercial clip, and stopping the TTS sidecar frees the GPU even when it runs behind `uv run`.

Units 1–4 fix the four Warnings and are driven now. Units 5–6 are the Suggestions, registered PENDING as follow-ups and not driven in this round.

Round 2 (range e234ed8..4a75676) found one Warning: the shipped Step 4 stops nothing once the worker itself has died, so a GPU-holding child left by a crashed `uv run` keeps running. Unit 7 fixes it. Unit 8 collects the round 2 Suggestions as a PENDING follow-up.

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

> **As shipped (commit d3271c3):** 4.1–4.2 were not built as written. `process_group(0)` takes the worker out of the terminal's foreground group, so Ctrl-C would no longer reach it. Instead, `stop()` scans the worker's descendants in `/proc` and pins each one with a pidfd, re-checking its parent so a reused pid is never signalled (`Descendants`, `crates/podling-core/src/plugin/sidecar.rs:377-445`). 4.3–4.4 shipped as `--mode orphan` and `a_child_of_the_worker_that_ignores_sigterm_is_killed_too`. The "direct child already exited" case of 4.2 was missed; round 2 Step 7 closes it.

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

### Step 7 — sidecar-orphan-after-exit (., rust, high) — round 2 Warning
Tooling: implementer · gates code-reviewer, security-auditor · guards cargo fmt, clippy, test
Depends on: none
- [ ] 7.1 Pin the descendants early. In `Sidecar::spawn` (`crates/podling-core/src/plugin/sidecar.rs:127-205`), once the worker has reported ready (`:197`), take `Descendants::of(pid)` and keep it in a new `Sidecar` field. A pidfd keeps naming its process after that process is re-parented to init, so the snapshot still reaches a grandchild whose wrapper has died. → accept: `Sidecar` holds the ready-time snapshot; non-Linux builds keep the empty stub.
- [ ] 7.2 Stop on every path. In `stop()` (`:291-339`), do not return early when `try_wait` shows the worker has exited. Log its exit, then still stop the pinned processes: TERM, wait up to `STOP_GRACE`, then KILL whatever is left. While the worker is alive, union the ready-time snapshot with a fresh `Descendants::of(pid)` to catch processes started after ready. De-duplicate by pid only while the pinned process is still alive. If it has exited, the pid may now name a new descendant, so keep the fresh pidfd. Never signal a bare pid, only pidfds. → accept: the existing `synthesises_through_the_stub_and_stops_it_on_drop`, `a_worker_that_ignores_sigterm_is_killed` and `a_child_of_the_worker_that_ignores_sigterm_is_killed_too` tests still pass.
- [ ] 7.3 Check liveness through the pidfd (folds in S-R2-2). `Descendants::running()` (`:438-443`) reads `/proc/<pid>/stat`, so a pid reused by an unrelated process counts as running. Instead, `poll` each pidfd for `POLLIN` with a zero timeout; it becomes readable once that process exits. This needs rustix's `event` feature on the workspace dependency (`Cargo.toml:49`). That enables a feature of an already-audited crate and adds no new crate. Keep `proc_stat` for the scan itself. → accept: `running()` no longer reads `/proc`; clippy is clean.
- [ ] 7.4 Add `--mode orphan-exit` to `crates/podling-core/tests/fixtures/fake_sidecar.py`. It spawns the same SIGTERM-ignoring child as `orphan`, prints the ready line, then exits on its own after about 0.5 s, before any request. → accept: the fixture leaves the tagged child running after it exits.
- [ ] 7.5 Add `a_child_left_behind_by_a_dead_worker_is_killed` to `crates/podling-core/tests/sidecar_tts.rs`. Start the `orphan-exit` stub, wait until the worker has exited while `running(tag)` (`:65`) still shows the child, drop the provider, and assert `running(tag)` is empty. → accept: fails on the pre-fix code (shown in the unit summary), passes after, 3/3 runs.
- [ ] 7.6 Record the limits in the `Descendants` doc comment and the unit summary. A process started after ready by a worker that then dies is still missed, and so is a double-forked or `setsid` descendant re-parented before the ready scan. The worker's `watch_parent` (`sidecars/tts/podling_tts/server.py:174`) stays the fallback for those. → accept: the doc comment names both limits.

### Step 8 — sidecar-hardening (PENDING follow-up, not driven this round)
- [ ] 8.1 S-R2-3: on `pidfd_open` `ENOSYS`/`EPERM` (`sidecar.rs:402`), warn once and fall back to a ppid-checked `kill()`. → accept: a unit test of the error mapping.
- [ ] 8.2 S-R2-4: hash every regular file of a `--model-dir` except metadata (`.cache/`, `*.lock`, `*.md`), or refuse a dir without `*.safetensors` (`sidecars/tts/podling_tts/backends/qwen.py:52-53`). → accept: a pytest where a changed `*.bin` changes `weights()`.
- [ ] 8.3 S-R2-5: cache the `--model-dir` digest keyed on each file's (size, mtime_ns, inode), or make `STARTUP_TIMEOUT` (`sidecar.rs:30`) a per-profile setting. → accept: a second start does not re-read the weights.
- [ ] 8.4 S-R2-6: store the trimmed licence in `VoiceRef` (`crates/podling-types/src/episode.rs:235,241-245`). → accept: `" CC0-1.0 "` is stored as `CC0-1.0`.
- [ ] 8.5 S-R2-7: give `RawVoiceRef.licence` (`episode.rs:199-202`) a schema enum of `VOICE_LICENCES`, and refresh the episode schema snapshot. → accept: the snapshot lists the three ids.
- [ ] 8.6 S-R2-9: a shared golden fixture (turns → spoken string) tested by both `SpokenTurn::said` (`crates/podling-core/src/plugin/tts.rs:68`) and `in_line` (`sidecars/tts/podling_tts/backends/__init__.py:116`). → accept: one JSON fixture read by a cargo test and a pytest.

Round 3 (range 4a75676..ea75403) found no Critical or Warning. Its four Suggestions join this PENDING step:
- [ ] 8.7 S-R3-1: when `SidecarTts::explain` finds the worker dead (`crates/podling-core/src/plugin/sidecar_tts.rs:236`, `exits_within`), stop the pinned tree right away instead of waiting for `Drop`. Make `Sidecar::stop` idempotent, or add a `reap_tree()` that `explain` calls. A provider kept alive after a failed request would otherwise hold VRAM until it is dropped. → accept: a test where the `orphan-exit` worker dies, a request fails, and the tagged child is gone *before* the provider is dropped.
- [ ] 8.8 S-R3-2: log `poll` errors in `is_running` (`crates/podling-core/src/plugin/sidecar.rs:494-501`) at `tracing::debug!` rather than silently counting the process as gone. → accept: clippy is clean; the behaviour is unchanged.
- [ ] 8.9 S-R3-3: cover `Descendants::extend` (`sidecar.rs:421-428`). Add a fixture mode whose worker starts the SIGTERM-ignoring child *after* ready (in the first GET, or on a timer), plus a drop test. → accept: the test fails if `stop()` skips the fresh scan.
- [ ] 8.10 S-R3-4: replace the 1.0 s timer in `--mode orphan-exit` (`crates/podling-core/tests/fixtures/fake_sidecar.py:136-138`) with a sentinel file in `--run-dir` that the test writes after `start` returns. → accept: `a_child_left_behind_by_a_dead_worker_is_killed` has no timing dependence and passes 3/3.

## Sequencing
Run the units in the order 3 → 1 → 4 → 2.
- **3, licence:** first, because it is the hard rule.
- **1, speech check:** a correctness fix.
- **4, process group:** the VRAM safety fix.
- **2, weights fingerprint:** after 4, because both edit `fake_sidecar.py`. Running 2 last keeps the cache-key change from holding back the safety fix.

Units 1, 3 and 4 touch separate files. Units 5–6 stay PENDING.

Round 2 drives unit 7 alone. Unit 8 stays PENDING; it depends on 7 because both edit `sidecar.rs`.

Round 3 drives nothing; it adds tasks 8.7–8.10 to the PENDING unit 8.

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

Round 2 (Step 7):
- Early return on an exited worker — `crates/podling-core/src/plugin/sidecar.rs:291-295`; the scan runs only after it — `:297-299`; `Descendants` and its `/proc`-based `running()` — `:377-445`; the ready line is parsed at `:197`.
- Plan task 4.2's promise for this case — Step 4 above.
- The worker's own fallback — `sidecars/tts/podling_tts/server.py:174-184` (`server.shutdown()` waits for an in-flight request, up to 900 s).
- rustix is a workspace dependency with only the `process` feature — `Cargo.toml:49`.
- Fixture `--mode orphan` — `crates/podling-core/tests/fixtures/fake_sidecar.py:35-45`; the test helper `running(tag)` — `crates/podling-core/tests/sidecar_tts.rs:65`; the tree test — `:263`.

CONSUMERS (round 2):
- `Sidecar` is owned only by `SidecarTts` (`crates/podling-core/src/plugin/sidecar_tts.rs`). The new field and the stop change are internal: no public type, wire format, cache key or artifact changes.

Round 3 (tasks 8.7–8.10):
- `explain` reaps only the worker on death — `crates/podling-core/src/plugin/sidecar_tts.rs:236-239`; the pinned tree is signalled only in `stop()` — `crates/podling-core/src/plugin/sidecar.rs:302`.
- `is_running` — `sidecar.rs:494-501`; `Descendants::extend` — `sidecar.rs:421-428`; both fixture orphan modes start their child before ready — `crates/podling-core/tests/fixtures/fake_sidecar.py:36-50`; the timer — `:136-138`.
- CONSUMERS: none new. `Sidecar` and `Descendants` stay private to `SidecarTts`.

## Risk & rollback
- Unit 2 changes every sidecar TTS cache key once: cached audio is re-synthesised on the next run. This is intended and stated in the unit summary and the README.
- Unit 3 can refuse an existing user episode whose clip has another licence. The error names the allow-list, and that is the point of the hard rule.
- Unit 4 detaches the worker from the terminal's Ctrl-C. Podling's `Drop` still stops it, and SIGKILL of Podling falls back to `watch_parent` (task 4.5).
- Each unit reverts with `git revert` of its merge commit.

## Out of scope
- Plan tasks 1.4 and 10.3 of phase5-tts-audio (human listening passes) stay with the user.
- Unit 5, 6 and 8 Suggestions are registered PENDING and are not driven in this round.
- The stale docs/architecture.md lines (S-R2-8) are left to `/sync-docs`.
