//! The GNU Emacs GDB data buffers that have their own keymaps: the Breakpoints
//! buffer, the Threads buffer, and the speedbar's watch-expression list.
//!
//! Each is a modal [`Component`] over zmax's live DAP session — the breakpoints
//! in `Editor::breakpoints`, the adapter's `threads` response, and the watch
//! expressions in [`crate::gud`]. There is no second debugger here; every key
//! drives the same `zmax-dap` client the rest of the debug UI uses.
//!
//! Keys, each on the command the Emacs manual's "Breakpoints Buffer", "Threads
//! Buffer" and "Watch Expressions" nodes name for it:
//!
//! Breakpoints buffer ([`GdbBreakpoints`], `:gdb-breakpoints-buffer`)
//!   `D`       — `gdb-delete-breakpoint`: delete the breakpoint on the line
//!   `RET`     — `gdb-goto-breakpoint`: visit its source line
//!   `mouse-2` — `gdb-goto-breakpoint`, from the middle mouse button
//!   `SPC`     — `gdb-toggle-breakpoint`: enable/disable it (it stays listed)
//!   `j`/`k`/`n`/`p`/arrows move, `g` refresh, `q`/`Esc` quit
//!
//! Threads buffer ([`GdbThreads`], `:gdb-threads-buffer`)
//!   `d` — `gdb-display-disassembly-for-thread`
//!   `f` — `gdb-display-stack-for-thread`
//!   `l` — `gdb-display-locals-for-thread`
//!   `r` — `gdb-display-registers-for-thread`
//!   `RET` — `gdb-select-thread`; `g` refresh, `q`/`Esc` quit
//!
//! Watch expressions ([`GdbWatch`], `:gdb-watch-buffer`)
//!   `D`   — `gdb-var-delete`: stop watching the expression on the line
//!   `RET` — `gdb-edit-value`: read a new value and assign it
//!   `g` refresh, `q`/`Esc` quit
//!
//! `d`/`f`/`l`/`r` act on the thread the cursor is on, as Emacs does: the thread
//! is selected first (DAP `select-thread`), then the data buffer is shown.

use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zmax_view::editor::Action;
use zmax_view::graphics::Rect;
use zmax_view::input::{MouseButton, MouseEventKind};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    key,
};

/// Run a `commands::Context` command from inside a component and hand back the
/// compositor callbacks it queued (a picker/popup/prompt it wanted to push).
fn run_command(
    cx: &mut Context,
    f: impl FnOnce(&mut crate::commands::Context),
) -> Option<Callback> {
    let mut ccx = crate::commands::Context {
        register: None,
        count: None,
        editor: cx.editor,
        callback: Vec::new(),
        on_next_key_callback: None,
        jobs: cx.jobs,
    };
    f(&mut ccx);
    let queued = ccx.callback;
    if queued.is_empty() {
        return None;
    }
    Some(Box::new(
        move |compositor: &mut Compositor, cx: &mut Context| {
            for cb in queued {
                cb(compositor, cx);
            }
        },
    ))
}

/// Pop this overlay.
fn close() -> Callback {
    Box::new(|compositor: &mut Compositor, _cx| {
        compositor.pop();
    })
}

/// Move `cursor` by `delta`, clamped to `0..len`.
fn move_cursor(cursor: &mut usize, len: usize, delta: isize) {
    if len == 0 {
        return;
    }
    *cursor = (*cursor as isize + delta).clamp(0, len as isize - 1) as usize;
}

/// Keep `scroll` such that `cursor` is inside a `viewport`-row window.
fn scroll_into_view(scroll: &mut usize, cursor: usize, viewport: usize) {
    if cursor < *scroll {
        *scroll = cursor;
    } else if viewport > 0 && cursor >= *scroll + viewport {
        *scroll = cursor + 1 - viewport;
    }
}

/// Draw a title row and, right-aligned on it, the key hint.
fn draw_header(surface: &mut Surface, area: Rect, title: &str, hint: &str, ctx: &Context) {
    let theme = &ctx.editor.theme;
    surface.set_stringn(
        area.x,
        area.y,
        title,
        area.width as usize,
        theme.get("ui.text.focus"),
    );
    if title.len() + hint.len() + 3 < area.width as usize {
        surface.set_stringn(
            area.x + area.width - hint.len() as u16 - 1,
            area.y,
            hint,
            hint.len(),
            theme.get("ui.linenr"),
        );
    }
}

