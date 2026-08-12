//! Ports of the spacemacs layers that drive an external tool: `pass`,
//! `prodigy`, `transmission`, `vagrant`, `conda`, `elasticsearch`, `quickurl`,
//! `sailfish-developer`, `perforce`, `dash`, `djvu` and `node`.
//!
//! Upstream each of these is an elisp wrapper that shells out to a binary (or
//! talks to an HTTP endpoint), parses the output into a buffer, and binds the
//! result under `SPC a`/`SPC m`. There is no elisp here, so every command
//! becomes a `:` command with the same contract as the rest of the integration
//! layers: `pub fn name(args: &[&str]) -> Result<Outcome, String>`, where the
//! `Outcome` carries a status line and optionally a page for a scratch buffer.
//!
//! The three places this deviates from upstream, all because a `:` command is a
//! one-shot call and not a live emacs buffer:
//!
//! * anything upstream that spawns an interactive program (`pass edit`,
//!   `vagrant ssh`) returns the non-interactive equivalent plus a status that
//!   says so;
//! * prodigy's service list lives in a JSON file instead of `prodigy-define-service`
//!   forms, because there is no lisp reader to evaluate them;
//! * quickurl's store is a two-column `name<TAB>url` file instead of
//!   `quickurl.el`'s lisp alist, for the same reason.
//!
//! Environment activation (`conda_activate`, `nvm_use`) mutates *this* process's
//! `PATH` with `std::env::set_var`, which is how a long-lived editor makes an
//! interpreter visible to every later subprocess (LSP servers, terminals, run
//! configurations) — the shell-function trick upstream relies on has no analogue
//! in a process that is not a shell.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::sm::{self, Outcome};

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `args[i]`, or a `usage: …` error naming the whole command form.
fn arg<'a>(args: &'a [&'a str], i: usize, usage: &str) -> Result<&'a str, String> {
    args.get(i)
        .copied()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("usage: {usage}"))
}

/// All of `args` as one string, for commands whose payload is free text.
fn joined(args: &[&str]) -> String {
    args.join(" ")
}

/// `sm::run` over an owned argv.
fn run_owned(program: &str, argv: &[String]) -> Result<String, String> {
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    sm::run(program, &refs)
}

/// A page built from a heading and a command's output, with a status that
/// reports how much came back. Empty output still gets a page so the caller
/// always has something to show.
fn output_page(title: &str, out: String) -> Outcome {
    let lines = out.lines().count();
    let body = if out.trim().is_empty() {
        "(no output)".to_string()
    } else {
        out
    };
    Outcome::page(
        format!("{title}: {lines} line{}", if lines == 1 { "" } else { "s" }),
        format!("{}{body}", sm::heading(title)),
    )
}

/// Require `program` on `PATH`, with the layer's install hint in the error.
fn require(program: &str, hint: &str) -> Result<(), String> {
    if sm::have(program) {
        Ok(())
    } else {
        Err(format!("`{program}` not found on PATH — {hint}"))
    }
}

// ---------------------------------------------------------------------------
// pass layer — the standard unix password manager
// ---------------------------------------------------------------------------

fn pass_ready() -> Result<(), String> {
    require("pass", "install password-store")
}

/// `pass ls [subdir]` — the store tree.
pub fn pass_list(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let out = match args.first() {
        Some(sub) => sm::run("pass", &["ls", sub])?,
        None => sm::run("pass", &["ls"])?,
    };
    let title = match args.first() {
        Some(sub) => format!("pass ls {sub}"),
        None => "pass ls".to_string(),
    };
    Ok(output_page(&title, out))
}

/// `pass -c <entry>` — copy the first line to the clipboard; pass clears it
/// again after 45 seconds.
pub fn pass_copy(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let entry = arg(args, 0, "pass-copy <entry>")?;
    sm::run("pass", &["-c", entry])?;
    Ok(Outcome::status(format!(
        "pass: copied {entry} to the clipboard (cleared after 45s)"
    )))
}

/// `pass show <entry>` — the decrypted entry.
pub fn pass_show(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let entry = arg(args, 0, "pass-show <entry>")?;
    let out = sm::run("pass", &["show", entry])?;
    Ok(output_page(&format!("pass {entry}"), out))
}

/// `pass generate <entry> [length]` — a fresh password, default length 25 (the
/// same default `pass` itself uses).
pub fn pass_generate(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let entry = arg(args, 0, "pass-generate <entry> [length]")?;
    let length = args.get(1).copied().unwrap_or("25");
    if length.parse::<u32>().is_err() {
        return Err(format!("pass-generate: `{length}` is not a length"));
    }
    let out = sm::run("pass", &["generate", entry, length])?;
    Ok(output_page(&format!("pass generate {entry}"), out))
}

/// `pass insert -m <entry>` with the remaining args as the multi-line body.
pub fn pass_insert(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    if args.len() < 2 {
        return Err("usage: pass-insert <entry> <password> [extra lines…]".to_string());
    }
    let entry = args[0];
    let body = format!("{}\n", args[1..].join("\n"));
    sm::run_with_stdin("pass", &["insert", "-m", "-f", entry], &body)?;
    Ok(Outcome::status(format!(
        "pass: stored {entry} ({} line{})",
        args.len() - 1,
        if args.len() == 2 { "" } else { "s" }
    )))
}

/// Upstream `pass edit <entry>` decrypts to a tempfile and spawns `$EDITOR`
/// inside emacs. A `:` command cannot host that interactive editor session — it
/// runs one process to completion and has nowhere to put a terminal — so this
/// shows the entry instead and says what to run for a real edit.
pub fn pass_edit(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let entry = arg(args, 0, "pass-edit <entry>")?;
    let out = sm::run("pass", &["show", entry])?;
    Ok(Outcome::page(
        format!("pass: showing {entry} — `pass edit {entry}` in a terminal to change it"),
        format!("{}{out}", sm::heading(&format!("pass {entry}"))),
    ))
}

/// `pass mv <old> <new>`.
pub fn pass_rename(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let old = arg(args, 0, "pass-rename <old> <new>")?;
    let new = arg(args, 1, "pass-rename <old> <new>")?;
    sm::run("pass", &["mv", old, new])?;
    Ok(Outcome::status(format!("pass: {old} → {new}")))
}

/// `pass rm -f <entry>`.
pub fn pass_remove(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let entry = arg(args, 0, "pass-remove <entry>")?;
    sm::run("pass", &["rm", "-f", entry])?;
    Ok(Outcome::status(format!("pass: removed {entry}")))
}

/// `pass init <gpg-id>` — create (or re-key) the store.
pub fn pass_init(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let gpg_id = arg(args, 0, "pass-init <gpg-id>")?;
    let out = sm::run("pass", &["init", gpg_id])?;
    Ok(output_page(&format!("pass init {gpg_id}"), out))
}

/// `pass otp <entry>` — the current TOTP token (password-store-otp extension).
pub fn pass_otp(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let entry = arg(args, 0, "pass-otp <entry>")?;
    let out = sm::run("pass", &["otp", entry])?;
    Ok(Outcome::status(format!("{entry}: {}", out.trim())))
}

/// `pass otp uri <entry>` — the stored `otpauth://` URI.
pub fn pass_otp_uri(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    let entry = arg(args, 0, "pass-otp-uri <entry>")?;
    let out = sm::run("pass", &["otp", "uri", entry])?;
    Ok(Outcome::status(format!("{entry}: {}", out.trim())))
}

/// `pass otp insert <entry>` fed the `otpauth://` URI on stdin.
pub fn pass_otp_insert(args: &[&str]) -> Result<Outcome, String> {
    pass_ready()?;
    if args.len() < 2 {
        return Err("usage: pass-otp-insert <entry> <otpauth-uri>".to_string());
    }
    let entry = args[0];
    let uri = args[1..].join(" ");
    sm::run_with_stdin("pass", &["otp", "insert", "-f", entry], &format!("{uri}\n"))?;
    Ok(Outcome::status(format!("pass: stored otp secret for {entry}")))
}

// ---------------------------------------------------------------------------
// prodigy layer — long-running services started/stopped from one place
// ---------------------------------------------------------------------------

/// One declared service. Upstream this is a `prodigy-define-service` form; here
/// it is one object in `~/.config/zmax/prodigy.json`.
#[derive(Clone, Debug, PartialEq)]
struct Service {
    name: String,
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
    tags: Vec<String>,
    url: Option<String>,
}

/// Services started by this process, as `(name, pid)`. Deliberately
/// process-lifetime only: prodigy also forgets its processes when emacs exits.
static PRODIGY_PIDS: Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());

fn prodigy_file() -> PathBuf {
    home().join(".config").join("zmax").join("prodigy.json")
}

