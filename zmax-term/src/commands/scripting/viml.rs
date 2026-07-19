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

/// Evaluate a single VimL *expression* and return its rendered value.
///
/// Not the same thing as [`eval`]: script evaluation reads a leading `"` as a
/// comment and a leading `'` as a mark, so `"XYZ"` and `'XYZ'` — perfectly good
/// expressions — evaluate to nothing there. The expression parser reads them as
/// the string literals they are, which is what a command line asking for a value
/// (`c_CTRL-\ e`) needs.
///
/// An error *reported* while the expression ran is an error here even when the
/// evaluator itself carried on: `no_such_fn()` raises E117 and yields `v:null`,
/// and vim leaves the command line alone rather than replacing it with that.
/// Errors are observed, not captured — capturing is vim's `emsg_silent` path and
/// would stop a `:try` inside a called function from ever catching anything.
///
/// `:echo` from a called function is captured so it cannot reach the TUI, and
/// dropped rather than joined: the caller wants the expression's value, and
/// vim's echo goes to the message area, never onto the command line.
#[cfg(unix)]
pub(super) fn eval_expr(src: &str) -> Result<String, String> {
    use vimlrs::fusevm_bridge::{observe_errors_begin, observe_errors_take};
    capture_begin();
    observe_errors_begin();
    let result = vimlrs::fusevm_bridge::eval_expr(src);
    let echo = capture_take();
    let errors = observe_errors_take();
    match result {
        Err(e) => Err(join(&echo, &e.0)),
        Ok(_) if !errors.is_empty() => Err(join(&echo, &errors.join("\n"))),
        Ok(v) => Ok(encode_tv2echo(&v)),
    }
}

#[cfg(not(unix))]
pub(super) fn eval_expr(_src: &str) -> Result<String, String> {
    Err("embedded vimscript is only supported on unix".into())
}

/// Publish the editor's live command line into vimlrs's `ccline` model, so an
/// expression evaluated from it (vim `c_CTRL-\ e`) reads the real line through
/// `getcmdline()`, `getcmdpos()` and `getcmdtype()`. `pos` is 1-based, as
/// `getcmdpos()` reports it; `cmdtype` is the command line's first typed
/// character (`:`, `/`, `?`, `=`, `-`, or `@` for an `input()`-style prompt).
#[cfg(unix)]
pub(super) fn cmdline_publish(line: &str, pos: usize, cmdtype: char) {
    let mut buf = [0u8; 4];
    vimlrs::fusevm_bridge::cmdline_host_publish(line, pos as i64, cmdtype.encode_utf8(&mut buf));
}

#[cfg(not(unix))]
pub(super) fn cmdline_publish(_line: &str, _pos: usize, _cmdtype: char) {}

/// The 1-based cursor position of the published command line, read back after
/// an expression that may have moved it with `setcmdpos()`.
#[cfg(unix)]
pub(super) fn cmdline_pos() -> usize {
    vimlrs::fusevm_bridge::cmdline_host_pos().max(0) as usize
}

#[cfg(not(unix))]
pub(super) fn cmdline_pos() -> usize {
    0
}

/// Drop the published command line — no command line is active any more, so the
/// three getters report "nothing" again, as vim's do outside cmdline mode.
#[cfg(unix)]
pub(super) fn cmdline_clear() {
    vimlrs::fusevm_bridge::cmdline_host_clear();
}

#[cfg(not(unix))]
pub(super) fn cmdline_clear() {}

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
