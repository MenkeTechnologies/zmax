//! Gnus — the newsreader core behind `M-x gnus`.
//!
//! This module owns everything that is not drawing: the NNTP protocol (RFC 3977
//! `MODE READER` / `LIST ACTIVE` / `GROUP` / `OVER` / `ARTICLE`), a local mbox
//! spool backend for people with no news server, the group-status model
//! (subscribed / unsubscribed / zombie / killed) and the `.newsrc` file that
//! persists it. The protocol and file formats are pure and unit-tested; the
//! transport uses only `std::net`, exactly like [`crate::irc`].
//!
//! The interactive layer is [`crate::ui::gnus`], a modal Component with the
//! `gnus-group-mode` and `gnus-summary-mode` keys; the `gnus-*` commands in
//! `commands.rs` drive that component.
//!
//! Two backends, picked by [`Server::open`]:
//!
//! * **NNTP** — `$NNTPSERVER`, or the address passed to `:gnus`. Port 119
//!   unless the address carries one. This is what a real news server speaks.
//! * **local mbox spool** — `~/News/<group>`, one mbox file per group, used
//!   when no NNTP server is configured or the connection fails. Emacs's
//!   `nnfolder` back end stores groups the same way, and it makes the reader
//!   usable (and testable) offline.
//!
//! Group statuses follow the Emacs manual: on the first run every server group
//! the user is not subscribed to becomes *killed*; a group that later appears on
//! the server and was never seen before becomes a *zombie*.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A group's subscription status (Emacs: `gnus-newsrc-alist` level plus the
/// `gnus-killed-list` / `gnus-zombie-list`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Subscribed — listed by `l` and `L`.
    Subscribed,
    /// Unsubscribed but still recorded in `.newsrc` — listed by `L` only.
    Unsubscribed,
    /// Newly appeared on the server since the last session — listed by `A z`.
    Zombie,
    /// Killed; not recorded in `.newsrc` — listed by `A k` only.
    Killed,
}

impl Level {
    /// The one-character status shown in the group buffer's first column, using
    /// the same letters Gnus writes into `.newsrc` (`:` / `!`) plus `Z`/`K`.
    pub fn mark(self) -> char {
        match self {
            Level::Subscribed => ':',
            Level::Unsubscribed => '!',
            Level::Zombie => 'Z',
            Level::Killed => 'K',
        }
    }
}

/// One newsgroup: its name, the server's article-number window and the ranges of
/// articles already read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    /// Lowest article number the server still holds.
    pub low: u64,
    /// Highest article number on the server (0 when the group is empty).
    pub high: u64,
    pub level: Level,
    /// Inclusive, sorted, non-overlapping ranges of read article numbers — the
    /// `1-1000,1005` field of a `.newsrc` line.
    pub read: Vec<(u64, u64)>,
}

impl Group {
    /// A group known only from `.newsrc` (no server window yet).
    pub fn new(name: &str, level: Level) -> Group {
        Group {
            name: name.to_string(),
            low: 0,
            high: 0,
            level,
            read: Vec::new(),
        }
    }

    /// Is article `n` marked read?
    pub fn is_read(&self, n: u64) -> bool {
        self.read.iter().any(|&(lo, hi)| n >= lo && n <= hi)
    }

    /// Mark article `n` read, merging it into the existing ranges.
    pub fn mark_read(&mut self, n: u64) {
        if self.is_read(n) {
            return;
        }
        self.read.push((n, n));
        self.read.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(self.read.len());
        for &(lo, hi) in &self.read {
            match merged.last_mut() {
                // Touching or overlapping: `1-4` and `5-6` become `1-6`.
                Some(last) if lo <= last.1.saturating_add(1) => last.1 = last.1.max(hi),
                _ => merged.push((lo, hi)),
            }
        }
        self.read = merged;
    }

    /// Mark article `n` unread again (splitting the range that holds it).
    pub fn mark_unread(&mut self, n: u64) {
        let mut out = Vec::with_capacity(self.read.len() + 1);
        for &(lo, hi) in &self.read {
            if n < lo || n > hi {
                out.push((lo, hi));
                continue;
            }
            if n > lo {
                out.push((lo, n - 1));
            }
            if n < hi {
                out.push((n + 1, hi));
            }
        }
        self.read = out;
    }

