//! Blackbox — the zemacs port of GNU Emacs `blackbox`, the ray-tracing puzzle.
//!
//! Hidden balls sit in an 8×8 box. Fire a ray from a border point and read how
//! it behaves to deduce where the balls are: a ray absorbed by a ball is a Hit
//! (`H`); a ray that bounces straight back out its entry is a Reflection (`R`,
//! caused by a ball beside the entry or two deflections cancelling); otherwise
//! it exits elsewhere and its entry/exit points share a number. Move the cursor
//! with the arrows or `hjkl`; on a border cell `SPC` fires, inside the box `SPC`
//! marks a ball guess; `c` reveals the solution, `n` deals a new board, `q`
//! quits. The ray tracer is pure and unit-tested (keys parse into a `blackbox`
//! keymap mode via `scripts/gen_port_report.py`).

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const N: i16 = 8;
const BALLS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

/// A ray outcome: absorbed by a ball, or emerging at a border point (equal to
/// the entry point ⇒ a reflection).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ray {
    Hit,
    Exit(Side, i16),
}

/// The pure box: ball positions and the ray tracer. No I/O — unit-tested.
#[derive(Clone)]
pub struct Grid {
    pub balls: [[bool; 8]; 8],
}

fn inside(r: i16, c: i16) -> bool {
    (0..N).contains(&r) && (0..N).contains(&c)
}

impl Grid {
    /// Deterministically place `BALLS` distinct balls from `seed`.
    pub fn from_seed(seed: u64) -> Self {
        let mut s = seed | 1;
        let mut balls = [[false; 8]; 8];
        let mut placed = 0;
        while placed < BALLS {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = ((s >> 40) % N as u64) as usize;
            let c = ((s >> 20) % N as u64) as usize;
            if !balls[r][c] {
                balls[r][c] = true;
                placed += 1;
            }
        }
        Grid { balls }
    }

    fn ball(&self, r: i16, c: i16) -> bool {
        inside(r, c) && self.balls[r as usize][c as usize]
    }

    /// The (outside cell, inward direction) a border firing point starts from.
    fn start(side: Side, idx: i16) -> (i16, i16, i16, i16) {
        match side {
            Side::Top => (-1, idx, 1, 0),
            Side::Bottom => (N, idx, -1, 0),
            Side::Left => (idx, -1, 0, 1),
            Side::Right => (idx, N, 0, -1),
        }
    }

    fn edge_of(r: i16, c: i16) -> (Side, i16) {
        if r < 0 {
            (Side::Top, c)
        } else if r >= N {
            (Side::Bottom, c)
        } else if c < 0 {
            (Side::Left, r)
        } else {
            (Side::Right, r)
        }
    }

    /// Trace a ray fired from border point `(side, idx)`. Deflection turns the
    /// ray 90° away from a ball flanking the cell it is about to enter; two such
    /// flanking balls reverse it; a ball flanking the very entry reflects it
    /// immediately; a ball directly ahead absorbs it.
    pub fn trace(&self, side: Side, idx: i16) -> Ray {
        let (mut r, mut c, mut dr, mut dc) = Self::start(side, idx);
        let mut first = true;
        for _ in 0..(4 * N * N) {
            // The two cells flanking the cell directly ahead (front ± perpendicular).
            let (lr, lc) = (r + dr - dc, c + dc + dr); // + left(d)  = (-dc, dr)
            let (rr, rc) = (r + dr + dc, c + dc - dr); // + right(d) = ( dc,-dr)
            let l = self.ball(lr, lc);
            let rt = self.ball(rr, rc);
            if first && (l || rt) {
                return Ray::Exit(side, idx); // aimed beside an edge ball → reflect
            }
            first = false;
            if l && rt {
                dr = -dr;
                dc = -dc; // two deflections cancel → reflect
                continue;
            } else if l {
                let (ndr, ndc) = (dc, -dr); // deflect right, away from the left ball
                dr = ndr;
                dc = ndc;
                continue;
            } else if rt {
                let (ndr, ndc) = (-dc, dr); // deflect left, away from the right ball
                dr = ndr;
                dc = ndc;
                continue;
            }
            // No deflection: a ball straight ahead is a hit, else advance.
            if self.ball(r + dr, c + dc) {
                return Ray::Hit;
            }
            r += dr;
            c += dc;
            if !inside(r, c) {
                let (es, ei) = Self::edge_of(r, c);
                return Ray::Exit(es, ei);
            }
        }
        Ray::Hit // trapped in a pocket of balls — treat as absorbed
    }
}

