//! zsh binding: an embedded shell command runner over zshrs.
//!
//! zsh writes to the real process fds (no in-process capture API), which would
//! corrupt the TUI, so output is captured via `super::capture`. Shell state
//! (variables, functions, cwd) persists across calls via a thread-local
//! `ShellExecutor`.
//!
//! NOTE: `cd` and `export` mutate the real process (cwd / env), since zshrs has
//! full OS access — that affects the editor process too. Acceptable for a
//! command runner; a sandboxed mode would be future work.

#[cfg(unix)]
use std::cell::RefCell;

#[cfg(unix)]
thread_local! {
    /// Persistent shell so vars/functions/cwd survive across `:zsh` calls.
    static SHELL: RefCell<Option<zsh::ShellExecutor>> = const { RefCell::new(None) };
}

/// Run a zsh command line, capturing stdout+stderr. Returns (exit status,
/// captured output).
#[cfg(unix)]
pub(super) fn run(cmd: &str) -> Result<(i32, String), String> {
    let (status, output) = super::capture::with_captured_fds(|| {
        SHELL.with(|cell| {
            let mut borrow = cell.borrow_mut();
            let sh = borrow.get_or_insert_with(zsh::ShellExecutor::new);
            sh.execute_script(cmd)
        })
    })?;

    match status {
        Ok(code) => Ok((code, output)),
        Err(e) if output.trim().is_empty() => Err(e),
        Err(e) => Err(format!("{e}\n{output}")),
    }
}

#[cfg(not(unix))]
pub(super) fn run(_cmd: &str) -> Result<(i32, String), String> {
    Err("embedded zsh is only supported on unix".into())
}
