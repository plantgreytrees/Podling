---
slug: phase1-core-contracts
craftsman_version: 2.1.0
goal: "`podling run` produces a fully cached, schema-validated episode artifact set (documents → chunks → claims → ledger → script → analysis) offline with a fake LLM."
classification: in-scope   # .claude/CLAUDE.md "Decisions (2026-09-29)" — Rust core, plugin kinds, claim ledger, artifacts-as-JSON-Schema, content-hash cache
tracker_rows: ["TRACKER#phase1-core-contracts::1", "TRACKER#phase1-core-contracts::2", "TRACKER#phase1-core-contracts::3", "TRACKER#phase1-core-contracts::4", "TRACKER#phase1-core-contracts::5", "TRACKER#phase1-core-contracts::6", "TRACKER#phase1-core-contracts::7"]
guards:
  blast_radius: done   # greenfield: no existing consumers (repo has no source files)
  completeness_sweep: done
  blind_rederivation: "skipped(root-only agent mode: no fresh agent available; plan-strategist exception unusable, see Risk)"
coverage:
  contract: "2.1-2.8, 6.2 (artifact types + JSON Schema export + schema snapshot test); consumers N/A(greenfield)"
  data: "3.1-3.5 (on-disk content-addressed cache; no database/migrations)"
  config: "1.1-1.3 (workspace, toolchain, lints), 2.7 (EpisodeSpec TOML), 6.4 (example episode)"
  security: "4.4 (LocalFilesConnector root confinement); 2.7 (no secret fields in EpisodeSpec, only env-var names)"
  tests: "every unit (unit tests per module + 5.6 pipeline integration + 6.5 CLI integration)"
  observability: "5.5 (tracing span per stage: id, version, cache hit/miss, duration), 6.1 (--verbose / RUST_LOG)"
  interface: "6.1-6.3 (clap CLI: schema export, run, cache stats|clear)"
  docs: "7.1-7.2 (README, docs/architecture.md)"
  rollback: "greenfield; each unit is one merge, reverted with `git revert <sha>`"
