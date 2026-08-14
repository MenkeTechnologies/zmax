//! The process-driving half of six spacemacs `+lang` layers — `alda`,
//! `extempore`, `factor`, `mercury`, `octave` and `windows-scripts` — plus a
//! one-line note for the two layers (`jr`, `kivy`) that ship a major mode and
//! nothing else.
//!
//! Every command here really runs the program its emacs counterpart runs. What
//! changed is the *shape*: emacs keeps a live inferior process (comint buffer,
//! network process, compilation buffer) per layer, while a zmax `:` command is
//! one shot. Where that difference is observable it is stated in the section
//! below rather than papered over.
//!
//! * **alda** — `alda-mode.el`'s `alda-run-cmd` first shells out to `alda
//!   status`, and when the output matches `[Ss]erver [Dd]own` it starts `alda
//!   server`, sleeps 2 s and runs the command anyway. [`ensure_server`] does the
//!   same check and start, but reports the restart instead of racing the
//!   command behind it — the caller re-runs. Playback is client-side (`alda
//!   play` hands the score to the server), so the play commands run
//!   synchronously and page whatever alda said.
//! * **extempore** — `extempore-mode.el` opens a raw TCP socket to a
//!   *separately started* Extempore process (`extempore-default-host`
//!   "localhost", `extempore-default-port` 7099) and `process-send-string`s each
//!   form terminated with `\r\n`. Ported literally with [`std::net::TcpStream`].
//!   The difference: emacs keeps the socket open and streams replies into a
//!   buffer; here each send opens a connection, writes the form, reads for
//!   250 ms and closes. Output the server prints later than that is missed.
//! * **factor** — FUEL boots the Factor VM with a remote listener and then talks
//!   to it over that wire for everything else. A one-shot command cannot hold
//!   that session, so only the parts the `factor` binary can do alone are here:
//!   run a file, evaluate source, start the listener, list a vocab's words.
//!   **Absent, because they need the live FUEL connection:** every refactoring
//!   (`fuel-refactor-extract-sexp` / `-region` / `-vocab`, `fuel-refactor-
//!   inline-word`, `-rename-word`, `-extract-article`, `-make-generic`,
//!   `fuel-update-usings`), `fuel-help`, `fuel-apropos`, `fuel-show-callers`,
//!   `fuel-show-callees`, `fuel-edit-word-at-point`, `fuel-stack-effect`,
//!   `fuel-test-vocab`, and the scaffolding commands (`fuel-scaffold-vocab`,
//!   `fuel-scaffold-help`). [`factor_vocab_words`] is the honest stand-in for
//!   `fuel-show-file-words` only.
//! * **mercury** — `metal-mercury-mode` derives the module name from the file
//!   name, `compile`s `mmc --make <module>`, and the runner then
//!   `shell-command`s `./<module>`. Same two steps here, run in the file's own
//!   directory because that is where `mmc` drops the executable.
//! * **octave** — `octave.el` runs `octave` as a comint process and pushes
//!   buffer/defun/line/region into it; `octave-help` and `octave-lookfor` ask
//!   that live process. These commands use octave's batch mode
//!   (`octave --no-gui --quiet --eval …`) instead. **That is a real behavioural
//!   difference:** each call is a fresh interpreter, so variables, `function`
//!   definitions, loaded packages and `pkg load` state do *not* survive from one
//!   `:octave-eval` to the next the way they do in an inferior-octave buffer.
//!   Anything stateful has to be sent as one snippet.
//! * **windows-scripts** — `bat-mode`'s `bat-run` / `bat-run-args` /
//!   `bat-cmd-help` / `bat-template`, the navigable half of `bmx-mode`, and
//!   `powershell.el`'s runner plus its `powershell-regexp-to-regex` command.
//!   Batch commands need Windows or `wine`; that is said in the error rather
//!   than silently doing nothing.
//! * **jr / kivy** — these two layers register a major mode (`M-x jr-mode`,
//!   `M-x kivy-mode`) and define no commands at all, so there is nothing to
//!   drive. [`mode_note`] is the whole port: the dispatcher sets the buffer's
//!   language and says so.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use crate::sm::{self, Outcome};

/* ── shared plumbing ────────────────────────────────────────────────────── */

/// A required positional argument.
fn need<'a>(args: &[&'a str], i: usize, what: &str) -> Result<&'a str, String> {
    match args.get(i) {
        Some(s) if !s.trim().is_empty() => Ok(s),
        _ => Err(format!("expected {what}")),
    }
}

/// An optional positional argument, empty when absent.
fn opt<'a>(args: &[&'a str], i: usize) -> &'a str {
    args.get(i).copied().unwrap_or("")
}

fn spawn_error(program: &Path, e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        format!("`{}` not found on PATH", program.display())
    } else {
        format!("{}: {e}", program.display())
    }
}

