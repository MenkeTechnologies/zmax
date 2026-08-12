//! Emacs `highlight-changes-mode` (hilit-chg.el): "a minor mode that uses faces
//! to indicate which parts of the buffer were changed most recently".
//!
//! Emacs puts a `hilit-chg` text property on inserted text as it is typed, and a
//! `hilit-chg-delete` property on the first character *after* text has been
//! deleted (the deleted characters are gone, so there is nothing else to mark).
//!
//! zmax reaches the same display from the other end: turning the mode on for a
//! buffer snapshots its rope, and the renderer diffs the snapshot against the
//! live text. The changed regions are exactly the ones Emacs' incremental
//! properties would have accumulated, and they survive undo, external reloads
//! and multi-cursor edits without a per-edit hook. [`changed_ranges`] is the
//! pure diff-to-ranges step and is unit tested.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Mutex;

use zmax_core::Rope;
use zmax_view::DocumentId;

/// The buffers the mode is on for, each with the text it was turned on over
/// (Emacs re-bases this on `highlight-changes-rotate-faces`; zmax re-bases it
/// when the buffer is saved, which is the point at which "changed" stops being
/// interesting).
fn state() -> &'static Mutex<HashMap<DocumentId, Rope>> {
    static STATE: std::sync::OnceLock<Mutex<HashMap<DocumentId, Rope>>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Whether `highlight-changes-mode` is on for `doc`.
pub fn enabled(doc: DocumentId) -> bool {
    state().lock().map(|s| s.contains_key(&doc)).unwrap_or(false)
}

/// Emacs `M-x highlight-changes-mode`: toggle it for `doc`, snapshotting `text`
/// as the baseline when turning it on. Returns the new state.
pub fn toggle(doc: DocumentId, text: &Rope) -> bool {
    let Ok(mut state) = state().lock() else {
        return false;
    };
    if state.remove(&doc).is_some() {
        false
    } else {
        state.insert(doc, text.clone());
        true
    }
}

/// Re-baseline `doc` to `text` — the buffer has been saved, so the changes
/// accumulated so far are no longer "recent". A buffer the mode is off for is
/// left alone.
pub fn rebase(doc: DocumentId, text: &Rope) {
    if let Ok(mut state) = state().lock() {
        if let Some(base) = state.get_mut(&doc) {
            *base = text.clone();
        }
    }
}

/// Forget `doc` entirely (the buffer is being closed).
pub fn forget(doc: DocumentId) {
    if let Ok(mut state) = state().lock() {
        state.remove(&doc);
    }
}

/// The changed char ranges in `doc`'s current `text`, or `None` when the mode is
/// off for it. Ranges are ascending and non-overlapping.
pub fn ranges_for(doc: DocumentId, text: &Rope) -> Option<Vec<Range<usize>>> {
    let base = state().lock().ok()?.get(&doc)?.clone();
    Some(changed_ranges(&base, text))
}

/// The char ranges of `after` that differ from `before`.
///
/// Insertions are marked over the inserted characters (Emacs' `hilit-chg`); a
/// deletion leaves no characters to mark, so — like Emacs' `hilit-chg-delete` —
/// the single character that now sits where the deleted text was is marked
/// instead. Pure.
pub fn changed_ranges(before: &Rope, after: &Rope) -> Vec<Range<usize>> {
    let changes = zmax_core::diff::compare_ropes(before, after);
    let len = after.len_chars();
    let mut out: Vec<Range<usize>> = Vec::new();
    // Walk the change set, tracking the position in the *new* text.
    let mut pos = 0usize;
    let push = |range: Range<usize>, out: &mut Vec<Range<usize>>| {
        if range.start >= range.end {
            return;
        }
        match out.last_mut() {
            // Adjacent runs coalesce so an edit that both deleted and inserted
            // shows as one region rather than two.
            Some(last) if last.end >= range.start => last.end = last.end.max(range.end),
            _ => out.push(range),
        }
    };
    // Whether the previous operation inserted text. A delete next to an insert
    // is a *replacement*: the new text already carries the mark, so adding the
    // deletion marker on top of it would spill onto an unchanged character.
    let mut after_insert = false;
    for op in changes.changes().changes() {
        use zmax_core::Operation::*;
        match op {
            Retain(n) => {
                pos += n;
                after_insert = false;
            }
            Insert(s) => {
                let n = s.chars().count();
                push(pos..pos + n, &mut out);
                pos += n;
                after_insert = true;
            }
            // The deleted characters are not in `after`; mark the one that took
            // their place, which is what `hilit-chg-delete` does.
            Delete(_) => {
                if !after_insert {
                    push(pos..(pos + 1).min(len), &mut out);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_insertion_is_marked_over_the_inserted_text() {
        let before = Rope::from_str("hello world");
        let after = Rope::from_str("hello brave world");
        // "brave " was inserted at char 6.
        assert_eq!(changed_ranges(&before, &after), vec![6..12]);
    }

    #[test]
    fn a_deletion_marks_the_character_that_took_its_place() {
        let before = Rope::from_str("hello brave world");
        let after = Rope::from_str("hello world");
        assert_eq!(changed_ranges(&before, &after), vec![6..7]);
    }

    #[test]
    fn an_unchanged_buffer_has_no_ranges() {
        let rope = Rope::from_str("nothing happened here");
        assert!(changed_ranges(&rope, &rope).is_empty());
    }

    #[test]
    fn a_deletion_at_the_very_end_marks_nothing_out_of_bounds() {
        let before = Rope::from_str("abcdef");
        let after = Rope::from_str("abc");
        let ranges = changed_ranges(&before, &after);
        assert!(ranges.iter().all(|r| r.end <= after.len_chars()));
    }

    #[test]
    fn two_separate_edits_produce_two_ranges() {
        let before = Rope::from_str("aaa\nbbb\nccc");
        let after = Rope::from_str("aXa\nbbb\ncYc");
        let ranges = changed_ranges(&before, &after);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], 1..2);
        assert_eq!(ranges[1], 9..10);
    }
}
