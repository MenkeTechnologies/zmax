//! Rmail — the zemacs port of the GNU Emacs `rmail` mail reader.
//!
//! A full-screen [`Component`] over the pure, unit-tested [`zemacs_core::rmail`]
//! `Mailbox`. It shows one message at a time (a pruned header block plus the
//! scrollable body) with a mode line reporting position and the Deleted flag.
//! Keys map to `rmail-mode` (parsed into an `rmail` keymap mode by
//! `scripts/gen_port_report.py`):
//!
//!   n/p         next/previous undeleted message; M-n/M-p include deleted
//!   `<` / `>`   first / last message;  `j`  jump to a typed message number
//!   SPC / DEL   scroll body down / up;  `.` / `/`  top / bottom of message
//!   d / C-d     delete forward / backward;  u undelete;  x expunge
//!   s           expunge and save the mbox;  g  reload the file from disk
//!   t           toggle full vs pruned headers
//!   C-c C-n/C-p next / previous message with the same subject
//!   r / f / m   reply / forward / compose new mail (opens a message-mode draft)
//!   q / Esc     quit the reader

use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zemacs_core::rmail::{forward_fields, reply_fields, Mailbox};
use zemacs_view::graphics::Rect;

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Which pane the reader is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    /// A single message (the default `rmail-mode` view).
    Message,
    /// The `rmail-summary` list, with a movable cursor over `sum`.
    Summary,
}

/// The command an active minibuffer prompt will run once the user hits RET.
/// Rmail reads these arguments in the echo area; the reader reads them inline.
#[derive(Clone, Copy)]
enum PromptAction {
    Search,
    AddLabel,
    KillLabel,
    SummaryByLabel,
    Input,
    Output,
    OutputBody,
}

/// An inline minibuffer: a labelled single-line text field the Component owns,
/// so it can mutate its own state on submit (a layer pushed on top could not).
struct Prompt {
    label: String,
    input: String,
    action: PromptAction,
}

/// The interactive Rmail reader overlay.
pub struct Rmail {
    mailbox: Mailbox,
    path: PathBuf,
    /// Body scroll offset in rendered lines.
    scroll: usize,
    /// Show every header vs the pruned set (`t`, `rmail-toggle-header`).
    full_headers: bool,
    /// Accumulated numeric prefix for `j` (`123 j`).
    count: String,
    status: String,
    /// Message vs summary pane.
    view: View,
    /// Message indices shown in the summary (all, or a label-filtered subset).
    sum: Vec<usize>,
    /// Cursor position within `sum`.
    sum_cursor: usize,
    /// Active inline prompt, if any.
    prompt: Option<Prompt>,
}

/// Headers Rmail shows by default when `full_headers` is off.
const PRUNED: [&str; 5] = ["Date", "From", "To", "Cc", "Subject"];

impl Rmail {
    pub fn new(mailbox: Mailbox, path: PathBuf) -> Self {
        Rmail {
            mailbox,
            path,
            scroll: 0,
            full_headers: false,
            count: String::new(),
            status: String::new(),
            view: View::Message,
            sum: Vec::new(),
            sum_cursor: 0,
            prompt: None,
        }
    }

    /// One `rmail-summary` row: number, Deleted flag, sender, subject.
    fn summary_line(&self, idx: usize) -> String {
        let m = &self.mailbox.msgs[idx];
        let flag = if m.deleted { 'D' } else { ' ' };
        let from = m.from();
        let from: String = from.chars().take(20).collect();
        format!("{:>4} {} {:<20}  {}", idx + 1, flag, from, m.subject())
    }

    /// The message index the summary cursor points at.
    fn sum_target(&self) -> Option<usize> {
        self.sum.get(self.sum_cursor).copied()
    }

    /// Open the summary over `indices` (all messages for `h`, a filtered subset
    /// for `l`), positioning the cursor on the current message if present.
    fn open_summary(&mut self, indices: Vec<usize>) {
        self.sum_cursor = indices
            .iter()
            .position(|&i| i == self.mailbox.current)
            .unwrap_or(0);
        self.sum = indices;
        self.view = View::Summary;
    }

