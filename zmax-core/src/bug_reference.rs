//! `bug-reference-mode` — find the bug references in buffer text.
//!
//! GNU Emacs' `bug-reference.el` scans for `bug-reference-bug-regexp` and
//! buttonizes what it finds, so `Bug#1234` in a commit message or a comment
//! becomes a link to that project's tracker. This is that scan, pure and
//! line-at-a-time so the renderer can run it over the visible lines only — the
//! same shape [`crate::goto_address`] has.
//!
//! Two things come out of a match, exactly as in the source (bug-reference.el:74):
//! the *region* to buttonize (the whole reference, group 1) and the *id* to
//! substitute into `bug-reference-url-format` (the number, group 2). They differ:
//! `Bug#42` is buttonized whole but only `42` goes into the URL.

use std::ops::Range;

/// The prefixes `bug-reference-bug-regexp` accepts before the number, each
/// allowing an optional space and (except `bug`) a required `#`: `bug`,
/// `patch`, `RFE`, and `PR <component>/`.
///
/// Matched case-insensitively. The regexp spells them `[Bb]ug`, `[Pp]atch` and
/// `RFE`, which reads as case-*sensitive*, but Emacs searches with
/// `case-fold-search` — t by default, in `bug-reference-fontify`'s
/// `re-search-forward` as everywhere else — so `BUG#12` and `rfe #7` match in a
/// real buffer. Verified by running the mode's own regexp under `emacs --batch`.
const PREFIXES: [&str; 3] = ["bug", "patch", "rfe"];

/// One bug reference in a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BugReference {
    /// Byte range of the whole reference within the line — what gets
    /// buttonized (the regexp's first subexpression).
    pub range: Range<usize>,
    /// The id substituted into the URL format (the second subexpression):
    /// `1234`, or `1234#5` for a reference to a message within a bug.
    pub id: String,
}

impl BugReference {
    /// The URL this reference points at, `format` being
    /// `bug-reference-url-format` — a template with one `%s` placeholder, which
    /// the id replaces. A format with no placeholder is used as-is, which is
    /// what Emacs' non-string (function) formats amount to here.
    pub fn url(&self, format: &str) -> String {
        format.replacen("%s", &self.id, 1)
    }
}

/// Every bug reference in `line`, in order.
///
/// Ported from `bug-reference-bug-regexp` (bug-reference.el:74):
///
/// ```text
/// \(\b\(?:[Bb]ug ?#?\|[Pp]atch ?#\|RFE ?#\|PR [a-z+-]+/\)\([0-9]+\(?:#[0-9]+\)?\)\)
/// ```
///
/// Written as a scan rather than a regex so this crate keeps its no-regex
/// dependency for buffer-scanning code, as `goto_address` does.
pub fn references(line: &str) -> Vec<BugReference> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        // `\b`: a reference starts at a word boundary.
        if i > 0 && is_word_byte(bytes[i - 1]) {
            i += 1;
            continue;
        }
        match prefix_at(line, i) {
            Some(after_prefix) => match number_at(line, after_prefix) {
                Some((id, end)) => {
                    out.push(BugReference { range: i..end, id });
                    i = end;
                }
                // A prefix with no number is not a reference; resume after it
                // so `bug bug#1` still finds the second one.
                None => i += 1,
            },
            None => i += 1,
        }
    }
    out
}

/// The end offset of a bug prefix starting at `at`, or `None`.
///
/// `[Bb]ug ?#?` and `[Pp]atch ?#` and `RFE ?#` take an optional space then a
/// `#` — required after `patch`/`RFE`, optional after `bug`, as the source
/// spells them. `PR [a-z+-]+/` is the gcc form: a component name then a slash.
fn prefix_at(line: &str, at: usize) -> Option<usize> {
    let rest = &line[at..];

    if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case("pr ") {
        let after = &rest[3..];
        let component: usize = after
            .bytes()
            .take_while(|b| b.is_ascii_alphabetic() || *b == b'+' || *b == b'-')
            .count();
        if component > 0 && after.as_bytes().get(component) == Some(&b'/') {
            return Some(at + 3 + component + 1);
        }
        return None;
    }

    for word in PREFIXES {
        let len = word.len();
        if rest.len() < len || !rest[..len].eq_ignore_ascii_case(word) {
            continue;
        }
        let mut j = at + len;
        if line.as_bytes().get(j) == Some(&b' ') {
            j += 1;
        }
        let hash = line.as_bytes().get(j) == Some(&b'#');
        if hash {
            j += 1;
        } else if word != "bug" {
            // `patch`/`RFE` require the `#`; only `bug` may go without.
            continue;
        }
        return Some(j);
    }
    None
}

