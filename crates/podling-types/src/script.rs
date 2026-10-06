//! The episode script: who says what, how, and on whose authority.

use std::borrow::Cow;
use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::audio::TurnRange;
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

/// The gap before a turn, for the assembler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pace {
    Quick,
    #[default]
    Normal,
    /// A deliberate pause, longer than normal.
    Beat,
    LongPause,
    /// Cuts in: starts before the previous turn has quite finished.
    Interrupt,
}

impl Pace {
    /// For `skip_serializing_if`: the default pace is left out of the JSON.
    pub fn is_normal(&self) -> bool {
        *self == Pace::Normal
    }
}

/// A sound that is not words: a laugh, or another speaker's "mm-hm".
///
/// `kind` is flattened into this object, so it reads
/// `{ "kind": "backchannel", "text": "mm-hm", "by": "guest", "at": "over" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Nonverbal {
    #[serde(flatten)]
    pub kind: NonverbalKind,
    /// Who makes the sound; must be in the cast.
    pub by: SpeakerId,
    pub at: NonverbalAt,
}

/// Internally tagged (`"kind": "laugh"`), so every variant is a struct
/// variant: serde can't put a tag inside a tuple variant's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NonverbalKind {
    Laugh {},
    Chuckle {},
    Sigh {},
    /// A short listener response, e.g. "mm-hm" or "right".
    Backchannel {
        text: String,
    },
}

/// Where a nonverbal sound goes relative to its turn's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NonverbalAt {
    Before,
    After,
    /// On a second track, while the turn is spoken.
    Over,
}

/// What a run of turns is doing. The chunk planner never splits a beat
/// across two chunks, so a joke and its answer are synthesised together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BeatKind {
    Narration,
    /// Quick back-and-forth between the hosts. Adds no new facts.
    Banter,
    QuoteReading,
    Transition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Beat {
    pub kind: BeatKind,
    pub turns: TurnRange,
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
    /// The gap before this turn. Left out of the JSON when normal, so a
    /// script written without audio in mind keeps its old shape.
    #[serde(default, skip_serializing_if = "Pace::is_normal")]
    pub pace: Pace,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nonverbal: Vec<Nonverbal>,
    /// An earlier turn (by index) this one refers back to; its audio is given
    /// to the TTS as context so the callback sounds like one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_to: Option<usize>,
}

/// A validated script: the cast is non-empty with unique ids, every turn is
/// spoken by a cast member, and the beats (if any) cover the turns in order.
/// Deserialisation runs the same checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawScript")]
pub struct Script {
    cast: Vec<Speaker>,
    turns: Vec<Turn>,
    /// Empty for a script written without beats; see [`Script::beats`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    beats: Vec<Beat>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptError {
    #[error("the cast is empty")]
    EmptyCast,
    #[error("speaker {0:?} appears twice in the cast")]
    DuplicateSpeaker(String),
    #[error("turn {turn} is spoken by {speaker:?}, who is not in the cast")]
    UnknownSpeaker { turn: usize, speaker: String },
    #[error("beat {beat} starts at turn {start}, so turns {expected}..{start} are in no beat")]
    BeatGap {
        beat: usize,
        expected: usize,
        start: usize,
    },
    #[error(
        "beat {beat} starts at turn {start}, inside the beat before it, which ends at turn {expected}"
    )]
    BeatOverlap {
        beat: usize,
        expected: usize,
        start: usize,
    },
    #[error("the beats end at turn {end}, but the script has {turns} turns")]
    BeatsEnd { end: usize, turns: usize },
    #[error("turn {turn} calls back to turn {to}, which is not an earlier turn")]
    ForwardCallback { turn: usize, to: usize },
    #[error("turn {turn} has a nonverbal sound by {speaker:?}, who is not in the cast")]
    UnknownNonverbalSpeaker { turn: usize, speaker: String },
}

impl Script {
    /// A script without beats: [`Script::beats`] treats each turn as its own.
    pub fn new(cast: Vec<Speaker>, turns: Vec<Turn>) -> Result<Self, ScriptError> {
        Self::with_beats(cast, turns, Vec::new())
    }

