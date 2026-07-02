//! JetBrains "Recent Files" switcher (SPC b r): a two-pane popup — tool-window
//! tool windows on the left, recent files on the right, a "Show edited only" toggle
//! at the top, and a workspace path at the bottom.
//!
//! Keys: j/k or ↑/↓ move the recent-files list · Tab or ←/→ switch to the left
//! rail · Enter / click activates · `e` toggles edited-only · Esc closes.

use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::{Modifier, Rect},
    input::{MouseButton, MouseEventKind},
    keyboard::KeyCode,
};

use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};

pub const ID: &str = "recent-files-switcher";

/// A left-rail tool-window entry: label, shortcut hint, and the panel/command it
/// opens.
struct Tool {
    label: &'static str,
    shortcut: &'static str,
    action: ToolAction,
}

#[derive(Clone, Copy)]
enum ToolAction {
    Panel(&'static str),
    Todo,
    RecentLocations,
}

const TOOLS: &[Tool] = &[
    Tool { label: "Project", shortcut: "", action: ToolAction::Panel("project") },
    Tool { label: "Bookmarks", shortcut: "", action: ToolAction::Panel("bookmarks") },
    Tool { label: "Problems", shortcut: "", action: ToolAction::Panel("problems") },
    Tool { label: "Structure", shortcut: "", action: ToolAction::Panel("structure") },
    Tool { label: "Git", shortcut: "", action: ToolAction::Panel("git") },
    Tool { label: "Run", shortcut: "", action: ToolAction::Panel("run") },
    Tool { label: "TODO", shortcut: "", action: ToolAction::Todo },
    Tool { label: "Recent Locations", shortcut: "", action: ToolAction::RecentLocations },
];

pub struct RecentFilesSwitcher {
    /// All recent files (MRU, newest first).
    all: Vec<PathBuf>,
    /// Paths currently open as buffers (the "edited" proxy).
    open: Vec<PathBuf>,
    edited_only: bool,
    root: PathBuf,
    sel: usize,       // selected recent-file index
    left_sel: usize,  // selected left-rail index
    on_left: bool,    // focus is on the left rail
    // Click hit regions recorded at render.
    file_rows: Vec<(u16, usize)>, // (screen row, file index)
    tool_rows: Vec<(u16, usize)>, // (screen row, tool index)
    toggle_row: u16,
    toggle_x: (u16, u16),
    /// Column of the left-rail / right-pane divider, recorded at render. A click
    /// left of this hits the tool rail; at-or-right hits the file list. Both
    /// panes share the same screen rows, so row alone can't disambiguate them.
    split_x: u16,
}

impl RecentFilesSwitcher {
    pub fn new(all: Vec<PathBuf>, open: Vec<PathBuf>, root: PathBuf) -> Self {
        Self {
            all,
            open,
            edited_only: false,
            root,
            sel: 0,
            left_sel: 0,
            on_left: false,
            file_rows: Vec::new(),
            tool_rows: Vec::new(),
            toggle_row: 0,
            toggle_x: (0, 0),
            split_x: 0,
        }
    }

    /// The recent-files list honoring the edited-only toggle.
    fn files(&self) -> Vec<&PathBuf> {
        if self.edited_only {
            self.all.iter().filter(|p| self.open.contains(p)).collect()
        } else {
            self.all.iter().collect()
        }
    }

    fn close() -> Callback {
        Box::new(|c: &mut Compositor, _| {
            c.remove(ID);
        })
    }

