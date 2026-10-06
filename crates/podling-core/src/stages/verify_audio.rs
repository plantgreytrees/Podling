//! Checks synthesised audio against the script with speech recognition.
//!
//! A chunk passes when the transcript's word error rate against the chunk's
//! text is at most `max_wer_pm` and every quote in the chunk is heard
//! verbatim. Both sides are normalised first (case, punctuation, numbers),
//! so "1908" and "nineteen oh eight" are the same words. A chunk that never
//! passes is kept, marked unverified, and reported as an `Error` finding:
//! the run completes, as it does for a misquote.

use std::cell::RefCell;
use std::ops::Range;

use podling_types::{ChunkRecord, ContentHash, Finding, PerMille, Severity};
use serde::Serialize;
use serde_json::{Value, json};

use crate::audio::Pcm;
use crate::cache::BlobStore;
use crate::error::{CoreError, Result};
use crate::plugin::{AsrProvider, AsrRequest, Transcript, transcribe_checked};
use crate::stage::Stage;

/// The analyser name on findings about audio that failed its checks.
pub const ANALYSER: &str = "verify_audio";

/// `text` as comparable words: lower case, no punctuation, apostrophes
/// dropped ("it's" → "its"), and numbers as digits, whether written as
/// digits ("1,908", "30th") or spelled out ("nineteen oh eight", "thirtieth").
pub fn words(text: &str) -> Vec<String> {
    let lower = text.to_lowercase().replace(['’', '‘'], "'");
    let mut cleaned = String::with_capacity(lower.len());
    let chars: Vec<char> = lower.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let digit_at = |j: Option<usize>| {
            j.and_then(|j| chars.get(j))
                .is_some_and(char::is_ascii_digit)
        };
        let between_digits = digit_at(i.checked_sub(1)) && digit_at(Some(i + 1));
        match c {
            '\'' => {}
            ',' if between_digits => {}
            '.' if between_digits => cleaned.push_str(" point "),
            '%' => cleaned.push_str(" percent "),
            '&' => cleaned.push_str(" and "),
            c if c.is_alphanumeric() => cleaned.push(c),
            _ => cleaned.push(' '),
        }
    }
    let tokens: Vec<&str> = cleaned
        .split_whitespace()
        .map(strip_ordinal_suffix)
        .collect();
    spell_numbers_as_digits(&tokens)
}

/// "30th" → "30"; anything else unchanged.
fn strip_ordinal_suffix(token: &str) -> &str {
    for suffix in ["st", "nd", "rd", "th"] {
        if let Some(digits) = token.strip_suffix(suffix)
            && !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit())
        {
            return digits;
        }
    }
    token
}

/// Below one hundred: units, teens and tens, as cardinals or ordinals.
fn small_number(word: &str) -> Option<u64> {
    const UNITS: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const ORDINALS: [&str; 20] = [
        "zeroth",
        "first",
        "second",
        "third",
        "fourth",
        "fifth",
        "sixth",
        "seventh",
        "eighth",
        "ninth",
        "tenth",
        "eleventh",
        "twelfth",
        "thirteenth",
        "fourteenth",
        "fifteenth",
        "sixteenth",
        "seventeenth",
        "eighteenth",
        "nineteenth",
    ];
    const TENS: [&str; 8] = [
        "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    const TENTHS: [&str; 8] = [
        "twentieth",
        "thirtieth",
        "fortieth",
        "fiftieth",
        "sixtieth",
        "seventieth",
        "eightieth",
        "ninetieth",
    ];
    let at = |list: &[&str]| list.iter().position(|w| *w == word).map(|i| i as u64);
    at(&UNITS)
        .or_else(|| at(&ORDINALS))
        .or_else(|| at(&TENS).map(|i| 20 + 10 * i))
        .or_else(|| at(&TENTHS).map(|i| 20 + 10 * i))
}

fn scale(word: &str) -> Option<u64> {
    match word {
        "hundred" | "hundredth" => Some(100),
        "thousand" | "thousandth" => Some(1_000),
        "million" | "millionth" => Some(1_000_000),
        "billion" | "billionth" => Some(1_000_000_000),
        _ => None,
    }
}

/// One piece of a spoken number.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Part {
    /// Below one hundred, e.g. "twenty one", or "oh eight" in a year.
    Small(u64),
    Scale(u64),
}

