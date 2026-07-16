//! Galaga — a zmax take on the swarming-formation shooter.
//!
//! Fly your ship along the bottom row: slide left/right with the arrows or
//! `h`/`l`, fire an upward shot with `SPC` or `↑`, `p` pauses, `n` starts a new
//! wave, `q`/`Esc` quits. Enemies hold a swaying formation near the top; every
//! so often one peels off and dives at your column before either escaping off
//! the bottom or is shot down for bonus points, raining the odd shot as it goes.
//! Clear the formation to advance the wave; lose if a diver rams you or an enemy
//! shot takes your last life. Like the other action games it animates itself via
//! `zmax_event::request_redraw` only while playing. The formation/diver/bullet
//! logic is pure and unit-tested (it uses the same LCG PRNG as the snake port).

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 44;
const H: i16 = 22;
const COLS: usize = 8;
const ROWS: usize = 3;
/// Horizontal spacing between formation columns.
const ENEMY_GAP: i16 = 4;
/// Column of the leftmost formation column at zero sway.
const BASE_X: i16 = 4;
/// Row of the top formation row.
const FORMATION_TOP: i16 = 1;
/// Row the ship (and any diver that reaches it) sits on.
const SHIP_ROW: i16 = H - 1;
/// How many player bullets may be in flight at once.
const MAX_BULLETS: usize = 3;
/// Ticks between successive formation sway steps.
const SWAY_CADENCE: u32 = 4;
/// How far the formation drifts to either side.
const SWAY_MAX: i16 = 3;
/// 1-in-N chance per tick that an enemy peels off to dive.
const DIVE_CHANCE: u64 = 12;
/// 1-in-N chance per tick that a diver drops a shot.
const DIVE_BOMB_CHANCE: u64 = 6;

/// How a round ended (or that it is still going).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Playing,
    Won,
    Lost,
}

/// An enemy that has peeled out of the formation and is diving, `(row, col)` on
/// the board, aiming its column at `target_x`.
#[derive(Clone)]
pub struct Diver {
    pub pos: (i16, i16),
    pub target_x: i16,
}

