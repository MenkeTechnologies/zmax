//! Sokoban — the zmax port of GNU Emacs `sokoban`.
//!
//! Shove every box (`▢`) onto a goal (`·`). Walk with the arrows or `hjkl`; when
//! you step into a box it is pushed one cell — but only if the square beyond is
//! empty floor, never a wall or a second box. `r` resets the current level, `n`
//! advances once the level is solved, `u` undoes the last move and `q`/`Esc`
//! quits. Like the other turn-based games it never animates: nothing happens
//! until a key arrives, so there is no frame loop. The board logic is pure and
//! unit-tested; a small LCG (shared with the other games) picks the starting
//! level so a given seed is reproducible.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const NUM_LEVELS: usize = 3;

/// Three small classic pushers, drawn in the standard Sokoban charset: `#` wall,
/// ` ` floor, `.` goal, `$` box, `*` box-on-goal, `@` player, `+` player-on-goal.
const LEVELS: [&str; NUM_LEVELS] = [
    // Level 1: nudge the single box one square left onto the goal.
    "\
#######
#     #
# .$@ #
#     #
#######",
    // Level 2: push both boxes straight up onto their goals.
    "\
########
# .  . #
# $  $ #
#  @   #
#      #
########",
    // Level 3: run both boxes across the room to the far markers.
    "\
##########
#        #
#  $  .  #
#  @     #
#  $  .  #
#        #
##########",
];

/// One point in the undo history: the board state before a move.
#[derive(Clone)]
struct Snapshot {
    player: (i16, i16),
    boxes: Vec<bool>,
    moves: u32,
}

