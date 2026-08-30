//! Example plugin: did that command actually change anything?
//!
//! This is what [`Host::change_number`] is for — vim's `changenr()`. A command's
//! exit status tells you it did not FAIL, which is a different question from
//! whether it did anything. `:s/nothing/x/` that matches nothing succeeds and
//! changes nothing.
//!
//! Comparing the change number across an [`Host::eval`] answers the real
//! question, and it is the only way to: the buffer's modified flag stays set if
//! it was already set, and comparing buffer text would mean copying the whole
//! thing twice.
//!
//! ```text
//! :plugin load .../libzmax_native_did_change.dylib
//! :did %s/foo/bar/g   # → "ran, buffer changed (12 → 13), selections 1 → 4"
//! :did noop-command   # → "ran, buffer unchanged"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// What the change number says about a command's effect.
///
/// The number is a revision counter, so any difference means the buffer moved.
/// It can go DOWN — an undo is a change too, landing on an earlier revision —
/// so this compares for inequality rather than for growth.
fn changed(before: usize, after: usize) -> bool {
    before != after
}

/// How the selection changed, which a command can alter without touching text.
fn selection_note(before: usize, after: usize) -> String {
    if before == after {
        String::new()
    } else {
        format!(", selections {before} → {after}")
    }
}

/// The report.
fn report(
    status: c_int,
    before: usize,
    after: usize,
    sel_before: usize,
    sel_after: usize,
) -> String {
    let ran = if status == 0 {
        "ran"
    } else {
        // A failing command may still have changed something before it failed,
        // so the change is reported either way rather than assumed away.
        "failed"
    };
    let effect = if changed(before, after) {
        format!("buffer changed ({before} → {after})")
    } else {
        "buffer unchanged".to_string()
    };
    format!("{ran}, {effect}{}", selection_note(sel_before, sel_after))
}

/// `:did {command…}` — run a command line and report whether it changed the
/// buffer.
fn did(host: &Host, args: &Args) -> c_int {
    let command = args.rest().join(" ");
    if command.trim().is_empty() {
        host.error("did: usage: :did {command}");
        return 1;
    }

    let before = host.change_number();
    let sel_before = host.selection_count();

    let status = host.eval(&command);

    host.message(&report(
        status,
        before,
        host.change_number(),
        sel_before,
        host.selection_count(),
    ));
    0
}

declare_plugin! {
    name: "did-change",
    version: "0.1.0",
    commands: { "did" => did },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The motivating case: a command that succeeds and does nothing. Its exit
    /// status cannot distinguish this from one that did work.
    #[test]
    fn success_does_not_imply_a_change() {
        let line = report(0, 12, 12, 1, 1);
        assert!(line.contains("ran"));
        assert!(line.contains("buffer unchanged"));
    }

    /// An undo moves to an EARLIER revision, so the number goes down — a
    /// comparison testing for growth would call that "unchanged".
    #[test]
    fn an_undo_counts_as_a_change() {
        assert!(changed(13, 12), "backwards is still a change");
        assert!(changed(12, 13));
        assert!(!changed(12, 12));
        assert!(report(0, 13, 12, 1, 1).contains("buffer changed"));
    }

    /// A failing command may still have changed something before it failed, so
    /// the change is reported rather than assumed away by the status.
    #[test]
    fn a_failure_still_reports_its_effect() {
        let line = report(1, 12, 13, 1, 1);
        assert!(line.contains("failed"));
        assert!(
            line.contains("buffer changed"),
            "it did work before failing"
        );
    }

    /// Selection changes are reported when they happen, since a command can
    /// alter the selection without touching a character of text.
    #[test]
    fn selection_changes_are_reported_separately() {
        let line = report(0, 12, 12, 1, 4);
        assert!(line.contains("buffer unchanged"), "no text changed");
        assert!(line.contains("selections 1 → 4"));

        assert_eq!(selection_note(1, 1), "", "no note when unchanged");
    }
}