const PRODIGY_EXAMPLE: &str = r#"[
  {
    "name": "web",
    "command": "npm",
    "args": ["run", "dev"],
    "cwd": "/path/to/project",
    "tags": ["node", "frontend"],
    "url": "http://localhost:3000"
  }
]
"#;

/// Parse the service array. Pure so the file format is testable without touching
/// the filesystem.
fn parse_services(text: &str) -> Result<Vec<Service>, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("prodigy.json: {e}"))?;
    let array = value
        .as_array()
        .ok_or_else(|| "prodigy.json: expected an array of services".to_string())?;
    let mut out = Vec::with_capacity(array.len());
    for (i, entry) in array.iter().enumerate() {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("prodigy.json: service {i} has no \"name\""))?;
        let command = entry
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("prodigy.json: service {name} has no \"command\""))?;
        let strings = |key: &str| -> Vec<String> {
            entry
                .get(key)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        out.push(Service {
            name: name.to_string(),
            command: command.to_string(),
            args: strings("args"),
            cwd: entry
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_string),
            tags: strings("tags"),
            url: entry
                .get("url")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    Ok(out)
}

/// Read the service file, writing a commented example the first time so the
/// error is actionable instead of "no such file".
fn load_services() -> Result<Vec<Service>, String> {
    let path = prodigy_file();
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_services(&text),
        Err(_) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let wrote = std::fs::write(&path, PRODIGY_EXAMPLE).is_ok();
            Err(format!(
                "prodigy: no services defined — {} an example at {}",
                if wrote { "wrote" } else { "could not write" },
                path.display()
            ))
        }
    }
}

fn find_service(services: &[Service], name: &str) -> Result<Service, String> {
    services
        .iter()
        .find(|s| s.name == name)
        .cloned()
        .ok_or_else(|| format!("prodigy: no service named `{name}`"))
}

/// `kill -0 <pid>` succeeds exactly while the process is alive and ours.
fn pid_alive(pid: u32) -> bool {
    sm::run("kill", &["-0", &pid.to_string()]).is_ok()
}

/// The registered pid for `name`, dropping entries whose process has exited.
fn running_pid(name: &str) -> Option<u32> {
    let mut reg = PRODIGY_PIDS.lock().unwrap_or_else(|e| e.into_inner());
    reg.retain(|(_, pid)| pid_alive(*pid));
    reg.iter()
        .find(|(n, _)| n == name)
        .map(|(_, pid)| *pid)
}

/// Table of declared services and their state. An optional arg filters by name
/// or tag substring — prodigy's `f n` / `f t` filters folded into one argument.
pub fn prodigy_list(args: &[&str]) -> Result<Outcome, String> {
    let services = load_services()?;
    let filter = joined(args).to_lowercase();
    let mut body = String::new();
    body.push_str(&format!(
        "{:<18} {:<9} {:<22} {}\n",
        "SERVICE", "STATE", "TAGS", "COMMAND"
    ));
    let mut shown = 0usize;
    for service in &services {
        if !filter.is_empty() {
            let hits_name = service.name.to_lowercase().contains(&filter);
            let hits_tag = service
                .tags
                .iter()
                .any(|t| t.to_lowercase().contains(&filter));
            if !hits_name && !hits_tag {
                continue;
            }
        }
        shown += 1;
        let state = match running_pid(&service.name) {
            Some(pid) => format!("run:{pid}"),
            None => "stopped".to_string(),
        };
        let command = format!("{} {}", service.command, service.args.join(" "));
        body.push_str(&format!(
            "{:<18} {:<9} {:<22} {}\n",
            sm::ellipsize(&service.name, 18),
            state,
            sm::ellipsize(&service.tags.join(","), 22),
            sm::ellipsize(command.trim(), 60)
        ));
    }
    Ok(Outcome::page(
        format!("prodigy: {shown}/{} service(s)", services.len()),
        format!("{}{body}", sm::heading("prodigy")),
    ))
}

/// Spawn the named service with its stdio detached (nothing reads it, so it must
/// not inherit the editor's terminal) and register the pid for this process's
/// lifetime.
pub fn prodigy_start(args: &[&str]) -> Result<Outcome, String> {
    let name = arg(args, 0, "prodigy-start <service>")?;
    let services = load_services()?;
    let service = find_service(&services, name)?;
    if let Some(pid) = running_pid(name) {
        return Err(format!("prodigy: {name} is already running (pid {pid})"));
    }

    let mut command = std::process::Command::new(&service.command);
    command
        .args(&service.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(cwd) = &service.cwd {
        command.current_dir(cwd);
    }
    let child = command
        .spawn()
        .map_err(|e| format!("prodigy: {}: {e}", service.command))?;
    let pid = child.id();
    PRODIGY_PIDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((name.to_string(), pid));
    Ok(Outcome::status(format!("prodigy: started {name} (pid {pid})")))
}

/// `kill -TERM <pid>` for the registered pid, then drop it from the registry.
pub fn prodigy_stop(args: &[&str]) -> Result<Outcome, String> {
    let name = arg(args, 0, "prodigy-stop <service>")?;
    let pid = running_pid(name).ok_or_else(|| format!("prodigy: {name} is not running"))?;
    let result = sm::run("kill", &["-TERM", &pid.to_string()]);
    PRODIGY_PIDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(n, _)| n != name);
    result?;
    Ok(Outcome::status(format!("prodigy: stopped {name} (pid {pid})")))
}

/// Stop (when running) then start.
pub fn prodigy_restart(args: &[&str]) -> Result<Outcome, String> {
    let name = arg(args, 0, "prodigy-restart <service>")?;
    let stopped = if running_pid(name).is_some() {
        prodigy_stop(args).is_ok()
    } else {
        false
    };
    let started = prodigy_start(args)?;
    Ok(Outcome::status(format!(
        "{}{}",
        if stopped { "restarted: " } else { "" },
        started.status
    )))
}

/// The service's declared `url`, handed back in the status for the caller to
/// open in a browser.
pub fn prodigy_browse(args: &[&str]) -> Result<Outcome, String> {
    let name = arg(args, 0, "prodigy-browse <service>")?;
    let services = load_services()?;
    let service = find_service(&services, name)?;
    let url = service
        .url
        .ok_or_else(|| format!("prodigy: {name} has no \"url\""))?;
    Ok(Outcome::status(url))
}

// ---------------------------------------------------------------------------
// transmission layer — Transmission RPC over HTTP
// ---------------------------------------------------------------------------

/// The RPC's session id, learned from the 409 handshake and reused until the
/// daemon rotates it.
static TRANSMISSION_SESSION: Mutex<Option<String>> = Mutex::new(None);

fn transmission_url() -> String {
    std::env::var("TRANSMISSION_RPC_URL")
        .unwrap_or_else(|_| "http://localhost:9091/transmission/rpc".to_string())
}

/// Standard base64 (RFC 4648) with padding. Hand-rolled because the only thing
/// this module needs it for is one `Authorization: Basic` header.
fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// `Authorization: Basic …` from `$TRANSMISSION_RPC_AUTH` (`user:pass`).
fn transmission_auth() -> Option<String> {
    let raw = std::env::var("TRANSMISSION_RPC_AUTH").ok()?;
    (!raw.trim().is_empty()).then(|| format!("Basic {}", base64(raw.trim().as_bytes())))
}

/// Turn an `sm::http_post_json` error into a status-line sentence.
fn transmission_error(err: &str) -> String {
    match sm::split_status_error(err) {
        Some((code, _, body)) => {
            let body = sm::ellipsize(&body, 160);
            format!("transmission: http {code}: {body}")
        }
        None => format!("transmission: {err}"),
    }
}

/// One RPC call. The daemon answers the first unauthenticated POST with 409 and
/// the session id in `X-Transmission-Session-Id`; retry once with it, cache it
/// for every later call.
fn rpc(method: &str, arguments: Value) -> Result<Value, String> {
    let url = transmission_url();
    let body = json!({ "method": method, "arguments": arguments });
    let auth = transmission_auth();

    for attempt in 0..2 {
        let session = TRANSMISSION_SESSION
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut headers: Vec<(&str, &str)> = Vec::new();
        if let Some(s) = session.as_deref() {
            headers.push(("X-Transmission-Session-Id", s));
        }
        if let Some(a) = auth.as_deref() {
            headers.push(("Authorization", a));
        }

        match sm::http_post_json(&url, &headers, &body) {
            Ok(value) => {
                let result = value.get("result").and_then(Value::as_str).unwrap_or("");
                if result != "success" {
                    return Err(format!("transmission: {method}: {result}"));
                }
                return Ok(value.get("arguments").cloned().unwrap_or(Value::Null));
            }
            Err(err) => {
                if attempt == 0 {
                    if let Some((409, header, _)) = sm::split_status_error(&err) {
                        if !header.is_empty() {
                            *TRANSMISSION_SESSION
                                .lock()
                                .unwrap_or_else(|e| e.into_inner()) = Some(header);
                            continue;
                        }
                    }
                }
                return Err(transmission_error(&err));
            }
        }
    }
    Err("transmission: session handshake failed".to_string())
}

