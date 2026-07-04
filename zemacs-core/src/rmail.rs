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

    /// The mbox envelope date (the tail of the `From ` line after the sender
    /// address), used as a fallback when there is no `Date:` header.
    fn envelope_date(&self) -> &str {
        // "alice@example.com Mon Jan  1 00:00:00 2026" -> "Mon Jan  1 ..."
        match self.envelope.split_once(char::is_whitespace) {
            Some((_addr, rest)) => rest.trim_start(),
            None => "",
        }
    }

    /// Sort key for `rmail-sort-by-date`: the `Date:` header (or the envelope
    /// date) normalised to a lexically comparable `YYYYMMDDhhmmss` string via
    /// [`sortable_date`].
    pub fn date_key(&self) -> String {
        let raw = self.header("Date").unwrap_or_else(|| self.envelope_date());
        sortable_date(raw)
    }

    /// Sort key for `rmail-sort-by-subject`: the subject with any `Re:`/`Fwd:`
    /// prefix chain stripped, lower-cased (so replies sort next to their root).
    pub fn subject_key(&self) -> String {
        normalize_subject(self.subject()).to_lowercase()
    }

    /// Sort key for `rmail-sort-by-author`: the first address of the `From:`
    /// header with any display name/comment stripped (Emacs
    /// `mail-strip-quoted-names`), lower-cased.
    pub fn author_key(&self) -> String {
        first_address(self.header("From").unwrap_or(""))
    }

    /// Sort key for `rmail-sort-by-recipient`: the first stripped address of the
    /// `To:` header, lower-cased.
    pub fn recipient_key(&self) -> String {
        first_address(self.header("To").unwrap_or(""))
    }

    /// Sort key for `rmail-sort-by-correspondent`: the first stripped address
    /// among `From:`, `To:`, then `Cc:`.
    ///
    /// Real Emacs skips addresses matching the user's own
    /// `rmail-user-mail-address-regexp` so the key is the *other* party; zemacs
    /// has no configured user identity, so this approximates it with the first
    /// non-empty of From/To/Cc.
    pub fn correspondent_key(&self) -> String {
        for field in ["From", "To", "Cc"] {
            if let Some(v) = self.header(field) {
                let a = first_address(v);
                if !a.is_empty() {
                    return a;
                }
            }
        }
        String::new()
    }

    /// Sort key for `rmail-sort-by-lines`: the number of body lines.
    pub fn line_count(&self) -> usize {
        if self.body.is_empty() {
            0
        } else {
            self.body.split('\n').count()
        }
    }

    /// Sort key for `rmail-sort-by-labels`: the message's labels sorted and
    /// joined, lower-cased (Emacs sorts by the keyword/label string).
    pub fn labels_key(&self) -> String {
        let mut ls: Vec<String> = self.labels.iter().map(|l| l.to_lowercase()).collect();
        ls.sort();
        ls.join(",")
    }
}

/// The first RFC 5322 address of a header value with its display name/comment
/// stripped (`mail-strip-quoted-names`), lower-cased. `"Alice <a@b.com>, …"`
/// and `"a@b.com (Alice)"` both yield `"a@b.com"`; an empty/nameless value is
/// returned verbatim, lower-cased.
fn first_address(value: &str) -> String {
    email::parse_addresses(value)
        .into_iter()
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

/// Normalise a mail `Date:` header (or an mbox asctime envelope date) into a
/// lexically comparable `YYYYMMDDhhmmss` string, so a plain string sort orders
/// messages chronologically (Emacs `rmail-sortable-date-string`).
///
/// Handles both RFC 5322 (`"Mon, 2 Jan 2026 15:04:05 +0000"`) and asctime
/// (`"Mon Jan  2 15:04:05 2026"`) forms by scanning tokens for a month name, a
/// 4-digit year, a day number, and an `hh:mm[:ss]` time. The zone offset is
/// ignored (times are compared as written). Anything unparseable yields all
/// zeros, so undated messages sort first.
pub fn sortable_date(raw: &str) -> String {
    let mut year = 0u32;
    let mut month = 0u32;
    let mut day = 0u32;
    let (mut hh, mut mm, mut ss) = (0u32, 0u32, 0u32);

    for tok in raw.split(|c: char| c.is_whitespace() || c == ',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        if let Some(m) = month_number(tok) {
            month = m;
        } else if tok.contains(':') {
            // hh:mm[:ss]
            let mut parts = tok.split(':');
            hh = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            mm = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            ss = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        } else if let Ok(n) = tok.parse::<u32>() {
            if tok.len() == 4 && (1000..=9999).contains(&n) {
                year = n;
            } else if (1..=31).contains(&n) && day == 0 {
                day = n;
            }
        }
    }
    format!("{year:04}{month:02}{day:02}{hh:02}{mm:02}{ss:02}")
}

