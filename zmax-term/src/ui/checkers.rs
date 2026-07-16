//! Checkers — a small terminal draughts game for zmax.
//!
//! Standard 8x8 English draughts against the computer. You are red (`●`, kings
//! `◉`) and move up the board; the computer is white (`○`, kings `◎`) and moves
//! down. Move the cursor with the arrows or `hjkl`, `SPC`/`Enter` picks up the
//! piece under the cursor and a second `SPC`/`Enter` on a highlighted
//! destination makes the move, `n` starts a fresh game and `q`/`Esc` quits.
//! Captures are mandatory: when a jump exists only jumps are offered, and a man
//! reaching the far row is crowned a king. Like the other puzzles this one is
//! turn-based — nothing animates, the board only changes in response to a key.
//! The rules are pure and unit-tested; the computer breaks ties with a small LCG
//! so a given seed is reproducible.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Board is 8x8; playable dark squares are those where `(row + col)` is odd.
const N: usize = 8;

/// Which side a piece (or the side to move) belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Player {
    Red,
    White,
}

/// A single board square.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    Empty,
    Man(Player),
    King(Player),
}

/// A legal move: slide or jump from `from` to `to`, removing every square in
/// `captures` (empty for a plain slide, one or more for a jump chain).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Move {
    pub from: (usize, usize),
    pub to: (usize, usize),
    pub captures: Vec<(usize, usize)>,
}

fn idx(r: usize, c: usize) -> usize {
    r * N + c
}

fn in_bounds(r: i32, c: i32) -> bool {
    r >= 0 && r < N as i32 && c >= 0 && c < N as i32
}

fn opponent(p: Player) -> Player {
    match p {
        Player::Red => Player::White,
        Player::White => Player::Red,
    }
}

/// The diagonal directions a piece may travel: men go forward only (red up the
/// board, white down), kings go every way.
fn piece_dirs(player: Player, king: bool) -> Vec<(i32, i32)> {
    if king {
        return vec![(-1, -1), (-1, 1), (1, -1), (1, 1)];
    }
    match player {
        // Red sits at the bottom and advances toward row 0.
        Player::Red => vec![(-1, -1), (-1, 1)],
        // White sits at the top and advances toward row 7.
        Player::White => vec![(1, -1), (1, 1)],
    }
}

/// `true` when `cell` holds a piece belonging to `player`'s opponent.
fn is_enemy(cell: Cell, player: Player) -> bool {
    match cell {
        Cell::Man(p) | Cell::King(p) => p != player,
        Cell::Empty => false,
    }
}

/// Recursively collect jump chains for a piece standing at `pos` that started at
/// `origin`. `board` is a live snapshot; captured squares are tracked in
/// `captured` (not removed from `board`) so the same man is never jumped twice.
fn gen_captures(
    board: &[Cell],
    player: Player,
    king: bool,
    pos: (usize, usize),
    origin: (usize, usize),
    captured: &mut Vec<(usize, usize)>,
    out: &mut Vec<Move>,
) {
    let mut extended = false;
    for (dr, dc) in piece_dirs(player, king) {
        let mr = pos.0 as i32 + dr;
        let mc = pos.1 as i32 + dc;
        let lr = pos.0 as i32 + 2 * dr;
        let lc = pos.1 as i32 + 2 * dc;
        if !in_bounds(lr, lc) {
            continue;
        }
        let (mr, mc) = (mr as usize, mc as usize);
        let (lr, lc) = (lr as usize, lc as usize);
        if !is_enemy(board[idx(mr, mc)], player) || captured.contains(&(mr, mc)) {
            continue;
        }
        // The landing square must be empty — or the man's own start square,
        // which it vacated at the head of the chain.
        if board[idx(lr, lc)] != Cell::Empty && (lr, lc) != origin {
            continue;
        }
        captured.push((mr, mc));
        extended = true;
        gen_captures(board, player, king, (lr, lc), origin, captured, out);
        captured.pop();
    }
    // A terminal square that captured at least one man is a complete jump.
    if !extended && !captured.is_empty() {
        out.push(Move {
            from: origin,
            to: pos,
            captures: captured.clone(),
        });
    }
}

