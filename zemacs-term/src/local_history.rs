//! JetBrains-style Local History: on every save, snapshot the file's contents to
//! `~/.zemacs/projects/<proj>/local-history/<relpath>/<unix-ts>.snap` (independent
//! of git). `:LocalHistory` lists a file's snapshots newest-first; opening one
//! shows that past version. Old snapshots are pruned to `MAX_SNAPSHOTS`.

use std::path::{Path, PathBuf};

use zemacs_core::Rope;

const MAX_SNAPSHOTS: usize = 50;

/// Per-file snapshot directory under the project's state dir.
fn dir_for(path: &Path) -> PathBuf {
    let root = zemacs_loader::find_workspace().0;
    let rel = path.strip_prefix(&root).unwrap_or(path);
    let key = rel.to_string_lossy().replace(['/', '\\'], "%");
    crate::run_config::project_dir()
        .join("local-history")
        .join(key)
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
    v.sort_by(|a, b| b.0.cmp(&a.0));
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
