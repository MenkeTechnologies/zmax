//! Integrated terminal: a real PTY-backed shell rendered inside zmax.
//!
//! A [`Component`] (confined to the focused editor pane) that spawns the user's `$SHELL` in a pseudo-
//! terminal ([`portable_pty`]), feeds its output through a [`vt100`] parser that
//! maintains a screen grid, and blits that grid onto the zmax `Surface` each
//! frame. Keystrokes are translated to terminal byte sequences and written back
//! to the PTY, so interactive programs (REPLs, `vim`, `htop`, prompts) work.
//!
//! A background reader thread drives redraws via `zmax_event::request_redraw`
//! as output arrives. **F12** detaches (closing the panel, leaving the shell to
//! be killed on drop); exiting the shell (`exit` / `C-d`) also closes it.
//!
//! Open: `:terminal` / the `terminal` command.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tui::buffer::Buffer as Surface;
use zmax_view::{
    graphics::{Color, CursorKind, Modifier, Rect, Style},
    input::{KeyCode, KeyEvent, KeyModifiers, MouseEventKind},
    ViewId,
};

use crate::compositor::{Component, Compositor, Context, Event, EventResult};

pub struct TerminalPanel {
    parser: Arc<Mutex<vt100::Parser>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    /// Set by the reader thread when the PTY hits EOF (the shell exited).
    dead: Arc<AtomicBool>,
    rows: u16,
    cols: u16,
    caret: Option<zmax_core::Position>,
    /// The editor pane this terminal lives in, captured on first render. The
    /// terminal stays pinned here (it doesn't follow `tree.focus`, which would
    /// make it appear to jump panes), and only takes keystrokes while this pane
    /// is the focused one — so clicking another split moves focus to the editor
    /// and clicking back returns focus to the terminal.
    pane: Option<ViewId>,
    /// True after the `C-\` window leader, so the next key is interpreted as a
    /// window command (split / move focus) instead of being sent to the shell.
    /// `C-\` is used because `C-w` is the shell's delete-previous-word.
    pending_window: bool,
    /// Emacs `term-line-mode`: keys are edited locally into [`Self::line`] and only
    /// reach the child when Enter is pressed. `false` is `term-char-mode`, where
    /// every key goes straight to the PTY (the default).
    line_mode: bool,
    /// The locally-edited input line, in `term-line-mode`.
    line: String,
    /// Emacs `term-pager-toggle`: shared with the reader thread, which stops
    /// feeding the vt100 parser once a screenful of output has arrived and waits
    /// for a key.
    pager: Pager,
}

/// The paging state (Emacs `term-pager-enabled`), shared with the reader thread.
///
/// With paging on, output is fed to the vt100 parser a line at a time and stops
/// after a screenful; the rest waits in [`Pager::held`] until a key resumes it.
/// Both the reader thread (as output arrives) and the main thread (when a key
/// resumes) drive [`Pager::pump`], so held output appears the moment the key is
/// pressed rather than when the child next writes something.
#[derive(Clone)]
struct Pager {
    /// Whether paging is on at all.
    on: Arc<AtomicBool>,
    /// Whether output is currently held back, waiting for a key.
    paused: Arc<AtomicBool>,
    /// Rows of the screen, so a "screenful" is the right size.
    rows: Arc<std::sync::atomic::AtomicUsize>,
    /// Output read from the PTY but not yet given to the parser.
    held: Arc<Mutex<Vec<u8>>>,
    /// Lines fed to the parser since the last pause.
    fed: Arc<std::sync::atomic::AtomicUsize>,
}