/// Replaces each run of number words with its digits; everything else is
/// kept. "nineteen oh eight", "nineteen hundred and eight" and "one thousand
/// nine hundred eight" all become "1908"; "twenty twenty four" becomes
/// "2024"; "one two three" stays three numbers.
fn spell_numbers_as_digits(tokens: &[&str]) -> Vec<String> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let (parts, used) = number_parts(&tokens[i..]);
        if used == 0 {
            out.push(tokens[i].to_owned());
            i += 1;
        } else {
            out.extend(fold(&parts).into_iter().map(|n| n.to_string()));
            i += used;
        }
    }
    out
}

/// The number at the start of `tokens`, as parts, and how many tokens it
/// used (0: none starts here).
fn number_parts(tokens: &[&str]) -> (Vec<Part>, usize) {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let word = tokens[i];
        let next = tokens.get(i + 1).copied();
        let unit_next = next.and_then(small_number).filter(|n| (1..10).contains(n));
        if let Some(n) = small_number(word) {
            // "twenty one" is one part.
            if n >= 20
                && let Some(unit) = unit_next
            {
                parts.push(Part::Small(n + unit));
                i += 2;
            } else {
                parts.push(Part::Small(n));
                i += 1;
            }
        } else if let Some(s) = scale(word) {
            parts.push(Part::Scale(s));
            i += 1;
        } else if (word == "oh" || word == "o")
            && matches!(parts.last(), Some(Part::Small(10..=99)))
            && let Some(unit) = unit_next
        {
            // "nineteen oh eight": the "08" of a year.
            parts.push(Part::Small(unit));
            i += 2;
        } else if parts.is_empty()
            && next.and_then(scale).is_some()
            && let Ok(n) = word.parse::<u64>()
        {
            // "80 million"
            parts.push(Part::Small(n));
            i += 1;
        } else if word == "a" && parts.is_empty() && next.and_then(scale).is_some() {
            // "a hundred"
            parts.push(Part::Small(1));
            i += 1;
        } else if word == "and"
            && matches!(parts.last(), Some(Part::Scale(_)))
            && next.is_some_and(|w| small_number(w).is_some())
        {
            i += 1;
        } else {
            break;
        }
    }
    // A lone "a" is just the article.
    if parts == [Part::Small(1)] && tokens[0] == "a" {
        return (Vec::new(), 0);
    }
    (parts, i)
}

/// Folds parts into numbers: scales multiply ("nine hundred"), a part after a
/// scale adds ("hundred eight"), and two adjacent parts below one hundred are
/// a year ("nineteen" "oh eight") when the first is 10 or more, otherwise
/// two numbers.
fn fold(parts: &[Part]) -> Vec<u64> {
    let mut numbers = Vec::new();
    let mut total = 0u64;
    let mut current: Option<u64> = None;
    // The last part was a small number that may still take a year's second half.
    let mut open_pair = false;
    for &part in parts {
        match part {
            Part::Small(v) => match current {
                Some(c) if open_pair && (10..=99).contains(&c) && total == 0 => {
                    current = Some(c * 100 + v);
                    open_pair = false;
                }
                Some(c) if !open_pair => {
                    // After a scale: "nine hundred" + "eight".
                    current = Some(c + v);
                    open_pair = false;
                }
                Some(c) => {
                    numbers.push(total + c);
                    total = 0;
                    current = Some(v);
                    open_pair = true;
                }
                None => {
                    current = Some(v);
                    open_pair = total == 0;
                }
            },
            Part::Scale(s) => {
                let c = current.take().unwrap_or(1);
                if s == 100 {
                    current = Some(c.saturating_mul(100));
                } else {
                    total = total.saturating_add(c.saturating_mul(s));
                }
                open_pair = false;
            }
        }
    }
    if current.is_some() || total > 0 {
        numbers.push(total + current.unwrap_or(0));
    }
    numbers
}

/// Word-level edit distance: substitutions, deletions and insertions.
pub fn word_errors(reference: &[String], hypothesis: &[String]) -> usize {
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, r) in reference.iter().enumerate() {
        let mut current = Vec::with_capacity(previous.len());
        current.push(i + 1);
        for (j, h) in hypothesis.iter().enumerate() {
            let substitute = previous[j] + usize::from(r != h);
            current.push(substitute.min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }
    previous[hypothesis.len()]
}

/// Word error rate in thousandths, rounded up and capped at 1000.
pub fn wer_pm(reference: &[String], hypothesis: &[String]) -> PerMille {
    let rate = if reference.is_empty() {
        if hypothesis.is_empty() { 0 } else { 1000 }
    } else {
        (word_errors(reference, hypothesis) * 1000).div_ceil(reference.len())
    };
    PerMille::new(rate.min(1000) as u16).expect("capped at 1000")
}

/// The quotes not heard word for word, in the order given.
pub fn quote_misses(quotes: &[String], heard: &[String]) -> Vec<String> {
    quotes
        .iter()
        .filter(|quote| {
            let wanted = words(quote);
            !wanted.is_empty() && !heard.windows(wanted.len()).any(|w| w == wanted.as_slice())
        })
        .cloned()
        .collect()
}

/// How a chunk's audio compared with its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub wer_pm: PerMille,
    pub quote_misses: Vec<String>,
}

