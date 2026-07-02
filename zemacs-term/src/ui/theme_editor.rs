//! Full custom color-scheme editor: edit per-scope foreground colors with a live
//! swatch, then save to `~/.zemacs/themes/<name>.toml` and point `config.toml` at
//! it. Colors seed from the currently-active theme on first render.

use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::{Color, Modifier, Rect},
    input::{KeyCode, KeyEvent, MouseButton, MouseEventKind},
};

/// (bit, label, toml-name, modifier) for the toggleable text styles, in display order.
const MODS: [(u8, char, &str, Modifier); 3] = [
    (0b001, 'B', "bold", Modifier::BOLD),
    (0b010, 'I', "italic", Modifier::ITALIC),
    (0b100, 'D', "dim", Modifier::DIM),
];

fn mods_of(m: Modifier) -> u8 {
    MODS.iter().fold(
        0u8,
        |acc, (bit, _, _, flag)| if m.contains(*flag) { acc | bit } else { acc },
    )
}

/// Build a ratatui style from an edited (fg-hex, bg-hex, modifier-mask).
fn style_for(fg: &str, bg: &str, mods: u8) -> ratatui::style::Style {
    use ratatui::style::{Color as RC, Modifier as RM, Style as RS};
    let mut st = RS::default();
    if let Ok(Color::Rgb(r, g, b)) = Color::from_hex(fg) {
        st = st.fg(RC::Rgb(r, g, b));
    }
    if let Ok(Color::Rgb(r, g, b)) = Color::from_hex(bg) {
        st = st.bg(RC::Rgb(r, g, b));
    }
    if mods & 0b001 != 0 {
        st = st.add_modifier(RM::BOLD);
    }
    if mods & 0b010 != 0 {
        st = st.add_modifier(RM::ITALIC);
    }
    if mods & 0b100 != 0 {
        st = st.add_modifier(RM::DIM);
    }
    st
}

use crate::compositor::{Component, Context, Event, EventResult};

/// Curated, high-impact theme scopes exposed in the editor.
const SCOPES: &[&str] = &[
    "ui.background",
    "ui.text",
    "ui.text.focus",
    "ui.selection",
    "ui.cursor",
    "ui.window",
    "ui.statusline",
    "comment",
    "keyword",
    "function",
    "type",
    "constant",
    "string",
    "variable",
    "operator",
    "diff.plus",
    "diff.minus",
    "warning",
    "error",
];

fn theme_name() -> &'static str {
    "zemacs-custom"
}

fn hex_of(c: Option<Color>) -> String {
    match c {
        Some(Color::Rgb(r, g, b)) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => String::new(),
    }
}

pub struct ThemeEditor {
    /// (scope, fg-hex) — seeded from the active theme on first render.
    colors: Vec<(String, String)>,
    /// Parallel background-hex per scope (empty = inherit).
    bgs: Vec<String>,
    /// Parallel modifier bitmask per scope (see `MODS`).
    mods: Vec<u8>,
    /// Name the custom theme is saved under (`<name>.toml`).
    custom_name: String,
    /// true while typing the custom theme name.
    naming: bool,
    /// 0 = editing foreground, 1 = editing background.
    target: u8,
    selected: usize,
    editing: bool,
    buf: String,
    seeded: bool,
    saved_msg: bool,
    /// All installed theme names (left pane); selecting one applies it live.
    themes: Vec<String>,
    theme_sel: usize,
    theme_top: usize,
    /// 0 = theme list focused, 1 = scope editor focused.
    pane: u8,
    row_hits: Vec<(u16, u16, u16, usize)>,
    theme_hits: Vec<(u16, u16, u16, usize)>,
    btn_hits: Vec<(u16, u16, u16, u8)>, // 0 = save
}

impl Default for ThemeEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeEditor {
    pub fn new() -> Self {
        let themes = crate::commands::typed::all_theme_names();
        Self {
            colors: Vec::new(),
            bgs: Vec::new(),
            mods: Vec::new(),
            custom_name: "my-theme".to_string(),
            naming: false,
            target: 0,
            selected: 0,
            editing: false,
            buf: String::new(),
            seeded: false,
            saved_msg: false,
            themes,
            theme_sel: 0,
            theme_top: 0,
            pane: 1,
            row_hits: Vec::new(),
            theme_hits: Vec::new(),
            btn_hits: Vec::new(),
        }
    }