    /// How many articles in `[low, high]` are still unread.
    pub fn unread(&self) -> u64 {
        if self.high == 0 || self.high < self.low {
            return 0;
        }
        let total = self.high - self.low + 1;
        let read: u64 = self
            .read
            .iter()
            .map(|&(lo, hi)| {
                let lo = lo.max(self.low);
                let hi = hi.min(self.high);
                if hi >= lo {
                    hi - lo + 1
                } else {
                    0
                }
            })
            .sum();
        total.saturating_sub(read)
    }

    /// Is this group shown by the given listing? (`gnus-group-list-*`.)
    pub fn in_listing(&self, listing: Listing) -> bool {
        match listing {
            // `l` / `A s`: subscribed groups that have unread articles.
            Listing::Unread => self.level == Level::Subscribed && self.unread() > 0,
            // `L` / `A u`: every subscribed and unsubscribed group.
            Listing::All => matches!(self.level, Level::Subscribed | Level::Unsubscribed),
            Listing::Killed => self.level == Level::Killed,
            Listing::Zombies => self.level == Level::Zombie,
        }
    }
}

/// Which subset of the groups the group buffer is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// `gnus-group-list-groups` (`l`, `A s`) — the default listing.
    Unread,
    /// `gnus-group-list-all-groups` (`L`, `A u`).
    All,
    /// `gnus-group-list-killed` (`A k`).
    Killed,
    /// `gnus-group-list-zombies` (`A z`).
    Zombies,
}

impl Listing {
    /// The name shown in the group buffer's mode line.
    pub fn label(self) -> &'static str {
        match self {
            Listing::Unread => "unread",
            Listing::All => "all",
            Listing::Killed => "killed",
            Listing::Zombies => "zombies",
        }
    }
}

// --- `.newsrc` ---------------------------------------------------------------

/// Parse a `.newsrc` read-range field: `1-1000,1005,1100-1200`. Malformed
/// entries are skipped rather than failing the whole line, which is what every
/// newsreader does with a hand-edited file.
pub fn parse_ranges(field: &str) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let range = match part.split_once('-') {
            Some((a, b)) => match (a.trim().parse::<u64>(), b.trim().parse::<u64>()) {
                (Ok(a), Ok(b)) if a <= b => (a, b),
                _ => continue,
            },
            None => match part.parse::<u64>() {
                Ok(n) => (n, n),
                Err(_) => continue,
            },
        };
        out.push(range);
    }
    out.sort_unstable();
    out
}

