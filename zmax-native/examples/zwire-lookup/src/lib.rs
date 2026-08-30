//! Example plugin: hand a term to the `zwire-host` bus and let the OS open the
//! page for it, on any site.
//!
//! Demonstrates talking to another local daemon over its Unix socket from
//! inside the editor, degrading to the `zwire-host` CLI and then to the OS
//! opener when the bus is not listening.
//!
//! ```text
//! :plugin load .../libzmax_native_zwire_lookup.dylib
//!
//! :zwire-lookup UnixStream                 # the default site
//! :zwire-lookup mdn fetch                  # a named site
//! :zwire-lookup https://example.com/x      # a URL, opened as-is
//! :zwire-lookup gh ripgrep hidden files    # everything after the site is the term
//! :zwire-lookup                            # the word under the cursor
//! ```
//!
//! ## Sites
//!
//! A site is a URL template. `{}` marks where the term goes; a template without
//! one gets the term appended, so a bare prefix like
//! `https://wiki.example.com/` works. Built-in names are in [`BUILTIN_SITES`].
//!
//! Two environment variables extend this without touching the code:
//!
//! * `ZWIRE_LOOKUP_BASE` — the template used when no site is named.
//! * `ZWIRE_LOOKUP_SITES` — extra or overriding names, as
//!   `name=template;name=template`. These are consulted before the built-ins,
//!   so a name here replaces one.
//!
//! ```sh
//! export ZWIRE_LOOKUP_BASE='https://kagi.com/search?q={}'
//! export ZWIRE_LOOKUP_SITES='rfc=https://www.rfc-editor.org/rfc/rfc{}.html;zsh=https://zsh.sourceforge.io/Doc/Release/{}.html'
//! ```
//!
//! ## Wire contract
//!
//! `zwire-host` speaks newline-delimited JSON on a Unix socket. A request is an
//! object with a `cmd` field; the reply is one JSON line. This plugin uses two
//! of its commands:
//!
//! * `{"cmd":"open","target":"<url>"}` — hands the URL to `open`(1) on macOS,
//!   `xdg-open` on Linux, so it lands in the user's default browser. The reply
//!   is `{"ok":true}`, or `{"err":"...","ok":false}` — note that a failure
//!   reply serialises its keys alphabetically.
//! * `{"cmd":"clipboard_get"}` — reads the system clipboard, replying
//!   `{"ok":true,"text":"..."}`. Only a fallback now: the no-argument form asks
//!   the host for the word under the cursor first, and reaches for the
//!   clipboard just when there is no word there (on whitespace, say).
//!
//! The socket path follows `zwire_host::default_socket`: `$ZWIRE_HOST_SOCK`,
//! else `$XDG_RUNTIME_DIR/zwire-host.sock`, else `$TMPDIR/zwire-host.sock`,
//! else `/tmp/zwire-host-$USER.sock`. It is resolved here rather than linked
//! against zwire-host so the plugin stays a leaf binary with no dependencies
//! beyond the SDK.
//!
//! zmax also ships `:zwire-host` (alias `:zh`) for raw requests, so
//! `:zh {"cmd":"open","target":"https://…"}` already works. What this adds is
//! the term-to-URL step: site templates, percent-encoding, and the fallbacks.

use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::raw::c_int;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use zmax_native::{declare_plugin, Args, Host};

/// How long to wait on the daemon. A local socket answers in microseconds;
/// anything slower means it is wedged, and the editor must not block on it.
const TIMEOUT: Duration = Duration::from_millis(500);

/// The template used when no site is named and `ZWIRE_LOOKUP_BASE` is unset.
const DEFAULT_BASE: &str = "https://docs.rs/releases/search?query={}";

/// Names that resolve without any configuration. `{}` is where the term goes.
const BUILTIN_SITES: &[(&str, &str)] = &[
    ("docs", "https://docs.rs/releases/search?query={}"),
    ("crates", "https://crates.io/search?q={}"),
    ("rust", "https://doc.rust-lang.org/std/?search={}"),
    ("mdn", "https://developer.mozilla.org/en-US/search?q={}"),
    ("gh", "https://github.com/search?q={}"),
    ("so", "https://stackoverflow.com/search?q={}"),
    ("wiki", "https://en.wikipedia.org/w/index.php?search={}"),
    ("ddg", "https://duckduckgo.com/?q={}"),
    ("man", "https://man7.org/linux/man-pages/man1/{}.1.html"),
];