/// The RPC's numeric torrent status.
fn status_name(code: i64) -> &'static str {
    match code {
        0 => "stopped",
        1 => "check-wait",
        2 => "check",
        3 => "dl-wait",
        4 => "download",
        5 => "seed-wait",
        6 => "seed",
        _ => "?",
    }
}

/// Bytes per second as kB/s (Transmission's own unit in its UI).
fn fmt_rate(bytes_per_sec: f64) -> String {
    format!("{:.0}k", bytes_per_sec / 1000.0)
}

/// Seconds remaining; the RPC uses negative values for "unknown"/"never".
fn fmt_eta(secs: i64) -> String {
    if secs < 0 {
        return "-".to_string();
    }
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Parse a list of torrent ids from the args (each arg one id).
fn parse_ids(args: &[&str], usage: &str) -> Result<Vec<i64>, String> {
    if args.is_empty() {
        return Err(format!("usage: {usage}"));
    }
    args.iter()
        .map(|a| {
            a.parse::<i64>()
                .map_err(|_| format!("transmission: `{a}` is not a torrent id"))
        })
        .collect()
}

/// `torrent-get` — the transfer table.
pub fn transmission_list(_args: &[&str]) -> Result<Outcome, String> {
    let arguments = rpc(
        "torrent-get",
        json!({
            "fields": [
                "id",
                "name",
                "status",
                "percentDone",
                "rateDownload",
                "rateUpload",
                "eta"
            ]
        }),
    )?;
    let torrents = arguments
        .get("torrents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut body = format!(
        "{:>4}  {:>5}  {:<11} {:>7} {:>7} {:>7}  {}\n",
        "ID", "DONE", "STATUS", "DOWN", "UP", "ETA", "NAME"
    );
    for t in &torrents {
        let id = t.get("id").and_then(Value::as_i64).unwrap_or(-1);
        let done = t.get("percentDone").and_then(Value::as_f64).unwrap_or(0.0) * 100.0;
        let status = status_name(t.get("status").and_then(Value::as_i64).unwrap_or(-1));
        let down = t.get("rateDownload").and_then(Value::as_f64).unwrap_or(0.0);
        let up = t.get("rateUpload").and_then(Value::as_f64).unwrap_or(0.0);
        let eta = t.get("eta").and_then(Value::as_i64).unwrap_or(-1);
        let name = t.get("name").and_then(Value::as_str).unwrap_or("");
        body.push_str(&format!(
            "{id:>4}  {done:>4.0}%  {status:<11} {:>7} {:>7} {:>7}  {}\n",
            fmt_rate(down),
            fmt_rate(up),
            fmt_eta(eta),
            sm::ellipsize(name, 60)
        ));
    }
    Ok(Outcome::page(
        format!("transmission: {} torrent(s)", torrents.len()),
        format!("{}{body}", sm::heading("transmission")),
    ))
}

/// `torrent-add` — magnet link, torrent URL or local `.torrent` path.
pub fn transmission_add(args: &[&str]) -> Result<Outcome, String> {
    if args.is_empty() {
        return Err("usage: transmission-add <magnet|url|file.torrent>".to_string());
    }
    // Magnet URIs contain no spaces, so joining is safe and also accepts paths
    // that the caller split on whitespace.
    let target = joined(args);
    let arguments = rpc("torrent-add", json!({ "filename": target }))?;
    let added = arguments
        .get("torrent-added")
        .or_else(|| arguments.get("torrent-duplicate"));
    let name = added
        .and_then(|t| t.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("torrent");
    let id = added
        .and_then(|t| t.get("id"))
        .and_then(Value::as_i64)
        .unwrap_or(-1);
    let dup = arguments.get("torrent-duplicate").is_some();
    Ok(Outcome::status(format!(
        "transmission: {} #{id} {name}",
        if dup { "already had" } else { "added" }
    )))
}

/// `torrent-remove` — drop from the list, keep the data.
pub fn transmission_remove(args: &[&str]) -> Result<Outcome, String> {
    let ids = parse_ids(args, "transmission-remove <id…>")?;
    rpc("torrent-remove", json!({ "ids": ids }))?;
    Ok(Outcome::status(format!(
        "transmission: removed {} torrent(s), data kept",
        ids.len()
    )))
}

/// `torrent-remove` with `delete-local-data` — drop the torrent and its files.
pub fn transmission_remove_delete(args: &[&str]) -> Result<Outcome, String> {
    let ids = parse_ids(args, "transmission-remove-delete <id…>")?;
    rpc(
        "torrent-remove",
        json!({ "ids": ids, "delete-local-data": true }),
    )?;
    Ok(Outcome::status(format!(
        "transmission: removed {} torrent(s) and their data",
        ids.len()
    )))
}

/// `torrent-start`.
pub fn transmission_start(args: &[&str]) -> Result<Outcome, String> {
    let ids = parse_ids(args, "transmission-start <id…>")?;
    rpc("torrent-start", json!({ "ids": ids }))?;
    Ok(Outcome::status(format!(
        "transmission: started {} torrent(s)",
        ids.len()
    )))
}

/// `torrent-stop`.
pub fn transmission_stop(args: &[&str]) -> Result<Outcome, String> {
    let ids = parse_ids(args, "transmission-stop <id…>")?;
    rpc("torrent-stop", json!({ "ids": ids }))?;
    Ok(Outcome::status(format!(
        "transmission: stopped {} torrent(s)",
        ids.len()
    )))
}

/// `torrent-verify` — re-hash the local data.
pub fn transmission_verify(args: &[&str]) -> Result<Outcome, String> {
    let ids = parse_ids(args, "transmission-verify <id…>")?;
    rpc("torrent-verify", json!({ "ids": ids }))?;
    Ok(Outcome::status(format!(
        "transmission: verifying {} torrent(s)",
        ids.len()
    )))
}

/// `torrent-set-location` with `move: true` — relocate the data, last arg is
/// the destination directory.
pub fn transmission_move(args: &[&str]) -> Result<Outcome, String> {
    if args.len() < 2 {
        return Err("usage: transmission-move <id…> <location>".to_string());
    }
    let (id_args, location) = args.split_at(args.len() - 1);
    let ids = parse_ids(id_args, "transmission-move <id…> <location>")?;
    let location = location[0];
    rpc(
        "torrent-set-location",
        json!({ "ids": ids, "location": location, "move": true }),
    )?;
    Ok(Outcome::status(format!(
        "transmission: moving {} torrent(s) to {location}",
        ids.len()
    )))
}

/// Session download cap in kB/s; `0` turns the cap off.
pub fn transmission_limit_down(args: &[&str]) -> Result<Outcome, String> {
    let raw = arg(args, 0, "transmission-limit-down <kBps|0>")?;
    let kbps: i64 = raw
        .parse()
        .map_err(|_| format!("transmission: `{raw}` is not a kB/s value"))?;
    rpc(
        "session-set",
        json!({ "speed-limit-down": kbps.max(0), "speed-limit-down-enabled": kbps > 0 }),
    )?;
    Ok(Outcome::status(if kbps > 0 {
        format!("transmission: download limit {kbps} kB/s")
    } else {
        "transmission: download limit off".to_string()
    }))
}

/// Session upload cap in kB/s; `0` turns the cap off.
pub fn transmission_limit_up(args: &[&str]) -> Result<Outcome, String> {
    let raw = arg(args, 0, "transmission-limit-up <kBps|0>")?;
    let kbps: i64 = raw
        .parse()
        .map_err(|_| format!("transmission: `{raw}` is not a kB/s value"))?;
    rpc(
        "session-set",
        json!({ "speed-limit-up": kbps.max(0), "speed-limit-up-enabled": kbps > 0 }),
    )?;
    Ok(Outcome::status(if kbps > 0 {
        format!("transmission: upload limit {kbps} kB/s")
    } else {
        "transmission: upload limit off".to_string()
    }))
}

/// Toggle turtle mode (`alt-speed-enabled`) from its current session value.
pub fn transmission_turtle(_args: &[&str]) -> Result<Outcome, String> {
    let session = rpc("session-get", json!({ "fields": ["alt-speed-enabled"] }))?;
    let current = session
        .get("alt-speed-enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    rpc("session-set", json!({ "alt-speed-enabled": !current }))?;
    Ok(Outcome::status(format!(
        "transmission: turtle mode {}",
        if current { "off" } else { "on" }
    )))
}

/// `torrent-get` `files` for one torrent.
pub fn transmission_files(args: &[&str]) -> Result<Outcome, String> {
    let id = arg(args, 0, "transmission-files <id>")?
        .parse::<i64>()
        .map_err(|_| "usage: transmission-files <id>".to_string())?;
    let arguments = rpc(
        "torrent-get",
        json!({ "ids": [id], "fields": ["name", "files"] }),
    )?;
    let torrent = arguments
        .get("torrents")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .cloned()
        .ok_or_else(|| format!("transmission: no torrent #{id}"))?;
    let name = torrent.get("name").and_then(Value::as_str).unwrap_or("");
    let files = torrent
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut body = format!("{:>5}  {:>12} {:>12}  {}\n", "DONE", "GOT", "SIZE", "FILE");
    for f in &files {
        let length = f.get("length").and_then(Value::as_f64).unwrap_or(0.0);
        let got = f.get("bytesCompleted").and_then(Value::as_f64).unwrap_or(0.0);
        let pct = if length > 0.0 { got / length * 100.0 } else { 0.0 };
        body.push_str(&format!(
            "{pct:>4.0}%  {got:>12.0} {length:>12.0}  {}\n",
            sm::ellipsize(f.get("name").and_then(Value::as_str).unwrap_or(""), 70)
        ));
    }
    Ok(Outcome::page(
        format!("transmission: {} file(s) in {name}", files.len()),
        format!("{}{body}", sm::heading(&format!("files: {name}"))),
    ))
}

/// `torrent-get` `peers` for one torrent.
pub fn transmission_peers(args: &[&str]) -> Result<Outcome, String> {
    let id = arg(args, 0, "transmission-peers <id>")?
        .parse::<i64>()
        .map_err(|_| "usage: transmission-peers <id>".to_string())?;
    let arguments = rpc(
        "torrent-get",
        json!({ "ids": [id], "fields": ["name", "peers"] }),
    )?;
    let torrent = arguments
        .get("torrents")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .cloned()
        .ok_or_else(|| format!("transmission: no torrent #{id}"))?;
    let name = torrent.get("name").and_then(Value::as_str).unwrap_or("");
    let peers = torrent
        .get("peers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut body = format!(
        "{:<40} {:>5} {:>7} {:>7}  {}\n",
        "ADDRESS", "DONE", "DOWN", "UP", "CLIENT"
    );
    for p in &peers {
        let address = p.get("address").and_then(Value::as_str).unwrap_or("");
        let port = p.get("port").and_then(Value::as_i64).unwrap_or(0);
        let progress = p.get("progress").and_then(Value::as_f64).unwrap_or(0.0) * 100.0;
        let down = p.get("rateToClient").and_then(Value::as_f64).unwrap_or(0.0);
        let up = p.get("rateToPeer").and_then(Value::as_f64).unwrap_or(0.0);
        let client = p.get("clientName").and_then(Value::as_str).unwrap_or("");
        body.push_str(&format!(
            "{:<40} {progress:>4.0}% {:>7} {:>7}  {}\n",
            sm::ellipsize(&format!("{address}:{port}"), 40),
            fmt_rate(down),
            fmt_rate(up),
            sm::ellipsize(client, 30)
        ));
    }
    Ok(Outcome::page(
        format!("transmission: {} peer(s) on {name}", peers.len()),
        format!("{}{body}", sm::heading(&format!("peers: {name}"))),
    ))
}

// ---------------------------------------------------------------------------
// vagrant layer
// ---------------------------------------------------------------------------

/// `vagrant <sub> [args…]` in the current directory, paged.
fn vagrant(sub: &str, args: &[&str]) -> Result<Outcome, String> {
    require("vagrant", "install vagrant")?;
    let mut argv = vec![sub.to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    let out = run_owned("vagrant", &argv)?;
    Ok(output_page(&format!("vagrant {}", argv.join(" ")), out))
}

/// `vagrant up`.
pub fn vagrant_up(args: &[&str]) -> Result<Outcome, String> {
    vagrant("up", args)
}

/// `vagrant halt`.
pub fn vagrant_halt(args: &[&str]) -> Result<Outcome, String> {
    vagrant("halt", args)
}

/// `vagrant suspend`.
pub fn vagrant_suspend(args: &[&str]) -> Result<Outcome, String> {
    vagrant("suspend", args)
}

/// `vagrant resume`.
pub fn vagrant_resume(args: &[&str]) -> Result<Outcome, String> {
    vagrant("resume", args)
}

/// `vagrant reload`.
pub fn vagrant_reload(args: &[&str]) -> Result<Outcome, String> {
    vagrant("reload", args)
}

/// `vagrant destroy -f` — non-interactive, since there is no prompt to answer.
pub fn vagrant_destroy(args: &[&str]) -> Result<Outcome, String> {
    let mut argv = vec!["-f"];
    argv.extend_from_slice(args);
    vagrant("destroy", &argv)
}

/// `vagrant provision`.
pub fn vagrant_provision(args: &[&str]) -> Result<Outcome, String> {
    vagrant("provision", args)
}

/// `vagrant status`.
pub fn vagrant_status(args: &[&str]) -> Result<Outcome, String> {
    vagrant("status", args)
}

/// Upstream `vagrant ssh` drops you into a shell inside the box. A `:` command
/// runs one process to completion and has no terminal to give it, so this
/// returns `vagrant ssh-config` — the host/port/key an ssh client (or the
/// editor's terminal) needs to open that session itself.
pub fn vagrant_ssh_command(args: &[&str]) -> Result<Outcome, String> {
    require("vagrant", "install vagrant")?;
    let mut argv = vec!["ssh-config".to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    let out = run_owned("vagrant", &argv)?;
    Ok(Outcome::page(
        "vagrant: ssh-config (run `vagrant ssh` in a terminal for the session itself)".to_string(),
        format!("{}{out}", sm::heading("vagrant ssh-config")),
    ))
}

// ---------------------------------------------------------------------------
// conda layer
// ---------------------------------------------------------------------------

/// PATH as it was before the first `conda_activate`, so deactivating restores
/// exactly what the process started with.
static CONDA_PREV_PATH: Mutex<Option<String>> = Mutex::new(None);
/// The prefix currently prepended to PATH by `conda_activate`.
static CONDA_ACTIVE_PREFIX: Mutex<Option<String>> = Mutex::new(None);

/// `conda` on PATH, else `$CONDA_HOME/bin/conda`, else `~/.anaconda3/bin/conda`.
fn conda_bin() -> Result<String, String> {
    if let Some(path) = sm::which("conda") {
        return Ok(path.to_string_lossy().into_owned());
    }
    let mut roots = Vec::new();
    if let Ok(root) = std::env::var("CONDA_HOME") {
        roots.push(PathBuf::from(root));
    }
    roots.push(home().join(".anaconda3"));
    for root in roots {
        let candidate = root.join("bin").join("conda");
        if candidate.is_file() {
            return Ok(candidate.to_string_lossy().into_owned());
        }
    }
    Err("`conda` not found on PATH, in $CONDA_HOME/bin or ~/.anaconda3/bin".to_string())
}

/// Find an env's prefix in `conda env list` output. Rows look like
/// `base   *  /opt/anaconda3` / `myenv     /opt/anaconda3/envs/myenv`; a prefix
/// path is accepted as `want` too.
fn conda_env_prefix(listing: &str, want: &str) -> Option<String> {
    for line in listing.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields: Vec<&str> = line.split_whitespace().collect();
        let Some(prefix) = fields.pop() else { continue };
        if !prefix.starts_with('/') {
            continue;
        }
        let name = fields.first().copied().unwrap_or("");
        let basename = prefix.rsplit('/').next().unwrap_or("");
        if name == want || prefix == want || basename == want {
            return Some(prefix.to_string());
        }
    }
    None
}

/// `conda env list`.
pub fn conda_env_list(_args: &[&str]) -> Result<Outcome, String> {
    let conda = conda_bin()?;
    let out = sm::run(&conda, &["env", "list"])?;
    Ok(output_page("conda env list", out))
}

/// Activate an env for *this* process. `conda activate` is a shell function
/// upstream, which a non-shell process cannot call; the part that matters for a
/// long-lived editor is the environment it leaves behind, so this resolves the
/// prefix and sets `PATH`/`CONDA_PREFIX`/`CONDA_DEFAULT_ENV` directly. Every
/// subprocess started afterwards (LSP servers, terminals, run configurations)
/// inherits them.
pub fn conda_activate(args: &[&str]) -> Result<Outcome, String> {
    let name = arg(args, 0, "conda-activate <env>")?;
    let conda = conda_bin()?;
    let listing = sm::run(&conda, &["env", "list"])?;
    let prefix = conda_env_prefix(&listing, name)
        .ok_or_else(|| format!("conda: no env named `{name}`"))?;
    let bin = format!("{prefix}/bin");
    if !Path::new(&bin).is_dir() {
        return Err(format!("conda: {bin} does not exist"));
    }

    let path = std::env::var("PATH").unwrap_or_default();
    {
        let mut prev = CONDA_PREV_PATH.lock().unwrap_or_else(|e| e.into_inner());
        if prev.is_none() {
            *prev = Some(path.clone());
        }
    }
    // Drop a previously activated prefix so repeated activations don't stack.
    let previous_prefix = CONDA_ACTIVE_PREFIX
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let cleaned = match previous_prefix {
        Some(old) => path_without(&path, &format!("{old}/bin")),
        None => path,
    };
    std::env::set_var("PATH", format!("{bin}:{cleaned}"));
    std::env::set_var("CONDA_PREFIX", &prefix);
    std::env::set_var("CONDA_DEFAULT_ENV", name);
    *CONDA_ACTIVE_PREFIX
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(prefix.clone());

    Ok(Outcome::status(format!("conda: activated {name} ({prefix})")))
}

/// Undo [`conda_activate`]: restore the pre-activation PATH (or, failing that,
/// strip the env's `bin`) and clear the two conda variables.
pub fn conda_deactivate(_args: &[&str]) -> Result<Outcome, String> {
    let prefix = CONDA_ACTIVE_PREFIX
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    let Some(prefix) = prefix else {
        return Ok(Outcome::status("conda: no env active"));
    };
    let previous = CONDA_PREV_PATH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    match previous {
        Some(path) => std::env::set_var("PATH", path),
        None => {
            let path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("PATH", path_without(&path, &format!("{prefix}/bin")));
        }
    }
    std::env::remove_var("CONDA_PREFIX");
    std::env::remove_var("CONDA_DEFAULT_ENV");
    Ok(Outcome::status(format!("conda: deactivated {prefix}")))
}

/// The env this process currently has activated.
pub fn conda_env_current(_args: &[&str]) -> Result<Outcome, String> {
    let name = std::env::var("CONDA_DEFAULT_ENV").unwrap_or_default();
    if name.is_empty() {
        return Ok(Outcome::status("conda: none"));
    }
    let prefix = std::env::var("CONDA_PREFIX").unwrap_or_default();
    Ok(Outcome::status(format!("conda: {name} ({prefix})")))
}

/// `path` with every occurrence of `entry` removed.
fn path_without(path: &str, entry: &str) -> String {
    path.split(':')
        .filter(|p| !p.is_empty() && *p != entry)
        .collect::<Vec<_>>()
        .join(":")
}

// ---------------------------------------------------------------------------
// elasticsearch layer
// ---------------------------------------------------------------------------

fn es_base() -> String {
    std::env::var("ES_URL")
        .unwrap_or_else(|_| "http://localhost:9200".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// `GET /_cluster/health?pretty`.
pub fn es_health(_args: &[&str]) -> Result<Outcome, String> {
    let out = sm::http_get(&format!("{}/_cluster/health?pretty", es_base()), &[])?;
    Ok(output_page("elasticsearch health", out))
}

/// `GET /_cat/indices?v`.
pub fn es_indices(_args: &[&str]) -> Result<Outcome, String> {
    let out = sm::http_get(&format!("{}/_cat/indices?v", es_base()), &[])?;
    Ok(output_page("elasticsearch indices", out))
}

/// `GET /_cat/nodes?v`.
pub fn es_nodes(_args: &[&str]) -> Result<Outcome, String> {
    let out = sm::http_get(&format!("{}/_cat/nodes?v", es_base()), &[])?;
    Ok(output_page("elasticsearch nodes", out))
}

/// `GET /<index>/_search?q=<lucene>` — the first 20 hits, one line each.
pub fn es_search(args: &[&str]) -> Result<Outcome, String> {
    let index = arg(args, 0, "es-search <index> <lucene-query>")?;
    if args.len() < 2 {
        return Err("usage: es-search <index> <lucene-query>".to_string());
    }
    let query = args[1..].join(" ");
    let url = format!(
        "{}/{}/_search?q={}&size=20",
        es_base(),
        index,
        sm::urlencode(&query)
    );
    let value = sm::http_get_json(&url, &[])?;
    let hits = value
        .get("hits")
        .and_then(|h| h.get("hits"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = value
        .get("hits")
        .and_then(|h| h.get("total"))
        .and_then(|t| t.get("value").cloned().or_else(|| Some(t.clone())))
        .and_then(|t| t.as_i64())
        .unwrap_or(hits.len() as i64);

    let mut body = String::new();
    for hit in &hits {
        let id = hit.get("_id").and_then(Value::as_str).unwrap_or("");
        let score = hit.get("_score").and_then(Value::as_f64).unwrap_or(0.0);
        let source = hit
            .get("_source")
            .map(|s| s.to_string())
            .unwrap_or_default();
        body.push_str(&format!(
            "{id:<24} {score:>7.3}  {}\n",
            sm::ellipsize(&source, 100)
        ));
    }
    Ok(Outcome::page(
        format!("es: {} hit(s) of {total} in {index}", hits.len()),
        format!(
            "{}{body}",
            sm::heading(&format!("es {index}: {query}"))
        ),
    ))
}

/// es-mode's "send the request under point": `<METHOD> <path> [json body…]`
/// against the configured cluster.
///
/// The HTTP substrate exposes GET and POST only, so PUT is issued as POST —
/// which Elasticsearch accepts for search/aggregation endpoints but not for
/// index/document creation. Methods other than GET/POST/PUT are rejected rather
/// than silently downgraded.
pub fn es_request(args: &[&str]) -> Result<Outcome, String> {
    let method = arg(args, 0, "es-request <METHOD> <path> [json body]")?.to_uppercase();
    let path = arg(args, 1, "es-request <METHOD> <path> [json body]")?;
    let url = format!("{}/{}", es_base(), path.trim_start_matches('/'));
    let raw_body = args.get(2..).map(|rest| rest.join(" ")).unwrap_or_default();

    let out = match method.as_str() {
        "GET" => sm::http_get(&url, &[])?,
        "POST" | "PUT" => {
            let body: Value = if raw_body.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&raw_body).map_err(|e| format!("es: body is not json: {e}"))?
            };
            let value = sm::http_post_json(&url, &[], &body)?;
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
        }
        other => {
            return Err(format!(
                "es-request: {other} is not supported (GET, POST and PUT only)"
            ))
        }
    };
    Ok(output_page(&format!("es {method} /{}", path.trim_start_matches('/')), out))
}

// ---------------------------------------------------------------------------
// quickurl layer
// ---------------------------------------------------------------------------

/// `quickurl.el` keeps its store as a lisp alist it `read`s back. There is no
/// lisp reader here, so zmax uses a plain two-column `name<TAB>url` file at
/// `~/.quickurls` — same location, readable format, no evaluator required.
fn quickurl_file() -> PathBuf {
    home().join(".quickurls")
}

fn parse_quickurls(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                return None;
            }
            let (name, url) = line.split_once('\t')?;
            let (name, url) = (name.trim(), url.trim());
            (!name.is_empty() && !url.is_empty()).then(|| (name.to_string(), url.to_string()))
        })
        .collect()
}

fn serialise_quickurls(rows: &[(String, String)]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = rows
        .iter()
        .map(|(name, url)| format!("{name}\t{url}"))
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    out
}

fn load_quickurls() -> Vec<(String, String)> {
    std::fs::read_to_string(quickurl_file())
        .map(|text| parse_quickurls(&text))
        .unwrap_or_default()
}

/// The stored entries.
pub fn quickurl_list(_args: &[&str]) -> Result<Outcome, String> {
    let rows = load_quickurls();
    let mut body = String::new();
    for (name, url) in &rows {
        body.push_str(&format!("{name:<24} {url}\n"));
    }
    Ok(Outcome::page(
        format!("quickurl: {} entr{}", rows.len(), if rows.len() == 1 { "y" } else { "ies" }),
        format!("{}{body}", sm::heading("quickurl")),
    ))
}

/// `<name> <url>` — add, replacing any entry with the same name.
pub fn quickurl_add(args: &[&str]) -> Result<Outcome, String> {
    let name = arg(args, 0, "quickurl-add <name> <url>")?;
    let url = arg(args, 1, "quickurl-add <name> <url>")?;
    let mut rows = load_quickurls();
    let replaced = rows.iter().any(|(n, _)| n == name);
    rows.retain(|(n, _)| n != name);
    rows.push((name.to_string(), url.to_string()));
    std::fs::write(quickurl_file(), serialise_quickurls(&rows))
        .map_err(|e| format!("quickurl: {e}"))?;
    Ok(Outcome::status(format!(
        "quickurl: {} {name} → {url}",
        if replaced { "replaced" } else { "added" }
    )))
}

/// `<name>` — the stored url, for the caller to insert at point.
pub fn quickurl_lookup(args: &[&str]) -> Result<Outcome, String> {
    let name = arg(args, 0, "quickurl-lookup <name>")?;
    load_quickurls()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, url)| Outcome::status(url))
        .ok_or_else(|| format!("quickurl: no entry named `{name}`"))
}

/// `<name>` — the stored url, for the caller to open in a browser.
pub fn quickurl_browse(args: &[&str]) -> Result<Outcome, String> {
    quickurl_lookup(args)
}

// ---------------------------------------------------------------------------
// sailfish-developer layer
// ---------------------------------------------------------------------------

/// `.rpm` files under `RPMS/` in the current directory, which is where `mb2`
/// leaves its build output.
fn built_rpms() -> Result<Vec<String>, String> {
    let dir = std::env::current_dir()
        .map_err(|e| format!("sailfish: {e}"))?
        .join("RPMS");
    let entries = std::fs::read_dir(&dir)
        .map_err(|_| format!("sailfish: no build output in {}", dir.display()))?;
    let mut rpms: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "rpm"))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    rpms.sort();
    if rpms.is_empty() {
        return Err(format!("sailfish: no .rpm files in {}", dir.display()));
    }
    Ok(rpms)
}

