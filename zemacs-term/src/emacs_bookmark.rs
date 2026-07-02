//! Emacs bookmarks (`C-x r m` / `C-x r b` / `C-x r l`).
//!
//! Named, persistent positions — the string-named, cross-session cousin of the
//! char-keyed point registers (`emacs_register`). The store logic lives in the
//! dependency-free engine `zemacs_core::bookmark::BookmarkStore`; this module is
//! the thin IO wrapper that loads/saves it at `<config-dir>/bookmarks` (one
//! `name\tfile\tline[\tcolumn]` record per line — the engine's text format).
//! `commands.rs` prompts for the name on set and offers a picker on jump.

use std::path::PathBuf;

use zemacs_core::bookmark::{Bookmark, BookmarkStore};
use zemacs_loader::config_dir;

const FILE_NAME: &str = "bookmarks";

fn store_path() -> PathBuf {
    config_dir().join(FILE_NAME)
}

/// Read the persisted store (empty if the file is missing or unreadable).
pub fn load() -> BookmarkStore {
    match std::fs::read_to_string(store_path()) {
        Ok(s) => BookmarkStore::deserialize(&s),
        Err(_) => BookmarkStore::new(),
    }
}

/// Persist the store, creating the config directory if needed.
fn save(store: &BookmarkStore) {
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, store.serialize());
}

/// `bookmark-set` (C-x r m): create or overwrite `name` at `file`:`line`:`col`.
pub fn set(name: &str, file: &str, line: usize, column: Option<usize>) {
    let mut store = load();
    store.set(name, Bookmark::new(file, line, column));
    save(&store);
}

/// `bookmark-set-no-overwrite` (C-x r M): create `name` only if unused. Returns
/// `true` when a new bookmark was inserted.
pub fn set_no_overwrite(name: &str, file: &str, line: usize, column: Option<usize>) -> bool {
    let mut store = load();
    let inserted = store.set_no_overwrite(name, Bookmark::new(file, line, column));
    if inserted {
        save(&store);
    }
    inserted
}

/// `bookmark-delete`: remove `name`. Returns `true` if it existed.
pub fn delete(name: &str) -> bool {
    let mut store = load();
    let removed = store.delete(name).is_some();
    if removed {
        save(&store);
    }
    removed
}

/// `bookmark-rename`: rename `from` to `to`. Returns `true` on success.
pub fn rename(from: &str, to: &str) -> bool {
    let mut store = load();
    let ok = store.rename(from, to);
    if ok {
        save(&store);
    }
    ok
}

/// All bookmarks as `(name, file, line, column)`, most recently set first.
pub fn list() -> Vec<(String, PathBuf, usize, Option<usize>)> {
    load()
        .list()
        .iter()
        .map(|(name, b)| (name.clone(), PathBuf::from(&b.file), b.line, b.column))
        .collect()
}

/// The bookmark names, for prompt completion.
pub fn names() -> Vec<String> {
    load().names().map(str::to_string).collect()
}
