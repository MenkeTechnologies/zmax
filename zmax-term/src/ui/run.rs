//! The IDE Run tool window's engine: spawn a command, stream its stdout/stderr into shared
//! state that the bottom panel renders live (JetBrains "Run" console). Kill/rerun supported.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Live state of a run, shared between the spawned task and the render loop.
pub struct RunState {
    pub cmd: String,
    pub shell: Vec<String>,
    pub cwd: PathBuf,
    pub lines: Vec<String>,
    /// Display (terminal-cell) width of each entry in `lines`, kept in lockstep.
    /// Computed once on the streaming background task so the render loop never
    /// has to Unicode-width-scan the whole console every frame (that scan, run
    /// on the UI thread over up to `MAX_LINES` lines, is what made a big run
    /// output feel laggy while scrolling / tail-following).
    pub line_widths: Vec<u16>,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub scroll: usize,
    pub follow: bool,
    abort: Option<tokio::task::AbortHandle>,
}

impl RunState {
    /// Append one already-ANSI-stripped output line, keeping `line_widths` in
    /// lockstep and enforcing the `MAX_LINES` ring cap. Centralizing this is what
    /// guarantees the render loop can trust `line_widths[i]` to describe `lines[i]`.
    fn push_line(&mut self, clean: String) {
        self.line_widths.push(line_width(&clean));
        self.lines.push(clean);
        if self.lines.len() > MAX_LINES {
            let drop = self.lines.len() - MAX_LINES;
            self.lines.drain(0..drop);
            self.line_widths.drain(0..drop);
        }
    }
}

pub type Run = Arc<Mutex<RunState>>;

const MAX_LINES: usize = 5000;

/// Terminal-cell width of a console line — the expensive Unicode scan we hoist
/// off the render thread onto the streaming task. Mirrors `ide::disp_width`.
fn line_width(s: &str) -> u16 {
    use zmax_core::unicode::width::UnicodeWidthStr;
    s.width() as u16
}

/// Strip ANSI escape sequences (CSI/OSC) and collapse `\r` overwrites from a
/// captured output line, so colour codes and progress-bar redraws render as
/// clean text in the console instead of garbage.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek() {
                // CSI `ESC [ … final` — final byte is 0x40..=0x7e
                Some('[') => {
                    chars.next();
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                // OSC `ESC ] … (BEL | ESC \)`
                Some(']') => {
                    chars.next();
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                // other two-char escapes (e.g. `ESC ( B`)
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            // carriage return = "redraw this line"; keep only what follows
            '\r' => out.clear(),
            _ => out.push(c),
        }
    }
    out
}

/// Pick a sensible `(command, working_dir)` for the current file: stryke for `.stk`, cargo for a
/// Rust crate, the interpreter for scripts — run from the project root (nearest manifest), not the
/// terminal's cwd. This is what makes ▶ Run "smart about the current file".
pub fn smart_command(path: Option<&Path>) -> (String, PathBuf) {
    let fallback = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(path) = path else {
        return ("cargo run".to_string(), fallback);
    };
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| fallback.clone());
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let p = path.display().to_string();
    match ext {
        "stk" => (
            format!("stryke {p}"),
            find_up(&dir, "stryke.toml").unwrap_or(dir),
        ),
        "rs" => (
            "cargo run".to_string(),
            find_up(&dir, "Cargo.toml").unwrap_or(dir),
        ),
        "py" => (format!("python3 {p}"), dir),
        "go" => ("go run .".to_string(), dir),
        "js" | "mjs" | "cjs" | "ts" => (format!("node {p}"), dir),
        "sh" | "bash" | "zsh" => (format!("bash {p}"), dir),
        "rb" => (format!("ruby {p}"), dir),
        _ => match find_up(&dir, "Cargo.toml") {
            Some(cwd) => ("cargo run".to_string(), cwd),
            None => (format!("\"{p}\""), dir),
        },
    }
}

