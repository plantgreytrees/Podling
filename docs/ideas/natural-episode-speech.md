---
slug: natural-episode-speech
status: pursue-with-changes
verdict: pursue-with-changes
confidence: medium
depth: standard
isolation: isolated
created: 2026-10-06
updated: 2026-10-06
related: [docs/plans/phase5-tts-audio.md, docs/plans/scrutinise-phase5-tts-audio.md, docs/architecture.md]
touches:
  - crates/podling-types/src/episode.rs                 # [tts] pronunciation list; schema snapshot
  - crates/podling-types/tests/snapshots/schema_snapshot__episode.snap
  - crates/podling-core/src/plugin/tts.rs               # SpokenTurn: model text vs said()
  - crates/podling-core/src/stages/synthesize.rs        # apply the list when building SpokenTurn
  - crates/podling-core/src/stages/verify_audio.rs      # accept respelt variants in the WER scorer
  - crates/podling-core/src/plugin/sidecar_tts.rs       # wire turn carries the model text
  - sidecars/tts/podling_tts/backends/__init__.py       # in_line() mirror note
  - sidecars/tts/podling_tts/backends/qwen.py
  - scripts/tts_bakeoff/                                # offline design-then-clone voice tool
  - examples/tunguska/episode-tts.toml
  - examples/tunguska/voices/README.md
  - sidecars/tts/README.md
  - docs/architecture.md
---

# Idea: natural episode speech (pronunciation and delivery)

## Proposal (neutral restatement)
**Problem.** In the user's listening pass (2026-10-06), the speech had two faults:
- **Mispronounced words.** "Kilometres" was the most noticeable, and the user heard it in the bake-off clips. The word is spelled out in full there, in British spelling (`scripts/tts_bakeoff/fixtures/tunguska-10min.json:10,14,18,24,36`). Other speech sounded "robotic".
- **Flat delivery.** The voices are "not bad" but lack personality.

Seams, pacing and loudness were judged fine (`docs/plans/phase5-tts-audio.md`, "Live results"). Every episode made with `[tts]` is affected.

**Claimed benefit.** Episodes that sound like two engaged people rather than a reader.

**Who benefits.** Listeners of generated episodes, and the person making them.

**Done.** A repeat listening pass where:
- the named bad words are said correctly;
- the delivery is judged livelier;
- WER and retry rate are no worse;
- every hard constraint still holds: commercial-safe licences, 8 GB with one model at a time, sidecars only from the user's config, the bump rules, and no change to an episode without `[tts]`.

## Verdict
**PURSUE-WITH-CHANGES** (medium). Both faults are real and named by the user. The architecture already separates what the model is told from what speech recognition checks (`SpokenTurn::said`, `crates/podling-core/src/plugin/tts.rs:61-68`), so a pronunciation list fits cleanly.

Pursue only the smallest slice:
1. Log the bad words.
2. Add a pronunciation list applied in Rust, which the speech check accepts.
3. Try a design-then-clone voice offline.

Leave out text normalisation (it does not touch an already spelled-out word) and a new TTS model (no candidate clearly fits 8 GB without a fresh bake-off).

**Most likely to flip it:** whether respelling actually changes how Qwen3-TTS Base pronounces a word. If it does not, the mispronunciation half becomes a model-swap problem, and that half moves to DEFER pending a model spike.

