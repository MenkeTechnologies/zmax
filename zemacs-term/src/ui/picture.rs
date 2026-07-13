//! Picture — the zemacs port of GNU Emacs `picture-mode`, the 2-D ASCII drawing
//! mode, as a self-contained overlay canvas.
//!
//! In picture-mode the buffer becomes a grid you paint on: a typed character
//! overwrites the cell under point and then advances point one step in the
//! current *drawing direction* (one of the eight compass points), rather than
//! inserting left-to-right. This overlay renders that grid and drives the pure,
//! unit-tested [`zemacs_core::picture::Canvas`], which owns all the model logic
//! (advance, rectangle draw/clear, `to_string`).
//!
//! Because every printable letter (including `h`/`j`/`k`/`l`) is a drawing
//! character here, cursor movement is on the arrow keys and the Emacs
//! `C-f`/`C-b`/`C-n`/`C-p` motions rather than bare `hjkl`, and **quit is
//! `Esc` / `C-c` / `C-g`** — bare `q` is a paintable character, so it is *not*
//! bound to quit (a deliberate deviation from the game overlays, which quit on
//! `q`).
//!
//! Key map — the real `picture-mode-map` (checked against Emacs 30's `C-h b`
//! dump), including its whole `C-c` prefix map, which this component walks with
//! a one-key prefix state:
//!   printable char / SPC         — picture-self-insert (draw, then advance)
//!   ←/→/↑/↓, C-b/C-f/C-p/C-n     — move point one cell (picture column/row motion)
//!   Enter                        — picture-newline (start column 0 of next row)
//!   TAB                          — picture-tab (move to the next tab stop)
//!   M-TAB / C-M-i                — picture-tab-search (under the next word above)
//!   The `C-c` prefix map (drawing direction, motion, rectangles):
//!   C-c > / C-c <                — picture-movement-right / -left  (east / west)
//!   C-c ^ / C-c .                — picture-movement-up / -down     (north / south)
//!   C-c ` / C-c ' / C-c / / C-c \ — NW / NE / SW / SE diagonals
//!   C-c ←/→/↑/↓/Home/End/PgUp/PgDn — the same eight directions on the arrow pad
//!   C-c C-f / C-c C-b            — picture-motion / -reverse (step along the dir)
//!   C-c C-d                      — picture-delete-char (shift the row left)
//!   C-c C-r                      — picture-draw-rectangle  (mark → point)
//!   C-c C-k                      — picture-clear-rectangle (mark → point, saved)
//!   C-c C-y                      — picture-yank-rectangle  (overlay it at point)
//!   C-c C-w r / C-c C-x r        — clear to / yank from register `r`
//!   C-c TAB                      — picture-set-tab-stops (from this row's words)
//!   C-c C-c                      — picture-mode-exit
//!
//! zemacs aliases (kept from before the `C-c` map existed): M-> / M-< / M-^ /
//! M-v / M-. set the direction, M-9 / M-7 / M-3 / M-1 the diagonals, M-r / M-c
//! draw / clear the rectangle. `M-m` sets the rectangle corner (Emacs uses the
//! region mark, which this overlay has no editor buffer for).
//! Esc / C-g also quit — bare `q` is a paintable character, so it is *not*
//! bound to quit.

use std::collections::BTreeMap;

use tui::buffer::Buffer as Surface;
use zemacs_core::picture::{next_tab_stop, set_tab_stops, Canvas, Dir};
use zemacs_view::graphics::Rect;
use zemacs_view::keyboard::{KeyCode, KeyModifiers};

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: usize = 60;
const H: usize = 20;

/// A rectangle read out of the canvas: one `String` per row, all the same width.
type Rectangle = Vec<String>;

/// What the next key names, once `C-c C-w` / `C-c C-x` has been typed: Emacs
/// reads the register with `register-read-with-preview`, i.e. a single character.
#[derive(Clone, Copy)]
enum RegisterOp {
    /// `C-c C-w` — picture-clear-rectangle-to-register.
    ClearTo,
    /// `C-c C-x` — picture-yank-rectangle-from-register.
    YankFrom,
}