/// `mb2 build` in the current directory.
pub fn sailfish_build(args: &[&str]) -> Result<Outcome, String> {
    require("mb2", "install the Sailfish SDK (mb2)")?;
    let mut argv = vec!["build".to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    let out = run_owned("mb2", &argv)?;
    Ok(output_page("mb2 build", out))
}

/// `sb2 -t $SAILFISH_SB2_TARGET rpm -i <rpms>` — the built RPMs, or the ones
/// named in `args`.
pub fn sailfish_install(args: &[&str]) -> Result<Outcome, String> {
    require("sb2", "install the Sailfish SDK (sb2)")?;
    let target = std::env::var("SAILFISH_SB2_TARGET")
        .map_err(|_| "sailfish: set $SAILFISH_SB2_TARGET to the sb2 target name".to_string())?;
    let rpms = if args.is_empty() {
        built_rpms()?
    } else {
        args.iter().map(|a| a.to_string()).collect()
    };
    let mut argv = vec![
        "-t".to_string(),
        target.clone(),
        "rpm".to_string(),
        "-i".to_string(),
    ];
    argv.extend(rpms.iter().cloned());
    let out = run_owned("sb2", &argv)?;
    Ok(Outcome::page(
        format!("sailfish: installed {} rpm(s) into {target}", rpms.len()),
        format!("{}{out}", sm::heading(&format!("sb2 -t {target} rpm -i"))),
    ))
}

/// `scp <rpms> $SAILFISH_DEVICE` — copy the build output to the device
/// (`user@host:path`).
pub fn sailfish_deploy(args: &[&str]) -> Result<Outcome, String> {
    require("scp", "install openssh")?;
    let device = std::env::var("SAILFISH_DEVICE").map_err(|_| {
        "sailfish: set $SAILFISH_DEVICE to the device destination (user@host:path)".to_string()
    })?;
    let rpms = if args.is_empty() {
        built_rpms()?
    } else {
        args.iter().map(|a| a.to_string()).collect()
    };
    let mut argv = rpms.clone();
    argv.push(device.clone());
    let out = run_owned("scp", &argv)?;
    Ok(Outcome::page(
        format!("sailfish: copied {} rpm(s) to {device}", rpms.len()),
        format!("{}{out}", sm::heading(&format!("scp → {device}"))),
    ))
}

// ---------------------------------------------------------------------------
// perforce layer
// ---------------------------------------------------------------------------

/// Full argv for a `p4` call: the subcommand words, then whatever the caller
/// passed (the command layer substitutes the current buffer's path when the
/// user gave no argument, so `args` arrives complete).
fn p4_argv(sub: &[&str], args: &[&str]) -> Vec<String> {
    sub.iter()
        .chain(args.iter())
        .map(|s| s.to_string())
        .collect()
}

fn p4(sub: &[&str], args: &[&str]) -> Result<Outcome, String> {
    require("p4", "install the Perforce command-line client")?;
    let argv = p4_argv(sub, args);
    let out = run_owned("p4", &argv)?;
    Ok(output_page(&format!("p4 {}", argv.join(" ")), out))
}

/// `p4 <args…>` — the escape hatch for any subcommand without a wrapper.
pub fn p4_run(args: &[&str]) -> Result<Outcome, String> {
    if args.is_empty() {
        return Err("usage: p4 <subcommand> [args…]".to_string());
    }
    p4(&[], args)
}

/// `p4 add`.
pub fn p4_add(args: &[&str]) -> Result<Outcome, String> {
    p4(&["add"], args)
}

/// `p4 delete`.
pub fn p4_delete(args: &[&str]) -> Result<Outcome, String> {
    p4(&["delete"], args)
}

/// `p4 describe`.
pub fn p4_describe(args: &[&str]) -> Result<Outcome, String> {
    p4(&["describe"], args)
}

/// `p4 edit` — open for edit.
pub fn p4_edit(args: &[&str]) -> Result<Outcome, String> {
    p4(&["edit"], args)
}

/// `p4 revert`.
pub fn p4_revert(args: &[&str]) -> Result<Outcome, String> {
    p4(&["revert"], args)
}

/// `p4 sync -f` — force-refresh the file from the depot.
pub fn p4_refresh(args: &[&str]) -> Result<Outcome, String> {
    p4(&["sync", "-f"], args)
}

/// `p4 submit`.
pub fn p4_submit(args: &[&str]) -> Result<Outcome, String> {
    p4(&["submit"], args)
}

/// `p4 shelve`.
pub fn p4_shelve(args: &[&str]) -> Result<Outcome, String> {
    p4(&["shelve"], args)
}

/// `p4 unshelve`.
pub fn p4_unshelve(args: &[&str]) -> Result<Outcome, String> {
    p4(&["unshelve"], args)
}

/// `p4 branches`.
pub fn p4_branches(args: &[&str]) -> Result<Outcome, String> {
    p4(&["branches"], args)
}

/// `p4 changes`.
pub fn p4_changes(args: &[&str]) -> Result<Outcome, String> {
    p4(&["changes"], args)
}

/// `p4 filelog` — revision history of a file.
pub fn p4_filelog(args: &[&str]) -> Result<Outcome, String> {
    p4(&["filelog"], args)
}

/// `p4 files`.
pub fn p4_files(args: &[&str]) -> Result<Outcome, String> {
    p4(&["files"], args)
}

/// `p4 info` — client/server configuration.
pub fn p4_info(args: &[&str]) -> Result<Outcome, String> {
    p4(&["info"], args)
}

/// `p4 sync`.
pub fn p4_sync(args: &[&str]) -> Result<Outcome, String> {
    p4(&["sync"], args)
}

/// `p4 opened` — files open in the current changelist.
pub fn p4_opened(args: &[&str]) -> Result<Outcome, String> {
    p4(&["opened"], args)
}

/// `p4 print` — depot contents of a file.
pub fn p4_print(args: &[&str]) -> Result<Outcome, String> {
    p4(&["print"], args)
}

/// `p4 resolve` — non-interactive only; pass a flag such as `-am`.
pub fn p4_resolve(args: &[&str]) -> Result<Outcome, String> {
    p4(&["resolve"], args)
}

/// `p4 diff`.
pub fn p4_diff(args: &[&str]) -> Result<Outcome, String> {
    p4(&["diff"], args)
}

/// `p4 users`.
pub fn p4_users(args: &[&str]) -> Result<Outcome, String> {
    p4(&["users"], args)
}

/// `p4 where` — depot/client/local mapping of a path.
pub fn p4_where(args: &[&str]) -> Result<Outcome, String> {
    p4(&["where"], args)
}

/// `p4 reconcile` — find work done outside of Perforce.
pub fn p4_reconcile(args: &[&str]) -> Result<Outcome, String> {
    p4(&["reconcile"], args)
}

/// `p4 annotate` — per-line revision attribution (perforce's blame).
pub fn p4_blame(args: &[&str]) -> Result<Outcome, String> {
    p4(&["annotate"], args)
}

/// `p4 jobs`.
pub fn p4_jobs(args: &[&str]) -> Result<Outcome, String> {
    p4(&["jobs"], args)
}

/// `p4 labels`.
pub fn p4_labels(args: &[&str]) -> Result<Outcome, String> {
    p4(&["labels"], args)
}

/// `p4 clients`.
pub fn p4_clients(args: &[&str]) -> Result<Outcome, String> {
    p4(&["clients"], args)
}

// ---------------------------------------------------------------------------
// dash layer — Dash.app on macOS, Zeal elsewhere
// ---------------------------------------------------------------------------

fn on_macos() -> bool {
    cfg!(target_os = "macos")
}

/// `open -g dash-plugin://…` on macOS, `zeal --query …` elsewhere. `-g` keeps
/// the editor focused, matching dash-at-point's behaviour.
fn docs_lookup(dash_url: String, zeal_query: String) -> Result<(), String> {
    if on_macos() && sm::have("open") {
        sm::run("open", &["-g", &dash_url])?;
        return Ok(());
    }
    if sm::have("zeal") {
        sm::run("zeal", &["--query", &zeal_query])?;
        return Ok(());
    }
    Err("dash: neither Dash.app (macOS `open dash-plugin://`) nor `zeal` is available".to_string())
}

/// Look the term up in the documentation browser.
pub fn dash_at_point(args: &[&str]) -> Result<Outcome, String> {
    let term = joined(args);
    if term.trim().is_empty() {
        return Err("usage: dash-at-point <term>".to_string());
    }
    docs_lookup(
        format!("dash-plugin://query={}", sm::urlencode(&term)),
        term.clone(),
    )?;
    Ok(Outcome::status(format!("dash: {term}")))
}

/// Same, restricted to one docset keyword: `<docset> <term…>`.
pub fn dash_at_point_with_docset(args: &[&str]) -> Result<Outcome, String> {
    let docset = arg(args, 0, "dash-at-point-with-docset <docset> <term>")?;
    if args.len() < 2 {
        return Err("usage: dash-at-point-with-docset <docset> <term>".to_string());
    }
    let term = args[1..].join(" ");
    docs_lookup(
        format!(
            "dash-plugin://keys={}&query={}",
            sm::urlencode(docset),
            sm::urlencode(&term)
        ),
        format!("{docset}:{term}"),
    )?;
    Ok(Outcome::status(format!("dash: {docset}:{term}")))
}

/// Installed docsets, read from the browser's docset directory.
pub fn dash_docsets(_args: &[&str]) -> Result<Outcome, String> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if on_macos() {
        dirs.push(
            home()
                .join("Library/Application Support/Dash/DocSets"),
        );
    }
    match std::env::var_os("XDG_DATA_HOME") {
        Some(xdg) => dirs.push(PathBuf::from(xdg).join("Zeal/Zeal/docsets")),
        None => dirs.push(home().join(".local/share/Zeal/Zeal/docsets")),
    }

    let mut names: Vec<String> = Vec::new();
    let mut searched = Vec::new();
    for dir in &dirs {
        searched.push(dir.display().to_string());
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Dash nests `<Name>.docset` directly; Zeal nests them one level in.
            if name.ends_with(".docset") {
                names.push(name.trim_end_matches(".docset").to_string());
            } else if path.is_dir() {
                if let Ok(inner) = std::fs::read_dir(&path) {
                    for sub in inner.filter_map(Result::ok) {
                        let sub_name = sub.file_name().to_string_lossy().into_owned();
                        if sub_name.ends_with(".docset") {
                            names.push(sub_name.trim_end_matches(".docset").to_string());
                        }
                    }
                }
            }
        }
    }
    names.sort();
    names.dedup();
    if names.is_empty() {
        return Err(format!("dash: no docsets under {}", searched.join(", ")));
    }
    Ok(Outcome::page(
        format!("dash: {} docset(s)", names.len()),
        format!("{}{}\n", sm::heading("docsets"), names.join("\n")),
    ))
}

