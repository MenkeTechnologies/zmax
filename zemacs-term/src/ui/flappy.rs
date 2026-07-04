//! Flappy — a tiny Flappy-Bird for zemacs, cut from the same cloth as `pong`.
//!
//! Flap with `SPC` (or `↑`) to fight gravity and thread the scrolling pipes;
//! clear a pipe for a point, and touching a pipe, the ground or the ceiling ends
//! the run. `p` pauses, `n` starts a new game (keeping your best score), `q`/`Esc`
//! quits. Like `pong` it is a real-time frame loop: each tick advances on
//! wall-clock delta and schedules the next via `zemacs_event::request_redraw`
//! only while playing, so it idles when paused, dead or closed. The board logic
//! is pure and unit-tested.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 44;
const H: i16 = 20;
/// The bird lives on this fixed column; only pipes there can hit it.
const BIRD_COL: i16 = 8;
/// Rows of clear air in each pipe.
const GAP: i16 = 6;
/// Columns between successive pipes.
const SPACING: i16 = 16;
const GRAVITY: i16 = 1;
const MAX_VEL: i16 = 3;
const FLAP: i16 = -2;

/// The pure flappy board. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    /// Bird row (0 = ceiling); it never leaves `BIRD_COL`.
    pub bird_y: i16,
    /// Vertical velocity — positive falls, negative rises.
    pub vel: i16,
    /// Pipes as `(column, gap_top)`; the gap spans `gap_top..gap_top + GAP`.
    pub pipes: Vec<(i16, i16)>,
    pub score: u32,
    pub dead: bool,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            bird_y: H / 2,
            vel: 0,
            pipes: Vec::new(),
            score: 0,
            dead: false,
            rng: seed | 1,
        };
        let gap = g.rand_gap();
        g.pipes.push((W - 1, gap));
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// A random gap top that keeps the whole gap on the board.
    fn rand_gap(&mut self) -> i16 {
        1 + (self.rand() % (H - GAP - 1) as u64) as i16
    }

    /// Give the bird an upward impulse.
    pub fn flap(&mut self) {
        self.vel = FLAP;
    }

    /// Advance one step: gravity pulls the bird down, pipes scroll left, a cleared
    /// pipe scores, and hitting a pipe, the ground or the ceiling is fatal.
    pub fn step(&mut self) {
        if self.dead {
            return;
        }
        // Gravity, then move the bird.
        self.vel = (self.vel + GRAVITY).min(MAX_VEL);
        self.bird_y += self.vel;
        if self.bird_y < 0 || self.bird_y >= H {
            self.dead = true;
            return;
        }
        // Scroll every pipe one column left.
        for p in self.pipes.iter_mut() {
            p.0 -= 1;
        }
        // A pipe on the bird's column must have its gap over the bird; the step it
        // slips just past scores a point.
        for &(col, gap_top) in self.pipes.iter() {
            if col == BIRD_COL {
                let in_gap = self.bird_y >= gap_top && self.bird_y < gap_top + GAP;
                if !in_gap {
                    self.dead = true;
                    return;
                }
            } else if col == BIRD_COL - 1 {
                self.score += 1;
            }
        }
        // Retire off-screen pipes and keep the stream flowing.
        self.pipes.retain(|p| p.0 >= 0);
        if self.pipes.iter().all(|p| p.0 <= W - SPACING) {
            let gap = self.rand_gap();
            self.pipes.push((W - 1, gap));
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Flappy overlay.
pub struct Flappy {
    game: Game,
    seed: u64,
    best: u32,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Flappy {
    pub fn new() -> Self {
        Flappy {
            game: Game::new(1),
            seed: 1,
            best: 0,
            paused: false,
            last: None,
            interval: Duration::from_millis(110),
        }
    }

    fn restart(&mut self) {
        self.best = self.best.max(self.game.score);
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.paused = false;
        self.last = None;
    }

    /// Running = alive and not paused; only then does the frame loop tick.
    fn running(&self) -> bool {
        !self.game.dead && !self.paused
    }
}

impl Default for Flappy {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Flappy {
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
            key!(' ') | key!(Up) => self.game.flap(),
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
        let wall_style = theme.get("ui.linenr");
        let pipe_style = theme.get("function");
        let bird_style = theme.get("warning");
        let over_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < (W as u16) + 4 || area.height < (H as u16) + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        let best = self.best.max(self.game.score);
        surface.set_string(
            ox,
            area.y,
            &format!("Flappy  score {}  best {}", self.game.score, best),
            header_style,
        );

        // Ground and ceiling.
        for c in 0..W {
            surface.set_string(ox + c as u16, oy - 1, "─", wall_style);
            surface.set_string(ox + c as u16, oy + H as u16, "─", wall_style);
        }

        let cell = |r: i16, c: i16| (ox + c as u16, oy + r as u16);
        // Pipes: solid walls except the open gap.
        for &(col, gap_top) in self.game.pipes.iter() {
            if col < 0 || col >= W {
                continue;
            }
            for r in 0..H {
                if r < gap_top || r >= gap_top + GAP {
                    let (x, y) = cell(r, col);
                    surface.set_string(x, y, "█", pipe_style);
                }
            }
        }
        let (bx, by) = cell(self.game.bird_y, BIRD_COL);
        surface.set_string(bx, by, "◐", bird_style);

        let sy = oy + H as u16 + 1;
        if self.game.dead {
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
            surface.set_string(ox, sy, "PAUSED — p resume · n new · q quit", text_style);
        } else {
            surface.set_string(ox, sy, "SPC/↑ flap · p pause · n new · q quit", text_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravity_pulls_the_bird_down() {
        let mut g = Game::new(1);
        g.pipes.clear(); // no pipe interference
        let (y0, v0) = (g.bird_y, g.vel);
        g.step();
        assert!(g.vel > v0, "downward velocity grows");
        assert!(g.bird_y > y0, "the bird falls (row increases)");
    }

    #[test]
    fn a_flap_lifts_the_bird() {
        let mut g = Game::new(1);
        g.pipes.clear();
        g.flap();
        let y0 = g.bird_y;
        g.step();
        assert!(g.bird_y < y0, "a flap moves the bird up on the next step");
    }

    #[test]
    fn hitting_the_ground_ends_the_game() {
        let mut g = Game::new(1);
        g.pipes.clear();
        g.bird_y = H - 1;
        g.vel = MAX_VEL;
        g.step();
        assert!(g.dead, "falling through the floor is fatal");
    }

    #[test]
    fn passing_a_pipe_scores() {
        let mut g = Game::new(1);
        g.pipes.clear();
        // Pipe one column right of the bird, gap wide open around it.
        g.pipes.push((BIRD_COL + 1, 8)); // gap rows 8..14
        g.bird_y = 10;
        g.vel = 0;
        let before = g.score;
        g.step(); // pipe reaches BIRD_COL, bird in the gap — survives
        assert!(!g.dead);
        g.step(); // pipe slips to BIRD_COL - 1 — a point
        assert_eq!(g.score, before + 1);
    }

    #[test]
    fn hitting_a_pipe_body_ends_the_game() {
        let mut g = Game::new(1);
        g.pipes.clear();
        // Gap near the ceiling; the bird sits well below it in the wall.
        g.pipes.push((BIRD_COL + 1, 0)); // gap rows 0..6
        g.bird_y = 12;
        g.vel = 0;
        g.step(); // pipe reaches BIRD_COL, bird outside the gap — crash
        assert!(g.dead, "flying into a pipe body is fatal");
    }
}
