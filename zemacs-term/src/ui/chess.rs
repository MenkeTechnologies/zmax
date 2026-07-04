//! Chess — a small turn-based chess game for zemacs, human (white) vs computer.
//!
//! You play white from the bottom of the board against the computer's black.
//! Move the cursor with the arrows or `hjkl`; `SPC`/`RET` selects the piece
//! under the cursor and, pressed again on a legal destination, moves it (a
//! two-step select-then-move). `n` starts a new game and `q`/`Esc` quits. Like
//! the other board games nothing animates: the position only changes in response
//! to a key. The engine — pseudo-legal move generation, legality filtering by
//! check, check/checkmate/stalemate detection and a small material search for
//! the computer — is pure and unit-tested. A tiny LCG breaks ties in the
//! computer's choice so a given seed is reproducible.

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The six kinds of chess piece.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

/// A piece is its kind plus its colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Piece {
    pub kind: Kind,
    pub white: bool,
}

/// The state of the side to move.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Playing,
    Check,
    Checkmate,
    Stalemate,
}

fn value(k: Kind) -> i32 {
    match k {
        Kind::Pawn => 1,
        Kind::Knight => 3,
        Kind::Bishop => 3,
        Kind::Rook => 5,
        Kind::Queen => 9,
        Kind::King => 0,
    }
}

fn letter(k: Kind) -> char {
    match k {
        Kind::Pawn => 'P',
        Kind::Knight => 'N',
        Kind::Bishop => 'B',
        Kind::Rook => 'R',
        Kind::Queen => 'Q',
        Kind::King => 'K',
    }
}

const KNIGHT: [(i32, i32); 8] = [
    (-2, -1),
    (-2, 1),
    (-1, -2),
    (-1, 2),
    (1, -2),
    (1, 2),
    (2, -1),
    (2, 1),
];
const DIAG: [(i32, i32); 4] = [(-1, -1), (-1, 1), (1, -1), (1, 1)];
const ORTHO: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
const QUEEN: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 1),
    (1, -1),
    (1, 1),
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
];

fn on_board(r: i32, c: i32) -> bool {
    r >= 0 && r < 8 && c >= 0 && c < 8
}

/// The pure chess position. No I/O, no timing — unit-tested. `Game::new(seed)`
/// sets up the standard start position and seeds the tie-break LCG.
#[derive(Clone)]
pub struct Game {
    /// `board[row][col]`, row 0 at the top (black), row 7 at the bottom (white).
    board: [[Option<Piece>; 8]; 8],
    white_to_move: bool,
    /// White pieces captured by black.
    captured_white: Vec<Kind>,
    /// Black pieces captured by white.
    captured_black: Vec<Kind>,
    rng: u64,
}

