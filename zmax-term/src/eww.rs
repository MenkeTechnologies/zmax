//! A real in-terminal web browser — the faithful port of Emacs `eww`.
//!
//! `eww` in Emacs fetches a URL over HTTP, renders the HTML to formatted text in
//! a normal buffer, and lets you read it without leaving the editor (as opposed
//! to `browse-url`, which shells out to the OS browser). This module reproduces
//! that: [`fetch`] pulls a document over HTTP with the already-vendored `ureq`
//! client, and [`html_to_text`] renders it to a readable plain-text page with
//! headings, list markers, and inline `[text](url)` links — the same shape as
//! eww's text rendering. No new dependencies: `ureq` (blocking HTTP) is already
//! used elsewhere in the tree, and the renderer is a self-contained tag walker.

/// Fetch `url` over HTTP(S) and return `(final_body, content_type)`. A bare host
/// (`example.com`) is promoted to `https://`. Runs on the blocking `ureq` agent,
/// so callers must invoke it from `spawn_blocking`, never the UI thread.
pub fn fetch(url: &str) -> Result<(String, String), String> {
    let url = normalize_url(url);
    let agent = ureq::AgentBuilder::new()
        .redirects(8)
        .timeout(std::time::Duration::from_secs(20))
        .build();
    match agent.get(&url).set("User-Agent", "zmax-eww").call() {
        Ok(resp) => {
            let ctype = resp
                .header("content-type")
                .unwrap_or("text/html")
                .to_string();
            resp.into_string()
                .map(|body| (body, ctype))
                .map_err(|e| format!("read body: {e}"))
        }
        Err(e) => Err(format!("{e}")),
    }
}

/// Promote a bare host to `https://`, leave explicit schemes and `file:` alone.
pub fn normalize_url(url: &str) -> String {
    let u = url.trim();
    if u.contains("://") || u.starts_with("file:") {
        u.to_string()
    } else {
        format!("https://{u}")
    }
}

/// Build a DuckDuckGo HTML-endpoint search URL for `query` (eww-search-words).
/// The `html.duckduckgo.com/html/` endpoint returns server-rendered results with
/// no JavaScript, so [`html_to_text`] can render them directly.
pub fn search_url(query: &str) -> String {
    let q: String = query
        .trim()
        .chars()
        .map(|c| match c {
            ' ' => "+".to_string(),
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            other => other
                .to_string()
                .bytes()
                .map(|b| format!("%{b:02X}"))
                .collect(),
        })
        .collect();
    format!("https://html.duckduckgo.com/html/?q={q}")
}

/// Render an HTML document to readable plain text: `<script>`/`<style>` dropped,
/// block elements broken onto their own lines, headings underlined, list items
/// bulleted, and `<a href>` links written as `text [url]`. HTML entities are
/// decoded. This is deliberately a lightweight tag walker, not a full layout
/// engine — the same tradeoff eww's text backend makes for terminal reading.
pub fn html_to_text(html: &str, base_url: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    // Text accumulated for the current line; flushed on block boundaries so we
    // collapse runs of whitespace the way a browser would.
    let mut line = String::new();
    let mut pending_link: Option<String> = None;
    let mut skip_until: Option<&'static str> = None;

    let flush = |line: &mut String, out: &mut String| {
        let trimmed = collapse_ws(line);
        if !trimmed.is_empty() {
            out.push_str(&trimmed);
            out.push('\n');
        }
        line.clear();
    };

    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Find tag end.
            let Some(end) = find_byte(bytes, i + 1, b'>') else {
                break;
            };
            let raw = &html[i + 1..end];
            let tag = tag_name(raw);
            let closing = raw.starts_with('/');

            if let Some(term) = skip_until {
                // Inside <script>/<style>: swallow everything until its close tag.
                if closing && tag == term {
                    skip_until = None;
                }
                i = end + 1;
                continue;
            }

            match tag.as_str() {
                "script" if !closing => skip_until = Some("script"),
                "style" if !closing => skip_until = Some("style"),
                "br" => {
                    flush(&mut line, &mut out);
                }
                "p" | "div" | "section" | "article" | "header" | "footer" | "table" | "tr"
                | "ul" | "ol" | "pre" | "blockquote" | "form" => {
                    flush(&mut line, &mut out);
                }
                "li" if !closing => {
                    flush(&mut line, &mut out);
                    line.push_str("  • ");
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    flush(&mut line, &mut out);
                    if closing {
                        // Underline the heading text just emitted.
                        if let Some(last) = out.trim_end_matches('\n').lines().last() {
                            let n = last.chars().count().min(80);
                            out.push_str(&"─".repeat(n));
                            out.push('\n');
                        }
                    }
                }
                "a" if !closing => {
                    pending_link = attr_value(raw, "href").map(|h| resolve_href(&h, base_url));
                }
                "a" if closing => {
                    if let Some(href) = pending_link.take() {
                        if !href.is_empty() && !line.ends_with(' ') {
                            line.push_str(&format!(" [{href}]"));
                        }
                    }
                }
                _ => {}
            }
            i = end + 1;
        } else {
            // Text run up to the next tag.
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            if skip_until.is_none() {
                line.push_str(&html[start..i]);
            }
        }
    }
    flush(&mut line, &mut out);

    let text = decode_entities(&out);
    // Collapse 3+ blank lines to a single blank line.
    let mut result = String::with_capacity(text.len());
    let mut blanks = 0;
    for l in text.lines() {
        if l.trim().is_empty() {
            blanks += 1;
            if blanks <= 1 {
                result.push('\n');
            }
        } else {
            blanks = 0;
            result.push_str(l);
            result.push('\n');
        }
    }
    result
}

