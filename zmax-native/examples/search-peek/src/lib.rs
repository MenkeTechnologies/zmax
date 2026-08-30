//! Example plugin: how many matches, and where is the next one?
//!
//! Demonstrates the search trio:
//!
//! - [`Host::search_pattern`] — the last search, vim's `/` register, so a bare
//!   `:sc` continues whatever you last looked for.
//! - [`Host::search_count`] — how many matches are in the buffer.
//! - [`Host::search_next`] — the next match at or after an offset.
//!
//! The patterns are **Rust regexes**, not vim regexes: `\v` magic, `\zs`/`\ze`
//! and `\%(` have no meaning here. An invalid pattern counts zero rather than
//! raising, so a count of zero can mean "no matches" or "not a valid regex",
//! and this plugin separates the two by checking the count against a match.
//!
//! ```text
//! :plugin load .../libzmax_native_search_peek.dylib
//! :sc             # the last search
//! :sc fn\s+\w+    # an explicit pattern
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host, Span};

/// What to report for a pattern, given its count and the next match.
///
/// A count of zero with no match is genuinely "nothing here". The SDK cannot
/// distinguish an invalid regex from a pattern that simply does not occur —
/// both count zero — so the wording stays honest about that rather than
/// claiming the pattern was fine.
fn describe(pattern: &str, count: usize, next: Option<Span>, from: usize) -> String {
    if count == 0 {
        return format!("{pattern:?}: no matches (or not a valid regex)");
    }
    match next {
        Some(span) if span.anchor >= from => {
            format!(
                "{pattern:?}: {count} matches, next at line {}",
                span.line + 1
            )
        }
        // Counted matches but none at or after the cursor: they are all behind.
        _ => format!("{pattern:?}: {count} matches, all before the cursor"),
    }
}

/// `:sc [pattern]` — count matches and locate the next one.
fn search_count(host: &Host, args: &Args) -> c_int {
    let pattern = match args.rest().first() {
        Some(explicit) => explicit.clone(),
        None => match host.search_pattern() {
            Some(last) => last,
            None => {
                host.error("sc: no pattern given and no previous search");
                return 1;
            }
        },
    };

    let from = host.cursor().map(|c| c.offset).unwrap_or(0);
    host.message(&describe(
        &pattern,
        host.search_count(&pattern),
        host.search_next(&pattern, from),
        from,
    ));
    0
}

declare_plugin! {
    name: "search-peek",
    version: "0.1.0",
    commands: { "sc" => search_count },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(anchor: usize, line: usize) -> Span {
        Span {
            anchor,
            head: anchor,
            line,
            valid: 1,
        }
    }

    /// Zero matches is reported honestly: the SDK cannot tell "no occurrences"
    /// from "invalid regex", both of which count zero, so the message does not
    /// claim the pattern was valid.
    #[test]
    fn zero_matches_does_not_claim_the_pattern_was_valid() {
        let line = describe("[unclosed", 0, None, 0);
        assert!(line.contains("no matches"));
        assert!(
            line.contains("not a valid regex"),
            "the ambiguity is stated"
        );
    }

    /// A match at or after the cursor is "next"; the line is reported 1-based
    /// for a human reading the status line.
    #[test]
    fn the_next_match_is_reported_one_based() {
        let line = describe("fn", 3, Some(span(100, 41)), 50);
        assert!(line.contains("3 matches"));
        assert!(line.contains("line 42"), "0-based 41 shown as 42");
    }

    /// Matches that all sit behind the cursor are called out, rather than
    /// pointing forward at one that is actually behind.
    #[test]
    fn matches_behind_the_cursor_are_called_out() {
        let line = describe("fn", 2, Some(span(10, 1)), 500);
        assert!(line.contains("all before the cursor"));

        let none_ahead = describe("fn", 2, None, 500);
        assert!(none_ahead.contains("all before the cursor"));
    }
}
