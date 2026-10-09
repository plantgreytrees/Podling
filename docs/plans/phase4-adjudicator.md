---
slug: phase4-adjudicator
goal: When two independent sources disagree, the episode says so and explains the disagreement from the sources, using a verdict an LLM adjudicator writes for each Contested claim without changing its status.
classification: in-scope   # .claude/CLAUDE.md "Grounding": "only Contested claims go to an LLM adjudicator"; docs/handoff.md "Goal"; docs/architecture.md:166 names it the next phase
tracker_rows: [TRACKER#phase4-adjudicator/1, TRACKER#phase4-adjudicator/2, TRACKER#phase4-adjudicator/3, TRACKER#phase4-adjudicator/4, TRACKER#phase4-adjudicator/5]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(root-only agent mode: agent-mode-guard blocks non-strategist Task; the plan-strategist pass supplied the independent decomposition, see "Decomposition")
coverage:
  contract:      1.1–1.4 (new `Verdict`/`Verdicts` types, `ArtifactKind::Verdicts`, schema snapshot, SCHEMA_VERSION 3→4), 2.5–2.7 (pipeline writes verdicts.json; every STAGES/ArtifactKind consumer updated), 3.1–3.3 (`LedgerClaim` gains `verdict`, PROMPT_VERSION 3→4, `ScriptInput` gains `verdicts`)
  data:          N/A(no database; the new stage id and the SCHEMA_VERSION bump only miss the content cache, which treats foreign versions as misses: docs/architecture.md "Cache key")
  config:        N/A(no new episode keys: the adjudicator uses the episode's `[llm]`; it only has work when `[embedding]`/`[nli]` produce Contested claims). 4.2 wires the live example's episode-ollama.toml
  security:      2.2–2.3 (claim/evidence text from untrusted sources goes in the request data only, never the instructions; replies validated: claim id, cited indices, sides; the explanation may hold no quotation marks, so the adjudicator can't quote), 2.4 (transport errors fail the stage and are never cached as verdicts)
  tests:         1.5, 2.8–2.12, 3.4–3.6, 4.3, 5.1–5.4
  observability: 2.4 (info log: contested, calls, verdicts by side, fallbacks; warn per fallback with the rejection reason, never the source text)
  interface:     2.5 (CLI stage table gains `adjudicate`; `verdicts.json` written next to the other artifacts; `schema export` writes verdicts.schema.json)
  docs:          5.5 (architecture.md), 5.6 (README), 5.7 (handoff.md)
  rollback:      git revert of the branch merge. The no-[nli] artifact bodies are unchanged (test 2.9), so reverting only drops verdicts.json and the stage row
units:
  - id: 1
    scope_id: verdict-contracts
    project: .
    depends_on: []
    module: crates/podling-types
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-types/src/claim.rs
        - crates/podling-types/src/ledger.rs
        - crates/podling-types/src/document.rs
        - crates/podling-types/src/ids.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/schema.rs
        - crates/podling-types/src/lib.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-types/src/verdict.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/schema.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-types/tests/snapshots/schema_snapshot__verdicts.snap
    tooling: { implementer: implementer, gates: [code-reviewer, api-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: adjudicate-stage
    project: .
    depends_on: [verdict-contracts]
    module: crates/podling-core/src/stages/adjudicate.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/stage.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/src/error.rs
        - crates/podling-core/src/plugin/mod.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-core/src/stages/adjudicate.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-cli/tests/cli.rs
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor, idiom-reviewer, observability-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: script-integration
    project: .
    depends_on: [adjudicate-stage]
    module: crates/podling-core/src/stages/script.rs
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/stages/adjudicate.rs
        - crates/podling-types/src/verdict.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/stages/script.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/pipeline.rs
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 4
    scope_id: titanic-example
    project: .
    depends_on: [script-integration]
    module: examples/titanic
    language: rust
    security: normal
    scope:
      read:
        - examples/tunguska/episode.toml
        - examples/tunguska/episode-ollama.toml
      docs: [README.md]
      write:
        - examples/titanic/episode.toml
        - examples/titanic/episode-ollama.toml
        - examples/titanic/sources/us-senate/report.md
        - examples/titanic/sources/british-inquiry/report.md
        - examples/titanic/SOURCES.md
        - crates/podling-cli/tests/cli.rs
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 5
    scope_id: live-and-docs
    project: .
    depends_on: [titanic-example]
    module: docs
    language: markdown
    security: normal
    scope:
      read:
        - crates/podling-core/src/stages/adjudicate.rs
        - crates/podling-types/src/verdict.rs
        - crates/podling-core/src/pipeline.rs
      docs: [docs/architecture.md, README.md, docs/handoff.md]
      write:
        - docs/plans/phase4-adjudicator.md
        - docs/architecture.md
        - README.md
        - docs/handoff.md
    tooling: { implementer: implementer, gates: [docs-curator],
               skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: Contested-claim adjudicator

## Outcome
When two independent sources disagree, the episode says so and explains the
disagreement from the sources. The adjudicator writes one verdict per Contested
claim in `verdicts.json`. It never changes the claim's status, and the script
stage reads the verdict.

## Scope Steps (executable core)

### Step 1 — verdict-contracts (., rust, normal)
Tooling: implementer · gates code-reviewer, api-reviewer, idiom-reviewer · skills language-aware-planning
Depends on: none
- [x] 1.1 Add `crates/podling-types/src/verdict.rs`:
  - `Favours { Supporting, Contradicting, Unresolved }`, snake_case.
  - `EvidenceRef { chunk: ChunkId, stance: Stance, premise: Option<TextSpan> }`. `premise` is skipped when `None`. It names one `Evidence` of the claim: `premise` is the `EvidenceBasis::Nli` span, and `None` means the evidence has no span.
  - `Verdict { claim: ClaimId, favours: Favours, explanation: String, cites: Vec<EvidenceRef>, fallback: Option<String> }`, built through `Verdict::new(..) -> Result<Self, InvalidVerdict>`, with deserialisation going through the same check (`try_from` a raw struct).

  Invariants:
  - `explanation` is non-empty, at most `MAX_EXPLANATION_CHARS` (600), and contains no `"`, `“` or `”`.
  - `Supporting` cites at least one `Supports` ref, and `Contradicting` cites at least one `Contradicts` ref.
  - `fallback.is_some()` ⇒ `favours == Unresolved`.
  - `cites` is non-empty, sorted and de-duplicated.
  → accept: unit tests build each invalid shape and get `Err`; the valid ones round-trip through serde.
- [x] 1.2 Add `Verdicts(Vec<Verdict>)`. It serialises as a bare JSON array (`#[serde(try_from = "Vec<Verdict>", into = "Vec<Verdict>")]`); `Verdicts::new` sorts by claim id and rejects a duplicate claim. Export from `lib.rs`. → accept: `Verdicts::default()` serialises as `[]`; two verdicts for one claim → `Err`.
> **Amended during execution:** 1.3 and 1.4 moved into step 2, where the pipeline
> starts writing `verdicts.json`. Adding the kind on its own leaves the pipeline
> and CLI tests that iterate `ArtifactKind::ALL` red until a file is written,
> and no unit merges red. Step 1 merged the types and their tests (1.1, 1.2, 1.5).
>
> **Ticked 2026-10-09 by /sync-docs:** steps 1–2 shipped in cd352ce (stage 3876528);
> 2.9 rests on 3876528's byte-for-byte claim plus the 2.6 golden test.
- [x] 1.3 `ArtifactKind::Verdicts` (`as_str` = `verdicts`), placed after `Ledger` in `ALL`; `schema::of` → `enveloped::<Verdicts>()`. → accept: `schema::all()` has 8 kinds.
- [x] 1.4 `SCHEMA_VERSION` 3→4. Add `schema_snapshot__verdicts.snap` and accept it with insta. Pin version 4 in `schema_version_is_pinned`. → accept: `cargo test -p podling-types` is green, and the only new snapshot is verdicts.
- [x] 1.5 Round-trip test of `Envelope<Verdicts>` in `tests/roundtrip.rs`. → accept: passes.

### Step 2 — adjudicate-stage (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor, idiom-reviewer, observability-reviewer · skills language-aware-planning
Depends on: verdict-contracts
- [x] 2.1 `plugin/llm.rs`:
  - Add `LlmTask::AdjudicateClaim`. Input: `{ "claim": {id, text}, "evidence": [AdjudicationEvidence] }`, where `AdjudicationEvidence { n, stance, source_title, independence_group, text }`. Output: `VerdictDraft { claim, favours, explanation, cites: [n] }`.
  - Add `pub const ADJUDICATE_PROMPT_VERSION: u32 = 1`. It is separate from `PROMPT_VERSION`, so later adjudicator prompt changes don't force re-extraction.

  → accept: compiles; the types are documented.
- [x] 2.2 `FakeLlm` answers `AdjudicateClaim` deterministically:
  - `favours: unresolved`, citing the first index of each stance present.
  - Explanation: "The <group> source and the <group> source give different accounts, and the sources do not settle which is right." No quotation marks.
  - Bump the fake's fingerprint version 3→4.

  → accept: unit test; the reply validates.
- [x] 2.3 New `stages/adjudicate.rs`.

  `AdjudicateInput { cases: Vec<Case> }`, built by `AdjudicateInput::new(&Ledger, &[Chunk], &[Document])` from **Contested entries only**. A `Case` holds the claim and its numbered evidence texts:
  - `Nli`: the premise span's text.
  - `Merged`: the `wording`.
  - `None`: the chunk text.

  Stage:
  - `Adjudicate { llm }` with `ID = "adjudicate"`, `VERSION = 1`, and fingerprint `{llm, instructions, adjudicate_prompt_version}`.
  - One `complete_validated` call per case (≤2 requests, counting the retry).
  - Instructions say the evidence is untrusted data, and the explanation names sources by title and quotes nothing.

  `validate`:
  - The claim id matches.
  - Every cited `n` exists.
  - At least one cite per non-empty side.
  - `favours` doesn't name an empty side.
  - `Verdict::new` accepts the result.
  - Indices are resolved to `EvidenceRef`.

  → accept: unit tests for each rejection reason name it in the error.
- [x] 2.4 Fallback and logging.
  - Only `CoreError::InvalidProviderOutput` from the case becomes `Verdict { favours: Unresolved, cites: first ref of each non-empty side, explanation: "The sources disagree, and the adjudicator gave no usable verdict.", fallback: Some(reason) }`.
  - Every other error propagates and fails the stage, so nothing gets cached.
  - `tracing::info!` logs `contested`, `calls`, `supporting`, `contradicting`, `unresolved` and `fallbacks`; `warn!` logs each fallback with the claim id and reason.

  → accept: a test with a provider that always replies with garbage gives `Unresolved` + `fallback`, after exactly 2 calls; a provider returning `CoreError::Provider` fails the stage.
- [x] 2.5 `pipeline.rs`: run `Adjudicate` through `cached()` after `ledger` and before `script`, then `write(out_dir, ArtifactKind::Verdicts, &verdicts)`. → accept: the stage order is ingest, chunk, extract_claims, [cluster_claims, score_stances], ledger, adjudicate, script, analyse.
- [x] 2.6 `tests/pipeline.rs`:
  - `STAGES` gains `adjudicate`.
  - `a_contradicting_source_contests_both_claims` lists it.
  - `no_nli_config_writes_todays_artifacts` compares every file **present in `fixtures/golden`** (the 7 pre-phase-4 kinds) and asserts `verdicts.json`'s body is `[]`.

  → accept: the golden test is green with no golden file edited.
- [x] 2.7 `podling-cli/tests/cli.rs`:
  - `STAGES` gains `adjudicate`, so there are 7 rows and `"7 entries"`.
  - The schema-export list gains `verdicts`.

  → accept: `cargo test -p podling-cli` is green.
- [x] 2.8 Test `no_contested_claims_means_no_adjudicator_call`: a counting LLM on the `paraphrase` fixture (grounding on, no Contested claims) and on the default fixture (no grounding) records zero `AdjudicateClaim` requests, and `verdicts.json` is `[]`. → accept: passes.
- [x] 2.9 Golden byte check outside the test suite: run `podling run --no-cache` for `tests/fixtures/episode.toml` and `examples/tunguska/episode.toml`, then diff the bodies against `$CLAUDE_JOB_DIR/tmp/golden/{fixtures,tunguska}`. Only `schema_version` lines may differ. → accept: the diff is empty apart from those lines.
- [x] 2.10 Test `contradiction_fixture_has_one_verdict_per_contested_claim`: the verdicts' claim ids equal the ledger's Contested claim ids, and every verdict cites both sides. → accept: passes.
- [x] 2.11 Test `a_cached_run_makes_no_adjudicator_call`: a second run with the same cache is a hit for `adjudicate`. → accept: passes.
- [x] 2.12 Test that injected source text never reaches the adjudicator's `instructions`. → accept: passes.

### Step 3 — script-integration (., rust, high)
Tooling: implementer · gates code-reviewer, idiom-reviewer · skills language-aware-planning
Depends on: adjudicate-stage
- [x] 3.1 `LedgerClaim` gains `verdict: Option<LedgerVerdict { favours: Favours, explanation: String }>`, skipped when `None`. `LedgerClaim::from_ledger(&Ledger, &Verdicts)` fills it for Contested claims. The refs stay out, for the same reason the evidence does. → accept: compiles.
- [x] 3.2 `PROMPT_VERSION` 3→4 with a doc line (4: a Contested claim's ledger entry carries the adjudicator's verdict). `WriteScript::VERSION` 7→8. → accept: the constants are bumped.
- [x] 3.3 `ScriptInput` gains `verdicts: Verdicts`, and `pipeline.rs` passes it. INSTRUCTIONS rule 2 says a `contested` claim with a `verdict`:
  - Present both sources' accounts.
  - Say which side the sources favour, or that it is unresolved.
  - Explain why from the verdict's explanation.
  - Never state either side as settled fact.

  → accept: compiles.
- [x] 3.4 Pin the shape: extend `the_ledger_the_model_sees_has_no_evidence`. The keys are `{id,status,text}` without a verdict, and `{id,status,text,verdict}` with one; the verdict's keys are `{explanation,favours}`. → accept: passes.
- [x] 3.5 `FakeLlm::write_script`: when a Contested entry has a verdict, the turn reads "The sources disagree here: <claim>. <explanation>". → accept: the contradiction-fixture script contains "disagree" and each verdict's explanation; no-verdict scripts are unchanged (golden test still green).
- [x] 3.6 Pipeline test `the_contradiction_script_mentions_the_disagreement`. → accept: passes.

### Step 4 — titanic-example (., rust, normal)
Tooling: implementer · gates code-reviewer
Depends on: script-integration
- [x] 4.1 `examples/titanic/sources/{us-senate,british-inquiry}/report.md`: short **verbatim** excerpts from the 1912 US Senate inquiry report and the 1912 British Wreck Commissioner's report.
  - Both are public domain: a US federal work, and a UK Crown publication whose 50-year term ended in 1962.
  - The two pairs that disagree: the collision time ("At 11.46 p.m. ship's time" against "a little before 11.40") and the survivors ("706 were saved" against "he took on board 712 persons").
  - Page and witness references in parentheses may be removed, and each removal is noted.

  `SOURCES.md` gives the URLs and the licence reasoning. → accept: every sentence in report.md appears in the fetched page text, after normalising whitespace.
- [x] 4.2 `episode.toml` (fake LLM) and `episode-ollama.toml` (llama3.1:8b, nomic-embed-text, cross_encoder `models/nli-deberta-v3-base`), modelled on Tunguska. → accept: `every_example_episode_parses` covers both.
- [x] 4.3 Run the fake episode offline (`podling run --episode examples/titanic/episode.toml`). → accept: exit 0; no error findings.

### Step 5 — live-and-docs (., markdown, normal)
Tooling: implementer · gates docs-curator
Depends on: titanic-example
- [x] 5.1 Two cold-cache runs of `examples/titanic/episode-ollama.toml` (fresh `--cache-dir` each time). → accept: each run meets all of these:
  - exit 0
  - `analysis.json` has no `error`
  - at least 1 Contested claim, each with a verdict
  - script.json names the disagreement

  Record the results under "Live results".
- [x] 5.2 One cold-cache run of `examples/tunguska/episode-ollama.toml`. → accept: exit 0, no `error`, and `verdicts.json` is consistent with its Contested count.
- [x] 5.3 `PODLING_LIVE_LLM_URL=http://localhost:11434/v1 PODLING_LIVE_LLM_MODEL=llama3.1:8b cargo test -p podling-cli -- --ignored`. → accept: passes.
- [x] 5.4 The fmt, clippy (`-D warnings`) and test workspace gates. → accept: all three pass.
- [x] 5.5 `docs/architecture.md`: update the artifact flow, add an "Adjudicating Contested claims" section (cost bound, fallback, caching, no quoting), the bump rules (ADJUDICATE_PROMPT_VERSION), and drop "next phase" at :166. → accept: docs-curator check; every cited path resolves.
- [x] 5.6 README: the artifact list, the stage list and the Titanic example. → accept: matches the code.
- [x] 5.7 `docs/handoff.md`: rewrite it for the next goal (NLI entailment in `is_grounded`). → accept: written.
- [x] 5.8 Live fix: cap each verdict reply (`CompletionRequest::max_tokens`, `MAX_VERDICT_TOKENS` = 512; the OpenAI-compatible provider sends the lower of it and `max_output_tokens`); `Adjudicate::VERSION` 2→3. Live, llama3.1:8b in JSON mode wrote 13,000+ tokens for one verdict. → accept: tests that the provider sends the lower cap.
- [x] 5.9 Live fix: the script request numbers its sources (`SourceText::source`, `QuoteRef { source, sentence }`) instead of showing chunk ids, which llama3.1:8b cited as claims on both attempts. `PROMPT_VERSION` 7→8 (ARCH-SPEECH-16), `WriteScript::VERSION` 11→12, fake 7→8. → accept: a test for an out-of-range source; no-NLI artifact bodies unchanged.
- [x] 5.10 Live fix: a script must cite every Contested claim that has a verdict (rule 2, checked by `build_script` with a rejection that names the missing claim), since a live script left both judged claims out. → accept: a unit test that a script missing a judged claim is rejected with its id.
- [x] 5.11 Live fix: the script stage gets three attempts (`SCRIPT_ATTEMPTS`), and each retry lists every earlier rejection (each cut by `reason_excerpt`). Live, every retry fixed the reported error and made a new one (source numbered from 1, then a sentence past the end; a missing judged claim, then a typed quotation). Extraction and adjudication keep two attempts, so the adjudicator's cost bound is unchanged. Folded into `WriteScript::VERSION` 12, which has not landed. → accept: a unit test that a third reply is accepted after two rejections and that the third request lists both reasons; a test that two-attempt stages still stop at two.
- [x] 5.12 Live fix: a quotation inside a sentence can be quoted by number. Each numbered sentence lists the quotations in it (`NumberedSentence::quoted`, by `text::quotation_ranges`, the spans the typed-quote check finds), and `QuoteRef` gains an optional `part`; the stage still copies the words from the document. A typed quotation that is a listed part is rejected with a hint naming its source, sentence and part. Live, on 3 of 4 cold Titanic runs llama3.1:8b typed the lookout's "Iceberg right ahead.", which sits inside a longer sentence, on every attempt. Folded into the unlanded `PROMPT_VERSION` 8 and `WriteScript::VERSION` 12. → accept: unit tests that a `part` resolves to the quotation's document span without its marks, that an out-of-range part is rejected, and that the typed-quote rejection names the part.
- [x] 5.13 Live fix: every LLM request is capped, not only the verdict. `MAX_CLAIMS_TOKENS` = 2048 per extraction reply, `MAX_SCRIPT_TOKENS` = 8192 per script reply (the provider still sends the lower of these and `max_output_tokens`). Live, an extraction reply ran past 23,000 tokens and would have held the GPU until the 1800 s timeout. `ExtractClaims::VERSION` 5→6; folded into the unlanded `WriteScript::VERSION` 12. → accept: tests that the extraction and script requests carry their caps.
- [x] 5.14 Live fix: a reply cut off at the token limit is a rejection, not a provider failure. The OpenAI-compatible provider reports `finish_reason: "length"` as `ProviderFailure::CutOff`, and `complete_validated_with` turns that into a rejection reason, so it is retried and the adjudicator falls back; the CLI gives it no hint. Live, a capped extraction reply was cut off and failed the run with no retry, contradicting 5.8 and 5.13. → accept: a unit test that a cut-off first reply is retried and the second accepted; a test that the provider maps `length` to `CutOff`.

## Sequencing
Contracts → stage → script → example → live/docs. Each step needs the types or stage before it. The example needs the script integration to show verdicts.

## Decomposition
plan-strategist compared three options:
- (A) An always-on `adjudicate` stage, cached once per run on the Contested cases only, always writing `verdicts.json`.
- (B) A per-claim cached sub-stage that runs only with grounding.
- (C) Verdicts stored on `LedgerEntry`.

It recommended A, and this plan adopts it:
- A keeps one row per stage id and one file per `ArtifactKind`.
- Because the input is only the Contested cases, an edit elsewhere doesn't rerun the adjudicator.
- C would mix LLM output into the deterministic ledger, whose deserialiser re-checks `classify`.

The plan diverges from it in one place: the golden test compares only the pre-existing files, instead of adding a `verdicts.json` golden, since that golden would claim pre-change provenance it doesn't have.

## Verification background
- Status is classify-only, and the ledger deserialiser re-checks it — `crates/podling-types/src/ledger.rs:31-55,79-90`
- `Contested.supporting` may be empty — `crates/podling-types/src/ledger.rs:21-24`
- `Evidence.basis` is None for plain extraction, Nli carries the premise span — `crates/podling-types/src/claim.rs:24-56`
- `complete_validated` makes 2 calls at most and returns `InvalidProviderOutput` after the second — `crates/podling-core/src/plugin/llm.rs:147-173`
- `PROMPT_VERSION` is in the extract_claims and script cache keys — `crates/podling-core/src/plugin/llm.rs:10-18`
- The script ledger view is pinned to `{id,status,text}` — `crates/podling-core/src/stages/script.rs:665-697`
- The golden test iterates `ArtifactKind::ALL` — `crates/podling-core/tests/pipeline.rs:85-106`
- The pipeline's stage order and artifact writes — `crates/podling-core/src/pipeline.rs:55-128`

CONSUMERS:
- `ArtifactKind` / `ArtifactKind::ALL`:
  - `crates/podling-types/src/schema.rs:17,28-34`
  - `crates/podling-core/src/pipeline.rs:124-130`
  - `crates/podling-core/tests/pipeline.rs:72,96`
  - `crates/podling-cli/src/commands.rs:13` (schema export)
  - `crates/podling-cli/tests/cli.rs:139-170` (file list)
  - `crates/podling-types/tests/roundtrip.rs:88-93`
- `SCHEMA_VERSION`: `crates/podling-types/tests/schema_snapshot.rs:19`; `crates/podling-core/tests/pipeline.rs:75,95`
- `LedgerClaim` / `from_ledger`: `crates/podling-core/src/stages/script.rs:72`; `crates/podling-core/src/plugin/llm.rs:201,364` (FakeLlm + test)
- `ScriptInput`: `crates/podling-core/src/pipeline.rs:93-99`; `crates/podling-core/src/stages/script.rs:401-409,678-681`
- Stage lists: `crates/podling-core/tests/pipeline.rs:14,474-498,555-560`; `crates/podling-cli/tests/cli.rs:14,98-111`
- `FakeLlm` fingerprint: `crates/podling-core/src/plugin/llm.rs:291-295`

## Risks
- llama3.1:8b may cite indices badly, which shows up as a high fallback rate. The goal says to stop and ask if most verdicts fall back.
- The NLI may not reach `CONTRADICT_PM` = 950 on the Titanic pairs. If so, choose different verbatim sentences from the same reports; never edit the sources or the thresholds to force a contest.
- The prompt is bigger when a basis-None evidence carries its whole chunk. The example chunks are small.

## Live results
Recorded 2026-10-06 on commit `7245b32`. The runs used llama3.1:8b (Q4_K_M) on a native Ollama 0.35.1 on the GPU (RTX 5060, `OLLAMA_CONTEXT_LENGTH=16384`), at 127.0.0.1:11435. The Docker Ollama on 11434 that `episode-ollama.toml` names ran on the CPU at 0.1–0.3 tok/s and timed out. The configs differed from `episode-ollama.toml` only in `base_url`. Each run used a fresh `--cache-dir`.

| Run | Exit | Claims | Contested | Verdicts | Fallbacks | `error` findings | Turns naming the disagreement | Time |
|---|---|---|---|---|---|---|---|---|
| Titanic (titanic11) | 0 | 20 | 4 | 4 | 2 | 0 | 3 | 168 s |
| Titanic (titanic12) | 0 | 13 | 2 | 2 | 0 | 0 | 2 | 94 s |
| Tunguska (tunguska2) | 0 | 9 | 0 | 0 (consistent) | 0 | 0 | — | 43 s |
| Ignored live CLI test | pass | | | | | | | 50 s |

In titanic12 the script says: "However, the British inquiry report states that only 712 people were saved, which contradicts the US Senate inquiry report's claim." That is the 706-vs-712 saved disagreement.

**Reliability with llama3.1:8b.** Titanic is not reliable on an 8B model. Of the cold runs on each fix's final form, these passed: titanic6, titanic8, titanic10, titanic11 and titanic12. These failed, each for a different reason:
- titanic5: a mangled claim id (66 hex characters) on all three script attempts.
- titanic9: extraction grounding rejected the paraphrase "Sunday evening".

Each earlier failure led to one of fixes 5.8–5.14:
- a chunk id cited as a claim;
- judged claims left out of the script;
- a typed lookout quotation;
- a runaway 13k-token verdict;
- a runaway 23k-token extraction reply;
- a cut-off reply failing the run with no retry.

The adjudicator often fell back when the model left out a supporting cite. A fallback always records why, and never takes a side. Some NLI contradictions are spurious; "lifeboats for 1,176" vs "712 saved" is not a real disagreement. That is a scoring limit, out of scope here.

## Risk & rollback
The only behaviour change without `[nli]` is the new stage row and an empty `verdicts.json`. To revert, run `git revert -m 1 <merge>`.

## Out of scope
Token budgeting, TTS, MCP connectors, PDF ingestion, NLI grounding in extraction (the next handoff).