impl Check {
    /// Compares `transcript` with the chunk's turn texts and its quotes.
    pub fn new(expected: &[String], quotes: &[String], transcript: &Transcript) -> Self {
        let reference: Vec<String> = expected.iter().flat_map(|t| words(t)).collect();
        let heard = words(&transcript.text());
        Self {
            wer_pm: wer_pm(&reference, &heard),
            quote_misses: quote_misses(quotes, &heard),
        }
    }

    pub fn passes(&self, max_wer_pm: PerMille) -> bool {
        self.wer_pm <= max_wer_pm && self.quote_misses.is_empty()
    }
}

/// Where each turn is in `pcm`, from the transcript's timings, for a backend
/// that gave none. `None` when the transcript can't place every turn.
///
/// Each transcript word is timed by its share of its segment, the
/// transcript is aligned word by word with the turns, and each turn starts
/// halfway between the last word heard before it and the first word heard of
/// it, moved to the quietest point nearby when there is a clear pause.
pub fn spans_from(
    transcript: &Transcript,
    expected: &[String],
    pcm: &Pcm,
) -> Option<Vec<Range<usize>>> {
    if expected.len() < 2 {
        return (!pcm.is_empty()).then(|| std::iter::once(0..pcm.len()).collect());
    }
    let mut heard: Vec<(String, f64, f64)> = Vec::new();
    for segment in &transcript.segments {
        let segment_words = words(&segment.text);
        let step = (segment.end - segment.start) / segment_words.len().max(1) as f64;
        for (i, word) in segment_words.into_iter().enumerate() {
            let start = segment.start + step * i as f64;
            heard.push((word, start, start + step));
        }
    }
    let mut reference = Vec::new();
    for (turn, text) in expected.iter().enumerate() {
        reference.extend(words(text).into_iter().map(|w| (w, turn)));
    }
    let heard_words: Vec<String> = heard.iter().map(|(w, ..)| w.clone()).collect();
    let reference_words: Vec<String> = reference.iter().map(|(w, _)| w.clone()).collect();
    // For each heard word, the turn of the reference word it lines up with.
    let turn_of: Vec<Option<usize>> = align(&reference_words, &heard_words)
        .into_iter()
        .map(|r| r.map(|r| reference[r].1))
        .collect();

    let rate = f64::from(pcm.rate);
    let mut starts = vec![0usize];
    for turn in 1..expected.len() {
        let first = turn_of.iter().position(|t| t.is_some_and(|t| t >= turn))?;
        let before = turn_of[..first].iter().rposition(Option::is_some)?;
        let at = (heard[before].2 + heard[first].1) / 2.0;
        let sample = snap_to_pause(pcm, (at * rate) as usize);
        if sample <= *starts.last().expect("starts with 0") || sample >= pcm.len() {
            return None;
        }
        starts.push(sample);
    }
    starts.push(pcm.len());
    Some(starts.windows(2).map(|w| w[0]..w[1]).collect())
}

/// Lines `hypothesis` up with `reference` by edit distance: for each
/// hypothesis word, the reference word it matched or replaced, or `None`
/// for an inserted word.
fn align(reference: &[String], hypothesis: &[String]) -> Vec<Option<usize>> {
    let (n, m) = (reference.len(), hypothesis.len());
    let mut cost = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in cost.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in cost[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let substitute =
                cost[i - 1][j - 1] + usize::from(reference[i - 1] != hypothesis[j - 1]);
            cost[i][j] = substitute.min(cost[i - 1][j] + 1).min(cost[i][j - 1] + 1);
        }
    }
    let mut matched = vec![None; m];
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        let substitute = cost[i - 1][j - 1] + usize::from(reference[i - 1] != hypothesis[j - 1]);
        if cost[i][j] == substitute {
            matched[j - 1] = Some(i - 1);
            i -= 1;
            j -= 1;
        } else if cost[i][j] == cost[i - 1][j] + 1 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    matched
}

