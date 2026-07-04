//! Table — the zemacs port of GNU Emacs `table.el`, the text-based table editor.
//!
//! A modal, full-screen [`Component`] that edits a grid of text cells and draws
//! it as an ASCII box-drawing table (`+`/`-`/`|`). The current cell is
//! highlighted; typing appends into it. All grid logic (sizing, rendering,
//! row/column insert & delete, cell navigation) lives in the pure, unit-tested
//! [`zemacs_core::table`]; this module is only key handling and rendering.
//!
//! Because letters are typed into cells, quitting is **`Esc`** (or `C-g`/`C-c`),
//! never a bare `q`.
//!
//! Keys (each maps to a `table.el` command in the port tracker):
//!   Tab / Enter / Right      table-forward-cell  (wraps across rows)
//!   Shift-Tab / Left         table-backward-cell (wraps across rows)
//!   Up / Down                move to the cell one row up / down
//!   printable char           insert the char into the current cell
//!   Backspace                delete the last char of the current cell
//!   M-o                      table-insert-row     (below the current row)
//!   M-c                      table-insert-column  (right of the current col)
//!   M-k                      table-delete-row     (the current row)
//!   M-d                      table-delete-column  (the current column)
//!   Esc / C-g / C-c          quit
//!
//! Deferred (behave as no-ops here, documented for the port tracker):
//!   table-split-cell / table-span-cell — need a cell-spanning model that this
//!     dense grid substrate does not carry; deferred to a later slice.
//!   table-justify (left/center/right cell justification) — deferred; every
//!     cell renders left-justified for now.

use tui::buffer::Buffer as Surface;
use zemacs_core::table::{backward_cell, forward_cell, Table};
use zemacs_view::graphics::Rect;
use zemacs_view::keyboard::{KeyCode, KeyModifiers};

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key, shift,
};

/// The interactive `table.el` overlay.
pub struct TableEditor {
    table: Table,
    row: usize,
    col: usize,
}

impl TableEditor {
    /// A new editor over a small blank 3x3 table with point at the top-left.
    pub fn new() -> Self {
        TableEditor {
            table: Table::new(3, 3),
            row: 0,
            col: 0,
        }
    }

    /// Keep point inside the (possibly resized) grid.
    fn clamp_point(&mut self) {
        self.row = self.row.min(self.table.rows().saturating_sub(1));
        self.col = self.col.min(self.table.cols().saturating_sub(1));
    }

    /// Append `ch` to the current cell.
    fn push_char(&mut self, ch: char) {
        let mut s = self.table.get(self.row, self.col).unwrap_or("").to_string();
        s.push(ch);
        self.table.set(self.row, self.col, s);
    }

    /// Delete the last char of the current cell.
    fn backspace(&mut self) {
        let mut s = self.table.get(self.row, self.col).unwrap_or("").to_string();
        s.pop();
        self.table.set(self.row, self.col, s);
    }

    /// Screen x-offset (from the table's left edge) of the content of column
    /// `c`: past the leading `|`, then each earlier column's `width + 2` gutter
    /// cell plus its `|` separator, then this column's own leading space.
    fn cell_x_offset(&self, c: usize) -> u16 {
        let mut x = 1usize;
        for i in 0..c {
            x += self.table.col_width(i) + 3;
        }
        (x + 1) as u16
    }
}

impl Default for TableEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for TableEditor {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        let rows = self.table.rows();
        let cols = self.table.cols();
        match key {
            key!(Esc) | ctrl!('g') | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!(Tab) | key!(Enter) | key!(Right) => {
                let (r, c) = forward_cell(self.row, self.col, rows, cols);
                self.row = r;
                self.col = c;
            }
            shift!(Tab) | key!(Left) => {
                let (r, c) = backward_cell(self.row, self.col, rows, cols);
                self.row = r;
                self.col = c;
            }
            key!(Up) => self.row = self.row.saturating_sub(1),
            key!(Down) => {
                if self.row + 1 < rows {
                    self.row += 1;
                }
            }
            key!(Backspace) => self.backspace(),
            alt!('o') => {
                self.table.insert_row(self.row + 1);
                self.row += 1;
            }
            alt!('c') => {
                self.table.insert_col(self.col + 1);
                self.col += 1;
            }
            alt!('k') => {
                self.table.delete_row(self.row);
                self.clamp_point();
            }
            alt!('d') => {
                self.table.delete_col(self.col);
                self.clamp_point();
            }
            _ => {
                if let KeyCode::Char(ch) = key.code {
                    // Letters (incl. Shift for capitals) and symbols go into the
                    // cell; Ctrl/Alt chords are reserved for commands above.
                    if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
                        self.push_char(ch);
                    }
                }
            }
        }
        // Modal: never leak a key to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let title_style = theme.get("function");
        let border_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let footer_style = theme.get("warning");

        surface.clear_with(area, bg);
        if area.width < 12 || area.height < 4 {
            return;
        }

        let rows = self.table.rows();
        let cols = self.table.cols();

        // Header: "Table  RxC  cell (r,c)".
        surface.set_string(area.x, area.y, "Table", title_style);
        let info = format!("{rows}x{cols}  cell ({},{})", self.row, self.col);
        surface.set_stringn(
            area.x + 6,
            area.y,
            &info,
            area.width.saturating_sub(6) as usize,
            header_style,
        );

        // Body: the rendered box table, separators dimmed.
        let body_y = area.y + 2;
        let last_y = area.y + area.height - 1; // reserved for the footer
        let rendered = self.table.render();
        for (i, line) in rendered.lines().enumerate() {
            let y = body_y + i as u16;
            if y >= last_y {
                break;
            }
            let style = if line.starts_with('+') {
                border_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, line, area.width as usize, style);
        }

        // Highlight the current cell (content row is line 2*row + 1).
        if rows > 0 && cols > 0 {
            let cy = body_y + (2 * self.row + 1) as u16;
            if cy < last_y {
                let w = self.table.col_width(self.col);
                let content = self.table.get(self.row, self.col).unwrap_or("");
                let pad = w.saturating_sub(content.chars().count());
                let mut disp = content.to_string();
                for _ in 0..pad {
                    disp.push(' ');
                }
                surface.set_stringn(
                    area.x + self.cell_x_offset(self.col),
                    cy,
                    &disp,
                    w,
                    sel_style,
                );
            }
        }

        // Footer: key hints.
        let footer = "Tab/Enter next  S-Tab prev  arrows move  M-o/M-c ins  M-k/M-d del  Esc quit";
        surface.set_stringn(area.x, last_y, footer, area.width as usize, footer_style);
    }
}
