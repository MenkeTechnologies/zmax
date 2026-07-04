//! Centipede — a zemacs terminal shooter in the spirit of the arcade classic.
//!
//! A shooter sits along the bottom rows and fires bullets straight up into a
//! centipede that weaves down through a field of mushrooms. Shooting a body
//! segment splits the chain in two; clearing every segment wins the wave. Move
//! with the arrows or `h`/`l`, fire with `SPC` or `Up`, `p` pauses, `n` starts a
//! new game, `q`/`Esc` quits. Like the other action games it animates itself on
//! wall-clock delta and only schedules the next frame via
//! `zemacs_event::request_redraw` while playing, so it idles when paused, won or
//! over. The board logic is pure and unit-tested.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 44;
const H: i16 = 22;
const SHOOTER_ROW: i16 = H - 1;
const MAX_BULLETS: usize = 3;

/// One centipede chain: contiguous segments with `segs[0]` the head, walking
/// horizontally by `dir` (`+1`/`-1`).
#[derive(Clone)]
pub struct Chain {
    pub segs: Vec<(i16, i16)>,
    pub dir: i16,
}

/// The pure centipede field. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    /// Shooter column along `SHOOTER_ROW`.
    pub shooter: i16,
    /// In-flight bullets as `(row, col)`, travelling up (decreasing row).
    pub bullets: Vec<(i16, i16)>,
    /// Mushroom cells scattered in the upper field.
    pub mushrooms: Vec<(i16, i16)>,
    /// Independent centipede chains.
    pub chains: Vec<Chain>,
    pub score: u32,
    pub lives: u32,
    pub won: bool,
    pub over: bool,
    pub wave: u32,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            shooter: W / 2,
            bullets: Vec::new(),
            mushrooms: Vec::new(),
            chains: Vec::new(),
            score: 0,
            lives: 3,
            won: false,
            over: false,
            wave: 1,
            rng: seed | 1,
        };
        g.spawn_mushrooms();
        g.spawn_centipede();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Scatter mushrooms across the upper field (clear of the shooter's rows).
    fn spawn_mushrooms(&mut self) {
        let count = 12 + (self.rand() % 8) as usize;
        for _ in 0..count {
            let r = 1 + (self.rand() % (H as u64 - 6)) as i16;
            let c = 1 + (self.rand() % (W as u64 - 2)) as i16;
            if !self.mushrooms.contains(&(r, c)) {
                self.mushrooms.push((r, c));
            }
        }
    }

    /// (Re)spawn a single centipede across the top row, its length and heading
    /// varied by the PRNG.
    fn spawn_centipede(&mut self) {
        let len = 8 + (self.rand() % 5) as i16;
        let dir = if self.rand() & 1 == 0 { 1 } else { -1 };
        let start = (W / 2 - len / 2).clamp(0, W - len);
        let segs = (0..len).map(|i| (0, (start + i).clamp(0, W - 1))).collect();
        self.chains = vec![Chain { segs, dir }];
    }

    /// Move the shooter by `d` columns, kept on the board.
    pub fn move_shooter(&mut self, d: i16) {
        self.shooter = (self.shooter + d).clamp(0, W - 1);
    }

    /// Fire a bullet from just above the shooter, up to `MAX_BULLETS` in flight.
    pub fn fire(&mut self) {
        if !self.over && !self.won && self.bullets.len() < MAX_BULLETS {
            self.bullets.push((SHOOTER_ROW - 1, self.shooter));
        }
    }

    /// Split a chain at segment `si`: the segment is removed and the two flanking
    /// runs become independent chains (the tail run reverses heading). Empty runs
    /// are dropped, so shooting the last segment clears the chain entirely.
    fn split_chain(&mut self, ci: usize, si: usize) {
        let old = self.chains.remove(ci);
        let dir = old.dir;
        let left: Vec<(i16, i16)> = old.segs[..si].to_vec();
        let right: Vec<(i16, i16)> = old.segs[si + 1..].to_vec();
        let mut k = 0;
        if !left.is_empty() {
            self.chains.insert(ci + k, Chain { segs: left, dir });
            k += 1;
        }
        if !right.is_empty() {
            self.chains.insert(
                ci + k,
                Chain {
                    segs: right,
                    dir: -dir,
                },
            );
        }
    }

    /// Resolve a bullet arriving at `(r, c)`: it damages a mushroom (small score)
    /// or splits a centipede at the struck segment (bigger score). Returns whether
    /// the bullet was consumed.
    fn resolve_bullet(&mut self, r: i16, c: i16) -> bool {
        if let Some(m) = self.mushrooms.iter().position(|&p| p == (r, c)) {
            self.mushrooms.swap_remove(m);
            self.score += 1;
            return true;
        }
        for ci in 0..self.chains.len() {
            if let Some(si) = self.chains[ci].segs.iter().position(|&s| s == (r, c)) {
                self.split_chain(ci, si);
                self.score += 2;
                return true;
            }
        }
        false
    }

    /// Advance every bullet one row up, resolving hits and dropping spent ones.
    fn advance_bullets(&mut self) {
        let mut kept = Vec::new();
        for (r, c) in std::mem::take(&mut self.bullets) {
            let nr = r - 1;
            if nr < 0 {
                continue;
            }
            if self.resolve_bullet(nr, c) {
                continue;
            }
            kept.push((nr, c));
        }
        self.bullets = kept;
    }

    /// Walk each chain one step: the head steps by `dir`, and on a wall or mushroom
    /// it drops a row and reverses; the body follows the leader. At the floor the
    /// chain keeps weaving in the shooter's zone.
    fn advance_chains(&mut self) {
        for ci in 0..self.chains.len() {
            let (hr, hc) = self.chains[ci].segs[0];
            let dir = self.chains[ci].dir;
            let nc = hc + dir;
            let blocked = nc < 0 || nc >= W || self.mushrooms.contains(&(hr, nc));
            let (new_head, new_dir) = if blocked {
                let mut nr = hr + 1;
                if nr >= H {
                    nr = SHOOTER_ROW;
                }
                ((nr, hc), -dir)
            } else {
                ((hr, nc), dir)
            };
            let chain = &mut self.chains[ci];
            for i in (1..chain.segs.len()).rev() {
                chain.segs[i] = chain.segs[i - 1];
            }
            chain.segs[0] = new_head;
            chain.dir = new_dir;
        }
    }

    /// A segment on the shooter's row and column costs a life; the last life ends
    /// the game, otherwise a fresh centipede drops in.
    fn check_shooter(&mut self) {
        let hit = self.chains.iter().any(|ch| {
            ch.segs
                .iter()
                .any(|&(r, c)| r >= SHOOTER_ROW && c == self.shooter)
        });
        if hit {
            self.lives = self.lives.saturating_sub(1);
            if self.lives == 0 {
                self.over = true;
            } else {
                self.spawn_centipede();
            }
        }
    }

    /// One frame: fly the bullets, walk the centipedes, settle collisions, and set
    /// the win state once the field is cleared.
    pub fn step(&mut self) {
        if self.over || self.won {
            return;
        }
        self.advance_bullets();
        self.advance_chains();
        self.check_shooter();
        if self.chains.is_empty() && !self.over {
            self.won = true;
            self.wave += 1;
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Centipede overlay.
pub struct Centipede {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Centipede {
    pub fn new() -> Self {
        Centipede {
            game: Game::new(1),
            seed: 1,
            paused: false,
            last: None,
            interval: Duration::from_millis(90),
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.paused = false;
        self.last = None;
    }

    /// Running = playing (not won, over or paused); only then does the loop tick.
    fn running(&self) -> bool {
        !self.game.over && !self.game.won && !self.paused
    }
}

impl Default for Centipede {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Centipede {
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
            key!(Left) | key!('h') => self.game.move_shooter(-1),
            key!(Right) | key!('l') => self.game.move_shooter(1),
            key!(' ') | key!(Up) => self.game.fire(),
            key!('p') => self.paused = !self.paused,
            key!('n') => self.restart(),
            _ => {}
        }
        // Restart the frame loop if a key resumed play (it idles when stopped).
        if self.running() {
            self.last = None;
            zemacs_event::request_redraw();
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
                zemacs_event::request_redraw();
            }
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let mush_style = theme.get("ui.linenr");
        let head_style = theme.get("ui.text.focus");
        let body_style = theme.get("error");
        let bullet_style = theme.get("warning");
        let shooter_style = theme.get("function");

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
                "Centipede  score {}  lives {}",
                self.game.score, self.game.lives
            ),
            header_style,
        );

        // Top/bottom border framing the field.
        for c in 0..W {
            surface.set_string(ox + c as u16, oy - 1, "─", mush_style);
            surface.set_string(ox + c as u16, oy + H as u16, "─", mush_style);
        }

        let on_board = |r: i16, c: i16| r >= 0 && r < H && c >= 0 && c < W;
        for &(r, c) in &self.game.mushrooms {
            if on_board(r, c) {
                surface.set_string(ox + c as u16, oy + r as u16, "♣", mush_style);
            }
        }
        for ch in &self.game.chains {
            for (i, &(r, c)) in ch.segs.iter().enumerate() {
                if on_board(r, c) {
                    let (glyph, st) = if i == 0 {
                        ("●", head_style)
                    } else {
                        ("o", body_style)
                    };
                    surface.set_string(ox + c as u16, oy + r as u16, glyph, st);
                }
            }
        }
        for &(r, c) in &self.game.bullets {
            if on_board(r, c) {
                surface.set_string(ox + c as u16, oy + r as u16, "|", bullet_style);
            }
        }
        surface.set_string(
            ox + self.game.shooter as u16,
            oy + SHOOTER_ROW as u16,
            "▲",
            shooter_style,
        );

        let sy = oy + H as u16 + 1;
        let status = if self.game.over {
            format!(
                "Game over — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.game.won {
            format!(
                "Wave cleared — you win!  score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.paused {
            format!("Paused — score {}.  p resume  q quit", self.game.score)
        } else {
            "←/h · →/l · SPC/↑ fire · p pause · n new · q quit".to_string()
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullet_removes_a_segment_and_scores() {
        let mut g = Game::new(1);
        g.mushrooms.clear();
        g.chains = vec![Chain {
            segs: vec![(3, 10), (3, 11), (3, 12)],
            dir: 1,
        }];
        g.bullets = vec![(4, 11)]; // one row below the middle segment, flying up
        let before = g.score;
        g.step();
        let remaining: usize = g.chains.iter().map(|c| c.segs.len()).sum();
        assert_eq!(remaining, 2, "the struck segment is gone");
        assert!(g.score > before, "hitting a segment scores");
    }

    #[test]
    fn hitting_a_wall_drops_and_reverses() {
        let mut g = Game::new(1);
        g.mushrooms.clear();
        g.bullets.clear();
        g.chains = vec![Chain {
            segs: vec![(5, W - 1)],
            dir: 1,
        }];
        g.step();
        assert_eq!(
            g.chains[0].segs[0].0, 6,
            "the centipede drops a row at the wall"
        );
        assert_eq!(g.chains[0].dir, -1, "and reverses direction");
    }

    #[test]
    fn a_segment_hit_splits_the_chain_in_two() {
        let mut g = Game::new(1);
        g.mushrooms.clear();
        g.bullets.clear();
        g.chains = vec![Chain {
            segs: vec![(3, 10), (3, 11), (3, 12)],
            dir: 1,
        }];
        g.bullets = vec![(4, 11)]; // strike the middle segment
        g.step();
        assert_eq!(g.chains.len(), 2, "a mid-chain hit yields two sub-chains");
    }

    #[test]
    fn reaching_the_shooter_costs_a_life() {
        let mut g = Game::new(1);
        g.mushrooms.clear();
        g.bullets.clear();
        g.shooter = 10;
        g.chains = vec![Chain {
            segs: vec![(SHOOTER_ROW, 9)],
            dir: 1,
        }];
        let before = g.lives;
        g.step(); // walks into the shooter's column
        assert_eq!(g.lives, before - 1, "touching the shooter costs a life");
    }

    #[test]
    fn clearing_all_segments_sets_the_win_state() {
        let mut g = Game::new(1);
        g.mushrooms.clear();
        g.chains = vec![Chain {
            segs: vec![(3, 10)],
            dir: 1,
        }];
        g.bullets = vec![(4, 10)]; // the killing shot on the last segment
        g.step();
        assert!(g.chains.is_empty(), "the field is cleared");
        assert!(g.won, "clearing the centipede wins the wave");
    }
}
