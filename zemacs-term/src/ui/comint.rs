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
//!   C-a / Home — comint-bol-or-process-mark; C-e / End — end of input
//!   C-k — kill to end of line; M-. — comint-insert-previous-argument (!$)
//!   Space — comint-magic-space (expand !! / !$ history, then space)
//!   C-d — comint-delchar-or-maybe-eof (delete char, or EOF on empty line)
//!   C-c is a prefix (comint job control), then:
//!     C-c — comint-interrupt-subjob (SIGINT)   C-z — comint-stop-subjob (SIGTSTP)
//!     C-u — comint-kill-input                  C-\\ — comint-quit-subjob (SIGQUIT)
//!     C-n / C-p — comint-next/previous-prompt   C-r — comint-show-output
//!     C-e — comint-show-maximum-output          C-o — comint-delete-output
//!     RET — comint-copy-old-input
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
/// `is_prompt[i]` marks line `i` as an echoed prompt/input line (written by
/// [`Comint::submit`]) rather than subprocess output — the marker Emacs keeps via
/// the `field`/`comint-prompt` text properties, used by prompt/output navigation.
#[derive(Default)]
struct Scrollback {
    lines: Vec<String>,
    is_prompt: Vec<bool>,
}

impl Scrollback {
    fn push(&mut self, line: String) {
        self.push_kind(line, false);
    }

