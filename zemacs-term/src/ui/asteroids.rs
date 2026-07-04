//! Asteroids — a small grid-based homage to the arcade classic for zemacs.
//!
//! Rotate the ship with `←`/`→`, `↑` thrusts, `SPC` fires, `p` pauses, `n`
//! starts a new game and `q`/`Esc` quits. Like `pong` and `snake` it animates
//! itself in real time via `zemacs_event::request_redraw` only while playing, so
//! it idles when paused, dead or closed. The board — an integer world that wraps
//! at every edge — is pure and unit-tested, seeded by a small LCG PRNG (the same
//! generator `snake` uses) so waves spawn deterministically.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 48;
const H: i16 = 22;
/// Per-axis speed cap so a thrusting ship never outruns collision detection.
const MAX_SPEED: i16 = 2;
/// How many ticks a fired bullet survives (comfortably crosses the board).
const BULLET_LIFE: u16 = 40;
/// Ticks of invulnerability granted after a respawn.
const INVULN: u16 = 12;

/// The eight compass headings as unit row/column steps, clockwise from North.
const DIRS: [(i16, i16); 8] = [
    (-1, 0),  // N
    (-1, 1),  // NE
    (0, 1),   // E
    (1, 1),   // SE
    (1, 0),   // S
    (1, -1),  // SW
    (0, -1),  // W
    (-1, -1), // NW
];

/// Glyph drawn for each heading, aligned with `DIRS`.
const GLYPHS: [&str; 8] = ["▲", "◥", "▶", "◢", "▼", "◣", "◀", "◤"];

/// A drifting rock. `size` 2 is large (splits when shot), 1 is small.
#[derive(Clone)]
pub struct Asteroid {
    pub pos: (i16, i16),
    pub vel: (i16, i16),
    pub size: u8,
}

/// A bullet travelling in a fixed heading with a limited lifetime.
#[derive(Clone)]
pub struct Bullet {
    pub pos: (i16, i16),
    pub vel: (i16, i16),
    pub life: u16,
}

/// The pure Asteroids world. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub ship_pos: (i16, i16),
    pub ship_vel: (i16, i16),
    /// Heading index into `DIRS`/`GLYPHS` (0..8).
    pub heading: usize,
    pub asteroids: Vec<Asteroid>,
    pub bullets: Vec<Bullet>,
    pub score: u32,
    pub lives: u32,
    pub wave: u32,
    pub alive: bool,
    /// Remaining invulnerability ticks after a respawn.
    pub invuln: u16,
    rng: u64,
}

/// Toroidal wrap of a position onto the board.
fn wrap(pos: (i16, i16)) -> (i16, i16) {
    (pos.0.rem_euclid(H), pos.1.rem_euclid(W))
}

