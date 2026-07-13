//! Buffer Menu — a buffer-list mode, the zemacs port of GNU Emacs `buffer-menu`
//! / `list-buffers` (`C-x C-b`, `Buffer-menu-mode`).
//!
//! A full-screen [`Component`] listing the open buffers. Each row shows the Emacs
//! Buffer Menu columns: a leftmost mark tag (`>` selected, `D` delete, `S` save),
//! the `C R M` flags (current `.`, read-only `%`, modified `*`), the buffer name,
//! its size in characters, the major-mode/language name and the file path. All
//! pure state — the ordered rows, the marks (keyed by buffer id so they survive a
//! refresh) and the cursor — lives in the unit-tested [`zemacs_core::buffer_menu`];
//! this module snapshots the editor's documents, renders the grid, and maps keys
//! to `Buffer-menu-mode` commands.
//!
//! Keys (parsed into a `bufmenu` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs Buffer Menu counterpart in the port tracker):
//!   RET/f — select the buffer at point in this window (`Buffer-menu-this-window`)
//!   1     — select it filling the frame (`Buffer-menu-1-window`)
//!   o / 2 — select it in another (split) window (`Buffer-menu-other-window` /
//!           `Buffer-menu-2-window`)
//!   C-o   — display it in another window, staying in the menu
//!           (`Buffer-menu-switch-other-window`)
//!   b     — bury the buffer at point (`Buffer-menu-bury`)
//!   v     — select the buffer at point plus all `>`-marked ones (`Buffer-menu-select`)
//!   m     — mark for display/selection `>` (`Buffer-menu-mark`)
//!   d     — flag for deletion `D` and advance (`Buffer-menu-delete`);
//!   C-d   — flag for deletion and move up (`Buffer-menu-delete-backwards`)
//!   s     — flag to be saved `S` (`Buffer-menu-save`)
//!   x     — execute: save the `S` buffers, kill the `D` buffers (`Buffer-menu-execute`)
//!   u     — unmark at point (`Buffer-menu-unmark`);  DEL — move up and unmark
//!           (`Buffer-menu-backup-unmark`);  U / M-DEL — unmark every buffer
//!           (`Buffer-menu-unmark-all-buffers`)
//!   ~     — clear the modified flag (`Buffer-menu-not-modified`)
//!   %     — toggle the read-only flag (`Buffer-menu-toggle-read-only`)
//!   T     — toggle showing only file-visiting buffers (`Buffer-menu-toggle-files-only`)
//!   I     — toggle showing internal buffers (`Buffer-menu-toggle-internal`)
//!   2     — this buffer in one window, the previously-current one in the other
//!           (`Buffer-menu-2-window`)
//!   M-DEL — remove one mark character from every buffer, RET = all of them
//!           (`Buffer-menu-unmark-all-buffers`)
//!   g     — refresh the list (revert-buffer);  q/Esc — quit
//! The Buffer Menu is a `tabulated-list-mode` buffer, so its column commands are
//! bound here too, over a column cursor the header underlines:
//!   M-← / M-→ — move the column cursor (`tabulated-list-previous/next-column`)
//!   S         — sort by the column at point, re-pressed to reverse it
//!               (`tabulated-list-sort`)
//!   { / }     — narrow / widen that column (`tabulated-list-narrow-current-column`
//!               / `-widen-current-column`)
//! (j/k/n/p, arrows and G/Home/End move point, vim-style aliases not in the
//! Emacs Buffer Menu map.)

use tui::buffer::Buffer as Surface;
use zemacs_core::buffer_menu::{BufferMenu as BufferMenuModel, BufferRow, Mark};
use zemacs_view::{editor::Action, graphics::Rect, DocumentId, Editor};

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The tabulated-list columns, in display order. The column cursor (`M-←`/`M-→`)
/// indexes this, and `S` / `{` / `}` act on the one it names.
const COLUMNS: [&str; 4] = ["Buffer", "Size", "Mode", "File"];
/// Natural width of the fixed-width columns (the name column is measured from the
/// rows, the file column takes what is left).
const SIZE_W: usize = 8;
const MODE_W: usize = 12;

