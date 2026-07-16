//! `goto-address-mode` — find the URLs and e-mail addresses in buffer text.
//!
//! GNU Emacs' `goto-address.el` scans for `goto-address-url-regexp` and
//! `goto-address-mail-regexp` and buttonizes what it finds, so the addresses in a
//! comment or a plain-text file become clickable. This is that scan, pure and
//! line-at-a-time so the renderer can run it over the visible lines only.
//!
//! The URL is trimmed the way a reader would: trailing sentence punctuation and a
//! closing bracket that has no opener inside the URL are not part of it.

use std::ops::Range;

/// The schemes `goto-address-mode` recognises, plus the `www.` prefix Emacs also
/// accepts.
const SCHEMES: [&str; 6] = [
    "https://", "http://", "ftp://", "file://", "mailto:", "www.",
];

/// What was found at a span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressKind {
    /// A URL (`https://…`, `www.…`).
    Url,
    /// A bare e-mail address (`user@example.com`).
    Mail,
}

/// One address in a line: its byte range and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    /// Byte range within the line.
    pub range: Range<usize>,
    /// URL or e-mail.
    pub kind: AddressKind,
}

impl Address {
    /// The address text, ready to hand to a browser: a bare `www.` host gets the
    /// `https://` Emacs' `goto-address-at-point` prepends, and an e-mail gets
    /// `mailto:`.
    pub fn target(&self, line: &str) -> String {
        let raw = &line[self.range.clone()];
        match self.kind {
            AddressKind::Mail => format!("mailto:{raw}"),
            AddressKind::Url if raw.starts_with("www.") => format!("https://{raw}"),
            AddressKind::Url => raw.to_string(),
        }
    }
}

/// A character that can appear inside a URL body.
fn is_url_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&b)
}

/// A character that can appear in the local part or domain of an e-mail address.
fn is_mail_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._+%".contains(&b)
}

/// Trim what a reader would not consider part of the URL: trailing sentence
/// punctuation, and a trailing `)` / `]` that has no opener inside the URL (the
/// `(see https://x.org/a)` case).
fn trim_url(url: &str) -> &str {
    let mut end = url.len();
    let b = url.as_bytes();
    while end > 0 {
        let last = b[end - 1];
        let drop = match last {
            b'.' | b',' | b';' | b':' | b'!' | b'?' | b'\'' => true,
            b')' => {
                let body = &url[..end];
                body.matches('(').count() < body.matches(')').count()
            }
            b']' => {
                let body = &url[..end];
                body.matches('[').count() < body.matches(']').count()
            }
            _ => false,
        };
        if !drop {
            break;
        }
        end -= 1;
    }
    &url[..end]
}

/// Every URL and e-mail address in `line`, in source order, as byte ranges.
pub fn addresses(line: &str) -> Vec<Address> {
    let bytes = line.as_bytes();
    let mut out: Vec<Address> = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        // A URL must start at a word boundary, so a scheme spelled inside a longer
        // token (`xhttps://`) is not one.
        let at_boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let scheme = at_boundary
            .then(|| SCHEMES.iter().find(|s| line[i..].starts_with(**s)).copied())
            .flatten();
        if let Some(scheme) = scheme {
            let mut end = i + scheme.len();
            while end < bytes.len() && is_url_byte(bytes[end]) {
                end += 1;
            }
            let trimmed = trim_url(&line[i..end]);
            // A scheme with nothing after it is not an address.
            if trimmed.len() > scheme.len() {
                out.push(Address {
                    range: i..i + trimmed.len(),
                    kind: AddressKind::Url,
                });
                i += trimmed.len();
                continue;
            }
        }
        // A bare e-mail: walk back over the local part from an `@`.
        if bytes[i] == b'@' && i > 0 {
            let mut start = i;
            while start > 0 && is_mail_byte(bytes[start - 1]) {
                start -= 1;
            }
            let mut end = i + 1;
            while end < bytes.len() && is_mail_byte(bytes[end]) {
                end += 1;
            }
            let domain = &line[i + 1..end];
            // A domain needs a dot and something on either side of it, which is
            // what keeps `@mention` and `foo@bar` from being addresses.
            let plausible = domain
                .rsplit_once('.')
                .is_some_and(|(host, tld)| !host.is_empty() && tld.len() >= 2);
            // Don't re-report the tail of a `mailto:` URL already emitted.
            let inside_url = out
                .last()
                .is_some_and(|a| a.range.start <= start && start < a.range.end);
            if start < i && plausible && !inside_url {
                let trimmed = trim_url(&line[start..end]);
                out.push(Address {
                    range: start..start + trimmed.len(),
                    kind: AddressKind::Mail,
                });
                i = start + trimmed.len();
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(line: &str) -> Vec<(&str, AddressKind)> {
        addresses(line)
            .into_iter()
            .map(|a| (&line[a.range.clone()], a.kind))
            .collect()
    }

    #[test]
    fn finds_an_https_url() {
        assert_eq!(
            spans("see https://example.com/a for more"),
            vec![("https://example.com/a", AddressKind::Url)]
        );
    }

    #[test]
    fn trailing_sentence_punctuation_is_not_part_of_the_url() {
        assert_eq!(
            spans("visit https://example.com."),
            vec![("https://example.com", AddressKind::Url)]
        );
        assert_eq!(
            spans("a, https://example.com/x, and b"),
            vec![("https://example.com/x", AddressKind::Url)]
        );
    }

    #[test]
    fn an_unmatched_closing_paren_is_dropped_but_a_matched_one_is_kept() {
        assert_eq!(
            spans("(see https://example.com/a)"),
            vec![("https://example.com/a", AddressKind::Url)]
        );
        assert_eq!(
            spans("https://en.wikipedia.org/wiki/Foo_(bar)"),
            vec![("https://en.wikipedia.org/wiki/Foo_(bar)", AddressKind::Url)]
        );
    }

    #[test]
    fn a_bare_www_host_is_a_url_and_gets_a_scheme_for_the_browser() {
        let line = "www.gnu.org/software";
        let found = addresses(line);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, AddressKind::Url);
        assert_eq!(found[0].target(line), "https://www.gnu.org/software");
    }

    #[test]
    fn an_email_becomes_a_mailto_target() {
        let line = "mail user@example.com please";
        let found = addresses(line);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, AddressKind::Mail);
        assert_eq!(found[0].target(line), "mailto:user@example.com");
    }

    #[test]
    fn an_at_mention_is_not_an_email() {
        assert!(addresses("cc @someone about it").is_empty());
        assert!(addresses("user@localhost").is_empty(), "no dotted domain");
    }

    #[test]
    fn a_scheme_inside_a_longer_token_is_not_a_url() {
        assert!(addresses("xhttps://example.com").is_empty());
    }

    #[test]
    fn a_bare_scheme_with_no_host_is_not_a_url() {
        assert!(addresses("https:// is a scheme").is_empty());
    }

    #[test]
    fn a_mailto_url_is_reported_once_not_twice() {
        let line = "write to mailto:a@b.com now";
        let found = addresses(line);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, AddressKind::Url);
    }

    #[test]
    fn several_addresses_on_one_line_are_all_found() {
        assert_eq!(
            spans("http://a.org and b@c.org and https://d.net"),
            vec![
                ("http://a.org", AddressKind::Url),
                ("b@c.org", AddressKind::Mail),
                ("https://d.net", AddressKind::Url),
            ]
        );
    }

    #[test]
    fn a_line_with_no_address_yields_nothing() {
        assert!(addresses("just some prose, 2 + 2 = 4").is_empty());
        assert!(addresses("").is_empty());
    }
}
