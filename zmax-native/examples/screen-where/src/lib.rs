//! Example plugin: where is the cursor — in the document, and on the screen?
//!
//! Two different questions with two different answers, and a plugin that draws
//! anything needs both:
//!
//! - [`Host::cursor`] — the position in the DOCUMENT. Unbounded by the window.
//! - [`Host::screen_position`] — vim `screenpos()`, the position inside the
//!   WINDOW, in cells. Row 0 is the top visible line, whatever line that is.
//! - [`Host::window_view`] — which lines are currently visible.
//!
//! `window_view` packs its answer asymmetrically, which is worth knowing before
//! you use it: `line` is the first visible line NUMBER, but the last visible
//! line arrives only as a char offset in `head`. Turning that into a line number
//! takes a trip through [`Host::byte_offset`] and [`Host::byte_to_line`], which
//! this plugin does once and names.
//!
//! ```text
//! :plugin load .../libzmax_native_screen_where.dylib
//! :where   # → "doc 412/900 · screen row 12 col 9 · showing 400–447 of 900 (37% down)"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// How far through the buffer the cursor sits, as a percentage.
///
/// Guards the empty buffer rather than dividing by zero.
fn progress(line: usize, total: usize) -> Option<usize> {
    (total > 0).then(|| line * 100 / total)
}

/// Whether the cursor is actually on screen.
///
/// It normally is, but a plugin acting after a programmatic jump can run before
/// the view has scrolled, and drawing relative to a cursor that is off-screen
/// puts the mark in the wrong place.
fn on_screen(line: usize, first: usize, last: usize) -> bool {
    line >= first && line <= last
}

/// The report line.
fn report(
    doc_line: usize,
    total: usize,
    row: usize,
    column: usize,
    first: usize,
    last: usize,
) -> String {
    let percent = match progress(doc_line, total) {
        Some(p) => format!(" ({p}% down)"),
        None => String::new(),
    };
    let warning = if on_screen(doc_line, first, last) {
        String::new()
    } else {
        " · CURSOR OFF SCREEN".to_string()
    };
    format!(
        "doc {}/{total} · screen row {row} col {column} · showing {}–{}{percent}{warning}",
        doc_line + 1,
        first + 1,
        last + 1,
    )
}

/// `:where` — the cursor's document and screen positions, and the viewport.
fn where_cmd(host: &Host, _args: &Args) -> c_int {
    let Some(cursor) = host.cursor() else {
        host.error("where: no active buffer");
        return 1;
    };
    let Some(view) = host.window_view() else {
        host.error("where: no window");
        return 1;
    };

    // `window_view` gives the first visible line as a NUMBER but the last only
    // as a char offset, so the last needs converting: chars → bytes → line.
    let first_line = view.line;
    let last_line = host.byte_to_line(host.byte_offset(view.head));

    // `screen_position` packs the cell column in `anchor` and the window row in
    // `line` — the row is relative to the top of the window, not the document.
    let (row, column) = match host.screen_position() {
        Some(pos) => (pos.line, pos.anchor),
        None => (0, 0),
    };

    host.message(&report(
        cursor.line,
        host.line_count(),
        row,
        column,
        first_line,
        last_line,
    ));
    0
}

declare_plugin! {
    name: "screen-where",
    version: "0.1.0",
    commands: { "where" => where_cmd },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty buffer has no position to express as a percentage, so none is
    /// shown rather than dividing by zero.
    #[test]
    fn an_empty_buffer_has_no_percentage() {
        assert_eq!(progress(0, 0), None);
        assert_eq!(progress(50, 100), Some(50));
        assert_eq!(progress(0, 100), Some(0), "the top is 0%, not absent");
    }

    /// The viewport bounds are inclusive: the last visible line IS visible.
    #[test]
    fn the_viewport_bounds_are_inclusive() {
        assert!(on_screen(400, 400, 447), "first line counts");
        assert!(on_screen(447, 400, 447), "last line counts");
        assert!(!on_screen(448, 400, 447));
        assert!(!on_screen(399, 400, 447));
    }

    /// A cursor outside the viewport is called out loudly — drawing relative to
    /// it would put the mark somewhere the user is not looking.
    #[test]
    fn an_off_screen_cursor_is_called_out() {
        let off = report(900, 1000, 0, 0, 400, 447);
        assert!(off.contains("CURSOR OFF SCREEN"));

        let on = report(410, 1000, 10, 4, 400, 447);
        assert!(!on.contains("OFF SCREEN"));
    }

    /// Everything a human reads is 1-based, while every value from the SDK is
    /// 0-based — the conversion happens once, at the point of display.
    #[test]
    fn display_is_one_based_throughout() {
        let line = report(411, 900, 12, 9, 399, 446);
        assert!(line.contains("doc 412/900"));
        assert!(line.contains("showing 400–447"));
        // The screen row and column are positions within the window, and are
        // shown as the SDK reports them.
        assert!(line.contains("row 12 col 9"));
    }
}
