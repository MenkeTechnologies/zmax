//! Pac-Man — a small maze chase in the spirit of GNU Emacs' action games.
//!
//! Munch every pellet while dodging the ghosts; grab a power pellet to turn the
//! tables and eat them. Move with the arrows or `hjkl`, `SPC` pauses, `n`
//! restarts, `q`/`Esc` quits. Like `pong` and `snake` it animates itself with no
//! always-on timer: each frame advances on wall-clock delta and schedules the
//! next via `zemacs_event::request_redraw` only while running, so it idles when
//! paused, won, dead or closed. The maze logic is pure and unit-tested.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: i16 = 19;
const H: i16 = 13;
/// Ticks a power pellet keeps the ghosts frightened.
const FRIGHT: u32 = 30;
const LIVES: u32 = 3;

/// The embedded maze: `#` wall, `.` pellet, `o` power pellet, `P` pac start,
/// `G` ghost start, ` ` empty. Symmetric and fully connected.
const MAZE: [&str; H as usize] = [
    "###################",
    "#o...............o#",
    "#.#.#.#.#.#.#.#.#.#",
    "#.................#",
    "#.#.#.#.#.#.#.#.#.#",
    "#.................#",
    "#.#.#.#G#G#G#.#.#.#",
    "#.................#",
    "#.#.#.#.#.#.#.#.#.#",
    "#.................#",
    "#.#.#.#.#.#.#.#.#.#",
    "#o.......P.......o#",
    "###################",
];

/// A single ghost: where it is, which way it is heading, and where it respawns.
#[derive(Clone)]
pub struct Ghost {
    pub pos: (i16, i16),
    dir: (i16, i16),
    start: (i16, i16),
}

/// The pure Pac-Man maze. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    walls: Vec<(i16, i16)>,
    pub pellets: Vec<(i16, i16)>,
    pub power: Vec<(i16, i16)>,
    pub pac: (i16, i16),
    pac_start: (i16, i16),
    pub pac_dir: (i16, i16),
    want_dir: (i16, i16),
    pub ghosts: Vec<Ghost>,
    pub score: u32,
    pub lives: u32,
    pub frightened: u32,
    pub over: bool,
    pub won: bool,
    rng: u64,
}

fn manhattan(a: (i16, i16), b: (i16, i16)) -> i16 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

