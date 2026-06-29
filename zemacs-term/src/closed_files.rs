//! In-memory stack of recently closed file paths, powering "reopen last closed
//! file" (the IDE / browser `Ctrl-Shift-T` gesture). Populated on
//! `DocumentDidClose` (see `handlers::closed_files`) and popped by the
//! `reopen_last_closed` command. Session-scoped — not persisted to disk.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

static STACK: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
const MAX: usize = 50;

/// Push a just-closed file onto the stack (most-recent last), deduping so the
/// same path doesn't pile up, and capping the history.
pub fn push(path: &Path) {
    if let Ok(mut stack) = STACK.lock() {
        let path = path.to_path_buf();
        stack.retain(|p| p != &path);
        stack.push(path);
        let len = stack.len();
        if len > MAX {
            stack.drain(0..len - MAX);
        }
    }
}

/// Pop the most-recently-closed file (LIFO), or `None` when the history is empty.
pub fn pop() -> Option<PathBuf> {
    STACK.lock().ok().and_then(|mut stack| stack.pop())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifo_with_dedup() {
        // Drain anything left by other code paths first.
        while pop().is_some() {}
        push(Path::new("/tmp/a.rs"));
        push(Path::new("/tmp/b.rs"));
        push(Path::new("/tmp/a.rs")); // re-close a → moves to top, no duplicate
        assert_eq!(pop(), Some(PathBuf::from("/tmp/a.rs")));
        assert_eq!(pop(), Some(PathBuf::from("/tmp/b.rs")));
        assert_eq!(pop(), None);
    }
}
