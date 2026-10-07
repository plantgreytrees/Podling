---
slug: story-followups
goal: Script coverage never exceeds 1.0 and reports cited Unsupported claims on their own, an API key sent over plain http to any non-loopback host (LAN included) warns, the pipeline and the eval harness build the script input through one constructor, and source ordering is tested for claims with evidence in several chunks.
idea: docs/ideas/story-driven-script.md
classification: in-scope   # round-2 /scrutinise Suggestions on story-driven-script (main b794309); docs/architecture/story.rules.md ARCH-STORY-03/06, privacy.rules.md ARCH-PRIVACY-01/06, speech.rules.md ARCH-SPEECH-07 (audio derived from [tts]; governs script.rs, pipeline.rs)
tracker_rows: [TRACKER#story-followups/sfu-story, TRACKER#story-followups/sfu-privacy]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(config:never)
coverage:
  contract:      1.1 (ScriptMetrics gains pub cited_unsupported; coverage numerator narrowed), 1.4 (new pub ScriptInput::for_episode; ScriptInput serialisation unchanged); consumers in CONSUMERS
  data:          N/A(no persistence or migration; ScriptMetrics is not cached; ScriptInput's serialised fields are unchanged, so no cache key moves and no WriteScript::VERSION bump — ARCH-STORY-03 covers stage logic, which does not change)
  config:        N/A(no new config key; [llm]/[embedding] base_url parsing unchanged)
  security:      2.1-2.3 (the plain-http API-key warning covers private LAN hosts; Reach policy gate, max_redirects(0) and credential rejection untouched — ARCH-PRIVACY-01/06)
  tests:         1.2, 1.3, 1.6, 2.2, 2.3
  observability: 2.1 (warning fires for every non-loopback plain-http host with a key; reworded "non-loopback")
  interface:     1.3 (eval harness table gains a cited-Unsupported column)
  docs:          1.7 (one note in docs/plans/story-driven-script.md that its coverage figures predate the numerator fix)
  rollback:      git revert of each unit's merge commit; nothing persisted depends on either change
units:
  - id: 1
    scope_id: sfu-story
    project: .
    depends_on: []
    module: script metrics, ScriptInput constructor, source-order test
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/script_metrics.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/script_eval_live.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-types/src/episode.rs
        - docs/plans/story-driven-script.md
      docs:
        - docs/architecture/story.rules.md
        - docs/architecture/speech.rules.md
      write:
        - crates/podling-core/src/script_metrics.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/script_eval_live.rs
        - docs/plans/story-driven-script.md
    arch: [ARCH-STORY-03, ARCH-STORY-06, ARCH-SPEECH-07]
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: sfu-privacy
    project: .
    depends_on: []
    module: plain-http API-key warning
    language: rust
    security: sensitive
    scope:
      read:
        - crates/podling-core/src/plugin/http.rs
      docs:
        - docs/architecture/privacy.rules.md
      write:
        - crates/podling-core/src/plugin/http.rs
    arch: [ARCH-PRIVACY-01, ARCH-PRIVACY-06]
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: story-followups

## Outcome
The four non-blocking Suggestions from the story-driven-script round-2 `/scrutinise` are closed. Script coverage is distinct cited *usable* claims over usable claims, so it never exceeds 1.0, and cited Unsupported claims are counted on their own. A configured API key sent over `http://` to any host other than localhost or a loopback literal logs a warning, including private LAN hosts. `pipeline.rs` and the eval harness share one `ScriptInput` constructor. The source-order test covers a claim whose evidence spans an earlier and a later chunk.

## Scope Steps (executable core)

### Step 1 — sfu-story (., rust, normal)
Background: `ScriptMetrics::of` (script_metrics.rs:42) counts `distinct_cited` over every cited claim the ledger holds, but `usable_claims` excludes Unsupported, so `coverage` can exceed 1.0 (ARCH-STORY-06, story.rules.md:16). `ScriptInput` is built field by field at pipeline.rs:206 and script_eval_live.rs:166. `in_source_order` (script.rs:187) takes the `.min()` chunk index over a claim's evidence, but its test (script.rs:1335) gives each claim one chunk.

- [x] 1.1 In `ScriptMetrics::of`, count `distinct_cited` only over cited claims whose ledger status is not `Unsupported`, and add `pub cited_unsupported: usize` (distinct cited claims the ledger holds as Unsupported) with a doc comment. `coverage` stays `ratio(distinct_cited, usable_claims)` → accept: `distinct_cited + cited_unsupported` equals the distinct cited ids the ledger holds.
- [x] 1.2 Add a unit test in script_metrics.rs: a ledger with one usable and one Unsupported claim, and a script citing both → accept: `distinct_cited == 1`, `cited_unsupported == 1`, `usable_claims == 1`, `coverage == 1.0` (≤ 1.0); the existing `unsupported_claims_are_not_usable` and `repeated_citations_count_once_toward_coverage` pass unchanged or with assertions updated only for the new field.
- [x] 1.3 In script_eval_live.rs, show `cited_unsupported` as its own column in the per-run table row (line ~289) and its header → accept: `cargo test -p podling-core --test script_eval_live --no-run` compiles; the header and row have the same column count.
- [x] 1.4 Add `pub fn for_episode(spec: &EpisodeSpec, ledger: Ledger, verdicts: Verdicts, chunks: Vec<Chunk>, documents: Vec<Document>) -> ScriptInput` beside `ScriptInput` in stages/script.rs, taking `topic`, `target_minutes`, `cast` (mapped to `Speaker`) and `audio = spec.tts.is_some()` from `spec` → accept: doc comment says the pipeline and the eval harness share it.
- [x] 1.5 Replace the struct literal at pipeline.rs:206 with `ScriptInput::for_episode(&spec, ledger, verdicts, claim_input.chunks, documents)`, and `Saved::input` (script_eval_live.rs:166) with `for_episode` on clones followed by `input.topic = topic.to_owned()` → accept: `grep -n "ScriptInput {" crates/podling-core/src/pipeline.rs crates/podling-core/tests/script_eval_live.rs` finds nothing; `without_nli_the_cache_keys_are_unchanged` (tests/pipeline.rs:727) passes, so the script key is unchanged; `WriteScript::VERSION` is not bumped.
- [x] 1.6 Extend `the_request_lists_claims_in_the_order_the_sources_tell_them` (or add a sibling test) with a claim whose evidence is in both `first` and `second`, and whose id sorts after `earlier`'s → accept: it is ordered by its earliest chunk (with `earlier`, by id) ahead of every claim seen only in `second`; the test fails if `.min()` is replaced by `.max()`.
- [x] 1.7 Add one line to docs/plans/story-driven-script.md's Notes from execution: its coverage figures were computed before story-followups narrowed the numerator to usable claims → accept: the note exists; the tables are not rewritten.

### Step 2 — sfu-privacy (., rust, sensitive)
Background: http.rs:115 warns about a plain-http key only when `reach == Reach::Hosted`, and `Reach::of` (http.rs:387) counts private LAN literals as Local, so a key sent to `http://192.168.1.2` is silent. `Reach` must stay the policy gate (ARCH-PRIVACY-01).

- [x] 2.1 Add a private `fn is_loopback(host: &str) -> bool` (`localhost`, case-insensitive; IPv4 127/8; IPv6 `::1`) using the existing `host()` extraction, and warn when `api_key.is_some() && !base_url.starts_with("https://") && !is_loopback(host(&base_url))`; reword the message to "non-loopback host" → accept: `Reach::of`, the hosted refusal (http.rs:83), `max_redirects(0)` and the credentials-in-URL rejection are unchanged (ARCH-PRIVACY-06).
- [x] 2.2 Unit-test the predicate: loopback = `localhost`, `LOCALHOST`, `127.0.0.1`, `127.5.0.1`, `::1` (from `http://[::1]:8080/v1` via `host()`); not loopback = `10.0.0.5`, `172.16.0.1`, `192.168.1.2`, `fd00::1`, `localhost.example.com`, `127.0.0.1.nip.io`, `api.together.xyz` → accept: test passes.
- [x] 2.3 Keep `only_localhost_and_loopback_or_private_addresses_are_local` and the ARCH-PRIVACY-06 tests green unchanged → accept: `cargo test -p podling-core plugin::http` passes with those tests untouched.

## Sequencing
The two units share no files and have no dependency; run sfu-story then sfu-privacy (either order works).

## Verification background

Whole-plan gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass on the merged base.

CONSUMERS:
- `ScriptMetrics` (struct; new field `cited_unsupported`, narrowed `distinct_cited`/`coverage`):
  - crates/podling-core/src/stages/script.rs:166 — reads `word_ratio`, `words` only; unaffected.
  - crates/podling-core/tests/script_eval_live.rs:27, 234, 250, 295-297 (table row), 328-350 (`mean_coverage`) — gains the column (1.3).
  - No struct-literal construction outside script_metrics.rs.
- `ScriptInput` (new constructor `for_episode`; fields unchanged):
  - crates/podling-core/src/pipeline.rs:206 — switches to `for_episode` (1.5).
  - crates/podling-core/tests/script_eval_live.rs:166 — switches to `for_episode` + topic override (1.5).
  - crates/podling-core/src/stages/mod.rs:36 — re-export; unchanged.
  - crates/podling-core/src/stages/script.rs tests (811, 835, 917, 986, 1003, 1295, 1370, 1436) — literals with hand-built fixtures; left as is.
- `Reach::of` / http.rs warning: internal to http.rs (82, 83, 115, tests 463/474); no outside consumer.

## Risk & rollback
- Coverage figures recorded in docs/plans/story-driven-script.md are not directly comparable after 1.1 (noted by 1.7).
- 2.1 sits next to `max_redirects(0)` (http.rs:127); ARCH-PRIVACY-06 must not move.
- Rollback: revert either unit's merge commit; no cache, schema or persisted state depends on them.

## Out of scope
- The act writer (ARCH-STORY-08, tripped; "land as is").
- The hosted Together measurement (waits on `TOGETHER_API_KEY`).
- Rewriting the existing `ScriptInput` literals in script.rs's unit tests.
