//! Animate — the zmax port of GNU Emacs `animate` (play/animate.el).
//!
//! Each character of a string starts at a random screen position and slides in
//! parallel to its destination over `animate-n-steps` (20) frames, per
//! `animate-initialize`/`animate-step`. `animate-birthday-present` scripts a
//! sequence of `animate-string` calls (the "Happy Birthday / You are my
//! sunshine" song); each string swoops in, then settles permanently while the
//! next begins. Self-animates via `zmax_event::request_redraw`; any key closes
//! the overlay. The placement math is pure and unit-tested.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};

const AREA_W: usize = 80;
const AREA_H: usize = 24;
/// `animate-n-steps`: intermediate positions before the final placement.
const N_STEPS: usize = 20;

/// One animated character: its source and destination cells (row, col).
#[derive(Clone, Copy)]
struct AChar {
    ch: char,
    sy: i16,
    sx: i16,
    dy: i16,
    dx: i16,
}

/// Interpolate a character `fraction` of the way from start to destination.
/// `fraction` 0.0 == start cell, 1.0 == destination cell. Floors each axis,
/// matching emacs `animate-place-char` (`move-to-column` on `(floor hpos)` and
/// integer `forward-line` for the row).
fn place(c: &AChar, fraction: f64) -> (i16, i16) {
    let remains = 1.0 - fraction;
    let y = (remains * c.sy as f64 + fraction * c.dy as f64).floor() as i16;
    let x = (remains * c.sx as f64 + fraction * c.dx as f64).floor() as i16;
    (y, x)
}

fn rand(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

/// `animate-initialize`: give each character of `string` a random start cell and
/// a destination on line `vpos`, the i-th char landing `hpos + i` columns in.
fn init_job(string: &str, vpos: i16, hpos: i16, rng: &mut u64) -> Vec<AChar> {
    string
        .chars()
        .enumerate()
        .map(|(i, ch)| AChar {
            ch,
            sy: (rand(rng) % AREA_H as u64) as i16,
            sx: (rand(rng) % (AREA_W as u64 - 1)) as i16,
            dy: vpos,
            dx: hpos + i as i16,
        })
        .collect()
}

/// The scripted `animate-birthday-present` (no NAME): each entry is
/// (string, vpos, optional hpos). `None` hpos centres the string horizontally.
const BIRTHDAY: &[(&str, i16, Option<i16>)] = &[
    ("Happy Birthday", 6, None),
    ("You are my sunshine,", 10, Some(30)),
    ("My only sunshine.", 11, Some(30)),
    ("I'm awful sad that", 12, Some(30)),
    ("You've moved away.", 13, Some(30)),
    ("Let's talk together", 15, Some(30)),
    ("And love more deeply.", 16, Some(30)),
    ("Please bring back", 17, Some(30)),
    ("my sunshine", 18, Some(34)),
    ("to stay!", 19, Some(34)),
];

/// The interactive Animate overlay.
pub struct Animate {
    /// Baked cells of strings that have finished swooping in.
    settled: Vec<(i16, i16, char)>,
    jobs: Vec<Vec<AChar>>,
    current: usize,
    step: usize,
    last: Option<Instant>,
    interval: Duration,
}

impl Animate {
    /// The birthday-present sequence (the interactive `animate-birthday-present`
    /// with no NAME argument).
    pub fn birthday_present() -> Self {
        let mut rng: u64 = 0x9e3779b97f4a7c15;
        let jobs = BIRTHDAY
            .iter()
            .map(|&(s, vpos, hpos)| {
                // nil hpos -> centre: max(0, (window-width - len) / 2).
                let hpos = hpos.unwrap_or_else(|| {
                    let len = s.chars().count() as i16;
                    ((AREA_W as i16 - len) / 2).max(0)
                });
                init_job(s, vpos, hpos, &mut rng)
            })
            .collect();
        Animate {
            settled: Vec::new(),
            jobs,
            current: 0,
            step: 0,
            last: None,
            interval: Duration::from_millis(60),
        }
    }

    /// Bake the current job at its final positions and move to the next.
    fn settle_current(&mut self) {
        for c in &self.jobs[self.current] {
            self.settled.push((c.dy, c.dx, c.ch));
        }
        self.current += 1;
        self.step = 0;
    }
}

impl Component for Animate {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
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
        if self.current < self.jobs.len() {
            let due = match self.last {
                Some(t) => now.duration_since(t) >= self.interval,
                None => true,
            };
            if due {
                self.last = Some(now);
                self.step += 1;
                if self.step > N_STEPS {
                    self.settle_current();
                }
            }
            zmax_event::request_redraw();
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let focus_style = theme.get("ui.text.focus");
        surface.clear_with(area, bg);

        let put = |surface: &mut Surface, y: i16, x: i16, ch: char, style| {
            if y < 0 || x < 0 || y >= area.height as i16 || x >= area.width as i16 {
                return;
            }
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            surface.set_string(area.x + x as u16, area.y + y as u16, s, style);
        };

        for &(y, x, ch) in &self.settled {
            put(surface, y, x, ch, text_style);
        }
        if self.current < self.jobs.len() {
            let fraction = self.step as f64 / N_STEPS as f64;
            for c in &self.jobs[self.current] {
                let (y, x) = place(c, fraction);
                put(surface, y, x, c.ch, focus_style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_zero_is_start_one_is_destination() {
        let c = AChar {
            ch: 'x',
            sy: 3,
            sx: 17,
            dy: 6,
            dx: 40,
        };
        assert_eq!(place(&c, 0.0), (3, 17), "fraction 0 -> start cell");
        assert_eq!(place(&c, 1.0), (6, 40), "fraction 1 -> destination cell");
    }

    #[test]
    fn destinations_lay_out_the_string_left_to_right() {
        let mut rng = 42u64;
        let job = init_job("abc", 5, 10, &mut rng);
        // The i-th char must end up hpos + i, on line vpos.
        assert_eq!((job[0].dy, job[0].dx), (5, 10));
        assert_eq!((job[1].dy, job[1].dx), (5, 11));
        assert_eq!((job[2].dy, job[2].dx), (5, 12));
    }

    #[test]
    fn birthday_settles_every_job() {
        let mut a = Animate::birthday_present();
        // Drive enough ticks to finish all jobs: each takes N_STEPS+1 settles.
        for _ in 0..(a.jobs.len() * (N_STEPS + 2)) {
            if a.current < a.jobs.len() {
                a.step += 1;
                if a.step > N_STEPS {
                    a.settle_current();
                }
            }
        }
        assert_eq!(a.current, a.jobs.len(), "all strings settled");
        // "Happy Birthday" (14) is the first baked string.
        assert!(a.settled.len() >= 14);
    }
}
