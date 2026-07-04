//! Bubbles — the zemacs port of GNU Emacs `bubbles`, the same-game.
//!
//! A grid of coloured bubbles. Pop a connected group (two or more of the same
//! colour, orthogonally adjacent) and it clears; the bubbles above fall to fill
//! the gaps and fully-empty columns collapse to the left. Larger pops score
//! more. The game ends when no group of two remains. Move the cursor with the
//! arrows or `hjkl`, pop with `SPC`/`RET`; `n` deals a new board, `q`/`Esc`
//! quits. The board mechanics are pure and unit-tested (keys parse into a
//! `bubbles` keymap mode via `scripts/gen_port_report.py`).

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const COLS: usize = 12;
const ROWS: usize = 10;
const COLORS: u8 = 4;

/// The pure bubbles board: `cells[r][c]` is `Some(colour)` or `None` (empty).
/// Row 0 is the top; gravity pulls bubbles toward the bottom (higher row index).
#[derive(Clone)]
pub struct Board {
    pub cells: Vec<Vec<Option<u8>>>,
    pub score: u64,
}

impl Board {
    /// Deterministically fill a full board from `seed`.
    pub fn from_seed(seed: u64) -> Self {
        let mut s = seed | 1;
        let mut cells = vec![vec![None; COLS]; ROWS];
        for row in cells.iter_mut() {
            for cell in row.iter_mut() {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                *cell = Some(((s >> 40) % COLORS as u64) as u8);
            }
        }
        Board { cells, score: 0 }
    }

    /// The connected same-colour group containing `(r, c)` (orthogonal), as a
    /// list of coordinates. Empty if the cell is empty.
    pub fn group_at(&self, r: usize, c: usize) -> Vec<(usize, usize)> {
        let Some(color) = self.cells[r][c] else {
            return Vec::new();
        };
        let mut seen = vec![vec![false; COLS]; ROWS];
        let mut stack = vec![(r, c)];
        let mut out = Vec::new();
        seen[r][c] = true;
        while let Some((y, x)) = stack.pop() {
            out.push((y, x));
            let push = |ny: usize,
                        nx: usize,
                        stack: &mut Vec<(usize, usize)>,
                        seen: &mut Vec<Vec<bool>>| {
                if !seen[ny][nx] && self.cells[ny][nx] == Some(color) {
                    seen[ny][nx] = true;
                    stack.push((ny, nx));
                }
            };
            if y > 0 {
                push(y - 1, x, &mut stack, &mut seen);
            }
            if y + 1 < ROWS {
                push(y + 1, x, &mut stack, &mut seen);
            }
            if x > 0 {
                push(y, x - 1, &mut stack, &mut seen);
            }
            if x + 1 < COLS {
                push(y, x + 1, &mut stack, &mut seen);
            }
        }
        out
    }

    /// Pop the group under `(r, c)` if it has two or more bubbles: clear it,
    /// apply gravity and collapse empty columns, and add to the score. Returns
    /// the number of bubbles popped (0 if the group was too small).
    pub fn pop(&mut self, r: usize, c: usize) -> usize {
        let group = self.group_at(r, c);
        if group.len() < 2 {
            return 0;
        }
        for &(y, x) in &group {
            self.cells[y][x] = None;
        }
        self.score += ((group.len() - 1) * (group.len() - 1)) as u64;
        self.settle();
        group.len()
    }

    /// Gravity within each column (bubbles fall to the bottom), then shift any
    /// fully-empty columns to the right so the board stays left-packed.
    fn settle(&mut self) {
        // Gravity per column.
        for c in 0..COLS {
            let mut col: Vec<u8> = (0..ROWS).filter_map(|r| self.cells[r][c]).collect();
            let empties = ROWS - col.len();
            for r in 0..empties {
                self.cells[r][c] = None;
            }
            for r in empties..ROWS {
                self.cells[r][c] = Some(col.remove(0));
            }
        }
        // Collapse empty columns to the right (keep non-empty columns left-packed).
        let non_empty: Vec<usize> = (0..COLS)
            .filter(|&c| (0..ROWS).any(|r| self.cells[r][c].is_some()))
            .collect();
        if non_empty.len() < COLS {
            let mut new = vec![vec![None; COLS]; ROWS];
            for (nc, &oc) in non_empty.iter().enumerate() {
                for r in 0..ROWS {
                    new[r][nc] = self.cells[r][oc];
                }
            }
            self.cells = new;
        }
    }

    pub fn remaining(&self) -> usize {
        self.cells.iter().flatten().filter(|c| c.is_some()).count()
    }

    /// Over when no poppable group (size >= 2) remains anywhere.
    pub fn over(&self) -> bool {
        for r in 0..ROWS {
            for c in 0..COLS {
                if self.cells[r][c].is_some() && self.group_at(r, c).len() >= 2 {
                    return false;
                }
            }
        }
        true
    }
}

/// The interactive Bubbles overlay.
pub struct Bubbles {
    board: Board,
    cur: (usize, usize),
    seed: u64,
    status: String,
}

impl Bubbles {
    pub fn new() -> Self {
        Bubbles {
            board: Board::from_seed(1),
            cur: (ROWS - 1, 0),
            seed: 1,
            status: "Pop connected groups of two or more.".into(),
        }
    }

