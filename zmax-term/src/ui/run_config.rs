//! JetBrains-style Run/Debug Configurations manager: a modal two-pane TUI.
//!
//! Left pane lists named configurations; the right pane is an editable form
//! (Name / Command / Dir / Env). Full CRUD plus "run the selected config".
//! Backed by [`crate::run_config`] (persisted to `<workspace>/.zmax/run-configs.toml`).
//!
//! Keys — list:  j/k move · a add · c copy · d delete · e/⏎ edit · r run · Esc close
//!        edit:  Tab/↓ next field · ↑ prev · ⏎ save · Esc cancel · type to edit

use tui::buffer::Buffer as Surface;
use zmax_view::{
    graphics::Rect,
    input::{KeyCode, KeyEvent, MouseButton, MouseEventKind},
};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    run_config::{self, RunConfig, RunConfigs},
};

const FIELD_NAMES: [&str; 4] = ["Name", "Command", "Dir", "Env"];

#[derive(PartialEq)]
enum Mode {
    List,
    Edit,
}

#[derive(Clone, Copy)]
enum Btn {
    Add,
    Copy,
    Delete,
    Edit,
    Choose,
    Run,
    Close,
}

pub struct RunConfigPanel {
    data: RunConfigs,
    selected: usize,
    mode: Mode,
    field: usize,
    buf: [String; 4],
    /// Click targets recorded during render: (x0, x1, row, button).
    btn_hits: Vec<(u16, u16, u16, Btn)>,
    /// Left-list click targets: (row, x0, x1, config index).
    list_hits: Vec<(u16, u16, u16, usize)>,
    /// Form-field click targets: (row, x0, x1, field index).
    field_hits: Vec<(u16, u16, u16, usize)>,
}

impl Default for RunConfigPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl RunConfigPanel {
    pub const ID: &'static str = "run-config-panel";

    pub fn new() -> Self {
        let data = run_config::load();
        let selected = data.active.min(data.configs.len().saturating_sub(1));
        Self {
            data,
            selected,
            mode: Mode::List,
            field: 0,
            buf: Default::default(),
            btn_hits: Vec::new(),
            list_hits: Vec::new(),
            field_hits: Vec::new(),
        }
    }

    fn close_cb() -> Callback {
        Box::new(|c: &mut Compositor, _| {
            c.pop();
        })
    }

    fn do_button(&mut self, b: Btn) -> EventResult {
        match b {
            Btn::Add => self.add(),
            Btn::Copy => self.duplicate(),
            Btn::Delete => self.delete(),
            Btn::Edit => self.enter_edit(),
            Btn::Choose => self.apply_selected(),
            Btn::Close => return EventResult::Consumed(Some(Self::close_cb())),
            Btn::Run => {
                if let Some(cb) = self.run_selected() {
                    return EventResult::Consumed(Some(cb));
                }
            }
        }
        EventResult::Consumed(None)
    }

    fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) -> EventResult {
        if !matches!(kind, MouseEventKind::Down(MouseButton::Left)) {
            return EventResult::Consumed(None);
        }
        // Toolbar buttons.
        if let Some(&(_, _, _, b)) = self
            .btn_hits
            .iter()
            .find(|&&(x0, x1, r, _)| row == r && col >= x0 && col < x1)
        {
            return self.do_button(b);
        }
        // A form field (right pane) opens it for editing — checked before the list so
        // a field click never collides with a same-row config row in the left pane.
        if let Some(&(_, _, _, fi)) = self
            .field_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            if !self.data.configs.is_empty() {
                if self.mode == Mode::List {
                    self.load_fields();
                }
                self.field = fi;
                self.mode = Mode::Edit;
            }
            return EventResult::Consumed(None);
        }
        // A config row in the left list selects it (and leaves edit mode).
        if let Some(&(_, _, _, idx)) = self
            .list_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            self.selected = idx;
            self.mode = Mode::List;
            return EventResult::Consumed(None);
        }
        EventResult::Consumed(None)
    }

    fn persist(&self) {
        run_config::save(&self.data);
    }

    fn load_fields(&mut self) {
        let c = self
            .data
            .configs
            .get(self.selected)
            .cloned()
            .unwrap_or_default();
        self.buf = [c.name, c.command, c.dir, c.env];
    }

    fn store_fields(&mut self) {
        if let Some(c) = self.data.configs.get_mut(self.selected) {
            c.name = self.buf[0].clone();
            c.command = self.buf[1].clone();
            c.dir = self.buf[2].clone();
            c.env = self.buf[3].clone();
        }
        self.persist();
    }

    fn add(&mut self) {
        // Empty name so the Name field is ready to type into (fields edit at the end).
        self.data.configs.push(RunConfig::default());
        self.selected = self.data.configs.len() - 1;
        self.persist();
        self.enter_edit();
    }

    fn duplicate(&mut self) {
        if let Some(c) = self.data.configs.get(self.selected).cloned() {
            let copy = RunConfig {
                name: format!("{} copy", c.name),
                ..c
            };
            self.data.configs.insert(self.selected + 1, copy);
            self.selected += 1;
            self.persist();
        }
    }

    fn delete(&mut self) {
        if self.selected < self.data.configs.len() {
            self.data.configs.remove(self.selected);
            if self.selected >= self.data.configs.len() {
                self.selected = self.data.configs.len().saturating_sub(1);
            }
            if self.data.active >= self.data.configs.len() {
                self.data.active = self.data.configs.len().saturating_sub(1);
            }
            self.persist();
        }
    }

    fn enter_edit(&mut self) {
        if self.data.configs.is_empty() {
            return;
        }
        self.load_fields();
        self.field = 0;
        self.mode = Mode::Edit;
    }

    /// Build the run callback for the selected config (sets it active, closes, runs).
    /// Make the selected config the active one (persisted) WITHOUT running it —
    /// JetBrains "choose a run configuration". Shown in the toolbar selector.
    fn apply_selected(&mut self) {
        if self.selected < self.data.configs.len() {
            self.data.active = self.selected;
            self.persist();
        }
    }

    fn run_selected(&mut self) -> Option<Callback> {
        let c = self.data.configs.get(self.selected)?.clone();
        if c.command.trim().is_empty() {
            return None;
        }
        self.data.active = self.selected;
        self.persist();
        let env_prefix: String = c
            .env
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && l.contains('='))
            .map(|l| format!("{l} "))
            .collect();
        let cmd = format!("{env_prefix}{}", c.command);
        let cwd = run_config::resolve_dir(&c.dir);
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                    view.start_run(cx, cmd, cwd);
                }
            },
        ))
    }

    fn handle_list_key(&mut self, key: KeyEvent) -> EventResult {
        let len = self.data.configs.len();
        match key.code {
            KeyCode::Esc => EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                c.pop();
            }))),
            KeyCode::Char('j') | KeyCode::Down => {
                if len > 0 {
                    self.selected = (self.selected + 1) % len;
                }
                EventResult::Consumed(None)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if len > 0 {
                    self.selected = (self.selected + len - 1) % len;
                }
                EventResult::Consumed(None)
            }
            KeyCode::Char('a') => {
                self.add();
                EventResult::Consumed(None)
            }
            KeyCode::Char('c') => {
                self.duplicate();
                EventResult::Consumed(None)
            }
            KeyCode::Char('d') => {
                self.delete();
                EventResult::Consumed(None)
            }
            KeyCode::Char('e') => {
                self.enter_edit();
                EventResult::Consumed(None)
            }
            // Space / 's' → choose (set active) without running.
            KeyCode::Char(' ') | KeyCode::Char('s') => {
                self.apply_selected();
                EventResult::Consumed(None)
            }
            KeyCode::Char('r') => match self.run_selected() {
                Some(cb) => EventResult::Consumed(Some(cb)),
                None => EventResult::Consumed(None),
            },
            KeyCode::Enter => match self.run_selected() {
                Some(cb) => EventResult::Consumed(Some(cb)),
                None => {
                    self.enter_edit();
                    EventResult::Consumed(None)
                }
            },
            _ => EventResult::Consumed(None),
        }
    }

    fn handle_edit_key(&mut self, key: KeyEvent) -> EventResult {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::List;
                EventResult::Consumed(None)
            }
            KeyCode::Enter => {
                self.store_fields();
                self.mode = Mode::List;
                EventResult::Consumed(None)
            }
            KeyCode::Tab | KeyCode::Down => {
                self.field = (self.field + 1) % FIELD_NAMES.len();
                EventResult::Consumed(None)
            }
            KeyCode::Up => {
                self.field = (self.field + FIELD_NAMES.len() - 1) % FIELD_NAMES.len();
                EventResult::Consumed(None)
            }
            KeyCode::Backspace => {
                self.buf[self.field].pop();
                self.store_fields();
                EventResult::Consumed(None)
            }
            KeyCode::Char(ch) => {
                self.buf[self.field].push(ch);
                self.store_fields();
                EventResult::Consumed(None)
            }
            _ => EventResult::Consumed(None),
        }
    }
}