/// Month name (any case, at least the 3-letter abbreviation) -> 1..=12.
fn month_number(tok: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let low = tok.to_ascii_lowercase();
    if low.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|m| low.starts_with(m))
        .map(|i| i as u32 + 1)
}

/// Build a case-insensitive matcher from a user pattern, treating it as a
/// regexp but falling back to a literal search if it does not compile (so a
/// plain substring typed at the summary prompt always works).
fn build_matcher(pattern: &str) -> regex::Regex {
    regex::Regex::new(&format!("(?i){pattern}"))
        .unwrap_or_else(|_| regex::Regex::new(&format!("(?i){}", regex::escape(pattern))).unwrap())
}

/// Whether any of `fields` on `msg` matches `re`.
fn any_field_matches(msg: &Msg, fields: &[&str], re: &regex::Regex) -> bool {
    fields
        .iter()
        .any(|f| msg.header(f).is_some_and(|v| re.is_match(v)))
}

/// Indices of messages whose `To`, `From`, or `Cc` matches `pattern`
/// (`rmail-summary-by-recipients`).
pub fn filter_by_recipients(msgs: &[Msg], pattern: &str) -> Vec<usize> {
    let re = build_matcher(pattern);
    msgs.iter()
        .enumerate()
        .filter(|(_, m)| any_field_matches(m, &["To", "From", "Cc"], &re))
        .map(|(i, _)| i)
        .collect()
}

/// Indices of messages whose `From` matches `pattern`
/// (`rmail-summary-by-senders`).
pub fn filter_by_senders(msgs: &[Msg], pattern: &str) -> Vec<usize> {
    let re = build_matcher(pattern);
    msgs.iter()
        .enumerate()
        .filter(|(_, m)| any_field_matches(m, &["From"], &re))
        .map(|(i, _)| i)
        .collect()
}

/// Indices of messages whose `Subject` matches `pattern`
/// (`rmail-summary-by-topic` / `rmail-summary-by-subject`).
pub fn filter_by_topic(msgs: &[Msg], pattern: &str) -> Vec<usize> {
    let re = build_matcher(pattern);
    msgs.iter()
        .enumerate()
        .filter(|(_, m)| any_field_matches(m, &["Subject"], &re))
        .map(|(i, _)| i)
        .collect()
}

