//! Settings page — a comprehensive, auto-generated editor for **every** `[editor]`
//! setting. The schema isn't hand-maintained: the live editor `Config` is
//! serialized to TOML each render and every leaf is exposed, grouped by section.
//! Edits write to `~/.zmax/config.toml` under `[editor]` and live-reload.
//!
//! Bool → toggle · Int/Float/Str → type · arrays/tables → edit as a TOML literal.

use tui::buffer::Buffer as Surface;
use zmax_view::{
    graphics::Rect,
    input::{KeyCode, KeyEvent, MouseButton, MouseEventKind},
};

use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Bool,
    Int,
    Float,
    Str,
    Toml,                          // arrays / inline tables — edited as a TOML literal
    Enum(&'static [&'static str]), // cycle through a fixed set of values
}

/// Known enum-valued settings (path relative to `[editor]`) → allowed values.
/// Lets the UI offer a cycle/dropdown instead of free-text TOML editing.
const DIAG: &[&str] = &["disable", "hint", "info", "warning", "error"];
const CURSOR: &[&str] = &["block", "bar", "underline"];
const ENUMS: &[(&[&str], &[&str])] = &[
    (&["line-number"], &["absolute", "relative"]),
    (&["bufferline"], &["never", "always", "multiple"]),
    (&["popup-border"], &["none", "all", "popup", "menu"]),
    (&["statusline", "mode", "normal"], &["NOR", "NORMAL"]),
    (&["end-of-line-diagnostics"], DIAG),
    (&["inline-diagnostics", "cursor-line"], DIAG),
    (&["inline-diagnostics", "other-lines"], DIAG),
    (&["indent-heuristic"], &["simple", "tree-sitter", "hybrid"]),
    (&["whitespace", "render"], &["none", "all", "trailing"]),
    (&["cursor-shape", "normal"], CURSOR),
    (&["cursor-shape", "insert"], CURSOR),
    (&["cursor-shape", "select"], CURSOR),
    (&["default-line-ending"], &["native", "lf", "crlf"]),
    (&["startup"], &["startify", "recent", "session", "file"]),
];

fn enum_for(path: &[String]) -> Option<&'static [&'static str]> {
    ENUMS.iter().find_map(|(p, opts)| {
        (p.len() == path.len() && p.iter().zip(path).all(|(a, b)| *a == b.as_str()))
            .then_some(*opts)
    })
}

enum Row {
    Header(String),
    Field {
        path: Vec<String>,
        label: String,
        kind: Kind,
        value: toml::Value,
        /// true when the live value differs from the compiled default.
        modified: bool,
    },
}

fn config_path() -> std::path::PathBuf {
    zmax_loader::config_dir().join("config.toml")
}

/// Live-reload config + theme + keymaps with no restart.
fn live_reload(cx: &mut Context) {
    cx.editor
        .config_events
        .0
        .send(zmax_view::editor::ConfigEvent::Refresh)
        .ok();
}

/// The full effective `[editor]` config as a TOML value (defaults + overrides).
fn live_editor_config(ctx: &Context) -> toml::Value {
    toml::Value::try_from(&*ctx.editor.config())
        .unwrap_or_else(|_| toml::Value::Table(Default::default()))
}

/// The compiled-in default `[editor]` config (for "modified?" comparison).
fn default_editor_config() -> toml::Value {
    toml::Value::try_from(zmax_view::editor::Config::default())
        .unwrap_or_else(|_| toml::Value::Table(Default::default()))
}

/// Collect every scalar/array leaf with its full path.
fn leaves(v: &toml::Value, prefix: Vec<String>, out: &mut Vec<(Vec<String>, toml::Value)>) {
    match v {
        toml::Value::Table(t) => {
            let mut keys: Vec<&String> = t.keys().collect();
            keys.sort();
            for k in keys {
                let mut p = prefix.clone();
                p.push(k.clone());
                leaves(&t[k], p, out);
            }
        }
        other => out.push((prefix, other.clone())),
    }
}

fn get_path<'a>(v: &'a toml::Value, path: &[String]) -> Option<&'a toml::Value> {
    let mut cur = v;
    for k in path {
        cur = cur.get(k.as_str())?;
    }
    Some(cur)
}

