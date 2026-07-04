//! Project — the zemacs port of GNU Emacs `project.el`.
//!
//! A full-screen [`Component`] presenting the current project's files as a
//! fuzzy-filterable finder (Emacs `project-find-file`). On [`Project::new`] we
//! walk up from the start directory looking for a project marker (`.git`,
//! `Cargo.toml`, `package.json`, `.hg`, `Makefile`, `.project`) to fix the root,
//! recursively list the files under it (skipping `.git`, `target`,
//! `node_modules`), then rank them live against a typed query. All the pure
//! ranking / root-detection logic lives in the filesystem-free, unit-tested
//! [`zemacs_core::project`]; this module does the directory I/O, rendering and
//! key handling.
//!
//! Keys (parsed into a `project` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs `project.el` counterpart in the port tracker):
//!   printable chars ([a-z0-9], `- _ . /`) — append to the filter query
//!   Backspace — delete the last query character
//!   n / C-n / Down, p / C-p / Up — move the selection
//!   Enter — visit the selected file (`project-find-file`, pops this overlay)
//!   d — `project-find-dir`      k — `project-kill-buffers`
//!   D — `project-dired` (opens Dired on the root)
//!   g — `project-find-regexp`   c — `project-compile`
//!   b — `project-switch-to-buffer`   s — `project-shell`
//!   q / Esc / C-c — quit
//!
//! Because the finder types letters into a query, the single-key project
//! commands (n p d k D g c b s q) are reserved from the query — the same
//! precedence Emacs's own `C-x p` prefix gives them. `project-find-dir`,
//! `project-kill-buffers`, `project-find-regexp`, `project-compile`,
//! `project-switch-to-buffer` and `project-shell` are DEFERRED to a later slice
//! (they need a target buffer/process host); each is wired to a status line so
//! the keys resolve in the port report. Only `project-find-file` (Enter) and
//! `project-dired` (`D`) act for real here.

use std::path::{Path, PathBuf};

use tui::buffer::Buffer as Surface;
use zemacs_core::project::{rank, PROJECT_MARKERS};
use zemacs_view::editor::Action;
use zemacs_view::graphics::Rect;
use zemacs_view::keyboard::{KeyCode, KeyModifiers};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Directory names never descended into when listing a project's files.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules"];

/// Cap on the number of files listed, so an accidental root at `/` can't hang the
/// finder scanning the whole disk.
const MAX_FILES: usize = 50_000;

/// The interactive Project file-finder overlay.
pub struct Project {
    /// The detected project root; all listed paths are relative to it.
    root: PathBuf,
    /// Every file under the root (relative paths), in lexical order.
    files: Vec<String>,
    /// The current query and the files ranked against it (relative paths).
    query: String,
    ranked: Vec<String>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    error: Option<String>,
}

impl Project {
    /// Open the Project finder, detecting the root by walking up from `start` and
    /// listing the files beneath it. Errors only if the resolved root cannot be
    /// read at all.
    pub fn new(start: PathBuf) -> std::io::Result<Self> {
        let start = std::fs::canonicalize(&start).unwrap_or(start);
        let root = detect_root_on_disk(&start);
        let mut files = Vec::new();
        collect_files(&root, &root, &mut files)?;
        files.sort();
        let ranked = files.clone();
        Ok(Project {
            root,
            files,
            query: String::new(),
            ranked,
            selected: 0,
            scroll: 0,
            viewport: 1,
            error: None,
        })
    }

    /// Re-filter and re-order `ranked` for the current query, keeping the
    /// selection in bounds.
    fn rerank(&mut self) {
        self.ranked = rank(&self.files, &self.query)
            .into_iter()
            .map(str::to_string)
            .collect();
        self.selected = 0;
        self.scroll = 0;
    }

    fn move_selection(&mut self, delta: isize) {
        if self.ranked.is_empty() {
            return;
        }
        let max = self.ranked.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Visit the selected file: pop this overlay and open it in the editor.
    fn open_selected(&self) -> Option<Callback> {
        let name = self.ranked.get(self.selected)?;
        let path = self.root.join(name);
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

    /// `project-dired`: pop this overlay and open Dired on the project root.
    fn open_dired(&self) -> Callback {
        let root = self.root.clone();
        Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            compositor.pop();
            match crate::ui::dired::Dired::new(root) {
                Ok(d) => compositor.push(Box::new(d)),
                Err(e) => cx.editor.set_error(format!("project-dired: {e}")),
            }
        })
    }
}

/// Characters that extend the fuzzy query (Emacs `project-find-file` completion).
fn is_query_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/')
}

