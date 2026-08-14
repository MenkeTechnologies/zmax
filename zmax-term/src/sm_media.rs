//! Spacemacs media / chat / input-method layer ports: `+music/pianobar`,
//! `+music/spotify`, `+music/tidalcycles`, `+chat/jabber`, `+tools/chrome`,
//! `+tools/ipython-notebook`, `+intl/chinese` and `+intl/japanese`.
//!
//! What the upstream emacs packages did, and what this module does instead:
//!
//! * **`pianobar.el`** ran the `pianobar` CLI in a comint buffer and wrote
//!   single characters to its stdin (`p` pause-toggle, `n` next, `+` love, `-`
//!   ban, `t` tired, `s` change station, `i` song info, `q` quit — the defaults
//!   in pianobar's `contrib/config-example`, keys `act_songpausetoggle`,
//!   `act_songnext`, `act_songlove`, `act_songban`, `act_songtired`,
//!   `act_stationchange`, `act_songinfo`, `act_quit`). Here pianobar is a
//!   long-lived child of the editor process ([`PIANOBAR`]) whose output a reader
//!   thread drains into a capped ring buffer so `:pianobar-output` can page it.
//!
//! * **`spotify.el`** drove the desktop client over AppleScript on macOS and
//!   D-Bus/MPRIS on Linux, and searched through the Spotify Web API. Same split
//!   here: `osascript` on macOS, `playerctl` when present on Linux, otherwise
//!   `dbus-send` against `org.mpris.MediaPlayer2.spotify`.
//!
//! * **`tidal.el`** started `ghci`, loaded the Tidal boot file with `:script`,
//!   and sent regions wrapped in GHCi's `:{` / `:}` multi-line brackets;
//!   `tidal-stop-dN` sent `mapM_ ($ silence) [dN]` and `tidal-hush` sent `hush`.
//!   GHCi is kept alive the same way pianobar is.
//!
//! * **`jabber.el`** was a full XMPP client. A roster, presence, subscription
//!   requests and MUC join all need a persistent authenticated XMPP session that
//!   a one-shot `:` command cannot hold, so **none of that is ported**. What is
//!   ported is the part that is genuinely reachable from a command: sending a
//!   message through the `sendxmpp` binary, which holds the credentials in
//!   `~/.sendxmpprc` and connects per invocation.
//!
//! * **`edit-server.el`** ("Edit with Emacs") listened on TCP 9292; the Chrome
//!   extension POSTs a textarea's contents, emacs edits them, and the finished
//!   text is written back as the HTTP response body. The server half is ported
//!   in full: a background accept loop parks each request in a pending queue
//!   with its connection still open, and `edit_server_finish` writes the edited
//!   text back on that same connection.
//!
//! * **EIN** (`+tools/ipython-notebook`) talked to a Jupyter server. The listing,
//!   notebook read and kernel lifecycle commands are ported against the Jupyter
//!   Server REST API. **Cell execution is not ported**: `jupyter_server`'s kernel
//!   routes are only `/api/kernels`, `/api/kernels/<id>` and
//!   `/api/kernels/<id>/{restart,interrupt}` — code is executed over the
//!   `/api/kernels/<id>/channels` websocket, and zmax has no websocket client, so
//!   there is no `ein_run_cell` rather than a fake one.
//!
//! * **`+intl/chinese`** used `chinese-conv`, whose `chinese-conv-backend`
//!   defaults to `opencc`; `+intl/japanese` used `migemo` (the `cmigemo` binary)
//!   for romaji-driven search. Both shell out to the same binaries here. The
//!   romaji→kana table and the hiragana/katakana codepoint shifts are finite and
//!   fully algorithmic, so they are implemented in-file instead.
//!
//! Every command keeps the layer contract `pub fn name(args: &[&str]) ->
//! Result<Outcome, String>`; the shared process/HTTP plumbing lives in
//! [`crate::sm`].

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::sm::{self, Outcome};

// ───────────────────────────── shared helpers ─────────────────────────────

/// Read an environment variable a layer cannot work without, turning unset or
/// blank into an error that names the variable and says what it is for.
fn env_required(name: &str, hint: &str) -> Result<String, String> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(format!("${name} is unset — {hint}")),
    }
}

/// Read an environment variable, treating blank as unset.
fn env_opt(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// `$HOME`, or an error when the process has none.
fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "$HOME is unset".to_string())
}

/// Join `args` back into the free-text argument a command was given.
fn joined(args: &[&str]) -> String {
    args.join(" ").trim().to_string()
}

/// `v[key]` as a `&str`, or `""`.
fn jstr<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// nbformat and the Jupyter API both write text fields either as one string or
/// as a list of line strings. Flatten both to a single string.
fn jtext(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(lines)) => lines
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .concat(),
        _ => String::new(),
    }
}

/// Standard base64 (RFC 4648, padded). Used for the Spotify token request's
/// `Authorization: Basic base64(id:secret)` header.
fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b1 = chunk[0] as u32;
        let b2 = *chunk.get(1).unwrap_or(&0) as u32;
        let b3 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b1 << 16) | (b2 << 8) | b3;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Drop ANSI CSI/OSC escape sequences. pianobar and GHCi both colour their
/// output, and the ring buffer is plain text a scratch buffer will show
/// verbatim.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: parameters/intermediates, then a final byte in 0x40..=0x7e.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: runs to BEL or ST (ESC \).
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' {
                        chars.next();
                        break;
                    }
                }
            }
            // Any other two-character escape.
            Some(_) | None => {}
        }
    }
    out
}

// ───────────────────── long-lived child process substrate ─────────────────────

/// How many output lines each long-lived child keeps. The buffers exist so the
/// output can be paged after the fact, not as a transcript, so they are capped.
const OUTPUT_LINES: usize = 500;

/// A child process the layer keeps alive and talks to over its stdin, with its
/// stdout and stderr drained by reader threads into a ring buffer. This is what
/// the comint buffer gave `pianobar.el` and `tidal.el`.
struct Repl {
    child: Child,
    stdin: ChildStdin,
    /// How the child was launched, for the status line.
    command: String,
}

impl Repl {
    /// Write `text` to the child's stdin and flush it. pianobar reads one
    /// character at a time and GHCi reads lines, so the caller decides whether
    /// `text` ends in a newline.
    fn send(&mut self, text: &str) -> Result<(), String> {
        self.stdin
            .write_all(text.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("{}: write failed: {e}", self.command))
    }

    /// True while the child has not exited.
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// Append one drained line to a ring buffer, dropping the oldest lines past
/// [`OUTPUT_LINES`].
fn push_line(sink: &'static Mutex<Vec<String>>, pending: &mut Vec<u8>) {
    if pending.is_empty() {
        return;
    }
    let raw = String::from_utf8_lossy(pending).into_owned();
    pending.clear();
    let text = strip_ansi(&raw).trim_end().to_string();
    if text.is_empty() {
        return;
    }
    if let Ok(mut lines) = sink.lock() {
        lines.push(text);
        let overflow = lines.len().saturating_sub(OUTPUT_LINES);
        if overflow > 0 {
            lines.drain(..overflow);
        }
    }
}

/// Drain `reader` into `sink` on a background thread until EOF.
///
/// Lines are split on `\n` *and* `\r`: pianobar redraws its playback timer with
/// a bare carriage return and no newline, so a `read_until(b'\n')` reader would
/// stall until the next song change.
fn drain<R: Read + Send + 'static>(mut reader: R, sink: &'static Mutex<Vec<String>>) {
    std::thread::spawn(move || {
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for &byte in &buf[..n] {
                        if byte == b'\n' || byte == b'\r' {
                            push_line(sink, &mut pending);
                        } else {
                            pending.push(byte);
                        }
                    }
                }
            }
        }
        push_line(sink, &mut pending);
    });
}

/// Spawn `program` with piped stdio and start draining its output into `sink`.
fn spawn_repl(
    program: &str,
    args: &[&str],
    sink: &'static Mutex<Vec<String>>,
) -> Result<Repl, String> {
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
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("{program}: no stdin"))?;
    if let Some(stdout) = child.stdout.take() {
        drain(stdout, sink);
    }
    if let Some(stderr) = child.stderr.take() {
        drain(stderr, sink);
    }
    let command = if args.is_empty() {
        program.to_string()
    } else {
        format!("{program} {}", args.join(" "))
    };
    Ok(Repl {
        child,
        stdin,
        command,
    })
}

/// Render a ring buffer as a page, keeping at most `limit` trailing lines.
fn page_output(title: &str, sink: &Mutex<Vec<String>>, limit: usize) -> Result<Outcome, String> {
    let lines = sink.lock().map_err(|_| "output buffer poisoned")?.clone();
    if lines.is_empty() {
        return Ok(Outcome::status(format!("{title}: no output captured yet")));
    }
    let start = lines.len().saturating_sub(limit);
    let shown = &lines[start..];
    let mut page = sm::heading(title);
    for line in shown {
        page.push_str(line);
        page.push('\n');
    }
    Ok(Outcome::page(
        format!("{title}: {} of {} lines", shown.len(), lines.len()),
        page,
    ))
}

