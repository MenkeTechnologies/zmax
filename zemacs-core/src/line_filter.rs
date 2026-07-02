//! Pure-Rust regex line-filtering — the substrate for GNU Emacs `keep-lines`
//! (a.k.a. `delete-non-matching-lines`), `flush-lines` (a.k.a.
//! `delete-matching-lines`) and `how-many` (a.k.a. `count-matches`). Emacs applies
//! these to the region when the mark is active, otherwise from point to the end of
//! the buffer; the command layer picks that span and hands the resulting substring
//! here.
//!
//! Like `region_ops` / `merge_ops`, everything here is a plain, editor-type-free
//! and regex-crate-free function over `&str`: the caller injects the actual match
//! test as a closure (exactly as [`crate::region_ops::occur`] does), so the engine
//! stays dependency-free and separately unit tested, and the same routines back a
//! `regex`-driven command or any other predicate.
//!
//! The transforming functions return the *character* ranges to delete — ascending
//! and non-overlapping — so the caller feeds them straight to
//! [`crate::Transaction::delete`] over the live rope (whose positions are chars).
//! Each range covers a whole line *including* its trailing newline, so deleting a
//! line leaves no empty remnant. Line boundaries are found with
//! `split_inclusive('\n')`, which yields no spurious trailing empty element, so the
//! ranges tile the input exactly and a full-buffer keep/flush round-trips cleanly.

/// A line and the half-open char range `[start, end)` it occupies in `text`,
/// where `end` includes the trailing `'\n'` (if any) so the range covers the whole
/// physical line. The `content` slice has that newline stripped, which is what the
/// match predicate is tested against — matching an Emacs pattern that is anchored
/// with `$` behaves as expected because the newline is not part of `content`.
fn line_spans(text: &str) -> Vec<(usize, usize, &str)> {
    let mut spans = Vec::new();
    let mut start = 0usize; // running char offset
    for seg in text.split_inclusive('\n') {
        let nchars = seg.chars().count();
        let content = seg.strip_suffix('\n').unwrap_or(seg);
        spans.push((start, start + nchars, content));
        start += nchars;
    }
    spans
}

/// Emacs `flush-lines` / `delete-matching-lines` (in-buffer `grep -v`): the char
/// ranges of every line that *matches* `matches`, i.e. the lines to delete. Ranges
/// are ascending and non-overlapping, each covering the line plus its newline.
pub fn flush_lines_ranges(text: &str, matches: impl Fn(&str) -> bool) -> Vec<(usize, usize)> {
    line_spans(text)
        .into_iter()
        .filter(|&(_, _, content)| matches(content))
        .map(|(s, e, _)| (s, e))
        .collect()
}

/// Emacs `keep-lines` / `delete-non-matching-lines` (in-buffer `grep`): the char
/// ranges of every line that does *not* match `matches`, i.e. the lines to delete
/// so that only matching lines survive. Ascending, non-overlapping, newline-
/// inclusive — the exact complement of [`flush_lines_ranges`].
pub fn keep_lines_ranges(text: &str, matches: impl Fn(&str) -> bool) -> Vec<(usize, usize)> {
    line_spans(text)
        .into_iter()
        .filter(|&(_, _, content)| !matches(content))
        .map(|(s, e, _)| (s, e))
        .collect()
}

/// Emacs `how-many` / `count-matches`: report `(total_matches, matching_lines)`.
/// `count_total` is applied once to the whole span (so patterns that span newlines
/// are counted, matching Emacs's `re-search-forward` loop); `line_matches` is the
/// per-line predicate used to tally how many distinct lines contain a match. The
/// two are independent because a single line may hold several matches.
pub fn how_many(
    text: &str,
    count_total: impl Fn(&str) -> usize,
    line_matches: impl Fn(&str) -> bool,
) -> (usize, usize) {
    let total = count_total(text);
    let lines = text.lines().filter(|l| line_matches(l)).count();
    (total, lines)
}