/// Clear the overlay's area, honouring `transparent-background`.
fn clear_body(surface: &mut Surface, area: Rect, ctx: &Context) {
    let mut bg = ctx.editor.theme.get("ui.background");
    if ctx.editor.config().transparent_background {
        bg.bg = None;
    }
    surface.clear_with(area, bg);
}

// ── Breakpoints buffer ──────────────────────────────────────────────────────

/// One row of the Breakpoints buffer.
struct BpRow {
    path: PathBuf,
    /// 0-based buffer line, as `Editor::breakpoints` stores it.
    line: usize,
    /// False for a breakpoint parked by `SPC` (gdb's `disable`).
    enabled: bool,
    /// Whether the adapter accepted (bound) the breakpoint.
    verified: bool,
    condition: Option<String>,
}

/// The Emacs GDB Breakpoints buffer.
pub struct GdbBreakpoints {
    rows: Vec<BpRow>,
    cursor: usize,
    scroll: usize,
    viewport: usize,
    /// Screen row the first list entry was drawn at, so a click maps to a row.
    body_y: u16,
    status: String,
}

impl GdbBreakpoints {
    /// Collect every breakpoint — the live ones from the editor plus the ones
    /// `SPC` has disabled — into a stable, sorted list.
    pub fn new(editor: &zmax_view::Editor) -> Self {
        let mut me = GdbBreakpoints {
            rows: Vec::new(),
            cursor: 0,
            scroll: 0,
            viewport: 1,
            body_y: 0,
            status: String::new(),
        };
        me.refresh(editor);
        me
    }

    fn refresh(&mut self, editor: &zmax_view::Editor) {
        let mut rows: Vec<BpRow> = Vec::new();
        for (path, list) in &editor.breakpoints {
            for bp in list {
                rows.push(BpRow {
                    path: path.clone(),
                    line: bp.line,
                    enabled: true,
                    verified: bp.verified,
                    condition: bp.condition.clone(),
                });
            }
        }
        for (path, line, condition) in crate::gud::disabled_breakpoints() {
            rows.push(BpRow {
                path,
                line,
                enabled: false,
                verified: false,
                condition,
            });
        }
        rows.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        self.rows = rows;
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    /// `NUM  Enb  file:line  [if COND]`, the columns gdb's `info breakpoints`
    /// (and so the Emacs buffer) shows.
    fn row_text(&self, index: usize, row: &BpRow) -> String {
        let name = row
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| row.path.to_string_lossy().into_owned());
        let enb = if row.enabled { 'y' } else { 'n' };
        let state = if !row.enabled {
            " (disabled)"
        } else if row.verified {
            ""
        } else {
            " (pending)"
        };
        match &row.condition {
            Some(c) => format!(
                "{:>3}  {}  {}:{}{}  if {}",
                index + 1,
                enb,
                name,
                row.line + 1,
                state,
                c
            ),
            None => format!(
                "{:>3}  {}  {}:{}{}",
                index + 1,
                enb,
                name,
                row.line + 1,
                state
            ),
        }
    }

    /// `gdb-goto-breakpoint` (`RET`, `mouse-2`): close the buffer and put point
    /// on the breakpoint's source line.
    fn goto(&self) -> Option<Callback> {
        let row = self.rows.get(self.cursor)?;
        let (path, line) = (row.path.clone(), row.line);
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                if cx.editor.open(&path, Action::Replace).is_err() {
                    cx.editor.set_error(format!(
                        "gdb-goto-breakpoint: cannot open {}",
                        path.display()
                    ));
                    return;
                }
                let (view, doc) = current!(cx.editor);
                let text = doc.text();
                let line = line.min(text.len_lines().saturating_sub(1));
                let pos = text.line_to_char(line);
                doc.set_selection(view.id, zmax_core::Selection::point(pos));
            },
        ))
    }

    /// `gdb-delete-breakpoint` (`D`): remove the breakpoint entirely and push the
    /// new set to the adapter.
    fn delete(&mut self, cx: &mut Context) {
        let Some(row) = self.rows.get(self.cursor) else {
            return;
        };
        let (path, line, enabled) = (row.path.clone(), row.line, row.enabled);
        if enabled {
            crate::gud::delete_breakpoint(cx.editor, &path, line);
        } else {
            crate::gud::forget_disabled(&path, line);
        }
        self.status = format!("deleted breakpoint at {}:{}", path.display(), line + 1);
        self.refresh(cx.editor);
    }

    /// `gdb-toggle-breakpoint` (`SPC`): enable/disable without deleting. A
    /// disabled breakpoint is withdrawn from the adapter but stays listed here,
    /// exactly as gdb's `disable` leaves it in `info breakpoints`.
    fn toggle_enabled(&mut self, cx: &mut Context) {
        let Some(row) = self.rows.get(self.cursor) else {
            return;
        };
        let (path, line, enabled) = (row.path.clone(), row.line, row.enabled);
        if enabled {
            crate::gud::disable_breakpoint(cx.editor, &path, line);
            self.status = format!("disabled breakpoint at {}:{}", path.display(), line + 1);
        } else {
            crate::gud::enable_breakpoint(cx.editor, &path, line);
            self.status = format!("enabled breakpoint at {}:{}", path.display(), line + 1);
        }
        self.refresh(cx.editor);
    }
}

