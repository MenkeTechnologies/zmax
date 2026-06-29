//! Filesystem watcher that live-refreshes the IDE file tree when files change
//! on disk outside the editor (create/delete/rename/modify).
//!
//! A dedicated OS thread owns the `notify` watcher and a receive loop. On a
//! relevant event it coalesces a short burst, then hops onto the main thread via
//! [`job::dispatch_blocking`] to rebuild the tree; the event loop renders right
//! after each dispatched callback, so the change shows up immediately.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use crate::job;
use crate::ui::EditorView;

/// Ensures we only ever spawn a single watcher for the process.
static SPAWNED: AtomicBool = AtomicBool::new(false);

/// Directories whose churn should never trigger a tree refresh (build output,
/// VCS internals, dependency caches) — they're noisy and usually hidden anyway.
fn is_ignored(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(
                ".git"
                    | "target"
                    | "node_modules"
                    | ".cache"
                    | "dist"
                    | "build"
                    | ".direnv"
                    | ".venv"
            )
        )
    })
}

/// Start watching `root` recursively. Idempotent: only the first call spawns a
/// watcher; later calls (e.g. reopening the IDE) are no-ops.
pub fn spawn(root: PathBuf) {
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }

    std::thread::Builder::new()
        .name("file-tree-watcher".into())
        .spawn(move || run(root))
        .ok();
}

fn run(root: PathBuf) {
    let (tx, rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        // Forward both events and errors; the loop decides what to do.
        let _ = tx.send(res);
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            log::warn!("file watcher unavailable: {err}");
            return;
        }
    };

    if let Err(err) = watcher.watch(&root, RecursiveMode::Recursive) {
        log::warn!("could not watch {}: {err}", root.display());
        return;
    }

    // Keep `watcher` alive for the lifetime of this thread.
    loop {
        // Block until something happens.
        let first = match rx.recv() {
            Ok(event) => event,
            Err(_) => return, // sender dropped — watcher gone
        };

        let mut relevant = event_is_relevant(&first);

        // Coalesce a burst (e.g. a `git checkout` touching many files) into one
        // refresh so we don't rebuild the tree dozens of times.
        while let Ok(event) = rx.recv_timeout(Duration::from_millis(150)) {
            relevant |= event_is_relevant(&event);
        }

        if relevant {
            job::dispatch_blocking(|_editor, compositor| {
                if let Some(view) = compositor.find::<EditorView>() {
                    view.refresh_file_tree();
                }
            });
        }
    }
}

/// True if an event touches a path we actually display (outside ignored dirs).
fn event_is_relevant(event: &notify::Result<notify::Event>) -> bool {
    match event {
        Ok(event) => event.paths.is_empty() || event.paths.iter().any(|path| !is_ignored(path)),
        // On error, be conservative and refresh.
        Err(_) => true,
    }
}
