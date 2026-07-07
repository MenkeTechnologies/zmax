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

use zemacs_view::{Document, DocumentId};

/// Cached `swapfile` / `directory` config so the change hook (which only gets the
/// document) can act without the editor config.
static SWAPFILE_ON: AtomicBool = AtomicBool::new(false);
static SWAP_DIR: Mutex<String> = Mutex::new(String::new());

thread_local! {
    // Per-document change counter, so the full buffer isn't rewritten on every
    // keystroke — only every `WRITE_EVERY` changes.
    static COUNTERS: RefCell<HashMap<DocumentId, usize>> = RefCell::new(HashMap::new());
}

const WRITE_EVERY: usize = 32;

fn swap_dir() -> String {
    SWAP_DIR.lock().map(|d| d.clone()).unwrap_or_default()
}

/// Swap-file path for `file`: `<dir>/.<name>.swp`, or beside the file when no
/// swap directory is configured.
fn swap_path(file: &std::path::Path, dir: &str) -> Option<PathBuf> {
    let name = file.file_name()?.to_string_lossy();
    let swp = format!(".{name}.swp");
    if dir.is_empty() {
        Some(file.parent().unwrap_or_else(|| std::path::Path::new(".")).join(swp))
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
    use zemacs_event::register_hook;
    use zemacs_view::events::{ConfigDidChange, DocumentDidChange};

    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        SWAPFILE_ON.store(event.new.swapfile, Ordering::Relaxed);
        if let Ok(mut d) = SWAP_DIR.lock() {
            *d = event.new.swap_directory.clone();
        }
        Ok(())
    });

    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        if SWAPFILE_ON.load(Ordering::Relaxed) {
            let id = event.doc.id();
            let due = COUNTERS.with(|c| {
                let mut c = c.borrow_mut();
                let n = c.entry(id).or_insert(0);
                *n += 1;
                *n % WRITE_EVERY == 0
            });
            if due {
                write_swap(event.doc);
            }
        }
        Ok(())
    });
}
