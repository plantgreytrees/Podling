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
        basis: None,
    });
    // Evidence from the NLI stages carries its audit trail.
    claim.add_evidence(Evidence {
        chunk: chunk.id().clone(),
        source: doc.source().id(),
        independence_group: "archive".into(),
        stance: Stance::Supports,
        basis: Some(EvidenceBasis::Merged {
            wording: "A fireball was seen crossing the sky.".into(),
            entailment_pm: PerMille::new(950).unwrap(),
        }),
    });
    claim.add_evidence(Evidence {
        chunk: chunk.id().clone(),
        source: doc.source().id(),
        independence_group: "survey".into(),
        stance: Stance::Contradicts,
        basis: Some(EvidenceBasis::Nli {
            premise: TextSpan::new(0, 27).unwrap(),
            similarity_pm: PerMille::new(812).unwrap(),
            entailment_pm: PerMille::new(1).unwrap(),
            contradiction_pm: PerMille::from_probability(0.987),
        }),
    });
    let ledger = Ledger::from_claims([claim.clone()]);
    let verdicts = Verdicts::new(vec![
        Verdict::new(
            claim.id().clone(),
            Favours::Contradicting,
            "The survey measured the site; the eyewitness wrote from memory.",
            vec![
                EvidenceRef {
                    chunk: chunk.id().clone(),
                    stance: Stance::Supports,
                    premise: None,
                },
                EvidenceRef {
                    chunk: chunk.id().clone(),
                    stance: Stance::Contradicts,
                    premise: Some(TextSpan::new(0, 27).unwrap()),
                },
            ],
            None,
        )
        .unwrap(),
    ])
    .unwrap();
    let quote = Quote::from_document(&doc, TextSpan::new(0, 27).unwrap()).unwrap();
    let host = SpeakerId("host".into());
    let script = Script::with_beats(
        vec![Speaker {
            id: host.clone(),
            name: "Ada".into(),
            role: "host".into(),
        }],
        vec![
            Turn {
                speaker: host.clone(),
                text: format!("One witness said: \"{}\"", quote.text()),
                emotion: Emotion::Serious,
                citations: vec![claim.id().clone()],
                quotes: vec![quote],
                pace: Pace::Normal,
                nonverbal: vec![],
                callback_to: None,
            },
            Turn {
                speaker: host.clone(),
                text: "Think about that.".into(),
                emotion: Emotion::Somber,
                citations: vec![],
                quotes: vec![],
                pace: Pace::LongPause,
                nonverbal: vec![Nonverbal {
                    kind: NonverbalKind::Backchannel { text: "Hm.".into() },
                    by: host,
                    at: NonverbalAt::Before,
                }],
                callback_to: Some(0),
            },
        ],
        vec![
            Beat {
                kind: BeatKind::QuoteReading,
                turns: TurnRange::new(0, 1).unwrap(),
            },
            Beat {
                kind: BeatKind::Transition,
                turns: TurnRange::new(1, 2).unwrap(),
            },
        ],
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
    roundtrip(&Envelope::new(ArtifactKind::Verdicts, verdicts));
    roundtrip(&Envelope::new(ArtifactKind::Script, script));
    roundtrip(&Envelope::new(ArtifactKind::Analysis, report));

    let manifest = AudioManifest {
        sample_rate: 48_000,
        chunks: vec![ChunkRecord {
            id: ContentHash::of_parts(&[b"chunk"]),
            turns: TurnRange::new(0, 3).unwrap(),
            blob: ContentHash::of_parts(&[b"pcm"]),
            seed: u64::MAX,
            take: 1,
            wer_pm: PerMille::new(42).unwrap(),
            quote_misses: vec!["We saw a fireball".into()],
            verified: false,
        }],
        episode: EpisodeAudio {
            path: "episode.wav".into(),
            duration_ms: 600_000,
            integrated_lufs: -16.02,
            true_peak_dbtp: -1.4,
            encoded: Some("episode.opus".into()),
        },
        voices: vec![VoiceCredit {
            speaker: SpeakerId("host".into()),
            reference: "voices/host.wav".into(),
            licence: "CC0-1.0".into(),
        }],
    };
    roundtrip(&Envelope::new(ArtifactKind::Audio, manifest));
}

const AUDIO: &str = r#"
[tts]
kind = "sidecar"
sidecar = "qwen3-tts"

[asr]
kind = "whisper"
model_dir = "models/whisper-base.en"

[[cast]]
id = "host"
name = "Ada"
role = "host"
voice = { reference = "voices/host.wav", transcript = "Welcome back.", licence = "CC0-1.0" }
"#;

