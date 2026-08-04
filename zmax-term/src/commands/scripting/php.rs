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

/// Run `code` as a pipeline stage with `input` bound to `$stdin`.
///
/// The binding cannot simply be prepended the way the other languages' can: PHP
/// source starts in text mode, so an assignment written outside a `<?php` tag
/// would be echoed as literal output. It is spliced *inside* an opening tag
/// instead, and source that carries its own `<?` tag is closed back into text
/// mode (`?>`) first so it reads exactly as it would have alone.
#[cfg(unix)]
pub(super) fn filter(code: &str, input: &str) -> Result<String, String> {
    let bind = format!(
        "<?php $stdin = \"{}\";\n",
        super::pipeline::dq(input, &['$'])
    );
    let src = if code.contains("<?") {
        format!("{bind}?>{code}")
    } else {
        format!("{bind}{code}")
    };
    phplang::eval_capture(&src).map(|out| out.trim_end_matches('\n').to_string())
}

#[cfg(not(unix))]
pub(super) fn filter(_code: &str, _input: &str) -> Result<String, String> {
    Err("embedded php is only supported on unix".into())
}