/// The interactive picture-mode overlay.
pub struct Picture {
    canvas: Canvas,
    /// One corner for the next rectangle draw/clear, set with `M-m`.
    mark: Option<(usize, usize)>,
    status: String,
    /// `C-c` has been typed; the next key indexes the `C-c` prefix map.
    prefix_cc: bool,
    /// `C-c C-w` / `C-c C-x` has been typed; the next key is the register name.
    pending_register: Option<RegisterOp>,
    /// The rectangle saved by `C-c C-k` (Emacs's `killed-rectangle`), overlaid by
    /// `C-c C-y`.
    killed_rectangle: Rectangle,
    /// Rectangles stashed by `C-c C-w`, keyed by register character.
    registers: BTreeMap<char, Rectangle>,
    /// Tab stops for `picture-tab`, set by `C-c TAB` from the current row. Empty
    /// = Emacs's default `tab-stop-list`, every 8 columns.
    tab_stops: Vec<usize>,
}

impl Picture {
    pub fn new() -> Self {
        Picture {
            canvas: Canvas::new(W, H),
            mark: None,
            status: String::new(),
            prefix_cc: false,
            pending_register: None,
            killed_rectangle: Vec::new(),
            registers: BTreeMap::new(),
            tab_stops: Vec::new(),
        }
    }

    /// Overwrite one cell without disturbing point or the drawing direction
    /// ([`Canvas::put_char`] always paints *at point* and then advances).
    fn set_cell(&mut self, r: usize, c: usize, ch: char) {
        if r >= self.canvas.height() || c >= self.canvas.width() {
            return;
        }
        let point = self.canvas.cursor();
        self.canvas.move_to(r, c);
        self.canvas.put_char(ch);
        self.canvas.move_to(point.0, point.1);
    }

    /// Row `r` of the canvas as a string (full width, blanks included).
    fn row(&self, r: usize) -> String {
        (0..self.canvas.width())
            .map(|c| self.canvas.get(r, c))
            .collect()
    }

    /// `picture-delete-char`: delete the character under point, sliding the rest
    /// of the row one column left and blanking the freed cell at the right edge.
    fn delete_char(&mut self) {
        let (r, c) = self.canvas.cursor();
        let mut cells: Vec<char> = self.row(r).chars().collect();
        if c >= cells.len() {
            return;
        }
        cells.remove(c);
        cells.push(' ');
        for (col, ch) in cells.into_iter().enumerate().skip(c) {
            self.set_cell(r, col, ch);
        }
    }

    /// The rectangle between the `M-m` mark and point, as text (`None` when no
    /// corner has been set).
    fn rectangle_at_mark(&self) -> Option<(usize, usize, usize, usize, Rectangle)> {
        let (r0, c0) = self.mark?;
        let (r1, c1) = self.canvas.cursor();
        let (top, bottom) = (r0.min(r1), r0.max(r1));
        let (left, right) = (c0.min(c1), c0.max(c1));
        let rect = (top..=bottom)
            .map(|r| (left..=right).map(|c| self.canvas.get(r, c)).collect())
            .collect();
        Some((top, left, bottom, right, rect))
    }

    /// `picture-clear-rectangle` / `-to-register`: save the mark→point rectangle
    /// and blank it out. Returns `false` when no corner has been set.
    fn clear_rectangle(&mut self, register: Option<char>) -> bool {
        let Some((top, left, bottom, right, rect)) = self.rectangle_at_mark() else {
            self.status = "M-m to set a corner first".to_string();
            return false;
        };
        self.canvas.clear_rectangle(top, left, bottom, right);
        let cells = rect.len() * rect.first().map_or(0, |r: &String| r.chars().count());
        match register {
            Some(reg) => {
                self.registers.insert(reg, rect);
                self.status = format!("cleared {cells} cells into register {reg}");
            }
            None => {
                self.killed_rectangle = rect;
                self.status = format!("cleared and saved {cells} cells");
            }
        }
        true
    }

