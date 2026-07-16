//! Bomberman — a compact terminal Bomberman for zmax.
//!
//! Walk the maze, drop bombs, blow up the destructible walls and the roaming
//! enemies without catching yourself in the blast. Move with the arrows or
//! `hjkl`, `SPC` drops a bomb, `p` pauses, `n` restarts, `q`/`Esc` quits. Like
//! the other action games it animates itself on a wall-clock delta and schedules
//! the next frame via `zmax_event::request_redraw` only while playing, so it
//! idles when paused, dead or closed. The arena logic is pure and unit-tested.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 31;
const H: i16 = 17;
const FUSE: i32 = 6; // ticks before a bomb detonates
const RANGE: i16 = 2; // blast reach in each direction
const BLAST_TICKS: i32 = 2; // how long a blast lingers on screen
const MAX_BOMBS: usize = 2;

/// A single arena cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cell {
    Empty,
    Hard,
    Soft,
}

/// A ticking bomb sitting on a cell.
#[derive(Clone, Copy)]
pub struct Bomb {
    pub pos: (i16, i16),
    pub fuse: i32,
}

/// The pure Bomberman arena. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    /// Row-major grid of `H * W` cells.
    pub grid: Vec<Cell>,
    pub player: (i16, i16),
    pub enemies: Vec<(i16, i16)>,
    pub bombs: Vec<Bomb>,
    /// Cells currently lit by a blast (drawn and resolved while it lingers).
    pub blast: Vec<(i16, i16)>,
    blast_life: i32,
    pub score: u32,
    pub lives: u32,
    pub level: u32,
    pub over: bool,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            grid: vec![Cell::Empty; (W * H) as usize],
            player: (1, 1),
            enemies: Vec::new(),
            bombs: Vec::new(),
            blast: Vec::new(),
            blast_life: 0,
            score: 0,
            lives: 3,
            level: 1,
            over: false,
            rng: seed | 1,
        };
        g.build();
        g.spawn_enemies(3);
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    fn in_bounds(&self, r: i16, c: i16) -> bool {
        (0..H).contains(&r) && (0..W).contains(&c)
    }

    fn at(&self, r: i16, c: i16) -> Cell {
        if self.in_bounds(r, c) {
            self.grid[(r * W + c) as usize]
        } else {
            Cell::Hard
        }
    }

    fn set(&mut self, r: i16, c: i16, cell: Cell) {
        if self.in_bounds(r, c) {
            self.grid[(r * W + c) as usize] = cell;
        }
    }

    fn is_open(&self, r: i16, c: i16) -> bool {
        self.at(r, c) == Cell::Empty
    }

    fn is_walkable(&self, r: i16, c: i16) -> bool {
        self.at(r, c) == Cell::Empty
    }

    /// The three cells around the player's start corner, always kept clear.
    fn start_clear(r: i16, c: i16) -> bool {
        matches!((r, c), (1, 1) | (1, 2) | (2, 1))
    }

    /// Lay the hard-wall border, the classic even/even pillar lattice, then
    /// scatter destructible soft walls in the remaining cells.
    fn build(&mut self) {
        for r in 0..H {
            for c in 0..W {
                let hard =
                    r == 0 || r == H - 1 || c == 0 || c == W - 1 || (r % 2 == 0 && c % 2 == 0);
                self.grid[(r * W + c) as usize] = if hard { Cell::Hard } else { Cell::Empty };
            }
        }
        for r in 1..H - 1 {
            for c in 1..W - 1 {
                if self.at(r, c) != Cell::Empty || Self::start_clear(r, c) {
                    continue;
                }
                if self.rand() % 100 < 45 {
                    self.set(r, c, Cell::Soft);
                }
            }
        }
    }

    fn spawn_enemies(&mut self, count: usize) {
        let mut placed = 0;
        let mut tries = 0;
        while placed < count && tries < 10_000 {
            tries += 1;
            let r = (self.rand() % H as u64) as i16;
            let c = (self.rand() % W as u64) as i16;
            if self.at(r, c) == Cell::Empty
                && (r, c) != self.player
                && (r - self.player.0).abs() + (c - self.player.1).abs() > 6
                && !self.enemies.contains(&(r, c))
            {
                self.enemies.push((r, c));
                placed += 1;
            }
        }
    }

    /// Walk into an open neighbouring cell (hard/soft walls block).
    pub fn move_player(&mut self, dr: i16, dc: i16) {
        if self.over {
            return;
        }
        let (r, c) = (self.player.0 + dr, self.player.1 + dc);
        if self.is_open(r, c) {
            self.player = (r, c);
        }
    }

    /// Drop a bomb on the player's cell, up to `MAX_BOMBS` at once.
    pub fn drop_bomb(&mut self) {
        if self.over || self.bombs.len() >= MAX_BOMBS {
            return;
        }
        if self.bombs.iter().any(|b| b.pos == self.player) {
            return;
        }
        self.bombs.push(Bomb {
            pos: self.player,
            fuse: FUSE,
        });
    }

    /// Light a cross-shaped blast from `(r, c)`, stopped by hard pillars and
    /// stopped *after* the first soft wall in each arm.
    fn detonate(&mut self, (r, c): (i16, i16)) {
        let mut cells = vec![(r, c)];
        for &(dr, dc) in &[(-1, 0), (1, 0), (0, -1), (0, 1)] {
            for k in 1..=RANGE {
                let (rr, cc) = (r + dr * k, c + dc * k);
                if !self.in_bounds(rr, cc) {
                    break;
                }
                match self.at(rr, cc) {
                    Cell::Hard => break,
                    Cell::Soft => {
                        cells.push((rr, cc));
                        break;
                    }
                    Cell::Empty => cells.push((rr, cc)),
                }
            }
        }
        for cell in cells {
            if !self.blast.contains(&cell) {
                self.blast.push(cell);
            }
        }
        self.blast_life = BLAST_TICKS;
    }

    /// Resolve the live blast against soft walls (+score), enemies (+score) and
    /// the player (life lost).
    fn resolve_blast(&mut self) {
        if self.blast.is_empty() {
            return;
        }
        let cells = self.blast.clone();
        for &(r, c) in &cells {
            if self.at(r, c) == Cell::Soft {
                self.set(r, c, Cell::Empty);
                self.score += 10;
            }
        }
        let before = self.enemies.len();
        self.enemies.retain(|e| !cells.contains(e));
        self.score += (before - self.enemies.len()) as u32 * 20;
        if cells.contains(&self.player) {
            self.lose_life();
        }
    }

    fn lose_life(&mut self) {
        if self.lives > 0 {
            self.lives -= 1;
        }
        if self.lives == 0 {
            self.over = true;
        } else {
            self.player = (1, 1);
        }
    }

    fn check_contact(&mut self) {
        if self.enemies.contains(&self.player) {
            self.lose_life();
        }
    }

    fn move_enemies(&mut self) {
        let dirs = [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)];
        let player = self.player;
        let n = self.enemies.len();
        for i in 0..n {
            let (r, c) = self.enemies[i];
            let mut opts = Vec::new();
            for &(dr, dc) in &dirs {
                let (rr, cc) = (r + dr, c + dc);
                if self.is_walkable(rr, cc) {
                    opts.push((rr, cc));
                }
            }
            if opts.is_empty() {
                continue;
            }
            // Now and then home in on the player; otherwise wander at random.
            let choice = if self.rand().is_multiple_of(3) {
                *opts
                    .iter()
                    .min_by_key(|&&(rr, cc)| (rr - player.0).abs() + (cc - player.1).abs())
                    .unwrap()
            } else {
                opts[(self.rand() % opts.len() as u64) as usize]
            };
            self.enemies[i] = choice;
        }
    }

    fn next_level(&mut self) {
        self.level += 1;
        self.bombs.clear();
        self.blast.clear();
        self.blast_life = 0;
        self.build();
        self.player = (1, 1);
        self.spawn_enemies(3);
    }

    /// Advance one tick: expire the old blast, move enemies, fuse and explode
    /// bombs, resolve the blast, then check for contact and level completion.
    pub fn step(&mut self) {
        if self.over {
            return;
        }
        if self.blast_life > 0 {
            self.blast_life -= 1;
            if self.blast_life == 0 {
                self.blast.clear();
            }
        }
        let had = !self.enemies.is_empty();
        self.move_enemies();

        let mut boom = Vec::new();
        let mut kept = Vec::new();
        for mut b in self.bombs.drain(..) {
            b.fuse -= 1;
            if b.fuse <= 0 {
                boom.push(b.pos);
            } else {
                kept.push(b);
            }
        }
        self.bombs = kept;
        for pos in boom {
            self.detonate(pos);
        }

        self.resolve_blast();
        self.check_contact();

        if had && self.enemies.is_empty() && !self.over {
            self.next_level();
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Bomberman overlay.
pub struct Bomberman {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Bomberman {
    pub fn new() -> Self {
        Bomberman {
            game: Game::new(1),
            seed: 1,
            paused: false,
            last: None,
            interval: Duration::from_millis(120),
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.paused = false;
        self.last = None;
    }

    /// Running = alive and not paused; only then do we keep the frame loop going.
    fn running(&self) -> bool {
        !self.game.over && !self.paused
    }
}

impl Default for Bomberman {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Bomberman {
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
            key!(Left) | key!('h') => self.game.move_player(0, -1),
            key!(Right) | key!('l') => self.game.move_player(0, 1),
            key!(Up) | key!('k') => self.game.move_player(-1, 0),
            key!(Down) | key!('j') => self.game.move_player(1, 0),
            key!(' ') => self.game.drop_bomb(),
            key!('p') => self.paused = !self.paused,
            key!('n') => self.restart(),
            _ => {}
        }
        // Restart the frame loop if a key resumed play (it idles when stopped).
        if self.running() {
            self.last = None;
            zmax_event::request_redraw();
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        // Advance on wall-clock delta, then schedule the next frame while running.
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
                zmax_event::request_redraw();
            }
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let hard_style = theme.get("ui.linenr");
        let soft_style = theme.get("function");
        let player_style = theme.get("ui.text.focus");
        let enemy_style = theme.get("error");
        let bomb_style = theme.get("warning");
        let blast_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < (W as u16) + 4 || area.height < (H as u16) + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(
            ox,
            area.y,
            &format!(
                "Bomberman  score {}  lives {}  enemies {}  lvl {}",
                self.game.score,
                self.game.lives,
                self.game.enemies.len(),
                self.game.level
            ),
            header_style,
        );

        let cell = |r: i16, c: i16| (ox + c as u16, oy + r as u16);
        for r in 0..H {
            for c in 0..W {
                let (x, y) = cell(r, c);
                match self.game.at(r, c) {
                    Cell::Hard => surface.set_string(x, y, "▓", hard_style),
                    Cell::Soft => surface.set_string(x, y, "▒", soft_style),
                    Cell::Empty => {}
                }
            }
        }
        for b in &self.game.bombs {
            if !self.game.in_bounds(b.pos.0, b.pos.1) {
                continue;
            }
            let (x, y) = cell(b.pos.0, b.pos.1);
            surface.set_string(x, y, "◍", bomb_style);
        }
        for &(r, c) in &self.game.blast {
            if !self.game.in_bounds(r, c) {
                continue;
            }
            let (x, y) = cell(r, c);
            surface.set_string(x, y, "✷", blast_style);
        }
        for &(r, c) in &self.game.enemies {
            if !self.game.in_bounds(r, c) {
                continue;
            }
            let (x, y) = cell(r, c);
            surface.set_string(x, y, "ᗣ", enemy_style);
        }
        if self.game.in_bounds(self.game.player.0, self.game.player.1) {
            let (px, py) = cell(self.game.player.0, self.game.player.1);
            surface.set_string(px, py, "☻", player_style);
        }

        let sy = oy + H as u16 + 1;
        let status = if self.game.over {
            format!(
                "Game over — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.paused {
            "Paused — SPC bomb · p resume · n new · q quit".to_string()
        } else {
            "arrows/hjkl move · SPC bomb · p pause · n new · q quit".to_string()
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bomb_detonates_after_its_fuse_into_a_cross() {
        let mut g = Game::new(1);
        g.enemies.clear();
        g.bombs.clear();
        g.player = (1, 1);
        g.drop_bomb();
        for _ in 0..FUSE {
            g.step();
        }
        assert!(!g.blast.is_empty(), "the bomb has gone off");
        assert!(g.blast.contains(&(1, 1)), "the blast covers the bomb cell");
        assert!(g.blast.contains(&(1, 2)), "and extends in a cross arm");
    }

    #[test]
    fn blast_destroys_a_soft_wall_and_scores() {
        let mut g = Game::new(1);
        g.enemies.clear();
        g.bombs.clear();
        g.player = (1, 1);
        g.set(1, 2, Cell::Empty);
        g.set(1, 3, Cell::Soft);
        let before = g.score;
        g.drop_bomb();
        for _ in 0..FUSE {
            g.step();
        }
        assert_eq!(g.at(1, 3), Cell::Empty, "the soft wall is gone");
        assert!(g.score > before, "destroying it scored points");
    }

    #[test]
    fn a_hard_pillar_blocks_the_blast() {
        let mut g = Game::new(1);
        g.enemies.clear();
        g.bombs.clear();
        g.player = (1, 2);
        assert_eq!(g.at(2, 2), Cell::Hard, "the even/even pillar is hard");
        g.drop_bomb();
        for _ in 0..FUSE {
            g.step();
        }
        assert!(!g.blast.contains(&(2, 2)), "the blast stops at the pillar");
        assert!(!g.blast.contains(&(3, 2)), "and never reaches beyond it");
    }

    #[test]
    fn an_enemy_caught_in_the_blast_dies() {
        let mut g = Game::new(1);
        g.enemies = vec![(5, 5), (7, 7)];
        g.blast = vec![(5, 5)];
        g.blast_life = BLAST_TICKS;
        let before = g.score;
        g.resolve_blast();
        assert!(!g.enemies.contains(&(5, 5)), "the caught enemy is dead");
        assert_eq!(g.enemies.len(), 1, "the other enemy survives");
        assert!(g.score > before, "killing it scored points");
    }

    #[test]
    fn the_player_caught_in_the_blast_loses_a_life() {
        let mut g = Game::new(1);
        g.enemies.clear();
        g.player = (5, 5);
        g.lives = 3;
        g.blast = vec![(5, 5)];
        g.blast_life = BLAST_TICKS;
        g.resolve_blast();
        assert_eq!(g.lives, 2, "the player loses exactly one life");
    }
}