/// Parse an optional trailing line-count argument, defaulting to the cap.
fn output_limit(args: &[&str]) -> usize {
    args.first()
        .and_then(|n| n.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(OUTPUT_LINES)
}

// ───────────────────────────── +music/pianobar ─────────────────────────────

/// The running pianobar, if any. Process-lifetime only, exactly like the comint
/// buffer `pianobar.el` used.
static PIANOBAR: Mutex<Option<Repl>> = Mutex::new(None);

/// pianobar's captured stdout/stderr.
static PIANOBAR_OUT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Ensure pianobar is running, spawning it when it is not. Returns whether this
/// call started it.
fn pianobar_ensure(slot: &mut Option<Repl>) -> Result<bool, String> {
    if let Some(repl) = slot.as_mut() {
        if repl.alive() {
            return Ok(false);
        }
        *slot = None;
    }
    if !sm::have("pianobar") {
        return Err(
            "`pianobar` not found on PATH — install pianobar to use the Pandora commands"
                .to_string(),
        );
    }
    // pianobar takes no command-line arguments; everything is configured through
    // ~/.config/pianobar/config and driven by the single-character keys below.
    *slot = Some(spawn_repl("pianobar", &[], &PIANOBAR_OUT)?);
    Ok(true)
}

/// Auto-start pianobar and write `keys` to its stdin verbatim.
fn pianobar_send(keys: &str) -> Result<bool, String> {
    let mut slot = PIANOBAR.lock().map_err(|_| "pianobar state poisoned")?;
    let started = pianobar_ensure(&mut slot)?;
    slot.as_mut()
        .ok_or_else(|| "pianobar is not running".to_string())?
        .send(keys)?;
    Ok(started)
}

/// One pianobar key command: send `keys`, report `what` on the status line.
fn pianobar_key(keys: &str, what: &str) -> Result<Outcome, String> {
    let started = pianobar_send(keys)?;
    Ok(Outcome::status(if started {
        format!("pianobar: started, {what}")
    } else {
        format!("pianobar: {what}")
    }))
}

/// Start pianobar if it is not already running. `args` is ignored — pianobar has
/// no command-line options.
pub fn pianobar_start(_args: &[&str]) -> Result<Outcome, String> {
    let mut slot = PIANOBAR.lock().map_err(|_| "pianobar state poisoned")?;
    let started = pianobar_ensure(&mut slot)?;
    Ok(Outcome::status(if started {
        "pianobar: started"
    } else {
        "pianobar: already running"
    }))
}

/// Toggle play/pause (pianobar's `act_songpausetoggle`, `p`). `args` is unused.
pub fn pianobar_play_pause(_args: &[&str]) -> Result<Outcome, String> {
    pianobar_key("p", "play/pause")
}

/// Skip to the next song (`act_songnext`, `n`). `args` is unused.
pub fn pianobar_next(_args: &[&str]) -> Result<Outcome, String> {
    pianobar_key("n", "next song")
}

/// Give the current song a thumbs-up (`act_songlove`, `+`). `args` is unused.
pub fn pianobar_love(_args: &[&str]) -> Result<Outcome, String> {
    pianobar_key("+", "loved current song")
}

/// Ban the current song (`act_songban`, `-`). `args` is unused.
pub fn pianobar_ban(_args: &[&str]) -> Result<Outcome, String> {
    pianobar_key("-", "banned current song")
}

/// Shelve the current song for a month (`act_songtired`, `t`). `args` is unused.
pub fn pianobar_tired(_args: &[&str]) -> Result<Outcome, String> {
    pianobar_key("t", "shelved current song (tired)")
}

/// Change station (`act_stationchange`, `s`). With `args[0]` present it is sent
/// as the station number and newline that pianobar's station prompt reads; with
/// no argument the prompt is left open and the station list lands in the output
/// buffer for [`pianobar_output`].
pub fn pianobar_station(args: &[&str]) -> Result<Outcome, String> {
    match args.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(number) => {
            let started = pianobar_send(&format!("s{number}\n"))?;
            Ok(Outcome::status(if started {
                format!("pianobar: started, selected station {number}")
            } else {
                format!("pianobar: selected station {number}")
            }))
        }
        None => pianobar_key(
            "s",
            "station prompt open — run :pianobar-output for the list",
        ),
    }
}

/// Print the current song's details (`act_songinfo`, `i`). The detail itself
/// arrives asynchronously in the output buffer. `args` is unused.
pub fn pianobar_info(_args: &[&str]) -> Result<Outcome, String> {
    pianobar_key("i", "song info requested — run :pianobar-output")
}

