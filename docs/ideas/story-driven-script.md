---
slug: story-driven-script
status: architected
verdict: pursue-with-changes
confidence: medium
depth: standard
isolation: isolated
created: 2026-10-07
updated: 2026-10-07
related: [docs/architecture/story.rules.md, docs/architecture/privacy.rules.md, docs/ideas/natural-episode-speech.md, docs/architecture/speech.rules.md, docs/plans/phase5-tts-audio.md, docs/plans/phase4-adjudicator.md, docs/handoff.md, docs/architecture.md]
touches:
  - crates/podling-core/src/stages/script.rs            # INSTRUCTIONS/AUDIO_RULES (arc, focus); later a sectioned writer
  - crates/podling-core/src/plugin/llm.rs               # PROMPT_VERSION (ARCH-SPEECH-16); a new LlmTask only if a section/outline call is added
  - crates/podling-core/src/plugin/openai.rs            # optional routing/data-policy field (OpenRouter provider filter); local-only enforcement
  - crates/podling-core/src/plugin/fake*.rs             # fake LLM follows any new task
  - crates/podling-core/src/stages/outline.rs           # only in step 3: deterministic act plan (program picks claim ids)
  - crates/podling-types/src/episode.rs                 # only if `topic` proves too weak: optional `focus`; SCHEMA_VERSION + snapshot (ARCH-SPEECH-07)
  - crates/podling-types/tests/snapshots/
  - crates/podling-core/src/pipeline.rs
  - crates/podling-core/tests/pipeline.rs               # cache-key pin test
  - examples/titanic/                                   # angle sources (e.g. a passenger/ship-description group)
  - scripts/script_eval/                                # before/after measurement harness (pass rate, words, quotes, citation errors, coverage)
  - docs/architecture.md
  - README.md
---

# Idea: story structure and per-episode angle for the script stage

## Proposal (neutral restatement)
**Problem.** The script stage is one LLM call whose instructions are grounding
rules plus "about 150 spoken words for each minute … covering every usable
claim" (`crates/podling-core/src/stages/script.rs:24-42`). Nothing asks for an
opening, a through-line, ordering toward a turning point, or an ending, so the
output is a walk through the claims. Every listener also gets the same script
from the same sources, whatever part of the topic they care about. Fiction is
planned but absent: `Mode` has only `NonFiction` (`crates/podling-types/src/episode.rs:55-57`).

**Claimed benefit.** Episodes with real story shape that stay inside every
grounding rule. The episode maker can steer an episode toward an angle (for the
Titanic: the opulence, or the engineering failure). Later, fantasy.

**Who benefits.** Listeners, and the person making episodes. For fiction, also
authors who want plot help and consistency checks (user answer, 2026-10-07).

**Done.**
- In a blind A/B across at least 3 episodes, the listener prefers the new script.
- The cold-run pass rate, citation validity and quotes are no worse than the
  measured baseline.
- Word count reaches at least 70% of the target.
- Two angles on the same sources produce clearly different cited-claim sets.
- Every grounding invariant holds: citations name ledger claims, quotes are
  verbatim via `{{quote:N}}`, and every judged Contested claim is cited.

## Verdict
**PURSUE-WITH-CHANGES** (medium).

Pursue only non-fiction story structure (A) and a per-episode angle (B), as a
measured staircase:
1. Baseline.
2. A prompt-only arc, with the angle carried in `topic`.
3. A sectioned writer in which the program, not the LLM, assigns claims to acts.
4. A bake-off of permissively licensed local writer models.

Split fiction (C) into its own idea.

The deciding fact is that **the local writer cannot yet meet the length of the
prompt it already has**: 216 words against a 10-minute (~1,500-word) target
(`docs/plans/phase5-tts-audio.md:540-545`). A story arc needs that length first.

**Most likely to flip it.** If the prompt-only arc *and* the sectioned writer
both lower the pass rate or citation validity on llama3.1:8b, A becomes
DEFER-until-a-better-local-model. B survives either way, because it is mostly
sources plus `topic`.

