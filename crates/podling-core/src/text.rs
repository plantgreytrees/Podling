//! Small text utilities shared by stages and the fake provider.

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
}
