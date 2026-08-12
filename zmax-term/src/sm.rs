//! Shared substrate for the spacemacs *integration* layers (`+tools`,
//! `+web-services`, `+music`, `+readers`, `+chat`).
//!
//! Every one of those layers is the same shape: an emacs command drives either
//! an external CLI or a JSON/HTTP endpoint and shows the result in a buffer. The
//! zmax ports are `:` commands, so they all need the same three things — run a
//! program and capture its output, fetch/post JSON, and hand back either a
//! status-line string or a page to drop into a scratch buffer. Those live here
//! so each layer module stays the layer's own logic.

use std::process::Command;
use std::time::Duration;

use serde_json::Value;

/// What a layer command produced: a status line, and optionally a page of text
/// for the caller to show in a scratch buffer.
pub struct Outcome {
    pub status: String,
    pub page: Option<String>,
}

impl Outcome {
    /// Status line only.
    pub fn status(msg: impl Into<String>) -> Self {
        Self {
            status: msg.into(),
            page: None,
        }
    }

    /// A page plus the status line that summarises it.
    pub fn page(status: impl Into<String>, page: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            page: Some(page.into()),
        }
    }
}

/// Run `program` with `args`, returning its stdout. A non-zero exit becomes an
/// `Err` carrying stderr (falling back to stdout when stderr is empty), which is
/// what the caller wants on the status line.
pub fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program).args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("`{program}` not found on PATH")
        } else {
            format!("{program}: {e}")
        }
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let detail = if stderr.is_empty() {
        stdout.trim().to_string()
    } else {
        stderr
    };
    Err(format!(
        "{program} exited {}: {detail}",
        out.status.code().unwrap_or(-1)
    ))
}

/// Run `program` with `args` and feed `stdin` to it, returning stdout.
pub fn run_with_stdin(program: &str, args: &[&str], stdin: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("`{program}` not found on PATH")
            } else {
                format!("{program}: {e}")
            }
        })?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| format!("{program}: no stdin"))?
        .write_all(stdin.as_bytes())
        .map_err(|e| format!("{program}: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("{program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "{program} exited {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// True when `program` is on `PATH` — used to give a layer's first command a
/// precise "install X" error instead of a generic spawn failure.
pub fn have(program: &str) -> bool {
    which(program).is_some()
}

/// Absolute path of `program` on `PATH`, if any.
pub fn which(program: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(program);
            candidate.is_file().then_some(candidate)
        })
    })
}

/// A `ureq` agent with the timeouts every layer wants.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(5)
        .timeout(Duration::from_secs(20))
        .build()
}

/// HTTP GET returning the body as text. `headers` are `(name, value)` pairs.
pub fn http_get(url: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    let mut req = agent().get(url).set("User-Agent", "zmax");
    for (k, v) in headers {
        req = req.set(k, v);
    }
    req.call()
        .map_err(|e| format!("{e}"))?
        .into_string()
        .map_err(|e| format!("read body: {e}"))
}

/// HTTP GET returning parsed JSON.
pub fn http_get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value, String> {
    let body = http_get(url, headers)?;
    serde_json::from_str(&body).map_err(|e| format!("parse json: {e}"))
}

/// HTTP POST of a JSON body, returning parsed JSON. A 409 reply is returned as
/// an `Err` carrying the body, which is how Transmission hands back its
/// session id.
pub fn http_post_json(url: &str, headers: &[(&str, &str)], body: &Value) -> Result<Value, String> {
    let mut req = agent().post(url).set("User-Agent", "zmax");
    for (k, v) in headers {
        req = req.set(k, v);
    }
    match req.send_json(body.clone()) {
        Ok(resp) => {
            let text = resp.into_string().map_err(|e| format!("read body: {e}"))?;
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            serde_json::from_str(&text).map_err(|e| format!("parse json: {e}"))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let header = resp
                .header("X-Transmission-Session-Id")
                .unwrap_or_default()
                .to_string();
            let text = resp.into_string().unwrap_or_default();
            Err(format!("http {code}\u{1}{header}\u{1}{text}"))
        }
        Err(e) => Err(format!("{e}")),
    }
}

/// Split the `\u{1}`-joined parts of an [`http_post_json`] status error into
/// `(code, header, body)`.
pub fn split_status_error(err: &str) -> Option<(u16, String, String)> {
    let mut parts = err.split('\u{1}');
    let code = parts.next()?.trim_start_matches("http ").trim().parse().ok()?;
    Some((
        code,
        parts.next()?.to_string(),
        parts.next().unwrap_or("").to_string(),
    ))
}

/// Percent-encode a query-string value (everything outside the unreserved set).
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A `═`-underlined page heading, the shape the existing layer buffers
/// (`:slack-buffer`, `:irc-view`) already use.
pub fn heading(title: &str) -> String {
    format!("{title}\n{}\n\n", "═".repeat(title.chars().count().max(8)))
}

/// Truncate `s` to `width` display columns, ending with `…` when it was cut.
pub fn ellipsize(s: &str, width: usize) -> String {
    let clean: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= width {
        return clean;
    }
    let kept: String = clean.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_escapes_everything_outside_the_unreserved_set() {
        assert_eq!(urlencode("a b&c=d/e"), "a%20b%26c%3Dd%2Fe");
        assert_eq!(urlencode("safe-_.~"), "safe-_.~");
    }

    #[test]
    fn ellipsize_collapses_whitespace_and_marks_a_cut() {
        assert_eq!(ellipsize("a   b\nc", 10), "a b c");
        assert_eq!(ellipsize("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn status_errors_round_trip_their_parts() {
        let err = "http 409\u{1}abc123\u{1}body text";
        assert_eq!(
            split_status_error(err),
            Some((409, "abc123".to_string(), "body text".to_string()))
        );
    }
}
