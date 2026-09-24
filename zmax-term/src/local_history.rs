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
            let (ts, snap) = snapshots_in(&entry.path())
                .into_iter()
                .max_by_key(|(ts, _)| *ts)?;
            Some((ts, file, snap))
        })
        .collect();
    out.sort_by_key(|(ts, _, _)| std::cmp::Reverse(*ts));
    out.truncate(limit);
    out
}

/// The `(timestamp, path)` of every snapshot in `dir`. A snapshot is a
/// `<unix-ts>.snap`; its label, if it has one, is the `<unix-ts>.label` beside it.
fn snapshots_in(dir: &Path) -> Vec<(u64, PathBuf)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()? != "snap" {
                return None;
            }
            let ts: u64 = p.file_stem()?.to_str()?.parse().ok()?;
            Some((ts, p))
        })
        .collect()
}

/// Snapshots for `path`, newest first: `(unix_timestamp, snapshot_path)`.
pub fn snapshots(path: &Path) -> Vec<(u64, PathBuf)> {
    let mut v = snapshots_in(&dir_for(path));
    v.sort_by_key(|b| std::cmp::Reverse(b.0));
    v
}

/// Every snapshot of every file in the project, newest first: `(timestamp,
/// absolute file path, snapshot path)` — JetBrains "Show Project History".
pub fn all() -> Vec<(u64, PathBuf, PathBuf)> {
    let root = zmax_loader::find_workspace().0;
    let store = crate::run_config::project_dir().join("local-history");
    let mut out: Vec<(u64, PathBuf, PathBuf)> = std::fs::read_dir(&store)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|entry| {
            let file = root.join(path_from_key(&entry.file_name().to_string_lossy()));
            snapshots_in(&entry.path())
                .into_iter()
                .map(move |(ts, snap)| (ts, file.clone(), snap))
        })
        .collect();
    out.sort_by_key(|(ts, _, _)| std::cmp::Reverse(*ts));
    out
}

/// The label a snapshot was given by "Put Label", if any.
pub fn label(snapshot: &Path) -> Option<String> {
    std::fs::read_to_string(snapshot.with_extension("label"))
        .ok()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// JetBrains Local History "Put Label": record the buffer as it is now under
/// `name`. When the text matches the newest snapshot, that snapshot takes the
/// label rather than a copy being written.
pub fn put_label(path: &Path, text: &Rope, name: &str) -> std::io::Result<()> {
    let content = text.slice(..).to_string();
    let dir = dir_for(path);
    std::fs::create_dir_all(&dir)?;
    let snapshot = match snapshots(path).first() {
        Some((_, latest)) if std::fs::read_to_string(latest).is_ok_and(|s| s == content) => {
            latest.clone()
        }
        _ => {
            let snapshot = dir.join(format!("{}.snap", now()));
            std::fs::write(&snapshot, content)?;
            snapshot
        }
    };
    std::fs::write(snapshot.with_extension("label"), name)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
    let _ = std::fs::write(dir.join(format!("{}.snap", now())), content);
    // Prune: keep the newest MAX_SNAPSHOTS, and their labels with them.
    for (_, old) in snapshots(path).into_iter().skip(MAX_SNAPSHOTS) {
        let _ = std::fs::remove_file(old.with_extension("label"));
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
