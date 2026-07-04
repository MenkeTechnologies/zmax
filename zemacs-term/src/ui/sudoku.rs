//! Sudoku — a small terminal sudoku for zemacs.
//!
//! Fill the 9×9 grid so every row, column and 3×3 box holds the digits 1–9.
//! Move the cursor with the arrows or `hjkl`, type `1`–`9` to place a digit,
//! `0`/`SPC`/`x` to clear one, `n` deals a fresh puzzle and `q`/`Esc` quits.
//! Like Minesweeper this game is turn-based: nothing animates, so there is no
//! frame loop — the board only changes in response to a key. The board logic is
//! pure and unit-tested; a puzzle is carved from one embedded solution by an LCG
//! so a given seed is reproducible.

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// One fully-solved, valid 9×9 grid. Puzzles are carved out of it by blanking
/// cells, so the completed board a player is aiming for is always exactly this.
const SOLUTION: [[u8; 9]; 9] = [
    [5, 3, 4, 6, 7, 8, 9, 1, 2],
    [6, 7, 2, 1, 9, 5, 3, 4, 8],
    [1, 9, 8, 3, 4, 2, 5, 6, 7],
    [8, 5, 9, 7, 6, 1, 4, 2, 3],
    [4, 2, 6, 8, 5, 3, 7, 9, 1],
    [7, 1, 3, 9, 2, 4, 8, 5, 6],
    [9, 6, 1, 5, 3, 7, 2, 8, 4],
    [2, 8, 7, 4, 1, 9, 6, 3, 5],
    [3, 4, 5, 2, 8, 6, 1, 7, 9],
];

/// How many cells to blank out when dealing a puzzle.
const BLANKS: usize = 45;

