//! Settings page: a JetBrains-style ratatui modal that edits `~/.zemacs/config.toml`.
//!
//! The file is loaded as a generic `toml::Value` tree so unknown keys are
//! preserved on save. A curated schema (`SETTINGS`) exposes the common options as
//! toggles / value fields; "Open config.toml" drops to raw editing. Mouse + keys.
//!
//! Keys:  j/k move · Space/⏎ toggle or edit · type to edit a value · o open raw · Esc close

use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::Rect,
    input::{KeyCode, KeyEvent, MouseButton, MouseEventKind},
};

use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Bool,
    Int,
    Str,
}

struct Spec {
    path: &'static [&'static str],
    label: &'static str,
    kind: Kind,
}

const SETTINGS: &[Spec] = &[
    Spec { path: &["theme"], label: "Theme", kind: Kind::Str },
    Spec { path: &["editor", "soft-wrap", "enable"], label: "Soft wrap", kind: Kind::Bool },
    Spec { path: &["editor", "word-completion", "enable"], label: "Word completion", kind: Kind::Bool },
    Spec { path: &["editor", "completion-trigger-len"], label: "Completion trigger length", kind: Kind::Int },
    Spec { path: &["editor", "auto-save", "focus-lost"], label: "Auto-save on focus lost", kind: Kind::Bool },
    Spec { path: &["editor", "auto-save", "after-delay", "enable"], label: "Auto-save after delay", kind: Kind::Bool },
    Spec { path: &["editor", "cursorline"], label: "Highlight cursor line", kind: Kind::Bool },
    Spec { path: &["editor", "color-modes"], label: "Color modes", kind: Kind::Bool },
    Spec { path: &["editor", "true-color"], label: "Force true color", kind: Kind::Bool },
    Spec { path: &["editor", "bufferline"], label: "Buffer line (tabs)", kind: Kind::Str },
    Spec { path: &["editor", "line-number"], label: "Line numbers", kind: Kind::Str },
    Spec { path: &["editor", "scrolloff"], label: "Scroll offset", kind: Kind::Int },
    Spec { path: &["editor", "rulers"], label: "Rulers (columns)", kind: Kind::Str },
    Spec { path: &["editor", "mouse"], label: "Mouse support", kind: Kind::Bool },
];

fn config_path() -> std::path::PathBuf {
    zemacs_loader::config_dir().join("config.toml")
}

fn load_config() -> toml::Value {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_else(|| toml::Value::Table(Default::default()))
}

fn save_config(v: &toml::Value) {
    if let Ok(s) = toml::to_string_pretty(v) {
        let p = config_path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(p, s);
    }
}

fn get_path<'a>(v: &'a toml::Value, path: &[&str]) -> Option<&'a toml::Value> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    Some(cur)
}

fn set_path(root: &mut toml::Value, path: &[&str], val: toml::Value) {
    if !root.is_table() {
        *root = toml::Value::Table(Default::default());
    }
    let mut cur = root;
    for (i, k) in path.iter().enumerate() {
        if i == path.len() - 1 {
            if let Some(t) = cur.as_table_mut() {
                t.insert(k.to_string(), val);
            }
            return;
        }
        let t = cur.as_table_mut().unwrap();
        cur = t
            .entry(k.to_string())
            .or_insert_with(|| toml::Value::Table(Default::default()));
        if !cur.is_table() {
            *cur = toml::Value::Table(Default::default());
        }
    }
}

/// Human display of the current value (or "—" when unset).
fn display(v: &toml::Value, spec: &Spec) -> String {
    match get_path(v, spec.path) {
        Some(toml::Value::Boolean(b)) => if *b { "✓ on".into() } else { "✗ off".into() },
        Some(toml::Value::Integer(n)) => n.to_string(),
        Some(toml::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "—".into(),
    }
}

pub struct SettingsPanel {
    cfg: toml::Value,
    selected: usize,
    editing: bool,
    buf: String,
    /// Click targets: (row, x0, x1, setting index).
    row_hits: Vec<(u16, u16, u16, usize)>,
    /// Button click targets: (x0, x1, row, is_open_raw).
    btn_hits: Vec<(u16, u16, u16, bool)>,
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            cfg: load_config(),
            selected: 0,
            editing: false,
            buf: String::new(),
            row_hits: Vec::new(),
            btn_hits: Vec::new(),
        }
    }

    fn toggle_or_edit(&mut self) {
        let spec = &SETTINGS[self.selected];
        match spec.kind {
            Kind::Bool => {
                let cur = matches!(get_path(&self.cfg, spec.path), Some(toml::Value::Boolean(true)));
                set_path(&mut self.cfg, spec.path, toml::Value::Boolean(!cur));
                save_config(&self.cfg);
            }
            Kind::Int | Kind::Str => {
                self.buf = display_raw(&self.cfg, spec);
                self.editing = true;
            }
        }
    }

    fn commit_edit(&mut self) {
        let spec = &SETTINGS[self.selected];
        let val = match spec.kind {
            Kind::Int => match self.buf.trim().parse::<i64>() {
                Ok(n) => toml::Value::Integer(n),
                Err(_) => {
                    self.editing = false;
                    return;
                }
            },
            _ => toml::Value::String(self.buf.clone()),
        };
        set_path(&mut self.cfg, spec.path, val);
        save_config(&self.cfg);
        self.editing = false;
    }

    fn open_raw_cb() -> Callback {
        Box::new(|compositor: &mut Compositor, cx: &mut Context| {
            compositor.pop();
            let path = config_path();
            let _ = cx.editor.open(&path, zemacs_view::editor::Action::Replace);
        })
    }

    fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) -> EventResult {
        if !matches!(kind, MouseEventKind::Down(MouseButton::Left)) {
            return EventResult::Consumed(None);
        }
        if let Some(&(_, _, _, open)) = self
            .btn_hits
            .iter()
            .find(|&&(x0, x1, r, _)| row == r && col >= x0 && col < x1)
        {
            if open {
                return EventResult::Consumed(Some(Self::open_raw_cb()));
            }
            return EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                c.pop();
            })));
        }
        if let Some(&(_, _, _, idx)) = self
            .row_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            self.editing = false;
            self.selected = idx;
            self.toggle_or_edit();
        }
        EventResult::Consumed(None)
    }
}

