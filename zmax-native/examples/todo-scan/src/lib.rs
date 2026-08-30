//! Example plugin: find the TODO markers, and walk them.
//!
//! A working tool rather than an API tour: two commands that share one scan.
//!
//! Demonstrates driving [`Host::search_next`] as a LOOP, which needs one care
//! the single-shot calls do not. `search_next` returns the match at or after an
//! offset, so feeding it the match it just returned yields that same match
//! forever. Every step must advance past the previous anchor, and the loop
//! needs a ceiling regardless, since a pattern that can match empty would
//! otherwise never terminate.
//!
//! Jumping uses `:goto`, which is **1-based** — the SDK's lines are 0-based, so
//! the conversion happens once, in one place, rather than at each call site.
//!
//! ```text
//! :plugin load .../libzmax_native_todo_scan.dylib
//! :todo        # → "7 markers: 4 TODO, 2 FIXME, 1 XXX — first at line 12"
//! :todo-next   # jump to the next one after the cursor, wrapping
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// The markers looked for, as one Rust regex. These are Rust regexes, not vim
/// ones, so alternation is plain `|` with no backslashes.
const PATTERN: &str = r"\b(TODO|FIXME|XXX|HACK)\b";

/// A ceiling on the scan, so a pathological pattern cannot hang the editor.
/// Reported when hit rather than silently truncating the count.
const MAX_MARKERS: usize = 1000;

/// Which marker a line holds, if any. Checked longest-first so `XXX` inside a
/// longer word cannot shadow a real marker earlier in the line.
fn marker_in(text: &str) -> Option<&'static str> {
    ["FIXME", "TODO", "HACK", "XXX"]
        .into_iter()
        .find(|marker| text.contains(marker))
}

/// The next offset to search from, given the match just found.
///
/// Always strictly greater than the anchor: handing `search_next` its own
/// result back would return the same match forever.
fn advance_past(anchor: usize) -> usize {
    anchor + 1
}

/// Counts per marker, in a stable order so the summary does not reshuffle
/// between runs.
fn tally(markers: &[&str]) -> Vec<(&'static str, usize)> {
    ["TODO", "FIXME", "XXX", "HACK"]
        .into_iter()
        .filter_map(|name| {
            let count = markers.iter().filter(|m| **m == name).count();
            (count > 0).then_some((name, count))
        })
        .collect()
}

/// The summary line.
fn summary(
    counts: &[(&str, usize)],
    total: usize,
    first_line: Option<usize>,
    capped: bool,
) -> String {
    if total == 0 {
        return "no TODO markers in this buffer".to_string();
    }
    let breakdown = counts
        .iter()
        .map(|(name, n)| format!("{n} {name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let first = match first_line {
        Some(line) => format!(" — first at line {}", line + 1),
        None => String::new(),
    };
    let note = if capped { " (scan capped)" } else { "" };
    format!("{total} markers: {breakdown}{first}{note}")
}

/// Every marker position in the buffer, as (char offset, 0-based line).
///
/// Shared by both commands so they can never disagree about what counts as a
/// marker.
fn scan(host: &Host) -> (Vec<(usize, usize)>, bool) {
    let mut found = Vec::new();
    let mut from = 0usize;
    let mut capped = false;
    while let Some(span) = host.search_next(PATTERN, from) {
        if found.len() >= MAX_MARKERS {
            capped = true;
            break;
        }
        found.push((span.anchor, span.line));
        // Strictly past the anchor, or this loops on one match forever.
        from = advance_past(span.anchor);
    }
    (found, capped)
}

/// `:todo` — count and break down the markers in the buffer.
fn todo(host: &Host, _args: &Args) -> c_int {
    let (found, capped) = scan(host);
    let markers: Vec<&str> = found
        .iter()
        .filter_map(|(_offset, line)| host.line(*line).and_then(|text| marker_in(&text)))
        .collect();

    host.message(&summary(
        &tally(&markers),
        found.len(),
        found.first().map(|(_offset, line)| *line),
        capped,
    ));
    0
}

/// `:todo-next` — jump to the first marker after the cursor, wrapping to the
/// top when there is none.
fn todo_next(host: &Host, _args: &Args) -> c_int {
    let cursor = host.cursor().map(|c| c.offset).unwrap_or(0);
    let (found, _capped) = scan(host);

    let Some((_offset, line)) = found
        .iter()
        .find(|(offset, _line)| *offset > cursor)
        .or_else(|| found.first())
    else {
        host.message("no TODO markers in this buffer");
        return 0;
    };

    // `:goto` counts from 1; the SDK counts from 0. Converted once, here.
    host.eval(&format!("goto {}", line + 1));
    0
}

declare_plugin! {
    name: "todo-scan",
    version: "0.1.0",
    commands: {
        "todo" => todo,
        "todo-next" => todo_next,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scan must move strictly forward, or `search_next` returns the same
    /// match for ever. This is the loop's whole correctness condition.
    #[test]
    fn the_scan_always_moves_forward() {
        assert!(advance_past(0) > 0);
        assert!(advance_past(41) > 41);
    }

    /// A line's marker is identified by name, and a line without one is not a
    /// marker at all.
    #[test]
    fn a_line_reports_its_marker() {
        assert_eq!(marker_in("// TODO: fix this"), Some("TODO"));
        assert_eq!(marker_in("// FIXME urgently"), Some("FIXME"));
        assert_eq!(marker_in("ordinary code"), None);
    }

    /// Counts come back in a fixed order, and markers that do not appear are
    /// omitted rather than listed as zero.
    #[test]
    fn the_tally_is_ordered_and_omits_zeroes() {
        let markers = ["TODO", "FIXME", "TODO", "XXX", "TODO"];
        assert_eq!(tally(&markers), vec![("TODO", 3), ("FIXME", 1), ("XXX", 1)]);
        assert!(
            !tally(&markers).iter().any(|(name, _)| *name == "HACK"),
            "absent markers are not listed"
        );
    }

    /// The first marker is reported 1-based for a human, matching what `:goto`
    /// expects and what the editor shows in the gutter.
    #[test]
    fn the_first_marker_is_reported_one_based() {
        let line = summary(&[("TODO", 1)], 1, Some(11), false);
        assert!(line.contains("first at line 12"), "0-based 11 shown as 12");
    }

    /// Hitting the ceiling is disclosed rather than silently truncating, so a
    /// count that is not the whole truth never reads as if it were.
    #[test]
    fn a_capped_scan_says_so() {
        let capped = summary(&[("TODO", 1000)], 1000, Some(0), true);
        assert!(capped.contains("scan capped"));
        assert!(!summary(&[("TODO", 2)], 2, Some(0), false).contains("capped"));
    }

    /// An empty buffer says so instead of reporting "0 markers:".
    #[test]
    fn no_markers_is_stated_plainly() {
        assert_eq!(
            summary(&[], 0, None, false),
            "no TODO markers in this buffer"
        );
    }
}
