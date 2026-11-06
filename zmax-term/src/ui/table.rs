//! Table — the zmax port of GNU Emacs `table.el`, the text-based table editor.
//!
//! A modal, full-screen [`Component`] that edits a grid of text cells and draws
//! it as an ASCII box-drawing table (`+`/`-`/`|`). The current cell is
//! highlighted; typing appends into it. All grid logic (sizing, rendering,
//! row/column insert & delete, cell navigation) lives in the pure, unit-tested
//! [`zmax_core::table`]; this module is only key handling and rendering.
//!
//! Because letters are typed into cells, quitting is **`Esc`** (or `C-g`/`C-c`),
//! never a bare `q`.
//!
//! Keys (each maps to a `table.el` command in the port tracker):
//!   Tab / Enter / Right      table-forward-cell  (wraps across rows)
//!   Shift-Tab / Left         table-backward-cell (wraps across rows)
//!   Up / Down                move to the cell one row up / down
//!   Home / End               the first / last cell of the row
//!   C-Home / C-End           the first / last row, same column
//!   Shift + Up/Down/Left/Right/Home/End, C-S-Home / C-S-End
//!                            the same moves (without wrapping), extending a
//!                            rectangular selection of cells from where it began
//!   Backspace on a selection clear every selected cell
//!   M-w                      copy the selected cells as tab-separated rows
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

use std::ops::RangeInclusive;
use tui::buffer::Buffer as Surface;
use zmax_core::table::{backward_cell, forward_cell, Table};
use zmax_view::graphics::Rect;
use zmax_view::input::KeyEvent;
use zmax_view::keyboard::{KeyCode, KeyModifiers};

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
    /// The fixed corner of a JetBrains `Table-*ExtendSelection` block: Shift
    /// with a move keeps this cell, selecting the rectangle between it and
    /// point. `None` selects only the current cell.
    anchor: Option<(usize, usize)>,
}

impl TableEditor {
    /// A new editor over a small blank 3x3 table with point at the top-left.
    pub fn new() -> Self {
        TableEditor {
            table: Table::new(3, 3),
            row: 0,
            col: 0,
            anchor: None,
        }
    }

    /// Keep point inside the (possibly resized) grid.
    fn clamp_point(&mut self) {
        self.row = self.row.min(self.table.rows().saturating_sub(1));
        self.col = self.col.min(self.table.cols().saturating_sub(1));
    }

    /// Move point to (`row`, `col`), clamped to the grid, extending the
    /// selected block from the cell it began on.
    fn extend_to(&mut self, row: usize, col: usize) {
        self.anchor.get_or_insert((self.row, self.col));
        self.row = row.min(self.table.rows().saturating_sub(1));
        self.col = col.min(self.table.cols().saturating_sub(1));
    }

    /// [`Self::extend_to`] as a handled key.
    fn extend(&mut self, row: usize, col: usize) -> EventResult {
        self.extend_to(row, col);
        EventResult::Consumed(None)
    }

    /// The selected rows and columns: the block between the anchor and point.
    fn block(&self) -> (RangeInclusive<usize>, RangeInclusive<usize>) {
        let (ar, ac) = self.anchor.unwrap_or((self.row, self.col));
        (
            ar.min(self.row)..=ar.max(self.row),
            ac.min(self.col)..=ac.max(self.col),
        )
    }

    /// Empty every selected cell.
    fn clear_block(&mut self) {
        let (rows, cols) = self.block();
        for r in rows {
            for c in cols.clone() {
                self.table.set(r, c, "");
            }
        }
    }

    /// The selected cells as text: one line per row, cells tab-separated.
    fn block_text(&self) -> String {
        let (rows, cols) = self.block();
        rows.map(|r| {
            cols.clone()
                .map(|c| self.table.get(r, c).unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        let rows = self.table.rows();
        let cols = self.table.cols();
        let (row, col) = (self.row, self.col);
        // JetBrains `Table-*ExtendSelection`: Shift with a move extends the
        // block; every other key acts on it (Backspace, M-w) and then drops it.
        match key {
            shift!(Up) => return self.extend(row.saturating_sub(1), col),
            shift!(Down) => return self.extend(row + 1, col),
            shift!(Left) => return self.extend(row, col.saturating_sub(1)),
            shift!(Right) => return self.extend(row, col + 1),
            shift!(Home) => return self.extend(row, 0),
            shift!(End) => return self.extend(row, usize::MAX),
            KeyEvent {
                code: KeyCode::Home,
                modifiers,
            } if modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT => {
                return self.extend(0, col)
            }
            KeyEvent {
                code: KeyCode::End,
                modifiers,
            } if modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT => {
                return self.extend(usize::MAX, col)
            }
            _ => {}
        }
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
            // JetBrains Table-selectFirstColumn / LastColumn / FirstRow / LastRow.
            key!(Home) => self.col = 0,
            key!(End) => self.col = cols.saturating_sub(1),
            ctrl!(Home) => self.row = 0,
            ctrl!(End) => self.row = rows.saturating_sub(1),
            key!(Backspace) if self.anchor.is_some() => self.clear_block(),
            key!(Backspace) => self.backspace(),
            alt!('w') => {
                let (rows, cols) = self.block();
                let text = self.block_text();
                let _ = cx.editor.registers.write('+', vec![text]);
                cx.editor
                    .set_status(format!("copied {} x {} cells", rows.count(), cols.count()));
            }
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
        self.anchor = None;
        // Modal: never leak a key to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
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

        // Highlight the selected cells (content row is line 2*row + 1).
        if rows > 0 && cols > 0 {
            let (sel_rows, sel_cols) = self.block();
            for r in sel_rows {
                let cy = body_y + (2 * r + 1) as u16;
                if cy >= last_y {
                    break;
                }
                for c in sel_cols.clone() {
                    let w = self.table.col_width(c);
                    let content = self.table.get(r, c).unwrap_or("");
                    let disp = format!("{content:<w$}");
                    surface.set_stringn(area.x + self.cell_x_offset(c), cy, &disp, w, sel_style);
                }
            }
        }

        // Footer: key hints.
        let footer = "Tab/Enter next  S-Tab prev  arrows move  S-arrows select  M-w copy  M-o/M-c ins  M-k/M-d del  Esc quit";
        surface.set_stringn(area.x, last_y, footer, area.width as usize, footer_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3x3 editor whose cells hold their own coordinates ("rc").
    fn filled() -> TableEditor {
        let mut ed = TableEditor::new();
        for r in 0..3 {
            for c in 0..3 {
                ed.table.set(r, c, format!("{r}{c}"));
            }
        }
        ed
    }

    #[test]
    fn shift_moves_select_a_clamped_block_that_copies_and_clears() {
        let mut ed = filled();
        ed.row = 1;
        ed.col = 1;
        assert_eq!(ed.block_text(), "11");

        // Up-left from the middle.
        ed.extend_to(0, 0);
        assert_eq!(ed.block_text(), "00\t01\n10\t11");

        // Swinging past the anchor selects the block on its other side; a
        // move past the last row stops at it.
        ed.extend_to(usize::MAX, 2);
        assert_eq!(ed.anchor, Some((1, 1)));
        assert_eq!(ed.block_text(), "11\t12\n21\t22");

        ed.clear_block();
        assert_eq!(ed.block_text(), "\t\n\t");
        assert_eq!(
            ed.table.get(1, 0),
            Some("10"),
            "cells outside the block stay"
        );
        assert_eq!(ed.table.get(0, 2), Some("02"));
    }
}
