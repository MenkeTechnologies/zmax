//! Life — the zmax port of GNU Emacs `life`, Conway's Game of Life.
//!
//! A fixed logical grid evolves by Conway's rules. Emacs animates it on a timer;
//! here each generation is stepped with `SPC`/`RET` so it is watchable without a
//! tick loop. `n` seeds a new random soup, `r` reloads the demo pattern, `c`
//! clears, `q`/`Esc` quits. The grid step is pure and unit-tested (keys parse
//! into a `life` keymap mode via `scripts/gen_port_report.py`).

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const COLS: usize = 60;
const ROWS: usize = 24;

/// The pure Life grid. Bounded (cells off the edge count as dead). Unit-tested.
#[derive(Clone)]
pub struct Grid {
    cells: Vec<bool>,
    gen: u64,
}

impl Grid {
    pub fn blank() -> Self {
        Grid {
            cells: vec![false; COLS * ROWS],
            gen: 0,
        }
    }

    fn idx(r: usize, c: usize) -> usize {
        r * COLS + c
    }

    pub fn get(&self, r: usize, c: usize) -> bool {
        self.cells[Self::idx(r, c)]
    }

    fn set(&mut self, r: usize, c: usize, on: bool) {
        self.cells[Self::idx(r, c)] = on;
    }

    fn live_neighbours(&self, r: usize, c: usize) -> usize {
        let mut n = 0;
        for dr in [-1isize, 0, 1] {
            for dc in [-1isize, 0, 1] {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let rr = r as isize + dr;
                let cc = c as isize + dc;
                if rr >= 0
                    && rr < ROWS as isize
                    && cc >= 0
                    && cc < COLS as isize
                    && self.get(rr as usize, cc as usize)
                {
                    n += 1;
                }
            }
        }
        n
    }

    /// Advance one generation by Conway's rules: a live cell with 2–3 live
    /// neighbours survives; a dead cell with exactly 3 is born; all else die.
    pub fn step(&mut self) {
        let mut next = vec![false; COLS * ROWS];
        for r in 0..ROWS {
            for c in 0..COLS {
                let n = self.live_neighbours(r, c);
                next[Self::idx(r, c)] =
                    matches!((self.get(r, c), n), (true, 2) | (true, 3) | (false, 3));
            }
        }
        self.cells = next;
        self.gen += 1;
    }

    pub fn population(&self) -> usize {
        self.cells.iter().filter(|&&on| on).count()
    }

    /// A small demo: a glider heading down-right plus a blinker.
    pub fn demo() -> Self {
        let mut g = Grid::blank();
        for (r, c) in [(1, 2), (2, 3), (3, 1), (3, 2), (3, 3)] {
            g.set(r, c, true);
        }
        for (r, c) in [(10, 20), (10, 21), (10, 22)] {
            g.set(r, c, true);
        }
        g
    }

    /// Deterministic pseudo-random soup from `seed` (~30% alive).
    pub fn soup(seed: u64) -> Self {
        let mut g = Grid::blank();
        let mut s = seed | 1;
        for cell in g.cells.iter_mut() {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *cell = (s >> 40) % 10 < 3;
        }
        g
    }
}

/// The interactive Life overlay.
pub struct Life {
    grid: Grid,
    seed: u64,
}

impl Life {
    pub fn new() -> Self {
        Life {
            grid: Grid::demo(),
            seed: 1,
        }
    }
}

impl Default for Life {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Life {
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
            key!(' ') | key!(Enter) => self.grid.step(),
            key!('n') => {
                self.seed = self.seed.wrapping_add(1);
                self.grid = Grid::soup(self.seed);
            }
            key!('r') => self.grid = Grid::demo(),
            key!('c') => self.grid = Grid::blank(),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let cell_style = theme.get("ui.text.focus");

        surface.clear_with(area, bg);
        if area.width < 24 || area.height < 8 {
            return;
        }
        let ox = area.x + 1;
        let oy = area.y + 1;
        surface.set_string(ox, area.y, "Life — Conway's Game of Life", header_style);

        let rows = ROWS.min(area.height.saturating_sub(3) as usize);
        let cols = COLS.min(area.width.saturating_sub(2) as usize);
        for r in 0..rows {
            let mut line = String::with_capacity(cols);
            for c in 0..cols {
                line.push(if self.grid.get(r, c) { '█' } else { ' ' });
            }
            surface.set_string(ox, oy + r as u16, &line, cell_style);
        }

        let sy = oy + rows as u16 + 1;
        let status = format!(
            "gen {}   pop {}   ·  SPC step  n random  r demo  c clear  q quit",
            self.grid.gen,
            self.grid.population()
        );
        surface.set_string(ox, sy, &status, text_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blinker_oscillates_with_period_two() {
        let mut g = Grid::blank();
        // Horizontal blinker.
        for c in [10, 11, 12] {
            g.set(5, c, true);
        }
        let start = g.clone();
        g.step();
        // Now vertical.
        assert!(g.get(4, 11) && g.get(5, 11) && g.get(6, 11));
        assert!(!g.get(5, 10) && !g.get(5, 12));
        g.step();
        // Back to horizontal — period 2.
        for r in 0..ROWS {
            for c in 0..COLS {
                assert_eq!(g.get(r, c), start.get(r, c), "differs at {r},{c}");
            }
        }
    }

    #[test]
    fn block_is_still_life() {
        let mut g = Grid::blank();
        for (r, c) in [(3, 3), (3, 4), (4, 3), (4, 4)] {
            g.set(r, c, true);
        }
        let before = g.population();
        g.step();
        assert_eq!(g.population(), before);
        assert!(g.get(3, 3) && g.get(3, 4) && g.get(4, 3) && g.get(4, 4));
    }

    #[test]
    fn lone_cell_dies() {
        let mut g = Grid::blank();
        g.set(8, 8, true);
        g.step();
        assert_eq!(g.population(), 0);
    }
}