fn display_raw(v: &toml::Value, spec: &Spec) -> String {
    match get_path(v, spec.path) {
        Some(toml::Value::String(s)) => s.clone(),
        Some(toml::Value::Integer(n)) => n.to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

impl Component for SettingsPanel {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key: KeyEvent = match event {
            Event::Key(k) => *k,
            Event::Mouse(ev) => return self.handle_mouse(ev.column, ev.row, ev.kind),
            _ => return EventResult::Ignored(None),
        };
        if self.editing {
            match key.code {
                KeyCode::Esc => self.editing = false,
                KeyCode::Enter => self.commit_edit(),
                KeyCode::Backspace => {
                    self.buf.pop();
                }
                KeyCode::Char(c) => self.buf.push(c),
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                    c.pop();
                })))
            }
            KeyCode::Char('o') => EventResult::Consumed(Some(Self::open_raw_cb())),
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected = (self.selected + 1) % SETTINGS.len();
                EventResult::Consumed(None)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = (self.selected + SETTINGS.len() - 1) % SETTINGS.len();
                EventResult::Consumed(None)
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                self.toggle_or_edit();
                EventResult::Consumed(None)
            }
            _ => EventResult::Consumed(None),
        }
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

        self.row_hits.clear();
        self.btn_hits.clear();

        let theme = &ctx.editor.theme;
        let bg = to_rat_style(theme.get("ui.background"));
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let border = to_rat_style(theme.get("ui.window"));
        let sel = to_rat_style(theme.get("ui.selection")).add_modifier(RMod::BOLD);
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let val_style = to_rat_style(theme.get("string"));
        surface.clear_with(area, theme.get("ui.background"));

        let frame = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border)
            .style(bg)
            .title(Span::styled(" Settings — config.toml ", accent));
        render(frame, area, surface);
        let inner = Rect::new(area.x + 2, area.y + 1, area.width.saturating_sub(4), area.height.saturating_sub(2));
        if inner.width < 6 || inner.height < 5 {
            return;
        }

        // toolbar buttons
        let btn_row = inner.y;
        let mut bx = inner.x;
        for (label, open) in [(" 📄 Open config.toml ", true), (" ✕ Close ", false)] {
            let w = label.chars().count() as u16;
            render(
                Paragraph::new(Span::styled(label, text.add_modifier(RMod::REVERSED))),
                Rect::new(bx, btn_row, w, 1),
                surface,
            );
            self.btn_hits.push((bx, bx + w, btn_row, open));
            bx += w + 1;
        }

        // settings rows
        let list_y = inner.y + 2;
        let val_x = inner.x + 32.min(inner.width / 2);
        for (i, spec) in SETTINGS.iter().enumerate() {
            let row = list_y + i as u16;
            if row >= inner.y + inner.height - 1 {
                break;
            }
            let is_sel = i == self.selected;
            if is_sel {
                surface.set_style(Rect::new(inner.x, row, inner.width, 1), theme.get("ui.selection"));
            }
            render(
                Paragraph::new(Span::styled(spec.label, if is_sel { accent } else { text })),
                Rect::new(inner.x + 1, row, val_x - inner.x - 2, 1),
                surface,
            );
            let shown = if is_sel && self.editing {
                format!("{}▏", self.buf)
            } else {
                display(&self.cfg, spec)
            };
            let vstyle = if is_sel && self.editing {
                text.add_modifier(RMod::UNDERLINED)
            } else {
                val_style
            };
            render(
                Paragraph::new(Span::styled(shown, vstyle)),
                Rect::new(val_x, row, inner.x + inner.width - val_x, 1),
                surface,
            );
            self.row_hits.push((row, inner.x, inner.x + inner.width, i));
        }

        // help
        let help = if self.editing {
            " type to edit · ⏎ save · Esc cancel"
        } else {
            " j/k move · Space/⏎ toggle or edit · o open raw config · Esc close"
        };
        let _ = Line::default();
        render(
            Paragraph::new(Span::styled(help, dim)),
            Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
            surface,
        );
    }
}
