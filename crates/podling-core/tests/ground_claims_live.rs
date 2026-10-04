//! `ground_claims` with the real NLI model: the distortion the lexical check
//! lets through is dropped, and the faithful claim is kept. Ignored by
//! default because it needs the model weights:
//!
//!   hf download cross-encoder/nli-deberta-v3-base --local-dir <dir>
//!   PODLING_NLI_MODEL_DIR=<dir> cargo test -p podling-core --test ground_claims_live -- --ignored

use std::collections::BTreeMap;
use std::path::PathBuf;

use podling_core::Stage;
use podling_core::plugin::{CrossEncoderNli, FakeEmbedding};
use podling_core::stages::{GroundClaims, GroundInput};
use podling_types::{Chunk, Claim, Document, Evidence, SourceRef, Stance, TextSpan};

#[test]
#[ignore = "needs the NLI model; set PODLING_NLI_MODEL_DIR"]
fn the_real_model_drops_led_and_keeps_joined() {
    let Ok(dir) = std::env::var("PODLING_NLI_MODEL_DIR") else {
        // Fail, don't skip: an ignored test that returns early reports "ok".
        panic!("set PODLING_NLI_MODEL_DIR to a local cross-encoder/nli-deberta-v3-base snapshot");
    };
    let nli = CrossEncoderNli::new(&PathBuf::from(dir)).unwrap();

    let text = "Leonid Kulik joined the 1927 expedition to the site.";
    let source = SourceRef {
        connector: "t".into(),
        locator: "expedition".into(),
        independence_group: "expedition".into(),
    };
    let doc = Document::new(source.clone(), "expedition", text);
    let chunk = Chunk::from_document(&doc, TextSpan::new(0, text.len()).unwrap(), vec![]).unwrap();
    let claim = |text: &str| {
        let mut claim = Claim::new(text);
        claim.add_evidence(Evidence {
            chunk: chunk.id().clone(),
            source: source.id(),
            independence_group: source.independence_group.clone(),
            stance: Stance::Supports,
            basis: None,
        });
        claim
    };
    let input = GroundInput {
        claims: vec![
            claim("Kulik led the 1927 expedition."),
            claim("Kulik joined the 1927 expedition."),
        ],
        chunks: vec![chunk.clone()],
        titles: BTreeMap::from([(doc.id().clone(), "The 1927 expedition".to_owned())]),
    };

    let grounded = GroundClaims {
        embedder: &FakeEmbedding,
        nli: &nli,
    }
    .run(&input)
    .unwrap();
    let kept: Vec<&str> = grounded.claims.iter().map(Claim::text).collect();
    assert_eq!(kept, ["Kulik joined the 1927 expedition."]);
    assert_eq!(grounded.rejected.len(), 1);
    assert_eq!(grounded.rejected[0].text, "Kulik led the 1927 expedition.");
    eprintln!(
        "led: entailment {} per mille",
        grounded.rejected[0].entailment_pm.get()
    );
}
