//! Reversi — a small terminal Othello for zemacs, played against the computer.
//!
//! You are black (●), the computer is white (○). Move the cursor with the arrows
//! or `hjkl`, `SPC` drops a disc on the square under the cursor when it is a legal
//! move — a move must sandwich at least one white disc in a straight line, and
//! every sandwiched disc flips to black. The computer then answers with its own
//! move. If you have no legal move you must `p` pass; when neither side can move
//! the game is over and the larger army wins. `n` starts a fresh board and
//! `q`/`Esc` quits. Like Minesweeper this is turn-based: nothing animates, so the
//! board only changes in response to a key. The board logic is pure and
//! unit-tested; the computer's tie-breaks come from the same LCG the other games
//! use, so `Game::new(seed)` is deterministic.

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const N: usize = 8;

/// The eight straight-line directions a sandwich can run along.
const DIRS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

/// A classic positional weight table: corners are gold, the squares next to them
/// are poison. The computer adds the flip count on top of this.
const WEIGHTS: [[i32; N]; N] = [
    [120, -20, 20, 5, 5, 20, -20, 120],
    [-20, -40, -5, -5, -5, -5, -40, -20],
    [20, -5, 15, 3, 3, 15, -5, 20],
    [5, -5, 3, 3, 3, 3, -5, 5],
    [5, -5, 3, 3, 3, 3, -5, 5],
    [20, -5, 15, 3, 3, 15, -5, 20],
    [-20, -40, -5, -5, -5, -5, -40, -20],
    [120, -20, 20, 5, 5, 20, -20, 120],
];

/// A single square's contents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    Empty,
    Black,
    White,
}

/// Whose discs are in play. The human is always `Black`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Player {
    Black,
    White,
}

impl Player {
    fn opp(self) -> Player {
        match self {
            Player::Black => Player::White,
            Player::White => Player::Black,
        }
    }

    fn cell(self) -> Cell {
        match self {
            Player::Black => Cell::Black,
            Player::White => Cell::White,
        }
    }
}

/// The pure Othello board. No I/O, no timing — unit-tested. The computer's
/// tie-breaking jitter comes from the same LCG the other games use, so
/// `Game::new(seed)` is deterministic.
#[derive(Clone)]
pub struct Game {
    /// Squares in row-major order, indexed by `idx(row, col)`.
    board: Vec<Cell>,
    /// Whose turn it is (only ever `Black` when we hand control back to the UI).
    turn: Player,
    /// Cursor as `(row, col)`.
    cursor: (usize, usize),
    /// `true` once neither side has a legal move.
    over: bool,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut board = vec![Cell::Empty; N * N];
        // Standard central opening.
        board[idx(3, 3)] = Cell::White;
        board[idx(4, 4)] = Cell::White;
        board[idx(3, 4)] = Cell::Black;
        board[idx(4, 3)] = Cell::Black;
        Game {
            board,
            turn: Player::Black,
            cursor: (2, 3),
            over: false,
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

    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    pub fn is_over(&self) -> bool {
        self.over
    }

    pub fn cell_at(&self, r: usize, c: usize) -> Cell {
        self.board[idx(r, c)]
    }

    /// `(black, white)` disc counts.
    pub fn counts(&self) -> (usize, usize) {
        let b = self.board.iter().filter(|&&c| c == Cell::Black).count();
        let w = self.board.iter().filter(|&&c| c == Cell::White).count();
        (b, w)
    }

    /// The discs `player` would flip by playing `pos`. Empty when the move is
    /// illegal (occupied square or no sandwich in any direction).
    pub fn flips(&self, pos: (usize, usize), player: Player) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        if self.board[idx(pos.0, pos.1)] != Cell::Empty {
            return out;
        }
        let me = player.cell();
        let opp = player.opp().cell();
        for (dr, dc) in DIRS {
            let mut line = Vec::new();
            let mut r = pos.0 as i32 + dr;
            let mut c = pos.1 as i32 + dc;
            while r >= 0 && r < N as i32 && c >= 0 && c < N as i32 {
                let cell = self.board[idx(r as usize, c as usize)];
                if cell == opp {
                    line.push((r as usize, c as usize));
                } else if cell == me {
                    // Closed the sandwich: keep the run we walked over.
                    out.extend(line.iter().copied());
                    break;
                } else {
                    break; // hit an empty square — no sandwich this way
                }
                r += dr;
                c += dc;
            }
        }
        out
    }