fn kind_of(v: &toml::Value) -> Kind {
    match v {
        toml::Value::Boolean(_) => Kind::Bool,
        toml::Value::Integer(_) => Kind::Int,
        toml::Value::Float(_) => Kind::Float,
        toml::Value::String(_) => Kind::Str,
        _ => Kind::Toml,
    }
}

fn pretty_section(key: &str) -> String {
    let mut s: String = key.replace('-', " ");
    if let Some(c) = s.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    s
}

fn build_rows(cfg: &toml::Value, defaults: &toml::Value) -> Vec<Row> {
    let is_modified = |path: &[String], val: &toml::Value| get_path(defaults, path) != Some(val);
    let mut all = Vec::new();
    leaves(cfg, Vec::new(), &mut all);
    all.sort_by(|a, b| a.0.cmp(&b.0));
    // group by top-level segment; scalars at depth 1 go under "General".
    let mut rows = Vec::new();
    let mut cur_section: Option<String> = None;
    // General first (depth-1 leaves)
    rows.push(Row::Header("General".into()));
    for (path, val) in &all {
        if path.len() == 1 {
            rows.push(Row::Field {
                path: path.clone(),
                label: path[0].clone(),
                kind: enum_for(path)
                    .map(Kind::Enum)
                    .unwrap_or_else(|| kind_of(val)),
                modified: is_modified(path, val),
                value: val.clone(),
            });
        }
    }
    for (path, val) in &all {
        if path.len() < 2 {
            continue;
        }
        let section = &path[0];
        if cur_section.as_deref() != Some(section.as_str()) {
            rows.push(Row::Header(pretty_section(section)));
            cur_section = Some(section.clone());
        }
        rows.push(Row::Field {
            path: path.clone(),
            label: path[1..].join("."),
            kind: enum_for(path)
                .map(Kind::Enum)
                .unwrap_or_else(|| kind_of(val)),
            modified: is_modified(path, val),
            value: val.clone(),
        });
    }
    rows
}

fn display(kind: Kind, v: &toml::Value) -> String {
    match (kind, v) {
        (Kind::Bool, toml::Value::Boolean(b)) => {
            if *b {
                "✓ on".into()
            } else {
                "✗ off".into()
            }
        }
        (Kind::Enum(_), toml::Value::String(s)) => format!("{s} ▾"),
        (_, toml::Value::String(s)) => s.clone(),
        (_, other) => other.to_string().trim().to_string(),
    }
}

fn raw(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string().trim().to_string(),
    }
}

/// Set `[editor].<path>` in the user's config.toml (preserving everything else).
fn set_user(path: &[String], val: toml::Value) {
    let p = config_path();
    let mut cfg: toml::Value = std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_else(|| toml::Value::Table(Default::default()));
    if !cfg.is_table() {
        cfg = toml::Value::Table(Default::default());
    }
    // navigate/create editor.<path...>
    let mut full = vec!["editor".to_string()];
    full.extend_from_slice(path);
    let mut cur = &mut cfg;
    for (i, k) in full.iter().enumerate() {
        if i == full.len() - 1 {
            if let Some(t) = cur.as_table_mut() {
                t.insert(k.clone(), val);
            }
            break;
        }
        let t = cur.as_table_mut().unwrap();
        cur = t
            .entry(k.clone())
            .or_insert_with(|| toml::Value::Table(Default::default()));
        if !cur.is_table() {
            *cur = toml::Value::Table(Default::default());
        }
    }
    if let Ok(s) = toml::to_string_pretty(&cfg) {
        if let Some(par) = p.parent() {
            let _ = std::fs::create_dir_all(par);
        }
        let _ = std::fs::write(p, s);
    }
}

/// Remove `[editor].<path>` from the user's config.toml (reset to default).
fn remove_user(path: &[String]) {
    let p = config_path();
    let Some(content) = std::fs::read_to_string(&p).ok() else {
        return;
    };
    let Ok(mut cfg) = toml::from_str::<toml::Value>(&content) else {
        return;
    };
    fn remove_rec(v: &mut toml::Value, keys: &[String]) {
        let Some(t) = v.as_table_mut() else { return };
        if keys.len() == 1 {
            t.remove(&keys[0]);
        } else if let Some(child) = t.get_mut(&keys[0]) {
            remove_rec(child, &keys[1..]);
        }
    }
    let mut full = vec!["editor".to_string()];
    full.extend_from_slice(path);
    remove_rec(&mut cfg, &full);
    if let Ok(s) = toml::to_string_pretty(&cfg) {
        let _ = std::fs::write(p, s);
    }
}

