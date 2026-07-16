//! Email — the zmax port of the GNU Emacs `message-mode` composition engine.
//!
//! `message-mode` is a buffer mode: you edit a mail draft as plain text, headers
//! above the [`HEADER_SEPARATOR`] line and the body below it, then a send command
//! turns that buffer into an RFC 5322 message. This module is the pure, tested
//! core of that: it builds the initial template, splits a draft buffer into
//! ordered headers + body, folds/assembles a wire-format message, validates
//! recipients, parses address lists, and produces the signature and MML
//! attachment markup the compose commands insert. It performs no I/O and knows
//! nothing about transport — the command layer decides where the assembled bytes
//! go (a draft/outbox file here, since there is no SMTP substrate yet).

/// The line message-mode puts between the header block and the body
/// (`mail-header-separator`). It is stripped when the draft is assembled.
pub const HEADER_SEPARATOR: &str = "--text follows this line--";

/// A parsed mail draft: headers in buffer order (duplicates preserved) plus the
/// body text below the separator.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Message {
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Message {
    /// First value of a header, matched case-insensitively (`To`, `to`, `TO`).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Assemble the RFC 5322 wire form: `Header: value` lines (long values
    /// folded), a blank line, then the body. Empty headers are dropped, matching
    /// message-mode's `message-send` which prunes blank header lines.
    pub fn assemble(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.headers {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            out.push_str(&fold_header(k, v));
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        out.push_str(&self.body);
        out
    }

    /// message-mode refuses to send a draft with no recipients. A draft is
    /// sendable when at least one of To/Cc/Bcc has a syntactically valid address.
    pub fn validate(&self) -> Result<(), String> {
        let mut any = false;
        for field in ["To", "Cc", "Bcc"] {
            if let Some(v) = self.header(field) {
                for addr in parse_addresses(v) {
                    if !is_valid_address(&addr) {
                        return Err(format!("invalid address in {field}: {addr}"));
                    }
                    any = true;
                }
            }
        }
        if !any {
            return Err("no recipients (fill in the To: header)".into());
        }
        Ok(())
    }
}

/// The initial `compose-mail` buffer: To/Subject headers and the separator.
pub fn compose_template(to: &str, subject: &str) -> String {
    format!("To: {to}\nSubject: {subject}\n{HEADER_SEPARATOR}\n")
}

/// Split a message-mode draft buffer into ordered headers + body.
///
/// Everything up to the [`HEADER_SEPARATOR`] (or, if absent, the first blank
/// line, per RFC 5322) is the header block; the rest is the body. Continuation
/// lines (starting with space/tab) are unfolded onto the preceding header.
pub fn parse_buffer(text: &str) -> Message {
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut lines = text.lines();
    let mut body_start_seen = false;

    for line in lines.by_ref() {
        if line == HEADER_SEPARATOR || line.is_empty() {
            body_start_seen = true;
            break;
        }
        if (line.starts_with(' ') || line.starts_with('\t')) && !headers.is_empty() {
            // Folded continuation of the previous header.
            let last = headers.last_mut().unwrap();
            last.1.push(' ');
            last.1.push_str(line.trim());
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
        // A non-header, non-continuation line before any separator is ignored
        // (message-mode would not produce one).
    }

    let body = if body_start_seen {
        lines.collect::<Vec<_>>().join("\n")
    } else {
        String::new()
    };
    Message { headers, body }
}

/// Split a comma-separated recipient list into bare addresses, unwrapping the
/// `Display Name <addr>` form to `addr`.
pub fn parse_addresses(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if let (Some(open), Some(close)) = (s.rfind('<'), s.rfind('>')) {
                if open < close {
                    return s[open + 1..close].trim().to_string();
                }
            }
            s.to_string()
        })
        .collect()
}

