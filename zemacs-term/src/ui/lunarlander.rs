//! Lunar Lander — a side-view landing game for zemacs.
//!
//! Ease the module down onto the flat landing pad: `SPC`/`↑` fire the main
//! engine (up), `←`/`→` (or `h`/`l`) fire the lateral thrusters, `p` pauses,
//! `n` starts a new descent and `q`/`Esc` quits. Like the other action games it
//! animates itself via `zemacs_event::request_redraw` only while flying, so it
//! idles when paused or after touchdown. Gravity, thrust and the land/crash
//! verdict are pure fixed-point physics (position and velocity scaled by ten)
//! and unit-tested.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 48;
const H: i16 = 22;
const PAD_W: i16 = 5;

/// Physics constants, all in tenths (velocity/position are ×10 so the maths is
/// integer and deterministic).
const GRAVITY: i32 = 1; // downward pull added to vy each step
const MAIN_THRUST: i32 = 3; // subtracted from vy when the main engine fires
const SIDE_THRUST: i32 = 2; // added to vx when a lateral thruster fires
const FUEL_MAIN: i32 = 2; // fuel burned per main-engine burst
const FUEL_SIDE: i32 = 1; // fuel burned per lateral burst
const START_FUEL: i32 = 400;
const SAFE_VY: i32 = 12; // ≤ 1.2 cells/step vertical is a safe touchdown
const SAFE_VX: i32 = 8; // ≤ 0.8 cells/step horizontal is a safe touchdown

/// How the descent ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Flying,
    Landed,
    Crashed,
}

/// The pure lander world. No I/O, no timing — unit-tested. Position and velocity
/// are stored in tenths of a cell so gravity and thrust are exact integers.
#[derive(Clone)]
pub struct Game {
    /// Horizontal / vertical position, ×10.
    pub x: i32,
    pub y: i32,
    /// Horizontal / vertical velocity, ×10 (vy positive = falling).
    pub vx: i32,
    pub vy: i32,
    /// Lean from the lateral thrusters; upright is near zero.
    pub tilt: i32,
    pub fuel: i32,
    pub score: u32,
    pub lives: u32,
    pub outcome: Outcome,
    /// Surface row (terrain top) for each column, and the flat pad.
    pub terrain: Vec<i16>,
    pub pad_x: i16,
    pub pad_row: i16,
    /// Cosmetic flame timers so the exhaust lingers a couple of frames.
    main_t: u8,
    side_t: u8,
    side_dir: i16,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            x: (W as i32 / 4) * 10,
            y: 0,
            vx: 3,
            vy: 2,
            tilt: 0,
            fuel: START_FUEL,
            score: 0,
            lives: 3,
            outcome: Outcome::Flying,
            terrain: Vec::new(),
            pad_x: 0,
            pad_row: 0,
            main_t: 0,
            side_t: 0,
            side_dir: 0,
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

    /// Build a jagged terrain line near the bottom with one flat landing pad at
    /// a PRNG-chosen column.
    fn generate(&mut self) {
        self.terrain.clear();
        let mut h = H - 4;
        for _ in 0..W {
            let d = (self.rand() % 3) as i16 - 1; // -1, 0 or +1
            h = (h + d).clamp(H - 7, H - 2);
            self.terrain.push(h);
        }
        let span = (W - PAD_W - 4).max(1) as u64;
        let px = (2 + (self.rand() % span) as i16).clamp(1, W - PAD_W - 1);
        let row = self.terrain[px as usize];
        for c in px..px + PAD_W {
            self.terrain[c as usize] = row;
        }
        self.pad_x = px;
        self.pad_row = row;
    }

    /// Whether the given column is on the landing pad.
    fn on_pad(&self, col: i16) -> bool {
        col >= self.pad_x && col < self.pad_x + PAD_W
    }

    /// Fire the main engine: slow the fall and burn fuel. Does nothing once the
    /// tanks are empty or the descent is over.
    pub fn thrust_main(&mut self) {
        if self.outcome != Outcome::Flying || self.fuel <= 0 {
            return;
        }
        self.vy -= MAIN_THRUST;
        self.fuel = (self.fuel - FUEL_MAIN).max(0);
        self.main_t = 2;
    }

    /// Fire a lateral thruster (`dir` = -1 left, +1 right): nudge horizontal
    /// velocity and lean, burning a little fuel.
    pub fn thrust_side(&mut self, dir: i16) {
        if self.outcome != Outcome::Flying || self.fuel <= 0 {
            return;
        }
        self.vx += dir as i32 * SIDE_THRUST;
        self.tilt = (self.tilt + dir as i32).clamp(-3, 3);
        self.fuel = (self.fuel - FUEL_SIDE).max(0);
        self.side_dir = dir;
        self.side_t = 2;
    }

