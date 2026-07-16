//! Bookmark menu — the zmax port of the GNU Emacs `*Bookmark List*` buffer
//! (`bookmark-bmenu-mode`, opened by `list-bookmarks` / `bookmark-bmenu-list`,
//! `C-x r l`).
//!
//! A full-screen [`Component`] listing the persistent named bookmarks, one per
//! row: a left-hand mark column (`*` marked, `D` flagged for deletion), the
//! bookmark name, and its `file:line` location. Marks and deletion flags are
//! keyed by bookmark **name** so they survive save/reload. All record logic
//! (set/delete/rename/find and the text serialization) lives in the
//! filesystem-free, unit-tested [`zmax_core::bookmark::BookmarkStore`]; this
//! module owns a working copy of that store, does the file I/O, and renders the
//! list.
//!
//! Keys (each an Emacs `bookmark-bmenu-*` counterpart):
//!   j/k/n/p/arrows, g/G/Home/End — move point
//!   Enter/f — bookmark-jump / bookmark-bmenu-this-window: open the file at its
//!             stored line (pops this overlay)
//!   m — bookmark-bmenu-mark;  u — bookmark-bmenu-unmark
//!   d — bookmark-bmenu-delete (flag for deletion);
//!   x — bookmark-bmenu-execute-deletions (delete flagged from the store *and*
//!       persist to the bookmarks file)
//!   r — bookmark-bmenu-rename: renames the bookmark at point to a fresh,
//!       collision-free suffixed name (`<name>-1`, `-2`, …) and persists. A
//!       real minibuffer prompt is deferred; this overlay can't read one, so it
//!       exercises the substrate `rename` with a generated unique name.
//!   s — bookmark-bmenu-save / bookmark-save: write the store to the file
//!   l — bookmark-bmenu-load / bookmark-load: reload the store from the file
//!       (discarding any unsaved marks/flags)
//!   q/Esc/C-c — quit
//!
//! Deferred: `bookmark-set` (`C-x r m`) is a *global* command, not a bmenu key —
//! it needs the current buffer's file and point, which this overlay can't see;
//! it lives in `commands.rs` / `emacs_bookmark`. This overlay is the bmenu plus
//! jump and management only.
//!
//! The bookmarks file is `<config-dir>/bookmarks` (via `zmax_loader::config_dir`),
//! the same file the global `C-x r m` / `bookmark-jump` commands use, so the menu
//! and those commands share one persistent list. (The task's suggested
//! `$HOME/.zmax-bookmarks` is deliberately *not* used: reading the app's real
//! bookmark file keeps the bmenu consistent with the rest of the editor.)

use std::collections::HashSet;
use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zmax_core::bookmark::BookmarkStore;
use zmax_loader::config_dir;
use zmax_view::{editor::Action, graphics::Rect};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The interactive Bookmark Menu overlay.
pub struct BookmarkMenu {
    /// Working copy of the persistent store; mutated in place, written back on
    /// `s`/`x`/`r`.
    store: BookmarkStore,
    /// Marked / deletion-flagged bookmarks, keyed by name (survive reordering).
    marked: HashSet<String>,
    flagged: HashSet<String>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    status: Option<String>,
}

impl BookmarkMenu {
    /// Open the Bookmark Menu, loading the bookmarks file if it exists (an empty
    /// list otherwise).
    pub fn new() -> Self {
        BookmarkMenu {
            store: load_store(),
            marked: HashSet::new(),
            flagged: HashSet::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            status: None,
        }
    }

    /// The bookmark name at point, if any.
    fn current_name(&self) -> Option<String> {
        self.store.list().get(self.selected).map(|(n, _)| n.clone())
    }

    fn move_selection(&mut self, delta: isize) {
        let n = self.store.len();
        if n == 0 {
            return;
        }
        let max = n as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    fn clamp_selection(&mut self) {
        if self.selected >= self.store.len() {
            self.selected = self.store.len().saturating_sub(1);
        }
    }

    /// A bookmark name derived from `base` that is not already in the store,
    /// appending `-1`, `-2`, … — the collision-free rename target used since the
    /// overlay can't prompt for a name.
    fn unique_name(&self, base: &str) -> String {
        let mut i = 1;
        loop {
            let cand = format!("{base}-{i}");
            if self.store.get(&cand).is_none() {
                return cand;
            }
            i += 1;
        }
    }

    /// Write the working store back to the bookmarks file.
    fn persist(&self) -> std::io::Result<()> {
        let path = bookmarks_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, self.store.serialize())
    }

    /// Reload the store from disk, dropping unsaved marks/flags.
    fn reload(&mut self) {
        self.store = load_store();
        self.marked.clear();
        self.flagged.clear();
        self.clamp_selection();
        self.status = Some(format!("Loaded {} bookmark(s)", self.store.len()));
    }