/// The interactive Blackbox overlay.
pub struct Blackbox {
    grid: Grid,
    seed: u64,
    guesses: [[bool; 8]; 8],
    /// Border firing results: one char per side/index (H, R, or a pairing digit).
    marks: [[char; 8]; 4],
    next_label: u8,
    /// Cursor over the whole grid including the one-cell border (−1..=N).
    cur: (i16, i16),
    revealed: bool,
    status: String,
}

fn side_row(side: Side) -> usize {
    match side {
        Side::Top => 0,
        Side::Bottom => 1,
        Side::Left => 2,
        Side::Right => 3,
    }
}

impl Blackbox {
    pub fn new() -> Self {
        Blackbox {
            grid: Grid::from_seed(1),
            seed: 1,
            guesses: [[false; 8]; 8],
            marks: [[' '; 8]; 4],
            next_label: 1,
            cur: (0, 0),
            revealed: false,
            status: "Fire from the border; deduce the balls.".into(),
        }
    }

    fn deal(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        *self = Blackbox {
            grid: Grid::from_seed(self.seed),
            seed: self.seed,
            ..Blackbox::new_blank()
        };
        self.status = "New board.".into();
    }

    fn new_blank() -> Self {
        let mut b = Blackbox::new();
        b.status.clear();
        b
    }

    /// The border side/index at the cursor, if it is on the border.
    fn border_at(&self) -> Option<(Side, i16)> {
        let (r, c) = self.cur;
        match (r, c) {
            (-1, c) if (0..N).contains(&c) => Some((Side::Top, c)),
            (rr, cc) if rr == N && (0..N).contains(&cc) => Some((Side::Bottom, cc)),
            (r, -1) if (0..N).contains(&r) => Some((Side::Left, r)),
            (r, cc) if cc == N && (0..N).contains(&r) => Some((Side::Right, r)),
            _ => None,
        }
    }

    fn fire(&mut self, side: Side, idx: i16) {
        if self.marks[side_row(side)][idx as usize] != ' ' {
            return; // already fired here
        }
        match self.grid.trace(side, idx) {
            Ray::Hit => {
                self.marks[side_row(side)][idx as usize] = 'H';
                self.status = "Hit.".into();
            }
            Ray::Exit(es, ei) if (es, ei) == (side, idx) => {
                self.marks[side_row(side)][idx as usize] = 'R';
                self.status = "Reflection.".into();
            }
            Ray::Exit(es, ei) => {
                let label = std::char::from_digit((self.next_label % 10) as u32, 10).unwrap();
                self.next_label = self.next_label.wrapping_add(1);
                self.marks[side_row(side)][idx as usize] = label;
                self.marks[side_row(es)][ei as usize] = label;
                self.status = format!("Exits at a matching {label}.");
            }
        }
    }

    fn act(&mut self) {
        if self.revealed {
            return;
        }
        if let Some((side, idx)) = self.border_at() {
            self.fire(side, idx);
        } else {
            let (r, c) = self.cur;
            if inside(r, c) {
                let g = &mut self.guesses[r as usize][c as usize];
                *g = !*g;
            }
        }
    }

    fn reveal(&mut self) {
        self.revealed = true;
        let mut right = 0;
        let mut wrong = 0;
        for r in 0..8 {
            for c in 0..8 {
                match (self.grid.balls[r][c], self.guesses[r][c]) {
                    (true, true) => right += 1,
                    (false, true) => wrong += 1,
                    _ => {}
                }
            }
        }
        self.status = format!("{right}/{BALLS} found, {wrong} wrong.  n: new");
    }
}

impl Default for Blackbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Blackbox {
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
            key!(Left) | key!('h') => self.cur.1 = (self.cur.1 - 1).max(-1),
            key!(Right) | key!('l') => self.cur.1 = (self.cur.1 + 1).min(N),
            key!(Up) | key!('k') => self.cur.0 = (self.cur.0 - 1).max(-1),
            key!(Down) | key!('j') => self.cur.0 = (self.cur.0 + 1).min(N),
            key!(' ') | key!(Enter) => self.act(),
            key!('c') => self.reveal(),
            key!('n') => self.deal(),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let grid_style = theme.get("ui.linenr");
        let mark_style = theme.get("function");
        let guess_style = theme.get("warning");
        let ball_style = theme.get("ui.text.focus");
        let wrong_style = theme.get("error");
        let cursor_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 24 || area.height < 14 {
            return;
        }
        let ox = area.x + 3;
        let oy = area.y + 2;
        surface.set_string(ox, area.y, "Blackbox — find the hidden balls", header_style);