    /// Handle a key while the summary pane is showing.
    fn summary_key(&mut self, key: zemacs_view::input::KeyEvent) -> EventResult {
        let last = self.sum.len().saturating_sub(1);
        match key {
            // q/Q leave the summary (rmail-summary-quit / rmail-summary-wipe).
            key!('q') | key!('Q') | key!(Esc) => self.view = View::Message,
            key!('n') | key!('j') => self.sum_cursor = (self.sum_cursor + 1).min(last),
            key!('p') | key!('k') => self.sum_cursor = self.sum_cursor.saturating_sub(1),
            key!('<') => self.sum_cursor = 0,
            key!('>') => self.sum_cursor = last,
            // RET / SPC select the message and return to the message view.
            key!(Enter) | key!(' ') => {
                if let Some(idx) = self.sum_target() {
                    self.mailbox.show(idx + 1);
                    self.scroll = 0;
                }
                self.view = View::Message;
            }
            key!('d') => {
                if let Some(idx) = self.sum_target() {
                    self.mailbox.msgs[idx].deleted = true;
                }
                self.sum_cursor = (self.sum_cursor + 1).min(last);
            }
            key!('u') => {
                if let Some(idx) = self.sum_target() {
                    self.mailbox.msgs[idx].deleted = false;
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    /// Feed a key into the active inline prompt. Returns true if the prompt
    /// consumed the key (so the caller stops processing it).
    fn prompt_key(&mut self, key: zemacs_view::input::KeyEvent) -> bool {
        let Some(prompt) = self.prompt.as_mut() else {
            return false;
        };
        match key {
            key!(Esc) => self.prompt = None,
            key!(Enter) => {
                let p = self.prompt.take().unwrap();
                self.run_prompt(p.action, p.input.trim().to_string());
            }
            key!(Backspace) => {
                prompt.input.pop();
            }
            zemacs_view::input::KeyEvent {
                code: zemacs_view::keyboard::KeyCode::Char(c),
                ..
            } => prompt.input.push(c),
            _ => {}
        }
        true
    }

    /// Apply the result of a completed prompt.
    fn run_prompt(&mut self, action: PromptAction, arg: String) {
        if arg.is_empty() && !matches!(action, PromptAction::Search) {
            return;
        }
        match action {
            PromptAction::Search => {
                if self.mailbox.search(&arg) {
                    self.scroll = 0;
                    self.status = format!("found: {arg}");
                } else {
                    self.status = format!("not found: {arg}");
                }
            }
            PromptAction::AddLabel => {
                for label in arg.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    self.mailbox.add_label(label);
                }
            }
            PromptAction::KillLabel => self.mailbox.kill_label(&arg),
            PromptAction::SummaryByLabel => {
                let hits: Vec<usize> = self
                    .mailbox
                    .msgs
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.labels.iter().any(|l| l == &arg))
                    .map(|(i, _)| i)
                    .collect();
                if hits.is_empty() {
                    self.status = format!("no messages labeled {arg}");
                } else {
                    self.open_summary(hits);
                }
            }
            PromptAction::Input => {
                let path = expand_tilde(&arg);
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        self.mailbox = Mailbox::from_mbox(&text);
                        self.path = path;
                        self.scroll = 0;
                        self.status = format!("{} messages", self.mailbox.len());
                    }
                    Err(e) => self.status = format!("cannot read {arg}: {e}"),
                }
            }
            PromptAction::Output => {
                let Some(msg) = self.mailbox.current() else {
                    return;
                };
                let entry = one_message_mbox(msg);
                match append_file(&expand_tilde(&arg), &entry) {
                    Ok(()) => self.status = format!("output to {arg}"),
                    Err(e) => self.status = format!("cannot write {arg}: {e}"),
                }
            }
            PromptAction::OutputBody => {
                let Some(msg) = self.mailbox.current() else {
                    return;
                };
                let body = msg.body.clone();
                match std::fs::write(expand_tilde(&arg), body) {
                    Ok(()) => self.status = format!("body written to {arg}"),
                    Err(e) => self.status = format!("cannot write {arg}: {e}"),
                }
            }
        }
    }

    /// Start an inline prompt.
    fn ask(&mut self, label: &str, action: PromptAction) {
        self.prompt = Some(Prompt {
            label: label.to_string(),
            input: String::new(),
            action,
        });
    }

    /// Rendered content lines for the current message (headers, blank, body).
    fn content_lines(&self) -> Vec<String> {
        let Some(msg) = self.mailbox.current() else {
            return vec!["[no message]".to_string()];
        };
        let mut lines = Vec::new();
        if self.full_headers {
            for (k, v) in &msg.headers {
                lines.push(format!("{k}: {v}"));
            }
        } else {
            for name in PRUNED {
                if let Some(v) = msg.header(name) {
                    lines.push(format!("{name}: {v}"));
                }
            }
        }
        if !msg.labels.is_empty() {
            lines.push(format!("Labels: {}", msg.labels.join(", ")));
        }
        lines.push(String::new());
        for line in msg.body.split('\n') {
            lines.push(line.to_string());
        }
        lines
    }