pub struct SettingsPanel {
    rows: Vec<Row>,
    sel: usize, // index into rows (always a Field)
    top: usize, // scroll offset
    editing: bool,
    buf: String,
    filter: String,
    filtering: bool,
    /// When true, only settings whose live value differs from the compiled
    /// default are shown (Emacs `customize-unsaved` / `customize-changed`).
    modified_only: bool,
    row_hits: Vec<(u16, u16, u16, usize)>,
    btn_hits: Vec<(u16, u16, u16, u8)>,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            sel: 0,
            top: 0,
            editing: false,
            buf: String::new(),
            filter: String::new(),
            filtering: false,
            modified_only: false,
            row_hits: Vec::new(),
            btn_hits: Vec::new(),
        }
    }

    /// Open the Settings tab pre-filtered to `filter` (Emacs `customize-variable`
    /// / `customize-option` / `customize-apropos` / `customize-group`).
    pub fn with_filter(filter: String) -> Self {
        Self {
            filter,
            ..Self::new()
        }
    }

    /// Open the Settings tab showing only settings changed from their default
    /// (Emacs `customize-unsaved` / `customize-changed` / `customize-saved`).
    pub fn with_modified_only() -> Self {
        Self {
            modified_only: true,
            ..Self::new()
        }
    }

    fn visible(&self) -> Vec<usize> {
        // indices of rows that pass the filter (headers always shown if they have matches)
        let f = self.filter.to_lowercase();
        if f.is_empty() && !self.modified_only {
            return (0..self.rows.len()).collect();
        }
        let mut out = Vec::new();
        for (i, r) in self.rows.iter().enumerate() {
            if let Row::Field {
                path,
                label,
                modified,
                ..
            } = r
            {
                let matches_text = f.is_empty()
                    || label.to_lowercase().contains(&f)
                    || path.join(".").to_lowercase().contains(&f);
                if matches_text && (!self.modified_only || *modified) {
                    out.push(i);
                }
            }
        }
        out
    }

    fn is_field(&self, i: usize) -> bool {
        matches!(self.rows.get(i), Some(Row::Field { .. }))
    }

    fn move_sel(&mut self, down: bool, vis: &[usize]) {
        let fields: Vec<usize> = vis.iter().copied().filter(|&i| self.is_field(i)).collect();
        if fields.is_empty() {
            return;
        }
        let cur = fields.iter().position(|&i| i == self.sel).unwrap_or(0);
        let next = if down {
            (cur + 1) % fields.len()
        } else {
            (cur + fields.len() - 1) % fields.len()
        };
        self.sel = fields[next];
    }

    fn activate(&mut self, cx: &mut Context) {
        let Some(Row::Field {
            path, kind, value, ..
        }) = self.rows.get(self.sel)
        else {
            return;
        };
        match kind {
            Kind::Bool => {
                let cur = matches!(value, toml::Value::Boolean(true));
                let path = path.clone();
                set_user(&path, toml::Value::Boolean(!cur));
                live_reload(cx);
            }
            Kind::Enum(opts) => {
                // Cycle to the next allowed value (like a dropdown).
                let cur = raw(value);
                let idx = opts
                    .iter()
                    .position(|o| *o == cur)
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let next = opts[idx % opts.len()].to_string();
                let path = path.clone();
                set_user(&path, toml::Value::String(next));
                live_reload(cx);
            }
            _ => {
                self.buf = raw(value);
                self.editing = true;
            }
        }
    }

    fn reset(&mut self, cx: &mut Context) {
        if let Some(Row::Field { path, .. }) = self.rows.get(self.sel) {
            let path = path.clone();
            remove_user(&path);
            live_reload(cx);
        }
    }

    fn commit(&mut self, cx: &mut Context) {
        let Some(Row::Field { path, kind, .. }) = self.rows.get(self.sel) else {
            self.editing = false;
            return;
        };
        let path = path.clone();
        let v = self.buf.trim();
        let parsed = match kind {
            Kind::Int => v.parse::<i64>().ok().map(toml::Value::Integer),
            Kind::Float => v.parse::<f64>().ok().map(toml::Value::Float),
            Kind::Str => Some(toml::Value::String(v.to_string())),
            Kind::Toml => toml::from_str::<toml::Value>(&format!("v = {v}"))
                .ok()
                .and_then(|t| t.get("v").cloned()),
            // Bool and Enum never enter text-edit mode (they cycle in `activate`).
            Kind::Bool | Kind::Enum(_) => None,
        };
        if let Some(val) = parsed {
            set_user(&path, val);
            live_reload(cx);
        }
        self.editing = false;
    }

    fn open_raw_cb() -> Callback {
        Box::new(|c: &mut Compositor, cx: &mut Context| {
            c.pop();
            let _ = cx
                .editor
                .open(&config_path(), zmax_view::editor::Action::Replace);
        })
    }

    fn handle_mouse(
        &mut self,
        col: u16,
        row: u16,
        kind: MouseEventKind,
        cx: &mut Context,
    ) -> EventResult {
        match kind {
            MouseEventKind::ScrollDown => {
                self.top = (self.top + 3).min(self.rows.len().saturating_sub(1));
                return EventResult::Consumed(None);
            }
            MouseEventKind::ScrollUp => {
                self.top = self.top.saturating_sub(3);
                return EventResult::Consumed(None);
            }
            MouseEventKind::Down(MouseButton::Left) => {}
            _ => return EventResult::Consumed(None),
        }
        if let Some(&(_, _, _, b)) = self
            .btn_hits
            .iter()
            .find(|&&(x0, x1, r, _)| row == r && col >= x0 && col < x1)
        {
            return match b {
                0 => EventResult::Consumed(Some(Self::open_raw_cb())),
                _ => EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                    c.pop();
                }))),
            };
        }
        if let Some(&(_, _, _, idx)) = self
            .row_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            if self.is_field(idx) {
                self.editing = false;
                self.sel = idx;
                self.activate(cx);
            }
        }
        EventResult::Consumed(None)
    }
}

