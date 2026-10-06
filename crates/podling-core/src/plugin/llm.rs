//! Text-generation providers and the output shapes they must produce.

use podling_types::{
    BeatKind, ChunkId, ClaimId, ClaimStatus, Emotion, Favours, Ledger, Nonverbal, NonverbalAt,
    NonverbalKind, Pace, Speaker, SpeakerId, Stance, Verdicts,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{CoreError, Result};
use crate::text::sentences;

/// Version of the stage prompts and the input shapes they describe. Part of
/// both LLM stages' cache keys: bump it when a prompt or an input layout
/// changes in a way the instruction text alone would not show.
///
/// 2: a turn's text carries `{{quote:N}}` placeholders where its quotes go,
///    instead of the quoted words.
/// 3: the script request's ledger lists each claim's id, text and status
///    ([`LedgerClaim`]), without evidence.
/// 4: the script request may carry the episode's fixed `cast`.
/// 5: a script for audio (`"audio": true`) adds beats, pace, nonverbal
///    sounds and callbacks.
/// 6: beats are marked on the turn that begins each one (`beat`), not listed
///    as index ranges: llama3.1:8b wrote inclusive ends for exclusive ones.
/// 7: a Contested claim's ledger entry carries the adjudicator's verdict
///    ([`LedgerVerdict`]).
pub const PROMPT_VERSION: u32 = 7;

/// Version of the adjudicator's prompt and input shape, in its cache key only.
/// Kept apart from [`PROMPT_VERSION`] so a change to the adjudicator doesn't
/// re-run claim extraction.
pub const ADJUDICATE_PROMPT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmTask {
    /// Input: `{ "chunk_text": string }`. Output: a JSON [`ClaimsDraft`]. It is
    /// an object, not a bare array, because JSON mode only guarantees objects.
    ExtractClaims,
    /// Input: `{ "topic", "target_minutes", "ledger": [LedgerClaim],
    /// "sources": [SourceText], "cast"?: [Speaker], "audio"?: true }`.
    /// Output: JSON `ScriptDraft`. `cast` is present only when the episode
    /// fixes it; `audio` only when the script will be spoken, and then the
    /// draft's turns may carry beat marks, pace, nonverbal sounds and
    /// callbacks.
    WriteScript,
    /// Input: `{ "claim": AdjudicationClaim, "evidence": [AdjudicationEvidence] }`
    /// for one Contested claim. Output: a JSON [`VerdictDraft`].
    AdjudicateClaim,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CompletionRequest {
    pub task: LlmTask,
    pub instructions: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub text: String,
}

pub trait LlmProvider {
    fn id(&self) -> &str;

    /// Everything about this provider that can change its output (model name,
    /// temperature, prompt version, …). Part of every cache key that depends
    /// on this provider.
    fn fingerprint(&self) -> Value;

    fn complete(&self, request: &CompletionRequest) -> Result<Completion>;

    /// Frees whatever the model holds on the GPU, when the run no longer
    /// needs it. A *default method*: providers that hold nothing (most of
    /// them) inherit this no-op and need not write one.
    fn release(&self) -> Result<()> {
        Ok(())
    }
}

/// The reply to [`LlmTask::ExtractClaims`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimsDraft {
    pub claims: Vec<ClaimDraft>,
}

/// One claim as returned by [`LlmTask::ExtractClaims`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimDraft {
    pub text: String,
}

/// One ledger claim as shown to the model in [`LlmTask::WriteScript`]: what
/// it may cite, and how firmly. The evidence is left out: its chunk and
/// source ids look just like claim ids, and a small model cited them as
/// claims once merged claims carried two chunks each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerClaim {
    pub id: ClaimId,
    pub text: String,
    pub status: ClaimStatus,
    /// The adjudicator's verdict, on Contested claims only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<LedgerVerdict>,
}

/// A verdict as the script model sees it. Its evidence references are left
/// out for the same reason the claim's evidence is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerVerdict {
    pub favours: Favours,
    pub explanation: String,
}

