//! Rmail — the zmax port of the GNU Emacs `rmail` mail reader.
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
    /// `rmail-user-mail-address-regexp` so the key is the *other* party; zmax
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
/// because zmax has no send transport beyond queuing to a local outbox.
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

// ---------------------------------------------------------------------------
// MIME (`rmail-mime` and friends)
// ---------------------------------------------------------------------------

/// One decoded MIME entity of a message, as `rmail-mime` displays it.
///
/// A non-MIME message yields exactly one entity holding its whole body, so the
/// display code never needs a special case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MimeEntity {
    /// Bare, lower-cased media type: `text/plain`, `image/png`, `message/rfc822`.
    pub media_type: String,
    /// The `filename=`/`name=` parameter, when the part carries one.
    pub filename: Option<String>,
    /// Decoded text. `Some` for `text/*` and `message/*` parts (after
    /// content-transfer-decoding and charset decoding), `None` for binary parts,
    /// which `rmail-mime` shows as an attachment line rather than as text.
    pub text: Option<String>,
    /// Size of the decoded content in bytes.
    pub size: usize,
    /// Whether the entity's content is collapsed. `rmail-mime` shows text parts
    /// expanded and attachments hidden; `rmail-mime-toggle-hidden` flips this.
    pub hidden: bool,
}

impl MimeEntity {
    /// The one-line label `rmail-mime` puts above an entity's content.
    pub fn label(&self) -> String {
        match (&self.filename, self.text.is_some()) {
            (Some(name), _) => format!("[{} {} ({} bytes)]", self.media_type, name, self.size),
            (None, _) => format!("[{} ({} bytes)]", self.media_type, self.size),
        }
    }
}

/// Value of a `Content-Type`-style parameter (`; name=value`, quotes optional),
/// matched case-insensitively on the parameter name.
pub fn content_param(header: &str, name: &str) -> Option<String> {
    for part in header.split(';').skip(1) {
        let (k, v) = part.split_once('=')?;
        if k.trim().eq_ignore_ascii_case(name) {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(v);
            return Some(v.to_string());
        }
    }
    None
}

