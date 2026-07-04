//! Picture — the pure 2-D ASCII drawing grid behind the zemacs port of GNU
//! Emacs `picture-mode`.
//!
//! `picture-mode` turns the buffer into a fixed grid you paint on: typing a
//! character overwrites the cell under point and then advances point one step in
//! the current *drawing direction* (one of the eight compass directions),
//! instead of the usual left-to-right insertion. This module is the substrate:
//! a bounded character grid with a cursor and a direction, plus the rectangle
//! primitives Emacs exposes as `picture-draw-rectangle` /
//! `picture-clear-rectangle`. It does no I/O and no rendering — the terminal
//! overlay in `zemacs-term/src/ui/picture.rs` drives it — so it is entirely
//! unit-tested here.

/// One of the eight drawing directions point advances in after a character is
/// typed — the `picture-movement-*` set in Emacs picture-mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Dir {
    /// The `(row, col)` step this direction advances by.
    pub fn delta(self) -> (isize, isize) {
        match self {
            Dir::N => (-1, 0),
            Dir::S => (1, 0),
            Dir::E => (0, 1),
            Dir::W => (0, -1),
            Dir::NE => (-1, 1),
            Dir::NW => (-1, -1),
            Dir::SE => (1, 1),
            Dir::SW => (1, -1),
        }
    }

    /// The opposite direction — used by `picture-motion-reverse`.
    pub fn reverse(self) -> Dir {
        match self {
            Dir::N => Dir::S,
            Dir::S => Dir::N,
            Dir::E => Dir::W,
            Dir::W => Dir::E,
            Dir::NE => Dir::SW,
            Dir::NW => Dir::SE,
            Dir::SE => Dir::NW,
            Dir::SW => Dir::NE,
        }
    }

    /// A short label for the header/indicator (`→`, `↖`, …).
    pub fn arrow(self) -> &'static str {
        match self {
            Dir::N => "↑",
            Dir::S => "↓",
            Dir::E => "→",
            Dir::W => "←",
            Dir::NE => "↗",
            Dir::NW => "↖",
            Dir::SE => "↘",
            Dir::SW => "↙",
        }
    }
}

/// A bounded character grid with a cursor and a drawing direction.
#[derive(Clone, Debug)]
pub struct Canvas {
    grid: Vec<Vec<char>>,
    cursor: (usize, usize),
    dir: Dir,
}

impl Canvas {
    /// A fresh `w`×`h` canvas of spaces, cursor at the origin heading east. Both
    /// dimensions are forced to at least 1 so the grid is never ragged/empty.
    pub fn new(w: usize, h: usize) -> Self {
        let w = w.max(1);
        let h = h.max(1);
        Canvas {
            grid: vec![vec![' '; w]; h],
            cursor: (0, 0),
            dir: Dir::E,
        }
    }

    /// Grid width (columns).
    pub fn width(&self) -> usize {
        self.grid[0].len()
    }

    /// Grid height (rows).
    pub fn height(&self) -> usize {
        self.grid.len()
    }

    /// The current cursor as `(row, col)`.
    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// The current drawing direction.
    pub fn dir(&self) -> Dir {
        self.dir
    }

    /// The character at `(r, c)`, or a space if out of bounds.
    pub fn get(&self, r: usize, c: usize) -> char {
        self.grid
            .get(r)
            .and_then(|row| row.get(c))
            .copied()
            .unwrap_or(' ')
    }

    /// Set the drawing direction (Emacs `picture-set-motion` family).
    pub fn set_dir(&mut self, dir: Dir) {
        self.dir = dir;
    }

    /// Move the cursor to `(r, c)`, clamped inside the grid.
    pub fn move_to(&mut self, r: usize, c: usize) {
        self.cursor = (r.min(self.height() - 1), c.min(self.width() - 1));
    }

    /// Advance the cursor one step in the current direction, clamped to the grid
    /// (picture-mode grows toward the edges but never past them here).
    pub fn move_step(&mut self) {
        self.step(self.dir);
    }

    fn step(&mut self, dir: Dir) {
        let (dr, dc) = dir.delta();
        let r = (self.cursor.0 as isize + dr).clamp(0, self.height() as isize - 1) as usize;
        let c = (self.cursor.1 as isize + dc).clamp(0, self.width() as isize - 1) as usize;
        self.cursor = (r, c);
    }