/// Run `program` and capture stdout and stderr *together*, the way a
/// compilation buffer shows them. `Err` is only a failure to spawn; a non-zero
/// exit comes back as `Ok((false, output))` so the caller can still page the
/// diagnostics, which for a compiler is the entire point.
fn run_capture(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<(bool, String), String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd
        .output()
        .map_err(|e| spawn_error(Path::new(program), e))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&err);
    }
    Ok((out.status.success(), text))
}

/// Start `program` with its stdio on `/dev/null` and do not wait for it — the
/// port of elisp `start-process` for the three servers (alda, extempore, the
/// FUEL listener) that are supposed to outlive the command.
fn spawn_detached(program: &Path, args: &[&str], cwd: Option<&Path>) -> Result<u32, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.spawn()
        .map(|child| child.id())
        .map_err(|e| spawn_error(program, e))
}

/// Wrap captured output as a page, with a status line that says whether the
/// program succeeded.
fn paged(title: &str, ok: bool, text: &str) -> Outcome {
    let body = if text.trim().is_empty() {
        "(no output)\n".to_string()
    } else {
        text.to_string()
    };
    let status = if ok {
        title.to_string()
    } else {
        format!("{title}: exited non-zero")
    };
    Outcome::page(status, format!("{}{body}", sm::heading(title)))
}

/// The directory a file lives in, for commands that must run beside it.
fn parent_dir(path: &str) -> PathBuf {
    Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/* ── alda ───────────────────────────────────────────────────────────────── */

/// `alda-run-cmd`'s `(string-match "[Ss]erver [Dd]own" …)`, without a regex
/// engine: exactly that pattern, both letters case-flexible, everything else
/// literal.
fn server_down(status: &str) -> bool {
    status.as_bytes().windows(11).any(|w| {
        (w[0] == b'S' || w[0] == b's')
            && &w[1..6] == b"erver"
            && w[6] == b' '
            && (w[7] == b'D' || w[7] == b'd')
            && &w[8..11] == b"own"
    })
}

/// `alda status`, and `alda server` when it says the server is down.
///
/// Returns `Ok(Some(msg))` when the server had to be started — the caller
/// should show `msg` and let the user re-run, which is the same two-step the
/// emacs command produces after its 2 s `sleep-for`. `Ok(None)` means the
/// server was already up and the command can proceed.
fn ensure_server() -> Result<Option<String>, String> {
    if !sm::have("alda") {
        return Err("`alda` not found on PATH".into());
    }
    let (_, status) = run_capture("alda", &["status"], None)?;
    if !server_down(&status) {
        return Ok(None);
    }
    let pid = spawn_detached(Path::new("alda"), &["server"], None)?;
    // alda-mode sleeps 2 s here "to stop a race condition"; same wait, so the
    // user's re-run lands on a server that has finished binding its port.
    std::thread::sleep(Duration::from_secs(2));
    Ok(Some(format!(
        "alda server was down — started it (pid {pid}); re-run the command"
    )))
}

/// `alda-play-file`: play a whole score file.
///
/// `args[0]` — path to the `.alda` file. Runs `alda play --file <path>`.
pub fn alda_play_file(args: &[&str]) -> Result<Outcome, String> {
    let path = need(args, 0, "a path to an .alda file")?;
    if let Some(msg) = ensure_server()? {
        return Ok(Outcome::status(msg));
    }
    let (ok, out) = run_capture("alda", &["play", "--file", path], None)?;
    Ok(paged(&format!("alda play --file {path}"), ok, &out))
}

/// `alda-play-text`: play a region/block/line with the earlier part of the
/// buffer supplied as history, so part and octave declarations above the played
/// text still apply.
///
/// `args[0]` — the history text (everything before the played region; may be
/// empty). `args[1]` — the code to play. Runs
/// `alda play --history <history> --code <code>`.
pub fn alda_play_code(args: &[&str]) -> Result<Outcome, String> {
    let history = opt(args, 0);
    let code = need(args, 1, "alda code to play")?;
    if let Some(msg) = ensure_server()? {
        return Ok(Outcome::status(msg));
    }
    let (ok, out) = run_capture(
        "alda",
        &["play", "--history", history, "--code", code],
        None,
    )?;
    Ok(paged("alda play --code", ok, &out))
}

/// `alda status`, paged. Takes no arguments.
pub fn alda_server_status(_args: &[&str]) -> Result<Outcome, String> {
    let (ok, out) = run_capture("alda", &["status"], None)?;
    Ok(paged("alda status", ok, &out))
}

/// `alda-server`: start `alda server` detached. Takes no arguments.
pub fn alda_server_start(_args: &[&str]) -> Result<Outcome, String> {
    let pid = spawn_detached(Path::new("alda"), &["server"], None)?;
    Ok(Outcome::status(format!(
        "alda server starting (pid {pid}) — it takes a moment to accept plays"
    )))
}

/* ── extempore ──────────────────────────────────────────────────────────── */

/// `extempore-default-host`.
const EXTEMPORE_HOST: &str = "localhost";
/// `extempore-default-port`.
const EXTEMPORE_PORT: u16 = 7099;
/// How long to wait for the Extempore process to accept the connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long to wait for a reply before giving the socket back.
const READ_TIMEOUT: Duration = Duration::from_millis(250);

/// The endpoint `:extempore-connect` last verified, so later sends do not have
/// to repeat the host/port. Emacs keeps `extempore-connection-list` for the same
/// reason.
static ENDPOINT: Mutex<Option<(String, u16)>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Host/port from the arguments, else the environment, else the emacs defaults.
fn extempore_endpoint(args: &[&str]) -> Result<(String, u16), String> {
    let host = match args.first() {
        Some(h) if !h.trim().is_empty() => h.trim().to_string(),
        _ => std::env::var("EXTEMPORE_HOST").unwrap_or_else(|_| EXTEMPORE_HOST.to_string()),
    };
    let port = match args.get(1) {
        Some(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => std::env::var("EXTEMPORE_PORT").unwrap_or_else(|_| EXTEMPORE_PORT.to_string()),
    };
    let port: u16 = port
        .parse()
        .map_err(|_| format!("extempore: `{port}` is not a port number"))?;
    Ok((host, port))
}

/// Connect, write one form terminated with `\r\n` (exactly what
/// `extempore-send-region` sends), read whatever comes back inside
/// [`READ_TIMEOUT`], and close.
fn send_form(host: &str, port: u16, code: &str) -> Result<String, String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("extempore {host}:{port}: {e}"))?
        .next()
        .ok_or_else(|| format!("extempore {host}:{port}: name resolved to no address"))?;
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| {
        format!("extempore {host}:{port}: {e} — is an Extempore process running there?")
    })?;
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|e| format!("extempore {host}:{port}: {e}"))?;
    if !code.is_empty() {
        stream
            .write_all(code.as_bytes())
            .and_then(|()| stream.write_all(b"\r\n"))
            .and_then(|()| stream.flush())
            .map_err(|e| format!("extempore {host}:{port}: {e}"))?;
    }

    let mut reply = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => reply.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break
            }
            Err(e) => return Err(format!("extempore {host}:{port}: {e}")),
        }
    }
    Ok(String::from_utf8_lossy(&reply).into_owned())
}