/// The bare media type of a `Content-Type` header value, lower-cased.
fn media_type(header: &str) -> String {
    header
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// Decode a base64 body (RFC 2045: `=` padding, any non-alphabet byte — notably
/// the line breaks base64 bodies are wrapped at — is skipped).
pub fn decode_base64(src: &str) -> Vec<u8> {
    const fn value(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &c in src.as_bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = value(c) else { continue };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// Decode a quoted-printable body (RFC 2045): `=XX` hex escapes and `=` at end
/// of line as a soft (deleted) line break.
pub fn decode_quoted_printable(src: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut lines = src.split('\n').peekable();
    while let Some(line) = lines.next() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let (content, soft) = match line.strip_suffix('=') {
            Some(rest) => (rest, true),
            None => (line, false),
        };
        let bytes = content.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'=' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        if !soft && lines.peek().is_some() {
            out.push(b'\n');
        }
    }
    out
}

/// Content-transfer-decode a part body.
fn transfer_decode(raw: &str, cte: &str) -> Vec<u8> {
    match cte.trim().to_ascii_lowercase().as_str() {
        "base64" => decode_base64(raw),
        "quoted-printable" => decode_quoted_printable(raw),
        _ => raw.as_bytes().to_vec(),
    }
}

/// Decode bytes with a named coding system (`rmail-redecode-body`, and the
/// `charset=` parameter of a MIME part). Emacs coding names are normalised to
/// WHATWG labels: an `-unix`/`-dos`/`-mac` end-of-line suffix is dropped and
/// `latin-N` is spelled `iso-8859-N`. Returns `None` for an unknown name.
pub fn decode_bytes(bytes: &[u8], coding: &str) -> Option<String> {
    let mut name = coding.trim().to_ascii_lowercase();
    for eol in ["-unix", "-dos", "-mac"] {
        if let Some(base) = name.strip_suffix(eol) {
            name = base.to_string();
            break;
        }
    }
    if let Some(n) = name.strip_prefix("latin-") {
        name = format!("iso-8859-{n}");
    }
    let encoding = encoding_rs::Encoding::for_label(name.as_bytes())?;
    let (text, _, _) = encoding.decode(bytes);
    Some(text.into_owned())
}

/// Decode a part's bytes as text, honouring its `charset=` parameter (unknown or
/// missing charsets fall back to UTF-8, lossily — Emacs's `undecided` default).
fn decode_text(bytes: &[u8], content_type: &str) -> String {
    content_param(content_type, "charset")
        .and_then(|cs| decode_bytes(bytes, &cs))
        .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned())
}

/// Split a multipart body on its boundary, returning each part's raw text
/// (headers included). The preamble and epilogue are dropped, per RFC 2046.
fn split_parts(body: &str, boundary: &str) -> Vec<String> {
    let delim = format!("--{boundary}");
    let close = format!("--{boundary}--");
    let mut parts: Vec<String> = Vec::new();
    let mut cur: Option<Vec<&str>> = None;
    for line in body.split('\n') {
        let t = line.trim_end_matches('\r');
        if t == close {
            break;
        }
        if t == delim {
            if let Some(lines) = cur.take() {
                parts.push(lines.join("\n"));
            }
            cur = Some(Vec::new());
            continue;
        }
        if let Some(lines) = cur.as_mut() {
            lines.push(line);
        }
    }
    if let Some(lines) = cur {
        parts.push(lines.join("\n"));
    }
    parts
}

/// The MIME entities of a message, in display order (`rmail-mime`).
///
/// `multipart/*` bodies are split on their boundary and each part is
/// content-transfer-decoded and charset-decoded; nested multiparts are walked
/// recursively. A message with no `Content-Type` (or a non-multipart one) yields
/// a single entity carrying the whole body.
pub fn parse_mime(msg: &Msg) -> Vec<MimeEntity> {
    let ct = msg
        .header("Content-Type")
        .unwrap_or("text/plain")
        .to_string();
    let cte = msg
        .header("Content-Transfer-Encoding")
        .unwrap_or("7bit")
        .to_string();
    let mut out = Vec::new();
    walk_part(&ct, &cte, None, &msg.body, 0, &mut out);
    out
}

/// Nesting depth cap: a malformed message must not recurse forever.
const MAX_MIME_DEPTH: usize = 8;

fn walk_part(
    content_type: &str,
    cte: &str,
    filename: Option<String>,
    body: &str,
    depth: usize,
    out: &mut Vec<MimeEntity>,
) {
    let mtype = media_type(content_type);
    if mtype.starts_with("multipart/") && depth < MAX_MIME_DEPTH {
        if let Some(boundary) = content_param(content_type, "boundary") {
            for part in split_parts(body, &boundary) {
                let parsed = email::parse_buffer(&part);
                let head = |name: &str| {
                    parsed
                        .headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(name))
                        .map(|(_, v)| v.clone())
                };
                let ct = head("Content-Type").unwrap_or_else(|| "text/plain".to_string());
                let cte = head("Content-Transfer-Encoding").unwrap_or_else(|| "7bit".to_string());
                let name = content_param(&ct, "name").or_else(|| {
                    head("Content-Disposition")
                        .as_deref()
                        .and_then(|cd| content_param(cd, "filename"))
                });
                walk_part(&ct, &cte, name, &parsed.body, depth + 1, out);
            }
            return;
        }
    }

    let bytes = transfer_decode(body, cte);
    let textual = mtype.starts_with("text/") || mtype.starts_with("message/");
    out.push(MimeEntity {
        text: textual.then(|| decode_text(&bytes, content_type)),
        size: bytes.len(),
        // rmail-mime shows text inline and leaves attachments collapsed.
        hidden: !textual,
        media_type: mtype,
        filename,
    });
}

// ---------------------------------------------------------------------------
// Digests and forwarded messages
// ---------------------------------------------------------------------------

/// Is this line an RFC 1153 digest separator (a run of hyphens on its own)?
/// RFC 1153 specifies exactly 30; real digests vary, so any long run counts —
/// this is Emacs's "sloppy" `rmail-digest-parse-rfc1153sloppy`.
fn is_digest_separator(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 27 && t.bytes().all(|b| b == b'-')
}

