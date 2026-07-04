//! Donkey Kong — a zemacs girder-climb in the spirit of the arcade classic.
//!
//! Kong sits at the top of a fixed lattice of girders and ladders hurling
//! barrels down at you; climb to the goal without being flattened. Walk with
//! the arrows or `h`/`l`, climb ladders with `k`/`j`, `SPC` jumps over a rolling
//! barrel, `p` pauses, `n` restarts, `q`/`Esc` quits. Like the other action
//! games it animates itself on wall-clock delta and schedules the next frame via
//! `zemacs_event::request_redraw` only while playing, so it idles when paused,
//! finished or closed. The level logic is pure and unit-tested (keys parse into a
//! `donkeykong` keymap mode by `scripts/gen_port_report.py`).

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The fixed level: `=` girder, `H` ladder, ` ` air, `@` player start, `&` goal,
/// `K` Kong. Alternating single-cell holes at the ends of each girder let barrels
/// roll off and drop to the girder below, zig-zagging down the board.
const LEVEL: [&str; 22] = [
    "                                ",
    "                                ",
    "==K==========================&= ",
    "      H                         ",
    "      H                         ",
    "      H                         ",
    " ===============================",
    "                          H     ",
    "                          H     ",
    "                          H     ",
    "=============================== ",
    "      H                         ",
    "      H                         ",
    "      H                         ",
    " ===============================",
    "                          H     ",
    "                          H     ",
    "                          H     ",
    "====@========================== ",
    "                                ",
    "                                ",
    "                                ",
];

const GIRDER: char = '=';
const LADDER: char = 'H';
const GOAL: char = '&';
const AIR: char = ' ';

/// A barrel tumbling down the girders.
#[derive(Clone)]
pub struct Barrel {
    pub r: i16,
    pub c: i16,
    pub dx: i16,
    falling: bool,
    scored: bool,
}