/// `extempore-connect`: verify an Extempore process is listening and remember
/// the endpoint for later sends.
///
/// `args[0]` — host (optional, default `$EXTEMPORE_HOST` else `localhost`).
/// `args[1]` — port (optional, default `$EXTEMPORE_PORT` else `7099`).
pub fn extempore_connect(args: &[&str]) -> Result<Outcome, String> {
    let (host, port) = extempore_endpoint(args)?;
    // An empty form: connects and reads the banner without evaluating anything.
    let banner = send_form(&host, port, "")?;
    *lock(&ENDPOINT) = Some((host.clone(), port));
    let status = format!("extempore: connected to {host}:{port}");
    if banner.trim().is_empty() {
        Ok(Outcome::status(status))
    } else {
        Ok(Outcome::page(
            status,
            format!(
                "{}{banner}",
                sm::heading(&format!("extempore {host}:{port}"))
            ),
        ))
    }
}

/// `extempore-send-region` / `-definition` / `-buffer`: send one chunk of code
/// to the connected Extempore process and page its reply.
///
/// `args[0]` — the code to evaluate. Uses the endpoint stored by
/// [`extempore_connect`], or the defaults when nothing was stored.
pub fn extempore_send(args: &[&str]) -> Result<Outcome, String> {
    let code = need(args, 0, "extempore code to send")?;
    let (host, port) = match lock(&ENDPOINT).clone() {
        Some(ep) => ep,
        None => extempore_endpoint(&[])?,
    };
    let reply = send_form(&host, port, code)?;
    let title = format!("extempore {host}:{port}");
    if reply.trim().is_empty() {
        Ok(Outcome::status(format!(
            "{title}: sent, no reply within 250ms"
        )))
    } else {
        Ok(Outcome::page(
            title.clone(),
            format!("{}{reply}", sm::heading(&title)),
        ))
    }
}

/// Forget the stored endpoint. Takes no arguments.
pub fn extempore_disconnect(_args: &[&str]) -> Result<Outcome, String> {
    match lock(&ENDPOINT).take() {
        Some((host, port)) => Ok(Outcome::status(format!(
            "extempore: disconnected from {host}:{port}"
        ))),
        None => Ok(Outcome::status("extempore: not connected")),
    }
}

