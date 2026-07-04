//! 15-Puzzle — the classic sliding-tile puzzle for zemacs.
//!
//! A 4x4 board holds the tiles 1..=15 and one blank; slide the tiles back into
//! order (1..15 with the blank last). Press an arrow (or `hjkl`) to slide the
//! tile next to the blank *in that direction*: `↑`/`k` slides the tile below the
//! blank up, `↓`/`j` slides the tile above it down, `←`/`h` slides the tile to
//! its right left, `→`/`l` slides the tile to its left right — equivalently, the
//! blank moves the opposite way. `n` reshuffles, `q`/`Esc` quits. Like the other
//! puzzles this is turn-based: nothing animates, so there is no frame loop — the
//! board logic is pure and unit-tested. Shuffling only ever applies legal blank
//! moves from the solved state, so every deal is guaranteed solvable. The LCG is
//! the same one the other games use, so a given seed is reproducible.

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The direction a tile slides into the blank. `Up` means the tile below the
/// blank moves up (the blank moves down), and so on for the others.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// The pure 15-puzzle board. No I/O, no timing — unit-tested. `Game::new(seed)`
/// starts solved; call `shuffle` to scramble it deterministically.
#[derive(Clone)]
pub struct Game {
    /// Row-major 4x4 board; `0` is the blank, tiles are `1..=15`.
    pub board: [u8; 16],
    pub moves: u32,
    pub won: bool,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut board = [0u8; 16];
        for i in 0..15 {
            board[i] = i as u8 + 1;
        }
        // board[15] stays 0 — the blank in the solved position.
        Game {
            board,
            moves: 0,
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

    /// Index of the blank cell.
    fn blank_index(&self) -> usize {
        self.board.iter().position(|&v| v == 0).unwrap()
    }

    /// The solved board is `1..15` in order with the blank last.
    pub fn is_solved(&self) -> bool {
        for i in 0..15 {
            if self.board[i] != i as u8 + 1 {
                return false;
            }
        }
        self.board[15] == 0
    }

    /// Slide the tile orthogonally adjacent to the blank in `dir` into the blank.
    /// Returns whether anything moved (a slide with no tile on that side of the
    /// blank is a no-op). Bumps the move counter and refreshes the win state.
    pub fn slide(&mut self, dir: Dir) -> bool {
        let b = self.blank_index();
        let br = b / 4;
        let bc = b % 4;
        // The neighbour tile that would fill the blank, if it exists.
        let src = match dir {
            Dir::Up => (br + 1 < 4).then(|| (br + 1) * 4 + bc),
            Dir::Down => (br >= 1).then(|| (br - 1) * 4 + bc),
            Dir::Left => (bc + 1 < 4).then(|| br * 4 + bc + 1),
            Dir::Right => (bc >= 1).then(|| br * 4 + bc - 1),
        };
        match src {
            Some(s) => {
                self.board.swap(b, s);
                self.moves += 1;
                self.won = self.is_solved();
                true
            }
            None => false,
        }
    }

    /// Scramble the board by applying `moves` random *legal* blank slides from
    /// the current position. Because every step is a legal move, the result is
    /// always solvable. The move counter and win flag are reset afterwards.
    pub fn shuffle(&mut self, moves: usize) {
        const DIRS: [Dir; 4] = [Dir::Up, Dir::Down, Dir::Left, Dir::Right];
        let mut done = 0;
        while done < moves {
            let d = DIRS[(self.rand() % 4) as usize];
            if self.slide(d) {
                done += 1;
            }
        }
        self.moves = 0;
        self.won = self.is_solved();
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive 15-Puzzle overlay.
pub struct Fifteen {
    game: Game,
    seed: u64,
}

impl Fifteen {
    pub fn new() -> Self {
        let mut game = Game::new(1);
        game.shuffle(300);
        Fifteen { game, seed: 1 }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        let mut game = Game::new(self.seed);
        game.shuffle(300);
        self.game = game;
    }
}

impl Default for Fifteen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Fifteen {
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
            key!(Up) | key!('k') => {
                self.game.slide(Dir::Up);
            }
            key!(Down) | key!('j') => {
                self.game.slide(Dir::Down);
            }
            key!(Left) | key!('h') => {
                self.game.slide(Dir::Left);
            }
            key!(Right) | key!('l') => {
                self.game.slide(Dir::Right);
            }
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
        let placed_style = theme.get("function");

        surface.clear_with(area, bg);

        // Each tile is 4 columns wide by 1 row tall (plus shared borders), so the
        // 4x4 grid spans 21 columns and 9 rows.
        const CW: u16 = 4; // interior width of a cell
        const CH: u16 = 1; // interior height of a cell
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
            &format!("15-Puzzle  moves {}", self.game.moves),
            header_style,
        );

        // Draw the grid lines. Rows of border sit at oy + r*(CH+1) for r in 0..=4;
        // vertical rules at ox + c*(CW+1) for c in 0..=4.
        for r in 0..=4u16 {
            let y = oy + r * (CH + 1);
            for c in 0..=4u16 {
                let x = ox + c * (CW + 1);
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
                if c < 4 {
                    for i in 1..=CW {
                        surface.set_string(x + i, y, "─", grid_style);
                    }
                }
            }
            if r < 4 {
                for c in 0..=4u16 {
                    let x = ox + c * (CW + 1);
                    for i in 1..=CH {
                        surface.set_string(x, y + i, "│", grid_style);
                    }
                }
            }
        }

        // Draw the tile values, right-aligned in each cell's interior. A tile that
        // already sits in its solved home is tinted; the blank is left empty.
        for r in 0..4usize {
            for c in 0..4usize {
                let i = r * 4 + c;
                let v = self.game.board[i];
                let cell_x = ox + c as u16 * (CW + 1) + 1;
                let cell_y = oy + r as u16 * (CH + 1) + 1;
                let mid_y = cell_y + (CH - 1) / 2;
                if v == 0 {
                    surface.set_string(cell_x, mid_y, "    ", empty_style);
                    continue;
                }
                let label = format!("{:>width$}", v, width = CW as usize);
                let style = if v == i as u8 + 1 {
                    placed_style
                } else {
                    text_style
                };
                surface.set_string(cell_x, mid_y, &label, style);
            }
        }

        let sy = oy + grid_h + 1;
        let status = if self.game.won {
            format!("Solved in {} moves!  n new · q quit", self.game.moves)
        } else {
            "↑↓←→/hjkl slide a tile toward the arrow · n new · q quit".to_string()
        };
        surface.set_string(area.x, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solved_board_is_detected_as_solved() {
        let g = Game::new(1);
        assert!(g.is_solved(), "a fresh board starts in the solved order");
        assert!(
            !g.won,
            "the win flag isn't set until a slide reaches the goal"
        );
        assert_eq!(g.board[15], 0, "the blank is last in the solved position");
    }

    #[test]
    fn a_legal_slide_moves_one_tile_into_the_blank() {
        let mut g = Game::new(1); // solved: blank at index 15 (row 3, col 3)
        assert_eq!(g.blank_index(), 15);
        // Sliding Down brings the tile *above* the blank (index 11, value 12) down.
        assert!(g.slide(Dir::Down));
        assert_eq!(
            g.blank_index(),
            11,
            "the blank moved up into the vacated cell"
        );
        assert_eq!(
            g.board[15], 12,
            "exactly that neighbour tile filled the blank"
        );
        assert_eq!(g.moves, 1, "one slide counts as one move");
    }

    #[test]
    fn an_illegal_slide_is_a_no_op() {
        let mut g = Game::new(1); // blank at the bottom-right corner
                                  // No tile below the blank (it's on the bottom row) → Up can't slide.
        assert!(!g.slide(Dir::Up));
        // No tile to the right of the blank (it's on the last column) → Left can't.
        assert!(!g.slide(Dir::Left));
        assert_eq!(g.moves, 0, "no-op slides don't advance the counter");
        assert!(g.is_solved(), "the board is untouched");
    }

    #[test]
    fn shuffling_from_solved_stays_solvable_and_scrambles() {
        let mut g = Game::new(1);
        g.shuffle(300);
        assert!(!g.is_solved(), "300 seeded moves leave the board scrambled");
        assert_eq!(g.moves, 0, "shuffling resets the move counter");
        // Only legal blank slides were applied, so the board is still a valid
        // permutation of 0..=15 — which is exactly what makes it solvable.
        let mut seen = [false; 16];
        for &v in g.board.iter() {
            seen[v as usize] = true;
        }
        assert!(
            seen.iter().all(|&s| s),
            "every tile 0..=15 appears exactly once"
        );
    }

    #[test]
    fn solving_the_board_sets_the_win_state() {
        let mut g = Game::new(1);
        // One move from solved: tile 15 sits right of the blank at (row 3, col 2).
        g.board = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 0, 15];
        g.won = false;
        assert!(!g.is_solved());
        // Slide that tile Left into the blank to complete the puzzle.
        assert!(g.slide(Dir::Left));
        assert!(g.is_solved(), "the last tile clicks into place");
        assert!(g.won, "reaching the solved order wins");
    }
}
