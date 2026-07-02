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
//!   g     — refresh the list (revert-buffer);  q/Esc — quit
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

/// The interactive Buffer Menu overlay.
pub struct BufferMenu {
    /// Pure model: rows, marks, cursor.
    menu: BufferMenuModel,
    /// Live document ids, index-aligned with `menu.rows()`.
    docs: Vec<DocumentId>,
    /// `T` toggle: list only file-visiting buffers.
    files_only: bool,
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
            docs: Vec::new(),
            files_only: false,
            scroll: 0,
            viewport: 1,
            status: String::new(),
        };
        menu.refresh(editor);
        menu
    }

    /// Rebuild the row list from the editor's documents (in `DocumentId` order,
    /// the `BTreeMap` order), preserving marks by buffer id. Honours `files_only`.
    fn refresh(&mut self, editor: &Editor) {
        let current = zemacs_view::current_ref!(editor).1.id();
        let mut rows = Vec::new();
        let mut docs = Vec::new();
        for doc in editor.documents() {
            if self.files_only && doc.path().is_none() {
                continue;
            }
            rows.push(BufferRow {
                key: doc_key(doc.id()),
                name: doc.display_name().into_owned(),
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
            docs.push(doc.id());
        }
        self.docs = docs;
        self.menu.set_rows(rows);
    }

    /// The document id under point, if any.
    fn current_doc(&self) -> Option<DocumentId> {
        self.docs.get(self.menu.selected()).copied()
    }

    /// The document id for a buffer key (matching a row).
    fn doc_for(&self, key: u64) -> Option<DocumentId> {
        self.menu
            .rows()
            .iter()
            .position(|r| r.key == key)
            .and_then(|i| self.docs.get(i).copied())
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
            key!('o') | key!('2') => {
                if let Some(cb) = self.select_current(Action::HorizontalSplit) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('v') => {
                if let Some(cb) = self.select_marked() {
                    return EventResult::Consumed(Some(cb));
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
            key!('U') | alt!(Backspace) => self.menu.unmark_all(),
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
            key!('g') | key!('R') => self.refresh(cx.editor),

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

        let name_w = self.menu.name_width();
        let title = format!(
            " Buffer Menu{}  CRM {:<name_w$}  {:>8}  {:<12}  File",
            if self.files_only { " (files only)" } else { "" },
            "Buffer",
            "Size",
            "Mode",
            name_w = name_w,
        );
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
            let line = self.menu.format_line(row, name_w);
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