/// The id at `at`: `[0-9]+` optionally followed by `#[0-9]+` (a message within
/// a bug). Returns the id and where it ends.
fn number_at(line: &str, at: usize) -> Option<(String, usize)> {
    let bytes = line.as_bytes();
    let mut j = at;
    while bytes.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    if j == at {
        return None;
    }
    if bytes.get(j) == Some(&b'#') && bytes.get(j + 1).is_some_and(u8::is_ascii_digit) {
        j += 1;
        while bytes.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
    }
    Some((line[at..j].to_string(), j))
}

/// Whether a byte is part of a word, for the leading `\b`.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The forms the default regexp accepts, and the id each yields.
    #[test]
    fn the_documented_forms_match() {
        let cases = [
            ("see Bug#1234 there", "1234", "Bug#1234"),
            ("see bug#1234 there", "1234", "bug#1234"),
            ("see bug 1234 there", "1234", "bug 1234"),
            ("see Bug #1234 there", "1234", "Bug #1234"),
            ("see Patch#99 there", "99", "Patch#99"),
            ("see RFE #7 there", "7", "RFE #7"),
            ("see PR c++/12345 there", "12345", "PR c++/12345"),
        ];
        for (line, id, text) in cases {
            let found = references(line);
            assert_eq!(found.len(), 1, "{line:?} -> {found:?}");
            assert_eq!(found[0].id, id, "{line:?}");
            assert_eq!(&line[found[0].range.clone()], text, "{line:?}");
        }
    }

    /// A reference to one message inside a bug keeps both numbers in the id,
    /// since that whole string is what the tracker URL needs.
    #[test]
    fn a_message_within_a_bug_keeps_both_numbers() {
        let found = references("Bug#1234#5");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "1234#5");
    }

    /// The buttonized region is the whole reference; only the number goes into
    /// the URL. Those are the regexp's two subexpressions and they differ.
    #[test]
    fn the_region_and_the_url_id_differ() {
        let found = references("fixes Bug#42");
        assert_eq!(&"fixes Bug#42"[found[0].range.clone()], "Bug#42");
        assert_eq!(
            found[0].url("https://example.org/show_bug.cgi?id=%s"),
            "https://example.org/show_bug.cgi?id=42"
        );
    }

    /// Case is folded, as Emacs's own search does. Each expectation here was
    /// produced by running `bug-reference-bug-regexp` under `emacs --batch`.
    #[test]
    fn case_is_folded() {
        for (line, id) in [
            ("BUG#12", "12"),
            ("BuG#3", "3"),
            ("rfe #7", "7"),
            ("PATCH#9", "9"),
            ("pr c++/1", "1"),
            ("PR C++/1", "1"),
            ("pR c/1", "1"),
        ] {
            let found = references(line);
            assert_eq!(found.len(), 1, "{line:?} -> {found:?}");
            assert_eq!(found[0].id, id, "{line:?}");
            assert_eq!(&line[found[0].range.clone()], line, "{line:?}");
        }
    }

    /// What must NOT match: a prefix with no number, `patch`/`RFE` without the
    /// `#` the source requires, and a reference glued to a preceding word (the
    /// leading `\b`).
    #[test]
    fn near_misses_do_not_match() {
        for line in [
            "just a bug here",
            "patch 99 without a hash",
            "RFE 7 without a hash",
            "debug#1234",
            "PR /12345",
        ] {
            assert!(references(line).is_empty(), "{line:?} should not match");
        }
    }

    /// Several references in one line are all found, in order.
    #[test]
    fn several_references_in_a_line() {
        let found = references("Bug#1 and bug#22 and Patch#333");
        assert_eq!(
            found.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["1", "22", "333"]
        );
    }
}
