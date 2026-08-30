//! Example plugin: which window is showing which buffer?
//!
//! Buffers and windows are not one-to-one, which is the whole reason both
//! directions exist in the SDK:
//!
//! - [`Host::window_buffer`] — the buffer a window shows. Total: every window
//!   shows something.
//! - [`Host::buffer_window`] — a window showing a buffer, if any. Partial: a
//!   buffer can be open with no window on it, and the same buffer can appear in
//!   two windows at once.
//!
//! So `buffer_window` is not an inverse of `window_buffer`; it is a lookup that
//! can fail, and this plugin reports exactly the buffers where it does — the
//! ones you have open but cannot see.
//!
//! ```text
//! :plugin load .../libzmax_native_window_map.dylib
//! :windows   # → "3 buffers in 2 windows · w0[80x24]→main.rs w1[80x24]→lib.rs · hidden: notes.md*"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// One window's cell: its index, size, and the buffer it shows.
fn window_cell(index: usize, size: Option<(usize, usize)>, buffer: Option<&str>) -> String {
    let name = buffer.unwrap_or("—");
    match size {
        Some((w, h)) => format!("w{index}[{w}x{h}]→{name}"),
        None => format!("w{index}→{name}"),
    }
}

/// Buffers that no window is showing, marked `*` when they have unsaved
/// changes — those are the ones worth knowing about, since closing the editor
/// would prompt for a buffer you cannot currently see.
fn hidden_note(hidden: &[(String, bool)]) -> String {
    if hidden.is_empty() {
        return String::new();
    }
    let list = hidden
        .iter()
        .map(|(name, modified)| {
            if *modified {
                format!("{name}*")
            } else {
                name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(" · hidden: {list}")
}

/// `:windows` — the window/buffer map, plus any buffers with no window.
fn windows(host: &Host, _args: &Args) -> c_int {
    let window_count = host.window_count();
    let buffer_count = host.buffer_count();

    let cells: Vec<String> = (0..window_count)
        .map(|w| {
            let buffer = host.window_buffer(w).and_then(|b| host.buffer_name(b));
            window_cell(w, host.window_size_at(w), buffer.as_deref())
        })
        .collect();

    // A buffer with no window is open but invisible. `buffer_window` returning
    // None is how that shows up.
    let hidden: Vec<(String, bool)> = (0..buffer_count)
        .filter(|b| host.buffer_window(*b).is_none())
        .filter_map(|b| {
            host.buffer_name(b)
                .map(|name| (name, host.buffer_modified(b)))
        })
        .collect();

    host.message(&format!(
        "{buffer_count} buffers in {window_count} windows · {}{}",
        cells.join(" "),
        hidden_note(&hidden),
    ));
    0
}

declare_plugin! {
    name: "window-map",
    version: "0.1.0",
    commands: { "windows" => windows },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window always shows something, and its size is part of the picture.
    #[test]
    fn a_window_cell_carries_its_size() {
        assert_eq!(
            window_cell(0, Some((80, 24)), Some("main.rs")),
            "w0[80x24]→main.rs"
        );
        assert_eq!(window_cell(1, None, Some("lib.rs")), "w1→lib.rs");
    }

    /// A window whose buffer cannot be named still renders, rather than being
    /// dropped from the map and leaving a gap in the window numbering.
    #[test]
    fn an_unnamed_buffer_still_gets_a_cell() {
        assert_eq!(window_cell(2, Some((40, 10)), None), "w2[40x10]→—");
    }

    /// Hidden buffers with unsaved changes are the ones that matter — they are
    /// invisible and would block a clean exit.
    #[test]
    fn unsaved_hidden_buffers_are_starred() {
        let hidden = vec![
            ("notes.md".to_string(), true),
            ("clean.rs".to_string(), false),
        ];
        let note = hidden_note(&hidden);
        assert!(note.contains("notes.md*"), "unsaved gets a star");
        assert!(note.contains("clean.rs"));
        assert!(!note.contains("clean.rs*"));
    }

    /// With every buffer on screen there is no note at all, rather than an
    /// empty "hidden:" label.
    #[test]
    fn nothing_hidden_says_nothing() {
        assert_eq!(hidden_note(&[]), "");
    }
}