impl Component for GdbBreakpoints {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        match event {
            Event::Mouse(me) => {
                // `mouse-2` (the middle button) is Emacs's other gdb-goto-breakpoint.
                if me.kind == MouseEventKind::Down(MouseButton::Middle) && me.row >= self.body_y {
                    let idx = (me.row - self.body_y) as usize + self.scroll;
                    if idx < self.rows.len() {
                        self.cursor = idx;
                        if let Some(cb) = self.goto() {
                            return EventResult::Consumed(Some(cb));
                        }
                    }
                }
                return EventResult::Consumed(None);
            }
            Event::Key(_) => {}
            _ => return EventResult::Ignored(None),
        }
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        let key = *key;
        self.status.clear();
        match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close())),
            key!('j') | key!('n') | key!(Down) => move_cursor(&mut self.cursor, self.rows.len(), 1),
            key!('k') | key!('p') | key!(Up) => move_cursor(&mut self.cursor, self.rows.len(), -1),
            key!('g') => self.refresh(cx.editor),
            key!('D') => self.delete(cx),
            key!(' ') => self.toggle_enabled(cx),
            key!(Enter) => {
                if let Some(cb) = self.goto() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        clear_body(surface, area, ctx);
        if area.width < 16 || area.height < 3 {
            return;
        }
        let title = format!(" *breakpoints of gdb*  {} breakpoints", self.rows.len());
        draw_header(
            surface,
            area,
            &title,
            "D delete  SPC enable/disable  RET goto  g refresh  q quit",
            ctx,
        );

        let theme = &ctx.editor.theme;
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");

        self.body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h as usize;
        if self.rows.is_empty() {
            surface.set_stringn(
                area.x,
                self.body_y,
                "(no breakpoints — set one with C-x C-a C-b)",
                area.width as usize,
                info_style,
            );
            return;
        }
        scroll_into_view(&mut self.scroll, self.cursor, self.viewport);
        for (offset, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = self.body_y + (offset - self.scroll) as u16;
            let style = if offset == self.cursor {
                sel_style
            } else if row.enabled {
                text_style
            } else {
                info_style
            };
            surface.set_stringn(
                area.x,
                y,
                &self.row_text(offset, row),
                area.width as usize,
                style,
            );
        }
        let footer = if self.status.is_empty() {
            format!("{}/{}", self.cursor + 1, self.rows.len())
        } else {
            self.status.clone()
        };
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &footer,
            area.width as usize,
            info_style,
        );
    }

    fn id(&self) -> Option<&'static str> {
        Some("gdb-breakpoints")
    }
}

// ── Threads buffer ──────────────────────────────────────────────────────────

/// One row of the Threads buffer.
struct ThreadRow {
    id: zmax_dap::ThreadId,
    name: String,
    state: String,
}

/// The Emacs GDB Threads buffer.
pub struct GdbThreads {
    rows: Vec<ThreadRow>,
    current: Option<zmax_dap::ThreadId>,
    cursor: usize,
    scroll: usize,
    viewport: usize,
    status: String,
}

impl GdbThreads {
    /// Fetch the adapter's thread list (DAP `threads`) and its states.
    pub fn new(editor: &zmax_view::Editor) -> Self {
        let mut me = GdbThreads {
            rows: Vec::new(),
            current: None,
            cursor: 0,
            scroll: 0,
            viewport: 1,
            status: String::new(),
        };
        me.refresh(editor);
        me
    }

