//! Battleship — a small terminal Battleship for zmax, played against the CPU.
//!
//! Two 10×10 grids sit side by side: your fleet on the left and your radar view
//! of the enemy on the right. Move the targeting cursor over the enemy grid with
//! the arrows or `hjkl`, `SPC`/`Enter` fires at the cell under the cursor, `n`
//! starts a fresh game and `q`/`Esc` quits. Like Minesweeper this one is
//! turn-based: nothing animates, the boards only change in response to a key.
//! After each of your shots the computer fires back with a hunt/target AI. The
//! board logic is pure and unit-tested (fleets are laid by a small LCG so a
//! given seed is reproducible).

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: usize = 10;
const H: usize = 10;
/// Standard fleet: carrier, battleship, two cruisers, destroyer.
const FLEET: [usize; 5] = [5, 4, 3, 3, 2];

/// The result of firing at a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shot {
    Hit,
    Miss,
    Sunk,
    Invalid,
}

/// Where the game is: still playing, the player cleared the enemy fleet, or the
/// CPU cleared the player's fleet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Playing,
    Won,
    Lost,
}

/// The pure Battleship game. No I/O, no timing — unit-tested. Both fleets are
/// laid with the same LCG the other games use, so `Game::new(seed)` is
/// deterministic.
#[derive(Clone)]
pub struct Game {
    /// Player's own board: `true` where a player ship sits.
    player_ships: Vec<bool>,
    /// Ship id per cell on the player board (`-1` = water).
    player_ship_id: Vec<i32>,
    /// Cells the CPU has fired on that hit a player ship.
    player_hits: Vec<bool>,
    /// Cells the CPU has fired on that missed.
    player_misses: Vec<bool>,
    /// Sizes of the player fleet, indexed by ship id.
    player_ship_sizes: Vec<usize>,

    /// Enemy board: `true` where an enemy ship sits.
    enemy_ships: Vec<bool>,
    /// Ship id per cell on the enemy board (`-1` = water).
    enemy_ship_id: Vec<i32>,
    /// Cells the player has fired on that hit an enemy ship.
    enemy_hits: Vec<bool>,
    /// Cells the player has fired on that missed.
    enemy_misses: Vec<bool>,
    /// Sizes of the enemy fleet, indexed by ship id.
    enemy_ship_sizes: Vec<usize>,

    /// Targeting cursor over the enemy grid as `(row, col)`.
    cursor: (usize, usize),

    /// CPU AI: cells queued to try next (the "target" phase after a hit).
    cpu_targets: Vec<(usize, usize)>,
    /// CPU AI: hits scored on the current, not-yet-sunk player ship. Used to
    /// follow a ship's line once two hits reveal its orientation.
    cpu_hits: Vec<(usize, usize)>,