    /// Every legal move for `player`.
    pub fn legal_moves(&self, player: Player) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for r in 0..N {
            for c in 0..N {
                if self.board[idx(r, c)] == Cell::Empty && !self.flips((r, c), player).is_empty() {
                    out.push((r, c));
                }
            }
        }
        out
    }

    /// Drop `player`'s disc on `pos` and flip everything it sandwiches. Assumes
    /// `pos` is legal (callers gate on `flips`); an empty flip set just lays the
    /// disc.
    pub fn apply(&mut self, pos: (usize, usize), player: Player) {
        let fl = self.flips(pos, player);
        self.board[idx(pos.0, pos.1)] = player.cell();
        for (r, c) in fl {
            self.board[idx(r, c)] = player.cell();
        }
    }

    fn any_legal(&self, player: Player) -> bool {
        !self.legal_moves(player).is_empty()
    }

    /// End the game once neither side can move.
    fn check_over(&mut self) {
        if !self.any_legal(Player::Black) && !self.any_legal(Player::White) {
            self.over = true;
        }
    }

    /// Move the cursor by `(dr, dc)`, clamped to the board.
    pub fn move_cursor(&mut self, dr: i32, dc: i32) {
        let r = (self.cursor.0 as i32 + dr).clamp(0, N as i32 - 1) as usize;
        let c = (self.cursor.1 as i32 + dc).clamp(0, N as i32 - 1) as usize;
        self.cursor = (r, c);
    }

    /// The interactive `SPC` action: play the human's disc under the cursor when
    /// it is legal, then let the computer respond.
    pub fn place_cursor(&mut self) {
        if self.over || self.turn != Player::Black {
            return;
        }
        let pos = self.cursor;
        if self.flips(pos, Player::Black).is_empty() {
            return; // illegal — nothing happens
        }
        self.apply(pos, Player::Black);
        self.turn = Player::White;
        self.resolve();
    }

    /// The interactive `p` action: pass when — and only when — black has no move.
    pub fn pass(&mut self) {
        if self.over || self.turn != Player::Black || self.any_legal(Player::Black) {
            return;
        }
        self.turn = Player::White;
        self.resolve();
    }

    /// Run the computer (and any forced passes) until it is the human's turn with
    /// a legal move, or the game is over.
    fn resolve(&mut self) {
        loop {
            self.check_over();
            if self.over {
                return;
            }
            match self.turn {
                Player::White => {
                    if self.any_legal(Player::White) {
                        let m = self.best_move(Player::White);
                        self.apply(m, Player::White);
                        self.turn = Player::Black;
                    } else {
                        // White must pass; black is guaranteed a move here.
                        self.turn = Player::Black;
                        return;
                    }
                }
                Player::Black => {
                    if self.any_legal(Player::Black) {
                        return; // hand control back to the human
                    }
                    // Black must pass; loop back and let white play again.
                    self.turn = Player::White;
                }
            }
        }
    }

    /// Pick the computer's move: corners and good squares first (the weight
    /// table), then most discs flipped, with a tiny LCG jitter to break exact
    /// ties. The `* 4` keeps distinct scores strictly ordered — the jitter
    /// (0..=2) can only reshuffle ties.
    fn best_move(&mut self, player: Player) -> (usize, usize) {
        let moves = self.legal_moves(player);
        let mut best = moves[0];
        let mut best_score = i32::MIN;
        for m in moves {
            let base = WEIGHTS[m.0][m.1] + self.flips(m, player).len() as i32;
            let jitter = (self.rand() % 3) as i32;
            let score = base * 4 + jitter;
            if score > best_score {
                best_score = score;
                best = m;
            }
        }
        best
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

/// The interactive Reversi overlay.
pub struct Reversi {
    game: Game,
    seed: u64,
}

impl Reversi {
    pub fn new() -> Self {
        Reversi {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Reversi {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Reversi {
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
            key!(' ') => self.game.place_cursor(),
            key!('p') => self.game.pass(),
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
        let cursor_style = theme.get("ui.selection");
        let black_style = theme.get("warning");
        let white_style = theme.get("function");

        surface.clear_with(area, bg);
        // Each cell is drawn two columns wide for legibility.
        if area.width < (N as u16) * 2 + 4 || area.height < (N as u16) + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let (b, w) = self.game.counts();
        let status = if self.game.is_over() {
            if b > w {
                "You win!"
            } else if w > b {
                "CPU wins"
            } else {
                "Draw"
            }
        } else if self.game.legal_moves(Player::Black).is_empty() {
            "no move — p to pass"
        } else {
            "your move"
        };
        surface.set_string(
            ox,
            area.y,
            &format!("Reversi  ● {}  ○ {}  [{}]", b, w, status),
            header_style,
        );

        // Rules above and below the grid, in the board colour.
        for c in 0..(N as u16) * 2 {
            surface.set_string(ox + c, oy - 1, "─", grid_style);
            surface.set_string(ox + c, oy + N as u16, "─", grid_style);
        }

        // Hint the human's legal squares while the game is live.
        let legal: Vec<(usize, usize)> = if self.game.is_over() {
            Vec::new()
        } else {
            self.game.legal_moves(Player::Black)
        };

        for r in 0..N {
            for c in 0..N {
                let (glyph, mut style): (&str, _) = match self.game.cell_at(r, c) {
                    Cell::Black => ("●", black_style),
                    Cell::White => ("○", white_style),
                    Cell::Empty => {
                        if legal.contains(&(r, c)) {
                            ("·", grid_style)
                        } else {
                            (" ", text_style)
                        }
                    }
                };
                if (r, c) == self.game.cursor() {
                    style = cursor_style;
                }
                let x = ox + (c as u16) * 2;
                let y = oy + r as u16;
                surface.set_string(x, y, glyph, style);
            }
        }

        let sy = oy + N as u16 + 1;
        surface.set_string(
            ox,
            sy,
            "hjkl/arrows move · SPC place · p pass · n new · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board fully packed with black discs, so neither side can move.
    fn full_black() -> Game {
        Game {
            board: vec![Cell::Black; N * N],
            turn: Player::Black,
            cursor: (0, 0),
            over: false,
            rng: 1,
        }
    }

    #[test]
    fn standard_opening() {
        let g = Game::new(1);
        assert_eq!(g.counts(), (2, 2), "four discs, split evenly");
        assert_eq!(
            g.legal_moves(Player::Black).len(),
            4,
            "black opens with four legal moves"
        );
    }

    #[test]
    fn a_legal_move_flips_the_sandwiched_line() {
        let g = Game::new(1);
        // Playing (2,3) sandwiches the lone white at (3,3) against black at (4,3).
        assert_eq!(g.flips((2, 3), Player::Black), vec![(3, 3)]);
    }

    #[test]
    fn illegal_move_is_rejected() {
        let mut g = Game::new(1);
        assert!(
            g.flips((0, 0), Player::Black).is_empty(),
            "a corner flips nothing at the opening"
        );
        let before = g.counts();
        g.cursor = (0, 0);
        g.place_cursor();
        assert_eq!(g.counts(), before, "an illegal placement changes nothing");
        assert_eq!(g.turn, Player::Black, "and the turn stays with the human");
    }

    #[test]
    fn flipping_updates_the_counts() {
        let mut g = Game::new(1);
        g.apply((2, 3), Player::Black);
        // +1 laid, +1 flipped from white → black.
        assert_eq!(g.counts(), (4, 1));
    }

    #[test]
    fn game_over_when_neither_can_move() {
        let mut g = full_black();
        assert!(g.legal_moves(Player::Black).is_empty());
        assert!(g.legal_moves(Player::White).is_empty());
        g.check_over();
        assert!(g.is_over(), "a full board ends the game");
    }
}