/// Indices of messages whose whole text (all headers plus body) matches
/// `pattern` (`rmail-summary-by-regexp`).
pub fn filter_by_regexp(msgs: &[Msg], pattern: &str) -> Vec<usize> {
    let re = build_matcher(pattern);
    msgs.iter()
        .enumerate()
        .filter(|(_, m)| {
            re.is_match(&m.body)
                || m.headers
                    .iter()
                    .any(|(k, v)| re.is_match(k) || re.is_match(v))
        })
        .map(|(i, _)| i)
        .collect()
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
            self.current = self
                .current
                .saturating_sub(deleted_before)
                .min(self.msgs.len() - 1);
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

    /// Stably reorder the message list by `key`, keeping the cursor on the same
    /// message it was on (its identity survives the move). The shared engine
    /// behind every `rmail-sort-by-*` command.
    fn sort_by_key<K: Ord>(&mut self, key: impl Fn(&Msg) -> K) {
        if self.msgs.len() < 2 {
            return;
        }
        let cur = self.current;
        let mut order: Vec<usize> = (0..self.msgs.len()).collect();
        // sort_by_key is stable, so equal keys keep their original order.
        order.sort_by_key(|&i| key(&self.msgs[i]));
        self.current = order.iter().position(|&i| i == cur).unwrap_or(cur);
        let mut slots: Vec<Option<Msg>> = std::mem::take(&mut self.msgs)
            .into_iter()
            .map(Some)
            .collect();
        self.msgs = order.iter().map(|&i| slots[i].take().unwrap()).collect();
    }

    /// `rmail-sort-by-date` — order messages by their `Date:` header.
    pub fn sort_by_date(&mut self) {
        self.sort_by_key(|m| m.date_key());
    }

    /// `rmail-sort-by-subject` — order by normalised subject.
    pub fn sort_by_subject(&mut self) {
        self.sort_by_key(|m| m.subject_key());
    }

    /// `rmail-sort-by-author` — order by the `From:` address.
    pub fn sort_by_author(&mut self) {
        self.sort_by_key(|m| m.author_key());
    }

    /// `rmail-sort-by-recipient` — order by the `To:` address.
    pub fn sort_by_recipient(&mut self) {
        self.sort_by_key(|m| m.recipient_key());
    }

    /// `rmail-sort-by-correspondent` — order by the correspondent address.
    pub fn sort_by_correspondent(&mut self) {
        self.sort_by_key(|m| m.correspondent_key());
    }

    /// `rmail-sort-by-lines` — order by body line count.
    pub fn sort_by_lines(&mut self) {
        self.sort_by_key(|m| m.line_count());
    }

    /// `rmail-sort-by-labels` — order by the message's labels.
    pub fn sort_by_labels(&mut self) {
        self.sort_by_key(|m| m.labels_key());
    }

    /// `rmail-summary-undelete-many` (no arg): clear the Deleted flag on every
    /// deleted message. Returns how many were undeleted.
    pub fn undelete_all(&mut self) -> usize {
        let mut n = 0;
        for m in &mut self.msgs {
            if m.deleted {
                m.deleted = false;
                n += 1;
            }
        }
        n
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
    /// writes the result of [`Self::expunge`] followed by this).
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
    let to = msg
        .header("Reply-To")
        .or_else(|| msg.header("From"))
        .unwrap_or("")
        .to_string();
    let subject = format!("Re: {}", normalize_subject(msg.subject()));
    let cited = cite_body(msg);
    (to, subject, cited)
}

/// `rmail-resend` fields: a bounce/resend draft. No recipient yet (the user
/// types the resend-to addresses), the original subject unchanged, and the
/// **verbatim** original message (its headers and body) as the draft body so
/// the recipient receives the message as-is.
///
/// This is a partial port: it hands message-mode an editable resend draft but
/// does not synthesise the `Resent-From`/`Resent-To` headers real Emacs adds,
/// because zemacs has no send transport beyond queuing to a local outbox.
pub fn resend_fields(msg: &Msg) -> (String, String, String) {
    let subject = msg.subject().to_string();
    let mut body = String::new();
    for (k, v) in &msg.headers {
        body.push_str(k);
        body.push_str(": ");
        body.push_str(v);
        body.push('\n');
    }
    body.push('\n');
    body.push_str(&msg.body);
    (String::new(), subject, body)
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
        if let Some(rest) = lower
            .strip_prefix("re:")
            .or_else(|| lower.strip_prefix("fwd:"))
            .or_else(|| lower.strip_prefix("fw:"))
        {
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
    msg.headers.iter().any(|(k, v)| {
        k.to_lowercase().contains(needle_lower) || v.to_lowercase().contains(needle_lower)
    })
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
        assert_eq!(
            mb.current().unwrap().header("From"),
            Some("Alice <alice@example.com>")
        );
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

    // A mailbox whose messages are deliberately out of every natural order, so
    // each sort visibly rearranges them.
    const SORTBOX: &str = "\
From carol@example.com Wed Jan  3 09:00:00 2026
From: Carol <carol@z.example>
To: team@example.com
Cc: ops@example.com
Date: Wed, 3 Jan 2026 09:00:00 +0000
Subject: Zeta report

one line only.

From alice@example.com Mon Jan  1 08:00:00 2026
From: Alice <alice@a.example>
To: bob@example.com
Date: Mon, 1 Jan 2026 08:00:00 +0000
Subject: Re: Alpha plan

body line 1
body line 2
body line 3

From bob@example.com Tue Feb 10 07:00:00 2026
From: Bob <bob@m.example>
To: carol@example.com
Date: Tue, 10 Feb 2026 07:00:00 +0000
Subject: alpha plan

two
lines
";

    fn subjects(mb: &Mailbox) -> Vec<String> {
        mb.msgs.iter().map(|m| m.subject().to_string()).collect()
    }

    #[test]
    fn sortable_date_parses_rfc_and_asctime() {
        assert_eq!(
            sortable_date("Wed, 3 Jan 2026 09:04:05 +0000"),
            "20260103090405"
        );
        // asctime (mbox envelope) form, single-digit day is space-padded.
        assert_eq!(sortable_date("Mon Jan  1 08:00:00 2026"), "20260101080000");
        assert_eq!(
            sortable_date("Tue, 10 Feb 2026 07:00:00 GMT"),
            "20260210070000"
        );
        // Unparseable -> all zeros so it sorts first.
        assert_eq!(sortable_date("not a date"), "00000000000000");
    }

    #[test]
    fn sort_by_date_orders_chronologically() {
        let mut mb = Mailbox::from_mbox(SORTBOX);
        // Start on Alice (Jan 1) and confirm the cursor follows her after sorting.
        mb.show(2);
        assert_eq!(
            mb.current().unwrap().header("From"),
            Some("Alice <alice@a.example>")
        );
        mb.sort_by_date();
        assert_eq!(
            subjects(&mb),
            vec!["Re: Alpha plan", "Zeta report", "alpha plan"]
        );
        assert_eq!(
            mb.current().unwrap().header("From"),
            Some("Alice <alice@a.example>")
        );
        assert_eq!(mb.current, 0);
    }

    #[test]
    fn sort_by_subject_groups_replies_with_root() {
        let mut mb = Mailbox::from_mbox(SORTBOX);
        mb.sort_by_subject();
        // "Re: Alpha plan" and "alpha plan" normalise to "alpha plan" and sort
        // before "Zeta report"; the two alphas keep source order (stable).
        assert_eq!(
            subjects(&mb),
            vec!["Re: Alpha plan", "alpha plan", "Zeta report"]
        );
    }

    #[test]
    fn sort_by_author_recipient_correspondent() {
        let mut mb = Mailbox::from_mbox(SORTBOX);
        mb.sort_by_author();
        // From addresses: alice@a, bob@m, carol@z.
        assert_eq!(
            subjects(&mb),
            vec!["Re: Alpha plan", "alpha plan", "Zeta report"]
        );
        let mut mb = Mailbox::from_mbox(SORTBOX);
        mb.sort_by_recipient();
        // To addresses: bob@ (Alice), carol@ (Bob), team@ (Carol).
        assert_eq!(
            subjects(&mb),
            vec!["Re: Alpha plan", "alpha plan", "Zeta report"]
        );
        let mut mb = Mailbox::from_mbox(SORTBOX);
        mb.sort_by_correspondent();
        // Correspondent = From here: alice@a, bob@m, carol@z.
        assert_eq!(mb.msgs[0].header("From"), Some("Alice <alice@a.example>"));
    }

    #[test]
    fn sort_by_lines_and_labels() {
        let mut mb = Mailbox::from_mbox(SORTBOX);
        mb.sort_by_lines();
        // Body line counts: Carol=1, Bob=2, Alice=3.
        assert_eq!(
            subjects(&mb),
            vec!["Zeta report", "alpha plan", "Re: Alpha plan"]
        );

        let mut mb = Mailbox::from_mbox(SORTBOX);
        mb.msgs[0].labels = vec!["work".into()]; // Carol
        mb.msgs[2].labels = vec!["home".into()]; // Bob
        mb.sort_by_labels();
        // Keys: Alice="", Bob="home", Carol="work" -> "" sorts first.
        assert_eq!(
            subjects(&mb),
            vec!["Re: Alpha plan", "alpha plan", "Zeta report"]
        );
    }

    #[test]
    fn summary_filters_select_matching_messages() {
        let mb = Mailbox::from_mbox(SORTBOX);
        // Senders.
        assert_eq!(filter_by_senders(&mb.msgs, "alice"), vec![1]);
        // Recipients: matches To/From/Cc — "ops" only in Carol's Cc.
        assert_eq!(filter_by_recipients(&mb.msgs, "ops@example"), vec![0]);
        // "bob" appears in Alice's To and Bob's From.
        assert_eq!(filter_by_recipients(&mb.msgs, "bob"), vec![1, 2]);
        // Topic: subject regexp, case-insensitive.
        assert_eq!(filter_by_topic(&mb.msgs, "alpha"), vec![1, 2]);
        // Whole-message regexp: body text.
        assert_eq!(filter_by_regexp(&mb.msgs, "one line only"), vec![0]);
        assert_eq!(filter_by_regexp(&mb.msgs, "zeta"), vec![0]);
    }

    #[test]
    fn undelete_all_clears_every_deletion() {
        let mut mb = Mailbox::from_mbox(SORTBOX);
        mb.msgs[0].deleted = true;
        mb.msgs[2].deleted = true;
        assert_eq!(mb.undelete_all(), 2);
        assert!(mb.msgs.iter().all(|m| !m.deleted));
        assert_eq!(mb.undelete_all(), 0);
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
