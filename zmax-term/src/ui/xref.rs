//! Xref — a cross-reference / find-definitions overlay, the zmax port of the
//! GNU Emacs `xref` results buffer (`*xref*`).
//!
//! A full-screen modal [`Component`]. Given a `root` directory and a `symbol`, it
//! walks the tree (skipping `.git`, `target`, `node_modules`), reads each text
//! file and asks the filesystem-free, unit-tested [`zmax_core::xref`] substrate
//! for every whole-word occurrence. Hits are grouped by file, one file header per
//! group followed by its `line: text` rows; lines that look like a *definition*
//! are highlighted. Visiting a hit opens its file and moves point to the match.
//!
//! Keys (parsed into an `xref` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs Xref counterpart in the port tracker):
//!   n / Down — next hit (`xref-next-line`)
//!   p / Up   — previous hit (`xref-prev-line`)
//!   } / M-n  — next file group (`xref-next-group`)
//!   { / M-p  — previous file group (`xref-prev-group`)
//!   Enter    — go to the hit, closing the overlay (`xref-goto-xref`)
//!   o        — quit and go to the hit (`xref-quit-and-goto-xref`)
//!   g        — rescan (`xref-show-xrefs` / revert)
//!   q / Esc  — quit (`xref-quit`)
//!
//! Deferred: `xref-query-replace-in-results` (needs an editable results buffer)
//! and the marker-stack navigation (`xref-go-back` / `xref-go-forward`), which
//! belong to the editor rather than this overlay.

use std::path::{Path, PathBuf};

use tui::buffer::Buffer as Surface;
use zmax_view::editor::Action;
use zmax_view::graphics::Rect;

use zmax_core::xref::{find_matches, group_by_file, looks_like_definition, XrefHit};

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Don't descend into these directory names while scanning the tree.
const SKIP_DIRS: [&str; 3] = [".git", "target", "node_modules"];
/// Cap the scan so a huge tree can't stall the overlay.
const MAX_FILES: usize = 5000;
const MAX_FILE_BYTES: u64 = 1 << 20; // 1 MiB

/// One rendered row: a file header, or a hit (index into [`Xref::hits`]).
enum Row {
    Header(String),
    Hit(usize),
}

/// The interactive Xref results overlay.
pub struct Xref {
    root: PathBuf,
    symbol: String,
    hits: Vec<XrefHit>,
    rows: Vec<Row>,
    /// Index into `rows`; always kept on a `Row::Hit` when any hit exists.
    selected: usize,
    scroll: usize,
    viewport: usize,
    error: Option<String>,
}

impl Xref {
    /// Build the overlay: canonicalize `root`, scan the tree for `symbol` and
    /// group the hits. Tolerates an empty `symbol` (shows a hint, no hits).
    pub fn new(root: PathBuf, symbol: String) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        let mut x = Xref {
            root,
            symbol,
            hits: Vec::new(),
            rows: Vec::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            error: None,
        };
        x.rescan();
        Ok(x)
    }

    /// Re-read the tree and recompute hits and rows for the current `symbol`.
    fn rescan(&mut self) {
        self.hits.clear();
        self.rows.clear();
        self.selected = 0;
        self.scroll = 0;
        if self.symbol.trim().is_empty() {
            self.error = Some("no symbol — nothing to search".to_string());
            return;
        }
        let files = read_tree(&self.root);
        self.hits = find_matches(&files, self.symbol.trim());
        self.build_rows();
        self.selected = self.first_hit_row();
        self.error = if self.hits.is_empty() {
            Some(format!("no matches for `{}`", self.symbol.trim()))
        } else {
            None
        };
    }

    /// Interleave file headers and hit rows from the grouped hits.
    fn build_rows(&mut self) {
        let groups = group_by_file(&self.hits);
        // Map each hit back to its index so a `Row::Hit(usize)` can address it.
        let mut rows = Vec::new();
        let mut idx = 0usize;
        for (path, group) in groups {
            rows.push(Row::Header(path));
            for _ in group {
                rows.push(Row::Hit(idx));
                idx += 1;
            }
        }
        self.rows = rows;
    }

    fn first_hit_row(&self) -> usize {
        self.rows
            .iter()
            .position(|r| matches!(r, Row::Hit(_)))
            .unwrap_or(0)
    }

    /// Move the cursor `delta` hit-rows (skipping headers), staying in range.
    fn move_cursor(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as isize;
        let mut i = self.selected as isize;
        loop {
            i += delta;
            if i < 0 || i >= n {
                return;
            }
            if matches!(self.rows[i as usize], Row::Hit(_)) {
                self.selected = i as usize;
                return;
            }
        }
    }

    /// Jump to the first hit of the next (`dir = 1`) or previous (`dir = -1`)
    /// file group.
    fn move_group(&mut self, dir: isize) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as isize;
        let mut i = self.selected as isize;
        loop {
            i += dir;
            if i < 0 || i >= n {
                return;
            }
            if matches!(self.rows[i as usize], Row::Header(_)) {
                // Land on the first hit below this header.
                for j in (i as usize + 1)..self.rows.len() {
                    if matches!(self.rows[j], Row::Hit(_)) {
                        self.selected = j;
                        return;
                    }
                }
                return;
            }
        }
    }

    fn current_hit(&self) -> Option<&XrefHit> {
        match self.rows.get(self.selected)? {
            Row::Hit(i) => self.hits.get(*i),
            Row::Header(_) => None,
        }
    }

    /// Build the callback that pops this overlay, opens the hit's file and moves
    /// point to the match (line/column). Mirrors the org-agenda / occur visit.
    fn goto(&self) -> Option<Callback> {
        let hit = self.current_hit()?;
        let path = PathBuf::from(&hit.path);
        let line = hit.line; // 1-based
        let col = hit.col;
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                if let Err(err) = cx.editor.open(&path, Action::Replace) {
                    cx.editor
                        .set_error(format!("failed to open {}: {err}", path.display()));
                    return;
                }
                let scrolloff = cx.editor.config().scrolloff;
                let (view, doc) = current!(cx.editor);
                let last = doc.text().len_lines().saturating_sub(1);
                let target = line.saturating_sub(1).min(last);
                let pos = (doc.text().line_to_char(target) + col).min(doc.text().len_chars());
                doc.set_selection(view.id, zmax_core::Selection::point(pos));
                view.ensure_cursor_in_view(doc, scrolloff);
            },
        ))
    }

    /// The path shown for a file header: relative to `root` when possible.
    fn rel(&self, path: &str) -> String {
        Path::new(path)
            .strip_prefix(&self.root)
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string())
    }
}

