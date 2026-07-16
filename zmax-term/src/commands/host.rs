//! Client bridge to the `zwire-host` universal local host.
//!
//! `zwire-host` is a single privileged daemon that also backs the browser HUD,
//! tmux, and other apps: system stats, a filesystem crawler, command exec, a
//! per-app key/value store, PTY terminals, and clipboard/notify/open. zmax
//! talks to the *same* running daemon by shelling out to its `call` subcommand,
//! which handles the Unix-socket / Windows-named-pipe transport for us.
//!
//! Shelling out (rather than linking the host as a library) keeps the editor
//! free of the host's heavy PTY/sysinfo dependencies and means the editor and
//! every other client share one host process. All calls here are one-shot
//! request/reply — no streaming — so they run synchronously from a command with
//! negligible latency against a local daemon.
//!
//! The binary is looked up on `PATH` as `zwire-host`, overridable with
//! `$ZWIRE_HOST_BIN`. The editor **starts and manages the daemon itself**: the
//! first request that finds no daemon spawns `zwire-host serve` (detached, so the
//! shared host outlives the editor and is reused by the browser HUD / tmux /
//! other apps and the next editor session), waits for it to come up, and retries.
//! No manual `zwire-host serve` step is ever required.

use base64::Engine;
use serde_json::{json, Value};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// The host CLI binary; overridable via `$ZWIRE_HOST_BIN`.
fn host_bin() -> String {
    std::env::var("ZWIRE_HOST_BIN").unwrap_or_else(|_| "zwire-host".to_string())
}

/// Run `zwire-host call <request>` once, returning the raw process output.
/// A spawn failure (e.g. the binary isn't installed) becomes a friendly `Err`;
/// a running-but-daemonless host succeeds here with empty stdout.
fn run_call(request: &Value) -> Result<Output, String> {
    Command::new(host_bin())
        .arg("call")
        .arg(request.to_string())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "`{}` not found on PATH — install zwire-host (or set $ZWIRE_HOST_BIN)",
                    host_bin()
                )
            } else {
                format!("could not run zwire-host: {e}")
            }
        })
}

/// Parse a `zwire-host call` result. `Ok(None)` means the client produced no
/// reply line — the signal that no daemon was listening. `Ok(Some(v))` is a
/// successful reply; `Err` is a host-side `{"ok":false,"err":…}` or bad JSON.
fn parse_reply(output: &Output) -> Result<Option<Value>, String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Ok(None);
    }
    let reply: Value = serde_json::from_str(line).map_err(|e| format!("bad reply: {e}"))?;
    if reply.get("ok") == Some(&Value::Bool(false)) {
        let err = reply
            .get("err")
            .and_then(Value::as_str)
            .unwrap_or("host error");
        return Err(err.to_string());
    }
    Ok(Some(reply))
}

/// Send one request to the daemon, **auto-starting it if needed**.
///
/// If the first attempt finds no daemon, spawn one, wait for it to become ready,
/// and retry once. Surfaces host-side `{"ok":false,…}` replies as errors.
pub fn call(request: &Value) -> Result<Value, String> {
    if let Some(reply) = parse_reply(&run_call(request)?)? {
        return Ok(reply);
    }
    // No daemon answered — start and manage one ourselves, then retry.
    ensure_daemon()?;
    parse_reply(&run_call(request)?)?
        .ok_or_else(|| "started the zwire-host daemon but it gave no reply".to_string())
}

/// True if a daemon is currently answering.
fn daemon_alive() -> bool {
    run_call(&json!({ "cmd": "ping" }))
        .ok()
        .and_then(|o| parse_reply(&o).ok().flatten())
        .is_some()
}

/// Ensure a daemon is running, spawning and awaiting one if not. Serialized so
/// concurrent commands don't race to start several (the losers' `bind` fails
/// harmlessly and they connect to the winner).
fn ensure_daemon() -> Result<(), String> {
    static START_LOCK: Mutex<()> = Mutex::new(());
    let _guard = START_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    if daemon_alive() {
        return Ok(());
    }
    spawn_daemon()?;
    // The daemon binds its socket within a few ms; poll up to ~3s.
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(50));
        if daemon_alive() {
            return Ok(());
        }
    }
    Err("`zwire-host serve` was started but never became ready".to_string())
}

