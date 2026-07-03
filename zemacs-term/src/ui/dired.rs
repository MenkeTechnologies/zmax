//! Dired — a directory-editor mode, the zemacs port of GNU Emacs Dired.
//!
//! A full-screen [`Component`] listing one directory. Each row is a file or
//! subdirectory with a left-hand mark column (`*` marked, `D` flagged for
//! deletion), a type/size column and the name. Marks are keyed by file **name**
//! so they survive re-sorting and refresh. All pure logic (sorting, human sizes,
//! name transforms, the mark glyph) lives in the filesystem-free, unit-tested
//! [`zemacs_core::dired`]; this module does the directory I/O, rendering and key
//! handling.
//!
//! Keys (parsed into a `dired` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs Dired counterpart in the port tracker):
//!   j/k/n/p/arrows, g/G/Home/End — move point
//!   Enter/f — visit file, or enter subdirectory in place
//!   ^ / - — go up to the parent directory
//!   m — mark; u — unmark (and advance); DEL — unmark previous; U — unmark all;
//!   t — toggle all marks
//!   d — flag for deletion (and advance); ~ flag backups; # flag auto-saves;
//!   & flag garbage (build/tex droppings); x — delete the flagged files;
//!   D — delete the marked files (or the file at point) immediately
//!   w — copy the marked names (or the name at point) to the clipboard
//!   s — cycle sort order (name/time/size/ext); r — reverse; `.` — toggle hidden
//!   R — refresh; q/Esc — quit
//!
//! Deferred to a later slice: in-mode copy/rename/mkdir (need a text prompt),
//! chmod/chown/chgrp, wdired (editable listing), subdirectory insertion.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use tui::buffer::Buffer as Surface;
use zemacs_core::dired::{human_size, mark_char, sort_entries, DiredEntry, SortKey};
use zemacs_view::{editor::Action, graphics::Rect};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The interactive Dired overlay for a single directory.
pub struct Dired {
    dir: PathBuf,
    entries: Vec<DiredEntry>,
    /// Marked / deletion-flagged entries, keyed by file name (survive re-sort).
    marked: HashSet<String>,
    flagged: HashSet<String>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    sort: SortKey,
    reverse: bool,
    show_hidden: bool,
    error: Option<String>,
}

