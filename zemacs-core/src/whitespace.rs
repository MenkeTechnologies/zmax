//! Pure-Rust whitespace / spacing transforms — a faithful port of the GNU Emacs
//! spacing commands (`tabify`, `untabify`, `delete-trailing-whitespace`,
//! `just-one-space`, `delete-horizontal-space`, `cycle-spacing`).
//!
//! Like [`crate::region_ops`] and [`crate::text_engine`], everything here is a
//! plain function over `&str` with no editor types leaking in, so each is unit
//! tested in isolation. The command layer extracts the live selection's line span
//! (for `tabify`/`untabify`/`delete-trailing-whitespace`) or the run of blanks
//! around point (for the point-local helpers), calls one of these, and applies the
//! result as a single undoable transaction.
//!
//! Column-oriented transforms (`tabify`/`untabify`) treat a TAB as advancing to the
//! next multiple of `tab_width`, matching Emacs' `current-column`/`indent-to` and
//! Vim's `tabstop`.

// ---------------------------------------------------------------------------
// Tabs -> spaces — Emacs `untabify`, Vim `:retab` (expandtab).
// ---------------------------------------------------------------------------

/// Expand every TAB in `line` to spaces, honoring column stops of width
/// `tab_width` (a TAB advances to the next multiple of `tab_width`). Non-TAB
/// characters — including interior spaces — are copied verbatim. This is the exact
/// inverse target of [`tabify`]: `untabify(&tabify(line, tw), tw)` reproduces the
/// all-spaces form of `line`.
///
/// `tab_width` of 0 is treated as 1 (a degenerate but well-defined stop width),
/// mirroring [`crate::text_engine::untabify`].
pub fn untabify(line: &str, tab_width: usize) -> String {
    let tw = tab_width.max(1);
    let mut out = String::new();
    let mut col = 0usize;
    for c in line.chars() {
        if c == '\t' {
            let n = tw - (col % tw);
            for _ in 0..n {
                out.push(' ');
            }
            col += n;
        } else {
            out.push(c);
            // Columns are counted in characters; this matches Emacs for ordinary
            // text and keeps the transform a pure inverse of `tabify`.
            col += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Spaces -> tabs — Emacs `tabify`.
// ---------------------------------------------------------------------------

/// Convert runs of blanks in `line` to TABs wherever this does not change the
/// column the run ends at — the Emacs `tabify` transform.
///
/// Faithful to `tabify.el`: only maximal runs of **two or more** blanks (spaces or
/// tabs, the default `tabify-regexp` `"[ \t][ \t]+"`) are candidates; each such run
/// is replaced by the output of `indent-to` from the run's start column to its end
/// column, i.e. as many TABs as fit between tab stops followed by the remaining
/// spaces. Lone blanks are left untouched, so interior single spaces between words
/// survive.
pub fn tabify(line: &str, tab_width: usize) -> String {
    let tw = tab_width.max(1);
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut col = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == ' ' || c == '\t' {
            // Consume the maximal run of blanks, tracking the end column.
            let start_col = col;
            let mut j = i;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                col += if chars[j] == '\t' { tw - (col % tw) } else { 1 };
                j += 1;
            }
            let run_len = j - i;
            if run_len >= 2 {
                // Rebuild the run with `indent-to`: optimal TABs then spaces.
                indent_to(&mut out, start_col, col, tw);
            } else {
                // A lone blank is preserved verbatim (may itself be a TAB).
                out.push(chars[i]);
            }
            i = j;
        } else {
            out.push(c);
            col += 1;
            i += 1;
        }
    }
    out
}

/// Emacs `indent-to`: append TABs then spaces to `out` so the column advances from
/// `from_col` to `to_col`, preferring TABs (`indent-tabs-mode` t) up to the last
/// tab stop that does not overshoot, then padding with spaces.
fn indent_to(out: &mut String, from_col: usize, to_col: usize, tw: usize) {
    let mut col = from_col;
    loop {
        let next_stop = (col / tw + 1) * tw;
        if next_stop <= to_col {
            out.push('\t');
            col = next_stop;
        } else {
            break;
        }
    }
    while col < to_col {
        out.push(' ');
        col += 1;
    }
}

// ---------------------------------------------------------------------------
// Trailing whitespace — Emacs `delete-trailing-whitespace`.
// ---------------------------------------------------------------------------

/// Strip the run of trailing spaces/tabs from a single logical `line` (which must
/// not contain a line ending). The building block used per-line by the
/// `delete-trailing-whitespace` command.
pub fn delete_trailing_whitespace_line(line: &str) -> &str {
    line.trim_end_matches([' ', '\t'])
}

// ---------------------------------------------------------------------------
// Point-local spacing helpers — Emacs `just-one-space` (M-SPC),
// `delete-horizontal-space` (M-\\), `cycle-spacing`.
// ---------------------------------------------------------------------------

/// The maximal run of horizontal blanks (spaces/tabs, never newlines) surrounding
/// the character position `idx` in `chars`, returned as the half-open char range
/// `[start, end)`. When `idx` sits outside any blank run the range is empty
/// (`start == end == idx`). `idx` is clamped to `chars.len()`.
///
/// This is the shared bound used by [`just_one_space`], [`delete_horizontal_space`]
/// and the `cycle-spacing` command; the term-layer equivalent scans the rope
/// directly with the same `' ' | '\t'` predicate.
pub fn horizontal_space_run(chars: &[char], idx: usize) -> (usize, usize) {
    let idx = idx.min(chars.len());
    let mut start = idx;
    while start > 0 && matches!(chars[start - 1], ' ' | '\t') {
        start -= 1;
    }
    let mut end = idx;
    while end < chars.len() && matches!(chars[end], ' ' | '\t') {
        end += 1;
    }
    (start, end)
}

/// Emacs `just-one-space` (M-SPC): collapse the run of spaces/tabs around `idx` to
/// exactly `n` spaces, returning the rewritten string and the new cursor position
/// (char index just past the inserted spaces). With no surrounding blanks this
/// inserts `n` spaces at `idx`. `n` counts spaces to leave (Emacs' prefix arg,
/// default 1).
pub fn just_one_space(s: &str, idx: usize, n: usize) -> (String, usize) {
    let chars: Vec<char> = s.chars().collect();
    let (start, end) = horizontal_space_run(&chars, idx);
    let mut out: String = chars[..start].iter().collect();
    for _ in 0..n {
        out.push(' ');
    }
    out.extend(&chars[end..]);
    (out, start + n)
}

/// Emacs `delete-horizontal-space` (M-\\): delete all spaces/tabs around `idx`,
/// returning the rewritten string and the new cursor position. With `backward_only`
/// (the `C-u` prefix form) only the blanks *before* `idx` are removed; the blanks
/// after point are left in place.
pub fn delete_horizontal_space(s: &str, idx: usize, backward_only: bool) -> (String, usize) {
    let chars: Vec<char> = s.chars().collect();
    let (start, run_end) = horizontal_space_run(&chars, idx);
    let end = if backward_only {
        idx.min(chars.len())
    } else {
        run_end
    };
    let mut out: String = chars[..start].iter().collect();
    out.extend(&chars[end..]);
    (out, start)
}

/// The three states of Emacs `cycle-spacing`, advanced on each consecutive call.
///
/// A first invocation collapses the surrounding blanks to a single space
/// ([`CycleSpacing::JustOne`]); calling it again with point unmoved deletes all the
/// blanks ([`CycleSpacing::None`]); a third call restores the original whitespace
/// verbatim ([`CycleSpacing::Restore`]) and the cycle repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleSpacing {
    /// Leave exactly one space (like `just-one-space`).
    JustOne,
    /// Delete every surrounding blank.
    None,
    /// Put the original whitespace run back.
    Restore,
}

