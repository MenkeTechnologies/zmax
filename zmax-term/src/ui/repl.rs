//! Interactive REPL panel for the embedded scripting languages.
//!
//! A modal, full-screen [`Component`] (same overlay pattern as [`crate::ui::help`])
//! fronting all ten languages — nine embedded interpreters (**elisp, vimscript,
//! stryke, awk, zsh, ruby, php, python, arb**) plus an external **node** session —
//! behind one read-eval-print loop. Type an
//! expression, press Enter, and the result
//! is appended to a scrollback transcript; `Tab` cycles the active language so the
//! single panel serves as a REPL for each. Per-language input history persists to
//! `~/.zmax/repl-history.toml`.
//!
//! Open: `SPC a r` · `:repl [lang]`. Enter evaluates · Alt-Enter inserts a newline ·
//! Tab/Shift-Tab switch language · ↑/↓ or C-p/C-n browse history · C-l clears the
//! transcript · PgUp/PgDn scroll · Esc closes.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tui::buffer::Buffer as Surface;
use zmax_core::Position;
use zmax_view::{
    document::Mode,
    graphics::{CursorKind, Rect},
    input::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind},
};

use crate::compositor::{Component, Compositor, Context, Event, EventResult};

/// One of the embedded scripting languages, plus the external node session.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplLang {
    Elisp,
    Viml,
    Stryke,
    Awk,
    Zsh,
    Node,
    Ruby,
    Php,
    Python,
    Arb,
}

impl ReplLang {
    /// Every language, in the order `Tab` cycles them.
    pub const ALL: [ReplLang; 10] = [
        ReplLang::Elisp,
        ReplLang::Viml,
        ReplLang::Stryke,
        ReplLang::Awk,
        ReplLang::Zsh,
        ReplLang::Node,
        ReplLang::Ruby,
        ReplLang::Php,
        ReplLang::Python,
        ReplLang::Arb,
    ];

