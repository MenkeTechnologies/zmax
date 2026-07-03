//! Pong — the zemacs port of GNU Emacs `pong`.
//!
//! Bounce the ball past the computer's paddle. Move your (left) paddle with the
//! arrows or `k`/`j`, `SPC` pauses, `n` restarts, `q`/`Esc` quits. Like the other
//! action games it animates itself via `zemacs_event::request_redraw` only while
//! playing. The ball/paddle physics is pure and unit-tested (keys parse into a
//! `pong` keymap mode by `scripts/gen_port_report.py`).

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 44;
const H: i16 = 22;
const PADDLE: i16 = 5;
const LEFT_COL: i16 = 1;
const RIGHT_COL: i16 = W - 2;

/// The pure pong court. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub ball: (i16, i16),
    pub vel: (i16, i16),
    /// Paddle tops (a paddle spans `PADDLE` rows).
    pub left_y: i16,
    pub right_y: i16,
    pub left_score: u32,
    pub right_score: u32,
    serve_dir: i16,
}

fn covers(top: i16, r: i16) -> bool {
    r >= top && r < top + PADDLE
}

impl Game {
    pub fn new() -> Self {
        let mut g = Game {
            ball: (0, 0),
            vel: (0, 0),
            left_y: (H - PADDLE) / 2,
            right_y: (H - PADDLE) / 2,
            left_score: 0,
            right_score: 0,
            serve_dir: 1,
        };
        g.serve();
        g
    }

    fn serve(&mut self) {
        self.ball = (H / 2, W / 2);
        self.vel = (1, self.serve_dir);
        self.serve_dir = -self.serve_dir; // alternate serves
    }

    /// Move the player's paddle by `d` rows, kept on the court.
    pub fn move_left(&mut self, d: i16) {
        self.left_y = (self.left_y + d).clamp(0, H - PADDLE);
    }

    /// One physics step: bounce off the top/bottom walls and the paddles, and
    /// score when the ball passes a paddle. The computer paddle tracks the ball.
    pub fn step(&mut self) {
        // Vertical wall bounce.
        if self.ball.0 + self.vel.0 < 0 || self.ball.0 + self.vel.0 >= H {
            self.vel.0 = -self.vel.0;
        }
        self.ball.0 = (self.ball.0 + self.vel.0).clamp(0, H - 1);

        // Horizontal: paddles and scoring.
        let nc = self.ball.1 + self.vel.1;
        if nc <= LEFT_COL {
            if covers(self.left_y, self.ball.0) {
                self.vel.1 = 1;
                self.ball.1 = LEFT_COL + 1;
            } else if nc < 0 {
                self.right_score += 1;
                self.serve();
                return;
            } else {
                self.ball.1 = nc;
            }
        } else if nc >= RIGHT_COL {
            if covers(self.right_y, self.ball.0) {
                self.vel.1 = -1;
                self.ball.1 = RIGHT_COL - 1;
            } else if nc >= W {
                self.left_score += 1;
                self.serve();
                return;
            } else {
                self.ball.1 = nc;
            }
        } else {
            self.ball.1 = nc;
        }

        // Computer paddle tracks the ball's row toward its own centre.
        let target = self.ball.0 - PADDLE / 2;
        if target > self.right_y {
            self.right_y = (self.right_y + 1).min(H - PADDLE);
        } else if target < self.right_y {
            self.right_y = (self.right_y - 1).max(0);
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

/// The interactive Pong overlay.
pub struct Pong {
    game: Game,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Pong {
    pub fn new() -> Self {
        Pong {
            game: Game::new(),
            paused: false,
            last: None,
            interval: Duration::from_millis(80),
        }
    }

    fn running(&self) -> bool {
        !self.paused
    }
}

impl Default for Pong {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Pong {
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
            key!(Up) | key!('k') => self.game.move_left(-2),
            key!(Down) | key!('j') => self.game.move_left(2),
            key!(' ') => self.paused = !self.paused,
            key!('n') => self.game = Game::new(),
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
            zemacs_event::request_redraw();
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let wall_style = theme.get("ui.linenr");
        let paddle_style = theme.get("ui.text.focus");
        let ball_style = theme.get("warning");

        surface.clear_with(area, bg);
        if area.width < (W as u16) + 4 || area.height < (H as u16) + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(
            ox,
            area.y,
            &format!("Pong    you {}  —  {} cpu", self.game.left_score, self.game.right_score),
            header_style,
        );

        for c in 0..W {
            surface.set_string(ox + c as u16, oy - 1, "─", wall_style);
            surface.set_string(ox + c as u16, oy + H as u16, "─", wall_style);
        }
        let cell = |r: i16, c: i16| (ox + c as u16, oy + r as u16);
        for i in 0..PADDLE {
            let (lx, ly) = cell(self.game.left_y + i, LEFT_COL);
            surface.set_string(lx, ly, "█", paddle_style);
            let (rx, ry) = cell(self.game.right_y + i, RIGHT_COL);
            surface.set_string(rx, ry, "█", paddle_style);
        }
        let (bx, by) = cell(self.game.ball.0, self.game.ball.1);
        surface.set_string(bx, by, "●", ball_style);

        let sy = oy + H as u16 + 1;
        let hint = if self.paused {
            "PAUSED — SPC resume · n new · q quit"
        } else {
            "↑/k up · ↓/j down · SPC pause · n new · q quit"
        };
        surface.set_string(ox, sy, hint, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ball_bounces_off_the_top_wall() {
        let mut g = Game::new();
        g.ball = (0, W / 2);
        g.vel = (-1, 1);
        g.step();
        assert_eq!(g.vel.0, 1, "vertical velocity flips at the top wall");
    }

    #[test]
    fn ball_bounces_off_the_player_paddle() {
        let mut g = Game::new();
        g.left_y = 8;
        g.ball = (10, LEFT_COL + 1);
        g.vel = (0, -1);
        g.step();
        assert_eq!(g.vel.1, 1, "ball reflects off the paddle it covers");
    }

    #[test]
    fn ball_past_the_player_scores_for_the_computer() {
        let mut g = Game::new();
        g.left_y = 0; // paddle at the top
        g.ball = (H - 1, 0); // already behind the paddle line, missing it
        g.vel = (0, -1);
        let before = g.right_score;
        g.step(); // next move takes it off the court (nc < 0) → point for the cpu
        assert_eq!(g.right_score, before + 1);
    }

    #[test]
    fn computer_paddle_tracks_the_ball() {
        let mut g = Game::new();
        g.right_y = 0;
        g.ball = (H - 2, W / 2);
        g.vel = (1, 1);
        g.step();
        assert!(g.right_y > 0, "cpu paddle moves toward a low ball");
    }

    #[test]
    fn player_paddle_stays_on_court() {
        let mut g = Game::new();
        for _ in 0..50 {
            g.move_left(-2);
        }
        assert_eq!(g.left_y, 0);
        for _ in 0..50 {
            g.move_left(2);
        }
        assert_eq!(g.left_y, H - PADDLE);
    }
}
