//! The episode script: who says what, how, and on whose authority.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{ClaimId, SpeakerId};
use crate::quote::Quote;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Speaker {
    pub id: SpeakerId,
    pub name: String,
    /// Free-form role, e.g. `host` or `narrator`.
    pub role: String,
}

/// Delivery hint for text-to-speech.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Emotion {
    #[default]
    Neutral,
    Curious,
    Excited,
    Serious,
    Amused,
    Somber,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Turn {
    pub speaker: SpeakerId,
    pub text: String,
    #[serde(default)]
    pub emotion: Emotion,
    /// Ledger claims this turn relies on.
    #[serde(default)]
    pub citations: Vec<ClaimId>,
    /// Verbatim quotes spoken in this turn.
    #[serde(default)]
    pub quotes: Vec<Quote>,
}

/// A validated script: the cast is non-empty with unique ids, and every turn
/// is spoken by a cast member. Deserialisation runs the same checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawScript")]
pub struct Script {
    cast: Vec<Speaker>,
    turns: Vec<Turn>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptError {
    #[error("the cast is empty")]
    EmptyCast,
    #[error("speaker {0:?} appears twice in the cast")]
    DuplicateSpeaker(String),
    #[error("turn {turn} is spoken by {speaker:?}, who is not in the cast")]
    UnknownSpeaker { turn: usize, speaker: String },
}

impl Script {
    pub fn new(cast: Vec<Speaker>, turns: Vec<Turn>) -> Result<Self, ScriptError> {
        if cast.is_empty() {
            return Err(ScriptError::EmptyCast);
        }
        let mut ids = BTreeSet::new();
        for speaker in &cast {
            if !ids.insert(&speaker.id) {
                return Err(ScriptError::DuplicateSpeaker(speaker.id.0.clone()));
            }
        }
        if let Some((turn, t)) = turns
            .iter()
            .enumerate()
            .find(|(_, t)| !ids.contains(&t.speaker))
        {
            return Err(ScriptError::UnknownSpeaker {
                turn,
                speaker: t.speaker.0.clone(),
            });
        }
        Ok(Self { cast, turns })
    }

    pub fn cast(&self) -> &[Speaker] {
        &self.cast
    }

    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }
}

#[derive(Deserialize, JsonSchema)]
struct RawScript {
    cast: Vec<Speaker>,
    turns: Vec<Turn>,
}

impl TryFrom<RawScript> for Script {
    type Error = ScriptError;

    fn try_from(raw: RawScript) -> Result<Self, Self::Error> {
        Self::new(raw.cast, raw.turns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speaker(id: &str) -> Speaker {
        Speaker {
            id: SpeakerId(id.into()),
            name: id.into(),
            role: "host".into(),
        }
    }

    fn turn(speaker: &str) -> Turn {
        Turn {
            speaker: SpeakerId(speaker.into()),
            text: "Hello.".into(),
            emotion: Emotion::Neutral,
            citations: vec![],
            quotes: vec![],
        }
    }

    #[test]
    fn validates_cast_and_speakers() {
        assert_eq!(Script::new(vec![], vec![]), Err(ScriptError::EmptyCast));
        assert_eq!(
            Script::new(vec![speaker("a"), speaker("a")], vec![]),
            Err(ScriptError::DuplicateSpeaker("a".into()))
        );
        assert_eq!(
            Script::new(vec![speaker("a")], vec![turn("a"), turn("b")]),
            Err(ScriptError::UnknownSpeaker {
                turn: 1,
                speaker: "b".into()
            })
        );
        assert!(Script::new(vec![speaker("a"), speaker("b")], vec![turn("a"), turn("b")]).is_ok());
    }

    #[test]
    fn deserialisation_validates() {
        let json = r#"{"cast": [], "turns": []}"#;
        assert!(serde_json::from_str::<Script>(json).is_err());
    }
}