    /// `picture-yank-rectangle` / `-from-register`: overlay a saved rectangle with
    /// its top-left corner at point, overwriting what it covers.
    fn yank_rectangle(&mut self, register: Option<char>) {
        let rect = match register {
            Some(reg) => match self.registers.get(&reg) {
                Some(r) => r.clone(),
                None => {
                    self.status = format!("register {reg} is empty");
                    return;
                }
            },
            None => self.killed_rectangle.clone(),
        };
        if rect.is_empty() {
            self.status = "no rectangle saved (C-c C-k first)".to_string();
            return;
        }
        let (r0, c0) = self.canvas.cursor();
        for (i, line) in rect.iter().enumerate() {
            for (j, ch) in line.chars().enumerate() {
                self.set_cell(r0 + i, c0 + j, ch);
            }
        }
        self.status = format!("yanked {} row(s)", rect.len());
    }

    /// `picture-tab`: move point (without drawing) to the next tab stop. With no
    /// stops set, Emacs falls back to `tab-stop-list`'s default — every 8 columns.
    fn tab(&mut self) {
        let (r, c) = self.canvas.cursor();
        let next = match next_tab_stop(c, &self.tab_stops) {
            Some(stop) => stop,
            None if self.tab_stops.is_empty() => c - c % 8 + 8,
            // Past the last explicit stop, Emacs's picture-tab stays put.
            None => c,
        };
        self.canvas.move_to(r, next);
    }

    /// `picture-tab-search`: move to the column beneath the next "interesting"
    /// character in the previous row — a non-blank preceded by whitespace. With
    /// none to the right of point, Emacs goes to the beginning of the line.
    fn tab_search(&mut self) {
        let (r, c) = self.canvas.cursor();
        let above = if r == 0 {
            String::new()
        } else {
            self.row(r - 1)
        };
        let target = set_tab_stops(&above)
            .into_iter()
            .find(|&stop| stop > c)
            .unwrap_or(0);
        self.canvas.move_to(r, target);
    }

    /// Move point one cell, clamped, in the given `(dr, dc)` direction — the
    /// arrow / `C-f`/`C-b`/`C-n`/`C-p` motions (which do not change the drawing
    /// direction).
    fn move_by(&mut self, dr: isize, dc: isize) {
        let (r, c) = self.canvas.cursor();
        let nr = (r as isize + dr).clamp(0, self.canvas.height() as isize - 1) as usize;
        let nc = (c as isize + dc).clamp(0, self.canvas.width() as isize - 1) as usize;
        self.canvas.move_to(nr, nc);
    }

    /// picture-newline: drop to column 0 of the next row (clamped at the bottom).
    fn newline(&mut self) {
        let (r, _) = self.canvas.cursor();
        let nr = (r + 1).min(self.canvas.height() - 1);
        self.canvas.move_to(nr, 0);
    }
}

impl Default for Picture {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Picture {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        // `C-c C-w` / `C-c C-x`: this key is the register name.
        if let Some(op) = self.pending_register.take() {
            if let KeyCode::Char(reg) = key.code {
                match op {
                    RegisterOp::ClearTo => {
                        self.clear_rectangle(Some(reg));
                    }
                    RegisterOp::YankFrom => self.yank_rectangle(Some(reg)),
                }
            }
            return EventResult::Consumed(None);
        }

