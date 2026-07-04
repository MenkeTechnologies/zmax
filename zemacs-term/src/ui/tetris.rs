//! Tetris — the zemacs port of GNU Emacs `tetris`.
//!
//! Falling tetrominoes; complete rows clear and score. Move with the arrows or
//! `hjkl`, rotate with `Up`/`k`/`x`, soft-drop with `Down`/`j`, hard-drop with
//! `SPC`, pause with `p`, restart with `n`, quit with `q`/`Esc`. Like snake it
//! animates itself via `zemacs_event::request_redraw` only while falling (idles
//! when paused / over / closed). The board logic — rotation, collision, locking
//! and line-clearing — is pure and unit-tested (keys parse into a `tetris`
//! keymap mode by `scripts/gen_port_report.py`).

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 10;
const H: i16 = 20;

/// The seven tetrominoes as 4 cell offsets each within a 4×4 box (rotation 0).
const SHAPES: [[(i16, i16); 4]; 7] = [
    [(1, 0), (1, 1), (1, 2), (1, 3)], // I
    [(1, 1), (1, 2), (2, 1), (2, 2)], // O
    [(0, 1), (1, 0), (1, 1), (1, 2)], // T
    [(0, 1), (0, 2), (1, 0), (1, 1)], // S
    [(0, 0), (0, 1), (1, 1), (1, 2)], // Z
    [(0, 0), (1, 0), (1, 1), (1, 2)], // J
    [(0, 2), (1, 0), (1, 1), (1, 2)], // L
];

/// The pure tetris board. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub board: [[Option<u8>; 10]; 20],
    pub shape: usize,
    pub rot: usize,
    /// Board coords of the falling piece's 4×4 box top-left.
    pub pos: (i16, i16),
    pub next: usize,
    pub alive: bool,
    pub score: u32,
    pub lines: u32,
    rng: u64,
}

/// Rotate a 4×4-box offset 90° clockwise: (r, c) → (c, 3 − r).
fn rotate_cw(cell: (i16, i16)) -> (i16, i16) {
    (cell.1, 3 - cell.0)
}