/// Remove `cell` from `v` if present, reporting whether it was there.
fn eat(v: &mut Vec<(i16, i16)>, cell: (i16, i16)) -> bool {
    if let Some(i) = v.iter().position(|&c| c == cell) {
        v.swap_remove(i);
        true
    } else {
        false
    }
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut walls = Vec::new();
        let mut pellets = Vec::new();
        let mut power = Vec::new();
        let mut pac = (0, 0);
        let mut ghosts = Vec::new();
        for (r, row) in MAZE.iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                let cell = (r as i16, c as i16);
                match ch {
                    '#' => walls.push(cell),
                    '.' => pellets.push(cell),
                    'o' => power.push(cell),
                    'P' => pac = cell,
                    'G' => ghosts.push(Ghost {
                        pos: cell,
                        dir: (0, 0),
                        start: cell,
                    }),
                    _ => {}
                }
            }
        }
        Game {
            walls,
            pellets,
            power,
            pac,
            pac_start: pac,
            pac_dir: (0, 0),
            want_dir: (0, 0),
            ghosts,
            score: 0,
            lives: LIVES,
            frightened: 0,
            over: false,
            won: false,
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

    fn is_wall(&self, cell: (i16, i16)) -> bool {
        self.walls.contains(&cell)
    }

    /// Buffer a desired heading; it is applied on the next tick that its cell is
    /// not a wall.
    pub fn steer(&mut self, dir: (i16, i16)) {
        self.want_dir = dir;
    }

    fn reset_positions(&mut self) {
        self.pac = self.pac_start;
        self.pac_dir = (0, 0);
        self.want_dir = (0, 0);
        self.frightened = 0;
        for g in self.ghosts.iter_mut() {
            g.pos = g.start;
            g.dir = (0, 0);
        }
    }

    fn lose_life(&mut self) {
        self.lives = self.lives.saturating_sub(1);
        if self.lives == 0 {
            self.over = true;
        } else {
            self.reset_positions();
        }
    }

    /// If a ghost shares pac's cell: eat it while frightened (+200, respawn),
    /// otherwise pac loses a life and everything resets.
    fn check_collisions(&mut self) {
        for i in 0..self.ghosts.len() {
            if self.ghosts[i].pos == self.pac {
                if self.frightened > 0 {
                    self.score += 200;
                    self.ghosts[i].pos = self.ghosts[i].start;
                    self.ghosts[i].dir = (0, 0);
                } else {
                    self.lose_life();
                    return;
                }
            }
        }
    }

    /// Corridor AI: step to the non-wall, non-reversing neighbour that minimises
    /// Manhattan distance to pac (chase); when frightened, maximise it (flee) or
    /// wander via the PRNG.
    fn move_ghosts(&mut self) {
        let pac = self.pac;
        let fright = self.frightened > 0;
        let dirs = [(0, 1), (0, -1), (1, 0), (-1, 0)];
        for i in 0..self.ghosts.len() {
            let pos = self.ghosts[i].pos;
            let dir = self.ghosts[i].dir;
            let reverse = (-dir.0, -dir.1);
            let mut cands: Vec<((i16, i16), (i16, i16))> = Vec::new();
            for d in dirs {
                let np = (pos.0 + d.0, pos.1 + d.1);
                if self.is_wall(np) {
                    continue;
                }
                if dir != (0, 0) && d == reverse {
                    continue;
                }
                cands.push((d, np));
            }
            if cands.is_empty() {
                // Dead end: reversing is the only way out.
                let np = (pos.0 + reverse.0, pos.1 + reverse.1);
                if !self.is_wall(np) {
                    cands.push((reverse, np));
                }
            }
            if cands.is_empty() {
                continue;
            }
            let pick = if fright && self.rand() % 3 == 0 {
                let k = (self.rand() as usize) % cands.len();
                cands[k]
            } else {
                let mut best = cands[0];
                let mut best_d = manhattan(best.1, pac);
                for &cand in cands.iter().skip(1) {
                    let d = manhattan(cand.1, pac);
                    let better = if fright { d > best_d } else { d < best_d };
                    if better {
                        best = cand;
                        best_d = d;
                    }
                }
                best
            };
            self.ghosts[i].pos = pick.1;
            self.ghosts[i].dir = pick.0;
        }
    }

    /// One tick: turn if buffered, glide pac forward (eating pellets), check the
    /// win, then move the ghosts and resolve collisions.
    pub fn step(&mut self) {
        if self.over || self.won {
            return;
        }
        // Apply the buffered turn when its cell is clear.
        if self.want_dir != (0, 0) {
            let n = (self.pac.0 + self.want_dir.0, self.pac.1 + self.want_dir.1);
            if !self.is_wall(n) {
                self.pac_dir = self.want_dir;
            }
        }
        // Move forward if the next cell is open, else stop.
        if self.pac_dir != (0, 0) {
            let n = (self.pac.0 + self.pac_dir.0, self.pac.1 + self.pac_dir.1);
            if !self.is_wall(n) {
                self.pac = n;
                if eat(&mut self.pellets, self.pac) {
                    self.score += 10;
                }
                if eat(&mut self.power, self.pac) {
                    self.score += 50;
                    self.frightened = FRIGHT;
                }
            }
        }
        if self.pellets.is_empty() && self.power.is_empty() {
            self.won = true;
            return;
        }
        // A ghost may already be on pac's new cell.
        self.check_collisions();
        if self.over {
            return;
        }
        self.move_ghosts();
        if self.frightened > 0 {
            self.frightened -= 1;
        }
        self.check_collisions();
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Pac-Man overlay.
pub struct Pacman {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl Pacman {
    pub fn new() -> Self {
        Pacman {
            game: Game::new(1),
            seed: 1,
            paused: false,
            last: None,
            interval: Duration::from_millis(150),
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
        self.paused = false;
        self.last = None;
    }

    /// Running = still in play and not paused; only then do we keep the frame
    /// loop going.
    fn running(&self) -> bool {
        !self.game.over && !self.game.won && !self.paused
    }
}

impl Default for Pacman {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Pacman {
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
        let pellet_style = theme.get("ui.text");
        let power_style = theme.get("warning");
        let pac_style = theme.get("function");
        let enemy_style = theme.get("error");
        let fright_style = theme.get("ui.selection");

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
                "Pac-Man  score {}  lives {}",
                self.game.score, self.game.lives
            ),
            header_style,
        );

        // Board: walls, power pellets, then ordinary pellets.
        for r in 0..H {
            for c in 0..W {
                let cell = (r, c);
                let x = ox + c as u16;
                let y = oy + r as u16;
                if self.game.walls.contains(&cell) {
                    surface.set_string(x, y, "▓", wall_style);
                } else if self.game.power.contains(&cell) {
                    surface.set_string(x, y, "●", power_style);
                } else if self.game.pellets.contains(&cell) {
                    surface.set_string(x, y, "·", pellet_style);
                }
            }
        }

        // Ghosts (skip any off the board).
        let ghost_style = if self.game.frightened > 0 {
            fright_style
        } else {
            enemy_style
        };
        for gh in &self.game.ghosts {
            let (r, c) = gh.pos;
            if r >= 0 && r < H && c >= 0 && c < W {
                surface.set_string(ox + c as u16, oy + r as u16, "▲", ghost_style);
            }
        }

        // Pac.
        let (pr, pc) = self.game.pac;
        if pr >= 0 && pr < H && pc >= 0 && pc < W {
            surface.set_string(ox + pc as u16, oy + pr as u16, "C", pac_style);
        }

        let sy = oy + H as u16 + 1;
        let status = if self.game.over {
            format!(
                "Game over — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.game.won {
            format!(
                "You win! — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.paused {
            format!("Paused — score {}.  SPC resume", self.game.score)
        } else {
            format!(
                "score {}  ·  arrows/hjkl move  SPC pause  n new  q quit",
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
    fn pac_moves_then_stops_at_a_wall() {
        let mut g = Game::new(1);
        // Pac starts at the bottom-centre corridor; glide left into open cells.
        g.pac_dir = (0, -1);
        g.step();
        assert_eq!(g.pac, (11, 8), "pac advances into an open corridor");
        // Now face straight down into the bottom border wall: it must not move.
        g.pac_dir = (1, 0);
        let before = g.pac;
        g.step();
        assert_eq!(g.pac, before, "pac stops when the next cell is a wall");
    }

    #[test]
    fn eating_a_pellet_scores_and_clears_it() {
        let mut g = Game::new(1);
        let target = (11, 8);
        assert!(g.pellets.contains(&target));
        let before = g.score;
        g.pac_dir = (0, -1);
        g.step();
        assert_eq!(g.score, before + 10);
        assert!(!g.pellets.contains(&target), "the eaten pellet is removed");
    }

    #[test]
    fn a_power_pellet_sets_the_frightened_timer() {
        let mut g = Game::new(1);
        // Sit next to the bottom-left power pellet and step onto it.
        g.pac = (11, 2);
        g.pac_dir = (0, -1);
        assert_eq!(g.frightened, 0);
        g.step();
        assert!(
            g.frightened > 0,
            "eating a power pellet frightens the ghosts"
        );
        assert_eq!(g.score, 50);
    }

    #[test]
    fn a_ghost_on_pac_costs_a_life_and_resets() {
        let mut g = Game::new(1);
        g.frightened = 0;
        g.pac_dir = (0, 0); // hold still so the collision check catches it
        let start_pac = g.pac;
        g.ghosts[0].pos = g.pac;
        let before = g.lives;
        g.step();
        assert_eq!(g.lives, before - 1, "an unfrightened ghost costs a life");
        assert_eq!(g.pac, start_pac, "positions reset after losing a life");
    }

    #[test]
    fn clearing_the_last_pellet_wins() {
        let mut g = Game::new(1);
        g.pellets.clear();
        g.power.clear();
        g.pellets.push((11, 8)); // one pellet directly to pac's left
        g.pac_dir = (0, -1);
        g.step();
        assert!(g.won, "eating the final pellet wins the game");
    }
}