/// `undigestify-rmail-message`: break a digest message into the messages it
/// carries.
///
/// Both digest forms Emacs understands are handled: a MIME `multipart/digest`
/// (each `message/rfc822` part is one message) and the classic RFC 1153 text
/// digest (messages separated by a line of hyphens, with a table-of-contents
/// preamble before the first separator and a trailer after the last). Each
/// sub-message inherits the digest's envelope line so it round-trips through
/// mbox. Returns an empty vector when the message is not a digest.
pub fn undigestify(msg: &Msg) -> Vec<Msg> {
    let build = |text: &str| -> Option<Msg> {
        let parsed = email::parse_buffer(text.trim_start_matches('\n'));
        // A digest chunk with no headers at all is the trailer ("End of Digest"),
        // not a message.
        if parsed.headers.is_empty() {
            return None;
        }
        Some(Msg {
            envelope: msg.envelope.clone(),
            headers: parsed.headers,
            body: parsed.body,
            deleted: false,
            seen: false,
            labels: Vec::new(),
        })
    };

    // MIME multipart/digest: every message/rfc822 part is a message.
    let ct = msg.header("Content-Type").unwrap_or("");
    if media_type(ct) == "multipart/digest" {
        if let Some(boundary) = content_param(ct, "boundary") {
            return split_parts(&msg.body, &boundary)
                .iter()
                .filter_map(|part| {
                    // Each part is `Content-Type: message/rfc822`, a blank line,
                    // then the embedded message.
                    let outer = email::parse_buffer(part);
                    build(&outer.body)
                })
                .collect();
        }
    }

    // RFC 1153: hyphen-separated chunks; the text before the first separator is
    // the digest's own table of contents, not a message.
    let mut chunks: Vec<String> = Vec::new();
    let mut cur: Option<Vec<&str>> = None;
    for line in msg.body.split('\n') {
        if is_digest_separator(line) {
            if let Some(lines) = cur.take() {
                chunks.push(lines.join("\n"));
            }
            cur = Some(Vec::new());
            continue;
        }
        if let Some(lines) = cur.as_mut() {
            lines.push(line);
        }
    }
    if let Some(lines) = cur {
        chunks.push(lines.join("\n"));
    }
    chunks.iter().filter_map(|c| build(c)).collect()
}

/// The markers a forwarded message is wrapped in — Emacs's
/// `rmail-forward-separator` shapes plus the widely used Gmail/MUA variants.
const FORWARD_START: [&str; 3] = [
    "start of forwarded message",
    "forwarded message",
    "original message",
];

