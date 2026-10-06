//! Small text utilities shared by stages and the fake provider.

use std::collections::BTreeSet;
use std::fmt;
use std::ops::Range;

/// Byte ranges of the sentences in `text`, trimmed of surrounding whitespace.
///
/// A sentence ends at `.`, `!` or `?` followed by whitespace or the end of the
/// text. Deliberately simple: good enough for the fake provider and tests;
/// real claim extraction is the language model's job.
pub fn sentences(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let at_boundary = matches!(c, '.' | '!' | '?')
            && chars.peek().is_none_or(|&(_, next)| next.is_whitespace());
        if at_boundary {
            push_trimmed(text, start..i + c.len_utf8(), &mut out);
            start = i + c.len_utf8();
        }
    }
    push_trimmed(text, start..text.len(), &mut out);
    out
}

/// Words that carry no claim content, so they can't ground one.
const STOP_WORDS: &[&str] = &[
    "the", "and", "that", "with", "from", "this", "for", "are", "was", "were", "has", "had",
    "have", "its", "his", "her", "their", "they", "them", "over", "into", "about", "also", "but",
    "not", "who", "which", "been", "than", "then", "there", "these", "those",
];

/// Lower-cased words of `text` that carry content: at least three characters
/// (a number of any length counts, since a changed figure is a changed claim)
/// and not a stop word.
pub fn content_words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| {
            (w.chars().count() >= 3 || w.chars().any(|c| c.is_ascii_digit()))
                && !STOP_WORDS.contains(&w.as_str())
        })
        .collect()
}

/// Whether `word` (as [`content_words`] returns it) is a number: any word
/// with a digit in it, such as `1908` or `2150km`.
pub fn is_number(word: &str) -> bool {
    word.chars().any(|c| c.is_ascii_digit())
}

/// The numbers among `text`'s content words. Two claims that differ in a
/// number state different facts, however alike the rest is.
pub fn numbers(text: &str) -> BTreeSet<String> {
    content_words(text)
        .into_iter()
        .filter(|w| is_number(w))
        .collect()
}

/// Shortest quoted span, in words, treated as a quotation. Shorter spans
/// ("so-called") read as scare quotes, not speech.
const MIN_QUOTED_WORDS: usize = 3;

/// Quotations in `text`: spans of at least [`MIN_QUOTED_WORDS`] words between
/// straight (`"…"`) or curly (`“…”`) quotation marks. An unmatched opening
/// mark yields nothing, so unbalanced text can't panic.
pub(crate) fn quotations(text: &str) -> Vec<&str> {
    quotation_ranges(text)
        .into_iter()
        .map(|range| &text[range])
        .collect()
}

/// Byte ranges of [`quotations`] in `text`, without the marks.
pub(crate) fn quotation_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut open: Option<(usize, char)> = None;
    for (i, c) in text.char_indices() {
        match open {
            None => match c {
                '"' => open = Some((i + 1, '"')),
                '\u{201C}' => open = Some((i + c.len_utf8(), '\u{201D}')),
                _ => {}
            },
            Some((start, close)) if c == close => {
                ranges.push(start..i);
                open = None;
            }
            Some(_) => {}
        }
    }
    ranges.retain(|r| text[r.clone()].split_whitespace().count() >= MIN_QUOTED_WORDS);
    ranges
}

/// Where the model says a quote goes in a turn's text: `{{quote:N}}`, with N
/// counting from 0 over the turn's own quote references.
const PLACEHOLDER_OPEN: &str = "{{quote:";
const PLACEHOLDER_SYNTAX: &str = "{{quote:N}}";

fn placeholder(index: usize) -> String {
    format!("{{{{quote:{index}}}}}")
}

/// Why a turn's text can't be filled in. Each message says what to change,
/// because the model gets exactly one retry with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlaceholderError {
    /// A `{{quote` that isn't `{{quote:N}}`, as written.
    Malformed(String),
    /// `{{quote:index}}` names a quote the turn doesn't have.
    Unknown { index: usize, quotes: usize },
    /// The turn references quote `.0` but its text never says where.
    Unused(usize),
    /// Quoted words the model typed itself.
    Typed(String),
    /// A quotation mark the model typed that is never closed, or a closing
    /// mark with nothing open: the mark and a little of what follows it.
    Unclosed(String),
}

/// Longest piece of model text an error repeats back. The reason goes into
/// the retry's instructions, so a long echo would crowd them.
const MAX_ECHO_CHARS: usize = 80;

