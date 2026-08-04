//! Python binding over the embedded pythonrs interpreter.
//!
//! Evaluation goes through `pythonrs::eval_str_captured`, the embedder entry
//! point: it captures `print` (and a direct `sys.stdout.write`) in-process
//! instead of letting them reach the process stdout fd, which would corrupt the
//! TUI, and it binds any values the caller supplies *after* the host reset that
//! starts every run. Printed output is shown; when nothing is printed, the
//! `repr` of the program's value is shown instead (the interactive `>>> `
//! convention). Stateless per call. Unix-only.

/// Evaluate Python source and return its captured `print` output, falling back to
/// the `repr` of the program's value when nothing was printed.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    run(code, &[])
}

/// Run `code` as a pipeline stage with `input` bound to `stdin`. The binding is
/// a real Python `str` on the interpreter's heap, so nothing is escaped
/// anywhere: the input is data, never syntax.
#[cfg(unix)]
pub(super) fn filter(code: &str, input: &str) -> Result<String, String> {
    run(code, &[("stdin", input)])
}

#[cfg(unix)]
fn run(code: &str, bindings: &[(&str, &str)]) -> Result<String, String> {
    let (result, output) = pythonrs::eval_str_captured(code, bindings);
    match result {
        Ok(value) => {
            let rendered = pythonrs::host::with_host(|h| h.repr_of(&value));
            Ok(super::pick_output(&output, &rendered))
        }
        Err(e) => Err(super::join_output(&output, &e)),
    }
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded python is only supported on unix".into())
}

#[cfg(not(unix))]
pub(super) fn filter(_code: &str, _input: &str) -> Result<String, String> {
    Err("embedded python is only supported on unix".into())
}
