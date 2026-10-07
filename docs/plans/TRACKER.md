# Tracker

Last updated 2026-10-07 (/sync-docs --tracker). Shipped rows of the old hand-written table
are in [TRACKER-archive.md](./TRACKER-archive.md); open work is in the live ledger below.

## Live ledger

<!-- craftsman:ledger:begin -->
<!-- Generated from the tracker ledger (.craftsman/tracker/events.jsonl) after every transition. Do not edit by hand: change a row with scripts/tracker.mjs. -->

| plan | unit | module | status | evidence | updated |
|---|---|---|---|---|---|
| [investigate-sidecar-test-tag](./investigate-sidecar-test-tag.md) | 1 (investigate-sidecar-test-tag) | podling-core sidecar_tts test tags | COMPLETE | all 3 acceptance criteria ticked; 92dc295 merge of 1c3e312; regression test fai… | 2026-10-06 |
| [natural-episode-speech](./natural-episode-speech.md) | 1 (nes-types) | podling-types lexicon/licence/provenance | COMPLETE | all 20 acceptance criteria ticked; session branch 542e494; cargo test 405 passe… | 2026-10-06 |
| [natural-episode-speech](./natural-episode-speech.md) | 2 (nes-core) | podling-core say_as/lexicon/fold/provenance | COMPLETE | all 20 acceptance criteria ticked; session branch 542e494; cargo test 405 passe… | 2026-10-06 |
| [natural-episode-speech](./natural-episode-speech.md) | 3 (nes-voice-design) | scripts/voice_design | COMPLETE | all 20 acceptance criteria ticked; session branch 542e494; cargo test 405 passe… | 2026-10-06 |
| [natural-episode-speech](./natural-episode-speech.md) | 4 (nes-docs) | docs + tunguska example | COMPLETE | all 20 acceptance criteria ticked; session branch 542e494; cargo test 405 passe… | 2026-10-06 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 1 (ws-scaffold) | workspace-root | COMPLETE | /sync-docs --tracker 2026-10-07: shipped 2026-09-30; plan tasks 47/47 ticked; h… | 2026-10-07 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 2 (artifact-types) | crates/podling-types | COMPLETE | /sync-docs --tracker 2026-10-07: shipped 2026-09-30; plan tasks 47/47 ticked; h… | 2026-10-07 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 3 (content-cache) | crates/podling-core/src/cache.rs | COMPLETE | /sync-docs --tracker 2026-10-07: shipped 2026-09-30; plan tasks 47/47 ticked; h… | 2026-10-07 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 4 (plugin-contracts) | crates/podling-core/src/plugin | COMPLETE | /sync-docs --tracker 2026-10-07: shipped 2026-09-30; plan tasks 47/47 ticked; h… | 2026-10-07 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 5 (stage-pipeline) | crates/podling-core/src/pipeline | COMPLETE | /sync-docs --tracker 2026-10-07: shipped 2026-09-30; plan tasks 47/47 ticked; h… | 2026-10-07 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 6 (cli) | crates/podling-cli | COMPLETE | /sync-docs --tracker 2026-10-07: shipped 2026-09-30; plan tasks 47/47 ticked; h… | 2026-10-07 |
| [phase1-core-contracts](./phase1-core-contracts.md) | 7 (docs) | docs | COMPLETE | /sync-docs --tracker 2026-10-07: shipped 2026-09-30; plan tasks 47/47 ticked; h… | 2026-10-07 |
| [phase4-adjudicator](./phase4-adjudicator.md) | 5 (live-and-docs) | — | COMPLETE | /sync-docs --tracker 2026-10-07: shipped 93f0d2a, dcb260f; adjudicator live on … | 2026-10-07 |
| [scrutinise-phase4-adjudicator](./scrutinise-phase4-adjudicator.md) | 2 (live-fix-polish) | crates/podling-core/src/plugin/openai.rs | COMPLETE | all 6 acceptance criteria ticked; dcb260f | 2026-10-06 |
| [scrutinise-phase5-tts-audio](./scrutinise-phase5-tts-audio.md) | 1 (asr-expected-text) | crates/podling-core/src/stages/synthesize.rs | COMPLETE | all 1 acceptance criteria ticked; merge 61ca815 into worktree-phase5-tts-plan; … | 2026-10-05 |
| [scrutinise-phase5-tts-audio](./scrutinise-phase5-tts-audio.md) | 2 (tts-weights-fingerprint) | sidecars/tts, crates/podling-core/src/plugin/sidecar_tts.rs | COMPLETE | all 1 acceptance criteria ticked; merge 4a75676 into worktree-phase5-tts-plan; … | 2026-10-05 |
| [scrutinise-phase5-tts-audio](./scrutinise-phase5-tts-audio.md) | 3 (voice-licence-allowlist) | crates/podling-types/src/episode.rs | COMPLETE | all 1 acceptance criteria ticked; merge 93279f7 into worktree-phase5-tts-plan; … | 2026-10-05 |
| [scrutinise-phase5-tts-audio](./scrutinise-phase5-tts-audio.md) | 4 (sidecar-process-group) | crates/podling-core/src/plugin/sidecar.rs | COMPLETE | all 1 acceptance criteria ticked; merge 985b12f into worktree-phase5-tts-plan; … | 2026-10-05 |
| [scrutinise-phase5-tts-audio](./scrutinise-phase5-tts-audio.md) | 5 (audio-cleanups) | crates/podling-core/src/pipeline.rs | PENDING | — | 2026-10-05 |
| [scrutinise-phase5-tts-audio](./scrutinise-phase5-tts-audio.md) | 6 (audio-dedupe) | crates/podling-core/src/plugin | PENDING | — | 2026-10-05 |
| [scrutinise-phase5-tts-audio](./scrutinise-phase5-tts-audio.md) | 7 (sidecar-orphan-after-exit) | crates/podling-core/src/plugin/sidecar.rs | COMPLETE | all 1 acceptance criteria ticked; merge ea75403 (f5f9442) into worktree-phase5-… | 2026-10-05 |
| [scrutinise-phase5-tts-audio](./scrutinise-phase5-tts-audio.md) | 8 (sidecar-hardening) | crates/podling-core/src/plugin/sidecar.rs, sidecars/tts | PENDING | — | 2026-10-05 |
| [scrutinise-speech-followups](./scrutinise-speech-followups.md) | 1 (ssf-tests) | provenance tests | COMPLETE | all 2 acceptance criteria ticked; af24d48 merge of 6731b5d; cargo test + voice_… | 2026-10-07 |
| [scrutinise-stance-precision](./scrutinise-stance-precision.md) | 1 (stance-subject) | crates/podling-core stance gate + eval | COMPLETE | all 6 acceptance criteria ticked; b6e753b (merge a35bc90 into worktree-stance-p… | 2026-10-06 |
| [scrutinise-stance-precision](./scrutinise-stance-precision.md) | 2 (stance-pronoun) | podling-core stance gate pronoun windows | COMPLETE | all 7 acceptance criteria ticked; 6347534 (merge 5ef59fa into worktree-stance-p… | 2026-10-06 |
| [scrutinise-stance-precision](./scrutinise-stance-precision.md) | 3 (stance-pronoun-anywhere) | podling-core stance gate | COMPLETE | all 8 acceptance criteria ticked; 043c8bf | 2026-10-06 |
| [scrutinise-stance-two-way](./scrutinise-stance-two-way.md) | 1 (scrutinise-stance-two-way) | podling-core stance tests | COMPLETE | all 5 acceptance criteria ticked; 2948dd0 merge of 52455e0; t03 measured (misse… | 2026-10-06 |
| [scrutinise-stance-whole-window](./scrutinise-stance-whole-window.md) | 1 (scrutinise-stance-whole-window) | podling-core stance comments + tests | COMPLETE | all 4 acceptance criteria ticked; a84314d on worktree-scrutinise-stance-whole-w… | 2026-10-07 |
| [speech-acceptance](./speech-acceptance.md) | 1 (sa-tool) | podling-cli lexicon A/B example | COMPLETE | all 6 acceptance criteria ticked; 738c87a merge of fc3b69e; cargo test + clippy… | 2026-10-07 |
| [speech-acceptance](./speech-acceptance.md) | 2 (sa-live) | live lexicon A/B run | BLOCKED | b34d67a: A/B inconclusive - script stage v12 writes one turn naming neither Kul… | 2026-10-07 |
| [speech-acceptance](./speech-acceptance.md) | 3 (sa-listen) | blind listening (user) | PENDING | — | 2026-10-07 |
| [speech-followups](./speech-followups.md) | 1 (sf-lexicon) | podling-types lexicon parsing | COMPLETE | all 3 acceptance criteria ticked; e6f5292 merge of b890817; cargo test workspac… | 2026-10-07 |
| [speech-followups](./speech-followups.md) | 2 (sf-provenance) | voice provenance clip hash | COMPLETE | all 5 acceptance criteria ticked; 0c75233 merge of 3c5088b; cargo test + both p… | 2026-10-07 |
| [speech-followups](./speech-followups.md) | 3 (sf-e2e) | podling-core e2e pronounce test | COMPLETE | all 2 acceptance criteria ticked; 2b7bd68 merge of 0217960; cargo test workspac… | 2026-10-07 |
| [stance-precision](./stance-precision.md) | 1 (stance-eval) | stance evaluation set + report | COMPLETE | all 5 acceptance criteria ticked; 6af56e6 (merged into worktree-stance-precisio… | 2026-10-06 |
| [stance-precision](./stance-precision.md) | 2 (stance-rule) | stance decision rule | COMPLETE | all 4 acceptance criteria ticked; 8251831 (merge b3632c1 into worktree-stance-p… | 2026-10-06 |
| [stance-two-way](./stance-two-way.md) | 1 (stance-two-way) | podling-core stance gate | COMPLETE | all 9 acceptance criteria ticked; 1fcc319 merge of feat/stance-two-way (4d2e853… | 2026-10-06 |
| [stance-whole-window](./stance-whole-window.md) | 1 (stance-whole-window) | podling-core stance reverse check | COMPLETE | all 4 acceptance criteria ticked; c7c86ec on worktree-stance-quantity: contradi… | 2026-10-07 |
| [story-driven-script](./story-driven-script.md) | 1 (sds-metrics) | podling-core script_metrics | COMPLETE | all 2 acceptance criteria ticked; d793686 merge of f0d565d into worktree-story-… | 2026-10-07 |
| [story-driven-script](./story-driven-script.md) | 2 (sds-harness-baseline) | script_eval_live + baseline table | COMPLETE | all 2 acceptance criteria ticked; 9eadba9: script_eval_live.rs (ignored script_… | 2026-10-07 |
| [story-driven-script](./story-driven-script.md) | 3 (sds-arc-script) | script stage order + arc prompt | COMPLETE | all 6 acceptance criteria ticked; e85c754 (b1c3f36): in_source_order + test, ar… | 2026-10-07 |
| [story-driven-script](./story-driven-script.md) | 4 (sds-arc-measure) | arc table, topic overlap, STORY-08 verdict | COMPLETE | all 4 acceptance criteria ticked; 7787fcb merged 1cd801b; tables recorded; ARCH… | 2026-10-07 |
| [story-driven-script](./story-driven-script.md) | 5 (sds-data-policy-types) | podling-types data_policy + consumers | COMPLETE | all 3 acceptance criteria ticked; aebbb6d: DataPolicy + optional data_policy, S… | 2026-10-07 |
| [story-driven-script](./story-driven-script.md) | 6 (sds-transport-policy) | http Transport local/hosted policy | COMPLETE | all 6 acceptance criteria ticked; 95d8e68 merged 6dd427a into worktree-story-dr… | 2026-10-07 |
| [story-driven-script](./story-driven-script.md) | sds-arc-measure | — | COMPLETE | all 4 acceptance criteria ticked; 7787fcb merged 1cd801b; tables recorded; ARCH… | 2026-10-07 |
| [story-driven-script](./story-driven-script.md) | sds-transport-policy | — | COMPLETE | all 6 acceptance criteria ticked; 95d8e68 merged 6dd427a into worktree-story-dr… | 2026-10-07 |
| [phase5-tts-audio](../../phase5-tts-audio) | 5 | — | COMPLETE | all 2 acceptance criteria ticked; merge a90c25f into worktree-phase5-tts-plan; … | 2026-10-05 |

<!-- craftsman:ledger:end -->