// ---------------------------------------------------------------------------
// djvu layer — djvulibre command line tools
// ---------------------------------------------------------------------------

/// `djvutxt <file>`, or one page with `--page=<n>`.
pub fn djvu_text(args: &[&str]) -> Result<Outcome, String> {
    require("djvutxt", "install djvulibre")?;
    let file = arg(args, 0, "djvu-text <file> [page]")?;
    let out = match args.get(1) {
        Some(page) => {
            page.parse::<u32>()
                .map_err(|_| format!("djvu: `{page}` is not a page number"))?;
            sm::run("djvutxt", &[&format!("--page={page}"), file])?
        }
        None => sm::run("djvutxt", &[file])?,
    };
    let title = match args.get(1) {
        Some(page) => format!("{file} p{page}"),
        None => file.to_string(),
    };
    Ok(output_page(&title, out))
}

/// `djvused -e n <file>` — the page count.
pub fn djvu_pages(args: &[&str]) -> Result<Outcome, String> {
    require("djvused", "install djvulibre")?;
    let file = arg(args, 0, "djvu-pages <file>")?;
    let pages = djvu_page_count(file)?;
    Ok(Outcome::status(format!("{file}: {pages} page(s)")))
}

fn djvu_page_count(file: &str) -> Result<u32, String> {
    let out = sm::run("djvused", &["-e", "n", file])?;
    out.trim()
        .lines()
        .last()
        .and_then(|l| l.trim().parse::<u32>().ok())
        .ok_or_else(|| format!("djvu: could not read a page count from `{}`", out.trim()))
}