impl CycleSpacing {
    /// The phase a fresh (non-consecutive) `cycle-spacing` invocation starts in.
    pub fn first() -> Self {
        CycleSpacing::JustOne
    }

    /// The phase that follows this one when the command is repeated in place.
    pub fn next(self) -> Self {
        match self {
            CycleSpacing::JustOne => CycleSpacing::None,
            CycleSpacing::None => CycleSpacing::Restore,
            CycleSpacing::Restore => CycleSpacing::JustOne,
        }
    }
}

/// Apply one `cycle-spacing` `phase` to the blanks around `idx`. `original` is the
/// exact whitespace text of the run captured on the first invocation, needed to
/// reproduce it in the [`CycleSpacing::Restore`] phase. Returns the rewritten
/// string and the new cursor position.
pub fn cycle_spacing(s: &str, idx: usize, phase: CycleSpacing, original: &str) -> (String, usize) {
    match phase {
        CycleSpacing::JustOne => just_one_space(s, idx, 1),
        CycleSpacing::None => delete_horizontal_space(s, idx, false),
        CycleSpacing::Restore => {
            let chars: Vec<char> = s.chars().collect();
            let (start, end) = horizontal_space_run(&chars, idx);
            let mut out: String = chars[..start].iter().collect();
            out.push_str(original);
            out.extend(&chars[end..]);
            (out, start + original.chars().count())
        }
    }
}

