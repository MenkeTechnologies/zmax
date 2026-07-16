//! Tron — light-cycle duel in the spirit of the classic terminal games.
//!
//! Two cycles race around a bordered arena leaving solid trails behind them;
//! crash into a wall or ANY trail and you're out. You steer the blue cycle with
//! the arrows or `hjkl` (no 180° turn into your own neck); the orange cycle is
//! driven by the computer with a small survival heuristic. `SPC` pauses, `n`
//! starts a new round, `q`/`Esc` quits. Like the other action games it animates
//! itself via `zmax_event::request_redraw` only while playing. The arena logic
//! is pure and unit-tested (keys parse into a `tron` keymap mode by
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

/// The pure Tron arena. No I/O, no timing — unit-tested.
///
/// `grid` tracks trails: `0` empty, `1` the player's trail, `2` the CPU's. A
/// cell is blocked when it leaves the arena or already holds a trail.
#[derive(Clone)]
pub struct Game {
    pub p_pos: (i16, i16),
    pub p_dir: (i16, i16),
    pub p_alive: bool,
    pub c_pos: (i16, i16),
    pub c_dir: (i16, i16),
    pub c_alive: bool,
    pub grid: Vec<u8>,
    pub p_wins: u32,
    pub c_wins: u32,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            p_pos: (H / 2, W / 4),
            p_dir: (0, 1),
            p_alive: true,
            c_pos: (H / 2, W - 1 - W / 4),
            c_dir: (0, -1),
            c_alive: true,
            grid: vec![0u8; (W as usize) * (H as usize)],
            p_wins: 0,
            c_wins: 0,
            rng: seed | 1,
        };
        g.mark(g.p_pos, 1);
        g.mark(g.c_pos, 2);
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    fn idx(&self, pos: (i16, i16)) -> usize {
        (pos.0 as usize) * (W as usize) + (pos.1 as usize)
    }

    fn mark(&mut self, pos: (i16, i16), who: u8) {
        let i = self.idx(pos);
        self.grid[i] = who;
    }

    /// A cell crashes a cycle when it is off the arena or already carries a
    /// trail (either cycle's own or the opponent's).
    fn blocked(&self, pos: (i16, i16)) -> bool {
        pos.0 < 0 || pos.0 >= H || pos.1 < 0 || pos.1 >= W || self.grid[self.idx(pos)] != 0
    }

    /// Count how many free cells lie straight ahead of `pos` along `dir` (up to a
    /// short look-ahead), used by the CPU to judge which turn has the most room.
    fn free_run(&self, pos: (i16, i16), dir: (i16, i16)) -> i32 {
        let mut p = pos;
        let mut n = 0;
        for _ in 0..6 {
            p = (p.0 + dir.0, p.1 + dir.1);
            if self.blocked(p) {
                break;
            }
            n += 1;
        }
        n
    }

    /// Queue a new player heading; a 180° reversal is ignored (you can't turn
    /// back into your own neck).
    pub fn turn(&mut self, dir: (i16, i16)) {
        if (dir.0 + self.p_dir.0, dir.1 + self.p_dir.1) != (0, 0) {
            self.p_dir = dir;
        }
    }

    /// The CPU's steering: keep going straight unless the cell ahead is blocked,
    /// otherwise turn to whichever open, non-reversing direction has the most
    /// free space ahead. The PRNG only breaks ties.
    fn cpu_think(&mut self) {
        let straight = (self.c_pos.0 + self.c_dir.0, self.c_pos.1 + self.c_dir.1);
        if !self.blocked(straight) {
            return;
        }
        let dirs = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        let mut best_dir = self.c_dir;
        let mut best_score = -1i32;
        for &d in dirs.iter() {
            if (d.0 + self.c_dir.0, d.1 + self.c_dir.1) == (0, 0) {
                continue; // no reversal
            }
            let n = (self.c_pos.0 + d.0, self.c_pos.1 + d.1);
            if self.blocked(n) {
                continue;
            }
            let run = self.free_run(self.c_pos, d);
            let jitter = (self.rand() % 2) as i32;
            let score = run * 2 + jitter;
            if score > best_score {
                best_score = score;
                best_dir = d;
            }
        }
        self.c_dir = best_dir;
    }

    /// Advance both cycles one cell simultaneously, laying trail and resolving
    /// crashes: only the player crashing → CPU wins; only the CPU → player wins;
    /// both on the same tick (including a head-on into the same cell) → draw.
    pub fn step(&mut self) {
        if !self.p_alive || !self.c_alive {
            return;
        }
        self.cpu_think();
        let p_next = (self.p_pos.0 + self.p_dir.0, self.p_pos.1 + self.p_dir.1);
        let c_next = (self.c_pos.0 + self.c_dir.0, self.c_pos.1 + self.c_dir.1);
        let head_on = p_next == c_next;
        let p_dead = head_on || self.blocked(p_next);
        let c_dead = head_on || self.blocked(c_next);

        if !p_dead {
            self.mark(p_next, 1);
            self.p_pos = p_next;
        }
        if !c_dead {
            self.mark(c_next, 2);
            self.c_pos = c_next;
        }

        if p_dead || c_dead {
            if p_dead && c_dead {
                // draw — no one scores
            } else if p_dead {
                self.c_wins += 1;
            } else {
                self.p_wins += 1;
            }
            self.p_alive = !p_dead;
            self.c_alive = !c_dead;
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Tron overlay.
pub struct Tron {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Tron {
    pub fn new() -> Self {
        Tron {
            game: Game::new(1),
            seed: 1,
            paused: false,
            last: None,
            interval: Duration::from_millis(90),
        }
    }

    /// A fresh round; the running win tally carries across rounds.
    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        let (pw, cw) = (self.game.p_wins, self.game.c_wins);
        self.game = Game::new(self.seed);
        self.game.p_wins = pw;
        self.game.c_wins = cw;
        self.paused = false;
        self.last = None;
    }

    /// Running = both cycles alive and not paused; only then do we keep the
    /// frame loop going for stepping.
    fn running(&self) -> bool {
        self.game.p_alive && self.game.c_alive && !self.paused
    }
}

impl Default for Tron {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Tron {
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
            key!(Left) | key!('h') => self.game.turn((0, -1)),
            key!(Right) | key!('l') => self.game.turn((0, 1)),
            key!(Up) | key!('k') => self.game.turn((-1, 0)),
            key!(Down) | key!('j') => self.game.turn((1, 0)),
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
        // Advance on wall-clock delta; when a round is over, hold the result for a
        // beat and then auto-start the next round. Redraw only while not paused.
        let now = Instant::now();
        if !self.paused {
            let over = !(self.game.p_alive && self.game.c_alive);
            let step_interval = if over {
                self.interval * 12
            } else {
                self.interval
            };
            match self.last {
                Some(t) if now.duration_since(t) >= step_interval => {
                    if over {
                        self.restart();
                    } else {
                        self.game.step();
                    }
                    self.last = Some(now);
                }
                None => self.last = Some(now),
                _ => {}
            }
            zmax_event::request_redraw();
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let wall_style = theme.get("ui.linenr");
        let p_head_style = theme.get("ui.text.focus");
        let p_style = theme.get("function");
        let c_style = theme.get("warning");

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
                "Tron    you {}  —  {} cpu",
                self.game.p_wins, self.game.c_wins
            ),
            header_style,
        );

        // Border.
        for c in -1..=W {
            surface.set_string(ox + (c + 1) as u16, oy - 1, "─", wall_style);
            surface.set_string(ox + (c + 1) as u16, oy + H as u16, "─", wall_style);
        }
        for r in 0..H {
            let y = oy + r as u16;
            surface.set_string(ox, y, "│", wall_style);
            surface.set_string(ox + (W + 1) as u16, y, "│", wall_style);
        }

        let cell = |r: i16, c: i16| (ox + (c + 1) as u16, oy + r as u16);
        // Trails.
        for r in 0..H {
            for c in 0..W {
                let v = self.game.grid[(r as usize) * (W as usize) + (c as usize)];
                if v != 0 {
                    let (x, y) = cell(r, c);
                    surface.set_string(x, y, "█", if v == 1 { p_style } else { c_style });
                }
            }
        }
        // Heads on top of their trail cells.
        let (px, py) = cell(self.game.p_pos.0, self.game.p_pos.1);
        surface.set_string(px, py, "◉", p_head_style);
        let (cx, cy) = cell(self.game.c_pos.0, self.game.c_pos.1);
        surface.set_string(cx, cy, "◉", c_style);

        let sy = oy + H as u16 + 1;
        let show_over = !(self.game.p_alive && self.game.c_alive);
        let status = if show_over {
            let who = if !self.game.p_alive && !self.game.c_alive {
                "draw"
            } else if !self.game.p_alive {
                "cpu wins"
            } else {
                "you win"
            };
            format!("Round over — {}.  n: new round  q: quit", who)
        } else if self.paused {
            "Paused — SPC resume · n new · q quit".to_string()
        } else {
            "arrows/hjkl steer · SPC pause · n new · q quit".to_string()
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_advances_one_cell_per_step() {
        let mut g = Game::new(1); // player heads right
        let (r, c) = g.p_pos;
        g.step();
        assert_eq!(
            g.p_pos,
            (r, c + 1),
            "the cycle moves one cell in its direction"
        );
        assert!(g.p_alive, "an open advance does not crash");
    }

    #[test]
    fn steering_turns_but_rejects_reversal() {
        let mut g = Game::new(1); // heading right
        g.turn((0, -1)); // straight back is illegal
        assert_eq!(g.p_dir, (0, 1), "a 180-degree turn must be rejected");
        g.turn((-1, 0)); // a legal turn
        assert_eq!(g.p_dir, (-1, 0));
    }

    #[test]
    fn running_into_a_wall_crashes() {
        let mut g = Game::new(1);
        g.p_pos = (0, W / 2);
        g.p_dir = (-1, 0); // straight off the top edge
        g.step();
        assert!(!g.p_alive, "the next cell is a wall — the cycle crashes");
    }

    #[test]
    fn running_into_a_trail_crashes() {
        let mut g = Game::new(1);
        let ahead = (g.p_pos.0, g.p_pos.1 + 1);
        g.mark(ahead, 2); // an opponent trail directly ahead
        g.step();
        assert!(!g.p_alive, "moving into an existing trail is fatal");
    }

    #[test]
    fn round_goes_to_the_survivor() {
        let mut g = Game::new(1);
        g.p_pos = (0, W / 2);
        g.p_dir = (-1, 0); // the player drives into the wall
        let before = g.c_wins;
        g.step();
        assert!(!g.p_alive, "the player crashed");
        assert!(g.c_alive, "the cpu is untouched");
        assert_eq!(g.c_wins, before + 1, "the survivor takes the round");
    }
}