    /// Short lowercase name (also the `:repl <name>` argument).
    pub fn label(self) -> &'static str {
        match self {
            ReplLang::Elisp => "elisp",
            ReplLang::Viml => "viml",
            ReplLang::Stryke => "stryke",
            ReplLang::Awk => "awk",
            ReplLang::Zsh => "zsh",
            ReplLang::Node => "node",
            ReplLang::Ruby => "ruby",
            ReplLang::Php => "php",
            ReplLang::Python => "python",
            ReplLang::Arb => "arb",
        }
    }

    /// Resolve a language from a `:repl <name>` argument (accepts a few aliases).
    pub fn from_name(s: &str) -> Option<ReplLang> {
        match s.trim().to_lowercase().as_str() {
            "elisp" | "el" | "emacs-lisp" | "lisp" => Some(ReplLang::Elisp),
            "viml" | "vim" | "vimscript" => Some(ReplLang::Viml),
            "stryke" | "st" | "stk" => Some(ReplLang::Stryke),
            "awk" => Some(ReplLang::Awk),
            "zsh" | "sh" | "shell" => Some(ReplLang::Zsh),
            "node" | "nodejs" | "js" | "javascript" => Some(ReplLang::Node),
            "ruby" | "rb" => Some(ReplLang::Ruby),
            "php" => Some(ReplLang::Php),
            "python" | "py" => Some(ReplLang::Python),
            "arb" => Some(ReplLang::Arb),
            _ => None,
        }
    }

    fn next(self) -> ReplLang {
        let i = Self::ALL.iter().position(|&l| l == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn prev(self) -> ReplLang {
        let i = Self::ALL.iter().position(|&l| l == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Evaluate `src` through this language against the live editor, returning the
    /// printed result. Each arm maps to the matching scripting host entry point;
    /// `node` is the odd one out — it is an out-of-process interpreter, so it needs
    /// the panel's long-lived [`NodeSession`] (spawned on first use) passed in.
    fn eval(
        self,
        cx: &mut Context,
        src: &str,
        node: &mut Option<NodeSession>,
    ) -> Result<String, String> {
        use crate::commands::scripting as s;
        match self {
            ReplLang::Node => {
                let session = match node {
                    Some(s) => s,
                    None => node.insert(NodeSession::spawn(&node_project_dir(cx))?),
                };
                session.eval(src)
            }
            ReplLang::Elisp => s::eval_elisp(cx, src),
            ReplLang::Viml => s::eval_viml(cx, src),
            ReplLang::Stryke => s::eval_stryke(cx, src),
            ReplLang::Awk => s::repl_awk(cx, src),
            ReplLang::Zsh => match s::run_zsh(src) {
                Ok((0, out)) => Ok(out.trim_end().to_string()),
                Ok((status, out)) if out.trim().is_empty() => Ok(format!("[exit {status}]")),
                Ok((status, out)) => Ok(format!("{}\n[exit {status}]", out.trim_end())),
                Err(e) => Err(e),
            },
            ReplLang::Ruby => s::eval_ruby(cx, src),
            ReplLang::Php => s::eval_php(cx, src),
            ReplLang::Python => s::eval_python(cx, src),
            ReplLang::Arb => s::repl_arb(cx, src),
        }
    }
}

/// Marker expression appended after every node input. Node's REPL echoes it back
/// as `'…'`, which tells the reader "this evaluation is finished" without having
/// to count `> ` / `| ` prompts (a multi-line paste emits an unpredictable number).
const NODE_SENTINEL: &str = "__zmax_repl_eoe__";

/// How long a single node evaluation may run before we give up and `.break` it.
const NODE_EVAL_TIMEOUT: Duration = Duration::from_secs(15);

/// Directory the node session runs in: the current buffer's directory, else the
/// terminal's cwd. This is what `node_modules/.bin` lookup walks up from.
fn node_project_dir(cx: &mut Context) -> PathBuf {
    let fallback = || std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    doc!(cx.editor)
        .path()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(fallback)
}

/// Spacemacs' `add-node-modules-path`: walk up from `dir` and prepend every
/// `node_modules/.bin` found to `PATH`, nearest first, so project-local `eslint`,
/// `tsc`, `prettier` &c. win over anything installed globally.
fn node_modules_path(dir: &Path) -> std::ffi::OsString {
    let mut bins: Vec<PathBuf> = Vec::new();
    for ancestor in dir.ancestors() {
        let bin = ancestor.join("node_modules").join(".bin");
        if bin.is_dir() {
            bins.push(bin);
        }
    }
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    bins.extend(std::env::split_paths(&inherited));
    std::env::join_paths(bins).unwrap_or(inherited)
}

/// A live `node -i` child process. Node is not embeddable the way elisp/awk/zsh
/// are, so the node tab drives the real interpreter over pipes — which is also
/// what makes it a REPL in the `nodejs-repl` sense: `var x = 1` on one line is
/// still in scope on the next, exactly like an interactive `node` session.
struct NodeSession {
    child: Child,
    stdin: ChildStdin,
    /// Merged stdout+stderr lines. Node's REPL prints results *and* `Uncaught …`
    /// on stdout; only explicit `console.error` / process writes reach stderr, so
    /// the interleaving is stable enough to read as one stream.
    rx: Receiver<String>,
}

impl NodeSession {
    fn spawn(dir: &Path) -> Result<NodeSession, String> {
        let mut child = Command::new("node")
            .arg("-i")
            .current_dir(dir)
            .env("PATH", node_modules_path(dir))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("node: {e}"))?;

        let stdin = child.stdin.take().ok_or("node: no stdin")?;
        let (tx, rx) = mpsc::channel();
        for pipe in [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let tx = tx.clone();
            std::thread::spawn(move || {
                // Read by line; the trailing prompt has no newline, so also flush
                // whatever partial text is buffered when the pipe goes quiet.
                let mut reader = BufReader::new(pipe);
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if tx.send(std::mem::take(&mut buf)).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }

        let mut session = NodeSession { child, stdin, rx };
        // Swallow the "Welcome to Node.js" banner so the first result is clean.
        session.eval("")?;
        Ok(session)
    }

    /// Send `src`, then the sentinel, and collect everything printed in between.
    fn eval(&mut self, src: &str) -> Result<String, String> {
        writeln!(self.stdin, "{src}").map_err(|e| format!("node: {e}"))?;
        writeln!(self.stdin, "{NODE_SENTINEL:?}").map_err(|e| format!("node: {e}"))?;
        self.stdin.flush().map_err(|e| format!("node: {e}"))?;

        let marker = format!("'{NODE_SENTINEL}'");
        let deadline = Instant::now() + NODE_EVAL_TIMEOUT;
        let mut raw = String::new();
        loop {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                // Almost always an unterminated block eating the sentinel as a
                // continuation line; `.break` drops back to the top-level prompt.
                let _ = writeln!(self.stdin, ".break");
                let _ = self.stdin.flush();
                return Err("node: evaluation timed out".to_string());
            };
            match self.rx.recv_timeout(left) {
                Ok(chunk) => raw.push_str(&chunk),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("node: session exited".to_string())
                }
            }
            if let Some(end) = raw.find(&marker) {
                raw.truncate(end);
                break;
            }
        }
        let out = clean_node_output(&raw);
        // Node prints throws as `Uncaught …` on stdout rather than failing; surface
        // those through `Err` so the transcript styles them as errors.
        if out.lines().any(|l| l.starts_with("Uncaught ")) {
            return Err(out);
        }
        Ok(out)
    }
}

impl Drop for NodeSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Strip node's REPL decoration: the `> ` primary and `| ` continuation prompts
/// that prefix each line, plus the banner and surrounding blank lines.
fn clean_node_output(raw: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let mut l = line;
        while let Some(rest) = l.strip_prefix("> ").or_else(|| l.strip_prefix("| ")) {
            l = rest;
        }
        let l = l.trim_end();
        if l.starts_with("Welcome to Node.js") || l.starts_with("Type \".help\"") {
            continue;
        }
        out.push(l);
    }
    while out.first().is_some_and(|l| l.is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// One evaluated input together with its result.
struct ReplEntry {
    lang: ReplLang,
    input: String,
    output: String,
    is_error: bool,
}

/// Per-language input history, persisted to `~/.zmax/repl-history.toml`.
#[derive(Default, Serialize, Deserialize)]
struct History {
    #[serde(default)]
    elisp: Vec<String>,
    #[serde(default)]
    viml: Vec<String>,
    #[serde(default)]
    stryke: Vec<String>,
    #[serde(default)]
    awk: Vec<String>,
    #[serde(default)]
    zsh: Vec<String>,
    #[serde(default)]
    node: Vec<String>,
    #[serde(default)]
    ruby: Vec<String>,
    #[serde(default)]
    php: Vec<String>,
    #[serde(default)]
    python: Vec<String>,
    #[serde(default)]
    arb: Vec<String>,
}

/// Most history entries we keep per language.
const HISTORY_CAP: usize = 200;

impl History {
    fn path() -> std::path::PathBuf {
        zmax_loader::config_dir().join("repl-history.toml")
    }

    fn load() -> History {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if let Ok(s) = toml::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), s);
        }
    }

    fn list(&self, lang: ReplLang) -> &Vec<String> {
        match lang {
            ReplLang::Elisp => &self.elisp,
            ReplLang::Viml => &self.viml,
            ReplLang::Stryke => &self.stryke,
            ReplLang::Awk => &self.awk,
            ReplLang::Zsh => &self.zsh,
            ReplLang::Node => &self.node,
            ReplLang::Ruby => &self.ruby,
            ReplLang::Php => &self.php,
            ReplLang::Python => &self.python,
            ReplLang::Arb => &self.arb,
        }
    }

    fn list_mut(&mut self, lang: ReplLang) -> &mut Vec<String> {
        match lang {
            ReplLang::Elisp => &mut self.elisp,
            ReplLang::Viml => &mut self.viml,
            ReplLang::Stryke => &mut self.stryke,
            ReplLang::Awk => &mut self.awk,
            ReplLang::Zsh => &mut self.zsh,
            ReplLang::Node => &mut self.node,
            ReplLang::Ruby => &mut self.ruby,
            ReplLang::Php => &mut self.php,
            ReplLang::Python => &mut self.python,
            ReplLang::Arb => &mut self.arb,
        }
    }

    /// Record `entry` as the most recent line for `lang` (skips consecutive dups).
    fn push(&mut self, lang: ReplLang, entry: &str) {
        let list = self.list_mut(lang);
        if list.last().map(String::as_str) == Some(entry) {
            return;
        }
        list.push(entry.to_string());
        let overflow = list.len().saturating_sub(HISTORY_CAP);
        if overflow > 0 {
            list.drain(0..overflow);
        }
    }
}

