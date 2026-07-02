//! Rmail — the zemacs port of the GNU Emacs `rmail` mail reader.
//!
//! Rmail reads a local Unix mbox file (`rmail-file-name`, default `~/RMAIL`):
//! messages are concatenated, each introduced by an mbox `From ` envelope line.
//! This module is the pure, tested core of that reader — it parses an mbox into
//! an ordered list of messages, tracks the current message plus per-message
//! Deleted/labels/seen attributes, navigates (next/previous, undeleted-only,
//! first/last, by label), marks and expunges deletions, and serialises the
//! mailbox back to mbox text for saving. It also builds the reply/forward
//! drafts that Rmail hands to message-mode. No I/O, no network — the command
//! layer supplies the file bytes and decides where a reply draft goes.
//!
//! Header/body splitting reuses [`crate::email::parse_buffer`], so Rmail and
//! message-mode share one RFC 5322 parser.

use crate::email;

/// A single message in the mailbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Msg {
    /// The mbox `From ` envelope line, minus the leading `From ` (sender + date).
    pub envelope: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// Rmail "Deleted" attribute — expunge removes these.
    pub deleted: bool,
    /// Rmail "Unseen" attribute, cleared once the message is shown.
    pub seen: bool,
    pub labels: Vec<String>,
}

impl Msg {
    /// First value of a header, matched case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The one-line summary Rmail shows: `Subject` (falling back to sender).
    pub fn subject(&self) -> &str {
        self.header("Subject").unwrap_or("(no subject)")
    }

    pub fn from(&self) -> &str {
        self.header("From").unwrap_or(&self.envelope)
    }
}

/// A parsed mailbox with a current-message cursor.
#[derive(Clone, Debug, Default)]
pub struct Mailbox {
    pub msgs: Vec<Msg>,
    pub current: usize,
}

