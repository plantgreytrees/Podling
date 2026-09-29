---
slug: scrutinise-phase1-core-contracts
goal: Identical sources in different groups still corroborate, `cache clear` can never delete non-cache files, and scripts cannot cite claims that aren't in the ledger.
classification: in-scope   # fixes to phase1-core-contracts, found by /scrutinise on 5a1824d..6a48665
tracker_rows: ["TRACKER#8"]
guards:
  blast_radius: done
  completeness_sweep: done
  blind_rederivation: "skipped(root-only agent mode; the findings were reproduced directly)"
coverage:
  contract:      "1.1 (the locator format of LocalFilesConnector changes, so SourceId/DocumentId values change)"
  data:          "1.2 (the cache layout is unchanged; clear() now deletes only cache-shaped entries)"
  config:        "N/A(no new config keys)"
  security:      "1.2 (clear() must not delete arbitrary directories)"
  tests:         "1.1, 1.2, 1.3, 1.4, 1.5"
  observability: "1.2 (warn when clear() leaves foreign files behind)"
  interface:     "N/A(the CLI surface is unchanged)"
  docs:          "1.6"
steps:
  - id: 1
    scope_id: scrutinise-fixes
    project: .
    depends_on: []
    module: crates/podling-core
    language: rust
    security: high
    scope:
      read: [crates/podling-types/src/document.rs, crates/podling-types/src/ledger.rs, crates/podling-types/src/script.rs]
      docs: [docs/plans/scrutinise-phase1-core-contracts.md, docs/architecture.md]
      write: [crates/podling-types/src/analysis.rs, crates/podling-core/src/plugin/source.rs, crates/podling-core/src/plugin/mod.rs, crates/podling-core/src/pipeline.rs,
              crates/podling-core/src/cache.rs, crates/podling-core/src/stages/script.rs, crates/podling-core/src/stages/chunk.rs,
              crates/podling-core/tests/pipeline.rs, crates/podling-cli/tests/cli.rs, docs/architecture.md, README.md]
    tooling: { implementer: implementer, gates: [code-reviewer, security-auditor], skills: [language-aware-planning], guards: [secrets-scan], mcp: [] }
---

# Plan: fixes from scrutinising phase 1

## Scope Steps (executable core)

### Step 1 — scrutinise-fixes (., rust, high)
Tooling: implementer · gates code-reviewer, security-auditor · guards secrets-scan
Depends on: none
- [x] 1.1 **Locator collision (Major, cross-unit).** Make `LocalFilesConnector`'s locator `<root label>/<file name>`, where the root label is the `root` exactly as written in the episode (relative, portable). `build_sources` passes it in. The connector used on its own defaults to the root path's display form. In `pipeline::run`, return `CoreError::Source` when two fetched documents share a `DocumentId` but have different `SourceRef`s (fail closed) → accept: the regression test (identical `notes.md` in groups `ga`/`gb`) shows `Corroborated { groups: [ga, gb] }`, and it fails on the old code.
- [x] 1.2 **`cache clear` safety (Major, security).** `DiskCache::clear` removes only `<2 lowercase hex>/<64 hex>.json` entries and leftover temp files in those shard directories, then any shard directory and the root that are now empty. Anything else stays, and a `tracing::warn!` names it → accept: a test with a foreign file in the root and one in a shard shows both survive, and all cache entries are gone.
- [x] 1.3 **Unchecked citations (Major, grounding).** In `WriteScript::run`, a `DraftTurn` citation whose `ClaimId` is not in the input ledger → `CoreError::InvalidProviderOutput` naming the turn and the id → accept: a unit test with a fabricated id fails that way, and the fake LLM's scripts still pass.
- [x] 1.4 **Coverage (Minor).** Move the Error-finding count into `AnalysisReport::error_count()` (used by `pipeline::run`) and test it by running `Analyse` with `QuoteVerifier` over a tampered script (see Background) → accept: the test fails if the `error_findings` counting or the non-zero exit is removed.
- [x] 1.5 **Fenced code (Minor).** The chunker ignores heading syntax inside fenced code blocks (lines opening/closing with backtick or tilde fences) → accept: a unit test with a `# not a heading` line inside a fence keeps it as chunk text with no heading path.
- [x] 1.6 **Docs.** `docs/architecture.md` describes the new locator format, the citation check and the safety of `cache clear` → accept: statements match the code.

## Acceptance criteria
- [x] Identical files in two independence groups are Corroborated; a document-id collision with differing sources is an error, not a silent overwrite.
- [x] `podling cache clear` deletes only cache entries; foreign files survive, with a warning.
- [x] A script citing a claim id absent from the ledger is rejected as invalid provider output.
- [x] The error-finding → non-zero exit path is covered by a test.
- [x] Heading syntax inside fenced code blocks is not treated as a heading.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass.

## Verification background
Findings came from `/scrutinise phase1-core-contracts` over `5a1824d..6a48665`. Finding 1 was reproduced: two roots `a/`, `b/` each held `notes.md` with identical text (groups `ga`, `gb`), and both claims came out `single_source {group: gb}`.

For 1.4: `FakeLlm` only ever produces valid quotes, so an end-to-end CLI run cannot produce an Error finding. The seam is `pipeline::run` → `Analyse` → `report.error_findings`. Move the counting into a small helper, `RunReport::count_errors(&AnalysisReport)`, and unit-test it with a tampered-quote analysis.

CONSUMERS:
- `SourceRef.locator` (format change) → `crates/podling-types/src/document.rs:26` (SourceId), `:71` (DocumentId), `crates/podling-core/src/stages/ingest.rs:30` (log), `crates/podling-core/src/plugin/source.rs:143,163,176` (tests), `crates/podling-core/src/plugin/mod.rs:38` (factory). Ids change value only, not shape. There is no schema change and no `SCHEMA_VERSION` bump. The cache invalidates naturally.
- `DiskCache::clear` → `crates/podling-cli/src/commands.rs:64`, `crates/podling-core/src/cache.rs:294-296` (tests).
- `Turn.citations` → produced only at `crates/podling-core/src/stages/script.rs:65`.
- `RunReport.error_findings` → `crates/podling-core/src/pipeline.rs:91`, `crates/podling-cli/src/commands.rs:39`, `crates/podling-core/tests/pipeline.rs:57`.

## Review
The strategist and plan-reviewer roles ran in-session (root-only agent mode). Review note: 1.4 was revised from "a CLI test" to "a helper plus unit test", because the CLI cannot reach an Error finding with `FakeLlm`. Stage versions: bump `WriteScript::VERSION` (1.3) and `ChunkDocuments::VERSION` (1.5), since their logic changes.
