//! Emacs `set-selective-display` (`C-x $`) support — hide lines indented past a
//! column threshold, reusing zemacs's fold model.
//!
//! Emacs's `selective-display` variable, when set to an integer N, displays only
//! lines that *start with fewer than N columns of indentation*; every line whose
//! indentation is N columns or more is elided (Emacs shows `...` at the end of the
//! preceding visible line). This module is the pure geometry: given each line's
//! leading-indentation width it returns the inclusive fold ranges to collapse. The
//! command wrapper turns those ranges into closed folds via
//! `crate::fold::Folds::create`.

/// Number of display columns of leading whitespace on `line`, expanding tabs to
/// the next multiple of `tab_width`. A blank or all-whitespace line reports the
/// full width of its whitespace (matching Emacs, which measures indentation up to
/// the first non-whitespace character or the end of the line). A trailing newline
/// or carriage return terminates the scan.
pub fn leading_indent_columns(line: &str, tab_width: usize) -> usize {
    let tw = tab_width.max(1);
    let mut col = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => col += 1,
            '\t' => col += tw - (col % tw),
            _ => break,
        }
    }
    col
}

/// Given the leading-indentation width of every line (`indents[i]` is line `i`'s
/// indentation in columns) and a `column` threshold, return the inclusive
/// `(start, end)` line ranges to collapse so that only lines indented fewer than
/// `column` columns remain visible.
///
/// Each returned range's `start` is a *visible* anchor line (indentation `<`
/// `column`) and lines `start + 1 ..= end` are the hidden, over-indented lines —
/// exactly the shape `crate::fold::Folds::create` expects (the header line stays
/// visible). A `column` of 0 disables selective display and yields no ranges. A run
/// of over-indented lines with no preceding visible anchor (i.e. one starting at
/// line 0) is left visible, since a fold needs a visible header line.
pub fn selective_display_folds(indents: &[usize], column: usize) -> Vec<(usize, usize)> {
    if column == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < indents.len() {
        if indents[i] >= column {
            let run_start = i;
            let mut end = i;
            while end + 1 < indents.len() && indents[end + 1] >= column {
                end += 1;
            }
            // A fold needs a preceding visible header line; skip a run that starts
            // at the very top of the buffer.
            if run_start >= 1 {
                out.push((run_start - 1, end));
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_spaces() {
        assert_eq!(leading_indent_columns("foo", 8), 0);
        assert_eq!(leading_indent_columns("    foo", 8), 4);
        assert_eq!(leading_indent_columns("  bar baz", 8), 2);
    }

    #[test]
    fn indent_tabs_expand_to_tab_stops() {
        // A leading tab advances to the next multiple of tab_width.
        assert_eq!(leading_indent_columns("\tfoo", 8), 8);
        assert_eq!(leading_indent_columns("\tfoo", 4), 4);
        // Two spaces then a tab: 2 -> next stop at 8.
        assert_eq!(leading_indent_columns("  \tfoo", 8), 8);
        // A tab then a tab with width 4: 4 -> 8.
        assert_eq!(leading_indent_columns("\t\tfoo", 4), 8);
    }

    #[test]
    fn indent_blank_and_empty() {
        assert_eq!(leading_indent_columns("", 8), 0);
        // An all-whitespace line reports the full whitespace width.
        assert_eq!(leading_indent_columns("   ", 8), 3);
        // A trailing newline is not counted.
        assert_eq!(leading_indent_columns("  \n", 8), 2);
    }

    #[test]
    fn disabled_when_column_zero() {
        assert_eq!(selective_display_folds(&[0, 4, 8, 0], 0), Vec::new());
    }

    #[test]
    fn hides_consecutive_over_indented_run() {
        // Threshold 1: lines 1 and 2 (indent 4, 8) hide under the visible line 0.
        assert_eq!(selective_display_folds(&[0, 4, 8, 0], 1), vec![(0, 2)]);
    }

    #[test]
    fn threshold_selects_deeper_indent() {
        // Threshold 5: only line 2 (indent 8) hides; its anchor is line 1 (indent 4).
        assert_eq!(selective_display_folds(&[0, 4, 8, 0], 5), vec![(1, 2)]);
    }

    #[test]
    fn two_separate_runs() {
        // Two 1-line runs each fold under their own preceding visible line.
        assert_eq!(
            selective_display_folds(&[0, 4, 0, 4], 1),
            vec![(0, 1), (2, 3)]
        );
    }

    #[test]
    fn top_run_without_anchor_is_skipped() {
        // A run starting at line 0 has no visible header, so it is left visible.
        assert_eq!(selective_display_folds(&[4, 0, 4], 1), vec![(1, 2)]);
    }

    #[test]
    fn nothing_over_threshold() {
        assert_eq!(selective_display_folds(&[0, 1, 2, 3], 8), Vec::new());
    }
}
