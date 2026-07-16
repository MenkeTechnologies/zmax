//! Connect Four — a small terminal Connect Four for zmax.
//!
//! Drop red discs against the computer's yellow ones and line up four in a row —
//! horizontally, vertically or on either diagonal. Move the column cursor with
//! `h`/`l` or the arrows, `SPC` (or `Down`) drops a disc into that column, `n`
//! starts a fresh game and `q`/`Esc` quits. Like Minesweeper this one is
//! turn-based: nothing animates, so there is no frame loop — the board only
//! changes in response to a key. After you drop, the computer replies with an
//! alpha-beta search that always takes an immediate win and blocks yours. The
//! board logic is pure and unit-tested (its tie-breaks use a small LCG so a given
//! seed is reproducible).

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const COLS: usize = 7;
const ROWS: usize = 6;
/// Search depth for the computer's alpha-beta reply.
const DEPTH: i32 = 5;

/// The two disc colors. The human is `Red` and moves first; the computer is
/// `Yellow`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Player {
    Red,
    Yellow,
}

/// Where the game is: still playing, someone connected four, or the board filled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Playing,
    RedWon,
    YellowWon,
    Draw,
}

fn opponent(p: Player) -> Player {
    match p {
        Player::Red => Player::Yellow,
        Player::Yellow => Player::Red,
    }
}

fn idx(r: usize, c: usize) -> usize {
    r * COLS + c
}

/// Score a single 4-cell window from `player`'s point of view. A window that
/// holds both colors can never be completed by either, so it is worth nothing.
fn score_window(window: &[Option<Player>; 4], player: Player) -> i32 {
    let opp = opponent(player);
    let mine = window.iter().filter(|&&x| x == Some(player)).count();
    let theirs = window.iter().filter(|&&x| x == Some(opp)).count();
    if mine > 0 && theirs > 0 {
        return 0;
    }
    match (mine, theirs) {
        (3, 0) => 50,
        (2, 0) => 10,
        (1, 0) => 1,
        (0, 3) => -60,
        (0, 2) => -8,
        _ => 0,
    }
}