impl Component for SettingsPanel {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key: KeyEvent = match event {
            Event::Key(k) => *k,
            Event::Mouse(ev) => return self.handle_mouse(ev.column, ev.row, ev.kind, cx),
            _ => return EventResult::Ignored(None),
        };
        if self.filtering {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.filtering = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                }
                KeyCode::Char(c) => self.filter.push(c),
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        if self.editing {
            match key.code {
                KeyCode::Esc => self.editing = false,
                KeyCode::Enter => self.commit(cx),
                KeyCode::Backspace => {
                    self.buf.pop();
                }
                KeyCode::Char(c) => self.buf.push(c),
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        let vis = self.visible();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                return EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                    c.pop();
                })))
            }
            KeyCode::Char('/') => {
                self.filtering = true;
                self.filter.clear();
            }
            KeyCode::Char('o') => return EventResult::Consumed(Some(Self::open_raw_cb())),
            KeyCode::Char('r') => self.reset(cx),
            KeyCode::Char('j') | KeyCode::Down => self.move_sel(true, &vis),
            KeyCode::Char('k') | KeyCode::Up => self.move_sel(false, &vis),
            KeyCode::Char(' ') | KeyCode::Enter => self.activate(cx),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::Span;
        use ratatui::widgets::Paragraph;

        // Rebuild from the live config each frame so every setting reflects reality.
        self.rows = build_rows(&live_editor_config(ctx), &default_editor_config());
        if !self.is_field(self.sel) {
            // snap selection onto the first field
            self.sel = self
                .rows
                .iter()
                .position(|r| matches!(r, Row::Field { .. }))
                .unwrap_or(0);
        }
        self.row_hits.clear();
        self.btn_hits.clear();

        let theme = &ctx.editor.theme;
        let bg = to_rat_style(theme.get("ui.background"));
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let border = to_rat_style(theme.get("ui.window"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let head = to_rat_style(theme.get("keyword")).add_modifier(RMod::BOLD);
        let valc = to_rat_style(theme.get("string"));
        // `transparent-background`: drop the page fill so the terminal shows through.
        let mut page_bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            page_bg.bg = None;
        }
        surface.clear_with(area, page_bg);

        let title = format!(
            " Editor Settings — {} options ",
            self.rows
                .iter()
                .filter(|r| matches!(r, Row::Field { .. }))
                .count()
        );
        // flat page header bar (no modal frame)
        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        render(
            Paragraph::new(Span::styled(title, accent)),
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
        if inner.width < 12 || inner.height < 5 {
            return;
        }

        // top: buttons + filter
        let mut bx = inner.x + 1;
        for (lbl, b) in [(" 📄 Raw ", 0u8), (" ✕ Close ", 1u8)] {
            let w = lbl.chars().count() as u16;
            render(
                Paragraph::new(Span::styled(lbl, text.add_modifier(RMod::REVERSED))),
                Rect::new(bx, inner.y, w, 1),
                surface,
            );
            self.btn_hits.push((bx, bx + w, inner.y, b));
            bx += w + 1;
        }
        let fstr = if self.filtering || !self.filter.is_empty() {
            format!(
                "  🔍 {}{}",
                self.filter,
                if self.filtering { "▏" } else { "" }
            )
        } else {
            "  /  to search".into()
        };
        render(
            Paragraph::new(Span::styled(fstr, dim)),
            Rect::new(
                bx + 1,
                inner.y,
                inner.width.saturating_sub(bx - inner.x + 1),
                1,
            ),
            surface,
        );

        // body: scrollable list
        let body_y = inner.y + 2;
        let body_h = inner.height.saturating_sub(3);
        let val_x = inner.x + 36.min(inner.width / 2);
        let vis = self.visible();
        // keep selection in view
        if let Some(pos) = vis.iter().position(|&i| i == self.sel) {
            if pos < self.top {
                self.top = pos;
            } else if pos >= self.top + body_h as usize {
                self.top = pos + 1 - body_h as usize;
            }
        }
        for (line, &ri) in vis.iter().skip(self.top).take(body_h as usize).enumerate() {
            let y = body_y + line as u16;
            match &self.rows[ri] {
                Row::Header(name) => {
                    render(
                        Paragraph::new(Span::styled(format!("▸ {name}"), head)),
                        Rect::new(inner.x, y, inner.width, 1),
                        surface,
                    );
                }
                Row::Field {
                    label,
                    kind,
                    value,
                    modified,
                    ..
                } => {
                    let is_sel = ri == self.sel;
                    if is_sel {
                        surface.set_style(
                            Rect::new(inner.x, y, inner.width, 1),
                            theme.get("ui.selection"),
                        );
                    }
                    // ● marks a value changed from its default (resettable with `r`).
                    let marker = if *modified { "●" } else { " " };
                    render(
                        Paragraph::new(Span::styled(marker, accent)),
                        Rect::new(inner.x, y, 1, 1),
                        surface,
                    );
                    render(
                        Paragraph::new(Span::styled(
                            format!(" {label}"),
                            if is_sel { accent } else { text },
                        )),
                        Rect::new(inner.x + 1, y, val_x - inner.x - 2, 1),
                        surface,
                    );
                    let shown = if is_sel && self.editing {
                        format!("{}▏", self.buf)
                    } else {
                        display(*kind, value)
                    };
                    let vstyle = if is_sel && self.editing {
                        text.add_modifier(RMod::UNDERLINED)
                    } else {
                        valc
                    };
                    render(
                        Paragraph::new(Span::styled(shown, vstyle)),
                        Rect::new(val_x, y, inner.x + inner.width - val_x, 1),
                        surface,
                    );
                    self.row_hits.push((y, inner.x, inner.x + inner.width, ri));
                }
            }
        }

        let help = if self.editing {
            " type a value · ⏎ save · Esc cancel"
        } else if self.filtering {
            " type to filter · ⏎/Esc done"
        } else {
            " j/k move · Space/⏎ toggle/edit · r reset (● = changed) · / search · o raw · Esc close"
        };
        render(
            Paragraph::new(Span::styled(help, dim)),
            Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
            surface,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_every_editor_setting() {
        let cfg = toml::Value::try_from(zmax_view::editor::Config::default())
            .expect("Config serializes to TOML");
        let rows = build_rows(&cfg, &default_editor_config());
        let fields = rows
            .iter()
            .filter(|r| matches!(r, Row::Field { .. }))
            .count();
        let headers = rows.iter().filter(|r| matches!(r, Row::Header(_))).count();
        eprintln!("settings: {fields} fields across {headers} sections");
        assert!(
            fields > 40,
            "expected the full editor surface, got {fields}"
        );
        assert!(headers > 4, "expected multiple sections, got {headers}");
    }
}
