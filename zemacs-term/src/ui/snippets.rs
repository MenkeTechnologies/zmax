//! User snippet library editor: a modal two-pane TUI (slice 1).
//!
//! Left pane lists snippets (`trigger — scope`); the right pane is an editable
//! form (Trigger / Scope / Description / Body). Body is multi-line and is
//! validated against the LSP-snippet engine ([`zemacs_core::snippets::Snippet`]).
//! Backed by [`crate::snippet_store`] (persisted to `<config-dir>/snippets.toml`).
//!
//! Unlike the run-config manager this store is not written on every keystroke:
//! edits accumulate in memory (marking the store dirty) and `Ctrl-s` writes the
//! whole store to disk after validating every body. Closing with unsaved edits
//! arms a discard guard (press `q`/`Esc` again to discard).
//!
//! Keys — list:  j/k move · a/n add · d delete · e/⏎ edit · Ctrl-s save · q/Esc close
//!        edit:  Tab/S-Tab fields · type to edit · ⏎ newline-in-body / save · Esc back
//!
//! Expansion-on-trigger is deferred to a later slice; this slice is the store +
//! editor only.

use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::Rect,
    input::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind},
};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    snippet_store::{self, SnippetStore, UserSnippet},
};

const FIELD_NAMES: [&str; 4] = ["Trigger", "Scope", "Description", "Body"];
const BODY_FIELD: usize = 3;

#[derive(PartialEq)]
enum Mode {
    List,
    Edit,
}

#[derive(Clone, Copy)]
enum Btn {
    Add,
    Delete,
    Edit,
    Save,
    Close,
}

pub struct SnippetPanel {
    data: SnippetStore,
    selected: usize,
    mode: Mode,
    field: usize,
    buf: [String; 4],
    /// In-memory edits not yet written to disk.
    dirty: bool,
    /// A close was requested while dirty; a second close discards.
    quit_armed: bool,
    /// Validation error for the selected snippet's body, if any.
    body_error: Option<String>,
    /// Click targets recorded during render: (x0, x1, row, button).
    btn_hits: Vec<(u16, u16, u16, Btn)>,
    /// Left-list click targets: (row, x0, x1, snippet index).
    list_hits: Vec<(u16, u16, u16, usize)>,
    /// Form-field click targets: (row, x0, x1, field index).
    field_hits: Vec<(u16, u16, u16, usize)>,
}

impl Default for SnippetPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SnippetPanel {
    pub const ID: &'static str = "snippet-panel";

    pub fn new() -> Self {
        let data = snippet_store::load();
        let mut panel = Self {
            selected: 0,
            mode: Mode::List,
            field: 0,
            buf: Default::default(),
            dirty: false,
            quit_armed: false,
            body_error: None,
            btn_hits: Vec::new(),
            list_hits: Vec::new(),
            field_hits: Vec::new(),
            data,
        };
        panel.revalidate();
        panel
    }

    fn close_cb() -> Callback {
        Box::new(|c: &mut Compositor, _| {
            c.pop();
        })
    }

    /// Validate the selected snippet's body, caching the message for the status line.
    fn revalidate(&mut self) {
        self.body_error = match self.data.snippets.get(self.selected) {
            Some(s) => snippet_store::validate_body(&s.body).err(),
            None => None,
        };
    }

    fn load_fields(&mut self) {
        let s = self
            .data
            .snippets
            .get(self.selected)
            .cloned()
            .unwrap_or_default();
        self.buf = [s.trigger, s.scope, s.description, s.body];
    }

    /// Write the edit buffer back into the in-memory store (no disk write).
    fn store_fields(&mut self) {
        if let Some(s) = self.data.snippets.get_mut(self.selected) {
            s.trigger = self.buf[0].clone();
            s.scope = self.buf[1].clone();
            s.description = self.buf[2].clone();
            s.body = self.buf[3].clone();
            self.dirty = true;
        }
        self.revalidate();
    }

