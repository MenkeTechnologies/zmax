//! Frogger — a zmax terminal game in the spirit of the classic arcade hop.
//!
//! Hop the frog from the bottom row up across the busy traffic lanes to the goal
//! banner at the top. Move with the arrows or `hjkl`, `SPC` pauses, `n` starts a
//! new game, `q`/`Esc` quits. Like `pong`/`snake` it animates itself with no
//! always-on timer: the traffic keeps rolling on wall-clock delta (even while the
//! frog sits still) and the next frame is scheduled via
//! `zmax_event::request_redraw` only while playing. The board logic is pure and
//! unit-tested.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 48;
const H: i16 = 20;
const START_ROW: i16 = H - 1;
const GOAL_ROW: i16 = 0;
const START_COL: i16 = W / 2;
const CAR_LEN: i16 = 2;
const LIVES: u32 = 3;

/// A single traffic lane: cars sit on a periodic pattern that shifts along `dir`
/// as `offset` advances, wrapping around horizontally.
#[derive(Clone, Copy)]
pub struct Lane {
    pub row: i16,
    /// `+1` = cars flow right, `-1` = cars flow left.
    pub dir: i16,
    /// Ticks between moves — smaller is faster.
    pub speed: u32,
    /// Gap period between successive cars.
    pub spacing: i16,
    /// Current horizontal shift of the pattern.
    pub offset: i16,
}

impl Lane {
    /// Whether a car currently occupies `col` in this lane.
    pub fn has_car(&self, col: i16) -> bool {
        (col - self.offset).rem_euclid(self.spacing.max(1)) < CAR_LEN
    }
}

