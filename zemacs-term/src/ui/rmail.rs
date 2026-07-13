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
//!   SPC / DEL / S-SPC  scroll body down / up / up
//!   `.` / `/`   top / bottom of message
//!   d / C-d     delete forward / backward;  u undelete;  x expunge
//!   s           expunge and save the mbox;  g  reload the file from disk
//!   t           toggle full vs pruned headers
//!   C-c C-n/C-p next / previous message with the same subject (N/P aliases)
//!   r / f / m   reply / forward / compose new mail (opens a message-mode draft)
//!   c           continue the draft previously being composed (rmail-continue)
//!   M-m         retry a failed (bounced) message: re-compose the returned original
//!   R           resend (bounce) the current message as a new draft
//!   e           edit the current message body (M-c commit, M-k abort)
//!   M-d/j/a/r/e/l/b  sort mailbox by date/subject/author/recipient/
//!               correspondent/lines/labels
//!   C-M-f / C-M-r / C-M-t / C-M-s  summary by senders / recipients / topic /
//!               regexp (S / T / B / G aliases)
//!   C-M-l       summary by labels (`l` alias);  h / C-M-h  the full summary
//!   C-M-n/C-M-p next / previous message carrying a prompted label (C-n/C-p aliases)
//!   U           undelete every deleted message
//!   C-o / O     output the message as-seen
//!   b / z       bury (close) the reader
//!   q / Esc     quit the reader

use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zemacs_core::rmail::{forward_fields, reply_fields, resend_fields, Mailbox};
use zemacs_view::graphics::Rect;

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key, shift,
};

/// Which pane the reader is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// A single message (the default `rmail-mode` view).
    Message,
    /// The `rmail-summary` list, with a movable cursor over `sum`.
    Summary,
    /// The `rmail-mime` display: the message's decoded MIME entities, with a
    /// cursor over them (`rmail-mime-next-item` / `-previous-item`) and each
    /// entity's content shown or collapsed (`rmail-mime-toggle-hidden`).
    Mime,
}

