//! Landmark — a "find the hidden tree" grid game for zmax, an homage to GNU
//! Emacs `landmark.el`.
//!
//! A single landmark tree is hidden somewhere on the grid. You steer a robot one
//! cell at a time with the arrows or `hjkl`; after every step the robot's "sense
//! of smell" reports whether you got `warmer` or `colder` and a heat label
//! (freezing/cold/warm/hot/burning) coloured on a ramp. Reach the tree to win.
//! `n` lays out a fresh board and `q`/`Esc` quits. Like Minesweeper this one is
//! turn-based: nothing animates, so there is no frame loop — the board only
//! changes in response to a key. The board logic is pure and unit-tested (the
//! tree and robot are placed by a small LCG so a given seed is reproducible).

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const GW: usize = 16;
const GH: usize = 12;

/// The pure landmark board. No I/O, no timing — unit-tested. The tree and robot
/// are placed with the same LCG the other games use, so `Game::new(seed)` is
/// deterministic.
#[derive(Clone)]
pub struct Game {
    /// The hidden landmark, as `(row, col)`.
    tree: (usize, usize),
    /// The robot the player steers, as `(row, col)`.
    robot: (usize, usize),
    /// How many steps the player has taken.
    moves: u32,
    /// Robot→tree distance before the most recent move (for the warmer/colder
    /// readout).
    prev_dist: i32,
    rng: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            tree: (0, 0),
            robot: (0, 0),
            moves: 0,
            prev_dist: 0,
            rng: seed | 1,
        };
        g.place();
        g
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Drop the tree on a random cell and the robot on a different random cell,
    /// then seed the distance readout.
    fn place(&mut self) {
        let tr = (self.rand() % GH as u64) as usize;
        let tc = (self.rand() % GW as u64) as usize;
        self.tree = (tr, tc);
        // Reject the tree's own cell so the robot never spawns on a win.
        loop {
            let rr = (self.rand() % GH as u64) as usize;
            let rc = (self.rand() % GW as u64) as usize;
            if (rr, rc) != self.tree {
                self.robot = (rr, rc);
                break;
            }
        }
        self.prev_dist = self.distance();
    }

    pub fn robot(&self) -> (usize, usize) {
        self.robot
    }

    pub fn tree(&self) -> (usize, usize) {
        self.tree
    }

    pub fn moves(&self) -> u32 {
        self.moves
    }

    /// Manhattan distance from the robot to the hidden tree.
    pub fn distance(&self) -> i32 {
        let dr = (self.robot.0 as i32 - self.tree.0 as i32).abs();
        let dc = (self.robot.1 as i32 - self.tree.1 as i32).abs();
        dr + dc
    }

    /// The discrete heat label for the current distance, hotter as the robot
    /// nears the tree.
    pub fn heat(&self) -> &'static str {
        match self.distance() {
            0..=2 => "burning",
            3..=5 => "hot",
            6..=9 => "warm",
            10..=14 => "cold",
            _ => "freezing",
        }
    }

    /// Whether the robot is standing on the landmark.
    pub fn found(&self) -> bool {
        self.robot == self.tree
    }

    /// The warmer/colder trend of the most recent move.
    fn trend(&self) -> &'static str {
        if self.moves == 0 {
            "sniffing"
        } else if self.distance() < self.prev_dist {
            "warmer"
        } else if self.distance() > self.prev_dist {
            "colder"
        } else {
            "no change"
        }
    }

    /// Step the robot by `(dr, dc)`, clamped to the grid; updates the distance
    /// readout and move count. A won board ignores further moves.
    pub fn move_robot(&mut self, dr: i32, dc: i32) {
        if self.found() {
            return;
        }
        self.prev_dist = self.distance();
        let r = (self.robot.0 as i32 + dr).clamp(0, GH as i32 - 1) as usize;
        let c = (self.robot.1 as i32 + dc).clamp(0, GW as i32 - 1) as usize;
        self.robot = (r, c);
        self.moves += 1;
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(1)
    }
}

/// The interactive Landmark overlay.
pub struct Landmark {
    game: Game,
    seed: u64,
}

impl Landmark {
    pub fn new() -> Self {
        Landmark {
            game: Game::new(1),
            seed: 1,
        }
    }

    fn restart(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.game = Game::new(self.seed);
    }
}

impl Default for Landmark {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Landmark {
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
            key!(Left) | key!('h') => self.game.move_robot(0, -1),
            key!(Right) | key!('l') => self.game.move_robot(0, 1),
            key!(Up) | key!('k') => self.game.move_robot(-1, 0),
            key!(Down) | key!('j') => self.game.move_robot(1, 0),
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
        let dim = theme.get("ui.linenr");
        let robot_style = theme.get("ui.selection");
        let tree_style = theme.get("function");
        let warning_style = theme.get("warning");
        let error_style = theme.get("error");

