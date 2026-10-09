---
slug: investigate-script-short-accepted
goal: A script that leaves most of the ledger unsaid, or speaks a claim id aloud, is rejected and retried instead of becoming the episode.
classification: in-scope   # /investigate 2026-10-07 inventory; blocks speech-acceptance sa-live (speech-acceptance.md 2.2)
tracker_rows: [TRACKER#investigate-script-short-accepted/isa-guard, TRACKER#investigate-script-short-accepted/isa-measure, TRACKER#investigate-script-short-accepted/isa-docs, TRACKER#investigate-script-short-accepted/isa-followups]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(trivial)   # code change is one file (script.rs) + its key pin; no exported type, schema, migration or security surface
coverage:
  contract:      N/A(`Script`, `ScriptInput` and the artifact schema are unchanged; only which replies the stage accepts)
  data:          N/A(no persistence change; the script cache key moves through `WriteScript::VERSION`, 1.6)
  config:        N/A(the floor is a constant, not an episode setting)
  security:      N/A(no authn/authz/secret surface; untrusted-data rule 4 unchanged)
  tests:         1.3, 1.4, 1.5, 1.6
  observability: 1.2 (the "script accepted" info log also carries coverage)
  interface:     N/A(no CLI or UI change; a failing stage already names its last rejection)
  docs:          1.7, 3.1, 3.2
  rollback:      git revert of the isa-guard merge (cache entries are keyed by VERSION, so old ones are simply unused)
units:
  - id: 1
    scope_id: isa-guard
    project: .
    depends_on: []
    module: podling-core script stage coverage floor + claim-id check
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/script_metrics.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-cli/tests/cli.rs
      docs:
        - docs/architecture/story.rules.md
        - docs/architecture/speech.rules.md
        - docs/architecture.md
        - docs/plans/investigate-script-short-accepted.md
      write:
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-cli/tests/cli.rs
        - docs/architecture.md
    arch: [ARCH-STORY-03, ARCH-STORY-04, ARCH-STORY-06, ARCH-STORY-11, ARCH-SPEECH-04, ARCH-SPEECH-11]
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer],
               skills: [], guards: [cargo fmt, cargo clippy --all-targets -- -D warnings, cargo test], mcp: [] }
  - id: 2
    scope_id: isa-measure
    project: .
    depends_on: [isa-guard]
    module: live script_eval with the floor
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/tests/script_eval_live.rs
        - examples/tunguska/episode-ollama.toml
        - examples/titanic/episode-ollama.toml
      docs:
        - docs/plans/story-driven-script.md
        - docs/plans/investigate-script-short-accepted.md
      write:
        - docs/plans/investigate-script-short-accepted.md
    arch: [ARCH-STORY-07]
    tooling: { implementer: implementer, gates: [],
               skills: [], guards: [cargo test], mcp: [] }
  - id: 3
    scope_id: isa-docs
    project: .
    depends_on: []
    module: correct the speech-acceptance "regression" record
    language: none
    security: normal
    scope:
      read: []
      docs:
        - docs/plans/phase5-tts-audio.md
        - docs/plans/speech-acceptance.md
        - docs/plans/investigate-script-short-accepted.md
      write:
        - docs/plans/phase5-tts-audio.md
        - docs/plans/speech-acceptance.md
    arch: []
    tooling: { implementer: implementer, gates: [],
               skills: [], guards: [], mcp: [] }
  - id: 4
    scope_id: isa-followups
    project: .
    depends_on: [isa-measure]
    module: "Follow-up: retry loops, sentence-number rejections, chatml template"
    language: none
    security: normal
    scope:
      read: []
      docs:
        - docs/plans/investigate-script-short-accepted.md
      write: []
    arch: []
    tooling: { implementer: implementer, gates: [],
               skills: [], guards: [], mcp: [] }
---

# Plan: reject short and id-speaking scripts (investigate-script-short-accepted)

## Outcome
A script that leaves most of the ledger unsaid, or speaks a claim id aloud, is rejected and retried instead of becoming the episode.

## Scope Steps (executable core)

### Step 1 — isa-guard (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer · guards cargo fmt, cargo clippy --all-targets -- -D warnings, cargo test
Depends on: none