    /// Overwrite the cell under the cursor with `ch`, then advance one step in
    /// the drawing direction — the core of Emacs `picture-self-insert`.
    pub fn put_char(&mut self, ch: char) {
        let (r, c) = self.cursor;
        self.grid[r][c] = ch;
        self.move_step();
    }

    /// Normalize a corner pair into `(r0, c0, r1, c1)` with `r0 <= r1`,
    /// `c0 <= c1`, all clamped to the grid.
    fn normalize(
        &self,
        r0: usize,
        c0: usize,
        r1: usize,
        c1: usize,
    ) -> (usize, usize, usize, usize) {
        let h = self.height() - 1;
        let w = self.width() - 1;
        (
            r0.min(r1).min(h),
            c0.min(c1).min(w),
            r0.max(r1).min(h),
            c0.max(c1).min(w),
        )
    }

    /// Draw a box outline between the two corners using `+` for corners, `-` for
    /// horizontal edges and `|` for vertical edges — Emacs
    /// `picture-draw-rectangle`.
    pub fn draw_rectangle(&mut self, r0: usize, c0: usize, r1: usize, c1: usize) {
        let (r0, c0, r1, c1) = self.normalize(r0, c0, r1, c1);
        for c in c0..=c1 {
            self.grid[r0][c] = '-';
            self.grid[r1][c] = '-';
        }
        for r in r0..=r1 {
            self.grid[r][c0] = '|';
            self.grid[r][c1] = '|';
        }
        for &(r, c) in &[(r0, c0), (r0, c1), (r1, c0), (r1, c1)] {
            self.grid[r][c] = '+';
        }
        self.cursor = (r0, c0);
    }

    /// Blank every cell in the rectangle between the two corners — Emacs
    /// `picture-clear-rectangle`.
    pub fn clear_rectangle(&mut self, r0: usize, c0: usize, r1: usize, c1: usize) {
        let (r0, c0, r1, c1) = self.normalize(r0, c0, r1, c1);
        for row in self.grid.iter_mut().take(r1 + 1).skip(r0) {
            for cell in row.iter_mut().take(c1 + 1).skip(c0) {
                *cell = ' ';
            }
        }
    }

