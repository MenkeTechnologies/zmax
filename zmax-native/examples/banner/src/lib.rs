//! Example plugin: insert a boxed banner around the given text.
//!
//! Demonstrates argument handling (`Args`) and multi-line `insert_text`. The box
//! is drawn to the exact width of the text so the borders always line up.
//!
//! ```text
//! :plugin load .../libzmax_native_banner.dylib
//! :banner Section One
//! ```
//! inserts:
//! ```text
//! ╭─────────────╮
//! │ Section One │
//! ╰─────────────╯
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// `:banner <text…>` — insert a Unicode box around `text` at the cursor.
fn banner(host: &Host, args: &Args) -> c_int {
    let text = args.rest().join(" ");
    if text.is_empty() {
        host.error("banner: expected some text");
        return 1;
    }
    // Inner width is the visible text width; the border repeats span the text
    // plus the two padding spaces, so top/middle/bottom are all `width + 4` wide.
    let inner = text.chars().count();
    let bar = "─".repeat(inner + 2);
    let boxed = format!("╭{bar}╮\n│ {text} │\n╰{bar}╯\n");
    if host.insert_text(&boxed) {
        0
    } else {
        host.error("banner: no active buffer");
        1
    }
}

declare_plugin! {
    name: "banner",
    version: "0.1.0",
    commands: { "banner" => banner },
}
