//! Snake — the zemacs port of GNU Emacs `snake`.
//!
//! Steer the snake to eat food and grow; hitting a wall or yourself ends the
//! game. Turn with the arrows or `hjkl`, `SPC` pauses, `n` restarts, `q`/`Esc`
//! quits. The game animates itself with no always-on timer: each frame advances
//! on wall-clock delta and schedules the next via `zemacs_event::request_redraw`
//! only while running, so it idles when paused, dead or closed. The board logic
//! is pure and unit-tested (keys parse into a `snake` keymap mode by
//! `scripts/gen_port_report.py`).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 30;
const H: i16 = 20;

/// The pure snake board. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    /// Body cells, head at the front.
    pub body: VecDeque<(i16, i16)>,
    pub dir: (i16, i16),
    pub food: (i16, i16),
    pub alive: bool,
    pub score: u32,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut body = VecDeque::new();
        let (hr, hc) = (H / 2, W / 2);
        body.push_back((hr, hc));
        body.push_back((hr, hc - 1));
        body.push_back((hr, hc - 2));
        let mut g = Game {
            body,
            dir: (0, 1),
            food: (0, 0),
            alive: true,
            score: 0,
            rng: seed | 1,
        };
        g.spawn_food();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    fn spawn_food(&mut self) {
        // Reject cells occupied by the snake (the board is far larger than it).
        for _ in 0..10_000 {
            let r = (self.rand() % H as u64) as i16;
            let c = (self.rand() % W as u64) as i16;
            if !self.body.contains(&(r, c)) {
                self.food = (r, c);
                return;
            }
        }
    }

    /// Queue a new heading; a 180° reversal is ignored (you can't turn back into
    /// your own neck).
    pub fn steer(&mut self, dir: (i16, i16)) {
        if (dir.0 + self.dir.0, dir.1 + self.dir.1) != (0, 0) {
            self.dir = dir;
        }
    }

    /// Advance one step: move the head, die on a wall or self-collision, grow and
    /// respawn food when eating.
    pub fn step(&mut self) {
        if !self.alive {
            return;
        }
        let (hr, hc) = *self.body.front().unwrap();
        let head = (hr + self.dir.0, hc + self.dir.1);
        if head.0 < 0 || head.0 >= H || head.1 < 0 || head.1 >= W {
            self.alive = false;
            return;
        }
        // Self-collision — but the tail cell frees up unless we're about to grow.
        let growing = head == self.food;
        let hits_self = if growing {
            self.body.contains(&head)
        } else {
            self.body
                .iter()
                .take(self.body.len() - 1)
                .any(|&c| c == head)
        };
        if hits_self {
            self.alive = false;
            return;
        }
        self.body.push_front(head);
        if growing {
            self.score += 1;
            self.spawn_food();
        } else {
            self.body.pop_back();
        }
    }
}

/// The interactive Snake overlay.
pub struct Snake {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Snake {
    pub fn new() -> Self {
        Snake {
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

impl Default for Snake {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Snake {
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
            key!(Left) | key!('h') => self.game.steer((0, -1)),
            key!(Right) | key!('l') => self.game.steer((0, 1)),
            key!(Up) | key!('k') => self.game.steer((-1, 0)),
            key!(Down) | key!('j') => self.game.steer((1, 0)),
            key!(' ') => self.paused = !self.paused,
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
        let wall_style = theme.get("ui.linenr");
        let snake_style = theme.get("ui.text.focus");
        let food_style = theme.get("warning");

        surface.clear_with(area, bg);
        if area.width < (W as u16) + 4 || area.height < (H as u16) + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(ox, area.y, "Snake — eat and grow", header_style);

        // Border.
        for c in -1..=W {
            surface.set_string(ox + (c + 1) as u16, oy - 1, "─", wall_style);
            surface.set_string(ox + (c + 1) as u16, oy + H as u16, "─", wall_style);
        }
        // Side walls span the interior rows (the corners are on the top/bottom
        // border above); r stays >= 0 so the u16 cast never wraps.
        for r in 0..H {
            let y = oy + r as u16;
            surface.set_string(ox, y, "│", wall_style);
            surface.set_string(ox + (W + 1) as u16, y, "│", wall_style);
        }

        let cell = |r: i16, c: i16| (ox + (c + 1) as u16, oy + r as u16);
        let (fx, fy) = cell(self.game.food.0, self.game.food.1);
        surface.set_string(fx, fy, "●", food_style);
        for (i, &(r, c)) in self.game.body.iter().enumerate() {
            let (x, y) = cell(r, c);
            surface.set_string(x, y, if i == 0 { "█" } else { "▓" }, snake_style);
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
            format!(
                "score {}   ·  arrows/hjkl turn  SPC pause  n new  q quit",
                self.game.score
            )
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_moves_the_head() {
        let mut g = Game::new(1);
        let (hr, hc) = *g.body.front().unwrap();
        g.food = (0, 0); // out of the way
        g.step();
        assert_eq!(*g.body.front().unwrap(), (hr, hc + 1));
        assert_eq!(g.body.len(), 3, "length unchanged when not eating");
    }

    #[test]
    fn eating_food_grows_and_scores() {
        let mut g = Game::new(1);
        let (hr, hc) = *g.body.front().unwrap();
        g.food = (hr, hc + 1); // directly ahead
        let len = g.body.len();
        g.step();
        assert_eq!(g.body.len(), len + 1);
        assert_eq!(g.score, 1);
    }

    #[test]
    fn hitting_a_wall_ends_the_game() {
        let mut g = Game::new(1);
        g.food = (0, 0);
        g.dir = (0, 1);
        for _ in 0..W {
            g.step();
        }
        assert!(!g.alive);
    }

    #[test]
    fn reversal_is_ignored() {
        let mut g = Game::new(1); // heading right
        g.steer((0, -1)); // try to reverse
        assert_eq!(g.dir, (0, 1), "180-degree turn must be rejected");
        g.steer((-1, 0)); // a legal turn
        assert_eq!(g.dir, (-1, 0));
    }

    #[test]
    fn running_into_yourself_ends_the_game() {
        let mut g = Game::new(1);
        g.food = (0, 0);
        // A long enough body turned in a tight box will bite itself: build a
        // vertical stack then turn back across it.
        g.body.clear();
        for r in 0..5 {
            g.body.push_back((r, 5));
        }
        // Head is at (0,5); heading up would leave; head down into the body.
        g.dir = (1, 0);
        g.step();
        assert!(!g.alive, "moving into your own body is fatal");
    }
}