/// The four board cells of shape `shape` at rotation `rot`, box at `pos`.
pub fn piece_cells(shape: usize, rot: usize, pos: (i16, i16)) -> [(i16, i16); 4] {
    let mut out = SHAPES[shape];
    for _ in 0..(rot % 4) {
        for cell in out.iter_mut() {
            *cell = rotate_cw(*cell);
        }
    }
    for cell in out.iter_mut() {
        *cell = (cell.0 + pos.0, cell.1 + pos.1);
    }
    out
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            board: [[None; 10]; 20],
            shape: 0,
            rot: 0,
            pos: (0, 0),
            next: 0,
            alive: true,
            score: 0,
            lines: 0,
            rng: seed | 1,
        };
        g.shape = g.rand_shape();
        g.next = g.rand_shape();
        g.spawn_pos();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    fn rand_shape(&mut self) -> usize {
        (self.rand() % 7) as usize
    }

    fn spawn_pos(&mut self) {
        self.rot = 0;
        self.pos = (0, 3);
    }

    /// Whether the given cells collide with a wall, the floor, or a locked cell.
    fn collides(&self, cells: &[(i16, i16); 4]) -> bool {
        cells.iter().any(|&(r, c)| {
            c < 0 || c >= W || r >= H || (r >= 0 && self.board[r as usize][c as usize].is_some())
        })
    }

    fn current(&self) -> [(i16, i16); 4] {
        piece_cells(self.shape, self.rot, self.pos)
    }

    /// Try to shift the falling piece by `(dr, dc)`. Returns whether it moved.
    pub fn shift(&mut self, dr: i16, dc: i16) -> bool {
        let np = (self.pos.0 + dr, self.pos.1 + dc);
        let cells = piece_cells(self.shape, self.rot, np);
        if self.collides(&cells) {
            false
        } else {
            self.pos = np;
            true
        }
    }

    /// Try to rotate the falling piece clockwise. Returns whether it rotated.
    pub fn rotate(&mut self) -> bool {
        let nr = (self.rot + 1) % 4;
        let cells = piece_cells(self.shape, nr, self.pos);
        if self.collides(&cells) {
            false
        } else {
            self.rot = nr;
            true
        }
    }

    /// Lock the piece, clear full lines, and spawn the next — ending the game if
    /// the new piece has nowhere to go.
    fn lock_and_next(&mut self) {
        let color = self.shape as u8 + 1;
        for &(r, c) in &self.current() {
            if r >= 0 && r < H && c >= 0 && c < W {
                self.board[r as usize][c as usize] = Some(color);
            }
        }
        self.clear_lines();
        self.shape = self.next;
        self.next = self.rand_shape();
        self.spawn_pos();
        if self.collides(&self.current()) {
            self.alive = false;
        }
    }

    fn clear_lines(&mut self) {
        let mut cleared = 0;
        let mut write = H - 1;
        for read in (0..H).rev() {
            let full = (0..W).all(|c| self.board[read as usize][c as usize].is_some());
            if full {
                cleared += 1;
            } else {
                if write != read {
                    self.board[write as usize] = self.board[read as usize];
                }
                write -= 1;
            }
        }
        for r in 0..=write {
            self.board[r as usize] = [None; 10];
        }
        if cleared > 0 {
            self.lines += cleared as u32;
            self.score += [0, 40, 100, 300, 1200][cleared.min(4)];
        }
    }

    /// One gravity step: fall one row, or lock and spawn the next piece.
    pub fn step(&mut self) {
        if !self.alive {
            return;
        }
        if !self.shift(1, 0) {
            self.lock_and_next();
        }
    }

    /// Drop the piece straight down and lock it immediately.
    pub fn hard_drop(&mut self) {
        if !self.alive {
            return;
        }
        while self.shift(1, 0) {}
        self.lock_and_next();
    }
}