/// Render read ranges back into the `.newsrc` field syntax (single-article
/// ranges collapse to a bare number, as Gnus writes them).
pub fn format_ranges(ranges: &[(u64, u64)]) -> String {
    ranges
        .iter()
        .map(|&(lo, hi)| {
            if lo == hi {
                lo.to_string()
            } else {
                format!("{lo}-{hi}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse a `.newsrc` file. Each line is `group: ranges` (subscribed) or
/// `group! ranges` (unsubscribed); options lines (`options ...`) are ignored.
pub fn parse_newsrc(text: &str) -> Vec<Group> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with("options") || line.starts_with('#') {
            continue;
        }
        let Some(sep) = line.find([':', '!']) else {
            continue;
        };
        let name = line[..sep].trim();
        if name.is_empty() {
            continue;
        }
        let level = if line.as_bytes()[sep] == b':' {
            Level::Subscribed
        } else {
            Level::Unsubscribed
        };
        let mut group = Group::new(name, level);
        group.read = parse_ranges(&line[sep + 1..]);
        out.push(group);
    }
    out
}

/// Serialise the subscribed and unsubscribed groups as a `.newsrc` file. Killed
/// and zombie groups are deliberately omitted — the Emacs manual is explicit
/// that killed groups are not recorded in `.newsrc`.
pub fn format_newsrc(groups: &[Group]) -> String {
    let mut out = String::new();
    for g in groups {
        let sep = match g.level {
            Level::Subscribed => ':',
            Level::Unsubscribed => '!',
            _ => continue,
        };
        out.push_str(&g.name);
        out.push(sep);
        let ranges = format_ranges(&g.read);
        if !ranges.is_empty() {
            out.push(' ');
            out.push_str(&ranges);
        }
        out.push('\n');
    }
    out
}

/// Parse the killed/zombie sidecar. Gnus keeps those two lists in the Lisp
/// `.newsrc.eld`; rather than write half-valid Lisp into a file another
/// newsreader owns, zmax keeps them in `~/.newsrc-zmax`, one `K <group>` or
/// `Z <group>` per line.
pub fn parse_sidecar(text: &str) -> Vec<Group> {
    let mut out = Vec::new();
    for line in text.lines() {
        let (tag, name) = match line.split_once(' ') {
            Some((t, n)) => (t, n.trim()),
            None => continue,
        };
        let level = match tag {
            "K" => Level::Killed,
            "Z" => Level::Zombie,
            _ => continue,
        };
        if !name.is_empty() {
            out.push(Group::new(name, level));
        }
    }
    out
}

/// Serialise the killed and zombie lists for [`parse_sidecar`].
pub fn format_sidecar(groups: &[Group]) -> String {
    let mut out = String::new();
    for g in groups {
        let tag = match g.level {
            Level::Killed => 'K',
            Level::Zombie => 'Z',
            _ => continue,
        };
        out.push(tag);
        out.push(' ');
        out.push_str(&g.name);
        out.push('\n');
    }
    out
}

/// Merge what the server reports (`name`, `low`, `high`) into the groups read
/// back from `.newsrc` and the sidecar, applying the manual's startup rule:
/// a group nobody has ever seen becomes *killed* on the very first run
/// (`first_run`) and a *zombie* on every later run.
pub fn merge_active(known: &mut Vec<Group>, active: &[(String, u64, u64)], first_run: bool) {
    for &(ref name, low, high) in active {
        match known.iter_mut().find(|g| &g.name == name) {
            Some(g) => {
                g.low = low;
                g.high = high;
            }
            None => {
                let mut g = Group::new(
                    name,
                    if first_run {
                        Level::Killed
                    } else {
                        Level::Zombie
                    },
                );
                g.low = low;
                g.high = high;
                known.push(g);
            }
        }
    }
}

// --- article overviews -------------------------------------------------------

/// One line of an NNTP `OVER`/`XOVER` response — the summary-buffer row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Overview {
    pub number: u64,
    pub subject: String,
    pub from: String,
    pub date: String,
    pub message_id: String,
    pub lines: u64,
}

impl Overview {
    /// Parse one `OVER` line: number, subject, from, date, message-id,
    /// references, bytes, lines — tab separated, in that fixed order (RFC 3977
    /// §8.3.2 defines this as the mandatory initial set).
    pub fn parse(line: &str) -> Option<Overview> {
        let mut f = line.split('\t');
        let number = f.next()?.trim().parse::<u64>().ok()?;
        let subject = f.next().unwrap_or("").to_string();
        let from = f.next().unwrap_or("").to_string();
        let date = f.next().unwrap_or("").to_string();
        let message_id = f.next().unwrap_or("").to_string();
        let _references = f.next().unwrap_or("");
        let _bytes = f.next().unwrap_or("");
        let lines = f.next().unwrap_or("").trim().parse::<u64>().unwrap_or(0);
        Some(Overview {
            number,
            subject,
            from,
            date,
            message_id,
            lines,
        })
    }

    /// The display name of the author: `Real Name <addr>` keeps the name, a bare
    /// address keeps the local part.
    pub fn author(&self) -> &str {
        if let Some(i) = self.from.find('<') {
            let name = self.from[..i].trim().trim_matches('"');
            if !name.is_empty() {
                return name;
            }
        }
        self.from.split('@').next().unwrap_or(&self.from).trim()
    }
}

/// Build an [`Overview`] from a parsed mbox message (the local spool backend has
/// no `OVER` command, so the headers stand in for it).
fn overview_from_msg(number: u64, msg: &zmax_core::rmail::Msg) -> Overview {
    Overview {
        number,
        subject: msg.subject().to_string(),
        from: msg.from().to_string(),
        date: msg.header("Date").unwrap_or("").to_string(),
        message_id: msg.header("Message-ID").unwrap_or("").to_string(),
        lines: msg.body.lines().count() as u64,
    }
}

// --- NNTP transport ----------------------------------------------------------

/// A blocking NNTP connection. Not `async`; the commands call it from a
/// `spawn_blocking`-style job, like the IRC client's transport.
pub struct Nntp {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    /// The server's greeting line, shown in the group buffer's mode line.
    pub greeting: String,
}

impl Nntp {
    /// Connect to `addr` (`host` or `host:port`, default port 119), read the
    /// greeting and switch the session into reader mode. `MODE READER` is
    /// required by servers that also serve peers; a server that rejects it still
    /// works, so its reply is not fatal.
    pub fn connect(addr: &str) -> std::io::Result<Nntp> {
        let addr = if addr.contains(':') {
            addr.to_string()
        } else {
            format!("{addr}:119")
        };
        let stream = TcpStream::connect(&addr)?;
        stream.set_read_timeout(Some(Duration::from_secs(20)))?;
        stream.set_write_timeout(Some(Duration::from_secs(20)))?;
        let reader = BufReader::new(stream.try_clone()?);
        let mut conn = Nntp {
            stream,
            reader,
            greeting: String::new(),
        };
        conn.greeting = conn.read_status()?;
        if !(conn.greeting.starts_with("200") || conn.greeting.starts_with("201")) {
            return Err(std::io::Error::other(format!(
                "server refused the connection: {}",
                conn.greeting
            )));
        }
        // Posting-capable servers answer 200, reader-only ones 201; either way a
        // rejected MODE READER just means the server was already in reader mode.
        let _ = conn.command("MODE READER");
        Ok(conn)
    }

    /// Send a command line and return its status line.
    pub fn command(&mut self, line: &str) -> std::io::Result<String> {
        self.stream.write_all(line.as_bytes())?;
        self.stream.write_all(b"\r\n")?;
        self.stream.flush()?;
        self.read_status()
    }

    /// Read a single status line (`NNN text`).
    fn read_status(&mut self) -> std::io::Result<String> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed",
            ));
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    /// Read a dot-terminated multi-line block, undoing the leading-dot stuffing
    /// (RFC 3977 §3.1.1). Must be called immediately after a status line whose
    /// code announces a multi-line response.
    fn read_block(&mut self) -> std::io::Result<Vec<String>> {
        let mut out = Vec::new();
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "truncated response",
                ));
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line == "." {
                return Ok(out);
            }
            out.push(
                line.strip_prefix("..")
                    .map(|r| format!(".{r}"))
                    .unwrap_or_else(|| line.to_string()),
            );
        }
    }

    /// Run a command that returns a multi-line block, checking the status code.
    fn block_command(&mut self, line: &str, want: &str) -> std::io::Result<Vec<String>> {
        let status = self.command(line)?;
        if !status.starts_with(want) {
            return Err(std::io::Error::other(status));
        }
        self.read_block()
    }
}

