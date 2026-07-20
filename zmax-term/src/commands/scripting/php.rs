//! PHP binding over the embedded phplang interpreter.
//!
//! phplang is a fusevm frontend that buffers `echo`/`print` output internally
//! when capturing, so — unlike ruby/python/node — it needs no process-fd
//! redirect: `phplang::eval_capture` resets the host, runs the program with the
//! output buffer on, and returns whatever it emitted. PHP starts in *text* mode
//! (source outside `<?php … ?>` is echoed verbatim as HTML), so a bare `:php`
//! snippet is wrapped in an open tag when it carries none — the command's input
//! is code, not a template. Stateless per call. Unix-only (pulls libc + fusevm's
//! native layer).

/// Evaluate PHP source and return its captured `echo`/`print` output. Snippets
/// with no `<?php`/`<?=` open tag are treated as code (wrapped in `<?php … `)
/// rather than literal HTML.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    let wrapped;
    let src = if code.contains("<?") {
        code
    } else {
        wrapped = format!("<?php {code}");
        &wrapped
    };
    phplang::eval_capture(src).map(|out| out.trim_end_matches('\n').to_string())
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded php is only supported on unix".into())
}