- [x] 1.1 In `stages/script.rs`, add `check_coverage(metrics: &ScriptMetrics) -> Result<(), String>` and call it in `WriteScript::run`'s `complete_validated_with` closure after `build_script`, on `ScriptMetrics::of(&script, &input.ledger, &input.verdicts, input.target_minutes)`. It applies only when `usable_claims >= 2`. It rejects when `distinct_cited < usable_claims.div_ceil(2)` or `turns < 2`. The reason names both counts and tells the model to give each usable claim a turn, e.g. "the script cites 0 of 9 usable claims and has 1 turn; cite at least 5, each in its own turn". `build_script` itself is unchanged by this task, so its existing tests are untouched (ARCH-STORY-04) → accept: `cargo test -p podling-core stages::script` passes, with no edit to an existing test body.
- [x] 1.2 Reuse the closure's metrics for the "script accepted" `tracing::info!`, adding `coverage`, `distinct_cited` and `usable_claims` fields (ARCH-STORY-11) → accept: the info call names all three; no second `ScriptMetrics::of` for the accepted script.
- [x] 1.3 Regression test: `WriteScript::run` with a scripted LLM that always replies with the 2026-10-07 one-turn draft fails with `InvalidProviderOutput` after `SCRIPT_ATTEMPTS`, and the message names the coverage. The draft is one host turn, "Welcome to our show … Let's start with some eyewitness accounts.", with `citations: []`, against a ledger of 9 SingleSource claims → accept: the test fails on the parent commit (shown by reverting 1.1 locally and running it) and passes after.
- [x] 1.4 Boundary tests: a draft citing exactly `ceil(usable/2)` distinct usable claims in 2 turns is accepted; one claim fewer is rejected; a ledger with 1 usable claim accepts a one-turn script citing it; a ledger whose claims are all Unsupported is not floored → accept: the four tests pass.
- [x] 1.5 In `build_script`'s per-turn loop, reject a turn whose `text` contains any ledger claim id, or any run of 64 lowercase hex characters. Do this before placeholders are filled, so quoted source text is not affected. The reason is "turn N speaks a claim id in its text; put ids only in `citations`" → accept: a test with the 2026-10-07 turn text ("… According to one source, no impact crater was found. (citations: [27d97049c4916338b5f69ae47776487b32efb5dfa28f21eacf48433229aa8644])") is rejected, and the same text without the parenthesis is accepted; the test fails before the change.
- [x] 1.6 Bump `WriteScript::VERSION` to 14 with a history comment covering 1.1 and 1.5 (ARCH-STORY-03; `SCRIPT_PROMPT_VERSION` unchanged, since the instructions are not edited). Move the `script` key in `NO_NLI_KEYS` (`tests/pipeline.rs:709`), and in `crates/podling-cli/tests/cli.rs` only if it pins that key, updating the pin's history comment. `extract_claims` and the stages before `script` keep their keys. Move the `analyse` key only if the fake script's bytes change → accept: `cargo test --workspace` passes; `git diff` shows no change to any other pinned key; `FakeLlm` scripts pass the floor unchanged (its fingerprint `version` is not bumped).
- [x] 1.7 In `docs/architecture.md`'s script-stage section, beside "Every judged claim must be cited" (`:513`), add one sentence on the coverage floor and the claim-id check → accept: the sentence exists and cites `script.rs`.

### Step 2 — isa-measure (., rust, normal)
Tooling: implementer · guards cargo test
Depends on: isa-guard

Needs the native GPU Ollama on `127.0.0.1:11435` with `OLLAMA_CONTEXT_LENGTH=16384` and llama3.1:8b (`story-driven-script.md` "Baseline table" setup).
- [x] 2.1 Save fresh runs of `examples/{tunguska,titanic}/episode-ollama.toml` at the isa-guard merge commit. Run `PODLING_SCRIPT_EVAL_RUN=<run> cargo test -p podling-core --test script_eval_live -- --ignored script_eval` with N=5 on each. Record a **Guard table** below, in the Arc table's shape (commit sha, per-run rows, summary) → accept: the section holds real numbers, the commit sha, and per-run rejection reasons.
- [x] 2.2 Compare with `story-driven-script.md` "Arc table" (Tunguska eventual pass 3/5, Titanic 2/5) and state, per episode: eventual pass, mean attempts, mean coverage, and how many rejections were the new coverage or claim-id reasons → accept: a "Guard vs arc" paragraph with those numbers. If either episode's eventual pass is 2 or more runs below the Arc table, write "**Floor costs passes**" and leave tuning the floor to the user. Never change the constant in this unit.

### Step 3 — isa-docs (., none, normal)
Depends on: none
- [ ] 3.1 In `docs/plans/phase5-tts-audio.md`, change "Lexicon A/B (speech-acceptance, 2026-10-07): inconclusive" so that its *Why* bullet no longer says the script stage regressed at version 12. Say instead that llama3.1:8b sometimes writes a one- or two-turn script, measured in `story-driven-script.md` "Baseline table". The stage accepted it because it had no coverage check, and the same input replayed later wrote 9–12 turns naming Kulik. Point to this plan → accept: no sentence in the section calls it a regression or blames version 12.
- [ ] 3.2 In `docs/plans/speech-acceptance.md`, change the "Stopped 2026-10-07" note the same way. 2.2–2.4 and step 3 now wait for isa-guard, then run unchanged → accept: the note names `investigate-script-short-accepted` and no longer says "a regression".
- [ ] 3.3 Move tracker row `speech-acceptance/sa-live` to BLOCKED again (via `tracker.mjs`), with evidence naming this plan in place of the version-12 claim → accept: `tracker.mjs status` shows the new evidence.