        // The playfield spans rows/cols -1..=N; map a logical (r,c) to screen.
        let sx = |c: i16| ox + ((c + 1) as u16) * 2;
        let sy = |r: i16| oy + (r + 1) as u16;
        for r in -1..=N {
            for c in -1..=N {
                let on_border = (r == -1 || r == N) ^ (c == -1 || c == N);
                let is_corner = (r == -1 || r == N) && (c == -1 || c == N);
                if is_corner {
                    continue;
                }
                let (glyph, mut style) = if on_border {
                    // Firing point / result.
                    let (side, idx) = if r == -1 {
                        (Side::Top, c)
                    } else if r == N {
                        (Side::Bottom, c)
                    } else if c == -1 {
                        (Side::Left, r)
                    } else {
                        (Side::Right, r)
                    };
                    let m = self.marks[side_row(side)][idx as usize];
                    if m == ' ' {
                        ("·".to_string(), grid_style)
                    } else {
                        (m.to_string(), mark_style)
                    }
                } else {
                    // Interior cell.
                    let (ru, cu) = (r as usize, c as usize);
                    if self.revealed && self.grid.balls[ru][cu] {
                        ("●".to_string(), ball_style)
                    } else if self.revealed && self.guesses[ru][cu] {
                        ("✗".to_string(), wrong_style)
                    } else if self.guesses[ru][cu] {
                        ("○".to_string(), guess_style)
                    } else {
                        ("·".to_string(), grid_style)
                    }
                };
                if self.cur == (r, c) {
                    style = cursor_style;
                }
                surface.set_string(sx(c), sy(r), &glyph, style);
            }
        }
        let by = sy(N) + 2;
        surface.set_string(ox, by, &self.status, text_style);
        surface.set_string(
            ox,
            by + 1,
            "border SPC fires · inside SPC marks · c reveal · n new · q quit",
            grid_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(cells: &[(usize, usize)]) -> Grid {
        let mut balls = [[false; 8]; 8];
        for &(r, c) in cells {
            balls[r][c] = true;
        }
        Grid { balls }
    }

    #[test]
    fn empty_box_passes_straight_through() {
        let g = grid(&[]);
        // Fired from the left at row 3, exits on the right at row 3.
        assert_eq!(g.trace(Side::Left, 3), Ray::Exit(Side::Right, 3));
    }

    #[test]
    fn ball_in_line_is_a_hit() {
        let g = grid(&[(3, 3)]);
        assert_eq!(g.trace(Side::Left, 3), Ray::Hit);
    }

    #[test]
    fn side_ball_deflects_ninety_degrees() {
        // Ball above the path deflects a rightward ray downward, so it exits the
        // bottom rather than the right.
        let g = grid(&[(2, 3)]);
        match g.trace(Side::Left, 3) {
            Ray::Exit(Side::Bottom, _) => {}
            other => panic!("expected a downward deflection, got {other:?}"),
        }
    }

    #[test]
    fn ball_beside_the_entry_reflects() {
        // A ball at (3,0) is on the edge, directly beside the row-2 entry cell
        // (2,0): firing there reflects straight back.
        let g = grid(&[(3, 0)]);
        assert_eq!(g.trace(Side::Left, 2), Ray::Exit(Side::Left, 2));
    }

    #[test]
    fn two_flanking_balls_reflect_to_entry() {
        // Balls symmetric about the path cancel into a reflection back out the
        // entry point.
        let g = grid(&[(2, 3), (4, 3)]);
        assert_eq!(g.trace(Side::Left, 3), Ray::Exit(Side::Left, 3));
    }

    #[test]
    fn deals_place_four_distinct_balls() {
        for seed in 0..8 {
            let g = Grid::from_seed(seed);
            let n: usize = g.balls.iter().flatten().filter(|&&b| b).count();
            assert_eq!(n, BALLS, "seed {seed}");
        }
    }
}
