//! 2048 — a sliding-tile puzzle in the style of the classic browser game.
//!
//! Slide the 4x4 board with the arrows or `hjkl`; equal neighbours merge into
//! their sum and every merge that changes the board spawns one new tile. Reach
//! 2048 to win (you may keep playing); fill the board with no legal move to
//! lose. `n` starts a new game, `q`/`Esc` quits. Unlike the action games this is
//! turn-based: nothing animates, so there is no frame loop — the board logic is
//! pure and unit-tested, seeded by the same LCG the snake port uses.

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Slide direction for a move.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Collapse a single row to the left: compact non-zero tiles, then merge each
/// equal adjacent pair once (a merged tile can't merge again this move), and
/// compact again. Returns the resulting row and the score gained from merges.
/// This is the pure primitive every slide direction is expressed in terms of.
pub fn collapse_left(row: [u32; 4]) -> ([u32; 4], u32) {
    // Compact: drop the zeros, keeping order.
    let vals: Vec<u32> = row.iter().copied().filter(|&v| v != 0).collect();
    let mut out = [0u32; 4];
    let mut gained = 0u32;
    let mut i = 0; // read index into `vals`
    let mut w = 0; // write index into `out`
    while i < vals.len() {
        if i + 1 < vals.len() && vals[i] == vals[i + 1] {
            let merged = vals[i] * 2;
            out[w] = merged;
            gained += merged;
            i += 2; // both tiles consumed by the merge
        } else {
            out[w] = vals[i];
            i += 1;
        }
        w += 1;
    }
    (out, gained)
}