/// The pure Donkey Kong level. No I/O, no timing — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub grid: Vec<Vec<char>>,
    pub w: i16,
    pub h: i16,
    pub player: (i16, i16),
    pub start: (i16, i16),
    pub kong: (i16, i16),
    pub goal: (i16, i16),
    pub barrels: Vec<Barrel>,
    /// Vertical velocity while jumping.
    pub vy: i16,
    pub jumping: bool,
    pub score: u32,
    pub lives: u32,
    pub won: bool,
    spawn: u32,
    next: u32,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut grid: Vec<Vec<char>> = LEVEL.iter().map(|row| row.chars().collect()).collect();
        let h = grid.len() as i16;
        let w = grid[0].len() as i16;
        let (mut player, mut kong, mut goal) = ((0, 0), (0, 0), (0, 0));
        for r in 0..grid.len() {
            for c in 0..grid[r].len() {
                match grid[r][c] {
                    '@' => {
                        player = (r as i16, c as i16);
                        grid[r][c] = GIRDER; // the start stands on a girder
                    }
                    'K' => {
                        kong = (r as i16, c as i16);
                        grid[r][c] = GIRDER; // Kong stands on the top girder
                    }
                    '&' => goal = (r as i16, c as i16),
                    _ => {}
                }
            }
        }
        Game {
            grid,
            w,
            h,
            player,
            start: player,
            kong,
            goal,
            barrels: Vec::new(),
            vy: 0,
            jumping: false,
            score: 0,
            lives: 3,
            won: false,
            spawn: 0,
            next: 14,
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

    fn at(&self, r: i16, c: i16) -> char {
        if r < 0 || r >= self.h || c < 0 || c >= self.w {
            return AIR;
        }
        self.grid[r as usize][c as usize]
    }

    /// A cell you can stand on: girders and the goal.
    fn supported(&self, r: i16, c: i16) -> bool {
        let ch = self.at(r, c);
        ch == GIRDER || ch == GOAL
    }

    fn on_ladder(&self) -> bool {
        self.at(self.player.0, self.player.1) == LADDER
    }

    /// Walk one cell left/right. Allowed while supported, clinging to a ladder or
    /// airborne (jumping) — stepping past a girder edge drops you into the air,
    /// where gravity takes over.
    pub fn walk(&mut self, dx: i16) {
        let (r, c) = self.player;
        let can = self.jumping || self.supported(r, c) || self.at(r, c) == LADDER;
        if !can {
            return;
        }
        let nc = c + dx;
        if nc < 0 || nc >= self.w {
            return;
        }
        self.player.1 = nc;
    }

    /// Climb a ladder by `dy` (up = -1). Grabs a ladder cell, or steps from a
    /// ladder onto the girder/goal it meets.
    pub fn climb(&mut self, dy: i16) {
        if self.jumping {
            return;
        }
        let (r, c) = self.player;
        let nr = r + dy;
        if nr < 0 || nr >= self.h {
            return;
        }
        let here = self.at(r, c);
        let target = self.at(nr, c);
        if target == LADDER || (here == LADDER && (target == GIRDER || target == GOAL)) {
            self.player.0 = nr;
        }
    }

    /// Leap off a girder, arcing up a couple of cells before gravity pulls you
    /// back down — used to hop over a rolling barrel.
    pub fn jump(&mut self) {
        let (r, c) = self.player;
        if !self.jumping && self.supported(r, c) {
            self.jumping = true;
            self.vy = -2;
        }
    }

    fn respawn(&mut self) {
        self.player = self.start;
        self.jumping = false;
        self.vy = 0;
    }

    /// Resolve barrel↔player overlaps: a barrel sharing your cell costs a life,
    /// while a barrel passing beneath a jump scores.
    fn resolve(&mut self) {
        let (pr, pc) = self.player;
        let mut i = 0;
        while i < self.barrels.len() {
            let (br, bc, scored) = {
                let b = &self.barrels[i];
                (b.r, b.c, b.scored)
            };
            if bc == pc {
                if br == pr && !self.jumping {
                    self.lives = self.lives.saturating_sub(1);
                    self.barrels.remove(i);
                    self.respawn();
                    continue;
                } else if self.jumping && pr < br && !scored {
                    self.barrels[i].scored = true;
                    self.score += 10;
                }
            }
            i += 1;
        }
    }

    /// Apply the player's jump arc, or gravity when merely airborne.
    fn tick_player(&mut self) {
        let (r, c) = self.player;
        if self.jumping {
            let nr = (r + self.vy).clamp(0, self.h - 1);
            self.player.0 = nr;
            self.vy += 1;
            if self.vy >= 0 && self.supported(self.player.0, self.player.1) {
                self.jumping = false;
                self.vy = 0;
            }
            return;
        }
        if self.on_ladder() {
            return; // clinging to a ladder
        }
        if !self.supported(r, c) && r + 1 < self.h {
            self.player.0 = r + 1; // fall toward the girder below
        }
    }

    /// Spawn Kong's barrels and roll every barrel one cell.
    fn tick_barrels(&mut self) {
        self.spawn += 1;
        if self.spawn >= self.next {
            self.spawn = 0;
            self.next = 10 + (self.rand() % 10) as u32;
            let dx = if self.player.1 >= self.kong.1 { 1 } else { -1 };
            self.barrels.push(Barrel {
                r: self.kong.0,
                c: self.kong.1,
                dx,
                falling: false,
                scored: false,
            });
        }
        let mut i = 0;
        while i < self.barrels.len() {
            if self.step_barrel(i) {
                self.barrels.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Advance one barrel; returns `true` when it has rolled off the board.
    fn step_barrel(&mut self, i: usize) -> bool {
        let (r, c, dx, falling) = {
            let b = &self.barrels[i];
            (b.r, b.c, b.dx, b.falling)
        };
        if self.at(r, c) != GIRDER {
            // In the air over a hole/edge — drop to the next girder.
            let nr = r + 1;
            if nr >= self.h {
                return true;
            }
            self.barrels[i].r = nr;
            self.barrels[i].falling = true;
            return false;
        }
        // On a girder: alternate direction each time we land, then roll.
        let mut dx = dx;
        if falling {
            dx = -dx;
            self.barrels[i].falling = false;
        }
        let nc = c + dx;
        if nc < 0 || nc >= self.w {
            self.barrels[i].dx = -dx; // bounce off the outer wall
        } else {
            self.barrels[i].c = nc;
            self.barrels[i].dx = dx;
        }
        false
    }

    /// One tick: collisions, player physics, barrels, then the win check.
    pub fn step(&mut self) {
        if self.won || self.lives == 0 {
            return;
        }
        self.resolve();
        self.tick_player();
        self.tick_barrels();
        if self.player == self.goal {
            self.won = true;
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Donkey Kong overlay.
pub struct DonkeyKong {
    game: Game,
    seed: u64,
    paused: bool,
    last: Option<Instant>,
    interval: Duration,
}

impl DonkeyKong {
    pub fn new() -> Self {
        DonkeyKong {
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

    /// Running = the round is live and not paused; only then do we keep the frame
    /// loop going.
    fn running(&self) -> bool {
        !self.game.won && self.game.lives != 0 && !self.paused
    }
}

impl Default for DonkeyKong {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for DonkeyKong {
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
            key!(Left) | key!('h') => self.game.walk(-1),
            key!(Right) | key!('l') => self.game.walk(1),
            key!(Up) | key!('k') => self.game.climb(-1),
            key!(Down) | key!('j') => self.game.climb(1),
            key!(' ') => self.game.jump(),
            key!('p') => self.paused = !self.paused,
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
        let girder_style = theme.get("ui.linenr");
        let ladder_style = theme.get("function");
        let player_style = theme.get("ui.text.focus");
        let kong_style = theme.get("error");
        let barrel_style = theme.get("warning");
        let goal_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < (self.game.w as u16) + 4 || area.height < (self.game.h as u16) + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(
            ox,
            area.y,
            &format!(
                "Donkey Kong  score {}  lives {}",
                self.game.score, self.game.lives
            ),
            header_style,
        );

        for r in 0..self.game.h {
            for c in 0..self.game.w {
                let (glyph, style) = match self.game.at(r, c) {
                    GIRDER => ("═", girder_style),
                    LADDER => ("‖", ladder_style),
                    GOAL => ("♥", goal_style),
                    _ => continue,
                };
                surface.set_string(ox + c as u16, oy + r as u16, glyph, style);
            }
        }

        surface.set_string(
            ox + self.game.kong.1 as u16,
            oy + self.game.kong.0 as u16,
            "▚",
            kong_style,
        );

        for b in &self.game.barrels {
            if b.r < 0 || b.r >= self.game.h || b.c < 0 || b.c >= self.game.w {
                continue; // skip off-board barrels
            }
            surface.set_string(ox + b.c as u16, oy + b.r as u16, "o", barrel_style);
        }

        let (pr, pc) = self.game.player;
        if pr >= 0 && pr < self.game.h && pc >= 0 && pc < self.game.w {
            surface.set_string(ox + pc as u16, oy + pr as u16, "☺", player_style);
        }

        let sy = oy + self.game.h as u16 + 1;
        let status = if self.game.won {
            format!(
                "You win! — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.game.lives == 0 {
            format!(
                "Game over — score {}.  n: new game  q: quit",
                self.game.score
            )
        } else if self.paused {
            format!("Paused — score {}.  p resume", self.game.score)
        } else {
            "h/l move · k/j climb · SPC jump · p pause · n new · q quit".to_string()
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravity_pulls_an_airborne_player_down() {
        let mut g = Game::new(1);
        g.player = (15, 10); // air above the bottom girder
        g.step();
        assert!(
            g.player.0 > 15,
            "an unsupported player falls toward the girder"
        );
    }

    #[test]
    fn climbing_moves_the_player_up_a_ladder() {
        let mut g = Game::new(1);
        g.player = (16, 26); // on a ladder cell
        g.climb(-1);
        assert_eq!(g.player.0, 15, "up one ladder rung");
    }

    #[test]
    fn a_jump_lifts_then_returns_to_the_girder() {
        let mut g = Game::new(1);
        g.player = (18, 15); // on the bottom girder
        let base = g.player.0;
        g.jump();
        g.step();
        assert!(
            g.player.0 < base,
            "the jump lifts the player off the girder"
        );
        for _ in 0..4 {
            g.step();
        }
        assert_eq!(g.player.0, base, "gravity returns the player to the girder");
        assert!(!g.jumping, "the jump is over once landed");
    }

    #[test]
    fn a_barrel_rolls_along_a_girder() {
        let mut g = Game::new(1);
        g.player = (18, 4); // out of the barrel's column
        g.barrels.push(Barrel {
            r: 18,
            c: 10,
            dx: 1,
            falling: false,
            scored: false,
        });
        g.step();
        assert_eq!(
            g.barrels[0].c, 11,
            "the barrel advances a cell along the girder"
        );
    }

    #[test]
    fn a_barrel_hitting_the_player_costs_a_life() {
        let mut g = Game::new(1);
        g.player = (18, 12);
        g.barrels.push(Barrel {
            r: 18,
            c: 12,
            dx: 1,
            falling: false,
            scored: false,
        });
        let lives = g.lives;
        g.step();
        assert_eq!(g.lives, lives - 1, "a barrel on the player costs a life");
        assert!(g.barrels.is_empty(), "the barrel is consumed by the hit");
    }
}