/// Parse Unix mbox text into an ordered message list. A message begins at a line
/// starting with `From ` (the mbox envelope line); everything up to the next
/// such line is that message. `>From`-quoted body lines are unquoted.
pub fn parse_mbox(text: &str) -> Vec<Msg> {
    let mut msgs = Vec::new();
    let mut envelope: Option<String> = None;
    let mut chunk: Vec<String> = Vec::new();

    let flush = |envelope: &mut Option<String>, chunk: &mut Vec<String>, msgs: &mut Vec<Msg>| {
        if let Some(env) = envelope.take() {
            // Unquote mbox `>From ` / `>>From ` escaping in the body region.
            let body_text: String = chunk
                .iter()
                .map(|l| {
                    if l.trim_start_matches('>').starts_with("From ") && l.starts_with('>') {
                        &l[1..]
                    } else {
                        l.as_str()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let parsed = email::parse_buffer(&body_text);
            msgs.push(Msg {
                envelope: env,
                headers: parsed.headers,
                body: parsed.body,
                deleted: false,
                seen: false,
                labels: Vec::new(),
            });
        }
        chunk.clear();
    };

    // mboxrd rule: a `From ` line is a message separator only at the start of the
    // file or immediately after a blank line. This disambiguates it from a body
    // line that merely begins with "From " (which real mailers `>`-quote, but
    // lenient readers must still not split on).
    let mut prev_blank = true;
    for line in text.lines() {
        let is_separator = prev_blank && line.starts_with("From ");
        if is_separator {
            flush(&mut envelope, &mut chunk, &mut msgs);
            envelope = Some(line["From ".len()..].to_string());
        } else if envelope.is_some() {
            chunk.push(line.to_string());
        }
        prev_blank = line.is_empty();
    }
    flush(&mut envelope, &mut chunk, &mut msgs);
    msgs
}

impl Mailbox {
    pub fn from_mbox(text: &str) -> Mailbox {
        let mut mb = Mailbox {
            msgs: parse_mbox(text),
            current: 0,
        };
        if let Some(m) = mb.msgs.get_mut(0) {
            m.seen = true;
        }
        mb
    }

    pub fn is_empty(&self) -> bool {
        self.msgs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.msgs.len()
    }

    pub fn current(&self) -> Option<&Msg> {
        self.msgs.get(self.current)
    }

    /// Set the cursor, clamping and marking the message seen.
    fn goto(&mut self, idx: usize) {
        if self.msgs.is_empty() {
            return;
        }
        self.current = idx.min(self.msgs.len() - 1);
        self.msgs[self.current].seen = true;
    }

    /// `n` `M-n` (`rmail-next-message` / undeleted variant). `skip_deleted`
    /// mirrors Rmail's default of skipping Deleted messages for `n`/`p`.
    pub fn next(&mut self, skip_deleted: bool) {
        let start = self.current;
        let mut i = self.current;
        while i + 1 < self.msgs.len() {
            i += 1;
            if !skip_deleted || !self.msgs[i].deleted {
                self.goto(i);
                return;
            }
        }
        // No later match: stay put (Rmail signals "end").
        self.current = start;
    }

    /// `p` `M-p` (`rmail-previous-message` / undeleted variant).
    pub fn prev(&mut self, skip_deleted: bool) {
        let start = self.current;
        let mut i = self.current;
        while i > 0 {
            i -= 1;
            if !skip_deleted || !self.msgs[i].deleted {
                self.goto(i);
                return;
            }
        }
        self.current = start;
    }

    /// `<` (`rmail-first-message`).
    pub fn first(&mut self) {
        self.goto(0);
    }

    /// `>` (`rmail-last-message`).
    pub fn last(&mut self) {
        if !self.msgs.is_empty() {
            self.goto(self.msgs.len() - 1);
        }
    }

    /// `j` (`rmail-show-message`): jump to a 1-based message number.
    pub fn show(&mut self, number: usize) {
        if number >= 1 {
            self.goto(number - 1);
        }
    }

    /// `d` (`rmail-delete-forward`): mark current Deleted, advance to the next
    /// undeleted message (staying if none).
    pub fn delete_forward(&mut self) {
        if let Some(m) = self.msgs.get_mut(self.current) {
            m.deleted = true;
        }
        self.next(true);
    }

    /// `C-d` (`rmail-delete-backward`).
    pub fn delete_backward(&mut self) {
        if let Some(m) = self.msgs.get_mut(self.current) {
            m.deleted = true;
        }
        self.prev(true);
    }

    /// `u` (`rmail-undelete-previous-message`): if the current message is
    /// deleted, undelete it; otherwise move back to the nearest deleted message
    /// and undelete that.
    pub fn undelete(&mut self) {
        if let Some(m) = self.msgs.get_mut(self.current) {
            if m.deleted {
                m.deleted = false;
                return;
            }
        }
        let mut i = self.current;
        while i > 0 {
            i -= 1;
            if self.msgs[i].deleted {
                self.msgs[i].deleted = false;
                self.current = i;
                return;
            }
        }
    }

    /// `x` (`rmail-expunge`): remove all Deleted messages, keeping the cursor on
    /// the message that follows (or the new last message).
    pub fn expunge(&mut self) {
        let deleted_before = self.msgs[..self.current.min(self.msgs.len())]
            .iter()
            .filter(|m| m.deleted)
            .count();
        self.msgs.retain(|m| !m.deleted);
        if self.msgs.is_empty() {
            self.current = 0;
        } else {
            self.current = self.current.saturating_sub(deleted_before).min(self.msgs.len() - 1);
        }
    }

    /// `a` (`rmail-add-label`) — add a label to the current message.
    pub fn add_label(&mut self, label: &str) {
        if let Some(m) = self.msgs.get_mut(self.current) {
            if !m.labels.iter().any(|l| l == label) {
                m.labels.push(label.to_string());
            }
        }
    }

    /// `k` (`rmail-kill-label`) — remove a label from the current message.
    pub fn kill_label(&mut self, label: &str) {
        if let Some(m) = self.msgs.get_mut(self.current) {
            m.labels.retain(|l| l != label);
        }
    }

    /// `C-M-n` (`rmail-next-labeled-message`): forward to the next message
    /// carrying `label`.
    pub fn next_labeled(&mut self, label: &str) {
        let mut i = self.current;
        while i + 1 < self.msgs.len() {
            i += 1;
            if self.msgs[i].labels.iter().any(|l| l == label) {
                self.goto(i);
                return;
            }
        }
    }

    /// `C-M-p` (`rmail-previous-labeled-message`).
    pub fn prev_labeled(&mut self, label: &str) {
        let mut i = self.current;
        while i > 0 {
            i -= 1;
            if self.msgs[i].labels.iter().any(|l| l == label) {
                self.goto(i);
                return;
            }
        }
    }

    /// `M-s` (`rmail-search`): forward to the next message whose headers or body
    /// contain `needle` (case-insensitive), wrapping is not performed.
    pub fn search(&mut self, needle: &str) -> bool {
        let needle = needle.to_lowercase();
        let mut i = self.current;
        while i + 1 < self.msgs.len() {
            i += 1;
            if message_contains(&self.msgs[i], &needle) {
                self.goto(i);
                return true;
            }
        }
        false
    }

    /// `C-c C-n` (`rmail-next-same-subject`).
    pub fn next_same_subject(&mut self) {
        let Some(subj) = self.current().map(|m| normalize_subject(m.subject())) else {
            return;
        };
        let mut i = self.current;
        while i + 1 < self.msgs.len() {
            i += 1;
            if normalize_subject(self.msgs[i].subject()) == subj {
                self.goto(i);
                return;
            }
        }
    }

    /// `C-c C-p` (`rmail-previous-same-subject`).
    pub fn prev_same_subject(&mut self) {
        let Some(subj) = self.current().map(|m| normalize_subject(m.subject())) else {
            return;
        };
        let mut i = self.current;
        while i > 0 {
            i -= 1;
            if normalize_subject(self.msgs[i].subject()) == subj {
                self.goto(i);
                return;
            }
        }
    }

    /// The number of undeleted messages (for the mode line).
    pub fn undeleted_count(&self) -> usize {
        self.msgs.iter().filter(|m| !m.deleted).count()
    }

    /// Serialise the mailbox back to Unix mbox text (`s` `rmail-expunge-and-save`
    /// writes the result of [`expunge`] followed by this).
    pub fn to_mbox(&self) -> String {
        let mut out = String::new();
        for m in &self.msgs {
            out.push_str("From ");
            out.push_str(&m.envelope);
            out.push('\n');
            for (k, v) in &m.headers {
                out.push_str(k);
                out.push_str(": ");
                out.push_str(v);
                out.push('\n');
            }
            out.push('\n');
            // Re-quote any body line beginning with `From `.
            for line in m.body.split('\n') {
                if line.starts_with("From ") {
                    out.push('>');
                }
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
        }
        out
    }
}

/// Build the message-mode draft body for `r`/`f`/`m` (`rmail-reply`,
/// `rmail-forward`, `rmail-mail`). Returns `(to, subject, cited_body)`.
pub fn reply_fields(msg: &Msg) -> (String, String, String) {
    let to = msg.header("Reply-To").or_else(|| msg.header("From")).unwrap_or("").to_string();
    let subject = format!("Re: {}", normalize_subject(msg.subject()));
    let cited = cite_body(msg);
    (to, subject, cited)
}

/// `f` (`rmail-forward`) fields: no recipient yet, `Fwd:` subject, quoted body.
pub fn forward_fields(msg: &Msg) -> (String, String, String) {
    let subject = format!("Fwd: {}", normalize_subject(msg.subject()));
    (String::new(), subject, cite_body(msg))
}

/// Prefix every body line with `> ` (message-mode `message-yank-prefix`).
fn cite_body(msg: &Msg) -> String {
    let mut out = format!("{} writes:\n", msg.from());
    for line in msg.body.split('\n') {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Strip a leading `Re:`/`Fwd:` chain so same-subject grouping and reply
/// subjects don't stack prefixes.
fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("re:").or_else(|| lower.strip_prefix("fwd:")).or_else(|| lower.strip_prefix("fw:")) {
            let cut = s.len() - rest.len();
            s = s[cut..].trim_start();
        } else {
            break;
        }
    }
    s.to_string()
}

fn message_contains(msg: &Msg, needle_lower: &str) -> bool {
    if msg.body.to_lowercase().contains(needle_lower) {
        return true;
    }
    msg.headers
        .iter()
        .any(|(k, v)| k.to_lowercase().contains(needle_lower) || v.to_lowercase().contains(needle_lower))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MBOX: &str = "\
From alice@example.com Mon Jan  1 00:00:00 2026
From: Alice <alice@example.com>
To: me@example.com
Subject: Hello

Hi there.
From is a tricky body line.

From bob@example.com Tue Jan  2 00:00:00 2026
From: Bob <bob@example.com>
Subject: Re: Hello

Reply body.

From carol@example.com Wed Jan  3 00:00:00 2026
From: Carol <carol@example.com>
Subject: Unrelated

Third.
";

    #[test]
    fn parses_all_messages() {
        let mb = Mailbox::from_mbox(MBOX);
        assert_eq!(mb.len(), 3);
        assert_eq!(mb.current().unwrap().header("From"), Some("Alice <alice@example.com>"));
        assert_eq!(mb.current().unwrap().subject(), "Hello");
        // The `From is a tricky...` body line must survive as body, not a split.
        assert!(mb.msgs[0].body.contains("From is a tricky"));
    }

    #[test]
    fn navigation_and_bounds() {
        let mut mb = Mailbox::from_mbox(MBOX);
        assert_eq!(mb.current, 0);
        mb.next(false);
        assert_eq!(mb.current, 1);
        mb.last();
        assert_eq!(mb.current, 2);
        mb.next(false); // at end, stays
        assert_eq!(mb.current, 2);
        mb.first();
        assert_eq!(mb.current, 0);
        mb.prev(false); // at start, stays
        assert_eq!(mb.current, 0);
        mb.show(2);
        assert_eq!(mb.current, 1);
    }

    #[test]
    fn delete_skip_and_undelete() {
        let mut mb = Mailbox::from_mbox(MBOX);
        mb.delete_forward(); // delete #0, move to #1
        assert!(mb.msgs[0].deleted);
        assert_eq!(mb.current, 1);
        mb.first(); // #0 (deleted)
        mb.next(true); // skip deleted #0? already at 0 which is deleted; next undeleted = 1
        assert_eq!(mb.current, 1);
        mb.undelete(); // no deleted at/behind 1 except 0
        assert!(!mb.msgs[0].deleted);
    }

    #[test]
    fn expunge_removes_deleted() {
        let mut mb = Mailbox::from_mbox(MBOX);
        mb.msgs[1].deleted = true;
        mb.current = 2;
        mb.expunge();
        assert_eq!(mb.len(), 2);
        assert_eq!(mb.undeleted_count(), 2);
        // cursor followed: was at index 2 with one deleted before it -> index 1
        assert_eq!(mb.current, 1);
        assert_eq!(mb.current().unwrap().subject(), "Unrelated");
    }

    #[test]
    fn labels_and_labeled_nav() {
        let mut mb = Mailbox::from_mbox(MBOX);
        mb.add_label("work");
        mb.last();
        mb.add_label("work");
        mb.first();
        mb.next_labeled("work"); // from 0 -> next with "work" is 2
        assert_eq!(mb.current, 2);
        mb.kill_label("work");
        assert!(mb.msgs[2].labels.is_empty());
    }

    #[test]
    fn search_and_same_subject() {
        let mut mb = Mailbox::from_mbox(MBOX);
        assert!(mb.search("reply body"));
        assert_eq!(mb.current, 1);
        mb.first();
        mb.next_same_subject(); // "Hello" and "Re: Hello" normalise equal -> 1
        assert_eq!(mb.current, 1);
    }

    #[test]
    fn reply_and_forward_fields() {
        let mb = Mailbox::from_mbox(MBOX);
        let (to, subj, body) = reply_fields(mb.current().unwrap());
        assert_eq!(to, "Alice <alice@example.com>");
        assert_eq!(subj, "Re: Hello");
        assert!(body.contains("> Hi there."));
        let (fto, fsubj, _) = forward_fields(mb.current().unwrap());
        assert_eq!(fto, "");
        assert_eq!(fsubj, "Fwd: Hello");
    }

    #[test]
    fn roundtrip_preserves_message_count() {
        let mb = Mailbox::from_mbox(MBOX);
        let text = mb.to_mbox();
        let mb2 = Mailbox::from_mbox(&text);
        assert_eq!(mb2.len(), 3);
        assert_eq!(mb2.msgs[0].subject(), "Hello");
        assert!(mb2.msgs[0].body.contains("From is a tricky"));
    }
}
