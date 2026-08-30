//! Example plugin: has this file changed underneath me?
//!
//! Also the example of a plugin holding STATE between commands.
//!
//! [`Host::file_time`] is vim `getftime()` — the file's mtime in seconds since
//! the epoch. On its own that answers nothing: the SDK exposes no clock and no
//! record of when the buffer was read, so a single reading cannot tell you
//! whether the disk copy is newer than what you have.
//!
//! What it can do is compare against a BASELINE the plugin took earlier. That
//! baseline has to live somewhere across command invocations, which is what the
//! static here is for — a plugin's own state, distinct from anything the editor
//! keeps.
//!
//! The dangerous combination is a file changed on disk AND unsaved changes in
//! the buffer: reloading loses your edits, writing loses theirs. Naming that
//! case is the point, since the safe ones need no action.
//!
//! ```text
//! :plugin load .../libzmax_native_file_fresh.dylib
//! :fresh-mark   # remember the file as it is now
//! …later…
//! :fresh        # → "CONFLICT: disk moved on 40s AND unsaved here — neither reload nor write is safe"
//! ```

use std::os::raw::c_int;
use std::sync::atomic::{AtomicI64, Ordering};

use zmax_native::{declare_plugin, Args, Host};

/// The mtime recorded by `:fresh-mark`, or `NO_BASELINE` when none has been
/// taken. A plugin's own state, living for as long as the dylib is loaded.
static BASELINE: AtomicI64 = AtomicI64::new(NO_BASELINE);

/// Sentinel for "no baseline taken". Real mtimes can be negative (files dated
/// before 1970), so a value no filesystem produces is used rather than 0.
const NO_BASELINE: i64 = i64::MIN;

/// A gap under this many seconds usually means a tool is still writing.
const JUST_NOW: i64 = 5;

/// Human-ish description of an elapsed gap.
fn gap(seconds: i64) -> String {
    match seconds {
        s if s < JUST_NOW => "moments".to_string(),
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// The verdict from the two facts that matter.
///
/// A pure function of them, so the conflict case is testable without a
/// filesystem, a clock, or a second process racing the test.
fn verdict(moved_by: Option<i64>, modified_here: bool) -> String {
    match (moved_by, modified_here) {
        (None, true) => {
            "no baseline — run :fresh-mark first (buffer has unsaved changes)".to_string()
        }
        (None, false) => "no baseline — run :fresh-mark first".to_string(),
        // The only case that can lose work whichever way you go.
        (Some(moved), true) if moved != 0 => format!(
            "CONFLICT: disk moved on {} AND unsaved here — neither reload nor write is safe",
            gap(moved.abs())
        ),
        (Some(moved), false) if moved != 0 => {
            format!("disk moved on {} — reload to pick it up", gap(moved.abs()))
        }
        (Some(_), true) => "unsaved changes here; disk untouched since the mark".to_string(),
        (Some(_), false) => "in sync with the mark".to_string(),
    }
}

/// The file's mtime, or an error message naming why there is none.
fn mtime_of(host: &Host) -> Result<i64, String> {
    let path = host
        .buffer_path()
        .ok_or_else(|| "buffer has never been written".to_string())?;
    host.file_time(&path)
        .ok_or_else(|| "the file is gone from disk".to_string())
}

/// `:fresh-mark` — record the file as it is now.
fn fresh_mark(host: &Host, _args: &Args) -> c_int {
    match mtime_of(host) {
        Ok(mtime) => {
            BASELINE.store(mtime, Ordering::Relaxed);
            host.message("baseline recorded");
            0
        }
        Err(why) => {
            host.error(&format!("fresh-mark: {why}"));
            1
        }
    }
}

/// `:fresh` — compare the file against the recorded baseline.
fn fresh(host: &Host, _args: &Args) -> c_int {
    let mtime = match mtime_of(host) {
        Ok(mtime) => mtime,
        Err(why) => {
            host.error(&format!("fresh: {why}"));
            return 1;
        }
    };

    let baseline = BASELINE.load(Ordering::Relaxed);
    let moved_by = (baseline != NO_BASELINE).then(|| mtime - baseline);

    // The undo history knows whether the buffer sits on the saved revision,
    // which survives an edit-then-undo where a dirty flag would not.
    let modified_here = !host.undo_tree().is_saved();

    host.message(&verdict(moved_by, modified_here));
    0
}

declare_plugin! {
    name: "file-fresh",
    version: "0.1.0",
    commands: {
        "fresh" => fresh,
        "fresh-mark" => fresh_mark,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case worth naming: both sides moved, and every obvious action loses
    /// somebody's work.
    #[test]
    fn both_sides_changed_is_the_dangerous_case() {
        let line = verdict(Some(40), true);
        assert!(line.starts_with("CONFLICT"));
        assert!(line.contains("40s"));
        assert!(line.contains("neither reload nor write is safe"));
    }

    /// Disk moved with a clean buffer is safe to reload, and says so rather
    /// than merely reporting the fact.
    #[test]
    fn a_clean_buffer_can_just_reload() {
        let line = verdict(Some(120), false);
        assert!(line.contains("reload to pick it up"));
        assert!(!line.contains("CONFLICT"));
    }

    /// A file dated BACKWARDS still moved — `git checkout` of an older commit
    /// restores an older mtime, and a signed comparison would call that "no
    /// change" if it only tested for growth.
    #[test]
    fn a_backwards_mtime_still_counts_as_moved() {
        let line = verdict(Some(-300), false);
        assert!(line.contains("reload"), "older is still different");
        assert!(line.contains("5m"), "the gap is reported unsigned");
    }

    /// Without a baseline there is nothing to compare against, and the plugin
    /// says so instead of inventing a comparison — the SDK gives it no clock
    /// and no load time to fall back on.
    #[test]
    fn no_baseline_is_stated_not_guessed() {
        assert!(verdict(None, false).contains("run :fresh-mark first"));
        // The unsaved half is still worth mentioning even with no baseline.
        assert!(verdict(None, true).contains("unsaved changes"));
    }

    /// An unmoved file with unsaved edits is the everyday state and needs no
    /// action.
    #[test]
    fn an_unmoved_file_is_ordinary() {
        assert_eq!(
            verdict(Some(0), true),
            "unsaved changes here; disk untouched since the mark"
        );
        assert_eq!(verdict(Some(0), false), "in sync with the mark");
    }

    /// Gaps scale, and a file touched moments ago is named as such — a tool
    /// may still be writing it.
    #[test]
    fn gaps_scale() {
        assert_eq!(gap(0), "moments");
        assert_eq!(gap(40), "40s");
        assert_eq!(gap(600), "10m");
        assert_eq!(gap(7200), "2h");
        assert_eq!(gap(172_800), "2d");
    }
}
