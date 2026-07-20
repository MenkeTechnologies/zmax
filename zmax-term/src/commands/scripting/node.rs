//! JavaScript (Node) binding over the embedded node-js interpreter.
//!
//! node-js is a fusevm frontend whose `console.log` writes the real process
//! stdout fd, so evaluation runs inside the shared [`super::capture`] fd redirect
//! to keep the TUI clean. Logged output is shown; when nothing is logged, the
//! Node-style `inspect` of the program's value is shown instead (the REPL
//! convention). Stateless per call. Unix-only. The crate is imported as `nodejs`
//! (its lib name); the package is `node-js`.

/// Evaluate JavaScript source and return its captured `console.log` output,
/// falling back to the `inspect` of the program's value when nothing was logged.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    let (result, output) = super::capture::with_captured_fds(|| match nodejs::eval_str(code) {
        Ok(v) => Ok(nodejs::host::with_host(|h| h.inspect(&v))),
        Err(e) => Err(e),
    })?;

    match result {
        Ok(value) => Ok(super::pick_output(&output, &value)),
        Err(e) => Err(super::join_output(&output, &e)),
    }
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded node is only supported on unix".into())
}
