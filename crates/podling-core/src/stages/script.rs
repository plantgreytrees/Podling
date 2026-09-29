//! Asks the LLM for a script and turns its quote references into real quotes.

use podling_types::{Chunk, Document, Ledger, Quote, Script, TextSpan, Turn};
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{CoreError, Result};
use crate::plugin::{CompletionRequest, LlmProvider, LlmTask, QuoteRef, ScriptDraft};
use crate::stage::Stage;

const INSTRUCTIONS: &str = "Write a two-host podcast script about the topic using only claims from the ledger. \
Cite claim ids for every factual statement. To quote a source, give its document id and byte span; \
never type quoted words yourself. Present contested and single-source claims as such.";

#[derive(Debug, Clone, Serialize)]
pub struct ScriptInput {
    pub topic: String,
    pub target_minutes: u16,
    pub ledger: Ledger,
    pub chunks: Vec<Chunk>,
    pub documents: Vec<Document>,
}

pub struct WriteScript<'a> {
    pub llm: &'a dyn LlmProvider,
}

impl Stage for WriteScript<'_> {
    const ID: &'static str = "script";
    const VERSION: u32 = 1;
    type Input = ScriptInput;
    type Output = Script;

    fn config_fingerprint(&self) -> Value {
        json!({ "llm": self.llm.fingerprint(), "instructions": INSTRUCTIONS })
    }

    fn run(&self, input: &ScriptInput) -> Result<Script> {
        let completion = self.llm.complete(&CompletionRequest {
            task: LlmTask::WriteScript,
            instructions: INSTRUCTIONS.to_owned(),
            input: json!({
                "topic": input.topic,
                "target_minutes": input.target_minutes,
                "ledger": input.ledger,
                "chunks": input.chunks,
            }),
        })?;
        let draft: ScriptDraft =
            serde_json::from_str(&completion.text).map_err(|err| invalid(err.to_string()))?;

        let mut turns = Vec::with_capacity(draft.turns.len());
        for (i, turn) in draft.turns.into_iter().enumerate() {
            let quotes = turn
                .quotes
                .iter()
                .map(|r| {
                    resolve(r, &input.documents).map_err(|m| invalid(format!("turn {i}: {m}")))
                })
                .collect::<Result<Vec<_>>>()?;
            turns.push(Turn {
                speaker: turn.speaker,
                text: turn.text,
                emotion: turn.emotion,
                citations: turn.citations,
                quotes,
            });
        }
        Script::new(draft.cast, turns).map_err(|err| invalid(err.to_string()))
    }
}

/// Copies the quoted words out of the source document. The LLM only ever
/// points at a span, so it cannot put words in a source's mouth.
fn resolve(quote: &QuoteRef, documents: &[Document]) -> std::result::Result<Quote, String> {
    let doc = documents
        .iter()
        .find(|d| d.id() == &quote.document)
        .ok_or_else(|| format!("quote cites unknown document {}", quote.document))?;
    let span = TextSpan::new(quote.start, quote.end).map_err(|err| err.to_string())?;
    Quote::from_document(doc, span).map_err(|err| err.to_string())
}

fn invalid(message: String) -> CoreError {
    CoreError::InvalidProviderOutput {
        stage: WriteScript::ID,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{Completion, FakeLlm};
    use podling_types::{DocumentId, SourceRef};

    fn document(text: &str) -> Document {
        let source = SourceRef {
            connector: "t".into(),
            locator: "a".into(),
            independence_group: "g".into(),
        };
        Document::new(source, "A", text)
    }

    #[test]
    fn resolves_quotes_by_copying_source_text() {
        let doc = document("The sky split in two.");
        let r = QuoteRef {
            document: doc.id().clone(),
            start: 4,
            end: 7,
        };
        assert_eq!(resolve(&r, &[doc]).unwrap().text(), "sky");
    }

    #[test]
    fn rejects_out_of_bounds_and_unknown_documents() {
        let doc = document("The sky split in two.");
        let bad_span = QuoteRef {
            document: doc.id().clone(),
            start: 4,
            end: 400,
        };
        assert!(resolve(&bad_span, std::slice::from_ref(&doc)).is_err());
        let other = document("Other.");
        let unknown = QuoteRef {
            document: other.id().clone(),
            start: 0,
            end: 1,
        };
        assert!(resolve(&unknown, &[doc]).is_err());
    }

    /// Returns a script whose only quote points past the end of the source.
    struct BadQuote(DocumentId);

    impl LlmProvider for BadQuote {
        fn id(&self) -> &str {
            "bad_quote"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn complete(&self, _: &CompletionRequest) -> Result<Completion> {
            let text = json!({
                "cast": [{ "id": "host", "name": "Ada", "role": "host" }],
                "turns": [{
                    "speaker": "host", "text": "Hi", "emotion": "neutral", "citations": [],
                    "quotes": [{ "document": self.0, "start": 0, "end": 999 }],
                }],
            });
            Ok(Completion {
                text: text.to_string(),
            })
        }
    }

    fn empty_input(documents: Vec<Document>) -> ScriptInput {
        ScriptInput {
            topic: "T".into(),
            target_minutes: 5,
            ledger: Ledger::from_claims([]),
            chunks: vec![],
            documents,
        }
    }

    #[test]
    fn unresolvable_quote_is_invalid_provider_output() {
        let doc = document("The sky split in two.");
        let llm = BadQuote(doc.id().clone());
        let err = WriteScript { llm: &llm }
            .run(&empty_input(vec![doc]))
            .unwrap_err();
        assert!(
            matches!(
                err,
                CoreError::InvalidProviderOutput {
                    stage: "script",
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn fake_llm_script_is_valid() {
        let script = WriteScript { llm: &FakeLlm }
            .run(&empty_input(vec![]))
            .unwrap();
        assert_eq!(script.cast().len(), 2);
    }
}