### Step 4 — isa-followups (., none, normal) — follow-up row, not driven by this run
Depends on: isa-measure
Recorded so they are not lost. Each needs its own `/investigate` or `/plan`; none is mechanical:
- C2: after a rejection, retries often loop to the 6000-token cap. On 2026-10-07 20:25–20:36, attempt 1 was rejected with "source 1 has 1 sentences, so sentence 3 does not exist", and attempts 2 and 3 were both cut off at 6000 tokens: about 11 minutes, then the stage failed. The Arc table's Tunguska run 4 took 462 s.
- L1: sentence-number rejections make up most script failures (Baseline and Arc tables, and the run above). This is prompt and validation design on the story track; the act-writer decision is the user's (`story-driven-script.md` "ARCH-STORY-08 verdict").
- L2: native Ollama 0.35.1 starts llama-server with `--chat-template chatml --no-jinja` for llama3.1 (serve log at the time: `~/.claude/jobs/7ad40ba9/tmp/ollama-serve.log`). Its effect on script quality is unproven; an A/B against the Docker Ollama or a llama3 template would show it.

## Sequencing
1 → 2 (the floor must exist before it is measured). 3 is independent and can land with 1. 4 is a parking row. After 1–3, speech-acceptance sa-live reruns 2.2–2.4 unchanged.

## Verification background
- Observable (2026-10-07 15:51–15:55, native Ollama :11435, llama3.1:8b, temperature 0.2): three runs of the Tunguska audio episode were accepted on the first attempt with one turn (101–185 completion tokens), 0–1 citations of 8–9 usable claims, and neither Kulik nor Vanavara named. One run's turn ended "(citations: [27d97049…])", was synthesised, and scored 112 ‰, unverified — `speech-acceptance.md` "Stopped" note; `phase5-tts-audio.md` "Lexicon A/B".
- Regression refuted. Same binary, same input (cache copy with only the script entry `36dca260…` deleted), 5 replays: 9/11/11/12 turns naming Kulik, plus one validation failure. The captured version-12 request replayed 6/6 long. v10 (`d9b4678`), rebuilt and run, also wrote long scripts. Context was 16384 with `truncated = 0`. One-turn replies occurred on GPU offloads of 27/33, 26/33 and 0/33 layers, so offload is not the cause. Sampler parameters were identical. The `story-driven-script.md` "Baseline table" and "Arc table" record 2-turn, 37-word Tunguska scripts on the same server, 3/5 at version 12 and 3/5 at version 13.
- `build_script` checks citations, quotes, placeholders, judged claims and cast, but no length or coverage — `crates/podling-core/src/stages/script.rs:303-365`. Judged-claim check pattern: `script.rs:427-447`. The metrics are computed only after acceptance, for a log — `script.rs:195-210`.
- `ScriptMetrics { turns, distinct_cited, usable_claims, coverage, … }` — `crates/podling-core/src/script_metrics.rs:20-45`.
- No word-ratio floor: the Tunguska ledger caps a 30-minute target at about 1 minute — `phase5-tts-audio.md` "What a 30-minute target gives".
- The script key pin is at `crates/podling-core/tests/pipeline.rs:709`.
- Rules: ARCH-STORY-03 (bump VERSION with history), ARCH-STORY-04 (keep rules 1–5 and `check_judged_claims_are_cited`; existing script tests unchanged), ARCH-STORY-06 (metrics stay pure in `script_metrics.rs`), ARCH-STORY-07 (harness), ARCH-STORY-11 (info log) — `docs/architecture/story.rules.md:11-21`.

CONSUMERS: none (no shared contract changes; `Script` and the artifact schema are untouched; the only cross-file effect is the `script` cache key pin, task 1.6).

## Risk & rollback
- The floor may cost passes: a model that already fails about half its scripts now has one more way to fail. Step 2 measures this and leaves any tuning to the user.
- A legitimately short episode (a target of 1 minute on a large ledger) could need fewer than half the claims. Not seen in any recorded run; if it shows up, the floor becomes `target_minutes`-aware in a follow-up.
- Rollback: revert the isa-guard merge.

