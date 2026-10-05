# Tracker

| Plan | Unit | Scope | Module | Language | Security | Status | Updated |
|---|---|---|---|---|---|---|---|
| [phase1-core-contracts](./phase1-core-contracts.md) | 1 | ws-scaffold | workspace root | rust | normal | COMPLETE | 2026-09-30 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 2 | artifact-types | crates/podling-types | rust | normal | COMPLETE | 2026-09-30 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 3 | content-cache | crates/podling-core/src/cache.rs | rust | normal | COMPLETE | 2026-09-30 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 4 | plugin-contracts | crates/podling-core/src/plugin | rust | high | COMPLETE | 2026-09-30 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 5 | stage-pipeline | crates/podling-core/src/pipeline | rust | normal | COMPLETE | 2026-09-30 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 6 | cli | crates/podling-cli | rust | normal | COMPLETE | 2026-09-30 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 7 | docs | README.md, docs/architecture.md | markdown | normal | COMPLETE | 2026-09-30 |
| [scrutinise-phase1-core-contracts](./scrutinise-phase1-core-contracts.md) | 1 | scrutinise-fixes | crates/podling-core | rust | high | COMPLETE | 2026-09-30 |
| [phase2-llm-provider](./phase2-llm-provider.md) | 1 | llm-config | crates/podling-types | rust | normal | COMPLETE | 2026-09-30 |
| [phase2-llm-provider](./phase2-llm-provider.md) | 2 | openai-provider | crates/podling-core/src/plugin/openai.rs | rust | high | COMPLETE | 2026-09-30 |
| [phase2-llm-provider](./phase2-llm-provider.md) | 3 | grounded-prompts | crates/podling-core/src/stages | rust | high | COMPLETE | 2026-09-30 |
| [phase2-llm-provider](./phase2-llm-provider.md) | 4 | llm-cli-docs | crates/podling-cli | rust | normal | COMPLETE | 2026-09-30 |
| [scrutinise-phase2-llm-provider](./scrutinise-phase2-llm-provider.md) | 1 | quote-verifier | crates/podling-core/src/plugin/analyser.rs | rust | high | COMPLETE | 2026-09-30 |
| [scrutinise-phase2-llm-provider](./scrutinise-phase2-llm-provider.md) | 2 | claim-grounding | crates/podling-core/src/stages/extract_claims.rs | rust | high | COMPLETE | 2026-09-30 |
| [scrutinise-phase2-llm-provider](./scrutinise-phase2-llm-provider.md) | 3 | live-test-guard | crates/podling-cli/tests/cli.rs | rust | normal | COMPLETE | 2026-09-30 |
| [scrutinise-phase2-llm-provider](./scrutinise-phase2-llm-provider.md) | 4 | script-input-size | crates/podling-core/src/stages/script.rs | rust | normal | COMPLETE | 2026-09-30 |
| [scrutinise-phase2-llm-provider](./scrutinise-phase2-llm-provider.md) | 5 | typed-provider-error | crates/podling-cli/src/commands.rs | rust | normal | COMPLETE | 2026-09-30 |
| [script-verbatim-retry](./script-verbatim-retry.md) | 1 | script-verbatim-retry | crates/podling-core/src/stages/script.rs | rust | high | COMPLETE | 2026-09-30 |
| [quote-placeholders](./quote-placeholders.md) | 1 | quote-placeholders | crates/podling-core/src/stages/script.rs | rust | high | COMPLETE | 2026-09-30 |
| [quote-placeholders](./quote-placeholders.md) | 2 | heading-grounding | crates/podling-core/src/stages/extract_claims.rs | rust | high | COMPLETE | 2026-09-30 |
| [quote-placeholders](./quote-placeholders.md) | 3 | live-acceptance | examples/tunguska | rust | normal | COMPLETE | 2026-09-30 |
| [scrutinise-quote-placeholders](./scrutinise-quote-placeholders.md) | 1 | quote-mark-balance | crates/podling-core/src/text.rs | rust | high | COMPLETE | 2026-09-30 |
| [scrutinise-quote-placeholders](./scrutinise-quote-placeholders.md) | 2 | grounding-share | crates/podling-core/src/stages/extract_claims.rs | rust | high | COMPLETE | 2026-09-30 |
| [scrutinise-quote-placeholders](./scrutinise-quote-placeholders.md) | 3 | live-recheck | examples/tunguska | rust | normal | COMPLETE | 2026-09-30 |
| [phase3-nli-ledger](./phase3-nli-ledger.md) | 1 | nli-spike | crates/podling-core/src/plugin/cross_encoder.rs | rust | normal | MERGED | 2026-10-01 |
| [phase3-nli-ledger](./phase3-nli-ledger.md) | 2 | ledger-contracts | crates/podling-types | rust | normal | MERGED | 2026-10-01 |
| [phase3-nli-ledger](./phase3-nli-ledger.md) | 3 | stance-stage | crates/podling-core/src/stages/score_stances.rs | rust | normal | MERGED | 2026-10-01 |
| [phase3-nli-ledger](./phase3-nli-ledger.md) | 4 | cluster-claims | crates/podling-core/src/stages/cluster_claims.rs | rust | normal | MERGED | 2026-10-01 |
| [phase3-nli-ledger](./phase3-nli-ledger.md) | 5 | real-providers | crates/podling-core/src/plugin | rust | high | MERGED | 2026-10-01 |
| [phase3-nli-ledger](./phase3-nli-ledger.md) | 6 | live-and-docs | examples/tunguska, docs | rust | normal | MERGED | 2026-10-01 |
| [scrutinise-phase3-nli-ledger](./scrutinise-phase3-nli-ledger.md) | 1 | script-prompt-version | crates/podling-core | rust | normal | MERGED | 2026-10-01 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 1 | tts-bakeoff | scripts/tts_bakeoff | python | normal | MERGED | 2026-10-05 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 2 | audio-contracts | crates/podling-types | rust | normal | MERGED | 2026-10-05 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 3 | tts-sidecar | sidecars/tts | python | high | MERGED | 2026-10-05 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 4 | tts-provider | crates/podling-core/src/plugin | rust | high | MERGED | 2026-10-05 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 5 | audio-e2e | crates/podling-core/src/stages | rust | normal | MERGED | 2026-10-05 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 6 | script-beats | crates/podling-types/src/script.rs | rust | normal | MERGED | 2026-10-05 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 7 | beat-chunker | crates/podling-core/src/stages/plan_chunks.rs | rust | normal | PENDING | 2026-10-04 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 8 | asr-verify | crates/podling-core/src/plugin/whisper.rs | rust | normal | PENDING | 2026-10-04 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 9 | full-assembler | crates/podling-core/src/stages/assemble.rs | rust | normal | PENDING | 2026-10-04 |
| [phase5-tts-audio](./phase5-tts-audio.md) | 10 | live-and-docs | examples/tunguska, docs | markdown | normal | PENDING | 2026-10-04 |
| [nli-extraction-grounding](./nli-extraction-grounding.md) | 1 | windows-helper | crates/podling-core/src/stages/windows.rs | rust | normal | MERGED | 2026-10-04 |
| [nli-extraction-grounding](./nli-extraction-grounding.md) | 2 | ground-claims-stage | crates/podling-core/src/stages/ground_claims.rs | rust | normal | MERGED | 2026-10-04 |
| [nli-extraction-grounding](./nli-extraction-grounding.md) | 3 | pipeline-wiring | crates/podling-core/src/pipeline.rs, crates/podling-cli | rust | normal | MERGED | 2026-10-04 |
| [nli-extraction-grounding](./nli-extraction-grounding.md) | 4 | live-check | examples/tunguska | rust | normal | MERGED | 2026-10-04 |
| [nli-extraction-grounding](./nli-extraction-grounding.md) | 5 | docs | docs, README.md, examples/tunguska | markdown | normal | MERGED | 2026-10-04 |
| [scrutinise-nli-extraction-grounding](./scrutinise-nli-extraction-grounding.md) | 1 | window-word-cap | crates/podling-core/src/stages, crates/podling-cli/tests | rust | normal | MERGED | 2026-10-04 |
