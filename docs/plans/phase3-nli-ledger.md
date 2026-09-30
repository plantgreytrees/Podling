---
slug: phase3-nli-ledger
goal: The same fact worded differently by two independent sources becomes one Corroborated claim, and a source that contradicts a claim makes it Contested; both statuses come from a local NLI model, not from exact text matching.
classification: in-scope   # .claude/CLAUDE.md "Grounding" (claims clustered, NLI-scored, deterministic status); docs/architecture.md "Known limit" and "Deferred to later phases" name the NLI provider as the fix
tracker_rows: [TRACKER#phase3-nli-ledger/1, TRACKER#phase3-nli-ledger/2, TRACKER#phase3-nli-ledger/3, TRACKER#phase3-nli-ledger/4, TRACKER#phase3-nli-ledger/5, TRACKER#phase3-nli-ledger/6]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: skipped(root-only agent mode: agent-mode-guard blocks non-strategist Task; the plan-strategist pass supplied the independent decomposition, see "Decomposition")
coverage:
  contract:      2.2 (Evidence gains `basis`), 2.3 (SCHEMA_VERSION 2→3 + snapshots), 2.4 (EpisodeSpec gains `embedding`/`nli`), 3.1–3.2 (new EmbeddingProvider / NliProvider traits), 3.6 (pipeline gains two gated stages), 4.1 (cluster_claims), 5.1 (openai.rs transport shared)
  data:          N/A(no database; new stage ids and the SCHEMA_VERSION bump only miss the content cache, which treats foreign versions as misses: docs/architecture.md "Cache key")
  config:        2.4 ([embedding] and [nli] episode sections), 3.5 (both-or-neither validation, model_dir resolved against the episode dir), 6.1 (episode-ollama.toml wired)
  security:      1.2 (safetensors only, never pickle `pytorch_model.bin`), 5.1–5.3 (embeddings reuse the existing URL/key/redirect/size-cap/redaction transport; provider output validated: vector count and dimension), 1.4 + 5.6 (dependency licence audit)
  tests:         1.3, 2.1, 2.5, 3.3, 3.4, 3.7, 3.8, 4.3, 4.4, 5.4, 5.7, 6.2–6.4
  observability: 3.6, 4.2 (info log per stage: pairs scored, supports/contradicts added, merges made), 5.3 (embeddings request span like llm_request), 5.5 (NLI batch span with pair count and elapsed_ms)
  interface:     3.6 (CLI stage rows gain cluster_claims/score_stances only when configured), 5.5 (a missing model_dir is one readable Config line naming the `hf download` command)
  docs:          6.5 (architecture.md), 6.6 (README config table), 6.7 (handoff.md)
  rollback:      git revert of the branch. With no [embedding]/[nli] the pipeline runs today's stages and writes today's artifact bodies (test 2.1/3.8); only the envelope's schema_version changes (2→3)
units:
  - id: 1
    scope_id: nli-spike
    project: .
    depends_on: []
    module: crates/podling-core/src/plugin/cross_encoder.rs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/Cargo.toml
        - Cargo.toml
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/error.rs
      docs: [.claude/CLAUDE.md, docs/architecture.md]
      write:
        - Cargo.toml
        - Cargo.lock
        - crates/podling-core/Cargo.toml
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/plugin/cross_encoder.rs
        - crates/podling-core/tests/cross_encoder_parity.rs
        - crates/podling-core/tests/fixtures/nli/reference_logits.json
        - scripts/nli_reference.py
        - .gitignore
    tooling: { implementer: implementer, gates: [code-reviewer, dependency-auditor, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 2
    scope_id: ledger-contracts
    project: .
    depends_on: []
    module: crates/podling-types
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-types/src/claim.rs
        - crates/podling-types/src/ledger.rs
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/document.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/episode.toml
      docs: [docs/architecture.md]
      write:
        - crates/podling-types/src/claim.rs
        - crates/podling-types/src/episode.rs
        - crates/podling-types/src/envelope.rs
        - crates/podling-types/src/lib.rs
        - crates/podling-types/tests/roundtrip.rs
        - crates/podling-types/tests/schema_snapshot.rs
        - crates/podling-types/tests/snapshots/schema_snapshot__claims.snap
        - crates/podling-types/tests/snapshots/schema_snapshot__ledger.snap
        - crates/podling-types/tests/snapshots/schema_snapshot__episode.snap
        - crates/podling-types/tests/snapshots/schema_snapshot__script.snap
        - crates/podling-core/tests/fixtures/golden/
        - crates/podling-core/tests/pipeline.rs
    tooling: { implementer: implementer, gates: [code-reviewer, api-reviewer, idiom-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 3
    scope_id: stance-stage
    project: .
    depends_on: [2]
    module: crates/podling-core/src/stages/score_stances.rs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/plugin/llm.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/stages/extract_claims.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/stage.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/src/error.rs
        - crates/podling-types/src/claim.rs
        - crates/podling-types/src/episode.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-cli/tests/cli.rs
      docs: [docs/architecture.md]
      write:
        - Cargo.toml
        - Cargo.lock
        - crates/podling-core/Cargo.toml
        - crates/podling-core/src/text.rs
        - crates/podling-core/src/plugin/embedding.rs
        - crates/podling-core/src/plugin/nli.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/stages/extract_claims.rs
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/paraphrase/
        - crates/podling-core/tests/fixtures/contradiction/
        - crates/podling-cli/tests/cli.rs
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer, performance-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 4
    scope_id: cluster-claims
    project: .
    depends_on: [3]
    module: crates/podling-core/src/stages/cluster_claims.rs
    language: rust
    security: normal
    scope:
      read:
        - crates/podling-core/src/plugin/embedding.rs
        - crates/podling-core/src/plugin/nli.rs
        - crates/podling-core/src/stages/score_stances.rs
        - crates/podling-core/src/text.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-types/src/claim.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-core/src/stages/cluster_claims.rs
        - crates/podling-core/src/stages/mod.rs
        - crates/podling-core/src/pipeline.rs
        - crates/podling-core/tests/pipeline.rs
        - crates/podling-core/tests/fixtures/near-miss/
        - crates/podling-cli/tests/cli.rs
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer, performance-reviewer],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 5
    scope_id: real-providers
    project: .
    depends_on: [1, 3]
    module: crates/podling-core/src/plugin
    language: rust
    security: high
    scope:
      read:
        - crates/podling-core/src/plugin/openai.rs
        - crates/podling-core/src/plugin/cross_encoder.rs
        - crates/podling-core/src/plugin/embedding.rs
        - crates/podling-core/src/plugin/nli.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/src/error.rs
        - crates/podling-core/tests/openai_provider.rs
        - crates/podling-cli/src/commands.rs
        - crates/podling-types/src/episode.rs
      docs: [docs/architecture.md]
      write:
        - crates/podling-core/src/plugin/openai.rs
        - crates/podling-core/src/plugin/cross_encoder.rs
        - crates/podling-core/src/plugin/mod.rs
        - crates/podling-core/tests/openai_provider.rs
        - crates/podling-core/tests/openai_embeddings.rs
        - crates/podling-cli/src/commands.rs
        - crates/podling-cli/tests/cli.rs
        - Cargo.toml
        - Cargo.lock
        - crates/podling-core/Cargo.toml
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor, idiom-reviewer, dependency-auditor],
               skills: [language-aware-planning], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
  - id: 6
    scope_id: live-and-docs
    project: .
    depends_on: [4, 5]
    module: examples/tunguska, docs
    language: rust
    security: normal
    scope:
      read:
        - examples/tunguska/episode-ollama.toml
        - examples/tunguska/episode.toml
        - crates/podling-cli/tests/cli.rs
        - crates/podling-core/src/plugin/mod.rs
      docs: [docs/architecture.md, README.md, docs/handoff.md, .claude/CLAUDE.md]
      write:
        - examples/tunguska/episode-ollama.toml
        - .gitignore
        - docs/architecture.md
        - README.md
        - docs/handoff.md
        - docs/plans/phase3-nli-ledger.md
    tooling: { implementer: implementer, gates: [code-reviewer],
               skills: [], guards: [cargo fmt, cargo clippy, cargo test], mcp: [] }
---

# Plan: Phase 3 — embeddings and NLI in the claim ledger

## Outcome
The same fact worded differently by two independent sources becomes one Corroborated
claim, and a source that contradicts a claim makes it Contested; both statuses come
from a local NLI model, not from exact text matching.

## Design (decided here; the executor follows it)

**Pipeline.** `extract_claims → [cluster_claims] → [score_stances] → ledger → script → analyse`.
The two bracketed stages run only when the episode has both `[embedding]` and `[nli]`.
Without them, no new stage runs, so the stage list, cache keys and artifact bodies are
exactly today's. `classify()` and `BuildLedger` are unchanged: status is still a pure
function of evidence, and the NLI model only *produces evidence*.

**Providers (plugins).** They go in `crates/podling-core/src/plugin/`, next to `LlmProvider`:

```rust
pub trait EmbeddingProvider {
    fn id(&self) -> &str;
    fn fingerprint(&self) -> Value;           // model, base_url, version; never a key
    /// One vector per text, same order. Stages reject a wrong count or ragged dims.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}
pub struct NliPair<'a> { pub premise: &'a str, pub hypothesis: &'a str }  // borrows, no copies
pub struct NliScores { pub entailment: f32, pub neutral: f32, pub contradiction: f32 }
pub trait NliProvider {
    fn id(&self) -> &str;
    fn fingerprint(&self) -> Value;           // includes a BLAKE3 of the weights for the real one
    fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>>;
}
```

| Kind | Fake (offline, deterministic) | Real |
|---|---|---|
| Embedding | `FakeEmbedding`: feature-hashed bag of content words (BLAKE3 of each word → one of 256 dims), L2-normalised, so cosine = lexical overlap | `OpenAiEmbeddings`: `POST {base_url}/embeddings` (Ollama `nomic-embed-text`), on the same transport as `OpenAiCompat` |
| NLI | `FakeNli`: asymmetric containment. Entailment is the share of the hypothesis's content words that the premise contains. Contradiction is 1.0 when the two share at least 60% of their non-number content words but the hypothesis has a number the premise lacks while the premise has a number of its own | `CrossEncoderNli`: candle 0.11 `DebertaV2SeqClassificationModel` on **CPU**, `cross-encoder/nli-deberta-v3-base` (Apache-2.0) loaded from a local `model_dir`, labels mapped by the model's `id2label` |

**NLI on CPU.** The CPU keeps the 8 GB card free for Ollama's LLM and embedder (no VRAM contention,
nothing to unload), and needs no CUDA toolchain. DeBERTa-v3-base on CPU scores pairs in tens of
milliseconds each, and the cost bound below keeps a run to a few hundred pairs at most. The model is
loaded once per stage call and dropped when the stage returns, which satisfies "one model stage at a time".

**Evidence audit (the single schema change).** `Evidence` gains:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub basis: Option<EvidenceBasis>,

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceBasis {
    /// This chunk stated the claim in other words; merged by `cluster_claims`.
    Merged { wording: String, entailment_pm: PerMille },   // min of both directions
    /// NLI scored this premise window of the chunk against the claim.
    Nli { premise: TextSpan, similarity_pm: PerMille, entailment_pm: PerMille, contradiction_pm: PerMille },
}

/// A probability in thousandths, 0..=1000. Deserialisation rejects > 1000.
#[serde(try_from = "u16", into = "u16")]
pub struct PerMille(u16);
```

`None` means the model extracted the claim from this chunk in exactly this wording, as today.
Scores are **per-mille integers** (0..=1000), rounded once. That keeps `Evidence: Eq + Ord`
(floats have neither) and gives canonical JSON. Thresholds are compared against the
*rounded* value, so the stored number is the one that decided the outcome. Recorded per claim:
- the first `Evidence` for a (claim, chunk, stance) wins;
- the stance stage emits **at most one** evidence per (claim, chunk), from the premise window with the
  highest decisive score. Entailment is checked first, then contradiction, so a chunk is never both Supports and Contradicts.

**cluster_claims.** For each claim (in `ClaimId` order), find candidate partners whose cosine
similarity is at least `MERGE_CANDIDATE_COS = 0.80`, at most `MAX_MERGE_CANDIDATES = 8` per claim. A pair merges only if:
1. NLI entailment is at least `MERGE_ENTAIL_PM = 900` **in both directions**;
2. the number veto passes: the two claims' number sets (`text::content_words`, digits) are equal;
3. the linkage is **complete**: the pair is compatible with every member already in the cluster, so A≈B and B≈C can't chain A to C.
Similarity alone never merges. The merged claim keeps the text of its lowest-`ClaimId` member,
because `Claim`'s id is rebuilt from its text (`claim.rs:47`). Evidence carried over from the other members gets
`basis: Merged { wording, entailment_pm }`.

**score_stances.** Premises are sentence windows of 1 and 2 consecutive sentences (`text::sentences`)
per chunk, so they stay well under DeBERTa's 512 tokens. For each claim, candidates are windows from chunks
whose independence group has no evidence on the claim yet. Retrieve the top `RETRIEVE_K = 4` by cosine,
keeping only those at or above `MIN_RETRIEVAL_COS = 0.30`, then score (premise = window, hypothesis = claim):
- `entailment_pm ≥ SUPPORT_ENTAIL_PM = 800` → `Supports`;
- else `contradiction_pm ≥ CONTRADICT_PM = 950` **and** the window's retrieval similarity `≥ MIN_CONTRADICT_COS = 0.60` → `Contradicts`. A contradiction only counts between passages about the same thing: the spike found DeBERTa gives 0.903 contradiction to the unrelated pair "Kulik reached the site in 1927" / "No impact crater was found". The similarity is stored as `similarity_pm` so the decision can be audited;
- else nothing.

**Cost bound:** at most `RETRIEVE_K × claims` NLI pairs, plus `claims + windows` embeddings, per run
(cluster adds at most `2 × MAX_MERGE_CANDIDATES × claims` pairs). Both stages are cached by content,
so a rerun costs nothing.

**Config** (`EpisodeSpec`, both optional, **both or neither**, else a `Config` error before any stage runs):

```toml
[embedding]
kind = "open_ai_compat"            # or "fake"
base_url = "http://localhost:11434/v1"
model = "nomic-embed-text"
# api_key_env, timeout_secs optional, as for [llm]

[nli]
kind = "cross_encoder"             # or "fake"
model_dir = "models/nli-deberta-v3-base"   # relative to the episode file
```

## Scope Steps (executable core)

### Step 1 — nli-spike (., rust, normal)
Tooling: implementer · gates code-reviewer, dependency-auditor, idiom-reviewer · skills language-aware-planning · guards fmt/clippy/test
Depends on: none
- [x] 1.1 Add `candle-core`, `candle-nn`, `candle-transformers` (0.11, default features, i.e. CPU) and `tokenizers` (0.22, `default-features = false`; add back only what `tokenizer.json` needs to load, and record which) to `[workspace.dependencies]` and podling-core → accept: `cargo build -p podling-core` succeeds with no build-time network download or C++ toolchain requirement beyond what cargo already needs
- [x] 1.2 Write `plugin/cross_encoder.rs`: `CrossEncoder::load(model_dir)` reads `config.json`, `tokenizer.json` and `model.safetensors` (never `pytorch_model.bin`: it is a pickle); `scores(&[(premise, hypothesis)]) -> Vec<[f32; 3]>` in (entailment, neutral, contradiction) order via `id2label`, truncating the pair to 512 tokens; missing files → `CoreError::Config` naming the `hf download` command. Explain the candle idioms (`VarBuilder` over mmapped safetensors, `Tensor` on `Device::Cpu`, softmax over logits) in comments → accept: unit test that a config with an unknown label set is a Config error
- [x] 1.3 Write `scripts/nli_reference.py` (torch + transformers, dev-only) that dumps softmax scores for 6 fixed pairs (entail/neutral/contradict, including 1907-vs-1908 and the "80 million trees" paraphrase) to `tests/fixtures/nli/reference_logits.json`; add an `#[ignore]` test `cross_encoder_parity` reading `PODLING_NLI_MODEL_DIR` that asserts every probability within 0.02 of the reference and the argmax equal → accept: `PODLING_NLI_MODEL_DIR=… cargo test -p podling-core -- --ignored parity` passes; record ms/pair on this CPU in "Verification background"
- [x] 1.4 Audit licences of every crate added to `Cargo.lock` (dependency-auditor) → accept: no AGPL/GPL/non-commercial; list recorded below
- [x] 1.5 (not needed: 1.3 passed) Fallback, **only if 1.3 cannot pass**: replace candle with `ort` (`default-features = false`, no `download-binaries`, ONNX Runtime from the system) behind the same `CrossEncoder` API, and record why → accept: same parity test passes

### Step 2 — ledger-contracts (., rust, normal)
Tooling: implementer · gates code-reviewer, api-reviewer, idiom-reviewer · skills language-aware-planning · guards fmt/clippy/test
Depends on: none
- [x] 2.1 **Before any other change**, capture the golden from the fake pipeline on `crates/podling-core/tests/fixtures/episode.toml`: write each artifact's `body` (pretty JSON, exactly as `pipeline::write` serialises it) to `tests/fixtures/golden/<kind>.json`, and add the test `no_nli_config_writes_todays_artifact_bodies` comparing bytes → accept: passes on the unchanged code
- [x] 2.2 Add `PerMille` (newtype, `PerMille::from_probability(f32)` rounds and clamps; `TryFrom<u16>` rejects > 1000), `EvidenceBasis` and `Evidence::basis` (as in Design) in `claim.rs`; export from `lib.rs`; update the 3 `Evidence {` literals (`extract_claims.rs:161`, `ledger.rs:124`, `tests/roundtrip.rs:32`) with `basis: None` → accept: roundtrip test covers `Merged` and `Nli` variants; `{"entailment_pm": 1001}` fails to deserialise
- [x] 2.3 Accept the new claims/ledger/script/episode snapshots; bump `SCHEMA_VERSION` 2→3 and `schema_version_is_pinned` → accept: `cargo test -p podling-types` passes
- [x] 2.4 Add `EmbeddingConfig { Fake {}, OpenAiCompat { base_url, model, api_key_env?, timeout_secs? } }` and `NliConfig { Fake {}, CrossEncoder { model_dir: PathBuf } }` (`deny_unknown_fields`, struct variants as the existing enums do) and `EpisodeSpec::{embedding, nli}: Option<…>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` → accept: a TOML test parses both; an unknown key in either section is rejected
- [x] 2.5 Re-run 2.1's golden test → accept: bodies byte-identical (only the envelope's `schema_version` differs)

### Step 3 — stance-stage (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer, performance-reviewer · skills language-aware-planning · guards fmt/clippy/test
Depends on: 2
- [x] 3.1 Move `content_words` (and the number test) from `extract_claims.rs` to `text.rs` as `pub fn content_words` / `pub fn numbers`; `extract_claims` calls them; its VERSION is **not** bumped (no logic change) → accept: existing grounding tests pass unchanged
- [x] 3.2 Write `plugin/embedding.rs` (trait + `FakeEmbedding` + `cosine`) and `plugin/nli.rs` (trait, `NliPair<'a>`, `NliScores`, `FakeNli`, `to_per_mille`), with doc comments explaining trait objects as plugins and the `'a` borrow in `NliPair` → accept: unit tests show `FakeNli` is asymmetric ("A B C" ⊨ "A B", not the reverse) and scores 1907 vs 1908 as contradiction
- [x] 3.3 Test the fakes' determinism: same input, same bytes; `FakeEmbedding` cosine of a reordered paraphrase ≥ 0.95 → accept: passes
- [x] 3.4 Write `stages/score_stances.rs` (`ScoreStances { embedder, nli }`, `StanceInput { claims, chunks, sources }`, VERSION 1, fingerprint = both providers' fingerprints + the named thresholds), with the premise windows, retrieval bound and one-evidence-per-(claim, chunk) rule from Design; validate provider output counts → `InvalidProviderOutput` → accept: unit tests with a scripted NLI: entailment → Supports with `basis: Nli`; contradiction → Contradicts; below both thresholds → none; never more than `RETRIEVE_K` pairs per claim
- [x] 3.5 Add `build_embedder(&EmbeddingConfig)` and `build_nli(&NliConfig, base_dir)` factories, plus `build_grounding(spec, base_dir) -> Result<Option<Grounding>>` with `pub struct Grounding { pub embedder: Box<dyn EmbeddingProvider>, pub nli: Box<dyn NliProvider> }`, enforcing both-or-neither (kept as two sections, not one nested table, because the NLI-only grounding follow-up needs `[nli]` without `[embedding]`) → accept: only one of the two sections → `CoreError::Config` naming both keys
- [x] 3.6 Wire `pipeline::run_with_llm`: after `extract_claims`, when grounding is `Some`, run `score_stances` (logging pairs scored, supports and contradicts added at info); otherwise skip it. `Grounding` is built before `extract_claims` (so a config error fails fast) and dropped right after the last grounding stage, before `script` → accept: the fake-config pipeline lists `score_stances`; the no-config pipeline's stage list is unchanged
- [x] 3.7 Pipeline tests with `[embedding] kind="fake"`, `[nli] kind="fake"`: (a) `fixtures/contradiction/` (group A "The explosion happened in June 1908.", group B "The explosion happened in June 1907.") → both claims Contested; (b) `fixtures/paraphrase/` (A "In June 1908 an explosion flattened about 80 million trees.", B "About 80 million trees were flattened by an explosion in June 1908.") → the claims are Corroborated across both groups → accept: both pass
- [x] 3.8 Re-run the golden test and the CLI `second_run_of_the_example_is_all_cache_hits` → accept: pass unchanged

### Step 4 — cluster-claims (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer, performance-reviewer · skills language-aware-planning · guards fmt/clippy/test
Depends on: 3
- [x] 4.1 Write `stages/cluster_claims.rs` (`ClusterClaims { embedder, nli }`, input and output `Vec<Claim>`, VERSION 1) implementing the Design rules (candidate cosine, bidirectional entailment, number veto, complete linkage, lowest-id text, `Merged` basis) → accept: unit tests: a scripted NLI that entails only one way does not merge; A≈B, B≈C, A≉C gives two clusters; output order is independent of input order
- [x] 4.2 Wire it in `pipeline.rs` before `score_stances` when grounding is configured; log merges at info → accept: stage order in the report is extract_claims, cluster_claims, score_stances, ledger
- [x] 4.3 Extend 3.7(b): the paraphrase fixture now yields **one** Corroborated claim whose evidence has one `None` and one `Merged` basis → accept: passes
- [x] 4.4 `fixtures/near-miss/`: 1907 vs 1908 not merged, (i) with the number veto disabled (unit test through a private flag or by calling the merge predicate with the veto off), so the NLI path alone rejects it, and (ii) through the pipeline → accept: two claims remain, both Contested
- [x] 4.5 CLI test: the example with fake `[embedding]`/`[nli]` runs twice; the second run is all cache hits, including the new stages → accept: passes

### Step 5 — real-providers (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor, idiom-reviewer, dependency-auditor · skills language-aware-planning · guards fmt/clippy/test
Depends on: 1, 3
- [x] 5.1 Refactor `openai.rs`: extract the transport (validated base URL, key from env with redacted `Debug`, ureq agent with redirects off and timeout, `post_json(path, body)` with 429/5xx retries, 4 MiB cap, excerpt redaction) into one private `Transport` that `OpenAiCompat` holds; behaviour and fingerprint unchanged → accept: `tests/openai_provider.rs` passes unmodified
- [x] 5.2 Add `OpenAiEmbeddings::from_config(&EmbeddingConfig)` on the same `Transport`: `POST {base}/embeddings` `{model, input: [...]}`, sorts `data` by `index`, rejects a wrong count, ragged or empty vectors, or non-finite values (`ProviderFailure::Other`); fingerprint {provider, base_url, model, request version} → accept: unit tests
- [x] 5.3 Batch requests at `EMBED_BATCH = 64` texts and add an `embedding_request` span (model, count, elapsed_ms, attempts; never text or key) → accept: log test or reviewed
- [x] 5.4 `tests/openai_embeddings.rs` against a local mock server: happy path, 5xx retried, 401 names the key variable and never prints the key, out-of-order `index` reordered → accept: passes
- [x] 5.5 `CrossEncoderNli` implements `NliProvider` over unit 1's `CrossEncoder`. Construction checks that the files exist and computes the fingerprint; the model loads lazily on the first `score` through a `std::cell::OnceCell` (explain the idiom), so a fully cached run never loads it, and it is freed when `Grounding` drops. Scores in batches, an `nli_batch` span (pairs, elapsed_ms), fingerprint {provider, model id from config, BLAKE3 of config.json + tokenizer.json + model.safetensors, streamed}; `build_nli` resolves `model_dir` against the episode dir. The CLI hint for a missing model names `hf download cross-encoder/nli-deberta-v3-base --local-dir <dir>` → accept: a missing dir is one readable line (CLI test); a unit test shows construction does not load weights (e.g. a dir whose `model.safetensors` is not a valid model builds, and fails only on `score`)
- [x] 5.6 Dependency audit of anything new → accept: recorded below
- [x] 5.7 `#[ignore]` live test `live_embeddings` (`PODLING_LIVE_EMBED_URL`, `PODLING_LIVE_EMBED_MODEL`) → accept: passes against Ollama with nomic-embed-text

### Step 6 — live-and-docs (., rust, normal)
Tooling: implementer · gates code-reviewer · guards fmt/clippy/test
Depends on: 4, 5
- [x] 6.1 Wire `examples/tunguska/episode-ollama.toml` with `[embedding]` (Ollama nomic-embed-text) and `[nli]` (`model_dir = "models/nli-deberta-v3-base"`), with a comment giving the `hf download` command; gitignore `examples/*/models/` → accept: file parses (`cargo test` episode-parsing test)
- [x] 6.2 Baseline: one cold-cache run **without** the new sections (a temp copy) and record which claims are SingleSource → accept: figures recorded under "Live results"
- [x] 6.3 Two cold-cache runs: `target/debug/podling run --episode examples/tunguska/episode-ollama.toml --cache-dir <empty dir>` → accept: both exit 0, `analysis.json` has no `error`, and `ledger.json` has at least one claim Corroborated across eyewitness+expedition that was SingleSource in 6.2; both runs' figures recorded
- [x] 6.4 `PODLING_LIVE_LLM_URL=http://localhost:11434/v1 PODLING_LIVE_LLM_MODEL=llama3.1:8b cargo test -p podling-cli -- --ignored live` → accept: passes
- [x] 6.5 `docs/architecture.md`: the artifact flow with the two stages, the provider table rows (Embedding, NLI), "Evidence audit", cluster/stance rules and cost bound, the fifth bump rule (a provider fingerprint also covers embedding/NLI), remove "Known limit", update "Deferred" (adjudicator; lexical grounding → NLI as a follow-up), status banner → accept: every cited path resolves
- [x] 6.6 README: `[embedding]`/`[nli]` config tables, model download, privacy note (embeddings send claim and sentence text to the embedding server) → accept: reviewed
- [x] 6.7 `docs/handoff.md`: next goal = Contested-claim adjudicator → accept: reviewed
- [x] 6.8 Final gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` → accept: all pass

## Sequencing
Steps 1 and 2 are independent (the risk spike and the types). 3 builds the offline vertical slice on 2, and on its own
already produces Contested and Supports-based Corroborated. 4 adds merging. 5 needs the spike (1) and the traits (3).
6 is last because it needs everything. Execute in the order 1, 2, 3, 4, 5, 6. A failed spike only changes unit 5's NLI provider (1.5 fallback).

## Decomposition (plan-strategist)
Three options were considered: by layer, by risk-first vertical slice, and by real model first. The risk-first vertical slice
was chosen with the full `Evidence` audit designed up front, so there is one schema bump. Rejected: layer-first, which puts
the riskiest part (candle DeBERTa-v3) mid-stream with nothing end to end until late; and model-first, which makes tests depend on weights early.
Adopted from the strategist: complete linkage, the lowest-id text, one evidence per (claim, chunk), per-mille scores
compared after rounding, a both-or-neither config, and a shared `post_json` transport.

## Verification background   (citations — for the reviewer, not the executor)
- Claims merge only by exact normalised text: `Claim::id_for` — `crates/podling-types/src/claim.rs:74`, `normalise` `:100`; merged in `crates/podling-core/src/stages/extract_claims.rs:157-167`.
- `Claim` deserialisation recomputes the id from text, so a merged claim's text must be a real member's — `crates/podling-types/src/claim.rs:47-60`.
- `classify` already returns Contested when any group contradicts; nothing produces `Stance::Contradicts` — `crates/podling-types/src/ledger.rs:31-60`; only producer of `Evidence` is `extract_claims.rs:161` (always `Supports`).
- `Evidence` derives `Eq + Ord` and is kept sorted; `add_evidence` dedups exact equals — `claim.rs:19-24`, `:92-97`.
- `EpisodeSpec` has `deny_unknown_fields` — `crates/podling-types/src/episode.rs:12`.
- Stage cache key = id, VERSION, input, config_fingerprint — `crates/podling-core/src/stage.rs:64`; stage outputs need only serde, not an `ArtifactKind`.
- `SCHEMA_VERSION = 2`, pinned by `crates/podling-types/tests/schema_snapshot.rs:19`.
- The fourth bump rule (provider fingerprint) — `docs/architecture.md` "The four bump rules" 3.
- "Known limit" and "Deferred to later phases" name the NLI provider — `docs/architecture.md` "Grounding check", "Known limit", "Plugins".
- candle-transformers 0.11.0 has `models/debertav2.rs` with `DebertaV2SeqClassificationModel` (line 1269), `DebertaV2ContextPooler`, and a `Config` with `id2label`, `position_buckets`, `norm_rel_ebd`, `share_att_key` (verified in the cargo registry, 2026-10-01).
- `cross-encoder/nli-deberta-v3-base`: Apache-2.0, trained on SNLI+MNLI, ships `model.safetensors` and `tokenizer.json`, `id2label {0: contradiction, 1: entailment, 2: neutral}`, `pooler_hidden_size 768`, `max_position_embeddings 512` (HF API, 2026-10-01). Chosen over `MoritzLaurer/DeBERTa-v3-base-mnli-fever-anli` (MIT weights, but trained partly on ANLI, whose data is CC BY-NC 4.0; not worth the licence question for a possible commercial release).
- Licences: candle-* MIT OR Apache-2.0; tokenizers 0.22.2 Apache-2.0; ort 2.0.0-rc.13 MIT OR Apache-2.0, but its default `download-binaries` fetches ONNX Runtime at build time (fallback only).
- Plan review (root-only, plan-reviewer brief): VERDICT revise → applied: `PerMille` newtype (illegal states), named `Grounding` struct (abstraction clarity), lazy model load + drop before `script` (resource lifecycle; "one model stage at a time").
- Ollama at localhost:11434 has `nomic-embed-text` (Apache-2.0, 137M, 768 dims) and `llama3.1:8b`; `POST /v1/embeddings` answers (checked 2026-10-01).
- The Tunguska sources share one sentence verbatim ("In June 1908 an explosion flattened about 80 million trees over the Tunguska forest.") — `examples/tunguska/sources/*/`; live, llama3.1:8b words it differently per chunk (handoff.md "Out of scope").

CONSUMERS:
- `Evidence` (struct literal; new field) → `crates/podling-core/src/stages/extract_claims.rs:161`, `crates/podling-types/src/ledger.rs:124` (test), `crates/podling-types/tests/roundtrip.rs:32` (test). Read-only consumers: `ledger.rs:classify` (uses stance, group), `plugin/llm.rs` (FakeLlm reads ledger status), `stages/script.rs` (ledger passed to model as JSON: gains `basis` only when configured).
- `EpisodeSpec` (new optional fields) → parsed from TOML in `crates/podling-cli/src/commands.rs:25`, `crates/podling-core/src/plugin/mod.rs:73` (test), `crates/podling-core/tests/pipeline.rs:25,172`; written as `episode.json` by `pipeline.rs:97`. No struct literals (fields added with `#[serde(default)]`).
- `SCHEMA_VERSION` → `crates/podling-core/src/cache.rs:113` (foreign version = miss), `crates/podling-core/tests/pipeline.rs:75`, `schema_snapshot.rs:19`.
- Stage list constants → `crates/podling-cli/tests/cli.rs:17`, `crates/podling-core/tests/pipeline.rs:15` (unchanged for no-config runs; new constants for configured runs).
- `pipeline::run` / `run_with_llm` signatures → unchanged (providers are built from `spec` inside); callers `crates/podling-cli/src/commands.rs:29`, `tests/pipeline.rs`.
- `OpenAiCompat` → `plugin/mod.rs:31`, `tests/openai_provider.rs`; the refactor keeps its public API.

## Risk & rollback
- **candle DeBERTa-v3 correctness** is the top risk; unit 1 proves parity against transformers before anything depends on it, with the `ort` fallback (1.5).
- **False contradictions**: NLI models over-call contradiction on partial overlap. `CONTRADICT_PM` is set high (900) and only for other groups' windows retrieved by similarity. Live results are recorded so the thresholds can be tuned by evidence.
- **Byte-identical output**: artifact *bodies* match a golden taken before any change. The envelope's `schema_version` must go 2→3, per the user's bump rule. This interpretation is stated here for the user.
- **tokenizers without default features** may fail to load `tokenizer.json`; the spike finds out and records the minimal feature set.
- **Floating point**: CPU results can differ in the last bits between machines. Per-mille rounding plus the content cache keep one machine's reruns stable. Across machines, a pair exactly on a threshold could flip; this is accepted and documented.
- Rollback: revert the branch; the cache treats schema-2 entries as foreign misses.

## Out of scope
- The Contested-claim LLM adjudicator (next phase; `docs/handoff.md` will say so).
- Replacing the lexical grounding check in `extract_claims.rs` with NLI. It is cheap once `NliProvider` exists (one pair per claim: chunk window ⊨ claim), so it goes in the handoff as a follow-up.
- GPU NLI (candle `cuda` feature), TTS, MCP connectors, PDF ingestion, token budgeting.

## Spike results (unit 1, merged d70c35b)
- candle 0.11 `DebertaV2SeqClassificationModel` matches transformers 5.17 on all 6 reference pairs, batched with padding and alone: **largest difference 0.000004** (tolerance tightened to 0.001), **~55–60 ms per pair** on CPU (dependencies at opt-level 3 in dev).
- `tokenizers` needs **no** default features: this `tokenizer.json` is Precompiled normaliser + Metaspace + Unigram, no regex, so no `onig`/C code.
- Weights are loaded with `VarBuilder::from_buffered_safetensors` (safe); `from_mmaped_safetensors` is an `unsafe fn` and the workspace forbids `unsafe`.
- Reference scores (entailment / neutral / contradiction): the paraphrase 0.998/0.002/0.000; **its reverse 0.000/1.000/0.000** (the premise lacks "over the Tunguska forest"), so bidirectional merging only joins claims with equal information and the stance stage corroborates the rest; 1908→1907 0.002/0.003/0.995; unrelated Kulik/crater **0.000/0.097/0.903** (a false contradiction; hence `CONTRADICT_PM = 950` plus `MIN_CONTRADICT_COS`); away→towards 0.000/0.000/1.000; two-sentence window 0.994/0.006/0.000.
- Licences of the 108 crates added to `Cargo.lock`: all permissive (MIT / Apache-2.0 / BSD-2 / Zlib / Unicode-3.0 / BSL-1.0 / Unlicense alternatives); `r-efi` is `MIT OR Apache-2.0 OR LGPL-2.1+`, used under MIT. No GPL-only, AGPL or non-commercial terms.

## Execution notes (units 3–5)
- Unit 5 dependency audit (5.6): **no new crates**. `blake3` (already a workspace dependency) gained a use in podling-core in unit 3.
- Deviations from the plan text, both deliberate:
  - The `CrossEncoderNli` fingerprint has no separate model id; the BLAKE3 hash covers `config.json`, which names the model.
  - The `nli_batch` span is at `debug` level (one per 16 pairs); the model load and each stage's summary are at `info`.
- Beyond the plan, needed for correctness: `CoreError::provider()` exposes the failing plugin, so the CLI hint for an embedding-server 401/404/unreachable names `[embedding]`'s URL and key variable, not `[llm]`'s.
- Live embeddings test (5.7), Ollama `nomic-embed-text`: paraphrase cosine 0.985, unrelated 0.349.

## Live results
Setup: Ollama `llama3.1:8b` (LLM) and `nomic-embed-text` (embeddings); `cross-encoder/nli-deberta-v3-base` on CPU. Every run is `target/debug/podling run --episode examples/tunguska/episode-ollama.toml --cache-dir <dir>`. `extract_claims` output varies between cold runs: the LLM sometimes copies the sentence both sources share word for word, and sometimes paraphrases one copy.

| Run | Cache | Exit | Claims (extracted → ledger) | Merges | Stance evidence | Statuses | Analysis errors | "80 million trees" |
|---|---|---|---|---|---|---|---|---|
| Baseline (6.2), no `[embedding]`/`[nli]` | empty | 0 | 10 → 10 | – | – | 10 SingleSource | 0 | two SingleSource claims, one per group: "In June 1908 an explosion flattened…" (expedition) and "An explosion flattened … in June 1908." (eyewitness) |
| 1 (6.3) | empty | 0 | 7 → 7 | 0 | 0 / 0 | 1 Corroborated, 6 SingleSource | 0 | Corroborated by **exact wording**: the LLM extracted the same sentence from both sources |
| 2 (6.3) | empty | 0 | 9 → 9 | 0 (0 pairs) | 0 / 0 (32 pairs) | 1 Corroborated, 8 SingleSource | 0 | Corroborated by **exact wording**, as in run 1 |
| 3 | empty | 0 | 8 → 7 | 1 | 0 / 0 (24 pairs) | 1 Corroborated, 6 SingleSource | 0 | Corroborated by **NLI**: the eyewitness paraphrase merged into the expedition wording, `Merged { entailment_pm: 997 }` |
| A/B, grounded | new keys (effectively cold) | 0 | 10 → 9 | 1 (2 pairs) | 0 / 0 (32 pairs) | 1 Corroborated, 8 SingleSource | 0 | Corroborated by **NLI**, `entailment_pm` 997 |
| A/B, same cache, grounding removed | extract_claims hit | 0 | 10 → 10 | – | – | 10 SingleSource | 0 | the same two wordings stay two SingleSource claims |

Stage timings, run 2 (ms): ingest 7, chunk 2, extract_claims 16791, cluster_claims 614, score_stances 3047 (NLI model load 672 of it), ledger 3, script 116828, analyse 3.

Reading:
- Runs 1 and 2 meet 6.3 as written: exit 0, no analysis errors, and a claim Corroborated across groups that the baseline had as SingleSource. But the corroboration there came from matching wording.
- Run 3 and the A/B pair show the goal itself. On identical extracted claims, only the grounding stages turn two SingleSource wordings into one Corroborated claim.
- `score_stances` added no evidence in any live run, correctly: the Tunguska sources share only the one fact, and neither contradicts the other. Contradiction is covered by the offline fixtures, not by this example.
- The first grounded attempt failed in the script stage. The 8B model cited a *chunk* id as a claim twice, both times the chunk id the new `Merged` evidence had added to the ledger entry. Fix (unit 6): the script request now shows each claim's id, text and status only (`LedgerClaim`), not its evidence; script `VERSION` 6→7. The golden test confirms that fake-LLM artifacts are unchanged.
- 6.4, the ignored live LLM test: passed (53.6 s).