/// The pure Connect Four board. No I/O, no timing — unit-tested. Row `0` is the
/// top of the board and row `ROWS - 1` is the bottom, so a dropped disc settles
/// on the largest empty row index in its column. `Game::new(seed)` is
/// deterministic; the seed only feeds tie-breaking in the computer's search.
#[derive(Clone)]
pub struct Game {
    /// Each cell, indexed by `idx(row, col)`; `None` is empty.
    board: Vec<Option<Player>>,
    /// The column the drop cursor sits over.
    cursor: usize,
    /// Whose turn it is while `state == Playing`.
    turn: Player,
    state: State,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Game {
            board: vec![None; COLS * ROWS],
            cursor: COLS / 2,
            turn: Player::Red,
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

    /// The disc (if any) at `(r, c)`.
    fn cell(&self, r: usize, c: usize) -> Option<Player> {
        self.board[idx(r, c)]
    }

    /// A column is full once its top row is occupied.
    fn column_full(&self, col: usize) -> bool {
        self.board[idx(0, col)].is_some()
    }

    fn is_full(&self) -> bool {
        (0..COLS).all(|c| self.column_full(c))
    }

    /// Move the drop cursor by `d` columns, clamped to the board.
    pub fn move_cursor(&mut self, d: i32) {
        self.cursor = (self.cursor as i32 + d).clamp(0, COLS as i32 - 1) as usize;
    }

    /// Drop a `player` disc into `col`; it settles on the lowest empty row.
    /// Returns the row it landed on, or `None` if the column is full / invalid.
    fn place(&mut self, col: usize, player: Player) -> Option<usize> {
        if col >= COLS {
            return None;
        }
        for r in (0..ROWS).rev() {
            if self.board[idx(r, col)].is_none() {
                self.board[idx(r, col)] = Some(player);
                return Some(r);
            }
        }
        None
    }

    /// `true` if `player` has four in a row anywhere (horizontal, vertical or
    /// either diagonal).
    fn four_in_a_row(&self, player: Player) -> bool {
        let dirs = [(0i32, 1i32), (1, 0), (1, 1), (1, -1)];
        for r in 0..ROWS {
            for c in 0..COLS {
                if self.cell(r, c) != Some(player) {
                    continue;
                }
                for (dr, dc) in dirs {
                    let mut count = 1;
                    let mut rr = r as i32 + dr;
                    let mut cc = c as i32 + dc;
                    while rr >= 0
                        && rr < ROWS as i32
                        && cc >= 0
                        && cc < COLS as i32
                        && self.cell(rr as usize, cc as usize) == Some(player)
                    {
                        count += 1;
                        rr += dr;
                        cc += dc;
                    }
                    if count >= 4 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The column where `player` could drop to complete four immediately, if any.
    /// Used by the computer to take a win and to block the opponent's.
    fn winning_move(&self, player: Player) -> Option<usize> {
        for c in 0..COLS {
            if self.column_full(c) {
                continue;
            }
            let mut g = self.clone();
            g.place(c, player);
            if g.four_in_a_row(player) {
                return Some(c);
            }
        }
        None
    }

    /// Drop for `player`, then settle the game state (win / draw / hand over the
    /// turn). Returns whether the disc was actually placed.
    fn commit(&mut self, col: usize, player: Player) -> bool {
        if self.place(col, player).is_none() {
            return false;
        }
        if self.four_in_a_row(player) {
            self.state = match player {
                Player::Red => State::RedWon,
                Player::Yellow => State::YellowWon,
            };
        } else if self.is_full() {
            self.state = State::Draw;
        } else {
            self.turn = opponent(player);
        }
        true
    }

    /// The interactive human drop at the cursor; if the game is still on, the
    /// computer immediately replies.
    pub fn player_drop(&mut self) {
        if self.state != State::Playing || self.turn != Player::Red {
            return;
        }
        if self.commit(self.cursor, Player::Red) && self.state == State::Playing {
            self.computer_move();
        }
    }

    /// The computer's (Yellow) reply: take an immediate win, else block the
    /// opponent's immediate win, else search for the best positional drop.
    fn computer_move(&mut self) {
        if self.state != State::Playing {
            return;
        }
        let col = if let Some(c) = self.winning_move(Player::Yellow) {
            c
        } else if let Some(c) = self.winning_move(Player::Red) {
            c
        } else {
            self.best_column().unwrap_or(COLS / 2)
        };
        self.commit(col, Player::Yellow);
    }

    /// Alpha-beta search for Yellow's strongest drop, breaking ties with the LCG.
    fn best_column(&mut self) -> Option<usize> {
        let mut best_score = i32::MIN;
        let mut best_cols: Vec<usize> = Vec::new();
        for c in 0..COLS {
            if self.column_full(c) {
                continue;
            }
            let mut g = self.clone();
            g.place(c, Player::Yellow);
            let score = if g.four_in_a_row(Player::Yellow) {
                900_000
            } else {
                -g.negamax(DEPTH - 1, i32::MIN + 1, i32::MAX - 1, Player::Red)
            };
            if score > best_score {
                best_score = score;
                best_cols.clear();
                best_cols.push(c);
            } else if score == best_score {
                best_cols.push(c);
            }
        }
        if best_cols.is_empty() {
            return None;
        }
        let pick = (self.rand() as usize) % best_cols.len();
        Some(best_cols[pick])
    }

    /// Negamax with alpha-beta: the value of the position for `to_move`.
    fn negamax(&self, depth: i32, mut alpha: i32, beta: i32, to_move: Player) -> i32 {
        if self.is_full() {
            return 0;
        }
        if depth == 0 {
            return self.heuristic(to_move);
        }
        let mut best = i32::MIN + 1;
        for c in 0..COLS {
            if self.column_full(c) {
                continue;
            }
            let mut g = self.clone();
            g.place(c, to_move);
            let score = if g.four_in_a_row(to_move) {
                900_000 + depth
            } else {
                -g.negamax(depth - 1, -beta, -alpha, opponent(to_move))
            };
            if score > best {
                best = score;
            }
            if best > alpha {
                alpha = best;
            }
            if alpha >= beta {
                break;
            }
        }
        best
    }

    /// Static evaluation from `player`'s point of view: prefer the center column
    /// and sum every 4-cell window's potential.
    fn heuristic(&self, player: Player) -> i32 {
        let mut score = 0i32;
        for r in 0..ROWS {
            if self.cell(r, COLS / 2) == Some(player) {
                score += 3;
            }
        }
        let dirs = [(0i32, 1i32), (1, 0), (1, 1), (1, -1)];
        for r in 0..ROWS as i32 {
            for c in 0..COLS as i32 {
                for (dr, dc) in dirs {
                    let er = r + 3 * dr;
                    let ec = c + 3 * dc;
                    if er < 0 || er >= ROWS as i32 || ec < 0 || ec >= COLS as i32 {
                        continue;
                    }
                    let mut window = [None; 4];
                    for k in 0..4 {
                        window[k] =
                            self.cell((r + k as i32 * dr) as usize, (c + k as i32 * dc) as usize);
                    }
                    score += score_window(&window, player);
                }
            }
        }
        score
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Connect Four overlay.
pub struct ConnectFour {
    game: Game,
    seed: u64,
}

impl ConnectFour {
    pub fn new() -> Self {
        ConnectFour {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for ConnectFour {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for ConnectFour {
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
            key!(Left) | key!('h') => self.game.move_cursor(-1),
            key!(Right) | key!('l') => self.game.move_cursor(1),
            key!(' ') | key!(Down) => self.game.player_drop(),
            key!('n') => self.restart(),
            _ => {}
        }
        zmax_event::request_redraw();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let grid_style = theme.get("ui.linenr");
        let cursor_style = theme.get("ui.selection");
        let red_style = theme.get("warning");
        let yellow_style = theme.get("function");
        let win_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < (COLS as u16) * 2 + 6 || area.height < (ROWS as u16) + 8 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let (status, status_style) = match self.game.state() {
            State::Playing => ("red to move", header_style),
            State::RedWon => ("you win!", win_style),
            State::YellowWon => ("computer wins", win_style),
            State::Draw => ("draw", header_style),
        };
        surface.set_string(ox, area.y, "Connect Four", header_style);
        surface.set_string(ox + 14, area.y, status, status_style);

        // The drop cursor arrow, above the grid.
        let arrow_x = ox + 1 + (self.game.cursor() as u16) * 2;
        surface.set_string(arrow_x, oy, "▼", cursor_style);

        // Grid borders.
        let dashes = "─".repeat(2 * COLS - 1);
        let top = format!("┌{}┐", dashes);
        let bottom = format!("└{}┘", dashes);
        let top_y = oy + 1;
        surface.set_string(ox, top_y, &top, grid_style);
        let bottom_y = top_y + 1 + ROWS as u16;
        surface.set_string(ox, bottom_y, &bottom, grid_style);

        // The discs, row by row.
        let right_x = ox + 2 * COLS as u16;
        for r in 0..ROWS {
            let y = top_y + 1 + r as u16;
            surface.set_string(ox, y, "│", grid_style);
            surface.set_string(right_x, y, "│", grid_style);
            for c in 0..COLS {
                let (glyph, style) = match self.game.cell(r, c) {
                    Some(Player::Red) => ("●", red_style),
                    Some(Player::Yellow) => ("●", yellow_style),
                    None => ("·", grid_style),
                };
                let x = ox + 1 + (c as u16) * 2;
                surface.set_string(x, y, glyph, style);
            }
        }

        surface.set_string(
            ox,
            bottom_y + 2,
            "h/l move · SPC/↓ drop · n new · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_lands_on_lowest_empty_row() {
        let mut g = Game::new(1);
        // The first disc settles on the bottom row, the next stacks above it.
        assert_eq!(g.place(3, Player::Red), Some(ROWS - 1));
        assert_eq!(g.place(3, Player::Red), Some(ROWS - 2));
    }

    #[test]
    fn full_column_is_rejected() {
        let mut g = Game::new(1);
        for _ in 0..ROWS {
            assert!(g.place(0, Player::Red).is_some());
        }
        assert!(g.column_full(0));
        assert_eq!(g.place(0, Player::Red), None);
    }

    #[test]
    fn horizontal_win_is_detected() {
        let mut g = Game::new(1);
        for c in 0..4 {
            g.place(c, Player::Red);
        }
        assert!(g.four_in_a_row(Player::Red));
        assert!(!g.four_in_a_row(Player::Yellow));
    }

    #[test]
    fn vertical_win_is_detected() {
        let mut g = Game::new(1);
        for _ in 0..4 {
            g.place(2, Player::Yellow);
        }
        assert!(g.four_in_a_row(Player::Yellow));
    }

    #[test]
    fn diagonal_win_is_detected() {
        let mut g = Game::new(1);
        // Build the ascending diagonal (5,0)-(4,1)-(3,2)-(2,3) in red, padding the
        // lower cells of each column with yellow so red lands on the right row.
        g.place(0, Player::Red); // (5,0)
        g.place(1, Player::Yellow); // (5,1)
        g.place(1, Player::Red); // (4,1)
        g.place(2, Player::Yellow);
        g.place(2, Player::Yellow);
        g.place(2, Player::Red); // (3,2)
        g.place(3, Player::Yellow);
        g.place(3, Player::Yellow);
        g.place(3, Player::Yellow);
        g.place(3, Player::Red); // (2,3)
        assert!(g.four_in_a_row(Player::Red));
    }

    #[test]
    fn computer_takes_immediate_win() {
        let mut g = Game::new(1);
        // Yellow already has three across the bottom; column 3 completes four.
        g.place(0, Player::Yellow);
        g.place(1, Player::Yellow);
        g.place(2, Player::Yellow);
        assert_eq!(g.winning_move(Player::Yellow), Some(3));
        g.computer_move();
        assert_eq!(g.state(), State::YellowWon);
    }

    #[test]
    fn computer_blocks_opponent_win() {
        let mut g = Game::new(1);
        // Red threatens four across the bottom; the computer has no win of its
        // own, so it must drop into column 3 to block.
        g.place(0, Player::Red);
        g.place(1, Player::Red);
        g.place(2, Player::Red);
        assert_eq!(g.winning_move(Player::Red), Some(3));
        assert_eq!(g.winning_move(Player::Yellow), None);
        g.computer_move();
        assert_eq!(g.cell(ROWS - 1, 3), Some(Player::Yellow));
    }
}