fn find_byte(b: &[u8], from: usize, target: u8) -> Option<usize> {
    (from..b.len()).find(|&j| b[j] == target)
}

/// Extract the lowercased element name from raw tag inner text (`a href=..` →
/// `a`, `/div` → `div`).
fn tag_name(raw: &str) -> String {
    raw.trim_start_matches('/')
        .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Read an attribute value out of a raw tag (`href="x"`, `href='x'`, `href=x`).
fn attr_value(raw: &str, name: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let key = format!("{name}=");
    let pos = lower.find(&key)? + key.len();
    let rest = &raw[pos..];
    let val = if let Some(stripped) = rest.strip_prefix('"') {
        stripped.split('"').next().unwrap_or("")
    } else if let Some(stripped) = rest.strip_prefix('\'') {
        stripped.split('\'').next().unwrap_or("")
    } else {
        rest.split(|c: char| c.is_whitespace() || c == '>')
            .next()
            .unwrap_or("")
    };
    Some(val.to_string())
}

/// Resolve a possibly-relative href against the page's base URL.
fn resolve_href(href: &str, base: &str) -> String {
    let h = decode_entities(href);
    if h.contains("://") || h.starts_with("mailto:") || h.starts_with('#') {
        h
    } else if let Some(rest) = h.strip_prefix("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        format!("{scheme}://{rest}")
    } else if h.starts_with('/') {
        // Absolute path: graft onto scheme+host of the base.
        if let Some(scheme_end) = base.find("://") {
            let after = &base[scheme_end + 3..];
            let host = after.split('/').next().unwrap_or(after);
            format!("{}://{host}{h}", &base[..scheme_end])
        } else {
            h
        }
    } else {
        h
    }
}

/// Collapse internal whitespace runs to single spaces and trim the ends.
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            in_ws = true;
        } else {
            if in_ws && !out.is_empty() {
                out.push(' ');
            }
            in_ws = false;
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// Decode the HTML entities that actually show up in page text.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((idx, c)) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        // Read the entity up to ';' (max 10 chars to avoid runaway).
        let rest = &s[idx + 1..];
        if let Some(semi) = rest.find(';').filter(|&p| p <= 10) {
            let ent = &rest[..semi];
            let replaced = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" | "#160" => Some(' '),
                "mdash" | "#8212" => Some('—'),
                "ndash" | "#8211" => Some('–'),
                "hellip" | "#8230" => Some('…'),
                "copy" => Some('©'),
                "reg" => Some('®'),
                "trade" => Some('™'),
                _ => decode_numeric(ent),
            };
            if let Some(ch) = replaced {
                out.push(ch);
                // Advance the iterator past the consumed entity + ';'.
                for _ in 0..=semi {
                    chars.next();
                }
                continue;
            }
        }
        out.push('&');
    }
    out
}

/// Decode `#NNN` / `#xHH` numeric character references.
fn decode_numeric(ent: &str) -> Option<char> {
    let num = ent.strip_prefix('#')?;
    let code = if let Some(hex) = num.strip_prefix(['x', 'X']) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        num.parse::<u32>().ok()?
    };
    char::from_u32(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_links_and_entities() {
        let html = "<html><body><h1>Hi&amp;Bye</h1><p>See <a href=\"/x\">here</a> now.\
                    </p><script>ignore()</script><ul><li>one</li><li>two</li></ul></body></html>";
        let text = html_to_text(html, "https://example.com/dir/page");
        assert!(text.contains("Hi&Bye"), "entity decoded: {text}");
        assert!(text.contains('─'), "heading underlined: {text}");
        assert!(
            text.contains("here [https://example.com/x]"),
            "link resolved+rendered: {text}"
        );
        assert!(
            text.contains("• one") && text.contains("• two"),
            "list: {text}"
        );
        assert!(!text.contains("ignore"), "script dropped: {text}");
    }

    #[test]
    fn normalizes_and_searches() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("http://x.io"), "http://x.io");
        assert!(search_url("rust lang").contains("q=rust+lang"));
    }
}