    /// Load + apply a theme live, persist `theme = <name>`, and reseed the scope colors.
    fn apply_theme(&mut self, cx: &mut Context, name: &str) {
        if let Ok(theme) = cx.editor.theme_loader.load(name) {
            let _ = cx.editor.set_theme(theme);
            // persist theme = name (preserving other config keys)
            let cfg_path = zemacs_loader::config_dir().join("config.toml");
            let mut cfg: toml::Value = std::fs::read_to_string(&cfg_path)
                .ok()
                .and_then(|s| toml::from_str(&s).ok())
                .unwrap_or_else(|| toml::Value::Table(Default::default()));
            if let Some(t) = cfg.as_table_mut() {
                t.insert("theme".into(), toml::Value::String(name.to_string()));
            }
            if let Ok(s) = toml::to_string_pretty(&cfg) {
                let _ = std::fs::write(cfg_path, s);
            }
            // reseed scope colors from the newly-active theme
            self.colors = SCOPES
                .iter()
                .map(|s| (s.to_string(), hex_of(cx.editor.theme.get(s).fg)))
                .collect();
            self.bgs = SCOPES
                .iter()
                .map(|s| hex_of(cx.editor.theme.get(s).bg))
                .collect();
            self.mods = SCOPES
                .iter()
                .map(|s| mods_of(cx.editor.theme.get(s).add_modifier))
                .collect();
        }
    }

    fn save(&mut self) {
        // Write `scope = "#fg"` or `scope = { fg = "#..", bg = "#.." }` when a bg is set.
        let valid = |h: &str| h.starts_with('#') && (h.len() == 7 || h.len() == 4);
        let mut body = String::from("# Generated by the zemacs color-scheme editor\n");
        for (i, (scope, fg)) in self.colors.iter().enumerate() {
            let bg = self.bgs.get(i).map(|s| s.as_str()).unwrap_or("");
            let m = self.mods.get(i).copied().unwrap_or(0);
            let mod_list: Vec<String> = MODS
                .iter()
                .filter(|(bit, _, _, _)| m & bit != 0)
                .map(|(_, _, name, _)| format!("\"{name}\""))
                .collect();
            // build the inline-table parts that are present
            let mut parts = Vec::new();
            if valid(fg) {
                parts.push(format!("fg = \"{fg}\""));
            }
            if valid(bg) {
                parts.push(format!("bg = \"{bg}\""));
            }
            if !mod_list.is_empty() {
                parts.push(format!("modifiers = [{}]", mod_list.join(", ")));
            }
            match parts.len() {
                0 => {}
                1 if valid(fg) && bg.is_empty() && mod_list.is_empty() => {
                    body.push_str(&format!("\"{scope}\" = \"{fg}\"\n"))
                }
                _ => body.push_str(&format!("\"{scope}\" = {{ {} }}\n", parts.join(", "))),
            }
        }
        // Sanitize the name into a safe file stem.
        let name: String = self
            .custom_name
            .trim()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let name = if name.is_empty() {
            theme_name().to_string()
        } else {
            name
        };
        let dir = zemacs_loader::config_dir().join("themes");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{name}.toml")), body);

        // Point config.toml at the saved theme (preserving other keys).
        let cfg_path = zemacs_loader::config_dir().join("config.toml");
        let mut cfg: toml::Value = std::fs::read_to_string(&cfg_path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_else(|| toml::Value::Table(Default::default()));
        if let Some(t) = cfg.as_table_mut() {
            t.insert("theme".into(), toml::Value::String(name.clone()));
        }
        if let Ok(s) = toml::to_string_pretty(&cfg) {
            let _ = std::fs::write(cfg_path, s);
        }
        self.custom_name = name.clone();
        // The new theme should appear in the picker immediately.
        self.themes = crate::commands::typed::all_theme_names();
        if let Some(i) = self.themes.iter().position(|n| *n == name) {
            self.theme_sel = i;
        }
        self.saved_msg = true;
    }

    fn commit(&mut self) {
        let v = self.buf.trim().to_string();
        // allow clearing a background (empty); foreground must be a valid hex
        let ok = v.is_empty() || Color::from_hex(&v).is_ok();
        if ok {
            if self.target == 1 {
                if let Some(bg) = self.bgs.get_mut(self.selected) {
                    *bg = v;
                }
            } else if !v.is_empty() {
                if let Some((_, fg)) = self.colors.get_mut(self.selected) {
                    *fg = v;
                }
            }
        }
        self.editing = false;
    }

    /// The hex currently being edited (fg or bg of the selected scope).
    fn cur_hex(&self) -> String {
        if self.target == 1 {
            self.bgs.get(self.selected).cloned().unwrap_or_default()
        } else {
            self.colors
                .get(self.selected)
                .map(|c| c.1.clone())
                .unwrap_or_default()
        }
    }

    fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) -> EventResult {
        match kind {
            MouseEventKind::ScrollDown => {
                if self.pane == 0 && !self.themes.is_empty() {
                    self.theme_sel = (self.theme_sel + 1).min(self.themes.len() - 1);
                } else if !self.colors.is_empty() {
                    self.selected = (self.selected + 1).min(self.colors.len() - 1);
                }
                return EventResult::Consumed(None);
            }
            MouseEventKind::ScrollUp => {
                if self.pane == 0 {
                    self.theme_sel = self.theme_sel.saturating_sub(1);
                } else {
                    self.selected = self.selected.saturating_sub(1);
                }
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
            if b == 1 {
                self.naming = true;
            } else {
                self.save();
            }
            return EventResult::Consumed(None);
        }
        if let Some(&(_, _, _, idx)) = self
            .theme_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            // Click only selects the theme (like j/k); apply is explicit via
            // Enter — see the "Themes (⏎ apply)" header — so a click never
            // silently changes and persists the colorscheme.
            self.pane = 0;
            self.theme_sel = idx;
            return EventResult::Consumed(None);
        }
        if let Some(&(_, _, _, idx)) = self
            .row_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            self.pane = 1;
            self.selected = idx;
            self.buf = self.cur_hex();
            self.editing = true;
            self.saved_msg = false;
        }
        EventResult::Consumed(None)
    }
}

