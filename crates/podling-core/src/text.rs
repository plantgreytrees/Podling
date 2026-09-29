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
}
