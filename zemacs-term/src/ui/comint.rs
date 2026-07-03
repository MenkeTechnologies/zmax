//! Comint — a line-oriented interactive-subprocess buffer, the zemacs port of
//! GNU Emacs `comint-mode` / `M-x shell`.
//!
//! Unlike the PTY-backed [`super::terminal`] (a full vt100 screen emulator, the
//! `term`/`ansi-term` analogue), comint is the *dumb-terminal* REPL model: a
//! scrollback of output lines plus a single editable input line at the bottom.
//! You type a command, press Enter, it is written to the child's stdin and its
//! stdout/stderr stream back into the scrollback. This is the mode inferior-lisp,
//! the Python shell and `gud` derive from in Emacs.
//!
//! The child (`$SHELL` by default) is spawned with piped stdio; two reader
//! threads append its stdout/stderr lines to shared state and request a redraw.
//! The pure input-ring history (`comint-input-ring`) lives in the filesystem-free
//! [`zemacs_core::comint`].
//!
//! Keys (parsed into a `comint` keymap mode by `scripts/gen_port_report.py`):
//!   Enter — comint-send-input (run the input line)
//!   Up / C-p — comint-previous-input; Down / C-n — comint-next-input
//!   C-a / Home — start of input; C-e / End — end of input
//!   C-k — comint-kill-input (clear the input line)
//!   C-c — comint-interrupt-subjob (send SIGINT-equivalent ^C)
//!   C-d — comint-send-eof (close the child's stdin) when the input is empty
//!   PageUp / PageDown — scroll the scrollback
//!   F12 — detach the comint panel (the child is killed on drop)

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tui::buffer::Buffer as Surface;
use zemacs_core::comint::InputRing;
use zemacs_view::graphics::{CursorKind, Rect};
use zemacs_view::input::KeyCode;

