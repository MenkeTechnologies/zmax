//! Space Invaders — a zemacs take on the arcade classic.
//!
//! Defend the bottom row with your cannon: slide left/right with the arrows or
//! `h`/`l`, fire an upward shot with `f` or `↑`, `SPC` pauses, `n` starts a new
//! wave, `q`/`Esc` quits. A block of aliens marches side to side, dropping one
//! row and reversing whenever it kisses a wall, and rains bombs on you. Clear the
//! fleet to win; lose if the aliens reach your row or the bombs take your last
//! life. Like the other action games it animates itself via
//! `zemacs_event::request_redraw` only while playing. The fleet/bullet/bomb logic
//! is pure and unit-tested.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 44;
const H: i16 = 22;
const COLS: usize = 6;
const ROWS: usize = 4;
/// Horizontal spacing between alien columns.
const ALIEN_GAP: i16 = 3;
/// Ticks between successive fleet marches (a lower cadence = a faster fleet).
const FLEET_CADENCE: u32 = 6;
/// How many player bullets may be in flight at once.
const MAX_BULLETS: usize = 2;
/// 1-in-N chance per tick that the fleet drops a bomb.
const BOMB_CHANCE: u64 = 8;
/// Row the cannon (and any alien that reaches it) sits on.
const CANNON_ROW: i16 = H - 1;

/// How a round ended (or that it is still going).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Playing,
    Won,
    Lost,
}

