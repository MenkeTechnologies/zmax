//! Missile Command — a zmax terminal port of the arcade classic.
//!
//! Defend six cities from a rain of enemy missiles. Move the aiming crosshair
//! with the arrows or `hjkl`, `SPC` fires an interceptor from the ground battery
//! up to the crosshair where it detonates into an expanding blast, `p` pauses,
//! `n` restarts, `q`/`Esc` quits. Like the other action games it animates itself
//! via `zmax_event::request_redraw` only while playing. The board logic is pure
//! and unit-tested (keys parse into a `missilecommand` keymap mode by
//! `scripts/gen_port_report.py`).

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 48;
const H: i16 = 22;
const CITY_ROW: i16 = H - 1;
const BAT_COL: i16 = W / 2;
const BLAST_MAX: i16 = 3;
const BLAST_TTL: i16 = 4;
const WAVE_BONUS: u32 = 25;
const CITY_COLS: [i16; 6] = [4, 10, 16, 32, 38, 44];

fn on_board(r: i16, c: i16) -> bool {
    (0..H).contains(&r) && (0..W).contains(&c)
}

/// A defended city sitting along the bottom row.
#[derive(Clone)]
pub struct City {
    pub col: i16,
    pub alive: bool,
}

/// An incoming enemy missile descending toward a target city column.
#[derive(Clone)]
pub struct Missile {
    pub r: i16,
    pub c: i16,
    /// Target column (a city) the missile drifts toward as it falls.
    pub tc: i16,
}

/// A player interceptor climbing from the battery to the crosshair.
#[derive(Clone)]
pub struct Interceptor {
    pub r: i16,
    pub c: i16,
    pub tr: i16,
    pub tc: i16,
}

/// An expanding detonation that destroys any missile it overlaps.
#[derive(Clone)]
pub struct Blast {
    pub r: i16,
    pub c: i16,
    pub radius: i16,
    pub ttl: i16,
}