/// `extempore-run`: start the Extempore process itself, detached.
///
/// `args[..]` — extra program arguments (`extempore-program-args`), all
/// optional. The binary is looked up on `PATH` first and then under
/// `$EXTEMPORE_PATH` (emacs' `extempore-path`), and it is started **in its own
/// directory** because Extempore loads its runtime relative to the install
/// root.
pub fn extempore_run(args: &[&str]) -> Result<Outcome, String> {
    let binary = sm::which("extempore")
        .or_else(|| {
            let root = std::env::var("EXTEMPORE_PATH").ok()?;
            let candidate = Path::new(&root).join("extempore");
            candidate.is_file().then_some(candidate)
        })
        .ok_or_else(|| {
            if std::env::var_os("EXTEMPORE_PATH").is_some() {
                "extempore: not on PATH and not found under $EXTEMPORE_PATH".to_string()
            } else {
                "extempore: not on PATH — set $EXTEMPORE_PATH to the Extempore install root"
                    .to_string()
            }
        })?;
    let run_dir = binary
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let pid = spawn_detached(&binary, args, Some(&run_dir))?;
    Ok(Outcome::status(format!(
        "extempore running (pid {pid}) in {} — connect with :extempore-connect",
        run_dir.display()
    )))
}

/* ── factor ─────────────────────────────────────────────────────────────── */

/// Reject a name that would break out of the Factor string literal the eval
/// commands build.
fn factor_word(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.contains('"') || name.contains('\n') {
        return Err(format!("factor: `{name}` is not a usable vocab name"));
    }
    Ok(name)
}

/// `fuel-run-file`: run a Factor source file.
///
/// `args[0]` — path to the `.factor` file. Runs `factor <path>`.
pub fn factor_run_file(args: &[&str]) -> Result<Outcome, String> {
    let path = need(args, 0, "a path to a .factor file")?;
    let (ok, out) = run_capture("factor", &[path], None)?;
    Ok(paged(&format!("factor {path}"), ok, &out))
}

/// `fuel-eval-region` / `fuel-eval-definition`, as far as a one-shot VM can go.
///
/// `args[0]` — Factor source. Runs `factor -e=<source>`.
pub fn factor_eval(args: &[&str]) -> Result<Outcome, String> {
    let source = need(args, 0, "factor source to evaluate")?;
    let (ok, out) = run_capture("factor", &[&format!("-e={source}")], None)?;
    Ok(paged("factor -e", ok, &out))
}

/// `run-factor`: start the UI listener with the FUEL remote listener running
/// inside it, detached, exactly as the layer boots it.
///
/// Takes no arguments. Uses `-image=$FACTOR_IMAGE` when that is set and lets
/// Factor find its own image otherwise.
pub fn factor_listener(_args: &[&str]) -> Result<Outcome, String> {
    const BOOT: &str =
        "-e=USING: fuel.remote vocabs.loader ; fuel-start-remote-listener* \"ui.tools\" run";
    let image = std::env::var("FACTOR_IMAGE").ok().filter(|s| !s.is_empty());
    let image_arg = image.as_ref().map(|i| format!("-image={i}"));
    let mut argv: Vec<&str> = Vec::new();
    if let Some(a) = image_arg.as_deref() {
        argv.push(a);
    }
    argv.push(BOOT);
    let pid = spawn_detached(Path::new("factor"), &argv, None)?;
    Ok(Outcome::status(format!(
        "factor listener starting (pid {pid}){}",
        image
            .map(|i| format!(" with image {i}"))
            .unwrap_or_default()
    )))
}

/// The honest stand-in for `fuel-show-file-words`: list the words a vocab
/// defines, by loading it in a throwaway VM instead of asking a live listener.
///
/// `args[0]` — the vocab name.
pub fn factor_vocab_words(args: &[&str]) -> Result<Outcome, String> {
    let vocab = factor_word(need(args, 0, "a factor vocab name")?)?;
    let program = format!(
        "-e=USING: vocabs vocabs.loader prettyprint sequences ; \
         \"{vocab}\" require \"{vocab}\" vocab-words [ name>> print ] each"
    );
    let (ok, out) = run_capture("factor", &[&program], None)?;
    Ok(paged(&format!("factor words in {vocab}"), ok, &out))
}

/* ── mercury ────────────────────────────────────────────────────────────── */

/// `metal-mercury-mode`'s module name: the emacs code is
/// `(replace-regexp-in-string ".*\\/\\(.*?\\)\\..*" "\\1" (buffer-file-name))` —
/// a greedy run up to the last `/`, then a *non-greedy* capture up to the first
/// `.` after it. So `/tmp/hello.world.m` is module `hello`, not `hello.world`.
fn mercury_module(path: &str) -> Result<String, String> {
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("mercury: no file name in `{path}`"))?;
    let stem = name.split('.').next().unwrap_or("");
    if stem.is_empty() {
        return Err(format!(
            "mercury: cannot derive a module name from `{path}`"
        ));
    }
    Ok(stem.to_string())
}

/// Run `mmc --make <module>` beside the file. Returns the module name, the
/// directory it was built in, and the compiler's combined output.
fn mercury_build(path: &str) -> Result<(String, PathBuf, bool, String), String> {
    let module = mercury_module(path)?;
    let dir = parent_dir(path);
    let (ok, out) = run_capture("mmc", &["--make", &module], Some(&dir))?;
    Ok((module, dir, ok, out))
}

