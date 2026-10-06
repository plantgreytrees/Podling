//! Plans how the script is cut into chunks for the TTS.
//!
//! A dialogue model speaks an exchange best when it hears all of it at once,
//! so whole beats are packed into chunks of about a minute, never past the
//! backend's `max_chunk_secs`. A beat too long for one chunk is cut between
//! its turns, and a turn too long for one chunk is cut at a sentence end
//! (never inside a quote). A per-turn model gets one turn per chunk.
//!
//! Lengths are estimated from word counts at a fixed speaking rate; the
//! planner runs before any audio exists.

use std::collections::BTreeSet;
use std::ops::Range;

use podling_types::{Script, SpeakerId, Turn, TurnRange};

use crate::plugin::TtsCapabilities;
use crate::text::sentences;

/// Speaking rate used to estimate how long text takes to say.
pub const WORDS_PER_MINUTE: f64 = 150.0;
/// A chunk is closed once it is estimated to be at least this long: long
/// enough for the model to settle into the exchange, short enough that an
/// edit re-synthesises little.
pub const TARGET_SECS: f64 = 60.0;

/// The part of one turn a chunk speaks: the whole turn, or, for a turn too
/// long for one chunk, a run of its sentences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub turn: usize,
    /// The beat the turn is in, an index into [`Script::beats`].
    pub beat: usize,
    /// Byte range of the spoken text in the turn's text.
    pub text: Range<usize>,
}

impl Piece {
    /// The text this piece speaks.
    pub fn text<'s>(&self, script: &'s Script) -> &'s str {
        &script.turns()[self.turn].text[self.text.clone()]
    }

    /// Whether this is where the turn begins, so a sound made before the
    /// turn's words goes here.
    pub fn starts_turn(&self) -> bool {
        self.text.start == 0
    }

    /// Whether this is where the turn ends.
    pub fn ends_turn(&self, script: &Script) -> bool {
        self.text.end == script.turns()[self.turn].text.len()
    }
}

/// One chunk: the pieces it speaks, in order. Never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedChunk {
    pub pieces: Vec<Piece>,
}

impl PlannedChunk {
    /// The turns this chunk speaks some or all of.
    pub fn turns(&self) -> TurnRange {
        let first = self.pieces.first().expect("a chunk is never empty").turn;
        let last = self.pieces.last().expect("a chunk is never empty").turn;
        TurnRange::new(first, last + 1).expect("pieces are in turn order")
    }
}

/// Estimated seconds to say `text`.
pub fn estimate_secs(text: &str) -> f64 {
    text.split_whitespace().count() as f64 * 60.0 / WORDS_PER_MINUTE
}

/// Cuts `script` into chunks for a backend that can do `capabilities`.
///
/// Every byte of speech is in exactly one chunk, in script order. A chunk
/// begins only at a beat boundary, unless its beat is too long for one chunk.
pub fn plan_chunks(script: &Script, capabilities: &TtsCapabilities) -> Vec<PlannedChunk> {
    let max_secs = f64::from(capabilities.max_chunk_secs);
    let mut packer = Packer {
        max_secs,
        max_speakers: usize::from(capabilities.max_speakers),
        chunks: Vec::new(),
        current: Vec::new(),
        secs: 0.0,
        speakers: BTreeSet::new(),
    };
    for (b, beat) in script.beats().iter().enumerate() {
        let pieces: Vec<(Piece, f64)> = beat
            .turns
            .indices()
            .flat_map(|t| split_turn(&script.turns()[t], t, b, max_secs))
            .map(|piece| {
                let secs = estimate_secs(piece.text(script));
                (piece, secs)
            })
            .collect();
        if !capabilities.multi_speaker {
            for (piece, secs) in pieces {
                packer.push(piece, secs, script);
                packer.close();
            }
            continue;
        }
        if !packer.fits(&pieces, script) {
            packer.close();
        }
        if packer.fits(&pieces, script) {
            for (piece, secs) in pieces {
                packer.push(piece, secs, script);
            }
        } else {
            // Too long (or too many voices) for one chunk even on its own:
            // cut between turns, about a minute at a time.
            for (piece, secs) in pieces {
                if !packer.fits(std::slice::from_ref(&(piece.clone(), secs)), script) {
                    packer.close();
                }
                packer.push(piece, secs, script);
                if packer.secs >= TARGET_SECS {
                    packer.close();
                }
            }
        }
        if packer.secs >= TARGET_SECS {
            packer.close();
        }
    }
    packer.close();
    packer.chunks
}