/// The interactive Buffer Menu overlay.
pub struct BufferMenu {
    /// Pure model: rows, marks, cursor.
    menu: BufferMenuModel,
    /// Live document ids keyed by [`BufferRow::key`], so a row lookup survives a
    /// `bury` reorder (which shuffles `menu.rows()` out of enumeration order).
    ids: std::collections::BTreeMap<u64, DocumentId>,
    /// The buffer that was current when the menu opened — the one `2`
    /// (`Buffer-menu-2-window`) puts in the other window.
    previous: Option<DocumentId>,
    /// `T` toggle: list only file-visiting buffers.
    files_only: bool,
    /// `I` toggle: also show internal (`*…*` / space-prefixed) buffers.
    show_internal: bool,
    /// The column cursor: an index into [`COLUMNS`] (`tabulated-list-*-column`).
    column: usize,
    /// The column the rows are sorted by, and whether that sort is reversed
    /// (`tabulated-list-sort`; `None` = the editor's own buffer order).
    sort: Option<(usize, bool)>,
    /// Per-column width adjustments from `{` / `}`, in characters.
    width_delta: [isize; COLUMNS.len()],
    /// `M-DEL`: the next key names the mark character to remove everywhere.
    pending_unmark: bool,
    scroll: usize,
    viewport: usize,
    status: String,
}

/// The stable numeric key of a document (its `DocumentId`, a `NonZeroUsize`).
fn doc_key(id: DocumentId) -> u64 {
    // `DocumentId`'s only public projection is its `Display` (the numeric id);
    // it parses back deterministically for use as the mark key.
    id.to_string().parse().unwrap_or(0)
}

impl BufferMenu {
    /// Open the Buffer Menu, snapshotting the editor's current buffers.
    pub fn new(editor: &Editor) -> Self {
        let mut menu = BufferMenu {
            menu: BufferMenuModel::default(),
            ids: std::collections::BTreeMap::new(),
            previous: Some(zemacs_view::current_ref!(editor).1.id()),
            files_only: false,
            show_internal: false,
            column: 0,
            sort: None,
            width_delta: [0; COLUMNS.len()],
            pending_unmark: false,
            scroll: 0,
            viewport: 1,
            status: String::new(),
        };
        menu.refresh(editor);
        menu
    }

    /// Width of each column: the name column is measured from the rows, `Size`
    /// and `Mode` are fixed, and `File` takes the rest of the line — each then
    /// shifted by whatever `{` / `}` have done to it (never below 1).
    fn widths(&self, total: usize) -> [usize; COLUMNS.len()] {
        let name = self.menu.name_width();
        let used = 4 + 1 + name + 2 + SIZE_W + 2 + MODE_W + 2;
        let file = total.saturating_sub(used).max("File".len());
        let base = [name, SIZE_W, MODE_W, file];
        let mut out = [0usize; COLUMNS.len()];
        for (i, w) in base.iter().enumerate() {
            out[i] = (*w as isize + self.width_delta[i]).max(1) as usize;
        }
        out
    }

    /// Render one row at the current column widths: `<mark><C><R><M> name size
    /// mode file`, the Emacs Buffer Menu column order. (The model's own
    /// `format_line` hardcodes the widths, so the `{` / `}` adjustments and the
    /// column cursor are applied here.)
    fn format_row(&self, row: &BufferRow, w: &[usize; COLUMNS.len()]) -> String {
        let clip = |s: &str, n: usize| -> String { s.chars().take(n).collect() };
        format!(
            "{mark}{crm} {name:<nw$}  {size:>sw$}  {mode:<mw$}  {file}",
            mark = self.menu.mark_of(row.key).glyph(),
            crm = self.menu.crm(row),
            name = clip(&row.name, w[0]),
            size = clip(&row.size.to_string(), w[1]),
            mode = clip(&row.mode, w[2]),
            file = clip(&row.file, w[3]),
            nw = w[0],
            sw = w[1],
            mw = w[2],
        )
    }

    /// `tabulated-list-sort` (`S`): order the rows by the column at point. A
    /// second `S` on the same column reverses it.
    fn sort_by_column(&mut self, editor: &Editor) {
        self.sort = match self.sort {
            Some((col, desc)) if col == self.column => Some((col, !desc)),
            _ => Some((self.column, false)),
        };
        self.refresh(editor);
        let (col, desc) = self.sort.unwrap();
        self.status = format!(
            "buffer-menu: sorted by {}{}",
            COLUMNS[col],
            if desc { " (descending)" } else { "" }
        );
    }