impl Pager {
    fn new(rows: u16) -> Self {
        Self {
            on: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            rows: Arc::new(std::sync::atomic::AtomicUsize::new(rows as usize)),
            held: Arc::new(Mutex::new(Vec::new())),
            fed: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// Feed held output into `parser` until a screenful has landed (then pause) or
    /// nothing is held. With paging off, everything held goes through at once.
    fn pump(&self, parser: &Mutex<vt100::Parser>) {
        loop {
            if self.paused.load(Ordering::Relaxed) {
                return;
            }
            let paging = self.on.load(Ordering::Relaxed);
            let chunk: Vec<u8> = {
                let Ok(mut held) = self.held.lock() else {
                    return;
                };
                if held.is_empty() {
                    return;
                }
                if !paging {
                    std::mem::take(&mut *held)
                } else {
                    let take = match held.iter().position(|&b| b == b'\n') {
                        Some(i) => i + 1,
                        None => held.len(),
                    };
                    held.drain(..take).collect()
                }
            };
            let ended_line = chunk.last() == Some(&b'\n');
            if let Ok(mut p) = parser.lock() {
                p.process(&chunk);
            }
            if !paging {
                self.fed.store(0, Ordering::Relaxed);
                return;
            }
            if ended_line {
                let screenful = self.rows.load(Ordering::Relaxed).saturating_sub(1);
                let fed = self.fed.fetch_add(1, Ordering::Relaxed) + 1;
                if screenful > 0 && fed >= screenful {
                    self.fed.store(0, Ordering::Relaxed);
                    self.paused.store(true, Ordering::Relaxed);
                    return;
                }
            }
        }
    }

    /// Let output flow again (a key was pressed at the `-- MORE --` prompt).
    fn resume(&self, parser: &Mutex<vt100::Parser>) {
        self.paused.store(false, Ordering::Relaxed);
        self.fed.store(0, Ordering::Relaxed);
        self.pump(parser);
        zmax_event::request_redraw();
    }
}

impl TerminalPanel {
    pub fn new() -> std::io::Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let cwd = std::env::current_dir().ok();
        Self::with_command(&shell, &[] as &[&str], cwd.as_deref())
    }

    /// Spawn an arbitrary `program` (with `args`) in a PTY panel — the same live,
    /// interactive, vt100-parsed terminal as [`Self::new`], but running a chosen
    /// command instead of `$SHELL`. Used to host a serial monitor
    /// (`arduino-cli monitor` / `pio device monitor`) or a firmware upload so its
    /// progress bar renders live. When the command exits the panel shows a dead
    /// terminal (dismiss with the close key) rather than dropping to a shell.
    pub fn with_command(
        program: &str,
        args: &[impl AsRef<std::ffi::OsStr>],
        cwd: Option<&std::path::Path>,
    ) -> std::io::Result<Self> {
        let (rows, cols) = (24u16, 80u16);
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let mut cmd = CommandBuilder::new(program);
        for a in args {
            cmd.arg(a);
        }
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        drop(pair.slave);

        // vim `scrollback`: lines of terminal history kept above the screen.
        let scrollback = crate::commands::vim_opt_num("scrollback").unwrap_or(2000);
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, scrollback)));
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let dead = Arc::new(AtomicBool::new(false));
        let pager = Pager::new(rows);
        {
            let parser = parser.clone();
            let dead = dead.clone();
            let pager = pager.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                // emacs `set-terminal-coding-system`: the coding system this
                // terminal's output bytes are decoded with. vt100 (the screen
                // emulator below) assumes UTF-8, so a terminal in another coding
                // system is transcoded to UTF-8 first. The decoder is
                // *incremental* — a read can land in the middle of a multi-byte
                // character — and is rebuilt if the coding system is changed while
                // the terminal is running. With none set the bytes go through
                // untouched, exactly as before.
                let mut decoder: Option<(&'static zmax_core::encoding::Encoding, _)> = None;
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let chunk = &buf[..n];
                            let coding = zmax_core::coding::terminal_coding();
                            if decoder.as_ref().map(|(e, _)| *e) != coding {
                                decoder = coding.map(|e| (e, e.new_decoder()));
                            }
                            if let Ok(mut held) = pager.held.lock() {
                                match decoder.as_mut() {
                                    Some((_, dec)) => {
                                        let cap = dec
                                            .max_utf8_buffer_length(chunk.len())
                                            .unwrap_or(chunk.len() * 4);
                                        let mut out = String::with_capacity(cap);
                                        // The buffer is sized by
                                        // `max_utf8_buffer_length`, so one call
                                        // always consumes the whole chunk.
                                        let (_result, _read, _had_errors) =
                                            dec.decode_to_string(chunk, &mut out, false);
                                        held.extend_from_slice(out.as_bytes());
                                    }
                                    None => held.extend_from_slice(chunk),
                                }
                            }
                            pager.pump(&parser);
                            zmax_event::request_redraw();
                        }
                    }
                }
                dead.store(true, Ordering::Relaxed);
                zmax_event::request_redraw();
            });
        }

        Ok(Self {
            parser,
            master: pair.master,
            child,
            writer,
            dead,
            rows,
            cols,
            caret: None,
            pane: None,
            pending_window: false,
            line_mode: false,
            line: String::new(),
            pager,
        })
    }

    /// Emacs `term-line-mode`: edit input locally, send it on Enter.
    pub fn set_line_mode(&mut self, on: bool) {
        self.line_mode = on;
        if !on {
            self.line.clear();
        }
    }

    /// Whether the panel is in `term-line-mode`.
    pub fn is_line_mode(&self) -> bool {
        self.line_mode
    }

    /// Emacs `term-pager-toggle`: turn paging of the child's output on or off.
    /// Returns the new state. Turning it off releases anything held back.
    pub fn toggle_pager(&mut self) -> bool {
        let on = !self.pager.on.fetch_xor(true, Ordering::Relaxed);
        if !on {
            self.pager.resume(&self.parser);
        }
        on
    }

    /// Resize the PTY + parser to `rows`×`cols` (no-op when unchanged).
    fn resize(&mut self, rows: u16, cols: u16) {
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows.max(1);
        self.cols = cols.max(1);
        self.pager.rows.store(self.rows as usize, Ordering::Relaxed);
        let _ = self.master.resize(PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(self.rows, self.cols);
        }
    }

    fn close() -> EventResult {
        EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
            c.pop();
        })))
    }

    /// A key in `term-line-mode`: it edits the local input line, and only Enter
    /// (or `C-c`, which interrupts) reaches the child.
    fn line_mode_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.line);
                self.send(line.as_bytes());
                self.send(b"\r");
            }
            KeyCode::Backspace => {
                self.line.pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.line.clear();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.line.clear();
                self.send(&[0x03]);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.send(&[0x04]);
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                // vim `:tmap` — a Terminal-mode mapping replaces the key before it
                // reaches the shell.
                match crate::commands::typed::terminal_map_lookup(&c.to_string()) {
                    Some(rhs) => self.line.push_str(&rhs),
                    None => self.line.push(c),
                }
            }
            _ => {}
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Scroll the terminal's scrollback view by `delta` lines (positive = back
    /// into history, negative = toward the live screen).
    fn scroll(&mut self, delta: isize) {
        if let Ok(mut parser) = self.parser.lock() {
            let cur = parser.screen().scrollback() as isize;
            let next = (cur + delta).max(0) as usize;
            parser.set_scrollback(next);
        }
        zmax_event::request_redraw();
    }

    /// Run a window command (the key after the `C-\` leader): split or move
    /// focus between panes, mirroring vim's `C-w` window maps. `v`/`s` split;
    /// `h`/`j`/`k`/`l` move focus; anything else is a no-op.
    fn window_command(key: &KeyEvent, cx: &mut Context) -> EventResult {
        use zmax_view::editor::Action;
        use zmax_view::tree::Direction;
        let editor = &mut cx.editor;
        let doc = editor.tree.try_get(editor.tree.focus).map(|view| view.doc);
        match key.code {
            KeyCode::Char('v') => {
                if let Some(doc) = doc {
                    editor.switch(doc, Action::VerticalSplit);
                }
            }
            KeyCode::Char('s') => {
                if let Some(doc) = doc {
                    editor.switch(doc, Action::HorizontalSplit);
                }
            }
            KeyCode::Char('h') | KeyCode::Left => editor.focus_direction(Direction::Left),
            KeyCode::Char('j') | KeyCode::Down => editor.focus_direction(Direction::Down),
            KeyCode::Char('k') | KeyCode::Up => editor.focus_direction(Direction::Up),
            KeyCode::Char('l') | KeyCode::Right => editor.focus_direction(Direction::Right),
            _ => {}
        }
        EventResult::Consumed(None)
    }
}

