//! Buffer Menu — a buffer-list mode, the zmax port of GNU Emacs `buffer-menu`
//! / `list-buffers` (`C-x C-b`, `Buffer-menu-mode`).
//!
//! A full-screen [`Component`] listing the open buffers. Each row shows the Emacs
//! Buffer Menu columns: a leftmost mark tag (`>` selected, `D` delete, `S` save),
//! the `C R M` flags (current `.`, read-only `%`, modified `*`), the buffer name,
//! its size in characters, the major-mode/language name and the file path. All
//! pure state — the ordered rows, the marks (keyed by buffer id so they survive a
//! refresh) and the cursor — lives in the unit-tested [`zmax_core::buffer_menu`];
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
//!   t     — visit the buffer at point as a tags table
//!           (`Buffer-menu-visit-tags-table`)
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
//!
//! `:bs-show` opens this same menu under the **bs** customization group
//! ([`crate::buffer_menus`]) — the manual describes `bs-show` as "a buffer list
//! similar to the one normally displayed by `C-x C-b`, but whose display you can
//! customize", so [`BufferMenu::new_bs`] is that menu with the group's
//! attribute set, name-column bounds and show/don't-show regexps applied.

use tui::buffer::Buffer as Surface;
use zmax_core::buffer_menu::{BufferMenu as BufferMenuModel, BufferRow, Mark};
use zmax_view::{editor::Action, graphics::Rect, DocumentId, Editor};

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
    /// Point's horizontal offset in the row, in characters.
    ///
    /// Emacs's `tabulated-list-mode` has no separate column cursor: the "current
    /// column" is simply the one point is sitting in, which is why
    /// `tabulated-list-next-column` is a *point motion*. This is that position;
    /// [`BufferMenu::current_column`] derives the column from it.
    point_col: usize,
    /// The column the rows are sorted by, and whether that sort is reversed
    /// (`tabulated-list-sort`; `None` = the editor's own buffer order).
    sort: Option<(usize, bool)>,
    /// Per-column width adjustments from `{` / `}`, in characters.
    width_delta: [isize; COLUMNS.len()],
    /// `M-DEL`: the next key names the mark character to remove everywhere.
    pending_unmark: bool,
    /// The **bs** customization group when the menu was opened by `bs-show`;
    /// `None` for the plain `C-x C-b` Buffer Menu, which shows every column and
    /// filters nothing.
    bs: Option<crate::buffer_menus::Config>,
    scroll: usize,
    viewport: usize,
    /// The row width the last render laid the columns out at. Key handling has
    /// no `Rect`, and the column boundaries depend on it.
    last_width: usize,
    status: String,
}

/// The stable numeric key of a document (its `DocumentId`, a `NonZeroUsize`).
fn doc_key(id: DocumentId) -> u64 {
    // `DocumentId`'s only public projection is its `Display` (the numeric id);
    // it parses back deterministically for use as the mark key.
    id.to_string().parse().unwrap_or(0)
}

/// The `focused_at` stamp that sinks a buried buffer under every other one.
///
/// `bury-buffer` puts the buffer at the END of the buffer list, so every other
/// buffer counts as more recently used than it. zmax spells that order as
/// `Document::focused_at` — the key `Editor::sort_buffers` reads for
/// `BufferSortKey::LastUsed` and the buffer switcher walks — so burying means
/// stamping the document older than the oldest of the others. `None` when there
/// are no other buffers: nothing to sink below.
fn buried_focus_stamp(
    others: impl Iterator<Item = std::time::Instant>,
) -> Option<std::time::Instant> {
    let oldest = others.min()?;
    // `Instant` is monotonic-clock-relative and has no fixed zero, so a
    // subtraction can in principle underflow; back off only as far as it allows.
    oldest
        .checked_sub(std::time::Duration::from_millis(1))
        .or(Some(oldest))
}

impl BufferMenu {
    /// Open the Buffer Menu, snapshotting the editor's current buffers.
    pub fn new(editor: &Editor) -> Self {
        let mut menu = BufferMenu {
            menu: BufferMenuModel::default(),
            ids: std::collections::BTreeMap::new(),
            previous: Some(zmax_view::current_ref!(editor).1.id()),
            files_only: false,
            show_internal: false,
            point_col: 0,
            sort: None,
            width_delta: [0; COLUMNS.len()],
            pending_unmark: false,
            bs: None,
            scroll: 0,
            viewport: 1,
            last_width: 80,
            status: String::new(),
        };
        menu.refresh(editor);
        menu
    }

