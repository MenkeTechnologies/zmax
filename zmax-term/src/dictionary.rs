//! Emacs `dictionary-tooltip-mode` (dictionary.el): while it is on, the word
//! under the mouse pointer is looked up in a dictionary server and its
//! definition is shown in a tooltip.
//!
//! Emacs talks to a DICT server — RFC 2229, `dictionary-server` "dict.org" on
//! port 2628 by default — and zmax speaks the same protocol over a plain
//! `TcpStream`. The tooltip is a zmax popup, which is what a terminal frame has
//! instead of a GUI tooltip window.
//!
//! [`parse_definitions`] is the whole protocol reply parser and is pure, so the
//! response handling is tested without a socket.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// `dictionary-server`.
pub const DEFAULT_SERVER: &str = "dict.org";
/// `dictionary-port`.
pub const DEFAULT_PORT: u16 = 2628;
/// `dictionary-default-dictionary`: `*` asks every database on the server.
pub const DEFAULT_DATABASE: &str = "*";

/// How long a lookup may spend connecting or waiting for the reply. Emacs uses
/// an asynchronous process with no explicit deadline; a tooltip that never
/// arrives is worse than one that gives up, so zmax bounds it.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Whether `dictionary-tooltip-mode` is on.
static TOOLTIP_MODE: AtomicBool = AtomicBool::new(false);

/// The server to query, `(host, port)`. Configurable so a local `dictd` can be
/// used instead of the public one.
static SERVER: Mutex<Option<(String, u16)>> = Mutex::new(None);

/// Whether `dictionary-tooltip-mode` is on.
pub fn tooltip_mode() -> bool {
    TOOLTIP_MODE.load(Ordering::Relaxed)
}

/// Toggle `dictionary-tooltip-mode`, returning the new state.
pub fn toggle_tooltip_mode() -> bool {
    !TOOLTIP_MODE.fetch_xor(true, Ordering::Relaxed)
}

/// The configured `(dictionary-server, dictionary-port)`.
pub fn server() -> (String, u16) {
    SERVER
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_else(|| (DEFAULT_SERVER.to_string(), DEFAULT_PORT))
}

/// Set `dictionary-server` / `dictionary-port`.
pub fn set_server(host: &str, port: u16) {
    if let Ok(mut s) = SERVER.lock() {
        *s = Some((host.to_string(), port));
    }
}

/// The word the pointer was last over, so sweeping across the same word does not
/// re-query the server on every mouse-move event.
static HOVERED: Mutex<Option<String>> = Mutex::new(None);

/// Record `word` as the one under the pointer. Returns whether it *changed* —
/// i.e. whether a lookup is worth making.
pub fn note_hover(word: &str) -> bool {
    let Ok(mut hovered) = HOVERED.lock() else {
        return false;
    };
    if hovered.as_deref() == Some(word) {
        return false;
    }
    *hovered = Some(word.to_string());
    true
}

/// The pointer left the word it was over (or the mode was turned off).
pub fn forget_hover() {
    if let Ok(mut hovered) = HOVERED.lock() {
        *hovered = None;
    }
}

/// One definition from a DICT `DEFINE` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// The headword the server matched.
    pub word: String,
    /// The database it came from, as the server names it in the `151` line.
    pub database: String,
    /// The definition body, newline-separated, with the protocol's leading-dot
    /// quoting undone.
    pub text: String,
}

/// Parse a DICT `DEFINE` reply (RFC 2229 §4.2). The reply is a `150 n
/// definitions retrieved` status, then `n` blocks each opening with `151 "word"
/// db "name"` and running to a line holding a single `.`, then `250 ok`.
///
/// A `552 no match` status yields no definitions. Pure — unit tested.
pub fn parse_definitions(reply: &str) -> Vec<Definition> {
    let mut out = Vec::new();
    let mut lines = reply.lines();
    while let Some(line) = lines.next() {
        if !line.starts_with("151 ") {
            continue;
        }
        let (word, database) = parse_151(&line[4..]);
        let mut body = String::new();
        for text in lines.by_ref() {
            let text = text.trim_end_matches('\r');
            if text == "." {
                break;
            }
            if !body.is_empty() {
                body.push('\n');
            }
            // RFC 2229 §2.4.2: a line that began with a period is sent with an
            // extra one prepended, so the doubled prefix collapses back to one.
            match text.strip_prefix('.') {
                Some(rest) if text.starts_with("..") => body.push_str(rest),
                _ => body.push_str(text),
            }
        }
        out.push(Definition {
            word,
            database,
            text: body.trim().to_string(),
        });
    }
    out
}