// --- the server abstraction --------------------------------------------------

/// Where articles come from. See the module docs for how one is chosen.
pub enum Server {
    /// A live NNTP session plus the address it was opened on.
    Nntp { conn: Box<Nntp>, addr: String },
    /// A directory of mbox files, one per group.
    Local(PathBuf),
}

impl Server {
    /// Open the news source named by `spec`:
    ///
    /// * empty — `$NNTPSERVER` if set, otherwise the local spool;
    /// * `local` or a path — the local mbox spool rooted there;
    /// * anything else — an NNTP address.
    pub fn open(spec: &str) -> std::io::Result<Server> {
        let spec = spec.trim();
        if spec.is_empty() {
            return match std::env::var("NNTPSERVER") {
                Ok(addr) if !addr.trim().is_empty() => Server::open(addr.trim()),
                _ => Ok(Server::Local(default_spool())),
            };
        }
        if spec == "local" {
            return Ok(Server::Local(default_spool()));
        }
        if spec.starts_with('/') || spec.starts_with("~/") || spec.starts_with("./") {
            return Ok(Server::Local(expand_tilde(spec)));
        }
        Nntp::connect(spec).map(|conn| Server::Nntp {
            conn: Box::new(conn),
            addr: spec.to_string(),
        })
    }

    /// A one-line description for the group buffer's mode line.
    pub fn describe(&self) -> String {
        match self {
            Server::Nntp { addr, .. } => format!("nntp:{addr}"),
            Server::Local(dir) => format!("spool:{}", dir.display()),
        }
    }