/// Where `zwire-host` binds, in the order it tries.
fn socket_path() -> PathBuf {
    if let Some(explicit) = env::var_os("ZWIRE_HOST_SOCK") {
        return PathBuf::from(explicit);
    }
    if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("zwire-host.sock");
    }
    if let Some(tmp) = env::var_os("TMPDIR") {
        return PathBuf::from(tmp).join("zwire-host.sock");
    }
    let user = env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    PathBuf::from("/tmp").join(format!("zwire-host-{user}.sock"))
}

/// Percent-encode a term for use inside a URL. Everything outside the RFC 3986
/// unreserved set is escaped, so a multi-word term, or one containing `&`, `#`
/// or `?`, cannot add parameters to the URL it lands in.
fn percent_encode(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for byte in term.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Minimal JSON string escaping, so a term with a quote or a backslash cannot
/// produce a malformed request that the daemon rejects confusingly.
fn escape_json(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Whether an argument is already a URL and should be opened untouched. A
/// scheme is the reliable signal; `www.` is accepted because people type it.
fn is_url(arg: &str) -> bool {
    arg.contains("://") || arg.starts_with("www.")
}

/// `name=template;name=template` from `ZWIRE_LOOKUP_SITES`. An entry without an
/// `=`, or with an empty name or template, is skipped rather than failing the
/// whole variable -- one typo should not take out every custom site.
fn configured_sites(raw: &str) -> Vec<(String, String)> {
    raw.split(';')
        .filter_map(|entry| {
            let (name, template) = entry.trim().split_once('=')?;
            let (name, template) = (name.trim(), template.trim());
            (!name.is_empty() && !template.is_empty())
                .then(|| (name.to_string(), template.to_string()))
        })
        .collect()
}

/// The template for `name`: a configured site first, so it can override a
/// built-in, then the built-ins.
fn site_template(name: &str, configured: &[(String, String)]) -> Option<String> {
    configured
        .iter()
        .rev()
        .find(|(site, _)| site == name)
        .map(|(_, template)| template.clone())
        .or_else(|| {
            BUILTIN_SITES
                .iter()
                .find(|(site, _)| *site == name)
                .map(|(_, template)| (*template).to_string())
        })
}

/// Put `term` into `template`: at `{}` if there is one, appended otherwise, so
/// a bare prefix like `https://example.com/docs/` works as a template.
fn fill(template: &str, term: &str) -> String {
    let encoded = percent_encode(term);
    if template.contains("{}") {
        template.replace("{}", &encoded)
    } else {
        format!("{template}{encoded}")
    }
}

/// Turn the command's arguments into the URL to open.
///
/// * a lone URL argument is taken as-is
/// * a first argument naming a site uses that template, with the rest as the
///   term
/// * anything else is the term for the default template
fn resolve_url(args: &[String], base: &str, configured: &[(String, String)]) -> Option<String> {
    let (first, rest) = args.split_first()?;

    if is_url(first) && rest.is_empty() {
        return Some(first.clone());
    }

    if !rest.is_empty() {
        if let Some(template) = site_template(first, configured) {
            return Some(fill(&template, &rest.join(" ")));
        }
    }

    Some(fill(base, &args.join(" ")))
}

/// Send one NDJSON request and return the reply line, or `None` when the daemon
/// is not listening.
fn call_daemon(request: &str) -> Option<String> {
    let stream = UnixStream::connect(socket_path()).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(TIMEOUT)).ok()?;

    let mut writer = &stream;
    writer.write_all(request.as_bytes()).ok()?;
    writer.write_all(b"\n").ok()?;
    writer.flush().ok()?;

    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply).ok()?;
    Some(reply)
}