fn start_board() -> [[Option<Piece>; 8]; 8] {
    let mut b = [[None; 8]; 8];
    let back = [
        Kind::Rook,
        Kind::Knight,
        Kind::Bishop,
        Kind::Queen,
        Kind::King,
        Kind::Bishop,
        Kind::Knight,
        Kind::Rook,
    ];
    for c in 0..8 {
        b[0][c] = Some(Piece {
            kind: back[c],
            white: false,
        });
        b[1][c] = Some(Piece {
            kind: Kind::Pawn,
            white: false,
        });
        b[6][c] = Some(Piece {
            kind: Kind::Pawn,
            white: true,
        });
        b[7][c] = Some(Piece {
            kind: back[c],
            white: true,
        });
    }
    b
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Game {
            board: start_board(),
            white_to_move: true,
            captured_white: Vec::new(),
            captured_black: Vec::new(),
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

    pub fn white_to_move(&self) -> bool {
        self.white_to_move
    }

    pub fn at(&self, r: usize, c: usize) -> Option<Piece> {
        self.board[r][c]
    }

    fn find_king(&self, white: bool) -> Option<(usize, usize)> {
        for r in 0..8 {
            for c in 0..8 {
                if let Some(p) = self.board[r][c] {
                    if p.white == white && p.kind == Kind::King {
                        return Some((r, c));
                    }
                }
            }
        }
        None
    }

    /// Pseudo-legal destinations for the piece at `from` (no check filtering).
    fn pseudo_moves(&self, from: (usize, usize)) -> Vec<(usize, usize)> {
        let p = match self.board[from.0][from.1] {
            Some(p) => p,
            None => return Vec::new(),
        };
        let (r, c) = (from.0 as i32, from.1 as i32);
        let mut moves = Vec::new();
        let step = |moves: &mut Vec<(usize, usize)>, nr: i32, nc: i32| {
            if on_board(nr, nc) {
                match self.board[nr as usize][nc as usize] {
                    Some(t) if t.white == p.white => {}
                    _ => moves.push((nr as usize, nc as usize)),
                }
            }
        };
        match p.kind {
            Kind::Pawn => {
                let dir = if p.white { -1 } else { 1 };
                let start = if p.white { 6 } else { 1 };
                let nr = r + dir;
                if on_board(nr, c) && self.board[nr as usize][c as usize].is_none() {
                    moves.push((nr as usize, c as usize));
                    if from.0 as i32 == start {
                        let nr2 = r + 2 * dir;
                        if self.board[nr2 as usize][c as usize].is_none() {
                            moves.push((nr2 as usize, c as usize));
                        }
                    }
                }
                for dc in [-1, 1] {
                    let nc = c + dc;
                    if on_board(nr, nc) {
                        if let Some(t) = self.board[nr as usize][nc as usize] {
                            if t.white != p.white {
                                moves.push((nr as usize, nc as usize));
                            }
                        }
                    }
                }
            }
            Kind::Knight => {
                for (dr, dc) in KNIGHT {
                    step(&mut moves, r + dr, c + dc);
                }
            }
            Kind::King => {
                for dr in -1..=1 {
                    for dc in -1..=1 {
                        if dr != 0 || dc != 0 {
                            step(&mut moves, r + dr, c + dc);
                        }
                    }
                }
            }
            Kind::Bishop | Kind::Rook | Kind::Queen => {
                let dirs: &[(i32, i32)] = match p.kind {
                    Kind::Bishop => &DIAG,
                    Kind::Rook => &ORTHO,
                    _ => &QUEEN,
                };
                for &(dr, dc) in dirs {
                    let (mut nr, mut nc) = (r + dr, c + dc);
                    while on_board(nr, nc) {
                        match self.board[nr as usize][nc as usize] {
                            None => moves.push((nr as usize, nc as usize)),
                            Some(t) => {
                                if t.white != p.white {
                                    moves.push((nr as usize, nc as usize));
                                }
                                break;
                            }
                        }
                        nr += dr;
                        nc += dc;
                    }
                }
            }
        }
        moves
    }

    /// A clone with the raw move applied: piece relocated, promotion to queen on
    /// the last rank. Turn and capture bookkeeping are left untouched.
    fn moved(&self, from: (usize, usize), to: (usize, usize)) -> Game {
        let mut g = self.clone();
        g.make_raw(from, to);
        g
    }

    fn make_raw(&mut self, from: (usize, usize), to: (usize, usize)) {
        let mut p = self.board[from.0][from.1].take();
        if let Some(ref mut pc) = p {
            if pc.kind == Kind::Pawn && ((pc.white && to.0 == 0) || (!pc.white && to.0 == 7)) {
                pc.kind = Kind::Queen;
            }
        }
        self.board[to.0][to.1] = p;
    }

    /// Whether square `(r, c)` is attacked by a piece of colour `by_white`.
    fn is_attacked(&self, r: usize, c: usize, by_white: bool) -> bool {
        let (ri, ci) = (r as i32, c as i32);
        for (dr, dc) in KNIGHT {
            let (nr, nc) = (ri + dr, ci + dc);
            if on_board(nr, nc) {
                if let Some(p) = self.board[nr as usize][nc as usize] {
                    if p.white == by_white && p.kind == Kind::Knight {
                        return true;
                    }
                }
            }
        }
        for dr in -1..=1 {
            for dc in -1..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let (nr, nc) = (ri + dr, ci + dc);
                if on_board(nr, nc) {
                    if let Some(p) = self.board[nr as usize][nc as usize] {
                        if p.white == by_white && p.kind == Kind::King {
                            return true;
                        }
                    }
                }
            }
        }
        // A pawn of `by_white` attacks diagonally toward the far side: a white
        // pawn sits one row below the square it attacks, a black pawn one above.
        let pr = if by_white { ri + 1 } else { ri - 1 };
        for dc in [-1, 1] {
            let nc = ci + dc;
            if on_board(pr, nc) {
                if let Some(p) = self.board[pr as usize][nc as usize] {
                    if p.white == by_white && p.kind == Kind::Pawn {
                        return true;
                    }
                }
            }
        }
        for (dr, dc) in DIAG {
            let (mut nr, mut nc) = (ri + dr, ci + dc);
            while on_board(nr, nc) {
                if let Some(p) = self.board[nr as usize][nc as usize] {
                    if p.white == by_white && (p.kind == Kind::Bishop || p.kind == Kind::Queen) {
                        return true;
                    }
                    break;
                }
                nr += dr;
                nc += dc;
            }
        }
        for (dr, dc) in ORTHO {
            let (mut nr, mut nc) = (ri + dr, ci + dc);
            while on_board(nr, nc) {
                if let Some(p) = self.board[nr as usize][nc as usize] {
                    if p.white == by_white && (p.kind == Kind::Rook || p.kind == Kind::Queen) {
                        return true;
                    }
                    break;
                }
                nr += dr;
                nc += dc;
            }
        }
        false
    }

    /// Whether `white`'s king is currently in check.
    pub fn in_check(&self, white: bool) -> bool {
        match self.find_king(white) {
            Some((r, c)) => self.is_attacked(r, c, !white),
            None => false,
        }
    }

    /// Fully legal destinations for the piece at `from`: pseudo-legal moves with
    /// any that leave the mover's own king in check removed.
    pub fn legal_moves(&self, from: (usize, usize)) -> Vec<(usize, usize)> {
        let p = match self.board[from.0][from.1] {
            Some(p) => p,
            None => return Vec::new(),
        };
        self.pseudo_moves(from)
            .into_iter()
            .filter(|&to| !self.moved(from, to).in_check(p.white))
            .collect()
    }

    /// Every legal `(from, to)` for the given colour.
    fn all_legal_moves(&self, white: bool) -> Vec<((usize, usize), (usize, usize))> {
        let mut out = Vec::new();
        for r in 0..8 {
            for c in 0..8 {
                if let Some(p) = self.board[r][c] {
                    if p.white == white {
                        for to in self.legal_moves((r, c)) {
                            out.push(((r, c), to));
                        }
                    }
                }
            }
        }
        out
    }

    /// Apply a move for real: record any capture, relocate (with promotion) and
    /// hand the turn to the other side.
    pub fn apply(&mut self, from: (usize, usize), to: (usize, usize)) {
        if let Some(cap) = self.board[to.0][to.1] {
            if cap.white {
                self.captured_white.push(cap.kind);
            } else {
                self.captured_black.push(cap.kind);
            }
        }
        self.make_raw(from, to);
        self.white_to_move = !self.white_to_move;
    }

    /// Material balance from black's point of view (black total − white total).
    fn evaluate(&self) -> i32 {
        let mut score = 0;
        for row in &self.board {
            for cell in row {
                if let Some(p) = cell {
                    let v = value(p.kind);
                    if p.white {
                        score -= v;
                    } else {
                        score += v;
                    }
                }
            }
        }
        score
    }

    /// The state of the side to move.
    pub fn status(&self) -> Status {
        let side = self.white_to_move;
        let has_moves = !self.all_legal_moves(side).is_empty();
        let check = self.in_check(side);
        if !has_moves {
            if check {
                Status::Checkmate
            } else {
                Status::Stalemate
            }
        } else if check {
            Status::Check
        } else {
            Status::Playing
        }
    }

    /// Choose and play black's move: the legal move maximising material after a
    /// 2-ply search (black plays, white makes its best material reply), with the
    /// LCG breaking ties. A no-op if black has no legal moves.
    pub fn cpu_move(&mut self) {
        let moves = self.all_legal_moves(false);
        if moves.is_empty() {
            return;
        }
        let mut best_score = i32::MIN;
        let mut best: Vec<((usize, usize), (usize, usize))> = Vec::new();
        for &(from, to) in &moves {
            let mut after = self.clone();
            after.apply(from, to);
            let replies = after.all_legal_moves(true);
            let score = if replies.is_empty() {
                // White has no reply: mate is ideal, stalemate is neutral.
                if after.in_check(true) {
                    100_000
                } else {
                    0
                }
            } else {
                replies
                    .iter()
                    .map(|&(f, t)| {
                        let mut c = after.clone();
                        c.apply(f, t);
                        c.evaluate()
                    })
                    .min()
                    .unwrap()
            };
            if score > best_score {
                best_score = score;
                best.clear();
                best.push((from, to));
            } else if score == best_score {
                best.push((from, to));
            }
        }
        let pick = (self.rand() as usize) % best.len();
        let (from, to) = best[pick];
        self.apply(from, to);
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Chess overlay.
pub struct Chess {
    game: Game,
    cursor: (usize, usize),
    selected: Option<(usize, usize)>,
    seed: u64,
}

impl Chess {
    pub fn new() -> Self {
        Chess {
            game: Game::new(1),
            cursor: (7, 4),
            selected: None,
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.cursor = (7, 4);
        self.selected = None;
    }

    fn game_over(&self) -> bool {
        matches!(self.game.status(), Status::Checkmate | Status::Stalemate)
    }

    /// The two-step select-then-move action bound to `SPC`/`RET`.
    fn on_select(&mut self) {
        if self.game_over() || !self.game.white_to_move() {
            return;
        }
        let sq = self.cursor;
        match self.selected {
            None => {
                if let Some(p) = self.game.at(sq.0, sq.1) {
                    if p.white {
                        self.selected = Some(sq);
                    }
                }
            }
            Some(from) => {
                if from == sq {
                    self.selected = None;
                } else if self.game.legal_moves(from).contains(&sq) {
                    self.game.apply(from, sq);
                    self.selected = None;
                    // Let the computer reply if the game continues.
                    if !self.game_over() && !self.game.white_to_move() {
                        self.game.cpu_move();
                    }
                } else if let Some(p) = self.game.at(sq.0, sq.1) {
                    // Clicking another of your own pieces reselects it.
                    self.selected = if p.white { Some(sq) } else { None };
                } else {
                    self.selected = None;
                }
            }
        }
    }
}

impl Default for Chess {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Chess {
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
            key!(Left) | key!('h') => self.cursor.1 = self.cursor.1.saturating_sub(1),
            key!(Right) | key!('l') => self.cursor.1 = (self.cursor.1 + 1).min(7),
            key!(Up) | key!('k') => self.cursor.0 = self.cursor.0.saturating_sub(1),
            key!(Down) | key!('j') => self.cursor.0 = (self.cursor.0 + 1).min(7),
            key!(' ') | key!(Enter) => self.on_select(),
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
        let dark_style = theme.get("ui.linenr");
        let white_style = theme.get("ui.text.focus");
        let black_style = theme.get("warning");
        let cursor_style = theme.get("ui.selection");
        let hint_style = theme.get("function");
        let check_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 8 * 2 + 4 || area.height < 8 + 6 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        // Header: whose turn and the game state.
        let turn = if self.game.white_to_move() {
            "White"
        } else {
            "Black"
        };
        let head = match self.game.status() {
            Status::Playing => format!("Chess  {} to move", turn),
            Status::Check => format!("Chess  {} to move — CHECK", turn),
            Status::Checkmate => format!("Chess  Checkmate — {} loses", turn),
            Status::Stalemate => "Chess  Stalemate — draw".to_string(),
        };
        surface.set_string(ox, area.y, &head, header_style);

        // Legal destinations for the currently selected piece, to hint the move.
        let hints: Vec<(usize, usize)> = match self.selected {
            Some(from) => self.game.legal_moves(from),
            None => Vec::new(),
        };

        for r in 0..8usize {
            for c in 0..8usize {
                let x = ox + (c as u16) * 2;
                let y = oy + r as u16;
                let (glyph, mut style) = match self.game.at(r, c) {
                    Some(p) => {
                        let ch = if p.white {
                            letter(p.kind)
                        } else {
                            letter(p.kind).to_ascii_lowercase()
                        };
                        let mut st = if p.white { white_style } else { black_style };
                        if p.kind == Kind::King && self.game.in_check(p.white) {
                            st = check_style;
                        }
                        (ch.to_string(), st)
                    }
                    None => {
                        // Empty squares alternate to read as a checkerboard.
                        let st = if (r + c) % 2 == 0 {
                            text_style
                        } else {
                            dark_style
                        };
                        ("·".to_string(), st)
                    }
                };
                if hints.contains(&(r, c)) {
                    style = hint_style;
                }
                if self.selected == Some((r, c)) || self.cursor == (r, c) {
                    style = cursor_style;
                }
                surface.set_string(x, y, &glyph, style);
            }
        }

        // Captured material.
        let mut cap = String::from("Taken:");
        for k in &self.game.captured_black {
            cap.push(' ');
            cap.push(letter(*k).to_ascii_lowercase());
        }
        cap.push_str("  /");
        for k in &self.game.captured_white {
            cap.push(' ');
            cap.push(letter(*k));
        }
        let cy = oy + 8 + 1;
        surface.set_string(ox, cy, &cap, text_style);
        surface.set_string(
            ox,
            cy + 1,
            "hjkl/arrows move · SPC select/move · n new · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cleared board with white to move, ready for hand placement.
    fn empty() -> Game {
        Game {
            board: [[None; 8]; 8],
            white_to_move: true,
            captured_white: Vec::new(),
            captured_black: Vec::new(),
            rng: 1,
        }
    }

    fn set(g: &mut Game, r: usize, c: usize, kind: Kind, white: bool) {
        g.board[r][c] = Some(Piece { kind, white });
    }

    /// Both kings, parked far apart so they never interfere with a test.
    fn kings(g: &mut Game, wk: (usize, usize), bk: (usize, usize)) {
        set(g, wk.0, wk.1, Kind::King, true);
        set(g, bk.0, bk.1, Kind::King, false);
    }

    fn sorted(mut v: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
        v.sort();
        v
    }

    #[test]
    fn knight_opening_moves() {
        // The b1 knight (row 7, col 1) opens to a3 and c3 only; d2 is its own pawn.
        let g = Game::new(1);
        let moves = sorted(g.legal_moves((7, 1)));
        assert_eq!(moves, vec![(5, 0), (5, 2)]);
    }

    #[test]
    fn bishop_blocked_at_start() {
        // The c1 bishop is hemmed in by its own pawns and back rank: no moves.
        let g = Game::new(1);
        assert!(g.legal_moves((7, 2)).is_empty());
    }

    #[test]
    fn pawn_promotes_to_queen() {
        let mut g = empty();
        kings(&mut g, (7, 7), (0, 7));
        set(&mut g, 1, 3, Kind::Pawn, true);
        g.apply((1, 3), (0, 3));
        assert_eq!(
            g.board[0][3],
            Some(Piece {
                kind: Kind::Queen,
                white: true,
            })
        );
    }

    #[test]
    fn moving_into_check_is_illegal() {
        // A black rook on column 3 forbids the white king stepping onto column 3.
        let mut g = empty();
        set(&mut g, 7, 4, Kind::King, true);
        set(&mut g, 0, 0, Kind::King, false);
        set(&mut g, 0, 3, Kind::Rook, false);
        let moves = g.legal_moves((7, 4));
        assert!(
            !moves.contains(&(7, 3)),
            "stepping onto the rook file is illegal"
        );
        assert!(!moves.contains(&(6, 3)), "the rook file is attacked");
        assert!(moves.contains(&(7, 5)), "stepping away stays legal");
    }

    #[test]
    fn back_rank_checkmate() {
        // King boxed in by its own pawns; a black rook mates along the back rank.
        let mut g = empty();
        set(&mut g, 7, 7, Kind::King, true);
        set(&mut g, 6, 6, Kind::Pawn, true);
        set(&mut g, 6, 7, Kind::Pawn, true);
        set(&mut g, 7, 0, Kind::Rook, false);
        set(&mut g, 0, 0, Kind::King, false);
        assert!(g.in_check(true));
        assert_eq!(g.status(), Status::Checkmate);
    }

    #[test]
    fn constructed_stalemate() {
        // Black king on a8, no legal move, but not in check — stalemate.
        let mut g = empty();
        g.white_to_move = false;
        set(&mut g, 0, 0, Kind::King, false);
        set(&mut g, 1, 2, Kind::Queen, true);
        set(&mut g, 7, 7, Kind::King, true);
        assert!(!g.in_check(false));
        assert_eq!(g.status(), Status::Stalemate);
    }

    #[test]
    fn cpu_grabs_a_free_queen() {
        // Black knight can capture an undefended white queen; the search takes it.
        let mut g = empty();
        g.white_to_move = false;
        kings(&mut g, (7, 7), (0, 0));
        set(&mut g, 4, 4, Kind::Knight, false);
        set(&mut g, 2, 3, Kind::Queen, true);
        g.cpu_move();
        assert_eq!(
            g.board[2][3],
            Some(Piece {
                kind: Kind::Knight,
                white: false,
            }),
            "black should capture the hanging queen"
        );
    }
}
