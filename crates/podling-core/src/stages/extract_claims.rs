//! Asks the LLM for the claims in each chunk and merges duplicates.

use std::collections::BTreeMap;

use podling_types::{Chunk, Claim, ClaimId, DocumentId, Evidence, SourceRef, Stance};
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{CoreError, Result};
use crate::plugin::{ClaimDraft, CompletionRequest, LlmProvider, LlmTask};
use crate::stage::Stage;

const INSTRUCTIONS: &str = "Extract every atomic, checkable factual claim stated in the text. \
Return a JSON array of objects with a single `text` field. Do not add facts that are not in the text.";

#[derive(Debug, Clone, Serialize)]
pub struct ClaimInput {
    pub chunks: Vec<Chunk>,
    /// Where each chunk's document came from, for evidence attribution.
    pub sources: BTreeMap<DocumentId, SourceRef>,
}

pub struct ExtractClaims<'a> {
    pub llm: &'a dyn LlmProvider,
}

impl Stage for ExtractClaims<'_> {
    const ID: &'static str = "extract_claims";
    const VERSION: u32 = 1;
    type Input = ClaimInput;
    type Output = Vec<Claim>;

    fn config_fingerprint(&self) -> Value {
        json!({ "llm": self.llm.fingerprint(), "instructions": INSTRUCTIONS })
    }

    fn run(&self, input: &ClaimInput) -> Result<Vec<Claim>> {
        let mut claims: BTreeMap<ClaimId, Claim> = BTreeMap::new();
        for chunk in &input.chunks {
            let source = input.sources.get(chunk.document()).ok_or_else(|| {
                invalid(format!(
                    "chunk {} belongs to an unknown document",
                    chunk.id()
                ))
            })?;
            let completion = self.llm.complete(&CompletionRequest {
                task: LlmTask::ExtractClaims,
                instructions: INSTRUCTIONS.to_owned(),
                input: json!({ "chunk_text": chunk.text() }),
            })?;
            let drafts: Vec<ClaimDraft> = serde_json::from_str(&completion.text)
                .map_err(|err| invalid(format!("chunk {}: {err}", chunk.id())))?;

            for draft in drafts.into_iter().filter(|d| !d.text.trim().is_empty()) {
                let id = Claim::id_for(&draft.text);
                let claim = claims
                    .entry(id)
                    .or_insert_with(|| Claim::new(draft.text.trim()));
                claim.add_evidence(Evidence {
                    chunk: chunk.id().clone(),
                    source: source.id(),
                    independence_group: source.independence_group.clone(),
                    stance: Stance::Supports,
                });
            }
        }
        Ok(claims.into_values().collect())
    }
}

fn invalid(message: String) -> CoreError {
    CoreError::InvalidProviderOutput {
        stage: ExtractClaims::ID,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{Completion, FakeLlm};
    use podling_types::{Document, TextSpan};

    fn chunk_for(locator: &str, group: &str, text: &str) -> (Chunk, SourceRef) {
        let source = SourceRef {
            connector: "t".into(),
            locator: locator.into(),
            independence_group: group.into(),
        };
        let doc = Document::new(source.clone(), locator, text);
        let span = TextSpan::new(0, text.len()).unwrap();
        (Chunk::from_document(&doc, span, vec![]).unwrap(), source)
    }

    fn input(chunks: Vec<(Chunk, SourceRef)>) -> ClaimInput {
        ClaimInput {
            sources: chunks
                .iter()
                .map(|(c, s)| (c.document().clone(), s.clone()))
                .collect(),
            chunks: chunks.into_iter().map(|(c, _)| c).collect(),
        }
    }

    #[test]
    fn merges_the_same_claim_across_sources() {
        let input = input(vec![
            chunk_for("a", "eyewitness", "The sky split in two. It was hot."),
            chunk_for(
                "b",
                "expedition",
                "The sky split in two. Trees were flattened.",
            ),
        ]);
        let claims = ExtractClaims { llm: &FakeLlm }.run(&input).unwrap();
        let shared = claims
            .iter()
            .find(|c| c.text() == "The sky split in two.")
            .unwrap();
        assert_eq!(shared.evidence().len(), 2);
        assert_eq!(claims.len(), 3);
    }

    struct Garbage;
    impl LlmProvider for Garbage {
        fn id(&self) -> &str {
            "garbage"
        }
        fn fingerprint(&self) -> Value {
            Value::Null
        }
        fn complete(&self, _: &CompletionRequest) -> Result<Completion> {
            Ok(Completion {
                text: "not json".into(),
            })
        }
    }

    #[test]
    fn invalid_provider_output_is_reported() {
        let input = input(vec![chunk_for("a", "g", "Text.")]);
        let err = ExtractClaims { llm: &Garbage }.run(&input).unwrap_err();
        assert!(matches!(
            err,
            CoreError::InvalidProviderOutput {
                stage: "extract_claims",
                ..
            }
        ));
    }
}