/// Quit pianobar (`act_quit`, `q`) and reap the child. `args` is unused.
pub fn pianobar_quit(_args: &[&str]) -> Result<Outcome, String> {
    let mut slot = PIANOBAR.lock().map_err(|_| "pianobar state poisoned")?;
    let Some(mut repl) = slot.take() else {
        return Ok(Outcome::status("pianobar: not running"));
    };
    let sent = repl.send("q").is_ok();
    // Give it a moment to exit cleanly before killing it.
    for _ in 0..20 {
        if !repl.alive() {
            return Ok(Outcome::status("pianobar: quit"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = repl.child.kill();
    let _ = repl.child.wait();
    Ok(Outcome::status(if sent {
        "pianobar: did not exit on `q`, killed"
    } else {
        "pianobar: killed"
    }))
}

/// Page pianobar's captured output. `args[0]` optionally caps how many trailing
/// lines to show.
pub fn pianobar_output(args: &[&str]) -> Result<Outcome, String> {
    page_output("pianobar", &PIANOBAR_OUT, output_limit(args))
}

// ────────────────────────────── +music/spotify ──────────────────────────────

/// The MPRIS bus name the Linux Spotify client registers.
const SPOTIFY_MPRIS_DEST: &str = "org.mpris.MediaPlayer2.spotify";

/// Run an AppleScript through `osascript` and return its trimmed output.
fn osascript(script: &str) -> Result<String, String> {
    if !sm::have("osascript") {
        return Err("`osascript` not found — Spotify control needs macOS".to_string());
    }
    sm::run("osascript", &["-e", script]).map(|out| out.trim().to_string())
}

/// Call a method on the Spotify MPRIS object over `dbus-send`. `interface` is
/// either `org.mpris.MediaPlayer2` (which owns `Raise` and `Quit`) or
/// `org.mpris.MediaPlayer2.Player` (which owns the transport methods).
fn dbus_call(interface: &str, method: &str, extra: &[&str]) -> Result<String, String> {
    if !sm::have("dbus-send") {
        return Err(
            "neither `playerctl` nor `dbus-send` found — install one to control Spotify"
                .to_string(),
        );
    }
    let dest = format!("--dest={SPOTIFY_MPRIS_DEST}");
    let member = format!("{interface}.{method}");
    let mut argv = vec![
        "--print-reply",
        dest.as_str(),
        "/org/mpris/MediaPlayer2",
        member.as_str(),
    ];
    argv.extend_from_slice(extra);
    sm::run("dbus-send", &argv)
}

/// Run a `playerctl` subcommand against the Spotify player.
fn playerctl(args: &[&str]) -> Result<String, String> {
    let mut argv = vec!["--player=spotify"];
    argv.extend_from_slice(args);
    sm::run("playerctl", &argv).map(|out| out.trim().to_string())
}

/// One transport command, expressed once per backend: the AppleScript verb, the
/// `playerctl` subcommand, and the MPRIS `(interface, method)` pair.
fn spotify_control(
    applescript_verb: &str,
    playerctl_command: &str,
    mpris_interface: &str,
    mpris_method: &str,
) -> Result<Outcome, String> {
    if cfg!(target_os = "macos") {
        osascript(&format!(
            "tell application \"Spotify\" to {applescript_verb}"
        ))?;
        return Ok(Outcome::status(format!("spotify: {applescript_verb}")));
    }
    if sm::have("playerctl") {
        playerctl(&[playerctl_command])?;
        return Ok(Outcome::status(format!("spotify: {playerctl_command}")));
    }
    dbus_call(mpris_interface, mpris_method, &[])?;
    Ok(Outcome::status(format!("spotify: {mpris_method}")))
}

/// Toggle play/pause on the desktop client. `args` is unused.
pub fn spotify_play_pause(_args: &[&str]) -> Result<Outcome, String> {
    spotify_control(
        "playpause",
        "play-pause",
        "org.mpris.MediaPlayer2.Player",
        "PlayPause",
    )
}

/// Skip to the next track. `args` is unused.
pub fn spotify_next(_args: &[&str]) -> Result<Outcome, String> {
    spotify_control(
        "next track",
        "next",
        "org.mpris.MediaPlayer2.Player",
        "Next",
    )
}

/// Go back to the previous track. `args` is unused.
pub fn spotify_previous(_args: &[&str]) -> Result<Outcome, String> {
    spotify_control(
        "previous track",
        "previous",
        "org.mpris.MediaPlayer2.Player",
        "Previous",
    )
}

/// Quit the desktop client. `playerctl` has no quit subcommand, so on Linux this
/// always goes through MPRIS's root-interface `Quit`. `args` is unused.
pub fn spotify_quit(_args: &[&str]) -> Result<Outcome, String> {
    if cfg!(target_os = "macos") {
        osascript("tell application \"Spotify\" to quit")?;
        return Ok(Outcome::status("spotify: quit"));
    }
    dbus_call("org.mpris.MediaPlayer2", "Quit", &[])?;
    Ok(Outcome::status("spotify: quit"))
}

/// The current track and player state. `args` is unused.
pub fn spotify_status(_args: &[&str]) -> Result<Outcome, String> {
    if cfg!(target_os = "macos") {
        // `player state` is the ePlS enumeration (stopped/playing/paused); the
        // track properties are `name`, `artist` and `album` on `current track`.
        let script = concat!(
            "tell application \"Spotify\"\n",
            "  if player state is stopped then return \"stopped\"\n",
            "  return (name of current track) & \" — \" & (artist of current track)",
            " & \" — \" & (album of current track) & \" [\" & (player state as text) & \"]\"\n",
            "end tell"
        );
        let line = osascript(script)?;
        return Ok(Outcome::status(format!("spotify: {line}")));
    }
    if sm::have("playerctl") {
        let state = playerctl(&["status"]).unwrap_or_else(|_| "unknown".to_string());
        let track = playerctl(&["metadata", "--format", "{{title}} — {{artist}} — {{album}}"])?;
        return Ok(Outcome::status(format!("spotify: {track} [{state}]")));
    }
    // Without playerctl, read the two MPRIS properties directly. dbus-send's
    // reply is a typed dump rather than JSON, so it is paged as-is.
    let state = dbus_call(
        "org.freedesktop.DBus.Properties",
        "Get",
        &[
            "string:org.mpris.MediaPlayer2.Player",
            "string:PlaybackStatus",
        ],
    )?;
    let metadata = dbus_call(
        "org.freedesktop.DBus.Properties",
        "Get",
        &["string:org.mpris.MediaPlayer2.Player", "string:Metadata"],
    )?;
    let mut page = sm::heading("Spotify (MPRIS)");
    page.push_str(state.trim());
    page.push_str("\n\n");
    page.push_str(metadata.trim());
    page.push('\n');
    Ok(Outcome::page("spotify: MPRIS status", page))
}

/// Cached client-credentials token and the instant it stops being valid.
static SPOTIFY_TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Fetch (or reuse) a Spotify Web API client-credentials token.
///
/// The token endpoint takes an `application/x-www-form-urlencoded` body, but
/// [`sm::http_post_json`] only sends JSON bodies — there is no form-encoded POST
/// in `sm`. So this one request goes through `curl --data-urlencode`, and the
/// reply is parsed with `serde_json` like every other call in this module. Every
/// subsequent Spotify call is a plain GET and uses `sm::http_get_json`.
fn spotify_token() -> Result<String, String> {
    if let Ok(cache) = SPOTIFY_TOKEN.lock() {
        if let Some((token, expiry)) = cache.as_ref() {
            if Instant::now() < *expiry {
                return Ok(token.clone());
            }
        }
    }
    let id = env_required(
        "SPOTIFY_CLIENT_ID",
        "set $SPOTIFY_CLIENT_ID and $SPOTIFY_CLIENT_SECRET from a Spotify developer app",
    )?;
    let secret = env_required(
        "SPOTIFY_CLIENT_SECRET",
        "set $SPOTIFY_CLIENT_ID and $SPOTIFY_CLIENT_SECRET from a Spotify developer app",
    )?;
    if !sm::have("curl") {
        return Err(
            "`curl` not found on PATH — the Spotify token request needs a form-encoded POST, \
             which curl provides"
                .to_string(),
        );
    }
    let auth = format!(
        "Authorization: Basic {}",
        base64(format!("{id}:{secret}").as_bytes())
    );
    let body = sm::run(
        "curl",
        &[
            "-fsS",
            "-X",
            "POST",
            "https://accounts.spotify.com/api/token",
            "-H",
            auth.as_str(),
            "-H",
            "Content-Type: application/x-www-form-urlencoded",
            "--data-urlencode",
            "grant_type=client_credentials",
        ],
    )?;
    let json: Value = serde_json::from_str(&body).map_err(|e| format!("token response: {e}"))?;
    let token = json
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!(
                "no access_token in token response: {}",
                sm::ellipsize(&body, 160)
            )
        })?
        .to_string();
    let ttl = json
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(3600)
        .saturating_sub(60);
    if let Ok(mut cache) = SPOTIFY_TOKEN.lock() {
        *cache = Some((
            token.clone(),
            Instant::now() + Duration::from_secs(ttl.max(1)),
        ));
    }
    Ok(token)
}

/// Comma-joined `name` fields of an `artists` array.
fn artist_names(v: &Value) -> String {
    v.get("artists")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|artist| jstr(artist, "name"))
                .filter(|n| !n.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// Render one page of Spotify search results. `kind` is the API's plural key
/// (`tracks`, `albums`, `artists`).
fn render_search(kind: &str, query: &str, json: &Value) -> (String, String) {
    let items = json
        .get(kind)
        .and_then(|v| v.get("items"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut page = sm::heading(&format!("Spotify {kind}: {query}"));
    for item in &items {
        let name = jstr(item, "name");
        let uri = jstr(item, "uri");
        let line = match kind {
            "tracks" => format!(
                "{}\n    {}\n    {}\n    {uri}",
                sm::ellipsize(name, 78),
                artist_names(item),
                jstr(item.get("album").unwrap_or(&Value::Null), "name"),
            ),
            "albums" => format!(
                "{}\n    {}\n    {}\n    {uri}",
                sm::ellipsize(name, 78),
                artist_names(item),
                jstr(item, "release_date"),
            ),
            _ => format!(
                "{}\n    {}\n    {uri}",
                sm::ellipsize(name, 78),
                item.get("genres")
                    .and_then(Value::as_array)
                    .map(|g| g
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", "))
                    .unwrap_or_default(),
            ),
        };
        page.push_str(&line);
        page.push_str("\n\n");
    }
    (
        format!("spotify: {} {kind} for “{query}”", items.len()),
        page,
    )
}

/// `GET /v1/search` for `kind`, rendered as a page.
fn spotify_search(kind: &str, args: &[&str]) -> Result<Outcome, String> {
    let query = joined(args);
    if query.is_empty() {
        return Err(format!(
            "usage: spotify-search-{}: <query>",
            &kind[..kind.len() - 1]
        ));
    }
    let token = spotify_token()?;
    let url = format!(
        "https://api.spotify.com/v1/search?q={}&type={}&limit=20",
        sm::urlencode(&query),
        &kind[..kind.len() - 1]
    );
    let auth = format!("Bearer {token}");
    let json = sm::http_get_json(&url, &[("Authorization", auth.as_str())])?;
    let (status, page) = render_search(kind, &query, &json);
    Ok(Outcome::page(status, page))
}

/// Search tracks. `args` is the query; pages name / artists / album / `spotify:`
/// URI for up to 20 hits.
pub fn spotify_search_track(args: &[&str]) -> Result<Outcome, String> {
    spotify_search("tracks", args)
}

/// Search albums. `args` is the query; pages name / artists / release date /
/// `spotify:` URI for up to 20 hits.
pub fn spotify_search_album(args: &[&str]) -> Result<Outcome, String> {
    spotify_search("albums", args)
}

/// Search artists. `args` is the query; pages name / genres / `spotify:` URI for
/// up to 20 hits.
pub fn spotify_search_artist(args: &[&str]) -> Result<Outcome, String> {
    spotify_search("artists", args)
}

/// Play a `spotify:` URI through the desktop client. `args[0]` is the URI (the
/// one the search commands page).
pub fn spotify_play_uri(args: &[&str]) -> Result<Outcome, String> {
    let uri = args.first().map(|s| s.trim()).unwrap_or("");
    if uri.is_empty() {
        return Err("usage: spotify-play-uri: <spotify:...>".to_string());
    }
    if !uri.starts_with("spotify:") && !uri.starts_with("https://open.spotify.com/") {
        return Err(format!("not a Spotify URI: {uri}"));
    }
    if cfg!(target_os = "macos") {
        // `play track` takes the URI as its direct parameter.
        osascript(&format!(
            "tell application \"Spotify\" to play track \"{}\"",
            uri.replace('\\', "\\\\").replace('"', "\\\"")
        ))?;
        return Ok(Outcome::status(format!("spotify: playing {uri}")));
    }
    if sm::have("playerctl") {
        playerctl(&["open", uri])?;
        return Ok(Outcome::status(format!("spotify: playing {uri}")));
    }
    let arg = format!("string:{uri}");
    dbus_call("org.mpris.MediaPlayer2.Player", "OpenUri", &[arg.as_str()])?;
    Ok(Outcome::status(format!("spotify: playing {uri}")))
}

// ─────────────────────────── +music/tidalcycles ───────────────────────────

/// The running GHCi, if any.
static TIDAL: Mutex<Option<Repl>> = Mutex::new(None);

/// GHCi's captured stdout/stderr.
static TIDAL_OUT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The stock Tidal boot file, copied verbatim from
/// <https://raw.githubusercontent.com/tidalcycles/Tidal/main/BootTidal.hs>.
/// `tidal.el` loads whatever `tidal-boot-script-path` points at, which by
/// default is this file as shipped inside the installed `tidal` Haskell package;
/// zmax writes this copy to a temp file and `:script`s it when
/// `$TIDAL_BOOT_PATH` is unset.
const BOOT_TIDAL: &str = r#":set -fno-warn-orphans -Wno-type-defaults -XMultiParamTypeClasses -XOverloadedStrings
:set prompt ""

-- Import all the boot functions and aliases.
import Sound.Tidal.Boot

default (Rational, Integer, Double, Pattern String)

-- Create a Tidal Stream with the default settings.
-- To customize these settings, use 'mkTidalWith' instead
tidalInst <- mkTidal

-- tidalInst <- mkTidalWith [(superdirtTarget { oLatency = 0.01 }, [superdirtShape])] (defaultConfig {cFrameTimespan = 1/50, cProcessAhead = 1/20})

-- This orphan instance makes the boot aliases work!
-- It has to go after you define 'tidalInst'.
instance Tidally where tidal = tidalInst

-- `enableLink` and `disableLink` can be used to toggle synchronisation using the Link protocol.
-- Uncomment the next line to enable Link on startup.
-- enableLink

-- You can also add your own aliases in this file. For example:
-- fastsquizzed pat = fast 2 $ pat # squiz 1.5

:set prompt "tidal> "
:set prompt-cont ""
"#;

/// Which interpreter to run, mirroring `tidal-interpreter` /
/// `tidal-interpreter-arguments`: `$TIDAL_GHCI` wins, then `cabal exec -- ghci`
/// when `$TIDAL_USE_CABAL` is set, then plain `ghci`.
fn tidal_interpreter() -> (String, Vec<String>) {
    if let Some(ghci) = env_opt("TIDAL_GHCI") {
        let mut words = ghci.split_whitespace().map(str::to_string);
        let program = words.next().unwrap_or_else(|| "ghci".to_string());
        return (program, words.collect());
    }
    if env_opt("TIDAL_USE_CABAL").is_some() {
        return (
            "cabal".to_string(),
            vec!["exec".into(), "--".into(), "ghci".into()],
        );
    }
    ("ghci".to_string(), Vec::new())
}

/// Path of the boot file to `:script`: `$TIDAL_BOOT_PATH` when set, otherwise a
/// temp-file copy of [`BOOT_TIDAL`].
fn tidal_boot_path() -> Result<PathBuf, String> {
    if let Some(path) = env_opt("TIDAL_BOOT_PATH") {
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(format!(
                "$TIDAL_BOOT_PATH: {} is not a file",
                path.display()
            ));
        }
        return Ok(path);
    }
    let path = std::env::temp_dir().join("zmax-BootTidal.hs");
    std::fs::write(&path, BOOT_TIDAL).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Ensure GHCi is running with the boot file loaded. Returns whether this call
/// started it.
fn tidal_ensure(slot: &mut Option<Repl>) -> Result<bool, String> {
    if let Some(repl) = slot.as_mut() {
        if repl.alive() {
            return Ok(false);
        }
        *slot = None;
    }
    let (program, args) = tidal_interpreter();
    if !sm::have(&program) {
        return Err(format!(
            "`{program}` not found on PATH — install GHC/Tidal, or point $TIDAL_GHCI at the \
             interpreter"
        ));
    }
    let boot = tidal_boot_path()?;
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut repl = spawn_repl(&program, &argv, &TIDAL_OUT)?;
    // `tidal.el` boots with `:script <tidal-boot-script-path>` rather than
    // pasting the text, because the boot file has top-level bindings and `--`
    // comments that GHCi's `:{`/`:}` bracket would not accept.
    repl.send(&format!(":script {}\n", boot.display()))?;
    *slot = Some(repl);
    Ok(true)
}

/// Send a line to GHCi, auto-starting it first. Returns whether this call
/// started GHCi.
fn tidal_send_line(line: &str) -> Result<bool, String> {
    let mut slot = TIDAL.lock().map_err(|_| "tidal state poisoned")?;
    let started = tidal_ensure(&mut slot)?;
    slot.as_mut()
        .ok_or_else(|| "ghci is not running".to_string())?
        .send(line)?;
    Ok(started)
}

/// Wrap `code` in GHCi's multi-line brackets, the way `tidal-eval-multiple-lines`
/// does (`:{`, the region, `:}`).
fn tidal_block(code: &str) -> String {
    format!(":{{\n{}\n:}}\n", code.trim_end())
}

/// Start GHCi and load the Tidal boot file. `args` is unused; the interpreter
/// comes from `$TIDAL_GHCI` / `$TIDAL_USE_CABAL` and the boot file from
/// `$TIDAL_BOOT_PATH`.
pub fn tidal_start(_args: &[&str]) -> Result<Outcome, String> {
    let mut slot = TIDAL.lock().map_err(|_| "tidal state poisoned")?;
    let started = tidal_ensure(&mut slot)?;
    let command = slot
        .as_ref()
        .map(|repl| repl.command.clone())
        .unwrap_or_default();
    Ok(Outcome::status(if started {
        format!("tidal: started {command} and loaded the boot file")
    } else {
        format!("tidal: {command} already running")
    }))
}

/// Send a block of Haskell to GHCi. `args[0]` is the code (the buffer region);
/// it is bracketed with `:{` / `:}` and the captured reply is paged.
pub fn tidal_send(args: &[&str]) -> Result<Outcome, String> {
    let code = args.first().map(|s| s.trim_end()).unwrap_or("");
    if code.trim().is_empty() {
        return Err("usage: tidal-send: <haskell code>".to_string());
    }
    tidal_send_line(&tidal_block(code))?;
    // GHCi answers asynchronously; give the reader thread a beat so the page is
    // not always empty, then show the tail of the buffer.
    std::thread::sleep(Duration::from_millis(250));
    page_output("tidal", &TIDAL_OUT, 60)
}

/// Parse a `d1`..`d9` orbit argument.
fn tidal_orbit(args: &[&str]) -> Result<u8, String> {
    match args.first().map(|s| s.trim()).unwrap_or("").parse::<u8>() {
        Ok(n) if (1..=9).contains(&n) => Ok(n),
        _ => Err("usage: <orbit 1-9> — tidal.el defines runners for d1 through d9".to_string()),
    }
}

/// Run code on an orbit. `args[0]` is `1`..`9`, `args[1]` is the Haskell to send;
/// it goes through the same `:{` / `:}` bracket `tidal-run-dN` uses.
pub fn tidal_run_orbit(args: &[&str]) -> Result<Outcome, String> {
    let orbit = tidal_orbit(args)?;
    let code = args.get(1).map(|s| s.trim_end()).unwrap_or("");
    if code.trim().is_empty() {
        return Err(format!("usage: tidal-run-orbit: {orbit} <haskell code>"));
    }
    tidal_send_line(&tidal_block(code))?;
    Ok(Outcome::status(format!("tidal: ran d{orbit}")))
}

/// Silence an orbit. `args[0]` is `1`..`9`. Sends exactly what
/// `tidal-create-runner-stop` sends — `mapM_ ($ silence) [dN]` inside `:{`/`:}`.
pub fn tidal_stop_orbit(args: &[&str]) -> Result<Outcome, String> {
    let orbit = tidal_orbit(args)?;
    tidal_send_line(&tidal_block(&format!(" mapM_ ($ silence) [d{orbit}]")))?;
    Ok(Outcome::status(format!("tidal: silenced d{orbit}")))
}

/// Silence everything (`tidal-hush`, which sends the bare `hush`). `args` is
/// unused.
pub fn tidal_hush(_args: &[&str]) -> Result<Outcome, String> {
    tidal_send_line("hush\n")?;
    Ok(Outcome::status("tidal: hush"))
}

/// Send `:quit` to GHCi and reap the child. `args` is unused.
pub fn tidal_quit(_args: &[&str]) -> Result<Outcome, String> {
    let mut slot = TIDAL.lock().map_err(|_| "tidal state poisoned")?;
    let Some(mut repl) = slot.take() else {
        return Ok(Outcome::status("tidal: not running"));
    };
    let sent = repl.send(":quit\n").is_ok();
    for _ in 0..20 {
        if !repl.alive() {
            return Ok(Outcome::status("tidal: quit"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = repl.child.kill();
    let _ = repl.child.wait();
    Ok(Outcome::status(if sent {
        "tidal: did not exit on :quit, killed"
    } else {
        "tidal: killed"
    }))
}

/// Page GHCi's captured output (`tidal-see-output`). `args[0]` optionally caps
/// how many trailing lines to show.
pub fn tidal_output(args: &[&str]) -> Result<Outcome, String> {
    page_output("tidal", &TIDAL_OUT, output_limit(args))
}

// ─────────────────────────────── +chat/jabber ───────────────────────────────

/// Run `sendxmpp` with `args`, feeding `message` on stdin.
fn sendxmpp(args: &[&str], message: &str) -> Result<String, String> {
    if !sm::have("sendxmpp") {
        return Err(
            "`sendxmpp` not found on PATH — install sendxmpp and configure ~/.sendxmpprc"
                .to_string(),
        );
    }
    sm::run_with_stdin("sendxmpp", args, message)
}

/// Send a one-to-one XMPP message. `args[0]` is the recipient JID, the remaining
/// arguments are the message body (joined with spaces) and go to `sendxmpp` on
/// stdin.
pub fn jabber_send(args: &[&str]) -> Result<Outcome, String> {
    let jid = args.first().map(|s| s.trim()).unwrap_or("");
    let body = joined(args.get(1..).unwrap_or(&[]));
    if jid.is_empty() || body.is_empty() {
        return Err("usage: jabber-send: <jid> <message>".to_string());
    }
    sendxmpp(&[jid], &body)?;
    Ok(Outcome::status(format!(
        "jabber: sent {} chars to {jid}",
        body.chars().count()
    )))
}

/// Send a message to a MUC room. `args[0]` is the room JID, the rest is the
/// body. Uses `sendxmpp -r zmax --chatroom <room>`; `-r` is the sender resource,
/// which `sendxmpp(1)` documents as the room alias when `--chatroom` is given.
pub fn jabber_send_muc(args: &[&str]) -> Result<Outcome, String> {
    let room = args.first().map(|s| s.trim()).unwrap_or("");
    let body = joined(args.get(1..).unwrap_or(&[]));
    if room.is_empty() || body.is_empty() {
        return Err("usage: jabber-send-muc: <room-jid> <message>".to_string());
    }
    sendxmpp(&["-r", "zmax", "--chatroom", room], &body)?;
    Ok(Outcome::status(format!(
        "jabber: sent {} chars to room {room}",
        body.chars().count()
    )))
}

/// Extract the JIDs from a `~/.sendxmpprc`, never the passwords.
///
/// `sendxmpp(1)` documents two formats. Since 1.24 it is a key-value block
/// (`username:`, `jserver:`, `port:`, `password:`, `component:`), which yields a
/// single `username@jserver` account. Version 1.23 and older used one line per
/// account, `user@server password [componentname]`. Both are parsed; only the
/// JID is ever returned.
fn parse_sendxmpprc(text: &str) -> Vec<String> {
    let mut username = String::new();
    let mut jserver = String::new();
    let mut jids: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match key.as_str() {
                "username" => {
                    username = value.to_string();
                    continue;
                }
                "jserver" => {
                    jserver = value.to_string();
                    continue;
                }
                "port" | "password" | "component" => continue,
                _ => {}
            }
        }
        // Old one-line format: the JID is the first whitespace-separated field.
        if let Some(jid) = line.split_whitespace().next() {
            if jid.contains('@') {
                jids.push(jid.to_string());
            }
        }
    }
    if !username.is_empty() && !jserver.is_empty() {
        jids.insert(0, format!("{username}@{jserver}"));
    }
    jids
}

/// Page the JIDs configured in `~/.sendxmpprc`. Passwords in that file are never
/// read out. `args` is unused.
pub fn jabber_accounts(_args: &[&str]) -> Result<Outcome, String> {
    let path = home()?.join(".sendxmpprc");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let jids = parse_sendxmpprc(&text);
    if jids.is_empty() {
        return Ok(Outcome::status(format!(
            "jabber: no accounts found in {}",
            path.display()
        )));
    }
    let mut page = sm::heading(&format!("XMPP accounts ({})", path.display()));
    for jid in &jids {
        page.push_str(jid);
        page.push('\n');
    }
    Ok(Outcome::page(
        format!("jabber: {} account(s)", jids.len()),
        page,
    ))
}

// ──────────────────────── +tools/chrome (edit server) ────────────────────────

/// The port `edit-server.el` binds by default, and what the Chrome extension
/// POSTs to.
const EDIT_SERVER_PORT: u16 = 9292;

/// Set while the accept loop should keep running.
static EDIT_RUNNING: AtomicBool = AtomicBool::new(false);

/// Monotonic id handed to each accepted edit request.
static EDIT_NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Accepted-but-unanswered edit requests: `(id, body, still-open connection)`.
static EDIT_PENDING: Mutex<Vec<(u64, String, TcpStream)>> = Mutex::new(Vec::new());

/// The address the accept loop is bound to, while it is running.
static EDIT_ADDR: Mutex<Option<String>> = Mutex::new(None);

/// The parts of an HTTP request head the edit server needs.
#[derive(Debug, PartialEq, Eq)]
struct RequestHead {
    method: String,
    path: String,
    content_length: usize,
}

/// Parse the request line and headers of an HTTP/1.1 request. Bare `LF` line
/// endings are accepted as well as `CRLF`.
fn parse_request_head(head: &str) -> Result<RequestHead, String> {
    let mut lines = head.split('\n').map(|line| line.trim_end_matches('\r'));
    let request_line = lines.next().unwrap_or("").trim();
    if request_line.is_empty() {
        return Err("empty HTTP request".to_string());
    }
    let mut fields = request_line.split_whitespace();
    let method = fields
        .next()
        .ok_or_else(|| format!("malformed request line: {request_line}"))?
        .to_string();
    let path = fields.next().unwrap_or("/").to_string();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            content_length = value
                .trim()
                .parse()
                .map_err(|_| format!("bad Content-Length: {}", value.trim()))?;
        }
    }
    Ok(RequestHead {
        method,
        path,
        content_length,
    })
}

/// Read one HTTP request off `stream`, returning its body.
fn read_edit_request(stream: &mut TcpStream) -> Result<String, String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut raw: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    // Read to the end of the header block. Byte-at-a-time keeps the body bytes
    // in the socket rather than in an over-read buffer.
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return Err("connection closed before the headers ended".to_string()),
            Ok(_) => raw.push(byte[0]),
            Err(e) => return Err(format!("read: {e}")),
        }
        if raw.ends_with(b"\r\n\r\n") || raw.ends_with(b"\n\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&raw).into_owned();
    let request = parse_request_head(&head)?;
    if request.content_length == 0 {
        // The extension POSTs the textarea contents; anything without a body
        // (its `GET /status` probe, a stray connection) is not an edit.
        return Err(format!(
            "{} {} carried no body",
            request.method, request.path
        ));
    }
    let mut body = vec![0u8; request.content_length];
    stream
        .read_exact(&mut body)
        .map_err(|e| format!("read body: {e}"))?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// Start the "Edit with Emacs" server. `args[0]` is an optional port, defaulting
/// to 9292. Binds `127.0.0.1` only and accepts one connection at a time, parking
/// each request body in the pending queue with its connection held open.
pub fn edit_server_start(args: &[&str]) -> Result<Outcome, String> {
    if EDIT_RUNNING.load(Ordering::SeqCst) {
        let addr = EDIT_ADDR
            .lock()
            .ok()
            .and_then(|a| a.clone())
            .unwrap_or_default();
        return Ok(Outcome::status(format!(
            "edit-server: already listening on {addr}"
        )));
    }
    let port: u16 = match args.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(p) => p.parse().map_err(|_| format!("bad port: {p}"))?,
        None => EDIT_SERVER_PORT,
    };
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!("edit-server: {addr} is already in use")
        } else {
            format!("edit-server: bind {addr}: {e}")
        }
    })?;
    EDIT_RUNNING.store(true, Ordering::SeqCst);
    if let Ok(mut slot) = EDIT_ADDR.lock() {
        *slot = Some(addr.clone());
    }
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            if !EDIT_RUNNING.load(Ordering::SeqCst) {
                break;
            }
            let Ok(mut stream) = incoming else { continue };
            match read_edit_request(&mut stream) {
                Ok(body) => {
                    let id = EDIT_NEXT_ID.fetch_add(1, Ordering::SeqCst);
                    if let Ok(mut pending) = EDIT_PENDING.lock() {
                        pending.push((id, body, stream));
                    }
                }
                Err(_) => {
                    let _ = stream.shutdown(Shutdown::Both);
                }
            }
        }
        EDIT_RUNNING.store(false, Ordering::SeqCst);
    });
    Ok(Outcome::status(format!("edit-server: listening on {addr}")))
}

