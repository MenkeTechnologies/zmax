//! Keymap editor: CRUD over the user's `[keys.<mode>]` overrides in
//! `~/.zemacs/config.toml`. Each entry is (mode, chord, command); saving rebuilds
//! the `[keys.normal]` / `[keys.select]` / `[keys.insert]` tables, preserving every
//! other config key. (These override the built-in vim keymap.)

use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::Rect,
    input::{KeyCode, KeyEvent, MouseButton, MouseEventKind},
};

use crate::compositor::{Component, Context, Event, EventResult};

const MODES: [&str; 3] = ["normal", "select", "insert"];
const FIELDS: [&str; 3] = ["Mode", "Chord", "Command"];

#[derive(Clone, Default)]
struct Bind {
    mode: usize, // index into MODES
    chord: String,
    command: String,
}

fn config_path() -> std::path::PathBuf {
    zemacs_loader::config_dir().join("config.toml")
}

fn load_binds() -> (toml::Value, Vec<Bind>) {
    let cfg: toml::Value = std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_else(|| toml::Value::Table(Default::default()));
    let mut binds = Vec::new();
    if let Some(keys) = cfg.get("keys").and_then(|k| k.as_table()) {
        for (mi, m) in MODES.iter().enumerate() {
            if let Some(tbl) = keys.get(*m).and_then(|t| t.as_table()) {
                for (chord, cmd) in tbl {
                    binds.push(Bind {
                        mode: mi,
                        chord: chord.clone(),
                        command: value_to_command(cmd),
                    });
                }
            }
        }
    }
    (cfg, binds)
}

