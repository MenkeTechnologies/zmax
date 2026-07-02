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
    use zemacs_core::unicode::width::UnicodeWidthStr;
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
                    return;
                }
            }
        }
        // streams closed before wait resolved — finish waiting
        let code = child.wait().await.ok().and_then(|s| s.code());
        let mut s = st.lock().unwrap();
        s.exit_code = code;
        s.running = false;
    });

    state.lock().unwrap().abort = Some(handle.abort_handle());
    state
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
}