| criterion | score 1–5 | evidence |
|---|---|---|
| value | 4 | The README promises "story-driven" episodes (`README.md:3-5`), and the prompt asks for none of it (`script.rs:24-42`). The flat "That's a … But what about …?" banter is a script-side cause (`docs/ideas/natural-episode-speech.md:112`) |
| system fit | 3 | A prompt change fits ARCH-SPEECH-16. A sectioned writer fits the typed-artifact and cache design. An angle field ripples into the schema (ARCH-SPEECH-07). Fiction conflicts with the source-ledger premise (`README.md:7-11`) |
| cost (5 = cheap) | 3 | Steps 1–2 take days. Step 3 adds a stage, fakes and a cache-pin change. Step 4 is a bake-off |
| risk (5 = low) | 2 | Script reliability is already marginal: one of seven cold Titanic runs failed in the script stage (`docs/handoff.md:86-89`), and a 10-minute run had a rejected attempt (`phase5-tts-audio.md:540-541`). Narrative pressure could dramatise Contested claims |
| reversibility | 4 | Prompt revert = cache invalidation only. An outline stage or schema field is harder to undo |
| evidence strength | 2 | The strong research (DOC, MirrorStories) is on large models. There is nothing on 8B grounded dialogue, one listener, and one 10-minute run |

## Overlap & prior work
**Novel; extends the script stage.** No duplicate or conflict.
- **Searched:**
  - `docs/ideas/*.md` (only `natural-episode-speech.md`, a TTS idea)
  - the `goal:` lines of `docs/plans/*.md`
  - `docs/plans/TRACKER.md`
  - `README.md`, `docs/architecture.md`, `docs/handoff.md`
  - `docs/architecture/speech.rules.md`
  - plan memory (recall on the phase5, phase4 and natural-episode-speech plans: nothing)
  - There is no `CHANGELOG.md`.
- **Governing rules** (`arch-check governs`: `speech.rules.md` governs `script.rs`, `episode.rs` and `pipeline.rs`):
  - ARCH-SPEECH-16: any script-prompt change bumps `PROMPT_VERSION`.
  - ARCH-SPEECH-07: an `EpisodeSpec` addition bumps `SCHEMA_VERSION` and the snapshot.
  - Neither is broken by this idea; both must be obeyed.
- **Related:** `natural-episode-speech` names the script as one cause of flat delivery. This idea is the script-side half of "sounds like engaged people".
- `topic` already works as an angle: "how the 1912 Titanic inquiries disagreed" (`examples/titanic/episode.toml:12`).

## System fit (whole project)
- **Contract ripple.**
  - Steps 1–2: none beyond the prompt text.
  - Step 3: a new artifact (an act plan: through-line, ordered acts, claim ids per act), possibly a new `LlmTask` (`crates/podling-core/src/plugin/llm.rs:43-57`), and `ScriptInput` gains the plan.
  - A `focus` field on `EpisodeSpec` only if `topic` steering proves too weak.
- **Data.** N/A (file cache only). A new stage id or version invalidates the `script` key, so the pin test `without_nli_the_cache_keys_are_unchanged` (`crates/podling-core/tests/pipeline.rs`) moves deliberately.
- **Config.** Optionally `focus` in the episode TOML. A per-stage LLM choice (the script on a different model from extraction) may be wanted for the bake-off. Today `[llm]` is one config for all tasks (`episode.rs:23`).
- **Security.** The angle text is the operator's own input placed in the prompt (low severity). Sources stay untrusted data (`script.rs:30`, rule 4). A story-ordering instruction must not weaken rule 2: Contested claims are never stated as settled (`script.rs:28`).
- **Tests.** The fake LLM follows any new task. Program-assigned claim ids per act come with a "judged Contested claim is in some act" check. The no-`[tts]` and no-NLI byte-identical guarantees hold.
- **Observability.** Log the act plan, words per section and the attempts per section. Record the measured word ratio in the run output.
- **UI.** N/A (CLI only).
- **Docs.** The `docs/architecture.md` artifact flow (`:31-61`), `README.md`, and the example episodes.
- **Second-order effects.**
  - More LLM calls per episode. The script stage already takes 108 s on the local GPU (`phase5-tts-audio.md:540`).
  - Every prompt tweak invalidates every cached script.
  - The 24 KiB large-input warning (`script.rs:137`) gets closer as instructions grow. Sectioning helps, because each call sees only its act's claims and sources.
  - Angle coverage is bounded by the sources. The Titanic example has only the two inquiry reports (`examples/titanic/sources/{us-senate,british-inquiry}`), so an "opulence" episode needs new sources first.

