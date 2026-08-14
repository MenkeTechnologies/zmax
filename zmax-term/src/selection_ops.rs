//! Two kakoune selection primitives zmax had no equivalent for: `<a-S>`, which
//! reduces each selection to its two ends, and `<a-&>`, which copies the main
//! selection's indentation onto every other selected line.
//!
//! Both are pure here — given the text and the selections they return what
//! should happen — so the commands in [`crate::commands`] only own the
//! transaction and the editor plumbing.

use zmax_core::{Range, RopeSlice, Selection};

/// kakoune `<a-S>`: keep the first and last character of each selection.
///
/// A selection one or two characters wide already *is* its ends, so it comes
/// back unchanged rather than being split into two overlapping ranges (which
/// `Selection::new` would merge straight back anyway).
pub fn first_and_last(selection: &Selection) -> Selection {
    let mut ranges: Vec<Range> = Vec::with_capacity(selection.len() * 2);
    for range in selection.iter() {
        let (from, to) = (range.from(), range.to());
        if to - from <= 2 {
            ranges.push(*range);
            continue;
        }
        ranges.push(Range::new(from, from + 1));
        ranges.push(Range::new(to - 1, to));
    }
    Selection::new(
        ranges.into(),
        selection.primary_index().min(ranges_len_guard(selection)),
    )
}

/// The primary index has to stay inside the new range list; splitting grows it,
/// so the original index is always valid, but a one-range selection that came
/// back unchanged must not point past the end.
fn ranges_len_guard(selection: &Selection) -> usize {
    selection.len().saturating_sub(1)
}

/// The leading whitespace of the line `char_idx` sits on.
pub fn indent_of_line(text: RopeSlice, char_idx: usize) -> String {
    let line = text.char_to_line(char_idx.min(text.len_chars()));
    let start = text.line_to_char(line);
    let mut indent = String::new();
    for ch in text.chars_at(start) {
        if ch == ' ' || ch == '\t' {
            indent.push(ch);
        } else {
            break;
        }
    }
    indent
}

/// kakoune `<a-&>`: replace the indentation of every line the selections touch
/// with `indent`. Returns `(start, end, replacement)` char ranges, one per line
/// that needs changing — lines that already carry that indentation are left
/// alone so the edit stays as small as what actually differs.
pub fn copy_indent_changes(
    text: RopeSlice,
    selection: &Selection,
    indent: &str,
) -> Vec<(usize, usize, String)> {
    let mut lines: Vec<usize> = Vec::new();
    for range in selection.iter() {
        let first = text.char_to_line(range.from());
        let last = text.char_to_line(range.to().saturating_sub(1).max(range.from()));
        for line in first..=last {
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    lines.sort_unstable();

    let mut changes = Vec::new();
    for line in lines {
        let start = text.line_to_char(line);
        let existing = indent_of_line(text, start);
        // An empty line has nothing to indent; kakoune leaves it be.
        if start + existing.len() >= text.len_chars() {
            continue;
        }
        let rest_starts_line = text
            .chars_at(start + existing.chars().count())
            .next()
            .is_none_or(|ch| ch == '\n' || ch == '\r');
        if rest_starts_line {
            continue;
        }
        if existing == indent {
            continue;
        }
        changes.push((start, start + existing.chars().count(), indent.to_string()));
    }
    changes
}

#[cfg(test)]
mod test {
    use super::*;
    use zmax_core::Rope;

    fn texts(rope: &Rope, selection: &Selection) -> Vec<String> {
        selection
            .iter()
            .map(|r| rope.slice(r.from()..r.to()).to_string())
            .collect()
    }

    #[test]
    fn first_and_last_keeps_both_ends() {
        let rope = Rope::from("hello world\n");
        let selection = Selection::single(0, 11);
        let ends = first_and_last(&selection);
        assert_eq!(texts(&rope, &ends), ["h", "d"]);
    }

    #[test]
    fn a_short_selection_is_already_its_ends() {
        // One and two characters wide: nothing to split.
        for range in [Range::new(0, 1), Range::new(0, 2)] {
            let selection = Selection::new(vec![range].into(), 0);
            let ends = first_and_last(&selection);
            assert_eq!(ends.ranges(), &[range]);
        }
    }

    #[test]
    fn every_selection_gets_split() {
        let rope = Rope::from("alpha beta gamma\n");
        let selection = Selection::new(vec![Range::new(0, 5), Range::new(11, 16)].into(), 0);
        let ends = first_and_last(&selection);
        assert_eq!(texts(&rope, &ends), ["a", "a", "g", "a"]);
    }

    #[test]
    fn copy_indent_rewrites_only_the_lines_that_differ() {
        let rope = Rope::from("    first\nsecond\n\tthird\n");
        let text = rope.slice(..);
        // Select all three lines.
        let selection = Selection::single(0, text.len_chars());
        let changes = copy_indent_changes(text, &selection, "    ");
        // Line 1 already has four spaces, so only lines 2 and 3 are rewritten.
        assert_eq!(changes.len(), 2);
        let (start, end, indent) = &changes[0];
        assert_eq!(rope.slice(*start..*end).to_string(), "");
        assert_eq!(indent, "    ");
        let (start, end, indent) = &changes[1];
        assert_eq!(rope.slice(*start..*end).to_string(), "\t");
        assert_eq!(indent, "    ");
    }

    #[test]
    fn an_empty_line_is_left_alone() {
        let rope = Rope::from("    a\n\n    b\n");
        let text = rope.slice(..);
        let selection = Selection::single(0, text.len_chars());
        // The blank line in the middle has nothing to indent, and indenting it
        // would leave trailing whitespace behind.
        assert!(copy_indent_changes(text, &selection, "    ").is_empty());
    }

    #[test]
    fn the_indent_of_a_line_is_its_leading_whitespace() {
        let rope = Rope::from("\t  mixed\nplain\n");
        let text = rope.slice(..);
        assert_eq!(indent_of_line(text, 3), "\t  ");
        assert_eq!(indent_of_line(text, 10), "");
    }
}
