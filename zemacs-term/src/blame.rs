//! GitLens-style current-line blame: a per-file cache of `git blame --porcelain`
//! output, formatted as "Author, <relative time> · <summary>" per line. Populated
//! lazily on first request for a file and shown (when enabled) as an idle status
//! hint by the editor. Toggled with `toggle_inline_blame`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Whether the idle blame hint is shown.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// path -> per-line blame strings (index 0 = line 1).
static CACHE: Mutex<Option<HashMap<PathBuf, Vec<String>>>> = Mutex::new(None);

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Toggle the feature; returns the new state.
pub fn toggle() -> bool {
    !ENABLED.fetch_xor(true, Ordering::Relaxed)
}

/// Drop the cached blame for `path` (call after it's saved/edited).
pub fn invalidate(path: &Path) {
    if let Ok(mut g) = CACHE.lock() {
        if let Some(m) = g.as_mut() {
            m.remove(path);
        }
    }
}

/// Blame string for `line` (1-based) of `path`, computing + caching the whole file
/// on first use. `None` if not in a git repo or the line is out of range.
pub fn line_blame(path: &Path, line: usize) -> Option<String> {
    let mut guard = CACHE.lock().ok()?;
    let map = guard.get_or_insert_with(HashMap::new);
    if !map.contains_key(path) {
        let blames = compute(path).unwrap_or_default();
        map.insert(path.to_path_buf(), blames);
    }
    map.get(path)?.get(line.saturating_sub(1)).cloned()
}

fn compute(path: &Path) -> Option<Vec<String>> {
    let dir = path.parent()?;
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["blame", "--porcelain", "--"])
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Porcelain: each blamed line begins with "<sha> <orig> <final> [count]"; the
    // first line of each commit is followed by a header block (author,
    // author-time, summary, …); repeats of a commit only carry the sha line. The
    // blamed content line starts with a tab.
    let mut lines = Vec::new();
    let mut meta: HashMap<String, (String, i64, String)> = HashMap::new();
    let (mut sha, mut author, mut time, mut summary) =
        (String::new(), String::new(), 0i64, String::new());
    for l in text.lines() {
        if let Some(content_line) = l.strip_prefix('\t') {
            let _ = content_line;
            let s = if author == "Not Committed Yet" || sha.starts_with("00000000") {
                "You · Uncommitted changes".to_string()
            } else {
                let when = crate::recent_files::humanize_age(crate::recent_files::age_since(
                    time.max(0) as u64,
                ));
                format!("{author}, {when} · {summary}")
            };
            meta.insert(sha.clone(), (author.clone(), time, summary.clone()));
            lines.push(s);
        } else if let Some(rest) = l.strip_prefix("author ") {
            author = rest.to_string();
        } else if let Some(rest) = l.strip_prefix("author-time ") {
            time = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = l.strip_prefix("summary ") {
            summary = rest.to_string();
        } else {
            let tok = l.split_whitespace().next().unwrap_or("");
            if tok.len() == 40 && tok.bytes().all(|b| b.is_ascii_hexdigit()) {
                sha = tok.to_string();
                if let Some((a, t, s)) = meta.get(tok) {
                    author = a.clone();
                    time = *t;
                    summary = s.clone();
                }
            }
        }
    }
    Some(lines)
}
