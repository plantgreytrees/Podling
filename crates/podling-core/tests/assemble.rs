//! The assembler on synthetic chunks: pacing, an interrupt, a sound over a
//! turn, chunks of different loudness, and the finished episode's level.

use std::ops::Range;

use podling_core::audio::Pcm;
use podling_core::stages::assemble::{
    Assembled, PEAK_CEILING_DBTP, SAMPLE_RATE, TARGET_LUFS, assemble, measure,
};
use podling_core::stages::{OverlayClip, Piece, SynthesizedChunk};
use podling_types::{
    ChunkRecord, ContentHash, Emotion, Gaps, Nonverbal, NonverbalAt, NonverbalKind, Pace, PerMille,
    Script, Speaker, SpeakerId, Turn, TurnRange,
};

/// The rate a TTS model typically makes.
const MODEL_RATE: u32 = 24_000;

fn turn(speaker: &str, pace: Pace) -> Turn {
    Turn {
        speaker: SpeakerId(speaker.into()),
        text: "Some words to say.".into(),
        emotion: Emotion::Neutral,
        citations: vec![],
        quotes: vec![],
        pace,
        nonverbal: vec![],
        callback_to: None,
    }
}

fn script(turns: Vec<Turn>) -> Script {
    let cast = ["host", "guest"]
        .into_iter()
        .map(|id| Speaker {
            id: SpeakerId(id.into()),
            name: id.into(),
            role: "host".into(),
        })
        .collect();
    Script::new(cast, turns).unwrap()
}

/// `seconds` of a tone at `hz`, scaled to `amplitude`, at the model's rate.
fn tone(seconds: f32, hz: f32, amplitude: f32) -> Vec<f32> {
    let n = (MODEL_RATE as f32 * seconds) as usize;
    (0..n)
        .map(|i| {
            amplitude * ((i as f32 + 0.5) * hz * std::f32::consts::TAU / MODEL_RATE as f32).sin()
        })
        .collect()
}

/// A chunk speaking whole turns `turns`, each a tone of `seconds` with the
/// model's own half second of silence around it; with spans, or without.
fn chunk(
    script: &Script,
    turns: Range<usize>,
    seconds: f32,
    amplitude: f32,
    spans: bool,
) -> SynthesizedChunk {
    let mut samples = Vec::new();
    let mut turn_spans = Vec::new();
    for k in 0..turns.len() {
        let start = samples.len();
        samples.extend(vec![0.0; MODEL_RATE as usize / 2]);
        samples.extend(tone(seconds, 150.0 + 60.0 * k as f32, amplitude));
        samples.extend(vec![0.0; MODEL_RATE as usize / 2]);
        turn_spans.push(start..samples.len());
    }
    let pieces = turns
        .clone()
        .map(|t| Piece {
            turn: t,
            beat: t,
            text: 0..script.turns()[t].text.len(),
        })
        .collect();
    SynthesizedChunk {
        record: ChunkRecord {
            id: ContentHash::of_parts(&[format!("{turns:?}").as_bytes()]),
            turns: TurnRange::new(turns.start, turns.end).unwrap(),
            blob: ContentHash::of_parts(&[b"blob"]),
            seed: 0,
            take: 0,
            wer_pm: PerMille::new(0).unwrap(),
            quote_misses: vec![],
            verified: true,
        },
        pcm: Pcm::new(MODEL_RATE, samples),
        pieces,
        turn_spans: spans.then_some(turn_spans),
    }
}

/// Runs of silence (under −80 dBFS: resampling leaves a faint residue)
/// at least 50 ms long, in ms, in order.
fn silences_ms(pcm: &Pcm) -> Vec<f64> {
    let mut runs = Vec::new();
    let mut run = 0usize;
    for &s in pcm.samples.iter().chain([1.0].iter()) {
        if s.abs() < 1e-4 {
            run += 1;
        } else {
            if run >= SAMPLE_RATE as usize / 20 {
                runs.push(run as f64 * 1000.0 / f64::from(SAMPLE_RATE));
            }
            run = 0;
        }
    }
    runs
}

fn check_level(episode: &Assembled) {
    let measured = measure(&episode.pcm).unwrap();
    assert!(
        (measured.integrated_lufs - TARGET_LUFS).abs() <= 0.5,
        "{measured:?}"
    );
    assert!(measured.true_peak_dbtp <= PEAK_CEILING_DBTP, "{measured:?}");
    assert_eq!(measured, episode.loudness);
}