    fn add(&mut self) {
        // A fresh snippet defaults its scope to "*" (all languages) and a `$0`
        // body so it parses cleanly before any editing.
        self.data.snippets.push(UserSnippet {
            scope: "*".into(),
            body: "$0".into(),
            ..UserSnippet::default()
        });
        self.selected = self.data.snippets.len() - 1;
        self.dirty = true;
        self.enter_edit();
    }

    fn delete(&mut self) {
        if self.selected < self.data.snippets.len() {
            self.data.snippets.remove(self.selected);
            if self.selected >= self.data.snippets.len() {
                self.selected = self.data.snippets.len().saturating_sub(1);
            }
            self.dirty = true;
            self.revalidate();
        }
    }

    fn enter_edit(&mut self) {
        if self.data.snippets.is_empty() {
            return;
        }
        self.load_fields();
        self.field = 0;
        self.mode = Mode::Edit;
    }

    /// Save the whole store to disk after validating every body. Refuses to save
    /// (and reports the offending trigger) if any body fails to parse.
    fn save(&mut self, cx: &mut Context) {
        if let Some(bad) = self
            .data
            .snippets
            .iter()
            .find(|s| snippet_store::validate_body(&s.body).is_err())
        {
            let name = if bad.trigger.is_empty() {
                "(no trigger)"
            } else {
                bad.trigger.as_str()
            };
            cx.editor
                .set_status(format!("not saved: snippet '{name}' has an invalid body"));
            return;
        }
        snippet_store::save(&self.data);
        self.dirty = false;
        cx.editor
            .set_status(format!("saved {} snippet(s)", self.data.snippets.len()));
    }

    fn do_button(&mut self, b: Btn, cx: &mut Context) -> EventResult {
        match b {
            Btn::Add => self.add(),
            Btn::Delete => self.delete(),
            Btn::Edit => self.enter_edit(),
            Btn::Save => self.save(cx),
            Btn::Close => return self.request_close(cx),
        }
        EventResult::Consumed(None)
    }

    /// Close, arming a discard guard when there are unsaved edits.
    fn request_close(&mut self, cx: &mut Context) -> EventResult {
        if self.dirty && !self.quit_armed {
            self.quit_armed = true;
            cx.editor
                .set_status("unsaved snippet edits — Ctrl-s to save, or close again to discard");
            return EventResult::Consumed(None);
        }
        EventResult::Consumed(Some(Self::close_cb()))
    }

