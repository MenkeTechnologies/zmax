//! JavaScript (Node) binding over the embedded node-js interpreter.
//!
//! Evaluation goes through `nodejs::eval_str_captured`, the embedder entry
//! point: it captures `console.log` (and `process.stdout.write`) in-process
//! instead of letting them reach the process stdout fd, which would corrupt the
//! TUI, and it binds any values the caller supplies *after* the host reset that
//! starts every run. Logged output is shown; when nothing is logged, the
//! Node-style `inspect` of the program's value is shown instead (the REPL
//! convention). Stateless per call. Unix-only. The crate is imported as
//! `nodejs` (its lib name); the package is `node-js`.

/// Evaluate JavaScript source and return its captured `console.log` output,
/// falling back to the `inspect` of the program's value when nothing was logged.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    run(code, &[])
}

/// Run `code` as a pipeline stage with `input` bound to `stdin`. The binding is
/// a real JS string on the interpreter's heap, so nothing is escaped anywhere:
/// the input is data, never syntax.
#[cfg(unix)]
pub(super) fn filter(code: &str, input: &str) -> Result<String, String> {
    run(code, &[("stdin", input)])
}

#[cfg(unix)]
fn run(code: &str, bindings: &[(&str, &str)]) -> Result<String, String> {
    let (result, output) = nodejs::eval_str_captured(code, bindings);
    match result {
        Ok(value) => {
            let rendered = nodejs::host::with_host(|h| h.inspect(&value));
            Ok(super::pick_output(&output, &rendered))
        }
        Err(e) => Err(super::join_output(&output, &e)),
    }
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded node is only supported on unix".into())
}

#[cfg(not(unix))]
pub(super) fn filter(_code: &str, _input: &str) -> Result<String, String> {
    Err("embedded node is only supported on unix".into())
}