/// The pure sudoku board. No I/O, no timing — unit-tested. Cells are carved out
/// with the same LCG the other games use, so `Game::new(seed)` is deterministic.
#[derive(Clone)]
pub struct Game {
    /// `true` for the pre-filled clues that the player may not change.
    given: [[bool; 9]; 9],
    /// The live board; `0` marks an empty cell.
    board: [[u8; 9]; 9],
    /// Cursor as `(row, col)`.
    cursor: (usize, usize),
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            given: [[true; 9]; 9],
            board: SOLUTION,
            cursor: (0, 0),
            rng: seed | 1,
        };
        g.carve();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Blank out `BLANKS` cells with rejection sampling; the removed cells stop
    /// being givens, the rest stay as immutable clues. `BLANKS < 81`, so there
    /// are always cells left to remove and the loop terminates.
    fn carve(&mut self) {
        let mut removed = 0;
        while removed < BLANKS {
            let r = (self.rand() % 9) as usize;
            let c = (self.rand() % 9) as usize;
            if self.given[r][c] {
                self.given[r][c] = false;
                self.board[r][c] = 0;
                removed += 1;
            }
        }
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// Move the cursor by `(dr, dc)`, clamped to the board.
    pub fn move_cursor(&mut self, dr: i32, dc: i32) {
        let r = (self.cursor.0 as i32 + dr).clamp(0, 8) as usize;
        let c = (self.cursor.1 as i32 + dc).clamp(0, 8) as usize;
        self.cursor = (r, c);
    }

    /// Set `(r, c)` to `v` (`0` clears). Givens are immutable and out-of-range
    /// digits are ignored, so both are silently rejected.
    pub fn set(&mut self, r: usize, c: usize, v: u8) {
        if self.given[r][c] || v > 9 {
            return;
        }
        self.board[r][c] = v;
    }

    /// Set the cell under the cursor (the interactive `1`–`9`/`0` action).
    pub fn set_cursor(&mut self, v: u8) {
        let (r, c) = self.cursor;
        self.set(r, c, v);
    }

    /// `true` when the digit at `(r, c)` duplicates another in its row, column
    /// or 3×3 box. An empty cell never conflicts.
    pub fn conflicts(&self, r: usize, c: usize) -> bool {
        let v = self.board[r][c];
        if v == 0 {
            return false;
        }
        for cc in 0..9 {
            if cc != c && self.board[r][cc] == v {
                return true;
            }
        }
        for rr in 0..9 {
            if rr != r && self.board[rr][c] == v {
                return true;
            }
        }
        let br = (r / 3) * 3;
        let bc = (c / 3) * 3;
        for rr in br..br + 3 {
            for cc in bc..bc + 3 {
                if (rr, cc) != (r, c) && self.board[rr][cc] == v {
                    return true;
                }
            }
        }
        false
    }

    /// The puzzle is solved when every cell is filled and nothing conflicts.
    pub fn is_solved(&self) -> bool {
        for r in 0..9 {
            for c in 0..9 {
                if self.board[r][c] == 0 || self.conflicts(r, c) {
                    return false;
                }
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

/// The interactive Sudoku overlay.
pub struct Sudoku {
    game: Game,
    seed: u64,
}

impl Sudoku {
    pub fn new() -> Self {
        Sudoku {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Sudoku {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Sudoku {
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
            key!('1') => self.game.set_cursor(1),
            key!('2') => self.game.set_cursor(2),
            key!('3') => self.game.set_cursor(3),
            key!('4') => self.game.set_cursor(4),
            key!('5') => self.game.set_cursor(5),
            key!('6') => self.game.set_cursor(6),
            key!('7') => self.game.set_cursor(7),
            key!('8') => self.game.set_cursor(8),
            key!('9') => self.game.set_cursor(9),
            key!('0') | key!(' ') | key!('x') => self.game.set_cursor(0),
            key!('n') => self.restart(),
            _ => {}
        }
        zemacs_event::request_redraw();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let rule_style = theme.get("ui.linenr");
        let cursor_style = theme.get("ui.selection");
        let given_style = theme.get("ui.text.focus");
        let entry_style = theme.get("function");
        let conflict_style = theme.get("error");
        let empty_style = theme.get("ui.linenr");
        let win_style = theme.get("warning");

        surface.clear_with(area, bg);
        // Cells are two columns wide with a two-column gap between 3×3 boxes.
        if area.width < 26 || area.height < 16 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        // A cell's top-left, offset by an extra step at each box boundary.
        let cell_x = |c: usize| ox + (c as u16) * 2 + (c as u16 / 3) * 2;
        let cell_y = |r: usize| oy + r as u16 + (r as u16 / 3);

        let status = if self.game.is_solved() {
            "Solved!"
        } else {
            "Playing"
        };
        let status_style = if self.game.is_solved() {
            win_style
        } else {
            header_style
        };
        surface.set_string(ox, area.y, "Sudoku", header_style);
        surface.set_string(ox + 8, area.y, status, status_style);

        // Box separators: horizontal rules first, then the vertical bars (they
        // never share a cell), then the crossings on top.
        let hrule: String = "─".repeat(21);
        for &gr in &[oy + 3, oy + 7] {
            surface.set_string(ox, gr, &hrule, rule_style);
        }
        for r in 0..9 {
            let y = cell_y(r);
            surface.set_string(ox + 6, y, "│", rule_style);
            surface.set_string(ox + 14, y, "│", rule_style);
        }
        for &gx in &[ox + 6, ox + 14] {
            for &gy in &[oy + 3, oy + 7] {
                surface.set_string(gx, gy, "┼", rule_style);
            }
        }

        for r in 0..9 {
            for c in 0..9 {
                let v = self.game.board[r][c];
                let glyph = if v == 0 {
                    "·".to_string()
                } else {
                    v.to_string()
                };
                let mut style = if v == 0 {
                    empty_style
                } else if self.game.given[r][c] {
                    given_style
                } else {
                    entry_style
                };
                if v != 0 && self.game.conflicts(r, c) {
                    style = conflict_style;
                }
                if (r, c) == self.game.cursor() {
                    style = cursor_style;
                }
                surface.set_string(cell_x(c), cell_y(r), &glyph, style);
            }
        }

        let sy = oy + 12;
        surface.set_string(
            ox,
            sy,
            "hjkl move · 1-9 set · 0/x clear · n new · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An all-empty board with no givens, ready for hand placement.
    fn empty() -> Game {
        Game {
            given: [[false; 9]; 9],
            board: [[0; 9]; 9],
            cursor: (0, 0),
            rng: 1,
        }
    }

    #[test]
    fn givens_are_immutable() {
        let mut g = Game::new(1);
        let mut clue = None;
        'find: for r in 0..9 {
            for c in 0..9 {
                if g.given[r][c] {
                    clue = Some((r, c));
                    break 'find;
                }
            }
        }
        let (r, c) = clue.expect("a fresh puzzle always keeps some clues");
        let orig = g.board[r][c];
        g.set(r, c, if orig == 1 { 2 } else { 1 });
        assert_eq!(g.board[r][c], orig, "a given cell can't be overwritten");
    }

    #[test]
    fn row_conflict_is_detected() {
        let mut g = empty();
        g.set(0, 0, 5);
        g.set(0, 6, 5);
        assert!(g.conflicts(0, 0));
        assert!(g.conflicts(0, 6));
    }

    #[test]
    fn column_conflict_is_detected() {
        let mut g = empty();
        g.set(0, 3, 7);
        g.set(5, 3, 7);
        assert!(g.conflicts(0, 3));
        assert!(g.conflicts(5, 3));
    }

    #[test]
    fn box_conflict_is_detected() {
        let mut g = empty();
        // Same 3×3 box, different row and column, so only the box rule catches it.
        g.set(0, 0, 3);
        g.set(2, 2, 3);
        assert!(g.conflicts(0, 0));
        assert!(g.conflicts(2, 2));
    }

    #[test]
    fn clearing_a_cell_works() {
        let mut g = empty();
        g.set(3, 3, 4);
        assert_eq!(g.board[3][3], 4);
        g.set(3, 3, 0);
        assert_eq!(g.board[3][3], 0);
        assert!(
            !g.conflicts(3, 3),
            "a cleared cell holds nothing to conflict"
        );
    }

    #[test]
    fn a_correct_board_is_solved() {
        let mut g = Game::new(1);
        for r in 0..9 {
            for c in 0..9 {
                g.set(r, c, SOLUTION[r][c]);
            }
        }
        assert!(g.is_solved());
    }
}
