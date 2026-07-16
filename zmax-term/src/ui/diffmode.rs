//! Diff-mode — the zmax port of GNU Emacs `diff-mode`, a self-contained
//! unified-diff viewer overlay.
//!
//! A modal full-screen [`Component`] that renders a parsed unified diff as a
//! scrollable, colour-coded listing: added lines green (`diff.plus`), removed red
//! (`diff.minus`, or `error` if that scope is absent), file banners highlighted
//! (`ui.text.focus`) and hunk headers accented (`function`). All parsing lives in
//! the filesystem-free, unit-tested [`zmax_core::diffmode`]; this module only
//! renders and handles keys.
//!
//! Keys (parsed into a `diffmode` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs `diff-mode` counterpart in the port tracker):
//!   j/k/n-arrows — line down/up (`C-n`/`C-p`, Down/Up)
//!   C-d/PgDn, C-u/PgUp — page down / up
//!   g/Home, G/End — top / bottom
//!   n / p — diff-hunk-next / diff-hunk-prev (jump to the next/prev `@@` header)
//!   } / M-n, { / M-p — diff-file-next / diff-file-prev (next/prev file banner)
//!   Enter / o — diff-goto-source: visit the current file's new path if on disk
//!   q/Esc/C-c — quit
//!
//! Deferred to a later slice: diff-restrict-view (`|`, narrow to one file/hunk)
//! and diff-refine-hunk (word-level intra-line refinement).

use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zmax_core::diffmode::{self, DiffLine, LineKind};
use zmax_view::{editor::Action, graphics::Rect};

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The interactive diff-mode overlay.
pub struct DiffMode {
    /// The flattened, renderable diff lines.
    flat: Vec<DiffLine>,
    /// Per-line kinds, kept alongside `flat` for the pure navigation helpers.
    kinds: Vec<LineKind>,
    /// Flat indices of each file's `FileHeader` line (aligned with `new_paths`).
    file_starts: Vec<usize>,
    /// Each file's new path, for diff-goto-source.
    new_paths: Vec<String>,
    file_count: usize,
    added: usize,
    removed: usize,
    /// Current line (point).
    cursor: usize,
    scroll: usize,
    viewport: usize,
    status: Option<String>,
}

impl DiffMode {
    /// Build the overlay from raw unified-diff text.
    pub fn new(diff_text: String) -> Self {
        let diff = diffmode::parse(&diff_text);
        let flat = diffmode::flatten(&diff);
        let kinds: Vec<LineKind> = flat.iter().map(|l| l.kind).collect();
        let file_starts: Vec<usize> = flat
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kind == LineKind::FileHeader)
            .map(|(i, _)| i)
            .collect();
        let new_paths: Vec<String> = diff.files.iter().map(|f| f.new_path.clone()).collect();
        let (added, removed) = diffmode::stats(&diff);
        DiffMode {
            flat,
            kinds,
            file_starts,
            new_paths,
            file_count: diff.files.len(),
            added,
            removed,
            cursor: 0,
            scroll: 0,
            viewport: 1,
            status: None,
        }
    }

    fn max_line(&self) -> usize {
        self.flat.len().saturating_sub(1)
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.flat.is_empty() {
            return;
        }
        let max = self.max_line() as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, max) as usize;
    }

    /// The ordinal of the file the cursor is currently inside.
    fn current_file(&self) -> Option<usize> {
        if self.file_starts.is_empty() {
            return None;
        }
        let mut ord = 0;
        for (k, &start) in self.file_starts.iter().enumerate() {
            if start <= self.cursor {
                ord = k;
            } else {
                break;
            }
        }
        Some(ord)
    }

    fn next_file(&mut self) {
        if let Some(&s) = self.file_starts.iter().find(|&&s| s > self.cursor) {
            self.cursor = s;
        }
    }

    fn prev_file(&mut self) {
        if let Some(&s) = self.file_starts.iter().rev().find(|&&s| s < self.cursor) {
            self.cursor = s;
        }
    }

    /// diff-goto-source: return a callback opening the current file's new path,
    /// or set a status message if there is nothing to visit.
    fn goto_source(&mut self) -> Option<Callback> {
        let path = self
            .current_file()
            .and_then(|ord| self.new_paths.get(ord))
            .cloned();
        let path = match path {
            Some(p) if !p.is_empty() && p != "/dev/null" => p,
            _ => {
                self.status = Some("diff: no source file at point".to_string());
                return None;
            }
        };
        let pb = PathBuf::from(&path);
        if !pb.exists() {
            self.status = Some(format!("diff: file not on disk: {path}"));
            return None;
        }
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                if let Err(err) = cx.editor.open(&pb, Action::Replace) {
                    cx.editor
                        .set_error(format!("failed to open {}: {err}", pb.display()));
                }
            },
        ))
    }
}

impl Component for DiffMode {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        let page = self.viewport.max(1) as isize;
        self.status = None;
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.move_cursor(1),
            key!('k') | key!(Up) | ctrl!('p') => self.move_cursor(-1),
            ctrl!('d') | key!(PageDown) => self.move_cursor(page),
            ctrl!('u') | key!(PageUp) => self.move_cursor(-page),
            key!('g') | key!(Home) => self.cursor = 0,
            key!('G') | key!(End) => self.cursor = self.max_line(),
            key!('n') => {
                if let Some(i) = diffmode::next_hunk_line(&self.kinds, self.cursor) {
                    self.cursor = i;
                }
            }
            key!('p') => {
                if let Some(i) = diffmode::prev_hunk_line(&self.kinds, self.cursor) {
                    self.cursor = i;
                }
            }
            key!('}') | alt!('n') => self.next_file(),
            key!('{') | alt!('p') => self.prev_file(),
            key!(Enter) | key!('o') => {
                if let Some(cb) = self.goto_source() {
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
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let plus_style = theme.get("diff.plus");
        let minus_style = theme
            .try_get("diff.minus")
            .unwrap_or_else(|| theme.get("error"));
        let hunk_style = theme.get("function");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }
        let width = area.width as usize;

        // Header: "Diff  N files  +A −R".
        let title = format!(
            "Diff  {} file{}  +{} −{}",
            self.file_count,
            if self.file_count == 1 { "" } else { "s" },
            self.added,
            self.removed
        );
        surface.set_stringn(area.x, area.y, &title, width, header_style);

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h.max(1) as usize;

        if let Some(msg) = &self.status {
            surface.set_stringn(area.x, area.y + 1, msg, width, minus_style);
        }

        if self.flat.is_empty() {
            surface.set_stringn(area.x, body_y, "(empty diff)", width, info_style);
            return;
        }

        // Keep the cursor in view.
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + self.viewport {
            self.scroll = self.cursor + 1 - self.viewport;
        }

        for (offset, line) in self
            .flat
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let style = if offset == self.cursor {
                sel_style
            } else {
                match line.kind {
                    LineKind::Added => plus_style,
                    LineKind::Removed => minus_style,
                    LineKind::FileHeader => header_style,
                    LineKind::HunkHeader => hunk_style,
                    LineKind::Header => info_style,
                    LineKind::Context => text_style,
                }
            };
            surface.set_stringn(area.x, y, &line.text, width, style);
        }

        // Footer: keys.
        let footer = "j/k line  C-d/C-u page  n/p hunk  {/} file  Enter open  q quit";
        surface.set_stringn(area.x, area.y + area.height - 1, footer, width, info_style);
    }
}