pub struct ReplPanel {
    lang: ReplLang,
    input: Vec<char>,
    cursor: usize, // char index into `input`
    transcript: Vec<ReplEntry>,
    scroll: u16,  // top transcript line shown
    follow: bool, // stick to the bottom as new output arrives
    history: History,
    /// `None` = editing fresh input; `Some(i)` = browsing history at index `i`.
    hist_idx: Option<usize>,
    /// In-progress input stashed while browsing history.
    stash: Vec<char>,
    /// Caret position computed during `render`, consumed by `cursor`.
    caret: Option<Position>,
    /// Language tab hit regions: `(x0, x1, row, lang_index)`.
    tab_hits: Vec<(u16, u16, u16, usize)>,
    /// The `node -i` child, spawned on the first node evaluation and kept alive
    /// (so bindings persist) until the panel closes.
    node: Option<NodeSession>,
}

impl ReplPanel {
    pub fn new(lang: ReplLang) -> Self {
        Self {
            lang,
            input: Vec::new(),
            cursor: 0,
            transcript: Vec::new(),
            scroll: 0,
            follow: true,
            history: History::load(),
            hist_idx: None,
            stash: Vec::new(),
            caret: None,
            tab_hits: Vec::new(),
            node: None,
        }
    }

    fn input_string(&self) -> String {
        self.input.iter().collect()
    }