/// The pure Missile Command board. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub cities: Vec<City>,
    pub missiles: Vec<Missile>,
    pub interceptors: Vec<Interceptor>,
    pub blasts: Vec<Blast>,
    /// The aiming crosshair (row, col).
    pub cross: (i16, i16),
    pub score: u32,
    pub wave: u32,
    pub spawned: u32,
    pub quota: u32,
    pub spawn_timer: i16,
    pub spawn_period: i16,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let cities = CITY_COLS
            .iter()
            .map(|&col| City { col, alive: true })
            .collect();
        Game {
            cities,
            missiles: Vec::new(),
            interceptors: Vec::new(),
            blasts: Vec::new(),
            cross: (H / 2, W / 2),
            score: 0,
            wave: 1,
            spawned: 0,
            quota: 8,
            spawn_timer: 12,
            spawn_period: 12,
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

    /// The game ends once every city has been destroyed.
    pub fn game_over(&self) -> bool {
        self.cities.iter().all(|c| !c.alive)
    }

    /// Move the crosshair, kept above the city line and on the board.
    pub fn move_cross(&mut self, dr: i16, dc: i16) {
        self.cross.0 = (self.cross.0 + dr).clamp(0, CITY_ROW - 1);
        self.cross.1 = (self.cross.1 + dc).clamp(0, W - 1);
    }

    /// Launch an interceptor from the battery toward the current crosshair.
    pub fn fire(&mut self) {
        self.interceptors.push(Interceptor {
            r: CITY_ROW,
            c: BAT_COL,
            tr: self.cross.0,
            tc: self.cross.1,
        });
    }

    fn spawn_missile(&mut self) {
        let col = (self.rand() % W as u64) as i16;
        let alive: Vec<i16> = self
            .cities
            .iter()
            .filter(|c| c.alive)
            .map(|c| c.col)
            .collect();
        let tc = if alive.is_empty() {
            col
        } else {
            alive[(self.rand() as usize) % alive.len()]
        };
        self.missiles.push(Missile { r: 0, c: col, tc });
    }

    /// One tick: advance missiles and interceptors, expand/expire blasts, resolve
    /// blast∩missile and missile∩city collisions, then spawn or clear the wave.
    pub fn step(&mut self) {
        if self.game_over() {
            return;
        }

        // Advance incoming missiles on their diagonal descent.
        for m in &mut self.missiles {
            m.r += 1;
            if m.c < m.tc {
                m.c += 1;
            } else if m.c > m.tc {
                m.c -= 1;
            }
        }

        // Advance interceptors; detonate into a blast on arrival.
        let mut flying = Vec::new();
        for mut it in std::mem::take(&mut self.interceptors) {
            if it.r < it.tr {
                it.r += 1;
            } else if it.r > it.tr {
                it.r -= 1;
            }
            if it.c < it.tc {
                it.c += 1;
            } else if it.c > it.tc {
                it.c -= 1;
            }
            if it.r == it.tr && it.c == it.tc {
                self.blasts.push(Blast {
                    r: it.tr,
                    c: it.tc,
                    radius: 1,
                    ttl: BLAST_TTL,
                });
            } else {
                flying.push(it);
            }
        }
        self.interceptors = flying;

        // Expand and expire blasts.
        for b in &mut self.blasts {
            b.radius = (b.radius + 1).min(BLAST_MAX);
            b.ttl -= 1;
        }
        self.blasts.retain(|b| b.ttl > 0);

        // Resolve blast∩missile: any missile inside an active blast is destroyed.
        let blasts = self.blasts.clone();
        let mut survivors = Vec::new();
        for m in std::mem::take(&mut self.missiles) {
            let hit = blasts
                .iter()
                .any(|b| (m.r - b.r).abs() <= b.radius && (m.c - b.c).abs() <= b.radius);
            if hit {
                self.score += 1;
            } else {
                survivors.push(m);
            }
        }
        self.missiles = survivors;

        // Resolve missile∩city: missiles reaching the ground destroy their city.
        let mut airborne = Vec::new();
        for m in std::mem::take(&mut self.missiles) {
            if m.r >= CITY_ROW {
                for city in &mut self.cities {
                    if city.alive && city.col == m.c {
                        city.alive = false;
                        break;
                    }
                }
            } else {
                airborne.push(m);
            }
        }
        self.missiles = airborne;

        // Spawn the next missile, or clear the wave once the quota is exhausted.
        if !self.game_over() {
            if self.spawned < self.quota {
                self.spawn_timer -= 1;
                if self.spawn_timer <= 0 {
                    self.spawn_missile();
                    self.spawned += 1;
                    self.spawn_timer = self.spawn_period;
                }
            } else if self.missiles.is_empty() {
                self.wave += 1;
                self.score += WAVE_BONUS;
                self.spawned = 0;
                self.quota += 2;
                self.spawn_period = (self.spawn_period - 1).max(4);
                self.spawn_timer = self.spawn_period;
            }
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Missile Command overlay.
pub struct MissileCommand {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl MissileCommand {
    pub fn new() -> Self {
        MissileCommand {
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

    /// Running = cities remain and not paused; only then do we keep the loop going.
    fn running(&self) -> bool {
        !self.game.game_over() && !self.paused
    }
}

impl Default for MissileCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for MissileCommand {
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
            key!(Left) | key!('h') => self.game.move_cross(0, -1),
            key!(Right) | key!('l') => self.game.move_cross(0, 1),
            key!(Up) | key!('k') => self.game.move_cross(-1, 0),
            key!(Down) | key!('j') => self.game.move_cross(1, 0),
            key!(' ') => self.game.fire(),
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
        let focus_style = theme.get("ui.text.focus");
        let linenr_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let warn_style = theme.get("warning");
        let error_style = theme.get("error");
        let func_style = theme.get("function");

        surface.clear_with(area, bg);
        if area.width < (W as u16) + 4 || area.height < (H as u16) + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let cities_left = self.game.cities.iter().filter(|c| c.alive).count();
        surface.set_string(
            ox,
            area.y,
            &format!(
                "Missile Command  score {}  cities {}",
                self.game.score, cities_left
            ),
            header_style,
        );

        // Cities and the ground battery.
        for city in &self.game.cities {
            let (glyph, st) = if city.alive {
                ("▟▙", func_style)
            } else {
                ("░░", linenr_style)
            };
            if on_board(CITY_ROW, city.col) {
                surface.set_string(ox + city.col as u16, oy + CITY_ROW as u16, glyph, st);
            }
        }
        if on_board(CITY_ROW, BAT_COL) {
            surface.set_string(ox + BAT_COL as u16, oy + CITY_ROW as u16, "▲", focus_style);
        }

        // Incoming missiles with a faint trail.
        for m in &self.game.missiles {
            if on_board(m.r - 1, m.c) {
                surface.set_string(ox + m.c as u16, oy + (m.r - 1) as u16, "*", linenr_style);
            }
            if on_board(m.r, m.c) {
                surface.set_string(ox + m.c as u16, oy + m.r as u16, "↓", error_style);
            }
        }

        // Interceptors climbing to their target.
        for it in &self.game.interceptors {
            if on_board(it.r, it.c) {
                surface.set_string(ox + it.c as u16, oy + it.r as u16, "▲", focus_style);
            }
        }

        // Blasts: an expanding ring around a bright centre.
        for b in &self.game.blasts {
            for dr in -b.radius..=b.radius {
                for dc in -b.radius..=b.radius {
                    if dr.abs().max(dc.abs()) > b.radius {
                        continue;
                    }
                    let (r, c) = (b.r + dr, b.c + dc);
                    if on_board(r, c) {
                        let glyph = if dr == 0 && dc == 0 { "✷" } else { "◌" };
                        surface.set_string(ox + c as u16, oy + r as u16, glyph, warn_style);
                    }
                }
            }
        }

        // The aiming crosshair, highlighted, drawn last so it stays on top.
        let (cr, cc) = self.game.cross;
        for dc in [-1i16, 1] {
            if on_board(cr, cc + dc) {
                surface.set_string(ox + (cc + dc) as u16, oy + cr as u16, " ", sel_style);
            }
        }
        if on_board(cr, cc) {
            surface.set_string(ox + cc as u16, oy + cr as u16, "+", focus_style);
        }

        let sy = oy + H as u16;
        let footer = if self.game.game_over() {
            format!("Game over — score {}.  n new  q quit", self.game.score)
        } else if self.paused {
            "PAUSED — p resume · n new · q quit".to_string()
        } else {
            "arrows/hjkl aim · SPC fire · p pause · n new · q quit".to_string()
        };
        surface.set_string(ox, sy, &footer, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interceptor_detonates_at_the_crosshair() {
        let mut g = Game::new(1);
        g.spawn_timer = 999; // keep the spawner quiet
        g.cross = (CITY_ROW - 1, BAT_COL);
        g.fire();
        g.step();
        assert!(
            g.blasts
                .iter()
                .any(|b| b.r == CITY_ROW - 1 && b.c == BAT_COL),
            "a blast forms at the crosshair once the interceptor arrives"
        );
    }

    #[test]
    fn blast_destroys_a_missile_and_scores() {
        let mut g = Game::new(1);
        g.spawn_timer = 999;
        g.missiles.clear();
        g.interceptors.clear();
        g.missiles.push(Missile {
            r: 5,
            c: 10,
            tc: 10,
        });
        g.blasts.push(Blast {
            r: 6,
            c: 10,
            radius: 3,
            ttl: 5,
        });
        let before = g.score;
        g.step();
        assert!(g.missiles.is_empty(), "the missile in the blast is gone");
        assert_eq!(g.score, before + 1, "destroying a missile scores a point");
    }

    #[test]
    fn missile_reaching_a_city_destroys_it() {
        let mut g = Game::new(1);
        g.spawn_timer = 999;
        g.missiles.clear();
        let cx = CITY_COLS[0];
        g.missiles.push(Missile {
            r: CITY_ROW - 1,
            c: cx,
            tc: cx,
        });
        g.step();
        assert!(!g.cities[0].alive, "the targeted city is destroyed");
        assert!(g.missiles.is_empty(), "the grounded missile is consumed");
    }

    #[test]
    fn all_cities_destroyed_ends_the_game() {
        let mut g = Game::new(1);
        g.spawn_timer = 999;
        for c in g.cities.iter_mut() {
            c.alive = false;
        }
        g.cities[0].alive = true; // one city left standing
        g.missiles.clear();
        let cx = g.cities[0].col;
        g.missiles.push(Missile {
            r: CITY_ROW - 1,
            c: cx,
            tc: cx,
        });
        assert!(!g.game_over(), "still one city alive before the hit");
        g.step();
        assert!(g.game_over(), "losing the last city ends the game");
    }

    #[test]
    fn clearing_a_wave_advances_and_awards_a_bonus() {
        let mut g = Game::new(1);
        g.missiles.clear();
        g.spawned = g.quota; // whole wave has been launched
        let wave = g.wave;
        let score = g.score;
        g.step();
        assert_eq!(g.wave, wave + 1, "the next wave begins");
        assert_eq!(
            g.score,
            score + WAVE_BONUS,
            "clearing a wave awards a bonus"
        );
    }
}