## Research
- **Outline-first generation.** DOC beats Re3 on plot coherence (+22.5%), outline relevance (+28.2%) and interestingness (+20.7%) in human evaluation. Planning takes load off generation. https://aclanthology.org/2023.acl-long.190 ; DOME: https://arxiv.org/pdf/2412.13575
- **LLM fiction quality.** LLM stories pass 3–10× fewer TTCW tests than professional ones, and no LLM judge correlated with the experts. https://arxiv.org/abs/2309.14556
- **LLM-as-judge.** Biased by length, position and self-preference (MT-Bench agreed with itself on only 65% of order-swapped pairs). Human listening is the measure. https://arxiv.org/html/2506.22316v1 , https://arxiv.org/pdf/2608.23705 (not fetched)
- **Radio narrative non-fiction.** Anecdote then reflection, alternating. A story without the reflection fails. https://hindenburg.com/blog/understanding-story-structure/
- **Steering.** NotebookLM's "Customize" is free-text focus, audience and expertise instructions, not a structured angle model. https://blog.google/technology/ai/notebooklm-update-october-2024/
- **Personalisation.** Interest-personalised stories were rated more engaging (4.22 vs 3.37). https://preview.aclanthology.org/setup/2024.emnlp-main.382
- **Long-fiction consistency** (relevant to the fiction split):
  - Errors cluster in factual and temporal details, mid-narrative. https://preview.aclanthology.org/ingest-acl/2026.findings-acl.410/ (not fetched)
  - Tracking narrative state (characters, events, future requirements) holds consistency to 100K words. https://deeplearn.org/arxiv/832622/scaling-long-form-story-generation-via-narrative-state-tracking
- **Writer models.** The open-weight models at the top of EQ-Bench Creative Writing v3 are large, and none fits 8 GB. https://github.com/EQ-bench/creative-writing-bench . Which ≤8 GB, permissively licensed models write best is **UNPROVEN**; that is step 4's job.

## Critique
- **Steelman.**
  - The product promise and the prompt disagree. The fix starts with text, not code.
  - A typed act plan is just another serde artifact with a content-hash key, and its arc can be checked deterministically.
  - Splitting the script into short calls also attacks the length shortfall, because each call writes about 300 words, not 1,500.
- **Strongest case against.**
  - llama3.1:8b already fails length (216/1,500 words) and sometimes citations. More instructions may make both worse.
  - The research gains come from large models.
  - The only evaluator is one listener.
- **Hidden assumptions, each with a cheap test:**
  - *An 8B model can follow an arc while keeping its citations valid.* Prompt-only change; 5 cold runs each on Titanic and Tunguska; compare pass rate, words, quotes and citation errors with the baseline.
  - *Structure, not length, is what makes episodes dull.* Hand-edit one existing script into an arc of the same length and listen to both.
  - *`topic` already steers coverage.* Two `topic` strings on one cached ledger; diff the cited claim ids.
  - *The ledger is big enough for acts.* Count the usable claims in a Titanic ledger (**UNPROVEN**).
  - *The 216-word result is typical.* The baseline step measures it over several runs.
