//! Example plugin: report statistics about the current buffer.
//!
//! Demonstrates reading the whole buffer with `buffer_text`, analysing it, and
//! reporting the result on the status line with `message`.
//!
//! ```text
//! :plugin load .../libzmax_plugin_buffer_stats.dylib
//! :bufstats    # → "42 lines, 310 words, 2.1k chars, 2.2k bytes, longest line 118"
//! ```

use std::os::raw::c_int;

use zmax_plugin::{declare_plugin, Args, Host};

/// Human-ish thousands formatting: `2148` → `2.1k`, `950` → `950`.
fn human(n: usize) -> String {
    if n < 1000 {
        n.to_string()
    } else {
        format!("{:.1}k", n as f64 / 1000.0)
    }
}

/// `:bufstats` — line / word / char / byte counts plus the longest line width.
fn buffer_stats(host: &Host, _args: &Args) -> c_int {
    let Some(text) = host.buffer_text() else {
        host.error("bufstats: no active buffer");
        return 1;
    };
    let bytes = text.len();
    let chars = text.chars().count();
    let lines = text.lines().count();
    let words = text.split_whitespace().count();
    let longest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    host.message(&format!(
        "{lines} lines, {words} words, {} chars, {} bytes, longest line {longest}",
        human(chars),
        human(bytes),
    ));
    0
}

declare_plugin! {
    name: "buffer-stats",
    version: "0.1.0",
    commands: { "bufstats" => buffer_stats },
}
