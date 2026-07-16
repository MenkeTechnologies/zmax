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