    /// The grid as text: rows joined by newlines, each row's trailing blanks
    /// trimmed.
    pub fn to_string(&self) -> String {
        self.grid
            .iter()
            .map(|row| {
                let s: String = row.iter().collect();
                s.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Advance a buffer `(row, col)` position `n` steps in `dir`, the geometry
/// behind picture-mode's overwrite self-insert and `picture-motion` /
/// `picture-motion-reverse`. Unlike [`Canvas`], a text buffer has no fixed
/// bounds — Emacs pads with spaces past the ends — so this only saturates at 0
/// (the top-left quadrant Emacs calls the "quarter-plane").
pub fn advance(row: usize, col: usize, dir: Dir, n: usize) -> (usize, usize) {
    let (dr, dc) = dir.delta();
    let n = n as isize;
    let r = (row as isize + dr * n).max(0) as usize;
    let c = (col as isize + dc * n).max(0) as usize;
    (r, c)
}

/// `picture-set-tab-stops`: derive tab-stop columns from the current line, one
/// at the start of every whitespace-delimited word *after* the first. Mirrors
/// picture.el's `"[^ \t][ \t]+"` scan: a stop sits at each column that begins a
/// non-blank run and is preceded by a blank. The word in column 0 (if any) is
/// intentionally omitted, matching Emacs.
pub fn set_tab_stops(line: &str) -> Vec<usize> {
    let chars: Vec<char> = line.chars().collect();
    let mut stops = Vec::new();
    for c in 1..chars.len() {
        let here_blank = chars[c] == ' ' || chars[c] == '\t';
        let prev_blank = chars[c - 1] == ' ' || chars[c - 1] == '\t';
        if !here_blank && prev_blank {
            stops.push(c);
        }
    }
    stops
}

/// `picture-tab`: the first tab stop strictly to the right of `col`, or `None`
/// if `col` is at/after the last stop.
pub fn next_tab_stop(col: usize, stops: &[usize]) -> Option<usize> {
    stops.iter().copied().find(|&s| s > col)
}

/// `picture-clear-column`: overwrite `n` cells of `line` starting at `col` with
/// spaces, in place (following text keeps its column). Short lines are padded
/// with spaces out to `col + n` first, exactly as Emacs's
/// `move-to-column`/`indent-to` dance leaves them. Point does not move, so the
/// caller keeps its column.
pub fn clear_columns(line: &str, col: usize, n: usize) -> String {
    let mut chars: Vec<char> = line.chars().collect();
    let end = col + n;
    if chars.len() < end {
        chars.resize(end, ' ');
    }
    for cell in chars.iter_mut().take(end).skip(col) {
        *cell = ' ';
    }
    chars.into_iter().collect()
}

/// `picture-yank-rectangle`: overlay `rect` onto `lines` with its top-left
/// corner at `(line, col)`, *overwriting* the cells it covers (unlike the
/// insert-and-shift [`crate` rectangle yank](crate)). Rows past the buffer's end
/// are appended; lines shorter than the target column are padded with spaces.
pub fn overlay_rectangle(
    lines: &[String],
    line: usize,
    col: usize,
    rect: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = lines.to_vec();
    for (i, piece) in rect.iter().enumerate() {
        let target = line + i;
        if target >= out.len() {
            out.resize(target + 1, String::new());
        }
        let mut chars: Vec<char> = out[target].chars().collect();
        let piece_chars: Vec<char> = piece.chars().collect();
        let needed = col + piece_chars.len();
        if chars.len() < needed {
            chars.resize(needed, ' ');
        }
        for (j, ch) in piece_chars.into_iter().enumerate() {
            chars[col + j] = ch;
        }
        out[target] = chars.into_iter().collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_saturates_at_the_origin() {
        assert_eq!(advance(2, 3, Dir::E, 1), (2, 4), "east adds a column");
        assert_eq!(advance(2, 3, Dir::W, 2), (2, 1), "west subtracts columns");
        assert_eq!(advance(0, 0, Dir::NW, 1), (0, 0), "clamped at the corner");
        assert_eq!(
            advance(1, 1, Dir::SE, 3),
            (4, 4),
            "diagonal steps scale by n"
        );
        assert_eq!(
            advance(0, 5, Dir::N, 4),
            (0, 5),
            "north clamps the row at 0"
        );
    }

    #[test]
    fn reverse_flips_each_direction() {
        assert_eq!(Dir::E.reverse(), Dir::W);
        assert_eq!(Dir::N.reverse(), Dir::S);
        assert_eq!(Dir::NE.reverse(), Dir::SW);
        assert_eq!(Dir::SE.reverse(), Dir::NW);
        // Reversing twice is the identity.
        for d in [
            Dir::N,
            Dir::S,
            Dir::E,
            Dir::W,
            Dir::NE,
            Dir::NW,
            Dir::SE,
            Dir::SW,
        ] {
            assert_eq!(d.reverse().reverse(), d);
        }
    }

    #[test]
    fn set_tab_stops_marks_word_starts_after_the_first() {
        // "  foo   bar baz": foo@2, bar@8, baz@12; the leading run has no word at 0.
        assert_eq!(set_tab_stops("  foo   bar baz"), vec![2, 8, 12]);
        // A word in column 0 is omitted; only later words become stops.
        assert_eq!(set_tab_stops("foo bar"), vec![4]);
        assert_eq!(set_tab_stops(""), Vec::<usize>::new());
        assert_eq!(set_tab_stops("solid"), Vec::<usize>::new());
    }

    #[test]
    fn next_tab_stop_finds_the_first_to_the_right() {
        let stops = [2usize, 8, 12];
        assert_eq!(next_tab_stop(0, &stops), Some(2));
        assert_eq!(next_tab_stop(2, &stops), Some(8), "strictly to the right");
        assert_eq!(next_tab_stop(11, &stops), Some(12));
        assert_eq!(next_tab_stop(12, &stops), None);
    }

    #[test]
    fn clear_columns_overwrites_in_place() {
        assert_eq!(clear_columns("abcdef", 1, 2), "a  def", "b,c become spaces");
        assert_eq!(
            clear_columns("ab", 1, 2),
            "a  ",
            "short line padded then blanked"
        );
        assert_eq!(clear_columns("xyz", 0, 1), " yz");
    }

    #[test]
    fn overlay_rectangle_overwrites_not_inserts() {
        let lines = vec!["abcdef".to_string()];
        assert_eq!(
            overlay_rectangle(&lines, 0, 2, &["XX".to_string()]),
            vec!["abXXef"]
        );

        // Multi-row, padding a short second line and appending a third.
        let lines = vec!["abcdef".to_string(), "gh".to_string()];
        let rect = vec!["11".to_string(), "22".to_string(), "33".to_string()];
        assert_eq!(
            overlay_rectangle(&lines, 0, 3, &rect),
            vec!["abc11f", "gh 22", "   33"],
        );
    }

    #[test]
    fn put_char_advances_east_by_default() {
        let mut c = Canvas::new(5, 3);
        c.put_char('a');
        assert_eq!(c.get(0, 0), 'a');
        assert_eq!(c.cursor(), (0, 1), "east advance moves one column right");
        c.put_char('b');
        assert_eq!(c.get(0, 1), 'b');
        assert_eq!(c.cursor(), (0, 2));
    }

    #[test]
    fn changing_direction_changes_advance() {
        let mut c = Canvas::new(5, 5);
        c.set_dir(Dir::S);
        c.put_char('x');
        assert_eq!(c.get(0, 0), 'x');
        assert_eq!(c.cursor(), (1, 0), "south advance moves one row down");
        c.set_dir(Dir::W);
        c.move_to(2, 3);
        c.put_char('y');
        assert_eq!(c.cursor(), (2, 2), "west advance moves one column left");
    }

    #[test]
    fn diagonal_advance_ne_and_sw() {
        let mut c = Canvas::new(5, 5);
        c.move_to(4, 0);
        c.set_dir(Dir::NE);
        c.put_char('/');
        assert_eq!(c.get(4, 0), '/');
        assert_eq!(c.cursor(), (3, 1), "NE advances up-and-right");

        c.move_to(0, 4);
        c.set_dir(Dir::SW);
        c.put_char('\\');
        assert_eq!(c.get(0, 4), '\\');
        assert_eq!(c.cursor(), (1, 3), "SW advances down-and-left");
    }

    #[test]
    fn advance_clamps_at_the_edges() {
        let mut c = Canvas::new(3, 3);
        c.move_to(0, 0);
        c.set_dir(Dir::NW);
        c.put_char('*');
        assert_eq!(c.cursor(), (0, 0), "clamped in the top-left corner");
        c.move_to(2, 2);
        c.set_dir(Dir::SE);
        c.put_char('*');
        assert_eq!(c.cursor(), (2, 2), "clamped in the bottom-right corner");
    }

    #[test]
    fn draw_rectangle_produces_border_chars() {
        let mut c = Canvas::new(6, 5);
        c.draw_rectangle(1, 1, 3, 4);
        // Corners.
        assert_eq!(c.get(1, 1), '+');
        assert_eq!(c.get(1, 4), '+');
        assert_eq!(c.get(3, 1), '+');
        assert_eq!(c.get(3, 4), '+');
        // Horizontal + vertical edges.
        assert_eq!(c.get(1, 2), '-');
        assert_eq!(c.get(3, 3), '-');
        assert_eq!(c.get(2, 1), '|');
        assert_eq!(c.get(2, 4), '|');
        // Interior stays blank.
        assert_eq!(c.get(2, 2), ' ');
    }

    #[test]
    fn draw_rectangle_normalizes_reversed_corners() {
        let mut c = Canvas::new(6, 5);
        c.draw_rectangle(3, 4, 1, 1); // bottom-right given first
        assert_eq!(c.get(1, 1), '+');
        assert_eq!(c.get(3, 4), '+');
    }

    #[test]
    fn clear_rectangle_blanks_a_region() {
        let mut c = Canvas::new(5, 5);
        // Fill a block, then wipe part of it.
        for r in 0..3 {
            for col in 0..3 {
                c.move_to(r, col);
                c.set_dir(Dir::E);
                c.put_char('#');
            }
        }
        c.clear_rectangle(0, 0, 1, 1);
        assert_eq!(c.get(0, 0), ' ');
        assert_eq!(c.get(1, 1), ' ');
        assert_eq!(c.get(2, 2), '#', "cells outside the region survive");
    }

    #[test]
    fn to_string_round_trips_a_small_drawing() {
        let mut c = Canvas::new(6, 3);
        c.move_to(0, 0);
        c.set_dir(Dir::E);
        for ch in "hi".chars() {
            c.put_char(ch);
        }
        c.move_to(1, 0);
        for ch in "yo".chars() {
            c.put_char(ch);
        }
        assert_eq!(
            c.to_string(),
            "hi\nyo\n",
            "trailing blank cells and rows trim to empty lines"
        );
    }

    #[test]
    fn move_to_clamps_inside_the_grid() {
        let mut c = Canvas::new(4, 2);
        c.move_to(99, 99);
        assert_eq!(c.cursor(), (1, 3), "row/col clamp to the last cell");
    }
}