/// The interactive Tetris overlay.
pub struct Tetris {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Tetris {
    pub fn new() -> Self {
        Tetris {
            game: Game::new(1),
            seed: 1,
            paused: false,
            last: None,
            interval: Duration::from_millis(500),
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.paused = false;
        self.last = None;
    }

    fn running(&self) -> bool {
        self.game.alive && !self.paused
    }
}

impl Default for Tetris {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Tetris {
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
            key!('p') => self.paused = !self.paused,
            key!('n') => self.restart(),
            _ if self.running() => match key {
                key!(Left) | key!('h') => {
                    self.game.shift(0, -1);
                }
                key!(Right) | key!('l') => {
                    self.game.shift(0, 1);
                }
                key!(Up) | key!('k') | key!('x') => {
                    self.game.rotate();
                }
                key!(Down) | key!('j') => {
                    self.game.step();
                }
                key!(' ') => self.game.hard_drop(),
                _ => {}
            },
            _ => {}
        }
        if self.running() {
            if self.last.is_none() {
                self.last = Some(Instant::now());
            }
            zemacs_event::request_redraw();
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let now = Instant::now();
        if self.running() {
            match self.last {
                Some(t) if now.duration_since(t) >= self.interval => {
                    self.game.step();
                    self.last = Some(now);
                }
                None => self.last = Some(now),
                _ => {}
            }
            if self.running() {
                zemacs_event::request_redraw();
            }
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let wall_style = theme.get("ui.linenr");
        let palette = [
            theme.get("ui.text.focus"),
            theme.get("warning"),
            theme.get("function"),
            theme.get("error"),
            theme.get("ui.text.focus"),
            theme.get("warning"),
            theme.get("function"),
        ];

        surface.clear_with(area, bg);
        if area.width < (W as u16) * 2 + 8 || area.height < (H as u16) + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(ox, area.y, "Tetris", header_style);

        // Compose a display grid = locked board + the falling piece.
        let mut cells = self.game.board;
        if self.game.alive {
            for &(r, c) in &piece_cells(self.game.shape, self.game.rot, self.game.pos) {
                if r >= 0 && r < H && c >= 0 && c < W {
                    cells[r as usize][c as usize] = Some(self.game.shape as u8 + 1);
                }
            }
        }
        for r in 0..H {
            surface.set_string(ox, oy + r as u16, "│", wall_style);
            surface.set_string(ox + (W as u16) * 2 + 1, oy + r as u16, "│", wall_style);
            for c in 0..W {
                let x = ox + 1 + (c as u16) * 2;
                let (glyph, style) = match cells[r as usize][c as usize] {
                    Some(col) => ("██", palette[(col as usize - 1) % 7]),
                    None => (" .", wall_style),
                };
                surface.set_string(x, oy + r as u16, glyph, style);
            }
        }
        surface.set_string(
            ox,
            oy + H as u16,
            &"─".repeat((W as usize) * 2 + 2),
            wall_style,
        );

        let sx = ox + (W as u16) * 2 + 4;
        surface.set_string(sx, oy, &format!("score {}", self.game.score), text_style);
        surface.set_string(
            sx,
            oy + 1,
            &format!("lines {}", self.game.lines),
            text_style,
        );
        let hint = if !self.game.alive {
            "GAME OVER  n: new"
        } else if self.paused {
            "PAUSED  p: resume"
        } else {
            "SPC drop · p pause"
        };
        surface.set_string(sx, oy + 3, hint, text_style);
        surface.set_string(
            ox,
            oy + H as u16 + 1,
            "←→ move · ↑ rotate · ↓ soft · SPC hard · n new · q quit",
            wall_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_piece_rotates_from_horizontal_to_vertical() {
        // Rotation 0 occupies one row; rotation 1 occupies one column.
        let flat = piece_cells(0, 0, (0, 0));
        let rows: std::collections::BTreeSet<i16> = flat.iter().map(|&(r, _)| r).collect();
        assert_eq!(rows.len(), 1, "horizontal I is one row");
        let up = piece_cells(0, 1, (0, 0));
        let cols: std::collections::BTreeSet<i16> = up.iter().map(|&(_, c)| c).collect();
        assert_eq!(cols.len(), 1, "vertical I is one column");
    }

    #[test]
    fn piece_locks_at_the_floor_and_spawns_new() {
        let mut g = Game::new(1);
        let start_shape = g.shape;
        // Drop repeatedly; eventually it locks and a new piece appears at the top.
        g.hard_drop();
        // Board now has locked cells.
        let filled: usize = g.board.iter().flatten().filter(|c| c.is_some()).count();
        assert_eq!(filled, 4, "one tetromino (4 cells) locked");
        assert!(g.pos.0 <= 1, "new piece spawns at the top");
        let _ = start_shape;
    }

    #[test]
    fn a_full_row_clears_and_scores() {
        let mut g = Game::new(1);
        // Fill the bottom row except leave the board's piece out of the way.
        for c in 0..W {
            g.board[(H - 1) as usize][c as usize] = Some(1);
        }
        let before = g.lines;
        g.clear_lines();
        assert_eq!(g.lines, before + 1);
        assert!(
            g.board[(H - 1) as usize].iter().all(|c| c.is_none()),
            "row cleared"
        );
        assert!(g.score >= 40);
    }

    #[test]
    fn partial_rows_are_not_cleared() {
        let mut g = Game::new(1);
        for c in 0..W - 1 {
            g.board[(H - 1) as usize][c as usize] = Some(1);
        }
        g.clear_lines();
        assert_eq!(g.lines, 0);
        assert!(g.board[(H - 1) as usize][0].is_some());
    }

    #[test]
    fn horizontal_shift_respects_walls() {
        let mut g = Game::new(1);
        // Shove left until it can't; the piece must stay in bounds.
        for _ in 0..20 {
            g.shift(0, -1);
        }
        assert!(piece_cells(g.shape, g.rot, g.pos)
            .iter()
            .all(|&(_, c)| c >= 0));
    }
}