/// `text`, cut to [`MAX_ECHO_CHARS`] with `…` when longer.
fn echo(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(MAX_ECHO_CHARS).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

impl fmt::Display for PlaceholderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(token) => write!(
                f,
                "\"{token}\" is not a valid placeholder; write {PLACEHOLDER_SYNTAX} with a \
                 quote number for N, such as {}",
                placeholder(0)
            ),
            Self::Unknown { index, quotes } => write!(
                f,
                "the text uses {} but this turn has {quotes} quote reference(s), numbered from 0; \
                 the numbering starts again at 0 in every turn, so use only the numbers of this \
                 turn's own references, or add the missing reference",
                placeholder(*index)
            ),
            Self::Unused(index) => write!(
                f,
                "quote {index} is referenced but the text never says where; put {} where the \
                 turn speaks it, or drop the reference",
                placeholder(*index)
            ),
            Self::Typed(span) => write!(
                f,
                "the text puts \"{span}\" in quotation marks, but you must not type quoted \
                 words; write {PLACEHOLDER_SYNTAX} where a quote goes and leave the words out"
            ),
            Self::Unclosed(at) => write!(
                f,
                "the text has a quotation mark that is not part of a quote, at \"{at}\"; remove \
                 it, since quotes come only from {PLACEHOLDER_SYNTAX} (write inches as a word)"
            ),
        }
    }
}

/// Byte offset of the first quotation mark in `text` that doesn't pair up the
/// way [`quotations`] pairs them: an opening `"` or `“` that is never closed,
/// or a `”` with nothing open.
fn stray_mark(text: &str) -> Option<usize> {
    let mut open: Option<(usize, char)> = None;
    for (i, c) in text.char_indices() {
        match open {
            None => match c {
                '"' => open = Some((i, '"')),
                '\u{201C}' => open = Some((i, '\u{201D}')),
                '\u{201D}' => return Some(i),
                _ => {}
            },
            Some((_, close)) if c == close => open = None,
            Some(_) => {}
        }
    }
    open.map(|(start, _)| start)
}

/// A turn's text cut at its placeholders.
enum Part<'a> {
    Text(&'a str),
    Quote(usize),
}

/// Cuts `text` at every `{{quote:N}}`. A quotation mark on each side of a
/// placeholder (`"{{quote:0}}"`, `“{{quote:0}}”`) is dropped: the marks are
/// added when the quote is filled in, so keeping them would double them.
fn split_placeholders(text: &str) -> Result<Vec<Part<'_>>, PlaceholderError> {
    let mut parts = Vec::new();
    let mut literal_start = 0;
    while let Some(found) = text[literal_start..].find(PLACEHOLDER_OPEN) {
        let start = literal_start + found;
        let digits_start = start + PLACEHOLDER_OPEN.len();
        let digits = text[digits_start..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        let close = digits_start + digits;
        let index = text[digits_start..close]
            .parse::<usize>()
            .ok()
            .filter(|_| text[close..].starts_with("}}"));
        let Some(index) = index else {
            let token: String = text[start..]
                .chars()
                .take_while(|c| !c.is_whitespace())
                .take(40)
                .collect();
            return Err(PlaceholderError::Malformed(echo(&token)));
        };
        let mut before = start;
        let mut after = close + "}}".len();
        if let (Some(open @ ('"' | '\u{201C}')), Some(shut @ ('"' | '\u{201D}'))) = (
            text[literal_start..start].chars().next_back(),
            text[after..].chars().next(),
        ) {
            before -= open.len_utf8();
            after += shut.len_utf8();
        }
        if before > literal_start {
            parts.push(Part::Text(&text[literal_start..before]));
        }
        parts.push(Part::Quote(index));
        literal_start = after;
    }
    if literal_start < text.len() {
        parts.push(Part::Text(&text[literal_start..]));
    }
    Ok(parts)
}

/// Fills each `{{quote:N}}` in `text` with the N-th of `quotes`, in curly
/// quotation marks. The model marks where a quote goes and never types the
/// words, so every quotation in the result was copied from a source.
///
/// Fails when the text has a placeholder that isn't `{{quote:N}}`, quoted
/// words of its own, a number with no quote behind it, or a quote with no
/// placeholder. The result is built in one pass, so a quote that itself
/// contains `{{quote:0}}` is inserted as it is and never filled in again.
///
/// Curly marks are used because `quotations` closes `“` only on `”`: a straight
/// `"` inside a source sentence can't pair with a mark elsewhere in the turn.
pub(crate) fn fill_quote_placeholders(
    text: &str,
    quotes: &[&str],
) -> Result<String, PlaceholderError> {
    let parts = split_placeholders(text)?;

    // The model's own words, each placeholder kept as one word, so
    // `"as {{quote:0}} says"` is still seen as three typed words.
    let mut typed = String::with_capacity(text.len());
    for part in &parts {
        match part {
            Part::Text(text) => typed.push_str(text),
            Part::Quote(index) => typed.push_str(&placeholder(*index)),
        }
    }
    if let Some(span) = quotations(&typed).into_iter().next() {
        return Err(PlaceholderError::Typed(echo(span)));
    }
    // `quotations` finds nothing after an opening mark that never closes, so
    // one stray `"` would hide a typed quotation from it, and from the checks
    // that run on the filled-in text. Model text must be balanced.
    if let Some(at) = stray_mark(&typed) {
        let excerpt: String = typed[at..].chars().take(41).collect();
        return Err(PlaceholderError::Unclosed(echo(&excerpt)));
    }

    let mut used = vec![false; quotes.len()];
    for part in &parts {
        if let Part::Quote(index) = part {
            *used.get_mut(*index).ok_or(PlaceholderError::Unknown {
                index: *index,
                quotes: quotes.len(),
            })? = true;
        }
    }
    if let Some(unused) = used.iter().position(|used| !used) {
        return Err(PlaceholderError::Unused(unused));
    }

    let mut filled = String::with_capacity(text.len());
    for part in parts {
        match part {
            Part::Text(text) => filled.push_str(text),
            Part::Quote(index) => {
                filled.push('\u{201C}');
                filled.push_str(quotes[index]);
                filled.push('\u{201D}');
            }
        }
    }
    Ok(filled)
}