    state: State,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            player_ships: vec![false; W * H],
            player_ship_id: vec![-1; W * H],
            player_hits: vec![false; W * H],
            player_misses: vec![false; W * H],
            player_ship_sizes: Vec::new(),
            enemy_ships: vec![false; W * H],
            enemy_ship_id: vec![-1; W * H],
            enemy_hits: vec![false; W * H],
            enemy_misses: vec![false; W * H],
            enemy_ship_sizes: Vec::new(),
            cursor: (0, 0),
            cpu_targets: Vec::new(),
            cpu_hits: Vec::new(),
            state: State::Playing,
            rng: seed | 1,
        };
        let (ps, pid) = g.place_fleet();
        g.player_ships = ps;
        g.player_ship_id = pid;
        g.player_ship_sizes = FLEET.to_vec();
        let (es, eid) = g.place_fleet();
        g.enemy_ships = es;
        g.enemy_ship_id = eid;
        g.enemy_ship_sizes = FLEET.to_vec();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Lay one fleet with rejection sampling: each ship gets a random
    /// orientation and origin, and is only kept if it overlaps nothing already
    /// placed. Adjacency is allowed. Returns the ship mask and the id map.
    fn place_fleet(&mut self) -> (Vec<bool>, Vec<i32>) {
        let mut ships = vec![false; W * H];
        let mut ids = vec![-1i32; W * H];
        for (sid, &len) in FLEET.iter().enumerate() {
            loop {
                let horiz = self.rand().is_multiple_of(2);
                let (span_r, span_c) = if horiz {
                    (H, W - len + 1)
                } else {
                    (H - len + 1, W)
                };
                let r = (self.rand() % span_r as u64) as usize;
                let c = (self.rand() % span_c as u64) as usize;
                let cells: Vec<usize> = (0..len)
                    .map(|k| if horiz { idx(r, c + k) } else { idx(r + k, c) })
                    .collect();
                if cells.iter().all(|&ci| !ships[ci]) {
                    for &ci in &cells {
                        ships[ci] = true;
                        ids[ci] = sid as i32;
                    }
                    break;
                }
            }
        }
        (ships, ids)
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// Move the targeting cursor by `(dr, dc)`, clamped to the enemy grid.
    pub fn move_cursor(&mut self, dr: i32, dc: i32) {
        let r = (self.cursor.0 as i32 + dr).clamp(0, H as i32 - 1) as usize;
        let c = (self.cursor.1 as i32 + dc).clamp(0, W as i32 - 1) as usize;
        self.cursor = (r, c);
    }

    /// Enemy ships not yet fully sunk.
    pub fn enemy_ships_left(&self) -> usize {
        (0..self.enemy_ship_sizes.len())
            .filter(|&id| !ship_sunk(&self.enemy_ship_id, &self.enemy_hits, id as i32))
            .count()
    }

    /// Player ships not yet fully sunk.
    pub fn player_ships_left(&self) -> usize {
        (0..self.player_ship_sizes.len())
            .filter(|&id| !ship_sunk(&self.player_ship_id, &self.player_hits, id as i32))
            .count()
    }

    /// `true` once every enemy ship cell has been hit.
    pub fn enemy_lost(&self) -> bool {
        (0..W * H).all(|i| !self.enemy_ships[i] || self.enemy_hits[i])
    }

    /// `true` once every player ship cell has been hit.
    pub fn player_lost(&self) -> bool {
        (0..W * H).all(|i| !self.player_ships[i] || self.player_hits[i])
    }

    /// `true` if the enemy ship covering `(r, c)` is fully sunk (drives the
    /// `▓` glyph in the radar view).
    pub fn enemy_cell_sunk(&self, r: usize, c: usize) -> bool {
        let id = self.enemy_ship_id[idx(r, c)];
        id >= 0 && ship_sunk(&self.enemy_ship_id, &self.enemy_hits, id)
    }

    /// The player fires at `(r, c)` on the enemy board. A cell that was already
    /// fired on is rejected (`Invalid`) with no side effects — no double-firing.
    pub fn fire_at_enemy(&mut self, r: usize, c: usize) -> Shot {
        if self.state != State::Playing {
            return Shot::Invalid;
        }
        let i = idx(r, c);
        if self.enemy_hits[i] || self.enemy_misses[i] {
            return Shot::Invalid;
        }
        if self.enemy_ships[i] {
            self.enemy_hits[i] = true;
            let id = self.enemy_ship_id[i];
            if ship_sunk(&self.enemy_ship_id, &self.enemy_hits, id) {
                if self.enemy_lost() {
                    self.state = State::Won;
                }
                Shot::Sunk
            } else {
                Shot::Hit
            }
        } else {
            self.enemy_misses[i] = true;
            Shot::Miss
        }
    }

    /// Fire at the enemy cell under the cursor, then — if the shot landed and
    /// the game is still going — let the computer take its turn.
    pub fn fire_cursor(&mut self) {
        let (r, c) = self.cursor;
        let shot = self.fire_at_enemy(r, c);
        if shot != Shot::Invalid && self.state == State::Playing {
            self.cpu_turn();
        }
    }

    fn player_shot(&self, i: usize) -> bool {
        self.player_hits[i] || self.player_misses[i]
    }

    /// The CPU fires at `(r, c)` on the player board, updating its hunt/target
    /// state. A hit queues the cell's orthogonal neighbours for follow-up;
    /// sinking a ship clears the target queue back to the hunt phase.
    fn cpu_fire_at(&mut self, r: usize, c: usize) -> Shot {
        let i = idx(r, c);
        if self.player_shot(i) {
            return Shot::Invalid;
        }
        if self.player_ships[i] {
            self.player_hits[i] = true;
            self.cpu_hits.push((r, c));
            let id = self.player_ship_id[i];
            if ship_sunk(&self.player_ship_id, &self.player_hits, id) {
                self.cpu_hits.clear();
                self.cpu_targets.clear();
                if self.player_lost() {
                    self.state = State::Lost;
                }
                Shot::Sunk
            } else {
                self.enqueue_followups();
                Shot::Hit
            }
        } else {
            self.player_misses[i] = true;
            Shot::Miss
        }
    }

    /// Queue the cells worth trying after a hit. With a single hit that means
    /// its orthogonal neighbours; once two hits share a row or column the AI
    /// commits to that line and only chases the cells extending its ends.
    fn enqueue_followups(&mut self) {
        if self.cpu_hits.len() >= 2 {
            let (r0, _) = self.cpu_hits[0];
            let (r1, c1) = self.cpu_hits[1];
            if r0 == r1 {
                let cols: Vec<usize> = self.cpu_hits.iter().map(|h| h.1).collect();
                let lo = *cols.iter().min().unwrap();
                let hi = *cols.iter().max().unwrap();
                if lo > 0 {
                    self.push_target(r0, lo - 1);
                }
                if hi + 1 < W {
                    self.push_target(r0, hi + 1);
                }
                return;
            }
            if self.cpu_hits[0].1 == c1 {
                let rows: Vec<usize> = self.cpu_hits.iter().map(|h| h.0).collect();
                let lo = *rows.iter().min().unwrap();
                let hi = *rows.iter().max().unwrap();
                if lo > 0 {
                    self.push_target(lo - 1, c1);
                }
                if hi + 1 < H {
                    self.push_target(hi + 1, c1);
                }
                return;
            }
        }
        let (r, c) = *self.cpu_hits.last().unwrap();
        for (nr, nc) in ortho_neighbors(r, c) {
            self.push_target(nr, nc);
        }
    }

    fn push_target(&mut self, r: usize, c: usize) {
        if !self.player_shot(idx(r, c)) {
            self.cpu_targets.push((r, c));
        }
    }

    /// Pop the next still-unshot cell off the target queue, if any.
    fn next_target(&mut self) -> Option<(usize, usize)> {
        while let Some((r, c)) = self.cpu_targets.pop() {
            if !self.player_shot(idx(r, c)) {
                return Some((r, c));
            }
        }
        None
    }

    /// A random cell the CPU has not fired on yet, hunting for a new ship.
    fn random_unshot(&mut self) -> Option<(usize, usize)> {
        for _ in 0..256 {
            let r = (self.rand() % H as u64) as usize;
            let c = (self.rand() % W as u64) as usize;
            if !self.player_shot(idx(r, c)) {
                return Some((r, c));
            }
        }
        // Deterministic fallback so we never spin forever near the endgame.
        (0..W * H)
            .find(|&i| !self.player_shot(i))
            .map(|i| (i / W, i % W))
    }

    /// One CPU turn: fire at a queued target if the AI is tracking a ship,
    /// otherwise hunt at a random unshot cell.
    pub fn cpu_turn(&mut self) -> Option<(usize, usize)> {
        if self.state != State::Playing {
            return None;
        }
        let cell = match self.next_target() {
            Some(t) => t,
            None => self.random_unshot()?,
        };
        self.cpu_fire_at(cell.0, cell.1);
        Some(cell)
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

fn idx(r: usize, c: usize) -> usize {
    r * W + c
}

/// The in-bounds orthogonal (4-way) neighbourhood of `(r, c)`.
fn ortho_neighbors(r: usize, c: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(4);
    for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nr = r as i32 + dr;
        let nc = c as i32 + dc;
        if nr >= 0 && nr < H as i32 && nc >= 0 && nc < W as i32 {
            out.push((nr as usize, nc as usize));
        }
    }
    out
}

/// `true` if ship `id` exists on `ids` and all of its cells are hit.
fn ship_sunk(ids: &[i32], hits: &[bool], id: i32) -> bool {
    let mut any = false;
    for i in 0..ids.len() {
        if ids[i] == id {
            any = true;
            if !hits[i] {
                return false;
            }
        }
    }
    any
}

/// The interactive Battleship overlay.
pub struct Battleship {
    game: Game,
    seed: u64,
}

impl Battleship {
    pub fn new() -> Self {
        Battleship {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Battleship {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Battleship {
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
            key!(Left) | key!('h') => self.game.move_cursor(0, -1),
            key!(Right) | key!('l') => self.game.move_cursor(0, 1),
            key!(Up) | key!('k') => self.game.move_cursor(-1, 0),
            key!(Down) | key!('j') => self.game.move_cursor(1, 0),
            key!(' ') | key!(Enter) => self.game.fire_cursor(),
            key!('n') => self.restart(),
            _ => {}
        }
        zmax_event::request_redraw();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let water_style = theme.get("ui.linenr");
        let ship_style = theme.get("function");
        let hit_you_style = theme.get("error");
        let radar_hit_style = theme.get("warning");
        let sunk_style = theme.get("error");
        let cursor_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        // Each grid is `W` columns wide; they sit side by side with a gap.
        let gap = 4u16;
        let need_w = 2 + (W as u16) * 2 + gap + 4;
        if area.width < need_w || area.height < (H as u16) + 5 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 3;
        let ex = ox + W as u16 + gap; // enemy grid origin x

        let status = match self.game.state() {
            State::Playing => "your move",
            State::Won => "YOU WIN!",
            State::Lost => "YOU LOSE",
        };
        surface.set_string(
            ox,
            area.y,
            &format!(
                "Battleship  enemy ships left {}  your fleet {}  [{}]",
                self.game.enemy_ships_left(),
                self.game.player_ships_left(),
                status
            ),
            header_style,
        );
        surface.set_string(ox, oy - 1, "Fleet", text_style);
        surface.set_string(ex, oy - 1, "Radar", text_style);

        for r in 0..H {
            for c in 0..W {
                let i = idx(r, c);
                // Left: the player's own fleet and the CPU's shots on it.
                let (pg, ps) = if self.game.player_hits[i] {
                    ("✷", hit_you_style)
                } else if self.game.player_misses[i] {
                    ("∘", water_style)
                } else if self.game.player_ships[i] {
                    ("▪", ship_style)
                } else {
                    ("·", water_style)
                };
                surface.set_string(ox + c as u16, oy + r as u16, pg, ps);

                // Right: the radar view of the enemy grid.
                let (eg, mut es) = if self.game.enemy_hits[i] {
                    if self.game.enemy_cell_sunk(r, c) {
                        ("▓", sunk_style)
                    } else {
                        ("✷", radar_hit_style)
                    }
                } else if self.game.enemy_misses[i] {
                    ("∘", water_style)
                } else {
                    ("·", water_style)
                };
                if (r, c) == self.game.cursor() {
                    es = cursor_style;
                }
                surface.set_string(ex + c as u16, oy + r as u16, eg, es);
            }
        }

        let sy = oy + H as u16 + 1;
        surface.set_string(
            ox,
            sy,
            "hjkl/arrows aim · SPC/Enter fire · n new · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty game with no ships laid, ready for hand placement.
    fn blank() -> Game {
        Game {
            player_ships: vec![false; W * H],
            player_ship_id: vec![-1; W * H],
            player_hits: vec![false; W * H],
            player_misses: vec![false; W * H],
            player_ship_sizes: Vec::new(),
            enemy_ships: vec![false; W * H],
            enemy_ship_id: vec![-1; W * H],
            enemy_hits: vec![false; W * H],
            enemy_misses: vec![false; W * H],
            enemy_ship_sizes: Vec::new(),
            cursor: (0, 0),
            cpu_targets: Vec::new(),
            cpu_hits: Vec::new(),
            state: State::Playing,
            rng: 1,
        }
    }

    fn place_enemy(g: &mut Game, id: i32, cells: &[(usize, usize)]) {
        for &(r, c) in cells {
            g.enemy_ships[idx(r, c)] = true;
            g.enemy_ship_id[idx(r, c)] = id;
        }
        while g.enemy_ship_sizes.len() <= id as usize {
            g.enemy_ship_sizes.push(0);
        }
        g.enemy_ship_sizes[id as usize] = cells.len();
    }

    fn place_player(g: &mut Game, id: i32, cells: &[(usize, usize)]) {
        for &(r, c) in cells {
            g.player_ships[idx(r, c)] = true;
            g.player_ship_id[idx(r, c)] = id;
        }
        while g.player_ship_sizes.len() <= id as usize {
            g.player_ship_sizes.push(0);
        }
        g.player_ship_sizes[id as usize] = cells.len();
    }

    #[test]
    fn firing_on_a_ship_is_a_hit_and_marks_the_cell() {
        let mut g = blank();
        place_enemy(&mut g, 0, &[(2, 3), (2, 4)]);
        assert_eq!(g.fire_at_enemy(2, 3), Shot::Hit);
        assert!(g.enemy_hits[idx(2, 3)], "the struck cell is marked hit");
        // A shot into open water reads as a miss.
        assert_eq!(g.fire_at_enemy(7, 7), Shot::Miss);
        assert!(g.enemy_misses[idx(7, 7)]);
    }

    #[test]
    fn sinking_a_ship_reports_sunk() {
        let mut g = blank();
        place_enemy(&mut g, 0, &[(4, 1), (4, 2), (4, 3)]);
        assert_eq!(g.fire_at_enemy(4, 1), Shot::Hit);
        assert_eq!(g.fire_at_enemy(4, 2), Shot::Hit);
        assert_eq!(
            g.fire_at_enemy(4, 3),
            Shot::Sunk,
            "last cell sinks the ship"
        );
    }

    #[test]
    fn firing_a_fired_cell_is_rejected() {
        let mut g = blank();
        place_enemy(&mut g, 0, &[(0, 0), (0, 1)]);
        assert_eq!(g.fire_at_enemy(0, 0), Shot::Hit);
        // A repeat shot at the same cell is Invalid and changes nothing.
        assert_eq!(g.fire_at_enemy(0, 0), Shot::Invalid);
        assert_eq!(g.enemy_hits.iter().filter(|&&h| h).count(), 1);
        // Repeating a miss is likewise rejected without a second miss mark.
        assert_eq!(g.fire_at_enemy(5, 5), Shot::Miss);
        assert_eq!(g.fire_at_enemy(5, 5), Shot::Invalid);
        assert_eq!(g.enemy_misses.iter().filter(|&&m| m).count(), 1);
    }

    #[test]
    fn cpu_targets_a_neighbor_after_a_hit() {
        let mut g = blank();
        place_player(&mut g, 0, &[(3, 3), (3, 4)]);
        // The CPU hits (3,3); this queues its orthogonal neighbours.
        assert_eq!(g.cpu_fire_at(3, 3), Shot::Hit);
        // Its next turn must fire an orthogonally-adjacent cell.
        let (r, c) = g.cpu_turn().expect("cpu fired");
        let adjacent = (r as i32 - 3).abs() + (c as i32 - 3).abs() == 1;
        assert!(
            adjacent,
            "cpu follows up next to its last hit, got ({r},{c})"
        );
    }

    #[test]
    fn enemy_lost_once_every_ship_cell_is_hit() {
        let mut g = blank();
        place_enemy(&mut g, 0, &[(1, 1), (1, 2)]);
        place_enemy(&mut g, 1, &[(6, 6)]);
        assert!(!g.enemy_lost());
        for &(r, c) in &[(1, 1), (1, 2), (6, 6)] {
            g.fire_at_enemy(r, c);
        }
        assert!(g.enemy_lost(), "all enemy ship cells are hit");
        assert_eq!(g.state(), State::Won, "clearing the fleet wins the game");
    }

    #[test]
    fn new_game_lays_full_non_overlapping_fleets() {
        let g = Game::new(42);
        let expected: usize = FLEET.iter().sum();
        assert_eq!(g.player_ships.iter().filter(|&&s| s).count(), expected);
        assert_eq!(g.enemy_ships.iter().filter(|&&s| s).count(), expected);
        assert_eq!(g.enemy_ships_left(), FLEET.len());
        assert_eq!(g.player_ships_left(), FLEET.len());
    }
}
