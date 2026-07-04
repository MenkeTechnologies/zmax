//! Gomoku — the zemacs port of GNU Emacs `gomoku`, five-in-a-row vs the computer.
//!
//! You play `X` against the computer's `O` on a square board; the first to make
//! five in a row (horizontal, vertical or diagonal) wins. Move the cursor with
//! the arrows or `hjkl`, place a stone with `SPC`/`RET`; `r` restarts, `q`/`Esc`
//! quits. The board, win test and the computer's scoring heuristic are pure and
//! unit-tested (keys parse into a `gomoku` keymap mode by
//! `scripts/gen_port_report.py`).

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const SIZE: usize = 13;
const WIN: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stone {
    Empty,
    X, // human
    O, // computer
}

/// The pure board: placement, five-in-a-row detection, and the heuristic the
/// computer uses to choose a move. No I/O — unit-tested.
#[derive(Clone)]
pub struct Board {
    cells: [[Stone; SIZE]; SIZE],
}

const DIRS: [(isize, isize); 4] = [(0, 1), (1, 0), (1, 1), (1, -1)];

impl Board {
    pub fn new() -> Self {
        Board {
            cells: [[Stone::Empty; SIZE]; SIZE],
        }
    }

    pub fn get(&self, r: usize, c: usize) -> Stone {
        self.cells[r][c]
    }

    pub fn place(&mut self, r: usize, c: usize, s: Stone) -> bool {
        if self.cells[r][c] == Stone::Empty {
            self.cells[r][c] = s;
            true
        } else {
            false
        }
    }

    pub fn full(&self) -> bool {
        self.cells.iter().flatten().all(|&s| s != Stone::Empty)
    }

    /// The longest run of `s` through `(r, c)` along any direction.
    fn run_through(&self, r: usize, c: usize, s: Stone) -> usize {
        let mut best = 0;
        for (dr, dc) in DIRS {
            let mut len = 1;
            for sign in [1isize, -1] {
                let (mut rr, mut cc) = (r as isize, c as isize);
                loop {
                    rr += dr * sign;
                    cc += dc * sign;
                    if rr < 0 || rr >= SIZE as isize || cc < 0 || cc >= SIZE as isize {
                        break;
                    }
                    if self.cells[rr as usize][cc as usize] == s {
                        len += 1;
                    } else {
                        break;
                    }
                }
            }
            best = best.max(len);
        }
        best
    }

    /// Whether `s` has five (or more) in a row anywhere.
    pub fn wins(&self, s: Stone) -> bool {
        for r in 0..SIZE {
            for c in 0..SIZE {
                if self.cells[r][c] == s && self.run_through(r, c, s) >= WIN {
                    return true;
                }
            }
        }
        false
    }

    /// Heuristic value of playing `s` at empty `(r, c)`: rewards long runs for
    /// `s` and, weighted slightly higher, blocking the opponent's runs, with a
    /// small pull toward the centre. Pure — drives `best_move`.
    fn score_at(&self, r: usize, c: usize, s: Stone) -> i64 {
        let opp = match s {
            Stone::X => Stone::O,
            Stone::O => Stone::X,
            Stone::Empty => Stone::Empty,
        };
        let mut b = self.clone();
        b.cells[r][c] = s;
        let mine = b.run_through(r, c, s);
        let mut b2 = self.clone();
        b2.cells[r][c] = opp;
        let theirs = b2.run_through(r, c, opp);
        // Winning immediately dominates; then blocking a would-be win; then runs.
        let win_bonus = if mine >= WIN { 1_000_000 } else { 0 };
        let block_bonus = if theirs >= WIN { 500_000 } else { 0 };
        let centre = SIZE as i64 / 2;
        let dist = (r as i64 - centre).abs() + (c as i64 - centre).abs();
        win_bonus + block_bonus + (mine as i64).pow(3) * 10 + (theirs as i64).pow(3) * 8 - dist
    }

