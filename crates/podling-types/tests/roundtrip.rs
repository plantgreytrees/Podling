//! Every artifact survives a JSON round trip, and the episode TOML is strict.

use podling_types::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) {
    let json = serde_json::to_string(value).unwrap();
    let back: T = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, value);
}

fn document() -> Document {
    let source = SourceRef {
        connector: "local_files".into(),
        locator: "eyewitness.md".into(),
        independence_group: "eyewitness".into(),
    };
    Document::new(
        source,
        "Eyewitness",
        "A fireball crossed the sky. The ground shook.",
    )
}

#[test]
fn artifacts_roundtrip() {
    let doc = document();
    let chunk =
        Chunk::from_document(&doc, TextSpan::new(0, 27).unwrap(), vec!["Intro".into()]).unwrap();
    let mut claim = Claim::new("A fireball crossed the sky.");
    claim.add_evidence(Evidence {
        chunk: chunk.id().clone(),
        source: doc.source().id(),
        independence_group: "eyewitness".into(),
        stance: Stance::Supports,
    });
    let ledger = Ledger::from_claims([claim.clone()]);
    let quote = Quote::from_document(&doc, TextSpan::new(0, 27).unwrap()).unwrap();
    let script = Script::new(
        vec![Speaker {
            id: SpeakerId("host".into()),
            name: "Ada".into(),
            role: "host".into(),
        }],
        vec![Turn {
            speaker: SpeakerId("host".into()),
            text: format!("One witness said: \"{}\"", quote.text()),
            emotion: Emotion::Serious,
            citations: vec![claim.id().clone()],
            quotes: vec![quote],
        }],
    )
    .unwrap();
    let report = AnalysisReport {
        findings: vec![Finding {
            analyser: "x".into(),
            severity: Severity::Info,
            turn: Some(0),
            message: "ok".into(),
        }],
    };

    roundtrip(&Envelope::new(ArtifactKind::Documents, vec![doc]));
    roundtrip(&Envelope::new(ArtifactKind::Chunks, vec![chunk]));
    roundtrip(&Envelope::new(ArtifactKind::Claims, vec![claim]));
    roundtrip(&Envelope::new(ArtifactKind::Ledger, ledger));
    roundtrip(&Envelope::new(ArtifactKind::Script, script));
    roundtrip(&Envelope::new(ArtifactKind::Analysis, report));
}

const EPISODE: &str = r#"
title = "The Tunguska Event"
topic = "What flattened 2,000 km² of Siberian forest in 1908?"
target_minutes = 10

[llm]
kind = "fake"

[[sources]]
kind = "local_files"
root = "sources/eyewitness"
independence_group = "eyewitness"

[[analysers]]
kind = "quote_verifier"
"#;

#[test]
fn episode_toml_parses() {
    let spec: EpisodeSpec = toml::from_str(EPISODE).unwrap();
    assert_eq!(spec.mode, Mode::NonFiction);
    assert_eq!(spec.llm, LlmConfig::Fake {});
    assert_eq!(spec.analysers, vec![AnalyserConfig::QuoteVerifier {}]);
    assert!(
        matches!(&spec.sources[0], SourceSpec::LocalFiles { independence_group, .. } if independence_group == "eyewitness")
    );
}

#[test]
fn episode_rejects_unknown_keys() {
    let top_level = format!("api_key = \"sk-123\"\n{EPISODE}");
    let err = toml::from_str::<EpisodeSpec>(&top_level)
        .unwrap_err()
        .to_string();
    assert!(err.contains("api_key"), "{err}");

    let nested = EPISODE.replace("kind = \"fake\"", "kind = \"fake\"\ntemperature = 0.2");
    let err = toml::from_str::<EpisodeSpec>(&nested)
        .unwrap_err()
        .to_string();
    assert!(err.contains("temperature"), "{err}");
}