/// The pure galaga court. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    /// Ship column on the bottom row.
    pub ship: i16,
    /// Which enemies in the `ROWS`×`COLS` formation are still holding station.
    pub formation: [[bool; COLS]; ROWS],
    /// Current horizontal drift of the whole formation …
    pub sway_x: i16,
    /// … and its direction: `1` right, `-1` left.
    pub sway_dir: i16,
    /// Ticks left before the formation sways again.
    pub sway_counter: u32,
    /// Enemies currently diving at the ship.
    pub divers: Vec<Diver>,
    /// Player shots travelling up, `(row, col)`.
    pub bullets: Vec<(i16, i16)>,
    /// Enemy shots travelling down, `(row, col)`.
    pub enemy_bullets: Vec<(i16, i16)>,
    pub score: u32,
    pub lives: u32,
    pub wave: u32,
    pub status: Status,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Game {
            ship: W / 2,
            formation: [[true; COLS]; ROWS],
            sway_x: 0,
            sway_dir: 1,
            sway_counter: SWAY_CADENCE,
            divers: Vec::new(),
            bullets: Vec::new(),
            enemy_bullets: Vec::new(),
            score: 0,
            lives: 3,
            wave: 1,
            status: Status::Playing,
            rng: seed | 1,
        }
    }

    /// The same LCG PRNG the snake port uses.
    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Board cell of the formation enemy at grid position `(row, col)`.
    fn enemy_cell(&self, row: usize, col: usize) -> (i16, i16) {
        (
            FORMATION_TOP + row as i16,
            BASE_X + col as i16 * ENEMY_GAP + self.sway_x,
        )
    }

    /// Slide the ship by `d`, kept inside the court.
    pub fn move_ship(&mut self, d: i16) {
        self.ship = (self.ship + d).clamp(1, W - 2);
    }

    /// Fire an upward bullet if we are under the in-flight cap and still playing.
    pub fn fire(&mut self) {
        if self.status == Status::Playing && self.bullets.len() < MAX_BULLETS {
            self.bullets.push((SHIP_ROW - 1, self.ship));
        }
    }

    /// Drift the whole formation one step on its cadence, reversing at the edges.
    fn sway(&mut self) {
        if self.sway_counter == 0 {
            let next = self.sway_x + self.sway_dir;
            if !(-SWAY_MAX..=SWAY_MAX).contains(&next) {
                self.sway_dir = -self.sway_dir;
            } else {
                self.sway_x = next;
            }
            self.sway_counter = SWAY_CADENCE;
        } else {
            self.sway_counter -= 1;
        }
    }

    /// With `1/DIVE_CHANCE` odds, peel a random formation enemy off into a dive
    /// aimed at the ship's column.
    fn maybe_spawn_diver(&mut self) {
        if !self.rand().is_multiple_of(DIVE_CHANCE) {
            return;
        }
        let mut alive = Vec::new();
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.formation[row][col] {
                    alive.push((row, col));
                }
            }
        }
        if alive.is_empty() {
            return;
        }
        let (row, col) = alive[(self.rand() % alive.len() as u64) as usize];
        self.formation[row][col] = false;
        let pos = self.enemy_cell(row, col);
        let target_x = self.ship;
        self.divers.push(Diver { pos, target_x });
    }

    /// Advance every diver one row down and one column toward its target; a diver
    /// reaching the ship costs a life, and one falling off the bottom escapes.
    fn advance_divers(&mut self) {
        let ship = self.ship;
        let mut kept = Vec::with_capacity(self.divers.len());
        for mut d in std::mem::take(&mut self.divers) {
            d.pos.0 += 1;
            if d.pos.1 < d.target_x {
                d.pos.1 += 1;
            } else if d.pos.1 > d.target_x {
                d.pos.1 -= 1;
            }
            if self.rand().is_multiple_of(DIVE_BOMB_CHANCE) {
                self.enemy_bullets.push((d.pos.0 + 1, d.pos.1));
            }
            if d.pos.0 >= H {
                continue; // escaped off the bottom
            }
            if d.pos.0 == SHIP_ROW && d.pos.1 == ship {
                self.lives = self.lives.saturating_sub(1);
                continue; // rammed the ship
            }
            kept.push(d);
        }
        self.divers = kept;
    }

    /// Remove the diving enemy at `(r, c)`, if any; returns whether one was hit.
    fn hit_diver(&mut self, r: i16, c: i16) -> bool {
        if let Some(i) = self.divers.iter().position(|d| d.pos == (r, c)) {
            self.divers.remove(i);
            return true;
        }
        false
    }

    /// Remove the formation enemy occupying `(r, c)`, if any; returns the hit.
    fn hit_enemy(&mut self, r: i16, c: i16) -> bool {
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.formation[row][col] && self.enemy_cell(row, col) == (r, c) {
                    self.formation[row][col] = false;
                    return true;
                }
            }
        }
        false
    }

    /// Move every player bullet up a row, scoring and vanishing on a hit; a diver
    /// is worth more than an enemy sitting in formation.
    fn advance_bullets(&mut self) {
        let mut kept = Vec::with_capacity(self.bullets.len());
        for (r, c) in std::mem::take(&mut self.bullets) {
            let r = r - 1;
            if r < 0 {
                continue; // off the top of the court
            }
            if self.hit_diver(r, c) {
                self.score += 30;
                continue;
            }
            if self.hit_enemy(r, c) {
                self.score += 10;
                continue;
            }
            kept.push((r, c));
        }
        self.bullets = kept;
    }

    /// Move every enemy shot down a row; one landing on the ship costs a life.
    fn advance_enemy_bullets(&mut self) {
        let ship = self.ship;
        let mut kept = Vec::with_capacity(self.enemy_bullets.len());
        for (r, c) in std::mem::take(&mut self.enemy_bullets) {
            let r = r + 1;
            if r >= H {
                continue; // fell off the court
            }
            if r == SHIP_ROW && c == ship {
                self.lives = self.lives.saturating_sub(1);
                continue;
            }
            kept.push((r, c));
        }
        self.enemy_bullets = kept;
    }

    /// Decide whether the round is over: out of lives → loss, an empty formation
    /// with no divers left → the wave is cleared and the next one begins.
    fn check_end(&mut self) {
        if self.lives == 0 {
            self.status = Status::Lost;
            return;
        }
        let any_formation = self.formation.iter().flatten().any(|&a| a);
        if !any_formation && self.divers.is_empty() {
            self.wave += 1;
            self.status = Status::Won;
        }
    }

    /// Advance one tick: sway the formation, spawn/advance divers, move shots,
    /// resolve collisions, then test for a win or loss.
    pub fn step(&mut self) {
        if self.status != Status::Playing {
            return;
        }
        self.sway();
        self.maybe_spawn_diver();
        self.advance_divers();
        self.advance_bullets();
        self.advance_enemy_bullets();
        self.check_end();
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Galaga overlay.
pub struct Galaga {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Galaga {
    pub fn new() -> Self {
        Galaga {
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

    /// Running = still playing and not paused; only then does the frame loop run.
    fn running(&self) -> bool {
        self.game.status == Status::Playing && !self.paused
    }
}

impl Default for Galaga {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Galaga {
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
            key!(Left) | key!('h') => self.game.move_ship(-1),
            key!(Right) | key!('l') => self.game.move_ship(1),
            key!(' ') | key!(Up) => self.game.fire(),
            key!('p') => self.paused = !self.paused,
            key!('n') => self.restart(),
            _ => {}
        }
        if self.running() {
            if self.last.is_none() {
                self.last = Some(Instant::now());
            }
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
            zmax_event::request_redraw();
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let wall_style = theme.get("ui.linenr");
        let enemy_style = theme.get("error");
        let diver_style = theme.get("ui.text.focus");
        let ship_style = theme.get("function");
        let bullet_style = theme.get("warning");
        let ebullet_style = theme.get("error");

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
                "Galaga  score {}  lives {}  wave {}",
                self.game.score, self.game.lives, self.game.wave
            ),
            header_style,
        );

        // Top and bottom court walls.
        for c in 0..W {
            surface.set_string(ox + c as u16, oy - 1, "─", wall_style);
            surface.set_string(ox + c as u16, oy + H as u16, "─", wall_style);
        }
        let cell = |r: i16, c: i16| (ox + c as u16, oy + r as u16);
        let on_board = |r: i16, c: i16| (0..H).contains(&r) && (0..W).contains(&c);

        // The swaying formation.
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.game.formation[row][col] {
                    let (r, c) = self.game.enemy_cell(row, col);
                    if on_board(r, c) {
                        let (x, y) = cell(r, c);
                        surface.set_string(x, y, "ᴥ", enemy_style);
                    }
                }
            }
        }
        // Divers peeling toward the ship.
        for d in &self.game.divers {
            let (r, c) = d.pos;
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, "W", diver_style);
            }
        }
        // Enemy shots, then player shots (player shots win a shared cell).
        for &(r, c) in &self.game.enemy_bullets {
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, "!", ebullet_style);
            }
        }
        for &(r, c) in &self.game.bullets {
            if on_board(r, c) {
                let (x, y) = cell(r, c);
                surface.set_string(x, y, "|", bullet_style);
            }
        }
        // The ship on the bottom row.
        let (sx, sy0) = cell(SHIP_ROW, self.game.ship);
        surface.set_string(sx, sy0, "▲", ship_style);

        let sy = oy + H as u16 + 1;
        let status = match self.game.status {
            Status::Lost => {
                format!(
                    "Game over — score {}.  n: new game  q: quit",
                    self.game.score
                )
            }
            Status::Won => {
                format!(
                    "Formation cleared! — score {}.  n: next wave  q: quit",
                    self.game.score
                )
            }
            Status::Playing if self.paused => "Paused — p resume · n new · q quit".to_string(),
            Status::Playing => "←/→ move · SPC/↑ fire · p pause · n new · q quit".to_string(),
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bullet_destroys_an_enemy_and_scores() {
        let mut g = Game::new(1);
        // Enemy (0,0) sits at (FORMATION_TOP, BASE_X) with no sway yet; park a
        // bullet one row below it and lift it with a bare, PRNG-free advance.
        g.bullets = vec![(FORMATION_TOP + 1, BASE_X)];
        let before = g.score;
        g.advance_bullets();
        assert!(!g.formation[0][0], "the enemy is removed");
        assert_eq!(g.score, before + 10, "and the hit is scored");
    }

    #[test]
    fn a_diver_descends_one_row_per_step() {
        let mut g = Game::new(1);
        let x = g.ship;
        g.divers = vec![Diver {
            pos: (5, x),
            target_x: x,
        }];
        g.step();
        assert_eq!(g.divers[0].pos.0, 6, "the diver drops exactly one row");
    }

    #[test]
    fn a_diver_reaching_the_ship_costs_a_life() {
        let mut g = Game::new(1);
        let x = g.ship;
        g.divers = vec![Diver {
            pos: (SHIP_ROW - 1, x),
            target_x: x,
        }];
        let before = g.lives;
        g.step();
        assert_eq!(g.lives, before - 1, "ramming the ship costs a life");
    }

    #[test]
    fn an_enemy_bullet_hitting_the_ship_costs_a_life() {
        let mut g = Game::new(1);
        let x = g.ship;
        g.enemy_bullets = vec![(SHIP_ROW - 1, x)];
        let before = g.lives;
        g.step();
        assert_eq!(
            g.lives,
            before - 1,
            "an enemy shot on the ship costs a life"
        );
    }

    #[test]
    fn clearing_the_formation_advances_the_wave() {
        let mut g = Game::new(1);
        for row in 0..ROWS {
            for col in 0..COLS {
                g.formation[row][col] = false;
            }
        }
        let wave = g.wave;
        g.step();
        assert_eq!(g.status, Status::Won);
        assert_eq!(
            g.wave,
            wave + 1,
            "the wave advances when the formation is cleared"
        );
    }
}
