//! Example plugin: inspect every selection, not just the primary one.
//!
//! zmax is multi-selection, and this is where the SDK deliberately parts ways
//! with vim. vim's `'<` and `'>` are always in document order; a zmax [`Span`]
//! carries `anchor` and `head`, and `head < anchor` for a BACKWARDS selection.
//!
//! That direction is real information — it is the end the user is extending
//! from — so the SDK keeps it rather than sorting it away. Any plugin that
//! slices text from a span must order the pair first, and any plugin that
//! extends a selection must not.
//!
//! ```text
//! :plugin load .../libzmax_native_multi_cursor.dylib
//! :cursors   # → "4 selections (2 backwards) · primary #1 line 12, 9 chars · 31 chars total"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host, Span};

/// A span's extent, regardless of which way round it is.
///
/// The ONLY safe way to turn a span into a range: slicing `anchor..head`
/// directly panics or silently yields nothing on a backwards selection.
fn extent(span: &Span) -> (usize, usize) {
    (span.anchor.min(span.head), span.anchor.max(span.head))
}

/// How many characters a span covers.
fn width(span: &Span) -> usize {
    let (from, to) = extent(span);
    to - from
}

/// Whether the user is extending leftwards — `head` before `anchor`.
fn is_backwards(span: &Span) -> bool {
    span.head < span.anchor
}

/// The summary line.
fn summary(spans: &[Span], primary: usize) -> String {
    if spans.is_empty() {
        return "no selections".to_string();
    }
    let backwards = spans.iter().filter(|s| is_backwards(s)).count();
    let total: usize = spans.iter().map(width).sum();
    let empty = spans.iter().filter(|s| width(s) == 0).count();

    let direction = if backwards > 0 {
        format!(" ({backwards} backwards)")
    } else {
        String::new()
    };
    // Every selection being empty is the ordinary multi-CURSOR case, as opposed
    // to multiple selected ranges — worth distinguishing.
    let shape = if empty == spans.len() {
        " · all empty (cursors, not ranges)".to_string()
    } else if empty > 0 {
        format!(" · {empty} empty")
    } else {
        String::new()
    };

    let primary_note = match spans.get(primary) {
        Some(span) => format!(
            " · primary #{} line {}, {} chars",
            primary,
            span.line + 1,
            width(span)
        ),
        None => String::new(),
    };

    format!(
        "{} selections{direction}{primary_note}{shape} · {total} chars total",
        spans.len()
    )
}

/// `:cursors` — describe every selection.
fn cursors(host: &Host, _args: &Args) -> c_int {
    let spans = host.selections();
    if spans.is_empty() {
        host.error("cursors: no active buffer");
        return 1;
    }
    // The primary selection is the one the SDK reports at index 0 through
    // `selection(0)`; `selections()` returns them in document order.
    host.message(&summary(&spans, 0));
    0
}

declare_plugin! {
    name: "multi-cursor",
    version: "0.1.0",
    commands: { "cursors" => cursors },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(anchor: usize, head: usize, line: usize) -> Span {
        Span {
            anchor,
            head,
            line,
            valid: 1,
        }
    }

    /// A backwards selection has the same extent as a forwards one over the
    /// same text — ordering the pair is what makes slicing safe.
    #[test]
    fn extent_is_direction_independent() {
        let forwards = span(10, 20, 0);
        let backwards = span(20, 10, 0);
        assert_eq!(extent(&forwards), (10, 20));
        assert_eq!(extent(&backwards), (10, 20), "same extent");
        assert_eq!(width(&forwards), width(&backwards));
    }

    /// Direction is preserved and reported, because it says which end the user
    /// is extending from — information vim's ordered marks throw away.
    #[test]
    fn direction_survives_because_it_means_something() {
        assert!(is_backwards(&span(20, 10, 0)));
        assert!(!is_backwards(&span(10, 20, 0)));
        assert!(!is_backwards(&span(10, 10, 0)), "empty is not backwards");
    }

    /// Backwards selections are counted and called out.
    #[test]
    fn backwards_selections_are_counted() {
        let spans = [span(10, 20, 0), span(40, 30, 1), span(60, 50, 2)];
        let line = summary(&spans, 0);
        assert!(line.contains("3 selections"));
        assert!(line.contains("2 backwards"));
        assert!(line.contains("30 chars total"), "10 + 10 + 10");
    }

    /// All-empty selections are multiple CURSORS rather than multiple ranges,
    /// which is a different editing situation and reads differently.
    #[test]
    fn all_empty_selections_are_cursors_not_ranges() {
        let cursors = [span(5, 5, 0), span(15, 15, 1)];
        let line = summary(&cursors, 0);
        assert!(line.contains("all empty (cursors, not ranges)"));
        assert!(line.contains("0 chars total"));

        let mixed = [span(5, 5, 0), span(10, 20, 1)];
        assert!(summary(&mixed, 0).contains("1 empty"));
    }

    /// The primary selection is named with a 1-based line for a human, and its
    /// own width rather than the total.
    #[test]
    fn the_primary_selection_is_described() {
        let spans = [span(100, 109, 11), span(200, 210, 20)];
        let line = summary(&spans, 0);
        assert!(line.contains("primary #0"));
        assert!(line.contains("line 12"), "0-based 11 shown as 12");
        assert!(line.contains("9 chars"), "its own width, not the total");
    }

    /// An empty list is stated rather than rendered as "0 selections".
    #[test]
    fn no_selections_is_stated() {
        assert_eq!(summary(&[], 0), "no selections");
    }
}