/// Stop accepting and close every pending connection. `args` is unused.
pub fn edit_server_stop(_args: &[&str]) -> Result<Outcome, String> {
    if !EDIT_RUNNING.load(Ordering::SeqCst) {
        return Ok(Outcome::status("edit-server: not running"));
    }
    EDIT_RUNNING.store(false, Ordering::SeqCst);
    // The accept loop is parked inside `accept()`; one connection wakes it so it
    // can observe the cleared flag and return.
    let addr = EDIT_ADDR
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
        .unwrap_or_else(|| format!("127.0.0.1:{EDIT_SERVER_PORT}"));
    if let Ok(poke) = TcpStream::connect(&addr) {
        let _ = poke.shutdown(Shutdown::Both);
    }
    let dropped = {
        let mut pending = EDIT_PENDING
            .lock()
            .map_err(|_| "edit-server state poisoned")?;
        let count = pending.len();
        for (_, _, stream) in pending.drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
        count
    };
    Ok(Outcome::status(format!(
        "edit-server: stopped {addr}, dropped {dropped} pending request(s)"
    )))
}

/// Page the pending edit ids with the first line of each body. `args` is unused.
pub fn edit_server_pending(_args: &[&str]) -> Result<Outcome, String> {
    let pending = EDIT_PENDING
        .lock()
        .map_err(|_| "edit-server state poisoned")?;
    if pending.is_empty() {
        return Ok(Outcome::status("edit-server: nothing pending"));
    }
    let mut page = sm::heading("Pending edit-server requests");
    for (id, body, _) in pending.iter() {
        let first = body.lines().next().unwrap_or("");
        page.push_str(&format!("{id:>4}  {}\n", sm::ellipsize(first, 72)));
    }
    Ok(Outcome::page(
        format!("edit-server: {} pending", pending.len()),
        page,
    ))
}

