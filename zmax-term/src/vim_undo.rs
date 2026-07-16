//! vim `undofile`: persist a document's undo history to disk on write and reload
//! it on open, so undo survives closing and reopening a file. The history is only
//! restored when the file's current content hash matches the hash stored with the
//! undo file, so a history is never replayed against mismatched text.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use zmax_view::Document;

#[derive(serde::Serialize, serde::Deserialize)]
struct UndoFile {
    /// Hash of the document text this history corresponds to.
    content_hash: u64,
    history: zmax_core::history::HistorySnapshot,
}

/// The directory undo files live in (`undo_dir` config, else `~/.zmax/undo`).
fn undo_dir(configured: &str) -> Option<PathBuf> {
    if !configured.is_empty() {
        return Some(PathBuf::from(shellexpand_home(configured)));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".zmax").join("undo"))
}

fn shellexpand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{}", home.to_string_lossy(), rest);
        }
    }
    p.to_string()
}

fn hash64<T: Hash>(v: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Undo-file path for `file` inside `dir`: a hash of the canonical path (so it is
/// stable and collision-resistant, and safe as a flat filename).
fn undo_file_path(dir: &Path, file: &Path) -> PathBuf {
    let key = zmax_stdx::path::canonicalize(file);
    dir.join(format!("{:016x}.undo", hash64(&key)))
}

fn text_hash(doc: &Document) -> u64 {
    hash64(&doc.text().to_string())
}

/// Persist the document's undo history (vim `:set undofile` on write).
pub fn save(doc: &Document, undo_dir_cfg: &str) {
    let Some(path) = doc.path().map(|p| p.to_path_buf()) else {
        return;
    };
    let Some(dir) = undo_dir(undo_dir_cfg) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let record = UndoFile {
        content_hash: text_hash(doc),
        history: doc.undo_snapshot(),
    };
    if let Ok(json) = serde_json::to_vec(&record) {
        let _ = std::fs::write(undo_file_path(&dir, &path), json);
    }
}

/// vim `:wundo {file}` — write the buffer's undo history to an explicit file.
pub fn save_to(doc: &Document, file: &std::path::Path) -> std::io::Result<()> {
    let record = UndoFile {
        content_hash: text_hash(doc),
        history: doc.undo_snapshot(),
    };
    let json = serde_json::to_vec(&record)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, json)
}

/// vim `:rundo {file}` — read undo history from an explicit file. Returns true
/// when it was restored (only if the on-disk text matches the history's base).
pub fn load_from(doc: &mut Document, file: &std::path::Path) -> bool {
    let Ok(bytes) = std::fs::read(file) else {
        return false;
    };
    let Ok(record) = serde_json::from_slice::<UndoFile>(&bytes) else {
        return false;
    };
    if record.content_hash != text_hash(doc) {
        return false;
    }
    doc.restore_undo(record.history);
    true
}

/// Reload undo history for a freshly opened document if a matching undo file
/// exists (vim `:set undofile` on open). Returns true when history was restored.
pub fn load(doc: &mut Document, undo_dir_cfg: &str) -> bool {
    let Some(path) = doc.path().map(|p| p.to_path_buf()) else {
        return false;
    };
    let Some(dir) = undo_dir(undo_dir_cfg) else {
        return false;
    };
    let Ok(bytes) = std::fs::read(undo_file_path(&dir, &path)) else {
        return false;
    };
    let Ok(record) = serde_json::from_slice::<UndoFile>(&bytes) else {
        return false;
    };
    // Only restore when the on-disk text matches the history's base text.
    if record.content_hash != text_hash(doc) {
        return false;
    }
    doc.restore_undo(record.history);
    true
}