/// Moves `at` to the middle of the quietest 20 ms within 300 ms of it, when
/// that is a clear pause (under half the window's median loudness);
/// otherwise leaves it.
fn snap_to_pause(pcm: &Pcm, at: usize) -> usize {
    let frame = (pcm.rate / 50).max(1) as usize;
    let reach = (pcm.rate as usize * 3) / 10;
    let from = at.saturating_sub(reach);
    let to = (at + reach).min(pcm.len());
    let frames: Vec<(usize, f32)> = (from..to.saturating_sub(frame))
        .step_by(frame)
        .map(|start| {
            let samples = &pcm.samples[start..start + frame];
            let energy = samples.iter().map(|s| s * s).sum::<f32>() / frame as f32;
            (start + frame / 2, energy)
        })
        .collect();
    let Some(&(quietest, energy)) = frames.iter().min_by(|a, b| a.1.total_cmp(&b.1)) else {
        return at;
    };
    let mut energies: Vec<f32> = frames.iter().map(|f| f.1).collect();
    energies.sort_by(f32::total_cmp);
    let median = energies[energies.len() / 2];
    if energy < median * 0.5 { quietest } else { at }
}

/// The input of one [`TranscribeChunk`] run, and so its cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscribeInput {
    /// The take's audio in the blob store.
    pub blob: ContentHash,
    /// What it should say, turn by turn (only a fake recogniser reads it).
    pub expected: Vec<String>,
}

/// Transcribes one take of a chunk. Cached on its own, so a rerun makes no
/// recogniser calls, and changing the recogniser (or the WER limit, which is
/// applied outside the cache) re-synthesises nothing.
pub struct TranscribeChunk<'a> {
    /// `RefCell`: [`Stage::run`] takes `&self`, [`AsrProvider::transcribe`]
    /// `&mut self`. See `SynthesizeChunk` for the same bridge.
    asr: RefCell<&'a mut dyn AsrProvider>,
    fingerprint: Value,
    blobs: &'a BlobStore,
}

impl<'a> TranscribeChunk<'a> {
    pub fn new(asr: &'a mut dyn AsrProvider, blobs: &'a BlobStore) -> Self {
        Self {
            fingerprint: asr.fingerprint(),
            asr: RefCell::new(asr),
            blobs,
        }
    }
}

impl Stage for TranscribeChunk<'_> {
    const ID: &'static str = "transcribe_chunk";
    const VERSION: u32 = 1;
    type Input = TranscribeInput;
    type Output = Transcript;

    fn config_fingerprint(&self) -> Value {
        json!({ "asr": self.fingerprint })
    }

    fn run(&self, input: &TranscribeInput) -> Result<Transcript> {
        let bytes =
            self.blobs
                .get(&input.blob)?
                .ok_or_else(|| CoreError::InvalidProviderOutput {
                    stage: Self::ID,
                    message: format!("audio blob {} vanished during the run", input.blob),
                })?;
        let pcm = Pcm::from_wav(&bytes)?;
        let request = AsrRequest {
            pcm: &pcm,
            expected: &input.expected,
        };
        let mut asr = self.asr.borrow_mut();
        transcribe_checked(&mut **asr, Self::ID, &request)
    }
}

