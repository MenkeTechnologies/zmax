//! Example plugin: find lines that are too long — measured properly.
//!
//! "Too long" is a question about SCREEN CELLS, not characters, and the two
//! disagree constantly:
//!
//! - a tab is one character but draws to the next tab stop,
//! - CJK and emoji are one character each but occupy two cells,
//! - combining marks are extra characters that occupy none.
//!
//! Counting `chars()` gets all three wrong. [`Host::display_width`] is the
//! editor's own measurement — the one that decides where the text actually
//! lands — so this plugin uses it and compares against `line_length`, the
//! character count, to show where they diverge.
//!
//! The limit comes from `textwidth` when set, via [`Host::option_num`].
//!
//! ```text
//! :plugin load .../libzmax_native_width_check.dylib
//! :width      # → "3 of 120 lines over 80 cells — worst line 42 at 118 cells (94 chars)"
//! :width 100  # explicit limit
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// vim's own default when `textwidth` is unset — 0 means "no limit" there, so
/// a visible fallback is more useful than reporting nothing.
const DEFAULT_LIMIT: usize = 80;

/// The limit to check against: an explicit argument wins, then `textwidth`,
/// then the fallback.
///
/// `textwidth` of 0 means "no wrapping" in vim, which is not a limit of zero —
/// treating it as one would report every line as too long.
fn limit_from(arg: Option<&str>, textwidth: Option<usize>) -> usize {
    if let Some(explicit) = arg.and_then(|a| a.parse::<usize>().ok()) {
        return explicit;
    }
    match textwidth {
        Some(0) | None => DEFAULT_LIMIT,
        Some(n) => n,
    }
}

/// One over-long line: which, how wide, and how many characters it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Offender {
    line: usize,
    cells: usize,
    chars: usize,
}

/// The widest offender, which is the one worth naming.
fn worst(offenders: &[Offender]) -> Option<Offender> {
    offenders.iter().copied().max_by_key(|o| o.cells)
}

/// The report line. Names the worst offender's character count alongside its
/// cell count, so a line that is over only because of tabs or wide glyphs is
/// visibly different from one that is simply long.
fn report(offenders: &[Offender], total: usize, limit: usize) -> String {
    let Some(worst) = worst(offenders) else {
        return format!("all {total} lines within {limit} cells");
    };
    let note = if worst.cells != worst.chars {
        format!(" ({} chars)", worst.chars)
    } else {
        String::new()
    };
    format!(
        "{} of {total} lines over {limit} cells — worst line {} at {} cells{note}",
        offenders.len(),
        worst.line + 1,
        worst.cells,
    )
}

/// `:width [limit]` — report lines wider than the limit, in screen cells.
fn width(host: &Host, args: &Args) -> c_int {
    let limit = limit_from(
        args.rest().first().map(String::as_str),
        host.option_num("textwidth"),
    );
    let total = host.line_count();

    let offenders: Vec<Offender> = (0..total)
        .filter_map(|line| {
            let text = host.line(line)?;
            // The editor's own measurement, not `text.chars().count()`.
            let cells = host.display_width(&text);
            (cells > limit).then(|| Offender {
                line,
                cells,
                chars: host
                    .line_length(line)
                    .unwrap_or_else(|| text.chars().count()),
            })
        })
        .collect();

    host.message(&report(&offenders, total, limit));
    0
}

declare_plugin! {
    name: "width-check",
    version: "0.1.0",
    commands: { "width" => width },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offender(line: usize, cells: usize, chars: usize) -> Offender {
        Offender { line, cells, chars }
    }

    /// An explicit argument beats the option, and a bad argument falls through
    /// rather than aborting.
    #[test]
    fn an_explicit_limit_wins() {
        assert_eq!(limit_from(Some("100"), Some(80)), 100);
        assert_eq!(limit_from(None, Some(72)), 72, "textwidth");
        assert_eq!(
            limit_from(Some("junk"), Some(72)),
            72,
            "unparseable falls back"
        );
    }

    /// vim's `textwidth=0` means "do not wrap", not "a limit of zero" —
    /// treating it literally would flag every line in the buffer.
    #[test]
    fn textwidth_zero_is_not_a_limit_of_zero() {
        assert_eq!(limit_from(None, Some(0)), DEFAULT_LIMIT);
        assert_eq!(limit_from(None, None), DEFAULT_LIMIT);
    }

    /// The worst line is the widest in CELLS, which need not be the one with
    /// the most characters — that is the entire point of measuring cells.
    #[test]
    fn the_worst_line_is_the_widest_not_the_longest() {
        // Line 2 has fewer characters but is wider on screen (wide glyphs).
        let offenders = [offender(1, 90, 90), offender(2, 118, 60)];
        assert_eq!(worst(&offenders).unwrap().line, 2);
    }

    /// When cells and characters disagree, the report says so — a line over
    /// budget because of tabs reads differently from one that is just long.
    #[test]
    fn a_divergence_between_cells_and_chars_is_shown() {
        let wide = report(&[offender(41, 118, 94)], 120, 80);
        assert!(wide.contains("line 42"), "1-based for humans");
        assert!(wide.contains("118 cells"));
        assert!(wide.contains("(94 chars)"), "the divergence");

        let plain = report(&[offender(0, 90, 90)], 10, 80);
        assert!(!plain.contains("chars"), "no note when they agree");
    }

    /// A clean buffer says so rather than reporting an empty list.
    #[test]
    fn a_clean_buffer_is_stated_plainly() {
        assert_eq!(report(&[], 42, 80), "all 42 lines within 80 cells");
    }
}
