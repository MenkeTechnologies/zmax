//! Python binding over the embedded pythonrs interpreter.
//!
//! pythonrs is a fusevm frontend whose `print(...)` writes the real process
//! stdout fd, so evaluation runs inside the shared [`super::capture`] fd redirect
//! to keep the TUI clean. Printed output is shown; when nothing is printed, the
//! `repr` of the program's value is shown instead (the interactive `>>> `
//! convention). Stateless per call. Unix-only.

/// Evaluate Python source and return its captured `print` output, falling back to
/// the `repr` of the program's value when nothing was printed.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    let (result, output) = super::capture::with_captured_fds(|| match pythonrs::eval_str(code) {
        Ok(v) => Ok(pythonrs::host::with_host(|h| h.repr_of(&v))),
        Err(e) => Err(e),
    })?;

    match result {
        Ok(value) => Ok(super::pick_output(&output, &value)),
        Err(e) => Err(super::join_output(&output, &e)),
    }
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded python is only supported on unix".into())
}
