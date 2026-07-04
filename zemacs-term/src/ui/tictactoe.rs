//! Tic-Tac-Toe — a small terminal noughts-and-crosses for zemacs.
//!
//! Play X against the computer's O. Move the cursor with the arrows or `hjkl`,
//! `SPC` drops your X on the empty cell under the cursor and the computer
//! replies immediately, `n` starts a fresh game and `q`/`Esc` quits. Like
//! Minesweeper this one is turn-based: nothing animates, so there is no frame
//! loop — the board only changes in response to a key. The board logic is pure
//! and unit-tested. The computer plays a full minimax search, so it never loses;
//! when several replies are equally good it breaks the tie with the same LCG the
//! other games use, so `Game::new(seed)` stays reproducible.

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// A single square: empty, the human's X, or the computer's O.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    Empty,
    X,
    O,
}

/// Where the game is: still playing, someone won, or a full-board draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Playing,
    XWins,
    OWins,
    Draw,
}

/// The three-in-a-row lines: rows, columns, and both diagonals.
const LINES: [[usize; 3]; 8] = [
    [0, 1, 2],
    [3, 4, 5],
    [6, 7, 8],
    [0, 3, 6],
    [1, 4, 7],
    [2, 5, 8],
    [0, 4, 8],
    [2, 4, 6],
];

/// Return the winner if a line is fully owned by one side, else `None`.
fn check(board: &[Cell; 9]) -> Option<Cell> {
    for line in LINES.iter() {
        let a = board[line[0]];
        if a != Cell::Empty && a == board[line[1]] && a == board[line[2]] {
            return Some(a);
        }
    }
    None
}

/// `true` once every square is occupied.
fn is_full(board: &[Cell; 9]) -> bool {
    board.iter().all(|&c| c != Cell::Empty)
}

/// Score a finished-or-ongoing position from O's point of view. Depth is folded
/// in so a faster win (or a slower loss) scores better, which makes O take an
/// immediate winning move and block an immediate threat rather than dawdle.
///
/// O is the maximiser, X the minimiser.
fn minimax(board: &mut [Cell; 9], player: Cell, depth: i32) -> i32 {
    if let Some(w) = check(board) {
        return if w == Cell::O { 10 - depth } else { depth - 10 };
    }
    if is_full(board) {
        return 0;
    }
    if player == Cell::O {
        let mut best = i32::MIN;
        for i in 0..9 {
            if board[i] == Cell::Empty {
                board[i] = Cell::O;
                let s = minimax(board, Cell::X, depth + 1);
                board[i] = Cell::Empty;
                if s > best {
                    best = s;
                }
            }
        }
        best
    } else {
        let mut best = i32::MAX;
        for i in 0..9 {
            if board[i] == Cell::Empty {
                board[i] = Cell::X;
                let s = minimax(board, Cell::O, depth + 1);
                board[i] = Cell::Empty;
                if s < best {
                    best = s;
                }
            }
        }
        best
    }
}