/// `metal-mercury-mode-compile`: build the file's module with `mmc --make`.
///
/// `args[0]` — path to the `.m` file. The compiler runs in the file's own
/// directory; its output is paged whether or not it succeeded.
pub fn mercury_compile(args: &[&str]) -> Result<Outcome, String> {
    let path = need(args, 0, "a path to a mercury (.m) file")?;
    let (module, _, ok, out) = mercury_build(path)?;
    Ok(paged(&format!("mmc --make {module}"), ok, &out))
}

/// `metal-mercury-mode-runner`: build with `mmc --make`, then run `./<module>`
/// in the same directory.
///
/// `args[0]` — path to the `.m` file. A compile failure is returned as the
/// error, carrying the compiler output.
pub fn mercury_run(args: &[&str]) -> Result<Outcome, String> {
    let path = need(args, 0, "a path to a mercury (.m) file")?;
    let (module, dir, ok, out) = mercury_build(path)?;
    if !ok {
        return Err(format!("mmc --make {module} failed:\n{}", out.trim_end()));
    }
    let exe = dir.join(&module);
    let program = exe.to_string_lossy().into_owned();
    let (ran, output) = run_capture(&program, &[], Some(&dir))?;
    Ok(paged(&format!("./{module}"), ran, &output))
}

/* ── octave ─────────────────────────────────────────────────────────────── */

/// The flags that turn `octave` into a batch evaluator: no GUI, no banner.
const OCTAVE_FLAGS: [&str; 2] = ["--no-gui", "--quiet"];

fn octave_eval_args(code: &str) -> [&str; 4] {
    [OCTAVE_FLAGS[0], OCTAVE_FLAGS[1], "--eval", code]
}

/// `octave-send-region` / `-block` / `-line` / `-buffer`, as a batch call.
///
/// `args[0]` — the octave code. Runs
/// `octave --no-gui --quiet --eval <code>`. Nothing survives between calls; see
/// the module docs.
pub fn octave_eval(args: &[&str]) -> Result<Outcome, String> {
    let code = need(args, 0, "octave code to evaluate")?;
    let (ok, out) = run_capture("octave", &octave_eval_args(code), None)?;
    Ok(paged("octave --eval", ok, &out))
}

/// Run a whole `.m` script: `octave --no-gui --quiet <path>`.
///
/// `args[0]` — path to the script.
pub fn octave_run_file(args: &[&str]) -> Result<Outcome, String> {
    let path = need(args, 0, "a path to an octave script")?;
    let (ok, out) = run_capture("octave", &[OCTAVE_FLAGS[0], OCTAVE_FLAGS[1], path], None)?;
    Ok(paged(&format!("octave {path}"), ok, &out))
}

/// `octave-help`: `help <name>` in a batch interpreter.
///
/// `args[0]` — the function or operator name.
pub fn octave_help(args: &[&str]) -> Result<Outcome, String> {
    let name = need(args, 0, "an octave function name")?;
    let code = format!("help {name}");
    let (ok, out) = run_capture("octave", &octave_eval_args(&code), None)?;
    Ok(paged(&format!("octave help {name}"), ok, &out))
}

/// `octave-lookfor`: `lookfor <keyword>` in a batch interpreter.
///
/// `args[0]` — the keyword to search help texts for.
pub fn octave_lookfor(args: &[&str]) -> Result<Outcome, String> {
    let keyword = need(args, 0, "a keyword to look for")?;
    let code = format!("lookfor {keyword}");
    let (ok, out) = run_capture("octave", &octave_eval_args(&code), None)?;
    Ok(paged(&format!("octave lookfor {keyword}"), ok, &out))
}

/* ── windows-scripts: batch ─────────────────────────────────────────────── */

