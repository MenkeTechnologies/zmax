//! Example plugin: strip trailing whitespace from every line.
//!
//! Demonstrates composing plugin logic with the editor's own commands: it reads
//! the buffer with `buffer_text` to decide whether there is anything to do, then
//! drives the built-in substitute via `eval` (a `:` command line) so the edit is
//! a normal, undoable editor operation.
//!
//! ```text
//! :plugin load .../libzmax_plugin_trim_trailing.dylib
//! :trim-trailing
//! ```

use std::os::raw::c_int;

use zmax_plugin::{declare_plugin, Args, Host};

/// `:trim-trailing` — remove trailing spaces/tabs from all lines.
fn trim_trailing(host: &Host, _args: &Args) -> c_int {
    let Some(text) = host.buffer_text() else {
        host.error("trim-trailing: no active buffer");
        return 1;
    };
    let has_trailing = text
        .lines()
        .any(|l| l.ends_with(' ') || l.ends_with('\t'));
    if !has_trailing {
        host.message("trim-trailing: nothing to trim");
        return 0;
    }
    // Run the editor's substitute over the whole file; the edit lands in the
    // undo history like any interactive `:%s`.
    let rc = host.eval(r"%s/\s\+$//");
    if rc == 0 {
        host.message("trim-trailing: trimmed trailing whitespace");
    }
    rc
}

declare_plugin! {
    name: "trim-trailing",
    version: "0.1.0",
    commands: { "trim-trailing" => trim_trailing },
}