    fn open_file(path: PathBuf) -> EventResult {
        EventResult::Consumed(Some(Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            compositor.remove(ID);
            if let Err(zemacs_view::DocumentOpenError::BinaryFile) =
                cx.editor.open(&path, zemacs_view::editor::Action::Replace)
            {
                crate::commands::typed::push_hex_view(cx, path);
            }
        })))
    }

    fn run_tool(action: ToolAction) -> EventResult {
        EventResult::Consumed(Some(Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            compositor.remove(ID);
            match action {
                ToolAction::Panel(name) => {
                    if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                        view.focus_ide_panel(name);
                    }
                }
                ToolAction::Todo => crate::commands::typed::run_command_line(cx, "Todo"),
                ToolAction::RecentLocations => {
                    crate::commands::typed::run_command_line(cx, "RecentLocations")
                }
            }
        })))
    }

    fn activate(&mut self) -> EventResult {
        if self.on_left {
            match TOOLS.get(self.left_sel) {
                Some(t) => Self::run_tool(t.action),
                None => EventResult::Consumed(Some(Self::close())),
            }
        } else {
            let files = self.files();
            match files.get(self.sel) {
                Some(p) => Self::open_file((*p).clone()),
                None => EventResult::Consumed(Some(Self::close())),
            }
        }
    }
}

