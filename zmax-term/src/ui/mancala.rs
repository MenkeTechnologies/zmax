//! Mancala — a small terminal Kalah for zmax.
//!
//! The classic two-rank sowing game. You own the bottom six pits and the store
//! on your right; the computer owns the top six and the store on its left. Move
//! the cursor across your pits with the arrows or `h`/`l`, `SPC`/`RET` sows the
//! selected pit, `n` starts a fresh board and `q`/`Esc` quits. Like Minesweeper
//! this one is turn-based: nothing animates, so there is no frame loop — the
//! board only changes in response to a key, and the computer takes its whole
//! (possibly chained) turn immediately after yours. The board logic is pure and
//! unit-tested; the computer breaks heuristic ties with the same LCG the other
//! games use, so a given seed is reproducible.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The two players. `You` own pits 0..=5 and store 6; `Cpu` owns pits 7..=12
/// and store 13.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    You,
    Cpu,
}

/// Who, if anyone, has the larger store once the board is empty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Winner {
    You,
    Cpu,
    Tie,
}

/// The store slot for a side.
const YOU_STORE: usize = 6;
const CPU_STORE: usize = 13;

/// What one sowing did, used both by the public `sow` and the CPU heuristic.
struct SowResult {
    /// The last seed landed in the mover's own store: they take another turn.
    extra: bool,
    /// Seeds pulled in by a capture (0 when no capture happened).
    captured: u32,
    /// Net seeds the mover added to their own store this turn.
    gain: u32,
}

