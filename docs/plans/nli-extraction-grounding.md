---
slug: nli-extraction-grounding
goal: With [embedding]+[nli] set, a claim is kept only where the NLI model finds its own chunk entails it, so distortions built from the chunk's own words ("Kulik led" for "Kulik joined") are dropped and counted; without them nothing changes.
classification: in-scope   # docs/architecture.md:167 and docs/handoff.md:54 list this as the planned follow-up
tracker_rows: [TRACKER#nli-extraction-grounding/1, TRACKER#nli-extraction-grounding/2, TRACKER#nli-extraction-grounding/3, TRACKER#nli-extraction-grounding/4, TRACKER#nli-extraction-grounding/5]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: self(root-only agent mode), see Verification background
coverage:
  contract:      1.1 (score_stances windows → shared helper, crate-private); 3.2 (RunReport gains a field, consumers: podling-cli commands.rs:34, tests/pipeline.rs:31); no podling-types / ArtifactKind / schema change
  data:          N/A(no persistence beyond the stage cache; new stage has its own ID so no old entry is reused)
  config:        N/A(no new episode keys; the stage runs exactly when build_grounding returns Some)
  security:      2.4 (source text reaches the NLI model only as premise data; no new I/O, no secrets)
  tests:         1.2, 2.2, 2.3, 2.5, 3.3, 3.4, 3.5, 3.6, 4.1
  observability: 2.4 (info log per stage run), 3.1 (pipeline logs each rejection, also on cache hit), 3.2 (CLI line)
  interface:     3.2 (CLI prints the rejection count when grounding ran)
  docs:          5.1, 5.2, 5.3
  rollback:      git revert of the merge; the new stage's cache entries are simply never read again
units:
  - id: 1
    scope_id: windows-helper
    project: .
    depends_on: []
    module: crates/podling-core/src/stages
    language: rust
    security: normal
    scope:
      read: [crates/podling-core/src/stages/score_stances.rs, crates/podling-core/src/text.rs, crates/podling-core/src/stages/mod.rs]
      docs: [docs/architecture.md]
      write: [crates/podling-core/src/stages/windows.rs, crates/podling-core/src/stages/score_stances.rs, crates/podling-core/src/stages/mod.rs]
    tooling: { implementer: implementer, gates: [code-reviewer], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: ground-claims-stage
    project: .
    depends_on: [windows-helper]
    module: crates/podling-core/src/stages/ground_claims.rs
    language: rust
    security: normal
    scope:
      read: [crates/podling-core/src/stages/score_stances.rs, crates/podling-core/src/stages/windows.rs, crates/podling-core/src/stages/extract_claims.rs, crates/podling-core/src/plugin/nli.rs, crates/podling-core/src/plugin/embedding.rs, crates/podling-core/src/stage.rs, crates/podling-types/src/claim.rs, crates/podling-core/tests/cross_encoder_parity.rs]
      docs: [docs/architecture.md]
      write: [crates/podling-core/src/stages/ground_claims.rs, crates/podling-core/src/stages/mod.rs, crates/podling-core/tests/ground_claims_live.rs]
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: pipeline-wiring
    project: .
    depends_on: [ground-claims-stage]
    module: crates/podling-core/src/pipeline.rs, crates/podling-cli
    language: rust
    security: normal
    scope:
      read: [crates/podling-core/src/pipeline.rs, crates/podling-core/src/stage.rs, crates/podling-core/src/stages/ground_claims.rs, crates/podling-cli/src/commands.rs, crates/podling-core/tests/pipeline.rs, crates/podling-core/tests/fixtures/paraphrase/episode.toml]
      docs: []
      write: [crates/podling-core/src/pipeline.rs, crates/podling-core/src/stage.rs, crates/podling-cli/src/commands.rs, crates/podling-core/tests/pipeline.rs, crates/podling-core/tests/fixtures/distortion/**]
    tooling: { implementer: implementer, gates: [code-reviewer], skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 4
    scope_id: live-check
    project: .
    depends_on: [pipeline-wiring]
    module: examples/tunguska
    language: rust
    security: normal
    scope:
      read: [examples/tunguska/episode-ollama.toml, examples/tunguska/sources/**]
      docs: []
      write: [docs/plans/nli-extraction-grounding.md]
    tooling: { implementer: implementer, gates: [], skills: [], guards: [], mcp: [] }
  - id: 5
    scope_id: docs
    project: .
    depends_on: [live-check]
    module: docs, examples/tunguska
    language: markdown
    security: normal
    scope:
      read: [crates/podling-core/src/stages/ground_claims.rs, crates/podling-core/src/pipeline.rs]
      docs: [docs/architecture.md, docs/handoff.md, README.md]
      write: [docs/architecture.md, docs/handoff.md, README.md, examples/tunguska/episode-ollama.toml]
    tooling: { implementer: implementer, gates: [], skills: [], guards: [], mcp: [] }
---

# Plan: NLI grounding in claim extraction

## Outcome
With `[embedding]` and `[nli]` set, a claim survives extraction only where the NLI model
finds that its own chunk entails it. Distortions such as "Kulik led the expedition", where
the source says he joined it, are dropped, and every drop is counted and logged. Without those
sections the stage list, cache keys and artifacts are byte-identical to today.

## Design decisions (answers to the goal's questions)

**Shape: a new `ground_claims` stage, not a change inside `ExtractClaims`.** It runs between
`extract_claims` and `cluster_claims`, only when `build_grounding` returns `Some`.
`ExtractClaims` keeps its code, `VERSION` 5 and fingerprint, so the no-NLI path is
byte-identical by construction. Bump rule 2 (`docs/architecture.md:124`: changing a stage's
logic means bumping its `VERSION`) would otherwise have forced a key change on the no-NLI
path too. The new stage's cached output carries the rejected list, so the count survives
cache hits; `ExtractClaims`' `Vec<Claim>` output has no room for it. "extract_claims keeps
a claim only if" therefore holds for the extraction phase as a whole (extract, then
ground), not for the single `ExtractClaims` struct.

1. **Premise: the best 1–2 sentence window of the claim's own chunk, prefixed with
   names.** A whole chunk does not work, and not only because of length. At the 800-word
   cap (`chunk.rs:18`) a chunk is about 960–1600 DeBERTa tokens (1.21 tokens/word on the
   Tunguska prose, 1.8–2.0 on technical text), far over `MAX_TOKENS = 512`
   (`cross_encoder.rs:33`). Even a 4-sentence chunk that fits gave a faithful paraphrase
   E=0.000, C=1.000. The same claim against its own sentence scored 0.998. The windows are
   score_stances' windows (`MAX_WINDOW_SENTENCES = 2`). Windows of 1, 2 and 3 sentences
   scored almost the same in the probe, with 2 slightly ahead. To bound cost, each
   (claim, chunk) pair is scored against only the top `GROUND_K = 4` windows of that chunk,
   ranked by embedding cosine. The embedder is already loaded whenever grounding is set.
2. **Names: in the premise.** Premise = `"<title>. <heading 1>. … <window>"`. Without the
   prefix, faithful claims that use a heading-only term scored 0.000; with it, 0.997. The
   lexical names exemption stays too, as part of the pre-filter (see 5).
3. **On rejection: drop and count, no retry, no failure.** A retry costs an LLM call
   without telling the model why the claim failed. Failing the run is too harsh when the
   model has false negatives. Grounding is per piece of evidence: a claim merged from two
   chunks keeps the chunks that entail it. A claim left with no evidence is dropped. Every
   rejected (claim, chunk) pair is recorded with its best entailment and premise span.
4. **Threshold: a separate `GROUND_ENTAIL_PM = 800`**, the same value as
   `SUPPORT_ENTAIL_PM` but its own constant, so either one can be tuned without moving the
   other. The probe shows a wide gap: 11 faithful claims scored ≥ 971, 6 distortions ≤ 3.
   It goes in the fingerprint with `GROUND_K` and `MAX_WINDOW_SENTENCES`.
5. **Lexical check: kept, unchanged, as the pre-filter.** It has to stay for the no-NLI
   path anyway. It is the exact-number gate the goal requires, and it rejects claims made
   only of names. NLI scored one such claim, "The Tunguska event happened.", at 0.996
   against the prefixed premise.

Status stays deterministic: the stage only removes evidence and claims; `classify` is
untouched. No new model: it reuses `grounding.nli` and `grounding.embedder`.

## Scope Steps (executable core)

### Step 1 — windows-helper (., rust, normal)
Tooling: implementer · gates code-reviewer · guards cargo fmt/clippy/test
Depends on: none
- [x] 1.1 Move `Window` and `windows()` from `score_stances.rs:197-269` into a new `crates/podling-core/src/stages/windows.rs` as `pub(crate)`. Give each window its chunk index and, in place of the group, an `&'a Chunk`, so both stages can use it. `score_stances` reads the group from `input.sources[chunk.document()]`. Comment the `pub(crate)` visibility idiom where it's introduced. → accept: `cargo test -p podling-core score_stances` passes with no test edited; `ScoreStances::VERSION` and fingerprint unchanged
- [x] 1.2 Add a unit test in `windows.rs`: a 3-sentence chunk yields 5 windows, in order, each with a document-relative span whose slice is the window text. → accept: test passes

### Step 2 — ground-claims-stage (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer · guards cargo fmt/clippy/test
Depends on: windows-helper
- [x] 2.1 Create `stages/ground_claims.rs` with `pub struct GroundClaims<'a> { embedder: &'a dyn EmbeddingProvider, nli: &'a dyn NliProvider }`, `GroundInput { claims: Vec<Claim>, chunks: Vec<Chunk>, titles: BTreeMap<DocumentId, String> }`, and output `Grounded { claims: Vec<Claim>, rejected: Vec<Rejection> }`. `Rejection { claim: ClaimId, text: String, chunk: ChunkId, entailment_pm: PerMille, premise: Option<TextSpan> }`, where `premise` is the best window or `None` if the chunk has none. These are core serde types (Serialize + Deserialize, for the cache), not an `ArtifactKind`. Set `ID = "ground_claims"`, `VERSION = 1`, and fingerprint `{embedding, nli, ground_k, ground_entail_pm, max_window_sentences}`. Add consts `GROUND_K = 4` and `GROUND_ENTAIL_PM = 800` with doc comments citing this plan's probe. Export them from `stages/mod.rs`. → accept: compiles; `cargo clippy -D warnings` clean
- [x] 2.2 Implement `run`. Embed every claim text and every window text in one `embed_checked` call. For each claim's evidence entry, take the top `GROUND_K` windows of that chunk by cosine (ties by window order) and build the premise `"<title>. <headings joined by \". \">. <window>"`. Leave out an empty title, and don't double the full stop when a name already ends with one. Score all pairs in one `score_checked` call. Keep an evidence entry when its best `PerMille::from_probability(entailment) >= GROUND_ENTAIL_PM`, otherwise push a `Rejection`. Rebuild each claim from its kept evidence, leaving every field, `basis` included, unchanged, and drop claims with none left. Preserve input claim order. An evidence chunk missing from `chunks` is `CoreError::InvalidProviderOutput { stage: "ground_claims", .. }`. → accept: unit tests in 2.3 pass
- [x] 2.3 Add unit tests with `FakeEmbedding` + `FakeNli`:
  (a) the claim "Kulik led the expedition." against the chunk "Kulik joined the expedition." is dropped with one `Rejection`, while "Kulik joined the expedition." is kept. FakeNli scores the first 2/3 → 667 < 800 (content words kulik/led/expedition). Keep the claim short: a 5-word variant like "Leonid Kulik led the 1927 expedition." scores 4/5 = 800 with the fake and would be kept;
  (b) a claim merged from two chunks, where only one entails it, keeps exactly that chunk's evidence;
  (c) a claim that names a heading-only term is kept, which checks the premise prefix: use a recording NLI to assert the premise starts with the title and headings;
  (d) a constant-score NLI at 0.80 keeps and 0.79 rejects;
  (e) at most `GROUND_K` pairs per (claim, chunk);
  (f) the fingerprint changes when the NLI fingerprint changes.
  → accept: all pass
- [x] 2.4 After grounding, emit `tracing::info!(claims, kept, dropped_claims, rejected_evidence, pairs, "claims grounded")`. Add a doc comment noting that source text reaches the NLI model only as premise data, never as instructions. → accept: present; `cargo clippy` clean
- [x] 2.5 Add `crates/podling-core/tests/ground_claims_live.rs`: an `#[ignore]` test gated on `PODLING_NLI_MODEL_DIR`, which panics if the variable is unset, like `cross_encoder_parity.rs:49`. With real `CrossEncoderNli` + `FakeEmbedding`, it checks that "Kulik led the 1927 expedition." is dropped and "Kulik joined the 1927 expedition." is kept against the chunk "Leonid Kulik joined the 1927 expedition to the site." → accept: `PODLING_NLI_MODEL_DIR=~/.cache/podling-models/nli-deberta-v3-base cargo test -p podling-core --test ground_claims_live -- --ignored` passes

### Step 3 — pipeline-wiring (., rust, normal)
Tooling: implementer · gates code-reviewer · guards cargo fmt/clippy/test
Depends on: ground-claims-stage
- [x] 3.0 BEFORE editing `pipeline.rs`, run the no-NLI fixture episode on the unmodified pipeline and record the `extract_claims` `StageRecord.key` hex. → accept: the hex is recorded in the test in 3.4
- [x] 3.1 In `pipeline.rs`, inside `if let Some(grounding)`, run `cached(&GroundClaims{..}, &GroundInput{claims, chunks: claim_input.chunks.clone(), titles: claim_input.titles.clone()}, ..)` before `ClusterClaims`. Feed `grounded.claims` to clustering. After `cached` returns, so it also runs on a cache hit, log each `Rejection` with `tracing::info!` (claim text, chunk, entailment_pm) and set the report count. → accept: compiles; NLI fixture tests reach 3.3
- [x] 3.2 Add `pub grounding: Option<GroundingCounts>` to `RunReport` (`stage.rs:44`), with `GroundingCounts { dropped_claims: usize, rejected_evidence: usize }`, and `#[serde(default)]` on the field. `None` means grounding did not run. In `podling-cli/src/commands.rs:39`, print `grounding: N claim(s) dropped, M evidence item(s) rejected` when it is `Some`. → accept: CLI output unchanged for the fake episode with no `[nli]`
- [x] 3.3 Update the stage-list assertion in `tests/pipeline.rs:479` to include `"ground_claims"` after `"extract_claims"`. → accept: `a_contradicting_source_contests_both_claims` passes; the paraphrase and near-miss ledgers are unchanged
- [x] 3.4 Add the test `no_nli_extract_claims_key_is_unchanged`. It asserts that the no-NLI run's `extract_claims` key equals the hex from 3.0. A second run of the same fixtures with `embedding = {kind="fake"}` and `nli = {kind="fake"}` added gives the same `extract_claims` key. The no-NLI report's `grounding` is `None`. → accept: passes; `no_nli_config_writes_todays_artifacts` and `without_embedding_and_nli_no_stance_stage_runs` pass untouched
- [x] 3.5 Add the fixture `tests/fixtures/distortion/`: an episode with fake embedding and fake NLI, and a test LLM that returns "Kulik led the expedition." for the chunk "Kulik joined the expedition.". This claim passes the lexical check (share 2/3 ≥ 0.6) and fails FakeNli (667 < 800). Add the test `nli_drops_a_distortion_the_lexical_check_lets_through`: the claim is absent from claims.json and the ledger, and `grounding == Some({dropped_claims: 1, rejected_evidence: 1})`. → accept: passes
- [x] 3.6 Add the test `rejection_count_survives_a_cache_hit`: run the distortion fixture twice with a `DiskCache`. The second run reports `ground_claims` as a hit, with the same `grounding`. → accept: passes
- [x] 3.7 Final gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. → accept: all pass

### Step 4 — live-check (., rust, normal)
Depends on: pipeline-wiring
- [ ] 4.1 Symlink the NLI weights into the worktree (`examples/tunguska/models/nli-deberta-v3-base`) and run `podling run --episode examples/tunguska/episode-ollama.toml` twice from a cold cache, then once warm. Record in this plan: exit code, the `ground_claims` timing, pairs, and dropped/rejected counts with each rejected claim's text and entailment. Judge each rejection as a true or false negative. Also run the ignored test from 2.5. → accept: exits 0 with no `error` in analysis.json; warm run shows `ground_claims` hit and the same counts; any false negative is recorded with its score

### Step 5 — docs (., markdown, normal)
Depends on: live-check
- [x] 5.1 `docs/architecture.md`: add `ground_claims` to the stage diagram (`:36`). Rewrite the grounding-check paragraph (`:245`), which currently ends "wiring it into this check is a follow-up", to describe the lexical pre-filter plus NLI grounding. Remove the "Replacing the lexical grounding check" bullet (`:167`). Add `ground_claims` to bump rule 5 (`:134`). → accept: every cited path and symbol exists
- [x] 5.2 `docs/handoff.md:54`: remove the "NLI grounding in extraction" follow-up and record the known misses: hedge removal ("almost burned" → "burned" scored 0.994) and a figure moved within the chunk (0.966). → accept: section updated
- [x] 5.3 `README.md` / `examples/tunguska/episode-ollama.toml` comment: one line saying that `[nli]` also drops claims their own chunk doesn't entail. → accept: present

## Sequencing
Pure refactor first, so the window helper is proven by score_stances' existing tests. Then
the stage in isolation, with fakes. Then the pipeline, where the key captured in 3.0 must
come from the unmodified pipeline. Then live, which decides whether `GROUND_K` and the
threshold need tuning before the docs freeze the numbers.

## Rust idioms to explain while implementing
- `pub(crate)`: visible inside the crate, not exported (1.1).
- Borrowing struct fields with a lifetime, `GroundClaims<'a>` and `Window<'a>`. This is already explained once in `plugin/nli.rs:17`. Point there and add what's new: windows borrow from the input that outlives them.
- `Vec::retain` / iterator `partition` for keep-vs-reject (2.2), if used.
- `#[serde(default)]` for a field added to a serialized struct (3.2).

## Verification background   (citations — for the reviewer, not the executor)
- Cache key = `CacheKey::new(S::ID, S::VERSION, input, &stage.config_fingerprint())`. `crates/podling-core/src/stage.rs:62`
- Bump rule 2, changing a stage's logic means bumping `VERSION`. `docs/architecture.md:124`
- `build_grounding` is both-or-neither. `crates/podling-core/src/plugin/mod.rs:60`
- The lexical check and the names exemption. `crates/podling-core/src/stages/extract_claims.rs:44`
- Windows and `SUPPORT_ENTAIL_PM`. `crates/podling-core/src/stages/score_stances.rs:37,241`
- `MAX_TOKENS = 512`, truncation. `crates/podling-core/src/plugin/cross_encoder.rs:33,81`
- Chunk cap of 800 words. `crates/podling-core/src/stages/chunk.rs:18`
- The no-NLI golden test. `crates/podling-core/tests/pipeline.rs:90`
- FakeNli is content-word containment. `crates/podling-core/src/plugin/nli.rs:82`
- Probe: the real DeBERTa via transformers, parity-tested with the candle impl (`tests/cross_encoder_parity.rs`), run on 2026-10-04 over the Tunguska sources with premise = names prefix + best window:

  | claim | k=1 | k=2 | no prefix |
  |---|---|---|---|
  | Kulik got to the site in 1927. (ok) | .998 | .998 | .998 |
  | No impact crater was found at the Tunguska site. (ok) | .998 | .998 | .000 |
  | The sky split in two at breakfast at the Vanavara trading post. (ok) | .997 | .997 | .000 |
  | Fire covered the northern sky above the Tunguska forest. (ok, lowest) | .971 | .971 | .983 |
  | Leonid Kulik led the 1927 expedition. (distortion) | .003 | .003 | .001 |
  | The trees pointed toward a central area. (distortion) | .001 | .001 | .129 |
  | Kulik reached the site in 1908. (changed figure) | .000 | .000 | .001 |
  | The Tunguska event happened. (names only) | .996 | .998 | .995 |
  | The heat burned the eyewitness's shirt. (hedge removed; MISS) | .987 | .994 | .977 |
  | An explosion in 1927 flattened trees. (figure moved; MISS) | .902 | .966 | .650 |

  Whole chunk as premise: "Kulik got to the site in 1927." E=.000 C=1.000, and "About 80 million trees were flattened in June 1908." E=.235.

**CONSUMERS:**
- `score_stances::windows` / `Window` (private → `stages::windows`, crate-private): `crates/podling-core/src/stages/score_stances.rs:91` (only caller).
- `RunReport` (pub, re-exported `crates/podling-core/src/lib.rs:13`): `crates/podling-core/src/pipeline.rs:50,121`; `crates/podling-cli/src/commands.rs:30-48`; `crates/podling-core/tests/pipeline.rs:31,60,320,365,456,467`. A new field with `Default` breaks no constructor; all build it via `RunReport::default()`.
- `stages` module exports (`crates/podling-core/src/stages/mod.rs:15`): pipeline.rs:16 only.
- Stage-list assertions: `crates/podling-core/tests/pipeline.rs:14` (no-NLI `STAGES`, must stay unchanged) and `:479` (NLI list, gains `ground_claims`).
- Unchanged by design: `ExtractClaims` (pipeline.rs:74), `ClaimInput`, `podling-types` (no schema snapshot change).

**Blind re-derivation (self, root-only mode):** working from the goal alone, the touch list
came out as extract_claims.rs, nli.rs, pipeline.rs, the stage/cache key, tests/pipeline.rs,
fixtures, architecture.md, handoff.md, the example toml, and the CLI output. Against the plan:
extract_claims.rs is deliberately unchanged (Design, Shape), and nli.rs needs no change
(`score_checked` is reused). Everything else is covered. One extra item came up: the
schema snapshot. It's N/A because there is no `podling-types` change.

## Risk & rollback
- Behaviour change only for runs with grounding set. Claims may disappear, and a merged
  claim may lose a group, Corroborated → SingleSource. That's the intent, and it's logged
  and counted.
- False negatives drop true claims silently except for the log and count. The live check
  (4.1) measures them on Tunguska.
- Cost: claims × evidence × `GROUND_K` NLI pairs, plus one extra embedding call over all
  windows, which duplicates part of score_stances' call. That's acceptable at this scale; noted for later.
- Rollback: revert the merge. The `ground_claims` cache entries are orphaned and harmless.

## Out of scope
- Catching hedge removal and moved figures. These are NLI model limits, recorded in the handoff.
- Recording an NLI `basis` on kept extraction evidence.
- Sharing embeddings between `ground_claims` and `score_stances`.
- Any change to the no-NLI lexical check.