/// Return a pending request's body as a page so the dispatcher can open it in a
/// buffer. `args[0]` is the id; with no argument the oldest request is used. The
/// request stays pending — [`edit_server_finish`] is what answers it.
pub fn edit_server_take(args: &[&str]) -> Result<Outcome, String> {
    let wanted = args.first().map(|s| s.trim()).filter(|s| !s.is_empty());
    let id = match wanted {
        Some(text) => text.parse::<u64>().map_err(|_| format!("bad id: {text}"))?,
        None => 0,
    };
    let pending = EDIT_PENDING
        .lock()
        .map_err(|_| "edit-server state poisoned")?;
    let entry = if id == 0 {
        pending.first()
    } else {
        pending.iter().find(|(pid, _, _)| *pid == id)
    };
    let Some((pid, body, _)) = entry else {
        return Err(if id == 0 {
            "edit-server: nothing pending".to_string()
        } else {
            format!("edit-server: no pending request {id}")
        });
    };
    Ok(Outcome::page(
        format!(
            "edit-server: request {pid} ({} chars)",
            body.chars().count()
        ),
        body.clone(),
    ))
}

/// Answer a pending request with the edited text and close its connection.
/// `args[0]` is the id, `args[1]` is the edited text. Writes exactly
/// `HTTP/1.1 200 OK\r\nContent-Length: <n>\r\n\r\n<text>`, which is the reply the
/// Chrome extension reads back into the textarea.
pub fn edit_server_finish(args: &[&str]) -> Result<Outcome, String> {
    let id: u64 = args
        .first()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "usage: edit-server-finish: <id> <text>".to_string())?
        .parse()
        .map_err(|_| format!("bad id: {}", args[0]))?;
    let text = args.get(1).copied().unwrap_or("");
    let mut stream = {
        let mut pending = EDIT_PENDING
            .lock()
            .map_err(|_| "edit-server state poisoned")?;
        let index = pending
            .iter()
            .position(|(pid, _, _)| *pid == id)
            .ok_or_else(|| format!("edit-server: no pending request {id}"))?;
        pending.remove(index).2
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{text}",
        text.len()
    );
    stream
        .write_all(response.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|e| format!("edit-server: write reply for {id}: {e}"))?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(Outcome::status(format!(
        "edit-server: answered {id} with {} bytes",
        text.len()
    )))
}

// ─────────────────────── +tools/ipython-notebook (EIN) ───────────────────────