/// The pure 2048 board. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub board: [[u32; 4]; 4],
    pub score: u32,
    pub won: bool,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Game {
            board: [[0; 4]; 4],
            score: 0,
            won: false,
            rng: seed | 1,
        }
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Place a `2` (~90% of the time) or `4` in a random empty cell. Returns
    /// `false` when the board is full and nothing could be placed.
    pub fn spawn_tile(&mut self) -> bool {
        let empties: Vec<(usize, usize)> = (0..4)
            .flat_map(|r| (0..4).map(move |c| (r, c)))
            .filter(|&(r, c)| self.board[r][c] == 0)
            .collect();
        if empties.is_empty() {
            return false;
        }
        let pick = (self.rand() % empties.len() as u64) as usize;
        let (r, c) = empties[pick];
        // ~90% twos, ~10% fours.
        self.board[r][c] = if self.rand() % 10 == 0 { 4 } else { 2 };
        true
    }

    /// Read one board line in the order a leftward collapse expects, given the
    /// travel direction; `k` selects the row (Left/Right) or column (Up/Down).
    fn read_line(&self, dir: Dir, k: usize) -> [u32; 4] {
        let mut row = [0u32; 4];
        for i in 0..4 {
            row[i] = match dir {
                Dir::Left => self.board[k][i],
                Dir::Right => self.board[k][3 - i],
                Dir::Up => self.board[i][k],
                Dir::Down => self.board[3 - i][k],
            };
        }
        row
    }

    /// Write a collapsed line back in the same orientation `read_line` used.
    fn write_line(&mut self, dir: Dir, k: usize, row: [u32; 4]) {
        for i in 0..4 {
            match dir {
                Dir::Left => self.board[k][i] = row[i],
                Dir::Right => self.board[k][3 - i] = row[i],
                Dir::Up => self.board[i][k] = row[i],
                Dir::Down => self.board[3 - i][k] = row[i],
            }
        }
    }

    /// Slide every line in `dir`, merging as it goes. Returns whether the board
    /// actually changed. Score and win state update as a side effect; spawning a
    /// new tile is the caller's job (only when a move changed the board).
    pub fn slide(&mut self, dir: Dir) -> bool {
        let mut moved = false;
        for k in 0..4 {
            let line = self.read_line(dir, k);
            let (out, gained) = collapse_left(line);
            if out != line {
                moved = true;
            }
            self.score += gained;
            self.write_line(dir, k, out);
        }
        if moved && self.max_tile() >= 2048 {
            self.won = true;
        }
        moved
    }

    /// The largest tile currently on the board.
    pub fn max_tile(&self) -> u32 {
        self.board.iter().flatten().copied().max().unwrap_or(0)
    }

    /// A move is possible if there is an empty cell or any equal orthogonal
    /// neighbours to merge.
    pub fn can_move(&self) -> bool {
        for r in 0..4 {
            for c in 0..4 {
                if self.board[r][c] == 0 {
                    return true;
                }
                if c + 1 < 4 && self.board[r][c] == self.board[r][c + 1] {
                    return true;
                }
                if r + 1 < 4 && self.board[r][c] == self.board[r + 1][c] {
                    return true;
                }
            }
        }
        false
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive 2048 overlay.
pub struct Game2048 {
    game: Game,
    seed: u64,
}

impl Game2048 {
    pub fn new() -> Self {
        let mut game = Game::new(1);
        game.spawn_tile();
        game.spawn_tile();
        Game2048 { game, seed: 1 }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        let mut game = Game::new(self.seed);
        game.spawn_tile();
        game.spawn_tile();
        self.game = game;
    }

    /// Handle a directional move: slide, and only spawn a fresh tile when the
    /// slide actually rearranged the board (classic 2048 rules).
    fn play(&mut self, dir: Dir) {
        if self.game.slide(dir) {
            self.game.spawn_tile();
        }
    }
}

impl Default for Game2048 {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Game2048 {
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
            key!(Up) | key!('k') => self.play(Dir::Up),
            key!(Down) | key!('j') => self.play(Dir::Down),
            key!(Left) | key!('h') => self.play(Dir::Left),
            key!(Right) | key!('l') => self.play(Dir::Right),
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
        let grid_style = theme.get("ui.linenr");
        let empty_style = theme.get("ui.selection");
        // Colour tiles by value tier.
        let small_style = theme.get("ui.text");
        let medium_style = theme.get("function");
        let large_style = theme.get("warning");
        let huge_style = theme.get("error");

        surface.clear_with(area, bg);

        // Each tile is 6 columns wide by 3 rows tall (2 body rows + a shared
        // border), so the 4x4 grid spans 25 columns and 13 rows.
        const CW: u16 = 6; // interior width of a cell
        const CH: u16 = 2; // interior height of a cell
        let grid_w = CW * 4 + 5; // 4 cells + 5 vertical rules
        let grid_h = CH * 4 + 5; // 4 cells + 5 horizontal rules
        if area.width < grid_w + 2 || area.height < grid_h + 4 {
            return;
        }
        let ox = area.x + 1;
        let oy = area.y + 2;

        surface.set_string(
            area.x,
            area.y,
            &format!(
                "2048  score {}  best-tile {}",
                self.game.score,
                self.game.max_tile()
            ),
            header_style,
        );

        // Draw the grid lines. Rows of border sit at oy + r*(CH+1) for r in 0..=4;
        // vertical rules at ox + c*(CW+1) for c in 0..=4.
        for r in 0..=4u16 {
            let y = oy + r * (CH + 1);
            for c in 0..=4u16 {
                let x = ox + c * (CW + 1);
                // Corner/junction glyph.
                let glyph = match (r, c) {
                    (0, 0) => "┌",
                    (0, 4) => "┐",
                    (4, 0) => "└",
                    (4, 4) => "┘",
                    (0, _) => "┬",
                    (4, _) => "┴",
                    (_, 0) => "├",
                    (_, 4) => "┤",
                    _ => "┼",
                };
                surface.set_string(x, y, glyph, grid_style);
                // Horizontal segment to the right of this junction.
                if c < 4 {
                    for i in 1..=CW {
                        surface.set_string(x + i, y, "─", grid_style);
                    }
                }
            }
            // Vertical segments below this row of junctions.
            if r < 4 {
                for c in 0..=4u16 {
                    let x = ox + c * (CW + 1);
                    for i in 1..=CH {
                        surface.set_string(x, y + i, "│", grid_style);
                    }
                }
            }
        }

        // Draw the tile values, centred in each cell's interior.
        for r in 0..4usize {
            for c in 0..4usize {
                let v = self.game.board[r][c];
                let cell_x = ox + c as u16 * (CW + 1) + 1;
                let cell_y = oy + r as u16 * (CH + 1) + 1;
                // Middle interior row.
                let mid_y = cell_y + (CH - 1) / 2;
                if v == 0 {
                    surface.set_string(cell_x, mid_y, "     ·", empty_style);
                    continue;
                }
                let label = format!("{}", v);
                let pad = (CW as usize).saturating_sub(label.len()) / 2;
                let text = format!("{:pad$}{}", "", label, pad = pad);
                let style = match v {
                    0..=8 => small_style,
                    16..=64 => medium_style,
                    128..=512 => large_style,
                    _ => huge_style,
                };
                surface.set_string(cell_x, mid_y, &text, style);
            }
        }

        // Status / footer.
        let sy = oy + grid_h + 1;
        let status = if self.game.won {
            "You win!  keep going · n new · q quit".to_string()
        } else if !self.game.can_move() {
            "Game over — no moves left · n new · q quit".to_string()
        } else {
            "↑↓←→/hjkl move · n new · q quit".to_string()
        };
        surface.set_string(area.x, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_two_collapses_to_four_and_scores() {
        let (row, gained) = collapse_left([2, 2, 0, 0]);
        assert_eq!(row, [4, 0, 0, 0]);
        assert_eq!(gained, 4);
    }

    #[test]
    fn gap_between_equal_tiles_still_merges() {
        let (row, gained) = collapse_left([2, 0, 2, 0]);
        assert_eq!(row, [4, 0, 0, 0]);
        assert_eq!(gained, 4);
    }

    #[test]
    fn four_equal_tiles_make_two_pairs() {
        let (row, gained) = collapse_left([2, 2, 2, 2]);
        assert_eq!(row, [4, 4, 0, 0]);
        assert_eq!(gained, 8, "two merges of four each");
    }

    #[test]
    fn full_board_with_no_merges_does_not_move() {
        // A checkerboard of distinct neighbours: nothing can slide or merge.
        let mut g = Game::new(1);
        g.board = [[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]];
        assert!(!g.slide(Dir::Left), "a locked board reports no move");
        assert!(!g.can_move(), "and no move is possible at all");
    }

    #[test]
    fn a_real_move_spawns_exactly_one_tile() {
        let mut g = Game::new(1);
        g.board = [[2, 2, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        let before = g.board.iter().flatten().filter(|&&v| v != 0).count();
        assert!(g.slide(Dir::Left));
        g.spawn_tile();
        let after = g.board.iter().flatten().filter(|&&v| v != 0).count();
        // Two 2s merged into one 4 (−1), then one tile spawned (+1): net same
        // count, but exactly one new non-zero cell appeared beyond the merge.
        assert_eq!(after, before - 1 + 1);
        assert_eq!(g.score, 4, "the merge added 4 to the score");
    }

    #[test]
    fn reaching_2048_sets_the_win_flag() {
        let mut g = Game::new(1);
        g.board = [[1024, 1024, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        assert!(!g.won);
        assert!(g.slide(Dir::Left));
        assert_eq!(g.board[0][0], 2048);
        assert!(g.won, "hitting 2048 wins");
    }
}