#[test]
fn turns_are_paced_trimmed_and_normalised() {
    let script = script(vec![
        turn("host", Pace::Normal),
        turn("guest", Pace::Beat),
        turn("host", Pace::Quick),
        turn("guest", Pace::LongPause),
    ]);
    // One chunk with spans, then one without: it is placed as one block.
    let chunks = [
        chunk(&script, 0..2, 1.0, 0.3, true),
        chunk(&script, 2..4, 1.0, 0.3, false),
    ];
    let episode = assemble(&script, &chunks, &[], &Gaps::default()).unwrap();
    assert_eq!(episode.pcm.rate, SAMPLE_RATE);
    check_level(&episode);

    // The model's silences are gone; the script's are in their place: a
    // beat inside the first chunk, then the quick turn that starts the
    // second. The second chunk's own gap stays inside its block (1 s of
    // model silence between its two turns).
    let silences = silences_ms(&episode.pcm);
    assert_eq!(silences.len(), 3, "{silences:?}");
    for (got, want) in silences.iter().zip([600.0, 120.0, 1000.0]) {
        assert!((got - want).abs() <= 10.0, "{silences:?}");
    }
    let spans = &episode.chunks;
    assert_eq!(spans.len(), 2);
    let (first, second) = (spans[0].clone().unwrap(), spans[1].clone().unwrap());
    assert_eq!(first.start, 0);
    assert!(second.start > first.end);
    assert_eq!(second.end, episode.pcm.len());
}

#[test]
fn an_interrupt_cuts_in_and_nothing_clips() {
    let script = script(vec![
        turn("host", Pace::Normal),
        turn("guest", Pace::Interrupt),
    ]);
    // Loud enough that the overlap would clip without the crossfade and
    // the limiter.
    let chunks = [chunk(&script, 0..2, 1.5, 0.95, true)];
    let episode = assemble(&script, &chunks, &[], &Gaps::default()).unwrap();
    check_level(&episode);
    assert!(episode.pcm.samples.iter().all(|s| s.abs() <= 1.0));
    // No gap at all between the two turns, and 150 ms of overlap: two
    // 1.5 s tones in 2.85 s.
    assert!(silences_ms(&episode.pcm).is_empty());
    let want = 2.0 * 1.5 - 0.15;
    assert!(
        (episode.pcm.seconds() - want).abs() < 0.005,
        "{}",
        episode.pcm.seconds()
    );
}

#[test]
fn a_sound_over_a_turn_goes_on_the_second_track() {
    let mut turns = vec![turn("host", Pace::Normal), turn("guest", Pace::Normal)];
    turns[0].nonverbal.push(Nonverbal {
        kind: NonverbalKind::Backchannel {
            text: "mm-hm".into(),
        },
        by: SpeakerId("guest".into()),
        at: NonverbalAt::Over,
    });
    let script = script(turns);
    let chunks = [chunk(&script, 0..2, 2.0, 0.3, true)];
    let overlay = OverlayClip {
        turn: 0,
        at: NonverbalAt::Over,
        pcm: Pcm::new(MODEL_RATE, tone(0.3, 500.0, 0.3)),
    };
    let without = assemble(&script, &chunks, &[], &Gaps::default()).unwrap();
    let with = assemble(&script, &chunks, &[overlay], &Gaps::default()).unwrap();
    assert_eq!(with.pcm.len(), without.pcm.len());
    // The same samples but for one stretch. Each is normalised with its own
    // gain, so compare their shape: outside the overlay, `with` is
    // `without` times one constant, read off the second turn.
    let at = 3 * SAMPLE_RATE as usize;
    let gain = with.pcm.samples[at] / without.pcm.samples[at];
    let differs: Vec<usize> = (0..with.pcm.len())
        .filter(|&i| (with.pcm.samples[i] - without.pcm.samples[i] * gain).abs() > 1e-3)
        .collect();
    let (first, last) = (differs[0], differs[differs.len() - 1]);
    let stretch = (last - first) as f64 / f64::from(SAMPLE_RATE);
    assert!(stretch <= 0.31, "{stretch} s");
    // Inside the first turn, which is two seconds long and starts at 0.
    assert!(first > 0 && last < 2 * SAMPLE_RATE as usize);
}

#[test]
fn chunks_of_different_loudness_come_out_level() {
    let script = script(vec![
        turn("host", Pace::Normal),
        turn("guest", Pace::Normal),
        turn("host", Pace::Normal),
    ]);
    // One chunk per turn, 18 dB apart at the extremes.
    let chunks = [
        chunk(&script, 0..1, 2.0, 0.05, true),
        chunk(&script, 1..2, 2.0, 0.4, true),
        chunk(&script, 2..3, 2.0, 0.2, true),
    ];
    let episode = assemble(&script, &chunks, &[], &Gaps::default()).unwrap();
    check_level(&episode);
    let levels: Vec<f64> = episode
        .chunks
        .iter()
        .map(|span| {
            let span = span.clone().unwrap();
            let part = Pcm::new(SAMPLE_RATE, episode.pcm.samples[span].to_vec());
            measure(&part).unwrap().integrated_lufs
        })
        .collect();
    let max = levels.iter().copied().fold(f64::MIN, f64::max);
    let min = levels.iter().copied().fold(f64::MAX, f64::min);
    assert!(max - min <= 1.0, "{levels:?}");
}