/// The command an active minibuffer prompt will run once the user hits RET.
/// Rmail reads these arguments in the echo area; the reader reads them inline.
#[derive(Clone, Copy)]
enum PromptAction {
    Search,
    AddLabel,
    KillLabel,
    SummaryByLabel,
    SummaryBySenders,
    SummaryByRecipients,
    SummaryByTopic,
    SummaryByRegexp,
    NextLabeled,
    PrevLabeled,
    Input,
    Output,
    OutputBody,
    OutputAsSeen,
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
    /// When editing the current message body (`e`, `rmail-edit-current-message`),
    /// the working copy of the body text. `None` means not editing.
    edit: Option<String>,
    /// `true` after a bare `C-c`, awaiting the second key of rmail-mode's `C-c`
    /// prefix (`C-c C-n` / `C-c C-p`, same-subject motion).
    pending_ctrl_c: bool,
    /// The decoded MIME entities of the current message, while `View::Mime` is up.
    mime: Vec<zemacs_core::rmail::MimeEntity>,
    /// Cursor over `mime` (the entity `rmail-mime-toggle-hidden` acts on).
    mime_cursor: usize,
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
            edit: None,
            pending_ctrl_c: false,
            mime: Vec::new(),
            mime_cursor: 0,
        }
    }

    // -----------------------------------------------------------------------
    // The public surface the `rmail-*` M-x commands drive (see
    // `commands::rmail_action`). Each one mutates the live reader and reports
    // what it did so the command layer can put it in the echo area.
    // -----------------------------------------------------------------------

    /// `rmail-mime` (`v`): toggle between the raw message and the decoded MIME
    /// display. Returns the number of entities when switching to MIME, `None`
    /// when switching back.
    pub fn toggle_mime(&mut self) -> Option<usize> {
        if self.view == View::Mime {
            self.view = View::Message;
            return None;
        }
        let Some(msg) = self.mailbox.current() else {
            return Some(0);
        };
        self.mime = zemacs_core::rmail::parse_mime(msg);
        self.mime_cursor = 0;
        self.scroll = 0;
        self.view = View::Mime;
        Some(self.mime.len())
    }

    /// `rmail-mime-next-item` / `rmail-mime-previous-item` (TAB / S-TAB): move the
    /// cursor over the MIME entities. False when there is nowhere to move.
    pub fn mime_move(&mut self, forward: bool) -> bool {
        if self.view != View::Mime || self.mime.is_empty() {
            return false;
        }
        self.scroll = 0;
        if forward {
            if self.mime_cursor + 1 >= self.mime.len() {
                return false;
            }
            self.mime_cursor += 1;
        } else {
            if self.mime_cursor == 0 {
                return false;
            }
            self.mime_cursor -= 1;
        }
        true
    }

    /// `rmail-mime-toggle-hidden` (RET): show or collapse the entity at point.
    /// Returns the new hidden state, or `None` when no entity is at point.
    pub fn mime_toggle_hidden(&mut self) -> Option<bool> {
        if self.view != View::Mime {
            return None;
        }
        let entity = self.mime.get_mut(self.mime_cursor)?;
        entity.hidden = !entity.hidden;
        Some(entity.hidden)
    }

    /// The entity the MIME cursor is on, for the command layer's echo-area report.
    pub fn mime_current_label(&self) -> Option<String> {
        self.mime.get(self.mime_cursor).map(|e| e.label())
    }

    /// `undigestify-rmail-message`: break the current digest into its messages,
    /// insert them after it, and leave the digest itself deleted (as Emacs does).
    /// Returns how many messages came out; 0 when this is not a digest.
    pub fn undigestify(&mut self) -> usize {
        let Some(msg) = self.mailbox.current() else {
            return 0;
        };
        let parts = zemacs_core::rmail::undigestify(msg);
        if parts.is_empty() {
            return 0;
        }
        let at = self.mailbox.current;
        let n = parts.len();
        self.mailbox.msgs[at].deleted = true;
        for (i, m) in parts.into_iter().enumerate() {
            self.mailbox.msgs.insert(at + 1 + i, m);
        }
        // Show the first message that came out of the digest.
        self.mailbox.current = at + 1;
        self.scroll = 0;
        n
    }

    /// `unforward-rmail-message`: extract the message a forward carries into a
    /// message of its own, right after the containing one.
    pub fn unforward(&mut self) -> bool {
        let Some(inner) = self
            .mailbox
            .current()
            .and_then(zemacs_core::rmail::unforward)
        else {
            return false;
        };
        let at = self.mailbox.current;
        self.mailbox.msgs.insert(at + 1, inner);
        self.mailbox.current = at + 1;
        self.scroll = 0;
        true
    }

    /// `rmail-epa-decrypt`: decrypt the OpenPGP armor in the current message with
    /// `gpg` and splice the plaintext back in its place. `gpg` runs in batch mode,
    /// so it uses whatever key material `gpg-agent` already holds rather than
    /// taking over the terminal with a passphrase prompt.
    pub fn epa_decrypt(&mut self) -> Result<usize, String> {
        let idx = self.mailbox.current;
        let body = self
            .mailbox
            .msgs
            .get(idx)
            .map(|m| m.body.clone())
            .ok_or("no message")?;
        let (start, end) =
            zemacs_core::rmail::pgp_armor_range(&body).ok_or("no OpenPGP armor in this message")?;
        let plain = gpg_decrypt(&body[start..end])?;
        let mut decrypted = String::with_capacity(body.len());
        decrypted.push_str(&body[..start]);
        decrypted.push_str(plain.trim_end_matches('\n'));
        decrypted.push_str(&body[end..]);
        self.mailbox.msgs[idx].body = decrypted;
        self.scroll = 0;
        Ok(plain.len())
    }

    /// `rmail-redecode-body`: re-decode the current message's body from the raw
    /// bytes on disk using the named coding system, for a message whose charset
    /// was mislabelled (or unlabelled) and came out as mojibake.
    pub fn redecode_body(&mut self, coding: &str) -> Result<(), String> {
        let raw = std::fs::read(&self.path)
            .map_err(|e| format!("cannot read {}: {e}", self.path.display()))?;
        let text = zemacs_core::rmail::decode_bytes(&raw, coding)
            .ok_or_else(|| format!("unknown coding system: {coding}"))?;
        let fresh = zemacs_core::rmail::parse_mbox(&text);
        let idx = self.mailbox.current;
        let body = fresh
            .get(idx)
            .map(|m| m.body.clone())
            .ok_or("this message is not in the file on disk")?;
        self.mailbox.msgs[idx].body = body;
        self.scroll = 0;
        Ok(())
    }

    /// The `(to, subject, body)` of a `rmail-resend` draft for the current message.
    pub fn resend_draft(&self) -> Option<(String, String, String)> {
        self.mailbox.current().map(resend_fields)
    }

    /// The `(to, subject, body)` of a `rmail-retry-failure` draft: the original
    /// message a delivery-failure notice returned. `None` when this is not a bounce.
    pub fn retry_failure_draft(&self) -> Option<(String, String, String)> {
        self.mailbox
            .current()
            .and_then(|m| retry_failure_fields(&m.body))
    }

    /// Put a line in the reader's own status area (the `rmail-*` commands echo
    /// through the editor, but the reader keeps the last word visible).
    pub fn report(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
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

    /// Handle a key while the `rmail-mime` display is showing.
    fn mime_key(&mut self, key: zemacs_view::input::KeyEvent) -> EventResult {
        match key {
            key!('q') | key!(Esc) | key!('v') => self.view = View::Message,
            key!(Tab) | key!('n') => {
                self.mime_move(true);
            }
            shift!(Tab) | key!('p') => {
                self.mime_move(false);
            }
            key!(Enter) => {
                self.mime_toggle_hidden();
            }
            key!(' ') => self.scroll += 10,
            key!(Backspace) | key!(Delete) => self.scroll = self.scroll.saturating_sub(10),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    /// The rendered lines of the MIME display: one label line per entity (the
    /// one at point marked), followed by its content unless it is collapsed.
    fn mime_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (i, e) in self.mime.iter().enumerate() {
            let cursor = if i == self.mime_cursor { '>' } else { ' ' };
            let state = if e.hidden { "+" } else { "-" };
            lines.push(format!("{cursor}{state} {}", e.label()));
            if e.hidden {
                continue;
            }
            match &e.text {
                Some(text) => lines.extend(text.split('\n').map(|l| format!("   {l}"))),
                None => lines.push("   (binary content — not shown)".to_string()),
            }
            lines.push(String::new());
        }
        if lines.is_empty() {
            lines.push("[no MIME entities]".to_string());
        }
        lines
    }

    /// Handle a key while the summary pane is showing.
    fn summary_key(&mut self, key: zemacs_view::input::KeyEvent) -> EventResult {
        let last = self.sum.len().saturating_sub(1);
        match key {
            // q/Q leave the summary (rmail-summary-quit / rmail-summary-wipe);
            // `b` buries it (rmail-summary-bury), which here is the same exit.
            key!('q') | key!('Q') | key!('b') | key!(Esc) => self.view = View::Message,
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

    /// Handle a key while editing the current message body (`rmail-edit-mode`).
    /// `M-c` finishes and commits (`rmail-cease-edit`); `M-k` discards
    /// (`rmail-abort-edit`); other keys type into the working body.
    fn edit_key(&mut self, key: zemacs_view::input::KeyEvent) {
        match key {
            // rmail-cease-edit: commit the edited body back to the message.
            alt!('c') => {
                if let (Some(text), Some(msg)) = (
                    self.edit.take(),
                    self.mailbox.msgs.get_mut(self.mailbox.current),
                ) {
                    msg.body = text;
                    self.status = "edit committed".to_string();
                }
            }
            // rmail-abort-edit: throw the edit away.
            alt!('k') | key!(Esc) => {
                self.edit = None;
                self.status = "edit aborted".to_string();
            }
            key!(Enter) => {
                if let Some(t) = self.edit.as_mut() {
                    t.push('\n');
                }
            }
            key!(Backspace) | key!(Delete) => {
                if let Some(t) = self.edit.as_mut() {
                    t.pop();
                }
            }
            zemacs_view::input::KeyEvent {
                code: zemacs_view::keyboard::KeyCode::Char(c),
                ..
            } => {
                if let Some(t) = self.edit.as_mut() {
                    t.push(c);
                }
            }
            _ => {}
        }
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
            PromptAction::SummaryBySenders => {
                self.open_filtered_summary(zemacs_core::rmail::filter_by_senders(
                    &self.mailbox.msgs,
                    &arg,
                ));
            }
            PromptAction::SummaryByRecipients => {
                self.open_filtered_summary(zemacs_core::rmail::filter_by_recipients(
                    &self.mailbox.msgs,
                    &arg,
                ));
            }
            PromptAction::SummaryByTopic => {
                self.open_filtered_summary(zemacs_core::rmail::filter_by_topic(
                    &self.mailbox.msgs,
                    &arg,
                ));
            }
            PromptAction::SummaryByRegexp => {
                self.open_filtered_summary(zemacs_core::rmail::filter_by_regexp(
                    &self.mailbox.msgs,
                    &arg,
                ));
            }
            PromptAction::NextLabeled => {
                self.mailbox.next_labeled(&arg);
                self.scroll = 0;
            }
            PromptAction::PrevLabeled => {
                self.mailbox.prev_labeled(&arg);
                self.scroll = 0;
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
            PromptAction::OutputAsSeen => {
                // rmail-output-as-seen writes exactly what the reader shows: the
                // currently visible (pruned or full) header set plus the body.
                let entry = self.as_seen_text();
                match append_file(&expand_tilde(&arg), &entry) {
                    Ok(()) => self.status = format!("output (as seen) to {arg}"),
                    Err(e) => self.status = format!("cannot write {arg}: {e}"),
                }
            }
        }
    }

    /// The message rendered exactly as shown (an mbox entry using only the
    /// headers currently visible — pruned or full — plus the body), for
    /// `rmail-output-as-seen`.
    fn as_seen_text(&self) -> String {
        let Some(msg) = self.mailbox.current() else {
            return String::new();
        };
        let mut out = format!("From {}\n", msg.envelope);
        let shown: Vec<(&str, &str)> = if self.full_headers {
            msg.headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect()
        } else {
            PRUNED
                .iter()
                .filter_map(|name| msg.header(name).map(|v| (*name, v)))
                .collect()
        };
        for (k, v) in shown {
            out.push_str(&format!("{k}: {v}\n"));
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

    /// Open a summary over `indices`, or report when nothing matched.
    fn open_filtered_summary(&mut self, indices: Vec<usize>) {
        if indices.is_empty() {
            self.status = "no matching messages".to_string();
        } else {
            self.open_summary(indices);
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
        // When editing, show the working copy (with a caret) instead of the
        // stored body.
        let body = match &self.edit {
            Some(text) => {
                lines.push("-- editing (M-c commit, M-k abort) --".to_string());
                format!("{text}\u{2588}")
            }
            None => msg.body.clone(),
        };
        for line in body.split('\n') {
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
    ///
    /// The message the draft replies to is handed to the command layer first:
    /// that is message-mode's `message-reply-buffer`, and it is what
    /// `message-yank-original` (`C-c C-y`) cites when you ask for the original
    /// inside the draft.
    fn compose(&self, to: String, subject: String, body: String) -> Callback {
        let original = self.original_text();
        Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            compositor.pop();
            if let Some(original) = original.clone() {
                crate::commands::set_yank_original(original);
            }
            crate::commands::typed::open_mail_draft(cx, &to, &subject, &body);
        })
    }

    /// The message under the cursor as RFC 5322 text (headers, blank line, body)
    /// — the "original" a reply/forward/resend draft is made from.
    fn original_text(&self) -> Option<String> {
        let msg = self.mailbox.current()?;
        let mut out = String::new();
        for (k, v) in &msg.headers {
            out.push_str(k);
            out.push_str(": ");
            out.push_str(v);
            out.push('\n');
        }
        out.push('\n');
        out.push_str(&msg.body);
        Some(out)
    }
}

/// The lines a mailer puts before the copy of the message it is returning —
/// Emacs's `mail-unsent-separator`, whose job is to find where the bounced
/// original starts inside a failure notice.
const UNSENT_SEPARATORS: [&str; 6] = [
    "----- Unsent message follows -----",
    "------- Unsent message follows -------",
    "----- Original message -----",
    "--- Below this line is a copy of the message.",
    "--- The unsent message follows ---",
    "Content-Type: message/rfc822",
];

/// `rmail-retry-failure` (`M-m`): pull the returned original out of a delivery
/// failure and return `(to, subject, body)` for a fresh draft of it. `None` when
/// the message carries no unsent-message copy (i.e. it is not a bounce).
///
/// Everything after the separator is the original message: its own header block,
/// a blank line, then its body. `To:` and `Subject:` come from that header block,
/// so the retry goes back to the original recipient, not to the mailer daemon.
fn retry_failure_fields(body: &str) -> Option<(String, String, String)> {
    let lines: Vec<&str> = body.lines().collect();
    let start = lines.iter().position(|line| {
        let t = line.trim();
        UNSENT_SEPARATORS
            .iter()
            .any(|sep| t.eq_ignore_ascii_case(sep) || t.starts_with(sep))
    })? + 1;

    let mut to = String::new();
    let mut subject = String::new();
    let mut i = start;
    // Skip blank lines between the separator and the returned header block.
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    while i < lines.len() && !lines[i].trim().is_empty() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("To:") {
            to = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Subject:") {
            subject = rest.trim().to_string();
        }
        i += 1;
    }
    if to.is_empty() {
        return None;
    }
    // Past the blank line that ends the header block is the original body.
    let body = lines
        .get(i + 1..)
        .map(|rest| rest.join("\n"))
        .unwrap_or_default();
    Some((to, subject, body))
}

impl Component for Rmail {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
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

        // Second key of the `C-c` prefix (rmail-mode's C-c map).
        if std::mem::take(&mut self.pending_ctrl_c) {
            match key {
                // C-c C-n / C-c C-p — rmail-next/previous-same-subject.
                ctrl!('n') => {
                    self.mailbox.next_same_subject();
                    self.scroll = 0;
                }
                ctrl!('p') => {
                    self.mailbox.prev_same_subject();
                    self.scroll = 0;
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        // Editing the message body swallows keys until M-c/M-k.
        if self.edit.is_some() {
            self.edit_key(key);
            return EventResult::Consumed(None);
        }

        // The summary and MIME panes have their own key handling.
        if self.view == View::Summary {
            return self.summary_key(key);
        }
        if self.view == View::Mime {
            return self.mime_key(key);
        }

        // Accumulate a numeric prefix for `j`.
        if let key!(c @ '0'..='9') = key {
            self.count.push(c);
            return EventResult::Consumed(None);
        }

        const STEP: usize = 10; // body scroll step in lines
        match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close)),
            // C-c is rmail-mode's prefix (C-c C-n / C-c C-p), not a quit chord.
            ctrl!('c') => self.pending_ctrl_c = true,

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

            // Scrolling. S-SPC scrolls back, like DEL (rmail binds both).
            key!(' ') => self.scroll += STEP,
            key!(Backspace) | key!(Delete) | shift!(' ') => {
                self.scroll = self.scroll.saturating_sub(STEP)
            }
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

            // Sorting (rmail-sort-by-*). Reorders the whole mailbox in place,
            // keeping the cursor on the current message.
            alt!('d') => {
                self.mailbox.sort_by_date();
                self.scroll = 0;
                self.status = "sorted by date".to_string();
            }
            alt!('j') => {
                self.mailbox.sort_by_subject();
                self.scroll = 0;
                self.status = "sorted by subject".to_string();
            }
            alt!('a') => {
                self.mailbox.sort_by_author();
                self.scroll = 0;
                self.status = "sorted by author".to_string();
            }
            alt!('r') => {
                self.mailbox.sort_by_recipient();
                self.scroll = 0;
                self.status = "sorted by recipient".to_string();
            }
            alt!('e') => {
                self.mailbox.sort_by_correspondent();
                self.scroll = 0;
                self.status = "sorted by correspondent".to_string();
            }
            alt!('l') => {
                self.mailbox.sort_by_lines();
                self.scroll = 0;
                self.status = "sorted by lines".to_string();
            }
            alt!('b') => {
                self.mailbox.sort_by_labels();
                self.scroll = 0;
                self.status = "sorted by labels".to_string();
            }

            // Labeled-message navigation (rmail-next/previous-labeled-message).
            ctrl!('n') => self.ask(
                "Move to next message with label: ",
                PromptAction::NextLabeled,
            ),
            ctrl!('p') => self.ask(
                "Move to previous message with label: ",
                PromptAction::PrevLabeled,
            ),

            // Undelete every deleted message (rmail-summary-undelete-many).
            key!('U') => {
                let n = self.mailbox.undelete_all();
                self.status = format!("{n} message(s) undeleted");
            }

            // Bury the reader (rmail-bury, `b`; `z` is the zemacs alias) — no
            // persistent buffer stack, so this simply closes the reader overlay.
            key!('b') | key!('z') => return EventResult::Consumed(Some(close)),

            // c — rmail-continue: go back to the outgoing message previously being
            // composed. A draft is an unsaved buffer holding the compose template,
            // so the newest such buffer is the one to return to.
            key!('c') => match draft_document(cx.editor) {
                Some(id) => {
                    return EventResult::Consumed(Some(Box::new(
                        move |compositor: &mut Compositor, cx: &mut Context| {
                            compositor.pop();
                            cx.editor.switch(id, zemacs_view::editor::Action::Replace);
                        },
                    )));
                }
                None => self.status = "rmail-continue: no message being composed".to_string(),
            },

            // M-m — rmail-retry-failure: re-compose the original message that a
            // delivery-failure notice returned, addressed to its real recipient.
            alt!('m') => {
                match self
                    .mailbox
                    .current()
                    .and_then(|m| retry_failure_fields(&m.body))
                {
                    Some((to, subject, body)) => {
                        return EventResult::Consumed(Some(self.compose(to, subject, body)));
                    }
                    None => {
                        self.status =
                            "rmail-retry-failure: no unsent message in this one".to_string()
                    }
                }
            }

            // Edit the current message body (rmail-edit-current-message).
            key!('e') => {
                self.edit = self.mailbox.current().map(|m| m.body.clone());
                self.scroll = 0;
                if self.edit.is_some() {
                    self.status = "editing: M-c commit, M-k abort".to_string();
                }
            }

            // Display.
            key!('t') => self.full_headers = !self.full_headers,
            key!('h') => self.open_summary((0..self.mailbox.len()).collect()),
            // v — rmail-mime: the decoded MIME display (TAB/S-TAB move between
            // entities, RET shows/collapses the one at point).
            key!('v') => {
                if let Some(n) = self.toggle_mime() {
                    self.status = format!("{n} MIME entit{}", if n == 1 { "y" } else { "ies" });
                }
            }

            // Labels, search, file output/input — all read an argument inline.
            key!('a') => self.ask("Add label: ", PromptAction::AddLabel),
            key!('k') => self.ask("Kill label: ", PromptAction::KillLabel),
            key!('l') => self.ask("Labels to summarize by: ", PromptAction::SummaryByLabel),
            key!('S') => self.ask("Summary by senders: ", PromptAction::SummaryBySenders),
            key!('T') => self.ask("Summary by recipients: ", PromptAction::SummaryByRecipients),
            key!('B') => self.ask("Summary by subject/topic: ", PromptAction::SummaryByTopic),
            key!('G') => self.ask("Summary by regexp: ", PromptAction::SummaryByRegexp),
            alt!('s') => self.ask("Search: ", PromptAction::Search),
            key!('i') => self.ask("Run rmail on file: ", PromptAction::Input),
            key!('o') => self.ask("Output message to file: ", PromptAction::Output),
            // C-o — rmail-output-as-seen (`O` is the zemacs alias).
            ctrl!('o') | key!('O') => {
                self.ask("Output (as seen) to file: ", PromptAction::OutputAsSeen)
            }
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
            // rmail-resend: open a bounce/resend draft of the current message.
            key!('R') => {
                if let Some((to, subject, body)) = self.mailbox.current().map(resend_fields) {
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

            // The C-M-… half of rmail-mode-map: the summary-by-* commands and the
            // labeled-message motions. CONTROL|ALT is not expressible with the
            // ctrl!/alt! macros, so the modifier pair is matched directly; the
            // single-key aliases above (S/T/B/G/l/h, C-n/C-p) stay bound.
            zemacs_view::input::KeyEvent {
                code: zemacs_view::keyboard::KeyCode::Char(c),
                modifiers,
            } if modifiers
                == zemacs_view::keyboard::KeyModifiers::CONTROL
                    | zemacs_view::keyboard::KeyModifiers::ALT =>
            {
                match c {
                    'f' => self.ask("Summary by senders: ", PromptAction::SummaryBySenders),
                    'r' => self.ask("Summary by recipients: ", PromptAction::SummaryByRecipients),
                    't' => self.ask("Summary by subject/topic: ", PromptAction::SummaryByTopic),
                    's' => self.ask("Summary by regexp: ", PromptAction::SummaryByRegexp),
                    'l' => self.ask("Labels to summarize by: ", PromptAction::SummaryByLabel),
                    'h' => self.open_summary((0..self.mailbox.len()).collect()),
                    'n' => self.ask(
                        "Move to next message with label: ",
                        PromptAction::NextLabeled,
                    ),
                    'p' => self.ask(
                        "Move to previous message with label: ",
                        PromptAction::PrevLabeled,
                    ),
                    _ => {}
                }
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

        if self.view == View::Mime {
            let title = format!(" RMAIL-MIME  {} entities", self.mime.len());
            surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
            let hint = "TAB/S-TAB item  RET show/hide  q back";
            if title.len() + hint.len() + 3 < area.width as usize {
                surface.set_stringn(
                    area.x + area.width - hint.len() as u16 - 1,
                    area.y,
                    hint,
                    hint.len(),
                    info_style,
                );
            }
            let lines = self.mime_lines();
            let rows = area.height.saturating_sub(3) as usize;
            let scroll = self.scroll.min(lines.len().saturating_sub(rows));
            for (row, line) in lines.iter().skip(scroll).take(rows).enumerate() {
                let style = if line.starts_with('>') {
                    field_style
                } else if line.starts_with(' ') && line.len() > 1 && &line[1..2] != " " {
                    header_style
                } else {
                    text_style
                };
                surface.set_stringn(
                    area.x,
                    area.y + 2 + row as u16,
                    line,
                    area.width as usize,
                    style,
                );
            }
            if !self.status.is_empty() {
                surface.set_stringn(
                    area.x,
                    area.y + area.height - 1,
                    &self.status,
                    area.width as usize,
                    info_style,
                );
            }
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

/// The draft `rmail-continue` returns to: the newest unsaved buffer that starts
/// with a message-mode `To:` header, i.e. the message last being composed.
pub fn draft_document(editor: &zemacs_view::Editor) -> Option<zemacs_view::DocumentId> {
    editor
        .documents()
        .filter(|d| d.path().is_none())
        .filter(|d| {
            d.text()
                .line(0)
                .as_str()
                .is_some_and(|l| l.starts_with("To:"))
        })
        .map(|d| d.id())
        .last()
}

/// Decrypt an OpenPGP armor block with `gpg` (`rmail-epa-decrypt`). Batch mode:
/// gpg never takes over the terminal for a passphrase, so decryption succeeds
/// when `gpg-agent` already holds the key and fails cleanly otherwise.
fn gpg_decrypt(armor: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("gpg")
        .args(["--batch", "--quiet", "--decrypt"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run gpg: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or("cannot write to gpg")?
        .write_all(armor.as_bytes())
        .map_err(|e| format!("cannot write to gpg: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("gpg failed: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let reason = err.lines().last().unwrap_or("decryption failed");
        return Err(format!("gpg: {reason}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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

    /// `M-m` (rmail-retry-failure) must re-address the ORIGINAL recipient, not the
    /// mailer daemon that bounced it: the To:/Subject: it uses come from the copy
    /// of the message after the unsent-message separator, not from the notice's own
    /// header block.
    #[test]
    fn retry_failure_recovers_the_bounced_original() {
        let bounce = "\
Your message could not be delivered to the following recipient:

  <nobody@example.invalid>: host not found

----- Unsent message follows -----
To: nobody@example.invalid
From: me@example.com
Subject: Lunch on Friday

Are you free at noon?
See you then.
";
        let (to, subject, body) = retry_failure_fields(bounce).expect("a bounce carries a copy");
        assert_eq!(to, "nobody@example.invalid");
        assert_eq!(subject, "Lunch on Friday");
        assert_eq!(body, "Are you free at noon?\nSee you then.\n".trim_end());
    }

    /// A message that is not a delivery failure has nothing to retry — the command
    /// must refuse rather than compose a draft out of the wrong headers.
    #[test]
    fn retry_failure_refuses_a_normal_message() {
        assert!(retry_failure_fields("Just a normal message.\nTo: not-a-header\n").is_none());
        assert!(retry_failure_fields("").is_none());
        // The separator is there, but the returned copy has no recipient.
        assert!(retry_failure_fields("----- Unsent message follows -----\n\nBody only.").is_none());
    }

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