/// Spawn `zwire-host serve`, detached from the editor so the shared daemon
/// persists across editor exit and is reused by other clients.
fn spawn_daemon() -> Result<(), String> {
    let mut cmd = Command::new(host_bin());
    cmd.arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // New session id: the daemon leaves the editor's process/terminal group so
    // it isn't killed when the editor exits or the terminal closes.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: `setsid` is async-signal-safe and the only pre-exec action.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    cmd.spawn().map(|_| ()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "`{}` not found on PATH — install zwire-host (or set $ZWIRE_HOST_BIN)",
                host_bin()
            )
        } else {
            format!("could not start zwire-host daemon: {e}")
        }
    })
}

/// Decode a base64 reply field into a lossy-UTF-8 string.
fn decode_text(reply: &Value, field: &str) -> String {
    reply
        .get(field)
        .and_then(Value::as_str)
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// One-line system-stats summary (`sysinfo_once`).
pub fn sysinfo_summary() -> Result<String, String> {
    let reply = call(&json!({ "cmd": "sysinfo_once" }))?;
    let s = &reply["sys"];
    let load = s["load"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_f64)
                .map(|x| format!("{x:.2}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    Ok(format!(
        "cpu {}% · mem {}% · load {} · up {}s",
        s["cpu"].as_i64().unwrap_or(0),
        s["mem"]["p"].as_u64().unwrap_or(0),
        load,
        s["uptime"].as_u64().unwrap_or(0),
    ))
}

/// One-line machine facts (`hostinfo`).
pub fn hostinfo_summary() -> Result<String, String> {
    let r = call(&json!({ "cmd": "hostinfo" }))?;
    Ok(format!(
        "{} {} · {} cpus · {} · zwire-host {}",
        r["os"].as_str().unwrap_or("?"),
        r["arch"].as_str().unwrap_or("?"),
        r["cpus"].as_u64().unwrap_or(0),
        r["hostname"].as_str().unwrap_or("?"),
        r["host_version"].as_str().unwrap_or("?"),
    ))
}

/// Run a command through the host; returns `(exit_code, stdout, stderr)`.
pub fn exec(cmdline: &str) -> Result<(i64, String, String), String> {
    let mut parts = cmdline.split_whitespace();
    let program = parts.next().ok_or("empty command")?;
    let args: Vec<&str> = parts.collect();
    let reply = call(&json!({ "cmd": "exec", "program": program, "args": args }))?;
    Ok((
        reply["code"].as_i64().unwrap_or(-1),
        decode_text(&reply, "stdout"),
        decode_text(&reply, "stderr"),
    ))
}

/// Recursively crawl `path` via the host; returns the matching file paths. `ext`
/// filters by extension (no leading dot).
pub fn crawl(path: &str, ext: Option<&str>) -> Result<Vec<String>, String> {
    let mut req = json!({ "cmd": "fs_walk", "path": path });
    if let Some(e) = ext {
        req["ext"] = json!(e);
    }
    let reply = call(&req)?;
    Ok(reply["entries"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["path"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default())
}

/* ---- background jobs ---- */
//
// Long-running commands are shipped to the daemon with `job_start` and run there
// in the background. A single poller thread watches this editor's outstanding
// jobs and, when one finishes, pushes a status-line notice onto the main loop
// (rmail-style) and stashes the output for `:zwire-job-output`. The daemon also
// fires a desktop notification on completion, so you're told even when the
// editor isn't focused. The poller runs only while jobs are pending, and checks
// each job by id (`job_result`) so it never collects another client's jobs.

/// Keep the output of at most this many finished jobs for `take_output`.
const MAX_RESULTS: usize = 50;

struct CompletedJob {
    id: u64,
    label: String,
    code: Option<i64>,
    stdout: String,
    stderr: String,
}

/// Job ids submitted by this editor that the poller is still watching.
static PENDING: Mutex<Vec<u64>> = Mutex::new(Vec::new());
/// Output of recently finished jobs, newest last (capped at [`MAX_RESULTS`]).
static RESULTS: Mutex<Vec<CompletedJob>> = Mutex::new(Vec::new());
/// Whether a poller thread is currently running (exactly one at a time).
static POLLER: AtomicBool = AtomicBool::new(false);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ship a long-running command to the daemon as a background job. Returns the
/// job id immediately; completion is reported asynchronously on the status line
/// and via a desktop notification, and the output is kept for [`take_output`].
pub fn submit(cmdline: &str) -> Result<u64, String> {
    let mut parts = cmdline.split_whitespace();
    let program = parts.next().ok_or("empty command")?;
    let args: Vec<&str> = parts.collect();
    let reply = call(&json!({
        "cmd": "job_start",
        "program": program,
        "args": args,
        "label": cmdline,
        "notify": true,
    }))?;
    let id = reply["job"]
        .as_u64()
        .ok_or("daemon did not return a job id")?;

    // Register the job and start a poller if one isn't already running. Both the
    // push and the start-decision happen under the PENDING lock, serialized
    // against the poller's stop-decision, so no completion is ever dropped.
    let start_poller = {
        let mut pending = lock(&PENDING);
        pending.push(id);
        POLLER
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    };
    if start_poller {
        thread::spawn(poll_loop);
    }
    Ok(id)
}

/// Watch this editor's pending jobs, reporting each as it finishes, until none
/// remain. Exactly one instance runs at a time (guarded by [`POLLER`]).
fn poll_loop() {
    loop {
        thread::sleep(Duration::from_millis(1500));
        let ids: Vec<u64> = lock(&PENDING).clone();
        let mut finished = Vec::new();
        for id in ids {
            match call(&json!({ "cmd": "job_result", "id": id })) {
                Ok(r) if r.get("done").and_then(Value::as_bool) == Some(true) => {
                    stash_and_report(&r);
                    finished.push(id);
                }
                Ok(r) if r.get("running").and_then(Value::as_bool) == Some(true) => {}
                // no_such_job / evicted / restarted daemon: stop tracking it.
                Ok(_) => finished.push(id),
                // Transient failure (daemon momentarily gone): retry next cycle.
                Err(_) => {}
            }
        }
        let mut pending = lock(&PENDING);
        pending.retain(|id| !finished.contains(id));
        if pending.is_empty() {
            POLLER.store(false, Ordering::SeqCst);
            return;
        }
    }
}

/// Stash a finished job's output and push a completion notice to the editor.
fn stash_and_report(r: &Value) {
    let id = r["id"].as_u64().unwrap_or(0);
    let label = r["label"].as_str().unwrap_or("job").to_string();
    let code = r["code"].as_i64();
    let stdout = decode_text(r, "stdout");
    let stderr = decode_text(r, "stderr");

    {
        let mut results = lock(&RESULTS);
        results.push(CompletedJob {
            id,
            label: label.clone(),
            code,
            stdout,
            stderr: stderr.clone(),
        });
        let overflow = results.len().saturating_sub(MAX_RESULTS);
        if overflow > 0 {
            results.drain(0..overflow);
        }
    }

    let msg = match code {
        Some(0) => format!("zwire-job #{id} done — {label}"),
        Some(c) => {
            let tail = stderr.lines().next().unwrap_or("").trim();
            if tail.is_empty() {
                format!("zwire-job #{id} exited {c} — {label}")
            } else {
                format!("zwire-job #{id} exited {c} — {label}: {tail}")
            }
        }
        None => format!("zwire-job #{id} finished — {label}"),
    };
    // Hop back onto the editor's main loop to set the status line.
    crate::job::dispatch_blocking(move |editor, _compositor| {
        editor.set_status(msg);
    });
}

/// One-line overview: jobs still running on the daemon plus recent completions.
pub fn jobs_overview() -> Result<String, String> {
    let reply = call(&json!({ "cmd": "job_list" }))?;
    let running: Vec<String> = reply["jobs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|j| j["running"].as_bool() == Some(true))
                .map(|j| format!("#{} {}", j["id"], j["label"].as_str().unwrap_or("")))
                .collect()
        })
        .unwrap_or_default();
    let recent: Vec<String> = {
        let results = lock(&RESULTS);
        results
            .iter()
            .rev()
            .take(5)
            .map(|c| {
                let code = c.code.map(|x| x.to_string()).unwrap_or_else(|| "?".into());
                format!("#{} {}({code})", c.id, c.label)
            })
            .collect()
    };
    Ok(format!(
        "running: [{}]  recent: [{}]",
        running.join(", "),
        recent.join(", ")
    ))
}

/// Retrieve the stored output (stdout then stderr) of a finished job — the given
/// `id`, or the most recent completion when `id` is `None`.
pub fn take_output(id: Option<u64>) -> Option<String> {
    let results = lock(&RESULTS);
    let job = match id {
        Some(id) => results.iter().find(|c| c.id == id)?,
        None => results.last()?,
    };
    let mut out = job.stdout.clone();
    if !job.stderr.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&job.stderr);
    }
    Some(out)
}