    fn refresh(&mut self, editor: &zmax_view::Editor) {
        let Some(debugger) = editor.debug_adapters.get_active_client() else {
            self.rows.clear();
            self.status = "no debug session — :dap-launch".to_string();
            return;
        };
        self.current = debugger.thread_id;
        let threads: Vec<zmax_dap::Thread> = zmax_lsp::block_on(debugger.threads())
            .ok()
            .and_then(|v| serde_json::from_value::<zmax_dap::requests::ThreadsResponse>(v).ok())
            .map(|r| r.threads)
            .unwrap_or_default();
        self.rows = threads
            .into_iter()
            .map(|t| ThreadRow {
                state: debugger
                    .thread_states
                    .get(&t.id)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string()),
                id: t.id,
                name: t.name,
            })
            .collect();
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    /// Make the thread under the cursor current, then run one of the per-thread
    /// data-buffer commands on it — what Emacs's `d`/`f`/`l`/`r` do.
    fn show_for_thread(
        &mut self,
        cx: &mut Context,
        what: &'static str,
        show: fn(&mut crate::commands::Context),
    ) -> Option<Callback> {
        let id = self.rows.get(self.cursor)?.id;
        zmax_lsp::block_on(zmax_view::handlers::dap::select_thread_id(
            cx.editor, id, true,
        ));
        self.current = Some(id);
        self.status = format!("{what} for thread {id}");
        run_command(cx, show)
    }
}

