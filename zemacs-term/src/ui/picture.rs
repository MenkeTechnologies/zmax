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
//! Key map (each parses into a `picture` keymap mode by
//! `scripts/gen_port_report.py`, mapping to its Emacs picture-mode counterpart):
//!   printable char / SPC         — picture-self-insert (draw, then advance)
//!   ←/→/↑/↓, C-b/C-f/C-p/C-n     — move point one cell (picture column/row motion)
//!   Enter                        — picture-newline (start column 0 of next row)
//!   M->                          — picture-movement-right   (draw east)
//!   M-<                          — picture-movement-left    (draw west)
//!   M-^                          — picture-movement-up      (draw north)
//!   M-v / M-.                    — picture-movement-down     (draw south)
//!   M-9 / M-7 / M-3 / M-1        — NE / NW / SE / SW diagonal (numpad layout)
//!   M-m                          — set the rectangle mark (a corner) at point
//!   M-r                          — picture-draw-rectangle  (mark → point)
//!   M-c                          — picture-clear-rectangle (mark → point)
//!   Esc / C-c / C-g              — quit
//!
//! Deferred (fiddly, need extra UI): picture-set-tab-stops, tab motion,
//! picture-backward-clear-column, and yanking rectangles from a register.

use tui::buffer::Buffer as Surface;
use zemacs_core::picture::{Canvas, Dir};
use zemacs_view::graphics::Rect;
use zemacs_view::keyboard::{KeyCode, KeyModifiers};

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

const W: usize = 60;
const H: usize = 20;

/// The interactive picture-mode overlay.
pub struct Picture {
    canvas: Canvas,
    /// One corner for the next rectangle draw/clear, set with `M-m`.
    mark: Option<(usize, usize)>,
    status: String,
}

impl Picture {
    pub fn new() -> Self {
        Picture {
            canvas: Canvas::new(W, H),
            mark: None,
            status: String::new(),
        }
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
        match key {
            // Quit — bare `q` is a drawing char, so only these quit.
            key!(Esc) | ctrl!('c') | ctrl!('g') => return EventResult::Consumed(Some(close)),

            // Cursor motion (does not change the drawing direction).
            key!(Left) | ctrl!('b') => self.move_by(0, -1),
            key!(Right) | ctrl!('f') => self.move_by(0, 1),
            key!(Up) | ctrl!('p') => self.move_by(-1, 0),
            key!(Down) | ctrl!('n') => self.move_by(1, 0),
            key!(Enter) => self.newline(),

            // Drawing direction (picture-movement-*).
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
            alt!('c') => match self.mark {
                Some((r0, c0)) => {
                    let (r1, c1) = self.canvas.cursor();
                    self.canvas.clear_rectangle(r0, c0, r1, c1);
                    self.status = "cleared rectangle".to_string();
                }
                None => self.status = "M-m to set a corner first".to_string(),
            },

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
        surface.set_string(
            ox + 16,
            area.y,
            &format!("({}, {})", cr, cc),
            frame_style,
        );

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
            let s = if ch == ' ' { "·".to_string() } else { ch.to_string() };
            surface.set_string(ox + mc as u16, oy + mr as u16, &s, mark_style);
        }
        // Cursor last so it wins even when it coincides with the mark.
        let cch = self.canvas.get(cr, cc);
        let cs = if cch == ' ' { " ".to_string() } else { cch.to_string() };
        surface.set_string(ox + cc as u16, oy + cr as u16, &cs, cursor_style);

        // Footer legend + transient status.
        let sy = oy + H as u16 + 1;
        surface.set_stringn(
            ox,
            sy,
            "type: draw  ←↑↓→: move  Enter: newline  M-><^v: dir  M-1/3/7/9: diag  M-m/r/c: rect  Esc: quit",
            area.width.saturating_sub(2) as usize,
            frame_style,
        );
        if !self.status.is_empty() {
            surface.set_stringn(ox, sy + 1, &self.status, W, header_style);
        }
    }
}
