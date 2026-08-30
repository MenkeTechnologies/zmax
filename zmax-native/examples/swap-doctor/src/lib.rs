//! Example plugin: is there anything to recover for this buffer?
//!
//! After a crash the useful question is not "does a swap file exist" but "does
//! it hold anything I do not already have". That needs three facts the SDK
//! keeps separate on purpose:
//!
//! - [`Host::swap_path`] — where the swap file WOULD be. A path here does not
//!   mean a file is there.
//! - [`Host::swap_exists`] — whether it actually is.
//! - [`Host::swap_locked_by`] — whether another process is holding it, which
//!   makes "recover" the wrong move.
//!
//! ```text
//! :plugin load .../libzmax_native_swap_doctor.dylib
//! :swap-doctor   # → "swap file present and unlocked — :recover would restore it"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// What the three swap facts add up to, as advice rather than as raw state.
///
/// Kept a pure function of the facts so the decision table is testable without
/// an editor, a swap file, or a second process.
fn verdict(path: Option<&str>, exists: bool, locked_by: Option<&str>) -> String {
    match (path, exists, locked_by) {
        // Another process holds it: recovering would fight a live editor.
        (_, _, Some(who)) => {
            format!("swap file held by {who} — another editor has this file open")
        }
        (None, _, None) => "no swap file for this buffer (:set noswapfile, or no path)".to_string(),
        (Some(_), true, None) => {
            "swap file present and unlocked — :recover would restore it".to_string()
        }
        (Some(path), false, None) => format!("no swap file on disk; it would live at {path}"),
    }
}

/// Whether the buffer holds anything the file on disk does not.
///
/// `undo_tree().is_saved()` answers this from the history rather than from a
/// dirty flag, so a buffer edited and then undone back to the saved revision
/// correctly reads as clean.
fn unsaved_note(is_saved: bool, modified: bool) -> &'static str {
    match (is_saved, modified) {
        (true, _) => "buffer matches the file on disk",
        (false, true) => "buffer has unsaved changes",
        // The history says we have moved off the saved revision even though the
        // modified flag disagrees — trust the history, and say so.
        (false, false) => "buffer sits on a different revision than the saved one",
    }
}

/// `:swap-doctor` — report recovery state for the current buffer.
fn swap_doctor(host: &Host, _args: &Args) -> c_int {
    if host.buffer_path().is_none() {
        host.error("swap-doctor: buffer has never been written");
        return 1;
    }
    let path = host.swap_path();
    let locked = host.swap_locked_by();
    let tree = host.undo_tree();

    host.message(&format!(
        "{}; {}",
        verdict(path.as_deref(), host.swap_exists(), locked.as_deref()),
        unsaved_note(tree.is_saved(), host.is_modified()),
    ));
    0
}

declare_plugin! {
    name: "swap-doctor",
    version: "0.1.0",
    commands: { "swap-doctor" => swap_doctor },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lock outranks everything else: whatever the other facts say, another
    /// process is editing this file and recovering is the wrong move.
    #[test]
    fn a_lock_outranks_the_other_facts() {
        let held = verdict(Some("/tmp/a.swp"), true, Some("host:4242"));
        assert!(held.contains("host:4242"));
        assert!(held.contains("another editor"));
        // Even with no swap file on disk, the lock is still the story.
        assert!(verdict(None, false, Some("host:1")).contains("another editor"));
    }

    /// Having a path is not having a file — the distinction the SDK keeps and
    /// the one that decides whether there is anything to recover.
    #[test]
    fn a_path_is_not_a_file() {
        let absent = verdict(Some("/tmp/a.swp"), false, None);
        assert!(absent.contains("no swap file on disk"));
        assert!(
            absent.contains("/tmp/a.swp"),
            "still says where it would be"
        );

        let present = verdict(Some("/tmp/a.swp"), true, None);
        assert!(present.contains(":recover"));
    }

    /// No path at all is its own case, not an absent file.
    #[test]
    fn no_path_is_distinct_from_no_file() {
        assert!(verdict(None, false, None).contains("no swap file for this buffer"));
    }

    /// The history is the authority on whether anything is unsaved: a buffer
    /// undone back to the saved revision is clean even if it was edited.
    #[test]
    fn the_history_outranks_the_modified_flag() {
        assert_eq!(unsaved_note(true, true), "buffer matches the file on disk");
        assert_eq!(unsaved_note(false, true), "buffer has unsaved changes");
        assert_eq!(
            unsaved_note(false, false),
            "buffer sits on a different revision than the saved one"
        );
    }
}