/// Base URL of the Jupyter server, `$JUPYTER_URL` or `http://localhost:8888`.
fn jupyter_url() -> String {
    env_opt("JUPYTER_URL")
        .unwrap_or_else(|| "http://localhost:8888".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// The `Authorization: token …` header, when `$JUPYTER_TOKEN` is set.
fn jupyter_auth() -> Option<String> {
    env_opt("JUPYTER_TOKEN").map(|token| format!("token {token}"))
}

/// GET a Jupyter API path (which must start with `/`), returning parsed JSON.
fn jupyter_get(path: &str) -> Result<Value, String> {
    let url = format!("{}{path}", jupyter_url());
    match jupyter_auth() {
        Some(auth) => sm::http_get_json(&url, &[("Authorization", auth.as_str())]),
        None => sm::http_get_json(&url, &[]),
    }
}

/// POST JSON to a Jupyter API path, returning parsed JSON.
fn jupyter_post(path: &str, body: &Value) -> Result<Value, String> {
    let url = format!("{}{path}", jupyter_url());
    match jupyter_auth() {
        Some(auth) => sm::http_post_json(&url, &[("Authorization", auth.as_str())], body),
        None => sm::http_post_json(&url, &[], body),
    }
}

/// List a Jupyter contents path. `args` is the path (default the server root);
/// pages the directory entries or the single file model.
pub fn ein_notebooks(args: &[&str]) -> Result<Outcome, String> {
    let path = joined(args);
    let json = jupyter_get(&format!(
        "/api/contents/{}",
        path.trim_matches('/')
            .split('/')
            .filter(|p| !p.is_empty())
            .map(sm::urlencode)
            .collect::<Vec<_>>()
            .join("/")
    ))?;
    let entries = json
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![json.clone()]);
    let mut page = sm::heading(&format!("Jupyter contents: /{}", path.trim_matches('/')));
    for entry in &entries {
        page.push_str(&format!(
            "{:<10} {:<20} {}\n",
            jstr(entry, "type"),
            jstr(entry, "last_modified"),
            jstr(entry, "path"),
        ));
    }
    Ok(Outcome::page(
        format!("ein: {} entry/entries", entries.len()),
        page,
    ))
}

/// Render one nbformat output as plain text.
fn render_output(output: &Value) -> String {
    match jstr(output, "output_type") {
        "stream" => jtext(output.get("text")),
        "execute_result" | "display_data" => output
            .get("data")
            .map(|data| jtext(data.get("text/plain")))
            .unwrap_or_default(),
        "error" => {
            let traceback = output
                .get("traceback")
                .and_then(Value::as_array)
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(Value::as_str)
                        .map(strip_ansi)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            if traceback.is_empty() {
                format!("{}: {}", jstr(output, "ename"), jstr(output, "evalue"))
            } else {
                traceback
            }
        }
        _ => String::new(),
    }
}

/// Render an nbformat notebook as the plain text a scratch buffer can show: a
/// `# In[<n>]:` header per cell, the source, then its `text/plain` and `stream`
/// output. Non-code cells get their `cell_type` in the header.
fn notebook_to_text(notebook: &Value) -> String {
    let cells = notebook
        .get("cells")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = String::new();
    for cell in &cells {
        let count = match cell.get("execution_count") {
            Some(Value::Number(n)) => n.to_string(),
            _ => " ".to_string(),
        };
        let kind = jstr(cell, "cell_type");
        if kind == "code" {
            out.push_str(&format!("# In[{count}]:\n"));
        } else {
            out.push_str(&format!("# In[{count}]: {kind}\n"));
        }
        let source = jtext(cell.get("source"));
        out.push_str(source.trim_end());
        out.push('\n');
        let outputs = cell
            .get("outputs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for output in &outputs {
            let text = render_output(output);
            if text.trim().is_empty() {
                continue;
            }
            out.push_str(text.trim_end());
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// Read one notebook and page it as text. `args` is the notebook path on the
/// Jupyter server, e.g. `work/analysis.ipynb`.
pub fn ein_open(args: &[&str]) -> Result<Outcome, String> {
    let path = joined(args);
    if path.is_empty() {
        return Err("usage: ein-open: <notebook path on the Jupyter server>".to_string());
    }
    let encoded = path
        .trim_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .map(sm::urlencode)
        .collect::<Vec<_>>()
        .join("/");
    let json = jupyter_get(&format!("/api/contents/{encoded}"))?;
    let content = json
        .get("content")
        .ok_or_else(|| format!("ein: {path} has no content — is it a notebook?"))?;
    let text = notebook_to_text(content);
    if text.trim().is_empty() {
        return Err(format!("ein: {path} has no cells"));
    }
    let mut page = sm::heading(&format!("Notebook: {path}"));
    page.push_str(&text);
    Ok(Outcome::page(format!("ein: opened {path}"), page))
}

/// List running kernels: id, name and last activity. `args` is unused.
pub fn ein_kernels(_args: &[&str]) -> Result<Outcome, String> {
    let json = jupyter_get("/api/kernels")?;
    let kernels = json.as_array().cloned().unwrap_or_default();
    let mut page = sm::heading("Jupyter kernels");
    for kernel in &kernels {
        page.push_str(&format!(
            "{:<40} {:<14} {:<12} {}\n",
            jstr(kernel, "id"),
            jstr(kernel, "name"),
            jstr(kernel, "execution_state"),
            jstr(kernel, "last_activity"),
        ));
    }
    Ok(Outcome::page(
        format!("ein: {} kernel(s)", kernels.len()),
        page,
    ))
}

/// Start a kernel. `args[0]` is the kernel spec name, defaulting to `python3`.
pub fn ein_kernel_start(args: &[&str]) -> Result<Outcome, String> {
    let name = args
        .first()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("python3");
    let json = jupyter_post("/api/kernels", &serde_json::json!({ "name": name }))?;
    Ok(Outcome::status(format!(
        "ein: started {} kernel {}",
        jstr(&json, "name"),
        jstr(&json, "id")
    )))
}

/// Stop a kernel. `args[0]` is the kernel id.
///
/// The Jupyter API deletes a kernel with `DELETE /api/kernels/<id>`, and `sm`
/// exposes only GET and JSON POST, so this one call shells out to `curl -X
/// DELETE` rather than adding an HTTP verb to the shared substrate for a single
/// use.
pub fn ein_kernel_stop(args: &[&str]) -> Result<Outcome, String> {
    let id = args.first().map(|s| s.trim()).unwrap_or("");
    if id.is_empty() {
        return Err("usage: ein-kernel-stop: <kernel id>".to_string());
    }
    if !sm::have("curl") {
        return Err(
            "`curl` not found on PATH — stopping a kernel is an HTTP DELETE, which curl provides"
                .to_string(),
        );
    }
    let url = format!("{}/api/kernels/{}", jupyter_url(), sm::urlencode(id));
    let auth = jupyter_auth().map(|value| format!("Authorization: {value}"));
    let mut argv = vec!["-fsS", "-X", "DELETE", url.as_str()];
    if let Some(header) = auth.as_deref() {
        argv.push("-H");
        argv.push(header);
    }
    sm::run("curl", &argv)?;
    Ok(Outcome::status(format!("ein: stopped kernel {id}")))
}

// ────────────────────────────── +intl/chinese ──────────────────────────────

/// Convert Chinese text between scripts.
///
/// `chinese-conv`'s default `chinese-conv-backend` is `opencc`, whose CLI reads
/// stdin and writes stdout when neither `-i` nor `-o` is given and takes the
/// conversion table with `-c`. `cconv` is the documented fallback backend and
/// names its scripts by locale (`UTF8-CN` simplified, `UTF8-TW` traditional).
fn chinese_convert(
    text: &str,
    opencc_config: &str,
    cconv_from: &str,
    cconv_to: &str,
) -> Result<String, String> {
    if sm::have("opencc") {
        return sm::run_with_stdin("opencc", &["-c", opencc_config], text);
    }
    if sm::have("cconv") {
        return sm::run_with_stdin("cconv", &["-f", cconv_from, "-t", cconv_to], text);
    }
    Err(
        "neither `opencc` nor `cconv` found on PATH — install one of them to convert Chinese text"
            .to_string(),
    )
}

/// Convert traditional Chinese to simplified. `args` is the text.
pub fn chinese_to_simplified(args: &[&str]) -> Result<Outcome, String> {
    let text = joined(args);
    if text.is_empty() {
        return Err("usage: chinese-to-simplified: <text>".to_string());
    }
    let converted = chinese_convert(&text, "t2s.json", "UTF8-TW", "UTF8-CN")?;
    let converted = converted.trim_end().to_string();
    Ok(Outcome::page(
        format!("chinese: {}", sm::ellipsize(&converted, 72)),
        converted,
    ))
}

/// Convert simplified Chinese to traditional. `args` is the text.
pub fn chinese_to_traditional(args: &[&str]) -> Result<Outcome, String> {
    let text = joined(args);
    if text.is_empty() {
        return Err("usage: chinese-to-traditional: <text>".to_string());
    }
    let converted = chinese_convert(&text, "s2t.json", "UTF8-CN", "UTF8-TW")?;
    let converted = converted.trim_end().to_string();
    Ok(Outcome::page(
        format!("chinese: {}", sm::ellipsize(&converted, 72)),
        converted,
    ))
}

/// Report the pinyin of `args`, using the external `pinyin` CLI. There is no
/// built-in fallback: a partial character table would silently give wrong
/// readings, so a missing binary is an error that names it.
pub fn chinese_pinyin(args: &[&str]) -> Result<Outcome, String> {
    let text = joined(args);
    if text.is_empty() {
        return Err("usage: chinese-pinyin: <text>".to_string());
    }
    if !sm::have("pinyin") {
        return Err(
            "`pinyin` not found on PATH — install a pinyin CLI (zmax ships no reading table)"
                .to_string(),
        );
    }
    let out = sm::run("pinyin", &[text.as_str()])?;
    let out = out.trim_end().to_string();
    let mut page = sm::heading(&format!("Pinyin: {}", sm::ellipsize(&text, 60)));
    page.push_str(&out);
    page.push('\n');
    Ok(Outcome::page(
        format!("pinyin: {}", sm::ellipsize(&out, 72)),
        page,
    ))
}

/// Render the Youdao dictionary JSON: the `ec` English→Chinese entry and the
/// `web_trans` web translations.
fn render_youdao(word: &str, json: &Value) -> String {
    let mut page = sm::heading(&format!("Youdao: {word}"));
    if let Some(entries) = json
        .get("ec")
        .and_then(|ec| ec.get("word"))
        .and_then(Value::as_array)
    {
        for entry in entries {
            let us = jstr(entry, "usphone");
            let uk = jstr(entry, "ukphone");
            if !us.is_empty() || !uk.is_empty() {
                page.push_str(&format!("US [{us}]  UK [{uk}]\n"));
            }
            for translation in entry
                .get("trs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                for tr in translation
                    .get("tr")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let line = tr.get("l").map(|l| jtext(l.get("i"))).unwrap_or_default();
                    if !line.trim().is_empty() {
                        page.push_str(&format!("  {}\n", line.trim()));
                    }
                }
            }
        }
        page.push('\n');
    }
    if let Some(webs) = json
        .get("web_trans")
        .and_then(|w| w.get("web-translation"))
        .and_then(Value::as_array)
    {
        page.push_str("web translations\n");
        for web in webs {
            let key = jstr(web, "key");
            let values: Vec<&str> = web
                .get("trans")
                .and_then(Value::as_array)
                .map(|t| t.iter().map(|v| jstr(v, "value")).collect())
                .unwrap_or_default();
            page.push_str(&format!("  {key}: {}\n", values.join("; ")));
        }
    }
    page
}

/// Look a word up in the Youdao dictionary (the layer's `youdao-dictionary`).
/// `args` is the word or phrase.
pub fn youdao_lookup(args: &[&str]) -> Result<Outcome, String> {
    let word = joined(args);
    if word.is_empty() {
        return Err("usage: youdao-lookup: <word>".to_string());
    }
    let url = format!("https://dict.youdao.com/jsonapi?q={}", sm::urlencode(&word));
    let json = sm::http_get_json(&url, &[])?;
    let page = render_youdao(&word, &json);
    Ok(Outcome::page(format!("youdao: {word}"), page))
}

// ───────────────────────────── +intl/japanese ─────────────────────────────

/// The romaji→hiragana table, longest key first at lookup time.
///
/// Covers the gojūon, the dakuten/handakuten rows, the yōon digraphs in both
/// Hepburn (`sha`, `chi`, `tsu`, `ja`) and kunrei/wāpuro (`sya`, `ti`, `tu`,
/// `zya`, `jya`) spellings, the foreign-sound rows (`fa`, `va`, `tha`), and the
/// small-kana escapes (`l`/`x` prefixes). Sokuon, `n`/`nn` and the `t`+`ch`
/// case are handled in [`romaji_to_hiragana`] because they are rules, not
/// entries.
const ROMAJI: &[(&str, &str)] = &[
    // four-letter small tsu escapes
    ("ltsu", "っ"),
    ("xtsu", "っ"),
    // yōon and other digraph-plus-vowel forms
    ("kya", "きゃ"),
    ("kyi", "きぃ"),
    ("kyu", "きゅ"),
    ("kye", "きぇ"),
    ("kyo", "きょ"),
    ("gya", "ぎゃ"),
    ("gyi", "ぎぃ"),
    ("gyu", "ぎゅ"),
    ("gye", "ぎぇ"),
    ("gyo", "ぎょ"),
    ("sha", "しゃ"),
    ("shi", "し"),
    ("shu", "しゅ"),
    ("she", "しぇ"),
    ("sho", "しょ"),
    ("sya", "しゃ"),
    ("syi", "しぃ"),
    ("syu", "しゅ"),
    ("sye", "しぇ"),
    ("syo", "しょ"),
    ("jya", "じゃ"),
    ("jyi", "じぃ"),
    ("jyu", "じゅ"),
    ("jye", "じぇ"),
    ("jyo", "じょ"),
    ("zya", "じゃ"),
    ("zyi", "じぃ"),
    ("zyu", "じゅ"),
    ("zye", "じぇ"),
    ("zyo", "じょ"),
    ("cha", "ちゃ"),
    ("chi", "ち"),
    ("chu", "ちゅ"),
    ("che", "ちぇ"),
    ("cho", "ちょ"),
    ("cya", "ちゃ"),
    ("cyi", "ちぃ"),
    ("cyu", "ちゅ"),
    ("cye", "ちぇ"),
    ("cyo", "ちょ"),
    ("tya", "ちゃ"),
    ("tyi", "ちぃ"),
    ("tyu", "ちゅ"),
    ("tye", "ちぇ"),
    ("tyo", "ちょ"),
    ("tsa", "つぁ"),
    ("tsi", "つぃ"),
    ("tsu", "つ"),
    ("tse", "つぇ"),
    ("tso", "つぉ"),
    ("tha", "てゃ"),
    ("thi", "てぃ"),
    ("thu", "てゅ"),
    ("the", "てぇ"),
    ("tho", "てょ"),
    ("dha", "でゃ"),
    ("dhi", "でぃ"),
    ("dhu", "でゅ"),
    ("dhe", "でぇ"),
    ("dho", "でょ"),
    ("dya", "ぢゃ"),
    ("dyi", "ぢぃ"),
    ("dyu", "ぢゅ"),
    ("dye", "ぢぇ"),
    ("dyo", "ぢょ"),
    ("nya", "にゃ"),
    ("nyi", "にぃ"),
    ("nyu", "にゅ"),
    ("nye", "にぇ"),
    ("nyo", "にょ"),
    ("hya", "ひゃ"),
    ("hyi", "ひぃ"),
    ("hyu", "ひゅ"),
    ("hye", "ひぇ"),
    ("hyo", "ひょ"),
    ("bya", "びゃ"),
    ("byi", "びぃ"),
    ("byu", "びゅ"),
    ("bye", "びぇ"),
    ("byo", "びょ"),
    ("pya", "ぴゃ"),
    ("pyi", "ぴぃ"),
    ("pyu", "ぴゅ"),
    ("pye", "ぴぇ"),
    ("pyo", "ぴょ"),
    ("mya", "みゃ"),
    ("myi", "みぃ"),
    ("myu", "みゅ"),
    ("mye", "みぇ"),
    ("myo", "みょ"),
    ("rya", "りゃ"),
    ("ryi", "りぃ"),
    ("ryu", "りゅ"),
    ("rye", "りぇ"),
    ("ryo", "りょ"),
    ("fya", "ふゃ"),
    ("fyu", "ふゅ"),
    ("fyo", "ふょ"),
    ("vya", "ゔゃ"),
    ("vyu", "ゔゅ"),
    ("vyo", "ゔょ"),
    ("lya", "ゃ"),
    ("lyu", "ゅ"),
    ("lyo", "ょ"),
    ("xya", "ゃ"),
    ("xyu", "ゅ"),
    ("xyo", "ょ"),
    ("lwa", "ゎ"),
    ("xwa", "ゎ"),
    ("ltu", "っ"),
    ("xtu", "っ"),
    // gojūon and the voiced rows
    ("ka", "か"),
    ("ki", "き"),
    ("ku", "く"),
    ("ke", "け"),
    ("ko", "こ"),
    ("ga", "が"),
    ("gi", "ぎ"),
    ("gu", "ぐ"),
    ("ge", "げ"),
    ("go", "ご"),
    ("sa", "さ"),
    ("si", "し"),
    ("su", "す"),
    ("se", "せ"),
    ("so", "そ"),
    ("za", "ざ"),
    ("zi", "じ"),
    ("zu", "ず"),
    ("ze", "ぜ"),
    ("zo", "ぞ"),
    ("ja", "じゃ"),
    ("ji", "じ"),
    ("ju", "じゅ"),
    ("je", "じぇ"),
    ("jo", "じょ"),
    ("ta", "た"),
    ("ti", "ち"),
    ("tu", "つ"),
    ("te", "て"),
    ("to", "と"),
    ("da", "だ"),
    ("di", "ぢ"),
    ("du", "づ"),
    ("de", "で"),
    ("do", "ど"),
    ("na", "な"),
    ("ni", "に"),
    ("nu", "ぬ"),
    ("ne", "ね"),
    ("no", "の"),
    ("ha", "は"),
    ("hi", "ひ"),
    ("hu", "ふ"),
    ("he", "へ"),
    ("ho", "ほ"),
    ("ba", "ば"),
    ("bi", "び"),
    ("bu", "ぶ"),
    ("be", "べ"),
    ("bo", "ぼ"),
    ("pa", "ぱ"),
    ("pi", "ぴ"),
    ("pu", "ぷ"),
    ("pe", "ぺ"),
    ("po", "ぽ"),
    ("fa", "ふぁ"),
    ("fi", "ふぃ"),
    ("fu", "ふ"),
    ("fe", "ふぇ"),
    ("fo", "ふぉ"),
    ("va", "ゔぁ"),
    ("vi", "ゔぃ"),
    ("vu", "ゔ"),
    ("ve", "ゔぇ"),
    ("vo", "ゔぉ"),
    ("ma", "ま"),
    ("mi", "み"),
    ("mu", "む"),
    ("me", "め"),
    ("mo", "も"),
    ("ya", "や"),
    ("yu", "ゆ"),
    ("yo", "よ"),
    ("ra", "ら"),
    ("ri", "り"),
    ("ru", "る"),
    ("re", "れ"),
    ("ro", "ろ"),
    ("wa", "わ"),
    ("wi", "ゐ"),
    ("we", "ゑ"),
    ("wo", "を"),
    ("la", "ぁ"),
    ("li", "ぃ"),
    ("lu", "ぅ"),
    ("le", "ぇ"),
    ("lo", "ぉ"),
    ("xa", "ぁ"),
    ("xi", "ぃ"),
    ("xu", "ぅ"),
    ("xe", "ぇ"),
    ("xo", "ぉ"),
    // bare vowels, ん, and the punctuation a kana keyboard produces
    ("a", "あ"),
    ("i", "い"),
    ("u", "う"),
    ("e", "え"),
    ("o", "お"),
    ("n", "ん"),
    ("-", "ー"),
    (".", "。"),
    (",", "、"),
    ("/", "・"),
];

/// The longest [`ROMAJI`] entry, so lookup knows where to start.
const ROMAJI_MAX_KEY: usize = 4;

/// True for the five romaji vowels.
fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'i' | 'u' | 'e' | 'o')
}

/// True for a consonant that can double into a sokuon (`っ`). `n` is excluded
/// because `nn` is the ん spelling, not a doubled consonant.
fn doubles_to_sokuon(c: char) -> bool {
    c.is_ascii_alphabetic() && !is_vowel(c) && c != 'n'
}

/// Convert romaji to hiragana.
///
/// Rules on top of the [`ROMAJI`] table, in the order they are applied:
/// 1. `nn` and `n'` are ん; a bare `n` is ん unless the next letter is a vowel or
///    `y`, in which case it starts a `na`/`nya` syllable.
/// 2. `t` before `ch` is a sokuon, so `matcha` is まっちゃ.
/// 3. Any other doubled consonant is a sokuon: `kitte` is きって.
/// 4. Otherwise the longest matching table key wins.
/// 5. Anything unmatched is copied through unchanged, so ASCII mixed into the
///    input survives.
fn romaji_to_hiragana(input: &str) -> String {
    let lowered = input.to_lowercase();
    let chars: Vec<char> = lowered.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    'outer: while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        if c == 'n' {
            match next {
                Some('n') | Some('\'') => {
                    out.push('ん');
                    i += 2;
                    continue;
                }
                Some(n) if is_vowel(n) || n == 'y' => {}
                _ => {
                    out.push('ん');
                    i += 1;
                    continue;
                }
            }
        }

        // `tch` is the Hepburn spelling of a sokuon before ち.
        if c == 't' && chars.get(i + 1) == Some(&'c') && chars.get(i + 2) == Some(&'h') {
            out.push('っ');
            i += 1;
            continue;
        }

        if doubles_to_sokuon(c) && next == Some(c) {
            out.push('っ');
            i += 1;
            continue;
        }

        for len in (1..=ROMAJI_MAX_KEY.min(chars.len() - i)).rev() {
            let key: String = chars[i..i + len].iter().collect();
            if let Some((_, kana)) = ROMAJI.iter().find(|(k, _)| *k == key) {
                out.push_str(kana);
                i += len;
                continue 'outer;
            }
        }

        out.push(c);
        i += 1;
    }
    out
}