    fn set_input(&mut self, s: &str) {
        self.input = s.chars().collect();
        self.cursor = self.input.len();
    }

    fn switch_lang(&mut self, lang: ReplLang) {
        self.lang = lang;
        self.hist_idx = None;
    }

    /// Evaluate the current input line, append the result, and reset for the next.
    fn submit(&mut self, cx: &mut Context) {
        let src = self.input_string();
        if src.trim().is_empty() {
            return;
        }
        let lang = self.lang;
        let (output, is_error) = match lang.eval(cx, &src, &mut self.node) {
            Ok(out) => (out, false),
            Err(err) => (err, true),
        };
        self.history.push(lang, &src);
        self.history.save();
        self.transcript.push(ReplEntry {
            lang,
            input: src,
            output,
            is_error,
        });
        self.input.clear();
        self.cursor = 0;
        self.hist_idx = None;
        self.follow = true;
    }

    /// Replace the input with an older/newer history line for the current language.
    fn history_move(&mut self, older: bool) {
        let len = self.history.list(self.lang).len();
        if len == 0 {
            return;
        }
        let new_idx = match (self.hist_idx, older) {
            (None, true) => {
                self.stash = self.input.clone();
                Some(len - 1)
            }
            (None, false) => return,
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) => {
                if i + 1 < len {
                    Some(i + 1)
                } else {
                    // past the newest entry → restore the stashed line
                    self.hist_idx = None;
                    let stash = self.stash.clone();
                    self.input = stash;
                    self.cursor = self.input.len();
                    return;
                }
            }
        };
        self.hist_idx = new_idx;
        if let Some(i) = new_idx {
            let line = self.history.list(self.lang)[i].clone();
            self.set_input(&line);
        }
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += 1;
        self.hist_idx = None;
    }

    fn delete_back(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.input.remove(self.cursor);
            self.hist_idx = None;
        }
    }

    fn delete_word_back(&mut self) {
        let start = self.cursor;
        while self.cursor > 0 && self.input[self.cursor - 1].is_whitespace() {
            self.cursor -= 1;
        }
        while self.cursor > 0 && !self.input[self.cursor - 1].is_whitespace() {
            self.cursor -= 1;
        }
        self.input.drain(self.cursor..start);
        self.hist_idx = None;
    }

    fn close() -> EventResult {
        EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
            c.pop();
        })))
    }

    fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) -> EventResult {
        match kind {
            MouseEventKind::ScrollUp => {
                self.scroll = self.scroll.saturating_sub(2);
                self.follow = false;
            }
            MouseEventKind::ScrollDown => {
                self.scroll = self.scroll.saturating_add(2);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(&(_, _, _, li)) = self
                    .tab_hits
                    .iter()
                    .find(|&&(x0, x1, r, _)| row == r && col >= x0 && col < x1)
                {
                    self.switch_lang(ReplLang::ALL[li]);
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }
}

impl Component for ReplPanel {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key: KeyEvent = match event {
            Event::Key(k) => *k,
            Event::Mouse(ev) => return self.handle_mouse(ev.column, ev.row, ev.kind),
            Event::Paste(s) => {
                for c in s.chars() {
                    self.insert_char(c);
                }
                return EventResult::Consumed(None);
            }
            _ => return EventResult::Ignored(None),
        };

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            KeyCode::Esc => return Self::close(),
            KeyCode::Char('c') if ctrl => return Self::close(),
            KeyCode::Char('g') if ctrl => {
                // Abort the current input line (keep the panel open).
                self.input.clear();
                self.cursor = 0;
                self.hist_idx = None;
            }
            KeyCode::Enter if alt => self.insert_char('\n'),
            KeyCode::Enter => self.submit(cx),
            KeyCode::Tab => {
                let l = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.lang.prev()
                } else {
                    self.lang.next()
                };
                self.switch_lang(l);
            }
            KeyCode::Up => self.history_move(true),
            KeyCode::Down => self.history_move(false),
            KeyCode::Char('p') if ctrl => self.history_move(true),
            KeyCode::Char('n') if ctrl => self.history_move(false),
            KeyCode::Char('l') if ctrl => {
                self.transcript.clear();
                self.scroll = 0;
                self.follow = true;
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Char('b') if ctrl => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.input.len()),
            KeyCode::Char('f') if ctrl => self.cursor = (self.cursor + 1).min(self.input.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.input.len(),
            KeyCode::Char('k') if ctrl => {
                self.input.truncate(self.cursor);
                self.hist_idx = None;
            }
            KeyCode::Char('u') if ctrl => {
                self.input.drain(0..self.cursor);
                self.cursor = 0;
                self.hist_idx = None;
            }
            KeyCode::Char('w') if ctrl => self.delete_word_back(),
            KeyCode::Backspace if alt => self.delete_word_back(),
            KeyCode::Backspace => self.delete_back(),
            KeyCode::Delete => {
                if self.cursor < self.input.len() {
                    self.input.remove(self.cursor);
                    self.hist_idx = None;
                }
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(5);
                self.follow = false;
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(5),
            KeyCode::Char(c) if !ctrl => self.insert_char(c),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, render_stateful, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs};

        self.tab_hits.clear();

        let theme = &ctx.editor.theme;
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let prompt_style = to_rat_style(theme.get("keyword")).add_modifier(RMod::BOLD);
        let err_style = to_rat_style(theme.get("error"));
        // `transparent-background`: drop the page fill so the terminal shows through.
        let mut page_bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            page_bg.bg = None;
        }
        surface.clear_with(area, page_bg);

        if area.height < 5 || area.width < 16 {
            self.caret = None;
            return;
        }

        // ── title bar: " REPL " + per-language tabs ──────────────────────────
        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        render(
            Paragraph::new(Span::styled(" REPL ", accent)),
            Rect::new(area.x + 1, area.y, 7, 1),
            surface,
        );
        let tabs_x = area.x + 8;
        let selected = ReplLang::ALL
            .iter()
            .position(|&l| l == self.lang)
            .unwrap_or(0);
        let titles: Vec<Line> = ReplLang::ALL
            .iter()
            .map(|l| Line::from(l.label()))
            .collect();
        let tabs = Tabs::new(titles)
            .select(selected)
            .style(dim)
            .highlight_style(accent.add_modifier(RMod::REVERSED))
            .divider(Span::styled("│", dim))
            .padding(" ", " ");
        render(
            tabs,
            Rect::new(
                tabs_x,
                area.y,
                area.width.saturating_sub(tabs_x - area.x),
                1,
            ),
            surface,
        );
        // Mirror the Tabs geometry for mouse hit-testing: each tab renders as
        // " {label} " (1-col padding each side); a 1-col divider follows all but
        // the last.
        let mut x = tabs_x;
        for (i, lang) in ReplLang::ALL.iter().enumerate() {
            let w = lang.label().chars().count() as u16 + 2;
            if x + w > area.x + area.width {
                break;
            }
            self.tab_hits.push((x, x + w, area.y, i));
            x += w + 1;
        }

        // ── geometry ─────────────────────────────────────────────────────────
        let prompt = format!("{}❯ ", self.lang.label());
        let prompt_w = prompt.chars().count() as u16;
        let footer_y = area.y + area.height - 1;
        let input_lines = self.input.iter().collect::<String>();
        let n_in = input_lines.split('\n').count().max(1) as u16;
        let input_h = n_in.clamp(1, 6);
        let input_y = footer_y.saturating_sub(input_h);
        let sep_y = input_y.saturating_sub(1);
        let body_y = area.y + 1;
        let body_h = sep_y.saturating_sub(body_y);
        let body_w = area.width.saturating_sub(2);

        // ── transcript ───────────────────────────────────────────────────────
        let mut lines: Vec<Line> = Vec::new();
        for e in &self.transcript {
            let glyph = format!("{}❯ ", e.lang.label());
            for (i, l) in e.input.split('\n').enumerate() {
                let head = if i == 0 {
                    Span::styled(glyph.clone(), prompt_style)
                } else {
                    Span::styled(" ".repeat(glyph.chars().count()), dim)
                };
                lines.push(Line::from(vec![head, Span::styled(l.to_string(), text)]));
            }
            let out_style = if e.is_error { err_style } else { dim };
            if e.output.is_empty() {
                lines.push(Line::from(Span::styled("·", dim)));
            } else {
                for l in e.output.split('\n') {
                    lines.push(Line::from(Span::styled(l.to_string(), out_style)));
                }
            }
        }

        let total = lines.len() as u16;
        if self.follow {
            self.scroll = total.saturating_sub(body_h);
        } else {
            self.scroll = self.scroll.min(total.saturating_sub(body_h));
        }
        // Reserve the last body column for a scrollbar when the transcript
        // overflows the viewport.
        let overflow = total > body_h && body_h > 0;
        let text_w = if overflow {
            body_w.saturating_sub(1)
        } else {
            body_w
        };
        render(
            Paragraph::new(lines).scroll((self.scroll, 0)),
            Rect::new(area.x + 1, body_y, text_w, body_h),
            surface,
        );
        if overflow {
            let mut sb_state = ScrollbarState::new(total as usize)
                .viewport_content_length(body_h as usize)
                .position(self.scroll as usize);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .track_symbol(Some("║"))
                .thumb_symbol("█")
                .style(dim);
            render_stateful(
                scrollbar,
                Rect::new(area.x + area.width - 1, body_y, 1, body_h),
                surface,
                &mut sb_state,
            );
        }

        // ── separator ────────────────────────────────────────────────────────
        render(
            Paragraph::new(Span::styled("─".repeat(area.width as usize), dim)),
            Rect::new(area.x, sep_y, area.width, 1),
            surface,
        );

        // ── input line(s) ─────────────────────────────────────────────────────
        for (i, l) in input_lines.split('\n').enumerate().take(input_h as usize) {
            let y = input_y + i as u16;
            let head = if i == 0 {
                Span::styled(prompt.clone(), prompt_style)
            } else {
                Span::styled(" ".repeat(prompt_w as usize), dim)
            };
            render(
                Paragraph::new(Line::from(vec![head, Span::styled(l.to_string(), text)])),
                Rect::new(area.x + 1, y, body_w, 1),
                surface,
            );
        }

        // ── footer hint ────────────────────────────────────────────────────────
        render(
            Paragraph::new(Span::styled(
                " Enter eval · Alt-Enter newline · Tab lang · ↑/↓ history · C-l clear · Esc close",
                dim,
            )),
            Rect::new(area.x + 1, footer_y, body_w, 1),
            surface,
        );

        // ── caret (real terminal cursor in the input) ──────────────────────────
        let before = &self.input[..self.cursor];
        let row = before.iter().filter(|&&c| c == '\n').count() as u16;
        let col = before.iter().rev().take_while(|&&c| c != '\n').count() as u16;
        let crow = (input_y + row).min(footer_y.saturating_sub(1));
        let ccol = area.x + 1 + prompt_w + col;
        self.caret = Some(Position::new(crow as usize, ccol as usize));
    }

    fn cursor(
        &self,
        _area: Rect,
        editor: &zmax_view::editor::Editor,
    ) -> (Option<Position>, CursorKind) {
        (
            self.caret,
            editor.config().cursor_shape.from_mode(Mode::Insert),
        )
    }

    fn id(&self) -> Option<&'static str> {
        Some("repl")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_cycles_through_every_tab() {
        let mut l = ReplLang::Elisp;
        let mut seen = Vec::new();
        for _ in 0..ReplLang::ALL.len() {
            seen.push(l);
            l = l.next();
        }
        assert_eq!(l, ReplLang::Elisp, "cycle wraps back to the start");
        assert_eq!(seen, ReplLang::ALL.to_vec());
    }

    #[test]
    fn from_name_resolves_aliases() {
        assert_eq!(ReplLang::from_name("el"), Some(ReplLang::Elisp));
        assert_eq!(ReplLang::from_name("VimScript"), Some(ReplLang::Viml));
        assert_eq!(ReplLang::from_name("stryke"), Some(ReplLang::Stryke));
        assert_eq!(ReplLang::from_name("st"), Some(ReplLang::Stryke));
        assert_eq!(ReplLang::from_name("shell"), Some(ReplLang::Zsh));
        assert_eq!(ReplLang::from_name("nodejs"), Some(ReplLang::Node));
        assert_eq!(ReplLang::from_name("JS"), Some(ReplLang::Node));
        assert_eq!(ReplLang::from_name("rb"), Some(ReplLang::Ruby));
        assert_eq!(ReplLang::from_name("PHP"), Some(ReplLang::Php));
        assert_eq!(ReplLang::from_name("py"), Some(ReplLang::Python));
        assert_eq!(ReplLang::from_name("arb"), Some(ReplLang::Arb));
        assert_eq!(ReplLang::from_name("cobol"), None);
    }

    #[test]
    fn node_output_loses_its_prompts_and_banner() {
        let raw = "Welcome to Node.js v22.0.0.\nType \".help\" for more information.\n\
                   > undefined\n> | 42\n> ";
        assert_eq!(clean_node_output(raw), "undefined\n42");
    }

    #[test]
    fn node_modules_bin_dirs_come_first_nearest_first() {
        let root = std::env::temp_dir().join("zmax-repl-node-path-test");
        let leaf = root.join("pkg").join("src");
        for d in [&root, &root.join("pkg")] {
            std::fs::create_dir_all(d.join("node_modules").join(".bin")).unwrap();
        }
        std::fs::create_dir_all(&leaf).unwrap();

        let path = node_modules_path(&leaf);
        let dirs: Vec<_> = std::env::split_paths(&path).collect();
        assert_eq!(dirs[0], root.join("pkg").join("node_modules").join(".bin"));
        assert_eq!(dirs[1], root.join("node_modules").join(".bin"));
        assert!(
            dirs.len() > 2,
            "the inherited PATH must still follow the project bins"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn history_dedups_and_caps() {
        let mut h = History::default();
        h.push(ReplLang::Elisp, "(+ 1 1)");
        h.push(ReplLang::Elisp, "(+ 1 1)"); // consecutive dup ignored
        assert_eq!(h.list(ReplLang::Elisp).len(), 1);
        for i in 0..HISTORY_CAP + 10 {
            h.push(ReplLang::Elisp, &format!("expr {i}"));
        }
        assert_eq!(h.list(ReplLang::Elisp).len(), HISTORY_CAP);
        assert_eq!(
            h.list(ReplLang::Elisp).last().unwrap(),
            &format!("expr {}", HISTORY_CAP + 9)
        );
    }
}
