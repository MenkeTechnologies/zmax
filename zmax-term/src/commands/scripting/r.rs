//! R binding over the embedded rlang interpreter.
//!
//! rlang is a fusevm frontend with an in-process output sink
//! (`host::start_capture`/`take_capture`), so — like phplang and tclrs — it
//! needs no process-fd redirect: autoprint, `print` and `cat` all land in the
//! captured transcript. `rlang::eval_capture` is the same three calls, but it
//! folds a failure into the transcript as a trailing `Error:` line; the binding
//! composes them itself instead so a failing script reaches the editor as an
//! `Err` (a red status line) rather than as ordinary output.
//!
//! Top-level echo stays on, so `1 + 1` shows `[1] 2` the way `Rscript` does.
//! Each call resets the host, so state does not persist across `:rlang` calls —
//! the stateless contract ruby/python/node/php have. Unix-only (matches its
//! siblings).

/// Evaluate R source and return its transcript (autoprint / `print` / `cat`),
/// falling back to the formatted value of the last expression when the program
/// printed nothing.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    rlang::host::reset_host();
    rlang::host::start_capture();
    let result = rlang::compile(code).and_then(rlang::run_compiled);
    let output = rlang::host::take_capture();

    match result {
        Ok(value) => Ok(super::pick_output(
            &output,
            &rlang::builtins::format_value(&value).join("\n"),
        )),
        Err(e) => Err(super::join_output(&output, &e)),
    }
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded r is only supported on unix".into())
}
