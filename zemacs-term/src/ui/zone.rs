//! Zone — the zemacs port of GNU Emacs `zone`, the screen-saver.
//!
//! A tick-driven "drip" animation (a rain of glyphs cascading down the frame),
//! in the spirit of Emacs' `zone-pgm-*` programs. Any key stops it and closes
//! the overlay. It self-animates via `zemacs_event::request_redraw`, idling once
//! closed. The rain state is pure and unit-tested (keys parse into a `zone`
//! keymap mode by `scripts/gen_port_report.py`).

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
};

const COLS: usize = 80;

/// The pure rain: one falling "drop" head-row per column, plus a trail length.
/// `None` = that column is currently empty. No I/O — unit-tested.
#[derive(Clone)]
pub struct Rain {
    pub heads: Vec<Option<i16>>,
    pub trail: Vec<i16>,
    pub rows: i16,
    rng: u64,
}

impl Rain {
    pub fn new(cols: usize, rows: i16, seed: u64) -> Self {
        let mut r = Rain {
            heads: vec![None; cols],
            trail: vec![0; cols],
            rows,
            rng: seed | 1,
        };
        // Stagger initial drops so the field fills in over the first frames.
        for c in 0..cols {
            let v = r.rand();
            if v % 3 == 0 {
                r.heads[c] = Some(-((v % rows.max(1) as u64) as i16));
                r.trail[c] = 3 + (v % 6) as i16;
            }
        }
        r
    }

    fn rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng >> 33
    }

    /// Advance every drop one row; finished drops (past the bottom + trail) are
    /// randomly respawned at the top, so the rain never stops.
    pub fn step(&mut self) {
        for c in 0..self.heads.len() {
            match self.heads[c] {
                Some(h) => {
                    let nh = h + 1;
                    if nh - self.trail[c] > self.rows {
                        self.heads[c] = None; // fully off the bottom
                    } else {
                        self.heads[c] = Some(nh);
                    }
                }
                None => {
                    if self.rand() % 8 == 0 {
                        self.heads[c] = Some(0);
                        self.trail[c] = 3 + (self.rand() % 8) as i16;
                    }
                }
            }
        }
    }

    /// The glyph brightness at `(row, col)`: 2 = head, 1 = trail, 0 = empty.
    pub fn intensity(&self, row: i16, col: usize) -> u8 {
        match self.heads.get(col).copied().flatten() {
            Some(h) if row == h => 2,
            Some(h) if row < h && row > h - self.trail[col] && row >= 0 => 1,
            _ => 0,
        }
    }
}

/// The interactive Zone overlay.
pub struct Zone {
    rain: Rain,
    last: Option<Instant>,
    interval: Duration,
}

impl Zone {
    pub fn new() -> Self {
        Zone {
            rain: Rain::new(COLS, 24, 1),
            last: None,
            interval: Duration::from_millis(70),
        }
    }
}

impl Default for Zone {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Zone {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        // Any key stops the screen-saver (like real `zone`).
        if let Event::Key(_) = event {
            let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
                compositor.pop();
            });
            return EventResult::Consumed(Some(close));
        }
        EventResult::Ignored(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let now = Instant::now();
        match self.last {
            Some(t) if now.duration_since(t) >= self.interval => {
                self.rain.step();
                self.last = Some(now);
            }
            None => self.last = Some(now),
            _ => {}
        }
        zemacs_event::request_redraw();

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let head_style = theme.get("ui.text.focus");
        let trail_style = theme.get("function");
        let dim_style = theme.get("ui.linenr");

        surface.clear_with(area, bg);
        // Resize the rain to the frame if needed.
        if self.rain.heads.len() != area.width as usize || self.rain.rows != area.height as i16 {
            self.rain = Rain::new(area.width as usize, area.height as i16, 1);
        }
        let glyphs = ['0', '1', '#', '$', '%', '&', '*', '+', '='];
        for r in 0..area.height as i16 {
            for c in 0..area.width as usize {
                let g = glyphs[(r as usize + c) % glyphs.len()];
                let mut buf = [0u8; 4];
                let s = g.encode_utf8(&mut buf);
                match self.rain.intensity(r, c) {
                    2 => surface.set_string(area.x + c as u16, area.y + r as u16, s, head_style),
                    1 => surface.set_string(area.x + c as u16, area.y + r as u16, s, trail_style),
                    _ => {}
                }
            }
        }
        let _ = dim_style;
        surface.set_string(area.x, area.y, "zone — press any key to stop", head_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_falls_one_row_per_step() {
        let mut r = Rain {
            heads: vec![Some(0)],
            trail: vec![3],
            rows: 10,
            rng: 1,
        };
        r.step();
        assert_eq!(r.heads[0], Some(1));
        r.step();
        assert_eq!(r.heads[0], Some(2));
    }

    #[test]
    fn a_drop_clears_after_falling_off_the_bottom() {
        let mut r = Rain {
            heads: vec![Some(10)],
            trail: vec![1],
            rows: 10,
            rng: 1,
        };
        // head 10, trail 1: nh=11, 11-1=10 not > 10 → still on. Next: nh=12, 12-1=11>10 → clears.
        r.step();
        assert_eq!(r.heads[0], Some(11));
        r.step();
        assert_eq!(r.heads[0], None);
    }

    #[test]
    fn intensity_marks_head_and_trail() {
        let r = Rain {
            heads: vec![Some(5)],
            trail: vec![3],
            rows: 10,
            rng: 1,
        };
        assert_eq!(r.intensity(5, 0), 2, "head is brightest");
        assert_eq!(r.intensity(4, 0), 1, "just above the head is trail");
        assert_eq!(r.intensity(3, 0), 1);
        assert_eq!(r.intensity(2, 0), 0, "beyond the trail is empty");
        assert_eq!(r.intensity(6, 0), 0, "below the head is empty");
    }
}
