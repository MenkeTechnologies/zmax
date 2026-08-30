//! Example plugin: the cursor has THREE columns, and they all mean something.
//!
//! - [`Host::cursor`]`.column` — characters into the line. Where the cursor is.
//! - [`Host::virtual_column`] — screen cells into the line. Where it LOOKS.
//! - [`Host::cursor_wanted_column`] — vim's `curswant`. Where vertical motion
//!   is AIMING.
//!
//! The third is the one people forget, and it is the one that explains a
//! behaviour everybody relies on without noticing: move down from the end of a
//! long line onto a short one and the cursor sits at the short line's end, but
//! keep going and it returns to the original column. The wanted column
//! remembers the ambition; the other two report the compromise.
//!
//! A plugin that repositions the cursor and wants motion to keep behaving must
//! be aware this exists — reading `column` and writing it back silently
//! discards the aim.
//!
//! ```text
//! :plugin load .../libzmax_native_sticky_column.dylib
//! :col   # → "char 4 · cell 4 · wants 37 — clamped: this line is 33 short of the aim"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// Whether the cursor is sitting where it wants to be, or has been clamped by a
/// short line.
///
/// `wanted` beyond the current column means vertical motion is still aiming
/// further right and will get there on the next long enough line.
fn is_clamped(column: usize, wanted: Option<usize>) -> bool {
    matches!(wanted, Some(w) if w > column)
}

/// How the three readings relate.
fn describe(column: usize, cells: usize, wanted: Option<usize>) -> String {
    let aim = match wanted {
        Some(w) => format!("wants {w}"),
        // No wanted column recorded: nothing has moved vertically yet.
        None => "no aim recorded".to_string(),
    };

    // Uses the same predicate the tests pin, so the reported state and the
    // tested one cannot drift apart.
    let note = match wanted {
        Some(w) if is_clamped(column, wanted) => {
            format!(" — clamped: this line is {} short of the aim", w - column)
        }
        Some(_) => " — at the aim".to_string(),
        None => String::new(),
    };

    // Characters and cells diverging means a tab or a wide glyph sits to the
    // left of the cursor on this line.
    let width_note = if cells != column {
        format!(" · {} cells of drift", cells.abs_diff(column))
    } else {
        String::new()
    };

    format!("char {column} · cell {cells} · {aim}{width_note}{note}")
}

/// `:col` — the three column readings and what they say together.
fn col(host: &Host, _args: &Args) -> c_int {
    let Some(cursor) = host.cursor() else {
        host.error("col: no active buffer");
        return 1;
    };
    host.message(&describe(
        cursor.column,
        host.virtual_column(),
        host.cursor_wanted_column(),
    ));
    0
}

declare_plugin! {
    name: "sticky-column",
    version: "0.1.0",
    commands: { "col" => col },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case the plugin exists for: parked on a short line, still aiming
    /// right. Reading `column` and writing it back would throw the aim away.
    #[test]
    fn a_short_line_clamps_but_keeps_the_aim() {
        assert!(is_clamped(4, Some(37)));
        let line = describe(4, 4, Some(37));
        assert!(line.contains("wants 37"));
        assert!(line.contains("clamped"));
        assert!(line.contains("33 short"), "37 - 4");
    }

    /// On a long enough line the cursor reaches its aim and there is nothing to
    /// report beyond that.
    #[test]
    fn a_long_enough_line_reaches_the_aim() {
        assert!(!is_clamped(37, Some(37)));
        assert!(describe(37, 37, Some(37)).contains("at the aim"));
    }

    /// An aim behind the cursor is not clamping — horizontal movement resets
    /// the wanted column, so this is the ordinary state after typing.
    #[test]
    fn an_aim_behind_the_cursor_is_not_clamping() {
        assert!(!is_clamped(10, Some(3)));
        assert!(!describe(10, 10, Some(3)).contains("clamped"));
    }

    /// With no vertical motion yet there is no aim, which is stated rather
    /// than defaulted to the current column.
    #[test]
    fn no_vertical_motion_means_no_aim() {
        assert!(!is_clamped(5, None));
        let line = describe(5, 5, None);
        assert!(line.contains("no aim recorded"));
        assert!(!line.contains("clamped"));
    }

    /// Characters and cells diverging is reported separately from the aim —
    /// it means a tab or wide glyph precedes the cursor, which is a different
    /// fact from being clamped.
    #[test]
    fn cell_drift_is_reported_independently() {
        let line = describe(4, 10, Some(4));
        assert!(line.contains("6 cells of drift"));
        assert!(!line.contains("clamped"), "drift is not clamping");
    }
}
