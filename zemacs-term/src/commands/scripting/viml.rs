//! Vimscript (VimL) binding over the embedded vimlrs interpreter.
//!
//! Currently this is the *eval half*: VimL source is evaluated with persistent
//! globals/functions (vimlrs keeps them in thread-local state across calls) and
//! `:echo` output is captured so it never leaks onto the TUI. Editor-mutating
//! ex-commands (`:map`, `:command`, `:set`, `:autocmd`) and editor builtins
//! (`setline`/`getline`/`feedkeys`) are vimlrs TODO stubs upstream; once they
//! gain a host hook they will route through the same [`super`] `api_*` surface
//! the [`CxGuard`](super) already publishes during eval.

use vimlrs::fusevm_bridge::{capture_begin, capture_take};
use vimlrs::ported::eval::encode::encode_tv2echo;

/// Evaluate VimL source, returning captured `:echo` output plus the rendered
/// value of a trailing bare expression (if any). Errors carry any echo emitted
/// before the failure.
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

/// Join captured echo output with a trailing fragment, skipping empties.
fn join(echo: &str, tail: &str) -> String {
    let echo = echo.trim_end_matches('\n');
    match (echo.is_empty(), tail.is_empty()) {
        (true, _) => tail.to_string(),
        (false, true) => echo.to_string(),
        (false, false) => format!("{echo}\n{tail}"),
    }
}