    /// Rebuild the row list from the editor's documents (in `DocumentId` order,
    /// the `BTreeMap` order), preserving marks by buffer id. Honours the
    /// `files_only` (`T`) and `show_internal` (`I`) filters.
    fn refresh(&mut self, editor: &Editor) {
        let current = zemacs_view::current_ref!(editor).1.id();
        let mut rows = Vec::new();
        let mut ids = std::collections::BTreeMap::new();
        for doc in editor.documents() {
            if self.files_only && doc.path().is_none() {
                continue;
            }
            let name = doc.display_name().into_owned();
            if !self.show_internal
                && doc.path().is_none()
                && zemacs_core::buffer_menu::is_internal_name(&name)
            {
                continue;
            }
            let key = doc_key(doc.id());
            rows.push(BufferRow {
                key,
                name,
                size: doc.text().len_chars(),
                mode: doc.language_name().unwrap_or("Fundamental").to_string(),
                file: doc
                    .path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                current: doc.id() == current,
                readonly: doc.readonly,
                modified: doc.is_modified(),
            });
            ids.insert(key, doc.id());
        }
        self.apply_sort(&mut rows);
        self.ids = ids;
        self.menu.set_rows(rows);
    }

    /// `tabulated-list-sort`: order `rows` by the sort column, if one is set —
    /// Size compares numerically, the rest lexically. Without a sort column the
    /// rows keep the editor's own buffer order.
    fn apply_sort(&self, rows: &mut [BufferRow]) {
        let Some((col, desc)) = self.sort else {
            return;
        };
        rows.sort_by(|a, b| {
            let ord = match col {
                0 => a.name.cmp(&b.name),
                1 => a.size.cmp(&b.size),
                2 => a.mode.cmp(&b.mode),
                _ => a.file.cmp(&b.file),
            };
            if desc {
                ord.reverse()
            } else {
                ord
            }
        });
    }

    /// `Buffer-menu-unmark-all-buffers` (`M-DEL`): drop the mark whose glyph is
    /// `wanted` from every buffer — `None` (RET at the prompt) drops them all.
    /// Returns how many buffers were unmarked.
    fn unmark_all_buffers(&mut self, wanted: Option<char>) -> usize {
        let keys: Vec<u64> = self.menu.rows().iter().map(|r| r.key).collect();
        let mut n = 0;
        for key in keys {
            let mark = self.menu.mark_of(key);
            if mark == Mark::None {
                continue;
            }
            if wanted.is_none() || wanted == Some(mark.glyph()) {
                self.menu.set_mark(key, Mark::None);
                n += 1;
            }
        }
        n
    }

    /// The document id under point, if any.
    fn current_doc(&self) -> Option<DocumentId> {
        self.menu.current_key().and_then(|k| self.doc_for(k))
    }

    /// The document id for a buffer key (matching a row).
    fn doc_for(&self, key: u64) -> Option<DocumentId> {
        self.ids.get(&key).copied()
    }

    /// Build a callback that pops the menu and switches to the buffer at point
    /// with `action` (RET/f/1 → Replace, o/2 → split).
    fn select_current(&self, action: Action) -> Option<Callback> {
        let id = self.current_doc()?;
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                cx.editor.switch(id, action);
            },
        ))
    }

    /// `v` (`Buffer-menu-select`): select the buffer at point plus every buffer
    /// marked `>`, the first in this window and the rest in splits.
    fn select_marked(&self) -> Option<Callback> {
        let mut ids: Vec<DocumentId> = self
            .menu
            .flagged(Mark::Select)
            .iter()
            .filter_map(|k| self.doc_for(*k))
            .collect();
        if ids.is_empty() {
            ids.extend(self.current_doc());
        }
        if ids.is_empty() {
            return None;
        }
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                for (i, id) in ids.iter().enumerate() {
                    let action = if i == 0 {
                        Action::Replace
                    } else {
                        Action::HorizontalSplit
                    };
                    cx.editor.switch(*id, action);
                }
            },
        ))
    }

    /// `x` (`Buffer-menu-execute`): save the `S`-flagged buffers, then kill the
    /// `D`-flagged ones, then refresh.
    fn execute(&mut self, editor: &mut Editor) {
        let mut saved = 0;
        for key in self.menu.flagged(Mark::Save) {
            if let Some(id) = self.doc_for(key) {
                if editor.save(id, None::<std::path::PathBuf>, false).is_ok() {
                    saved += 1;
                }
            }
        }
        let mut killed = 0;
        for key in self.menu.flagged(Mark::Delete) {
            if let Some(id) = self.doc_for(key) {
                if editor.close_document(id, false).is_ok() {
                    killed += 1;
                }
            }
        }
        self.refresh(editor);
        self.status = format!("buffer-menu: saved {saved}, killed {killed}");
    }
}