/// The pure Sokoban board. No I/O, no timing — unit-tested. Cells are indexed by
/// `idx(row, col)` over a `width`×`height` grid parsed from a level map. The LCG
/// only picks the starting level, so `Game::new(seed)` is deterministic.
#[derive(Clone)]
pub struct Game {
    width: i16,
    height: i16,
    /// `true` where a wall sits.
    walls: Vec<bool>,
    /// `true` where a goal sits.
    goals: Vec<bool>,
    /// `true` where a box currently rests.
    boxes: Vec<bool>,
    /// Player position as `(row, col)`.
    player: (i16, i16),
    /// Which level (0-based) is loaded.
    level: usize,
    /// Player moves made on this level (walks and pushes both count).
    moves: u32,
    /// Set once the final level is cleared.
    all_solved: bool,
    /// Undo stack of pre-move snapshots.
    history: Vec<Snapshot>,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            width: 0,
            height: 0,
            walls: Vec::new(),
            goals: Vec::new(),
            boxes: Vec::new(),
            player: (0, 0),
            level: 0,
            moves: 0,
            all_solved: false,
            history: Vec::new(),
            rng: seed | 1,
        };
        let start = (g.rand() % NUM_LEVELS as u64) as usize;
        g.load_level(start);
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Load level `idx` from the embedded maps, clearing move count and history.
    fn load_level(&mut self, idx: usize) {
        self.parse_into(LEVELS[idx]);
        self.level = idx;
    }

    /// Parse a level map into the board fields and reset the per-level counters.
    fn parse_into(&mut self, map: &str) {
        let lines: Vec<&str> = map.lines().collect();
        let height = lines.len() as i16;
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as i16;
        let n = (width * height) as usize;
        let mut walls = vec![false; n];
        let mut goals = vec![false; n];
        let mut boxes = vec![false; n];
        let mut player = (0i16, 0i16);
        for (r, line) in lines.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                let i = r * width as usize + c;
                match ch {
                    '#' => walls[i] = true,
                    '.' => goals[i] = true,
                    '$' => boxes[i] = true,
                    '*' => {
                        boxes[i] = true;
                        goals[i] = true;
                    }
                    '@' => player = (r as i16, c as i16),
                    '+' => {
                        goals[i] = true;
                        player = (r as i16, c as i16);
                    }
                    _ => {}
                }
            }
        }
        self.width = width;
        self.height = height;
        self.walls = walls;
        self.goals = goals;
        self.boxes = boxes;
        self.player = player;
        self.moves = 0;
        self.history.clear();
        self.all_solved = false;
    }

    fn idx(&self, r: i16, c: i16) -> usize {
        (r * self.width + c) as usize
    }

    fn in_bounds(&self, r: i16, c: i16) -> bool {
        r >= 0 && r < self.height && c >= 0 && c < self.width
    }

    fn is_wall(&self, r: i16, c: i16) -> bool {
        self.walls[self.idx(r, c)]
    }

    fn is_box(&self, r: i16, c: i16) -> bool {
        self.boxes[self.idx(r, c)]
    }

    fn push_history(&mut self) {
        self.history.push(Snapshot {
            player: self.player,
            boxes: self.boxes.clone(),
            moves: self.moves,
        });
    }

    /// Attempt a move by `(dr, dc)`. Into a wall (or off the map) does nothing.
    /// Into a box pushes it one cell in the same direction, but only when the
    /// square beyond is empty floor/goal — otherwise the move is blocked and the
    /// board is untouched. A successful move records an undo snapshot.
    pub fn move_by(&mut self, dr: i16, dc: i16) {
        if self.all_solved {
            return;
        }
        let (pr, pc) = self.player;
        let (tr, tc) = (pr + dr, pc + dc);
        if !self.in_bounds(tr, tc) || self.is_wall(tr, tc) {
            return;
        }
        if self.is_box(tr, tc) {
            let (br, bc) = (tr + dr, tc + dc);
            if !self.in_bounds(br, bc) || self.is_wall(br, bc) || self.is_box(br, bc) {
                return; // blocked: wall or a second box behind it
            }
            self.push_history();
            let ti = self.idx(tr, tc);
            let bi = self.idx(br, bc);
            self.boxes[ti] = false;
            self.boxes[bi] = true;
            self.player = (tr, tc);
            self.moves += 1;
        } else {
            self.push_history();
            self.player = (tr, tc);
            self.moves += 1;
        }
    }

    /// Undo the most recent move, if any.
    pub fn undo(&mut self) {
        if let Some(s) = self.history.pop() {
            self.player = s.player;
            self.boxes = s.boxes;
            self.moves = s.moves;
        }
    }

    /// Reload the current level from scratch.
    pub fn reset(&mut self) {
        self.load_level(self.level);
    }

    /// Advance to the next level once the current one is solved; on the last
    /// level this flags every level cleared.
    pub fn next_level(&mut self) {
        if !self.is_solved() {
            return;
        }
        if self.level + 1 < NUM_LEVELS {
            self.load_level(self.level + 1);
        } else {
            self.all_solved = true;
        }
    }

    /// A level is solved when every box sits on a goal (and there is a box).
    pub fn is_solved(&self) -> bool {
        let mut any = false;
        for i in 0..self.boxes.len() {
            if self.boxes[i] {
                any = true;
                if !self.goals[i] {
                    return false;
                }
            }
        }
        any
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Sokoban overlay.
pub struct Sokoban {
    game: Game,
    seed: u64,
}

impl Sokoban {
    pub fn new() -> Self {
        Sokoban {
            game: Game::new(1),
            seed: 1,
        }
    }

    /// Start the whole game over with a fresh seed (a new random first level).
    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Sokoban {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Sokoban {
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
            key!(Left) | key!('h') => self.game.move_by(0, -1),
            key!(Right) | key!('l') => self.game.move_by(0, 1),
            key!(Up) | key!('k') => self.game.move_by(-1, 0),
            key!(Down) | key!('j') => self.game.move_by(1, 0),
            key!('r') => self.game.reset(),
            key!('n') => {
                if self.game.all_solved {
                    self.restart();
                } else {
                    self.game.next_level();
                }
            }
            key!('u') => self.game.undo(),
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
        let wall_style = theme.get("ui.linenr");
        let box_style = theme.get("warning");
        let box_goal_style = theme.get("function");
        let player_style = theme.get("ui.text.focus");

        surface.clear_with(area, bg);
        if area.width < self.game.width as u16 + 4 || area.height < self.game.height as u16 + 4 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let mut head = format!(
            "Sokoban  level {}/{}  moves {}",
            self.game.level + 1,
            NUM_LEVELS,
            self.game.moves
        );
        if self.game.all_solved {
            head = "Sokoban  All levels solved!  n restart".to_string();
        } else if self.game.is_solved() {
            head.push_str("  — solved! press n");
        }
        surface.set_string(ox, area.y, &head, header_style);

        for r in 0..self.game.height {
            for c in 0..self.game.width {
                let i = self.game.idx(r, c);
                let (glyph, style): (&str, _) = if self.game.walls[i] {
                    ("▓", wall_style)
                } else if self.game.boxes[i] {
                    if self.game.goals[i] {
                        ("▢", box_goal_style)
                    } else {
                        ("▢", box_style)
                    }
                } else if (r, c) == self.game.player {
                    ("☺", player_style)
                } else if self.game.goals[i] {
                    ("·", text_style)
                } else {
                    (" ", text_style)
                };
                let x = ox + c as u16;
                let y = oy + r as u16;
                surface.set_string(x, y, glyph, style);
            }
        }

        let sy = oy + self.game.height as u16 + 1;
        surface.set_string(
            ox,
            sy,
            "hjkl/arrows move · r reset · n next · u undo · q quit",
            text_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a board directly from a map string (mirrors the level parser but
    /// keeps `level` at 0), so tests are hand-placed and deterministic.
    fn from_map(map: &str) -> Game {
        let mut g = Game {
            width: 0,
            height: 0,
            walls: Vec::new(),
            goals: Vec::new(),
            boxes: Vec::new(),
            player: (0, 0),
            level: 0,
            moves: 0,
            all_solved: false,
            history: Vec::new(),
            rng: 1,
        };
        g.parse_into(map);
        g
    }

    #[test]
    fn player_walks_into_free_floor() {
        let mut g = from_map("#####\n#@  #\n#####");
        g.move_by(0, 1);
        assert_eq!(g.player, (1, 2), "player steps right onto floor");
        assert_eq!(g.moves, 1);
    }

    #[test]
    fn pushing_a_box_into_floor_moves_both() {
        let mut g = from_map("#####\n#@$ #\n#####");
        g.move_by(0, 1);
        assert_eq!(g.player, (1, 2), "player advances onto the box's old cell");
        assert!(g.is_box(1, 3), "box is pushed one cell right");
        assert!(!g.is_box(1, 2), "box left its previous cell");
    }

    #[test]
    fn box_cannot_be_pushed_into_a_wall() {
        let mut g = from_map("####\n#@$#\n####");
        g.move_by(0, 1);
        assert_eq!(g.player, (1, 1), "player is blocked by the wedged box");
        assert!(g.is_box(1, 2), "box has not moved");
        assert_eq!(g.moves, 0, "a blocked move is not counted");
        assert!(g.history.is_empty(), "a blocked move records no undo state");
    }

    #[test]
    fn box_cannot_be_pushed_into_another_box() {
        let mut g = from_map("######\n#@$$ #\n######");
        g.move_by(0, 1);
        assert_eq!(g.player, (1, 1), "two boxes in a row cannot be shoved");
        assert!(g.is_box(1, 2) && g.is_box(1, 3), "both boxes stay put");
        assert_eq!(g.moves, 0);
    }

    #[test]
    fn win_when_all_boxes_cover_goals() {
        let mut g = from_map("#####\n#@$.#\n#####");
        assert!(!g.is_solved(), "not solved while the box is off its goal");
        g.move_by(0, 1);
        assert!(g.is_box(1, 3), "box was pushed onto the goal");
        assert!(g.is_solved(), "every box now sits on a goal");
    }
}
