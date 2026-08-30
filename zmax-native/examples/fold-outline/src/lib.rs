//! Example plugin: the buffer's structure, from its folds.
//!
//! Fold levels already encode the nesting the file has, so an outline can be
//! read straight out of them without a second parse:
//!
//! - [`Host::fold_level`] — how deeply a line is nested. Level 0 is top level.
//! - [`Host::fold_closed`] — the first line of the closed fold containing this
//!   one, or `None` when it is not inside a closed fold.
//!
//! The second is what makes a line INVISIBLE. A line inside a closed fold still
//! has a level and still exists; it is simply not on screen. Every line of one
//! closed fold reports the same first line, which is how the whole run collapses
//! to a single entry here rather than repeating.
//!
//! ```text
//! :plugin load .../libzmax_native_fold_outline.dylib
//! :outline   # → "12 folds, deepest 3 · 2 closed hiding 47 lines"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// Where a fold begins: the first line at a deeper level than the one before.
///
/// Reading starts from level changes rather than from a grammar, so this works
/// for any language the editor can fold at all.
fn fold_starts(levels: &[usize]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut previous = 0usize;
    for (line, &level) in levels.iter().enumerate() {
        if level > previous {
            starts.push((line, level));
        }
        previous = level;
    }
    starts
}

/// Distinct closed folds, and how many lines they hide between them.
///
/// Every line of a closed fold reports the same first line, so counting rows
/// would multiply-count one fold. Collapsing on that first line is what makes
/// the count mean "folds" rather than "hidden lines".
fn closed_folds(closed: &[Option<usize>]) -> (usize, usize) {
    let mut seen: Vec<usize> = Vec::new();
    let mut hidden = 0usize;
    for entry in closed.iter().flatten() {
        hidden += 1;
        if !seen.contains(entry) {
            seen.push(*entry);
        }
    }
    (seen.len(), hidden)
}

/// The summary line.
fn summary(starts: &[(usize, usize)], closed: (usize, usize)) -> String {
    if starts.is_empty() {
        return "no folds in this buffer".to_string();
    }
    let deepest = starts
        .iter()
        .map(|(_line, level)| *level)
        .max()
        .unwrap_or(0);
    let (folds, hidden) = closed;
    let closed_note = if folds == 0 {
        "none closed".to_string()
    } else {
        format!("{folds} closed hiding {hidden} lines")
    };
    format!("{} folds, deepest {deepest} · {closed_note}", starts.len())
}

/// `:outline` — summarise the buffer's fold structure.
fn outline(host: &Host, _args: &Args) -> c_int {
    let count = host.line_count();
    let levels: Vec<usize> = (0..count).map(|line| host.fold_level(line)).collect();
    let closed: Vec<Option<usize>> = (0..count).map(|line| host.fold_closed(line)).collect();

    host.message(&summary(&fold_starts(&levels), closed_folds(&closed)));
    0
}

declare_plugin! {
    name: "fold-outline",
    version: "0.1.0",
    commands: { "outline" => outline },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fold starts where the level DEEPENS; staying at a level is inside the
    /// same fold, not a new one.
    #[test]
    fn a_fold_starts_where_the_level_deepens() {
        //             0  1  1  2  2  1  0
        let levels = [0, 1, 1, 2, 2, 1, 0];
        let starts = fold_starts(&levels);
        assert_eq!(starts, vec![(1, 1), (3, 2)], "two starts, not four");
    }

    /// Returning to a level and deepening again is a second fold, not a
    /// continuation of the first.
    #[test]
    fn reopening_a_level_starts_a_new_fold() {
        let levels = [0, 1, 0, 1, 0];
        assert_eq!(fold_starts(&levels), vec![(1, 1), (3, 1)]);
    }

    /// Every line of a closed fold names the same first line, so the fold is
    /// counted once while its hidden lines are counted individually.
    #[test]
    fn one_closed_fold_counts_once_but_hides_many() {
        // Lines 5..8 all belong to the fold starting at 5.
        let closed = [None, None, None, None, None, Some(5), Some(5), Some(5)];
        assert_eq!(
            closed_folds(&closed),
            (1, 3),
            "one fold, three hidden lines"
        );
    }

    /// Two closed folds are two, even though each contributes several lines.
    #[test]
    fn separate_closed_folds_are_counted_separately() {
        let closed = [Some(1), Some(1), None, Some(7), Some(7), Some(7)];
        assert_eq!(closed_folds(&closed), (2, 5));
    }

    /// A buffer with no folds says so rather than reporting zeroes.
    #[test]
    fn a_flat_buffer_says_so() {
        assert_eq!(summary(&[], (0, 0)), "no folds in this buffer");
        let open = summary(&[(1, 1), (4, 2)], (0, 0));
        assert!(open.contains("2 folds"));
        assert!(open.contains("deepest 2"));
        assert!(open.contains("none closed"));
    }
}
