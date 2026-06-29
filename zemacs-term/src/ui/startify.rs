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
    /// Top file extensions in the project, by count — for the language BarChart.
    lang_stats: Vec<(String, u64)>,
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

        let lang_stats = cwd.as_deref().map(scan_languages).unwrap_or_default();

        Startify {
            header,
            entries,
            selected: 0,
            lang_stats,
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

    /// Right-hand dashboard shown on wide terminals: a ratatui [`Canvas`] logo
    /// above a [`BarChart`] of the project's top file types.
    fn render_dashboard(&self, surface: &mut Surface, theme: &zemacs_view::Theme, area: Rect) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::{Color as RColor, Modifier as RMod};
        use ratatui::symbols::Marker;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Rectangle};
        use ratatui::widgets::{BarChart, Block, Borders};

        // Only when there's room for a genuine second column.
        if area.width < 72 || area.height < 12 {
            return;
        }
        let rx = area.x + area.width / 2 + 2;
        let rw = area.width.saturating_sub(area.width / 2 + 3);
        if rw < 24 {
            return;
        }

        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let dim = to_rat_style(theme.get("comment"));
        let text = to_rat_style(theme.get("ui.text"));
        let bg = theme.get("ui.background");
        let stroke = dim.fg.unwrap_or(RColor::Gray);

        // ── Canvas logo ────────────────────────────────────────────────────────
        let logo_h = 7u16.min(area.height / 3);
        let logo_rect = Rect::new(rx, area.y + 1, rw, logo_h);
        surface.clear_with(logo_rect, bg);
        let logo = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([0.0, 100.0])
            .y_bounds([0.0, 100.0])
            .paint(move |ctx| {
                ctx.draw(&Rectangle {
                    x: 1.0,
                    y: 1.0,
                    width: 97.0,
                    height: 97.0,
                    color: stroke,
                });
                // a stylized "Z" stroke inside the frame
                for (x1, y1, x2, y2) in [
                    (20.0, 76.0, 72.0, 76.0),
                    (72.0, 76.0, 20.0, 24.0),
                    (20.0, 24.0, 72.0, 24.0),
                ] {
                    ctx.draw(&CanvasLine { x1, y1, x2, y2, color: stroke });
                }
                ctx.print(10.0, 50.0, Line::from(Span::styled("zemacs", accent)));
            });
        render(logo, logo_rect, surface);

        // ── Language BarChart ────────────────────────────────────────────────────
        if self.lang_stats.is_empty() {
            return;
        }
        let by = area.y + 1 + logo_h + 1;
        let bottom = area.y + area.height;
        if by + 4 > bottom {
            return;
        }
        let bh = (bottom - by - 1).min(12);
        let chart_rect = Rect::new(rx, by, rw, bh);
        surface.clear_with(chart_rect, bg);

        let data: Vec<(&str, u64)> = self
            .lang_stats
            .iter()
            .map(|(e, c)| (e.as_str(), *c))
            .collect();
        let chart = BarChart::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(dim)
                    .title(Span::styled(" Languages ", accent)),
            )
            .data(data.as_slice())
            .bar_width(6)
            .bar_gap(1)
            .bar_style(text)
            .value_style(dim)
            .label_style(text);
        render(chart, chart_rect, surface);
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

        // Right-hand dashboard (Canvas logo + language BarChart) on wide terminals.
        self.render_dashboard(surface, theme, area);
    }

    fn id(&self) -> Option<&'static str> {
        Some("startify")
    }
}

/// Count files by extension under `root` (respecting `.gitignore`), returning
/// the top handful by frequency for the language BarChart. Capped so very large
/// trees don't stall startup.
fn scan_languages(root: &Path) -> Vec<(String, u64)> {
    use std::collections::HashMap;
    let mut counts: HashMap<String, u64> = HashMap::new();
    let mut seen = 0u64;
    for entry in ignore::WalkBuilder::new(root).build().flatten() {
        if entry.file_type().is_some_and(|t| t.is_file()) {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                *counts.entry(ext.to_lowercase()).or_default() += 1;
            }
            seen += 1;
            if seen >= 20_000 {
                break;
            }
        }
    }
    let mut v: Vec<(String, u64)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(6);
    v
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
