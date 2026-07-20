//! Ruby binding over the embedded rubylang interpreter.
//!
//! rubylang is a fusevm frontend whose `puts`/`print`/`p` write the real process
//! stdout fd, which would corrupt the TUI — so evaluation runs inside the shared
//! [`super::capture`] fd redirect. The program's printed output is shown; when it
//! prints nothing, the last expression's `inspect` value is shown instead (the
//! irb `=> …` convention). Each call is a fresh eval (stateless), matching the
//! other filter-style bindings. Unix-only (pulls libc + fusevm's native layer).

/// Evaluate Ruby source and return its captured `puts`/`print` output, falling
/// back to the `inspect` of the program's value when nothing was printed.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    let (result, output) = super::capture::with_captured_fds(|| match rubylang::eval_str(code) {
        Ok(v) => Ok(rubylang::host::with_host(|h| h.inspect(&v))),
        Err(e) => Err(e),
    })?;

    match result {
        Ok(value) => Ok(super::pick_output(&output, &value)),
        Err(e) => Err(super::join_output(&output, &e)),
    }
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded ruby is only supported on unix".into())
}
