//! vim `swapfile`: a recovery swap file of unsaved changes. While a buffer is
//! edited (and `:set swapfile` is on) its contents are periodically written to a
//! `.<name>.swp` file; the swap is removed on a clean save. If a swap file
//! already exists when a file is opened, the user is warned (recovery awareness),
//! as vim's `E325`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use zmax_view::{Document, DocumentId};

/// Cached `swapfile` / `directory` config so the change hook (which only gets the
/// document) can act without the editor config.
static SWAPFILE_ON: AtomicBool = AtomicBool::new(false);
static SWAP_DIR: Mutex<String> = Mutex::new(String::new());

thread_local! {
    // Per-document change counter, so the full buffer isn't rewritten on every
    // keystroke — only every `updatecount` changes.
    static COUNTERS: RefCell<HashMap<DocumentId, usize>> = RefCell::new(HashMap::new());
    // Documents opened under vim `:noswapfile` — the modifier says "this command
    // doesn't touch the swap file", and a buffer that was opened that way must
    // not grow one behind the user's back either.
    static NO_SWAP: RefCell<std::collections::HashSet<DocumentId>> =
        RefCell::new(std::collections::HashSet::new());
}

/// vim `:noswapfile {cmd}` — the document `{cmd}` opened keeps no swap file, for
/// as long as it is open. Called by the command layer once the wrapped command
/// has run (it is the command that knows the modifier was there).
pub fn set_no_swap(doc: DocumentId) {
    NO_SWAP.with(|s| s.borrow_mut().insert(doc));
}

/// Whether `doc` was opened under `:noswapfile`.
fn no_swap(doc: DocumentId) -> bool {
    NO_SWAP.with(|s| s.borrow().contains(&doc))
}

/// vim `updatecount`'s own default: the swap file is rewritten after this many
/// changes when the option was never `:set`.
const UPDATECOUNT_DEFAULT: usize = 200;

/// vim `updatecount`: "After typing this many characters the swap file will be
/// written to disk. When zero, no swap file will be produced at all." `count` is
/// the document's running change count. Pure — unit tested.
fn swap_write_due(count: usize, updatecount: usize) -> bool {
    updatecount != 0 && count.is_multiple_of(updatecount)
}

/// The live `updatecount` (vim's default when it was never `:set`).
fn updatecount() -> usize {
    crate::commands::typed::vim_opt_num("updatecount")
        .or_else(|| crate::commands::typed::vim_opt_num("uc"))
        .unwrap_or(UPDATECOUNT_DEFAULT)
}

fn swap_dir() -> String {
    SWAP_DIR.lock().map(|d| d.clone()).unwrap_or_default()
}

/// Swap-file path for `file`: `<dir>/.<name>.swp`, or beside the file when no
/// swap directory is configured.
fn swap_path(file: &std::path::Path, dir: &str) -> Option<PathBuf> {
    let name = file.file_name()?.to_string_lossy();
    let swp = format!(".{name}.swp");
    if dir.is_empty() {
        Some(
            file.parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(swp),
        )
    } else {
        let dir = if let Some(rest) = dir.strip_prefix("~/") {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(rest))
                .unwrap_or_else(|| PathBuf::from(dir))
        } else {
            PathBuf::from(dir)
        };
        // Flatten the path into the swap dir name to avoid collisions.
        let flat = file.to_string_lossy().replace(['/', '\\'], "%");
        Some(dir.join(format!(".{flat}.swp")))
    }
}

/// Write the buffer to its swap file (best-effort).
fn write_swap(doc: &Document) {
    let dir = swap_dir();
    let Some(path) = doc.path().and_then(|p| swap_path(p, &dir)) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, doc.text().to_string());
}

/// The swap-file path for a document (vim `:swapname`), if it has a file name.
pub fn path_for(doc: &Document) -> Option<PathBuf> {
    swap_path(doc.path()?, &swap_dir())
}

/// vim `:preserve` — flush the buffer to its swap file now (rather than waiting
/// for the periodic change hook).
pub fn preserve(doc: &Document) {
    write_swap(doc);
}