    /// Jump to the bookmark at point: open its file and move point to the stored
    /// `(line, column)`, popping this overlay — `bookmark-jump`.
    fn jump(&self) -> Option<Callback> {
        let (_, b) = self.store.list().get(self.selected)?;
        let path = PathBuf::from(&b.file);
        let line = b.line;
        let column = b.column;
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                if let Err(err) = cx.editor.open(&path, Action::Replace) {
                    cx.editor
                        .set_error(format!("failed to open {}: {err}", path.display()));
                    return;
                }
                let (view, doc) = current!(cx.editor);
                let text = doc.text();
                let line_idx = line.min(text.len_lines().saturating_sub(1));
                let line_start = text.line_to_char(line_idx);
                // Clamp the column inside the line (excluding its trailing newline).
                let line_len = text.line(line_idx).len_chars().saturating_sub(1);
                let col = column.unwrap_or(0).min(line_len);
                let pos = (line_start + col).min(text.len_chars());
                doc.set_selection(view.id, zmax_core::Selection::point(pos));
            },
        ))
    }
}

impl Default for BookmarkMenu {
    fn default() -> Self {
        Self::new()
    }
}

/// The bookmarks file path (`<config-dir>/bookmarks`), shared with the global
/// bookmark commands.
fn bookmarks_path() -> PathBuf {
    config_dir().join("bookmarks")
}

/// Read the persisted store (empty if the file is missing or unreadable).
fn load_store() -> BookmarkStore {
    match std::fs::read_to_string(bookmarks_path()) {
        Ok(s) => BookmarkStore::deserialize(&s),
        Err(_) => BookmarkStore::new(),
    }
}

impl Component for BookmarkMenu {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        self.status = None;
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('k') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!('g') | key!(Home) => self.selected = 0,
            key!('G') | key!(End) => self.selected = self.store.len().saturating_sub(1),
            key!(Enter) | key!('f') => {
                if let Some(cb) = self.jump() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('m') => {
                if let Some(n) = self.current_name() {
                    self.marked.insert(n);
                    self.move_selection(1);
                }
            }
            key!('u') => {
                if let Some(n) = self.current_name() {
                    self.marked.remove(&n);
                    self.flagged.remove(&n);
                    self.move_selection(1);
                }
            }
            key!('d') => {
                if let Some(n) = self.current_name() {
                    self.flagged.insert(n);
                    self.move_selection(1);
                }
            }
            key!('x') => {
                let names: Vec<String> = self
                    .store
                    .list()
                    .iter()
                    .filter(|(n, _)| self.flagged.contains(n))
                    .map(|(n, _)| n.clone())
                    .collect();
                if names.is_empty() {
                    self.status = Some("No bookmarks flagged for deletion".into());
                } else {
                    for n in &names {
                        self.store.delete(n);
                        self.marked.remove(n);
                    }
                    self.flagged.clear();
                    self.clamp_selection();
                    self.status = Some(match self.persist() {
                        Ok(()) => format!("Deleted {} bookmark(s)", names.len()),
                        Err(e) => format!("deleted, but save failed: {e}"),
                    });
                }
            }
            key!('r') => {
                if let Some(old) = self.current_name() {
                    let new = self.unique_name(&old);
                    if self.store.rename(&old, &new) {
                        if self.flagged.remove(&old) {
                            self.flagged.insert(new.clone());
                        }
                        if self.marked.remove(&old) {
                            self.marked.insert(new.clone());
                        }
                        self.status = Some(match self.persist() {
                            Ok(()) => format!("Renamed '{old}' -> '{new}'"),
                            Err(e) => format!("renamed, but save failed: {e}"),
                        });
                    }
                }
            }
            key!('s') => {
                self.status = Some(match self.persist() {
                    Ok(()) => format!(
                        "Wrote {} bookmark(s) to {}",
                        self.store.len(),
                        bookmarks_path().display()
                    ),
                    Err(e) => format!("save failed: {e}"),
                });
            }
            key!('l') => self.reload(),
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
        let file_style = theme.get("ui.text.directory");
        let status_style = theme.get("warning");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = format!(" Bookmarks ({})", self.store.len());
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);

        if let Some(s) = &self.status {
            surface.set_stringn(area.x, area.y + 1, s, area.width as usize, status_style);
        }

        let body_y = area.y + 2;
        let list_h = area.height.saturating_sub(3) as usize;
        self.viewport = list_h.max(1);

        if self.store.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "(no bookmarks — set one with C-x r m)",
                area.width as usize,
                info_style,
            );
            return;
        }

        // Keep the selection in view.
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected + 1 - self.viewport;
        }

        for (offset, (name, b)) in self
            .store
            .list()
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(list_h)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let m = if self.flagged.contains(name) {
                'D'
            } else if self.marked.contains(name) {
                '*'
            } else {
                ' '
            };
            let base = if offset == self.selected {
                sel_style
            } else {
                text_style
            };
            let prefix = format!("{} {}  ", m, name);
            surface.set_stringn(area.x, y, &prefix, area.width as usize, base);
            let plen = prefix.chars().count();
            if (plen as u16) < area.width {
                let loc = format!("{}:{}", b.file, b.line + 1);
                let loc_style = if offset == self.selected {
                    base
                } else {
                    file_style
                };
                surface.set_stringn(
                    area.x + plen as u16,
                    y,
                    &loc,
                    (area.width as usize).saturating_sub(plen),
                    loc_style,
                );
            }
            // Accent the mark column.
            if m != ' ' {
                let ms = if m == 'D' { flag_style } else { mark_style };
                surface.set_stringn(area.x, y, &m.to_string(), 1, ms);
            }
        }

        let footer =
            " Enter jump  m mark  u unmark  d flag  x delete  r rename  s save  l load  q quit";
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            footer,
            area.width as usize,
            info_style,
        );
    }
}
