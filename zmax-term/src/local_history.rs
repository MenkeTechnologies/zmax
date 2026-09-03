//! JetBrains-style Local History: on every save, snapshot the file's contents to
//! `~/.zmax/projects/<proj>/local-history/<relpath>/<unix-ts>.snap` (independent
//! of git). `:LocalHistory` lists a file's snapshots newest-first; opening one
//! shows that past version. Old snapshots are pruned to `MAX_SNAPSHOTS`.

use std::path::{Path, PathBuf};

use zmax_core::Rope;

const MAX_SNAPSHOTS: usize = 50;

/// The directory name a file's snapshots live under: its project-relative path
/// with the separators flattened, so one directory level holds every file.
fn key_for(path: &Path) -> String {
    let root = zmax_loader::find_workspace().0;
    let rel = path.strip_prefix(&root).unwrap_or(path);
    rel.to_string_lossy().replace(['/', '\\'], "%")
}

/// The project-relative path a snapshot directory name came from — the inverse
/// of [`key_for`]. A file whose own name contains `%` cannot be told apart from
/// a separator here; that is inherent to the flattening and only affects the
/// label shown in the Recent Changes list, never which file is opened.
fn path_from_key(key: &str) -> PathBuf {
    PathBuf::from(key.replace('%', "/"))
}

/// Per-file snapshot directory under the project's state dir.
fn dir_for(path: &Path) -> PathBuf {
    crate::run_config::project_dir()
        .join("local-history")
        .join(key_for(path))
}

/// The project's most recent snapshots across ALL files, newest first:
/// `(unix_timestamp, absolute file path, snapshot path)`, at most `limit`.
///
/// This is the project-wide view of the same store [`snapshots`] reads per
/// file, which is what JetBrains Recent Changes lists.
pub fn recent(limit: usize) -> Vec<(u64, PathBuf, PathBuf)> {
    let root = zmax_loader::find_workspace().0;
    let store = crate::run_config::project_dir().join("local-history");
    let mut out: Vec<(u64, PathBuf, PathBuf)> = std::fs::read_dir(&store)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let key = entry.file_name().to_string_lossy().into_owned();
            let file = root.join(path_from_key(&key));
            // Newest snapshot in this file's directory.
            let (ts, snap) = std::fs::read_dir(entry.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    let ts: u64 = p.file_stem()?.to_str()?.parse().ok()?;
                    Some((ts, p))
                })
                .max_by_key(|(ts, _)| *ts)?;
            Some((ts, file, snap))
        })
        .collect();
    out.sort_by_key(|(ts, _, _)| std::cmp::Reverse(*ts));
    out.truncate(limit);
    out
}

/// Snapshots for `path`, newest first: `(unix_timestamp, snapshot_path)`.
pub fn snapshots(path: &Path) -> Vec<(u64, PathBuf)> {
    let dir = dir_for(path);
    let mut v: Vec<(u64, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ts: u64 = p.file_stem()?.to_str()?.parse().ok()?;
            Some((ts, p))
        })
        .collect();
    v.sort_by_key(|b| std::cmp::Reverse(b.0));
    v
}

/// Record a snapshot of `text` for `path` (called on save). Skips a write when
/// the content is identical to the most recent snapshot, and prunes old ones.
pub fn record(path: &Path, text: &Rope) {
    let content = text.slice(..).to_string();
    let dir = dir_for(path);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let existing = snapshots(path);
    if let Some((_, latest)) = existing.first() {
        if std::fs::read_to_string(latest).is_ok_and(|s| s == content) {
            return; // unchanged since the last snapshot
        }
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = std::fs::write(dir.join(format!("{ts}.snap")), content);
    // Prune: keep the newest MAX_SNAPSHOTS.
    for (_, old) in snapshots(path).into_iter().skip(MAX_SNAPSHOTS) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::path_from_key;

    #[test]
    fn a_snapshot_directory_name_maps_back_to_its_path() {
        assert_eq!(
            path_from_key("src%ui%editor.rs"),
            std::path::PathBuf::from("src/ui/editor.rs")
        );
        assert_eq!(
            path_from_key("README.md"),
            std::path::PathBuf::from("README.md")
        );
    }
}