/// The pure draughts position. No I/O, no timing — unit-tested. The computer's
/// tie-breaking uses the same LCG the other games use, so a seed is deterministic.
#[derive(Clone)]
pub struct Game {
    board: Vec<Cell>,
    turn: Player,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut board = vec![Cell::Empty; N * N];
        for r in 0..N {
            for c in 0..N {
                if (r + c) % 2 == 1 {
                    if r <= 2 {
                        board[idx(r, c)] = Cell::Man(Player::White);
                    } else if r >= 5 {
                        board[idx(r, c)] = Cell::Man(Player::Red);
                    }
                }
            }
        }
        Game {
            board,
            turn: Player::Red,
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

    pub fn turn(&self) -> Player {
        self.turn
    }

    /// How many pieces `player` has on the board.
    pub fn count(&self, player: Player) -> usize {
        self.board
            .iter()
            .filter(|&&c| match c {
                Cell::Man(p) | Cell::King(p) => p == player,
                Cell::Empty => false,
            })
            .count()
    }

    /// Every legal move for `player`. Captures are mandatory: if any jump is
    /// available only jumps are returned; otherwise plain slides are returned.
    pub fn legal_moves(&self, player: Player) -> Vec<Move> {
        let mut captures = Vec::new();
        for r in 0..N {
            for c in 0..N {
                let king = match self.board[idx(r, c)] {
                    Cell::Man(p) if p == player => false,
                    Cell::King(p) if p == player => true,
                    _ => continue,
                };
                let mut chain = Vec::new();
                gen_captures(
                    &self.board,
                    player,
                    king,
                    (r, c),
                    (r, c),
                    &mut chain,
                    &mut captures,
                );
            }
        }
        if !captures.is_empty() {
            return captures;
        }

        let mut slides = Vec::new();
        for r in 0..N {
            for c in 0..N {
                let king = match self.board[idx(r, c)] {
                    Cell::Man(p) if p == player => false,
                    Cell::King(p) if p == player => true,
                    _ => continue,
                };
                for (dr, dc) in piece_dirs(player, king) {
                    let nr = r as i32 + dr;
                    let nc = c as i32 + dc;
                    if in_bounds(nr, nc) && self.board[idx(nr as usize, nc as usize)] == Cell::Empty
                    {
                        slides.push(Move {
                            from: (r, c),
                            to: (nr as usize, nc as usize),
                            captures: Vec::new(),
                        });
                    }
                }
            }
        }
        slides
    }

    /// Apply `m`: vacate `from`, remove the captured men, land on `to`,
    /// promoting a man that reaches the far row, and hand the turn over.
    pub fn apply(&mut self, m: &Move) {
        let moving = self.board[idx(m.from.0, m.from.1)];
        self.board[idx(m.from.0, m.from.1)] = Cell::Empty;
        for &(cr, cc) in &m.captures {
            self.board[idx(cr, cc)] = Cell::Empty;
        }
        let landed = match moving {
            Cell::Man(p) => {
                let back = match p {
                    Player::Red => 0,
                    Player::White => N - 1,
                };
                if m.to.0 == back {
                    Cell::King(p)
                } else {
                    Cell::Man(p)
                }
            }
            other => other,
        };
        self.board[idx(m.to.0, m.to.1)] = landed;
        self.turn = opponent(self.turn);
    }

    /// The winner, if the game is over: a side with no pieces or no legal move
    /// loses. `None` while play continues.
    pub fn winner(&self) -> Option<Player> {
        if self.count(Player::Red) == 0 {
            return Some(Player::White);
        }
        if self.count(Player::White) == 0 {
            return Some(Player::Red);
        }
        if self.legal_moves(self.turn).is_empty() {
            return Some(opponent(self.turn));
        }
        None
    }

    /// Let the computer (white) reply: take the move that captures the most men,
    /// breaking ties with the LCG. Does nothing if it is not white's turn or the
    /// game is already decided.
    pub fn cpu_move(&mut self) {
        if self.turn != Player::White || self.winner().is_some() {
            return;
        }
        let moves = self.legal_moves(Player::White);
        if moves.is_empty() {
            return;
        }
        let best = moves.iter().map(|m| m.captures.len()).max().unwrap_or(0);
        let choices: Vec<&Move> = moves.iter().filter(|m| m.captures.len() == best).collect();
        let pick = (self.rand() as usize) % choices.len();
        let chosen = choices[pick].clone();
        self.apply(&chosen);
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Checkers overlay.
pub struct Checkers {
    game: Game,
    seed: u64,
    cursor: (usize, usize),
    selected: Option<(usize, usize)>,
}

impl Checkers {
    pub fn new() -> Self {
        Checkers {
            game: Game::new(1),
            seed: 1,
            cursor: (5, 0),
            selected: None,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.cursor = (5, 0);
        self.selected = None;
    }

    fn move_cursor(&mut self, dr: i32, dc: i32) {
        let r = (self.cursor.0 as i32 + dr).clamp(0, N as i32 - 1) as usize;
        let c = (self.cursor.1 as i32 + dc).clamp(0, N as i32 - 1) as usize;
        self.cursor = (r, c);
    }

    /// The `SPC`/`Enter` action: pick up a red piece, or — if one is already
    /// held — drop it on a legal destination and let the computer reply.
    fn act(&mut self) {
        if self.game.winner().is_some() || self.game.turn() != Player::Red {
            return;
        }
        let moves = self.game.legal_moves(Player::Red);
        match self.selected {
            Some(from) => {
                if let Some(m) = moves
                    .iter()
                    .find(|m| m.from == from && m.to == self.cursor)
                    .cloned()
                {
                    self.game.apply(&m);
                    self.selected = None;
                    self.game.cpu_move();
                } else if moves.iter().any(|m| m.from == self.cursor) {
                    // Tapping another movable piece re-selects it.
                    self.selected = Some(self.cursor);
                } else {
                    self.selected = None;
                }
            }
            None => {
                if moves.iter().any(|m| m.from == self.cursor) {
                    self.selected = Some(self.cursor);
                }
            }
        }
    }
}

impl Default for Checkers {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Checkers {
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
            key!(Left) | key!('h') => self.move_cursor(0, -1),
            key!(Right) | key!('l') => self.move_cursor(0, 1),
            key!(Up) | key!('k') => self.move_cursor(-1, 0),
            key!(Down) | key!('j') => self.move_cursor(1, 0),
            key!(' ') | key!(Enter) => self.act(),
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
        let dark_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let red_man = theme.get("warning");
        let red_king = theme.get("error");
        let white_man = theme.get("function");
        let white_king = theme.get("ui.text.focus");
        let hint_style = theme.get("ui.text");

        surface.clear_with(area, bg);
        // Each square is drawn two columns wide; leave room for two footer lines.
        if area.width < (N as u16) * 2 + 4 || area.height < (N as u16) + 5 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        surface.set_string(ox, area.y, "Checkers  you R  —  W cpu", header_style);

        // Highlight the squares the held piece may legally reach.
        let mut hints: Vec<(usize, usize)> = Vec::new();
        if let Some(from) = self.selected {
            for m in self.game.legal_moves(Player::Red) {
                if m.from == from {
                    hints.push(m.to);
                }
            }
        }

        for r in 0..N {
            for c in 0..N {
                let x = ox + (c as u16) * 2;
                let y = oy + r as u16;
                let highlighted = self.cursor == (r, c) || self.selected == Some((r, c));
                let base = if highlighted {
                    sel_style
                } else if (r + c) % 2 == 1 {
                    dark_style
                } else {
                    bg
                };
                surface.set_string(x, y, "  ", base);

                let (glyph, glyph_style) = match self.game.board[idx(r, c)] {
                    Cell::Man(Player::Red) => ("●", red_man),
                    Cell::King(Player::Red) => ("◉", red_king),
                    Cell::Man(Player::White) => ("○", white_man),
                    Cell::King(Player::White) => ("◎", white_king),
                    Cell::Empty => {
                        if !highlighted && hints.contains(&(r, c)) {
                            surface.set_string(x, y, "·", hint_style);
                        }
                        continue;
                    }
                };
                let style = if highlighted { sel_style } else { glyph_style };
                surface.set_string(x, y, glyph, style);
            }
        }

        let status = match self.game.winner() {
            Some(Player::Red) => "you win!".to_string(),
            Some(Player::White) => "cpu wins".to_string(),
            None => {
                if self.selected.is_some() {
                    "choose a destination".to_string()
                } else {
                    "your move".to_string()
                }
            }
        };
        let info_y = oy + N as u16;
        surface.set_string(
            ox,
            info_y,
            &format!(
                "R {}  —  W {}   [{}]",
                self.game.count(Player::Red),
                self.game.count(Player::White),
                status
            ),
            text_style,
        );
        surface.set_string(
            ox,
            info_y + 1,
            "hjkl/arrows move · SPC select/move · n new · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty board with red to move, ready for hand placement.
    fn blank() -> Game {
        Game {
            board: vec![Cell::Empty; N * N],
            turn: Player::Red,
            rng: 1,
        }
    }

    #[test]
    fn simple_forward_move_is_legal_and_applies() {
        let mut g = blank();
        g.board[idx(5, 2)] = Cell::Man(Player::Red);
        let moves = g.legal_moves(Player::Red);
        // Red advances up the board to either forward diagonal.
        let m = moves
            .iter()
            .find(|m| m.to == (4, 1) && m.captures.is_empty())
            .expect("forward slide is legal")
            .clone();
        g.apply(&m);
        assert_eq!(g.board[idx(4, 1)], Cell::Man(Player::Red));
        assert_eq!(g.board[idx(5, 2)], Cell::Empty);
    }

    #[test]
    fn capture_removes_the_jumped_man_and_lands_beyond() {
        let mut g = blank();
        g.board[idx(5, 2)] = Cell::Man(Player::Red);
        g.board[idx(4, 3)] = Cell::Man(Player::White);
        let moves = g.legal_moves(Player::Red);
        let m = moves
            .iter()
            .find(|m| m.to == (3, 4))
            .expect("the jump is offered")
            .clone();
        assert_eq!(m.captures, vec![(4, 3)]);
        g.apply(&m);
        assert_eq!(g.board[idx(3, 4)], Cell::Man(Player::Red));
        assert_eq!(g.board[idx(4, 3)], Cell::Empty, "jumped man is removed");
        assert_eq!(g.board[idx(5, 2)], Cell::Empty);
    }

    #[test]
    fn man_reaching_the_back_row_is_promoted() {
        let mut g = blank();
        g.board[idx(1, 2)] = Cell::Man(Player::Red);
        let moves = g.legal_moves(Player::Red);
        let m = moves.iter().find(|m| m.to == (0, 1)).unwrap().clone();
        g.apply(&m);
        assert_eq!(g.board[idx(0, 1)], Cell::King(Player::Red));
    }

    #[test]
    fn a_capture_excludes_every_plain_slide() {
        let mut g = blank();
        g.board[idx(5, 2)] = Cell::Man(Player::Red);
        g.board[idx(4, 3)] = Cell::Man(Player::White);
        let moves = g.legal_moves(Player::Red);
        assert!(!moves.is_empty());
        assert!(
            moves.iter().all(|m| !m.captures.is_empty()),
            "mandatory capture leaves only jumps"
        );
        assert!(
            !moves.iter().any(|m| m.to == (4, 1)),
            "the non-capturing slide is filtered out"
        );
    }

    #[test]
    fn winner_when_a_side_has_no_pieces() {
        let mut g = blank();
        g.board[idx(5, 2)] = Cell::Man(Player::Red);
        g.turn = Player::White;
        assert_eq!(g.winner(), Some(Player::Red), "white has no pieces");
    }

    #[test]
    fn cpu_takes_the_available_capture() {
        let mut g = blank();
        // A white man that can jump a red, plus one that can only shuffle.
        g.board[idx(2, 3)] = Cell::Man(Player::White);
        g.board[idx(3, 4)] = Cell::Man(Player::Red);
        g.board[idx(2, 1)] = Cell::Man(Player::White);
        g.turn = Player::White;
        g.cpu_move();
        assert_eq!(g.board[idx(3, 4)], Cell::Empty, "the red man was jumped");
        assert_eq!(g.board[idx(4, 5)], Cell::Man(Player::White));
    }
}