struct Packer {
    max_secs: f64,
    max_speakers: usize,
    chunks: Vec<PlannedChunk>,
    current: Vec<Piece>,
    secs: f64,
    speakers: BTreeSet<SpeakerId>,
}

impl Packer {
    /// Whether `pieces` can join the current chunk. An empty chunk takes
    /// anything, so a single sentence longer than the limit still gets said.
    fn fits(&self, pieces: &[(Piece, f64)], script: &Script) -> bool {
        if self.current.is_empty() && pieces.len() <= 1 {
            return true;
        }
        let secs: f64 = pieces.iter().map(|(_, s)| s).sum();
        let mut speakers = self.speakers.clone();
        speakers.extend(pieces.iter().map(|(p, _)| speaker(script, p).clone()));
        self.secs + secs <= self.max_secs && speakers.len() <= self.max_speakers
    }

    fn push(&mut self, piece: Piece, secs: f64, script: &Script) {
        self.speakers.insert(speaker(script, &piece).clone());
        self.secs += secs;
        self.current.push(piece);
    }

    fn close(&mut self) {
        if !self.current.is_empty() {
            self.chunks.push(PlannedChunk {
                pieces: std::mem::take(&mut self.current),
            });
        }
        self.secs = 0.0;
        self.speakers.clear();
    }
}

fn speaker<'s>(script: &'s Script, piece: &Piece) -> &'s SpeakerId {
    &script.turns()[piece.turn].speaker
}

/// `turn` as one piece, or, when it is longer than `max_secs`, as runs of
/// whole sentences of about equal length, none past `max_secs` unless a
/// single sentence is. A cut never falls inside one of the turn's quotes.
fn split_turn(turn: &Turn, index: usize, beat: usize, max_secs: f64) -> Vec<Piece> {
    let piece = |text: Range<usize>| Piece {
        turn: index,
        beat,
        text,
    };
    let total = estimate_secs(&turn.text);
    if total <= max_secs {
        return vec![piece(0..turn.text.len())];
    }
    let quotes: Vec<Range<usize>> = turn
        .quotes
        .iter()
        .flat_map(|q| {
            turn.text
                .match_indices(q.text())
                .map(|(at, s)| at..at + s.len())
        })
        .collect();
    let inside_quote = |at: usize| quotes.iter().any(|q| q.start < at && at < q.end);

    let parts = (total / max_secs).ceil();
    let target = total / parts;
    let sentences = sentences(&turn.text);
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut secs = 0.0;
    for (i, sentence) in sentences.iter().enumerate() {
        secs += estimate_secs(&turn.text[sentence.clone()]);
        let Some(next) = sentences.get(i + 1) else {
            break;
        };
        let next_secs = estimate_secs(&turn.text[next.clone()]);
        let cut = secs >= target || secs + next_secs > max_secs;
        if cut && !inside_quote(sentence.end) {
            pieces.push(piece(start..sentence.end));
            start = next.start;
            secs = 0.0;
        }
    }
    pieces.push(piece(start..turn.text.len()));
    pieces
}

#[cfg(test)]
mod tests {
    use podling_types::{
        Beat, BeatKind, Document, Emotion, Pace, Quote, SourceRef, Speaker, TextSpan, Turn,
    };

    use super::*;

    /// `n` distinct words of filler, `w0 w1 …`, ten to a sentence.
    fn words(n: usize) -> String {
        let mut text = String::new();
        for i in 0..n {
            if i > 0 {
                text.push(' ');
            }
            text.push_str(&format!("w{i}"));
            if i % 10 == 9 || i + 1 == n {
                text.push('.');
            }
        }
        text
    }

