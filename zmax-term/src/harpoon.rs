//! Harpoon-style file marks: a small, ordered, per-project list of "pinned"
//! files you can jump to instantly by slot (1..N), independent of the buffer
//! list. Inspired by ThePrimeagen's harpoon.nvim.
//!
//! Persisted at `<config-dir>/harpoon` as `<project-cwd>\t<file-path>` lines so
//! each project keeps its own list in one flat store. `list()` returns only the
//! entries tagged with the current working directory, in pin order.

use std::path::{Path, PathBuf};

const FILE_NAME: &str = "harpoon";
const MAX_MARKS: usize = 20;

fn store_path() -> PathBuf {
    zmax_loader::config_dir().join(FILE_NAME)
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// All `(project, path)` rows in the store (existing files only).
fn load_all() -> Vec<(PathBuf, PathBuf)> {
    let Ok(contents) = std::fs::read_to_string(store_path()) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| {
            let (proj, path) = line.split_once('\t')?;
            let path = PathBuf::from(path);
            path.is_file().then(|| (PathBuf::from(proj), path))
        })
        .collect()
}

fn write_all(rows: &[(PathBuf, PathBuf)]) {
    let body = rows
        .iter()
        .map(|(proj, path)| format!("{}\t{}", proj.to_string_lossy(), path.to_string_lossy()))
        .collect::<Vec<_>>()
        .join("\n");
    let store = store_path();
    if let Some(parent) = store.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(store, body);
}

/// The current project's pinned files, in pin order.
pub fn list() -> Vec<PathBuf> {
    let here = cwd();
    load_all()
        .into_iter()
        .filter(|(proj, _)| proj == &here)
        .map(|(_, path)| path)
        .collect()
}

/// Pin `path` for the current project (idempotent — moves nothing if already
/// pinned). Returns the resulting 1-based slot of the file.
pub fn add(path: &Path) -> usize {
    let here = cwd();
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut rows = load_all();

    // Already pinned in this project? Report its slot, don't duplicate.
    let existing: Vec<&PathBuf> = rows
        .iter()
        .filter(|(proj, _)| proj == &here)
        .map(|(_, p)| p)
        .collect();
    if let Some(pos) = existing.iter().position(|p| **p == path) {
        return pos + 1;
    }

    let slot = existing.len() + 1;
    if slot <= MAX_MARKS {
        rows.push((here, path));
        write_all(&rows);
    }
    slot
}

/// Remove `path` from the current project's marks.
pub fn remove(path: &Path) {
    let here = cwd();
    let mut rows = load_all();
    rows.retain(|(proj, p)| !(proj == &here && p == path));
    write_all(&rows);
}

/// Move `path`'s mark one slot up (or down) within the current project's list,
/// reordering the store. Returns false if it's already at the end in that
/// direction or isn't pinned.
pub fn move_mark(path: &Path, up: bool) -> bool {
    let here = cwd();
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut rows = load_all();
    // store indices belonging to the current project, in order
    let proj: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, (p, _))| *p == here)
        .map(|(i, _)| i)
        .collect();
    let Some(pos) = proj.iter().position(|&i| rows[i].1 == path) else {
        return false;
    };
    let target = if up {
        if pos == 0 {
            return false;
        }
        proj[pos - 1]
    } else {
        if pos + 1 >= proj.len() {
            return false;
        }
        proj[pos + 1]
    };
    rows.swap(proj[pos], target);
    write_all(&rows);
    true
}

/// The file at 1-based slot `n` in the current project, if any.
pub fn get(n: usize) -> Option<PathBuf> {
    if n == 0 {
        return None;
    }
    list().into_iter().nth(n - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_lookup_is_one_based() {
        let v = [PathBuf::from("/a"), PathBuf::from("/b")];
        assert_eq!(v.get(1 - 1), Some(&PathBuf::from("/a")));
        assert_eq!(v.get(2 - 1), Some(&PathBuf::from("/b")));
    }
}