impl LedgerClaim {
    /// The ledger's claims, in ledger order, each with its verdict if it has
    /// one.
    pub fn from_ledger(ledger: &Ledger, verdicts: &Verdicts) -> Vec<Self> {
        ledger
            .entries()
            .iter()
            .map(|entry| Self {
                id: entry.claim.id().clone(),
                text: entry.claim.text().to_owned(),
                status: entry.status.clone(),
                verdict: verdicts.get(entry.claim.id()).map(|v| LedgerVerdict {
                    favours: v.favours(),
                    explanation: v.explanation().to_owned(),
                }),
            })
            .collect()
    }
}

/// The Contested claim shown to the model in [`LlmTask::AdjudicateClaim`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjudicationClaim {
    pub id: ClaimId,
    pub text: String,
}

/// One piece of the claim's evidence, numbered so the verdict can cite it by
/// `n` instead of by its (hash-shaped) chunk id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjudicationEvidence {
    /// Position in the claim's evidence list, counting from 0.
    pub n: usize,
    pub stance: Stance,
    /// Title of the evidence's source document.
    pub source: String,
    pub independence_group: String,
    /// The passage the stance rests on: the NLI premise, the merged wording,
    /// or the chunk the claim was extracted from.
    pub text: String,
}

/// The reply to [`LlmTask::AdjudicateClaim`]. The stage checks it against the
/// claim's evidence before it becomes a `Verdict`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerdictDraft {
    pub claim: ClaimId,
    pub favours: Favours,
    pub explanation: String,
    /// `n` of each piece of evidence the verdict rests on.
    pub cites: Vec<usize>,
}

/// One chunk of source text as shown to the model in [`LlmTask::WriteScript`],
/// split into numbered sentences so a quote can be pointed at by number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceText {
    pub chunk: ChunkId,
    /// Title of the chunk's document, for the model's orientation.
    pub title: String,
    pub sentences: Vec<NumberedSentence>,
}

/// Sentence `sentence` (counting from 0) of a chunk, per `text::sentences`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberedSentence {
    pub sentence: usize,
    pub text: String,
}

/// A script as returned by [`LlmTask::WriteScript`]. Quotes are *references*
/// to source spans; the script stage copies the actual words from the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptDraft {
    pub cast: Vec<Speaker>,
    pub turns: Vec<DraftTurn>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftTurn {
    pub speaker: SpeakerId,
    /// What the speaker says. Where the N-th entry of `quotes` is spoken the
    /// text holds `{{quote:N}}`, not the quoted words: the script stage fills
    /// the placeholder in from the source.
    pub text: String,
    #[serde(default)]
    pub emotion: Emotion,
    #[serde(default)]
    pub citations: Vec<ClaimId>,
    #[serde(default)]
    pub quotes: Vec<QuoteRef>,
    /// Set on the first turn of each beat; the turns after it, up to the
    /// next mark, belong to the same beat. Only asked for when the script
    /// will be spoken. The script stage turns the marks into ranges, so the
    /// model never counts turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat: Option<BeatKind>,
    #[serde(default, skip_serializing_if = "Pace::is_normal")]
    pub pace: Pace,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nonverbal: Vec<Nonverbal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_to: Option<usize>,
}

impl DraftTurn {
    /// A plain turn: normal pace, no sounds, no callback.
    fn plain(speaker: SpeakerId, text: String, emotion: Emotion) -> Self {
        Self {
            speaker,
            text,
            emotion,
            citations: vec![],
            quotes: vec![],
            beat: None,
            pace: Pace::Normal,
            nonverbal: vec![],
            callback_to: None,
        }
    }
}

/// The sentence of a chunk that the turn quotes verbatim. The model counts
/// sentences far more reliably than bytes; the script stage turns this into a
/// span and copies the words out of the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuoteRef {
    pub chunk: ChunkId,
    /// Counting from 0, as in [`SourceText`].
    pub sentence: usize,
}

/// The most characters of a rejection reason that are shown to the model or
/// stored. A reason can quote part of the model's reply (serde names an
/// unknown variant in full), so it is bounded.
pub const MAX_REASON_CHARS: usize = 500;

/// `reason`, cut to [`MAX_REASON_CHARS`] with a trailing `…` if it was longer.
pub fn reason_excerpt(reason: &str) -> String {
    match reason.char_indices().nth(MAX_REASON_CHARS) {
        Some((end, _)) => format!("{}…", &reason[..end]),
        None => reason.to_owned(),
    }
}

