//! Hanoi — the zmax port of GNU Emacs `hanoi`, the Towers of Hanoi solver.
//!
//! Rings start stacked on the left peg and the optimal solution is precomputed;
//! `SPC`/`RET` steps one ring move (Emacs animates it — here it is stepped so it
//! is watchable without a timer), `+`/`-` change the ring count, `r` restarts,
//! `q`/`Esc` quits. The move generator and peg state are pure and unit-tested
//! (keys parse into a `hanoi` keymap mode via `scripts/gen_port_report.py`).

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The pure Hanoi state: three pegs (each a bottom-to-top stack of ring sizes)
/// and the optimal move list. No I/O — unit-tested.
#[derive(Clone)]
pub struct Game {
    pub pegs: [Vec<usize>; 3],
    pub moves: Vec<(usize, usize)>,
    pub idx: usize,
    pub rings: usize,
}

/// Optimal Towers-of-Hanoi move sequence: move `n` rings from `from` to `to`
/// using `via`, as a flat list of `(from, to)` single-ring moves. 2^n - 1 moves.
pub fn solve(n: usize, from: usize, to: usize, via: usize, out: &mut Vec<(usize, usize)>) {
    if n == 0 {
        return;
    }
    solve(n - 1, from, via, to, out);
    out.push((from, to));
    solve(n - 1, via, to, from, out);
}

impl Game {
    pub fn new(rings: usize) -> Self {
        let rings = rings.clamp(1, 9);
        let mut moves = Vec::new();
        solve(rings, 0, 2, 1, &mut moves);
        Game {
            pegs: [(1..=rings).rev().collect(), Vec::new(), Vec::new()],
            moves,
            idx: 0,
            rings,
        }
    }

    /// Apply the next optimal move. Returns false when the puzzle is solved.
    pub fn step(&mut self) -> bool {
        let Some(&(from, to)) = self.moves.get(self.idx) else {
            return false;
        };
        if let Some(ring) = self.pegs[from].pop() {
            self.pegs[to].push(ring);
        }
        self.idx += 1;
        true
    }

    pub fn solved(&self) -> bool {
        self.idx >= self.moves.len()
    }
}

/// The interactive Hanoi overlay.
pub struct Hanoi {
    game: Game,
}

impl Hanoi {
    pub fn new() -> Self {
        Hanoi { game: Game::new(3) }
    }
}

impl Default for Hanoi {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Hanoi {
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
            key!(' ') | key!(Enter) => {
                self.game.step();
            }
            key!('+') | key!('=') => self.game = Game::new(self.game.rings + 1),
            key!('-') => self.game = Game::new(self.game.rings.saturating_sub(1).max(1)),
            key!('r') => self.game = Game::new(self.game.rings),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let ring_style = theme.get("ui.text.focus");
        let post_style = theme.get("ui.linenr");

        surface.clear_with(area, bg);
        if area.width < 40 || area.height < 14 {
            return;
        }
        let ox = area.x + 2;
        let oy = area.y + 2;
        surface.set_string(ox, area.y, "Towers of Hanoi", header_style);

        let n = self.game.rings;
        let max_w = (2 * n + 1) as u16; // widest ring
        let peg_gap = max_w + 3;
        let base_y = oy + (n as u16) + 1;

        for (p, stack) in self.game.pegs.iter().enumerate() {
            let cx = ox + (p as u16) * peg_gap + max_w / 2;
            // The post.
            for y in oy..=base_y {
                surface.set_string(cx, y, "│", post_style);
            }
            // Rings, bottom-up.
            for (level, &ring) in stack.iter().enumerate() {
                let y = base_y - 1 - (level as u16);
                let w = (2 * ring + 1) as u16;
                let x = cx.saturating_sub(w / 2);
                let bar: String = "▓".repeat(w as usize);
                surface.set_string(x, y, &bar, ring_style);
            }
        }

        let sy = base_y + 2;
        let status = if self.game.solved() {
            format!(
                "Solved in {} moves.  +/- rings  r restart  q quit",
                self.game.moves.len()
            )
        } else {
            format!(
                "move {}/{}   ·  SPC step  +/- rings  r restart  q quit",
                self.game.idx,
                self.game.moves.len()
            )
        };
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_count_is_two_to_the_n_minus_one() {
        for n in 1..=8 {
            let mut m = Vec::new();
            solve(n, 0, 2, 1, &mut m);
            assert_eq!(m.len(), (1usize << n) - 1, "n={n}");
        }
    }

    #[test]
    fn stepping_through_solves_the_puzzle() {
        let mut g = Game::new(4);
        assert_eq!(g.pegs[0].len(), 4);
        while g.step() {}
        assert!(g.solved());
        // All rings on the destination peg, largest at the bottom.
        assert_eq!(g.pegs[2], vec![4, 3, 2, 1]);
        assert!(g.pegs[0].is_empty() && g.pegs[1].is_empty());
    }

    #[test]
    fn every_move_respects_the_no_larger_on_smaller_rule() {
        let mut g = Game::new(5);
        while g.step() {
            for peg in &g.pegs {
                for w in peg.windows(2) {
                    assert!(w[0] > w[1], "a larger ring sat on a smaller one: {peg:?}");
                }
            }
        }
    }
}
