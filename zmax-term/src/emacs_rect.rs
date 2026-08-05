//! Emacs rectangle commands (`C-x r k/d/c/y/M-w`).
//!
//! zmax emulates column selection with multiple cursors, but has no true
//! *rectangle* — a column span `[c0,c1)` over a line range `[l0,l1]` operated on as
//! a unit. This module is the pure geometry (extract / delete / clear / yank),
//! tested on a small char grid; `commands.rs` translates the live selection's
//! two corners into (l0,l1,c0,c1), calls these, and applies the result as one
//! whole-document transaction. The killed rectangle is held here for yank.
//!
//! Columns are character columns (a simplification of emacs's display columns,
//! good enough for the ASCII/code case). Short lines are treated as if padded
//! with spaces to the needed width.

use std::sync::Mutex;

use once_cell::sync::Lazy;

static SAVED: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

pub fn save(rect: Vec<String>) {
    *SAVED.lock().unwrap() = rect;
}

pub fn saved() -> Vec<String> {
    SAVED.lock().unwrap().clone()
}

fn cols(line: &str) -> Vec<char> {
    line.chars().collect()
}

/// The text inside the rectangle, one string per line in `[l0, l1]`.
pub fn extract(lines: &[String], l0: usize, l1: usize, c0: usize, c1: usize) -> Vec<String> {
    let (c0, c1) = (c0.min(c1), c0.max(c1));
    (l0..=l1)
        .filter_map(|i| lines.get(i))
        .map(|line| {
            let cs = cols(line);
            let from = c0.min(cs.len());
            let to = c1.min(cs.len());
            cs[from..to].iter().collect()
        })
        .collect()
}

/// Remove the rectangle's columns from each line in range.
pub fn delete(lines: &[String], l0: usize, l1: usize, c0: usize, c1: usize) -> Vec<String> {
    let (c0, c1) = (c0.min(c1), c0.max(c1));
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i < l0 || i > l1 {
                return line.clone();
            }
            let cs = cols(line);
            let from = c0.min(cs.len());
            let to = c1.min(cs.len());
            let mut out: String = cs[..from].iter().collect();
            out.extend(cs[to..].iter());
            out
        })
        .collect()
}

/// Replace the rectangle's columns with spaces (blank it, keeping width).
pub fn clear(lines: &[String], l0: usize, l1: usize, c0: usize, c1: usize) -> Vec<String> {
    let (c0, c1) = (c0.min(c1), c0.max(c1));
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i < l0 || i > l1 {
                return line.clone();
            }
            let mut cs = cols(line);
            // pad short lines so the cleared block is rectangular
            if cs.len() < c1 {
                cs.resize(c1, ' ');
            }
            for ch in cs.iter_mut().take(c1).skip(c0) {
                *ch = ' ';
            }
            cs.into_iter().collect()
        })
        .collect()
}

/// Emacs `open-rectangle`: insert `|c1-c0|` spaces at column `c0` on each line in
/// range, shifting existing text right. Short lines are padded to `c0` first.
pub fn open(lines: &[String], l0: usize, l1: usize, c0: usize, c1: usize) -> Vec<String> {
    let (c0, c1) = (c0.min(c1), c0.max(c1));
    let width = c1 - c0;
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i < l0 || i > l1 {
                return line.clone();
            }
            let mut cs = cols(line);
            if cs.len() < c0 {
                cs.resize(c0, ' ');
            }
            cs.splice(c0..c0, vec![' '; width]);
            cs.into_iter().collect()
        })
        .collect()
}

/// Emacs `delete-whitespace-rectangle`: starting at the rectangle's left column
/// `c0` on each line in range, delete the following run of spaces/tabs. Lines
/// shorter than `c0` are left untouched.
pub fn delete_whitespace(lines: &[String], l0: usize, l1: usize, c0: usize) -> Vec<String> {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i < l0 || i > l1 {
                return line.clone();
            }
            let cs = cols(line);
            if c0 >= cs.len() {
                return line.clone();
            }
            let mut end = c0;
            while end < cs.len() && (cs[end] == ' ' || cs[end] == '\t') {
                end += 1;
            }
            let mut out: String = cs[..c0].iter().collect();
            out.extend(cs[end..].iter());
            out
        })
        .collect()
}