/// `djvused -e print-outline <file>` — the document bookmarks.
pub fn djvu_outline(args: &[&str]) -> Result<Outcome, String> {
    require("djvused", "install djvulibre")?;
    let file = arg(args, 0, "djvu-outline <file>")?;
    let out = sm::run("djvused", &["-e", "print-outline", file])?;
    Ok(output_page(&format!("outline: {file}"), out))
}

/// Matching lines of one page's text, prefixed with the page number.
fn occur_hits(page: u32, text: &str, needle_lower: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && line.to_lowercase().contains(needle_lower))
        .map(|line| format!("page {page}: {line}"))
        .collect()
}

/// `<file> <pattern>` — every line containing `pattern` (plain substring,
/// case-insensitive), page by page. djvulibre has no search of its own, so this
/// extracts each page's text and filters it.
pub fn djvu_occur(args: &[&str]) -> Result<Outcome, String> {
    require("djvutxt", "install djvulibre")?;
    require("djvused", "install djvulibre")?;
    let file = arg(args, 0, "djvu-occur <file> <pattern>")?;
    if args.len() < 2 {
        return Err("usage: djvu-occur <file> <pattern>".to_string());
    }
    let needle = args[1..].join(" ").to_lowercase();
    let pages = djvu_page_count(file)?;

    let mut hits: Vec<String> = Vec::new();
    for page in 1..=pages {
        let Ok(text) = sm::run("djvutxt", &[&format!("--page={page}"), file]) else {
            continue;
        };
        hits.extend(occur_hits(page, &text, &needle));
    }
    Ok(Outcome::page(
        format!("djvu: {} hit(s) for `{}`", hits.len(), args[1..].join(" ")),
        format!("{}{}\n", sm::heading(&format!("occur: {file}")), hits.join("\n")),
    ))
}