use crate::{
    alt,
    compositor::{Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Cap on retained scrollback lines (matches `super::run::MAX_LINES` intent).
const MAX_LINES: usize = 5000;

/// Output scrollback shared between the reader threads and the render loop.
#[derive(Default)]
struct Scrollback {
    lines: Vec<String>,
}

impl Scrollback {
    fn push(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > MAX_LINES {
            let drop = self.lines.len() - MAX_LINES;
            self.lines.drain(0..drop);
        }
    }
}

/// The interactive comint overlay hosting one subprocess.
pub struct Comint {
    program: String,
    scrollback: Arc<Mutex<Scrollback>>,
    /// Child handle, kept so the process is killed when the panel is dropped.
    child: Child,
    /// Writer to the child's stdin; `None` after `comint-send-eof`.
    stdin: Option<ChildStdin>,
    ring: InputRing,
    /// The current, unsubmitted input line and caret column (in chars).
    input: String,
    caret: usize,
    /// Input stashed when history navigation began, restored on `next` past the
    /// newest entry (Emacs `comint-stored-incomplete-input`).
    stash: Option<String>,
    /// Lines scrolled up from the bottom (0 = following the tail).
    scroll: usize,
    /// Set by a reader thread at EOF (the child exited).
    dead: Arc<AtomicBool>,
    /// Screen cursor, updated each render for `Component::cursor`.
    cursor: Option<zemacs_core::Position>,
}

impl Comint {
    /// Open a comint on `$SHELL` (Emacs `M-x shell`).
    pub fn shell() -> std::io::Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        Self::with_program(&shell, &[] as &[&str])
    }

    /// Open a comint running `program` with `args` (Emacs `comint-run`).
    pub fn with_program(program: &str, args: &[impl AsRef<std::ffi::OsStr>]) -> std::io::Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let scrollback = Arc::new(Mutex::new(Scrollback::default()));
        let dead = Arc::new(AtomicBool::new(false));
        let stdin = child.stdin.take();

        // One reader thread each for stdout and stderr; both append lines to the
        // shared scrollback and request a redraw as output arrives.
        for pipe in [
            child.stdout.take().map(PipeSource::Out),
            child.stderr.take().map(PipeSource::Err),
        ]
        .into_iter()
        .flatten()
        {
            let scrollback = scrollback.clone();
            let dead = dead.clone();
            std::thread::spawn(move || {
                let is_out = matches!(pipe, PipeSource::Out(_));
                let mut reader: BufReader<Box<dyn std::io::Read + Send>> = match pipe {
                    PipeSource::Out(o) => BufReader::new(Box::new(o)),
                    PipeSource::Err(e) => BufReader::new(Box::new(e)),
                };
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let line = buf.trim_end_matches(['\n', '\r']).to_string();
                            if let Ok(mut sb) = scrollback.lock() {
                                sb.push(line);
                            }
                            zemacs_event::request_redraw();
                        }
                    }
                }
                // Only stdout EOF marks the process as dead; stderr may close
                // first without the child having exited.
                if is_out {
                    dead.store(true, Ordering::Relaxed);
                    zemacs_event::request_redraw();
                }
            });
        }

        Ok(Self {
            program: program.to_string(),
            scrollback,
            child,
            stdin,
            ring: InputRing::default(),
            input: String::new(),
            caret: 0,
            stash: None,
            scroll: 0,
            dead,
            cursor: None,
        })
    }

    /// Echo the submitted input into the scrollback with a prompt, then write it
    /// to the child's stdin — `comint-send-input`.
    fn send_input(&mut self) {
        let line = std::mem::take(&mut self.input);
        self.caret = 0;
        self.stash = None;
        if let Ok(mut sb) = self.scrollback.lock() {
            sb.push(format!("{} {line}", self.prompt().trim_end()));
        }
        self.ring.add(&line);
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = stdin.write_all(line.as_bytes());
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
        }
        self.scroll = 0;
    }

    /// The input prompt shown at the bottom.
    fn prompt(&self) -> String {
        if self.dead.load(Ordering::Relaxed) {
            "[process exited] ".to_string()
        } else {
            "> ".to_string()
        }
    }

    /// Replace the input line with a history entry (or the stashed incomplete
    /// input) during `comint-previous-input` / `comint-next-input`.
    fn set_input(&mut self, text: &str) {
        self.input = text.to_string();
        self.caret = self.input.chars().count();
    }

    fn history_previous(&mut self) {
        if !self.ring.navigating() {
            self.stash = Some(self.input.clone());
        }
        if let Some(prev) = self.ring.previous().map(str::to_string) {
            self.set_input(&prev);
        }
    }

    fn history_next(&mut self) {
        match self.ring.next().map(str::to_string) {
            Some(next) => self.set_input(&next),
            None => {
                // Stepped past the newest entry: restore the stashed input.
                let stash = self.stash.take().unwrap_or_default();
                self.set_input(&stash);
            }
        }
    }

    /// Send a `^C` (interrupt) to the child — `comint-interrupt-subjob`.
    fn interrupt(&mut self) {
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = stdin.write_all(&[0x03]);
            let _ = stdin.flush();
        }
    }

    /// Close the child's stdin — `comint-send-eof`.
    fn send_eof(&mut self) {
        self.stdin = None;
    }

    fn insert_char(&mut self, c: char) {
        let byte = char_index_to_byte(&self.input, self.caret);
        self.input.insert(byte, c);
        self.caret += 1;
        self.ring.reset();
    }

    fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        let start = char_index_to_byte(&self.input, self.caret - 1);
        let end = char_index_to_byte(&self.input, self.caret);
        self.input.replace_range(start..end, "");
        self.caret -= 1;
        self.ring.reset();
    }

    fn close() -> EventResult {
        EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
            c.pop();
        })))
    }
}