/// Walk up from `start` on disk, returning the nearest ancestor containing a
/// project marker, or `start` itself if none is found.
fn detect_root_on_disk(start: &Path) -> PathBuf {
    for ancestor in start.ancestors() {
        if PROJECT_MARKERS.iter().any(|m| ancestor.join(m).exists()) {
            return ancestor.to_path_buf();
        }
    }
    start.to_path_buf()
}

/// Recursively collect files under `dir` as paths relative to `root`, skipping
/// the [`SKIP_DIRS`] and stopping at [`MAX_FILES`].
fn collect_files(dir: &Path, root: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    if out.len() >= MAX_FILES {
        return Ok(());
    }
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        // A subdirectory we can't read is skipped; only a wholly unreadable root
        // surfaces as an error (handled by the caller reading the root first).
        Err(e) if dir == root => return Err(e),
        Err(_) => return Ok(()),
    };
    for entry in read.flatten() {
        if out.len() >= MAX_FILES {
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            collect_files(&entry.path(), root, out)?;
        } else {
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            out.push(rel);
        }
    }
    Ok(())
}

impl Component for Project {
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
            key!(Enter) => {
                if let Some(cb) = self.open_selected() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('n') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('p') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!(Backspace) => {
                self.query.pop();
                self.rerank();
            }
            // project-dired: wired for real.
            key!('D') => return EventResult::Consumed(Some(self.open_dired())),
            // Deferred command-style entries: acknowledge so the keys resolve.
            key!('d') => cx
                .editor
                .set_status("project-find-dir: deferred (needs a directory prompt)"),
            key!('k') => cx
                .editor
                .set_status("project-kill-buffers: deferred (needs buffer management)"),
            key!('g') => cx
                .editor
                .set_status("project-find-regexp: deferred (needs a grep prompt)"),
            key!('c') => cx
                .editor
                .set_status("project-compile: deferred (needs a build runner)"),
            key!('b') => cx
                .editor
                .set_status("project-switch-to-buffer: deferred (needs a buffer picker)"),
            key!('s') => cx
                .editor
                .set_status("project-shell: deferred (needs a shell host)"),
            _ => {
                // Any other bare (or shifted) printable char extends the query.
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    if let KeyCode::Char(c) = key.code {
                        if is_query_char(c) {
                            self.query.push(c);
                            self.rerank();
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
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let dir_style = theme.get("ui.text.directory");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let query_style = theme.get("function");
        let err_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 4 {
            return;
        }

        let header = format!(
            " Project: {}  ({} files)",
            self.root.display(),
            self.files.len()
        );
        surface.set_stringn(area.x, area.y, &header, area.width as usize, header_style);

        // Query line: "  > <query>    N/M".
        let prompt = format!(" > {}", self.query);
        surface.set_stringn(
            area.x,
            area.y + 1,
            &prompt,
            area.width as usize,
            query_style,
        );
        let count = format!("{}/{}", self.ranked.len(), self.files.len());
        if prompt.len() + count.len() + 2 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - count.len() as u16 - 1,
                area.y + 1,
                &count,
                count.len(),
                info_style,
            );
        }

        if let Some(err) = &self.error {
            surface.set_stringn(area.x, area.y + 2, err, area.width as usize, err_style);
        }

        let body_y = area.y + 3;
        let body_h = area.height.saturating_sub(4);
        self.viewport = body_h.max(1) as usize;

        if self.ranked.is_empty() {
            let msg = if self.files.is_empty() {
                "(no files)"
            } else {
                "(no matches)"
            };
            surface.set_stringn(area.x, body_y, msg, area.width as usize, info_style);
        } else {
            // Keep the selection in view.
            if self.selected < self.scroll {
                self.scroll = self.selected;
            } else if self.selected >= self.scroll + self.viewport {
                self.scroll = self.selected + 1 - self.viewport;
            }

            for (offset, name) in self
                .ranked
                .iter()
                .enumerate()
                .skip(self.scroll)
                .take(body_h as usize)
            {
                let y = body_y + (offset - self.scroll) as u16;
                let base = if offset == self.selected {
                    sel_style
                } else {
                    text_style
                };
                let marker = if offset == self.selected { "> " } else { "  " };
                let line = format!("{}{}", marker, name);
                surface.set_stringn(area.x, y, &line, area.width as usize, base);
                // Accent the directory portion of unselected rows.
                if offset != self.selected {
                    if let Some(slash) = name.rfind('/') {
                        let dir = &line[..2 + slash + 1];
                        surface.set_stringn(area.x, y, dir, dir.len(), dir_style);
                    }
                }
            }
        }

        // Footer hint.
        let hint = "Enter open  n/p move  D dired  q quit";
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            hint,
            area.width as usize,
            info_style,
        );
    }
}