fn value_to_command(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

pub struct KeymapEditor {
    cfg: toml::Value,
    binds: Vec<Bind>,
    selected: usize,
    editing: bool,
    field: usize,
    buf: Bind,
    /// Press-a-key capture: each keypress appends its chord token to `buf.chord`.
    capturing: bool,
    /// "All bindings" reference mode: (mode, chord, command) from the live keymap.
    browse: bool,
    all_binds: Vec<(String, String, String)>,
    bfilter: String,
    bsel: usize,
    btop: usize,
    row_hits: Vec<(u16, u16, u16, usize)>,
    field_hits: Vec<(u16, u16, u16, usize)>,
    btn_hits: Vec<(u16, u16, u16, u8)>, // 0 add, 1 delete, 2 capture, 3 browse-toggle
}

/// Every binding in the default keymap as (mode, chord, command), sorted.
fn all_bindings() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (mode, trie) in &crate::keymap::default() {
        let m = match *mode {
            zemacs_view::document::Mode::Normal => "normal",
            zemacs_view::document::Mode::Select => "select",
            zemacs_view::document::Mode::Insert => "insert",
        };
        for (cmd, chords) in trie.reverse_map() {
            for chord in chords {
                let s = chord
                    .iter()
                    .map(|k| k.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push((m.to_string(), s, cmd.clone()));
            }
        }
    }
    out.sort();
    out
}

impl Default for KeymapEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl KeymapEditor {
    pub fn new() -> Self {
        let (cfg, binds) = load_binds();
        Self {
            cfg,
            binds,
            selected: 0,
            editing: false,
            field: 0,
            buf: Bind::default(),
            capturing: false,
            browse: false,
            all_binds: all_bindings(),
            bfilter: String::new(),
            bsel: 0,
            btop: 0,
            row_hits: Vec::new(),
            field_hits: Vec::new(),
            btn_hits: Vec::new(),
        }
    }

    fn browse_matches(&self) -> Vec<usize> {
        let f = self.bfilter.to_lowercase();
        self.all_binds
            .iter()
            .enumerate()
            .filter(|(_, (m, c, cmd))| {
                f.is_empty()
                    || c.to_lowercase().contains(&f)
                    || cmd.to_lowercase().contains(&f)
                    || m.contains(&f)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn persist(&mut self) {
        // Rebuild the keys tables from `binds`, leaving other config keys intact.
        if !self.cfg.is_table() {
            self.cfg = toml::Value::Table(Default::default());
        }
        let mut keys = toml::value::Table::new();
        for m in MODES {
            keys.insert(m.to_string(), toml::Value::Table(Default::default()));
        }
        for b in &self.binds {
            if b.chord.trim().is_empty() || b.command.trim().is_empty() {
                continue;
            }
            if let Some(toml::Value::Table(t)) = keys.get_mut(MODES[b.mode]) {
                // command with commas → array of commands (vim sequence)
                let val = if b.command.contains(',') {
                    toml::Value::Array(
                        b.command
                            .split(',')
                            .map(|s| toml::Value::String(s.trim().to_string()))
                            .collect(),
                    )
                } else {
                    toml::Value::String(b.command.trim().to_string())
                };
                t.insert(b.chord.clone(), val);
            }
        }
        // drop empty mode tables for a tidy file
        keys.retain(|_, v| v.as_table().map(|t| !t.is_empty()).unwrap_or(false));
        if let Some(root) = self.cfg.as_table_mut() {
            if keys.is_empty() {
                root.remove("keys");
            } else {
                root.insert("keys".into(), toml::Value::Table(keys));
            }
        }
        if let Ok(s) = toml::to_string_pretty(&self.cfg) {
            let p = config_path();
            if let Some(par) = p.parent() {
                let _ = std::fs::create_dir_all(par);
            }
            let _ = std::fs::write(p, s);
        }
    }

    fn add(&mut self) {
        self.binds.push(Bind::default());
        self.selected = self.binds.len() - 1;
        self.buf = Bind::default();
        self.field = 0;
        self.editing = true;
    }

    fn delete(&mut self) {
        if self.selected < self.binds.len() {
            self.binds.remove(self.selected);
            if self.selected >= self.binds.len() {
                self.selected = self.binds.len().saturating_sub(1);
            }
            self.persist();
        }
    }

    fn start_edit(&mut self) {
        if let Some(b) = self.binds.get(self.selected) {
            self.buf = b.clone();
            self.field = 0;
            self.editing = true;
        }
    }

    fn commit(&mut self) {
        if let Some(b) = self.binds.get_mut(self.selected) {
            *b = self.buf.clone();
        }
        self.persist();
        self.editing = false;
    }

    fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) -> EventResult {
        if !matches!(kind, MouseEventKind::Down(MouseButton::Left)) {
            return EventResult::Consumed(None);
        }
        if let Some(&(_, _, _, b)) = self
            .btn_hits
            .iter()
            .find(|&&(x0, x1, r, _)| row == r && col >= x0 && col < x1)
        {
            match b {
                0 => self.add(),
                1 => self.delete(),
                3 => self.browse = true,
                _ => {
                    // Capture: start editing the selected (or a new) bind and record keys.
                    if !self.editing {
                        if self.binds.is_empty() {
                            self.add();
                        } else {
                            self.start_edit();
                        }
                    }
                    self.field = 1;
                    self.buf.chord.clear();
                    self.capturing = true;
                }
            }
            return EventResult::Consumed(None);
        }
        if let Some(&(_, _, _, fi)) = self
            .field_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            if !self.binds.is_empty() {
                if !self.editing {
                    self.start_edit();
                }
                self.field = fi;
                if fi == 0 {
                    // Mode field: click cycles it.
                    self.buf.mode = (self.buf.mode + 1) % MODES.len();
                }
            }
            return EventResult::Consumed(None);
        }
        if let Some(&(_, _, _, idx)) = self
            .row_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            self.selected = idx;
            self.editing = false;
        }
        EventResult::Consumed(None)
    }
}

impl Component for KeymapEditor {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let reload = |cx: &mut Context| {
            cx.editor
                .config_events
                .0
                .send(zemacs_view::editor::ConfigEvent::Refresh)
                .ok();
        };
        let key: KeyEvent = match event {
            Event::Key(k) => *k,
            Event::Mouse(ev) => {
                let r = self.handle_mouse(ev.column, ev.row, ev.kind);
                reload(cx);
                return r;
            }
            _ => return EventResult::Ignored(None),
        };
        // "All bindings" reference mode: a searchable read-only list.
        if self.browse {
            let n = self.browse_matches().len();
            match key.code {
                KeyCode::Esc => self.browse = false,
                KeyCode::Tab => self.browse = false,
                KeyCode::Down => self.bsel = (self.bsel + 1).min(n.saturating_sub(1)),
                KeyCode::Up => self.bsel = self.bsel.saturating_sub(1),
                KeyCode::Backspace => {
                    self.bfilter.pop();
                    self.bsel = 0;
                }
                KeyCode::Char(c) => {
                    self.bfilter.push(c);
                    self.bsel = 0;
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        // Press-a-key capture: record each keypress as a chord token. Enter ends
        // capture, Esc cancels it. (Tab/Enter/Esc themselves can't be captured this way.)
        if self.capturing {
            match key.code {
                KeyCode::Enter => self.capturing = false,
                KeyCode::Esc => self.capturing = false,
                _ => {
                    let tok = key.to_string();
                    if !tok.is_empty() {
                        if !self.buf.chord.is_empty() {
                            self.buf.chord.push(' ');
                        }
                        self.buf.chord.push_str(&tok);
                    }
                }
            }
            return EventResult::Consumed(None);
        }
        if self.editing {
            match key.code {
                KeyCode::Esc => self.editing = false,
                KeyCode::Enter => {
                    self.commit();
                    reload(cx);
                }
                KeyCode::Tab | KeyCode::Down => self.field = (self.field + 1) % FIELDS.len(),
                KeyCode::Up => self.field = (self.field + FIELDS.len() - 1) % FIELDS.len(),
                KeyCode::Char(' ') if self.field == 0 => {
                    self.buf.mode = (self.buf.mode + 1) % MODES.len()
                }
                KeyCode::Backspace => {
                    match self.field {
                        1 => {
                            self.buf.chord.pop();
                        }
                        2 => {
                            self.buf.command.pop();
                        }
                        _ => {}
                    };
                }
                KeyCode::Char(c) => match self.field {
                    1 => self.buf.chord.push(c),
                    2 => self.buf.command.push(c),
                    _ => {}
                },
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                return EventResult::Consumed(Some(Box::new(|c, _| {
                    c.pop();
                })))
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.binds.is_empty() {
                    self.selected = (self.selected + 1) % self.binds.len();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !self.binds.is_empty() {
                    self.selected = (self.selected + self.binds.len() - 1) % self.binds.len();
                }
            }
            KeyCode::Tab => self.browse = true,
            KeyCode::Char('a') => self.add(),
            KeyCode::Char('d') => {
                self.delete();
                reload(cx);
            }
            KeyCode::Enter | KeyCode::Char('e') => self.start_edit(),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::Span;
        use ratatui::widgets::Paragraph;

        self.row_hits.clear();
        self.field_hits.clear();
        self.btn_hits.clear();

        let theme = &ctx.editor.theme;
        let bg = to_rat_style(theme.get("ui.background"));
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let border = to_rat_style(theme.get("ui.window"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let key_st = to_rat_style(theme.get("keyword"));
        surface.clear_with(area, theme.get("ui.background"));

        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        render(
            Paragraph::new(Span::styled(" Keymap — config.toml [keys.*] ", accent)),
            Rect::new(area.x + 1, area.y, area.width.saturating_sub(1), 1),
            surface,
        );
        let _ = (border, bg);
        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(1),
        );
        if inner.width < 12 || inner.height < 6 {
            return;
        }

        // buttons
        let mut bx = inner.x;
        for (lbl, b) in [
            (" + Add ", 0u8),
            (" − Delete ", 1u8),
            (" ⌨ Capture key ", 2u8),
            (" 🔍 All Bindings (Tab) ", 3u8),
        ] {
            let w = lbl.chars().count() as u16;
            let st = if (b == 2 && self.capturing) || (b == 3 && self.browse) {
                to_rat_style(theme.get("diff.plus")).add_modifier(RMod::REVERSED | RMod::BOLD)
            } else {
                text.add_modifier(RMod::REVERSED)
            };
            render(
                Paragraph::new(Span::styled(lbl, st)),
                Rect::new(bx, inner.y, w, 1),
                surface,
            );
            self.btn_hits.push((bx, bx + w, inner.y, b));
            bx += w + 1;
        }

        // "All bindings" reference list (read-only, searchable).
        if self.browse {
            let matched = self.browse_matches();
            if self.bsel >= matched.len() {
                self.bsel = matched.len().saturating_sub(1);
            }
            render(
                Paragraph::new(Span::styled(
                    format!(
                        "🔍 {}▏   {} of {} bindings   (Tab/Esc back)",
                        self.bfilter,
                        matched.len(),
                        self.all_binds.len()
                    ),
                    dim,
                )),
                Rect::new(inner.x, inner.y + 1, inner.width, 1),
                surface,
            );
            let by = inner.y + 2;
            let bh = inner.height.saturating_sub(3);
            if self.bsel < self.btop {
                self.btop = self.bsel;
            } else if self.bsel >= self.btop + bh as usize {
                self.btop = self.bsel + 1 - bh as usize;
            }
            let last = (self.btop + bh as usize).min(matched.len());
            for (pos, &mi) in matched.iter().enumerate().take(last).skip(self.btop) {
                let (m, chord, cmd) = &self.all_binds[mi];
                let y = by + (pos - self.btop) as u16;
                let is_sel = pos == self.bsel;
                if is_sel {
                    surface.set_style(
                        Rect::new(inner.x, y, inner.width, 1),
                        theme.get("ui.selection"),
                    );
                }
                render(
                    Paragraph::new(Span::styled(format!(" {m:<6}"), key_st)),
                    Rect::new(inner.x, y, 8, 1),
                    surface,
                );
                render(
                    Paragraph::new(Span::styled(
                        chord.clone(),
                        if is_sel { accent } else { text },
                    )),
                    Rect::new(inner.x + 8, y, 22, 1),
                    surface,
                );
                render(
                    Paragraph::new(Span::styled(format!("→ {cmd}"), dim)),
                    Rect::new(inner.x + 31, y, inner.x + inner.width - (inner.x + 31), 1),
                    surface,
                );
            }
            render(
                Paragraph::new(Span::styled(
                    " type to search · ↑/↓ move · Tab/Esc back to overrides",
                    dim,
                )),
                Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
                surface,
            );
            return;
        }

        // split: list (left) | edit fields (right)
        let split = inner.x + (inner.width * 3 / 5);
        let list_y = inner.y + 2;
        // header
        render(
            Paragraph::new(Span::styled("mode    chord → command", dim)),
            Rect::new(inner.x, list_y - 1, split - inner.x, 1),
            surface,
        );
        for (i, b) in self.binds.iter().enumerate() {
            let row = list_y + i as u16;
            if row >= inner.y + inner.height - 1 {
                break;
            }
            let is_sel = i == self.selected;
            if is_sel {
                surface.set_style(
                    Rect::new(inner.x, row, split - inner.x, 1),
                    theme.get("ui.selection"),
                );
            }
            let line = format!("{:<7} {} → {}", MODES[b.mode], b.chord, b.command);
            render(
                Paragraph::new(Span::styled(line, if is_sel { accent } else { text })),
                Rect::new(inner.x + 1, row, split - inner.x - 2, 1),
                surface,
            );
            self.row_hits.push((row, inner.x, split, i));
        }

        // right: edit form for the selected/edited bind
        let fx = split + 2;
        let src = if self.editing {
            &self.buf
        } else {
            self.binds.get(self.selected).unwrap_or(&self.buf)
        };
        let vals = [
            MODES[src.mode].to_string(),
            src.chord.clone(),
            src.command.clone(),
        ];
        for (fi, fname) in FIELDS.iter().enumerate() {
            let y = list_y + fi as u16 * 2;
            if y >= inner.y + inner.height - 1 {
                break;
            }
            let active = self.editing && fi == self.field;
            render(
                Paragraph::new(Span::styled(
                    format!("{fname}:"),
                    if active { accent } else { dim },
                )),
                Rect::new(fx, y, 10, 1),
                surface,
            );
            let vx = fx + 10;
            let shown = if active && fi != 0 {
                format!("{}▏", vals[fi])
            } else {
                vals[fi].clone()
            };
            let vstyle = if fi == 0 {
                key_st
            } else {
                text.add_modifier(if active {
                    RMod::UNDERLINED
                } else {
                    RMod::empty()
                })
            };
            render(
                Paragraph::new(Span::styled(shown, vstyle)),
                Rect::new(vx, y, (inner.x + inner.width).saturating_sub(vx), 1),
                surface,
            );
            self.field_hits.push((y, fx, inner.x + inner.width, fi));
        }

        let help = if self.capturing {
            " ⌨ press keys to record the chord · ⏎ finish capture · Esc cancel"
        } else if self.editing {
            " Tab/↑↓ field · Space cycles Mode · type or ⌨ Capture the chord · ⏎ save · Esc cancel"
        } else {
            " j/k move · a add · d delete · ⏎/e edit · ⌨ Capture key · click a field"
        };
        render(
            Paragraph::new(Span::styled(help, dim)),
            Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
            surface,
        );
    }
}