/// The pure frogger board. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub frog: (i16, i16),
    pub lanes: Vec<Lane>,
    pub score: u32,
    pub lives: u32,
    pub over: bool,
    ticks: u64,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            frog: (START_ROW, START_COL),
            lanes: Vec::new(),
            score: 0,
            lives: LIVES,
            over: false,
            ticks: 0,
            rng: seed | 1,
        };
        g.build_lanes();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Populate the interior lanes with randomized direction, speed, spacing and
    /// starting offset. Lanes sit on the even interior rows so the odd rows form
    /// safe medians the frog can rest on.
    fn build_lanes(&mut self) {
        self.lanes.clear();
        let mut idx = 0u16;
        let mut row = 2;
        while row <= H - 3 {
            let dir = if idx.is_multiple_of(2) { 1 } else { -1 };
            let speed = 1 + (self.rand() % 3) as u32; // 1..=3
            let spacing = 4 + (self.rand() % 4) as i16; // 4..=7
            let offset = (self.rand() % spacing as u64) as i16;
            self.lanes.push(Lane {
                row,
                dir,
                speed,
                spacing,
                offset,
            });
            idx += 1;
            row += 2;
        }
    }

    /// Whether any lane has a car on `(row, col)`.
    pub fn car_at(&self, row: i16, col: i16) -> bool {
        self.lanes.iter().any(|l| l.row == row && l.has_car(col))
    }

    fn lose_life(&mut self) {
        self.lives = self.lives.saturating_sub(1);
        self.frog = (START_ROW, START_COL);
        if self.lives == 0 {
            self.over = true;
        }
    }

    /// One traffic step: advance every lane that is due this tick, then squash the
    /// frog if a car has rolled onto its cell.
    pub fn step(&mut self) {
        if self.over {
            return;
        }
        self.ticks = self.ticks.wrapping_add(1);
        for lane in &mut self.lanes {
            if self.ticks.is_multiple_of(lane.speed as u64) {
                lane.offset += lane.dir;
            }
        }
        if self.car_at(self.frog.0, self.frog.1) {
            self.lose_life();
        }
    }

    /// Hop the frog by `(dr, dc)`, clamped to the board. Reaching the goal row
    /// scores and sends the frog back to the start; hopping straight into a car
    /// costs a life.
    pub fn move_frog(&mut self, dr: i16, dc: i16) {
        if self.over {
            return;
        }
        let nr = (self.frog.0 + dr).clamp(0, H - 1);
        let nc = (self.frog.1 + dc).clamp(0, W - 1);
        self.frog = (nr, nc);
        if self.frog.0 == GOAL_ROW {
            self.score += 1;
            self.frog = (START_ROW, START_COL);
            return;
        }
        if self.car_at(self.frog.0, self.frog.1) {
            self.lose_life();
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Frogger overlay.
pub struct Frogger {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Frogger {
    pub fn new() -> Self {
        Frogger {
            game: Game::new(1),
            seed: 1,
            paused: false,
            last: None,
            interval: Duration::from_millis(140),
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
        !self.game.over && !self.paused
    }
}

impl Default for Frogger {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Frogger {
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
            key!(Up) | key!('k') => self.game.move_frog(-1, 0),
            key!(Down) | key!('j') => self.game.move_frog(1, 0),
            key!(Left) | key!('h') => self.game.move_frog(0, -1),
            key!(Right) | key!('l') => self.game.move_frog(0, 1),
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
        let goal_style = theme.get("function");
        let frog_style = theme.get("warning");
        let car_styles = [
            theme.get("error"),
            theme.get("function"),
            theme.get("ui.text.focus"),
            theme.get("ui.selection"),
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
                "Frogger  score {}  lives {}",
                self.game.score, self.game.lives
            ),
            header_style,
        );

        // Top/bottom border.
        for c in 0..W {
            surface.set_string(ox + c as u16, oy - 1, "─", wall_style);
            surface.set_string(ox + c as u16, oy + H as u16, "─", wall_style);
        }

        let cell = |r: i16, c: i16| (ox + c as u16, oy + r as u16);

        // Goal banner across the top row.
        for c in 0..W {
            let (gx, gy) = cell(GOAL_ROW, c);
            surface.set_string(gx, gy, "─", goal_style);
        }
        surface.set_string(ox + (W / 2 - 2) as u16, oy, "GOAL", goal_style);

        // Traffic.
        for (i, lane) in self.game.lanes.iter().enumerate() {
            let style = car_styles[i % car_styles.len()];
            for c in 0..W {
                if lane.has_car(c) {
                    let (cx, cy) = cell(lane.row, c);
                    surface.set_string(cx, cy, "▣", style);
                }
            }
        }

        // The frog on top of it all.
        let (fx, fy) = cell(self.game.frog.0, self.game.frog.1);
        surface.set_string(fx, fy, "@", frog_style);

        let sy = oy + H as u16 + 1;
        let status = if self.game.over {
            format!(
                "Game over — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.paused {
            format!(
                "Paused — score {}.  SPC resume  n new  q quit",
                self.game.score
            )
        } else {
            "arrows/hjkl hop · SPC pause · n new · q quit".to_string()
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frog_hops_up_one_row() {
        let mut g = Game::new(1);
        let (r, c) = g.frog; // starts on the safe bottom row
        g.move_frog(-1, 0);
        assert_eq!(
            g.frog,
            (r - 1, c),
            "up moves the frog one row toward the goal"
        );
    }

    #[test]
    fn car_collision_costs_a_life_and_resets_the_frog() {
        let mut g = Game::new(1);
        // Park a slow car right under the frog so the next tick squashes it.
        g.lanes[0].offset = 0; // car pattern starts at col 0
        g.lanes[0].speed = 100; // will not advance on tick 1
        g.frog = (g.lanes[0].row, 0);
        let lives = g.lives;
        g.step();
        assert_eq!(g.lives, lives - 1, "a car on the frog's cell costs a life");
        assert_eq!(
            g.frog,
            (START_ROW, START_COL),
            "the frog respawns at the start"
        );
    }

    #[test]
    fn reaching_the_goal_scores_and_resets() {
        let mut g = Game::new(1);
        g.frog = (GOAL_ROW + 1, START_COL); // one hop below the goal, a safe row
        g.move_frog(-1, 0);
        assert_eq!(g.score, 1, "reaching the top row scores");
        assert_eq!(
            g.frog,
            (START_ROW, START_COL),
            "the frog restarts from the bottom"
        );
    }

    #[test]
    fn frog_stays_within_the_board() {
        let mut g = Game::new(1);
        // Stay on the safe start row so horizontal hops never hit traffic.
        for _ in 0..80 {
            g.move_frog(0, -1);
        }
        assert_eq!(g.frog.1, 0, "clamps to the left edge");
        for _ in 0..80 {
            g.move_frog(0, 1);
        }
        assert_eq!(g.frog.1, W - 1, "clamps to the right edge");
        assert_eq!(g.frog.0, START_ROW, "row is unchanged by horizontal hops");
    }

    #[test]
    fn traffic_advances_on_each_step() {
        let mut g = Game::new(1);
        g.frog = (H - 1, START_COL); // safe row, out of harm's way
        for lane in &mut g.lanes {
            lane.speed = 1; // every lane moves each tick
        }
        let before: Vec<i16> = g.lanes.iter().map(|l| l.offset).collect();
        g.step();
        let after: Vec<i16> = g.lanes.iter().map(|l| l.offset).collect();
        assert_ne!(before, after, "every lane shifts its cars on a step");
    }
}
