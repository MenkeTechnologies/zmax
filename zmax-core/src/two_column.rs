//! Two-column (`2C`) mode — the pure text transforms behind the zmax port of
//! GNU Emacs `two-column.el`.
//!
//! Two-column mode links two side-by-side buffers so a wide document can be
//! edited as a left and a right column. The *live* linkage (a buffer-local
//! pointer to the partner buffer, synchronized scrolling, `2C-newline` splitting
//! a newline across both) is a display/association concern handled in the
//! terminal layer. What lives here is the reversible text geometry that is the
//! heart of `2C-split` and `2C-merge`, kept pure so it can be unit-tested:
//!
//! * [`split_columns`] cuts each line (from a starting row down) at a column,
//!   leaving the left part in place and returning the right part as its own set
//!   of lines — `2C-split`.
//! * [`merge_columns`] pads each left line out to a separator column and appends
//!   the matching right line — `2C-merge`.
//!
//! The two are inverses when `merge_columns`'s separator equals
//! `split_columns`'s cut column and every left line reaches that column.

/// The character length of a line.
fn width(line: &str) -> usize {
    line.chars().count()
}

/// `2C-split`: cut every line from `from_line` to the end at column `col`. The
/// returned `left` keeps lines `0..from_line` verbatim and truncates the rest to
/// their first `col` characters; `right` holds the removed right-hand parts of
/// lines `from_line..`, one entry per split line (empty where the line was
/// shorter than `col`).
pub fn split_columns(lines: &[String], from_line: usize, col: usize) -> (Vec<String>, Vec<String>) {
    let mut left = Vec::with_capacity(lines.len());
    let mut right = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        if i < from_line {
            left.push(line.clone());
            continue;
        }
        let cut = col.min(chars.len());
        left.push(chars[..cut].iter().collect());
        right.push(chars[cut..].iter().collect());
    }
    (left, right)
}

/// `2C-merge`: rejoin two columns. Each `left` line is padded with spaces out to
/// `sep_col` and the matching `right` line (by index) is appended. Left lines
/// with no right counterpart are padded and kept; right lines past the end of
/// `left` are appended on their own (indented to `sep_col`).
pub fn merge_columns(left: &[String], right: &[String], sep_col: usize) -> Vec<String> {
    let rows = left.len().max(right.len());
    let mut out = Vec::with_capacity(rows);
    for i in 0..rows {
        let l = left.get(i).cloned().unwrap_or_default();
        let mut line = l;
        if width(&line) < sep_col {
            line.push_str(&" ".repeat(sep_col - width(&line)));
        }
        if let Some(r) = right.get(i) {
            line.push_str(r);
        }
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn split_cuts_from_the_given_row() {
        let lines = v(&["keep me", "left1right1", "left2right2"]);
        // from row 1, cut at column 5
        let (left, right) = split_columns(&lines, 1, 5);
        assert_eq!(left, v(&["keep me", "left1", "left2"]));
        assert_eq!(right, v(&["right1", "right2"]));
    }

    #[test]
    fn split_handles_lines_shorter_than_the_column() {
        let lines = v(&["ab", "abcdef"]);
        let (left, right) = split_columns(&lines, 0, 4);
        assert_eq!(
            left,
            v(&["ab", "abcd"]),
            "short line kept whole on the left"
        );
        assert_eq!(
            right,
            v(&["", "ef"]),
            "short line contributes an empty right part"
        );
    }

    #[test]
    fn merge_pads_to_the_separator_and_appends_right() {
        let left = v(&["ab", "abcd"]);
        let right = v(&["XY", "Z"]);
        assert_eq!(merge_columns(&left, &right, 4), v(&["ab  XY", "abcdZ"]));
    }

    #[test]
    fn merge_keeps_unmatched_left_and_appends_extra_right() {
        let left = v(&["one", "two", "three"]);
        let right = v(&["R"]);
        // only row 0 has a right part; rows 1,2 are padded and kept
        assert_eq!(
            merge_columns(&left, &right, 6),
            v(&["one   R", "two   ", "three "])
        );

        let left = v(&["x"]);
        let right = v(&["a", "b"]);
        assert_eq!(merge_columns(&left, &right, 3), v(&["x  a", "   b"]));
    }

    #[test]
    fn split_then_merge_round_trips() {
        let lines = v(&["hello world", "foo    bar", "abcdefghij"]);
        let (left, right) = split_columns(&lines, 0, 6);
        // every line reaches column 6, so padding is a no-op and merge inverts split
        assert_eq!(merge_columns(&left, &right, 6), lines);
    }
}