impl Drop for Comint {
    /// A `std::process::Child` is *not* reaped on drop, so kill and wait for the
    /// subprocess explicitly when the panel closes to avoid orphaning the shell.
    fn drop(&mut self) {
        // Dropping stdin sends EOF first, letting well-behaved shells exit.
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A taken child pipe tagged with its source stream.
enum PipeSource {
    Out(std::process::ChildStdout),
    Err(std::process::ChildStderr),
}

/// Byte offset of the `n`-th char in `s` (or `s.len()` when `n` is past the end).
fn char_index_to_byte(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(b, _)| b)
}

impl Component for Comint {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        // F12 detaches the panel (KeyCode::F(12) has no `key!` form).
        if key.code == KeyCode::F(12) {
            return Comint::close();
        }
        match key {
            key!(Enter) => self.send_input(),
            // Emacs binds history to M-p / M-n (comint-previous/next-input); Up/Down
            // do the same at the prompt via comint-{up,down}-or-history.
            key!(Up) | alt!('p') => self.history_previous(),
            key!(Down) | alt!('n') => self.history_next(),
            ctrl!('p') => self.history_previous(),
            ctrl!('n') => self.history_next(),
            key!(Home) | ctrl!('a') => self.caret = 0,
            key!(End) | ctrl!('e') => self.caret = self.input.chars().count(),
            key!(Left) | ctrl!('b') => self.caret = self.caret.saturating_sub(1),
            key!(Right) | ctrl!('f') => {
                self.caret = (self.caret + 1).min(self.input.chars().count());
            }
            key!(Backspace) => self.backspace(),
            ctrl!('k') => {
                self.input.clear();
                self.caret = 0;
                self.ring.reset();
            }
            ctrl!('c') => self.interrupt(),
            ctrl!('d') => {
                if self.input.is_empty() {
                    self.send_eof();
                } else {
                    // Non-empty: emacs `comint-delchar-or-maybe-eof` deletes char.
                    if self.caret < self.input.chars().count() {
                        let start = char_index_to_byte(&self.input, self.caret);
                        let end = char_index_to_byte(&self.input, self.caret + 1);
                        self.input.replace_range(start..end, "");
                    }
                }
            }
            key!(PageUp) => self.scroll = self.scroll.saturating_add(5),
            key!(PageDown) => self.scroll = self.scroll.saturating_sub(5),
            _ => {
                // Plain printable character (no control/alt) -> text input.
                if let KeyCode::Char(c) = key.code {
                    use zemacs_view::keyboard::KeyModifiers;
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                        self.insert_char(c);
                    }
                }
            }
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let prompt_style = theme.get("ui.text.directory");

        surface.clear_with(area, bg);
        if area.width < 4 || area.height < 3 {
            return;
        }

        let title = format!(" Comint: {} ", self.program);
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);

        // Body spans from the row below the title to the row above the input.
        let body_top = area.y + 1;
        let input_y = area.y + area.height - 1;
        let body_rows = input_y.saturating_sub(body_top) as usize;

        let lines = self.scrollback.lock().map(|sb| sb.lines.clone()).unwrap_or_default();
        // Tail-follow, offset upward by `scroll`.
        let total = lines.len();
        let max_scroll = total.saturating_sub(body_rows);
        let scroll = self.scroll.min(max_scroll);
        let end = total.saturating_sub(scroll);
        let start = end.saturating_sub(body_rows);
        for (i, line) in lines[start..end].iter().enumerate() {
            surface.set_stringn(
                area.x,
                body_top + i as u16,
                line,
                area.width as usize,
                text_style,
            );
        }

        // Input line: prompt + current input.
        let prompt = self.prompt();
        surface.set_stringn(area.x, input_y, &prompt, area.width as usize, prompt_style);
        let px = area.x + prompt.chars().count() as u16;
        surface.set_stringn(
            px,
            input_y,
            &self.input,
            (area.width as usize).saturating_sub(prompt.chars().count()),
            text_style,
        );
        self.cursor = Some(zemacs_core::Position::new(
            input_y as usize,
            (px as usize) + self.caret,
        ));
    }

    fn cursor(
        &self,
        _area: Rect,
        _editor: &zemacs_view::editor::Editor,
    ) -> (Option<zemacs_core::Position>, CursorKind) {
        (self.cursor, CursorKind::Block)
    }
}
