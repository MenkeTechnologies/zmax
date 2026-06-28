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
    pub running: bool,
    pub exit_code: Option<i32>,
    pub scroll: usize,
    pub follow: bool,
    abort: Option<tokio::task::AbortHandle>,
}

pub type Run = Arc<Mutex<RunState>>;

const MAX_LINES: usize = 5000;

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
        "stk" => (format!("stryke {p}"), find_up(&dir, "stryke.toml").unwrap_or(dir)),
        "rs" => ("cargo run".to_string(), find_up(&dir, "Cargo.toml").unwrap_or(dir)),
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
                s.lines.push(format!("failed to start: {err}"));
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
            let mut s = st.lock().unwrap();
            s.lines.push(line);
            if s.lines.len() > MAX_LINES {
                let drop = s.lines.len() - MAX_LINES;
                s.lines.drain(0..drop);
            }
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