## Out of scope
Prompt changes (`SCRIPT_PROMPT_VERSION` stays 1); the act writer (ARCH-STORY-09/10, the user's decision); a word-ratio floor; C2, L1 and L2 (step 4).

## Guard table
The guard (`WriteScript::VERSION` 14, `SCRIPT_PROMPT_VERSION` 1), measured 2026-10-09 at commit
`3227adf` (isa-guard merged) with `script_eval` (N=5 per episode). Same model and server as the
Arc table: llama3.1:8b on the native GPU Ollama at `127.0.0.1:11435`
(`OLLAMA_CONTEXT_LENGTH=16384`), temperature 0.2. Requests went through a logging proxy, so every
rejection reason is known (each retry lists the earlier ones). Saved runs are fresh `podling run`s
of `examples/{tunguska,titanic}/episode-ollama.toml` at `3227adf`, NLI on. **The ledgers differ
from the Arc table's**: Tunguska has 9 claims (8 SingleSource, 1 Corroborated; the Arc had 7) and
Titanic has 11 (9 SingleSource, 1 Corroborated, 1 Contested with a verdict; the Arc had 10).
Target 5 minutes (750 words).

Rejections are listed in order; **bold** marks the new reasons (coverage floor, spoken claim id).

**Tunguska** — topic "the 1908 Tunguska explosion"

| run | ok | attempts | secs | words | word ratio | turns | quotes | citations | coverage | rejections |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | yes | 1 | 43 | 318 | 0.42 | 8 | 7 | 8 | 7/9 (0.78) | — |
| 2 | yes | 1 | 99 | 605 | 0.81 | 14 | 18 | 16 | 5/9 (0.56) | — |
| 3 | yes | 3 | 84 | 235 | 0.31 | 8 | 10 | 8 | 7/9 (0.78) | quote part does not exist; **cites 1 of 9, 1 turn** |
| 4 | no | 3 | 118 | — | — | — | — | — | — | quote part does not exist; **cites 4 of 9, 4 turns**; quote cites source 3 of 3 |
| 5 | yes | 2 | 51 | 261 | 0.35 | 7 | 6 | 8 | 6/9 (0.67) | **cites 1 of 9, 1 turn** |

Summary: eventual pass **4/5**, first-try pass 2/5, mean attempts 2.00, mean word ratio **0.47**
(passing runs), mean quotes 10.2, mean coverage **0.69**, unknown-citation rejections 0;
rejections 6, of which coverage **3**, claim id **0**.

**Titanic** — topic "how the 1912 Titanic inquiries disagreed"

| run | ok | attempts | secs | words | word ratio | turns | quotes | citations | coverage | judged cited | rejections |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | yes | 1 | 105 | 517 | 0.69 | 17 | 8 | 18 | 7/11 (0.64) | 1/1 | — |
| 2 | no | 3 | 580 | — | — | — | — | — | — | — | cut off at 8192 tokens; typed quotation; quote part does not exist |
| 3 | no | 3 | 635 | — | — | — | — | — | — | — | cut off at 8192 tokens; typed quotation; sentence 3 of a 3-sentence source |
| 4 | no | 3 | 309 | — | — | — | — | — | — | — | typed quotation (after an Ollama "token repeat limit" error, retried by the transport); contested claim not cited; **cites 3 of 11, 4 turns** |
| 5 | no | 3 | 615 | — | — | — | — | — | — | — | missing field `speaker`; cut off at 8192 tokens; typed quotation |

Summary: eventual pass **1/5**, first-try pass 1/5, mean attempts 2.60, mean word ratio **0.69**
(passing runs), mean quotes 8.0, mean coverage **0.64**, unknown-citation rejections 0, judged
Contested cited 1/1; rejections 12, of which coverage **1**, claim id **0**.

An earlier Tunguska pass at the same commit without the proxy (so without rejection reasons):
eventual pass 4/5, first-try 2/5, mean attempts 2.00, mean word ratio 0.29, mean coverage 0.86.

## Guard vs arc
- **Tunguska:** eventual pass 3/5 → **4/5**, mean attempts 2.80 → 2.00, mean coverage 0.29 →
  **0.69**, word ratio 0.06 → 0.47, turns 2 → 7–14. The floor rejected a one-turn, one-citation
  draft twice (runs 3 and 5), and both retries wrote a 7–8 turn script. That is the 2026-10-07
  failure caught and recovered. Every Arc-table pass was a 2-turn, 2/7 script the floor would now
  reject.
- **Titanic:** eventual pass 2/5 → **1/5**, mean attempts 2.60 → 2.60, mean coverage 0.55 →
  0.64. The floor was the final rejection in one run (run 4, 3 of 11 claims in 4 turns), after
  two other reasons. The other three failures never reached it: they were token-cap cut-offs,
  typed quotations and sentence numbers (C2 and L1 in step 4).
- **Claim-id check:** fired 0 times in 23 requests.
- **Verdict:** neither episode is 2 or more runs below the Arc table, so no "Floor costs passes"
  flag. Titanic's one-run drop is within N=5 noise, and the floor decided only one of its four
  failures. The ledgers differ from the Arc table's (9 vs 7 and 11 vs 10 claims), so this is not
  a same-input comparison. The constant was not changed.

