//! Text-generation providers and the output shapes they must produce.

use podling_types::{Chunk, ClaimId, ClaimStatus, DocumentId, Emotion, Ledger, Speaker, SpeakerId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{CoreError, Result};
use crate::text::sentences;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmTask {
    /// Input: `{ "chunk_text": string }`. Output: JSON `Vec<ClaimDraft>`.
    ExtractClaims,
    /// Input: `{ "topic": string, "ledger": Ledger, "chunks": [Chunk] }`.
    /// Output: JSON `ScriptDraft`.
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

/// One claim as returned by [`LlmTask::ExtractClaims`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimDraft {
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

/// A byte range in a document that the turn quotes verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuoteRef {
    pub document: DocumentId,
    pub start: usize,
    pub end: usize,
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
    fn extract_claims(input: &Value) -> Result<Vec<ClaimDraft>> {
        let text = input["chunk_text"]
            .as_str()
            .ok_or_else(|| invalid_input("chunk_text must be a string"))?;
        Ok(sentences(text)
            .into_iter()
            .map(|r| ClaimDraft {
                text: text[r].to_owned(),
            })
            .collect())
    }

    fn write_script(input: &Value) -> Result<ScriptDraft> {
        let topic = input["topic"].as_str().unwrap_or("today's story");
        let ledger: Ledger = serde_json::from_value(input["ledger"].clone())
            .map_err(|err| invalid_input(format!("ledger: {err}")))?;
        let chunks: Vec<Chunk> = serde_json::from_value(input["chunks"].clone())
            .map_err(|err| invalid_input(format!("chunks: {err}")))?;

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

        let mut turns = vec![Self::opening(&host, topic, chunks.first())];
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

    fn opening(host: &SpeakerId, topic: &str, first_chunk: Option<&Chunk>) -> DraftTurn {
        let quote = first_chunk.and_then(|chunk| {
            let first = sentences(chunk.text()).into_iter().next()?;
            let start = chunk.span().start() + first.start;
            Some((
                QuoteRef {
                    document: chunk.document().clone(),
                    start,
                    end: chunk.span().start() + first.end,
                },
                &chunk.text()[first],
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
        message: message.into(),
    }
}

impl LlmProvider for FakeLlm {
    fn id(&self) -> &str {
        "fake"
    }

    fn fingerprint(&self) -> Value {
        // Bump when the fake's behaviour changes.
        json!({ "provider": "fake", "version": 1 })
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
    use podling_types::{Claim, Document, SourceRef, TextSpan};

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
        let drafts: Vec<ClaimDraft> = serde_json::from_str(&a.text).unwrap();
        assert_eq!(
            drafts,
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
        let input = json!({"topic": "Tunguska", "ledger": ledger, "chunks": [chunk]});

        let completion = FakeLlm
            .complete(&request(LlmTask::WriteScript, input))
            .unwrap();
        let draft: ScriptDraft = serde_json::from_str(&completion.text).unwrap();
        let quote = &draft.turns[0].quotes[0];
        assert_eq!(&doc.text()[quote.start..quote.end], "A flash was seen.");
        assert!(draft.turns[0].text.contains("\"A flash was seen.\""));
    }

    #[test]
    fn rejects_malformed_input() {
        let err = FakeLlm
            .complete(&request(LlmTask::ExtractClaims, json!({})))
            .unwrap_err();
        assert!(matches!(err, CoreError::Provider { .. }));
    }
}