#[test]
fn episode_without_audio_sections_is_unchanged() {
    let spec: EpisodeSpec = toml::from_str(EPISODE).unwrap();
    assert!(spec.cast.is_empty() && spec.tts.is_none() && spec.asr.is_none());
    // Absent sections are not written back, so the episode artifact of an
    // audio-free run keeps its old shape.
    let json = serde_json::to_value(&spec).unwrap();
    for key in ["cast", "tts", "asr"] {
        assert!(json.get(key).is_none(), "{key} serialised: {json}");
    }
}

#[test]
fn audio_sections_parse_with_defaults_and_roundtrip() {
    let spec: EpisodeSpec = toml::from_str(&format!("{EPISODE}{AUDIO}")).unwrap();
    assert_eq!(
        spec.tts,
        Some(TtsConfig::Sidecar {
            sidecar: "qwen3-tts".into(),
            takes: 2,
            max_retries: 2,
            pronounce: Lexicon::default(),
        })
    );
    assert_eq!(
        spec.asr,
        Some(AsrConfig::Whisper {
            model_dir: "models/whisper-base.en".into(),
            max_wer_pm: PerMille::new(80).unwrap(),
        })
    );
    let voice = &spec.cast[0].voice;
    assert_eq!(
        (voice.reference(), voice.licence()),
        (std::path::Path::new("voices/host.wav"), "CC0-1.0")
    );
    roundtrip(&spec);
}

