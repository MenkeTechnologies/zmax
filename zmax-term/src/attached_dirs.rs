//! Directories attached to the project — JetBrains "Attach Directory to
//! Project" (`AttachDirectory`).
//!
//! The IDE lets a project hold more than one content root, so a library you
//! are reading beside your own code is browsable and searchable without
//! opening a second window. zmax's workspace is one directory; this is the
//! list of extra ones, honoured by the file picker (`SPC f f`) and by project
//! search (`SPC /`).
//!
//! The list is per session and never written to disk: an attached directory is
//! a working decision about what you are reading right now, and a stale one
//! restored at start-up would silently widen every later search.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ATTACHED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Attach `dir`. Returns false when it was already attached, so the caller can
/// say so rather than reporting a second attach that did nothing.
pub fn attach(dir: PathBuf) -> bool {
    let Ok(mut dirs) = ATTACHED.lock() else {
        return false;
    };
    if dirs.iter().any(|d| d == &dir) {
        return false;
    }
    dirs.push(dir);
    true
}

/// Detach `dir`; returns whether it was attached.
pub fn detach(dir: &Path) -> bool {
    let Ok(mut dirs) = ATTACHED.lock() else {
        return false;
    };
    let before = dirs.len();
    dirs.retain(|d| d != dir);
    dirs.len() != before
}

/// Detach everything; returns how many went.
pub fn detach_all() -> usize {
    match ATTACHED.lock() {
        Ok(mut dirs) => {
            let n = dirs.len();
            dirs.clear();
            n
        }
        Err(_) => 0,
    }
}

/// The attached directories, in attach order.
pub fn list() -> Vec<PathBuf> {
    ATTACHED.lock().map(|dirs| dirs.clone()).unwrap_or_default()
}

/// The attached directories that are not inside `root` already — the ones a
/// walk rooted at `root` would otherwise miss. Pure over its inputs; unit
/// tested through [`extra_roots_for`].
pub fn extra_roots(root: &Path) -> Vec<PathBuf> {
    extra_roots_for(&list(), root)
}

/// The filtering [`extra_roots`] does, over an explicit list so it can be
/// tested without touching the global state.
pub fn extra_roots_for(attached: &[PathBuf], root: &Path) -> Vec<PathBuf> {
    attached
        .iter()
        .filter(|dir| !dir.starts_with(root))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_roots_skip_directories_already_under_the_workspace() {
        let attached = vec![
            PathBuf::from("/work/project/vendor"), // inside the root already
            PathBuf::from("/other/library"),
        ];
        let extra = extra_roots_for(&attached, Path::new("/work/project"));
        assert_eq!(extra, vec![PathBuf::from("/other/library")]);
    }

    #[test]
    fn attaching_twice_reports_the_second_as_a_no_op() {
        detach_all();
        assert!(attach(PathBuf::from("/tmp/a")));
        assert!(!attach(PathBuf::from("/tmp/a")), "already attached");
        assert_eq!(list().len(), 1);
        assert!(detach(Path::new("/tmp/a")));
        assert!(!detach(Path::new("/tmp/a")), "gone already");
        detach_all();
    }
}