impl Dired {
    /// Open Dired on `dir`, reading its contents. Errors if the directory can't
    /// be read.
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        let mut d = Dired {
            dir,
            entries: Vec::new(),
            marked: HashSet::new(),
            flagged: HashSet::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            sort: SortKey::Name,
            reverse: false,
            show_hidden: false,
            error: None,
        };
        d.read_dir()?;
        Ok(d)
    }

    /// Read `self.dir` into `self.entries` (respecting `show_hidden`) and sort.
    /// Marks/flags naming files no longer present are dropped.
    fn read_dir(&mut self) -> std::io::Result<()> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if !self.show_hidden && name.starts_with('.') {
                continue;
            }
            let ft = entry.file_type().ok();
            let meta = entry.metadata().ok();
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            entries.push(DiredEntry {
                name,
                is_dir: ft.map(|f| f.is_dir()).unwrap_or(false),
                is_symlink: ft.map(|f| f.is_symlink()).unwrap_or(false),
                size: meta.map(|m| m.len()).unwrap_or(0),
                mtime,
            });
        }
        sort_entries(&mut entries, self.sort, self.reverse);
        let present: HashSet<&String> = entries.iter().map(|e| &e.name).collect();
        self.marked.retain(|n| present.contains(n));
        self.flagged.retain(|n| present.contains(n));
        self.entries = entries;
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        self.error = None;
        Ok(())
    }

    fn resort(&mut self) {
        sort_entries(&mut self.entries, self.sort, self.reverse);
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    fn current_name(&self) -> Option<String> {
        self.entries.get(self.selected).map(|e| e.name.clone())
    }

    /// Names to act on: the marked set if non-empty, else the entry at point.
    fn targets(&self) -> Vec<String> {
        if !self.marked.is_empty() {
            self.entries
                .iter()
                .filter(|e| self.marked.contains(&e.name))
                .map(|e| e.name.clone())
                .collect()
        } else {
            self.current_name().into_iter().collect()
        }
    }

    fn toggle_all_marks(&mut self) {
        let mut next = HashSet::new();
        for e in &self.entries {
            if !self.marked.contains(&e.name) {
                next.insert(e.name.clone());
            }
        }
        self.marked = next;
    }

    /// Flag every entry whose name satisfies `pred` for deletion, returning the
    /// number newly flagged — the shared engine behind the Emacs `~`/`#`/`&`
    /// dired flag-by-pattern commands.
    fn flag_matching(&mut self, pred: impl Fn(&str) -> bool) -> usize {
        let mut n = 0;
        for e in &self.entries {
            if pred(&e.name) && self.flagged.insert(e.name.clone()) {
                n += 1;
            }
        }
        n
    }

    /// Delete a set of names from disk (files or directory trees). Returns the
    /// count deleted; records the first error.
    fn delete_names(&mut self, names: &[String]) -> usize {
        let mut n = 0;
        for name in names {
            let path = self.dir.join(name);
            let res = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match res {
                Ok(()) => {
                    self.marked.remove(name);
                    self.flagged.remove(name);
                    n += 1;
                }
                Err(e) => {
                    self.error = Some(format!("delete {name}: {e}"));
                    break;
                }
            }
        }
        n
    }

    /// Visit the entry at point: enter a subdirectory in place, or open a file
    /// (popping this overlay).
    fn visit(&mut self) -> Option<Callback> {
        let e = self.entries.get(self.selected)?;
        let path = self.dir.join(&e.name);
        if e.is_dir {
            self.dir = std::fs::canonicalize(&path).unwrap_or(path);
            self.selected = 0;
            self.scroll = 0;
            self.marked.clear();
            self.flagged.clear();
            if let Err(err) = self.read_dir() {
                self.error = Some(format!("{err}"));
            }
            None
        } else {
            Some(Box::new(
                move |compositor: &mut Compositor, cx: &mut Context| {
                    compositor.pop();
                    if let Err(err) = cx.editor.open(&path, Action::Replace) {
                        cx.editor
                            .set_error(format!("failed to open {}: {err}", path.display()));
                    }
                },
            ))
        }
    }

    /// Go up to the parent directory, selecting the directory we came from.
    fn up_dir(&mut self) {
        if let Some(parent) = self.dir.parent().map(|p| p.to_path_buf()) {
            let from = self
                .dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            self.dir = parent;
            self.marked.clear();
            self.flagged.clear();
            self.selected = 0;
            self.scroll = 0;
            if let Err(err) = self.read_dir() {
                self.error = Some(format!("{err}"));
            }
            if let Some(from) = from {
                if let Some(i) = self.entries.iter().position(|e| e.name == from) {
                    self.selected = i;
                }
            }
        }
    }
}