    /// Every group the server carries, as `(name, low, high)`.
    ///
    /// NNTP: `LIST ACTIVE`, whose lines are `group high low status`. The local
    /// spool: one entry per readable file in the spool directory, numbered
    /// `1..=n` over the mbox messages it holds.
    pub fn list_active(&mut self) -> std::io::Result<Vec<(String, u64, u64)>> {
        match self {
            Server::Nntp { conn, .. } => {
                let lines = conn.block_command("LIST ACTIVE", "215")?;
                let mut out = Vec::with_capacity(lines.len());
                for line in lines {
                    let mut f = line.split_whitespace();
                    let (Some(name), Some(high), Some(low)) = (f.next(), f.next(), f.next()) else {
                        continue;
                    };
                    let high = high.parse::<u64>().unwrap_or(0);
                    let low = low.parse::<u64>().unwrap_or(0);
                    out.push((name.to_string(), low, high));
                }
                Ok(out)
            }
            Server::Local(dir) => {
                let mut out = Vec::new();
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
                    let count = zmax_core::rmail::parse_mbox(&text).len() as u64;
                    out.push((name, if count == 0 { 0 } else { 1 }, count));
                }
                out.sort();
                Ok(out)
            }
        }
    }

    /// The overview lines for every article in `group`.
    pub fn overviews(
        &mut self,
        group: &str,
        low: u64,
        high: u64,
    ) -> std::io::Result<Vec<Overview>> {
        match self {
            Server::Nntp { conn, .. } => {
                let status = conn.command(&format!("GROUP {group}"))?;
                if !status.starts_with("211") {
                    return Err(std::io::Error::other(status));
                }
                if high == 0 || high < low {
                    return Ok(Vec::new());
                }
                // `OVER` is the RFC 3977 spelling; `XOVER` is the pre-standard
                // one many servers still answer, so fall back to it.
                let range = format!("{low}-{high}");
                let lines = match conn.block_command(&format!("OVER {range}"), "224") {
                    Ok(lines) => lines,
                    Err(_) => conn.block_command(&format!("XOVER {range}"), "224")?,
                };
                Ok(lines.iter().filter_map(|l| Overview::parse(l)).collect())
            }
            Server::Local(dir) => {
                let text = std::fs::read_to_string(dir.join(group))?;
                Ok(zmax_core::rmail::parse_mbox(&text)
                    .iter()
                    .enumerate()
                    .map(|(i, m)| overview_from_msg(i as u64 + 1, m))
                    .collect())
            }
        }
    }

    /// The full text (headers, blank line, body) of one article.
    pub fn article(&mut self, group: &str, number: u64) -> std::io::Result<String> {
        match self {
            Server::Nntp { conn, .. } => {
                let status = conn.command(&format!("GROUP {group}"))?;
                if !status.starts_with("211") {
                    return Err(std::io::Error::other(status));
                }
                let lines = conn.block_command(&format!("ARTICLE {number}"), "220")?;
                Ok(lines.join("\n"))
            }
            Server::Local(dir) => {
                let text = std::fs::read_to_string(dir.join(group))?;
                let msgs = zmax_core::rmail::parse_mbox(&text);
                let msg = msgs
                    .get(number.saturating_sub(1) as usize)
                    .ok_or_else(|| std::io::Error::other(format!("no article {number}")))?;
                let mut out = String::new();
                for (k, v) in &msg.headers {
                    out.push_str(k);
                    out.push_str(": ");
                    out.push_str(v);
                    out.push('\n');
                }
                out.push('\n');
                out.push_str(&msg.body);
                Ok(out)
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Server::Nntp { conn, .. } = self {
            let _ = conn.command("QUIT");
        }
    }
}

// --- paths -------------------------------------------------------------------

/// `~/News` — where the local mbox spool lives (Emacs's `nnfolder-directory`
/// default is `~/Mail`; news groups go under `~/News` to keep the two apart).
pub fn default_spool() -> PathBuf {
    home().join("News")
}

/// `~/.newsrc`, the news initialization file the manual names.
pub fn newsrc_path() -> PathBuf {
    home().join(".newsrc")
}

/// `~/.newsrc-zmax`, the killed/zombie sidecar (see [`parse_sidecar`]).
pub fn sidecar_path() -> PathBuf {
    home().join(".newsrc-zmax")
}

fn home() -> PathBuf {
    zmax_stdx::path::home_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

/// Split an article's text into its header block and body, the way the summary
/// buffer's article pane shows it.
pub fn split_article(text: &str) -> (Vec<(String, String)>, String) {
    let (head, body) = match text.split_once("\n\n") {
        Some((h, b)) => (h, b),
        None => (text, ""),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        // A continuation line (leading whitespace) belongs to the header above.
        if line.starts_with([' ', '\t']) {
            if let Some(last) = headers.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    (headers, body.to_string())
}

/// Read a file, returning `None` when it does not exist (so a first run can be
/// told apart from an empty file).
pub fn read_optional(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newsrc_round_trips() {
        let text = "comp.lang.rust: 1-100,105\nnews.announce! 1-5\noptions -n all\n";
        let groups = parse_newsrc(text);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "comp.lang.rust");
        assert_eq!(groups[0].level, Level::Subscribed);
        assert_eq!(groups[0].read, vec![(1, 100), (105, 105)]);
        assert_eq!(groups[1].level, Level::Unsubscribed);
        assert_eq!(
            format_newsrc(&groups),
            "comp.lang.rust: 1-100,105\nnews.announce! 1-5\n"
        );
    }

    #[test]
    fn killed_groups_are_not_written_to_newsrc() {
        // The manual: "Killed groups are not recorded in the `.newsrc' file".
        let mut groups = parse_newsrc("a.b: 1\n");
        groups.push(Group::new("dead.group", Level::Killed));
        groups.push(Group::new("new.group", Level::Zombie));
        assert_eq!(format_newsrc(&groups), "a.b: 1\n");
        assert_eq!(format_sidecar(&groups), "K dead.group\nZ new.group\n");
        let back = parse_sidecar(&format_sidecar(&groups));
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].level, Level::Killed);
        assert_eq!(back[1].level, Level::Zombie);
    }

    #[test]
    fn unread_count_and_marking() {
        let mut g = Group::new("g", Level::Subscribed);
        g.low = 10;
        g.high = 20;
        assert_eq!(g.unread(), 11);
        g.read = vec![(1, 15)]; // ranges may reach below the server's window
        assert_eq!(g.unread(), 5);
        g.mark_read(16);
        assert_eq!(g.read, vec![(1, 16)]);
        g.mark_read(18);
        assert_eq!(g.read, vec![(1, 16), (18, 18)]);
        g.mark_read(17);
        assert_eq!(g.read, vec![(1, 18)]);
        assert_eq!(g.unread(), 2);
        g.mark_unread(5);
        assert_eq!(g.read, vec![(1, 4), (6, 18)]);
    }

    #[test]
    fn listings_select_the_documented_subsets() {
        let mut subscribed_unread = Group::new("a", Level::Subscribed);
        subscribed_unread.low = 1;
        subscribed_unread.high = 3;
        let mut subscribed_read = Group::new("b", Level::Subscribed);
        subscribed_read.low = 1;
        subscribed_read.high = 3;
        subscribed_read.read = vec![(1, 3)];
        let unsub = Group::new("c", Level::Unsubscribed);
        let killed = Group::new("d", Level::Killed);
        let zombie = Group::new("e", Level::Zombie);

        // `l` — subscribed AND unread only.
        assert!(subscribed_unread.in_listing(Listing::Unread));
        assert!(!subscribed_read.in_listing(Listing::Unread));
        assert!(!unsub.in_listing(Listing::Unread));
        // `L` — subscribed + unsubscribed, never killed or zombie.
        assert!(subscribed_read.in_listing(Listing::All));
        assert!(unsub.in_listing(Listing::All));
        assert!(!killed.in_listing(Listing::All));
        assert!(!zombie.in_listing(Listing::All));
        // `A k` / `A z`.
        assert!(killed.in_listing(Listing::Killed));
        assert!(zombie.in_listing(Listing::Zombies));
    }

    #[test]
    fn first_run_kills_unknown_groups_later_runs_zombify_them() {
        let active = vec![
            ("comp.lang.rust".to_string(), 1, 40),
            ("alt.test".to_string(), 5, 9),
        ];
        let mut first = parse_newsrc("comp.lang.rust: 1-10\n");
        merge_active(&mut first, &active, true);
        assert_eq!(first[0].high, 40);
        assert_eq!(first[1].name, "alt.test");
        assert_eq!(first[1].level, Level::Killed);

        let mut later = parse_newsrc("comp.lang.rust: 1-10\n");
        merge_active(&mut later, &active, false);
        assert_eq!(later[1].level, Level::Zombie);
    }

    #[test]
    fn overview_line_parses() {
        let line = "42\tRe: ranges\tJane Doe <j@example.com>\tMon, 1 Jan 2035 00:00:00 +0000\t<abc@example.com>\t<prev@example.com>\t2048\t17\tXref: x";
        let ov = Overview::parse(line).unwrap();
        assert_eq!(ov.number, 42);
        assert_eq!(ov.subject, "Re: ranges");
        assert_eq!(ov.message_id, "<abc@example.com>");
        assert_eq!(ov.lines, 17);
        assert_eq!(ov.author(), "Jane Doe");
        assert!(Overview::parse("not-a-number\tx").is_none());
    }

    #[test]
    fn bare_address_author_falls_back_to_the_local_part() {
        let ov = Overview {
            from: "someone@example.com".into(),
            ..Overview::default()
        };
        assert_eq!(ov.author(), "someone");
    }

    #[test]
    fn article_splits_headers_from_body() {
        let (headers, body) = split_article(
            "From: a@b.c\nSubject: Long\n  continued\n\nBody line one.\nBody line two.",
        );
        assert_eq!(headers[0], ("From".into(), "a@b.c".into()));
        assert_eq!(headers[1], ("Subject".into(), "Long continued".into()));
        assert_eq!(body, "Body line one.\nBody line two.");
    }

    #[test]
    fn range_syntax_survives_junk() {
        assert_eq!(
            parse_ranges("1-3,,7,bad,9-4,12"),
            vec![(1, 3), (7, 7), (12, 12)]
        );
        assert_eq!(format_ranges(&[(1, 3), (7, 7)]), "1-3,7");
        assert_eq!(format_ranges(&[]), "");
    }

    /// The local spool backend end to end: the directory listing becomes the
    /// active list, the mbox messages become numbered overviews, and one article
    /// comes back as a header block plus its body.
    #[test]
    fn local_spool_lists_groups_and_serves_articles() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            dir.path().join("comp.lang.rust"),
            "From a@b.c Mon Jan  1 00:00:00 2035\n\
             From: Ann Author <a@b.c>\n\
             Subject: borrow checker\n\n\
             The borrow checker rejected my code.\n\
             \n\
             From d@e.f Tue Jan  2 00:00:00 2035\n\
             From: Bob Builder <d@e.f>\n\
             Subject: lifetimes\n\n\
             Lifetimes are elided here.\n\n",
        )
        .expect("write group");
        // A dotfile in the spool is not a group.
        std::fs::write(dir.path().join(".hidden"), "").expect("write dotfile");

        let mut server = Server::Local(dir.path().to_path_buf());
        assert!(server.describe().starts_with("spool:"));
        let active = server.list_active().expect("list active");
        assert_eq!(active, vec![("comp.lang.rust".to_string(), 1, 2)]);

        let ovs = server.overviews("comp.lang.rust", 1, 2).expect("overviews");
        assert_eq!(ovs.len(), 2);
        assert_eq!(ovs[0].number, 1);
        assert_eq!(ovs[0].subject, "borrow checker");
        assert_eq!(ovs[1].author(), "Bob Builder");

        let text = server.article("comp.lang.rust", 2).expect("article");
        let (headers, body) = split_article(&text);
        assert!(headers
            .iter()
            .any(|(k, v)| k == "Subject" && v == "lifetimes"));
        assert!(body.contains("Lifetimes are elided here."));
        assert!(server.article("comp.lang.rust", 99).is_err());
    }
}
