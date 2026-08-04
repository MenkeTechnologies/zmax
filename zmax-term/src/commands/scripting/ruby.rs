//! Ruby binding over the embedded rubylang interpreter.
//!
//! Evaluation goes through `rubylang::eval_str_captured`, the embedder entry
//! point: it captures `puts`/`print`/`p` in-process instead of letting them
//! reach the process stdout fd (which would corrupt the TUI), and it binds any
//! values the caller supplies *after* the host reset that starts every run. The
//! program's printed output is shown; when it prints nothing, the last
//! expression's `inspect` value is shown instead (the irb `=> …` convention).
//! Each call is a fresh eval (stateless), matching the other filter-style
//! bindings. Unix-only (pulls libc + fusevm's native layer).

/// Evaluate Ruby source and return its captured `puts`/`print` output, falling
/// back to the `inspect` of the program's value when nothing was printed.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    run(code, &[])
}

/// Run `code` as a pipeline stage with `input` bound to `stdin`. The binding is
/// a real Ruby `String` on the interpreter's heap, so nothing is escaped
/// anywhere: the input is data, never syntax.
#[cfg(unix)]
pub(super) fn filter(code: &str, input: &str) -> Result<String, String> {
    run(code, &[("stdin", input)])
}

#[cfg(unix)]
fn run(code: &str, bindings: &[(&str, &str)]) -> Result<String, String> {
    let (result, output) = rubylang::eval_str_captured(code, bindings);
    match result {
        Ok(value) => {
            let rendered = rubylang::host::with_host(|h| h.inspect(&value));
            Ok(super::pick_output(&output, &rendered))
        }
        Err(e) => Err(super::join_output(&output, &e)),
    }
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded ruby is only supported on unix".into())
}

#[cfg(not(unix))]
pub(super) fn filter(_code: &str, _input: &str) -> Result<String, String> {
    Err("embedded ruby is only supported on unix".into())
}