    /// The computer's move for `s`: the empty cell of highest heuristic score.
    /// Returns `None` only when the board is full.
    pub fn best_move(&self, s: Stone) -> Option<(usize, usize)> {
        let mut best: Option<((usize, usize), i64)> = None;
        for r in 0..SIZE {
            for c in 0..SIZE {
                if self.cells[r][c] != Stone::Empty {
                    continue;
                }
                let sc = self.score_at(r, c, s);
                if best.is_none_or(|(_, bs)| sc > bs) {
                    best = Some(((r, c), sc));
                }
            }
        }
        best.map(|(pos, _)| pos)
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

/// The interactive Gomoku overlay.
pub struct Gomoku {
    board: Board,
    cur: (usize, usize),
    status: String,
    over: bool,
}

impl Gomoku {
    pub fn new() -> Self {
        Gomoku {
            board: Board::new(),
            cur: (SIZE / 2, SIZE / 2),
            status: "Your move (X). Five in a row wins.".into(),
            over: false,
        }
    }

    fn play(&mut self) {
        if self.over {
            return;
        }
        let (r, c) = self.cur;
        if !self.board.place(r, c, Stone::X) {
            self.status = "Occupied.".into();
            return;
        }
        if self.board.wins(Stone::X) {
            self.status = "You win!  r: play again".into();
            self.over = true;
            return;
        }
        if self.board.full() {
            self.status = "Draw.  r: play again".into();
            self.over = true;
            return;
        }
        if let Some((ar, ac)) = self.board.best_move(Stone::O) {
            self.board.place(ar, ac, Stone::O);
            self.cur = (ar, ac);
            if self.board.wins(Stone::O) {
                self.status = "Computer wins.  r: play again".into();
                self.over = true;
            } else {
                self.status = "Your move (X).".into();
            }
        }
    }
}

impl Default for Gomoku {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Gomoku {
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
            key!(Left) | key!('h') => self.cur.1 = self.cur.1.saturating_sub(1),
            key!(Right) | key!('l') => self.cur.1 = (self.cur.1 + 1).min(SIZE - 1),
            key!(Up) | key!('k') => self.cur.0 = self.cur.0.saturating_sub(1),
            key!(Down) | key!('j') => self.cur.0 = (self.cur.0 + 1).min(SIZE - 1),
            key!(' ') | key!(Enter) => self.play(),
            key!('r') => *self = Gomoku::new(),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let grid_style = theme.get("ui.linenr");
        let x_style = theme.get("ui.text.focus");
        let o_style = theme.get("warning");
        let cursor_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < (SIZE as u16) * 2 + 4 || area.height < SIZE as u16 + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(ox, area.y, "Gomoku — five in a row", header_style);

        for r in 0..SIZE {
            for c in 0..SIZE {
                let x = ox + (c as u16) * 2;
                let y = oy + (r as u16);
                let (glyph, base) = match self.board.get(r, c) {
                    Stone::Empty => ("·", grid_style),
                    Stone::X => ("X", x_style),
                    Stone::O => ("O", o_style),
                };
                let style = if self.cur == (r, c) {
                    cursor_style
                } else {
                    base
                };
                surface.set_string(x, y, glyph, style);
            }
        }
        let sy = oy + SIZE as u16 + 1;
        surface.set_string(ox, sy, &self.status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_in_a_row_wins() {
        let mut b = Board::new();
        for c in 3..8 {
            b.place(6, c, Stone::X);
        }
        assert!(b.wins(Stone::X));
        assert!(!b.wins(Stone::O));
    }

    #[test]
    fn four_in_a_row_does_not_win() {
        let mut b = Board::new();
        for c in 3..7 {
            b.place(6, c, Stone::X);
        }
        assert!(!b.wins(Stone::X));
    }

    #[test]
    fn diagonal_five_wins() {
        let mut b = Board::new();
        for i in 0..5 {
            b.place(2 + i, 2 + i, Stone::O);
        }
        assert!(b.wins(Stone::O));
    }

    #[test]
    fn computer_takes_an_immediate_win() {
        let mut b = Board::new();
        // O has four in a row with an open end at (6,7); best_move must complete it.
        for c in 3..7 {
            b.place(6, c, Stone::O);
        }
        let mv = b.best_move(Stone::O).unwrap();
        b.place(mv.0, mv.1, Stone::O);
        assert!(
            b.wins(Stone::O),
            "computer should complete five, played {mv:?}"
        );
    }

    #[test]
    fn computer_blocks_an_immediate_loss() {
        let mut b = Board::new();
        // X threatens five at (6,7); with no win of its own, O must block there.
        for c in 3..7 {
            b.place(6, c, Stone::X);
        }
        let mv = b.best_move(Stone::O).unwrap();
        assert_eq!(
            mv,
            (6, 7),
            "computer should block the open four, played {mv:?}"
        );
    }
}