/// `<file> <page> [out]` — render one page with `ddjvu -format=ppm`.
pub fn djvu_export_page(args: &[&str]) -> Result<Outcome, String> {
    require("ddjvu", "install djvulibre")?;
    let file = arg(args, 0, "djvu-export-page <file> <page> [out]")?;
    let page = arg(args, 1, "djvu-export-page <file> <page> [out]")?;
    page.parse::<u32>()
        .map_err(|_| format!("djvu: `{page}` is not a page number"))?;
    let out_path = match args.get(2) {
        Some(p) => p.to_string(),
        None => {
            let stem = Path::new(file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "page".to_string());
            format!("{stem}-p{page}.ppm")
        }
    };
    sm::run(
        "ddjvu",
        &["-format=ppm", &format!("-page={page}"), file, &out_path],
    )?;
    Ok(Outcome::status(format!(
        "djvu: wrote page {page} to {out_path}"
    )))
}

// ---------------------------------------------------------------------------
// node layer — nvm + npm
// ---------------------------------------------------------------------------

fn nvm_versions_dir() -> PathBuf {
    std::env::var_os("NVM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".nvm"))
        .join("versions")
        .join("node")
}

fn installed_node_versions() -> Result<Vec<String>, String> {
    let dir = nvm_versions_dir();
    let entries = std::fs::read_dir(&dir)
        .map_err(|_| format!("nvm: no node versions under {}", dir.display()))?;
    let mut versions: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    versions.sort();
    Ok(versions)
}

/// Pick an installed version for `want`: exact match first, then with/without
/// the `v` prefix, then the highest version that starts with it (so `18` finds
/// `v18.20.4`).
fn resolve_node_version(available: &[String], want: &str) -> Option<String> {
    let want_v = if want.starts_with('v') {
        want.to_string()
    } else {
        format!("v{want}")
    };
    if let Some(hit) = available.iter().find(|v| *v == want || **v == want_v) {
        return Some(hit.clone());
    }
    available
        .iter()
        .filter(|v| v.starts_with(&want_v) || v.starts_with(want))
        .max()
        .cloned()
}

/// Node versions installed under `$NVM_DIR/versions/node`.
pub fn nvm_list(_args: &[&str]) -> Result<Outcome, String> {
    let versions = installed_node_versions()?;
    Ok(Outcome::page(
        format!("nvm: {} version(s)", versions.len()),
        format!("{}{}\n", sm::heading("node versions"), versions.join("\n")),
    ))
}

/// Put a version's `bin` at the front of this process's `PATH`. `nvm use` is a
/// shell function upstream; what carries over to a long-lived editor is the
/// PATH change, which every later subprocess (LSP servers, terminals, run
/// configurations) inherits.
pub fn nvm_use(args: &[&str]) -> Result<Outcome, String> {
    let want = arg(args, 0, "nvm-use <version>")?;
    let versions = installed_node_versions()?;
    let version = resolve_node_version(&versions, want)
        .ok_or_else(|| format!("nvm: no installed version matching `{want}`"))?;
    let bin = nvm_versions_dir().join(&version).join("bin");
    if !bin.is_dir() {
        return Err(format!("nvm: {} does not exist", bin.display()));
    }
    let node = bin.join("node");
    if !node.is_file() {
        return Err(format!("nvm: {} does not exist", node.display()));
    }

    let path = std::env::var("PATH").unwrap_or_default();
    // Drop any other nvm version already on PATH so repeated calls don't stack.
    let root = nvm_versions_dir();
    let cleaned: Vec<&str> = path
        .split(':')
        .filter(|p| !p.is_empty() && !Path::new(p).starts_with(&root))
        .collect();
    std::env::set_var(
        "PATH",
        format!("{}:{}", bin.to_string_lossy(), cleaned.join(":")),
    );

    let reported = sm::run(&node.to_string_lossy(), &["--version"])
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|_| version.clone());
    Ok(Outcome::status(format!(
        "node: {reported} ({})",
        node.display()
    )))
}