        // The `C-c` prefix map.
        if std::mem::take(&mut self.prefix_cc) {
            match key {
                // C-c C-c — picture-mode-exit.
                ctrl!('c') => return EventResult::Consumed(Some(close)),
                // Drawing direction: the eight compass points, on both the
                // punctuation keys and the arrow pad (picture-movement-*).
                key!('>') | key!(Right) => self.canvas.set_dir(Dir::E),
                key!('<') | key!(Left) => self.canvas.set_dir(Dir::W),
                key!('^') | key!(Up) => self.canvas.set_dir(Dir::N),
                key!('.') | key!(Down) => self.canvas.set_dir(Dir::S),
                key!('\'') | key!(PageUp) => self.canvas.set_dir(Dir::NE),
                key!('`') | key!(Home) => self.canvas.set_dir(Dir::NW),
                key!('/') | key!(End) => self.canvas.set_dir(Dir::SW),
                key!('\\') | key!(PageDown) => self.canvas.set_dir(Dir::SE),
                // C-c C-f / C-c C-b — picture-motion / picture-motion-reverse:
                // step ALONG the drawing direction (unlike C-f, which is columnar).
                ctrl!('f') => self.canvas.move_step(),
                ctrl!('b') => {
                    let dir = self.canvas.dir();
                    self.canvas.set_dir(dir.reverse());
                    self.canvas.move_step();
                    self.canvas.set_dir(dir);
                }
                // C-c C-d — picture-delete-char.
                ctrl!('d') => self.delete_char(),
                // C-c C-r / C-c C-k / C-c C-y — rectangle draw / clear / yank.
                ctrl!('r') => match self.mark {
                    Some((r0, c0)) => {
                        let (r1, c1) = self.canvas.cursor();
                        self.canvas.draw_rectangle(r0, c0, r1, c1);
                        self.status = "drew rectangle".to_string();
                    }
                    None => self.status = "M-m to set a corner first".to_string(),
                },
                ctrl!('k') => {
                    self.clear_rectangle(None);
                }
                ctrl!('y') => self.yank_rectangle(None),
                // C-c C-w r / C-c C-x r — clear into / yank from a register.
                ctrl!('w') => {
                    self.pending_register = Some(RegisterOp::ClearTo);
                    self.status = "Clear rectangle to register: ".to_string();
                }
                ctrl!('x') => {
                    self.pending_register = Some(RegisterOp::YankFrom);
                    self.status = "Yank rectangle from register: ".to_string();
                }
                // C-c TAB — picture-set-tab-stops, from this row's words.
                key!(Tab) => {
                    let (r, _) = self.canvas.cursor();
                    self.tab_stops = set_tab_stops(&self.row(r));
                    self.status = format!("tab stops: {:?}", self.tab_stops);
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        match key {
            // Quit — bare `q` is a drawing char, so only these quit (`C-c` is now
            // the prefix; Emacs exits with `C-c C-c`).
            key!(Esc) | ctrl!('g') => return EventResult::Consumed(Some(close)),
            ctrl!('c') => self.prefix_cc = true,

            // Cursor motion (does not change the drawing direction).
            key!(Left) | ctrl!('b') => self.move_by(0, -1),
            key!(Right) | ctrl!('f') => self.move_by(0, 1),
            key!(Up) | ctrl!('p') => self.move_by(-1, 0),
            key!(Down) | ctrl!('n') => self.move_by(1, 0),
            key!(Enter) => self.newline(),

            // TAB — picture-tab; M-TAB — picture-tab-search.
            key!(Tab) => self.tab(),
            alt!(Tab) => self.tab_search(),

            // Drawing direction (picture-movement-*), zemacs M- aliases of the
            // `C-c` chords above.
            alt!('>') => self.canvas.set_dir(Dir::E),
            alt!('<') => self.canvas.set_dir(Dir::W),
            alt!('^') => self.canvas.set_dir(Dir::N),
            alt!('v') | alt!('.') => self.canvas.set_dir(Dir::S),
            alt!('9') => self.canvas.set_dir(Dir::NE),
            alt!('7') => self.canvas.set_dir(Dir::NW),
            alt!('3') => self.canvas.set_dir(Dir::SE),
            alt!('1') => self.canvas.set_dir(Dir::SW),

            // Rectangle mark + draw/clear.
            alt!('m') => {
                let cur = self.canvas.cursor();
                self.mark = Some(cur);
                self.status = format!("mark set at ({}, {})", cur.0, cur.1);
            }
            alt!('r') => match self.mark {
                Some((r0, c0)) => {
                    let (r1, c1) = self.canvas.cursor();
                    self.canvas.draw_rectangle(r0, c0, r1, c1);
                    self.status = "drew rectangle".to_string();
                }
                None => self.status = "M-m to set a corner first".to_string(),
            },
            alt!('c') => {
                self.clear_rectangle(None);
            }

            // C-M-i — the other Emacs binding of picture-tab-search (CONTROL|ALT
            // is not expressible with the ctrl!/alt! macros).
            other
                if other.code == KeyCode::Char('i')
                    && other.modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT =>
            {
                self.tab_search()
            }

            // Printable characters draw at point (picture-self-insert).
            other => {
                if other.modifiers == KeyModifiers::NONE || other.modifiers == KeyModifiers::SHIFT {
                    if let KeyCode::Char(ch) = other.code {
                        if !ch.is_control() {
                            self.canvas.put_char(ch);
                        }
                    }
                }
            }
        }
        // Stay modal: never leak keys to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let frame_style = theme.get("ui.linenr");
        let cursor_style = theme.get("ui.selection");
        let mark_style = theme.get("warning");
        let dir_style = theme.get("function");

        surface.clear_with(area, bg);
        if area.width < (W as u16) + 4 || area.height < (H as u16) + 4 {
            surface.set_stringn(
                area.x,
                area.y,
                "Picture: window too small",
                area.width as usize,
                text_style,
            );
            return;
        }

        let ox = area.x + 2;
        let oy = area.y + 2;
        let (cr, cc) = self.canvas.cursor();

        // Header: mode, drawing direction and point.
        surface.set_string(ox, area.y, "Picture", header_style);
        surface.set_string(
            ox + 8,
            area.y,
            &format!("dir={} ", self.canvas.dir().arrow()),
            dir_style,
        );
        surface.set_string(ox + 16, area.y, &format!("({}, {})", cr, cc), frame_style);

        // Top / bottom frame.
        for c in 0..W {
            surface.set_string(ox + c as u16, oy - 1, "─", frame_style);
            surface.set_string(ox + c as u16, oy + H as u16, "─", frame_style);
        }

        // Grid rows, then overlay the mark and cursor cells.
        for r in 0..H {
            let y = oy + r as u16;
            let row: String = (0..W).map(|c| self.canvas.get(r, c)).collect();
            surface.set_stringn(ox, y, &row, W, text_style);
        }
        if let Some((mr, mc)) = self.mark {
            let ch = self.canvas.get(mr, mc);
            let s = if ch == ' ' {
                "·".to_string()
            } else {
                ch.to_string()
            };
            surface.set_string(ox + mc as u16, oy + mr as u16, &s, mark_style);
        }
        // Cursor last so it wins even when it coincides with the mark.
        let cch = self.canvas.get(cr, cc);
        let cs = if cch == ' ' {
            " ".to_string()
        } else {
            cch.to_string()
        };
        surface.set_string(ox + cc as u16, oy + cr as u16, &cs, cursor_style);

        // Footer legend + transient status.
        let sy = oy + H as u16 + 1;
        surface.set_stringn(
            ox,
            sy,
            "type: draw  ←↑↓→: move  TAB: tab stop  C-c <dir>: draw dir  C-c C-k/C-y: rect  C-c C-c: exit",
            area.width.saturating_sub(2) as usize,
            frame_style,
        );
        if !self.status.is_empty() {
            surface.set_stringn(ox, sy + 1, &self.status, W, header_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paint `text` starting at `(r, c)` going east, leaving point back where it
    /// started — a test fixture, not a picture-mode command.
    fn paint(p: &mut Picture, r: usize, c: usize, text: &str) {
        for (i, ch) in text.chars().enumerate() {
            p.set_cell(r, c + i, ch);
        }
    }

    /// `C-c C-d` (picture-delete-char) deletes the cell under point and slides the
    /// rest of the row left — it does *not* just blank the cell (that is
    /// picture-clear-column), and point does not move.
    #[test]
    fn delete_char_shifts_the_row_left() {
        let mut p = Picture::new();
        paint(&mut p, 0, 0, "abcd");
        p.canvas.move_to(0, 1);

        p.delete_char();

        assert_eq!(p.row(0).trim_end(), "acd");
        assert_eq!(p.canvas.cursor(), (0, 1), "point stays put");
        // The freed cell at the right edge is blank, not a copy of the last char.
        assert_eq!(p.canvas.get(0, W - 1), ' ');
    }

    /// `C-c C-k` saves the rectangle it clears, and `C-c C-y` overlays that copy at
    /// point — overwriting what is under it, not inserting and shifting.
    #[test]
    fn clear_rectangle_saves_it_and_yank_overlays_at_point() {
        let mut p = Picture::new();
        paint(&mut p, 0, 0, "AB");
        paint(&mut p, 1, 0, "CD");
        paint(&mut p, 5, 0, "xxxx");

        // Mark the top-left corner, put point on the bottom-right, clear.
        p.mark = Some((0, 0));
        p.canvas.move_to(1, 1);
        assert!(p.clear_rectangle(None));
        assert_eq!(p.killed_rectangle, vec!["AB".to_string(), "CD".to_string()]);
        assert_eq!(p.row(0).trim_end(), "");
        assert_eq!(p.row(1).trim_end(), "");

        // Yank it at (5, 1): it overwrites the middle of the `xxxx` run.
        p.canvas.move_to(5, 1);
        p.yank_rectangle(None);
        assert_eq!(p.row(5).trim_end(), "xABx");
        assert_eq!(p.row(6).trim_end(), " CD");

        // A register round-trip is the same rectangle under a name (C-c C-w / C-c C-x).
        p.mark = Some((5, 1));
        p.canvas.move_to(5, 2);
        assert!(p.clear_rectangle(Some('a')));
        assert_eq!(p.registers.get(&'a'), Some(&vec!["AB".to_string()]));
        p.canvas.move_to(9, 0);
        p.yank_rectangle(Some('a'));
        assert_eq!(p.row(9).trim_end(), "AB");
    }

    /// `TAB` (picture-tab) moves point without drawing: to the next stop set by
    /// `C-c TAB`, and — with no stops set — to the next multiple of 8.
    #[test]
    fn tab_moves_to_the_next_stop_without_drawing() {
        let mut p = Picture::new();
        paint(&mut p, 0, 0, "foo  bar   baz");
        let before = p.row(0);

        // Default (no stops set): every 8 columns.
        p.canvas.move_to(0, 3);
        p.tab();
        assert_eq!(p.canvas.cursor(), (0, 8));

        // C-c TAB reads the stops off this row: the words after the first.
        p.tab_stops = set_tab_stops(&p.row(0));
        assert_eq!(p.tab_stops, vec![5, 11]);
        p.canvas.move_to(0, 0);
        p.tab();
        assert_eq!(p.canvas.cursor(), (0, 5));
        p.tab();
        assert_eq!(p.canvas.cursor(), (0, 11));

        // Nothing was drawn along the way.
        assert_eq!(p.row(0), before);
    }

    /// `M-TAB` (picture-tab-search) moves to the column beneath the next word in
    /// the row *above*, and to column 0 when there is none.
    #[test]
    fn tab_search_lands_under_the_next_word_above() {
        let mut p = Picture::new();
        paint(&mut p, 0, 2, "ab   cd");
        p.canvas.move_to(1, 0);

        // The row above has interesting chars at column 2 (`ab`) and column 7
        // (`cd`) — each a non-blank preceded by whitespace. Point walks them.
        p.tab_search();
        assert_eq!(p.canvas.cursor(), (1, 2));
        p.tab_search();
        assert_eq!(p.canvas.cursor(), (1, 7));

        // Past the last one, it goes back to the beginning of the line.
        p.tab_search();
        assert_eq!(p.canvas.cursor(), (1, 0));
    }
}