units:
  - id: 1
    scope_id: ws-scaffold
    project: .
    depends_on: []
    module: workspace root
    language: rust
    security: normal
    scope:
      read: [.claude/CLAUDE.md, .gitignore]
      docs: [docs/plans/phase1-core-contracts.md]
      write: [Cargo.toml, Cargo.lock, rust-toolchain.toml, clippy.toml, rustfmt.toml, .gitignore,
              crates/podling-types/Cargo.toml, crates/podling-types/src/lib.rs,
              crates/podling-core/Cargo.toml, crates/podling-core/src/lib.rs,
              crates/podling-cli/Cargo.toml, crates/podling-cli/src/main.rs]
    tooling: { implementer: implementer, gates: [code-reviewer, dependency-auditor], skills: [language-aware-planning], guards: [secrets-scan], mcp: [] }
  - id: 2
    scope_id: artifact-types
    project: .
    depends_on: [ws-scaffold]
    module: crates/podling-types
    language: rust
    security: normal
    scope:
      read: [Cargo.toml, crates/podling-types/Cargo.toml, clippy.toml]
      docs: [docs/plans/phase1-core-contracts.md, .claude/CLAUDE.md]
      write: [crates/podling-types/Cargo.toml, crates/podling-types/src/lib.rs, crates/podling-types/src/ids.rs,
              crates/podling-types/src/document.rs, crates/podling-types/src/quote.rs, crates/podling-types/src/claim.rs,
              crates/podling-types/src/ledger.rs, crates/podling-types/src/script.rs, crates/podling-types/src/episode.rs,
              crates/podling-types/src/envelope.rs, crates/podling-types/src/schema.rs, crates/podling-types/src/analysis.rs,
              crates/podling-types/tests/roundtrip.rs, crates/podling-types/tests/schema_snapshot.rs,
              crates/podling-types/tests/snapshots/*]
    tooling: { implementer: implementer, gates: [code-reviewer, api-reviewer, idiom-reviewer], skills: [language-aware-planning], guards: [secrets-scan], mcp: [] }
  - id: 3
    scope_id: content-cache
    project: .
    depends_on: [artifact-types]
    module: crates/podling-core/src/cache.rs
    language: rust
    security: normal
    scope:
      read: [crates/podling-types/src/ids.rs, crates/podling-core/Cargo.toml]
      docs: [docs/plans/phase1-core-contracts.md]
      write: [crates/podling-core/Cargo.toml, crates/podling-core/src/lib.rs, crates/podling-core/src/error.rs, crates/podling-core/src/cache.rs]
    tooling: { implementer: implementer, gates: [code-reviewer, performance-reviewer], skills: [language-aware-planning], guards: [secrets-scan], mcp: [] }
  - id: 4
    scope_id: plugin-contracts
    project: .
    depends_on: [artifact-types, content-cache]
    module: crates/podling-core/src/plugin
    language: rust
    security: high   # filesystem reads from user-supplied paths
    scope:
      read: [crates/podling-types/src/lib.rs, crates/podling-types/src/document.rs, crates/podling-types/src/quote.rs,
             crates/podling-types/src/script.rs, crates/podling-types/src/episode.rs, crates/podling-core/src/error.rs]
      docs: [docs/plans/phase1-core-contracts.md, .claude/CLAUDE.md]
      write: [crates/podling-core/Cargo.toml, crates/podling-core/src/lib.rs, crates/podling-core/src/error.rs,
              crates/podling-core/src/text.rs, crates/podling-core/src/plugin/mod.rs, crates/podling-core/src/plugin/llm.rs,
              crates/podling-core/src/plugin/source.rs, crates/podling-core/src/plugin/analyser.rs]
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor, idiom-reviewer], skills: [language-aware-planning], guards: [secrets-scan], mcp: [] }
  - id: 5
    scope_id: stage-pipeline
    project: .
    depends_on: [content-cache, plugin-contracts]
    module: crates/podling-core/src/pipeline
    language: rust
    security: normal
    scope:
      read: [crates/podling-types/src/lib.rs, crates/podling-core/src/cache.rs, crates/podling-core/src/error.rs,
             crates/podling-core/src/plugin/mod.rs, crates/podling-core/src/plugin/llm.rs,
             crates/podling-core/src/plugin/source.rs, crates/podling-core/src/plugin/analyser.rs]
      docs: [docs/plans/phase1-core-contracts.md]
      write: [crates/podling-core/Cargo.toml, crates/podling-core/src/lib.rs, crates/podling-core/src/error.rs,
              crates/podling-core/src/stage.rs, crates/podling-core/src/pipeline.rs,
              crates/podling-core/src/stages/mod.rs, crates/podling-core/src/stages/ingest.rs,
              crates/podling-core/src/stages/chunk.rs, crates/podling-core/src/stages/extract_claims.rs,
              crates/podling-core/src/stages/ledger.rs, crates/podling-core/src/stages/script.rs,
              crates/podling-core/src/stages/analyse.rs, crates/podling-core/tests/pipeline.rs,
              crates/podling-core/tests/fixtures/*]
    tooling: { implementer: implementer, gates: [code-reviewer, observability-reviewer, idiom-reviewer], skills: [language-aware-planning], guards: [secrets-scan], mcp: [] }
  - id: 6
    scope_id: cli
    project: .
    depends_on: [stage-pipeline]
    module: crates/podling-cli
    language: rust
    security: normal
    scope:
      read: [crates/podling-core/src/lib.rs, crates/podling-core/src/pipeline.rs, crates/podling-core/src/cache.rs,
             crates/podling-types/src/schema.rs, crates/podling-types/src/episode.rs]
      docs: [docs/plans/phase1-core-contracts.md]
      write: [crates/podling-cli/Cargo.toml, crates/podling-cli/src/main.rs, crates/podling-cli/src/commands.rs,
              crates/podling-cli/tests/cli.rs, examples/tunguska/episode.toml, examples/tunguska/sources/*]
    tooling: { implementer: implementer, gates: [code-reviewer, idiom-reviewer], skills: [language-aware-planning], guards: [secrets-scan], mcp: [] }
  - id: 7
    scope_id: docs
    project: .
    depends_on: [cli]
    module: docs
    language: markdown
    security: normal
    scope:
      read: [Cargo.toml, crates/podling-cli/src/main.rs, crates/podling-core/src/cache.rs, crates/podling-core/src/stage.rs, .claude/CLAUDE.md]
      docs: [docs/plans/phase1-core-contracts.md]
      write: [README.md, docs/architecture.md]
    tooling: { implementer: implementer, gates: [], skills: [], guards: [secrets-scan], mcp: [] }
---

# Plan: Podling Phase 1 — core contracts, cache, pipeline, CLI

## Outcome
`podling run --episode examples/tunguska/episode.toml` produces a fully cached, schema-validated episode artifact set (documents → chunks → claims → ledger → script → analysis) offline with a fake LLM, and a second run is 100% cache hits.

## Prerequisites (user actions — block `/orchestrate`)
- **P1 Rust toolchain is not installed** (`cargo`, `rustc`, `clippy`, `rustfmt` absent on PATH). Install: `sudo pacman -S rustup && rustup default stable && rustup component add clippy rustfmt`.
- **P2 The repo is not a git repository.** `/orchestrate` merges each unit to a base branch, so it needs `git init` plus an initial commit of the craftsman scaffold.

## Scope Steps (executable core)

### Step 1 — ws-scaffold (., rust, normal)
Tooling: implementer · gates code-reviewer, dependency-auditor · skills language-aware-planning · guards secrets-scan
Depends on: none
- [x] 1.1 Create root `Cargo.toml` as a virtual workspace (`resolver = "3"`, `edition = "2024"`, members `crates/*`), with `[workspace.dependencies]` for serde (derive), serde_json, schemars, blake3, thiserror, anyhow, clap (derive), toml, tracing, tracing-subscriber (env-filter); dev-deps tempfile, assert_cmd, predicates, insta; and `[workspace.lints]` (`rust.unsafe_code = "forbid"`, `clippy.all = "warn"`) → accept: every crate uses `workspace = true` for deps and lints.
- [x] 1.2 Add `rust-toolchain.toml` (channel stable, components clippy + rustfmt), `rustfmt.toml` (defaults), and `clippy.toml` with `disallowed-types = ["std::collections::HashMap", "std::collections::HashSet"]` (artifact hashing must be deterministic; use BTreeMap/BTreeSet) → accept: `cargo clippy` flags a HashMap if one is introduced.
- [x] 1.3 Create the three crates: `podling-types` (lib), `podling-core` (lib, depends on types), `podling-cli` (bin named `podling`, depends on core + types) with empty `lib.rs`/`main.rs`; append `target/` to `.gitignore` → accept: `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings` pass.

### Step 2 — artifact-types (., rust, normal)
Tooling: implementer · gates code-reviewer, api-reviewer, idiom-reviewer · skills language-aware-planning · guards secrets-scan
Depends on: ws-scaffold
All public artifact types derive `Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq`; collections are `Vec`/`BTreeMap` only.
- [x] 2.1 `ids.rs`: `ContentHash` newtype (blake3 → lowercase hex, `FromStr` validates 64 hex chars) plus typed ids `SourceId`, `DocumentId`, `ChunkId`, `ClaimId`, `SpeakerId` (newtypes, `#[serde(transparent)]`) → accept: unit test confirms two different id types can't be swapped (compile-level) and `ContentHash::from_str` rejects bad hex.
- [x] 2.2 `document.rs`: `SourceRef { connector: String, locator: String, independence_group: String }`, `Document { id, source: SourceRef, title, text }` where `id` = hash of the text; `TextSpan { start, end }` (byte offsets; constructor rejects `start > end`); `Chunk { id, document: DocumentId, span: TextSpan, heading_path: Vec<String>, text }` → accept: `Document::new` derives the same id for the same text; the span test passes.
- [x] 2.3 `quote.rs`: `Quote { document, span, text }` with **private fields** and only `Quote::from_document(&Document, TextSpan) -> Result<Quote, QuoteError>` (rejects out-of-bounds and non-char-boundary spans; copies `text` from the document slice); deserialisation goes through `#[serde(try_from = "RawQuote")]` for shape only (verbatim check is the analyser's job, 4.5) → accept: no public constructor accepts free text; tests cover out-of-bounds, mid-UTF-8 boundary, and happy path.
- [x] 2.4 `claim.rs`: `Stance { Supports, Contradicts }`, `Evidence { chunk: ChunkId, source: SourceId, independence_group: String, stance }`, `Claim { id, text, evidence: Vec<Evidence> }` with `id` = hash of normalised text (lowercase, whitespace-collapsed) → accept: claims that differ only in case or whitespace get equal ids.
- [x] 2.5 `ledger.rs`: `ClaimStatus` as an enum carrying data: `Corroborated { groups: Vec<String> }` (≥2), `SingleSource { group: String }`, `Contested { supporting: Vec<String>, contradicting: Vec<String> }`, `Unsupported`; pure `classify(&Claim) -> ClaimStatus` counting **distinct independence groups**, not documents; `Ledger { entries: Vec<LedgerEntry { claim, status }> }` built only via `Ledger::from_claims` → accept: table-driven test covers all four statuses, including two documents in the same group → `SingleSource`.
- [x] 2.6 `script.rs`: `Speaker { id, name, role }`, `Emotion` enum (`Neutral, Curious, Excited, Serious, Amused, Somber`), `Turn { speaker: SpeakerId, text, emotion, citations: Vec<ClaimId>, quotes: Vec<Quote> }`, `Script { cast, turns }` built via `Script::new` that rejects turns whose speaker isn't in the cast and an empty cast → accept: tests for unknown speaker, empty cast, and valid script.
- [x] 2.7 `episode.rs`: `EpisodeSpec { title, topic, mode: Mode, target_minutes: u16, sources: Vec<SourceSpec>, llm: LlmConfig, analysers: Vec<AnalyserConfig> }`; `Mode` is `#[non_exhaustive]` with only `NonFiction`; `SourceSpec` is `#[serde(tag = "kind")]` with `LocalFiles { root: PathBuf, independence_group: String }`; `LlmConfig` is tagged with `Fake` only (an `OpenAiCompat` variant lands with the provider that implements it); `AnalyserConfig` is tagged with `QuoteVerifier`; **no secret-valued fields** (future keys are env-var *names*); `deny_unknown_fields` everywhere → accept: parsing a TOML fixture succeeds and an unknown key fails with an error naming the field.
- [x] 2.8 `envelope.rs` + `schema.rs`: `pub const SCHEMA_VERSION: u32 = 1`; `Envelope<T> { schema_version, kind: ArtifactKind, body: T }`; `schema::all() -> Vec<(ArtifactKind, schemars::Schema)>` for EpisodeSpec, Document, Chunk, Ledger, Script, AnalysisReport-placeholder (moved in 4.5 if needed); `tests/schema_snapshot.rs` snapshots every schema with `insta`; `tests/roundtrip.rs` does serde JSON round-trips of a sample of every artifact → accept: `cargo test -p podling-types` passes; changing any field fails the snapshot test (the documented rule is to bump `SCHEMA_VERSION` when accepting the snapshot).

### Step 3 — content-cache (., rust, normal)
Tooling: implementer · gates code-reviewer, performance-reviewer · skills language-aware-planning · guards secrets-scan
Depends on: artifact-types
- [x] 3.1 `error.rs`: `CoreError` (`thiserror`) with variants `Io { path, source }`, `Serde`, `Cache`, `Provider { plugin, message }`, `InvalidProviderOutput { stage, source }`, `Stage { stage, source: Box<CoreError> }`, `Source` → accept: every variant has an `#[error]` message naming its context.
- [x] 3.2 `cache.rs`: `CacheKey::new(stage_id: &str, stage_version: u32, input: &impl Serialize, config: &impl Serialize) -> Result<CacheKey>` = blake3 over a length-prefixed concatenation of stage_id, version, and **canonical JSON** (`serde_json::to_value` then `to_vec`; serde_json's default `BTreeMap` gives sorted keys; do not enable `preserve_order`) → accept: test shows key-order-insensitive inputs hash equal, and changing version or config changes the key.
- [x] 3.3 `DiskCache { root }` with `get<T: DeserializeOwned>(&CacheKey) -> Result<Option<T>>` and `put<T: Serialize>(&CacheKey, &T)`; files stored at `root/<first 2 hex>/<hash>.json` wrapped in `Envelope`; writes are atomic (`tempfile::NamedTempFile::new_in(dir)` → `persist`) → accept: a test writes then reads back the same value; no partial file is ever visible under the final name.
- [x] 3.4 Treat a corrupt/undeserialisable entry or a `schema_version` mismatch as a **miss** with `tracing::warn!` (key, reason), never an error → accept: a test with a truncated file returns `Ok(None)`.
- [x] 3.5 `DiskCache::stats() -> CacheStats { entries, bytes }` and `clear()` → accept: tests on a tempdir.

### Step 4 — plugin-contracts (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor, idiom-reviewer · skills language-aware-planning · guards secrets-scan
Depends on: artifact-types, content-cache
Only traits with a phase-1 implementation are defined. TTS/Embedding/NLI/ASR traits are deferred to the phases that implement them (abstraction budget).
- [x] 4.1 `plugin/llm.rs`: `trait LlmProvider { fn id(&self) -> &str; fn fingerprint(&self) -> serde_json::Value; fn complete(&self, req: &CompletionRequest) -> Result<Completion, CoreError>; }` with `CompletionRequest { task: LlmTask, instructions: String, input: serde_json::Value }`, `LlmTask { ExtractClaims, WriteScript }`, `Completion { text }` → accept: the trait is object-safe (`Box<dyn LlmProvider>` compiles).
- [x] 4.2 `FakeLlm` (in `plugin::llm`, not `cfg(test)`, because the CLI runs it offline): deterministic rule-based. `ExtractClaims` returns JSON `[{ "text": <sentence> }]` for each sentence of `input.chunk_text`. `WriteScript` returns a `ScriptDraft` JSON: two-speaker cast, one turn per ledger entry, and on the first turn a `QuoteRef { document, start, end }` pointing at the first sentence of the first chunk → accept: the same input gives byte-identical output (test).
- [x] 4.3 `plugin/source.rs`: `trait SourceConnector { fn id(&self) -> &str; fn fetch(&self) -> Result<Vec<Document>, CoreError>; }` + `LocalFilesConnector { root, independence_group }` reading `*.md`/`*.txt` in sorted path order → accept: a tempdir test returns documents in a deterministic order with `SourceRef.connector == "local_files"`.
- [x] 4.4 **Security:** `LocalFilesConnector` canonicalises `root` and every entry and skips (with `warn!`) any path whose canonical form isn't under the canonical root (symlink escape); non-UTF-8 files → `CoreError::Source` naming the file; files over 10 MiB are skipped with a warning → accept: tests with a symlink pointing outside root, an oversized file, and invalid UTF-8.
- [x] 4.5 `plugin/analyser.rs`: `trait Analyser { fn id(&self) -> &str; fn analyse(&self, script: &Script, docs: &[Document]) -> Vec<Finding>; }`, `Finding { analyser, severity: Severity { Info, Warning, Error }, turn: Option<usize>, message }`, `AnalysisReport { findings }` (move the schema entry here if 2.8 used a placeholder); `QuoteVerifier` flags each `Quote` whose `text` ≠ the document slice at its span, and each turn whose `text` doesn't contain its quote's text → accept: tests for a tampered quote text (Error) and a paraphrased quote in turn text (Error).
- [x] 4.6 `plugin/mod.rs`: `build_llm(&LlmConfig) -> Box<dyn LlmProvider>`, `build_sources(&[SourceSpec]) -> Vec<Box<dyn SourceConnector>>`, `build_analysers(&[AnalyserConfig]) -> Vec<Box<dyn Analyser>>`, each a plain `match` (no registry) → accept: a test builds all three from a parsed `EpisodeSpec`.

### Step 5 — stage-pipeline (., rust, normal)
Tooling: implementer · gates code-reviewer, observability-reviewer, idiom-reviewer · skills language-aware-planning · guards secrets-scan
Depends on: content-cache, plugin-contracts
- [x] 5.1 `stage.rs`: `trait Stage { const ID: &'static str; const VERSION: u32; type Input: Serialize; type Output: Serialize + DeserializeOwned; fn run(&self, input: &Self::Input) -> Result<Self::Output, CoreError>; fn config_fingerprint(&self) -> serde_json::Value; }` + `fn cached<S: Stage>(stage: &S, input: &S::Input, cache: &DiskCache, report: &mut RunReport) -> Result<S::Output>` → accept: a unit test with a counting stage shows `run` is called once across two `cached` calls.
- [x] 5.2 `stages/ingest.rs` (connectors → `Vec<Document>`; the cache key includes the documents' content hashes, so edited source files invalidate the cache) and `stages/chunk.rs` (split on Markdown headings, then paragraphs; cap about 800 words with a paragraph fallback; record `heading_path`; spans must satisfy `doc.text[span] == chunk.text`) → accept: a property-style test over fixtures confirms that span invariant for every chunk.
- [x] 5.3 `stages/extract_claims.rs`: one `LlmTask::ExtractClaims` call per chunk; parse into `Vec<{text}>` (bad JSON → `CoreError::InvalidProviderOutput` naming the chunk); merge claims by `ClaimId`, appending `Evidence { stance: Supports }`; the fingerprint includes `llm.fingerprint()` → accept: two fixture sources in different independence groups that share one sentence produce a claim with two evidence entries.
- [x] 5.4 `stages/ledger.rs` (`Ledger::from_claims`), `stages/script.rs` (`LlmTask::WriteScript` → `ScriptDraft` → resolve each `QuoteRef` via `Quote::from_document`, where a failed resolution is a `CoreError::InvalidProviderOutput`, then `Script::new`), and `stages/analyse.rs` (runs configured analysers → `AnalysisReport`) → accept: the fixture run yields at least one `Corroborated` and one `SingleSource` entry and an `AnalysisReport` with zero Error findings.
- [x] 5.5 `pipeline.rs`: `run(spec: &EpisodeSpec, cache: &DiskCache, out_dir: &Path) -> Result<RunReport>` calls the stages in order inside `tracing::info_span!("stage", id, version)`, logging `cache_hit` and `elapsed_ms`; writes each artifact as `out_dir/<kind>.json` in an `Envelope`; `RunReport { stages: Vec<StageRecord { id, version, key, cache_hit, elapsed_ms }> }` → accept: `RunReport` lists all 6 stages in order.
- [x] 5.6 `tests/pipeline.rs` + `tests/fixtures/` (two small Markdown sources in different independence groups sharing one sentence): run twice → second `RunReport` is all `cache_hit`; edit one fixture copy in a tempdir → ingest and every downstream stage miss → accept: `cargo test -p podling-core` passes.

### Step 6 — cli (., rust, normal)
Tooling: implementer · gates code-reviewer, idiom-reviewer · skills language-aware-planning · guards secrets-scan
Depends on: stage-pipeline
- [x] 6.1 `main.rs`: clap derive with a global `--verbose` flag and `tracing_subscriber` using `EnvFilter` (`RUST_LOG` takes precedence) and `--cache-dir` (default `.podling/cache`); `anyhow` for errors with `.context(...)` at each call → accept: `podling --help` lists `schema`, `run`, `cache`.
- [x] 6.2 `podling schema export --out <dir>` writes `<kind>.schema.json` for every entry in `schema::all()` → accept: files exist and each parses as JSON.
- [x] 6.3 `podling run --episode <file.toml> [--out <dir>] [--no-cache]` prints a stage table (id, hit/miss, ms) and the output dir; `podling cache stats|clear` → accept: exit code 0 on success, non-zero with a readable error for a missing episode file.
- [x] 6.4 `examples/tunguska/episode.toml` + `examples/tunguska/sources/{eyewitness.md, expedition.md}` (short, hand-written, two independence groups, one shared factual sentence) → accept: `cargo run -p podling-cli -- run --episode examples/tunguska/episode.toml` succeeds.
- [x] 6.5 `tests/cli.rs` (`assert_cmd`, tempdir cache/out): run the example twice and assert the second run's output reports every stage as a hit; `schema export` writes the expected file set → accept: `cargo test --workspace` passes.

### Step 7 — docs (., markdown, normal)
Tooling: implementer · gates none · guards secrets-scan
Depends on: cli
- [x] 7.1 `README.md`: what Podling is, prerequisites (rustup), `cargo test`, the three CLI commands, the example episode, and the project status (phase 1: offline fake LLM) → accept: every command shown is copy-pasteable and was run once.
- [x] 7.2 `docs/architecture.md`: crate map; artifact flow diagram; the **three bump rules** (field change → accept snapshot + bump `SCHEMA_VERSION`; stage logic change → bump `Stage::VERSION`; provider change → reflected in `fingerprint()`); cache key formula; plugin kinds with the deferred traits roadmap; why a claim ledger instead of debating agents → accept: every cited path exists.

## Acceptance criteria (restore into `.craftsman/acceptance.md` when `/orchestrate` starts)
- [x] `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass.
- [x] No `HashMap`/`HashSet` in any crate (enforced by `clippy.toml` `disallowed-types`).
- [x] `podling schema export --out <dir>` writes one parseable `<kind>.schema.json` per artifact kind, and an `insta` snapshot test fails when any artifact type changes.
- [x] `podling run --episode examples/tunguska/episode.toml` completes offline (FakeLlm) and writes documents/chunks/claims/ledger/script/analysis JSON envelopes carrying `schema_version`.
- [x] A second identical run reports every stage as a cache hit; editing a source file invalidates ingest and all downstream stages; bumping a `Stage::VERSION` invalidates that stage, and downstream stages whenever its output changes (early cutoff: identical output is safely reused).
- [x] A `Quote` can only be built from a document span (`Quote::from_document`); the script stage resolves LLM `QuoteRef`s through it, and `QuoteVerifier` reports an Error for tampered or paraphrased quotes.
- [x] `classify` is a pure function covering all four `ClaimStatus` variants; `Corroborated` requires ≥2 distinct independence groups (two documents in one group → `SingleSource`).
- [x] `Script::new` rejects an empty cast and turns with unknown speakers; `EpisodeSpec` rejects unknown TOML keys and has no secret-valued fields.
- [x] `LocalFilesConnector` skips symlinks escaping its root and files over 10 MiB, and reports non-UTF-8 files as an error naming the file.
- [x] A corrupt or schema-mismatched cache entry is treated as a miss with a warning (never an error); cache writes are atomic.
- [x] Each stage emits a tracing span with id, version, `cache_hit`, and `elapsed_ms`; `RunReport` lists all 6 stages in order.
- [x] `README.md` and `docs/architecture.md` exist; every command and path they cite resolves; the three bump rules are documented.

## Sequencing
Scaffold → types (the contract everything depends on) → cache (depends on ids/envelope) → plugins (depend on types + CoreError) → pipeline (composes cache + plugins) → CLI (thin shell) → docs. The dependency graph is linear, with no cycles; each step is tests-first within its unit.

## Verification background (citations for the reviewer)
- Rust core, Python only as sidecars; three plugin kinds; claim ledger; quotes as verbatim spans; artifacts as serde + JSON Schema; content-hash cache; licensing constraints — `.claude/CLAUDE.md:6-12`.
- Hardware and sequential model loading (why phase 1 is synchronous, no tokio) — `.claude/CLAUDE.md:13`.
- CONSUMERS: none. The repo contains no source files (greenfield, verified by `ls` at plan time). Future consumers of the JSON Schemas: Python sidecar workers (phase 3+) and any UI.
- Strategist decision (done in-session; the Task exception was blocked): Option B, a 3-crate workspace, typed stages via associated types; trait objects only where config selects the implementation at runtime; rejected A (single crate: boundaries unenforced, split later) and C (JSON-erased stages + dynamic registry: loses illegal-state guarantees, over-abstracts).
- Plan review (plan-reviewer brief applied in-session): VERDICT revise → applied: (1) `disallowed-types` HashMap for deterministic hashing (1.2); (2) schema snapshot test + `SCHEMA_VERSION` bump rule (2.8, 7.2); (3) `Quote` private constructor, with `QuoteRef` resolution at the script stage so the LLM can only point at spans (2.3, 5.4); (4) deferred the TTS/Embed/NLI/ASR traits, which had no implementation (Step 4 note); (5) corrupt cache entry = miss, not error (3.4).

## Risk & rollback
- **Rust learning curve:** units are small and synchronous; the implementer should explain new idioms (associated types, `try_from` serde, trait objects) in code comments sparingly and in the unit summary.
- **Canonical JSON:** relies on serde_json *without* `preserve_order`; if any dependency enables that feature, key-order stability breaks. Test 3.2 guards it.
- **Craftsman tooling:** the `plan-strategist` Task exception is blocked by `agent-mode-guard.mjs` (it matches the bare name, while the registered agent is `craftsman:plan-strategist`), and blind re-derivation couldn't run in root-only mode. Both were done in-session instead.
- **Rollback:** greenfield; each unit is one merge; `git revert <sha>`.

## Out of scope
Real LLM providers (Ollama/OpenAI-compatible HTTP), TTS/embeddings/NLI/ASR traits and models, Python sidecar protocol, MCP connectors, PDF ingestion (pdfium/Docling), NLI-based stance scoring (phase 1 uses Supports-only evidence), audio rendering and mixing, async/tokio, CHANGELOG (pre-release; no users yet).

## Execution log (2026-09-29)
All 7 units are built on branch `worktree-phase1-core-contracts`: 2649fca (ws-scaffold), 676ed57 (artifact-types), eb40391 (content-cache), a4a2c0f (plugin-contracts), a4d1229 (stage-pipeline), 5117688 (cli), plus the docs commit. The gate is green: `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, and `cargo test --workspace` (65 passed, 0 failed); gitleaks is clean. Merged to `main` on 2026-09-30 at the user's request, together with the `scrutinise-phase1-core-contracts` fixes.

Deviations:
- The example and fixture sources live in one directory per independence group (`sources/eyewitness/`, `sources/expedition/`). `LocalFilesConnector` assigns one group per root, so two flat files would share a group.
- `pipeline::run` takes `Option<&DiskCache>` (for `--no-cache`) and `base_dir`.
- `run` exits non-zero when analysers report Error findings.
- The acceptance criterion on `Stage::VERSION` was reworded to describe early cutoff.
- Reviews ran in-session (root-only agent mode).