    /// `bs-show`: the same menu under the live **bs** customization group — its
    /// `bs-dont-show-regexp` / `bs-must-always-show-regexp` filters, its
    /// name-column bounds and the columns `bs-attributes-list` selects.
    pub fn new_bs(editor: &Editor) -> Self {
        let mut menu = Self::new(editor);
        menu.bs = Some(crate::buffer_menus::config());
        menu.refresh(editor);
        menu
    }

    /// Width of each column: the name column is measured from the rows, `Size`
    /// and `Mode` are fixed, and `File` takes the rest of the line — each then
    /// shifted by whatever `{` / `}` have done to it (never below 1).
    fn widths(&self, total: usize) -> [usize; COLUMNS.len()] {
        // `bs-minimal-buffer-name-column` / `bs-maximal-buffer-name-column`
        // bound the name column of a `bs-show` listing.
        let name = match &self.bs {
            Some(bs) => bs.name_column(self.menu.name_width()),
            None => self.menu.name_width(),
        };
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
        // `bs-attributes-list`: a `bs-show` listing draws only the attributes the
        // group selects. The plain Buffer Menu draws all of them.
        let attrs = self.bs.as_ref().map(|bs| bs.attributes).unwrap_or_default();
        let cell = |on: bool, text: String| if on { text } else { String::new() };
        format!(
            "{mark}{crm} {name:<nw$}{size}{mode}{file}",
            mark = self.menu.mark_of(row.key).glyph(),
            crm = cell(attrs.flags, self.menu.crm(row)),
            name = clip(&row.name, w[0]),
            size = cell(
                attrs.size,
                format!("  {:>w$}", clip(&row.size.to_string(), w[1]), w = w[1])
            ),
            mode = cell(
                attrs.mode,
                format!("  {:<w$}", clip(&row.mode, w[2]), w = w[2])
            ),
            file = cell(attrs.file, format!("  {}", clip(&row.file, w[3]))),
            nw = w[0],
        )
    }

    /// The character offset at which each column starts, in the layout
    /// [`BufferMenu::format_row`] draws: the mark glyph, then the `CRM` flag
    /// cell when the listing shows it, a space, and then the four columns, each
    /// of the later three preceded by the two-space gap. Pure — unit tested.
    fn column_starts(&self, w: &[usize; COLUMNS.len()]) -> [usize; COLUMNS.len()] {
        let attrs = self.bs.as_ref().map(|bs| bs.attributes).unwrap_or_default();
        let mut at = 1 + if attrs.flags { 3 } else { 0 } + 1;
        let mut out = [0usize; COLUMNS.len()];
        for (i, width) in w.iter().enumerate() {
            if i > 0 {
                at += 2; // the two-space gap before every column but the name
            }
            out[i] = at;
            at += width;
        }
        out
    }

    /// The column point is in — the last one whose start is at or before it,
    /// which is what `tabulated-list-get-column` resolves. Pure — unit tested
    /// through [`BufferMenu::column_starts`].
    fn current_column(&self) -> usize {
        let starts = self.column_starts(&self.widths(self.last_width));
        starts
            .iter()
            .rposition(|&start| start <= self.point_col)
            .unwrap_or(0)
    }

    /// Move point to the start of the column `delta` away from the one it is in
    /// — `tabulated-list-previous-column` / `-next-column`, which are point
    /// motions rather than a cursor index.
    fn move_column(&mut self, delta: isize) {
        let current = self.current_column() as isize;
        let target = (current + delta).clamp(0, COLUMNS.len() as isize - 1) as usize;
        self.point_col = self.column_starts(&self.widths(self.last_width))[target];
    }

    /// `tabulated-list-narrow-current-column` (`{`): make the column at point one
    /// character narrower.
    pub fn narrow_current_column(&mut self) {
        let column = self.current_column();
        self.width_delta[column] -= 1;
        self.status = format!("buffer-menu: narrowed {}", COLUMNS[column]);
    }

    /// `tabulated-list-widen-current-column` (`}`): make the column at point one
    /// character wider.
    pub fn widen_current_column(&mut self) {
        let column = self.current_column();
        self.width_delta[column] += 1;
        self.status = format!("buffer-menu: widened {}", COLUMNS[column]);
    }