/// A deliberately small `local@domain.tld` syntactic check — a non-empty local
/// part, a single `@`, and a dotted domain with no spaces. Not a full RFC 5322
/// grammar; it is the "does this look like an address" guard message-mode-style
/// composition needs before saving a draft to the outbox.
pub fn is_valid_address(addr: &str) -> bool {
    let addr = addr.trim();
    if addr.contains(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = addr.split_once('@') else {
        return false;
    };
    if local.is_empty() || domain.contains('@') {
        return false;
    }
    let mut labels = domain.split('.');
    domain.contains('.') && labels.all(|l| !l.is_empty())
}

/// message-mode's signature block: the RFC 3676 `-- ` separator line (trailing
/// space is significant), then the signature text (`message-insert-signature`).
pub fn signature_block(signature: &str) -> String {
    format!("\n-- \n{}\n", signature.trim_end())
}

/// The MML tag `mml-attach-file` inserts to attach a file (`C-c C-a`). The
/// message-mode send step turns `<#part ...>` markup into a MIME part.
pub fn mml_attach_tag(path: &str, mime_type: &str) -> String {
    format!("<#part type=\"{mime_type}\" filename=\"{path}\" disposition=attachment>\n<#/part>\n")
}

/// Guess a MIME type from a filename extension, for [`mml_attach_tag`]. Falls
/// back to `application/octet-stream` like message-mode's `mailcap` default.
pub fn guess_mime_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "text" | "org" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "zip" => "application/zip",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

// ── header positioning (message-position-on-field) ──────────────────────────

/// Where the header block of a draft ends: the char offset of the start of the
/// [`HEADER_SEPARATOR`] line, or of the first blank line, or the end of the text.
fn header_block_end(text: &str) -> usize {
    let mut at = 0usize; // char offset of the current line's start
    for line in text.split('\n') {
        if line.trim_end_matches('\r') == HEADER_SEPARATOR || line.trim().is_empty() {
            return at;
        }
        at += line.chars().count() + 1;
    }
    text.chars().count()
}

/// `message-position-on-field`: put point at the end of FIELD's header line,
/// *creating the field* (empty, at the end of the header block) if the draft does
/// not have it. This is what `message-goto-fcc` / `message-goto-reply-to` / …
/// rely on — you can jump to a header the draft has never had and start typing.
///
/// Returns the (possibly rewritten) draft text and the char offset to put point
/// at: just after `Field: `, i.e. at the end of that line's existing value.
pub fn position_on_field(text: &str, field: &str) -> (String, usize) {
    let mut at = 0usize;
    let end = header_block_end(text);
    for line in text.split('\n') {
        let len = line.chars().count();
        if at >= end {
            break;
        }
        if let Some((k, _)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(field) {
                return (text.to_string(), at + len); // end of the existing line
            }
        }
        at += len + 1;
    }
    // Not present: insert `Field: ` as the last header line.
    let insert = format!("{field}: \n");
    let chars: Vec<char> = text.chars().collect();
    let head: String = chars[..end.min(chars.len())].iter().collect();
    let tail: String = chars[end.min(chars.len())..].iter().collect();
    let point = end + field.chars().count() + 2; // after "Field: "
    (format!("{head}{insert}{tail}"), point)
}

// ── mail aliases (~/.mailrc, `mail-abbrevs`) ────────────────────────────────

/// Parse a `.mailrc` into `(alias, definition)` pairs, in file order.
///
/// The mailrc lines that define an alias are `alias NAME DEFN…` and its synonym
/// `group NAME DEFN…` (Emacs's `mail-abbrev` reads both). The definition runs to
/// the end of the line; a trailing `\` continues onto the next. Recipients are
/// separated by whitespace and/or commas in mailrc, and are normalised to a
/// comma-separated list here — the form a `To:` header wants. Everything else
/// (`set`, `source`, comments) is ignored.
pub fn parse_mailrc(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut pending = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Line continuation: `alias big a@x.com \` + `b@y.com`.
        let (line, more) = match line.strip_suffix('\\') {
            Some(head) => (head.trim_end(), true),
            None => (line, false),
        };
        if !pending.is_empty() {
            pending.push(' ');
        }
        pending.push_str(line);
        if more {
            continue;
        }
        let joined = std::mem::take(&mut pending);
        let mut words = joined.split_whitespace();
        let Some(kw) = words.next() else { continue };
        if kw != "alias" && kw != "group" {
            continue;
        }
        let Some(name) = words.next() else { continue };
        let defn: Vec<&str> = words
            .flat_map(|w| w.split(','))
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .collect();
        if defn.is_empty() {
            continue;
        }
        // A redefinition replaces the earlier one, as re-reading .mailrc does.
        out.retain(|(k, _)| k != name);
        out.push((name.to_string(), defn.join(", ")));
    }
    out
}

/// Expand a mail alias to its address list, recursively: an alias's definition
/// may name other aliases (`alias all team, boss`). Self-reference and cycles
/// resolve to the literal word rather than looping. `None` if `name` is not an
/// alias at all.
pub fn expand_alias(table: &[(String, String)], name: &str) -> Option<String> {
    fn go(table: &[(String, String)], name: &str, seen: &mut Vec<String>, out: &mut Vec<String>) {
        if seen.iter().any(|s| s == name) {
            out.push(name.to_string()); // cycle: emit the word itself
            return;
        }
        match table.iter().find(|(k, _)| k == name) {
            Some((_, defn)) => {
                seen.push(name.to_string());
                for part in defn.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    go(table, part, seen, out);
                }
                seen.pop();
            }
            None => out.push(name.to_string()),
        }
    }
    table.iter().find(|(k, _)| k == name)?;
    let mut out = Vec::new();
    go(table, name, &mut Vec::new(), &mut out);
    Some(out.join(", "))
}

/// The `.mailrc` line that defines an alias (`define-mail-abbrev` appends this).
pub fn mailrc_alias_line(name: &str, defn: &str) -> String {
    let addrs: Vec<&str> = defn
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    format!("alias {name} {}\n", addrs.join(" "))
}

/// The alias word immediately before `pos` in `line` — what
/// `mail-abbrev-complete-alias` expands. Only the last comma-separated recipient
/// is considered, so `To: boss, te|` completes `te`.
pub fn alias_word_before(line: &str, pos: usize) -> &str {
    let upto = &line[..pos.min(line.len())];
    let start = upto
        .rfind(|c: char| c == ',' || c == ':' || c.is_whitespace())
        .map(|i| i + 1)
        .unwrap_or(0);
    upto[start..].trim()
}

// ── citing (message-yank-original, mail-fill-yanked-message) ────────────────

/// `message-yank-original` (`C-c C-y`): the original message, cited — an
/// attribution line built from its `From:`/`Date:` headers, then every body line
/// behind `prefix` (`message-yank-prefix`, "> " by default). Headers other than
/// the attribution are dropped, as message-mode's default `message-cite-function`
/// does. An empty body line gets the prefix trimmed of its trailing space, so the
/// citation has no trailing whitespace.
pub fn cite_message(original: &str, prefix: &str) -> String {
    let msg = parse_buffer(original);
    // A raw RFC 5322 message has no separator line; `parse_buffer` then splits on
    // the first blank line, which is the right thing here.
    let body = if msg.headers.is_empty() {
        original
    } else {
        msg.body.as_str()
    };
    let mut out = String::new();
    let who = msg.header("From").unwrap_or("").trim();
    let when = msg.header("Date").unwrap_or("").trim();
    match (who.is_empty(), when.is_empty()) {
        (false, false) => out.push_str(&format!("On {when}, {who} wrote:\n")),
        (false, true) => out.push_str(&format!("{who} writes:\n")),
        _ => {}
    }
    for line in body.lines() {
        if line.trim().is_empty() {
            out.push_str(prefix.trim_end());
        } else {
            out.push_str(prefix);
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// `mail-fill-yanked-message`: refill the cited paragraphs of a yanked message to
/// `width`, keeping each paragraph's citation prefix (`> `, `> > `, …) on every
/// line. Paragraphs are runs of consecutive lines sharing a prefix; a blank line
/// or a change of prefix starts a new one, so nested quotes are not merged.
pub fn fill_cited(text: &str, width: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut para_prefix = String::new();

    for line in text.lines() {
        let prefix = citation_prefix(line);
        let blank = line.trim().is_empty();
        // A blank line, or a different citation depth, closes the paragraph.
        if (blank || prefix != para_prefix) && !para.is_empty() {
            out.push(crate::text_engine::fill_paragraph(
                &para.join("\n"),
                width,
                &para_prefix,
            ));
            para.clear();
        }
        if blank {
            out.push(line.trim_end().to_string());
            para_prefix.clear();
            continue;
        }
        para_prefix = prefix.clone();
        para.push(line[prefix.len()..].trim().to_string());
    }
    if !para.is_empty() {
        out.push(crate::text_engine::fill_paragraph(
            &para.join("\n"),
            width,
            &para_prefix,
        ));
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// The citation prefix of a line: the leading run of `>`, spaces and tabs, up to
/// and including the last `>` (plus one following space if there is one).
/// `"> > text"` → `"> > "`, `"plain"` → `""`.
fn citation_prefix(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut last_gt = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'>' => last_gt = Some(i),
            b' ' | b'\t' => {}
            _ => break,
        }
        i += 1;
    }
    match last_gt {
        Some(g) => {
            let mut end = g + 1;
            if bytes.get(end) == Some(&b' ') {
                end += 1;
            }
            line[..end].to_string()
        }
        None => String::new(),
    }
}

/// Fold a header whose `Header: value` line exceeds 78 columns onto continuation
/// lines (RFC 5322 §2.2.3), breaking at spaces. Short headers pass through.
fn fold_header(key: &str, value: &str) -> String {
    let mut line = format!("{key}: {value}");
    if line.len() <= 78 {
        return line;
    }
    let mut out = String::new();
    let indent = "\r\n ";
    line = format!("{key}: ");
    let mut col = line.len();
    out.push_str(&line);
    for (i, word) in value.split_whitespace().enumerate() {
        if i > 0 && col + 1 + word.len() > 78 {
            out.push_str(indent);
            col = 1;
        } else if i > 0 {
            out.push(' ');
            col += 1;
        }
        out.push_str(word);
        col += word.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_has_headers_and_separator() {
        let t = compose_template("", "");
        assert!(t.starts_with("To: \nSubject: \n"));
        assert!(t.contains(HEADER_SEPARATOR));
    }

    #[test]
    fn parse_splits_headers_and_body() {
        let buf = "To: a@b.com\nSubject: Hi\n--text follows this line--\nHello\nworld\n";
        let m = parse_buffer(buf);
        assert_eq!(m.header("to"), Some("a@b.com"));
        assert_eq!(m.header("Subject"), Some("Hi"));
        assert_eq!(m.body, "Hello\nworld");
    }

    #[test]
    fn parse_unfolds_continuations() {
        let buf = "To: a@b.com,\n  c@d.com\nSubject: X\n\nbody";
        let m = parse_buffer(buf);
        assert_eq!(m.header("to"), Some("a@b.com, c@d.com"));
        assert_eq!(m.body, "body");
    }

    #[test]
    fn assemble_drops_empty_headers_and_uses_crlf() {
        let m = Message {
            headers: vec![
                ("To".into(), "a@b.com".into()),
                ("Cc".into(), "".into()),
                ("Subject".into(), "Hi".into()),
            ],
            body: "text".into(),
        };
        let wire = m.assemble();
        assert_eq!(wire, "To: a@b.com\r\nSubject: Hi\r\n\r\ntext");
        assert!(!wire.contains("Cc:"));
    }

    #[test]
    fn validate_requires_a_recipient() {
        let mut m = parse_buffer("To: \nSubject: x\n\nbody");
        assert!(m.validate().is_err());
        m.headers[0].1 = "a@b.com".into();
        assert!(m.validate().is_ok());
        m.headers[0].1 = "not-an-address".into();
        assert!(m.validate().is_err());
    }

    #[test]
    fn address_parsing_and_validation() {
        assert_eq!(
            parse_addresses("Jane Doe <jane@example.com>, bob@x.org"),
            vec!["jane@example.com".to_string(), "bob@x.org".to_string()]
        );
        assert!(is_valid_address("user@example.com"));
        assert!(is_valid_address("a.b+c@sub.domain.io"));
        assert!(!is_valid_address("nope"));
        assert!(!is_valid_address("no@domain"));
        assert!(!is_valid_address("has space@x.com"));
        assert!(!is_valid_address("two@@x.com"));
    }

    #[test]
    fn signature_and_attachment_markup() {
        assert_eq!(
            signature_block("Sent from zmax"),
            "\n-- \nSent from zmax\n"
        );
        let tag = mml_attach_tag("/tmp/a.png", "image/png");
        assert!(tag.contains("type=\"image/png\""));
        assert!(tag.contains("filename=\"/tmp/a.png\""));
        assert_eq!(guess_mime_type("x.PNG"), "image/png");
        assert_eq!(guess_mime_type("x.unknownext"), "application/octet-stream");
    }

    #[test]
    fn long_header_is_folded_under_78_cols() {
        let long = (0..20)
            .map(|i| format!("addr{i}@example.com"))
            .collect::<Vec<_>>()
            .join(" ");
        let folded = fold_header("To", &long);
        for line in folded.split("\r\n") {
            assert!(line.len() <= 78, "line too long: {line:?}");
        }
        // Unfolding (join continuations) recovers the original words.
        let recovered = folded.replace("\r\n ", " ");
        assert!(recovered.starts_with("To: addr0@example.com"));
    }

    /// `message-goto-fcc` on a draft that has no `Fcc:` must create the field —
    /// point lands right after `Fcc: `, ready to type, and the header block keeps
    /// its separator.
    #[test]
    fn position_on_field_creates_a_missing_header() {
        let draft = "To: a@x.com\nSubject: hi\n--text follows this line--\nbody\n";
        let (text, point) = position_on_field(draft, "Fcc");
        assert_eq!(
            text,
            "To: a@x.com\nSubject: hi\nFcc: \n--text follows this line--\nbody\n"
        );
        assert_eq!(
            text.chars().nth(point - 1),
            Some(' '),
            "point is after `Fcc: `"
        );
        assert_eq!(&text[..point], "To: a@x.com\nSubject: hi\nFcc: ");
    }

    /// An existing header is not duplicated: point goes to the end of its line.
    #[test]
    fn position_on_field_jumps_to_an_existing_header() {
        let draft = "To: a@x.com\nSubject: hi\n--text follows this line--\n";
        let (text, point) = position_on_field(draft, "subject"); // case-insensitive
        assert_eq!(text, draft, "unchanged");
        assert_eq!(&draft[..point], "To: a@x.com\nSubject: hi");
    }

    /// mailrc aliases: `alias` and `group`, comma- or space-separated recipients,
    /// backslash continuations, redefinition, and comments.
    #[test]
    fn mailrc_parses_aliases() {
        let rc = "# my aliases\n\
                  alias boss boss@example.com\n\
                  group team a@x.com, b@x.com\n\
                  alias big c@x.com \\\n   d@x.com\n\
                  set foo=bar\n\
                  alias boss newboss@example.com\n";
        let t = parse_mailrc(rc);
        assert_eq!(t.len(), 3);
        assert_eq!(
            expand_alias(&t, "team").as_deref(),
            Some("a@x.com, b@x.com")
        );
        assert_eq!(expand_alias(&t, "big").as_deref(), Some("c@x.com, d@x.com"));
        // The later definition of `boss` wins, as re-reading .mailrc does.
        assert_eq!(
            expand_alias(&t, "boss").as_deref(),
            Some("newboss@example.com")
        );
        assert_eq!(expand_alias(&t, "nobody"), None);
    }

    /// An alias may name other aliases; expansion is recursive, and a cycle
    /// terminates instead of hanging.
    #[test]
    fn alias_expansion_is_recursive_and_cycle_safe() {
        let t = parse_mailrc("alias a a1@x.com\nalias b b1@x.com\nalias all a, b\n");
        assert_eq!(
            expand_alias(&t, "all").as_deref(),
            Some("a1@x.com, b1@x.com")
        );

        let looped = parse_mailrc("alias x y\nalias y x\n");
        assert_eq!(expand_alias(&looped, "x").as_deref(), Some("x"));
    }

    /// The alias word being completed is the last recipient on the header line.
    #[test]
    fn alias_word_before_takes_the_last_recipient() {
        assert_eq!(alias_word_before("To: boss, te", 12), "te");
        assert_eq!(alias_word_before("To: te", 6), "te");
        assert_eq!(alias_word_before("To: ", 4), "");
    }

    /// The line `define-mail-abbrev` appends to ~/.mailrc.
    #[test]
    fn mailrc_alias_line_is_space_separated() {
        assert_eq!(
            mailrc_alias_line("team", "a@x.com, b@x.com"),
            "alias team a@x.com b@x.com\n"
        );
    }

    /// `message-yank-original`: attribution from From:/Date:, body behind the
    /// prefix, and no trailing whitespace on the blank cited lines.
    #[test]
    fn cite_message_prefixes_the_body_and_attributes_it() {
        let orig = "From: A <a@x.com>\nDate: Mon, 1 Jan 2024\nSubject: hi\n\nfirst\n\nsecond\n";
        assert_eq!(
            cite_message(orig, "> "),
            "On Mon, 1 Jan 2024, A <a@x.com> wrote:\n> first\n>\n> second\n"
        );
        // A bare body (no headers) is cited as-is.
        assert_eq!(cite_message("hello\n", "| "), "| hello\n");
    }

    /// `mail-fill-yanked-message`: each citation level is refilled on its own,
    /// keeping its prefix; blank lines and quote depth changes break paragraphs.
    #[test]
    fn fill_cited_refills_each_quote_level() {
        let cited = "> aaa bbb ccc ddd eee\n> fff\n\n> > deep quote here\n";
        let out = fill_cited(cited, 12);
        assert_eq!(
            out,
            "> aaa bbb\n> ccc ddd\n> eee fff\n\n> > deep\n> > quote\n> > here\n"
        );
    }
}
