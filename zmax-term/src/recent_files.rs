//! Persistent most-recently-used (MRU) + frecency file store backing the
//! startify start screen, the IDE "RECENT" tab, and the recent-files picker.
//!
//! Stored at `<config-dir>/recent_files`, one entry per line as
//! `path\t<rank>\t<unix-time>` (tab-separated). Older stores that hold a bare
//! path per line are read transparently (rank 1, time 0). Files are recorded on
//! `DocumentDidOpen` (see `handlers::recent_files`).
//!
//! Ranking uses the `z`/`autojump` "frecency" algorithm (rupa/z): a hit bumps
//! the entry's `rank` and stamps its access `time`; the combined score weights
//! frequency by how recently the file was touched, via fixed time buckets.
//! Aging keeps the store bounded: once the summed rank passes 9000 every rank is
//! scaled by 0.99 and sub-1.0 entries are dropped — exactly like `z`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_NAME: &str = "recent_files";
const MAX_ENTRIES: usize = 50;
/// `z`'s aging threshold: when total rank exceeds this, scale everything down.
const AGING_THRESHOLD: f64 = 9000.0;

/// Emacs `recentf-mode`: whether opening a file records it in the store. On by
/// default (zmax has always tracked); `recentf-mode` turns tracking off and on,
/// and [`record`] — the one write path, driven by the `DocumentDidOpen` hook in
/// `handlers::recent_files` — obeys it.
static TRACKING: AtomicBool = AtomicBool::new(true);

struct Entry {
    path: PathBuf,
    rank: f64,
    time: u64,
}

fn store_path() -> PathBuf {
    zmax_loader::config_dir().join(FILE_NAME)
}

/// The file the recent-files list is stored in (`recentf-save-file`).
pub fn store_file() -> PathBuf {
    store_path()
}

/// Is `recentf-mode` on — i.e. does opening a file record it?
pub fn tracking() -> bool {
    TRACKING.load(Ordering::Relaxed)
}

/// Turn recording of opened files on or off (`recentf-mode`). Returns the new state.
pub fn set_tracking(on: bool) -> bool {
    TRACKING.store(on, Ordering::Relaxed);
    on
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The `z` frecency score: frequency (`rank`) weighted by recency via the
/// classic rupa/z buckets — within an hour ×4, a day ×2, a week ÷2, else ÷4.
fn frecency(rank: f64, age_secs: u64) -> f64 {
    if age_secs < 3600 {
        rank * 4.0
    } else if age_secs < 86_400 {
        rank * 2.0
    } else if age_secs < 604_800 {
        rank / 2.0
    } else {
        rank / 4.0
    }
}

/// Parse the store, dropping entries whose files no longer exist on disk so no
/// consumer ever offers a dead path. A line is `path[\trank[\ttime]]`.
fn load_entries() -> Vec<Entry> {
    let Ok(contents) = std::fs::read_to_string(store_path()) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let mut parts = line.split('\t');
            let path = PathBuf::from(parts.next()?);
            let rank = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let time = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            Some(Entry { path, rank, time })
        })
        .filter(|e| e.path.is_file())
        .collect()
}

fn write_entries(entries: &[Entry]) -> std::io::Result<()> {
    let body = entries
        .iter()
        .map(|e| format!("{}\t{}\t{}", e.path.to_string_lossy(), e.rank, e.time))
        .collect::<Vec<_>>()
        .join("\n");
    let store = store_path();
    if let Some(parent) = store.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(store, body)
}

/// Emacs `recentf-save-list`: write the recent-files list out to
/// [`store_file`]. The list that gets written is the live one — dead paths
/// (files deleted or moved since they were recorded) are dropped by
/// [`load_entries`], so saving also purges them from the store, and legacy
/// bare-path lines are rewritten in the current `path\trank\ttime` form.
/// Returns how many entries were written.
pub fn save_list() -> std::io::Result<usize> {
    let mut entries = load_entries();
    entries.sort_by_key(|b| std::cmp::Reverse(b.time));
    entries.truncate(MAX_ENTRIES);
    write_entries(&entries)?;
    Ok(entries.len())
}

/// Drop `paths` from the store and persist (`recentf-edit-list`'s deletions).
/// Returns how many entries were actually removed.
pub fn remove(paths: &[PathBuf]) -> std::io::Result<usize> {
    let mut entries = load_entries();
    let before = entries.len();
    entries.retain(|e| !paths.contains(&e.path));
    let removed = before - entries.len();
    if removed > 0 {
        write_entries(&entries)?;
    }
    Ok(removed)
}

/// Load the recent-files list, newest first (pure recency / MRU order).
pub fn load() -> Vec<PathBuf> {
    let mut entries = load_entries();
    entries.sort_by_key(|b| std::cmp::Reverse(b.time));
    entries.truncate(MAX_ENTRIES);
    entries.into_iter().map(|e| e.path).collect()
}