/// How a batch command line can be run on this machine. `cmd` on Windows, wine
/// elsewhere, and an error when neither exists — `bat-run` on a unix box is a
/// silent no-op in emacs, which is worse than saying so.
fn batch_runner() -> Result<(&'static str, &'static [&'static str]), String> {
    if cfg!(windows) {
        // Windows cannot exec a .bat directly (CreateProcess refuses it), so the
        // interpreter is always named explicitly, which is also what emacs'
        // `shell-command` ends up doing through `cmdproxy`.
        return Ok(("cmd", &["/c"]));
    }
    if sm::have("wine") {
        return Ok(("wine", &["cmd", "/c"]));
    }
    Err("running a batch file needs Windows, or wine on this machine".into())
}

/// `bat-run` / `bat-run-args`: run a batch file.
///
/// `args[0]` — path to the `.bat`/`.cmd` file. `args[1..]` — arguments passed
/// to it. Runs it through `cmd /c` on Windows and `wine cmd /c` elsewhere.
pub fn bat_run(args: &[&str]) -> Result<Outcome, String> {
    let path = need(args, 0, "a path to a .bat or .cmd file")?;
    let (program, prefix) = batch_runner()?;
    let mut argv: Vec<&str> = prefix.to_vec();
    argv.push(path);
    argv.extend_from_slice(&args[1..]);
    let (ok, out) = run_capture(program, &argv, Some(&parent_dir(path)))?;
    Ok(paged(&format!("bat-run {path}"), ok, &out))
}

/// `bat-cmd-help`: the shell's own help for a batch command — `net /?` for
/// `net`, `help <cmd>` for everything else, exactly as bat-mode branches.
///
/// `args[0]` — the command name.
pub fn bat_cmd_help(args: &[&str]) -> Result<Outcome, String> {
    let cmd = need(args, 0, "a batch command name")?;
    let (program, prefix) = batch_runner()?;
    let mut argv: Vec<&str> = prefix.to_vec();
    if cmd.eq_ignore_ascii_case("net") {
        argv.extend_from_slice(&["net", "/?"]);
    } else {
        argv.extend_from_slice(&["help", cmd]);
    }
    let (ok, out) = run_capture(program, &argv, None)?;
    Ok(paged(&format!("bat help {cmd}"), ok, &out))
}

/// Exactly what `bat-template` inserts at point-min:
/// `(insert "@echo off\nsetlocal\n\n")`. No `rem` header and no `endlocal` —
/// upstream's template really is these two lines and a blank one
/// (emacs `lisp/progmodes/bat-mode.el`, `bat-template`).
const BAT_TEMPLATE: &str = "@echo off\nsetlocal\n\n";

/// `bat-template`: the minimal batch file template, for the dispatcher to
/// insert. Pure; takes no arguments. The template text is the page.
pub fn bat_template(_args: &[&str]) -> Result<Outcome, String> {
    Ok(Outcome::page("bat template", BAT_TEMPLATE))
}

/// One batch label: where it is defined and where it is jumped to.
#[derive(Debug, PartialEq, Eq)]
struct Label {
    /// The spelling first seen in the file (batch labels are case-insensitive).
    name: String,
    /// 1-based lines carrying `:name`.
    defs: Vec<usize>,
    /// 1-based lines carrying `goto name` / `call :name`.
    refs: Vec<usize>,
}

/// The label references on one line: `goto x`, `goto:x`, `call :x`, `call:x`.
/// `call x` without a colon calls another *file*, not a label, so it is not a
/// reference — that is the same distinction bmx-mode draws.
fn line_refs(line: &str) -> Vec<String> {
    let toks: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '&' | '|'))
        .filter(|t| !t.is_empty())
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let tok = toks[i];
        let lower = tok.to_ascii_lowercase();
        if lower == "goto" || lower == "call" {
            if let Some(next) = toks.get(i + 1) {
                let name = next.trim_start_matches(':');
                if (lower == "goto" || next.starts_with(':')) && !name.is_empty() {
                    out.push(name.to_string());
                }
            }
            i += 2;
            continue;
        }
        if (lower.starts_with("goto:") || lower.starts_with("call:")) && tok.len() > 5 {
            out.push(tok[5..].to_string());
        }
        i += 1;
    }
    out
}

/// Every label in a batch script, in order of first appearance, with the lines
/// that define it and the lines that jump to it. `::` comment lines and `rem`
/// lines are skipped so a commented-out `goto` is not counted.
fn scan_labels(text: &str) -> Vec<Label> {
    let mut labels: Vec<Label> = Vec::new();
    let mut index: Vec<String> = Vec::new(); // lowercase keys, parallel to labels

    let record = |key: String,
                  spelling: &str,
                  line: usize,
                  is_def: bool,
                  labels: &mut Vec<Label>,
                  index: &mut Vec<String>| {
        let pos = match index.iter().position(|k| *k == key) {
            Some(p) => p,
            None => {
                index.push(key);
                labels.push(Label {
                    name: spelling.to_string(),
                    defs: Vec::new(),
                    refs: Vec::new(),
                });
                labels.len() - 1
            }
        };
        if is_def {
            labels[pos].defs.push(line);
        } else {
            labels[pos].refs.push(line);
        }
    };

    for (n, raw) in text.lines().enumerate() {
        let line = n + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with("::") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(':') {
            let name: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && !matches!(c, '+' | '=' | ',' | ';'))
                .collect();
            if !name.is_empty() {
                record(
                    name.to_ascii_lowercase(),
                    &name,
                    line,
                    true,
                    &mut labels,
                    &mut index,
                );
                continue;
            }
        }
        let first = trimmed.split_whitespace().next().unwrap_or("");
        if first.eq_ignore_ascii_case("rem") {
            continue;
        }
        for name in line_refs(trimmed) {
            record(
                name.to_ascii_lowercase(),
                &name,
                line,
                false,
                &mut labels,
                &mut index,
            );
        }
    }
    labels
}