/// Grid overlap test: within the asteroid's (Manhattan) radius.
fn collide(a: (i16, i16), b: (i16, i16), size: u8) -> bool {
    (a.0 - b.0).abs() + (a.1 - b.1).abs() <= size as i16
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            ship_pos: (H / 2, W / 2),
            ship_vel: (0, 0),
            heading: 0,
            asteroids: Vec::new(),
            bullets: Vec::new(),
            score: 0,
            lives: 3,
            wave: 1,
            alive: true,
            invuln: 0,
            rng: seed | 1,
        };
        g.spawn_wave(3);
        g
    }

    /// LCG step (identical constants to `snake`), returning a scrambled value.
    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Populate a fresh wave of large asteroids at random edges with small
    /// random velocities.
    fn spawn_wave(&mut self, count: u32) {
        self.asteroids.clear();
        for _ in 0..count {
            let pos = match self.rand() % 4 {
                0 => (0, (self.rand() % W as u64) as i16),     // top edge
                1 => (H - 1, (self.rand() % W as u64) as i16), // bottom edge
                2 => ((self.rand() % H as u64) as i16, 0),     // left edge
                _ => ((self.rand() % H as u64) as i16, W - 1), // right edge
            };
            let vr = (self.rand() % 3) as i16 - 1;
            let vc = (self.rand() % 3) as i16 - 1;
            let vel = if (vr, vc) == (0, 0) { (0, 1) } else { (vr, vc) };
            self.asteroids.push(Asteroid { pos, vel, size: 2 });
        }
    }

    /// Rotate the heading by `delta` eighth-turns (negative = counter-clockwise).
    pub fn rotate(&mut self, delta: i16) {
        self.heading = (self.heading as i16 + delta).rem_euclid(8) as usize;
    }

    /// Add the heading's unit vector to the ship velocity, clamped to `MAX_SPEED`.
    pub fn thrust(&mut self) {
        let (dr, dc) = DIRS[self.heading];
        self.ship_vel.0 = (self.ship_vel.0 + dr).clamp(-MAX_SPEED, MAX_SPEED);
        self.ship_vel.1 = (self.ship_vel.1 + dc).clamp(-MAX_SPEED, MAX_SPEED);
    }

    /// Fire a bullet one cell ahead of the ship, travelling in the heading.
    pub fn fire(&mut self) {
        if !self.alive {
            return;
        }
        let (dr, dc) = DIRS[self.heading];
        let pos = wrap((self.ship_pos.0 + dr, self.ship_pos.1 + dc));
        self.bullets.push(Bullet {
            pos,
            vel: (dr, dc),
            life: BULLET_LIFE,
        });
    }

    /// Send the ship back to the centre, stationary and briefly invulnerable.
    fn respawn(&mut self) {
        self.ship_pos = (H / 2, W / 2);
        self.ship_vel = (0, 0);
        self.heading = 0;
        self.invuln = INVULN;
    }

    /// One world step: move everything (wrapping), resolve bullet and ship
    /// collisions, and advance to the next wave once the field is clear.
    pub fn step(&mut self) {
        if !self.alive {
            return;
        }
        if self.invuln > 0 {
            self.invuln -= 1;
        }

        // Move the ship.
        self.ship_pos = wrap((
            self.ship_pos.0 + self.ship_vel.0,
            self.ship_pos.1 + self.ship_vel.1,
        ));

        // Drift the asteroids.
        for a in &mut self.asteroids {
            a.pos = wrap((a.pos.0 + a.vel.0, a.pos.1 + a.vel.1));
        }

        // Advance and age the bullets, dropping any that expired.
        for b in &mut self.bullets {
            b.pos = wrap((b.pos.0 + b.vel.0, b.pos.1 + b.vel.1));
            b.life = b.life.saturating_sub(1);
        }
        self.bullets.retain(|b| b.life > 0);

        // Bullet vs. asteroid: each hit destroys the rock (+score); a large one
        // splits into two smaller rocks flung perpendicular to its drift.
        let mut spent = vec![false; self.bullets.len()];
        let mut next: Vec<Asteroid> = Vec::new();
        for a in &self.asteroids {
            let mut hit = false;
            for (i, b) in self.bullets.iter().enumerate() {
                if !spent[i] && collide(b.pos, a.pos, a.size) {
                    spent[i] = true;
                    hit = true;
                    self.score += 10 * a.size as u32;
                    break;
                }
            }
            if hit {
                if a.size > 1 {
                    next.push(Asteroid {
                        pos: a.pos,
                        vel: (a.vel.1, -a.vel.0),
                        size: a.size - 1,
                    });
                    next.push(Asteroid {
                        pos: a.pos,
                        vel: (-a.vel.1, a.vel.0),
                        size: a.size - 1,
                    });
                }
            } else {
                next.push(a.clone());
            }
        }
        self.asteroids = next;
        let mut i = 0;
        self.bullets.retain(|_| {
            let keep = !spent[i];
            i += 1;
            keep
        });

        // Ship vs. asteroid: costs a life and respawns (unless invulnerable).
        if self.invuln == 0
            && self
                .asteroids
                .iter()
                .any(|a| collide(self.ship_pos, a.pos, a.size))
        {
            self.lives = self.lives.saturating_sub(1);
            if self.lives == 0 {
                self.alive = false;
            } else {
                self.respawn();
            }
        }

        // Clearing the field advances to a larger wave.
        if self.alive && self.asteroids.is_empty() {
            self.wave += 1;
            self.spawn_wave(2 + self.wave);
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Asteroids overlay.
pub struct Asteroids {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Asteroids {
    pub fn new() -> Self {
        Asteroids {
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

    /// Running = alive and not paused; only then do we keep the frame loop going.
    fn running(&self) -> bool {
        self.game.alive && !self.paused
    }
}

impl Default for Asteroids {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Asteroids {
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
            key!(Left) => self.game.rotate(-1),
            key!(Right) => self.game.rotate(1),
            key!(Up) => self.game.thrust(),
            key!(' ') => self.game.fire(),
            key!('p') => self.paused = !self.paused,
            key!('n') => self.restart(),
            _ => {}
        }
        if self.running() {
            if self.last.is_none() {
                self.last = Some(Instant::now());
            }
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
        let wall_style = theme.get("ui.linenr");
        let ship_style = theme.get("function");
        let asteroid_style = theme.get("ui.text");
        let bullet_style = theme.get("warning");
        let over_style = theme.get("error");

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
                "Asteroids  score {}  lives {}  wave {}",
                self.game.score, self.game.lives, self.game.wave
            ),
            header_style,
        );

        // Top and bottom borders (the field wraps, so the sides are open).
        for c in 0..W {
            surface.set_string(ox + c as u16, oy - 1, "─", wall_style);
            surface.set_string(ox + c as u16, oy + H as u16, "─", wall_style);
        }

        // Draw the rocks, then bullets, then the ship on top. Every entity is
        // skipped if its grid coords fall outside the board.
        for a in &self.game.asteroids {
            let (r, c) = a.pos;
            if r >= 0 && r < H && c >= 0 && c < W {
                let glyph = if a.size >= 2 { "◯" } else { "●" };
                surface.set_string(ox + c as u16, oy + r as u16, glyph, asteroid_style);
            }
        }
        for b in &self.game.bullets {
            let (r, c) = b.pos;
            if r >= 0 && r < H && c >= 0 && c < W {
                surface.set_string(ox + c as u16, oy + r as u16, "·", bullet_style);
            }
        }
        let (sr, sc) = self.game.ship_pos;
        if sr >= 0 && sr < H && sc >= 0 && sc < W {
            surface.set_string(
                ox + sc as u16,
                oy + sr as u16,
                GLYPHS[self.game.heading],
                ship_style,
            );
        }

        let sy = oy + H as u16 + 1;
        if !self.game.alive {
            surface.set_string(
                ox,
                sy,
                &format!(
                    "Game over — score {}.  n: new game  q: quit",
                    self.game.score
                ),
                over_style,
            );
        } else if self.paused {
            surface.set_string(ox, sy, "Paused — p resume · n new · q quit", text_style);
        } else {
            surface.set_string(
                ox,
                sy,
                "←/→ rotate · ↑ thrust · SPC fire · p pause · n new · q quit",
                text_style,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ship_wraps_around_edges() {
        let mut g = Game::new(1);
        g.invuln = 1000; // stay put through any drifting-asteroid contact
        g.ship_pos = (0, 5);
        g.ship_vel = (-1, 0); // heading off the top edge
        g.step();
        assert_eq!(
            g.ship_pos.0,
            H - 1,
            "the ship wraps from the top to the bottom"
        );
    }

    #[test]
    fn rotation_cycles_heading() {
        let mut g = Game::new(1);
        let start = g.heading;
        g.rotate(1);
        assert_eq!(g.heading, (start + 1) % 8, "one step clockwise");
        for _ in 0..8 {
            g.rotate(1);
        }
        assert_eq!(
            g.heading,
            (start + 1) % 8,
            "eight steps return to the same heading"
        );
        g.rotate(-1);
        assert_eq!(
            g.heading, start,
            "a counter-clockwise step undoes one clockwise"
        );
    }

    #[test]
    fn firing_spawns_a_bullet() {
        let mut g = Game::new(1);
        let before = g.bullets.len();
        g.fire();
        assert_eq!(g.bullets.len(), before + 1, "SPC adds a bullet");
    }

    #[test]
    fn bullet_destroys_asteroid_and_scores() {
        let mut g = Game::new(1);
        g.invuln = 1000;
        g.asteroids.clear();
        g.bullets.clear();
        // Two rocks so clearing one doesn't trigger a fresh wave.
        g.asteroids.push(Asteroid {
            pos: (5, 5),
            vel: (0, 0),
            size: 1,
        });
        g.asteroids.push(Asteroid {
            pos: (15, 30),
            vel: (0, 0),
            size: 1,
        });
        // A bullet one cell to the left, moving right onto the first rock.
        g.bullets.push(Bullet {
            pos: (5, 4),
            vel: (0, 1),
            life: 10,
        });
        let before = g.score;
        g.step();
        assert_eq!(g.asteroids.len(), 1, "the shot asteroid is gone");
        assert!(g.score > before, "destroying an asteroid scores");
    }

    #[test]
    fn ship_asteroid_collision_costs_a_life() {
        let mut g = Game::new(1);
        g.asteroids.clear();
        g.bullets.clear();
        g.ship_pos = (7, 10);
        g.ship_vel = (0, 0);
        g.invuln = 0;
        g.asteroids.push(Asteroid {
            pos: (7, 10),
            vel: (0, 0),
            size: 1,
        });
        let lives = g.lives;
        g.step();
        assert_eq!(
            g.lives,
            lives - 1,
            "colliding with an asteroid costs a life"
        );
        assert_eq!(
            g.ship_pos,
            (H / 2, W / 2),
            "the ship respawns at the centre"
        );
    }
}
