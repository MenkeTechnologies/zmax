//! Structural regular expressions — sam's `x` and `y` loops, as vis inherits
//! them (`sam(1)` "Loops and Conditionals", `vis(1)` SAM COMMANDS).
//!
//! `x/re/ command` runs `command` for every match of `re` inside the current
//! range; `y/re/ command` runs it for every stretch *between* matches. In sam
//! that is a loop that re-runs the command with dot set to each piece. zmax
//! already holds many pieces at once — a selection per match is exactly what
//! `x` produces — so the port sets the selection set and runs the command once
//! over it. Every zmax editing command is multi-selection aware, so `x/\w+/ d`
//! deletes every word just as sam does; what differs is that the command sees
//! the pieces together rather than one at a time, which is visible only to
//! commands that care about ordering.
//!
//! The two functions here are pure: given the text, the ranges to search inside
//! and a regex, they return the new ranges. The `:x` / `:y` commands own the
//! editor plumbing.

use zmax_core::{Range, RopeSlice, Selection};
use zmax_stdx::rope::{self, RopeSliceExt};

/// Every match of `re` inside `within` — sam's `x`.
///
/// An empty selection (a bare cursor) means the whole buffer, which is what
/// makes `:x/re/ …` useful without selecting anything first: sam's default
/// address for a command is dot, but its idiom is to run `x` over the file.
pub fn matches(text: RopeSlice, within: &Selection, re: &rope::Regex) -> Option<Selection> {
    let mut out: Vec<Range> = Vec::new();
    for range in scope(text, within) {
        for m in re.find_iter(text.regex_input_at(range.clone())) {
            let (start, end) = (m.start(), m.end());
            if start < end {
                out.push(Range::new(text.byte_to_char(start), text.byte_to_char(end)));
            }
        }
    }
    (!out.is_empty()).then(|| Selection::new(out.into(), 0))
}

/// Every stretch *between* matches of `re` inside `within` — sam's `y`.
///
/// The pieces before the first match and after the last one count, so
/// `y/,/ …` over `a,b,c` yields `a`, `b` and `c`. Empty stretches (two
/// adjacent matches) are dropped: sam has no empty dot to run a command on.
pub fn between(text: RopeSlice, within: &Selection, re: &rope::Regex) -> Option<Selection> {
    let mut out: Vec<Range> = Vec::new();
    for range in scope(text, within) {
        let (from, to) = (range.start, range.end);
        let mut cursor = from;
        for m in re.find_iter(text.regex_input_at(range.clone())) {
            let start = text.byte_to_char(m.start());
            if cursor < start {
                out.push(Range::new(cursor, start));
            }
            cursor = text.byte_to_char(m.end()).max(cursor);
        }
        if cursor < to {
            out.push(Range::new(cursor, to));
        }
    }
    (!out.is_empty()).then(|| Selection::new(out.into(), 0))
}