/// Nearest ancestor directory (inclusive) that contains `marker`.
fn find_up(start: &Path, marker: &str) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(marker).exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// Start `cmd` under `shell` (e.g. `["sh","-c"]`) in `cwd`, streaming output into a fresh `Run`.
pub fn spawn(cmd: String, shell: Vec<String>, cwd: PathBuf) -> Run {
    let state = Arc::new(Mutex::new(RunState {
        cmd: cmd.clone(),
        shell: shell.clone(),
        cwd: cwd.clone(),
        lines: Vec::new(),
        line_widths: Vec::new(),
        running: true,
        exit_code: None,
        scroll: 0,
        follow: true,
        abort: None,
    }));

    let st = state.clone();
    let handle = tokio::spawn(async move {
        let prog = shell.first().cloned().unwrap_or_else(|| "sh".to_string());
        let args: Vec<String> = shell.iter().skip(1).cloned().collect();
        let mut command = Command::new(&prog);
        command
            .args(&args)
            .arg(&cmd)
            .current_dir(&cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let mut s = st.lock().unwrap();
                s.push_line(format!("failed to start: {err}"));
                s.running = false;
                return;
            }
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        if let Some(stdout) = child.stdout.take() {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx.send(line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx.send(line);
                }
            });
        }
        drop(tx);

        let push = |st: &Run, line: String| {
            let clean = strip_ansi(&line);
            st.lock().unwrap().push_line(clean);
        };

        loop {
            tokio::select! {
                line = rx.recv() => match line {
                    Some(line) => push(&st, line),
                    None => break, // both readers done
                },
                status = child.wait() => {
                    while let Ok(line) = rx.try_recv() {
                        push(&st, line);
                    }
                    let mut s = st.lock().unwrap();
                    s.exit_code = status.ok().and_then(|s| s.code());
                    s.running = false;
                    remember(&s);
                    return;
                }
            }
        }
        // streams closed before wait resolved — finish waiting
        let code = child.wait().await.ok().and_then(|s| s.code());
        let mut s = st.lock().unwrap();
        s.exit_code = code;
        s.running = false;
        remember(&s);
    });

    state.lock().unwrap().abort = Some(handle.abort_handle());
    state
}

/// Put a finished run in the test history, if it was a test run. Called at both
/// points where a run ends, so a run that outlives its output streams is
/// remembered the same as one that does not.
fn remember(s: &RunState) {
    crate::test_history::record(
        &s.cmd,
        &s.cwd,
        s.exit_code == Some(0),
        failed_tests(&s.lines),
    );
}

/// Stop a running command (kills the child via kill-on-drop when the task aborts).
pub fn stop(run: &Run) {
    let mut s = run.lock().unwrap();
    if let Some(abort) = s.abort.take() {
        abort.abort();
    }
    s.running = false;
}

/// Re-run the same command, returning a fresh `Run`.
/// The names of the tests a run reported as failed, in the shapes the common
/// runners print. Pure — unit tested.
///
/// * cargo/libtest: `test foo::bar ... FAILED`, and the `failures:` block that
///   follows lists the same names indented.
/// * pytest: `FAILED tests/test_x.py::test_y - AssertionError`.
/// * jest/vitest: `✕ adds two numbers` / `✗ …`, and `● Console` is not a test.
/// * go: `--- FAIL: TestThing (0.00s)`.
///
/// Names are deduplicated and kept in the order they first appeared, because
/// that is the order a re-run reports them in and the order you read.
pub fn failed_tests(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        let name = name.trim();
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    };
    for raw in lines {
        let line = raw.trim();
        // cargo / libtest
        if let Some(rest) = line.strip_prefix("test ") {
            if rest.ends_with("FAILED") {
                if let Some((name, _)) = rest.split_once(" ... ") {
                    push(name);
                    continue;
                }
            }
        }
        // go
        if let Some(rest) = line.strip_prefix("--- FAIL: ") {
            push(rest.split_whitespace().next().unwrap_or(""));
            continue;
        }
        // pytest
        if let Some(rest) = line.strip_prefix("FAILED ") {
            push(rest.split(" - ").next().unwrap_or(rest));
            continue;
        }
        // jest / vitest
        for marker in ["✕ ", "✗ "] {
            if let Some(rest) = line.strip_prefix(marker) {
                // `✕ adds (12 ms)` — the timing is not part of the name.
                let name = rest.rsplit_once(" (").map(|(n, _)| n).unwrap_or(rest);
                push(name);
            }
        }
    }
    out
}

/// The command that re-runs only `failed`, given the command that produced
/// them — JetBrains "Rerun Failed Tests" (`RerunFailedTests`). `None` when the
/// runner is one whose filter syntax we do not know, so the caller can say so
/// rather than running something that means something else. Pure — unit tested.
pub fn rerun_failed_command(cmd: &str, failed: &[String]) -> Option<String> {
    if failed.is_empty() {
        return None;
    }
    let base = cmd.trim();
    // cargo test: the filter is a positional substring, and `--exact` with
    // several `--` filters is not a thing, so one name per run is wrong —
    // libtest takes repeated filters after `--`.
    if base.starts_with("cargo test") || base.starts_with("cargo nextest") {
        let filters = failed.join(" ");
        return Some(if base.contains(" -- ") {
            format!("{base} {filters}")
        } else {
            format!("{base} -- {filters}")
        });
    }
    if base.starts_with("pytest") || base.contains("python -m pytest") {
        // pytest takes each nodeid as its own argument.
        return Some(format!("{base} {}", failed.join(" ")));
    }
    if base.starts_with("go test") {
        // go's filter is one regex alternation.
        return Some(format!("{base} -run '^({})$'", failed.join("|")));
    }
    if base.starts_with("npx jest") || base.starts_with("jest") || base.starts_with("npx vitest") || base.starts_with("vitest") {
        // jest matches test names by regex with -t.
        let escaped: Vec<String> = failed
            .iter()
            .map(|n| n.replace(['(', ')', '[', ']', '.', '+', '*', '?'], "."))
            .collect();
        return Some(format!("{base} -t '{}'", escaped.join("|")));
    }
    None
}