impl Component for GdbThreads {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        let key = *key;
        self.status.clear();
        match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close())),
            key!('j') | key!('n') | key!(Down) => move_cursor(&mut self.cursor, self.rows.len(), 1),
            key!('k') | key!('p') | key!(Up) => move_cursor(&mut self.cursor, self.rows.len(), -1),
            key!('g') => self.refresh(cx.editor),
            // The four per-thread data buffers of the Emacs Threads buffer.
            key!('d') => {
                let cb = self.show_for_thread(
                    cx,
                    "disassembly",
                    crate::commands::dap::gdb_display_disassembly_for_thread,
                );
                if let Some(cb) = cb {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('f') => {
                let cb = self.show_for_thread(
                    cx,
                    "stack",
                    crate::commands::dap::gdb_display_stack_for_thread,
                );
                if let Some(cb) = cb {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('l') => {
                let cb = self.show_for_thread(
                    cx,
                    "locals",
                    crate::commands::dap::gdb_display_locals_for_thread,
                );
                if let Some(cb) = cb {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('r') => {
                let cb = self.show_for_thread(
                    cx,
                    "registers",
                    crate::commands::dap::gdb_display_registers_for_thread,
                );
                if let Some(cb) = cb {
                    return EventResult::Consumed(Some(cb));
                }
            }
            // `gdb-select-thread`: make it current without opening a data buffer.
            key!(Enter) => {
                if let Some(row) = self.rows.get(self.cursor) {
                    let id = row.id;
                    zmax_lsp::block_on(zmax_view::handlers::dap::select_thread_id(
                        cx.editor, id, true,
                    ));
                    self.current = Some(id);
                    self.status = format!("selected thread {id}");
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        clear_body(surface, area, ctx);
        if area.width < 16 || area.height < 3 {
            return;
        }
        let title = format!(" *threads of gdb*  {} threads", self.rows.len());
        draw_header(
            surface,
            area,
            &title,
            "d disassembly  f stack  l locals  r registers  RET select  q quit",
            ctx,
        );

        let theme = &ctx.editor.theme;
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h as usize;
        if self.rows.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "(no threads)",
                area.width as usize,
                info_style,
            );
            return;
        }
        scroll_into_view(&mut self.scroll, self.cursor, self.viewport);
        for (offset, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let style = if offset == self.cursor {
                sel_style
            } else {
                text_style
            };
            let marker = if self.current == Some(row.id) {
                '*'
            } else {
                ' '
            };
            let line = format!("{marker} {:>4}  {}  ({})", row.id, row.name, row.state);
            surface.set_stringn(area.x, y, &line, area.width as usize, style);
        }
        let footer = if self.status.is_empty() {
            format!("{}/{}", self.cursor + 1, self.rows.len())
        } else {
            self.status.clone()
        };
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &footer,
            area.width as usize,
            info_style,
        );
    }

    fn id(&self) -> Option<&'static str> {
        Some("gdb-threads")
    }
}

// ── Watch expressions (the speedbar list) ───────────────────────────────────

/// The Emacs GDB speedbar's watch-expression list.
pub struct GdbWatch {
    /// `(expression, value)`; the value is re-read from the adapter on refresh.
    rows: Vec<(String, String)>,
    cursor: usize,
    scroll: usize,
    viewport: usize,
    status: String,
}

impl GdbWatch {
    /// Build the list from [`crate::gud::watch_list`], evaluating each
    /// expression in the selected frame.
    pub fn new(editor: &zmax_view::Editor) -> Self {
        let mut me = GdbWatch {
            rows: Vec::new(),
            cursor: 0,
            scroll: 0,
            viewport: 1,
            status: String::new(),
        };
        me.refresh(editor);
        me
    }

    fn refresh(&mut self, editor: &zmax_view::Editor) {
        let exprs = crate::gud::watch_list();
        let frame_id = crate::gud::selected_frame_id(editor);
        let debugger = editor.debug_adapters.get_active_client();
        self.rows = exprs
            .into_iter()
            .map(|expr| {
                let value = match debugger {
                    Some(d) => match zmax_lsp::block_on(d.eval(expr.clone(), frame_id)) {
                        Ok(r) => r.result,
                        Err(e) => format!("<{e}>"),
                    },
                    None => "<no debug session>".to_string(),
                };
                (expr, value)
            })
            .collect();
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    /// `gdb-edit-value` (`RET`): read a new value for the watched expression and
    /// assign it through DAP `setExpression` (or an assignment `evaluate`).
    fn edit_value(&self, editor: &zmax_view::Editor) -> Option<Callback> {
        let expr = self.rows.get(self.cursor)?.0.clone();
        let frame_id = crate::gud::selected_frame_id(editor);
        Some(Box::new(
            move |compositor: &mut Compositor, _cx: &mut Context| {
                let expr = expr.clone();
                let prompt = crate::ui::Prompt::new(
                    format!("gdb-edit-value — {expr} = ").into(),
                    None,
                    |_editor: &zmax_view::Editor, _input: &str| Vec::new(),
                    move |cx, input, event| {
                        if event != crate::ui::PromptEvent::Validate {
                            return;
                        }
                        let value = input.trim().to_string();
                        if value.is_empty() {
                            return;
                        }
                        match crate::gud::assign_value(cx.editor, &expr, &value, frame_id) {
                            Ok(v) => cx.editor.set_status(format!("{expr} = {v}")),
                            Err(e) => cx.editor.set_error(format!("gdb-edit-value: {e}")),
                        }
                    },
                );
                compositor.push(Box::new(prompt));
            },
        ))
    }
}

impl Component for GdbWatch {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        let key = *key;
        self.status.clear();
        match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close())),
            key!('j') | key!('n') | key!(Down) => move_cursor(&mut self.cursor, self.rows.len(), 1),
            key!('k') | key!('p') | key!(Up) => move_cursor(&mut self.cursor, self.rows.len(), -1),
            key!('g') => self.refresh(cx.editor),
            // `gdb-var-delete`: stop watching the expression on this line.
            key!('D') => {
                match crate::gud::watch_remove_at(self.cursor) {
                    Some(expr) => self.status = format!("gdb-var-delete: dropped {expr}"),
                    None => self.status = "gdb-var-delete: no expression here".to_string(),
                }
                self.refresh(cx.editor);
            }
            key!(Enter) => {
                if let Some(cb) = self.edit_value(cx.editor) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        clear_body(surface, area, ctx);
        if area.width < 16 || area.height < 3 {
            return;
        }
        let title = format!(" *watch expressions*  {}", self.rows.len());
        draw_header(
            surface,
            area,
            &title,
            "D delete  RET edit value  g refresh  q quit",
            ctx,
        );

        let theme = &ctx.editor.theme;
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h as usize;
        if self.rows.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "(no watch expressions — add one with :gdb-watch EXPR)",
                area.width as usize,
                info_style,
            );
            return;
        }
        scroll_into_view(&mut self.scroll, self.cursor, self.viewport);
        for (offset, (expr, value)) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let style = if offset == self.cursor {
                sel_style
            } else {
                text_style
            };
            surface.set_stringn(
                area.x,
                y,
                &format!("  {expr} = {value}"),
                area.width as usize,
                style,
            );
        }
        let footer = if self.status.is_empty() {
            format!("{}/{}", self.cursor + 1, self.rows.len())
        } else {
            self.status.clone()
        };
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &footer,
            area.width as usize,
            info_style,
        );
    }

    fn id(&self) -> Option<&'static str> {
        Some("gdb-watch")
    }
}
