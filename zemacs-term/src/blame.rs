//! Git blame data behind two views of the same cache:
//!   * a GitLens-style current-line hint ("Author, <relative time> · <summary>")
//!     shown as an idle status hint by the editor, toggled with
//!     `toggle_inline_blame`; and
//!   * the JetBrains "Annotate" gutter column (`toggle_blame_annotate`), rendered
//!     by `zemacs_view`'s gutter (which can't shell out to git itself, so we
//!     compute here and push the formatted lines across the crate boundary).
//!
//! Both derive from a per-file cache of `git blame --porcelain`, populated lazily
//! on first request.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Whether the idle current-line blame hint is shown.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// path -> structured per-line blame (index 0 = line 1).
static CACHE: Mutex<Option<HashMap<PathBuf, Vec<BlameLine>>>> = Mutex::new(None);

/// One blamed line's commit metadata.
#[derive(Clone)]
struct BlameLine {
    author: String,
    time: i64,
    summary: String,
    uncommitted: bool,
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Toggle the idle current-line hint; returns the new state.
pub fn toggle() -> bool {
    !ENABLED.fetch_xor(true, Ordering::Relaxed)
}

/// Whether the blame annotate gutter is enabled (state lives in `zemacs_view`,
/// which owns the gutter).
pub fn annotate_enabled() -> bool {
    zemacs_view::gutter::blame_gutter_enabled()
}

/// Toggle the blame annotate gutter; returns the new state.
pub fn toggle_annotate() -> bool {
    let on = !annotate_enabled();
    zemacs_view::gutter::set_blame_gutter(on);
    on
}

/// When the annotate gutter is on and `path` has no cached annotate lines yet,
/// compute them and push them to the gutter. Cheap no-op once cached.
pub fn ensure_annotate(path: &Path) {
    if !annotate_enabled() || zemacs_view::gutter::has_blame_annotate(path) {
        return;
    }
    let lines = blame_lines(path)
        .into_iter()
        .map(|b| {
            if b.uncommitted {
                "You uncommitted".to_string()
            } else {
                let when = crate::recent_files::humanize_age(crate::recent_files::age_since(
                    b.time.max(0) as u64,
                ));
                format!("{} {when}", b.author)
            }
        })
        .collect();
    zemacs_view::gutter::set_blame_annotate(path.to_path_buf(), lines);
}

/// Drop the cached blame for `path` (call after it's saved/edited).
pub fn invalidate(path: &Path) {
    if let Ok(mut g) = CACHE.lock() {
        if let Some(m) = g.as_mut() {
            m.remove(path);
        }
    }
    zemacs_view::gutter::invalidate_blame_annotate(path);
}

/// GitLens-style blame string for `line` (1-based) of `path`. `None` if not in a
/// git repo or the line is out of range.
pub fn line_blame(path: &Path, line: usize) -> Option<String> {
    let mut guard = CACHE.lock().ok()?;
    let map = guard.get_or_insert_with(HashMap::new);
    if !map.contains_key(path) {
        map.insert(path.to_path_buf(), compute(path).unwrap_or_default());
    }
    let b = map.get(path)?.get(line.saturating_sub(1))?;
    Some(if b.uncommitted {
        "You · Uncommitted changes".to_string()
    } else {
        let when =
            crate::recent_files::humanize_age(crate::recent_files::age_since(b.time.max(0) as u64));
        format!("{}, {when} · {}", b.author, b.summary)
    })
}

/// Cached structured blame for `path`, computing on first use.
fn blame_lines(path: &Path) -> Vec<BlameLine> {
    let Ok(mut guard) = CACHE.lock() else {
        return Vec::new();
    };
    let map = guard.get_or_insert_with(HashMap::new);
    if !map.contains_key(path) {
        map.insert(path.to_path_buf(), compute(path).unwrap_or_default());
    }
    map.get(path).cloned().unwrap_or_default()
}

fn compute(path: &Path) -> Option<Vec<BlameLine>> {
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
            let uncommitted = author == "Not Committed Yet" || sha.starts_with("00000000");
            meta.insert(sha.clone(), (author.clone(), time, summary.clone()));
            lines.push(BlameLine {
                author: author.clone(),
                time,
                summary: summary.clone(),
                uncommitted,
            });
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