| criterion | score 1–5 | evidence |
|---|---|---|
| value | 4 | The user named both faults as what remains (`docs/plans/phase5-tts-audio.md:576-582`) |
| system fit | 4 | Fits the adapter split (`sidecars/tts/podling_tts/backends/__init__.py:3-6`), the `said()` split and the chunk cache key; normalisation and a model swap fit poorly |
| cost (5 = cheap) | 3 | The slice takes days; all five levers would need a new bake-off |
| risk (5 = low) | 3 | Scorer mismatch, voice consistency (Qwen's minimum ECAPA is already 0.43, `phase5-tts-audio.md:486`), VRAM for a new model |
| reversibility | 4 | The pronunciation list is config-gated and the voices are offline clips; a model swap is not cheap to undo |
| evidence strength | 2 | One listener; only "kilometres" is named; under 3 minutes of live audio |

## Overlap & prior work
**Extends** Phase 5 (`docs/plans/phase5-tts-audio.md`). There is no duplicate or conflict.
- **Searched:** `docs/ideas/` (did not exist), the `goal:` lines of `docs/plans/*.md`, `docs/plans/TRACKER.md`, `README.md`, `docs/architecture.md` and `docs/handoff.md`. There is no `CHANGELOG.md` and no `docs/architecture/*.rules.md`.
- **Phase 5 decisions it builds on:**
  - The spike chose per-turn Qwen3-TTS 1.7B Base (`phase5-tts-audio.md:468-526`).
  - "`generate_voice_clone` takes no instruction, so emotion comes from the wording alone" (`:497`).
  - The adapter drops `emotion` and laughs, chuckles and sighs (`:508-511`; `sidecars/tts/podling_tts/backends/qwen.py:155-162`).
  - Dia2 was judged near identical by ear and not built (`:576`).
- **Not overlapping:** the PENDING units 5, 6 and 8 in `docs/plans/scrutinise-phase5-tts-audio.md` are hardening and clean-up, not speech quality.
- **Related future idea (not this one):** the user wants people to upload their own voice recordings and sell the use of their voice for other users' podcasts. That conflicts with today's CC0/CC-BY-only rule (`crates/podling-types/src/episode.rs:208`). It needs its own `/idea`: consent, impersonation, payment, and licence terms beyond CC.

## System fit (whole project)
- **Contract ripple.** `SpokenTurn` gains a model-facing text separate from `said()`. The wire turn sends that text, and the `said()`/`in_line()` mirror note (`crates/podling-core/src/plugin/tts.rs:66-67`, `sidecars/tts/podling_tts/backends/__init__.py:116-128`) changes. Protocol v1 stays, because the text field already exists.
- **Data.** Applied in Rust, the pronunciation list changes the turn text, so the `synthesize_chunk` key invalidates exactly the edited chunks. `audio.json` can record which entries were applied.
- **Config.** An optional `[tts]` table of `word = "respelling"`, plus a `schema_snapshot__episode.snap` refresh and a `SCHEMA_VERSION` bump (bump rule 1). Without `[tts]` nothing changes.
- **Security.** The episode file only supplies strings, never a program. Designed voice clips need a licence policy (open question 1).
- **Tests.**
  - The respelling reaches the TTS, while `said()` keeps the original.
  - The WER scorer accepts the respelt variant.
  - Editing the list re-synthesises only the affected chunks.
  - An episode without `[tts]` is unchanged.
- **Observability.** Log the applied entries per chunk, and record them in `audio.json`.
- **UI.** N/A (CLI only).
- **Docs.** `docs/architecture.md` "Episode audio", `sidecars/tts/README.md`, and the example episode and voices README.
- **Second-order effects.**
  - A growing per-episode list with no tests can rot, and respellings tuned to one model may be wrong after a weights change.
  - If the scorer does not know the respellings, takes get burned: 6 of 18 chunks in the 10-minute run already needed a second take (`~/podling-listening/ten-minute/audio.json`).

## Research
- **Qwen3-TTS family** (Apache-2.0):
  - 1.7B Base clones a voice and takes no instruction.
  - CustomVoice has 9 fixed voices plus style instructions and cannot clone.
  - VoiceDesign makes a voice from a description.
  - The Base card documents a **design-then-clone** workflow: generate a clip with VoiceDesign, `create_voice_clone_prompt`, then `generate_voice_clone`.
  - Sources: https://huggingface.co/Qwen/Qwen3-TTS-12Hz-1.7B-Base , https://huggingface.co/Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice
- **Known Qwen English text-frontend bugs** ("he's" said as "his") suggest pronunciation follows the text tokens, so respelling should help, but this is **unverified**. https://huggingface.co/Qwen/Qwen3-TTS-12Hz-1.7B-Base/discussions/7 ; there is a stress-control proposal, not shipped: https://github.com/QwenLM/Qwen3-TTS/discussions/53
- **Respelling dictionaries** applied just before TTS are the engine-agnostic fix. https://deepgram.com/learn/developers-guide-fixing-tts-pronunciation-errors
- **Text normalisation:** `nemo_text_processing` (Apache-2.0, Pynini WFST) expands "123 kg" and similar. It does nothing for a word that is already spelled out. https://github.com/NVIDIA/NeMo-text-processing
- **Other expressive models with cloning:**
  - Chatterbox: MIT, 0.5B, with an exaggeration dial. Every output carries a Perth watermark; whether it can be turned off is **undocumented**. https://huggingface.co/ResembleAI/chatterbox
  - Zonos v0.1: Apache-2.0, with emotion conditioning. It needs 6 GB minimum and 10 GB is recommended, so on this card it is **unproven**. https://huggingface.co/Zyphra/Zonos-v0.1-hybrid/raw/main/README.md
- **Expressive CC-BY data:** NaturalVoices shows a CC-BY-4.0 badge, but whether that covers the audio is **unverified**. https://arxiv.org/abs/2511.00256 . Expresso and EARS are non-commercial (`examples/tunguska/voices/README.md`).

## Critique
- **Steelman.** The script already pays for an `emotion` on every turn (`crates/podling-types/src/script.rs:24-32`), and synthesis throws it away (`qwen.py:160-161`). The architecture already splits model text from checked text, and versions the adapter in the cache key (`sidecar_tts.rs:424-433`). Phase 5's core is done, so speech quality is now the weakest link.
- **Strongest case against.** The evidence is one listener, one named word, and under 3 minutes of live audio. Part of the flatness may come from the script: llama3.1:8b's repetitive "That's a … But what about …?" banter is something no TTS change fixes.
- **Hidden assumptions, each with a cheap test:**
  - *Qwen mispronounces "kilometres" specifically.* Synthesise "kilometres", "kilometers" and a respelling in isolation and listen.
  - *Respelling changes Qwen's output.* Compare "Kulik" with "Koolik". **Confirmed** by the word test below, for the guest voice.
  - *A designed clip's liveliness survives cloning.* Blind A/B on the same three turns against the LibriTTS-R clip.
  - *How Whisper base.en spells "kilometres".* Run it once on an existing chunk.
- **Failure modes:**
  - Respellings leak into transcripts or findings.
  - The scorer compares the respelt sound with the original text and burns takes.
  - Clips chosen per emotion weaken voice consistency.
  - A list applied inside the sidecar would bypass the cache key; apply it in Rust.
  - Self-generated clips have no licence policy yet.
- **Kill criteria:**
  - Respelling does not change Qwen's pronunciation: drop the list.
  - A designed voice is judged no livelier blind, or its mean speaker similarity falls below 0.5: drop the voice lever.
  - Rewording the script alone fixes the flatness: stop the TTS work.
  - Mean WER rises above 10 ‰, or the retry rate rises.
- **Cheaper alternatives:**
  - **Do nothing.** It fails "done", because the user called both faults serious.
  - **The smallest slice.** This is what is recommended.
  - **Pick a clip per emotion.** One designed clip per speaker and emotion, chosen by `turn.emotion`. Only after a single designed clip proves livelier.
  - **A new cloning model that takes instructions.** The highest cost, and it re-runs the bake-off.

## Word test (step 0, 2026-10-06)
`scripts/tts_bakeoff/word_test.py`: 8 sentences × 2 bake-off voices, seed 1234, Qwen3-TTS 1.7B Base (peak 4,386 MiB), then Whisper base.en on the CPU. The clips are in `~/podling-listening/word-test/` (outside the repository), with `whisper.json`.

| Test | Host heard | Guest heard |
|---|---|---|
| "kilometres" / "kilometers" / "kill-oh-meeters" / "60 km" | "60 kilometers" for all, except "kilo meters" for the respelling | "60 kilometers" for all four |
| "Kulik" → "Koolick" | Kulik → Kulik | **Koolik → Kulik** |
| "Tunguska" → "Toon-goose-kah" | Tunguska → **Tungus Ka** | Tunguska → **Tungus Ka** |

What it shows:
- **Respelling changes Qwen's output.** The guest's "Kulik" moved from "Koolik" to "Kulik". So the deciding fact holds, and the pronunciation list stays in scope.
- **Hyphenated respellings split the word** ("Tungus Ka"). Respellings should be single unhyphenated words.
- **Qwen expands "60 km" itself**, which confirms text normalisation is not needed.
- **Whisper cannot judge the "kilometres" fault.** It hears every variant as "kilometers", so only a listener can say which variant sounds right.
- **Whisper writes American spellings** ("kilometers", "center"). A script with "kilometres" and "centre" therefore scores false WER errors today. The scorer needs British/American folding, independently of the pronunciation list.

**Pending, the user's ears:** which of clips 01–04 says "kilometres" correctly in each voice.

## Disputed
- *"'kilometres' appears nowhere in the recorded notes, so the named example is not in evidence."* **Softened.** The user named it in conversation after the notes were written (2026-10-06). It is in neither live script (0 matches in `~/podling-listening/{ten,thirty}-minute/script.json`), so it was heard in the bake-off clips, whose fixture spells it out (`scripts/tts_bakeoff/fixtures/tunguska-10min.json:10`). It is direct evidence, but it should be added to the plan's "Live results", as recommendation 1 says.

## Recommendations
1. **Step 0, before planning.** Log each mispronounced word, with its clip and time. Run the isolated test of "kilometres", "kilometers", a respelling, and "Kulik" vs "Koolik". Record the results in `phase5-tts-audio.md` "Live results".
2. **Drop text normalisation** from scope until a script contains a misread digit, unit or abbreviation.
3. **Apply the pronunciation list in Rust.**
   - Add a model-facing text to `SpokenTurn`, and keep `said()` on the original words.
   - Give the WER scorer the respellings, and British/American spellings, as accepted variants. This also covers the existing "Koolik" finding.
   - Update the `said()`/`in_line()` mirror note.
   - Bump `ADAPTER_VERSION` only if the adapter changes.
4. **Build design-then-clone as an offline tool** in `scripts/tts_bakeoff/`, with a written licence policy for self-generated clips before any is committed (open question 1).
5. **Acceptance:**
   - an episode without `[tts]` is unchanged;
   - editing the list invalidates only the affected chunks;
   - a blind A/B listening pass;
   - WER and retry rate no worse.
6. **Afterwards, test the script half separately:** a prompt change against repeated banter openers, with a `PROMPT_VERSION` bump.
7. **Defer the model swap** (Chatterbox or Zonos) to its own spike, with a VRAM gate of ≤ 7.0 GB for its own process.

## Open questions
1. **Licence for self-generated voice clips.** May VoiceDesign clips be declared CC0-1.0, with the user as author? *Answer (2026-10-06): not yet decided.* The user plans a voice marketplace, where people upload their own recordings and sell their use for other users' podcasts. That needs its own `/idea`, and will reshape the licence rule. Until then, designed clips stay local and uncommitted.
2. **Scope of the first slice.** *Answer (2026-10-06): both halves, smallest version* (recommendations 1–4).

## Next step
`/architect natural-episode-speech`, after recommendation 1's word log and isolated tests. Separately, `/idea` for the voice marketplace.