fn push_trimmed(text: &str, range: Range<usize>, out: &mut Vec<Range<usize>>) {
    let slice = &text[range.clone()];
    let lead = slice.len() - slice.trim_start().len();
    let trail = slice.len() - slice.trim_end().len();
    if lead + trail < slice.len() {
        out.push(range.start + lead..range.end - trail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str) -> Vec<&str> {
        sentences(text).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn splits_on_terminators_followed_by_space() {
        assert_eq!(
            split("  It was 7.14 a.m. Trees fell!  Why?\nNo one knew"),
            vec!["It was 7.14 a.m.", "Trees fell!", "Why?", "No one knew"]
        );
        assert!(split("   ").is_empty());
    }

    #[test]
    fn finds_straight_and_curly_quotations_but_not_scare_quotes() {
        assert_eq!(
            quotations(
                "A \"so-called\" blast; \"the sky split open\" and \u{201C}trees fell down flat\u{201D}."
            ),
            vec!["the sky split open", "trees fell down flat"]
        );
        assert!(quotations("an \"unbalanced mark with many words").is_empty());
    }

    const SKY: &str = "The sky split in two.";

    #[test]
    fn a_placeholder_becomes_the_quote_in_curly_marks() {
        assert_eq!(
            fill_quote_placeholders("A witness said: {{quote:0}} Then silence.", &[SKY]).unwrap(),
            "A witness said: \u{201C}The sky split in two.\u{201D} Then silence."
        );
        assert_eq!(
            fill_quote_placeholders("No quote here.", &[]).unwrap(),
            "No quote here."
        );
    }

    #[test]
    fn a_placeholder_may_be_used_twice() {
        let filled = fill_quote_placeholders("{{quote:0}} again: {{quote:0}}", &[SKY]).unwrap();
        assert_eq!(filled.matches(SKY).count(), 2);
    }

    #[test]
    fn marks_the_model_typed_around_a_placeholder_are_not_doubled() {
        for typed in ["\"{{quote:0}}\"", "\u{201C}{{quote:0}}\u{201D}"] {
            assert_eq!(
                fill_quote_placeholders(&format!("He said {typed} then left."), &[SKY]).unwrap(),
                "He said \u{201C}The sky split in two.\u{201D} then left.",
                "{typed}"
            );
        }
    }

    #[test]
    fn a_number_with_no_quote_behind_it_is_unknown() {
        assert_eq!(
            fill_quote_placeholders("{{quote:0}} and {{quote:1}}", &[SKY]),
            Err(PlaceholderError::Unknown {
                index: 1,
                quotes: 1
            })
        );
        assert_eq!(
            fill_quote_placeholders("{{quote:0}}", &[]),
            Err(PlaceholderError::Unknown {
                index: 0,
                quotes: 0
            })
        );
    }

    #[test]
    fn a_quote_with_no_placeholder_is_unused() {
        assert_eq!(
            fill_quote_placeholders("A witness said something.", &[SKY]),
            Err(PlaceholderError::Unused(0))
        );
        assert_eq!(
            fill_quote_placeholders("Only {{quote:1}} here.", &[SKY, "Trees fell."]),
            Err(PlaceholderError::Unused(0))
        );
    }

    #[test]
    fn a_malformed_placeholder_is_reported_as_written() {
        for (text, token) in [
            ("Hi {{quote:x}} there", "{{quote:x}}"),
            ("Hi {{quote:}} there", "{{quote:}}"),
            ("Hi {{quote:0", "{{quote:0"),
            ("Hi {{quote:0} there", "{{quote:0}"),
            (
                "Hi {{quote:99999999999999999999999}}",
                "{{quote:99999999999999999999999}}",
            ),
        ] {
            assert_eq!(
                fill_quote_placeholders(text, &[SKY]),
                Err(PlaceholderError::Malformed(token.into())),
                "{text}"
            );
        }
    }

    #[test]
    fn quoted_words_the_model_typed_are_rejected() {
        assert_eq!(
            fill_quote_placeholders("\"every tree caught fire at once\" and {{quote:0}}", &[SKY]),
            Err(PlaceholderError::Typed(
                "every tree caught fire at once".into()
            ))
        );
        // The placeholder counts as a word, so the model can't hide typed
        // words on either side of it.
        assert!(matches!(
            fill_quote_placeholders("\"as {{quote:0}} says\"", &[SKY]),
            Err(PlaceholderError::Typed(_))
        ));
        // Two words in marks read as a scare quote, not speech.
        assert!(fill_quote_placeholders("A \"so-called\" blast. {{quote:0}}", &[SKY]).is_ok());
    }

    #[test]
    fn a_stray_mark_that_would_hide_a_typed_quotation_is_rejected() {
        for text in [
            // A stray straight mark swallows the curly quotation after it.
            "He said \"oops. {{quote:0}} Then \u{201C}every tree caught fire at once\u{201D} ended.",
            // A stray curly mark swallows the straight quotation after it.
            "He said \u{201C}oops. {{quote:0}} Then \"every tree caught fire at once\" ended.",
            "a 5\" shell {{quote:0}}",
            "fell\u{201D} {{quote:0}}",
            "{{quote:0}}\u{201D}",
        ] {
            assert!(
                matches!(
                    fill_quote_placeholders(text, &[SKY]),
                    Err(PlaceholderError::Unclosed(_))
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn a_source_quote_with_its_own_stray_mark_is_still_filled_in() {
        let filled = fill_quote_placeholders("{{quote:0}}", &["He said \u{201C}hello"]).unwrap();
        assert_eq!(filled, "\u{201C}He said \u{201C}hello\u{201D}");
        // The balance rule is for the model's words, not the source's.
        assert!(fill_quote_placeholders("\u{201C}{{quote:0}}\u{201D}", &[SKY]).is_ok());
    }

    #[test]
    fn a_typed_span_shows_the_placeholders_the_model_wrote() {
        let Err(PlaceholderError::Typed(span)) =
            fill_quote_placeholders("\"{{quote:0}} and {{quote:1}}\"", &[SKY, "Trees fell."])
        else {
            panic!("expected Typed");
        };
        assert_eq!(span, "{{quote:0}} and {{quote:1}}");
    }

    #[test]
    fn model_text_repeated_in_an_error_is_capped() {
        let long = format!("\"{}\" {{{{quote:0}}}}", "word ".repeat(40));
        let Err(PlaceholderError::Typed(span)) = fill_quote_placeholders(&long, &[SKY]) else {
            panic!("expected Typed");
        };
        assert_eq!(span.chars().count(), MAX_ECHO_CHARS + 1);
        assert!(span.ends_with('…'));
    }

    #[test]
    fn a_quote_is_inserted_as_it_is_and_never_filled_in_again() {
        let tricky = "The tag {{quote:0}} is literal.";
        let filled = fill_quote_placeholders("He read {{quote:0}} aloud.", &[tricky]).unwrap();
        assert_eq!(
            filled,
            "He read \u{201C}The tag {{quote:0}} is literal.\u{201D} aloud."
        );
    }

    #[test]
    fn a_quote_containing_marks_cannot_pair_with_the_turns_own_marks() {
        let inner = "He said \"come back here right now\" and left.";
        let filled = fill_quote_placeholders("Then: {{quote:0}} Nothing else.", &[inner]).unwrap();
        // The whole quotation is inside the curly marks, and nothing outside
        // them is read as one.
        assert_eq!(quotations(&filled), vec![inner]);
    }

    #[test]
    fn every_error_says_what_to_change() {
        let shown = |e: PlaceholderError| e.to_string();
        assert!(shown(PlaceholderError::Malformed("{{quote:x}}".into())).contains("{{quote:0}}"));
        assert!(
            shown(PlaceholderError::Unknown {
                index: 3,
                quotes: 1
            })
            .contains("{{quote:3}}")
        );
        // A model that numbers quotes across the whole script is told to restart.
        assert!(
            shown(PlaceholderError::Unknown {
                index: 1,
                quotes: 1
            })
            .contains("in every turn")
        );
        assert!(shown(PlaceholderError::Unused(2)).contains("{{quote:2}}"));
        assert!(shown(PlaceholderError::Typed("a b c".into())).contains("{{quote:N}}"));
        let unclosed = shown(PlaceholderError::Unclosed("\" shell".into()));
        assert!(unclosed.contains("remove"), "{unclosed}");
        assert!(unclosed.contains("inches"), "{unclosed}");
    }
}
