//! Breakout — a small brick-breaker in the spirit of the other zmax arcade
//! ports.
//!
//! Bounce the ball off your paddle to chip away the wall of bricks. Slide the
//! paddle with the arrows or `h`/`l`, `SPC` pauses, `n` restarts, `q`/`Esc`
//! quits. Like `pong` and `snake` it animates itself via
//! `zmax_event::request_redraw` only while playing, idling when paused, won
//! or dead. The ball/brick/paddle physics is pure and unit-tested (keys parse
//! into a `breakout` keymap mode by `scripts/gen_port_report.py`).

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 44;
const H: i16 = 22;
const PADDLE_W: i16 = 7;
const PADDLE_ROW: i16 = H - 2;
const BRICK_TOP: i16 = 1;
const BRICK_ROWS: usize = 5;
const BRICK_COLS: usize = 11;
const BRICK_W: i16 = W / BRICK_COLS as i16;

/// The paddle (a `PADDLE_W`-wide bar) covers column `c` when it starts at `left`.
fn covers(left: i16, c: i16) -> bool {
    c >= left && c < left + PADDLE_W
}

/// The pure breakout court. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub ball: (i16, i16),
    pub vel: (i16, i16),
    /// Left column of the paddle bar.
    pub paddle_x: i16,
    /// `bricks[row][col]` — `true` while the brick still stands.
    pub bricks: Vec<Vec<bool>>,
    pub remaining: u32,
    pub score: u32,
    pub lives: u32,
    pub alive: bool,
    pub won: bool,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let bricks = vec![vec![true; BRICK_COLS]; BRICK_ROWS];
        let mut g = Game {
            ball: (0, 0),
            vel: (0, 0),
            paddle_x: (W - PADDLE_W) / 2,
            bricks,
            remaining: (BRICK_ROWS * BRICK_COLS) as u32,
            score: 0,
            lives: 3,
            alive: true,
            won: false,
            rng: seed | 1,
        };
        g.serve();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Place the ball just above the paddle heading up, with a rng-chosen sideways
    /// direction so each serve is a little different.
    fn serve(&mut self) {
        let dir = if self.rand() & 1 == 0 { -1 } else { 1 };
        self.ball = (PADDLE_ROW - 1, W / 2);
        self.vel = (-1, dir);
    }

    /// Slide the paddle by `d` columns, kept on the court.
    pub fn move_paddle(&mut self, d: i16) {
        self.paddle_x = (self.paddle_x + d).clamp(0, W - PADDLE_W);
    }

    /// The standing brick covering board cell `(r, c)`, if any.
    fn brick_at(&self, r: i16, c: i16) -> Option<(usize, usize)> {
        if r >= BRICK_TOP && r < BRICK_TOP + BRICK_ROWS as i16 {
            let br = (r - BRICK_TOP) as usize;
            let bc = (c / BRICK_W) as usize;
            if bc < BRICK_COLS && self.bricks[br][bc] {
                return Some((br, bc));
            }
        }
        None
    }

    /// Drop the ball below the paddle: lose a life and re-serve, or end the game
    /// when the last life is gone.
    fn lose_life(&mut self) {
        self.lives = self.lives.saturating_sub(1);
        if self.lives == 0 {
            self.alive = false;
        } else {
            self.serve();
        }
    }

    /// One physics step: move the ball, then resolve wall, paddle and brick
    /// collisions, updating the score and lives.
    pub fn step(&mut self) {
        if !self.alive || self.won {
            return;
        }

        // Horizontal: bounce off the side walls, otherwise advance.
        let nc = self.ball.1 + self.vel.1;
        if !(0..W).contains(&nc) {
            self.vel.1 = -self.vel.1;
        } else {
            self.ball.1 = nc;
        }

        // Vertical: ceiling bounces, the floor costs a life.
        let nr = self.ball.0 + self.vel.0;
        if nr < 0 {
            self.vel.0 = -self.vel.0;
        } else if nr >= H {
            self.lose_life();
            return;
        } else {
            self.ball.0 = nr;
        }

        // Paddle: reflect upward, steering by where the ball struck the bar.
        if self.vel.0 > 0 && self.ball.0 == PADDLE_ROW && covers(self.paddle_x, self.ball.1) {
            self.vel.0 = -self.vel.0;
            let hit = self.ball.1 - self.paddle_x;
            self.vel.1 = if hit < PADDLE_W / 2 { -1 } else { 1 };
            self.ball.0 = PADDLE_ROW - 1;
        }

        // Brick: knock it out, score, and bounce back the way we came.
        if let Some((br, bc)) = self.brick_at(self.ball.0, self.ball.1) {
            self.bricks[br][bc] = false;
            self.remaining -= 1;
            self.score += 10;
            self.vel.0 = -self.vel.0;
            if self.remaining == 0 {
                self.won = true;
            }
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Breakout overlay.
pub struct Breakout {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Breakout {
    pub fn new() -> Self {
        Breakout {
            game: Game::new(1),
            seed: 1,
            paused: false,
            last: None,
            interval: Duration::from_millis(80),
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.paused = false;
        self.last = None;
    }

    /// Running = the game is live (not won, not lost) and not paused; only then
    /// do we keep the frame loop going.
    fn running(&self) -> bool {
        self.game.alive && !self.game.won && !self.paused
    }
}

impl Default for Breakout {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Breakout {
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
            key!(Left) | key!('h') => self.game.move_paddle(-3),
            key!(Right) | key!('l') => self.game.move_paddle(3),
            key!(' ') => self.paused = !self.paused,
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
        let wall_style = theme.get("ui.linenr");
        let paddle_style = theme.get("ui.selection");
        let ball_style = theme.get("warning");
        let brick_styles = [
            theme.get("error"),
            theme.get("warning"),
            theme.get("function"),
            theme.get("ui.text.focus"),
        ];

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
                "Breakout  score {}  lives {}",
                self.game.score, self.game.lives
            ),
            header_style,
        );

        // Top and bottom walls (the ball falls through past the paddle above the
        // bottom wall, so the floor is just a frame).
        for c in 0..W {
            surface.set_string(ox + c as u16, oy - 1, "─", wall_style);
            surface.set_string(ox + c as u16, oy + H as u16, "─", wall_style);
        }

        let cell = |r: i16, c: i16| (ox + c as u16, oy + r as u16);

        // Bricks, coloured by row.
        for br in 0..BRICK_ROWS {
            let style = brick_styles[br % brick_styles.len()];
            let r = BRICK_TOP + br as i16;
            for bc in 0..BRICK_COLS {
                if self.game.bricks[br][bc] {
                    for k in 0..BRICK_W {
                        let (x, y) = cell(r, bc as i16 * BRICK_W + k);
                        surface.set_string(x, y, "▩", style);
                    }
                }
            }
        }

        // Paddle.
        for i in 0..PADDLE_W {
            let (px, py) = cell(PADDLE_ROW, self.game.paddle_x + i);
            surface.set_string(px, py, "█", paddle_style);
        }

        // Ball.
        let (bx, by) = cell(self.game.ball.0, self.game.ball.1);
        surface.set_string(bx, by, "●", ball_style);

        let sy = oy + H as u16 + 1;
        let status = if self.game.won {
            format!(
                "You cleared the wall! — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if !self.game.alive {
            format!(
                "Game over — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.paused {
            format!("Paused — score {}.  SPC resume", self.game.score)
        } else {
            "←/h left · →/l right · SPC pause · n new · q quit".to_string()
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_game_serves_a_full_wall() {
        let g = Game::new(1);
        assert_eq!(g.lives, 3);
        assert_eq!(g.remaining, (BRICK_ROWS * BRICK_COLS) as u32);
        assert!(g.vel.0 < 0, "the serve heads up toward the bricks");
        assert!(g.alive && !g.won);
    }

    #[test]
    fn ball_bounces_off_the_ceiling() {
        let mut g = Game::new(1);
        g.ball = (0, W / 2);
        g.vel = (-1, 1);
        g.step();
        assert_eq!(g.vel.0, 1, "vertical velocity flips at the top wall");
    }

    #[test]
    fn hitting_a_brick_removes_it_and_scores() {
        let mut g = Game::new(1);
        g.ball = (BRICK_TOP + BRICK_ROWS as i16, 6); // just below the bottom brick row
        g.vel = (-1, 0);
        let before = g.remaining;
        g.step();
        assert_eq!(g.remaining, before - 1, "one brick is knocked out");
        assert_eq!(g.score, 10);
        assert_eq!(g.vel.0, 1, "the ball rebounds downward off the brick");
    }

    #[test]
    fn ball_bounces_off_the_paddle() {
        let mut g = Game::new(1);
        g.ball = (PADDLE_ROW - 1, g.paddle_x + PADDLE_W / 2);
        g.vel = (1, 0);
        g.step();
        assert_eq!(g.vel.0, -1, "the ball reflects up off the paddle");
        assert_eq!(g.ball.0, PADDLE_ROW - 1, "and is nudged back above it");
    }

    #[test]
    fn missing_the_ball_costs_a_life_and_re_serves() {
        let mut g = Game::new(1);
        g.paddle_x = 0; // paddle at the far left, nowhere near the ball
        g.ball = (H - 1, W / 2); // already below the paddle line
        g.vel = (1, 0);
        let before = g.lives;
        g.step();
        assert_eq!(g.lives, before - 1, "a missed ball costs a life");
        assert!(g.alive, "still playing with lives to spare");
        assert!(g.vel.0 < 0, "the re-serve heads back up");
    }
}