    /// `tabulated-list-sort` (`S`): order the rows by the column at point. A
    /// second `S` on the same column reverses it.
    pub fn sort_by_column(&mut self, editor: &Editor) {
        let column = self.current_column();
        self.sort = match self.sort {
            Some((col, desc)) if col == column => Some((col, !desc)),
            _ => Some((column, false)),
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
        let current = zmax_view::current_ref!(editor).1.id();
        let mut rows = Vec::new();
        let mut ids = std::collections::BTreeMap::new();
        for doc in editor.documents() {
            if self.files_only && doc.path().is_none() {
                continue;
            }
            let name = doc.display_name().into_owned();
            if !self.show_internal
                && doc.path().is_none()
                && zmax_core::buffer_menu::is_internal_name(&name)
            {
                continue;
            }
            // `bs-dont-show-regexp` / `bs-must-always-show-regexp`, the filters
            // that make `bs-show` a *customizable* buffer list.
            if self.bs.as_ref().is_some_and(|bs| !bs.shows(&name)) {
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

    /// `t` (`Buffer-menu-visit-tags-table`): visit the file of the buffer at
    /// point as a tags table, i.e. `visit-tags-table` on that file. zmax's tags
    /// table is the `tags` option the `:tag` family reads, so pointing it at
    /// this file is what makes the subsequent lookups use it. The file is parsed
    /// first, so a buffer that is not a tags table is refused rather than
    /// silently breaking every later `:tag`.
    fn visit_tags_table(&mut self) {
        let Some(id) = self.current_doc() else {
            self.status = "buffer-menu: no buffer at point".to_string();
            return;
        };
        let Some(row) = self
            .menu
            .rows()
            .iter()
            .find(|r| self.doc_for(r.key) == Some(id))
        else {
            return;
        };
        if row.file.is_empty() {
            self.status = "buffer-menu: that buffer is not visiting a file".to_string();
            return;
        }
        match crate::commands::set_tags_table(std::path::Path::new(&row.file)) {
            Ok(tags) => self.status = format!("buffer-menu: {tags} tags from {}", row.file),
            Err(err) => self.status = format!("buffer-menu: {err}"),
        }
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
                zmax_view::input::KeyEvent {
                    code: zmax_view::keyboard::KeyCode::Char(c),
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
            // b (`Buffer-menu-bury`): sink the buffer at point to the bottom of
            // the list AND call `bury-buffer` on it, which moves it to the end
            // of the global buffer list (buf-menu.el). zmax's most-recently-used
            // order is `Document::focused_at` (`BufferSortKey::LastUsed`,
            // zmax-view/src/editor.rs), so the global half is a demotion of that
            // stamp; without it the buffer switcher still offered the buried
            // buffer first. Emacs echoes "Buffer buried." for the live buffer.
            key!('b') => {
                if let Some(key) = self.menu.bury_current() {
                    if let Some(id) = self.doc_for(key) {
                        let stamp = buried_focus_stamp(
                            cx.editor
                                .documents()
                                .filter(|doc| doc.id() != id)
                                .map(|doc| doc.focused_at),
                        );
                        if let (Some(stamp), Some(doc)) = (stamp, cx.editor.document_mut(id)) {
                            doc.focused_at = stamp;
                        }
                    }
                    self.status = "Buffer buried.".to_string();
                }
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
            // t (`Buffer-menu-visit-tags-table`): use this line's file as the
            // tags table from now on.
            key!('t') => self.visit_tags_table(),

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
            // `tabulated-list-previous-column` / `-next-column`: point motions.
            alt!(Left) => self.move_column(-1),
            alt!(Right) => self.move_column(1),
            // Point moves along the row too, which is what makes the column
            // commands act on "the column at point" rather than on a cursor the
            // user has to drive separately.
            key!(Left) | ctrl!('b') => self.point_col = self.point_col.saturating_sub(1),
            key!(Right) | ctrl!('f') => {
                self.point_col = (self.point_col + 1).min(self.last_width.saturating_sub(1))
            }
            key!('S') => self.sort_by_column(cx.editor),
            key!('{') => self.narrow_current_column(),
            key!('}') => self.widen_current_column(),

            _ => {}
        }
        // Stay modal: never leak keys to the editor behind us.
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

        self.last_width = area.width as usize;
        let widths = self.widths(area.width as usize);
        let current = self.current_column();
        // Header row: the same columns as the rows, with the column point is in
        // called out (←/→ and M-←/M-→ move point; S / { / } act on that column).
        let files_only = if self.files_only { " (files only)" } else { "" };
        let mut title = format!(" Buffer Menu{files_only}  CRM");
        for (i, label) in COLUMNS.iter().enumerate() {
            // `^`/`v` mark the sort column and its direction; `[` the column cursor.
            let arrow = match self.sort {
                Some((c, true)) if c == i => "v",
                Some((c, false)) if c == i => "^",
                _ => "",
            };
            let cursor = if i == current { "[" } else { " " };
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
            point_col: 0,
            sort: None,
            width_delta: [0; COLUMNS.len()],
            pending_unmark: false,
            bs: None,
            scroll: 0,
            viewport: 10,
            last_width: 80,
            status: String::new(),
        }
    }

    /// The column commands act on the column POINT is in, as
    /// `tabulated-list-mode` does — there is no separate cursor to drive. The
    /// boundaries must line up with the layout `format_row` actually draws.
    #[test]
    fn the_current_column_is_the_one_point_sits_in() {
        let mut m = menu(vec![row(1, "alpha", 10, "Rust")]);
        let w = m.widths(m.last_width);
        let starts = m.column_starts(&w);

        // The name cell begins after the mark glyph, the `C R M` triple and a
        // space — the same offset the existing row-format test skips to.
        assert_eq!(starts[0], 5);
        // Each later column starts after the two-space gap that precedes it.
        assert_eq!(starts[1], starts[0] + w[0] + 2);
        assert_eq!(starts[2], starts[1] + w[1] + 2);
        assert_eq!(starts[3], starts[2] + w[2] + 2);

        // Point anywhere inside a column names that column, including its first
        // and last character; before the first column it is still the first.
        for (col, start) in starts.iter().enumerate() {
            m.point_col = *start;
            assert_eq!(m.current_column(), col, "at the start of {}", COLUMNS[col]);
            m.point_col = start + w[col] - 1;
            assert_eq!(m.current_column(), col, "at the end of {}", COLUMNS[col]);
        }
        m.point_col = 0;
        assert_eq!(m.current_column(), 0);

        // `tabulated-list-next-column` is a point motion: it leaves point at the
        // start of the next column, and stops at the last one.
        m.point_col = 0;
        m.move_column(1);
        assert_eq!(m.point_col, starts[1]);
        assert_eq!(m.current_column(), 1);
        m.move_column(-1);
        assert_eq!(m.point_col, starts[0]);
        for _ in 0..10 {
            m.move_column(1);
        }
        assert_eq!(m.current_column(), COLUMNS.len() - 1);
        for _ in 0..10 {
            m.move_column(-1);
        }
        assert_eq!(m.current_column(), 0);
    }

    /// `{` / `}` / `S` read that same column, so moving point into the Size
    /// column and pressing `{` narrows Size — not whatever a cursor last named.
    #[test]
    fn the_column_commands_follow_point() {
        let mut m = menu(vec![row(1, "alpha", 10, "Rust")]);
        let starts = m.column_starts(&m.widths(m.last_width));
        m.point_col = starts[1];
        m.narrow_current_column();
        assert_eq!(m.width_delta[1], -1);
        assert_eq!(m.width_delta[0], 0, "the name column is untouched");
        m.widen_current_column();
        m.widen_current_column();
        assert_eq!(m.width_delta[1], 1);
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

        // Point moves into Size (M-→), so `}` widens Size, not Buffer.
        m.move_column(1);
        assert_eq!(m.current_column(), 1);
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

    /// `Buffer-menu-bury` does not only reorder the menu: it calls `bury-buffer`,
    /// which sends the buffer to the end of the GLOBAL buffer list. zmax's
    /// most-recently-used order is `Document::focused_at`, so the buried buffer's
    /// stamp has to land under every other buffer's — a buried buffer the
    /// switcher still offers first is the bug this covers.
    #[test]
    fn burying_demotes_the_buffer_in_the_most_recently_used_order() {
        let base = std::time::Instant::now();
        let older = base + std::time::Duration::from_secs(1);
        let newer = base + std::time::Duration::from_secs(2);

        // Burying the most recently used buffer stamps it under both others.
        let buried = buried_focus_stamp([newer, older].into_iter()).expect("two other buffers");
        assert!(buried < older, "under the oldest of the others");
        assert!(buried < newer);

        // Which is exactly what `Editor::sort_buffers` (BufferSortKey::LastUsed,
        // `Reverse(focused_at)`) needs to place it last.
        let mut order = [("a", newer), ("b", older), ("buried", buried)];
        order.sort_by_key(|&(_, at)| std::cmp::Reverse(at));
        let names: Vec<&str> = order.iter().map(|&(n, _)| n).collect();
        assert_eq!(names, ["a", "b", "buried"]);

        // The only buffer has nothing to sink below, so its stamp is left alone.
        assert_eq!(buried_focus_stamp(std::iter::empty()), None);
    }
}