/// The whitespace run around `idx` as an owned string — captured by the
/// `cycle-spacing` command before its first transform so a later
/// [`CycleSpacing::Restore`] can reproduce it exactly.
pub fn horizontal_space_text(s: &str, idx: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let (start, end) = horizontal_space_run(&chars, idx);
    chars[start..end].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untabify_expands_to_stops() {
        assert_eq!(untabify("\tx", 4), "    x");
        assert_eq!(untabify("a\tb", 4), "a   b"); // TAB from col 1 -> col 4
        assert_eq!(untabify("ab\tc", 4), "ab  c"); // col 2 -> col 4
        assert_eq!(untabify("abcd\te", 4), "abcd    e"); // exactly on a stop -> full tab
        assert_eq!(untabify("no tabs", 4), "no tabs");
        assert_eq!(untabify("\t", 8), "        ");
    }

    #[test]
    fn untabify_tab_width_zero_is_one() {
        assert_eq!(untabify("\t\tx", 0), "  x");
    }

    #[test]
    fn tabify_collapses_runs_at_stops() {
        assert_eq!(tabify("        x", 4), "\t\tx"); // 8 spaces = 2 tabs
        assert_eq!(tabify("     x", 4), "\t x"); // 5 spaces = 1 tab + 1 space
        assert_eq!(tabify("    x", 4), "\tx"); // exactly one stop
                                               // A lone space between words is NOT a candidate run (needs 2+ blanks).
        assert_eq!(tabify("a b", 4), "a b");
        // A run of >=2 interior spaces IS converted, from its own start column.
        assert_eq!(tabify("a       b", 4), "a\t\tb"); // col1..col8 -> tabs to 4 then 8
    }

    #[test]
    fn tabify_untabify_round_trip() {
        // untabify(tabify(x)) reproduces the all-spaces column layout for any width.
        for tw in [1usize, 2, 4, 8] {
            for line in [
                "        x",
                "     x",
                "a       b   c",
                "no runs here",
                "\t\talready tabbed",
                "   ",
            ] {
                let spaces = untabify(line, tw);
                let round = untabify(&tabify(line, tw), tw);
                assert_eq!(round, spaces, "tw={tw} line={line:?}");
            }
        }
    }

    #[test]
    fn tabify_preserves_end_columns() {
        // Every produced run must reach the same column as the space form.
        let line = "a   b     c";
        assert_eq!(untabify(&tabify(line, 4), 4), untabify(line, 4));
    }

    #[test]
    fn delete_trailing_line() {
        assert_eq!(delete_trailing_whitespace_line("abc   "), "abc");
        assert_eq!(delete_trailing_whitespace_line("abc\t \t"), "abc");
        assert_eq!(delete_trailing_whitespace_line("   "), "");
        assert_eq!(delete_trailing_whitespace_line("abc"), "abc");
        assert_eq!(delete_trailing_whitespace_line("a b c "), "a b c");
    }

    #[test]
    fn horizontal_run_bounds() {
        let c: Vec<char> = "a   b".chars().collect();
        assert_eq!(horizontal_space_run(&c, 2), (1, 4)); // inside the run
        assert_eq!(horizontal_space_run(&c, 1), (1, 4)); // at the run's left edge
        assert_eq!(horizontal_space_run(&c, 4), (1, 4)); // at the run's right edge
        assert_eq!(horizontal_space_run(&c, 0), (0, 0)); // on non-blank -> empty
        assert_eq!(horizontal_space_run(&c, 99), (5, 5)); // clamped past end
    }

    #[test]
    fn just_one_space_collapses() {
        assert_eq!(just_one_space("a   b", 2, 1), ("a b".to_string(), 2));
        assert_eq!(just_one_space("a\t \tb", 2, 1), ("a b".to_string(), 2));
        // No surrounding blanks: insert one space at point.
        assert_eq!(just_one_space("ab", 1, 1), ("a b".to_string(), 2));
        // N > 1 leaves N spaces (prefix arg).
        assert_eq!(just_one_space("a     b", 3, 3), ("a   b".to_string(), 4));
        // At the edge of the run collapses the whole run.
        assert_eq!(just_one_space("a   b", 1, 1), ("a b".to_string(), 2));
    }

    #[test]
    fn delete_horizontal_space_both_and_backward() {
        assert_eq!(
            delete_horizontal_space("a   b", 2, false),
            ("ab".to_string(), 1)
        );
        assert_eq!(
            delete_horizontal_space("a\t \tb", 2, false),
            ("ab".to_string(), 1)
        );
        // backward-only keeps the blanks after point.
        assert_eq!(
            delete_horizontal_space("a   b", 2, true),
            ("a  b".to_string(), 1)
        );
        // No surrounding blanks: no-op at point.
        assert_eq!(
            delete_horizontal_space("ab", 1, false),
            ("ab".to_string(), 1)
        );
    }

    #[test]
    fn cycle_spacing_phases() {
        assert_eq!(CycleSpacing::first(), CycleSpacing::JustOne);
        assert_eq!(CycleSpacing::JustOne.next(), CycleSpacing::None);
        assert_eq!(CycleSpacing::None.next(), CycleSpacing::Restore);
        assert_eq!(CycleSpacing::Restore.next(), CycleSpacing::JustOne);
    }

    #[test]
    fn cycle_spacing_full_cycle() {
        let s = "a   b";
        let orig = horizontal_space_text(s, 2);
        assert_eq!(orig, "   ");
        // Phase 1: collapse to one space.
        let (s1, p1) = cycle_spacing(s, 2, CycleSpacing::JustOne, &orig);
        assert_eq!((s1.as_str(), p1), ("a b", 2));
        // Phase 2: delete all blanks (point now at 2 in the collapsed string).
        let (s2, p2) = cycle_spacing(&s1, 2, CycleSpacing::None, &orig);
        assert_eq!((s2.as_str(), p2), ("ab", 1));
        // Phase 3: restore the original run at point.
        let (s3, p3) = cycle_spacing(&s2, 1, CycleSpacing::Restore, &orig);
        assert_eq!((s3.as_str(), p3), ("a   b", 4));
    }
}