    /// Reload the mbox from disk (`g`, `rmail-get-new-mail`).
    fn reload(&mut self) {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                self.mailbox = Mailbox::from_mbox(&text);
                self.scroll = 0;
                self.status = format!("{} messages", self.mailbox.len());
            }
            Err(e) => self.status = format!("cannot read {}: {e}", self.path.display()),
        }
    }

    /// Write the mailbox back to disk (`s`, after expunge).
    fn save(&mut self) {
        match std::fs::write(&self.path, self.mailbox.to_mbox()) {
            Ok(()) => self.status = format!("saved {}", self.path.display()),
            Err(e) => self.status = format!("cannot write {}: {e}", self.path.display()),
        }
    }

    /// Build the callback that pops the reader and opens a message-mode draft.
    fn compose(&self, to: String, subject: String, body: String) -> Callback {
        Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            compositor.pop();
            crate::commands::typed::open_mail_draft(cx, &to, &subject, &body);
        })
    }
}

impl Component for Rmail {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        self.status.clear();
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        // An active inline prompt swallows keys until RET/Esc.
        if self.prompt.is_some() {
            self.prompt_key(key);
            return EventResult::Consumed(None);
        }

        // The summary pane has its own key handling.
        if self.view == View::Summary {
            return self.summary_key(key);
        }

        // Accumulate a numeric prefix for `j`.
        if let key!(c @ '0'..='9') = key {
            self.count.push(c);
            return EventResult::Consumed(None);
        }

        const STEP: usize = 10; // body scroll step in lines
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),

            // Motion.
            key!('n') => {
                self.mailbox.next(true);
                self.scroll = 0;
            }
            key!('p') => {
                self.mailbox.prev(true);
                self.scroll = 0;
            }
            alt!('n') => {
                self.mailbox.next(false);
                self.scroll = 0;
            }
            alt!('p') => {
                self.mailbox.prev(false);
                self.scroll = 0;
            }
            key!('<') => {
                self.mailbox.first();
                self.scroll = 0;
            }
            key!('>') => {
                self.mailbox.last();
                self.scroll = 0;
            }
            key!('j') => {
                if let Ok(num) = self.count.parse::<usize>() {
                    self.mailbox.show(num);
                    self.scroll = 0;
                }
            }
            // Same-subject motion. Rmail binds these to C-c C-n / C-c C-p, but
            // C-c is the quit chord here, so N/P are the reachable aliases.
            key!('N') => {
                self.mailbox.next_same_subject();
                self.scroll = 0;
            }
            key!('P') => {
                self.mailbox.prev_same_subject();
                self.scroll = 0;
            }

            // Scrolling.
            key!(' ') => self.scroll += STEP,
            key!(Backspace) | key!(Delete) => self.scroll = self.scroll.saturating_sub(STEP),
            key!('.') => self.scroll = 0,
            key!('/') => self.scroll = usize::MAX / 2, // clamped in render

            // Deletion.
            key!('d') => {
                self.mailbox.delete_forward();
                self.scroll = 0;
            }
            ctrl!('d') => {
                self.mailbox.delete_backward();
                self.scroll = 0;
            }
            key!('u') => self.mailbox.undelete(),
            key!('x') => {
                self.mailbox.expunge();
                self.scroll = 0;
            }
            key!('s') => {
                self.mailbox.expunge();
                self.save();
                self.scroll = 0;
            }
            key!('g') => self.reload(),

            // Display.
            key!('t') => self.full_headers = !self.full_headers,
            key!('h') => self.open_summary((0..self.mailbox.len()).collect()),

            // Labels, search, file output/input — all read an argument inline.
            key!('a') => self.ask("Add label: ", PromptAction::AddLabel),
            key!('k') => self.ask("Kill label: ", PromptAction::KillLabel),
            key!('l') => self.ask("Labels to summarize by: ", PromptAction::SummaryByLabel),
            alt!('s') => self.ask("Search: ", PromptAction::Search),
            key!('i') => self.ask("Run rmail on file: ", PromptAction::Input),
            key!('o') => self.ask("Output message to file: ", PromptAction::Output),
            key!('w') => self.ask("Output body to file: ", PromptAction::OutputBody),

            // Reply / forward / new mail — open a message-mode draft.
            key!('r') => {
                if let Some((to, subject, body)) = self.mailbox.current().map(reply_fields) {
                    return EventResult::Consumed(Some(self.compose(to, subject, body)));
                }
            }
            key!('f') => {
                if let Some((to, subject, body)) = self.mailbox.current().map(forward_fields) {
                    return EventResult::Consumed(Some(self.compose(to, subject, body)));
                }
            }
            key!('m') => {
                return EventResult::Consumed(Some(self.compose(
                    String::new(),
                    String::new(),
                    String::new(),
                )));
            }

            _ => {}
        }
        self.count.clear();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let field_style = theme.get("ui.selection");
        let del_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 16 || area.height < 4 {
            return;
        }

        if self.view == View::Summary {
            let shown = self.sum.len();
            let title = format!(" RMAIL-summary  {shown} messages");
            surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
            let hint = "n/p move  RET select  d del  u undel  q back";
            if title.len() + hint.len() + 3 < area.width as usize {
                surface.set_stringn(
                    area.x + area.width - hint.len() as u16 - 1,
                    area.y,
                    hint,
                    hint.len(),
                    info_style,
                );
            }
            let rows = area.height.saturating_sub(2) as usize;
            let top = self
                .sum_cursor
                .saturating_sub(rows / 2)
                .min(shown.saturating_sub(rows));
            for (row, pos) in (top..shown).take(rows).enumerate() {
                let idx = self.sum[pos];
                let y = area.y + 2 + row as u16;
                let style = if pos == self.sum_cursor {
                    field_style
                } else if self.mailbox.msgs[idx].deleted {
                    del_style
                } else {
                    text_style
                };
                surface.set_stringn(
                    area.x,
                    y,
                    &self.summary_line(idx),
                    area.width as usize,
                    style,
                );
            }
            self.render_prompt(area, surface, info_style);
            return;
        }

        // Mode line.
        let total = self.mailbox.len();
        let cur = if total == 0 {
            0
        } else {
            self.mailbox.current + 1
        };
        let deleted = self.mailbox.current().map(|m| m.deleted).unwrap_or(false);
        let subject = self.mailbox.current().map(|m| m.subject()).unwrap_or("");
        let mode = format!(
            " RMAIL  {cur}/{total}  {} undeleted{}  {subject}",
            self.mailbox.undeleted_count(),
            if deleted { "  [DELETED]" } else { "" },
        );
        surface.set_stringn(
            area.x,
            area.y,
            &mode,
            area.width as usize,
            if deleted { del_style } else { header_style },
        );

        let hint = "n/p move  d del  u undel  x expunge  r reply  s save  t hdrs  q quit";
        if mode.len() + hint.len() + 3 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }

        // Message body region (below the mode line, above an optional status).
        let lines = self.content_lines();
        let body_top = area.y + 2;
        let body_rows = area.height.saturating_sub(3) as usize;
        let max_scroll = lines.len().saturating_sub(body_rows);
        let scroll = self.scroll.min(max_scroll);

        for (row, line) in lines.iter().skip(scroll).take(body_rows).enumerate() {
            let y = body_top + row as u16;
            // Colour header field names (before the first `:`) distinctly.
            let style = if line.contains(": ") && !line.starts_with('>') && !line.starts_with(' ') {
                field_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, line, area.width as usize, style);
        }

        // Status line (errors, save/reload notices) or the active prompt.
        if !self.render_prompt(area, surface, info_style) && !self.status.is_empty() {
            surface.set_stringn(
                area.x,
                area.y + area.height - 1,
                &self.status,
                area.width as usize,
                info_style,
            );
        }
    }
}

