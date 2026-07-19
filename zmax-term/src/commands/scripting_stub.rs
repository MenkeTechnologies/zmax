//! Stub embedded-scripting host, compiled when the `scripting` feature is OFF.
//!
//! It mirrors the public entry-point surface of the real
//! [`crate::commands::scripting`] module (see `scripting/mod.rs`) so the
//! `:elisp`/`:vim`/`:awk`/`:zsh`/`:stryke` commands and the REPL still link, but
//! every entry point simply reports that the interpreters were not compiled into
//! this build. None of the interpreter crates (elisprs/vimlrs/awkrs/zsh/stryke)
//! are pulled in this configuration.

use crate::compositor;

/// Message surfaced by every entry point when scripting is disabled.
const DISABLED: &str =
    "embedded scripting was not compiled into this build (rebuild with the `scripting` feature)";

/// See [`crate::commands::scripting::eval_elisp`].
pub fn eval_elisp(_cx: &mut compositor::Context, _src: &str) -> Result<String, String> {
    Err(DISABLED.to_string())
}

/// See [`crate::commands::scripting::elisp_global_bool`].
pub fn elisp_global_bool(_name: &str) -> Option<bool> {
    None
}

/// See [`crate::commands::scripting::eval_viml`].
pub fn eval_viml(_cx: &mut compositor::Context, _src: &str) -> Result<String, String> {
    Err(DISABLED.to_string())
}

/// See [`crate::commands::scripting::eval_viml_expr`].
pub fn eval_viml_expr(_cx: &mut compositor::Context, _expr: &str) -> Result<String, String> {
    Err(DISABLED.to_string())
}

/// See [`crate::commands::scripting::viml_cmdline_publish`].
pub fn viml_cmdline_publish(_line: &str, _pos: usize, _cmdtype: char) {}

/// See [`crate::commands::scripting::viml_cmdline_pos`].
pub fn viml_cmdline_pos() -> usize {
    0
}

/// See [`crate::commands::scripting::viml_cmdline_clear`].
pub fn viml_cmdline_clear() {}

/// See [`crate::commands::scripting::source_viml_file`].
pub fn source_viml_file(
    _cx: &mut compositor::Context,
    _path: &std::path::Path,
) -> Result<(), String> {
    Err(DISABLED.to_string())
}

/// See [`crate::commands::scripting::run_awk_filter`].
pub fn run_awk_filter(_cx: &mut compositor::Context, _program: &str) -> Result<String, String> {
    Err(DISABLED.to_string())
}

/// See [`crate::commands::scripting::eval_stryke`].
pub fn eval_stryke(_cx: &mut compositor::Context, _code: &str) -> Result<String, String> {
    Err(DISABLED.to_string())
}

/// See [`crate::commands::scripting::run_zsh`].
pub fn run_zsh(_cmd: &str) -> Result<(i32, String), String> {
    Err(DISABLED.to_string())
}

/// See [`crate::commands::scripting::repl_awk`].
pub fn repl_awk(_cx: &mut compositor::Context, _program: &str) -> Result<String, String> {
    Err(DISABLED.to_string())
}

/// See [`crate::commands::scripting::load_init_scripts`]. No-op without the
/// interpreters.
pub fn load_init_scripts(_cx: &mut compositor::Context) {}