/// Runs `request` and checks the reply with `validate` (parse it, resolve its
/// references, whatever the stage needs). If the reply is rejected, asks once
/// more with the rejection reason ([`reason_excerpt`]) appended to the
/// instructions, then gives up
/// with [`CoreError::InvalidProviderOutput`]. Transport failures are not
/// retried here: the provider has its own policy.
pub fn complete_validated<T>(
    llm: &dyn LlmProvider,
    stage: &'static str,
    request: &CompletionRequest,
    validate: impl Fn(&str) -> std::result::Result<T, String>,
) -> Result<T> {
    let first = llm.complete(request)?;
    let reason = match validate(&first.text) {
        Ok(value) => return Ok(value),
        Err(reason) => reason,
    };
    tracing::warn!(stage, %reason, "provider output rejected; asking once more");

    let excerpt = reason_excerpt(&reason);
    let retry = CompletionRequest {
        instructions: format!(
            "{}\n\nYour previous reply was rejected: {excerpt}\nReply again with the corrected JSON object only.",
            request.instructions
        ),
        ..request.clone()
    };
    let second = llm.complete(&retry)?;
    validate(&second.text).map_err(|message| CoreError::InvalidProviderOutput {
        stage,
        message: format!("{message} (after 2 attempts)"),
    })
}

/// A deterministic, offline stand-in for a real model.
///
/// Claim extraction returns every sentence of the chunk. Script writing
/// produces a two-host script: an opening that quotes the first sentence of
/// the first chunk, one turn per ledger claim (phrased by its status), and a
/// sign-off.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeLlm;

impl FakeLlm {
    fn extract_claims(input: &Value) -> Result<ClaimsDraft> {
        let text = input["chunk_text"]
            .as_str()
            .ok_or_else(|| invalid_input("chunk_text must be a string"))?;
        Ok(ClaimsDraft {
            claims: sentences(text)
                .into_iter()
                .map(|r| ClaimDraft {
                    text: text[r].to_owned(),
                })
                .collect(),
        })
    }

    fn write_script(input: &Value) -> Result<ScriptDraft> {
        let topic = input["topic"].as_str().unwrap_or("today's story");
        let ledger: Vec<LedgerClaim> = serde_json::from_value(input["ledger"].clone())
            .map_err(|err| invalid_input(format!("ledger: {err}")))?;
        let sources: Vec<SourceText> = serde_json::from_value(input["sources"].clone())
            .map_err(|err| invalid_input(format!("sources: {err}")))?;

        // A declared cast is used as given: the first speaker hosts, the
        // second (or the first again, for a solo show) answers.
        let declared: Vec<Speaker> = match input.get("cast") {
            Some(cast) => serde_json::from_value(cast.clone())
                .map_err(|err| invalid_input(format!("cast: {err}")))?,
            None => Vec::new(),
        };
        let cast = if declared.is_empty() {
            vec![
                Speaker {
                    id: SpeakerId("host".into()),
                    name: "Ada".into(),
                    role: "host".into(),
                },
                Speaker {
                    id: SpeakerId("guest".into()),
                    name: "Ben".into(),
                    role: "co-host".into(),
                },
            ]
        } else {
            declared
        };
        let host = cast[0].id.clone();
        let guest = cast.get(1).unwrap_or(&cast[0]).id.clone();

        let mut turns = vec![Self::opening(&host, topic, sources.first())];
        for (i, entry) in ledger.iter().enumerate() {
            let (lead, emotion) = match &entry.status {
                ClaimStatus::Corroborated { .. } => ("Independent sources agree", Emotion::Serious),
                ClaimStatus::SingleSource { .. } => ("One source reports", Emotion::Curious),
                ClaimStatus::Contested { .. } => ("The sources disagree here", Emotion::Serious),
                ClaimStatus::Unsupported => continue,
            };
            let speaker = if i % 2 == 0 {
                guest.clone()
            } else {
                host.clone()
            };
            // A claim copied from a source sentence can carry its quotation
            // marks, and a turn must not type quotations, so they are dropped.
            let claim: String = entry
                .text
                .chars()
                .filter(|c| !matches!(c, '"' | '\u{201C}' | '\u{201D}'))
                .collect();
            let text = match &entry.verdict {
                Some(verdict) => format!("{lead}: {claim} {}", verdict.explanation),
                None => format!("{lead}: {claim}"),
            };
            turns.push(DraftTurn {
                citations: vec![entry.id.clone()],
                ..DraftTurn::plain(speaker, text, emotion)
            });
        }
        turns.push(DraftTurn::plain(
            host.clone(),
            "That's all for today.".into(),
            Emotion::Neutral,
        ));
        let mut draft = ScriptDraft { cast, turns };
        if input.get("audio") == Some(&Value::Bool(true)) {
            Self::direct_for_audio(&mut draft, &host);
        }
        Ok(draft)
    }