/// The char ranges to search inside: each selection, or the whole buffer when
/// the selection is a bare cursor.
fn scope(text: RopeSlice, within: &Selection) -> Vec<std::ops::Range<usize>> {
    // A lone cursor is not an address. In the vim-semantics keymaps a cursor is a
    // one-character range rather than an empty one, so both count as "no
    // selection" and the scope is the whole file — which is what `:structural-x`
    // with no address means in vis, and what sam's `,x/re/` idiom spells out.
    let whole = within.len() == 1 && within.primary().len() <= 1;
    if whole {
        return vec![0..text.len_chars()];
    }
    within
        .iter()
        .filter(|r| !r.is_empty())
        .map(|r| r.from()..r.to())
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use zmax_core::Rope;

    fn re(pattern: &str) -> rope::Regex {
        rope::RegexBuilder::new().build(pattern).unwrap()
    }

    fn text_of(rope: &Rope, selection: &Selection) -> Vec<String> {
        selection
            .iter()
            .map(|r| rope.slice(r.from()..r.to()).to_string())
            .collect()
    }

    #[test]
    fn x_selects_every_match_in_the_buffer() {
        let rope = Rope::from("one two three\n");
        let text = rope.slice(..);
        let cursor = Selection::point(0);
        let got = matches(text, &cursor, &re(r"\w+")).unwrap();
        assert_eq!(text_of(&rope, &got), ["one", "two", "three"]);
    }

    #[test]
    fn y_selects_the_gaps_including_the_ends() {
        let rope = Rope::from("a,b,c");
        let text = rope.slice(..);
        let cursor = Selection::point(0);
        let got = between(text, &cursor, &re(",")).unwrap();
        assert_eq!(text_of(&rope, &got), ["a", "b", "c"]);
    }

    #[test]
    fn adjacent_matches_leave_no_empty_piece() {
        let rope = Rope::from("a,,b");
        let text = rope.slice(..);
        let got = between(text, &Selection::point(0), &re(",")).unwrap();
        // The empty stretch between the two commas is not a piece sam would run
        // a command on, so it is dropped rather than kept as an empty range.
        assert_eq!(text_of(&rope, &got), ["a", "b"]);
    }

    #[test]
    fn a_selection_narrows_the_scope() {
        let rope = Rope::from("one two\nthree four\n");
        let text = rope.slice(..);
        // Only the first line is selected, so the second line's words are out of
        // scope — sam's `x` runs inside the current address, not the whole file.
        let first_line = Selection::single(0, 8);
        let got = matches(text, &first_line, &re(r"\w+")).unwrap();
        assert_eq!(text_of(&rope, &got), ["one", "two"]);
    }

    #[test]
    fn several_selections_are_each_searched() {
        let rope = Rope::from("a1 b2\nc3 d4\n");
        let text = rope.slice(..);
        let both_lines = Selection::new(vec![Range::new(0, 5), Range::new(6, 11)].into(), 0);
        let got = matches(text, &both_lines, &re(r"\d")).unwrap();
        assert_eq!(text_of(&rope, &got), ["1", "2", "3", "4"]);
    }

    #[test]
    fn no_match_leaves_the_caller_to_keep_the_selection() {
        let rope = Rope::from("nothing here\n");
        let text = rope.slice(..);
        assert!(matches(text, &Selection::point(0), &re(r"\d+")).is_none());
        // `y` with no match is the whole scope: one piece, not none.
        let got = between(text, &Selection::point(0), &re(r"\d+")).unwrap();
        assert_eq!(text_of(&rope, &got), ["nothing here\n"]);
    }
}

#[cfg(test)]
mod builder_probe {
    use super::*;
    use zmax_core::Rope;

    /// The `:structural-x` command builds its regex through `RegexBuilder` with a
    /// syntax `Config`; this pins that the configured builder finds the same
    /// matches as a bare one (a mismatch here is why `:sx /beta/` found nothing).
    #[test]
    fn the_command_path_builder_matches_too() {
        let rope = Rope::from("alpha, beta, gamma\n");
        let text = rope.slice(..);
        let configured = rope::RegexBuilder::new()
            .syntax(rope::Config::new().multi_line(true))
            .build("beta")
            .unwrap();
        let got = matches(text, &Selection::point(0), &configured);
        assert!(got.is_some(), "configured builder found no match");
        assert_eq!(got.unwrap().len(), 1);
    }
}

#[cfg(test)]
mod cursor_scope {
    use super::*;
    use zmax_core::Rope;

    /// A one-character cursor — what the vim-semantics keymaps carry — is not an
    /// address: the scope is the whole file, not the character under the cursor.
    /// `:sx /beta/` finding nothing was exactly this.
    #[test]
    fn a_one_char_cursor_still_means_the_whole_file() {
        let rope = Rope::from("alpha, beta, gamma\n");
        let text = rope.slice(..);
        let re = rope::RegexBuilder::new().build("beta").unwrap();
        let vim_cursor = Selection::single(0, 1);
        let got = matches(text, &vim_cursor, &re).expect("whole-file scope");
        assert_eq!(got.len(), 1);
        assert_eq!(rope.slice(got.primary().from()..got.primary().to()), "beta");
        // A real selection still narrows: only the first eight chars are in scope.
        let narrowed = Selection::single(0, 8);
        assert!(matches(text, &narrowed, &re).is_none());
    }
}