    fn handle_mouse(
        &mut self,
        col: u16,
        row: u16,
        kind: MouseEventKind,
        cx: &mut Context,
    ) -> EventResult {
        if !matches!(kind, MouseEventKind::Down(MouseButton::Left)) {
            return EventResult::Consumed(None);
        }
        // Toolbar buttons.
        if let Some(&(_, _, _, b)) = self
            .btn_hits
            .iter()
            .find(|&&(x0, x1, r, _)| row == r && col >= x0 && col < x1)
        {
            return self.do_button(b, cx);
        }
        // A form field (right pane) opens it for editing — checked before the list
        // so a field click never collides with a same-row snippet row.
        if let Some(&(_, _, _, fi)) = self
            .field_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            if !self.data.snippets.is_empty() {
                if self.mode == Mode::List {
                    self.load_fields();
                }
                self.field = fi;
                self.mode = Mode::Edit;
            }
            return EventResult::Consumed(None);
        }
        // A snippet row in the left list selects it (and leaves edit mode).
        if let Some(&(_, _, _, idx)) = self
            .list_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            self.selected = idx;
            self.mode = Mode::List;
            self.revalidate();
            return EventResult::Consumed(None);
        }
        EventResult::Consumed(None)
    }

    fn handle_list_key(&mut self, key: KeyEvent, cx: &mut Context) -> EventResult {
        let len = self.data.snippets.len();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.request_close(cx),
            KeyCode::Char('j') | KeyCode::Down => {
                if len > 0 {
                    self.selected = (self.selected + 1) % len;
                    self.revalidate();
                }
                EventResult::Consumed(None)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if len > 0 {
                    self.selected = (self.selected + len - 1) % len;
                    self.revalidate();
                }
                EventResult::Consumed(None)
            }
            KeyCode::Char('a') | KeyCode::Char('n') => {
                self.add();
                EventResult::Consumed(None)
            }
            KeyCode::Char('d') => {
                self.delete();
                EventResult::Consumed(None)
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                self.enter_edit();
                EventResult::Consumed(None)
            }
            _ => EventResult::Consumed(None),
        }
    }

    fn handle_edit_key(&mut self, key: KeyEvent) -> EventResult {
        // Shift-Tab arrives as Tab + SHIFT (terminal BackTab is normalized here).
        if key.code == KeyCode::Tab {
            let back = key.modifiers.contains(KeyModifiers::SHIFT);
            self.field = if back {
                (self.field + FIELD_NAMES.len() - 1) % FIELD_NAMES.len()
            } else {
                (self.field + 1) % FIELD_NAMES.len()
            };
            return EventResult::Consumed(None);
        }
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::List;
                EventResult::Consumed(None)
            }
            KeyCode::Enter => {
                if self.field == BODY_FIELD {
                    // Body is multi-line: Enter inserts a newline.
                    self.buf[BODY_FIELD].push('\n');
                    self.store_fields();
                } else {
                    self.mode = Mode::List;
                }
                EventResult::Consumed(None)
            }
            KeyCode::Backspace => {
                self.buf[self.field].pop();
                self.store_fields();
                EventResult::Consumed(None)
            }
            KeyCode::Char(ch) => {
                // Ignore control-chord chars (Ctrl-s is handled in handle_event).
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
                    self.buf[self.field].push(ch);
                    self.store_fields();
                }
                EventResult::Consumed(None)
            }
            _ => EventResult::Consumed(None),
        }
    }
}