/// An `Error` finding for each chunk that never passed its checks.
pub fn findings(records: &[ChunkRecord], max_wer_pm: PerMille) -> Vec<Finding> {
    records
        .iter()
        .filter(|record| !record.verified)
        .map(|record| {
            let turns = record.turns;
            let mut message = format!(
                "the audio of turns {}..{} failed speech recognition on every take; \
                 the one kept (take {}) has word error rate {}‰ (limit {}‰)",
                turns.start(),
                turns.end(),
                u16::from(record.take) + 1,
                record.wer_pm.get(),
                max_wer_pm.get()
            );
            if !record.quote_misses.is_empty() {
                message.push_str(&format!(
                    "; quotes not heard: {}",
                    record
                        .quote_misses
                        .iter()
                        .map(|q| format!("{q:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Finding {
                analyser: ANALYSER.into(),
                severity: Severity::Error,
                turn: Some(turns.start()),
                message,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::Segment;

    fn w(text: &str) -> Vec<String> {
        words(text)
    }

    #[test]
    fn case_punctuation_and_apostrophes_are_ignored() {
        assert_eq!(
            w("It's   the END, isn't it?"),
            ["its", "the", "end", "isnt", "it"]
        );
        assert_eq!(
            w("Tunguska—a forest; “quiet”."),
            ["tunguska", "a", "forest", "quiet"]
        );
    }

    #[test]
    fn numbers_read_the_same_however_they_are_written() {
        let year = ["in", "1908"];
        assert_eq!(w("in 1908"), year);
        assert_eq!(w("in nineteen oh eight"), year);
        assert_eq!(w("in nineteen hundred and eight"), year);
        assert_eq!(w("in one thousand nine hundred eight"), year);
        assert_eq!(w("twenty twenty-four"), ["2024"]);
        assert_eq!(w("80 million trees"), w("eighty million trees"));
        assert_eq!(w("1,000 km"), w("one thousand km"));
        assert_eq!(w("a hundred"), ["100"]);
        assert_eq!(w("on the 30th"), w("on the thirtieth"));
        assert_eq!(w("one two three"), ["1", "2", "3"]);
        assert_eq!(w("a tree"), ["a", "tree"]);
        assert_eq!(w("3.5 percent"), w("3.5%"));
        assert_eq!(w("oh no"), ["oh", "no"]);
    }

    #[test]
    fn word_error_rate_counts_edits_per_reference_word() {
        let reference = w("the trees fell in 1908");
        assert_eq!(
            wer_pm(&reference, &w("The trees fell in nineteen oh eight.")).get(),
            0
        );
        // One substitution and one deletion out of five words.
        assert_eq!(wer_pm(&reference, &w("the tree fell 1908")).get(), 400);
        assert_eq!(wer_pm(&[], &[]).get(), 0);
        assert_eq!(wer_pm(&[], &w("noise")).get(), 1000);
        // Insertions can pass 100%; the rate is capped.
        assert_eq!(wer_pm(&w("hi"), &w("hi hi hi hi")).get(), 1000);
    }

    fn transcript(text: &str) -> Transcript {
        Transcript {
            segments: vec![Segment {
                text: text.into(),
                start: 0.0,
                end: 1.0,
            }],
        }
    }

    #[test]
    fn a_missing_quote_fails_the_chunk_even_when_the_wer_passes() {
        let expected = [
            "He wrote that the sky split in two and fire covered the north, and we have read that line many times over the years since then."
                .to_owned(),
        ];
        let quotes = ["the sky split in two".to_owned()];
        let limit = PerMille::new(80).unwrap();

        let heard = Check::new(&expected, &quotes, &transcript(&expected[0]));
        assert!(heard.passes(limit));

        // One word misheard inside the quote: 1 error in 26 words, 39‰ rounded up.
        let misheard = expected[0].replace("split", "spit");
        let check = Check::new(&expected, &quotes, &transcript(&misheard));
        assert_eq!(check.wer_pm.get(), 39);
        assert_eq!(check.quote_misses, quotes);
        assert!(!check.passes(limit), "the quote fails it on its own");
    }

    #[test]
    fn spans_follow_the_transcript_and_snap_to_a_pause() {
        // Two turns of tone with a 200 ms pause at 1.1 s; the transcript
        // puts the turn change at 1.0 s.
        let rate = 16_000;
        let mut samples = vec![0.3f32; rate * 2];
        for s in &mut samples[rate * 11 / 10..rate * 13 / 10] {
            *s = 0.0;
        }
        let pcm = Pcm::new(rate as u32, samples);
        let expected = ["one two three".to_owned(), "four five six".to_owned()];
        let heard = Transcript {
            segments: vec![
                Segment {
                    text: "one two three".into(),
                    start: 0.0,
                    end: 1.0,
                },
                Segment {
                    text: "four five six".into(),
                    start: 1.0,
                    end: 2.0,
                },
            ],
        };
        let spans = spans_from(&heard, &expected, &pcm).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].start, spans[1].end), (0, pcm.len()));
        let cut = spans[1].start as f64 / rate as f64;
        assert!((1.1..=1.3).contains(&cut), "cut at {cut} s");

        // A turn the transcript never reached can't be placed.
        let short = transcript("one two three");
        assert_eq!(spans_from(&short, &expected, &pcm), None);
    }

    #[test]
    fn unverified_chunks_become_error_findings() {
        let record = |verified, misses: &[&str]| ChunkRecord {
            id: ContentHash::of_parts(&[b"c"]),
            turns: podling_types::TurnRange::new(3, 5).unwrap(),
            blob: ContentHash::of_parts(&[b"b"]),
            seed: 1,
            take: 2,
            wer_pm: PerMille::new(120).unwrap(),
            quote_misses: misses.iter().map(|q| (*q).to_owned()).collect(),
            verified,
        };
        let found = findings(
            &[record(true, &[]), record(false, &["the sky split"])],
            PerMille::new(80).unwrap(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].turn, Some(3));
        assert!(
            found[0].message.contains("every take")
                && found[0].message.contains("take 3")
                && found[0].message.contains("\"the sky split\""),
            "{}",
            found[0].message
        );
    }
}