    fn deal(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.board = Board::from_seed(self.seed);
        self.cur = (ROWS - 1, 0);
        self.status = "New board.".into();
    }

    fn pop_here(&mut self) {
        let (r, c) = self.cur;
        let n = self.board.pop(r, c);
        if n == 0 {
            self.status = "Need a group of two or more.".into();
        } else if self.board.over() {
            self.status = format!(
                "Popped {n}. Game over — score {}, {} left.  n: new",
                self.board.score,
                self.board.remaining()
            );
        } else {
            self.status = format!("Popped {n}. Score {}.", self.board.score);
        }
    }
}

impl Default for Bubbles {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Bubbles {
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
            key!(Right) | key!('l') => self.cur.1 = (self.cur.1 + 1).min(COLS - 1),
            key!(Up) | key!('k') => self.cur.0 = self.cur.0.saturating_sub(1),
            key!(Down) | key!('j') => self.cur.0 = (self.cur.0 + 1).min(ROWS - 1),
            key!(' ') | key!(Enter) => self.pop_here(),
            key!('n') => self.deal(),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let cursor_style = theme.get("ui.selection");
        let empty_style = theme.get("ui.linenr");
        // Distinct-ish styles per colour, reused from the theme.
        let palette = [
            theme.get("ui.text.focus"),
            theme.get("warning"),
            theme.get("function"),
            theme.get("error"),
        ];
        let glyphs = ['●', '◆', '▲', '■'];

        surface.clear_with(area, bg);
        if area.width < (COLS as u16) * 2 + 4 || area.height < ROWS as u16 + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(ox, area.y, "Bubbles — pop the groups", header_style);

        for r in 0..ROWS {
            for c in 0..COLS {
                let x = ox + (c as u16) * 2;
                let y = oy + (r as u16);
                let (glyph, style) = match self.board.cells[r][c] {
                    Some(col) => (glyphs[col as usize], palette[col as usize]),
                    None => ('·', empty_style),
                };
                let style = if self.cur == (r, c) {
                    cursor_style
                } else {
                    style
                };
                let mut buf = [0u8; 4];
                surface.set_string(x, y, glyph.encode_utf8(&mut buf), style);
            }
        }
        let sy = oy + ROWS as u16 + 1;
        surface.set_string(ox, sy, &self.status, text_style);
        surface.set_string(
            ox,
            sy + 1,
            "arrows/hjkl move · SPC pop · n new · q quit",
            empty_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_from(rows: &[&str]) -> Board {
        // Build a board from chars: '.'=empty, digit=colour. Bottom-aligned rows.
        let mut cells = vec![vec![None; COLS]; ROWS];
        for (r, line) in rows.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                cells[r][c] = if ch == '.' {
                    None
                } else {
                    Some(ch as u8 - b'0')
                };
            }
        }
        Board { cells, score: 0 }
    }

    #[test]
    fn group_finds_connected_same_colour() {
        let mut b = board_from(&["11.", "12."]);
        // (0,0),(0,1),(1,0) are colour 1 and connected; (1,1) is colour 2.
        let g = b.group_at(0, 0);
        assert_eq!(g.len(), 3);
        assert!(!g.contains(&(1, 1)));
        // A lone different colour is a group of one.
        assert_eq!(b.group_at(1, 1).len(), 1);
        let _ = &mut b;
    }

    #[test]
    fn pop_requires_two_and_scores() {
        let mut b = board_from(&["1.", "1."]);
        assert_eq!(b.pop(0, 0), 2);
        assert_eq!(b.score, 1); // (2-1)^2
                                // Board emptied and collapsed.
        assert_eq!(b.remaining(), 0);
    }

    #[test]
    fn single_bubble_does_not_pop() {
        let mut b = board_from(&["12"]);
        assert_eq!(b.pop(0, 0), 0);
        assert_eq!(b.remaining(), 2);
    }

    #[test]
    fn gravity_pulls_bubbles_down() {
        // Column 0: colours at top with a hole; after settle they sit at bottom.
        let mut b = Board {
            cells: vec![vec![None; COLS]; ROWS],
            score: 0,
        };
        b.cells[0][0] = Some(1);
        b.cells[2][0] = Some(2);
        b.settle();
        assert_eq!(b.cells[ROWS - 1][0], Some(2));
        assert_eq!(b.cells[ROWS - 2][0], Some(1));
        assert_eq!(b.cells[0][0], None);
    }

    #[test]
    fn empty_columns_collapse_left() {
        let mut b = Board {
            cells: vec![vec![None; COLS]; ROWS],
            score: 0,
        };
        // Only column 3 has a bubble → after settle it must move to column 0.
        b.cells[ROWS - 1][3] = Some(1);
        b.settle();
        assert_eq!(b.cells[ROWS - 1][0], Some(1));
        assert!((1..COLS).all(|c| b.cells[ROWS - 1][c].is_none()));
    }

    #[test]
    fn over_when_no_group_of_two() {
        let b = board_from(&["12", "21"]);
        assert!(b.over());
        let b2 = board_from(&["11"]);
        assert!(!b2.over());
    }
}
