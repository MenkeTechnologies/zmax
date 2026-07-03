//! Solitaire — the zemacs port of GNU Emacs `solitaire`, English peg solitaire.
//!
//! The board is the 33-hole cross. Every hole starts with a peg except the
//! centre. A move jumps a peg over an orthogonally-adjacent peg into the empty
//! hole two cells away, removing the jumped peg. The goal is to leave a single
//! peg, ideally in the centre. Move the cursor with the arrows or `hjkl`;
//! `SPC`/`RET` picks up the peg under the cursor, then a direction jumps it;
//! `q`/`Esc` quits. The board + jump logic is pure and unit-tested below (keys
//! are parsed into a `solitaire` keymap mode by `scripts/gen_port_report.py`).

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: usize = 7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    /// Outside the cross — not a playable hole.
    Invalid,
    Peg,
    Empty,
}

/// The pure peg-solitaire board and its jump rule. No I/O — unit-tested.
#[derive(Clone)]
pub struct Board {
    cells: [[Cell; W]; W],
}

impl Board {
    /// The standard English board: a plus/cross of 33 holes, all pegs but the
    /// centre.
    pub fn new() -> Self {
        let mut cells = [[Cell::Invalid; W]; W];
        for (r, row) in cells.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                let arm = (2..=4).contains(&r) || (2..=4).contains(&c);
                if arm {
                    *cell = Cell::Peg;
                }
            }
        }
        cells[3][3] = Cell::Empty;
        Board { cells }
    }

    pub fn get(&self, r: usize, c: usize) -> Cell {
        self.cells[r][c]
    }

    pub fn is_playable(&self, r: usize, c: usize) -> bool {
        r < W && c < W && self.cells[r][c] != Cell::Invalid
    }

    /// Attempt to jump the peg at `(r, c)` by `(dr, dc)` ∈ {(±1,0),(0,±1)}.
    /// Legal when the source is a peg, the adjacent cell is a peg, and the cell
    /// two away is an empty (playable) hole. On success the source and the
    /// jumped peg become empty and the landing hole becomes a peg.
    pub fn jump(&mut self, r: usize, c: usize, dr: isize, dc: isize) -> bool {
        let mid = (r as isize + dr, c as isize + dc);
        let dst = (r as isize + 2 * dr, c as isize + 2 * dc);
        if dst.0 < 0 || dst.0 >= W as isize || dst.1 < 0 || dst.1 >= W as isize {
            return false;
        }
        let (mr, mc) = (mid.0 as usize, mid.1 as usize);
        let (dr_, dc_) = (dst.0 as usize, dst.1 as usize);
        if self.cells[r][c] != Cell::Peg
            || self.cells[mr][mc] != Cell::Peg
            || self.cells[dr_][dc_] != Cell::Empty
        {
            return false;
        }
        self.cells[r][c] = Cell::Empty;
        self.cells[mr][mc] = Cell::Empty;
        self.cells[dr_][dc_] = Cell::Peg;
        true
    }

    pub fn pegs(&self) -> usize {
        self.cells
            .iter()
            .flatten()
            .filter(|&&c| c == Cell::Peg)
            .count()
    }

    /// True when no legal jump remains anywhere on the board.
    pub fn stuck(&self) -> bool {
        for r in 0..W {
            for c in 0..W {
                if self.cells[r][c] != Cell::Peg {
                    continue;
                }
                for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    if self.clone().jump(r, c, dr, dc) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Won when exactly one peg remains, in the centre.
    pub fn is_won(&self) -> bool {
        self.pegs() == 1 && self.cells[3][3] == Cell::Peg
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

/// The interactive solitaire overlay.
pub struct Solitaire {
    board: Board,
    cur: (usize, usize),
    /// The peg picked up with SPC, awaiting a direction key.
    selected: Option<(usize, usize)>,
    status: String,
}

impl Solitaire {
    pub fn new() -> Self {
        Solitaire {
            board: Board::new(),
            cur: (0, 3),
            selected: None,
            status: String::new(),
        }
    }

    fn move_cursor(&mut self, dr: isize, dc: isize) {
        let nr = self.cur.0 as isize + dr;
        let nc = self.cur.1 as isize + dc;
        if nr >= 0 && nr < W as isize && nc >= 0 && nc < W as isize {
            let (nr, nc) = (nr as usize, nc as usize);
            if self.board.is_playable(nr, nc) {
                self.cur = (nr, nc);
            }
        }
    }

    /// A direction key: if a peg is selected, jump it; otherwise move the cursor.
    fn direction(&mut self, dr: isize, dc: isize) {
        if let Some((r, c)) = self.selected.take() {
            if self.board.jump(r, c, dr, dc) {
                self.cur = (
                    (r as isize + 2 * dr) as usize,
                    (c as isize + 2 * dc) as usize,
                );
                if self.board.is_won() {
                    self.status = "You win! One peg, dead centre.".into();
                } else if self.board.stuck() {
                    self.status = format!("Stuck with {} pegs. r: restart", self.board.pegs());
                }
            } else {
                self.status = "Illegal jump.".into();
            }
        } else {
            self.move_cursor(dr, dc);
        }
    }

    fn toggle_select(&mut self) {
        let (r, c) = self.cur;
        if self.selected == Some((r, c)) {
            self.selected = None;
        } else if self.board.get(r, c) == Cell::Peg {
            self.selected = Some((r, c));
            self.status = "Peg selected — press a direction to jump.".into();
        } else {
            self.status = "No peg here.".into();
        }
    }
}

impl Default for Solitaire {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Solitaire {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        self.status.clear();
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!(Left) | key!('h') => self.direction(0, -1),
            key!(Right) | key!('l') => self.direction(0, 1),
            key!(Up) | key!('k') => self.direction(-1, 0),
            key!(Down) | key!('j') => self.direction(1, 0),
            key!(' ') | key!(Enter) => self.toggle_select(),
            key!('r') => *self = Solitaire::new(),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let dim_style = theme.get("ui.linenr");
        let peg_style = theme.get("ui.text.focus");
        let cursor_style = theme.get("ui.selection");
        let sel_style = theme.get("warning");

        surface.clear_with(area, bg);
        if area.width < 20 || area.height < 12 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(ox, area.y, "Solitaire — jump to one peg", header_style);

        for r in 0..W {
            for c in 0..W {
                let x = ox + (c as u16) * 2;
                let y = oy + (r as u16);
                let (glyph, base) = match self.board.get(r, c) {
                    Cell::Invalid => (" ", dim_style),
                    Cell::Peg => ("●", peg_style),
                    Cell::Empty => ("·", dim_style),
                };
                let style = if self.cur == (r, c) {
                    cursor_style
                } else if self.selected == Some((r, c)) {
                    sel_style
                } else {
                    base
                };
                surface.set_string(x, y, glyph, style);
            }
        }

        let sy = oy + (W as u16) + 1;
        let help = if self.status.is_empty() {
            format!(
                "pegs {}   ·  arrows/hjkl move  SPC pick up  direction jumps  r restart  q quit",
                self.board.pegs()
            )
        } else {
            self.status.clone()
        };
        surface.set_string(ox, sy, &help, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_board_has_32_pegs_and_empty_centre() {
        let b = Board::new();
        assert_eq!(b.pegs(), 32);
        assert_eq!(b.get(3, 3), Cell::Empty);
        assert_eq!(b.get(0, 0), Cell::Invalid);
    }

    #[test]
    fn legal_jump_into_centre_removes_the_jumped_peg() {
        let mut b = Board::new();
        // Peg at (1,3) jumps down over (2,3) into the empty centre (3,3).
        assert!(b.jump(1, 3, 1, 0));
        assert_eq!(b.get(1, 3), Cell::Empty);
        assert_eq!(b.get(2, 3), Cell::Empty);
        assert_eq!(b.get(3, 3), Cell::Peg);
        assert_eq!(b.pegs(), 31);
    }

    #[test]
    fn jump_needs_a_peg_to_hop_and_an_empty_landing() {
        let mut b = Board::new();
        // (0,3) down would land on (2,3) which is a peg, not empty -> illegal.
        assert!(!b.jump(0, 3, 1, 0));
        // Jumping off the board is illegal.
        assert!(!b.jump(3, 2, 0, -1) || b.pegs() == 31);
    }

    #[test]
    fn out_of_bounds_landing_is_rejected() {
        let mut b = Board::new();
        assert!(!b.jump(2, 3, -1, 0)); // would land at row -1
    }

    #[test]
    fn single_centre_peg_is_a_win() {
        let mut b = Board {
            cells: [[Cell::Invalid; W]; W],
        };
        // Rebuild the cross as empty, then one centre peg.
        for r in 0..W {
            for c in 0..W {
                if (2..=4).contains(&r) || (2..=4).contains(&c) {
                    b.cells[r][c] = Cell::Empty;
                }
            }
        }
        b.cells[3][3] = Cell::Peg;
        assert!(b.is_won());
        assert!(b.stuck());
    }
}