/// Apply a set of ascending, non-overlapping char ranges as deletions, returning
/// the resulting text — the pure-string mirror of feeding the same ranges to
/// [`crate::Transaction::delete`]. Ranges must be sorted and disjoint (as produced
/// by [`flush_lines_ranges`] / [`keep_lines_ranges`]); out-of-order ranges are
/// ignored to stay panic-free. Handy for callers wanting a string result and for
/// verifying the ranges in tests.
pub fn delete_ranges(text: &str, ranges: &[(usize, usize)]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for &(from, to) in ranges {
        if from < cursor || to > chars.len() || from > to {
            continue;
        }
        out.extend(&chars[cursor..from]);
        cursor = to;
    }
    out.extend(&chars[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // A tiny substring / prefix matcher so the tests stay regex-crate-free, exactly
    // like the closures the command layer injects.
    fn contains<'a>(needle: &'a str) -> impl Fn(&str) -> bool + 'a {
        move |line: &str| line.contains(needle)
    }

    #[test]
    fn line_spans_tile_input_exactly() {
        // Every span is contiguous and the last reaches the end (char count).
        let text = "ab\ncd\ne";
        let spans = line_spans(text);
        assert_eq!(spans, vec![(0, 3, "ab"), (3, 6, "cd"), (6, 7, "e")]);
        // Trailing newline: no spurious empty final span.
        assert_eq!(line_spans("ab\n"), vec![(0, 3, "ab")]);
        // Empty input yields nothing.
        assert!(line_spans("").is_empty());
    }

    #[test]
    fn flush_deletes_matching_lines() {
        let text = "apple\nbanana\napricot\ncherry";
        let ranges = flush_lines_ranges(text, contains("ap"));
        // "apple" [0,6) and "apricot" [13,21) match.
        assert_eq!(ranges, vec![(0, 6), (13, 21)]);
        assert_eq!(delete_ranges(text, &ranges), "banana\ncherry");
    }

    #[test]
    fn keep_deletes_non_matching_lines() {
        let text = "apple\nbanana\napricot\ncherry";
        let ranges = keep_lines_ranges(text, contains("ap"));
        assert_eq!(delete_ranges(text, &ranges), "apple\napricot\n");
        // keep is the exact complement of flush.
        let flush = flush_lines_ranges(text, contains("ap"));
        let mut all: Vec<_> = ranges.iter().chain(flush.iter()).copied().collect();
        all.sort();
        assert_eq!(
            all,
            line_spans(text)
                .into_iter()
                .map(|(s, e, _)| (s, e))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn all_match_and_none_match() {
        let text = "aa\nab\nac";
        // all lines contain 'a': flush removes everything, keep removes nothing.
        assert_eq!(
            delete_ranges(text, &flush_lines_ranges(text, contains("a"))),
            ""
        );
        assert_eq!(
            delete_ranges(text, &keep_lines_ranges(text, contains("a"))),
            text
        );
        // no line contains 'z': flush removes nothing, keep removes everything.
        assert!(flush_lines_ranges(text, contains("z")).is_empty());
        assert_eq!(
            delete_ranges(text, &keep_lines_ranges(text, contains("z"))),
            ""
        );
    }

    #[test]
    fn anchored_predicate() {
        let text = "foo\nafoo\nfoobar\n barbar";
        // "starts with foo" — leading-space and 'a'-prefixed lines are excluded.
        let starts = |l: &str| l.starts_with("foo");
        assert_eq!(
            delete_ranges(text, &keep_lines_ranges(text, starts)),
            "foo\nfoobar\n"
        );
    }

    #[test]
    fn adjacent_matches_stay_separate_ranges() {
        let text = "x\nx\ny\nx";
        let ranges = flush_lines_ranges(text, contains("x"));
        // Three separate line ranges, not one merged span.
        assert_eq!(ranges, vec![(0, 2), (2, 4), (6, 7)]);
        assert_eq!(delete_ranges(text, &ranges), "y\n");
    }

    #[test]
    fn how_many_counts_matches_and_lines() {
        let text = "a a b\nc a\nzzz";
        // total 'a' occurrences = 3, over 2 lines.
        let total = |t: &str| t.matches('a').count();
        let (m, l) = how_many(text, total, contains("a"));
        assert_eq!((m, l), (3, 2));
        // no match: both zero.
        let (m0, l0) = how_many(text, |t: &str| t.matches('Q').count(), contains("Q"));
        assert_eq!((m0, l0), (0, 0));
    }

    #[test]
    fn empty_input_is_a_no_op() {
        assert!(flush_lines_ranges("", contains("a")).is_empty());
        assert!(keep_lines_ranges("", contains("a")).is_empty());
        assert_eq!(delete_ranges("", &[]), "");
        assert_eq!(how_many("", |t: &str| t.len(), contains("a")), (0, 0));
    }
}