impl Component for RecentFilesSwitcher {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Esc => EventResult::Consumed(Some(Self::close())),
                KeyCode::Char('e') => {
                    self.edited_only = !self.edited_only;
                    self.sel = 0;
                    EventResult::Consumed(None)
                }
                KeyCode::Tab => {
                    self.on_left = !self.on_left;
                    EventResult::Consumed(None)
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    self.on_left = true;
                    EventResult::Consumed(None)
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    self.on_left = false;
                    EventResult::Consumed(None)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.on_left {
                        if self.left_sel + 1 < TOOLS.len() {
                            self.left_sel += 1;
                        }
                    } else {
                        let n = self.files().len();
                        if n > 0 && self.sel + 1 < n {
                            self.sel += 1;
                        }
                    }
                    EventResult::Consumed(None)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.on_left {
                        self.left_sel = self.left_sel.saturating_sub(1);
                    } else {
                        self.sel = self.sel.saturating_sub(1);
                    }
                    EventResult::Consumed(None)
                }
                KeyCode::Enter => self.activate(),
                _ => EventResult::Consumed(None),
            },
            Event::Mouse(ev) => match ev.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    // Toggle "edited only".
                    if ev.row == self.toggle_row
                        && ev.column >= self.toggle_x.0
                        && ev.column < self.toggle_x.1
                    {
                        self.edited_only = !self.edited_only;
                        self.sel = 0;
                        return EventResult::Consumed(None);
                    }
                    // The tool rail and file list share screen rows, so the
                    // column decides which pane was clicked: left of the divider
                    // is the rail, at-or-right is the file list.
                    if ev.column < self.split_x {
                        if let Some(&(_, i)) = self.tool_rows.iter().find(|&&(r, _)| r == ev.row) {
                            self.on_left = true;
                            self.left_sel = i;
                            return self.activate();
                        }
                    } else if let Some(&(_, i)) =
                        self.file_rows.iter().find(|&&(r, _)| r == ev.row)
                    {
                        self.on_left = false;
                        self.sel = i;
                        return self.activate();
                    }
                    // Click outside → dismiss (consumed so it doesn't leak).
                    EventResult::Consumed(Some(Self::close()))
                }
                MouseEventKind::ScrollDown => {
                    let n = self.files().len();
                    if n > 0 && self.sel + 1 < n {
                        self.sel += 1;
                    }
                    EventResult::Consumed(None)
                }
                MouseEventKind::ScrollUp => {
                    self.sel = self.sel.saturating_sub(1);
                    EventResult::Consumed(None)
                }
                _ => EventResult::Consumed(None),
            },
            _ => EventResult::Ignored(None),
        }
    }

    fn render(&mut self, viewport: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.menu");
        let border = theme.get("ui.window");
        let text = theme.get("ui.text");
        let dim = theme.get("comment");
        let sel = theme.get("ui.menu.selected");
        let accent = theme.get("function").add_modifier(Modifier::BOLD);

        // Centered popup, ~70% of the viewport.
        let w = (viewport.width as f32 * 0.72) as u16;
        let w = w.clamp(40, viewport.width.saturating_sub(2));
        let h = (viewport.height as f32 * 0.72) as u16;
        let h = h.clamp(8, viewport.height.saturating_sub(2));
        let x = viewport.x + (viewport.width - w) / 2;
        let y = viewport.y + (viewport.height - h) / 2;
        let area = Rect::new(x, y, w, h);
        surface.clear_with(area, bg);

        // Border box.
        use ratatui::widgets::{Block, BorderType, Borders};
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(crate::ui::rat::to_rat_style(border))
            .style(crate::ui::rat::to_rat_style(bg));
        crate::ui::rat::render(block, area, surface);

        let inner = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);

        // Header row: title + right-aligned "Show edited only" toggle.
        surface.set_stringn(inner.x + 1, inner.y, " Recent Files", inner.width as usize, accent);
        let check = if self.edited_only { "☑" } else { "☐" };
        let toggle = format!("{check} Show edited only  e ");
        let tw = toggle.chars().count() as u16;
        let tx = inner.x + inner.width.saturating_sub(tw);
        surface.set_stringn(tx, inner.y, &toggle, tw as usize, dim);
        self.toggle_row = inner.y;
        self.toggle_x = (tx, tx + tw);

        let body_y = inner.y + 2;
        let body_h = inner.height.saturating_sub(3);
        let left_w = 22u16.min(inner.width / 3);
        let split_x = inner.x + left_w;
        self.split_x = split_x;

        // Left rail: tool windows.
        self.tool_rows.clear();
        for (i, tool) in TOOLS.iter().enumerate() {
            let ry = body_y + i as u16;
            if ry >= body_y + body_h {
                break;
            }
            let is_sel = self.on_left && i == self.left_sel;
            let style = if is_sel { sel } else { text };
            if is_sel {
                surface.set_style(Rect::new(inner.x, ry, left_w, 1), sel);
            }
            surface.set_stringn(inner.x + 1, ry, tool.label, (left_w - 1) as usize, style);
            if !tool.shortcut.is_empty() {
                let sc = format!("{} ", tool.shortcut);
                let scw = sc.chars().count() as u16;
                surface.set_stringn(split_x.saturating_sub(scw), ry, &sc, scw as usize, dim);
            }
            self.tool_rows.push((ry, i));
        }
        // Vertical divider.
        for r in body_y..body_y + body_h {
            surface.set_stringn(split_x, r, "│", 1, border);
        }

        // Right pane: recent files. Own the paths so no borrow of `self` is held
        // across the file_rows.push below.
        self.file_rows.clear();
        let files: Vec<PathBuf> = self.files().into_iter().cloned().collect();
        let list_x = split_x + 2;
        let list_w = (inner.x + inner.width).saturating_sub(list_x);
        for (i, path) in files.iter().enumerate() {
            let ry = body_y + i as u16;
            if ry >= body_y + body_h {
                break;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let parent = path
                .parent()
                .and_then(|p| p.strip_prefix(&self.root).ok().or(Some(p)))
                .map(|p| {
                    let s = p.to_string_lossy();
                    if s.is_empty() { "./".to_string() } else { s.into_owned() }
                })
                .unwrap_or_default();
            let icon = crate::ui::icons::file_icon(&name);
            let is_sel = !self.on_left && i == self.sel;
            let style = if is_sel { sel } else { text };
            if is_sel {
                surface.set_style(Rect::new(list_x, ry, list_w, 1), sel);
            }
            let head = format!("{icon} {name}  ");
            let (nx, _) = surface.set_stringn(list_x, ry, &head, list_w as usize, style);
            let rem = (list_x + list_w).saturating_sub(nx);
            if rem > 0 {
                surface.set_stringn(nx, ry, &parent, rem as usize, dim);
            }
            self.file_rows.push((ry, i));
        }

        // Footer: workspace path.
        let footer = format!(" {}", self.root.to_string_lossy());
        surface.set_stringn(inner.x + 1, inner.y + inner.height - 1, &footer, inner.width as usize, dim);
    }

    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
}
