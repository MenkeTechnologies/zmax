//! Email — the zemacs port of the GNU Emacs `message-mode` composition engine.
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
            signature_block("Sent from zemacs"),
            "\n-- \nSent from zemacs\n"
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
}
