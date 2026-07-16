//! `@web` search backend — a real web search for the AI chat's `@web` context (Cursor's `@web`).
//!
//! Honesty bar (same as `@codebase`/`@docs`): this performs a *real* network search and returns
//! live results — it is not a stub. The default backend scrapes DuckDuckGo's keyless HTML endpoint;
//! set `ZMAX_AI_WEB_SEARCH_URL` to a template containing `{query}` to route through any JSON/text
//! search API instead (the raw response body is handed to the model as context).
//!
//! Blocking (uses `ureq`) — call from `tokio::task::spawn_blocking`, like the chat providers.

use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Duration;

/// One web search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

const DDG_HTML: &str = "https://html.duckduckgo.com/html/";

/// Run a web search for `query`, returning up to `limit` results.
///
/// If `ZMAX_AI_WEB_SEARCH_URL` is set (a URL template containing `{query}`), GETs that endpoint
/// with the URL-encoded query substituted and returns its raw body as a single result — this lets a
/// user plug in Brave/Tavily/SerpAPI or any JSON search service. Otherwise scrapes DuckDuckGo.
pub fn search(query: &str, limit: usize) -> Result<Vec<WebResult>, String> {
    if let Ok(tmpl) = std::env::var("ZMAX_AI_WEB_SEARCH_URL") {
        if !tmpl.trim().is_empty() {
            let url = tmpl.replace("{query}", &encode(query));
            let body = http_get(&url)?;
            return Ok(vec![WebResult {
                title: format!("web search: {query}"),
                url,
                snippet: body.chars().take(4000).collect(),
            }]);
        }
    }
    let url = format!("{DDG_HTML}?q={}", encode(query));
    let html = http_get(&url)?;
    let results = parse_ddg(&html, limit);
    if results.is_empty() {
        return Err("no web results".into());
    }
    Ok(results)
}

/// Search and format the results as a plain-text block suitable for an AI context attachment.
pub fn search_context(query: &str, limit: usize) -> Result<String, String> {
    let results = search(query, limit)?;
    let mut out = String::new();
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!("[{}] {}\n{}\n", i + 1, r.title, r.url));
        if !r.snippet.is_empty() {
            out.push_str(&r.snippet);
            out.push('\n');
        }
        out.push('\n');
    }
    Ok(out)
}

fn http_get(url: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(15))
        .build();
    match agent
        .get(url)
        .set(
            "User-Agent",
            "Mozilla/5.0 (compatible; zmax/1.0; +https://github.com/MenkeTechnologies/zmax)",
        )
        .call()
    {
        Ok(resp) => resp.into_string().map_err(|e| format!("web: read: {e}")),
        Err(ureq::Error::Status(code, r)) => Err(format!(
            "web: HTTP {code}: {}",
            r.into_string().unwrap_or_default().trim()
        )),
        Err(e) => Err(format!("web: {e}")),
    }
}

static RESULT_A: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?s)class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#).unwrap());
static SNIPPET: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?s)class="result__snippet"[^>]*>(.*?)</a>"#).unwrap());
static TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());

/// Parse DuckDuckGo's HTML results page. Titles/urls and snippets each appear once per result in
/// document order, so we zip the two streams.
fn parse_ddg(html: &str, limit: usize) -> Vec<WebResult> {
    let titles: Vec<(String, String)> = RESULT_A
        .captures_iter(html)
        .map(|c| (real_url(&c[1]), clean(&c[2])))
        .collect();
    let snippets: Vec<String> = SNIPPET.captures_iter(html).map(|c| clean(&c[1])).collect();
    titles
        .into_iter()
        .enumerate()
        .take(limit)
        .map(|(i, (url, title))| WebResult {
            title,
            url,
            snippet: snippets.get(i).cloned().unwrap_or_default(),
        })
        .collect()
}

/// DuckDuckGo wraps result links as `//duckduckgo.com/l/?uddg=<percent-encoded-target>&rut=…`.
/// Pull out and decode the real destination; fall back to normalizing a protocol-relative href.
fn real_url(href: &str) -> String {
    if let Some(i) = href.find("uddg=") {
        let rest = &href[i + 5..];
        // The href is HTML-escaped, so the param boundary is `&amp;`; splitting on `&` is enough
        // since a percent-encoded value never contains a bare `&`.
        let enc = rest.split('&').next().unwrap_or(rest);
        return percent_decode(enc);
    }
    if let Some(stripped) = href.strip_prefix("//") {
        format!("https://{stripped}")
    } else {
        href.to_string()
    }
}

/// Strip HTML tags, unescape entities, and collapse whitespace.
fn clean(s: &str) -> String {
    let no_tags = TAG.replace_all(s, "");
    unescape(&no_tags)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Minimal `application/x-www-form-urlencoded`-style encoder for the query (no extra deps).
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decode a percent-encoded string (`%XX`, `+` → space).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trimmed capture of DuckDuckGo's real HTML results markup.
    const SAMPLE: &str = r#"
      <div class="result results_links">
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2Fureq%2Flatest%2Fureq%2F&amp;rut=abc">ureq - <b>Rust</b></a>
        <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2F">A simple, safe HTTP client &amp; more for <b>Rust</b>.</a>
      </div>
      <div class="result results_links">
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fgithub.com%2Falgesten%2Fureq&amp;rut=def">algesten/ureq</a>
        <a class="result__snippet" href="//duckduckgo.com/l/?uddg=x">Minimal request library.</a>
      </div>
    "#;

    #[test]
    fn parses_titles_urls_snippets() {
        let r = parse_ddg(SAMPLE, 10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].url, "https://docs.rs/ureq/latest/ureq/");
        assert_eq!(r[0].title, "ureq - Rust");
        assert_eq!(r[0].snippet, "A simple, safe HTTP client & more for Rust.");
        assert_eq!(r[1].url, "https://github.com/algesten/ureq");
        assert_eq!(r[1].title, "algesten/ureq");
    }

    #[test]
    fn limit_caps_results() {
        assert_eq!(parse_ddg(SAMPLE, 1).len(), 1);
    }

    #[test]
    fn encode_decode_roundtrip() {
        assert_eq!(encode("rust ureq/example"), "rust+ureq%2Fexample");
        assert_eq!(percent_decode("https%3A%2F%2Fa.b%2Fc"), "https://a.b/c");
    }
}