/// Split a `151` status line's argument list into `(word, database name)`. The
/// form is `"word" dbshort "db long name"`; the long name is preferred because
/// it is what Emacs shows above each definition.
fn parse_151(args: &str) -> (String, String) {
    let mut quoted = Vec::new();
    let mut bare = Vec::new();
    let mut rest = args.trim();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('"') {
            match after.find('"') {
                Some(end) => {
                    quoted.push(after[..end].to_string());
                    rest = after[end + 1..].trim_start();
                }
                None => {
                    quoted.push(after.to_string());
                    break;
                }
            }
        } else {
            let end = rest.find(' ').unwrap_or(rest.len());
            bare.push(rest[..end].to_string());
            rest = rest[end..].trim_start();
        }
    }
    let word = quoted.first().cloned().unwrap_or_default();
    let database = quoted
        .get(1)
        .cloned()
        .or_else(|| bare.first().cloned())
        .unwrap_or_default();
    (word, database)
}

/// The word under a column in a line — what the pointer is hovering over.
/// Letters, digits, `-` and `'` make up a dictionary headword. Pure.
pub fn word_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '-' || c == '\'';
    let mut i = col.min(chars.len().checked_sub(1)?);
    if !is_word(chars[i]) {
        if i > 0 && is_word(chars[i - 1]) {
            i -= 1;
        } else {
            return None;
        }
    }
    let mut start = i;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = i;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    // A bare number or punctuation run is not a headword.
    word.chars()
        .any(|c| c.is_alphabetic())
        .then_some(word)
}

/// Look `word` up on the configured DICT server. Blocking — callers run it on a
/// `spawn_blocking` task.
pub fn define(word: &str) -> Result<Vec<Definition>, String> {
    let (host, port) = server();
    define_on(&host, port, DEFAULT_DATABASE, word)
}

/// [`define`] against an explicit server, so a local `dictd` can be used.
pub fn define_on(
    host: &str,
    port: u16,
    database: &str,
    word: &str,
) -> Result<Vec<Definition>, String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("{host}:{port}: {e}"))?
        .next()
        .ok_or_else(|| format!("{host}:{port}: no address"))?;
    let stream = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(TIMEOUT)).ok();
    stream.set_write_timeout(Some(TIMEOUT)).ok();
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);

    // The banner (220), then the request, then everything up to the closing
    // status line.
    let mut banner = String::new();
    reader.read_line(&mut banner).map_err(|e| e.to_string())?;
    if !banner.starts_with("220") {
        return Err(format!("dict: unexpected greeting: {}", banner.trim()));
    }
    // A quoted word keeps spaces and protocol characters out of the command.
    let request = format!("DEFINE {database} \"{}\"\r\n", word.replace('"', ""));
    writer
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;

    let mut reply = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        // 250 closes a successful DEFINE; 5xx is a failure status (552 = no
        // match), and both end the exchange.
        let terminal = trimmed.starts_with("250") || trimmed.starts_with('5');
        reply.push_str(trimmed);
        reply.push('\n');
        if terminal {
            break;
        }
    }
    let _ = writer.write_all(b"QUIT\r\n");
    Ok(parse_definitions(&reply))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = "220 dict.org dictd\n\
                         150 2 definitions retrieved\n\
                         151 \"emacs\" gcide \"The Collaborative International Dictionary\"\n\
                         Emacs \\Em\"acs\\, n.\n\
                         An extensible text editor.\n\
                         .\n\
                         151 \"emacs\" wn \"WordNet (r) 3.0\"\n\
                         emacs\n\
                             n 1: a text editor\n\
                         .\n\
                         250 ok\n";

    #[test]
    fn parses_every_definition_block() {
        let defs = parse_definitions(REPLY);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].word, "emacs");
        assert_eq!(defs[0].database, "The Collaborative International Dictionary");
        assert!(defs[0].text.starts_with("Emacs \\Em\"acs\\, n."));
        assert!(defs[0].text.ends_with("An extensible text editor."));
        assert_eq!(defs[1].database, "WordNet (r) 3.0");
        assert!(defs[1].text.contains("a text editor"));
    }

    #[test]
    fn a_no_match_reply_has_no_definitions() {
        let reply = "220 dict.org dictd\n552 no match\n";
        assert!(parse_definitions(reply).is_empty());
    }

    #[test]
    fn a_151_line_without_a_long_name_falls_back_to_the_short_one() {
        let (word, db) = parse_151("\"cat\" wn");
        assert_eq!(word, "cat");
        assert_eq!(db, "wn");
    }

    #[test]
    fn word_at_finds_the_headword_under_the_column() {
        let line = "the quick brown-fox jumps";
        assert_eq!(word_at(line, 4).as_deref(), Some("quick"));
        // Hyphenated words are one headword.
        assert_eq!(word_at(line, 12).as_deref(), Some("brown-fox"));
        // A space picks up the word that ends just before it.
        assert_eq!(word_at(line, 3).as_deref(), Some("the"));
        // Nothing to look up in a blank run or past the end.
        assert_eq!(word_at("   ", 1), None);
        assert_eq!(word_at("", 0), None);
        // A bare number is not a headword.
        assert_eq!(word_at("1234", 2), None);
    }
}