/// The same request through `zwire-host call`. This does **not** rescue a
/// stopped daemon -- the CLI dials the same socket and fails the same way -- but
/// it does cover the case where zwire-host resolves the socket somewhere this
/// plugin did not look, which is the only difference between the two paths.
fn call_cli(request: &str) -> Option<String> {
    let out = Command::new("zwire-host")
        .args(["call", request])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A reply is `{"ok":true,...}` on success. Parsed by substring rather than by
/// pulling in a JSON crate: the plugin only needs the one field, and staying
/// dependency-free keeps the example a single file.
fn reply_is_ok(reply: &str) -> bool {
    reply.contains("\"ok\":true") || reply.contains("\"ok\": true")
}

/// Pull the clipboard out of a `clipboard_get` reply, whose shape is
/// `{"ok":true,"text":"..."}` -- the field is `text`, not `value`.
fn reply_text(reply: &str) -> Option<String> {
    let start = reply.find("\"text\"")?;
    let rest = &reply[start + "\"text\"".len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;

    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(value),
            '\\' => match chars.next()? {
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                other => value.push(other),
            },
            c => value.push(c),
        }
    }
    None
}

/// Open a URL through the OS directly. `zwire-host`'s `open` command does
/// exactly this; running it here keeps the command useful when the bus is down,
/// and the status line says which path was taken so the difference is visible.
fn open_directly(url: &str) -> bool {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(opener)
        .arg(url)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Ask the bus for the system clipboard. The last resort for the no-argument
/// form, once the selection and the word under the cursor have come up empty.
fn clipboard_word() -> Option<String> {
    let request = "{\"cmd\":\"clipboard_get\"}";
    let reply = call_daemon(request).or_else(|| call_cli(request))?;
    let text = reply_text(&reply)?;
    let word = text.split_whitespace().next()?.to_string();
    (!word.is_empty()).then_some(word)
}

/// `:zwire-lookup [site] [term…]` — open a page for `term`, on the default
/// site, a named one, or a URL given outright.
fn zwire_lookup(host: &Host, args: &Args) -> c_int {
    let base = env::var("ZWIRE_LOOKUP_BASE").unwrap_or_else(|_| DEFAULT_BASE.to_string());
    let configured = env::var("ZWIRE_LOOKUP_SITES")
        .map(|raw| configured_sites(&raw))
        .unwrap_or_default();

    let mut terms: Vec<String> = args.rest().to_vec();
    if terms.is_empty() {
        // The selection first, so a deliberate visual selection wins over
        // whatever the cursor happens to rest on; then the word under the
        // cursor; then the clipboard, for when the cursor is on whitespace.
        let from_editor = host
            .selection_text()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty() && !text.contains('\n'))
            .or_else(|| host.word_at_cursor());

        match from_editor.or_else(clipboard_word) {
            Some(term) => terms.push(term),
            None => {
                host.error("zwire-lookup: pass a term or a URL, or put the cursor on a word");
                return 1;
            }
        }
    }

    let Some(url) = resolve_url(&terms, &base, &configured) else {
        host.error("zwire-lookup: nothing to look up");
        return 1;
    };
    let label = terms.join(" ");
    let request = format!("{{\"cmd\":\"open\",\"target\":\"{}\"}}", escape_json(&url));

    if let Some(reply) = call_daemon(&request) {
        if reply_is_ok(&reply) {
            host.message(&format!("zwire: opened {label}"));
            return 0;
        }
        host.error(&format!("zwire: bus refused to open {label}"));
        return 1;
    }

    // Nothing on our socket. Try the CLI, which may resolve it elsewhere.
    match call_cli(&request) {
        Some(reply) if reply_is_ok(&reply) => {
            host.message(&format!("zwire: opened {label} via the CLI"));
            return 0;
        }
        Some(_) => {
            host.error(&format!("zwire: bus refused to open {label}"));
            return 1;
        }
        None => {}
    }

    // The bus is down. Open it anyway rather than making the user start a
    // daemon to read documentation, and say so.
    if open_directly(&url) {
        host.message(&format!(
            "zwire-host is not running; opened {label} directly"
        ));
        0
    } else {
        host.error(&format!("zwire-lookup: could not open {label}"));
        1
    }
}