impl Component for RunConfigPanel {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            Event::Mouse(ev) => return self.handle_mouse(ev.column, ev.row, ev.kind),
            _ => return EventResult::Ignored(None),
        };
        match self.mode {
            Mode::List => self.handle_list_key(key),
            Mode::Edit => self.handle_edit_key(key),
        }
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, render_stateful, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{
            Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap,
        };

        // Shrink a zmax Rect by one cell on every side (a widget's bordered interior).
        let inset = |r: Rect| {
            Rect::new(
                r.x + 1,
                r.y + 1,
                r.width.saturating_sub(2),
                r.height.saturating_sub(2),
            )
        };

        self.btn_hits.clear();
        self.list_hits.clear();
        self.field_hits.clear();

        let theme = &ctx.editor.theme;
        let bg = to_rat_style(theme.get("ui.background"));
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let border = to_rat_style(theme.get("ui.window"));
        let sel = to_rat_style(theme.get("ui.selection")).add_modifier(RMod::BOLD);
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let key = to_rat_style(theme.get("keyword"));
        // `transparent-background`: drop the page fill so the terminal shows through.
        let mut page_bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            page_bg.bg = None;
        }
        surface.clear_with(area, page_bg);

        // Flat page header bar (no modal frame).
        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        render(
            Paragraph::new(Span::styled(" Run/Debug Configurations ", accent)),
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
        if inner.width < 4 || inner.height < 4 {
            return;
        }

        // inner rows: [ button bar (1) | body | help (1) ]
        let btn_row = Rect::new(inner.x, inner.y, inner.width, 1);
        let help_row = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
        let body = Rect::new(
            inner.x,
            inner.y + 2,
            inner.width,
            inner.height.saturating_sub(3),
        );

        // --- toolbar buttons (clickable) ---
        let buttons = [
            (Btn::Add, "+ Add"),
            (Btn::Copy, "⧉ Copy"),
            (Btn::Delete, "− Delete"),
            (Btn::Edit, "✎ Edit"),
            (Btn::Choose, "✓ Choose"),
            (Btn::Run, "▶ Run"),
            (Btn::Close, "✕ Close"),
        ];
        let mut bx = btn_row.x;
        for (b, label) in buttons {
            let txt = format!(" {label} ");
            let w = txt.chars().count() as u16;
            if bx + w > btn_row.x + btn_row.width {
                break;
            }
            let style = match b {
                Btn::Run => to_rat_style(theme.get("diff.plus")).add_modifier(RMod::BOLD),
                Btn::Delete => to_rat_style(theme.get("error")),
                _ => text,
            };
            render(
                Paragraph::new(Line::from(Span::styled(
                    txt,
                    style.add_modifier(RMod::REVERSED),
                ))),
                Rect::new(bx, btn_row.y, w, 1),
                surface,
            );
            self.btn_hits.push((bx, bx + w, btn_row.y, b));
            bx += w + 1;
        }

        // --- body: [ list | form ] ---
        let list_w = 30.min(body.width / 2).max(12);
        let list_rect = Rect::new(body.x, body.y, list_w, body.height);
        let form_rect = Rect::new(
            body.x + list_w,
            body.y,
            body.width.saturating_sub(list_w),
            body.height,
        );

        // Left list of configs (bordered List widget).
        let list_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border)
            .style(bg)
            .title(Span::styled(" Configurations ", dim));
        let list_inner = inset(list_rect);
        let items: Vec<ListItem> = self
            .data
            .configs
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let marker = if i == self.data.active { "▶ " } else { "  " };
                let name = if c.name.is_empty() {
                    "(unnamed)"
                } else {
                    c.name.as_str()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(marker, key),
                    Span::styled(name.to_string(), text),
                ]))
            })
            .collect();
        for i in 0..self.data.configs.len() {
            let row = list_inner.y + i as u16;
            if row < list_inner.y + list_inner.height {
                self.list_hits
                    .push((row, list_inner.x, list_inner.x + list_inner.width, i));
            }
        }
        let mut state = ListState::default();
        if !self.data.configs.is_empty() {
            state.select(Some(self.selected));
        }
        let list = List::new(items).block(list_block).highlight_style(sel);
        render_stateful(list, list_rect, surface, &mut state);
        if self.data.configs.is_empty() {
            render(
                Paragraph::new(Span::styled("No configs — click + Add", dim)),
                Rect::new(list_inner.x, list_inner.y, list_inner.width, 1),
                surface,
            );
        }

        // Right form.
        let form_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border)
            .style(bg)
            .title(Span::styled(" Configuration ", dim));
        render(form_block, form_rect, surface);
        let form_inner = inset(form_rect);

        if let Some(c) = self.data.configs.get(self.selected) {
            let stored = [&c.name, &c.command, &c.dir, &c.env];
            for (fi, fname) in FIELD_NAMES.iter().enumerate() {
                let y = form_inner.y + fi as u16 * 2;
                if y + 1 >= form_inner.y + form_inner.height {
                    break;
                }
                let editing = self.mode == Mode::Edit;
                let val: String = if editing {
                    self.buf[fi].clone()
                } else {
                    stored[fi].clone()
                };
                let active = editing && fi == self.field;
                // label
                render(
                    Paragraph::new(Span::styled(
                        format!("{fname}:"),
                        if active { accent } else { dim },
                    )),
                    Rect::new(form_inner.x, y, 12.min(form_inner.width), 1),
                    surface,
                );
                // value input row (underline + highlight when active, caret while editing)
                let vx = form_inner.x + 12;
                let vw = form_inner.width.saturating_sub(12).max(1);
                let val_style = if active {
                    text.add_modifier(RMod::UNDERLINED).patch(sel)
                } else {
                    text.add_modifier(RMod::UNDERLINED)
                };
                let shown = if active { format!("{val}▏") } else { val };
                render(
                    Paragraph::new(Span::styled(shown, val_style)).wrap(Wrap { trim: false }),
                    Rect::new(vx, y, vw, 1),
                    surface,
                );
                self.field_hits
                    .push((y, form_inner.x, form_inner.x + form_inner.width, fi));
            }
        }

        // --- help line ---
        let help = match self.mode {
            Mode::List => " click a config · j/k move · Space choose · r run · a/c/d/e · Esc close",
            Mode::Edit => " click a field · Tab/↑↓ fields · type to edit · ⏎ save · Esc cancel",
        };
        render(Paragraph::new(Span::styled(help, dim)), help_row, surface);
    }
}