    fn push_kind(&mut self, line: String, is_prompt: bool) {
        self.lines.push(line);
        self.is_prompt.push(is_prompt);
        if self.lines.len() > MAX_LINES {
            let drop = self.lines.len() - MAX_LINES;
            self.lines.drain(0..drop);
            self.is_prompt.drain(0..drop);
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
    /// A scrollback line index that the next render should bring to the top of
    /// the body (resolved there, where the body height is known). Set by
    /// `comint-show-output` / `comint-next-prompt` / `comint-previous-prompt`.
    pending_top: Option<usize>,
    /// Set by a reader thread at EOF (the child exited).
    dead: Arc<AtomicBool>,
    /// Screen cursor, updated each render for `Component::cursor`.
    cursor: Option<zemacs_core::Position>,
    /// `true` after a bare `C-c`, awaiting the second key of a `C-c <key>` comint
    /// prefix command (interrupt/stop/kill-input/prompt-nav/…).
    pending_ctrl_c: bool,
}

impl Comint {
    /// Open a comint on `$SHELL` (Emacs `M-x shell`).
    pub fn shell() -> std::io::Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        Self::with_program(&shell, &[] as &[&str])
    }

    /// Open a comint running `program` with `args` (Emacs `comint-run`).
    pub fn with_program(
        program: &str,
        args: &[impl AsRef<std::ffi::OsStr>],
    ) -> std::io::Result<Self> {
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
            pending_top: None,
            dead,
            cursor: None,
            pending_ctrl_c: false,
        })
    }

    /// Echo `line` into the scrollback with the prompt, record it in the input
    /// ring and write it (plus a newline) to the child's stdin.
    fn submit(&mut self, line: &str) {
        if let Ok(mut sb) = self.scrollback.lock() {
            sb.push_kind(format!("{} {line}", self.prompt().trim_end()), true);
        }
        self.ring.add(line);
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = stdin.write_all(line.as_bytes());
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
        }
        self.scroll = 0;
    }

    /// Submit the current input line to the subprocess — `comint-send-input`.
    fn send_input(&mut self) {
        let line = std::mem::take(&mut self.input);
        self.caret = 0;
        self.stash = None;
        self.submit(&line);
    }

    /// Send a caller-supplied command line to the subprocess (used by `gud-*`
    /// to drive an inferior debugger running in this comint).
    pub fn send_command(&mut self, cmd: &str) {
        self.submit(cmd);
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

    /// Deliver Unix signal `sig` to the child process. Because the comint child
    /// is spawned with piped stdio (no controlling PTY / distinct process group),
    /// this signals the direct child by PID — faithful for a single inferior
    /// (shell/gdb) but not a whole job-control pipeline. Returns whether the
    /// signal was dispatched (`false` when the child has already exited).
    #[cfg(unix)]
    fn signal(&self, sig: i32) -> bool {
        if self.dead.load(Ordering::Relaxed) {
            return false;
        }
        // SAFETY: `kill(2)` with a valid pid and signal number has no memory
        // effects; a stale pid merely yields ESRCH, which we ignore.
        unsafe { libc::kill(self.child.id() as libc::pid_t, sig) == 0 }
    }
    #[cfg(not(unix))]
    fn signal(&self, _sig: i32) -> bool {
        false
    }

    /// `comint-interrupt-subjob` (C-c C-c) — send SIGINT to the child.
    pub fn interrupt_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGINT)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-stop-subjob` (C-c C-z) — suspend the child with SIGTSTP.
    pub fn stop_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGTSTP)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-continue-subjob` — resume a stopped child with SIGCONT.
    pub fn continue_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGCONT)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-quit-subjob` — send SIGQUIT (core-dumping quit) to the child.
    pub fn quit_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGQUIT)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-kill-subjob` — send SIGKILL to the child.
    pub fn kill_subjob(&self) -> bool {
        #[cfg(unix)]
        {
            self.signal(libc::SIGKILL)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// `comint-kill-input` (C-c C-u) — discard the pending (unsubmitted) input.
    pub fn kill_input(&mut self) {
        self.input.clear();
        self.caret = 0;
        self.stash = None;
        self.ring.reset();
    }

    /// `comint-bol-or-process-mark` (C-a) — go to the process mark, or, when
    /// already there, to the true beginning of line.
    pub fn bol_or_process_mark(&mut self) {
        self.caret = zemacs_core::comint::bol_or_process_mark_target(self.caret, 0);
    }

    /// `comint-delchar-or-maybe-eof` (C-d) — delete the char after point, or send
    /// EOF (close stdin) when the input line is empty. Returns whether EOF was
    /// sent.
    pub fn delchar_or_maybe_eof(&mut self) -> bool {
        if self.input.is_empty() {
            self.send_eof();
            return true;
        }
        if self.caret < self.input.chars().count() {
            let start = char_index_to_byte(&self.input, self.caret);
            let end = char_index_to_byte(&self.input, self.caret + 1);
            self.input.replace_range(start..end, "");
        }
        false
    }

    /// `comint-insert-previous-argument` (M-.) — insert the last argument of the
    /// previous command (`!$`) at point.
    pub fn insert_previous_argument(&mut self) {
        if let Some(arg) = self
            .ring
            .newest()
            .and_then(zemacs_core::comint::last_argument)
            .map(str::to_string)
        {
            self.insert_str(&arg);
        }
    }

    /// `comint-magic-space` — expand any history designators (`!!`, `!$`, …) in
    /// the input line, then insert a space at point.
    pub fn magic_space(&mut self) {
        if let Some(expanded) = zemacs_core::comint::expand_history(&self.input, &self.ring) {
            self.input = expanded;
            self.caret = self.input.chars().count();
        }
        self.insert_char(' ');
    }

    /// `comint-get-next-from-history` — replace the input with the next entry
    /// forward from the current history position (like `M-n` after a history
    /// yank).
    pub fn get_next_from_history(&mut self) {
        self.history_next();
    }

    /// `comint-copy-old-input` (C-c RET) — copy the most recent submitted input
    /// onto the current input line (zemacs has no separate point in the
    /// scrollback, so this yanks the last command rather than the one under a
    /// buffer cursor).
    pub fn copy_old_input(&mut self) -> bool {
        if let Some(old) = self.ring.newest().map(str::to_string) {
            self.set_input(&old);
            self.ring.reset();
            true
        } else {
            false
        }
    }

    /// `comint-show-maximum-output` (C-c C-e) — scroll so the newest output is at
    /// the bottom of the window.
    pub fn show_maximum_output(&mut self) {
        self.scroll = 0;
        self.pending_top = None;
    }

    /// `comint-show-output` (C-c C-r) — put the beginning of the last command's
    /// output at the top of the window.
    pub fn show_output(&mut self) -> bool {
        let anchor = self
            .scrollback
            .lock()
            .ok()
            .and_then(|sb| zemacs_core::comint::last_prompt_line(&sb.is_prompt));
        match anchor {
            Some(idx) => {
                self.pending_top = Some(idx);
                true
            }
            None => false,
        }
    }

    /// Shared prompt navigation for `comint-next-prompt` / `comint-previous-prompt`.
    fn goto_prompt(&mut self, forward: bool) -> bool {
        let (prompts, total) = match self.scrollback.lock() {
            Ok(sb) => (
                sb.is_prompt
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &p)| p.then_some(i))
                    .collect::<Vec<_>>(),
                sb.lines.len(),
            ),
            Err(_) => return false,
        };
        if prompts.is_empty() {
            return false;
        }
        // The line currently anchored to the top of the body (approx.).
        let current_top = self
            .pending_top
            .unwrap_or_else(|| total.saturating_sub(self.scroll + 1));
        let target = if forward {
            prompts.iter().copied().find(|&p| p > current_top)
        } else {
            prompts.iter().copied().rev().find(|&p| p < current_top)
        };
        match target {
            Some(idx) => {
                self.pending_top = Some(idx);
                true
            }
            None => false,
        }
    }

    /// `comint-next-prompt` (C-c C-n) — move to the next prompt below.
    pub fn next_prompt(&mut self) -> bool {
        self.goto_prompt(true)
    }

    /// `comint-previous-prompt` (C-c C-p) — move to the previous prompt above.
    pub fn previous_prompt(&mut self) -> bool {
        self.goto_prompt(false)
    }

    /// `comint-delete-output` (C-c C-o) — delete the output produced by the last
    /// command (the lines after the most recent prompt). Returns how many lines
    /// were removed.
    pub fn delete_output(&mut self) -> usize {
        let Ok(mut sb) = self.scrollback.lock() else {
            return 0;
        };
        if let Some((start, end)) = zemacs_core::comint::last_output_range(&sb.is_prompt) {
            sb.lines.drain(start..end);
            sb.is_prompt.drain(start..end);
            end - start
        } else {
            0
        }
    }

    /// `comint-write-output` — write the last command's output to `path`. Returns
    /// the number of lines written.
    pub fn write_output(&self, path: &std::path::Path) -> std::io::Result<usize> {
        let lines = {
            let sb = self
                .scrollback
                .lock()
                .map_err(|_| std::io::Error::other("scrollback poisoned"))?;
            match zemacs_core::comint::last_output_range(&sb.is_prompt) {
                Some((start, end)) => sb.lines[start..end].to_vec(),
                None => Vec::new(),
            }
        };
        let mut body = lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        std::fs::write(path, body)?;
        Ok(lines.len())
    }

    /// `comint-truncate-buffer` — trim the scrollback to at most `max` lines,
    /// keeping the newest (the tail nearest the prompt). Returns lines removed.
    pub fn truncate_buffer(&mut self, max: usize) -> usize {
        let Ok(mut sb) = self.scrollback.lock() else {
            return 0;
        };
        if sb.lines.len() > max {
            let drop = sb.lines.len() - max;
            sb.lines.drain(0..drop);
            sb.is_prompt.drain(0..drop);
            drop
        } else {
            0
        }
    }

    /// `comint-strip-ctrl-m` — strip carriage returns (`^M`) from every scrollback
    /// line. Returns the number of lines that changed.
    pub fn strip_ctrl_m(&mut self) -> usize {
        let Ok(mut sb) = self.scrollback.lock() else {
            return 0;
        };
        let mut changed = 0;
        for line in sb.lines.iter_mut() {
            if line.contains('\r') {
                *line = zemacs_core::comint::strip_ctrl_m(line);
                changed += 1;
            }
        }
        changed
    }

    /// `comint-dynamic-list-input-ring` — echo the input history into the
    /// scrollback (Emacs pops a `*Input History*` help buffer; zemacs lists it
    /// inline). Returns the number of entries listed.
    pub fn list_input_ring(&mut self) -> usize {
        let items: Vec<String> = self.ring.items().to_vec();
        if let Ok(mut sb) = self.scrollback.lock() {
            sb.push("=== Input history (newest first) ===".to_string());
            for (i, it) in items.iter().enumerate() {
                sb.push(format!("{:4}  {it}", i + 1));
            }
        }
        self.scroll = 0;
        items.len()
    }

    /// `comint-history-isearch-backward-regexp` (degraded) — search the input
    /// ring backward for `needle` (substring) and yank the first older match onto
    /// the input line. Returns the matched entry.
    pub fn history_search_backward(&mut self, needle: &str) -> Option<String> {
        if !self.ring.navigating() {
            self.stash = Some(self.input.clone());
        }
        let found = self.ring.previous_matching(needle).map(str::to_string);
        if let Some(m) = &found {
            self.set_input(m);
        }
        found
    }

    /// `shell-forward-command` (M-f) — move point forward over the next shell
    /// command on the input line (`;`/`|`/`&`-separated).
    pub fn forward_command(&mut self) {
        self.caret = zemacs_core::comint::forward_command(&self.input, self.caret);
    }

    /// `shell-backward-command` (M-b) — move point backward to the start of the
    /// shell command on the input line.
    pub fn backward_command(&mut self) {
        self.caret = zemacs_core::comint::backward_command(&self.input, self.caret);
    }

    /// Insert an owned string at point (used by history/argument insertion).
    fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char(c);
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
        // Second key of a `C-c <key>` comint job-control prefix.
        if self.pending_ctrl_c {
            self.pending_ctrl_c = false;
            match key {
                ctrl!('c') => {
                    self.interrupt_subjob();
                }
                ctrl!('u') => self.kill_input(),
                ctrl!('z') => {
                    self.stop_subjob();
                }
                ctrl!('n') => {
                    self.next_prompt();
                }
                ctrl!('p') => {
                    self.previous_prompt();
                }
                ctrl!('r') => {
                    self.show_output();
                }
                ctrl!('e') => self.show_maximum_output(),
                ctrl!('o') => {
                    self.delete_output();
                }
                key!(Enter) => {
                    self.copy_old_input();
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        match key {
            key!(Enter) => self.send_input(),
            // Emacs binds history to M-p / M-n (comint-previous/next-input); Up/Down
            // do the same at the prompt via comint-{up,down}-or-history.
            key!(Up) | alt!('p') => self.history_previous(),
            key!(Down) | alt!('n') => self.history_next(),
            ctrl!('p') => self.history_previous(),
            ctrl!('n') => self.history_next(),
            key!(Home) | ctrl!('a') => self.bol_or_process_mark(),
            key!(End) | ctrl!('e') => self.caret = self.input.chars().count(),
            key!(Left) | ctrl!('b') => self.caret = self.caret.saturating_sub(1),
            key!(Right) | ctrl!('f') => {
                self.caret = (self.caret + 1).min(self.input.chars().count());
            }
            key!(Backspace) => self.backspace(),
            // C-k — kill from point to end of the input line (Emacs `kill-line`).
            ctrl!('k') => {
                let start = char_index_to_byte(&self.input, self.caret);
                self.input.truncate(start);
                self.ring.reset();
            }
            // M-. — comint-insert-previous-argument (!$).
            alt!('.') => self.insert_previous_argument(),
            // C-c — enter the comint job-control prefix.
            ctrl!('c') => self.pending_ctrl_c = true,
            ctrl!('d') => {
                self.delchar_or_maybe_eof();
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

        let lines = self
            .scrollback
            .lock()
            .map(|sb| sb.lines.clone())
            .unwrap_or_default();
        // Tail-follow, offset upward by `scroll`.
        let total = lines.len();
        // Resolve a pending "put this line at the top" request now that the body
        // height is known (comint-show-output / next-prompt / previous-prompt).
        if let Some(top) = self.pending_top.take() {
            let end = (top + body_rows).min(total);
            self.scroll = total.saturating_sub(end);
        }
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