pub fn rerun(run: &Run) -> Run {
    let (cmd, shell, cwd) = {
        let s = run.lock().unwrap();
        (s.cmd.clone(), s.shell.clone(), s.cwd.clone())
    };
    stop(run);
    spawn(cmd, shell, cwd)
}

#[cfg(test)]
mod ansi_tests {
    use super::strip_ansi;

    #[test]
    fn strips_color_and_progress() {
        // SGR color codes removed, text kept
        assert_eq!(strip_ansi("\u{1b}[31merror\u{1b}[0m: boom"), "error: boom");
        // carriage-return progress redraw keeps only the final state
        assert_eq!(strip_ansi("Building 10%\rBuilding 80%"), "Building 80%");
        // OSC (window title) sequence removed
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}done"), "done");
        // plain text untouched
        assert_eq!(strip_ansi("just text"), "just text");
    }
}

#[cfg(test)]
mod push_tests {
    use super::*;

    fn blank_state() -> RunState {
        RunState {
            cmd: String::new(),
            shell: Vec::new(),
            cwd: PathBuf::from("."),
            lines: Vec::new(),
            line_widths: Vec::new(),
            running: true,
            exit_code: None,
            scroll: 0,
            follow: true,
            abort: None,
        }
    }

    #[test]
    fn widths_stay_in_lockstep_and_ring_caps() {
        let mut s = blank_state();
        for i in 0..(MAX_LINES + 50) {
            s.push_line(format!("line {i}"));
        }
        // ring cap holds and the two vecs never desync
        assert_eq!(s.lines.len(), MAX_LINES);
        assert_eq!(s.line_widths.len(), s.lines.len());
        // every cached width still describes its line
        for (l, w) in s.lines.iter().zip(&s.line_widths) {
            assert_eq!(*w, line_width(l));
        }
        // oldest lines were dropped from the front, in lockstep
        assert_eq!(s.lines[0], "line 50");
    }

    #[test]
    fn measures_wide_glyphs() {
        let mut s = blank_state();
        s.push_line("abc".to_string());
        s.push_line("你好".to_string()); // CJK: 2 cells each
        assert_eq!(s.line_widths[0], 3);
        assert_eq!(s.line_widths[1], 4);
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn failed_tests_reads_every_runner_we_claim() {
        let out = failed_tests(&lines(&[
            "test parser::parses_empty ... ok",
            "test parser::parses_nested ... FAILED",
            "--- FAIL: TestRoundTrip (0.00s)",
            "FAILED tests/test_api.py::test_timeout - AssertionError: 3 != 4",
            "  ✕ adds two numbers (12 ms)",
            "test parser::parses_nested ... FAILED",
        ]));
        assert_eq!(
            out,
            vec![
                "parser::parses_nested".to_string(),
                "TestRoundTrip".to_string(),
                "tests/test_api.py::test_timeout".to_string(),
                "adds two numbers".to_string(),
            ],
            "one entry per failure, deduplicated, in first-seen order"
        );
    }

    #[test]
    fn failed_tests_ignores_passes_and_noise() {
        assert!(failed_tests(&lines(&[
            "test a ... ok",
            "--- PASS: TestOk (0.00s)",
            "● Console",
            "running 3 tests",
        ]))
        .is_empty());
    }

    #[test]
    fn rerun_failed_command_speaks_each_runners_filter() {
        let failed = vec!["a::b".to_string(), "c::d".to_string()];
        assert_eq!(
            rerun_failed_command("cargo test", &failed).unwrap(),
            "cargo test -- a::b c::d"
        );
        // An existing `--` is not duplicated.
        assert_eq!(
            rerun_failed_command("cargo test -- --nocapture", &failed).unwrap(),
            "cargo test -- --nocapture a::b c::d"
        );
        assert_eq!(
            rerun_failed_command("go test ./...", &vec!["TestA".into()]).unwrap(),
            "go test ./... -run '^(TestA)$'"
        );
        assert_eq!(
            rerun_failed_command("pytest -q", &vec!["t.py::test_x".into()]).unwrap(),
            "pytest -q t.py::test_x"
        );
        // An unknown runner is refused rather than guessed at.
        assert!(rerun_failed_command("make check", &failed).is_none());
        // Nothing failed, nothing to re-run.
        assert!(rerun_failed_command("cargo test", &[]).is_none());
    }
}
