//! Text-generation providers and the output shapes they must produce.

use podling_types::{ChunkId, ClaimId, ClaimStatus, Emotion, Ledger, Speaker, SpeakerId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{CoreError, Result};
use crate::text::sentences;

/// Version of the stage prompts and the input shapes they describe. Part of
/// both LLM stages' cache keys: bump it when a prompt or an input layout
/// changes in a way the instruction text alone would not show.
pub const PROMPT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmTask {
    /// Input: `{ "chunk_text": string }`. Output: a JSON [`ClaimsDraft`]. It is
    /// an object, not a bare array, because JSON mode only guarantees objects.
    ExtractClaims,
    /// Input: `{ "topic", "target_minutes", "ledger": Ledger,
    /// "sources": [SourceText] }`. Output: JSON `ScriptDraft`.
    WriteScript,
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
    pub text: String,
    #[serde(default)]
    pub emotion: Emotion,
    #[serde(default)]
    pub citations: Vec<ClaimId>,
    #[serde(default)]
    pub quotes: Vec<QuoteRef>,
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

/// Runs `request` and checks the reply with `validate` (parse it, resolve its
/// references, whatever the stage needs). If the reply is rejected, asks once
/// more with the rejection reason appended to the instructions, then gives up
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

    let excerpt: String = reason.chars().take(500).collect();
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
        let ledger: Ledger = serde_json::from_value(input["ledger"].clone())
            .map_err(|err| invalid_input(format!("ledger: {err}")))?;
        let sources: Vec<SourceText> = serde_json::from_value(input["sources"].clone())
            .map_err(|err| invalid_input(format!("sources: {err}")))?;

        let host = SpeakerId("host".into());
        let guest = SpeakerId("guest".into());
        let cast = vec![
            Speaker {
                id: host.clone(),
                name: "Ada".into(),
                role: "host".into(),
            },
            Speaker {
                id: guest.clone(),
                name: "Ben".into(),
                role: "co-host".into(),
            },
        ];

        let mut turns = vec![Self::opening(&host, topic, sources.first())];
        for (i, entry) in ledger.entries().iter().enumerate() {
            let (lead, emotion) = match &entry.status {
                ClaimStatus::Corroborated { .. } => ("Independent sources agree", Emotion::Serious),
                ClaimStatus::SingleSource { .. } => ("One source reports", Emotion::Curious),
                ClaimStatus::Contested { .. } => ("The sources disagree here", Emotion::Serious),
                ClaimStatus::Unsupported => continue,
            };
            turns.push(DraftTurn {
                speaker: if i % 2 == 0 {
                    guest.clone()
                } else {
                    host.clone()
                },
                text: format!("{lead}: {}", entry.claim.text()),
                emotion,
                citations: vec![entry.claim.id().clone()],
                quotes: vec![],
            });
        }
        turns.push(DraftTurn {
            speaker: host,
            text: "That's all for today.".into(),
            emotion: Emotion::Neutral,
            citations: vec![],
            quotes: vec![],
        });
        Ok(ScriptDraft { cast, turns })
    }

    fn opening(host: &SpeakerId, topic: &str, first_source: Option<&SourceText>) -> DraftTurn {
        let quote = first_source.and_then(|source| {
            let first = source.sentences.first()?;
            Some((
                QuoteRef {
                    chunk: source.chunk.clone(),
                    sentence: first.sentence,
                },
                first.text.as_str(),
            ))
        });
        match quote {
            Some((quote_ref, words)) => DraftTurn {
                speaker: host.clone(),
                text: format!("Today: {topic}. It begins with this: \"{words}\""),
                emotion: Emotion::Curious,
                citations: vec![],
                quotes: vec![quote_ref],
            },
            None => DraftTurn {
                speaker: host.clone(),
                text: format!("Today: {topic}."),
                emotion: Emotion::Curious,
                citations: vec![],
                quotes: vec![],
            },
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
        json!({ "provider": "fake", "version": 2 })
    }

    fn complete(&self, request: &CompletionRequest) -> Result<Completion> {
        let text = match request.task {
            LlmTask::ExtractClaims => {
                serde_json::to_string(&Self::extract_claims(&request.input)?)?
            }
            LlmTask::WriteScript => serde_json::to_string(&Self::write_script(&request.input)?)?,
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
        let input = json!({"topic": "Tunguska", "ledger": ledger, "sources": sources});

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
        assert!(draft.turns[0].text.contains("\"A flash was seen.\""));
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
    fn rejects_malformed_input() {
        let err = FakeLlm
            .complete(&request(LlmTask::ExtractClaims, json!({})))
            .unwrap_err();
        assert!(matches!(err, CoreError::Provider { .. }));
    }
}