/// The pure tic-tac-toe board. No I/O, no timing — unit-tested. The computer's
/// tie-breaking uses the same LCG the other games use, so `Game::new(seed)` is
/// deterministic.
#[derive(Clone)]
pub struct Game {
    board: [Cell; 9],
    /// Cursor as a flat cell index `0..9`.
    cursor: usize,
    state: State,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Game {
            board: [Cell::Empty; 9],
            cursor: 0,
            state: State::Playing,
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

    pub fn state(&self) -> State {
        self.state
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn cell(&self, i: usize) -> Cell {
        self.board[i]
    }

    /// Move the cursor by `(dr, dc)` on the 3x3 grid, clamped to the board.
    pub fn move_cursor(&mut self, dr: i32, dc: i32) {
        let r = (self.cursor as i32 / 3 + dr).clamp(0, 2);
        let c = (self.cursor as i32 % 3 + dc).clamp(0, 2);
        self.cursor = (r * 3 + c) as usize;
    }

    /// Recompute the game state from the board (winner, draw, or still playing).
    fn update_state(&mut self) {
        self.state = match check(&self.board) {
            Some(Cell::X) => State::XWins,
            Some(Cell::O) => State::OWins,
            _ if is_full(&self.board) => State::Draw,
            _ => State::Playing,
        };
    }

    /// Every reply that scores as well as the best one, for O to move.
    fn optimal_moves(&self) -> Vec<usize> {
        let mut board = self.board;
        let mut best_score = i32::MIN;
        let mut best = Vec::new();
        for i in 0..9 {
            if board[i] == Cell::Empty {
                board[i] = Cell::O;
                let s = minimax(&mut board, Cell::X, 1);
                board[i] = Cell::Empty;
                if s > best_score {
                    best_score = s;
                    best.clear();
                    best.push(i);
                } else if s == best_score {
                    best.push(i);
                }
            }
        }
        best
    }

    /// O's best reply. Ties are resolved to the lowest index so the search is
    /// deterministic and easy to test; interactive play breaks ties with the LCG.
    pub fn best_move(&self) -> usize {
        self.optimal_moves()[0]
    }

    /// Let the computer place its O, picking randomly among equally-good moves.
    fn computer_reply(&mut self) {
        let moves = self.optimal_moves();
        if moves.is_empty() {
            return;
        }
        let pick = (self.rand() % moves.len() as u64) as usize;
        self.board[moves[pick]] = Cell::O;
    }

    /// Drop the human's X on the cell under the cursor (the interactive `SPC`
    /// action). Occupied cells and finished games are ignored; when the move
    /// doesn't end the game the computer replies at once.
    pub fn place_cursor(&mut self) {
        if self.state != State::Playing {
            return;
        }
        let i = self.cursor;
        if self.board[i] != Cell::Empty {
            return;
        }
        self.board[i] = Cell::X;
        self.update_state();
        if self.state == State::Playing {
            self.computer_reply();
            self.update_state();
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Tic-Tac-Toe overlay.
pub struct TicTacToe {
    game: Game,
    seed: u64,
}

impl TicTacToe {
    pub fn new() -> Self {
        TicTacToe {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for TicTacToe {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for TicTacToe {
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
        let x_style = theme.get("warning");
        let o_style = theme.get("function");

        surface.clear_with(area, bg);
        // The grid is 11 columns wide and 5 rows tall; keep a small margin.
        if area.width < 20 || area.height < 12 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let status = match self.game.state() {
            State::Playing => "Your move (X)",
            State::XWins => "You win!",
            State::OWins => "Computer wins",
            State::Draw => "Draw",
        };
        surface.set_string(
            ox,
            area.y,
            &format!("Tic-Tac-Toe  [{}]", status),
            header_style,
        );

        for r in 0..3usize {
            let y = oy + (r as u16) * 2;
            for c in 0..3usize {
                let i = r * 3 + c;
                let (glyph, glyph_style) = match self.game.cell(i) {
                    Cell::X => ("X", x_style),
                    Cell::O => ("O", o_style),
                    Cell::Empty => (" ", text_style),
                };
                let style = if i == self.game.cursor() {
                    cursor_style
                } else {
                    glyph_style
                };
                let x = ox + (c as u16) * 4;
                surface.set_string(x, y, &format!(" {} ", glyph), style);
                if c < 2 {
                    surface.set_string(x + 3, y, "│", grid_style);
                }
            }
            if r < 2 {
                surface.set_string(ox, y + 1, "───┼───┼───", grid_style);
            }
        }

        let sy = oy + 6;
        surface.set_string(
            ox,
            sy,
            "hjkl/arrows move · SPC place · n new · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Cell::{Empty as E, O, X};
    use super::*;

    /// A board built from a hand-laid layout, ready for the computer to move.
    fn game_with(board: [Cell; 9]) -> Game {
        Game {
            board,
            cursor: 0,
            state: State::Playing,
            rng: 1,
        }
    }

    #[test]
    fn minimax_takes_the_immediate_win() {
        // O owns 0 and 1; the winning move is to complete the top row at 2.
        let g = game_with([O, O, E, E, X, E, X, E, E]);
        assert_eq!(g.best_move(), 2, "O completes its own three-in-a-row");
    }

    #[test]
    fn minimax_blocks_the_opponents_win() {
        // X threatens the top row (0, 1); O must block at 2 rather than lose.
        let g = game_with([X, X, E, O, E, E, E, E, E]);
        assert_eq!(g.best_move(), 2, "O blocks X's immediate threat");
    }

    #[test]
    fn detects_row_col_and_diagonal_wins() {
        assert_eq!(
            check(&[X, X, X, E, E, E, E, E, E]),
            Some(Cell::X),
            "top row"
        );
        assert_eq!(
            check(&[O, E, E, O, E, E, O, E, E]),
            Some(Cell::O),
            "left column"
        );
        assert_eq!(
            check(&[X, E, E, E, X, E, E, E, X]),
            Some(Cell::X),
            "main diagonal"
        );
        assert_eq!(
            check(&[E, E, O, E, O, E, O, E, E]),
            Some(Cell::O),
            "anti diagonal"
        );
        assert_eq!(check(&[E; 9]), None, "an empty board has no winner");
    }

    #[test]
    fn full_board_with_no_line_is_a_draw() {
        // X O X / X O O / O X X — full, and no line is owned by one side.
        let mut g = game_with([X, O, X, X, O, O, O, X, X]);
        assert_eq!(check(&g.board), None, "the layout has no three-in-a-row");
        g.update_state();
        assert_eq!(g.state(), State::Draw);
    }

    #[test]
    fn placing_on_an_occupied_cell_is_rejected() {
        let mut g = game_with([X, E, E, E, E, E, E, E, E]);
        g.cursor = 0; // cell 0 already holds an X
        let before = g.board;
        g.place_cursor();
        assert_eq!(g.board, before, "an occupied cell is left untouched");
    }
}