#[test]
fn a_voice_needs_a_licence_and_a_transcript() {
    assert_eq!(
        VoiceRef::new("v.wav", "Hi.", "  "),
        Err(VoiceRefError::MissingLicence("v.wav".into()))
    );
    assert_eq!(
        VoiceRef::new("v.wav", "", "CC0-1.0"),
        Err(VoiceRefError::MissingTranscript("v.wav".into()))
    );

    let no_licence = AUDIO.replace("licence = \"CC0-1.0\"", "licence = \"\"");
    let err = toml::from_str::<EpisodeSpec>(&format!("{EPISODE}{no_licence}"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("no licence"), "{err}");

    let stray = AUDIO.replace("licence = \"CC0-1.0\"", "licence = \"CC0-1.0\", gain = 2");
    let err = toml::from_str::<EpisodeSpec>(&format!("{EPISODE}{stray}"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("gain"), "{err}");
}

#[test]
fn a_voice_clip_must_be_cc0_or_cc_by() {
    for allowed in podling_types::episode::VOICE_LICENCES {
        assert!(VoiceRef::new("v.wav", "Hi.", allowed).is_ok(), "{allowed}");
    }
    for refused in ["CC-BY-NC-4.0", "CC-BY-SA-4.0", "proprietary", "cc0"] {
        assert_eq!(
            VoiceRef::new("v.wav", "Hi.", refused),
            Err(VoiceRefError::LicenceNotAllowed {
                reference: "v.wav".into(),
                licence: refused.into(),
            }),
        );
    }

    let nc = AUDIO.replace("licence = \"CC0-1.0\"", "licence = \"CC-BY-NC-4.0\"");
    let err = toml::from_str::<EpisodeSpec>(&format!("{EPISODE}{nc}"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("\"CC-BY-NC-4.0\"")
            && err.contains("CC-BY-4.0")
            && err.contains("not allowed"),
        "{err}"
    );
}

#[test]
fn the_only_licence_beyond_cc_is_the_generated_one() {
    assert_eq!(
        podling_types::episode::VOICE_LICENCES,
        [
            "CC0-1.0",
            "CC-BY-3.0",
            "CC-BY-4.0",
            "LicenseRef-Podling-Generated"
        ]
    );
    assert_eq!(GENERATED_VOICE_LICENCE, "LicenseRef-Podling-Generated");
    let generated = AUDIO.replace(
        "licence = \"CC0-1.0\"",
        "licence = \"LicenseRef-Podling-Generated\"",
    );
    let spec: EpisodeSpec = toml::from_str(&format!("{EPISODE}{generated}")).unwrap();
    assert_eq!(spec.cast[0].voice.licence(), GENERATED_VOICE_LICENCE);
}

/// `AUDIO` with `[tts.pronounce]` holding `entries`.
fn with_pronounce(entries: &str) -> String {
    format!("{EPISODE}{AUDIO}\n[tts.pronounce]\n{entries}\n")
}

#[test]
fn pronounce_takes_a_respelling_or_a_table_with_heard_variants() {
    let spec: EpisodeSpec = toml::from_str(&with_pronounce(
        "Kulik = \"Koolick\"\n\"Le Mans\" = { say = \"Luh Mon\", heard = [\"lemon\", \"le mon\"] }",
    ))
    .unwrap();
    let Some(TtsConfig::Sidecar { pronounce, .. }) = &spec.tts else {
        panic!("expected a sidecar: {:?}", spec.tts);
    };
    assert_eq!(pronounce.len(), 2);
    let kulik = pronounce.get("Kulik").unwrap();
    assert_eq!((kulik.say(), kulik.heard()), ("Koolick", &[][..]));
    let le_mans = pronounce.get("Le Mans").unwrap();
    assert_eq!(le_mans.say(), "Luh Mon");
    assert_eq!(le_mans.heard(), ["lemon", "le mon"]);
    roundtrip(&spec);

    // A table without `heard` is the respelling only.
    let spec: EpisodeSpec =
        toml::from_str(&with_pronounce("Kulik = { say = \"Koolick\" }")).unwrap();
    let Some(TtsConfig::Sidecar { pronounce, .. }) = &spec.tts else {
        panic!("expected a sidecar");
    };
    assert!(pronounce.get("Kulik").unwrap().heard().is_empty());
}

#[test]
fn an_empty_pronounce_is_not_written_back() {
    let spec: EpisodeSpec = toml::from_str(&format!("{EPISODE}{AUDIO}")).unwrap();
    let json = serde_json::to_value(&spec).unwrap();
    assert!(json["tts"].get("pronounce").is_none(), "{json}");
}

#[test]
fn a_pronounce_entry_with_nothing_in_it_is_refused() {
    for (entries, wanted) in [
        ("\"\" = \"Koolick\"", "empty name"),
        ("\" \" = \"Koolick\"", "empty name"),
        ("Kulik = \"\"", "empty `say`"),
        ("Kulik = { say = \"  \" }", "empty `say`"),
        (
            "Kulik = { say = \"Koolick\", heard = [\"\"] }",
            "empty `heard`",
        ),
    ] {
        let err = toml::from_str::<EpisodeSpec>(&with_pronounce(entries))
            .unwrap_err()
            .to_string();
        assert!(err.contains(wanted), "{entries}: {err}");
    }
    // A table form takes `say` and `heard` only.
    assert!(
        toml::from_str::<EpisodeSpec>(&with_pronounce("Kulik = { say = \"Koolick\", stress = 1 }"))
            .is_err()
    );
}

#[test]
fn a_later_lexicon_wins_per_name() {
    let say = |s: &str| Pronunciation::new(s, Vec::new()).unwrap();
    let user = Lexicon::new(
        [
            ("Kulik".into(), say("Koolick")),
            ("Vanavara".into(), say("Vanavahra")),
        ]
        .into(),
    )
    .unwrap();
    let episode = Lexicon::new([("Kulik".into(), say("Kooleek"))].into()).unwrap();
    let merged = user.overlaid(&episode);
    assert_eq!(merged.get("Kulik").unwrap().say(), "Kooleek");
    assert_eq!(merged.get("Vanavara").unwrap().say(), "Vanavahra");
    assert_eq!(
        Lexicon::new([(" ".into(), say("x"))].into()),
        Err(LexiconError::EmptyName)
    );
}

const PROVENANCE: &str = r#"{
  "model": "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign",
  "weights_commit": "0123abcd",
  "design_prompt": "A warm, lively woman in her thirties.",
  "seed": 1234,
  "tool_version": "0.1.0"
}"#;

#[test]
fn voice_provenance_needs_every_field() {
    let provenance: VoiceProvenance = serde_json::from_str(PROVENANCE).unwrap();
    assert_eq!(provenance.model(), "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign");
    assert_eq!(provenance.weights_commit(), "0123abcd");
    assert_eq!(
        provenance.design_prompt(),
        "A warm, lively woman in her thirties."
    );
    assert_eq!(provenance.seed(), 1234);
    assert_eq!(provenance.tool_version(), "0.1.0");

    let missing = PROVENANCE.replace("  \"seed\": 1234,\n", "");
    assert!(serde_json::from_str::<VoiceProvenance>(&missing).is_err());
    let unknown = PROVENANCE.replace("\"seed\"", "\"gain\": 2, \"seed\"");
    let err = serde_json::from_str::<VoiceProvenance>(&unknown).unwrap_err();
    assert!(err.to_string().contains("gain"), "{err}");
    let empty = PROVENANCE.replace("0123abcd", " ");
    let err = serde_json::from_str::<VoiceProvenance>(&empty).unwrap_err();
    assert!(err.to_string().contains("weights_commit"), "{err}");
}

#[test]
fn provenance_sits_beside_the_clip_under_its_whole_name() {
    assert_eq!(
        provenance_path(std::path::Path::new("voices/host.wav")),
        std::path::Path::new("voices/host.wav.provenance.json")
    );
}

#[test]
fn max_wer_is_a_per_mille_score() {
    let over = AUDIO.replace(
        "model_dir = \"models/whisper-base.en\"",
        "model_dir = \"m\"\nmax_wer_pm = 1001",
    );
    assert!(toml::from_str::<EpisodeSpec>(&format!("{EPISODE}{over}")).is_err());
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

#[test]
fn open_ai_compat_episode_parses_and_roundtrips() {
    let full = EPISODE.replace(
        "kind = \"fake\"",
        "kind = \"open_ai_compat\"\nbase_url = \"http://localhost:11434/v1\"\nmodel = \"llama3.1:8b\"\napi_key_env = \"OPENAI_API_KEY\"\ntemperature = 0.5\ntimeout_secs = 120\nmax_output_tokens = 2048\nunload_after = true",
    );
    let spec: EpisodeSpec = toml::from_str(&full).unwrap();
    assert_eq!(
        spec.llm,
        LlmConfig::OpenAiCompat {
            base_url: "http://localhost:11434/v1".into(),
            model: "llama3.1:8b".into(),
            api_key_env: Some("OPENAI_API_KEY".into()),
            temperature: Some(0.5),
            timeout_secs: Some(120),
            max_output_tokens: Some(2048),
            unload_after: true,
        }
    );
    roundtrip(&spec);

    // Optional fields may be left out entirely (a local server needs no key).
    let minimal = EPISODE.replace(
        "kind = \"fake\"",
        "kind = \"open_ai_compat\"\nbase_url = \"http://localhost:11434/v1\"\nmodel = \"m\"",
    );
    let spec: EpisodeSpec = toml::from_str(&minimal).unwrap();
    assert!(matches!(
        spec.llm,
        LlmConfig::OpenAiCompat {
            api_key_env: None,
            unload_after: false,
            ..
        }
    ));
}

#[test]
fn open_ai_compat_rejects_a_key_value_in_the_episode() {
    for field in ["api_key", "token"] {
        let src = EPISODE.replace(
            "kind = \"fake\"",
            &format!(
                "kind = \"open_ai_compat\"\nbase_url = \"http://x/v1\"\nmodel = \"m\"\n{field} = \"sk-123\""
            ),
        );
        let err = toml::from_str::<EpisodeSpec>(&src).unwrap_err().to_string();
        assert!(err.contains(field), "{err}");
    }
}

const GROUNDING: &str = r#"
[embedding]
kind = "open_ai_compat"
base_url = "http://localhost:11434/v1"
model = "nomic-embed-text"

[nli]
kind = "cross_encoder"
model_dir = "models/nli-deberta-v3-base"
"#;

#[test]
fn embedding_and_nli_sections_parse_and_roundtrip() {
    let spec: EpisodeSpec = toml::from_str(&format!("{EPISODE}{GROUNDING}")).unwrap();
    assert_eq!(
        spec.embedding,
        Some(EmbeddingConfig::OpenAiCompat {
            base_url: "http://localhost:11434/v1".into(),
            model: "nomic-embed-text".into(),
            api_key_env: None,
            timeout_secs: None,
            unload_after: false,
        })
    );
    assert_eq!(
        spec.nli,
        Some(NliConfig::CrossEncoder {
            model_dir: "models/nli-deberta-v3-base".into()
        })
    );
    roundtrip(&spec);

    let fakes = "\n[embedding]\nkind = \"fake\"\n\n[nli]\nkind = \"fake\"\n";
    let spec: EpisodeSpec = toml::from_str(&format!("{EPISODE}{fakes}")).unwrap();
    assert_eq!(spec.embedding, Some(EmbeddingConfig::Fake {}));
    assert_eq!(spec.nli, Some(NliConfig::Fake {}));

    // Both are optional, and an episode without them serialises without them.
    let plain: EpisodeSpec = toml::from_str(EPISODE).unwrap();
    assert_eq!((plain.embedding.as_ref(), plain.nli.as_ref()), (None, None));
    let json = serde_json::to_value(&plain).unwrap();
    assert!(json.get("embedding").is_none() && json.get("nli").is_none());
}

#[test]
fn embedding_and_nli_sections_reject_unknown_keys() {
    for (from, to, key) in [
        (
            "model = \"nomic-embed-text\"",
            "model = \"m\"\napi_key = \"sk-1\"",
            "api_key",
        ),
        ("model_dir = ", "device = \"cuda\"\nmodel_dir = ", "device"),
    ] {
        let src = format!("{EPISODE}{}", GROUNDING.replace(from, to));
        let err = toml::from_str::<EpisodeSpec>(&src).unwrap_err().to_string();
        assert!(err.contains(key), "{err}");
    }
}
