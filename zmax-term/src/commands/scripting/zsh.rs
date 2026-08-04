//! zsh binding: an embedded shell command runner over zshrs.
//!
//! Runs through `ShellExecutor::execute_script_captured`, which captures the
//! run's stdout and stderr so nothing reaches the TUI's terminal. The capture is
//! at fd level, not in a buffer like the language runtimes': a shell forks, and
//! a child inherits fd 1 knowing nothing about in-process buffers. zshrs does it
//! with the fd >= 10 discipline its own redirections require, and on the *same*
//! VM — so shell state (variables, functions, cwd) still persists across calls
//! via the thread-local `ShellExecutor`, which a `$(…)`-style subshell capture
//! would have thrown away.
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
    Ok(SHELL.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let sh = borrow.get_or_insert_with(zsh::ShellExecutor::new);
        sh.execute_script_captured(cmd)
    }))
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
    Ok(SHELL.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let sh = borrow.get_or_insert_with(zsh::ShellExecutor::new);
        sh.set_scalar("stdin".to_string(), input.to_string());
        sh.execute_script_captured(code).1
    }))
}

#[cfg(not(unix))]
pub(super) fn filter(_code: &str, _input: &str) -> Result<String, String> {
    Err("embedded zsh is only supported on unix".into())
}