    /// `beats` must be empty, or run contiguously from turn 0 to the last turn.
    pub fn with_beats(
        cast: Vec<Speaker>,
        turns: Vec<Turn>,
        beats: Vec<Beat>,
    ) -> Result<Self, ScriptError> {
        if cast.is_empty() {
            return Err(ScriptError::EmptyCast);
        }
        let mut ids = BTreeSet::new();
        for speaker in &cast {
            if !ids.insert(&speaker.id) {
                return Err(ScriptError::DuplicateSpeaker(speaker.id.0.clone()));
            }
        }
        for (turn, t) in turns.iter().enumerate() {
            if !ids.contains(&t.speaker) {
                return Err(ScriptError::UnknownSpeaker {
                    turn,
                    speaker: t.speaker.0.clone(),
                });
            }
            if let Some(to) = t.callback_to.filter(|&to| to >= turn) {
                return Err(ScriptError::ForwardCallback { turn, to });
            }
            if let Some(sound) = t.nonverbal.iter().find(|n| !ids.contains(&n.by)) {
                return Err(ScriptError::UnknownNonverbalSpeaker {
                    turn,
                    speaker: sound.by.0.clone(),
                });
            }
        }
        check_beats(&beats, turns.len())?;
        Ok(Self { cast, turns, beats })
    }

    pub fn cast(&self) -> &[Speaker] {
        &self.cast
    }

    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    /// The script's beats, in order, covering every turn. A script written
    /// without beats gets one narration beat per turn.
    ///
    /// `Cow` ("clone on write") is either a borrow of the stored beats or a
    /// freshly built `Vec`; the caller reads both through the same `&[Beat]`.
    pub fn beats(&self) -> Cow<'_, [Beat]> {
        if !self.beats.is_empty() {
            return Cow::Borrowed(&self.beats);
        }
        let implied = (0..self.turns.len())
            .map(|i| Beat {
                kind: BeatKind::Narration,
                turns: TurnRange::new(i, i + 1).expect("i < i + 1"),
            })
            .collect();
        Cow::Owned(implied)
    }
}