impl Component for BufferMenu {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        self.status.clear();

        // M-DEL (`Buffer-menu-unmark-all-buffers`): this key is the mark char.
        if std::mem::take(&mut self.pending_unmark) {
            let wanted = match key {
                key!(Enter) => None, // RET — every mark
                zemacs_view::input::KeyEvent {
                    code: zemacs_view::keyboard::KeyCode::Char(c),
                    ..
                } => Some(c),
                _ => return EventResult::Consumed(None),
            };
            let n = self.unmark_all_buffers(wanted);
            self.status = format!("buffer-menu: unmarked {n} buffer(s)");
            return EventResult::Consumed(None);
        }

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),

            // Motion (n/p/j/k, arrows, Home/End/G are vim-style aliases).
            key!('j') | key!(Down) | key!('n') | ctrl!('n') => self.menu.move_selection(1),
            key!('k') | key!(Up) | key!('p') | ctrl!('p') => self.menu.move_selection(-1),
            key!(Home) => self.menu.goto_first(),
            key!('G') | key!(End) => self.menu.goto_last(),

            // Select the buffer at point.
            key!(Enter) | key!('f') | key!('1') => {
                if let Some(cb) = self.select_current(Action::Replace) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('o') => {
                if let Some(cb) = self.select_current(Action::HorizontalSplit) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            // 2 (`Buffer-menu-2-window`): this line's buffer in one window and the
            // buffer that was current before the menu opened in the other, with
            // point left in this line's buffer.
            key!('2') => {
                if let (Some(id), Some(prev)) = (self.current_doc(), self.previous) {
                    return EventResult::Consumed(Some(Box::new(
                        move |compositor: &mut Compositor, cx: &mut Context| {
                            compositor.pop();
                            cx.editor.switch(prev, Action::Replace);
                            cx.editor.switch(id, Action::HorizontalSplit);
                        },
                    )));
                }
            }
            // C-o (`Buffer-menu-switch-other-window`): display the buffer at point
            // in another (split) window but stay in the Buffer Menu.
            ctrl!('o') => {
                if let Some(id) = self.current_doc() {
                    cx.editor.switch(id, Action::HorizontalSplit);
                }
            }
            key!('v') => {
                if let Some(cb) = self.select_marked() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            // b (`Buffer-menu-bury`): sink the buffer at point to the bottom of the
            // list. zemacs has no separate global buffer-list order, so this buries
            // it within the menu ordering.
            key!('b') => {
                self.menu.bury_current();
            }

            // Marks.
            key!('m') => self.menu.mark_current(Mark::Select),
            key!('d') => self.menu.mark_current(Mark::Delete),
            ctrl!('d') => {
                if let Some(k) = self.menu.current_key() {
                    self.menu.set_mark(k, Mark::Delete);
                }
                self.menu.move_selection(-1);
            }
            key!('s') => self.menu.mark_current(Mark::Save),
            key!('u') => self.menu.unmark_current(),
            key!(Backspace) => self.menu.backup_unmark(),
            key!('U') => self.menu.unmark_all(),
            // M-DEL (`Buffer-menu-unmark-all-buffers`) asks which mark to remove;
            // `U` (`Buffer-menu-unmark-all`) removes them all without asking.
            alt!(Backspace) => {
                self.pending_unmark = true;
                self.status = "Remove marks (RET means all): ".to_string();
            }
            key!('x') => self.execute(cx.editor),

            // Per-buffer flags acting on the underlying document.
            key!('~') => {
                if let Some(id) = self.current_doc() {
                    if let Some(doc) = cx.editor.document_mut(id) {
                        doc.reset_modified();
                    }
                }
                self.refresh(cx.editor);
            }
            key!('%') => {
                if let Some(id) = self.current_doc() {
                    if let Some(doc) = cx.editor.document_mut(id) {
                        doc.readonly = !doc.readonly;
                    }
                }
                self.refresh(cx.editor);
            }

            // Display.
            key!('T') => {
                self.files_only = !self.files_only;
                self.refresh(cx.editor);
            }
            // I (`Buffer-menu-toggle-internal`): reveal/hide internal (`*…*` /
            // space-prefixed) buffers.
            key!('I') => {
                self.show_internal = !self.show_internal;
                self.refresh(cx.editor);
            }
            key!('g') | key!('R') => self.refresh(cx.editor),

            // ---- tabulated-list-mode column commands ----
            // M-← / M-→ move the column cursor; S sorts by it; { / } resize it.
            alt!(Left) => self.column = self.column.saturating_sub(1),
            alt!(Right) => self.column = (self.column + 1).min(COLUMNS.len() - 1),
            key!('S') => self.sort_by_column(cx.editor),
            key!('{') => {
                self.width_delta[self.column] -= 1;
                self.status = format!("buffer-menu: narrowed {}", COLUMNS[self.column]);
            }
            key!('}') => {
                self.width_delta[self.column] += 1;
                self.status = format!("buffer-menu: widened {}", COLUMNS[self.column]);
            }

            _ => {}
        }
        // Stay modal: never leak keys to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let mark_style = theme.get("diff.plus");
        let flag_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 12 || area.height < 3 {
            return;
        }

        let widths = self.widths(area.width as usize);
        // Header row: the same columns as the rows, with the column cursor's label
        // called out (M-← / M-→ move it; S / { / } act on it).
        let files_only = if self.files_only { " (files only)" } else { "" };
        let mut title = format!(" Buffer Menu{files_only}  CRM");
        for (i, label) in COLUMNS.iter().enumerate() {
            // `^`/`v` mark the sort column and its direction; `[` the column cursor.
            let arrow = match self.sort {
                Some((c, true)) if c == i => "v",
                Some((c, false)) if c == i => "^",
                _ => "",
            };
            let cursor = if i == self.column { "[" } else { " " };
            let cell = format!("{cursor}{label}{arrow}");
            title.push_str(&format!(" {cell:<w$}", w = widths[i]));
        }
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "RET open  m mark  d del  s save  x exec  g refresh  q quit";
        if title.len() + hint.len() + 3 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h as usize;

        if self.menu.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "(no buffers)",
                area.width as usize,
                info_style,
            );
            return;
        }

        let selected = self.menu.selected();
        // Keep the selection in view.
        if selected < self.scroll {
            self.scroll = selected;
        } else if self.viewport > 0 && selected >= self.scroll + self.viewport {
            self.scroll = selected + 1 - self.viewport;
        }

        for (offset, row) in self
            .menu
            .rows()
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let line = self.format_row(row, &widths);
            let base = if offset == selected {
                sel_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, &line, area.width as usize, base);
            // Accent the mark column glyph.
            let m = self.menu.mark_of(row.key).glyph();
            if m != ' ' {
                let ms = if m == 'D' { flag_style } else { mark_style };
                surface.set_stringn(area.x, y, &m.to_string(), 1, ms);
            }
        }

        // Footer: counts / last action.
        let footer = if self.status.is_empty() {
            let d = self.menu.flagged(Mark::Delete).len();
            let s = self.menu.flagged(Mark::Save).len();
            format!("{} buffers  {} to kill  {} to save", self.menu.len(), d, s)
        } else {
            self.status.clone()
        };
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &footer,
            area.width as usize,
            info_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: u64, name: &str, size: usize, mode: &str) -> BufferRow {
        BufferRow {
            key,
            name: name.to_string(),
            size,
            mode: mode.to_string(),
            file: format!("/tmp/{name}"),
            current: false,
            readonly: false,
            modified: false,
        }
    }

    /// A menu over fixed rows — the overlay's own state without an `Editor`
    /// (`refresh` is the only thing that needs one, and the column commands act on
    /// the rows already in the model).
    fn menu(rows: Vec<BufferRow>) -> BufferMenu {
        BufferMenu {
            menu: BufferMenuModel::new(rows),
            ids: std::collections::BTreeMap::new(),
            previous: None,
            files_only: false,
            show_internal: false,
            column: 0,
            sort: None,
            width_delta: [0; COLUMNS.len()],
            pending_unmark: false,
            scroll: 0,
            viewport: 10,
            status: String::new(),
        }
    }

    /// `{` / `}` (tabulated-list-narrow/widen-current-column) resize the column the
    /// column cursor names — not always the buffer-name column — and the row
    /// formatter honours the new width (clipping, not overflowing).
    #[test]
    fn narrow_and_widen_resize_the_column_at_point() {
        let mut m = menu(vec![row(1, "alpha", 10, "Rust")]);
        let total = 80;
        let natural = m.widths(total);

        // Column 0 (Buffer) is the cursor's default: narrowing it shrinks the name.
        m.width_delta[0] -= 2;
        let narrowed = m.widths(total);
        assert_eq!(narrowed[0], natural[0] - 2);
        // The name cell sits right after the mark glyph and the `C R M` triple.
        let line = m.format_row(&m.menu.rows()[0], &narrowed);
        let name_cell: String = line.chars().skip(5).take(narrowed[0]).collect();
        assert_eq!(name_cell, "alph", "clipped to the narrowed width: {line}");

        // The column cursor moves (M-→), so `}` widens Size, not Buffer.
        m.column = 1;
        m.width_delta[1] += 3;
        let wider = m.widths(total);
        assert_eq!(wider[1], natural[1] + 3);
        assert_eq!(wider[0], narrowed[0], "the Buffer column is untouched");

        // A column never collapses past one character.
        m.width_delta[1] = -100;
        assert_eq!(m.widths(total)[1], 1);
    }

    /// `S` (tabulated-list-sort) sorts by the column at point — by NAME on the
    /// first column, by SIZE (numerically, not lexically) on the second — and a
    /// second `S` on the same column reverses it.
    #[test]
    fn sort_orders_rows_by_the_column_at_point() {
        let rows = vec![
            row(1, "beta", 9, "Rust"),
            row(2, "alpha", 100, "Text"),
            row(3, "gamma", 20, "Toml"),
        ];
        let mut m = menu(rows.clone());
        let names =
            |rows: &[BufferRow]| -> Vec<String> { rows.iter().map(|r| r.name.clone()).collect() };

        // No sort column: the editor's own buffer order survives.
        let mut r = rows.clone();
        m.apply_sort(&mut r);
        assert_eq!(names(&r), ["beta", "alpha", "gamma"]);

        // Column 0 (Buffer) -> by name.
        m.sort = Some((0, false));
        let mut r = rows.clone();
        m.apply_sort(&mut r);
        assert_eq!(names(&r), ["alpha", "beta", "gamma"]);

        // Re-pressed on the same column -> reversed.
        m.sort = Some((0, true));
        let mut r = rows.clone();
        m.apply_sort(&mut r);
        assert_eq!(names(&r), ["gamma", "beta", "alpha"]);

        // Column 1 (Size) -> numerically: 9 < 20 < 100, which a lexical sort of
        // the rendered strings would get wrong ("100" < "20" < "9").
        m.sort = Some((1, false));
        let mut r = rows.clone();
        m.apply_sort(&mut r);
        assert_eq!(names(&r), ["beta", "gamma", "alpha"]);
    }

    /// `M-DEL` (Buffer-menu-unmark-all-buffers) removes only the mark character it
    /// is given, unlike `U`, which removes every mark.
    #[test]
    fn unmark_all_buffers_removes_only_the_named_mark() {
        let mut m = menu(vec![
            row(1, "a", 1, "Text"),
            row(2, "b", 2, "Text"),
            row(3, "c", 3, "Text"),
        ]);
        m.menu.set_mark(1, Mark::Delete);
        m.menu.set_mark(2, Mark::Save);
        m.menu.set_mark(3, Mark::Delete);

        // `D` at the prompt drops the deletion flags and leaves the `S` alone.
        assert_eq!(m.unmark_all_buffers(Some('D')), 2);
        assert_eq!(m.menu.mark_of(1), Mark::None);
        assert_eq!(m.menu.mark_of(3), Mark::None);
        assert_eq!(m.menu.mark_of(2), Mark::Save, "an S flag is not a D flag");

        // A mark character nothing carries removes nothing.
        assert_eq!(m.unmark_all_buffers(Some('>')), 0);
        assert_eq!(m.menu.mark_of(2), Mark::Save);

        // RET (None) drops whatever is left.
        assert_eq!(m.unmark_all_buffers(None), 1);
        assert_eq!(m.menu.mark_of(2), Mark::None);
    }
}