/// Shift hiragana (U+3041–U+3096, plus the U+309D–U+309E iteration marks) up to
/// the katakana block. Everything else, including the U+30FC prolonged sound
/// mark, is left alone.
fn hiragana_to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| match c as u32 {
            0x3041..=0x3096 | 0x309D..=0x309E => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Shift katakana (U+30A1–U+30F6, plus the U+30FD–U+30FE iteration marks) down to
/// the hiragana block. U+30FC (ー) has no hiragana counterpart and is preserved.
fn katakana_to_hiragana(s: &str) -> String {
    s.chars()
        .map(|c| match c as u32 {
            0x30A1..=0x30F6 | 0x30FD..=0x30FE => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Convert romaji to hiragana. `args` is the romaji (joined with spaces).
pub fn romaji_to_kana(args: &[&str]) -> Result<Outcome, String> {
    let text = joined(args);
    if text.is_empty() {
        return Err("usage: romaji-to-kana: <romaji>".to_string());
    }
    let kana = romaji_to_hiragana(&text);
    Ok(Outcome::page(format!("kana: {kana}"), kana))
}

/// Convert hiragana to katakana. `args` is the text.
pub fn kana_to_katakana(args: &[&str]) -> Result<Outcome, String> {
    let text = joined(args);
    if text.is_empty() {
        return Err("usage: kana-to-katakana: <hiragana>".to_string());
    }
    let kana = hiragana_to_katakana(&text);
    Ok(Outcome::page(format!("katakana: {kana}"), kana))
}

/// Convert katakana to hiragana. `args` is the text.
pub fn katakana_to_kana(args: &[&str]) -> Result<Outcome, String> {
    let text = joined(args);
    if text.is_empty() {
        return Err("usage: katakana-to-kana: <katakana>".to_string());
    }
    let kana = katakana_to_hiragana(&text);
    Ok(Outcome::page(format!("hiragana: {kana}"), kana))
}

/// Expand romaji into the Japanese-matching regex `cmigemo` generates, and hand
/// that regex back on the status line so the caller can search with it. `args`
/// is the romaji query; the dictionary comes from `$MIGEMO_DICTIONARY`.
///
/// `cmigemo -w <word>` expands one word and exits, which is the one-shot form
/// this command wants; `-q` drops the `QUERY:`/`PATTERN:` prompts. Set
/// `$MIGEMO_STYLE` to `vim` or `emacs` to add `-v` / `-e` for those regex
/// dialects.
pub fn migemo_search(args: &[&str]) -> Result<Outcome, String> {
    let query = joined(args);
    if query.is_empty() {
        return Err("usage: migemo-search: <romaji>".to_string());
    }
    if !sm::have("cmigemo") {
        return Err("`cmigemo` not found on PATH — install cmigemo for romaji search".to_string());
    }
    let dictionary = env_required(
        "MIGEMO_DICTIONARY",
        "point it at cmigemo's migemo-dict (e.g. /usr/share/cmigemo/utf-8/migemo-dict)",
    )?;
    let mut argv = vec!["-q", "-d", dictionary.as_str()];
    match env_opt("MIGEMO_STYLE").as_deref() {
        Some("vim") => argv.push("-v"),
        Some("emacs") => argv.push("-e"),
        _ => {}
    }
    argv.push("-w");
    argv.push(query.as_str());
    let regex = sm::run("cmigemo", &argv)?;
    let regex = regex.trim().to_string();
    if regex.is_empty() {
        return Err(format!("cmigemo: no pattern for “{query}”"));
    }
    Ok(Outcome::status(regex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // The exact shape of the Spotify token request's Basic credential.
        assert_eq!(base64(b"client:secret"), "Y2xpZW50OnNlY3JldA==");
    }

    #[test]
    fn strip_ansi_drops_csi_and_osc_but_keeps_text() {
        assert_eq!(strip_ansi("\u{1b}[1;32mok\u{1b}[0m"), "ok");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}body"), "body");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
        // pianobar's timer redraw carries a bare CSI K.
        assert_eq!(strip_ansi("#   -02:13/03:44\u{1b}[K"), "#   -02:13/03:44");
    }

    #[test]
    fn romaji_covers_the_gojuon_and_the_yoon_digraphs() {
        assert_eq!(romaji_to_hiragana("aiueo"), "あいうえお");
        assert_eq!(romaji_to_hiragana("kyo"), "きょ");
        assert_eq!(romaji_to_hiragana("shi"), "し");
        assert_eq!(romaji_to_hiragana("chi"), "ち");
        assert_eq!(romaji_to_hiragana("tsu"), "つ");
        assert_eq!(romaji_to_hiragana("jya"), "じゃ");
        assert_eq!(romaji_to_hiragana("ja"), "じゃ");
        assert_eq!(romaji_to_hiragana("sya"), "しゃ");
        assert_eq!(romaji_to_hiragana("tokyo"), "ときょ");
    }

    #[test]
    fn romaji_handles_sokuon_and_the_small_kana_escapes() {
        assert_eq!(romaji_to_hiragana("ltsu"), "っ");
        assert_eq!(romaji_to_hiragana("xtsu"), "っ");
        assert_eq!(romaji_to_hiragana("ltu"), "っ");
        assert_eq!(romaji_to_hiragana("kitte"), "きって");
        assert_eq!(romaji_to_hiragana("nippon"), "にっぽん");
        assert_eq!(romaji_to_hiragana("matcha"), "まっちゃ");
        assert_eq!(romaji_to_hiragana("gakkou"), "がっこう");
        assert_eq!(romaji_to_hiragana("lya"), "ゃ");
    }

    #[test]
    fn romaji_resolves_n_against_the_following_letter() {
        assert_eq!(romaji_to_hiragana("san"), "さん");
        assert_eq!(romaji_to_hiragana("n"), "ん");
        assert_eq!(romaji_to_hiragana("kanji"), "かんじ");
        assert_eq!(romaji_to_hiragana("nyanko"), "にゃんこ");
        // `nn` is the explicit ん, so the following vowel starts a fresh
        // syllable rather than joining it — the same as `n'`.
        assert_eq!(romaji_to_hiragana("nna"), "んあ");
        assert_eq!(romaji_to_hiragana("n'a"), "んあ");
        assert_eq!(romaji_to_hiragana("nani"), "なに");
        assert_eq!(romaji_to_hiragana("nihon"), "にほん");
        assert_eq!(romaji_to_hiragana("shinbun"), "しんぶん");
    }

    #[test]
    fn romaji_is_case_insensitive_and_passes_unmatched_text_through() {
        assert_eq!(romaji_to_hiragana("Tokyo"), "ときょ");
        assert_eq!(romaji_to_hiragana("ka!"), "か!");
        assert_eq!(romaji_to_hiragana("ra-men"), "らーめん");
    }

    #[test]
    fn kana_shifts_move_between_the_two_blocks() {
        assert_eq!(hiragana_to_katakana("ひらがな"), "ヒラガナ");
        assert_eq!(katakana_to_hiragana("カタカナ"), "かたかな");
        assert_eq!(hiragana_to_katakana("きって"), "キッテ");
        assert_eq!(katakana_to_hiragana("キッテ"), "きって");
        // U+30FC has no hiragana counterpart and must survive both directions.
        assert_eq!(katakana_to_hiragana("ラーメン"), "らーめん");
        assert_eq!(hiragana_to_katakana("らーめん"), "ラーメン");
        // Non-kana is untouched.
        assert_eq!(hiragana_to_katakana("abc 漢字"), "abc 漢字");
        assert_eq!(katakana_to_hiragana("abc 漢字"), "abc 漢字");
    }

    #[test]
    fn sendxmpprc_yields_jids_and_never_passwords() {
        let old = "# my accounts\nuser@example.com secret\nbot@jabber.org hunter2 gmail.com\n";
        assert_eq!(
            parse_sendxmpprc(old),
            vec!["user@example.com".to_string(), "bot@jabber.org".to_string()]
        );
        assert!(!parse_sendxmpprc(old).join(" ").contains("secret"));
        assert!(!parse_sendxmpprc(old).join(" ").contains("hunter2"));

        let new = "username: someone\njserver: talk.example.com\nport: 5222\npassword: hunter2\n";
        assert_eq!(parse_sendxmpprc(new), vec!["someone@talk.example.com"]);
        assert!(!parse_sendxmpprc(new).join(" ").contains("hunter2"));

        assert!(parse_sendxmpprc("").is_empty());
    }

    #[test]
    fn request_head_parsing_reads_the_method_path_and_length() {
        let head = "POST /edit HTTP/1.1\r\nHost: localhost:9292\r\nContent-Length: 12\r\n\r\n";
        assert_eq!(
            parse_request_head(head).unwrap(),
            RequestHead {
                method: "POST".to_string(),
                path: "/edit".to_string(),
                content_length: 12,
            }
        );
        // Header names are case-insensitive and bare LF is accepted.
        let lf = "POST / HTTP/1.1\ncontent-length: 3\n\n";
        assert_eq!(parse_request_head(lf).unwrap().content_length, 3);
        // A GET with no body.
        let get = "GET /status HTTP/1.1\r\nHost: x\r\n\r\n";
        let parsed = parse_request_head(get).unwrap();
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.content_length, 0);

        assert!(parse_request_head("").is_err());
        assert!(parse_request_head("POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n").is_err());
    }

    #[test]
    fn notebook_renders_cells_sources_and_outputs() {
        let notebook = serde_json::json!({
            "cells": [
                {
                    "cell_type": "markdown",
                    "source": ["# Title\n", "text\n"],
                    "metadata": {}
                },
                {
                    "cell_type": "code",
                    "execution_count": 3,
                    "source": "print('hi')\n",
                    "outputs": [
                        { "output_type": "stream", "name": "stdout", "text": ["hi\n"] }
                    ]
                },
                {
                    "cell_type": "code",
                    "execution_count": 4,
                    "source": "1 + 1",
                    "outputs": [
                        { "output_type": "execute_result", "data": { "text/plain": "2" } }
                    ]
                },
                {
                    "cell_type": "code",
                    "execution_count": null,
                    "source": "boom()",
                    "outputs": [
                        {
                            "output_type": "error",
                            "ename": "NameError",
                            "evalue": "boom",
                            "traceback": ["\u{1b}[31mNameError\u{1b}[0m: boom"]
                        }
                    ]
                }
            ]
        });
        let text = notebook_to_text(&notebook);
        assert_eq!(
            text,
            "# In[ ]: markdown\n# Title\ntext\n\n\
             # In[3]:\nprint('hi')\nhi\n\n\
             # In[4]:\n1 + 1\n2\n\n\
             # In[ ]:\nboom()\nNameError: boom\n\n"
        );
    }

    #[test]
    fn youdao_render_pulls_the_ec_and_web_translations() {
        let json = serde_json::json!({
            "ec": {
                "word": [{
                    "usphone": "həˈloʊ",
                    "ukphone": "həˈləʊ",
                    "trs": [{ "tr": [{ "l": { "i": ["int. 喂，你好"] } }] }]
                }]
            },
            "web_trans": {
                "web-translation": [{
                    "key": "Hello Kitty",
                    "trans": [{ "value": "凯蒂猫" }, { "value": "吉蒂猫" }]
                }]
            }
        });
        let page = render_youdao("hello", &json);
        assert!(page.contains("Youdao: hello"));
        assert!(page.contains("US [həˈloʊ]  UK [həˈləʊ]"));
        assert!(page.contains("int. 喂，你好"));
        assert!(page.contains("Hello Kitty: 凯蒂猫; 吉蒂猫"));
    }

    #[test]
    fn tidal_wraps_blocks_the_way_tidal_el_does() {
        assert_eq!(
            tidal_block("d1 $ sound \"bd\""),
            ":{\nd1 $ sound \"bd\"\n:}\n"
        );
        assert_eq!(
            tidal_block(" mapM_ ($ silence) [d3]\n"),
            ":{\n mapM_ ($ silence) [d3]\n:}\n"
        );
    }

    #[test]
    fn jupyter_text_fields_flatten_from_either_shape() {
        assert_eq!(jtext(Some(&serde_json::json!("one"))), "one");
        assert_eq!(jtext(Some(&serde_json::json!(["a\n", "b"]))), "a\nb");
        assert_eq!(jtext(Some(&Value::Null)), "");
        assert_eq!(jtext(None), "");
    }
}