/// vim `:recover` — the contents of the document's swap file, if one exists.
pub fn recover_text(doc: &Document) -> Option<String> {
    std::fs::read_to_string(path_for(doc)?).ok()
}

/// Remove a document's swap file (on a clean save).
pub fn remove(doc: &Document) {
    let dir = swap_dir();
    if let Some(path) = doc.path().and_then(|p| swap_path(p, &dir)) {
        let _ = std::fs::remove_file(path);
    }
}

/// Whether a swap file already exists for the document (recovery detection).
pub fn swap_exists(doc: &Document) -> bool {
    let dir = swap_dir();
    doc.path()
        .and_then(|p| swap_path(p, &dir))
        .map(|s| s.is_file())
        .unwrap_or(false)
}

/// Refresh swap files on edits, prime the cached config, and warn on recovery.
pub fn register_hooks() {
    use zmax_event::register_hook;
    use zmax_view::events::{ConfigDidChange, DocumentDidChange};

    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        SWAPFILE_ON.store(event.new.swapfile, Ordering::Relaxed);
        if let Ok(mut d) = SWAP_DIR.lock() {
            *d = event.new.swap_directory.clone();
        }
        Ok(())
    });

    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        // vim `updatecount`: how many changes go by between swap-file writes, and
        // `updatecount=0` means no swap file is produced at all.
        let updatecount = updatecount();
        if SWAPFILE_ON.load(Ordering::Relaxed) && updatecount != 0 && !no_swap(event.doc.id()) {
            let id = event.doc.id();
            let count = COUNTERS.with(|c| {
                let mut c = c.borrow_mut();
                let n = c.entry(id).or_insert(0);
                *n += 1;
                *n
            });
            if swap_write_due(count, updatecount) {
                write_swap(event.doc);
            }
        }
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// vim `updatecount`: the swap file is rewritten every N changes, and
    /// `updatecount=0` turns swap-file writing off entirely.
    #[test]
    fn updatecount_controls_the_swap_write_cadence() {
        // The default: every 200th change.
        assert!(!swap_write_due(1, UPDATECOUNT_DEFAULT));
        assert!(!swap_write_due(199, UPDATECOUNT_DEFAULT));
        assert!(swap_write_due(200, UPDATECOUNT_DEFAULT));
        assert!(swap_write_due(400, UPDATECOUNT_DEFAULT));

        // `:set updatecount=10` writes ten times as often.
        assert!(swap_write_due(10, 10));
        assert!(swap_write_due(20, 10));
        assert!(!swap_write_due(11, 10));

        // `:set updatecount=0` never writes.
        for n in [0, 1, 200, 1000] {
            assert!(!swap_write_due(n, 0), "updatecount=0 must never write");
        }
    }

    /// vim `:noswapfile {cmd}`: the buffer that command opened keeps no swap file
    /// — including from the periodic writer, which is the only thing that would
    /// have created one after the command itself was over.
    #[test]
    fn noswapfile_buffers_never_get_a_swap_file() {
        let doc = DocumentId::default();
        assert!(!no_swap(doc), "an ordinary buffer does get a swap file");
        set_no_swap(doc);
        assert!(
            no_swap(doc),
            "`:noswapfile edit x` must keep the periodic writer off that buffer"
        );
        NO_SWAP.with(|s| s.borrow_mut().clear());
    }

    /// `:set updatecount=N` is what the change hook reads; unset keeps vim's 200.
    #[test]
    fn updatecount_reads_the_option_store() {
        assert_eq!(updatecount(), UPDATECOUNT_DEFAULT);
        crate::commands::typed::vim_opt_store("updatecount", "50".to_string());
        assert_eq!(updatecount(), 50);
        crate::commands::typed::vim_opt_store("updatecount", "0".to_string());
        assert_eq!(updatecount(), 0);
        crate::commands::typed::vim_opt_store("updatecount", String::new());
        assert_eq!(updatecount(), UPDATECOUNT_DEFAULT);
    }
}