impl Component for ThemeEditor {
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
                let saved = self.saved_msg;
                let r = self.handle_mouse(ev.column, ev.row, ev.kind);
                if self.saved_msg && !saved {
                    reload(cx);
                }
                return r;
            }
            _ => return EventResult::Ignored(None),
        };
        self.saved_msg = false;
        if self.naming {
            match key.code {
                KeyCode::Esc => self.naming = false,
                KeyCode::Enter => {
                    self.naming = false;
                    self.save();
                    reload(cx);
                }
                KeyCode::Backspace => {
                    self.custom_name.pop();
                }
                KeyCode::Char(c) => self.custom_name.push(c),
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        if self.editing {
            match key.code {
                KeyCode::Esc => self.editing = false,
                KeyCode::Enter => self.commit(),
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
                return EventResult::Consumed(Some(Box::new(|c, _| {
                    c.pop();
                })))
            }
            // Tab / h / l toggle focus between the theme list and the scope editor.
            KeyCode::Tab | KeyCode::Char('h') | KeyCode::Char('l') => self.pane ^= 1,
            // f/b switch the edit target between foreground and background.
            KeyCode::Char('f') => self.target = 0,
            KeyCode::Char('b') => self.target = 1,
            // n names the custom theme before saving.
            KeyCode::Char('n') => self.naming = true,
            // 1/2/3 toggle bold / italic / dim on the selected scope.
            KeyCode::Char(d @ '1'..='3') if self.pane == 1 => {
                let idx = d as usize - '1' as usize;
                if let (Some(m), Some((bit, _, _, _))) =
                    (self.mods.get_mut(self.selected), MODS.get(idx))
                {
                    *m ^= bit;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.pane == 0 {
                    if !self.themes.is_empty() {
                        self.theme_sel = (self.theme_sel + 1) % self.themes.len();
                    }
                } else if !self.colors.is_empty() {
                    self.selected = (self.selected + 1) % self.colors.len();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.pane == 0 {
                    if !self.themes.is_empty() {
                        self.theme_sel =
                            (self.theme_sel + self.themes.len() - 1) % self.themes.len();
                    }
                } else if !self.colors.is_empty() {
                    self.selected = (self.selected + self.colors.len() - 1) % self.colors.len();
                }
            }
            KeyCode::Enter => {
                if self.pane == 0 {
                    if let Some(name) = self.themes.get(self.theme_sel).cloned() {
                        self.apply_theme(cx, &name);
                    }
                } else {
                    self.buf = self.cur_hex();
                    self.editing = true;
                }
            }
            KeyCode::Char('s') => {
                self.save();
                reload(cx);
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::{Modifier as RMod, Style as RStyle};
        use ratatui::text::Span;
        use ratatui::widgets::Paragraph;

        if !self.seeded {
            self.colors = SCOPES
                .iter()
                .map(|s| (s.to_string(), hex_of(ctx.editor.theme.get(s).fg)))
                .collect();
            self.bgs = SCOPES
                .iter()
                .map(|s| hex_of(ctx.editor.theme.get(s).bg))
                .collect();
            self.mods = SCOPES
                .iter()
                .map(|s| mods_of(ctx.editor.theme.get(s).add_modifier))
                .collect();
            self.seeded = true;
        }
        self.row_hits.clear();
        self.theme_hits.clear();
        self.btn_hits.clear();

        let theme = &ctx.editor.theme;
        let bg = to_rat_style(theme.get("ui.background"));
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let border = to_rat_style(theme.get("ui.window"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let cur_theme = ctx.editor.theme.name().to_string();
        surface.clear_with(area, theme.get("ui.background"));

        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        render(
            Paragraph::new(Span::styled(
                format!(" Color Scheme — {cur_theme} "),
                accent,
            )),
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
        if inner.width < 16 || inner.height < 5 {
            return;
        }

        // save button + name field (top)
        let label = if self.saved_msg {
            " ✓ Saved — applied live "
        } else {
            " 💾 Save theme "
        };
        let w = label.chars().count() as u16;
        render(
            Paragraph::new(Span::styled(label, text.add_modifier(RMod::REVERSED))),
            Rect::new(inner.x, inner.y, w, 1),
            surface,
        );
        self.btn_hits.push((inner.x, inner.x + w, inner.y, 0));
        // editable "as: <name>" field
        let nx = inner.x + w + 2;
        let nlabel = if self.naming {
            format!(" as: {}▏ ", self.custom_name)
        } else {
            format!(" as: {} ✎ ", self.custom_name)
        };
        let nw = nlabel.chars().count() as u16;
        let nst = if self.naming {
            accent.add_modifier(RMod::REVERSED)
        } else {
            dim
        };
        render(
            Paragraph::new(Span::styled(nlabel, nst)),
            Rect::new(nx, inner.y, nw, 1),
            surface,
        );
        self.btn_hits.push((nx, nx + nw, inner.y, 1));

        let body_y = inner.y + 2;
        let body_h = inner.height.saturating_sub(4); // reserve a row for the live preview
                                                     // LEFT: theme picker
        let tw = 22u16.min(inner.width / 2);
        render(
            Paragraph::new(Span::styled(
                "Themes (⏎ apply)",
                if self.pane == 0 { accent } else { dim },
            )),
            Rect::new(inner.x, body_y - 1, tw, 1),
            surface,
        );
        if self.theme_sel < self.theme_top {
            self.theme_top = self.theme_sel;
        } else if self.theme_sel >= self.theme_top + body_h as usize {
            self.theme_top = self.theme_sel + 1 - body_h as usize;
        }
        for (line, name) in self
            .themes
            .iter()
            .enumerate()
            .skip(self.theme_top)
            .take(body_h as usize)
        {
            let row = body_y + (line - self.theme_top) as u16;
            let is_sel = line == self.theme_sel;
            let active = *name == cur_theme;
            if is_sel && self.pane == 0 {
                surface.set_style(Rect::new(inner.x, row, tw, 1), theme.get("ui.selection"));
            }
            let mark = if active { "● " } else { "  " };
            render(
                Paragraph::new(Span::styled(
                    format!("{mark}{name}"),
                    if is_sel { accent } else { text },
                )),
                Rect::new(inner.x, row, tw, 1),
                surface,
            );
            self.theme_hits.push((row, inner.x, inner.x + tw, line));
        }

        // divider
        let dx = inner.x + tw + 1;
        for y in body_y..body_y + body_h {
            render(
                Paragraph::new(Span::styled("│", dim)),
                Rect::new(dx, y, 1, 1),
                surface,
            );
        }

        // RIGHT: scope color editor   name | #hex | swatch
        let sx = dx + 2;
        let avail = inner.x + inner.width - sx;
        let hex_x = sx + 18.min(avail / 3);
        let swatch_x = hex_x + 8;
        let bg_x = swatch_x + 5;
        let bg_sw_x = bg_x + 8;
        // column headers
        render(
            Paragraph::new(Span::styled(
                "fg",
                if self.target == 0 { accent } else { dim },
            )),
            Rect::new(hex_x, body_y - 1, 8, 1),
            surface,
        );
        render(
            Paragraph::new(Span::styled(
                "bg (b)",
                if self.target == 1 { accent } else { dim },
            )),
            Rect::new(bg_x, body_y - 1, 8, 1),
            surface,
        );
        render(
            Paragraph::new(Span::styled(
                "Edit colors",
                if self.pane == 1 { accent } else { dim },
            )),
            Rect::new(sx, body_y - 1, inner.x + inner.width - sx, 1),
            surface,
        );
        for i in 0..self.colors.len() {
            let row = body_y + i as u16;
            if row >= body_y + body_h {
                break;
            }
            let (scope, fg) = (self.colors[i].0.clone(), self.colors[i].1.clone());
            let bg = self.bgs.get(i).cloned().unwrap_or_default();
            let is_sel = i == self.selected && self.pane == 1;
            if is_sel {
                surface.set_style(
                    Rect::new(sx, row, inner.x + inner.width - sx, 1),
                    theme.get("ui.selection"),
                );
            }
            render(
                Paragraph::new(Span::styled(scope, if is_sel { accent } else { text })),
                Rect::new(sx, row, hex_x - sx - 1, 1),
                surface,
            );
            // fg hex + swatch
            let fg_buf = is_sel && self.editing && self.target == 0;
            let fg_shown = if fg_buf {
                format!("{}▏", self.buf)
            } else {
                fg.clone()
            };
            render(
                Paragraph::new(Span::styled(fg_shown, text.add_modifier(RMod::UNDERLINED))),
                Rect::new(hex_x, row, 8, 1),
                surface,
            );
            let fg_cur = if fg_buf {
                self.buf.as_str()
            } else {
                fg.as_str()
            };
            if let Ok(Color::Rgb(r, g, b)) = Color::from_hex(fg_cur) {
                render(
                    Paragraph::new(Span::styled(
                        "    ",
                        RStyle::default().bg(ratatui::style::Color::Rgb(r, g, b)),
                    )),
                    Rect::new(swatch_x, row, 4, 1),
                    surface,
                );
            }
            // bg hex + swatch
            let bg_buf = is_sel && self.editing && self.target == 1;
            let bg_shown = if bg_buf {
                format!("{}▏", self.buf)
            } else if bg.is_empty() {
                "—".into()
            } else {
                bg.clone()
            };
            render(
                Paragraph::new(Span::styled(bg_shown, text.add_modifier(RMod::UNDERLINED))),
                Rect::new(bg_x, row, 8, 1),
                surface,
            );
            let bg_cur = if bg_buf {
                self.buf.as_str()
            } else {
                bg.as_str()
            };
            if let Ok(Color::Rgb(r, g, b)) = Color::from_hex(bg_cur) {
                render(
                    Paragraph::new(Span::styled(
                        "    ",
                        RStyle::default().bg(ratatui::style::Color::Rgb(r, g, b)),
                    )),
                    Rect::new(bg_sw_x, row, 4, 1),
                    surface,
                );
            }
            // modifier indicators: B I D (lit when set)
            let m = self.mods.get(i).copied().unwrap_or(0);
            let mods_x = bg_sw_x + 5;
            for (j, (bit, ch, _, _)) in MODS.iter().enumerate() {
                let on = m & bit != 0;
                let st = if on {
                    accent.add_modifier(RMod::REVERSED)
                } else {
                    dim
                };
                render(
                    Paragraph::new(Span::styled(format!("{ch}"), st)),
                    Rect::new(mods_x + j as u16 * 2, row, 1, 1),
                    surface,
                );
            }
            self.row_hits.push((row, sx, inner.x + inner.width, i));
        }

        // Live preview: a sample snippet styled with the current (edited) theme.
        let preview_y = inner.y + inner.height - 2;
        let sample: &[(&str, &str)] = &[
            ("comment", "// preview"),
            ("keyword", "fn"),
            ("function", "main"),
            ("operator", "()"),
            ("keyword", "let"),
            ("variable", "x"),
            ("operator", "="),
            ("string", "\"hi\""),
            ("constant", "42"),
            ("type", "Vec"),
        ];
        render(
            Paragraph::new(Span::styled("preview ", dim)),
            Rect::new(inner.x, preview_y, 8, 1),
            surface,
        );
        let mut px = inner.x + 8;
        for (scope, word) in sample {
            if px + word.chars().count() as u16 + 1 >= inner.x + inner.width {
                break;
            }
            if let Some(i) = SCOPES.iter().position(|s| s == scope) {
                let st = style_for(
                    &self.colors[i].1,
                    self.bgs.get(i).map(|s| s.as_str()).unwrap_or(""),
                    self.mods.get(i).copied().unwrap_or(0),
                );
                let w = word.chars().count() as u16;
                render(
                    Paragraph::new(Span::styled(*word, st)),
                    Rect::new(px, preview_y, w, 1),
                    surface,
                );
                px += w + 1;
            }
        }

        let help = if self.naming {
            " type a theme name · ⏎ save as · Esc cancel"
        } else if self.editing {
            " type #rrggbb (empty bg clears) · ⏎ apply · Esc cancel"
        } else {
            " Tab pane · j/k move · f fg/b bg · 1/2/3 bold/italic/dim · n name · s save · Esc"
        };
        render(
            Paragraph::new(Span::styled(help, dim)),
            Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
            surface,
        );
    }
}