/// Insert `rect` with its top-left corner at (`line`, `col`): `rect[i]` goes into
/// line `line + i`, padding short lines with spaces up to `col`. Lines beyond
/// the buffer are appended.
pub fn yank(lines: &[String], line: usize, col: usize, rect: &[String]) -> Vec<String> {
    let mut out = lines.to_vec();
    for (i, piece) in rect.iter().enumerate() {
        let target = line + i;
        if target >= out.len() {
            out.resize(target + 1, String::new());
        }
        let mut cs = cols(&out[target]);
        if cs.len() < col {
            cs.resize(col, ' ');
        }
        let insert: Vec<char> = piece.chars().collect();
        cs.splice(col..col, insert);
        out[target] = cs.into_iter().collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Vec<String> {
        vec!["abcdef".into(), "ghijkl".into(), "mnopqr".into()]
    }

    #[test]
    fn extract_takes_column_span() {
        // columns [1,4) over lines 0..=2
        assert_eq!(extract(&grid(), 0, 2, 1, 4), vec!["bcd", "hij", "nop"]);
    }

    #[test]
    fn delete_removes_the_block() {
        assert_eq!(delete(&grid(), 0, 2, 1, 4), vec!["aef", "gkl", "mqr"]);
    }

    #[test]
    fn clear_blanks_and_pads_short_lines() {
        let mut g = grid();
        g.push("xy".into()); // short line
        let out = clear(&g, 0, 3, 1, 4);
        assert_eq!(out[0], "a   ef");
        assert_eq!(out[3], "x   "); // padded then blanked
    }

    #[test]
    fn yank_inserts_at_corner_padding_short_lines() {
        let lines = vec!["ab".into(), "c".into()];
        let rect = vec!["XX".into(), "YY".into()];
        // insert at line 0, col 2 (end of "ab"); "c" padded to col 2
        let out = yank(&lines, 0, 2, &rect);
        assert_eq!(out, vec!["abXX", "c YY"]);
    }

    #[test]
    fn extract_handles_swapped_columns() {
        assert_eq!(extract(&grid(), 0, 0, 4, 1), vec!["bcd"]);
    }

    #[test]
    fn open_shifts_text_right_by_rect_width() {
        // columns [1,4) => insert 3 spaces at col 1 on each line
        assert_eq!(
            open(&grid(), 0, 2, 1, 4),
            vec!["a   bcdef", "g   hijkl", "m   nopqr"]
        );
    }

    #[test]
    fn open_pads_short_lines_to_left_column() {
        let lines = vec!["abcdef".into(), "xy".into()];
        let out = open(&lines, 0, 1, 4, 6); // width 2 at col 4
        assert_eq!(out[0], "abcd  ef");
        assert_eq!(out[1], "xy    "); // padded to col 4 then 2 spaces
    }

    #[test]
    fn delete_whitespace_removes_run_from_left_column() {
        let lines = vec!["a   bc".into(), "d\t e".into(), "fg".into()];
        // left column 1: line0 drops "   ", line1 drops "\t ", line2 unchanged (col 1 = 'g')
        let out = delete_whitespace(&lines, 0, 2, 1);
        assert_eq!(out, vec!["abc", "de", "fg"]);
    }
}

/// Emacs `rectangle--default-line-number-format` (rect.el:597): right-align the
/// numbers to the width of the largest one, then a space — so a run of numbers
/// forms a column.
pub fn default_number_format(line_count: usize, start_at: i64) -> String {
    let widest = (line_count as i64 + start_at).to_string().len();
    format!("%{widest}d ")
}

/// Render one number through a `format`-style template, supporting the shapes
/// `rectangle-number-lines` can be given: `%d`, `%Nd` (right-aligned to N) and
/// `%0Nd` (zero-padded). Text around the directive is literal, and a template
/// with no directive is inserted as-is on every line, which is what `format`
/// does with it.
pub fn render_number(format: &str, n: i64) -> String {
    let Some(pos) = format.find('%') else {
        return format.to_string();
    };
    let rest = &format[pos + 1..];
    let zero = rest.starts_with('0');
    let digits: String = rest
        .chars()
        .skip(usize::from(zero))
        .take_while(char::is_ascii_digit)
        .collect();
    let after = pos + 1 + usize::from(zero) + digits.len();
    if format[after..].starts_with('d') {
        let width: usize = digits.parse().unwrap_or(0);
        let body = n.to_string();
        let pad = width.saturating_sub(body.len());
        let padded = if zero && n >= 0 {
            format!("{}{body}", "0".repeat(pad))
        } else {
            format!("{}{body}", " ".repeat(pad))
        };
        format!("{}{padded}{}", &format[..pos], &format[after + 1..])
    } else {
        format.to_string()
    }
}

/// Emacs `rectangle-number-lines` (rect.el:604): insert an incrementing number
/// at column `c0` of each line in `[l0, l1]`.
///
/// `start_at` is the first number. `format` defaults to
/// [`default_number_format`]. A line shorter than `c0` is padded out to it
/// first — Emacs's `move-to-column START t`, the `t` being what makes the
/// numbers line up even past the end of a short line.
pub fn number_lines(
    lines: &[String],
    l0: usize,
    l1: usize,
    c0: usize,
    start_at: i64,
    format: Option<&str>,
) -> Vec<String> {
    let l1 = l1.min(lines.len().saturating_sub(1));
    let owned;
    let format = match format {
        Some(f) => f,
        None => {
            owned = default_number_format(l1.saturating_sub(l0) + 1, start_at);
            &owned
        }
    };

    let mut out = lines.to_vec();
    for (counter, line) in (start_at..).zip(out.iter_mut().take(l1 + 1).skip(l0)) {
        let mut chars = cols(line);
        if chars.len() < c0 {
            chars.resize(c0, ' ');
        }
        let text = render_number(format, counter);
        let head: String = chars[..c0].iter().collect();
        let tail: String = chars[c0..].iter().collect();
        *line = format!("{head}{text}{tail}");
    }
    out
}

#[cfg(test)]
mod number_tests {
    use super::*;

    fn grid(s: &str) -> Vec<String> {
        s.split('\n').map(str::to_string).collect()
    }

    /// Each expectation is what Emacs itself produced for the same input
    /// (`emacs --batch`, `rectangle-number-lines`).
    #[test]
    fn matches_emacs_output() {
        // col 1, counting from 1: "a1 aa" / "b2 bb" / "c3 cc".
        let out = number_lines(&grid("aaa\nbbb\nccc"), 0, 2, 1, 1, None);
        assert_eq!(out, grid("a1 aa\nb2 bb\nc3 cc"));

        // col 2, counting from 8: the width comes from the largest number
        // (3 lines + 8 = 11, so two columns), hence " 8 ", " 9 ", "10 ".
        let out = number_lines(&grid("aaa\nbbb\nccc"), 0, 2, 2, 8, None);
        assert_eq!(out, grid("aa 8 a\nbb 9 b\ncc10 c"));
    }

    /// A line too short to reach the column is padded out to it first, so the
    /// numbers still line up — Emacs's `move-to-column START t`.
    #[test]
    fn short_lines_are_padded_to_the_column() {
        let out = number_lines(&grid("x\n\nyyy"), 0, 2, 1, 1, None);
        assert_eq!(out, grid("x1 \n 2 \ny3 yy"));
    }

    /// The format directive shapes the command can be given.
    #[test]
    fn format_directives() {
        assert_eq!(render_number("%d", 7), "7");
        assert_eq!(render_number("%3d ", 7), "  7 ");
        assert_eq!(render_number("%03d", 7), "007");
        assert_eq!(render_number("[%d] ", 42), "[42] ");
        // No directive: the template is the text, on every line.
        assert_eq!(render_number("- ", 42), "- ");
    }

    /// The default width counts the largest number that will be printed, not
    /// the line count (rect.el:597).
    #[test]
    fn default_format_widens_for_the_largest_number() {
        assert_eq!(default_number_format(3, 1), "%1d ");
        assert_eq!(default_number_format(3, 8), "%2d ");
        assert_eq!(default_number_format(95, 5), "%3d ");
    }
}
