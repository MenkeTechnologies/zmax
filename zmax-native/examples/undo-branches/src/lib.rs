//! Example plugin: the undo history is a TREE, and this shows the shape of it.
//!
//! Undoing and then typing again does not discard the old future — it branches
//! away from it. That work is still reachable, and this is how you find out it
//! is there.
//!
//! [`Host::undo_tree`] reports a parent per revision, which is what makes the
//! structure visible. Two revisions sharing a parent is a branch; the revision
//! you are on is one leaf among possibly several.
//!
//! Revision 0 is the empty root and is its OWN parent, which is what lets
//! [`UndoTree::ancestry`] terminate without a sentinel — a malformed history
//! cannot make it spin.
//!
//! ```text
//! :plugin load .../libzmax_native_undo_branches.dylib
//! :undo-tree   # → "23 revisions, 3 branches · on 21, depth 7 · 2 revisions off the saved path"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host, UndoRevision, UndoTree};

/// Revisions with more than one child: the points where the history forked.
///
/// The root is excluded as a parent of itself, or every history would report
/// one spurious branch at revision 0.
fn branch_points(revisions: &[UndoRevision]) -> usize {
    let mut children = vec![0usize; revisions.len()];
    for (index, revision) in revisions.iter().enumerate() {
        // Skip the root's self-parent link, which is structural rather than a
        // real parent-child edge.
        if revision.parent != index && revision.parent < children.len() {
            children[revision.parent] += 1;
        }
    }
    children.iter().filter(|count| **count > 1).count()
}

/// How far the current revision is from the saved one, measured along the tree
/// rather than by subtracting indices.
///
/// Revision numbers are creation order, so `current - saved` is meaningless
/// across a branch: the saved revision may not be an ancestor of the current
/// one at all. Walking the ancestry is the only honest answer.
fn distance_from_saved(tree: &UndoTree) -> Option<usize> {
    let path = tree.ancestry(tree.current);
    path.iter().position(|revision| *revision == tree.saved)
}

/// The summary line.
fn summary(tree: &UndoTree, branches: usize, depth: usize, from_saved: Option<usize>) -> String {
    if tree.revisions.len() <= 1 {
        return "nothing to undo yet".to_string();
    }
    let branch_note = match branches {
        0 => "no branches".to_string(),
        1 => "1 branch".to_string(),
        n => format!("{n} branches"),
    };
    let saved_note = match from_saved {
        Some(0) => " · saved".to_string(),
        Some(n) => format!(" · {n} revisions off the saved point"),
        // The saved revision is not an ancestor: the buffer sits on a different
        // branch entirely, and no number of undos walks back to it.
        None => " · on a different branch from the saved revision".to_string(),
    };
    format!(
        "{} revisions, {branch_note} · on {}, depth {depth}{saved_note}",
        tree.revisions.len(),
        tree.current,
    )
}

/// `:undo-tree` — the shape of the undo history.
fn undo_tree(host: &Host, _args: &Args) -> c_int {
    let tree = host.undo_tree();
    let branches = branch_points(&tree.revisions);
    // Ancestry includes the current revision and the root, so depth is the
    // number of steps between them.
    let depth = tree.ancestry(tree.current).len().saturating_sub(1);

    host.message(&summary(&tree, branches, depth, distance_from_saved(&tree)));
    0
}

declare_plugin! {
    name: "undo-branches",
    version: "0.1.0",
    commands: { "undo-tree" => undo_tree },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(parents: &[usize], current: usize, saved: usize) -> UndoTree {
        UndoTree {
            revisions: parents
                .iter()
                .map(|parent| UndoRevision {
                    parent: *parent,
                    seconds_ago: 0,
                })
                .collect(),
            current,
            saved,
        }
    }

    /// A linear history has no branches, however long it is.
    #[test]
    fn a_linear_history_has_no_branches() {
        let linear = tree(&[0, 0, 1, 2, 3], 4, 4);
        assert_eq!(branch_points(&linear.revisions), 0);
    }

    /// The root's self-parent is structural, not an edge — counting it would
    /// report a branch in every history that exists.
    #[test]
    fn the_roots_self_parent_is_not_a_branch() {
        let single = tree(&[0], 0, 0);
        assert_eq!(branch_points(&single.revisions), 0);
        // Root with two real children is still only one branch point.
        let forked = tree(&[0, 0, 0], 2, 1);
        assert_eq!(branch_points(&forked.revisions), 1);
    }

    /// Two revisions sharing a parent is a branch — the shape produced by
    /// undoing and then editing again.
    #[test]
    fn shared_parents_are_branches() {
        // 3 and 4 both descend from 2.
        let branched = tree(&[0, 0, 1, 2, 2], 4, 3);
        assert_eq!(branch_points(&branched.revisions), 1);
    }

    /// Distance is measured along the tree, not by subtracting revision
    /// numbers — those are creation order and say nothing across a branch.
    #[test]
    fn distance_is_measured_along_the_ancestry() {
        // 0 → 1 → 2 → 3, saved at 1, sitting on 3: two steps back.
        let linear = tree(&[0, 0, 1, 2], 3, 1);
        assert_eq!(distance_from_saved(&linear), Some(2));
        assert_eq!(
            distance_from_saved(&tree(&[0, 0, 1], 2, 2)),
            Some(0),
            "saved"
        );
    }

    /// When the saved revision is on another branch it is NOT an ancestor, and
    /// no number of undos reaches it. Subtracting indices would have produced a
    /// confident, wrong number here.
    #[test]
    fn a_saved_revision_on_another_branch_is_unreachable() {
        // 3 and 4 both descend from 2; saved on 3, sitting on 4.
        let branched = tree(&[0, 0, 1, 2, 2], 4, 3);
        assert_eq!(distance_from_saved(&branched), None);

        let line = summary(&branched, 1, 3, None);
        assert!(line.contains("different branch"));
    }

    /// A fresh buffer says so rather than reporting a one-revision tree.
    #[test]
    fn a_fresh_buffer_has_nothing_to_undo() {
        assert_eq!(
            summary(&tree(&[0], 0, 0), 0, 0, Some(0)),
            "nothing to undo yet"
        );
    }
}