    /// Adds one of each audio direction: beat marks (the opening, the claims
    /// as banter, the sign-off), a quick reply with the host's "mm-hm" over
    /// it, and a sign-off that pauses and calls back to the opening.
    fn direct_for_audio(draft: &mut ScriptDraft, host: &SpeakerId) {
        let n = draft.turns.len();
        let opening = &mut draft.turns[0];
        opening.beat = Some(if opening.quotes.is_empty() {
            BeatKind::Narration
        } else {
            BeatKind::QuoteReading
        });
        // Turns 1..n-1 are the claims, if the ledger had any usable ones.
        if n > 2 {
            let reply = &mut draft.turns[1];
            reply.beat = Some(BeatKind::Banter);
            reply.pace = Pace::Quick;
            reply.nonverbal.push(Nonverbal {
                kind: NonverbalKind::Backchannel {
                    text: "Mm-hm.".into(),
                },
                by: host.clone(),
                at: NonverbalAt::Over,
            });
        }
        let sign_off = &mut draft.turns[n - 1];
        sign_off.beat = Some(BeatKind::Transition);
        sign_off.pace = Pace::LongPause;
        sign_off.callback_to = Some(0);
    }

    /// Never takes a side: cites the first piece of evidence on each side and
    /// says the sources disagree.
    fn adjudicate(input: &Value) -> Result<VerdictDraft> {
        let claim: AdjudicationClaim = serde_json::from_value(input["claim"].clone())
            .map_err(|err| invalid_input(format!("claim: {err}")))?;
        let evidence: Vec<AdjudicationEvidence> = serde_json::from_value(input["evidence"].clone())
            .map_err(|err| invalid_input(format!("evidence: {err}")))?;
        let first = |stance| evidence.iter().find(|e| e.stance == stance);
        let sides: Vec<&AdjudicationEvidence> = [Stance::Supports, Stance::Contradicts]
            .into_iter()
            .filter_map(first)
            .collect();
        let explanation = match sides.as_slice() {
            [supporting, contradicting] => format!(
                "The {} source and the {} source give different accounts, and the sources \
                 do not settle which is right.",
                supporting.independence_group, contradicting.independence_group
            ),
            _ => "The sources give different accounts, and they do not settle which is right."
                .to_owned(),
        };
        Ok(VerdictDraft {
            claim: claim.id,
            favours: Favours::Unresolved,
            explanation,
            cites: sides.iter().map(|e| e.n).collect(),
        })
    }

    fn opening(host: &SpeakerId, topic: &str, first_source: Option<&SourceText>) -> DraftTurn {
        let quote = first_source.and_then(|source| {
            let first = source.sentences.first()?;
            Some(QuoteRef {
                chunk: source.chunk.clone(),
                sentence: first.sentence,
            })
        });
        match quote {
            Some(quote_ref) => DraftTurn {
                quotes: vec![quote_ref],
                ..DraftTurn::plain(
                    host.clone(),
                    format!("Today: {topic}. It begins with this: {{{{quote:0}}}}"),
                    Emotion::Curious,
                )
            },
            None => DraftTurn::plain(host.clone(), format!("Today: {topic}."), Emotion::Curious),
        }
    }
}

fn invalid_input(message: impl Into<String>) -> CoreError {
    CoreError::Provider {
        plugin: "fake".into(),
        kind: crate::error::ProviderFailure::Other,
        message: message.into(),
    }
}

impl LlmProvider for FakeLlm {
    fn id(&self) -> &str {
        "fake"
    }

