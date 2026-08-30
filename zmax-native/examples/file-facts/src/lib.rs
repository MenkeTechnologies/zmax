//! Example plugin: facts about a file on disk, not about the buffer.
//!
//! Two details in this family surprise people, and both are faithful to vim:
//!
//! - [`Host::file_type`] reports a symlink as `link`, never as what it points
//!   at. It stats the link itself, like `getftype()`, so a broken symlink is
//!   still a `link` rather than nothing.
//! - [`Host::file_writable`] keeps vim's THREE-valued answer: 0 not writable,
//!   1 writable, 2 a writable directory. Collapsing it to a bool would lose the
//!   distinction between "you may write this file" and "you may create files
//!   here".
//!
//! [`Host::fname_modify`] applies vim's `:p` `:h` `:t` `:r` `:e` modifiers, left
//! to right, so paths are manipulated the same way `expand()` does it.
//!
//! ```text
//! :plugin load .../libzmax_native_file_facts.dylib
//! :finfo             # the current buffer's file
//! :finfo /etc/hosts  # any path
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// vim's `filewritable()` three-valued answer, spelled out.
///
/// 2 is not "more writable than 1" — it means the path is a DIRECTORY you may
/// create in, which is a different permission from writing a file.
fn writable_note(writable: i32) -> &'static str {
    match writable {
        0 => "not writable",
        1 => "writable",
        2 => "writable directory",
        // The SDK passes vim's value through; anything else is new and worth
        // showing rather than silently folding into one of the above.
        other if other > 2 => "writable (unrecognised code)",
        _ => "not writable",
    }
}

/// Bytes in human-ish units. Directories have no meaningful size, so callers
/// pass `None` for them rather than printing 0.
fn size_note(size: Option<u64>) -> String {
    match size {
        None => "size unknown".to_string(),
        Some(bytes) if bytes < 1024 => format!("{bytes} B"),
        Some(bytes) if bytes < 1024 * 1024 => format!("{:.1} KiB", bytes as f64 / 1024.0),
        Some(bytes) => format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0)),
    }
}

/// The one-line report, kept pure so every branch is testable without a
/// filesystem.
fn report(
    name: &str,
    ftype: Option<&str>,
    size: Option<u64>,
    perm: Option<&str>,
    readable: bool,
    writable: i32,
) -> String {
    let Some(ftype) = ftype else {
        // No type means the path does not exist — nothing else is worth saying.
        return format!("{name}: does not exist");
    };
    let access = match (readable, writable) {
        (true, w) => format!("readable, {}", writable_note(w)),
        (false, w) => format!("NOT readable, {}", writable_note(w)),
    };
    format!(
        "{name}: {ftype}, {}, {} — {access}",
        size_note(size),
        perm.unwrap_or("?????????"),
    )
}

/// `:finfo [path]` — facts about a path, defaulting to the current buffer's.
fn finfo(host: &Host, args: &Args) -> c_int {
    let path = match args.rest().first() {
        Some(explicit) => explicit.clone(),
        None => match host.buffer_path() {
            Some(path) => path,
            None => {
                host.error("finfo: buffer has no file; pass a path");
                return 1;
            }
        },
    };

    // `:t` is vim's "tail": the basename, for a shorter report line.
    let shown = host
        .fname_modify(&path, ":t")
        .unwrap_or_else(|| path.clone());
    let ftype = host.file_type(&path);
    // A directory's byte size is not meaningful; skip it rather than print 0.
    let size = (!host.is_directory(&path))
        .then(|| host.file_size(&path))
        .flatten();

    host.message(&report(
        &shown,
        ftype.as_deref(),
        size,
        host.file_perm(&path).as_deref(),
        host.file_readable(&path),
        host.file_writable(&path),
    ));
    0
}

declare_plugin! {
    name: "file-facts",
    version: "0.1.0",
    commands: { "finfo" => finfo },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// vim's third value means a writable DIRECTORY — a different permission
    /// from writing a file, which a bool would erase.
    #[test]
    fn writable_keeps_vims_three_values() {
        assert_eq!(writable_note(0), "not writable");
        assert_eq!(writable_note(1), "writable");
        assert_eq!(writable_note(2), "writable directory");
    }

    /// A symlink reports as a link, not as its target — the file type stats the
    /// link itself.
    #[test]
    fn a_symlink_reports_as_a_link() {
        let line = report("cfg", Some("link"), Some(12), Some("rwxrwxrwx"), true, 1);
        assert!(line.contains("link"), "not resolved to file/dir");
    }

    /// A path that does not exist says so and stops, rather than reporting
    /// zeroes for every other field.
    #[test]
    fn a_missing_path_is_stated_and_nothing_else() {
        let line = report("gone", None, None, None, false, 0);
        assert_eq!(line, "gone: does not exist");
        assert!(!line.contains("readable"), "no fields for a missing file");
    }

    /// Unreadable is worth shouting about, since it is the case that breaks
    /// opening the file.
    #[test]
    fn unreadable_is_called_out() {
        let line = report("secret", Some("file"), Some(9), Some("---------"), false, 0);
        assert!(line.contains("NOT readable"));
    }

    /// Sizes scale, and an absent size is named rather than shown as zero — a
    /// directory has no meaningful byte count.
    #[test]
    fn sizes_scale_and_absence_is_named() {
        assert_eq!(size_note(Some(512)), "512 B");
        assert_eq!(size_note(Some(2048)), "2.0 KiB");
        assert_eq!(size_note(Some(3 * 1024 * 1024)), "3.0 MiB");
        assert_eq!(size_note(None), "size unknown");
    }
}
