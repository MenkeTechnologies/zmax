//! Vimscript (VimL) binding over the embedded vimlrs interpreter.
//!
//! VimL source is evaluated with persistent globals/functions (vimlrs keeps them
//! in thread-local state across calls) and `:echo` output is captured so it never
//! leaks onto the TUI. Editor-mutating ex-commands `:map`, `:command`, `:set` and
//! editor builtins (`setline`/`getline`/`feedkeys`) are wired: they route through
//! the host hooks installed in [`super`] (`install_map_hook`, `install_excmd_hook`,
//! `install_set_hook`, `install_editor_host`) onto the same `api_*` surface the
//! [`CxGuard`](super) publishes during eval. `:autocmd` is the remaining exception
//! — it has no vimlrs host hook yet; native autocmds (`vim_autocmd`) handle that
//! surface independently.

#[cfg(unix)]
use vimlrs::fusevm_bridge::{capture_begin, capture_take};
#[cfg(unix)]
use vimlrs::ported::eval::encode::encode_tv2echo;

/// Evaluate VimL source, returning captured `:echo` output plus the rendered
/// value of a trailing bare expression (if any). Errors carry any echo emitted
/// before the failure.
#[cfg(unix)]
pub(super) fn eval(src: &str) -> Result<String, String> {
    capture_begin();
    let result = vimlrs::eval_source(src);
    let echo = capture_take();
    match result {
        Ok(Some(v)) => {
            let rendered = encode_tv2echo(&v);
            Ok(join(&echo, &rendered))
        }
        Ok(None) => Ok(echo.trim_end_matches('\n').to_string()),
        Err(e) => Err(join(&echo, &e.0)),
    }
}

#[cfg(not(unix))]
pub(super) fn eval(_src: &str) -> Result<String, String> {
    Err("embedded vimscript is only supported on unix".into())
}

/// Join captured echo output with a trailing fragment, skipping empties.
#[cfg(unix)]
fn join(echo: &str, tail: &str) -> String {
    let echo = echo.trim_end_matches('\n');
    match (echo.is_empty(), tail.is_empty()) {
        (true, _) => tail.to_string(),
        (false, true) => echo.to_string(),
        (false, false) => format!("{echo}\n{tail}"),
    }
}