/// Like [`load`] but pairs each path with its unix access time (0 for legacy
/// stores that predate timestamps). Newest first. Used to annotate the RECENT
/// tab with a relative-age column.
pub fn load_with_time() -> Vec<(PathBuf, u64)> {
    let mut entries = load_entries();
    entries.sort_by_key(|b| std::cmp::Reverse(b.time));
    entries.truncate(MAX_ENTRIES);
    entries.into_iter().map(|e| (e.path, e.time)).collect()
}

/// Seconds elapsed since `time` (a unix timestamp), saturating at 0.
pub fn age_since(time: u64) -> u64 {
    now().saturating_sub(time)
}

/// Compact human-readable age for a duration in seconds: `now`, `5m`, `3h`,
/// `2d`, `4w`. Used to annotate the RECENT tab.
pub fn humanize_age(age_secs: u64) -> String {
    if age_secs < 60 {
        "now".into()
    } else if age_secs < 3600 {
        format!("{}m", age_secs / 60)
    } else if age_secs < 86_400 {
        format!("{}h", age_secs / 3600)
    } else if age_secs < 604_800 {
        format!("{}d", age_secs / 86_400)
    } else {
        format!("{}w", age_secs / 604_800)
    }
}

/// Load the file list ranked by `z` frecency (frequency × recency), best first.
pub fn load_frecent() -> Vec<PathBuf> {
    let t = now();
    let mut entries = load_entries();
    entries.sort_by(|a, b| {
        let sb = frecency(b.rank, t.saturating_sub(b.time));
        let sa = frecency(a.rank, t.saturating_sub(a.time));
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    entries.truncate(MAX_ENTRIES);
    entries.into_iter().map(|e| e.path).collect()
}

/// Record `path` as a hit: bump its rank, stamp the access time, age the store
/// if it has grown too heavy, and persist. Non-files are ignored, and nothing is
/// recorded while `recentf-mode` is off (see [`tracking`]).
pub fn record(path: &Path) {
    if !tracking() || !path.is_file() {
        return;
    }
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let t = now();

    let mut entries = load_entries();
    if let Some(e) = entries.iter_mut().find(|e| e.path == path) {
        e.rank += 1.0;
        e.time = t;
    } else {
        entries.push(Entry {
            path,
            rank: 1.0,
            time: t,
        });
    }

    // `z` aging: once the store is heavy, decay every rank and drop the dregs.
    let total: f64 = entries.iter().map(|e| e.rank).sum();
    if total > AGING_THRESHOLD {
        for e in &mut entries {
            e.rank *= 0.99;
        }
        entries.retain(|e| e.rank >= 1.0);
    }

    // Persist newest-first, capped.
    entries.sort_by_key(|b| std::cmp::Reverse(b.time));
    entries.truncate(MAX_ENTRIES);
    let _ = write_entries(&entries);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frecency_buckets_match_z() {
        // Same rank, more-recent access scores strictly higher across buckets.
        let r = 10.0;
        let within_hour = frecency(r, 60);
        let within_day = frecency(r, 7200);
        let within_week = frecency(r, 200_000);
        let older = frecency(r, 1_000_000);
        assert!(within_hour > within_day);
        assert!(within_day > within_week);
        assert!(within_week > older);
        assert_eq!(within_hour, r * 4.0);
        assert_eq!(older, r / 4.0);
    }

    #[test]
    fn humanize_age_buckets() {
        assert_eq!(humanize_age(0), "now");
        assert_eq!(humanize_age(59), "now");
        assert_eq!(humanize_age(60), "1m");
        assert_eq!(humanize_age(3599), "59m");
        assert_eq!(humanize_age(3600), "1h");
        assert_eq!(humanize_age(7200), "2h");
        assert_eq!(humanize_age(86_400), "1d");
        assert_eq!(humanize_age(259_200), "3d");
        assert_eq!(humanize_age(604_800), "1w");
        assert_eq!(humanize_age(1_209_600), "2w");
    }

    /// `recentf-mode` off must actually stop the recording hook — the flag is
    /// read by `record`, the single write path, so no store write happens at all.
    #[test]
    fn recentf_mode_off_stops_recording() {
        let probe = std::env::temp_dir().join(format!("zmax-recentf-probe-{}", std::process::id()));
        std::fs::write(&probe, "x").expect("temp file");
        let canonical = std::fs::canonicalize(&probe).expect("canonical temp path");

        let previous = tracking();
        set_tracking(false);
        record(&probe);
        assert!(
            !load().contains(&canonical),
            "recentf-mode off must not record an opened file"
        );

        set_tracking(previous);
        let _ = std::fs::remove_file(&probe);
    }

    #[test]
    fn frequency_outranks_when_equally_recent() {
        // Two files touched "now": the more frequently used one wins.
        let t = 0; // age 0 for both → same bucket
        let hot = frecency(50.0, t);
        let cold = frecency(2.0, t);
        assert!(hot > cold);
    }
}