/// `unforward-rmail-message`: pull the message a forward carries back out.
///
/// The forwarded copy is either a MIME `message/rfc822` part or, for the classic
/// Rmail/RFC 934 forward, the region between the "forwarded message" markers,
/// whose lines are `- `-stuffed (RFC 934 §2) and must be unstuffed. Returns
/// `None` when the message carries no forwarded copy.
pub fn unforward(msg: &Msg) -> Option<Msg> {
    // MIME: the embedded message is a message/rfc822 entity.
    for entity in parse_mime(msg) {
        if entity.media_type == "message/rfc822" {
            if let Some(text) = entity.text {
                let parsed = email::parse_buffer(&text);
                if !parsed.headers.is_empty() {
                    return Some(Msg {
                        envelope: msg.envelope.clone(),
                        headers: parsed.headers,
                        body: parsed.body,
                        deleted: false,
                        seen: false,
                        labels: Vec::new(),
                    });
                }
            }
        }
    }

    // RFC 934 / Rmail text forward.
    let lines: Vec<&str> = msg.body.split('\n').collect();
    let is_marker = |line: &str, kinds: &[&str]| {
        let t = line.trim().trim_matches('-').trim().to_ascii_lowercase();
        kinds.iter().any(|k| t == *k)
    };
    let start = lines
        .iter()
        .position(|l| l.contains("---") && is_marker(l, &FORWARD_START))?
        + 1;
    let end = lines
        .iter()
        .skip(start)
        .position(|l| {
            l.contains("---")
                && is_marker(l, &["end of forwarded message", "end forwarded message"])
        })
        .map(|i| start + i)
        .unwrap_or(lines.len());

    // RFC 934 unstuffing: the forwarder prefixed every line that began with `-`
    // with `- `, so the markers stayed unambiguous.
    let text: String = lines[start..end]
        .iter()
        .map(|l| l.strip_prefix("- ").unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n");
    let parsed = email::parse_buffer(text.trim_start_matches('\n'));
    if parsed.headers.is_empty() {
        return None;
    }
    Some(Msg {
        envelope: msg.envelope.clone(),
        headers: parsed.headers,
        body: parsed.body,
        deleted: false,
        seen: false,
        labels: Vec::new(),
    })
}

/// The byte range of the first OpenPGP armor block in `body`, `BEGIN` line
/// through `END` line inclusive (`rmail-epa-decrypt` decrypts exactly this and
/// splices the plaintext back in its place). `None` when there is no armor.
pub fn pgp_armor_range(body: &str) -> Option<(usize, usize)> {
    const BEGIN: &str = "-----BEGIN PGP MESSAGE-----";
    const END: &str = "-----END PGP MESSAGE-----";
    let start = body.find(BEGIN)?;
    let end = body[start..].find(END)? + start + END.len();
    Some((start, end))
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

    // --- MIME -------------------------------------------------------------

    fn msg_from(text: &str) -> Msg {
        let parsed = email::parse_buffer(text);
        Msg {
            envelope: "sender@example.com Mon Jan  1 00:00:00 2026".into(),
            headers: parsed.headers,
            body: parsed.body,
            deleted: false,
            seen: false,
            labels: Vec::new(),
        }
    }

    #[test]
    fn base64_decodes_wrapped_lines_and_padding() {
        // Line breaks inside the armor are not alphabet bytes and must be skipped.
        assert_eq!(decode_base64("aGVsbG8="), b"hello");
        assert_eq!(decode_base64("aGVs\nbG8="), b"hello");
        assert_eq!(decode_base64("aGVsbG8gd29ybGQ="), b"hello world");
        // No padding at all (some mailers omit it).
        assert_eq!(decode_base64("aGVsbG8"), b"hello");
    }

    #[test]
    fn quoted_printable_decodes_escapes_and_soft_breaks() {
        assert_eq!(decode_quoted_printable("caf=C3=A9"), "café".as_bytes());
        // A trailing `=` is a soft break: the line join leaves no newline.
        assert_eq!(decode_quoted_printable("one=\ntwo"), b"onetwo");
        assert_eq!(decode_quoted_printable("one\ntwo"), b"one\ntwo");
        // A stray `=` that is not a valid escape survives verbatim.
        assert_eq!(decode_quoted_printable("a=zz"), b"a=zz");
    }

    #[test]
    fn parse_mime_splits_multipart_and_decodes_each_part() {
        let msg = msg_from(
            "From: a@b.com\n\
             Subject: Report\n\
             Content-Type: multipart/mixed; boundary=\"SEP\"\n\
             \n\
             preamble ignored\n\
             --SEP\n\
             Content-Type: text/plain; charset=utf-8\n\
             Content-Transfer-Encoding: quoted-printable\n\
             \n\
             caf=C3=A9 time\n\
             --SEP\n\
             Content-Type: application/pdf; name=\"q3.pdf\"\n\
             Content-Transfer-Encoding: base64\n\
             \n\
             aGVsbG8=\n\
             --SEP--\n\
             epilogue ignored\n",
        );
        let parts = parse_mime(&msg);
        assert_eq!(parts.len(), 2);

        assert_eq!(parts[0].media_type, "text/plain");
        assert_eq!(parts[0].text.as_deref(), Some("café time"));
        assert!(!parts[0].hidden, "text parts show inline");

        assert_eq!(parts[1].media_type, "application/pdf");
        assert_eq!(parts[1].filename.as_deref(), Some("q3.pdf"));
        assert_eq!(parts[1].text, None, "binary parts carry no text");
        assert_eq!(parts[1].size, 5, "base64 `aGVsbG8=` is 5 decoded bytes");
        assert!(parts[1].hidden, "attachments start collapsed");
    }

    #[test]
    fn parse_mime_gives_a_plain_message_one_entity() {
        let msg = msg_from("From: a@b.com\nSubject: Hi\n\nJust text.\n");
        let parts = parse_mime(&msg);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].media_type, "text/plain");
        assert_eq!(parts[0].text.as_deref(), Some("Just text."));
    }

    #[test]
    fn charset_is_honoured_when_decoding_text() {
        // 0xE9 is `é` in latin-1 but invalid UTF-8: getting it right proves the
        // charset parameter (not a lossy UTF-8 fallback) drove the decode.
        let bytes = [b'c', b'a', b'f', 0xE9];
        assert_eq!(decode_bytes(&bytes, "iso-8859-1").as_deref(), Some("café"));
        // Emacs spellings: `latin-1`, and an end-of-line suffix.
        assert_eq!(
            decode_bytes(&bytes, "latin-1-unix").as_deref(),
            Some("café")
        );
        assert_eq!(decode_bytes(&bytes, "no-such-coding"), None);
    }

    // --- digests / forwards ------------------------------------------------

    #[test]
    fn undigestify_splits_an_rfc1153_digest() {
        let digest = msg_from(
            "From: list@example.com\n\
             Subject: Example Digest, Vol 1, Issue 2\n\
             \n\
             Today's Topics:\n\
             \n\
             ------------------------------\n\
             \n\
             From: alice@example.com\n\
             Subject: First topic\n\
             \n\
             First body.\n\
             \n\
             ------------------------------\n\
             \n\
             From: bob@example.com\n\
             Subject: Second topic\n\
             \n\
             Second body.\n\
             \n\
             ------------------------------\n\
             \n\
             End of Example Digest\n",
        );
        let msgs = undigestify(&digest);
        assert_eq!(
            msgs.len(),
            2,
            "the TOC preamble and trailer are not messages"
        );
        assert_eq!(msgs[0].subject(), "First topic");
        assert_eq!(msgs[0].from(), "alice@example.com");
        assert!(msgs[0].body.contains("First body."));
        assert_eq!(msgs[1].subject(), "Second topic");
        assert!(msgs[1].body.contains("Second body."));
    }

    #[test]
    fn undigestify_splits_a_mime_digest() {
        let digest = msg_from(
            "From: list@example.com\n\
             Subject: Digest\n\
             Content-Type: multipart/digest; boundary=\"B\"\n\
             \n\
             --B\n\
             Content-Type: message/rfc822\n\
             \n\
             From: alice@example.com\n\
             Subject: Inner one\n\
             \n\
             Inner body.\n\
             --B--\n",
        );
        let msgs = undigestify(&digest);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].subject(), "Inner one");
        assert!(msgs[0].body.contains("Inner body."));
    }

    #[test]
    fn undigestify_refuses_a_plain_message() {
        assert!(undigestify(&msg_from("From: a@b.com\nSubject: Hi\n\nNot a digest.\n")).is_empty());
    }

    #[test]
    fn unforward_recovers_the_forwarded_message_and_unstuffs_it() {
        let fwd = msg_from(
            "From: bob@example.com\n\
             Subject: Fwd: Budget\n\
             \n\
             Thought you should see this.\n\
             \n\
             ------- Start of forwarded message -------\n\
             From: alice@example.com\n\
             Subject: Budget\n\
             \n\
             Numbers below.\n\
             - --- signature marker survives unstuffing\n\
             ------- End of forwarded message -------\n\
             \n\
             Bob\n",
        );
        let inner = unforward(&fwd).expect("a forward carries a copy");
        assert_eq!(inner.from(), "alice@example.com");
        assert_eq!(inner.subject(), "Budget");
        assert!(inner.body.contains("Numbers below."));
        // The `- ` stuffing is stripped, restoring the original line.
        assert!(inner
            .body
            .contains("--- signature marker survives unstuffing"));
        assert!(
            !inner.body.contains("Bob"),
            "text after the end marker is not part of it"
        );
    }

    #[test]
    fn unforward_recovers_a_mime_forward() {
        let fwd = msg_from(
            "From: bob@example.com\n\
             Subject: Fwd\n\
             Content-Type: multipart/mixed; boundary=\"B\"\n\
             \n\
             --B\n\
             Content-Type: text/plain\n\
             \n\
             See attached.\n\
             --B\n\
             Content-Type: message/rfc822\n\
             \n\
             From: alice@example.com\n\
             Subject: Budget\n\
             \n\
             Numbers below.\n\
             --B--\n",
        );
        let inner = unforward(&fwd).expect("the message/rfc822 part is the forward");
        assert_eq!(inner.subject(), "Budget");
        assert!(inner.body.contains("Numbers below."));
    }

    #[test]
    fn unforward_refuses_a_message_with_no_forward() {
        assert!(unforward(&msg_from("From: a@b.com\nSubject: Hi\n\nNothing here.\n")).is_none());
    }

    #[test]
    fn pgp_armor_range_spans_the_whole_block() {
        let body =
            "Before.\n-----BEGIN PGP MESSAGE-----\n\nhQEMA\n-----END PGP MESSAGE-----\nAfter.\n";
        let (s, e) = pgp_armor_range(body).expect("an armored body");
        assert!(body[s..e].starts_with("-----BEGIN PGP MESSAGE-----"));
        assert!(body[s..e].ends_with("-----END PGP MESSAGE-----"));
        assert_eq!(&body[..s], "Before.\n");
        assert_eq!(&body[e..], "\nAfter.\n");
        assert!(pgp_armor_range("no armor here").is_none());
    }
}