- **Failure modes:**
  - The LLM names claim ids that do not exist, and retries burn time. This is the failure seen on Titanic.
  - The script silently ignores the act plan, or strict plan checks raise the failure rate.
  - Narrative pressure turns a Contested claim into a "reveal" stated as settled.
  - Prompt churn invalidates caches.
  - An angle with no supporting sources produces a thin episode, or tempts the model to invent.
- **Kill criteria:**
  - The arc version lowers the cold pass rate or citation validity below the baseline.
  - The listener cannot tell it apart in a blind A/B of 3 episodes.
  - For a separate `focus` field: two `topic` strings already give clearly different coverage.
- **Cheaper alternatives.**
  - **Do nothing:** reasonable only if length is fixed elsewhere.
  - **Smallest useful slice:** the prompt-only arc plus documenting `topic` as the angle.

## Disputed
- **Critic:** "Part B mostly exists already (`topic`)." **Softened.** `topic` covers the *steering* (`examples/titanic/episode.toml:12`). It does not cover the *material*: an angle the sources do not contain cannot be grounded. The Titanic sources are only the two inquiry reports. So B is "`topic` + per-angle sources", not "nothing to do".
- **Critic:** the brief overstated script failures. **Accepted.** Of the two failed cold runs, one was in the script stage and one in extraction grounding (`docs/handoff.md:86-89`). This doc uses the corrected figure.
- **Critic:** "Drop C." **Accepted as a split, not a rejection.** The user wants fiction (both original and myth/lore retelling) plus author plot and consistency help. That gets its own `/idea`, because it replaces the source ledger with a canon ledger. One reusable piece is worth recording: the NLI contradiction machinery (`score_stances`, `docs/architecture.md:45`) is what an author continuity check would need.

## Recommendations
In order of impact:
1. **Measure first.** Build a small harness (e.g. `scripts/script_eval/`) that runs N cold scripts and reports:
   - pass rate and attempts;
   - word ratio against the target;
   - quote count, citation errors, cited-claim coverage, and Contested-claim handling.
   Everything after this is judged against it.
2. **Prompt-only arc (spike).** Ask for:
   - a cold open (a concrete scene or quote);
   - one through-line taken from `topic`;
   - anecdote→reflection alternation;
   - ordering that builds to the Contested dispute as the turning point, told as a dispute, never settled;
   - a closing reflection.
   Replace "covering every usable claim" with "the claims that serve the through-line". Bump `PROMPT_VERSION`. Document `topic` as the angle.
3. **Sectioned writer, if step 2 stays short or loses the arc.**
   - The **program** builds the act plan: it orders and buckets claim ids by `topic` relevance (embeddings already exist), status and time.
   - The LLM writes only the through-line and one act per call, against that act's claims and sources.
   - The LLM never generates the claim-id plan. This is the local-first answer: small calls fit an 8B model and add up to the target length.
4. **Local writer bake-off.** Compare llama3.1:8b with 2–3 permissively licensed models that fit under 7 GB, using the harness and a blind listen. Check each licence against `.claude/CLAUDE.md` (no non-commercial, AGPL or revenue-capped weights). A hosted model stays an opt-in comparison point through the existing provider, never the default.
5. **Angles need sources.** Add one source group per demo angle to the Titanic example (e.g. a public-domain description of the ship and its passengers). Add an `EpisodeSpec.focus` field only if the step-2 diff shows `topic` steers too weakly.
6. **Fiction: write a separate idea** (`/idea fiction-canon-ledger`). Cover:
   - original fiction from a world bible;
   - myth and lore retellings, which fit today's ledger as-is;
   - an author consistency checker built on the extract → NLI-contradiction path.