/// Walk `root` breadth/depth-first, reading every UTF-8 text file (skipping the
/// build/VCS directories and oversized/binary files) into `(path, contents)`
/// pairs keyed by absolute path. Unreadable entries are silently skipped.
fn read_tree(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let path = entry.path();
            if ft.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
            } else if ft.is_file() {
                if files.len() >= MAX_FILES {
                    return files;
                }
                if entry
                    .metadata()
                    .map(|m| m.len() > MAX_FILE_BYTES)
                    .unwrap_or(true)
                {
                    continue;
                }
                // read_to_string fails on non-UTF-8 (binary) files, skipping them.
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    files.push((path.to_string_lossy().into_owned(), contents));
                }
            }
        }
    }
    files
}

impl Component for Xref {
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
            key!('n') | key!(Down) | ctrl!('n') => self.move_cursor(1),
            key!('p') | key!(Up) | ctrl!('p') => self.move_cursor(-1),
            key!('j') => self.move_cursor(1),
            key!('k') => self.move_cursor(-1),
            key!('}') | alt!('n') => self.move_group(1),
            key!('{') | alt!('p') => self.move_group(-1),
            key!('g') => self.rescan(),
            key!(Enter) | key!('o') => {
                if let Some(cb) = self.goto() {
                    return EventResult::Consumed(Some(cb));
                }
            }
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
        let dir_style = theme.get("ui.text.directory");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let def_style = theme.get("function");
        let err_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = format!(" Xref: {}   ({} hits)", self.symbol.trim(), self.hits.len());
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);

        if let Some(err) = &self.error {
            surface.set_stringn(area.x, area.y + 1, err, area.width as usize, err_style);
        }

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h as usize;

        if self.rows.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "(no results — press g to rescan, q to quit)",
                area.width as usize,
                info_style,
            );
            return;
        }

        // Keep the selection in view.
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.viewport > 0 && self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected + 1 - self.viewport;
        }

        for (offset, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            match row {
                Row::Header(path) => {
                    let line = format!("{}:", self.rel(path));
                    surface.set_stringn(area.x, y, &line, area.width as usize, dir_style);
                }
                Row::Hit(i) => {
                    let hit = &self.hits[*i];
                    let line = format!("  {:>5}: {}", hit.line, hit.text.trim_start());
                    let base = if offset == self.selected {
                        sel_style
                    } else if looks_like_definition(&hit.text, self.symbol.trim()) {
                        def_style
                    } else {
                        text_style
                    };
                    surface.set_stringn(area.x, y, &line, area.width as usize, base);
                }
            }
        }

        let footer = "n/p move  }/{ group  RET/o goto  g rescan  q quit";
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            footer,
            area.width as usize,
            info_style,
        );
    }
}