impl Component for SnippetPanel {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            Event::Mouse(ev) => return self.handle_mouse(ev.column, ev.row, ev.kind, cx),
            _ => return EventResult::Ignored(None),
        };

        // Quit-arming only survives consecutive close requests: clear it now, and
        // re-arm only in `request_close`.
        let was_armed = self.quit_armed;
        self.quit_armed = false;

        // Ctrl-s saves the whole store from either mode.
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.save(cx);
            return EventResult::Consumed(None);
        }

        // Re-arm the guard so the close branches can see the prior state.
        self.quit_armed = was_armed;
        match self.mode {
            Mode::List => self.handle_list_key(key, cx),
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

        // Shrink a zemacs Rect by one cell on every side (a widget's bordered interior).
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
        let err = to_rat_style(theme.get("error"));
        surface.clear_with(area, theme.get("ui.background"));

        // Flat page header bar (no modal frame).
        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        let dirty_mark = if self.dirty { " ●" } else { "" };
        let header = format!(
            " Snippets ({}){}  —  Ctrl-s save · a add · d delete · e edit · q close ",
            self.data.snippets.len(),
            dirty_mark,
        );
        render(
            Paragraph::new(Span::styled(header, accent)),
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
            (Btn::Delete, "− Delete"),
            (Btn::Edit, "✎ Edit"),
            (Btn::Save, "💾 Save"),
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
                Btn::Save => to_rat_style(theme.get("diff.plus")).add_modifier(RMod::BOLD),
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

        // Left list of snippets (bordered List widget).
        let list_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border)
            .style(bg)
            .title(Span::styled(" Snippets ", dim));
        let list_inner = inset(list_rect);
        let items: Vec<ListItem> = self
            .data
            .snippets
            .iter()
            .map(|s| {
                let trigger = if s.trigger.is_empty() {
                    "(no trigger)"
                } else {
                    s.trigger.as_str()
                };
                let scope = if s.scope.is_empty() {
                    "*"
                } else {
                    s.scope.as_str()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(trigger.to_string(), text),
                    Span::styled(format!(" — {scope}"), dim),
                ]))
            })
            .collect();
        for i in 0..self.data.snippets.len() {
            let row = list_inner.y + i as u16;
            if row < list_inner.y + list_inner.height {
                self.list_hits
                    .push((row, list_inner.x, list_inner.x + list_inner.width, i));
            }
        }
        let mut state = ListState::default();
        if !self.data.snippets.is_empty() {
            state.select(Some(self.selected));
        }
        let list = List::new(items).block(list_block).highlight_style(sel);
        render_stateful(list, list_rect, surface, &mut state);
        if self.data.snippets.is_empty() {
            render(
                Paragraph::new(Span::styled("No snippets — click + Add", dim)),
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
            .title(Span::styled(" Snippet ", dim));
        render(form_block, form_rect, surface);
        let form_inner = inset(form_rect);

        if let Some(s) = self.data.snippets.get(self.selected) {
            let editing = self.mode == Mode::Edit;
            let stored = [&s.trigger, &s.scope, &s.description, &s.body];
            // Single-line fields occupy rows 0,2,4; the Body spans the remaining rows.
            let mut y = form_inner.y;
            let label_w = 14.min(form_inner.width);
            let vx = form_inner.x + label_w;
            let vw = form_inner.width.saturating_sub(label_w).max(1);
            for fi in 0..BODY_FIELD {
                if y + 1 >= form_inner.y + form_inner.height {
                    break;
                }
                let val: String = if editing {
                    self.buf[fi].clone()
                } else {
                    stored[fi].clone()
                };
                let active = editing && fi == self.field;
                render(
                    Paragraph::new(Span::styled(
                        format!("{}:", FIELD_NAMES[fi]),
                        if active { accent } else { dim },
                    )),
                    Rect::new(form_inner.x, y, label_w, 1),
                    surface,
                );
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
                y += 2;
            }

            // --- Body (multi-line) ---
            if y + 1 < form_inner.y + form_inner.height {
                let body_active = editing && self.field == BODY_FIELD;
                render(
                    Paragraph::new(Span::styled(
                        "Body:",
                        if body_active { accent } else { dim },
                    )),
                    Rect::new(form_inner.x, y, label_w, 1),
                    surface,
                );
                self.field_hits.push((
                    y,
                    form_inner.x,
                    form_inner.x + form_inner.width,
                    BODY_FIELD,
                ));
                let body_top = y + 1;
                let body_h = (form_inner.y + form_inner.height).saturating_sub(body_top);
                let body_val: String = if editing {
                    self.buf[BODY_FIELD].clone()
                } else {
                    stored[BODY_FIELD].clone()
                };
                let shown = if body_active {
                    format!("{body_val}▏")
                } else {
                    body_val
                };
                let body_style = if body_active { text.patch(sel) } else { text };
                render(
                    Paragraph::new(Span::styled(shown, body_style)).wrap(Wrap { trim: false }),
                    Rect::new(form_inner.x, body_top, form_inner.width, body_h),
                    surface,
                );
            }
        }

        // --- help / status line ---
        if let Some(msg) = &self.body_error {
            render(
                Paragraph::new(Span::styled(format!(" ⚠ invalid body: {msg}"), err)),
                help_row,
                surface,
            );
        } else {
            let help = match self.mode {
                Mode::List => {
                    " j/k move · a/n add · d delete · e/⏎ edit · Ctrl-s save · q/Esc close"
                }
                Mode::Edit => {
                    " Tab/S-Tab fields · type to edit · ⏎ newline-in-body / save · Esc back"
                }
            };
            render(Paragraph::new(Span::styled(help, dim)), help_row, surface);
        }
    }
}