impl Drop for TerminalPanel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Component for TerminalPanel {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        // Only capture input while the terminal's pane is the focused one. When
        // another split is focused, fall through (Ignored) so the editor handles
        // keys and clicks (e.g. click-to-focus on another pane).
        let focused = self.pane.is_none_or(|pane| cx.editor.tree.focus == pane);
        // The terminal's own pane rect (for distinguishing clicks inside it from
        // clicks on another split).
        let pane_area = self
            .pane
            .and_then(|p| cx.editor.tree.try_get(p))
            .map(|view| view.area);
        match event {
            // Second key after the `C-\` window leader: run it as a window
            // command (split / move focus) instead of sending it to the shell.
            Event::Key(key) if focused && self.pending_window => {
                self.pending_window = false;
                Self::window_command(key, cx)
            }
            // `C-\` arms the window leader (C-w is the shell's word-kill, so we
            // can't use it). Swallowed; the next key is the window command.
            Event::Key(key)
                if focused
                    && key.code == KeyCode::Char('\\')
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.pending_window = true;
                EventResult::Consumed(None)
            }
            Event::Key(key) if focused => {
                // F12 detaches without needing to exit the shell.
                if key.code == KeyCode::F(12) {
                    return Self::close();
                }
                // If the shell already exited, any key closes the panel.
                if self.dead.load(Ordering::Relaxed) {
                    return Self::close();
                }
                // At the pager's `-- MORE --`, the key releases the next screenful
                // and is not passed on to the child (Emacs `term-pager-page`).
                if self.pager.paused.load(Ordering::Relaxed) {
                    self.pager.resume(&self.parser);
                    return EventResult::Consumed(None);
                }
                if self.line_mode {
                    self.line_mode_key(key);
                    return EventResult::Consumed(None);
                }
                if let Some(bytes) = key_to_bytes(key) {
                    self.send(&bytes);
                }
                EventResult::Consumed(None)
            }
            Event::Paste(s) if focused => {
                self.send(s.as_bytes());
                EventResult::Consumed(None)
            }
            // While focused, the wheel scrolls the terminal's own scrollback
            // rather than the editor underneath it.
            Event::Mouse(me) if focused && matches!(me.kind, MouseEventKind::ScrollUp) => {
                self.scroll(3);
                EventResult::Consumed(None)
            }
            Event::Mouse(me) if focused && matches!(me.kind, MouseEventKind::ScrollDown) => {
                self.scroll(-3);
                EventResult::Consumed(None)
            }
            // A click inside the terminal's pane stays here (and is swallowed so
            // it doesn't disturb the editor); a click on another pane falls
            // through so the editor's click-to-focus moves focus there.
            Event::Mouse(me)
                if focused
                    && matches!(me.kind, MouseEventKind::Down(_))
                    && pane_area.is_some_and(|a| {
                        me.column >= a.x
                            && me.column < a.x.saturating_add(a.width)
                            && me.row >= a.y
                            && me.row < a.y.saturating_add(a.height)
                    }) =>
            {
                EventResult::Consumed(None)
            }
            _ => EventResult::Ignored(None),
        }
    }

    fn render(&mut self, screen: Rect, surface: &mut Surface, ctx: &mut Context) {
        // Pin to a single editor pane (captured the first time we render, while
        // that pane is focused) and always draw there. Confining to one pane —
        // rather than following `tree.focus` — keeps other splits and the
        // statusline visible and stops the terminal from appearing to swap panes
        // when focus moves. Fall back to the full area if the pane is gone.
        let pane = *self.pane.get_or_insert(ctx.editor.tree.focus);
        let area = ctx
            .editor
            .tree
            .try_get(pane)
            .map(|view| view.area)
            .unwrap_or(screen);

        let theme = &ctx.editor.theme;
        // `transparent-background`: drop the page fill so the terminal shows through.
        let mut page_bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            page_bg.bg = None;
        }
        surface.clear_with(area, page_bg);
        if area.height < 2 || area.width < 2 {
            self.caret = None;
            return;
        }

        // title bar
        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        let title = if self.dead.load(Ordering::Relaxed) {
            " Terminal — process exited · press any key to close ".to_string()
        } else if self.pager.paused.load(Ordering::Relaxed) {
            " Terminal — -- MORE -- (any key: next page) ".to_string()
        } else {
            format!(
                " Terminal — {} — F12 detach{} ",
                if self.line_mode {
                    "line mode"
                } else {
                    "char mode"
                },
                if self.pager.on.load(Ordering::Relaxed) {
                    " · pager"
                } else {
                    ""
                }
            )
        };
        surface.set_stringn(
            area.x + 1,
            area.y,
            &title,
            area.width as usize,
            theme.get("function"),
        );

        let grid = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        self.resize(grid.height, grid.width);

        let parser = match self.parser.lock() {
            Ok(p) => p,
            Err(_) => return,
        };
        let screen = parser.screen();
        for row in 0..grid.height {
            for col in 0..grid.width {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                let mut style = Style::default()
                    .fg(conv_color(cell.fgcolor(), Color::Reset))
                    .bg(conv_color(cell.bgcolor(), Color::Reset));
                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.italic() {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.inverse() {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                let contents = cell.contents();
                let sym = if contents.is_empty() { " " } else { &contents };
                if let Some(c) = surface.get_mut(grid.x + col, grid.y + row) {
                    c.set_symbol(sym);
                    c.set_style(style);
                }
            }
        }

        // place the real terminal cursor where the shell put it
        if !screen.hide_cursor() && !self.dead.load(Ordering::Relaxed) {
            let (crow, ccol) = screen.cursor_position();
            if crow < grid.height && ccol < grid.width {
                self.caret = Some(zmax_core::Position::new(
                    (grid.y + crow) as usize,
                    (grid.x + ccol) as usize,
                ));
            } else {
                self.caret = None;
            }
        } else {
            self.caret = None;
        }

        // `term-line-mode`: the child has not seen the input line yet (nothing has
        // been sent), so echo it locally at the cursor and leave the caret at its
        // end — the local editing Emacs's line mode gives you.
        if self.line_mode && !self.dead.load(Ordering::Relaxed) {
            let (crow, ccol) = screen.cursor_position();
            if crow < grid.height {
                let x = grid.x + ccol;
                let room = grid.width.saturating_sub(ccol) as usize;
                let style = Style::default().add_modifier(Modifier::REVERSED);
                surface.set_stringn(x, grid.y + crow, &self.line, room, style);
                let end = ccol + (self.line.chars().count() as u16).min(room as u16);
                if end < grid.width {
                    self.caret = Some(zmax_core::Position::new(
                        (grid.y + crow) as usize,
                        (grid.x + end) as usize,
                    ));
                }
            }
        }
    }

    fn cursor(
        &self,
        _area: Rect,
        editor: &zmax_view::editor::Editor,
    ) -> (Option<zmax_core::Position>, CursorKind) {
        // Only own the cursor while our pane is focused; otherwise yield so the
        // editor draws its cursor in the focused split.
        let focused = self.pane.is_none_or(|pane| editor.tree.focus == pane);
        if focused {
            (self.caret, CursorKind::Block)
        } else {
            (None, CursorKind::Hidden)
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("terminal")
    }
}

/// Map a `vt100::Color` to a zmax `Color`, using `default` for the terminal
/// default color.
fn conv_color(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Translate a key event into the byte sequence a terminal expects.
fn key_to_bytes(key: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let esc = |s: &str| -> Vec<u8> {
        let mut v = vec![0x1b];
        v.extend_from_slice(s.as_bytes());
        v
    };
    let bytes = match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                // Control byte: C-a → 0x01 … C-_ etc.
                let b = (c.to_ascii_uppercase() as u8).wrapping_sub(b'@');
                if b < 0x20 || c == ' ' {
                    vec![b & 0x1f]
                } else {
                    return None;
                }
            } else {
                let mut s = [0u8; 4];
                let mut v = Vec::new();
                if alt {
                    v.push(0x1b);
                }
                // emacs `set-keyboard-coding-system`: a typed character is encoded
                // into the bytes the terminal is sent with the keyboard coding
                // system — so a terminal running in EUC-JP is typed to in EUC-JP.
                // Unset (the normal case) this is UTF-8, as it has always been.
                let keyboard = zmax_core::coding::keyboard_coding();
                v.extend_from_slice(&zmax_core::coding::encode_with(
                    keyboard,
                    c.encode_utf8(&mut s),
                ));
                v
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => esc("[A"),
        KeyCode::Down => esc("[B"),
        KeyCode::Right => esc("[C"),
        KeyCode::Left => esc("[D"),
        KeyCode::Home => esc("[H"),
        KeyCode::End => esc("[F"),
        KeyCode::PageUp => esc("[5~"),
        KeyCode::PageDown => esc("[6~"),
        KeyCode::Delete => esc("[3~"),
        KeyCode::Insert => esc("[2~"),
        KeyCode::F(n) => match n {
            1 => esc("OP"),
            2 => esc("OQ"),
            3 => esc("OR"),
            4 => esc("OS"),
            5 => esc("[15~"),
            6 => esc("[17~"),
            7 => esc("[18~"),
            8 => esc("[19~"),
            9 => esc("[20~"),
            10 => esc("[21~"),
            11 => esc("[23~"),
            _ => return None,
        },
        _ => return None,
    };
    Some(bytes)
}
