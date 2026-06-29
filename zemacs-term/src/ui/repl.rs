//! Interactive REPL panel for the embedded scripting languages.
//!
//! A modal, full-screen [`Component`] (same overlay pattern as [`crate::ui::help`])
//! fronting all five embedded interpreters — **elisp, vimscript, stryke, awk, zsh** —
//! behind one read-eval-print loop. Type an expression, press Enter, and the result
//! is appended to a scrollback transcript; `Tab` cycles the active language so the
//! single panel serves as a REPL for each. Per-language input history persists to
//! `~/.zemacs/repl-history.toml`.
//!
//! Open: `SPC a r` · `:repl [lang]`. Enter evaluates · Alt-Enter inserts a newline ·
//! Tab/Shift-Tab switch language · ↑/↓ or C-p/C-n browse history · C-l clears the
//! transcript · PgUp/PgDn scroll · Esc closes.

use serde::{Deserialize, Serialize};
use tui::buffer::Buffer as Surface;
use zemacs_core::Position;
use zemacs_view::{
    document::Mode,
    graphics::{CursorKind, Rect},
    input::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind},
};

use crate::compositor::{Component, Compositor, Context, Event, EventResult};

/// One of the five embedded scripting languages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplLang {
    Elisp,
    Viml,
    Stryke,
    Awk,
    Zsh,
}

impl ReplLang {
    /// Every language, in the order `Tab` cycles them.
    pub const ALL: [ReplLang; 5] = [
        ReplLang::Elisp,
        ReplLang::Viml,
        ReplLang::Stryke,
        ReplLang::Awk,
        ReplLang::Zsh,
    ];

    /// Short lowercase name (also the `:repl <name>` argument).
    pub fn label(self) -> &'static str {
        match self {
            ReplLang::Elisp => "elisp",
            ReplLang::Viml => "viml",
            ReplLang::Stryke => "stryke",
            ReplLang::Awk => "awk",
            ReplLang::Zsh => "zsh",
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
    /// printed result. Each arm maps to the matching scripting host entry point.
    fn eval(self, cx: &mut Context, src: &str) -> Result<String, String> {
        use crate::commands::scripting as s;
        match self {
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
        }
    }
}

/// One evaluated input together with its result.
struct ReplEntry {
    lang: ReplLang,
    input: String,
    output: String,
    is_error: bool,
}

/// Per-language input history, persisted to `~/.zemacs/repl-history.toml`.
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
}

/// Most history entries we keep per language.
const HISTORY_CAP: usize = 200;

impl History {
    fn path() -> std::path::PathBuf {
        zemacs_loader::config_dir().join("repl-history.toml")
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
        }
    }

    fn list_mut(&mut self, lang: ReplLang) -> &mut Vec<String> {
        match lang {
            ReplLang::Elisp => &mut self.elisp,
            ReplLang::Viml => &mut self.viml,
            ReplLang::Stryke => &mut self.stryke,
            ReplLang::Awk => &mut self.awk,
            ReplLang::Zsh => &mut self.zsh,
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
    scroll: u16,    // top transcript line shown
    follow: bool,   // stick to the bottom as new output arrives
    history: History,
    /// `None` = editing fresh input; `Some(i)` = browsing history at index `i`.
    hist_idx: Option<usize>,
    /// In-progress input stashed while browsing history.
    stash: Vec<char>,
    /// Caret position computed during `render`, consumed by `cursor`.
    caret: Option<Position>,
    /// Language tab hit regions: `(x0, x1, row, lang_index)`.
    tab_hits: Vec<(u16, u16, u16, usize)>,
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
        let (output, is_error) = match lang.eval(cx, &src) {
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
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Paragraph;

        self.tab_hits.clear();

        let theme = &ctx.editor.theme;
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let prompt_style = to_rat_style(theme.get("keyword")).add_modifier(RMod::BOLD);
        let err_style = to_rat_style(theme.get("error"));
        surface.clear_with(area, theme.get("ui.background"));

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
        let mut x = area.x + 8;
        for (i, lang) in ReplLang::ALL.iter().enumerate() {
            let lbl = format!(" {} ", lang.label());
            let w = lbl.chars().count() as u16;
            if x + w >= area.x + area.width {
                break;
            }
            let st = if *lang == self.lang {
                accent.add_modifier(RMod::REVERSED)
            } else {
                dim
            };
            render(Paragraph::new(Span::styled(lbl, st)), Rect::new(x, area.y, w, 1), surface);
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
        render(
            Paragraph::new(lines).scroll((self.scroll, 0)),
            Rect::new(area.x + 1, body_y, body_w, body_h),
            surface,
        );

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

    fn cursor(&self, _area: Rect, editor: &zemacs_view::editor::Editor) -> (Option<Position>, CursorKind) {
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
    fn lang_cycles_through_all_five() {
        let mut l = ReplLang::Elisp;
        let mut seen = Vec::new();
        for _ in 0..ReplLang::ALL.len() {
            seen.push(l);
            l = l.next();
        }
        assert_eq!(l, ReplLang::Elisp, "cycle wraps back to the start");
        assert_eq!(seen.len(), 5);
        assert_eq!(seen, ReplLang::ALL.to_vec());
    }

    #[test]
    fn from_name_resolves_aliases() {
        assert_eq!(ReplLang::from_name("el"), Some(ReplLang::Elisp));
        assert_eq!(ReplLang::from_name("VimScript"), Some(ReplLang::Viml));
        assert_eq!(ReplLang::from_name("stryke"), Some(ReplLang::Stryke));
        assert_eq!(ReplLang::from_name("st"), Some(ReplLang::Stryke));
        assert_eq!(ReplLang::from_name("shell"), Some(ReplLang::Zsh));
        assert_eq!(ReplLang::from_name("cobol"), None);
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
        assert_eq!(h.list(ReplLang::Elisp).last().unwrap(), &format!("expr {}", HISTORY_CAP + 9));
    }
}