        surface.clear_with(area, bg);
        // Each cell is drawn two columns wide for legibility, with a box border.
        if area.width < (GW as u16) * 2 + 6 || area.height < (GH as u16) + 6 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;

        let header = if self.game.found() {
            format!("Landmark  found in {} moves!", self.game.moves())
        } else {
            format!("Landmark  moves {}", self.game.moves())
        };
        surface.set_string(ox, area.y, &header, header_style);

        // Border around the grid.
        let left = ox - 1;
        let right = ox + (GW as u16) * 2 - 1;
        let top = oy - 1;
        let bottom = oy + GH as u16;
        for c in left..=right {
            surface.set_string(c, top, "─", dim);
            surface.set_string(c, bottom, "─", dim);
        }
        for r in top..=bottom {
            surface.set_string(left, r, "│", dim);
            surface.set_string(right, r, "│", dim);
        }
        surface.set_string(left, top, "┌", dim);
        surface.set_string(right, top, "┐", dim);
        surface.set_string(left, bottom, "└", dim);
        surface.set_string(right, bottom, "┘", dim);

        for r in 0..GH {
            for c in 0..GW {
                let (glyph, style) = if (r, c) == self.game.robot() && !self.game.found() {
                    ("☗", robot_style)
                } else if (r, c) == self.game.tree() && self.game.found() {
                    ("♣", tree_style)
                } else {
                    ("·", dim)
                };
                let x = ox + (c as u16) * 2;
                let y = oy + r as u16;
                surface.set_string(x, y, glyph, style);
            }
        }

        let sy = oy + GH as u16 + 1;
        surface.set_string(ox, sy, "hjkl/arrows move · n new · q quit", text_style);

        // Heat readout on the ramp ui.linenr→ui.text→function→warning→error.
        let heat_style = match self.game.heat() {
            "burning" => error_style,
            "hot" => warning_style,
            "warm" => tree_style,
            "cold" => text_style,
            _ => dim,
        };
        let readout = if self.game.found() {
            "♣ landmark reached — press n for a new board".to_string()
        } else {
            format!("smell: {} · {}", self.game.heat(), self.game.trend())
        };
        surface.set_string(ox, sy + 1, &readout, heat_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board with the robot and tree placed by hand, ready for deterministic
    /// steering tests.
    fn at(robot: (usize, usize), tree: (usize, usize)) -> Game {
        let mut g = Game {
            tree,
            robot,
            moves: 0,
            prev_dist: 0,
            rng: 1,
        };
        g.prev_dist = g.distance();
        g
    }

    #[test]
    fn moving_toward_the_tree_warms_up() {
        let mut g = at((5, 5), (5, 8));
        let before = g.distance();
        g.move_robot(0, 1); // step right, toward the tree
        assert!(
            g.distance() < before,
            "distance shrinks moving toward the tree"
        );
        assert_eq!(g.trend(), "warmer");
    }

    #[test]
    fn moving_away_cools_down() {
        let mut g = at((5, 5), (5, 8));
        let before = g.distance();
        g.move_robot(0, -1); // step left, away from the tree
        assert!(
            g.distance() > before,
            "distance grows moving away from the tree"
        );
        assert_eq!(g.trend(), "colder");
    }

    #[test]
    fn robot_stays_in_bounds_at_the_edges() {
        let mut g = at((0, 0), (5, 5));
        g.move_robot(-1, -1); // shove past the top-left corner
        assert_eq!(g.robot(), (0, 0));
        let mut g = at((GH - 1, GW - 1), (0, 0));
        g.move_robot(1, 1); // shove past the bottom-right corner
        assert_eq!(g.robot(), (GH - 1, GW - 1));
    }

    #[test]
    fn stepping_onto_the_tree_wins() {
        let mut g = at((5, 5), (5, 6));
        assert!(!g.found());
        g.move_robot(0, 1); // step onto the landmark
        assert!(g.found(), "the robot standing on the tree wins");
        assert_eq!(g.heat(), "burning", "distance zero is the hottest reading");
    }

    #[test]
    fn a_new_board_can_place_the_tree_elsewhere() {
        let base = Game::new(1);
        // Some other seed lays the tree on a different cell (and always in bounds).
        let mut found_elsewhere = false;
        for s in 2..50 {
            let g = Game::new(s);
            assert!(g.tree().0 < GH && g.tree().1 < GW, "tree stays on the grid");
            if g.tree() != base.tree() {
                found_elsewhere = true;
                break;
            }
        }
        assert!(
            found_elsewhere,
            "a fresh board can hide the tree somewhere new"
        );
    }
}
