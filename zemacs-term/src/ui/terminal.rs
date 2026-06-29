//! Integrated terminal: a real PTY-backed shell rendered inside zemacs.
//!
//! A [`Component`] (confined to the focused editor pane) that spawns the user's `$SHELL` in a pseudo-
//! terminal ([`portable_pty`]), feeds its output through a [`vt100`] parser that
//! maintains a screen grid, and blits that grid onto the zemacs `Surface` each
//! frame. Keystrokes are translated to terminal byte sequences and written back
//! to the PTY, so interactive programs (REPLs, `vim`, `htop`, prompts) work.
//!
//! A background reader thread drives redraws via `zemacs_event::request_redraw`
//! as output arrives. **F12** detaches (closing the panel, leaving the shell to
//! be killed on drop); exiting the shell (`exit` / `C-d`) also closes it.
//!
//! Open: `:terminal` / the `terminal` command.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::{Color, CursorKind, Modifier, Rect, Style},
    input::{KeyCode, KeyEvent, KeyModifiers},
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
    caret: Option<zemacs_core::Position>,
    /// The editor pane this terminal lives in, captured on first render. The
    /// terminal stays pinned here (it doesn't follow `tree.focus`, which would
    /// make it appear to jump panes), and only takes keystrokes while this pane
    /// is the focused one — so clicking another split moves focus to the editor
    /// and clicking back returns focus to the terminal.
    pane: Option<ViewId>,
}

impl TerminalPanel {
    pub fn new() -> std::io::Result<Self> {
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

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut cmd = CommandBuilder::new(shell);
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        drop(pair.slave);

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000)));
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let dead = Arc::new(AtomicBool::new(false));
        {
            let parser = parser.clone();
            let dead = dead.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut p) = parser.lock() {
                                p.process(&buf[..n]);
                            }
                            zemacs_event::request_redraw();
                        }
                    }
                }
                dead.store(true, Ordering::Relaxed);
                zemacs_event::request_redraw();
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
        })
    }

    /// Resize the PTY + parser to `rows`×`cols` (no-op when unchanged).
    fn resize(&mut self, rows: u16, cols: u16) {
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows.max(1);
        self.cols = cols.max(1);
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

    fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
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
        // keys; mouse events always fall through so a click on any pane (incl.
        // this one) reaches the editor's click-to-focus handler.
        let focused = self.pane.map_or(true, |pane| cx.editor.tree.focus == pane);
        match event {
            Event::Key(key) if focused => {
                // F12 detaches without needing to exit the shell.
                if key.code == KeyCode::F(12) {
                    return Self::close();
                }
                // If the shell already exited, any key closes the panel.
                if self.dead.load(Ordering::Relaxed) {
                    return Self::close();
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
        surface.clear_with(area, theme.get("ui.background"));
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
            " Terminal — process exited · press any key to close "
        } else {
            " Terminal — F12 detach "
        };
        surface.set_stringn(
            area.x + 1,
            area.y,
            title,
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
                self.caret = Some(zemacs_core::Position::new(
                    (grid.y + crow) as usize,
                    (grid.x + ccol) as usize,
                ));
            } else {
                self.caret = None;
            }
        } else {
            self.caret = None;
        }
    }

    fn cursor(
        &self,
        _area: Rect,
        editor: &zemacs_view::editor::Editor,
    ) -> (Option<zemacs_core::Position>, CursorKind) {
        // Only own the cursor while our pane is focused; otherwise yield so the
        // editor draws its cursor in the focused split.
        let focused = self.pane.map_or(true, |pane| editor.tree.focus == pane);
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

/// Map a `vt100::Color` to a zemacs `Color`, using `default` for the terminal
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
                v.extend_from_slice(c.encode_utf8(&mut s).as_bytes());
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