/// The `scripts` table of `package.json` in the current directory.
pub fn npm_scripts(_args: &[&str]) -> Result<Outcome, String> {
    let path = std::env::current_dir()
        .map_err(|e| format!("npm: {e}"))?
        .join("package.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|_| format!("npm: no package.json at {}", path.display()))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("package.json: {e}"))?;
    let scripts = value
        .get("scripts")
        .and_then(Value::as_object)
        .ok_or_else(|| "npm: package.json has no \"scripts\"".to_string())?;

    let mut body = String::new();
    for (name, command) in scripts {
        body.push_str(&format!(
            "{name:<24} {}\n",
            command.as_str().unwrap_or_default()
        ));
    }
    Ok(Outcome::page(
        format!("npm: {} script(s)", scripts.len()),
        format!("{}{body}", sm::heading("npm scripts")),
    ))
}

/// `npm run <script> [args…]`.
pub fn npm_run(args: &[&str]) -> Result<Outcome, String> {
    require("npm", "install node")?;
    let script = arg(args, 0, "npm-run <script>")?;
    let mut argv = vec!["run".to_string(), script.to_string()];
    argv.extend(args[1..].iter().map(|a| a.to_string()));
    let out = run_owned("npm", &argv)?;
    Ok(output_page(&format!("npm run {script}"), out))
}

// ---------------------------------------------------------------------------
// tests — pure logic only (no process spawning, no network)
// ---------------------------------------------------------------------------

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
        // The shape the Transmission Authorization header actually carries.
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
    }

    #[test]
    fn transmission_status_codes_map_to_the_rpc_names() {
        let names: Vec<&str> = (0..7).map(status_name).collect();
        assert_eq!(
            names,
            [
                "stopped",
                "check-wait",
                "check",
                "dl-wait",
                "download",
                "seed-wait",
                "seed"
            ]
        );
        assert_eq!(status_name(99), "?");
        assert_eq!(status_name(-1), "?");
    }

    #[test]
    fn transmission_eta_hides_the_rpcs_negative_unknown() {
        assert_eq!(fmt_eta(-1), "-");
        assert_eq!(fmt_eta(45), "45s");
        assert_eq!(fmt_eta(125), "2m05s");
        assert_eq!(fmt_eta(7325), "2h02m");
    }

    #[test]
    fn quickurl_entries_round_trip_through_the_two_column_format() {
        let rows = vec![
            ("docs".to_string(), "https://example.com/docs".to_string()),
            ("api".to_string(), "https://example.com/api?a=1".to_string()),
        ];
        let text = serialise_quickurls(&rows);
        assert_eq!(
            text,
            "docs\thttps://example.com/docs\napi\thttps://example.com/api?a=1\n"
        );
        assert_eq!(parse_quickurls(&text), rows);
        assert_eq!(serialise_quickurls(&[]), "");
    }

    #[test]
    fn quickurl_skips_blank_comment_and_untabbed_lines() {
        let text = "\n# a comment\nnotabhere https://x\n  docs\thttps://example.com  \n";
        assert_eq!(
            parse_quickurls(text),
            vec![("docs".to_string(), "https://example.com".to_string())]
        );
    }

    #[test]
    fn prodigy_services_parse_with_optional_fields_defaulted() {
        let text = r#"[
            {"name": "web", "command": "npm", "args": ["run", "dev"],
             "cwd": "/srv/app", "tags": ["node"], "url": "http://localhost:3000"},
            {"name": "bare", "command": "true"}
        ]"#;
        let services = parse_services(text).expect("parses");
        assert_eq!(
            services[0],
            Service {
                name: "web".to_string(),
                command: "npm".to_string(),
                args: vec!["run".to_string(), "dev".to_string()],
                cwd: Some("/srv/app".to_string()),
                tags: vec!["node".to_string()],
                url: Some("http://localhost:3000".to_string()),
            }
        );
        assert_eq!(
            services[1],
            Service {
                name: "bare".to_string(),
                command: "true".to_string(),
                args: Vec::new(),
                cwd: None,
                tags: Vec::new(),
                url: None,
            }
        );
    }

    #[test]
    fn prodigy_rejects_a_service_without_a_command() {
        let err = parse_services(r#"[{"name": "web"}]"#).expect_err("no command");
        assert!(err.contains("web"), "{err}");
        assert!(parse_services("{}").is_err());
    }

    #[test]
    fn p4_argv_keeps_subcommand_words_ahead_of_the_file() {
        assert_eq!(
            p4_argv(&["sync", "-f"], &["main.rs"]),
            vec!["sync".to_string(), "-f".to_string(), "main.rs".to_string()]
        );
        assert_eq!(p4_argv(&["info"], &[]), vec!["info".to_string()]);
        // p4_run passes the whole command through with no wrapper subcommand.
        assert_eq!(
            p4_argv(&[], &["changes", "-m", "5"]),
            vec!["changes".to_string(), "-m".to_string(), "5".to_string()]
        );
    }

    #[test]
    fn djvu_occur_filters_case_insensitively_and_tags_the_page() {
        let text = "  The Quick Brown Fox\n\nnothing here\nquick again\n";
        assert_eq!(
            occur_hits(7, text, "quick"),
            vec![
                "page 7: The Quick Brown Fox".to_string(),
                "page 7: quick again".to_string()
            ]
        );
        assert!(occur_hits(1, text, "zebra").is_empty());
    }

    #[test]
    fn conda_prefix_resolves_by_name_active_marker_or_path() {
        let listing = "# conda environments:\n#\n\
                       base                  *  /opt/anaconda3\n\
                       ml                       /opt/anaconda3/envs/ml\n";
        assert_eq!(
            conda_env_prefix(listing, "base"),
            Some("/opt/anaconda3".to_string())
        );
        assert_eq!(
            conda_env_prefix(listing, "ml"),
            Some("/opt/anaconda3/envs/ml".to_string())
        );
        assert_eq!(
            conda_env_prefix(listing, "/opt/anaconda3/envs/ml"),
            Some("/opt/anaconda3/envs/ml".to_string())
        );
        assert_eq!(conda_env_prefix(listing, "missing"), None);
    }

    #[test]
    fn node_version_resolution_accepts_bare_majors() {
        let available = vec![
            "v18.19.0".to_string(),
            "v18.20.4".to_string(),
            "v20.11.1".to_string(),
        ];
        assert_eq!(
            resolve_node_version(&available, "v20.11.1"),
            Some("v20.11.1".to_string())
        );
        assert_eq!(
            resolve_node_version(&available, "20.11.1"),
            Some("v20.11.1".to_string())
        );
        assert_eq!(
            resolve_node_version(&available, "18"),
            Some("v18.20.4".to_string())
        );
        assert_eq!(resolve_node_version(&available, "16"), None);
    }

    #[test]
    fn path_without_drops_every_copy_of_an_entry() {
        assert_eq!(path_without("/a:/b:/a:/c", "/a"), "/b:/c");
        assert_eq!(path_without("/a::/b", "/x"), "/a:/b");
    }
}
