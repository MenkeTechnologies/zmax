//! Nonogram — a small terminal Picross/nonogram for zmax.
//!
//! Reconstruct the hidden picture. Each row and column carries a list of clue
//! numbers giving the lengths of the consecutive filled runs in that line; fill
//! the cells that match. Move the cursor with the arrows or `hjkl`, `SPC`
//! toggles a filled cell, `x`/`f` toggles a "known empty" mark, `n` picks a new
//! puzzle, `c` clears wrong fills and `q`/`Esc` quits. Like Minesweeper this one
//! is turn-based — nothing animates, the board only changes on a key. The clue
//! derivation and the solved check are pure and unit-tested; the puzzle is one
//! of a few embedded bitmaps chosen by a small LCG so a seed is reproducible.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const N: usize = 10;

/// The embedded solution bitmaps. `#` is a filled cell, anything else empty.
/// Each is `N` rows of `N` columns.
const PUZZLES: &[&[&str]] = &[
    // A heart.
    &[
        ".##....##.",
        "####..####",
        "##########",
        "##########",
        "##########",
        ".########.",
        "..######..",
        "...####...",
        "....##....",
        "..........",
    ],
    // A smiley face.
    &[
        "..######..",
        ".########.",
        "##.####.##",
        "##.####.##",
        "##########",
        "##.####.##",
        "###....###",
        "##.####.##",
        ".########.",
        "..######..",
    ],
    // An arrow pointing up.
    &[
        "....##....",
        "...####...",
        "..######..",
        ".########.",
        "##########",
        "....##....",
        "....##....",
        "....##....",
        "....##....",
        "....##....",
    ],
];

/// What the player has done to a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    Empty,
    Filled,
    Marked,
}