/// The pure invaders court. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    /// Cannon column on the bottom row.
    pub cannon: i16,
    /// Which aliens in the `ROWS`×`COLS` block are still alive.
    pub alive: [[bool; COLS]; ROWS],
    /// Column of the leftmost alien column …
    pub fleet_x: i16,
    /// … and the row of the top alien row.
    pub fleet_y: i16,
    /// Current march direction: `1` right, `-1` left.
    pub dir: i16,
    /// Ticks left before the fleet marches again.
    pub move_counter: u32,
    /// Player shots travelling up, `(row, col)`.
    pub bullets: Vec<(i16, i16)>,
    /// Alien bombs travelling down, `(row, col)`.
    pub bombs: Vec<(i16, i16)>,
    pub score: u32,
    pub lives: u32,
    pub status: Status,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Game {
            cannon: W / 2,
            alive: [[true; COLS]; ROWS],
            fleet_x: 2,
            fleet_y: 1,
            dir: 1,
            move_counter: FLEET_CADENCE,
            bullets: Vec::new(),
            bombs: Vec::new(),
            score: 0,
            lives: 3,
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

    /// Board cell of the alien at grid position `(row, col)`.
    fn alien_cell(&self, row: usize, col: usize) -> (i16, i16) {
        (
            self.fleet_y + row as i16,
            self.fleet_x + col as i16 * ALIEN_GAP,
        )
    }

    /// Slide the cannon by `d`, kept inside the court.
    pub fn move_cannon(&mut self, d: i16) {
        self.cannon = (self.cannon + d).clamp(1, W - 2);
    }

    /// Fire an upward bullet if we are under the in-flight cap and still playing.
    pub fn fire(&mut self) {
        if self.status == Status::Playing && self.bullets.len() < MAX_BULLETS {
            self.bullets.push((CANNON_ROW - 1, self.cannon));
        }
    }

    /// March the block one column, or reverse-and-descend on hitting a wall.
    fn move_fleet(&mut self) {
        let mut min_x = i16::MAX;
        let mut max_x = i16::MIN;
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.alive[row][col] {
                    let x = self.alien_cell(row, col).1;
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                }
            }
        }
        if min_x == i16::MAX {
            return; // empty fleet — nothing to march
        }
        if max_x + self.dir > W - 2 || min_x + self.dir < 1 {
            self.dir = -self.dir;
            self.fleet_y += 1;
        } else {
            self.fleet_x += self.dir;
        }
    }

    /// Kill the alien occupying `(r, c)`, if any; returns whether one was hit.
    fn hit_alien(&mut self, r: i16, c: i16) -> bool {
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.alive[row][col] && self.alien_cell(row, col) == (r, c) {
                    self.alive[row][col] = false;
                    return true;
                }
            }
        }
        false
    }

    /// Move every bullet up a row, scoring and vanishing on an alien hit.
    fn advance_bullets(&mut self) {
        let mut kept = Vec::with_capacity(self.bullets.len());
        for (r, c) in std::mem::take(&mut self.bullets) {
            let r = r - 1;
            if r < 0 {
                continue; // off the top of the court
            }
            if self.hit_alien(r, c) {
                self.score += 10;
                continue; // bullet is spent on the hit
            }
            kept.push((r, c));
        }
        self.bullets = kept;
    }

    /// Move every bomb down a row; one landing on the cannon costs a life.
    fn advance_bombs(&mut self) {
        let mut kept = Vec::with_capacity(self.bombs.len());
        for (r, c) in std::mem::take(&mut self.bombs) {
            let r = r + 1;
            if r >= H {
                continue; // fell off the court
            }
            if r == CANNON_ROW && c == self.cannon {
                self.lives = self.lives.saturating_sub(1);
                continue;
            }
            kept.push((r, c));
        }
        self.bombs = kept;
    }

    /// With `1/BOMB_CHANCE` odds, drop a bomb from the bottom alien of a random
    /// column.
    fn maybe_drop_bomb(&mut self) {
        if self.rand() % BOMB_CHANCE != 0 {
            return;
        }
        let col = (self.rand() % COLS as u64) as usize;
        for row in (0..ROWS).rev() {
            if self.alive[row][col] {
                let (r, c) = self.alien_cell(row, col);
                self.bombs.push((r + 1, c));
                return;
            }
        }
    }

    /// Decide whether the round is over: out of lives or overrun → loss, an empty
    /// fleet → win.
    fn check_end(&mut self) {
        if self.lives == 0 {
            self.status = Status::Lost;
            return;
        }
        let mut any = false;
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.alive[row][col] {
                    any = true;
                    if self.fleet_y + row as i16 >= CANNON_ROW {
                        self.status = Status::Lost;
                        return;
                    }
                }
            }
        }
        if !any {
            self.status = Status::Won;
        }
    }

    /// Advance one tick: march the fleet on its cadence, move shots and bombs,
    /// resolve collisions, spawn a bomb, then test for a win or loss.
    pub fn step(&mut self) {
        if self.status != Status::Playing {
            return;
        }
        if self.move_counter == 0 {
            self.move_fleet();
            self.move_counter = FLEET_CADENCE;
        } else {
            self.move_counter -= 1;
        }
        self.advance_bullets();
        self.advance_bombs();
        self.maybe_drop_bomb();
        self.check_end();
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Space Invaders overlay.
pub struct Invaders {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Invaders {
    pub fn new() -> Self {
        Invaders {
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

impl Default for Invaders {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Invaders {
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
            key!(Left) | key!('h') => self.game.move_cannon(-1),
            key!(Right) | key!('l') => self.game.move_cannon(1),
            key!('f') | key!(Up) => self.game.fire(),
            key!(' ') => self.paused = !self.paused,
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
            zemacs_event::request_redraw();
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let wall_style = theme.get("ui.linenr");
        let alien_style = theme.get("function");
        let cannon_style = theme.get("ui.text.focus");
        let bullet_style = theme.get("warning");
        let bomb_style = theme.get("error");

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
                "Space Invaders  score {}  lives {}",
                self.game.score, self.game.lives
            ),
            header_style,
        );

        // Top and bottom court walls.
        for c in 0..W {
            surface.set_string(ox + c as u16, oy - 1, "─", wall_style);
            surface.set_string(ox + c as u16, oy + H as u16, "─", wall_style);
        }
        let cell = |r: i16, c: i16| (ox + c as u16, oy + r as u16);

        // The marching fleet.
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.game.alive[row][col] {
                    let (r, c) = self.game.alien_cell(row, col);
                    let (x, y) = cell(r, c);
                    surface.set_string(x, y, "▚", alien_style);
                }
            }
        }
        // Bombs, then bullets (bullets win a shared cell so shots read clearly).
        for &(r, c) in &self.game.bombs {
            let (x, y) = cell(r, c);
            surface.set_string(x, y, "*", bomb_style);
        }
        for &(r, c) in &self.game.bullets {
            let (x, y) = cell(r, c);
            surface.set_string(x, y, "|", bullet_style);
        }
        // The cannon on the bottom row.
        let (cx, cy) = cell(CANNON_ROW, self.game.cannon);
        surface.set_string(cx, cy, "▲", cannon_style);

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
                    "Fleet cleared! — score {}.  n: next wave  q: quit",
                    self.game.score
                )
            }
            Status::Playing if self.paused => "Paused — SPC resume · n new · q quit".to_string(),
            Status::Playing => "←/→ move · f/↑ fire · SPC pause · n new · q quit".to_string(),
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bullet_destroys_an_alien_and_scores() {
        let mut g = Game::new(1);
        // Alien (0,0) sits at (fleet_y, fleet_x) = (1, 2); park a bullet just below
        // it so this step lifts it onto the alien.
        g.bullets = vec![(2, 2)];
        g.move_counter = 5; // keep the fleet still this tick
        let before = g.score;
        g.step();
        assert!(!g.alive[0][0], "the alien is removed");
        assert_eq!(g.score, before + 10, "and the hit is scored");
    }

    #[test]
    fn the_cannon_stays_on_court() {
        let mut g = Game::new(1);
        for _ in 0..100 {
            g.move_cannon(-1);
        }
        assert_eq!(g.cannon, 1);
        for _ in 0..100 {
            g.move_cannon(1);
        }
        assert_eq!(g.cannon, W - 2);
    }

    #[test]
    fn the_fleet_reverses_and_descends_at_a_wall() {
        let mut g = Game::new(1);
        g.dir = 1;
        // Push the rightmost column (col 5) exactly onto the right wall.
        g.fleet_x = W - 2 - 5 * ALIEN_GAP;
        let y0 = g.fleet_y;
        g.move_counter = 0; // force a march this tick
        g.step();
        assert_eq!(g.dir, -1, "direction flips at the wall");
        assert_eq!(g.fleet_y, y0 + 1, "and the block drops a row");
    }

    #[test]
    fn a_bomb_on_the_cannon_costs_a_life() {
        let mut g = Game::new(1);
        g.cannon = 10;
        g.bombs = vec![(CANNON_ROW - 1, 10)];
        g.move_counter = 5;
        g.step();
        assert_eq!(g.lives, 2, "a bomb landing on the cannon costs one life");
        assert!(g.bombs.is_empty(), "and the bomb is consumed");
    }

    #[test]
    fn clearing_the_fleet_wins() {
        let mut g = Game::new(1);
        for row in 0..ROWS {
            for col in 0..COLS {
                g.alive[row][col] = false;
            }
        }
        g.move_counter = 5;
        g.step();
        assert_eq!(g.status, Status::Won);
    }
}
