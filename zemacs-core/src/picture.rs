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
        self.grid.get(r).and_then(|row| row.get(c)).copied().unwrap_or(' ')
    }

    /// Set the drawing direction (Emacs `picture-set-motion` family).
    pub fn set_dir(&mut self, dir: Dir) {
        self.dir = dir;
    }

    /// Move the cursor to `(r, c)`, clamped inside the grid.
    pub fn move_to(&mut self, r: usize, c: usize) {
        self.cursor = (
            r.min(self.height() - 1),
            c.min(self.width() - 1),
        );
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
    fn normalize(&self, r0: usize, c0: usize, r1: usize, c1: usize) -> (usize, usize, usize, usize) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(c.to_string(), "hi\nyo\n", "trailing blank cells and rows trim to empty lines");
    }

    #[test]
    fn move_to_clamps_inside_the_grid() {
        let mut c = Canvas::new(4, 2);
        c.move_to(99, 99);
        assert_eq!(c.cursor(), (1, 3), "row/col clamp to the last cell");
    }
}
