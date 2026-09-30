//! Small text utilities shared by stages and the fake provider.

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

/// Shortest quoted span, in words, treated as a quotation. Shorter spans
/// ("so-called") read as scare quotes, not speech.
const MIN_QUOTED_WORDS: usize = 3;

/// Quotations in `text`: spans of at least [`MIN_QUOTED_WORDS`] words between
/// straight (`"…"`) or curly (`“…”`) quotation marks. An unmatched opening
/// mark yields nothing, so unbalanced text can't panic.
pub(crate) fn quotations(text: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut open: Option<(usize, char)> = None;
    for (i, c) in text.char_indices() {
        match open {
            None => match c {
                '"' => open = Some((i + 1, '"')),
                '\u{201C}' => open = Some((i + c.len_utf8(), '\u{201D}')),
                _ => {}
            },
            Some((start, close)) if c == close => {
                spans.push(&text[start..i]);
                open = None;
            }
            Some(_) => {}
        }
    }
    spans.retain(|s| s.split_whitespace().count() >= MIN_QUOTED_WORDS);
    spans
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
        }
    }
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
            return Err(PlaceholderError::Malformed(token));
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

    // Each placeholder counts as one word, so `"as {{quote:0}} says"` is still
    // seen as three typed words.
    let bare: String = parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => text,
            Part::Quote(_) => "_",
        })
        .collect();
    if let Some(span) = quotations(&bare).into_iter().next() {
        return Err(PlaceholderError::Typed(span.to_owned()));
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
    }
}