impl Rmail {
    /// Draw the inline prompt on the bottom row if one is active. Returns whether
    /// it drew (so the caller can skip the status line).
    fn render_prompt(
        &self,
        area: Rect,
        surface: &mut Surface,
        style: zemacs_view::graphics::Style,
    ) -> bool {
        let Some(prompt) = self.prompt.as_ref() else {
            return false;
        };
        let line = format!("{}{}", prompt.label, prompt.input);
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &line,
            area.width as usize,
            style,
        );
        true
    }
}

/// Expand a leading `~/` to the home directory.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = zemacs_stdx::path::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// Serialise one message as a single mbox entry (`o`, rmail-output).
fn one_message_mbox(msg: &zemacs_core::rmail::Msg) -> String {
    let mut out = String::from("From ");
    out.push_str(&msg.envelope);
    out.push('\n');
    for (k, v) in &msg.headers {
        out.push_str(k);
        out.push_str(": ");
        out.push_str(v);
        out.push('\n');
    }
    out.push('\n');
    for line in msg.body.split('\n') {
        if line.starts_with("From ") {
            out.push('>');
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out
}

/// Append text to a file, creating it if needed.
fn append_file(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_entry_reparses_as_one_message() {
        // `o` (rmail-output) must emit a valid single-message mbox entry.
        let mb = Mailbox::from_mbox(
            "From a@b.com Mon Jan 1 00:00:00 2026\nFrom: a@b.com\nSubject: Hi\n\nBody line.\n",
        );
        let entry = one_message_mbox(mb.current().unwrap());
        let round = Mailbox::from_mbox(&entry);
        assert_eq!(round.len(), 1);
        assert_eq!(round.current().unwrap().subject(), "Hi");
        assert!(round.current().unwrap().body.contains("Body line."));
    }

    #[test]
    fn tilde_expands_to_home() {
        let p = expand_tilde("~/mail/inbox");
        assert!(p.is_absolute() || p.to_string_lossy().contains("mail/inbox"));
        assert_eq!(expand_tilde("/etc/passwd"), PathBuf::from("/etc/passwd"));
    }
}
