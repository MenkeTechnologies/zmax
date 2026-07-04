//! Dig Dug — a small terminal take on the arcade classic for zemacs.
//!
//! Tunnel through the soil with the arrows or `hjkl`, harpoon the Pooka with
//! `SPC` (it fires in the direction you last dug), `p` pauses, `n` starts a new
//! game and `q`/`Esc` quits. Like the other action games it animates itself via
//! `zemacs_event::request_redraw` only while playing, advancing on a wall-clock
//! delta. The soil/enemy/harpoon logic is a pure `Game` and unit-tested; enemy
//! chasing uses the same LCG PRNG as the snake port.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 40;
const H: i16 = 18;
const SPAWN: (i16, i16) = (H / 2, W / 2);
const HARPOON_LEN: i16 = 4;
const N_ENEMIES: usize = 3;
const PHASE_CHANCE: u64 = 6; // percent chance an enemy phases through soil
const SCORE_POP: u32 = 100;
const START_LIVES: u32 = 3;

fn in_bounds((r, c): (i16, i16)) -> bool {
    r >= 0 && r < H && c >= 0 && c < W
}

/// The pure Dig Dug field. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    /// Soil map: `true` is solid soil, `false` is carved tunnel.
    grid: Vec<bool>,
    pub digger: (i16, i16),
    /// The direction the digger last moved — the harpoon fires this way.
    pub face: (i16, i16),
    pub enemies: Vec<(i16, i16)>,
    /// The harpoon tip while a shot is in flight.
    pub harpoon: Option<(i16, i16)>,
    harpoon_dir: (i16, i16),
    harpoon_life: i16,
    pub score: u32,
    pub lives: u32,
    pub level: u32,
    pub alive: bool,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            grid: vec![true; (W * H) as usize],
            digger: SPAWN,
            face: (0, 1),
            enemies: Vec::new(),
            harpoon: None,
            harpoon_dir: (0, 1),
            harpoon_life: 0,
            score: 0,
            lives: START_LIVES,
            level: 1,
            alive: true,
            rng: seed | 1,
        };
        g.generate();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Reset the soil to solid, carve the starting tunnel and scatter enemies.
    fn generate(&mut self) {
        self.grid = vec![true; (W * H) as usize];
        self.digger = SPAWN;
        self.face = (0, 1);
        self.harpoon = None;
        self.enemies.clear();
        let (r, c) = SPAWN;
        for dc in -2..=2 {
            self.carve(r, c + dc);
        }
        let mut guard = 0;
        while self.enemies.len() < N_ENEMIES && guard < 10_000 {
            guard += 1;
            let er = (self.rand() % H as u64) as i16;
            let ec = (self.rand() % W as u64) as i16;
            if (er - r).abs() + (ec - c).abs() < 6 || self.enemies.contains(&(er, ec)) {
                continue;
            }
            // Give each Pooka a pocket of tunnel to start moving from.
            self.carve(er, ec);
            self.enemies.push((er, ec));
        }
    }

    fn idx(r: i16, c: i16) -> usize {
        (r * W + c) as usize
    }

    /// Is `(r, c)` still solid soil? Out-of-bounds counts as soil (a wall).
    fn soil(&self, r: i16, c: i16) -> bool {
        if !in_bounds((r, c)) {
            return true;
        }
        self.grid[Self::idx(r, c)]
    }

    fn carve(&mut self, r: i16, c: i16) {
        if in_bounds((r, c)) {
            self.grid[Self::idx(r, c)] = false;
        }
    }

    /// Move the digger one cell, carving the entered cell into tunnel.
    pub fn move_dir(&mut self, d: (i16, i16)) {
        if !self.alive {
            return;
        }
        self.face = d;
        let n = (self.digger.0 + d.0, self.digger.1 + d.1);
        if !in_bounds(n) {
            return;
        }
        self.carve(n.0, n.1);
        self.digger = n;
        self.resolve_contact();
    }

    /// Fire a harpoon from the digger in the facing direction.
    pub fn pump(&mut self) {
        let tip = (self.digger.0 + self.face.0, self.digger.1 + self.face.1);
        self.harpoon = Some(tip);
        self.harpoon_dir = self.face;
        self.harpoon_life = HARPOON_LEN;
    }

    /// Greedy chase: step one cell toward the digger through existing tunnel,
    /// occasionally phasing a cell through soil.
    fn chase_step(&mut self, e: (i16, i16)) -> (i16, i16) {
        let dr = (self.digger.0 - e.0).signum();
        let dc = (self.digger.1 - e.1).signum();
        let vgap = (self.digger.0 - e.0).abs();
        let hgap = (self.digger.1 - e.1).abs();
        let prefer_vert = if vgap == hgap {
            self.rand() & 1 == 0
        } else {
            vgap > hgap
        };
        let opts: [(i16, i16); 2] = if prefer_vert {
            [(dr, 0), (0, dc)]
        } else {
            [(0, dc), (dr, 0)]
        };
        for (mr, mc) in opts {
            if mr == 0 && mc == 0 {
                continue;
            }
            let (nr, nc) = (e.0 + mr, e.1 + mc);
            if in_bounds((nr, nc)) && !self.soil(nr, nc) {
                return (nr, nc);
            }
        }
        if self.rand() % 100 < PHASE_CHANCE {
            for (mr, mc) in opts {
                if mr == 0 && mc == 0 {
                    continue;
                }
                let (nr, nc) = (e.0 + mr, e.1 + mc);
                if in_bounds((nr, nc)) {
                    return (nr, nc);
                }
            }
        }
        e
    }

    fn step_enemies(&mut self) {
        for i in 0..self.enemies.len() {
            let e = self.enemies[i];
            self.enemies[i] = self.chase_step(e);
        }
    }

    /// Advance the harpoon: pop an enemy at the tip, else extend until it runs
    /// out of reach or bites into soil.
    fn step_harpoon(&mut self) {
        let tip = match self.harpoon {
            Some(t) => t,
            None => return,
        };
        if let Some(i) = self.enemies.iter().position(|&e| e == tip) {
            self.enemies.remove(i);
            self.score += SCORE_POP;
            self.harpoon = None;
            return;
        }
        self.harpoon_life -= 1;
        let next = (tip.0 + self.harpoon_dir.0, tip.1 + self.harpoon_dir.1);
        if self.harpoon_life <= 0 || !in_bounds(next) || self.soil(next.0, next.1) {
            self.harpoon = None;
        } else {
            self.harpoon = Some(next);
        }
    }

    fn resolve_contact(&mut self) {
        if self.enemies.iter().any(|&e| e == self.digger) {
            self.enemies.retain(|&e| e != self.digger);
            self.lives = self.lives.saturating_sub(1);
            if self.lives == 0 {
                self.alive = false;
            } else {
                self.digger = SPAWN;
            }
        }
    }

    fn next_level(&mut self) {
        self.level += 1;
        self.generate();
    }

    /// One tick: move enemies, advance the harpoon, resolve collisions, and
    /// drop to the next level once the field is cleared.
    pub fn step(&mut self) {
        if !self.alive {
            return;
        }
        self.step_enemies();
        self.step_harpoon();
        self.resolve_contact();
        if self.alive && self.enemies.is_empty() {
            self.next_level();
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Dig Dug overlay.
pub struct DigDug {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl DigDug {
    pub fn new() -> Self {
        DigDug {
            game: Game::new(1),
            seed: 1,
            paused: false,
            last: None,
            interval: Duration::from_millis(110),
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
        self.game.alive && !self.paused
    }
}

impl Default for DigDug {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for DigDug {
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
            key!(Left) | key!('h') => self.game.move_dir((0, -1)),
            key!(Right) | key!('l') => self.game.move_dir((0, 1)),
            key!(Up) | key!('k') => self.game.move_dir((-1, 0)),
            key!(Down) | key!('j') => self.game.move_dir((1, 0)),
            key!(' ') => self.game.pump(),
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
        let soil_style = theme.get("ui.linenr");
        let digger_style = theme.get("function");
        let pump_style = theme.get("ui.text.focus");
        let enemy_style = theme.get("error");
        let harpoon_style = theme.get("warning");

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
                "Dig Dug  score {}  lives {}  level {}",
                self.game.score, self.game.lives, self.game.level
            ),
            header_style,
        );

        let cell = |r: i16, c: i16| (ox + c as u16, oy + r as u16);

        // Soil (tunnels are left as background).
        for r in 0..H {
            for c in 0..W {
                if self.game.soil(r, c) {
                    let (x, y) = cell(r, c);
                    surface.set_string(x, y, "▓", soil_style);
                }
            }
        }

        // Harpoon: a short line along the facing axis.
        if let Some((hr, hc)) = self.game.harpoon {
            if in_bounds((hr, hc)) {
                let glyph = if self.game.harpoon_dir.0 != 0 {
                    "|"
                } else {
                    "-"
                };
                let (x, y) = cell(hr, hc);
                surface.set_string(x, y, glyph, harpoon_style);
            }
        }

        // Enemies.
        for &(r, c) in &self.game.enemies {
            if in_bounds((r, c)) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, "ᗧ", enemy_style);
            }
        }

        // Digger — brighter while a harpoon is in flight.
        let (dr, dc) = self.game.digger;
        if in_bounds((dr, dc)) {
            let (x, y) = cell(dr, dc);
            let style = if self.game.harpoon.is_some() {
                pump_style
            } else {
                digger_style
            };
            surface.set_string(x, y, "☺", style);
        }

        let sy = oy + H as u16 + 1;
        let status = if !self.game.alive {
            format!(
                "Game over — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.paused {
            format!("Paused — score {}.  SPC resume", self.game.score)
        } else {
            "arrows/hjkl dig · SPC pump · p pause · n new · q quit".to_string()
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_into_soil_carves_a_tunnel() {
        let mut g = Game::new(1);
        let (r, c) = g.digger;
        let target = (r - 1, c); // above the horizontal starting tunnel: still soil
        assert!(g.soil(target.0, target.1), "cell above the start is soil");
        g.move_dir((-1, 0));
        assert_eq!(g.digger, target, "the digger advances into the cell");
        assert!(
            !g.soil(target.0, target.1),
            "the entered cell becomes tunnel"
        );
    }

    #[test]
    fn enemy_steps_toward_the_digger_through_a_tunnel() {
        let mut g = Game::new(1);
        g.enemies.clear();
        g.harpoon = None;
        let r = g.digger.0;
        for c in 0..W {
            g.carve(r, c); // open a clear horizontal tunnel
        }
        let e = (r, g.digger.1 + 5);
        g.enemies = vec![e];
        let before = (g.digger.0 - e.0).abs() + (g.digger.1 - e.1).abs();
        g.step();
        let now = g.enemies[0];
        let after = (g.digger.0 - now.0).abs() + (g.digger.1 - now.1).abs();
        assert!(after < before, "the Pooka closes the distance");
    }

    #[test]
    fn harpoon_pops_an_enemy_and_scores() {
        let mut g = Game::new(1);
        g.enemies.clear();
        g.face = (0, 1);
        let e = (g.digger.0, g.digger.1 + 1);
        g.carve(e.0, e.1);
        g.enemies = vec![e];
        let score = g.score;
        g.pump();
        g.step_harpoon();
        assert!(g.enemies.is_empty(), "the harpoon pops the enemy");
        assert_eq!(g.score, score + SCORE_POP, "popping scores");
    }

    #[test]
    fn enemy_contact_costs_a_life() {
        let mut g = Game::new(1);
        g.enemies.clear();
        g.harpoon = None;
        let e = (g.digger.0, g.digger.1 + 1); // adjacent, will step onto the digger
        g.carve(e.0, e.1);
        g.enemies = vec![e];
        let lives = g.lives;
        g.step();
        assert_eq!(g.lives, lives - 1, "touching a Pooka costs a life");
    }

    #[test]
    fn clearing_all_enemies_advances_the_level() {
        let mut g = Game::new(1);
        g.enemies.clear();
        g.face = (0, 1);
        g.carve(g.digger.0, g.digger.1 + 1);
        g.carve(g.digger.0, g.digger.1 + 2);
        let e = (g.digger.0, g.digger.1 + 2);
        g.enemies = vec![e];
        g.pump(); // tip lands at digger + 1
        let level = g.level;
        g.step(); // enemy steps to digger + 1, harpoon pops it, field clears
        assert_eq!(g.level, level + 1, "an empty field advances the level");
    }
}