    fn fingerprint(&self) -> Value {
        // Bump when the fake's behaviour changes.
        // 3: the opening turn says `{{quote:0}}` instead of typing the sentence.
        // 4: a declared cast in the script request is used.
        // 5: a script request for audio gets beats, pace, a backchannel and
        //    a callback.
        // 6: beats are marked on the turns that begin them.
        // 7: answers `AdjudicateClaim`; a judged claim's turn adds the
        //    verdict's explanation, and quotation marks are dropped from claims.
        json!({ "provider": "fake", "version": 7 })
    }

    fn complete(&self, request: &CompletionRequest) -> Result<Completion> {
        let text = match request.task {
            LlmTask::ExtractClaims => {
                serde_json::to_string(&Self::extract_claims(&request.input)?)?
            }
            LlmTask::WriteScript => serde_json::to_string(&Self::write_script(&request.input)?)?,
            LlmTask::AdjudicateClaim => serde_json::to_string(&Self::adjudicate(&request.input)?)?,
        };
        Ok(Completion { text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use podling_types::{Chunk, Claim, Document, SourceRef, TextSpan};

    fn request(task: LlmTask, input: Value) -> CompletionRequest {
        CompletionRequest {
            task,
            instructions: String::new(),
            input,
        }
    }

    #[test]
    fn a_reason_excerpt_is_cut_on_a_character_boundary() {
        assert_eq!(reason_excerpt("short"), "short");
        let exact = "é".repeat(MAX_REASON_CHARS);
        assert_eq!(reason_excerpt(&exact), exact);
        let long = "é".repeat(MAX_REASON_CHARS + 3);
        let cut = reason_excerpt(&long);
        assert_eq!(cut.chars().count(), MAX_REASON_CHARS + 1);
        assert!(cut.ends_with("é…"));
    }

    #[test]
    fn extract_claims_returns_sentences_deterministically() {
        let req = request(
            LlmTask::ExtractClaims,
            json!({"chunk_text": "Trees fell. The sky lit up!"}),
        );
        let a = FakeLlm.complete(&req).unwrap();
        assert_eq!(a, FakeLlm.complete(&req).unwrap());
        let drafts: ClaimsDraft = serde_json::from_str(&a.text).unwrap();
        assert_eq!(
            drafts.claims,
            vec![
                ClaimDraft {
                    text: "Trees fell.".into()
                },
                ClaimDraft {
                    text: "The sky lit up!".into()
                },
            ]
        );
    }

    #[test]
    fn write_script_quotes_the_first_sentence_by_reference() {
        let source = SourceRef {
            connector: "t".into(),
            locator: "a".into(),
            independence_group: "g".into(),
        };
        let doc = Document::new(source, "A", "# Title\n\nA flash was seen. Then a boom.");
        let chunk = Chunk::from_document(&doc, TextSpan::new(9, doc.text().len()).unwrap(), vec![])
            .unwrap();
        let ledger = Ledger::from_claims([Claim::new("A flash was seen.")]);
        let sources = json!([{
            "chunk": chunk.id(),
            "title": "A",
            "sentences": [
                { "sentence": 0, "text": "A flash was seen." },
                { "sentence": 1, "text": "Then a boom." },
            ],
        }]);
        let input = json!({
            "topic": "Tunguska",
            "ledger": LedgerClaim::from_ledger(&ledger, &Verdicts::default()),
            "sources": sources,
        });

        let completion = FakeLlm
            .complete(&request(LlmTask::WriteScript, input))
            .unwrap();
        let draft: ScriptDraft = serde_json::from_str(&completion.text).unwrap();
        assert_eq!(
            draft.turns[0].quotes,
            vec![QuoteRef {
                chunk: chunk.id().clone(),
                sentence: 0
            }]
        );
        // The fake points at the sentence and never types it.
        assert!(draft.turns[0].text.contains("{{quote:0}}"));
        assert!(!draft.turns[0].text.contains('"'));
        assert!(!draft.turns[0].text.contains("A flash was seen."));
    }

    /// Replies with each canned text in turn and records every request.
    struct Scripted {
        replies: std::cell::RefCell<std::collections::VecDeque<&'static str>>,
        seen: std::cell::RefCell<Vec<CompletionRequest>>,
    }

    impl Scripted {
        fn new(replies: &[&'static str]) -> Self {
            Self {
                replies: std::cell::RefCell::new(replies.iter().copied().collect()),
                seen: Default::default(),
            }
        }
    }

    impl LlmProvider for Scripted {
        fn id(&self) -> &str {
            "scripted"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn complete(&self, request: &CompletionRequest) -> Result<Completion> {
            self.seen.borrow_mut().push(request.clone());
            let text = self.replies.borrow_mut().pop_front().unwrap_or("");
            Ok(Completion { text: text.into() })
        }
    }

    fn parse_object(text: &str) -> std::result::Result<Value, String> {
        serde_json::from_str::<Value>(text)
            .map_err(|err| err.to_string())
            .and_then(|v| {
                v.is_object()
                    .then_some(v)
                    .ok_or_else(|| "not an object".to_string())
            })
    }

    #[test]
    fn a_reply_that_fails_once_is_retried_with_the_reason_appended() {
        let llm = Scripted::new(&["nonsense", r#"{"ok":true}"#]);
        let req = request(LlmTask::ExtractClaims, json!({}));
        let value = complete_validated(&llm, "s", &req, parse_object).unwrap();
        assert_eq!(value["ok"], true);

        let seen = llm.seen.borrow();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0], req);
        assert!(
            seen[1].instructions.contains("previous reply was rejected"),
            "{}",
            seen[1].instructions
        );
        assert_eq!(seen[1].input, req.input, "only the instructions change");
    }

    #[test]
    fn a_reply_that_always_fails_errors_after_exactly_two_calls() {
        let llm = Scripted::new(&["nonsense", "still nonsense", r#"{"ok":true}"#]);
        let req = request(LlmTask::ExtractClaims, json!({}));
        let err = complete_validated(&llm, "s", &req, parse_object).unwrap_err();
        assert!(
            matches!(&err, CoreError::InvalidProviderOutput { stage: "s", message }
                if message.contains("after 2 attempts")),
            "{err}"
        );
        assert_eq!(llm.seen.borrow().len(), 2);
    }

    #[test]
    fn a_good_first_reply_is_one_call() {
        let llm = Scripted::new(&[r#"{"ok":true}"#]);
        let req = request(LlmTask::ExtractClaims, json!({}));
        complete_validated(&llm, "s", &req, parse_object).unwrap();
        assert_eq!(llm.seen.borrow().len(), 1);
    }

    #[test]
    fn provider_errors_are_not_retried_here() {
        struct Down;
        impl LlmProvider for Down {
            fn id(&self) -> &str {
                "down"
            }
            fn fingerprint(&self) -> Value {
                Value::Null
            }
            fn complete(&self, _: &CompletionRequest) -> Result<Completion> {
                Err(invalid_input("unreachable"))
            }
        }
        let req = request(LlmTask::ExtractClaims, json!({}));
        let err = complete_validated(&Down, "s", &req, parse_object).unwrap_err();
        assert!(matches!(err, CoreError::Provider { .. }));
    }

    #[test]
    fn the_fake_adjudicator_cites_both_sides_and_takes_none() {
        let claim = podling_types::Claim::new("The blast was in 1908.");
        let input = json!({
            "claim": { "id": claim.id(), "text": claim.text() },
            "evidence": [
                { "n": 0, "stance": "supports", "source": "A", "independence_group": "a", "text": "1908." },
                { "n": 1, "stance": "supports", "source": "A", "independence_group": "a", "text": "1908!" },
                { "n": 2, "stance": "contradicts", "source": "B", "independence_group": "b", "text": "1907." },
            ],
        });
        let reply = FakeLlm
            .complete(&request(LlmTask::AdjudicateClaim, input))
            .unwrap();
        let draft: VerdictDraft = serde_json::from_str(&reply.text).unwrap();
        assert_eq!(&draft.claim, claim.id());
        assert_eq!(draft.favours, Favours::Unresolved);
        assert_eq!(draft.cites, vec![0, 2]);
        assert!(
            draft
                .explanation
                .starts_with("The a source and the b source give different accounts"),
            "{}",
            draft.explanation
        );
        assert!(!draft.explanation.contains('"'));
    }

    #[test]
    fn rejects_malformed_input() {
        let err = FakeLlm
            .complete(&request(LlmTask::ExtractClaims, json!({})))
            .unwrap_err();
        assert!(matches!(err, CoreError::Provider { .. }));
    }
}
