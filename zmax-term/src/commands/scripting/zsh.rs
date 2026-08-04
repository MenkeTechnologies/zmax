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

/// Run `code` as a pipeline stage with `input` bound to `$stdin`, returning what
/// the code wrote. The executor persists across calls, so the binding is a real
/// parameter assignment (`set_scalar`) rather than a literal spliced into the
/// source — no escaping, and no copy of the input beyond the parameter itself.
/// A non-zero exit is not a stage failure: a filter's output is what it wrote,
/// exactly as it is through a shell pipe.
#[cfg(unix)]
pub(super) fn filter(code: &str, input: &str) -> Result<String, String> {
    let (status, output) = super::capture::with_captured_fds(|| {
        SHELL.with(|cell| {
            let mut borrow = cell.borrow_mut();
            let sh = borrow.get_or_insert_with(zsh::ShellExecutor::new);
            sh.set_scalar("stdin".to_string(), input.to_string());
            sh.execute_script(code)
        })
    })?;

    match status {
        Ok(_) => Ok(output.trim_end_matches('\n').to_string()),
        Err(e) => Err(super::join_output(&output, &e)),
    }
}

#[cfg(not(unix))]
pub(super) fn filter(_code: &str, _input: &str) -> Result<String, String> {
    Err("embedded zsh is only supported on unix".into())
}