fn join_lines(lines: &[usize]) -> String {
    lines
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The navigable half of `bmx-mode` (`bmx-navigate-to-symbol-at-point`,
/// `bmx-find-references-at-point`): a table of every `:label` in the script,
/// the line it is defined on, and how many `goto`/`call` references reach it.
/// References to labels the script never defines are listed separately.
///
/// `args[0]` — the buffer's **text** (not a path). Pure.
pub fn bat_labels(args: &[&str]) -> Result<Outcome, String> {
    let text = need(args, 0, "the batch file's text")?;
    let labels = scan_labels(text);
    let (defined, dangling): (Vec<&Label>, Vec<&Label>) =
        labels.iter().partition(|l| !l.defs.is_empty());

    let width = defined
        .iter()
        .map(|l| l.name.chars().count() + 1)
        .chain(std::iter::once(5))
        .max()
        .unwrap_or(5);

    let mut page = sm::heading("Batch labels");
    if defined.is_empty() {
        page.push_str("no labels defined\n");
    } else {
        page.push_str(&format!(
            "{:<width$}  {:>5}  {}\n",
            "label", "refs", "defined on",
        ));
        for l in &defined {
            page.push_str(&format!(
                "{:<width$}  {:>5}  {}\n",
                format!(":{}", l.name),
                l.refs.len(),
                join_lines(&l.defs),
            ));
        }
    }
    if !dangling.is_empty() {
        page.push_str("\nreferences to labels this file does not define:\n");
        for l in &dangling {
            page.push_str(&format!("  :{}  lines {}\n", l.name, join_lines(&l.refs)));
        }
    }
    Ok(Outcome::page(
        format!(
            "{} label{} ({} dangling reference{})",
            defined.len(),
            if defined.len() == 1 { "" } else { "s" },
            dangling.len(),
            if dangling.len() == 1 { "" } else { "s" },
        ),
        page,
    ))
}

/* ── windows-scripts: powershell ────────────────────────────────────────── */

/// `pwsh` when present, else Windows PowerShell.
fn powershell_binary() -> Result<&'static str, String> {
    if sm::have("pwsh") {
        return Ok("pwsh");
    }
    if sm::have("powershell") {
        return Ok("powershell");
    }
    Err("neither `pwsh` nor `powershell` is on PATH".into())
}

/// Run a PowerShell script file.
///
/// `args[0]` — path to the `.ps1`. `args[1..]` — arguments for the script. Runs
/// `pwsh -NoLogo -NoProfile -File <path> [args]`, falling back to `powershell`.
pub fn powershell_run(args: &[&str]) -> Result<Outcome, String> {
    let path = need(args, 0, "a path to a .ps1 file")?;
    let program = powershell_binary()?;
    let mut argv = vec!["-NoLogo", "-NoProfile", "-File", path];
    argv.extend_from_slice(&args[1..]);
    let (ok, out) = run_capture(program, &argv, Some(&parent_dir(path)))?;
    Ok(paged(&format!("{program} {path}"), ok, &out))
}

/// Evaluate PowerShell source.
///
/// `args[0]` — the source. Runs `pwsh -NoLogo -NoProfile -Command <src>`.
pub fn powershell_eval(args: &[&str]) -> Result<Outcome, String> {
    let source = need(args, 0, "powershell source to evaluate")?;
    let program = powershell_binary()?;
    let (ok, out) = run_capture(
        program,
        &["-NoLogo", "-NoProfile", "-Command", source],
        None,
    )?;
    Ok(paged(&format!("{program} -Command"), ok, &out))
}

/// `powershell-regexp-to-regex`, ported from the elisp body: three sequential
/// passes over the text replacing `\(` with `(`, then `\)` with `)`, then `\|`
/// with `|` — the escapes emacs' `regexp-opt` emits that .NET regexes do not
/// use. Sequential `str::replace` matches elisp's `while (re-search-forward …)`
/// exactly, because both scan left to right over non-overlapping matches.
///
/// Upstream does *not* touch `\{`, `\}`, `\_<` or `\_>`; those are left alone
/// here for the same reason.
fn regexp_to_regex(text: &str) -> String {
    text.replace("\\(", "(")
        .replace("\\)", ")")
        .replace("\\|", "|")
}

/// `powershell-regexp-to-regex`: rewrite an emacs regexp (typically
/// `regexp-opt` output) as a .NET/PowerShell regex.
///
/// `args[0]` — the region's text. Pure; the rewritten text is the page, for the
/// dispatcher to put back in place of the region.
pub fn powershell_regexp_to_regex(args: &[&str]) -> Result<Outcome, String> {
    let text = need(args, 0, "the region's text")?;
    let converted = regexp_to_regex(text);
    Ok(Outcome::page(
        format!("regexp-to-regex: {} chars", converted.chars().count()),
        converted,
    ))
}

/* ── jr / kivy ──────────────────────────────────────────────────────────── */

/// The whole port of the `jr` and `kivy` layers. Both define a major mode
/// (`M-x jr-mode`, `M-x kivy-mode`) and no commands whatsoever — no runner, no
/// repl, no lookup — so once the dispatcher has set the buffer's language there
/// is nothing left to do but say so.
pub fn mode_note(language: &str) -> String {
    format!("{language}: buffer language set to {language}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alda_server_down_matches_the_elisp_pattern() {
        // "[Ss]erver [Dd]own" — only those two letters vary.
        assert!(server_down("Server Down"));
        assert!(server_down("server down"));
        assert!(server_down("Server down"));
        assert!(server_down("server Down"));
        assert!(server_down("alda: nrepl: Server Down (port 27713)"));
        assert!(!server_down("SERVER DOWN"));
        assert!(!server_down("Server  Down"));
        assert!(!server_down("Server Up"));
        assert!(!server_down("server"));
        assert!(!server_down(""));
    }

    #[test]
    fn mercury_module_is_the_name_up_to_the_first_dot() {
        assert_eq!(mercury_module("/tmp/hello.m").unwrap(), "hello");
        assert_eq!(mercury_module("hello.m").unwrap(), "hello");
        // non-greedy capture: stops at the FIRST dot, not the last
        assert_eq!(mercury_module("/tmp/hello.world.m").unwrap(), "hello");
        assert_eq!(mercury_module("/a/b/c/queens.m").unwrap(), "queens");
        assert!(mercury_module("/tmp/.m").is_err());
        assert!(mercury_module("/").is_err());
    }

    #[test]
    fn bat_template_is_upstreams_exact_insertion() {
        let out = bat_template(&[]).unwrap();
        assert_eq!(out.page.as_deref(), Some("@echo off\nsetlocal\n\n"));
    }

    #[test]
    fn powershell_regexp_to_regex_drops_emacs_group_escapes() {
        // regexp-opt output, the documented input.
        assert_eq!(regexp_to_regex("\\(?:foo\\|bar\\)"), "(?:foo|bar)");
        assert_eq!(regexp_to_regex("\\(a\\|b\\|c\\)"), "(a|b|c)");
        // An escaped backslash before a paren keeps one backslash, matching
        // emacs' left-to-right non-overlapping scan.
        assert_eq!(regexp_to_regex("\\\\("), "\\(");
        // Untouched by upstream: braces and symbol boundaries.
        assert_eq!(regexp_to_regex("\\_<a\\{2\\}\\_>"), "\\_<a\\{2\\}\\_>");
        assert_eq!(regexp_to_regex("[abc]+"), "[abc]+");
        assert_eq!(regexp_to_regex(""), "");
    }

    #[test]
    fn bat_labels_finds_definitions_and_their_references() {
        let script = "@echo off\n\
                      setlocal\n\
                      if \"%1\"==\"\" goto usage\n\
                      call :build\n\
                      goto :eof\n\
                      \n\
                      :build\n\
                      rem goto notreal\n\
                      :: goto alsonotreal\n\
                      echo building\n\
                      goto BUILD\n\
                      \n\
                      :usage\n\
                      echo usage: x.bat name\n";
        let labels = scan_labels(script);

        let build = labels
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case("build"))
            .unwrap();
        assert_eq!(build.defs, vec![7]);
        // `call :build` on line 4 and the case-insensitive `goto BUILD` on 11.
        assert_eq!(build.refs, vec![4, 11]);

        let usage = labels.iter().find(|l| l.name == "usage").unwrap();
        assert_eq!(usage.defs, vec![13]);
        assert_eq!(usage.refs, vec![3]);

        // `rem`-commented and `::`-commented gotos are not references.
        assert!(labels.iter().all(|l| l.name != "notreal"));
        assert!(labels.iter().all(|l| l.name != "alsonotreal"));

        // `goto :eof` is a reference to a label this file never defines.
        let eof = labels.iter().find(|l| l.name == "eof").unwrap();
        assert!(eof.defs.is_empty());
        assert_eq!(eof.refs, vec![5]);

        let out = bat_labels(&[script]).unwrap();
        assert_eq!(out.status, "2 labels (1 dangling reference)");
        let page = out.page.unwrap();
        assert!(page.contains(":build"), "{page}");
        assert!(page.contains(":usage"), "{page}");
        assert!(
            page.contains("references to labels this file does not define:"),
            "{page}"
        );
        assert!(page.contains(":eof  lines 5"), "{page}");
    }

    #[test]
    fn bat_labels_on_a_script_without_labels() {
        let out = bat_labels(&["@echo off\necho hi\n"]).unwrap();
        assert_eq!(out.status, "0 labels (0 dangling references)");
        assert!(out.page.unwrap().contains("no labels defined"));
    }

    #[test]
    fn bat_label_refs_ignore_calls_to_other_files() {
        // `call other.bat` calls a FILE; only `call :label` is a label ref.
        assert!(line_refs("call other.bat").is_empty());
        assert_eq!(line_refs("call :sub"), vec!["sub".to_string()]);
        assert_eq!(line_refs("goto:eof"), vec!["eof".to_string()]);
        assert_eq!(
            line_refs("if errorlevel 1 goto fail"),
            vec!["fail".to_string()]
        );
    }

    #[test]
    fn mode_note_names_the_language_twice() {
        assert_eq!(mode_note("jr"), "jr: buffer language set to jr");
        assert_eq!(mode_note("kivy"), "kivy: buffer language set to kivy");
    }
}
