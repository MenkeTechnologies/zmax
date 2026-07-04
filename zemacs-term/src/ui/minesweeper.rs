//! Minesweeper — a small terminal minesweeper for zemacs.
//!
//! Uncover every cell that isn't a mine. Move the cursor with the arrows or
//! `hjkl`, `SPC` reveals the cell under the cursor, `f` toggles a flag, `n`
//! starts a fresh board and `q`/`Esc` quits. Unlike the action games this one is
//! turn-based: nothing animates, so there is no frame loop — the board only
//! changes in response to a key. The board logic is pure and unit-tested (mines
//! are laid by a small LCG so a given seed is reproducible).

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: usize = 12;
const H: usize = 10;
const MINES: usize = 15;

/// Where the game is: still playing, cleared, or blown up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Playing,
    Won,
    Lost,
}

/// The pure minesweeper board. No I/O, no timing — unit-tested. Mines are laid
/// with the same LCG the other games use, so `Game::new(seed)` is deterministic.
#[derive(Clone)]
pub struct Game {
    /// `true` where a mine sits, indexed by `idx(row, col)`.
    mines: Vec<bool>,
    /// Cells the player has uncovered.
    revealed: Vec<bool>,
    /// Cells the player has flagged.
    flagged: Vec<bool>,
    /// Cursor as `(row, col)`.
    cursor: (usize, usize),
    state: State,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            mines: vec![false; W * H],
            revealed: vec![false; W * H],
            flagged: vec![false; W * H],
            cursor: (0, 0),
            state: State::Playing,
            rng: seed | 1,
        };
        g.lay_mines();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    fn lay_mines(&mut self) {
        let mut placed = 0;
        // The board is far larger than the mine count, so rejection sampling
        // terminates quickly; the bound is just a belt-and-braces guard.
        while placed < MINES {
            let r = (self.rand() % H as u64) as usize;
            let c = (self.rand() % W as u64) as usize;
            let i = idx(r, c);
            if !self.mines[i] {
                self.mines[i] = true;
                placed += 1;
            }
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// Mines left to find = total mines minus flags placed (may go negative if
    /// the player over-flags).
    pub fn mines_remaining(&self) -> i32 {
        MINES as i32 - self.flagged.iter().filter(|&&f| f).count() as i32
    }

    /// Count of mines adjacent to `(r, c)` (0..=8), computed on demand.
    pub fn adjacent(&self, r: usize, c: usize) -> u8 {
        let mut n = 0;
        for (nr, nc) in neighbors(r, c) {
            if self.mines[idx(nr, nc)] {
                n += 1;
            }
        }
        n
    }

    /// Move the cursor by `(dr, dc)`, clamped to the board.
    pub fn move_cursor(&mut self, dr: i32, dc: i32) {
        let r = (self.cursor.0 as i32 + dr).clamp(0, H as i32 - 1) as usize;
        let c = (self.cursor.1 as i32 + dc).clamp(0, W as i32 - 1) as usize;
        self.cursor = (r, c);
    }

    /// Toggle a flag on `(r, c)`; revealed cells can't be flagged.
    pub fn toggle_flag(&mut self, r: usize, c: usize) {
        if self.state != State::Playing {
            return;
        }
        let i = idx(r, c);
        if self.revealed[i] {
            return;
        }
        self.flagged[i] = !self.flagged[i];
    }

    /// Reveal `(r, c)`. A flagged or already-revealed cell is left alone. A mine
    /// loses the game (and every mine is exposed); a zero-count cell flood-fills
    /// its connected empty region and the numbered border around it.
    pub fn reveal(&mut self, r: usize, c: usize) {
        if self.state != State::Playing {
            return;
        }
        let i = idx(r, c);
        if self.flagged[i] || self.revealed[i] {
            return;
        }
        if self.mines[i] {
            self.state = State::Lost;
            for m in 0..self.mines.len() {
                if self.mines[m] {
                    self.revealed[m] = true;
                }
            }
            return;
        }
        self.flood(r, c);
        self.check_win();
    }

    /// Iterative flood fill: uncover the start cell, and when it has no adjacent
    /// mines keep spreading to its neighbors.
    fn flood(&mut self, sr: usize, sc: usize) {
        let mut stack = vec![(sr, sc)];
        while let Some((r, c)) = stack.pop() {
            let i = idx(r, c);
            if self.revealed[i] || self.flagged[i] || self.mines[i] {
                continue;
            }
            self.revealed[i] = true;
            if self.adjacent(r, c) == 0 {
                stack.extend(neighbors(r, c));
            }
        }
    }

    fn check_win(&mut self) {
        let revealed = self.revealed.iter().filter(|&&x| x).count();
        let mines = self.mines.iter().filter(|&&m| m).count();
        if revealed == W * H - mines {
            self.state = State::Won;
        }
    }

    /// Reveal the cell under the cursor (the interactive `SPC` action).
    pub fn reveal_cursor(&mut self) {
        let (r, c) = self.cursor;
        self.reveal(r, c);
    }

    /// Toggle the flag under the cursor (the interactive `f` action).
    pub fn flag_cursor(&mut self) {
        let (r, c) = self.cursor;
        self.toggle_flag(r, c);
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

fn idx(r: usize, c: usize) -> usize {
    r * W + c
}

/// The in-bounds 8-neighborhood of `(r, c)`.
fn neighbors(r: usize, c: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(8);
    for dr in -1i32..=1 {
        for dc in -1i32..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;
            if nr >= 0 && nr < H as i32 && nc >= 0 && nc < W as i32 {
                out.push((nr as usize, nc as usize));
            }
        }
    }
    out
}

/// The interactive Minesweeper overlay.
pub struct Minesweeper {
    game: Game,
    seed: u64,
}

impl Minesweeper {
    pub fn new() -> Self {
        Minesweeper {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Minesweeper {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Minesweeper {
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
            key!(' ') => self.game.reveal_cursor(),
            key!('f') => self.game.flag_cursor(),
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
        let hidden_style = theme.get("ui.linenr");
        let cursor_style = theme.get("ui.selection");
        let flag_style = theme.get("warning");
        let mine_style = theme.get("error");
        // Number colors, ramping up with the adjacent-mine count.
        let n1 = theme.get("ui.text");
        let n2 = theme.get("function");
        let n3 = theme.get("warning");
        let n4 = theme.get("error");

        surface.clear_with(area, bg);
        // Each cell is drawn two columns wide for legibility.
        if area.width < (W as u16) * 2 + 4 || area.height < (H as u16) + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let status = match self.game.state() {
            State::Playing => "Playing",
            State::Won => "You win!",
            State::Lost => "BOOM",
        };
        surface.set_string(
            ox,
            area.y,
            &format!(
                "Minesweeper  mines {}  [{}]",
                self.game.mines_remaining(),
                status
            ),
            header_style,
        );

        for r in 0..H {
            for c in 0..W {
                let i = idx(r, c);
                let (glyph, mut style): (String, _) = if self.game.revealed[i] {
                    if self.game.mines[i] {
                        ("✷".to_string(), mine_style)
                    } else {
                        let n = self.game.adjacent(r, c);
                        if n == 0 {
                            (" ".to_string(), text_style)
                        } else {
                            let st = match n {
                                1 => n1,
                                2 => n2,
                                3 => n3,
                                _ => n4,
                            };
                            (n.to_string(), st)
                        }
                    }
                } else if self.game.flagged[i] {
                    ("⚑".to_string(), flag_style)
                } else {
                    ("·".to_string(), hidden_style)
                };
                if (r, c) == self.game.cursor() {
                    style = cursor_style;
                }
                let x = ox + (c as u16) * 2;
                let y = oy + r as u16;
                surface.set_string(x, y, &glyph, style);
            }
        }

        let sy = oy + H as u16 + 1;
        surface.set_string(
            ox,
            sy,
            "hjkl/arrows move · SPC reveal · f flag · n new · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board with no mines laid, ready for hand placement.
    fn blank() -> Game {
        Game {
            mines: vec![false; W * H],
            revealed: vec![false; W * H],
            flagged: vec![false; W * H],
            cursor: (0, 0),
            state: State::Playing,
            rng: 1,
        }
    }

    fn place(g: &mut Game, r: usize, c: usize) {
        g.mines[idx(r, c)] = true;
    }

    #[test]
    fn revealing_a_mine_loses() {
        let mut g = blank();
        place(&mut g, 3, 4);
        g.reveal(3, 4);
        assert_eq!(g.state(), State::Lost);
        assert!(g.revealed[idx(3, 4)], "the mine is exposed on loss");
    }

    #[test]
    fn adjacent_count_is_correct() {
        let mut g = blank();
        // Three mines around (2, 2).
        place(&mut g, 1, 1);
        place(&mut g, 1, 2);
        place(&mut g, 3, 3);
        assert_eq!(g.adjacent(2, 2), 3);
        // A corner cell sees only its three in-bounds neighbors: (0,1), (1,0)
        // and the already-placed (1,1) — never any out-of-bounds cell.
        place(&mut g, 0, 1);
        place(&mut g, 1, 0);
        assert_eq!(g.adjacent(0, 0), 3);
    }

    #[test]
    fn zero_cell_flood_fills() {
        let mut g = blank();
        // One mine far away; revealing the opposite corner floods a huge region.
        place(&mut g, H - 1, W - 1);
        g.reveal(0, 0);
        assert!(g.revealed[idx(0, 0)]);
        assert!(g.revealed[idx(0, 5)], "flood spread across the empty board");
        assert!(g.revealed[idx(5, 5)], "flood spread down the empty board");
        // The mine itself is never uncovered by a flood.
        assert!(!g.revealed[idx(H - 1, W - 1)]);
    }

    #[test]
    fn flag_toggles_and_protects() {
        let mut g = blank();
        place(&mut g, 0, 0);
        g.toggle_flag(5, 5);
        assert!(g.flagged[idx(5, 5)]);
        // A flagged cell can't be revealed: flag the mine, then try to reveal it.
        g.toggle_flag(0, 0);
        g.reveal(0, 0);
        assert_eq!(
            g.state(),
            State::Playing,
            "flag protects the mine from reveal"
        );
        // Toggling again clears it.
        g.toggle_flag(5, 5);
        assert!(!g.flagged[idx(5, 5)]);
    }

    #[test]
    fn win_when_all_safe_cells_revealed() {
        let mut g = blank();
        place(&mut g, 0, 0);
        place(&mut g, H - 1, W - 1);
        for r in 0..H {
            for c in 0..W {
                if !g.mines[idx(r, c)] {
                    g.reveal(r, c);
                }
            }
        }
        assert_eq!(g.state(), State::Won);
    }

    #[test]
    fn mines_remaining_tracks_flags() {
        let mut g = Game::new(1);
        assert_eq!(g.mines_remaining(), MINES as i32);
        g.toggle_flag(0, 0);
        g.toggle_flag(0, 1);
        assert_eq!(g.mines_remaining(), MINES as i32 - 2);
    }
}