    /// One physics step: gravity, drift, and surface-contact classification.
    pub fn step(&mut self) {
        if self.outcome != Outcome::Flying {
            return;
        }
        // Fade the exhaust flames.
        if self.main_t > 0 {
            self.main_t -= 1;
        }
        if self.side_t > 0 {
            self.side_t -= 1;
            if self.side_t == 0 {
                self.side_dir = 0;
            }
        }

        self.vy += GRAVITY;
        // Lean drifts back to upright.
        if self.tilt > 0 {
            self.tilt -= 1;
        } else if self.tilt < 0 {
            self.tilt += 1;
        }

        self.x = (self.x + self.vx).clamp(0, (W as i32 - 1) * 10);
        self.y = (self.y + self.vy).max(0);

        let col = (self.x / 10) as i16;
        let surf = self.terrain[col as usize];
        if self.y / 10 >= surf as i32 {
            self.y = surf as i32 * 10;
            self.classify(col);
        }
    }

    /// Decide land-or-crash once the surface is touched.
    fn classify(&mut self, col: i16) {
        let gentle = self.vy.abs() <= SAFE_VY && self.vx.abs() <= SAFE_VX;
        let upright = self.tilt.abs() <= 1;
        if self.on_pad(col) && gentle && upright {
            self.outcome = Outcome::Landed;
            self.score += self.fuel.max(0) as u32;
        } else {
            self.outcome = Outcome::Crashed;
            self.lives = self.lives.saturating_sub(1);
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Lunar Lander overlay.
pub struct LunarLander {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl LunarLander {
    pub fn new() -> Self {
        LunarLander {
            game: Game::new(1),
            seed: 1,
            paused: false,
            last: None,
            interval: Duration::from_millis(100),
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.paused = false;
        self.last = None;
    }

    /// Running = still flying and not paused; only then do we keep animating.
    fn running(&self) -> bool {
        self.game.outcome == Outcome::Flying && !self.paused
    }
}

impl Default for LunarLander {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for LunarLander {
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
            key!(' ') | key!(Up) | key!('k') => self.game.thrust_main(),
            key!(Left) | key!('h') => self.game.thrust_side(-1),
            key!(Right) | key!('l') => self.game.thrust_side(1),
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
        let ground_style = theme.get("ui.linenr");
        let pad_style = theme.get("function");
        let lander_style = theme.get("ui.text.focus");
        let flame_style = theme.get("warning");
        let safe_style = theme.get("function");
        let danger_style = theme.get("error");
        let sky_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < (W as u16) + 4 || area.height < (H as u16) + 5 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(
            ox,
            area.y,
            &format!(
                "Lunar Lander  fuel {}  score {}",
                self.game.fuel.max(0),
                self.game.score
            ),
            header_style,
        );

        let cell = |r: i16, c: i16| (ox + c as u16, oy + r as u16);

        // A faint horizon marker on the top border.
        for c in 0..W {
            let (x, y) = cell(-1, c);
            surface.set_string(x, y, "·", sky_style);
        }

        // Terrain: a surface line with the flat pad, filled below for body.
        for c in 0..W {
            let surf = self.game.terrain[c as usize];
            let (x, y) = cell(surf, c);
            if self.game.on_pad(c) {
                surface.set_string(x, y, "═", pad_style);
            } else {
                surface.set_string(x, y, "^", ground_style);
            }
            for r in (surf + 1)..H {
                let (gx, gy) = cell(r, c);
                surface.set_string(gx, gy, "▒", ground_style);
            }
        }

        // The lander (skip if somehow off-board).
        let lcol = (self.game.x / 10).clamp(0, W as i32 - 1) as i16;
        let lrow = (self.game.y / 10) as i16;
        if lrow >= 0 && lrow < H && lcol >= 0 && lcol < W {
            let (lx, ly) = cell(lrow, lcol);
            let glyph = if self.game.outcome == Outcome::Crashed || self.game.tilt.abs() > 1 {
                "A"
            } else {
                "Λ"
            };
            surface.set_string(lx, ly, glyph, lander_style);
            if self.game.main_t > 0 && lrow + 1 < H {
                let (fx, fy) = cell(lrow + 1, lcol);
                surface.set_string(fx, fy, "v", flame_style);
            }
            if self.game.side_t > 0 {
                // Exhaust puffs opposite the push direction.
                let fc = lcol - self.game.side_dir;
                if fc >= 0 && fc < W {
                    let (fx, fy) = cell(lrow, fc);
                    let puff = if self.game.side_dir > 0 { "<" } else { ">" };
                    surface.set_string(fx, fy, puff, flame_style);
                }
            }
        }

        // HUD: altitude and speeds, coloured by safety.
        let ground = self.game.terrain[lcol as usize];
        let alt = (ground - lrow).max(0);
        let vy = self.game.vy;
        let vx = self.game.vx;
        let vstyle = if vy.abs() <= SAFE_VY {
            safe_style
        } else {
            danger_style
        };
        let hstyle = if vx.abs() <= SAFE_VX {
            safe_style
        } else {
            danger_style
        };

        let hud_y = oy + H as u16;
        let mut cx = ox;
        let seg = format!("alt {:>3}   ", alt);
        surface.set_string(cx, hud_y, &seg, text_style);
        cx += seg.chars().count() as u16;
        surface.set_string(cx, hud_y, "vspd ", text_style);
        cx += 5;
        let vs = fmt_speed(vy);
        surface.set_string(cx, hud_y, &vs, vstyle);
        cx += vs.chars().count() as u16;
        surface.set_string(cx, hud_y, "   hspd ", text_style);
        cx += 8;
        let hs = fmt_speed(vx);
        surface.set_string(cx, hud_y, &hs, hstyle);
        cx += hs.chars().count() as u16;
        surface.set_string(
            cx,
            hud_y,
            &format!("   fuel {}", self.game.fuel.max(0)),
            text_style,
        );

        // Footer / status.
        let foot_y = oy + H as u16 + 1;
        let status = match self.game.outcome {
            Outcome::Landed => format!(
                "LANDED! +{} fuel bonus — score {}.  n new  q quit",
                self.game.score, self.game.score
            ),
            Outcome::Crashed => format!("CRASHED — {} lives left.  n new  q quit", self.game.lives),
            Outcome::Flying if self.paused => "PAUSED — p resume · n new · q quit".to_string(),
            Outcome::Flying => {
                "SPC/↑ thrust · ←/→ (h/l) steer · p pause · n new · q quit".to_string()
            }
        };
        surface.set_string(ox, foot_y, &status, text_style);
    }
}

/// Format a ×10 velocity as a signed one-decimal string.
fn fmt_speed(v: i32) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let a = v.abs();
    format!("{}{}.{}", sign, a / 10, a % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravity_increases_downward_velocity() {
        let mut g = Game::new(1);
        g.x = 40; // clear of the terrain
        g.y = 0;
        g.vx = 0;
        g.vy = 0;
        g.step();
        assert!(g.vy > 0, "gravity pulls the lander down");
        assert_eq!(g.outcome, Outcome::Flying);
    }

    #[test]
    fn main_thrust_slows_fall_and_burns_fuel() {
        let mut g = Game::new(1);
        g.vy = 30;
        let fuel = g.fuel;
        g.thrust_main();
        assert!(g.vy < 30, "the main engine cancels downward speed");
        assert!(g.fuel < fuel, "thrust burns fuel");
    }

    #[test]
    fn fuel_floors_and_thrust_stops_when_empty() {
        let mut g = Game::new(1);
        g.fuel = 1; // less than one burst costs
        g.thrust_main();
        assert_eq!(g.fuel, 0, "fuel never goes negative");
        let vy = g.vy;
        g.thrust_main();
        assert_eq!(g.vy, vy, "an empty tank produces no thrust");
    }

    #[test]
    fn gentle_touchdown_on_the_pad_lands() {
        let mut g = Game::new(1);
        let col = g.pad_x + 2; // pad centre
        g.x = col as i32 * 10;
        g.vx = 0;
        g.vy = 0;
        g.tilt = 0;
        g.y = g.pad_row as i32 * 10 - 1; // a whisker above the pad
        g.step();
        assert_eq!(g.outcome, Outcome::Landed);
        assert!(g.score > 0, "a landing banks the leftover fuel");
    }

    #[test]
    fn slamming_the_pad_too_fast_crashes() {
        let mut g = Game::new(1);
        let col = g.pad_x + 2;
        g.x = col as i32 * 10;
        g.vx = 0;
        g.vy = 40; // far above the safe threshold
        g.tilt = 0;
        g.y = g.pad_row as i32 * 10 - 1;
        g.step();
        assert_eq!(g.outcome, Outcome::Crashed);
    }
}
