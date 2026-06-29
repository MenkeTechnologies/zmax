//! Startify-style start screen, shown on launch with no file argument.
//!
//! A faithful vim-startify port: a `fortune | cowsay` quote header, an
//! `[e] <empty buffer>` action, a 0-indexed most-recently-used (MRU) file list,
//! an `MRU <cwd>` section of recent files under the working directory, and a
//! `[q] <quit>`. Modal overlay on top of the empty scratch buffer: press a
//! bracketed shortcut, move with `j`/`k`/arrows + `Enter`, or `Esc` to dismiss.

use std::path::{Path, PathBuf};
use std::process::Command;

use tui::buffer::Buffer as Surface;
use zemacs_view::{editor::Action, graphics::Rect, keyboard::KeyCode};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Fallback header when `fortune`/`cowsay` aren't available.
const FALLBACK_HEADER: &[&str] = &[
    r" ______________________________",
    r"< zemacs — the hackable editor >",
    r" ------------------------------",
    r"        \   ^__^",
    r"         \  (oo)\_______",
    r"            (__)\       )\/\",
    r"                ||----w |",
    r"                ||     ||",
];

enum EntryAction {
    NewFile,
    Open(PathBuf),
    Quit,
}

struct Entry {
    /// Text inside the brackets, e.g. "e", "0", "10", "q".
    bracket: String,
    /// Single-key shortcut accepted directly (None for indices > 9).
    shortcut: Option<char>,
    /// Display label (a path for MRU entries, or a `<…>` literal).
    label: String,
    /// Whether `label` is a path that should be drawn dir-dim / filename-bright.
    is_path: bool,
    /// Optional section break drawn before this entry: a blank line, plus a label
    /// when non-empty (e.g. "MRU", "MRU <cwd>").
    section: Option<String>,
    action: EntryAction,
}

pub struct Startify {
    header: Vec<String>,
    entries: Vec<Entry>,
    selected: usize,
}

impl Startify {
    pub fn new() -> Self {
        let header = build_header();
        let mut entries = Vec::new();

        // [e] <empty buffer>
        entries.push(Entry {
            bracket: "e".into(),
            shortcut: Some('e'),
            label: "<empty buffer>".into(),
            is_path: false,
            section: None,
            action: EntryAction::NewFile,
        });

        let frecent = crate::recent_files::load_frecent();
        let recent = crate::recent_files::load();
        let cwd = std::env::current_dir().ok();
        let mut idx = 0usize;

        // FRECENT — global files ranked by z-frecency (frequency × recency), [0]..[9].
        let mut first = true;
        for path in frecent.iter().take(10) {
            entries.push(Entry {
                bracket: idx.to_string(),
                shortcut: (idx <= 9).then(|| char::from(b'0' + idx as u8)),
                label: tilde(path),
                is_path: true,
                section: first.then(|| "FRECENT".to_string()),
                action: EntryAction::Open(path.clone()),
            });
            idx += 1;
            first = false;
        }

        // MRU <cwd> — recent files under the working directory, relative paths.
        if let Some(cwd) = &cwd {
            let under: Vec<&PathBuf> = recent.iter().filter(|p| p.starts_with(cwd)).collect();
            let mut first = true;
            for path in under.iter().take(10) {
                let rel = path.strip_prefix(cwd).unwrap_or(path);
                entries.push(Entry {
                    bracket: idx.to_string(),
                    shortcut: (idx <= 9).then(|| char::from(b'0' + idx as u8)),
                    label: rel.display().to_string(),
                    is_path: true,
                    section: first.then(|| format!("MRU {}", cwd.display())),
                    action: EntryAction::Open((*path).clone()),
                });
                idx += 1;
                first = false;
            }
        }

        // [q] <quit>
        entries.push(Entry {
            bracket: "q".into(),
            shortcut: Some('q'),
            label: "<quit>".into(),
            is_path: false,
            section: Some(String::new()),
            action: EntryAction::Quit,
        });

        Startify {
            header,
            entries,
            selected: 0,
        }
    }