/// The pure Kalah board. No I/O, no timing — unit-tested. The CPU's tie-breaks
/// use the same LCG the other games use, so `Game::new(seed)` is deterministic.
#[derive(Clone)]
pub struct Game {
    /// Seed counts: 0..=5 your pits, 6 your store, 7..=12 the cpu's pits, 13
    /// the cpu's store.
    pits: [u32; 14],
    /// Whose move it is.
    turn: Side,
    /// Cursor over your pits, 0..=5.
    cursor: usize,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Game {
            pits: [4, 4, 4, 4, 4, 4, 0, 4, 4, 4, 4, 4, 4, 0],
            turn: Side::You,
            cursor: 0,
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

    pub fn pits(&self) -> &[u32; 14] {
        &self.pits
    }

    pub fn turn(&self) -> Side {
        self.turn
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn score(&self, side: Side) -> u32 {
        match side {
            Side::You => self.pits[YOU_STORE],
            Side::Cpu => self.pits[CPU_STORE],
        }
    }

    /// Move the cursor across your six pits, clamped to the board.
    pub fn move_cursor(&mut self, d: i32) {
        self.cursor = (self.cursor as i32 + d).clamp(0, 5) as usize;
    }

    /// Non-empty pits a side may legally sow from.
    pub fn legal(&self, side: Side) -> Vec<usize> {
        let range = match side {
            Side::You => 0..=5,
            Side::Cpu => 7..=12,
        };
        range.filter(|&i| self.pits[i] > 0).collect()
    }

    /// Sow the seeds from `pit` counterclockwise, skipping the opponent's store,
    /// and resolve a capture if the last seed lands in one of the mover's own
    /// empty pits opposite a loaded opponent pit. Returns whether the mover
    /// earned another turn (the last seed landed in their own store). The mover
    /// is inferred from the pit's owner.
    pub fn sow(&mut self, pit: usize) -> bool {
        self.apply(pit).extra
    }

    fn apply(&mut self, pit: usize) -> SowResult {
        let side = if pit <= 5 { Side::You } else { Side::Cpu };
        let (own_store, opp_store): (usize, usize) = match side {
            Side::You => (YOU_STORE, CPU_STORE),
            Side::Cpu => (CPU_STORE, YOU_STORE),
        };
        let before = self.pits[own_store];

        let mut seeds = self.pits[pit];
        self.pits[pit] = 0;
        let mut i = pit;
        while seeds > 0 {
            i = (i + 1) % 14;
            if i == opp_store {
                continue;
            }
            self.pits[i] += 1;
            seeds -= 1;
        }

        // Capture: the last seed just made one of the mover's own pits hold
        // exactly one seed (it was empty), and the opposite pit is loaded.
        let own_pit = match side {
            Side::You => i <= 5,
            Side::Cpu => (7..=12).contains(&i),
        };
        let mut captured = 0;
        if own_pit && self.pits[i] == 1 {
            let opposite = 12 - i;
            if self.pits[opposite] > 0 {
                captured = self.pits[opposite] + 1;
                self.pits[own_store] += captured;
                self.pits[opposite] = 0;
                self.pits[i] = 0;
            }
        }

        SowResult {
            extra: i == own_store,
            captured,
            gain: self.pits[own_store] - before,
        }
    }

    /// A side's six pits are all empty, so the game is finished.
    pub fn game_over(&self) -> bool {
        (0..=5).all(|i| self.pits[i] == 0) || (7..=12).all(|i| self.pits[i] == 0)
    }

    /// Rake each side's remaining pit seeds into that side's store. Called once
    /// the game is over so the final scores reflect the whole board.
    fn sweep(&mut self) {
        for i in 0..=5 {
            self.pits[YOU_STORE] += self.pits[i];
            self.pits[i] = 0;
        }
        for i in 7..=12 {
            self.pits[CPU_STORE] += self.pits[i];
            self.pits[i] = 0;
        }
    }

    /// Whoever holds the larger store, once the board has been swept.
    pub fn winner(&self) -> Winner {
        let you = self.pits[YOU_STORE];
        let cpu = self.pits[CPU_STORE];
        if you > cpu {
            Winner::You
        } else if cpu > you {
            Winner::Cpu
        } else {
            Winner::Tie
        }
    }

    /// If the game just ended, sweep the remainder into the stores.
    fn finish_if_over(&mut self) -> bool {
        if self.game_over() {
            self.sweep();
            true
        } else {
            false
        }
    }

    /// Sow the pit under the cursor (the interactive `SPC`/`RET` action) and, if
    /// the turn passes, let the computer play out its side.
    pub fn play_cursor(&mut self) {
        if self.turn != Side::You || self.game_over() {
            return;
        }
        let pit = self.cursor;
        if self.pits[pit] == 0 {
            return;
        }
        let extra = self.sow(pit);
        if self.finish_if_over() {
            return;
        }
        if !extra {
            self.turn = Side::Cpu;
            self.run_cpu();
        }
    }

    /// Play the computer's (possibly chained) turn until control returns to you.
    fn run_cpu(&mut self) {
        while self.turn == Side::Cpu && !self.game_over() {
            let pit = match self.cpu_pick() {
                Some(p) => p,
                None => break,
            };
            let extra = self.sow(pit);
            if self.finish_if_over() {
                return;
            }
            if !extra {
                self.turn = Side::You;
            }
        }
    }

    /// The computer's heuristic: prefer a move that lands in its store (an extra
    /// turn), then a capturing move, then the move dropping the most seeds into
    /// its store. Ties are broken with the LCG.
    fn cpu_pick(&mut self) -> Option<usize> {
        let moves = self.legal(Side::Cpu);
        if moves.is_empty() {
            return None;
        }
        let scored: Vec<(usize, (u32, u32, u32))> = moves
            .iter()
            .map(|&p| {
                let r = self.clone().apply(p);
                (p, (r.extra as u32, (r.captured > 0) as u32, r.gain))
            })
            .collect();
        let best = scored.iter().map(|&(_, k)| k).max().unwrap();
        let winners: Vec<usize> = scored
            .iter()
            .filter(|&&(_, k)| k == best)
            .map(|&(p, _)| p)
            .collect();
        Some(winners[(self.rand() as usize) % winners.len()])
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Mancala overlay.
pub struct Mancala {
    game: Game,
    seed: u64,
}

impl Mancala {
    pub fn new() -> Self {
        Mancala {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Mancala {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Mancala {
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
            key!(' ') | key!(Enter) => self.game.play_cursor(),
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
        let dim_style = theme.get("ui.linenr");
        let cursor_style = theme.get("ui.selection");
        let store_style = theme.get("function");
        let win_style = theme.get("warning");
        let lose_style = theme.get("error");

        surface.clear_with(area, bg);
        // Layout: a left store, six pit columns five wide, and a right store.
        if area.width < 44 || area.height < 10 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        let pcol = |j: u16| ox + 5 + j * 5;
        let right_store_x = pcol(6);

        surface.set_string(
            ox,
            area.y,
            &format!(
                "Mancala  you {}  —  {} cpu",
                self.game.score(Side::You),
                self.game.score(Side::Cpu)
            ),
            header_style,
        );

        let pits = self.game.pits();
        let you_active = self.game.turn() == Side::You && !self.game.game_over();
        let cpu_active = self.game.turn() == Side::Cpu && !self.game.game_over();

        // Top rank: the cpu's pits 12..=7, drawn left-to-right (right-to-left in
        // pit order) so the flow reads counterclockwise.
        for j in 0..6u16 {
            let pit = 12 - j as usize;
            let style = if cpu_active { header_style } else { dim_style };
            surface.set_string(pcol(j), oy, &format!("{:>3}", pits[pit]), style);
        }
        // The stores sit at the ends, spanning the middle row.
        surface.set_string(
            ox,
            oy + 1,
            &format!("[{:>2}]", pits[CPU_STORE]),
            store_style,
        );
        surface.set_string(
            right_store_x,
            oy + 1,
            &format!("[{:>2}]", pits[YOU_STORE]),
            store_style,
        );
        // Bottom rank: your pits 0..=5, left-to-right, cursor highlighted.
        for j in 0..6u16 {
            let pit = j as usize;
            let style = if pit == self.game.cursor() {
                cursor_style
            } else if you_active {
                header_style
            } else {
                dim_style
            };
            surface.set_string(pcol(j), oy + 2, &format!("{:>3}", pits[pit]), style);
        }

        let sy = oy + 4;
        if self.game.game_over() {
            let (msg, style) = match self.game.winner() {
                Winner::You => ("you win!", win_style),
                Winner::Cpu => ("cpu wins", lose_style),
                Winner::Tie => ("a tie", text_style),
            };
            surface.set_string(ox, sy, &format!("Game over — {}", msg), style);
            surface.set_string(ox, sy + 1, "n new · q quit", text_style);
        } else {
            let whose = if you_active {
                "your move"
            } else {
                "cpu thinking"
            };
            surface.set_string(ox, sy, whose, text_style);
            surface.set_string(
                ox,
                sy + 1,
                "←/→ or h/l move · SPC/RET sow · n new · q quit",
                text_style,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board with hand-set counts, your move, fixed RNG.
    fn board(pits: [u32; 14]) -> Game {
        Game {
            pits,
            turn: Side::You,
            cursor: 0,
            rng: 1,
        }
    }

    #[test]
    fn sowing_drops_one_seed_per_slot_counterclockwise() {
        let mut g = Game::new(1); // every pit holds 4
        let extra = g.sow(0);
        // Four seeds walk into pits 1, 2, 3, 4 — one apiece.
        assert_eq!(g.pits()[0], 0, "the sown pit is emptied");
        assert_eq!(g.pits()[1], 5);
        assert_eq!(g.pits()[2], 5);
        assert_eq!(g.pits()[3], 5);
        assert_eq!(g.pits()[4], 5);
        assert_eq!(g.pits()[5], 4, "seeds stop after four slots");
        assert_eq!(g.pits()[YOU_STORE], 0, "none reached the store");
        assert!(!extra, "the last seed did not land in the store");
    }

    #[test]
    fn sowing_skips_the_opponents_store() {
        // Nine seeds from pit 5 must wrap past the cpu store (13) without filling
        // it: 6,7,8,9,10,11,12, skip 13, then 0, 1.
        let mut pits = [0u32; 14];
        pits[5] = 9;
        // Pre-seed the final landing pit so the wrap does not end in an empty own
        // pit — that would (correctly) trigger a capture and obscure the store-skip
        // behaviour this test is isolating.
        pits[1] = 1;
        let mut g = board(pits);
        g.sow(5);
        assert_eq!(g.pits()[CPU_STORE], 0, "the opponent store is skipped");
        assert_eq!(g.pits()[YOU_STORE], 1, "your own store still fills");
        assert_eq!(g.pits()[12], 1);
        assert_eq!(g.pits()[0], 1, "sowing wrapped around to your pits");
        assert_eq!(g.pits()[1], 2, "the pre-seeded pit gained the wrapped seed");
    }

    #[test]
    fn landing_in_your_store_grants_another_turn() {
        // One seed from pit 5 lands exactly in your store (slot 6).
        let mut pits = [0u32; 14];
        pits[5] = 1;
        let mut g = board(pits);
        assert!(g.sow(5), "the last seed in your store earns another turn");
        assert_eq!(g.pits()[YOU_STORE], 1);
    }

    #[test]
    fn landing_in_your_empty_pit_captures_the_opposite() {
        // One seed from pit 0 lands in the empty pit 1, opposite the loaded pit
        // 11 (12 - 1). Both the lander and the five across go to your store.
        let mut pits = [0u32; 14];
        pits[0] = 1;
        pits[11] = 5;
        let mut g = board(pits);
        g.sow(0);
        assert_eq!(
            g.pits()[YOU_STORE],
            6,
            "captured seed plus the opposite pit"
        );
        assert_eq!(g.pits()[1], 0, "the landing pit is emptied by the capture");
        assert_eq!(
            g.pits()[11],
            0,
            "the opposite pit is emptied by the capture"
        );
    }

    #[test]
    fn game_ends_and_the_remainder_is_swept() {
        // Your pits are already empty, so the game is over; the cpu's leftover
        // seeds are raked into the cpu store.
        let mut pits = [0u32; 14];
        pits[YOU_STORE] = 2;
        pits[7] = 1;
        pits[8] = 2;
        pits[9] = 3;
        pits[CPU_STORE] = 1;
        let mut g = board(pits);
        assert!(g.game_over(), "one side has no seeds left");
        g.sweep();
        assert_eq!(g.pits()[CPU_STORE], 7, "1 + (1 + 2 + 3) swept home");
        assert_eq!(g.pits()[YOU_STORE], 2, "your store is untouched");
        assert!((7..=12).all(|i| g.pits()[i] == 0), "the pits are cleared");
    }

    #[test]
    fn winner_is_decided_by_store_totals() {
        let mut pits = [0u32; 14];
        pits[YOU_STORE] = 10;
        pits[CPU_STORE] = 5;
        assert_eq!(board(pits).winner(), Winner::You);
        pits[YOU_STORE] = 5;
        pits[CPU_STORE] = 10;
        assert_eq!(board(pits).winner(), Winner::Cpu);
        pits[YOU_STORE] = 8;
        pits[CPU_STORE] = 8;
        assert_eq!(board(pits).winner(), Winner::Tie);
    }
}