impl Component for Dired {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('k') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!('g') | key!(Home) => self.selected = 0,
            key!('G') | key!(End) => self.selected = self.entries.len().saturating_sub(1),
            key!('R') => {
                if let Err(err) = self.read_dir() {
                    self.error = Some(format!("{err}"));
                }
            }
            key!(Enter) | key!('f') => {
                if let Some(cb) = self.visit() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('^') | key!('-') => self.up_dir(),
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
            key!(Backspace) => {
                self.move_selection(-1);
                if let Some(n) = self.current_name() {
                    self.marked.remove(&n);
                    self.flagged.remove(&n);
                }
            }
            key!('U') => {
                self.marked.clear();
                self.flagged.clear();
            }
            key!('t') => self.toggle_all_marks(),
            key!('d') => {
                if let Some(n) = self.current_name() {
                    self.flagged.insert(n);
                    self.move_selection(1);
                }
            }
            key!('~') => {
                let n = self.flag_matching(zemacs_core::dired::is_backup_file);
                cx.editor
                    .set_status(format!("dired: flagged {n} backup file(s)"));
            }
            key!('#') => {
                let n = self.flag_matching(zemacs_core::dired::is_auto_save_file);
                cx.editor
                    .set_status(format!("dired: flagged {n} auto-save file(s)"));
            }
            key!('&') => {
                let n = self.flag_matching(zemacs_core::dired::is_garbage_file);
                cx.editor
                    .set_status(format!("dired: flagged {n} garbage file(s)"));
            }
            key!('x') => {
                let names: Vec<String> = self
                    .entries
                    .iter()
                    .filter(|e| self.flagged.contains(&e.name))
                    .map(|e| e.name.clone())
                    .collect();
                if names.is_empty() {
                    cx.editor.set_status("dired: no files flagged for deletion");
                } else {
                    let n = self.delete_names(&names);
                    let _ = self.read_dir();
                    cx.editor.set_status(format!("dired: deleted {n} file(s)"));
                }
            }
            key!('D') => {
                let names = self.targets();
                if !names.is_empty() {
                    let n = self.delete_names(&names);
                    let _ = self.read_dir();
                    cx.editor.set_status(format!("dired: deleted {n} file(s)"));
                }
            }
            key!('w') => {
                let names = self.targets();
                if !names.is_empty() {
                    let joined = names.join(" ");
                    let _ = cx.editor.registers.write('+', vec![joined.clone()]);
                    cx.editor.set_status(format!("dired: copied {joined}"));
                }
            }
            key!('s') => {
                self.sort = self.sort.next();
                self.resort();
                cx.editor
                    .set_status(format!("dired: sorted by {}", self.sort.label()));
            }
            key!('r') => {
                self.reverse = !self.reverse;
                self.resort();
            }
            key!('.') => {
                self.show_hidden = !self.show_hidden;
                if let Err(err) = self.read_dir() {
                    self.error = Some(format!("{err}"));
                }
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
        let dir_style = theme.get("ui.text.directory");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let mark_style = theme.get("diff.plus");
        let flag_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = format!(" Dired: {}", self.dir.display());
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "Enter open  m mark  d flag  x del  s sort  q quit";
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
        let body_h = area.height.saturating_sub(2);
        self.viewport = body_h as usize;

        if let Some(err) = &self.error {
            surface.set_stringn(area.x, area.y + 1, err, area.width as usize, flag_style);
        }

        if self.entries.is_empty() {
            surface.set_stringn(area.x, body_y, "(empty)", area.width as usize, info_style);
            return;
        }

        // Keep the selection in view.
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected + 1 - self.viewport;
        }

        for (offset, e) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let m = mark_char(
                self.marked.contains(&e.name),
                self.flagged.contains(&e.name),
            );
            let kind = if e.is_symlink {
                "l"
            } else if e.is_dir {
                "d"
            } else {
                "-"
            };
            let size = if e.is_dir {
                String::new()
            } else {
                human_size(e.size)
            };
            let name = if e.is_dir {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            };
            let line = format!("{} {} {:>7}  {}", m, kind, size, name);
            let base = if offset == self.selected {
                sel_style
            } else if e.is_dir {
                dir_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, &line, area.width as usize, base);
            // Accent the mark column.
            if m != ' ' {
                let ms = if m == 'D' { flag_style } else { mark_style };
                surface.set_stringn(area.x, y, &m.to_string(), 1, ms);
            }
        }

        // Footer: counts.
        let footer = format!(
            "{} items  {} marked  {} flagged  sort:{}{}",
            self.entries.len(),
            self.marked.len(),
            self.flagged.len(),
            self.sort.label(),
            if self.reverse { " (rev)" } else { "" }
        );
        if body_h > 0 {
            surface.set_stringn(
                area.x,
                area.y + area.height - 1,
                &footer,
                area.width as usize,
                info_style,
            );
        }
    }
}