/// Beats run back to back from turn 0, and the last one ends at the last turn.
fn check_beats(beats: &[Beat], turns: usize) -> Result<(), ScriptError> {
    if beats.is_empty() {
        return Ok(());
    }
    let mut expected = 0;
    for (beat, b) in beats.iter().enumerate() {
        let start = b.turns.start();
        if start > expected {
            return Err(ScriptError::BeatGap {
                beat,
                expected,
                start,
            });
        }
        if start < expected {
            return Err(ScriptError::BeatOverlap {
                beat,
                expected,
                start,
            });
        }
        expected = b.turns.end();
    }
    if expected != turns {
        return Err(ScriptError::BeatsEnd {
            end: expected,
            turns,
        });
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct RawScript {
    cast: Vec<Speaker>,
    turns: Vec<Turn>,
    #[serde(default)]
    beats: Vec<Beat>,
}

impl TryFrom<RawScript> for Script {
    type Error = ScriptError;

    fn try_from(raw: RawScript) -> Result<Self, Self::Error> {
        Self::with_beats(raw.cast, raw.turns, raw.beats)
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
            pace: Pace::Normal,
            nonverbal: vec![],
            callback_to: None,
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

    fn beat(kind: BeatKind, start: usize, end: usize) -> Beat {
        Beat {
            kind,
            turns: TurnRange::new(start, end).unwrap(),
        }
    }

    /// Four turns by `a`, with `beats`.
    fn four_turns(beats: Vec<Beat>) -> Result<Script, ScriptError> {
        Script::with_beats(vec![speaker("a")], vec![turn("a"); 4], beats)
    }

    #[test]
    fn beats_must_cover_every_turn_in_order() {
        use BeatKind::{Banter, Narration};
        let ok = four_turns(vec![beat(Narration, 0, 1), beat(Banter, 1, 4)]).unwrap();
        assert_eq!(ok.beats()[1], beat(Banter, 1, 4));

        assert_eq!(
            four_turns(vec![beat(Narration, 0, 1), beat(Banter, 2, 4)]),
            Err(ScriptError::BeatGap {
                beat: 1,
                expected: 1,
                start: 2
            })
        );
        assert_eq!(
            four_turns(vec![beat(Narration, 1, 4)]),
            Err(ScriptError::BeatGap {
                beat: 0,
                expected: 0,
                start: 1
            })
        );
        assert_eq!(
            four_turns(vec![beat(Narration, 0, 2), beat(Banter, 1, 4)]),
            Err(ScriptError::BeatOverlap {
                beat: 1,
                expected: 2,
                start: 1
            })
        );
        assert_eq!(
            four_turns(vec![beat(Narration, 0, 3)]),
            Err(ScriptError::BeatsEnd { end: 3, turns: 4 })
        );
        assert_eq!(
            four_turns(vec![beat(Narration, 0, 5)]),
            Err(ScriptError::BeatsEnd { end: 5, turns: 4 })
        );
    }

    #[test]
    fn a_callback_must_point_back() {
        let mut turns = vec![turn("a"); 3];
        turns[2].callback_to = Some(0);
        assert!(Script::new(vec![speaker("a")], turns.clone()).is_ok());
        turns[1].callback_to = Some(1);
        assert_eq!(
            Script::new(vec![speaker("a")], turns.clone()),
            Err(ScriptError::ForwardCallback { turn: 1, to: 1 })
        );
        turns[1].callback_to = Some(2);
        assert_eq!(
            Script::new(vec![speaker("a")], turns),
            Err(ScriptError::ForwardCallback { turn: 1, to: 2 })
        );
    }

    #[test]
    fn a_nonverbal_sound_is_made_by_a_cast_member() {
        let mut turns = vec![turn("a"), turn("b")];
        turns[1].nonverbal = vec![Nonverbal {
            kind: NonverbalKind::Laugh {},
            by: SpeakerId("c".into()),
            at: NonverbalAt::After,
        }];
        assert_eq!(
            Script::new(vec![speaker("a"), speaker("b")], turns.clone()),
            Err(ScriptError::UnknownNonverbalSpeaker {
                turn: 1,
                speaker: "c".into()
            })
        );
        turns[1].nonverbal[0].by = SpeakerId("a".into());
        assert!(Script::new(vec![speaker("a"), speaker("b")], turns).is_ok());
    }

    /// A script stored before beats existed: no `beats`, `pace`, `nonverbal`
    /// or `callback_to`, as every `Turn` field the old code wrote.
    const OLD_SCRIPT: &str = r#"{
        "cast": [{ "id": "a", "name": "A", "role": "host" }],
        "turns": [
            { "speaker": "a", "text": "One.", "emotion": "neutral", "citations": [], "quotes": [] },
            { "speaker": "a", "text": "Two.", "emotion": "amused", "citations": [], "quotes": [] }
        ]
    }"#;

    #[test]
    fn an_old_script_parses_with_one_beat_per_turn_and_is_written_unchanged() {
        let script: Script = serde_json::from_str(OLD_SCRIPT).unwrap();
        assert_eq!(
            script.beats().as_ref(),
            [
                beat(BeatKind::Narration, 0, 1),
                beat(BeatKind::Narration, 1, 2)
            ]
        );
        assert_eq!(script.turns()[1].pace, Pace::Normal);
        let written = serde_json::to_value(&script).unwrap();
        let old: serde_json::Value = serde_json::from_str(OLD_SCRIPT).unwrap();
        assert_eq!(written, old);
    }

    #[test]
    fn the_new_fields_roundtrip() {
        let json = serde_json::json!({
            "cast": [{ "id": "a", "name": "A", "role": "host" },
                     { "id": "b", "name": "B", "role": "guest" }],
            "turns": [
                { "speaker": "a", "text": "One.", "emotion": "neutral", "citations": [], "quotes": [] },
                { "speaker": "b", "text": "Two.", "emotion": "amused", "citations": [], "quotes": [],
                  "pace": "interrupt", "callback_to": 0,
                  "nonverbal": [
                      { "kind": "backchannel", "text": "mm-hm", "by": "a", "at": "over" },
                      { "kind": "laugh", "by": "b", "at": "before" }
                  ] }
            ],
            "beats": [
                { "kind": "narration", "turns": { "start": 0, "end": 1 } },
                { "kind": "banter", "turns": { "start": 1, "end": 2 } }
            ]
        });
        let script: Script = serde_json::from_value(json.clone()).unwrap();
        let second = &script.turns()[1];
        assert_eq!(second.pace, Pace::Interrupt);
        assert_eq!(
            second.nonverbal[0].kind,
            NonverbalKind::Backchannel {
                text: "mm-hm".into()
            }
        );
        assert_eq!(serde_json::to_value(&script).unwrap(), json);

        // Beats that break the rules fail deserialisation too.
        let mut bad = json;
        bad["beats"][1]["turns"]["start"] = 0.into();
        assert!(serde_json::from_value::<Script>(bad).is_err());
    }
}
