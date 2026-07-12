//! Occur — the zemacs port of the GNU Emacs `occur` line search (`M-s o`).
//!
//! `occur` reads a regexp and collects every line of the buffer that contains a
//! match into an `*Occur*` buffer, one entry per matching line prefixed with the
//! line number; visiting an entry jumps to that line in the source. This module
//! is the pure, dependency-free core of that collection step: given the buffer
//! text and a per-line matcher, it walks the lines and returns one [`Match`] for
//! each line that matches — regardless of how many times the regexp matches on
//! that line (Emacs lists a line once, `occur-mode`'s entry pointing at the
//! first match). The matcher is a closure so the engine stays free of any regex
//! dependency; the command layer supplies one backed by the `regex` crate, and
//! the tests here supply plain substring/anchored matchers.
//!
//! Unlike [`crate::region_ops::occur`] — a boolean line filter that returns just
//! `(line_number, text)` for `:g/re/p`-style transforms — this engine also
//! records the column of the first match on the line, which the interactive
//! `occur-mode` overlay uses to place the cursor when jumping to a hit.

/// One matching line in the source buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// 1-based line number, as shown in the `*Occur*` buffer.
    pub line_number: usize,
    /// The full text of the matching line (no trailing newline).
    pub line_text: String,
    /// 0-based character column of the first match on the line — where
    /// `occur-mode-goto-occurrence` places point.
    pub match_col: usize,
}

/// Collect every line of `text` for which `first_match` reports a hit.
///
/// `first_match` is applied to each line in isolation and returns the 0-based
/// character column of the first match on that line, or `None` if the line does
/// not match. Because it runs per line, `^`/`$` anchors bind to the line's own
/// start/end (matching Emacs's line-oriented `occur`), and a line matching more
/// than once still yields a single [`Match`] pointing at the first hit.
///
/// Lines are split on `\n` with any trailing `\r` stripped (so CRLF buffers list
/// clean line text); the final line is included even without a trailing newline.
pub fn occur(text: &str, first_match: impl Fn(&str) -> Option<usize>) -> Vec<Match> {
    let mut out = Vec::new();
    for (i, line) in split_lines(text).enumerate() {
        if let Some(col) = first_match(line) {
            out.push(Match {
                line_number: i + 1,
                line_text: line.to_string(),
                match_col: col,
            });
        }
    }
    out
}

/// Collect [`occur`] matches across several named buffers (Emacs `multi-occur`
/// / `multi-occur-in-matching-buffers`). For each `(label, text)` buffer with at
/// least one matching line, yields `(label, Match)` pairs in buffer then line
/// order. `first_match` is the same per-line matcher [`occur`] takes.
pub fn multi_occur<'a>(
    buffers: impl IntoIterator<Item = (&'a str, &'a str)>,
    first_match: impl Fn(&str) -> Option<usize>,
) -> Vec<(String, Match)> {
    let mut out = Vec::new();
    for (label, text) in buffers {
        for m in occur(text, &first_match) {
            out.push((label.to_string(), m));
        }
    }
    out
}

/// Split `text` into lines on `\n`, stripping a trailing `\r` from each, and
/// keeping a final non-empty line that lacks a newline. An empty string yields
/// no lines; a trailing newline does not produce a spurious empty last line.
fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    // `str::lines` already implements exactly this contract (strip `\r`, no
    // trailing empty line, empty input -> empty iterator), matching how Emacs
    // numbers buffer lines.
    text.lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain case-sensitive substring matcher: column of the first occurrence.
    fn substr(needle: &'static str) -> impl Fn(&str) -> Option<usize> {
        move |line: &str| line.find(needle).map(|b| line[..b].chars().count())
    }

    #[test]
    fn collects_matching_lines_with_numbers() {
        let text = "alpha\nbeta\ngamma beta\ndelta\n";
        let hits = occur(text, substr("beta"));
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0],
            Match {
                line_number: 2,
                line_text: "beta".to_string(),
                match_col: 0,
            }
        );
        // Third line: "gamma beta" — first match starts at column 6.
        assert_eq!(
            hits[1],
            Match {
                line_number: 3,
                line_text: "gamma beta".to_string(),
                match_col: 6,
            }
        );
    }

    #[test]
    fn multi_occur_collects_across_buffers() {
        let buffers = vec![
            ("a.txt", "foo\nbar\nfoobar"),
            ("b.txt", "nope\nfoo"),
            ("c.txt", "nothing here"),
        ];
        let hits = multi_occur(buffers, substr("foo"));
        // a.txt lines 1 & 3, b.txt line 2, c.txt none.
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].0, "a.txt");
        assert_eq!(hits[0].1.line_number, 1);
        assert_eq!(hits[1].0, "a.txt");
        assert_eq!(hits[1].1.line_number, 3);
        assert_eq!(hits[2].0, "b.txt");
        assert_eq!(hits[2].1.line_number, 2);
    }

    #[test]
    fn multiple_matches_on_a_line_count_once() {
        // "aaa" contains three overlapping "a" matches but is one Occur entry,
        // pointing at the first (column 0).
        let hits = occur("aaa\nbbb\n", substr("a"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line_number, 1);
        assert_eq!(hits[0].match_col, 0);
    }

    #[test]
    fn no_match_yields_empty() {
        assert!(occur("one\ntwo\nthree\n", substr("zzz")).is_empty());
        assert!(occur("", substr("x")).is_empty());
    }

    #[test]
    fn anchored_matcher_binds_per_line() {
        // A "^foo" style matcher: only lines beginning with "foo".
        let anchored = |line: &str| line.starts_with("foo").then_some(0usize);
        let text = "foobar\nx foo\nfoo\n";
        let hits = occur(text, anchored);
        assert_eq!(
            hits.iter().map(|m| m.line_number).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[test]
    fn final_line_without_newline_is_scanned() {
        let hits = occur("no match here\nhit target", substr("target"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line_number, 2);
        assert_eq!(hits[0].line_text, "hit target");
    }

    #[test]
    fn crlf_lines_are_trimmed() {
        let hits = occur("keep\r\ndrop\r\nkeepistoo\r\n", substr("keep"));
        assert_eq!(hits.len(), 2);
        // The `\r` is stripped from the reported line text.
        assert_eq!(hits[0].line_text, "keep");
        assert_eq!(hits[1].line_text, "keepistoo");
    }

    #[test]
    fn match_col_is_character_not_byte() {
        // Multi-byte prefix: "é" is 2 bytes but 1 char; the match on "x" sits at
        // character column 2, not byte column 3.
        let hits = occur("éex\n", substr("x"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].match_col, 2);
    }
}
