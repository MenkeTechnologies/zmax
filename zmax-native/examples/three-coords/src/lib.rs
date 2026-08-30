//! Example plugin: the same cursor, in all three coordinate systems.
//!
//! A position in a text editor has three different numbers, and mixing them up
//! is the most common source of off-by-N bugs in plugins:
//!
//! | System | Counts | Who uses it |
//! |---|---|---|
//! | characters | Unicode scalar values | this SDK, everywhere |
//! | bytes | UTF-8 bytes | language servers |
//! | cells | terminal columns | anything drawing on screen |
//!
//! They agree on plain ASCII and diverge the moment a tab, an accent or an
//! emoji appears, which is exactly when a plugin that conflated them breaks.
//!
//! The SDK provides the bridges: [`Host::byte_offset`] and
//! [`Host::char_offset`] convert between the first two — the same split vim has
//! between `col()` and `charcol()` — while [`Host::virtual_column`] and
//! [`Host::virtcol_to_char`] convert between the first and third.
//!
//! ```text
//! :plugin load .../libzmax_native_three_coords.dylib
//! :pos   # → "line 3 · char 7 (byte 11, cell 9) · agree: no — 4 bytes, 2 cells adrift"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// How far apart the three readings are for one position.
///
/// Zero drift means the line is plain ASCII up to the cursor. Non-zero drift is
/// the signal that a plugin passing one number where another is expected would
/// land in the wrong place.
fn drift(chars: usize, bytes: usize, cells: usize) -> String {
    let byte_drift = bytes.saturating_sub(chars);
    let cell_drift = cells.abs_diff(chars);
    if byte_drift == 0 && cell_drift == 0 {
        return "agree: yes (plain ASCII)".to_string();
    }
    let mut parts = Vec::new();
    if byte_drift > 0 {
        parts.push(format!("{byte_drift} bytes"));
    }
    if cell_drift > 0 {
        parts.push(format!("{cell_drift} cells"));
    }
    format!("agree: no — {} adrift", parts.join(", "))
}

/// Whether the round trip through a coordinate system returns where it started.
///
/// Characters↔bytes is a bijection, so it always does. Characters↔cells is not:
/// several cells share one character wherever a tab or a wide glyph sits, so
/// the trip back lands on that glyph's own offset rather than where it began.
fn round_trip_note(original: usize, back: Option<usize>) -> String {
    match back {
        Some(at) if at == original => "cell round-trip: exact".to_string(),
        Some(at) => format!("cell round-trip: lands on char {at} (inside a wide glyph or tab)"),
        None => "cell round-trip: unavailable".to_string(),
    }
}

/// `:pos` — the cursor in characters, bytes and cells, plus the round trip.
fn pos(host: &Host, _args: &Args) -> c_int {
    let Some(cursor) = host.cursor() else {
        host.error("pos: no active buffer");
        return 1;
    };

    // Characters is the SDK's native unit; the other two are derived from it.
    let chars = cursor.offset;
    let bytes = host.byte_offset(chars);
    let cells = host.virtual_column();

    // Back the other way: which character does that cell belong to?
    let back = host.virtcol_to_char(cursor.line, cells);

    // Compare the three WITHIN the line, since `cells` is a column and the
    // other two are whole-buffer offsets. The line's own start converts once.
    let line_start = chars.saturating_sub(cursor.column);
    let byte_column = bytes.saturating_sub(host.byte_offset(line_start));

    host.message(&format!(
        "line {} · char {} (byte {byte_column}, cell {cells}) · {} · {}",
        cursor.line + 1,
        cursor.column,
        drift(cursor.column, byte_column, cells),
        round_trip_note(chars, back),
    ));
    0
}

declare_plugin! {
    name: "three-coords",
    version: "0.1.0",
    commands: { "pos" => pos },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain ASCII is the case where all three agree, and saying so is more
    /// useful than printing three zeroes.
    #[test]
    fn ascii_makes_all_three_agree() {
        assert_eq!(drift(7, 7, 7), "agree: yes (plain ASCII)");
    }

    /// A multi-byte character pushes bytes ahead of characters while leaving
    /// the cell count alone — an accent is one column wide.
    #[test]
    fn an_accent_moves_bytes_but_not_cells() {
        let note = drift(7, 11, 7);
        assert!(note.contains("4 bytes"));
        assert!(!note.contains("cells"), "still one column each");
    }

    /// A tab moves cells ahead of characters while leaving bytes alone — a tab
    /// is one byte that draws several columns.
    #[test]
    fn a_tab_moves_cells_but_not_bytes() {
        let note = drift(7, 7, 13);
        assert!(note.contains("6 cells"));
        assert!(!note.contains("bytes"));
    }

    /// Wide glyphs move both at once, which is the case that breaks plugins
    /// assuming any two of the three are interchangeable.
    #[test]
    fn wide_glyphs_move_both() {
        let note = drift(4, 10, 8);
        assert!(note.contains("6 bytes"));
        assert!(note.contains("4 cells"));
    }

    /// Characters to cells and back is not a bijection: a cell inside a tab
    /// belongs to that tab, so the trip back lands on the tab rather than where
    /// it started. Reporting that is the point.
    #[test]
    fn the_cell_round_trip_need_not_be_exact() {
        assert_eq!(round_trip_note(9, Some(9)), "cell round-trip: exact");

        let inexact = round_trip_note(9, Some(7));
        assert!(inexact.contains("lands on char 7"));
        assert!(inexact.contains("wide glyph or tab"));

        assert!(round_trip_note(9, None).contains("unavailable"));
    }
}