    fn turn(speaker: &str, text: String) -> Turn {
        Turn {
            speaker: SpeakerId(speaker.into()),
            text,
            emotion: Emotion::Neutral,
            citations: vec![],
            quotes: vec![],
            pace: Pace::Normal,
            nonverbal: vec![],
            callback_to: None,
        }
    }

    /// A script whose beats hold turns of the given word counts, speakers
    /// alternating.
    fn script(beats: &[&[usize]]) -> Script {
        let cast = ["ada", "ben"]
            .map(|id| Speaker {
                id: SpeakerId(id.into()),
                name: id.into(),
                role: "host".into(),
            })
            .to_vec();
        let mut turns = Vec::new();
        let mut spans = Vec::new();
        for beat in beats {
            let start = turns.len();
            for &n in *beat {
                let who = if turns.len() % 2 == 0 { "ada" } else { "ben" };
                turns.push(turn(who, words(n)));
            }
            spans.push(Beat {
                kind: BeatKind::Banter,
                turns: TurnRange::new(start, turns.len()).unwrap(),
            });
        }
        Script::with_beats(cast, turns, spans).unwrap()
    }

    fn dialogue(max_chunk_secs: u32) -> TtsCapabilities {
        TtsCapabilities {
            multi_speaker: true,
            max_chunk_secs,
            max_speakers: 8,
            native_sample_rate: 24_000,
            context: true,
        }
    }

    /// Every byte of every turn's speech is in exactly one piece, in order:
    /// the pieces of a turn are its sentences, cut into runs, and nothing
    /// else.
    fn assert_covers_every_turn_once(script: &Script, chunks: &[PlannedChunk]) {
        let pieces: Vec<&Piece> = chunks.iter().flat_map(|c| &c.pieces).collect();
        for (t, turn) in script.turns().iter().enumerate() {
            let mine: Vec<&&Piece> = pieces.iter().filter(|p| p.turn == t).collect();
            assert!(!mine.is_empty(), "turn {t} is in no chunk");
            assert_eq!(mine.first().unwrap().text.start, 0, "turn {t} starts");
            assert_eq!(mine.last().unwrap().text.end, turn.text.len(), "turn {t}");
            for pair in mine.windows(2) {
                let gap = &turn.text[pair[0].text.end..pair[1].text.start];
                assert!(gap.trim().is_empty(), "turn {t} loses {gap:?}");
            }
        }
        let order: Vec<(usize, usize)> = pieces.iter().map(|p| (p.turn, p.text.start)).collect();
        assert!(order.is_sorted(), "pieces are in script order");
    }

    /// For each chunk boundary: whether it falls between two beats (true)
    /// or inside one (false).
    fn boundaries_between_beats(chunks: &[PlannedChunk]) -> Vec<bool> {
        chunks
            .windows(2)
            .map(|pair| {
                let last = pair[0].pieces.last().unwrap();
                let first = pair[1].pieces.first().unwrap();
                last.beat != first.beat
            })
            .collect()
    }

    #[test]
    fn whole_beats_are_packed_into_chunks_of_about_a_minute() {
        // 150 words a minute: beats of 50, 75, 100, 40 and 60 seconds.
        let script = script(&[&[60, 65], &[90, 97], &[250], &[50, 50], &[150]]);
        let chunks = plan_chunks(&script, &dialogue(120));
        assert_covers_every_turn_once(&script, &chunks);
        let turns: Vec<_> = chunks.iter().map(|c| c.turns().indices()).collect();
        // 50 + 75 > 120, so the first beat is a chunk of its own; 75 and
        // 100 each reach a minute alone; 40 + 60 share the last chunk.
        assert_eq!(turns, [0..2, 2..4, 4..5, 5..8]);
        assert!(boundaries_between_beats(&chunks).iter().all(|&b| b));
        for chunk in &chunks {
            let secs: f64 = chunk
                .pieces
                .iter()
                .map(|p| estimate_secs(p.text(&script)))
                .sum();
            assert!(secs <= 120.0, "{secs}");
        }
    }

