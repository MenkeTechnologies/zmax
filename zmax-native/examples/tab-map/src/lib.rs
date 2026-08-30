//! Example plugin: which tabpage am I on, and what else is open?
//!
//! Demonstrates [`Host::tab_count`] and [`Host::tab_index`] — vim's
//! `tabpagenr("$")` and `tabpagenr()`.
//!
//! One difference from vim to keep straight: these are **zero-based**, where
//! vim counts tabs from 1. The SDK is zero-based throughout and does not make
//! an exception here, so a plugin echoing a tab number to the user has to add
//! one, exactly as it would for a line.
//!
//! The windows and buffers reported alongside belong to the ACTIVE tab: only
//! one tab's window tree is live at a time, so `window_count` describes the tab
//! you are on, not the editor as a whole. Buffers are shared across all tabs,
//! which is why the buffer count does not move when you switch.
//!
//! ```text
//! :plugin load .../libzmax_native_tab_map.dylib
//! :tabs   # → "tab 2 of 3 · 2 windows here · 7 buffers (shared across tabs)"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// The position report, converting to vim's 1-based counting for display.
fn position(index: usize, count: usize) -> String {
    if count <= 1 {
        return "one tab".to_string();
    }
    format!("tab {} of {count}", index + 1)
}

/// What belongs to this tab versus what is shared.
///
/// Windows are per-tab; buffers are not. Saying so prevents the reasonable
/// wrong conclusion that switching tabs changes which buffers exist.
fn scope_note(windows: usize, buffers: usize, tabs: usize) -> String {
    let window_note = if windows == 1 {
        "1 window here".to_string()
    } else {
        format!("{windows} windows here")
    };
    let buffer_note = if tabs > 1 {
        format!("{buffers} buffers (shared across tabs)")
    } else {
        format!("{buffers} buffers")
    };
    format!("{window_note} · {buffer_note}")
}

/// `:tabs` — where you are, and what belongs to this tab.
fn tabs(host: &Host, _args: &Args) -> c_int {
    let count = host.tab_count();
    host.message(&format!(
        "{} · {}",
        position(host.tab_index(), count),
        scope_note(host.window_count(), host.buffer_count(), count),
    ));
    0
}

declare_plugin! {
    name: "tab-map",
    version: "0.1.0",
    commands: { "tabs" => tabs },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SDK counts tabs from 0 and vim counts from 1, so anything shown to
    /// a user is converted — the same rule as lines.
    #[test]
    fn display_converts_to_vims_counting() {
        assert_eq!(position(0, 3), "tab 1 of 3", "index 0 is the first tab");
        assert_eq!(position(2, 3), "tab 3 of 3");
    }

    /// A single tab is the normal case and does not need a position at all.
    #[test]
    fn one_tab_needs_no_position() {
        assert_eq!(position(0, 1), "one tab");
    }

    /// Windows belong to the active tab; buffers do not. The wording says so,
    /// because concluding that switching tabs changes the buffer list is a
    /// reasonable mistake to make from the bare numbers.
    #[test]
    fn windows_are_per_tab_and_buffers_are_not() {
        let many = scope_note(2, 7, 3);
        assert!(many.contains("2 windows here"));
        assert!(many.contains("shared across tabs"));

        // With one tab there is nothing to share with, so the note is dropped.
        let single = scope_note(1, 7, 1);
        assert!(!single.contains("shared"));
        assert!(single.contains("1 window here"), "singular reads correctly");
    }
}
