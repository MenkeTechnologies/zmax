//! Example plugin: read the current selection as a rectangle.
//!
//! Demonstrates [`Host::region`] and [`Host::region_pos`], the pair that reads
//! a span under a selection TYPE rather than as a flat run of characters.
//!
//! The blockwise case has a rule worth seeing: a row whose text does not reach
//! the block's left column is **skipped**, not padded out to an empty string —
//! the same thing `CTRL-V` itself does. So a three-row block over ragged text
//! can come back with two rows, and only `region_pos` can tell you which two.
//! That is why the two calls exist as a pair.
//!
//! ```text
//! :plugin load .../libzmax_native_block_peek.dylib
//! :block-peek   # → "block: 2 of 3 rows reach the left column — [ab] [xy]"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host, RegionMode};

/// The selection kind as a [`RegionMode`], defaulting to charwise.
///
/// `select_kind` describes the selection that exists now; it is not vim's
/// `visualmode()`, which remembers the last one used.
fn mode_of(kind: Option<&str>) -> RegionMode {
    match kind {
        Some("block") => RegionMode::Blockwise,
        Some("line") => RegionMode::Linewise,
        _ => RegionMode::Charwise,
    }
}

/// How many rows the span covers, versus how many came back.
///
/// For a blockwise read the two differ whenever a row was too short to reach
/// the left column. Reported as a fraction so the skip is visible rather than
/// silently changing the row count.
fn coverage(extents: &[(usize, usize)], rows_spanned: usize) -> String {
    if extents.len() == rows_spanned {
        format!("{} rows", extents.len())
    } else {
        format!(
            "{} of {rows_spanned} rows reach the left column",
            extents.len(),
        )
    }
}

/// Render the returned rows, bracketed so an empty row is still visible.
fn render(rows: &[String]) -> String {
    if rows.is_empty() {
        return "(nothing)".to_string();
    }
    rows.iter()
        .map(|row| format!("[{row}]"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `:block-peek` — read the primary selection under its own selection kind.
fn block_peek(host: &Host, _args: &Args) -> c_int {
    let Some(span) = host.selection(0) else {
        host.error("block-peek: no selection");
        return 1;
    };
    let mode = mode_of(host.select_kind().as_deref());

    // anchor/head carry direction — head < anchor for a backwards selection —
    // so order them before asking for a region.
    let (from, to) = (span.anchor.min(span.head), span.anchor.max(span.head));

    let rows = host.region(from, to, mode);
    let extents = host.region_pos(from, to, mode);
    // A linewise read returns one row per line of the span, which is the row
    // count a blockwise read may not fill.
    let rows_spanned = host.region(from, to, RegionMode::Linewise).len();

    host.message(&format!(
        "{mode:?}: {} — {}",
        coverage(&extents, rows_spanned),
        render(&rows),
    ));
    0
}

declare_plugin! {
    name: "block-peek",
    version: "0.1.0",
    commands: { "block-peek" => block_peek },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The selection kind chooses how the span is read; anything unrecognised
    /// falls back to charwise rather than guessing.
    #[test]
    fn the_selection_kind_chooses_the_mode() {
        assert_eq!(mode_of(Some("block")), RegionMode::Blockwise);
        assert_eq!(mode_of(Some("line")), RegionMode::Linewise);
        assert_eq!(mode_of(Some("char")), RegionMode::Charwise);
        assert_eq!(mode_of(None), RegionMode::Charwise, "no selection kind");
        assert_eq!(mode_of(Some("weird")), RegionMode::Charwise, "unknown kind");
    }

    /// When every row came back, the count is stated plainly. When some were
    /// skipped, the fraction says so — a blockwise read silently returning
    /// fewer rows is the thing worth surfacing.
    #[test]
    fn a_skipped_row_shows_up_as_a_fraction() {
        let full = [(0usize, 2usize), (10, 12), (20, 22)];
        assert_eq!(coverage(&full, 3), "3 rows");

        let ragged = [(0usize, 2usize), (20, 22)];
        assert_eq!(coverage(&ragged, 3), "2 of 3 rows reach the left column");
    }

    /// Rows are bracketed so a genuinely empty row stays visible, and an empty
    /// result is named rather than rendered as nothing at all.
    #[test]
    fn rows_render_visibly_even_when_empty() {
        assert_eq!(render(&["ab".to_string(), "xy".to_string()]), "[ab] [xy]");
        assert_eq!(
            render(&["".to_string()]),
            "[]",
            "an empty row is still a row"
        );
        assert_eq!(render(&[]), "(nothing)");
    }
}