    #[test]
    fn a_per_turn_backend_gets_one_turn_per_chunk() {
        let script = script(&[&[10, 10, 10], &[20]]);
        let mut caps = dialogue(120);
        caps.multi_speaker = false;
        let chunks = plan_chunks(&script, &caps);
        assert_covers_every_turn_once(&script, &chunks);
        let turns: Vec<_> = chunks.iter().map(|c| c.turns().indices()).collect();
        assert_eq!(turns, [0..1, 1..2, 2..3, 3..4]);
    }

    #[test]
    fn a_200_second_beat_is_cut_between_turns_and_its_long_turn_at_sentence_ends() {
        // One beat of 30 + 400 + 70 words: 12 s, 160 s and 28 s.
        let script = script(&[&[30, 400, 70], &[20]]);
        let chunks = plan_chunks(&script, &dialogue(120));
        assert_covers_every_turn_once(&script, &chunks);
        for chunk in &chunks {
            let secs: f64 = chunk
                .pieces
                .iter()
                .map(|p| estimate_secs(p.text(&script)))
                .sum();
            assert!(secs <= 120.0, "{secs}");
        }
        // The long turn is cut once, at a sentence end, into two halves.
        let long: Vec<&Piece> = chunks
            .iter()
            .flat_map(|c| &c.pieces)
            .filter(|p| p.turn == 1)
            .collect();
        assert_eq!(long.len(), 2);
        assert!(long[0].text(&script).ends_with('.'));
        let halves: Vec<f64> = long
            .iter()
            .map(|p| estimate_secs(p.text(&script)))
            .collect();
        assert_eq!(halves, [80.0, 80.0]);
        // 12 + 80 reaches a minute; the second half alone does too; the
        // long beat's 28 s tail then shares a chunk with the short next beat.
        // Both boundaries are inside the long beat, and only there.
        let turns: Vec<_> = chunks.iter().map(|c| c.turns().indices()).collect();
        assert_eq!(turns, [0..2, 1..2, 2..4]);
        assert_eq!(boundaries_between_beats(&chunks), [false, false]);
    }

    #[test]
    fn a_cut_never_falls_inside_a_quote() {
        let text = words(400);
        // The halves would meet after word 200; quote words 195..205, so the
        // cut has to move to the next sentence end, after word 210.
        let start = text.find("w195 ").unwrap();
        let end = text.find("w204 ").unwrap() + "w204".len();
        let doc = Document::new(
            SourceRef {
                connector: "t".into(),
                locator: "d".into(),
                independence_group: "d".into(),
            },
            "d",
            &text,
        );
        let quote = Quote::from_document(&doc, TextSpan::new(start, end).unwrap()).unwrap();
        let mut t = turn("ada", text.clone());
        t.quotes.push(quote);
        let ada = Speaker {
            id: SpeakerId("ada".into()),
            name: "Ada".into(),
            role: "host".into(),
        };
        let script = Script::new(vec![ada], vec![t]).unwrap();

        let chunks = plan_chunks(&script, &dialogue(120));
        assert_covers_every_turn_once(&script, &chunks);
        assert_eq!(chunks.len(), 2);
        let cut = chunks[0].pieces[0].text.end;
        assert!(cut >= end, "cut at {cut} splits the quote {start}..{end}");
        assert_eq!(
            chunks[0].pieces[0].text(&script).split_whitespace().count(),
            210
        );
    }

    #[test]
    fn too_many_voices_for_one_chunk_cut_the_beat() {
        let script = script(&[&[10, 10, 10]]);
        let mut caps = dialogue(120);
        caps.max_speakers = 1;
        let chunks = plan_chunks(&script, &caps);
        assert_covers_every_turn_once(&script, &chunks);
        assert_eq!(chunks.len(), 3);
    }
}