/// The pure nonogram board. No I/O, no timing — unit-tested. Which embedded
/// puzzle is used is chosen with the same LCG the other games use, so
/// `Game::new(seed)` is deterministic.
#[derive(Clone)]
pub struct Game {
    /// The hidden solution, `true` where a cell is filled, indexed `idx(r, c)`.
    solution: Vec<bool>,
    /// The player's marks on each cell, indexed `idx(r, c)`.
    cells: Vec<Cell>,
    /// Cursor as `(row, col)`.
    cursor: (usize, usize),
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            solution: vec![false; N * N],
            cells: vec![Cell::Empty; N * N],
            cursor: (0, 0),
            rng: seed | 1,
        };
        let pick = (g.rand() as usize) % PUZZLES.len();
        g.load(PUZZLES[pick]);
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Decode a `#`/`.` bitmap into the solution grid.
    fn load(&mut self, art: &[&str]) {
        for (r, line) in art.iter().enumerate().take(N) {
            for (c, ch) in line.chars().enumerate().take(N) {
                self.solution[idx(r, c)] = ch == '#';
            }
        }
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// The clue numbers for row `r`, top-to-bottom left-to-right.
    pub fn row_clues(&self, r: usize) -> Vec<u32> {
        let line: Vec<bool> = (0..N).map(|c| self.solution[idx(r, c)]).collect();
        line_clues(&line)
    }

    /// The clue numbers for column `c`.
    pub fn col_clues(&self, c: usize) -> Vec<u32> {
        let line: Vec<bool> = (0..N).map(|r| self.solution[idx(r, c)]).collect();
        line_clues(&line)
    }

    /// Move the cursor by `(dr, dc)`, clamped to the board.
    pub fn move_cursor(&mut self, dr: i32, dc: i32) {
        let r = (self.cursor.0 as i32 + dr).clamp(0, N as i32 - 1) as usize;
        let c = (self.cursor.1 as i32 + dc).clamp(0, N as i32 - 1) as usize;
        self.cursor = (r, c);
    }

    /// Toggle a filled cell under the cursor: Filled ⇄ Empty. A marked cell
    /// becomes Filled.
    pub fn toggle_fill(&mut self) {
        let i = idx(self.cursor.0, self.cursor.1);
        self.cells[i] = match self.cells[i] {
            Cell::Filled => Cell::Empty,
            _ => Cell::Filled,
        };
    }

    /// Toggle a "known empty" mark under the cursor: Marked ⇄ Empty. A filled
    /// cell becomes Marked.
    pub fn toggle_mark(&mut self) {
        let i = idx(self.cursor.0, self.cursor.1);
        self.cells[i] = match self.cells[i] {
            Cell::Marked => Cell::Empty,
            _ => Cell::Marked,
        };
    }

    /// Clear every filled cell that isn't actually part of the solution.
    pub fn clear_mistakes(&mut self) {
        for i in 0..self.cells.len() {
            if self.cells[i] == Cell::Filled && !self.solution[i] {
                self.cells[i] = Cell::Empty;
            }
        }
    }

    /// True when exactly the solution's filled cells are Filled and no others
    /// (marks are ignored).
    pub fn is_solved(&self) -> bool {
        for i in 0..self.solution.len() {
            let filled = self.cells[i] == Cell::Filled;
            if filled != self.solution[i] {
                return false;
            }
        }
        true
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

fn idx(r: usize, c: usize) -> usize {
    r * N + c
}

/// Run-length encode the `true` runs of a line into clue numbers. An all-`false`
/// line yields `[0]` so every line has at least one clue to print.
pub fn line_clues(cells: &[bool]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut run = 0u32;
    for &b in cells {
        if b {
            run += 1;
        } else if run > 0 {
            out.push(run);
            run = 0;
        }
    }
    if run > 0 {
        out.push(run);
    }
    if out.is_empty() {
        out.push(0);
    }
    out
}

/// The interactive Nonogram overlay.
pub struct Nonogram {
    game: Game,
    seed: u64,
}

impl Nonogram {
    pub fn new() -> Self {
        Nonogram {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Nonogram {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Nonogram {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!(Left) | key!('h') => self.game.move_cursor(0, -1),
            key!(Right) | key!('l') => self.game.move_cursor(0, 1),
            key!(Up) | key!('k') => self.game.move_cursor(-1, 0),
            key!(Down) | key!('j') => self.game.move_cursor(1, 0),
            key!(' ') => self.game.toggle_fill(),
            key!('x') | key!('f') => self.game.toggle_mark(),
            key!('c') => self.game.clear_mistakes(),
            key!('n') => self.restart(),
            _ => {}
        }
        zmax_event::request_redraw();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let clue_style = theme.get("ui.text");
        let active_clue_style = theme.get("ui.text.focus");
        let filled_style = theme.get("function");
        let mark_style = theme.get("ui.linenr");
        let cursor_style = theme.get("ui.selection");
        let win_style = theme.get("warning");

        surface.clear_with(area, bg);

        // Clue gutters: rows need up to N/2 numbers on the left (2 cols each),
        // columns need up to N/2 rows of clues above. Cells are two columns wide.
        let row_clue_cols: usize = 2 * N.div_ceil(2);
        let col_clue_rows: usize = N.div_ceil(2);
        let need_w = (row_clue_cols + N * 2 + 4) as u16;
        let need_h = (col_clue_rows + N + 4) as u16;
        if area.width < need_w || area.height < need_h {
            return;
        }

        let (cr, cc) = self.game.cursor();

        // Header.
        let solved = self.game.is_solved();
        let status = if solved { "Solved!" } else { "Playing" };
        surface.set_string(
            area.x + 2,
            area.y,
            &format!("Nonogram  {}x{}  [{}]", N, N, status),
            header_style,
        );

        // Origin of the grid itself (past the clue gutters).
        let grid_x = area.x + 2 + row_clue_cols as u16;
        let grid_y = area.y + 2 + col_clue_rows as u16;

        // Column clues, written upward so the last number sits just above the grid.
        for c in 0..N {
            let clues = self.game.col_clues(c);
            let x = grid_x + (c as u16) * 2;
            let style = if c == cc {
                active_clue_style
            } else {
                clue_style
            };
            for (k, v) in clues.iter().rev().enumerate() {
                let y = grid_y - 1 - k as u16;
                surface.set_string(x, y, &format!("{}", v), style);
            }
        }

        // Row clues, right-aligned into the left gutter.
        for r in 0..N {
            let clues = self.game.row_clues(r);
            let text = clues
                .iter()
                .map(|v| format!("{}", v))
                .collect::<Vec<_>>()
                .join(" ");
            let y = grid_y + r as u16;
            let style = if r == cr {
                active_clue_style
            } else {
                clue_style
            };
            let x = grid_x.saturating_sub(text.len() as u16 + 1);
            surface.set_string(x, y, &text, style);
        }

        // The grid.
        for r in 0..N {
            for c in 0..N {
                let i = idx(r, c);
                let (glyph, mut style) = match self.game.cells[i] {
                    Cell::Filled => ("█", filled_style),
                    Cell::Marked => ("✗", mark_style),
                    Cell::Empty => ("·", mark_style),
                };
                if (r, c) == (cr, cc) {
                    style = cursor_style;
                }
                let x = grid_x + (c as u16) * 2;
                let y = grid_y + r as u16;
                surface.set_string(x, y, glyph, style);
            }
        }

        // Footer.
        let sy = grid_y + N as u16 + 1;
        surface.set_string(
            area.x + 2,
            sy,
            "hjkl/arrows move · SPC fill · x/f mark · c clear · n new · q quit",
            text_style,
        );
        if solved {
            surface.set_string(area.x + 2, sy + 1, "You solved the picture!", win_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `line_clues` run-length-encodes the filled runs of a line.
    #[test]
    fn line_clues_encodes_runs() {
        let line = [true, true, false, true, true, true, false];
        assert_eq!(line_clues(&line), vec![2, 3]);
    }

    /// An all-empty line still yields a single `0` clue.
    #[test]
    fn line_clues_of_empty_is_zero() {
        let line = [false; N];
        assert_eq!(line_clues(&line), vec![0]);
    }

    /// A fully filled line yields one clue equal to its length.
    #[test]
    fn line_clues_of_full_is_length() {
        let line = [true; N];
        assert_eq!(line_clues(&line), vec![N as u32]);
    }

    /// A fresh board (nothing filled) is not solved as long as the puzzle has
    /// filled cells.
    #[test]
    fn fresh_board_is_not_solved() {
        let g = Game::new(1);
        assert!(!g.is_solved());
    }

    /// Filling exactly the solution's cells — and nothing else — solves it.
    #[test]
    fn filling_the_solution_solves() {
        let mut g = Game::new(1);
        for i in 0..g.solution.len() {
            if g.solution[i] {
                g.cells[i] = Cell::Filled;
            }
        }
        assert!(g.is_solved());
    }

    /// One extra wrong fill breaks the solved check.
    #[test]
    fn an_extra_fill_breaks_the_solution() {
        let mut g = Game::new(1);
        for i in 0..g.solution.len() {
            if g.solution[i] {
                g.cells[i] = Cell::Filled;
            }
        }
        // Find an empty solution cell and wrongly fill it.
        let wrong = (0..g.solution.len()).find(|&i| !g.solution[i]).unwrap();
        g.cells[wrong] = Cell::Filled;
        assert!(!g.is_solved());
    }
}