declare_plugin! {
    name: "zwire-lookup",
    version: "0.1.0",
    commands: { "zwire-lookup" => zwire_lookup },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    /// A term goes into a URL, so everything outside the unreserved set is
    /// escaped. A multi-word term, or one containing `&`, `#` or `?`, must not
    /// be able to add parameters to the URL it lands in.
    #[test]
    fn terms_are_percent_encoded_into_the_url() {
        assert_eq!(percent_encode("UnixStream"), "UnixStream");
        assert_eq!(percent_encode("hidden files"), "hidden%20files");
        assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
        assert_eq!(percent_encode("#frag?q"), "%23frag%3Fq");
        assert_eq!(percent_encode("a-b_c.d~e"), "a-b_c.d~e", "unreserved");
        // Non-ASCII is encoded per UTF-8 byte.
        assert_eq!(percent_encode("café"), "caf%C3%A9");
    }

    /// `{}` marks the slot; a template without one is a prefix and gets the term
    /// appended, which is what makes a bare base URL usable as a site.
    #[test]
    fn a_template_takes_the_term_at_its_slot_or_at_the_end() {
        assert_eq!(
            fill("https://docs.rs/releases/search?query={}", "serde"),
            "https://docs.rs/releases/search?query=serde"
        );
        assert_eq!(
            fill("https://example.com/docs/", "intro page"),
            "https://example.com/docs/intro%20page"
        );
        // More than one slot is filled everywhere it appears.
        assert_eq!(fill("{}/{}", "x"), "x/x");
    }

    /// An argument that is already a URL is opened untouched -- no template and
    /// no encoding, since the user typed the address they want.
    #[test]
    fn a_url_argument_is_opened_as_given() {
        let url = "https://example.com/a?b=c#d";
        assert_eq!(
            resolve_url(&args(&[url]), DEFAULT_BASE, &[]).as_deref(),
            Some(url)
        );
        assert!(is_url("http://x.test"));
        assert!(is_url("www.example.com"));
        assert!(!is_url("UnixStream"), "a bare symbol is a term");
        assert!(!is_url("mdn"), "and so is a site name");
    }

    /// A leading site name selects its template and the rest is the term. A
    /// first word that is not a site name stays part of the term, so
    /// `:zwire-lookup async trait` searches for "async trait" rather than
    /// silently dropping "async".
    #[test]
    fn a_leading_site_name_selects_its_template() {
        assert_eq!(
            resolve_url(&args(&["mdn", "fetch"]), DEFAULT_BASE, &[]).as_deref(),
            Some("https://developer.mozilla.org/en-US/search?q=fetch")
        );
        assert_eq!(
            resolve_url(&args(&["gh", "ripgrep", "hidden files"]), DEFAULT_BASE, &[]).as_deref(),
            Some("https://github.com/search?q=ripgrep%20hidden%20files")
        );
        assert_eq!(
            resolve_url(&args(&["async", "trait"]), DEFAULT_BASE, &[]).as_deref(),
            Some("https://docs.rs/releases/search?query=async%20trait"),
            "not a site name, so it is part of the term"
        );
        // A site name alone is a term: there would be nothing to look up.
        assert_eq!(
            resolve_url(&args(&["mdn"]), DEFAULT_BASE, &[]).as_deref(),
            Some("https://docs.rs/releases/search?query=mdn")
        );
    }

    /// `ZWIRE_LOOKUP_SITES` adds names and can replace a built-in one. A
    /// malformed entry is skipped rather than discarding the whole variable.
    #[test]
    fn configured_sites_extend_and_override_the_built_ins() {
        let sites = configured_sites(
            "rfc=https://www.rfc-editor.org/rfc/rfc{}.html; mdn=https://example.test/{} ;;bad;=nope;alsobad=",
        );

        assert_eq!(
            sites.len(),
            2,
            "the malformed entries are skipped: {sites:?}"
        );
        assert_eq!(
            site_template("rfc", &sites).as_deref(),
            Some("https://www.rfc-editor.org/rfc/rfc{}.html")
        );
        assert_eq!(
            site_template("mdn", &sites).as_deref(),
            Some("https://example.test/{}"),
            "a configured name replaces the built-in"
        );
        assert_eq!(
            site_template("docs", &sites).as_deref(),
            Some("https://docs.rs/releases/search?query={}"),
            "built-ins still resolve"
        );
        assert_eq!(site_template("nosuchsite", &sites), None);

        assert_eq!(
            resolve_url(&args(&["rfc", "2119"]), DEFAULT_BASE, &sites).as_deref(),
            Some("https://www.rfc-editor.org/rfc/rfc2119.html")
        );
    }

    /// The default template is what an unrecognised term falls back to, and
    /// `ZWIRE_LOOKUP_BASE` replaces it wholesale -- including with a bare prefix.
    #[test]
    fn the_default_base_is_replaceable() {
        assert_eq!(
            resolve_url(&args(&["serde"]), "https://kagi.com/search?q={}", &[]).as_deref(),
            Some("https://kagi.com/search?q=serde")
        );
        assert_eq!(
            resolve_url(&args(&["serde"]), "https://example.com/q/", &[]).as_deref(),
            Some("https://example.com/q/serde")
        );
        assert_eq!(resolve_url(&[], DEFAULT_BASE, &[]), None, "nothing to open");
    }

    /// Every built-in template must have a slot and be a real URL, or the site
    /// silently opens the wrong page.
    #[test]
    fn every_builtin_site_is_a_usable_template() {
        for (name, template) in BUILTIN_SITES {
            assert!(
                template.starts_with("https://"),
                "{name} is not an https URL: {template}"
            );
            assert!(template.contains("{}"), "{name} has no slot: {template}");
            let filled = fill(template, "x y");
            assert!(!filled.contains("{}"), "{name} left a slot unfilled");
            assert!(!filled.contains(' '), "{name} left a raw space: {filled}");
        }

        let names: std::collections::HashSet<_> =
            BUILTIN_SITES.iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), BUILTIN_SITES.len(), "duplicate site name");
    }

    /// A term with a quote or a backslash must not be able to break out of the
    /// JSON string and change the request's shape.
    #[test]
    fn escaping_keeps_a_term_inside_its_json_string() {
        assert_eq!(escape_json("UnixStream"), "UnixStream");
        assert_eq!(escape_json(r#"a"b"#), r#"a\"b"#);
        assert_eq!(escape_json(r"a\b"), r"a\\b");
        assert_eq!(escape_json("a\nb"), "a\\nb");
        assert_eq!(escape_json("a\u{1}b"), "a\\u0001b");
    }

    /// The reply is one JSON line and only `ok` decides success. The strings
    /// here are what `zwire-host` actually sent when this was checked against a
    /// running daemon -- note that a failure reply serialises its keys
    /// alphabetically, so `ok` is not the first field and cannot be found by
    /// looking at the start of the line.
    #[test]
    fn ok_is_read_wherever_it_appears() {
        assert!(reply_is_ok(r#"{"ok":true}"#), "a successful open");
        assert!(
            reply_is_ok(r#"{"ok":true,"text":"UnixStream"}"#),
            "clipboard_get"
        );
        assert!(
            reply_is_ok(r#"{"ok": true}"#),
            "spaced, in case that changes"
        );

        assert!(
            !reply_is_ok(r#"{"err":"exit Some(1)","ok":false}"#),
            "open failed"
        );
        assert!(!reply_is_ok(r#"{"ok":false,"err":"unknown_cmd"}"#));
        assert!(!reply_is_ok("not json at all"));
    }

    /// `clipboard_get` puts the clipboard in `text` -- the exact reply shape
    /// `zwire-host` produces, checked against its `osops::clipboard_get`.
    #[test]
    fn the_clipboard_text_is_unescaped() {
        assert_eq!(
            reply_text(r#"{"ok":true,"text":"UnixStream"}"#).as_deref(),
            Some("UnixStream")
        );
        assert_eq!(
            reply_text(r#"{"ok":true,"text":"say \"hi\""}"#).as_deref(),
            Some(r#"say "hi""#)
        );
        assert_eq!(
            reply_text(r#"{"ok":true,"text":"one\ntwo"}"#).as_deref(),
            Some("one\ntwo")
        );
        assert_eq!(
            reply_text(r#"{"ok":false,"err":"no_clipboard_tool"}"#),
            None
        );
        assert_eq!(reply_text(r#"{"text":"unterminated"#), None);
    }

    /// The socket is resolved the way zwire-host binds it, so an explicit
    /// `ZWIRE_HOST_SOCK` wins over every default.
    #[test]
    fn an_explicit_socket_path_wins() {
        let previous = env::var_os("ZWIRE_HOST_SOCK");
        env::set_var("ZWIRE_HOST_SOCK", "/tmp/explicit.sock");
        assert_eq!(socket_path(), PathBuf::from("/tmp/explicit.sock"));
        match previous {
            Some(value) => env::set_var("ZWIRE_HOST_SOCK", value),
            None => env::remove_var("ZWIRE_HOST_SOCK"),
        }
    }
}