    /// Build the callback that performs entry `idx`'s action and dismisses the screen.
    fn activate(&self, idx: usize) -> Callback {
        match &self.entries[idx].action {
            EntryAction::Open(path) => {
                let path = path.clone();
                Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
                    compositor.pop();
                    if let Err(err) = cx.editor.open(&path, Action::Replace) {
                        cx.editor
                            .set_error(format!("Failed to open {}: {err}", path.display()));
                    }
                })
            }
            // Scratch buffer is already open underneath; just reveal it.
            EntryAction::NewFile => Box::new(|compositor: &mut Compositor, _cx| {
                compositor.pop();
            }),
            EntryAction::Quit => Box::new(|compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                let view_id = cx.editor.tree.focus;
                cx.editor.close(view_id);
            }),
        }
    }
}

impl Component for Startify {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            Event::Mouse(_) => return EventResult::Consumed(None),
            _ => return EventResult::Ignored(None),
        };

        let len = self.entries.len();
        let dismiss: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        match key {
            key!(Esc) | ctrl!('c') => EventResult::Consumed(Some(dismiss)),
            key!('j') | key!(Down) | ctrl!('n') => {
                if len > 0 {
                    self.selected = (self.selected + 1) % len;
                }
                EventResult::Consumed(None)
            }
            key!('k') | key!(Up) | ctrl!('p') => {
                if len > 0 {
                    self.selected = (self.selected + len - 1) % len;
                }
                EventResult::Consumed(None)
            }
            key!(Enter) => {
                if self.entries.is_empty() {
                    EventResult::Consumed(None)
                } else {
                    EventResult::Consumed(Some(self.activate(self.selected)))
                }
            }
            _ => {
                if let KeyCode::Char(c) = key.code {
                    if key.modifiers.is_empty() {
                        if let Some(idx) = self.entries.iter().position(|e| e.shortcut == Some(c)) {
                            return EventResult::Consumed(Some(self.activate(idx)));
                        }
                    }
                }
                // Stay modal: swallow everything else so it never edits the buffer behind.
                EventResult::Consumed(None)
            }
        }
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let header_style = theme.get("function");
        let key_style = theme.get("function");
        let section_style = theme.get("string");
        let dir_style = theme.get("comment");
        let file_style = theme.get("ui.text");
        let base = theme.get("ui.text");
        let sel = theme.get("ui.selection");

        surface.clear_with(area, bg);

        let x0 = area.x + 1;
        let bottom = area.y + area.height;
        let mut y = area.y;

        // --- fortune | cowsay header ---
        for line in &self.header {
            if y >= bottom {
                return;
            }
            surface.set_string(x0, y, line, header_style);
            y += 1;
        }
        y += 1;

        // --- entries (with section breaks) ---
        for (i, entry) in self.entries.iter().enumerate() {
            if let Some(label) = &entry.section {
                y += 1;
                if !label.is_empty() && y < bottom {
                    surface.set_string(x0, y, label, section_style);
                    y += 1;
                }
            }
            if y >= bottom {
                break;
            }

            if i == self.selected {
                surface.set_style(Rect::new(x0, y, area.width.saturating_sub(2), 1), sel);
            }
            let prefix = format!("[{}] ", entry.bracket);
            let px = prefix.chars().count() as u16;
            surface.set_string(x0, y, &prefix, key_style);

            if entry.is_path {
                draw_path(surface, x0 + px, y, &entry.label, dir_style, file_style);
            } else {
                surface.set_string(x0 + px, y, &entry.label, base);
            }
            y += 1;
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("startify")
    }
}

/// Run `fortune | cowsay` for the header, falling back to a static cow.
fn build_header() -> Vec<String> {
    let out = Command::new("sh")
        .arg("-c")
        .arg("fortune -s 2>/dev/null | cowsay 2>/dev/null")
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::to_string)
                .collect();
            if lines.iter().any(|l| !l.trim().is_empty()) {
                return lines;
            }
        }
    }
    FALLBACK_HEADER.iter().map(|s| s.to_string()).collect()
}

/// Draw a path with the directory portion dimmed and the filename bright.
fn draw_path(surface: &mut Surface, x: u16, y: u16, path: &str, dir: zemacs_view::graphics::Style, file: zemacs_view::graphics::Style) {
    match path.rfind('/') {
        Some(slash) => {
            let (head, tail) = path.split_at(slash + 1); // head includes the '/'
            surface.set_string(x, y, head, dir);
            surface.set_string(x + head.chars().count() as u16, y, tail, file);
        }
        None => {
            surface.set_string(x, y, path, file);
        }
    }
}

/// `~`-relative display of an absolute path.
fn tilde(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        if let Ok(rel) = path.strip_prefix(PathBuf::from(home)) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
}