7. **Hosted MVP, under the no-training rule (below).** The fastest way to test a stronger writer needs no code change: point an example episode's `[llm]` at an open-weight model on a host that does not train on inputs and retains nothing.
   - The existing provider already sends the key from a named environment variable and asks for JSON mode (`crates/podling-core/src/plugin/openai.rs:121`).
   - Estimated cost: under ~$0.10 per 10-minute episode at about $0.21 in / $4.20 out per million tokens (DeepSeek V4 Pro pricing, https://pricepertoken.com/endpoints/openrouter).
   - A 10-minute script fits `MAX_SCRIPT_TOKENS = 8192` (`crates/podling-core/src/stages/script.rs:152`). A 30–60-minute one does not, so long episodes still need the sectioned writer (recommendation 3).
   - Run the harness on local and hosted, before and after the arc prompt. That separates "the model is too weak" from "the prompt asks for no story".

## Data privacy: no training on Podling data
**Constraint (user, 2026-10-07):** none of the user's information, data or stories may be used to train any model. This applies even more strongly to the work authors will submit under the planned fiction/author feature.

**What it means for providers:**
- **Local inference is the default** and the only unconditional guarantee. Today only the LLM, and any embedding server configured as hosted, sends text off the machine. TTS and Whisper run locally (`README.md:313`), and the NLI model runs on the CPU.
- **A hosted LLM is allowed only on endpoints that both refuse training by default *and* retain nothing:**
  - **Together AI:** no training without explicit consent; turn on zero data retention in the organisation's Privacy settings ("Store prompts and model responses" = No). https://docs.together.ai/docs/zero-data-retention
  - **Fireworks AI:** reported as no training or logging for open models without opt-in. This is second-hand (https://zoftwarehub.com/en-sa/products/together-ai/zoftware-analysis), **UNPROVEN** against Fireworks' own docs.
  - **OpenRouter, paid:** only with `provider: { "data_collection": "deny", "zdr": true }` (https://openrouter.ai/docs/guides/routing/provider-selection). Podling does not send a `provider` object today, so this needs an optional routing field on `[llm]`, part of the provider fingerprint. OpenRouter's *own* logging policy is **UNPROVEN** from its docs.
- **Excluded:**
  - DeepSeek's first-party API: trains on inputs by default, opt-out only, stored in China (https://meetily.ai/llm-privacy/deepseek, https://cdn.deepseek.com/policies/en-US/deepseek-terms-of-use.html).
  - Free tiers whose prompts may be used for training (https://openrouter.ai/blog/tutorials/free-llm-apis-compared/).
  - The MIT-licensed DeepSeek V4 *weights* remain fine to use; it is DeepSeek's own *host* that is excluded.

**Design obligations, carried to `/architect` and the fiction idea:**
- A source or episode marked private can force **local-only**: the run refuses any non-local provider. This is a hard check, not a convention.
- When hosted is allowed, the config names a zero-retention endpoint, and Podling records which endpoint processed which text.
- Providers' terms change, so re-check a provider's privacy page before any author text is sent.

## Open questions
- **Writer model** (answered 2026-10-07): "Ideally, I want it to be local but I understand those limitations, please see what we can do." So: local-first, and recommendations 3 and 4 exist to make that work. A hosted model is a comparison point only.
- **Who picks the angle** (answered): the episode maker, per episode.
- **Fantasy** (answered): both original fiction and retellings, plus "helping out with plots for actual authors… keep the story consistent and to the point". Split to its own idea.
- **Larger model off this machine** (answered 2026-10-07): the cheapest MVP is a pay-per-token API for an open-weight model, not a rented GPU (roughly $1–3 per session, https://www.spheron.network/blog/gpu-cloud-pricing-comparison-runpod-vs-vastai-2026/). It is constrained by the no-training rule: see recommendation 7 and "Data privacy".
- **Still open:** what episode length matters most? Story structure at 5 minutes and at 30–60 minutes are different problems. Phase 5's goal is 30–60 minutes (`docs/plans/phase5-tts-audio.md:4`).

## Next step
`/architect story-driven-script`. Ask it to sequence the baseline harness and the prompt-only spike before any new stage or schema field.
