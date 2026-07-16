//! CI status: fetch recent GitHub Actions runs for the current repo via the
//! GitHub REST API (over `ureq`, blocking — call from a `spawn_blocking` task,
//! never the UI thread). Results live in a process-global so both the IDE CI
//! panel and the statusline segment can read them without threading state
//! through the editor.

use std::sync::Mutex;

use serde::Deserialize;

/// One workflow run, flattened for display.
#[derive(Clone, Debug)]
pub struct CiRun {
    pub status: String,             // queued | in_progress | completed
    pub conclusion: Option<String>, // success | failure | cancelled | … (None while running)
    pub workflow: String,
    pub title: String,
    pub branch: String,
    pub sha: String,
    pub created_at: String,
    pub url: String,
}

impl CiRun {
    /// Status glyph + theme scope key for colouring.
    pub fn icon(&self) -> (&'static str, &'static str) {
        if self.status != "completed" {
            return ("●", "warning"); // running / queued
        }
        match self.conclusion.as_deref() {
            Some("success") => ("✓", "diff.plus"),
            Some("failure") | Some("timed_out") | Some("startup_failure") => ("✗", "error"),
            Some("cancelled") | Some("skipped") | Some("neutral") => ("○", "comment"),
            _ => ("·", "comment"),
        }
    }

    pub fn short_sha(&self) -> String {
        self.sha.chars().take(7).collect()
    }

    /// Human "2m ago" from the RFC3339 `created_at`, best-effort.
    pub fn age(&self) -> String {
        crate::ci::age_of(&self.created_at)
    }
}

struct CiState {
    runs: Vec<CiRun>,
    error: Option<String>,
    loading: bool,
    fetched: bool,
}

static STATE: Mutex<CiState> = Mutex::new(CiState {
    runs: Vec::new(),
    error: None,
    loading: false,
    fetched: false,
});

/// Snapshot of the current runs (clone, so callers don't hold the lock).
pub fn snapshot() -> Vec<CiRun> {
    STATE.lock().map(|s| s.runs.clone()).unwrap_or_default()
}

/// `(loading, error)` for status/empty rendering.
pub fn status() -> (bool, Option<String>) {
    STATE
        .lock()
        .map(|s| (s.loading, s.error.clone()))
        .unwrap_or((false, None))
}

/// Has a fetch ever completed (success or error)?
pub fn fetched() -> bool {
    STATE.lock().map(|s| s.fetched).unwrap_or(false)
}

pub fn set_loading(v: bool) {
    if let Ok(mut s) = STATE.lock() {
        s.loading = v;
    }
}

/// Store a fetch result (clears loading, marks fetched).
pub fn store(result: Result<Vec<CiRun>, String>) {
    if let Ok(mut s) = STATE.lock() {
        s.loading = false;
        s.fetched = true;
        match result {
            Ok(runs) => {
                s.runs = runs;
                s.error = None;
            }
            Err(e) => s.error = Some(e),
        }
    }
}

/// Latest run's glyph + theme key for the statusline (None if nothing fetched).
pub fn latest_badge() -> Option<(&'static str, &'static str)> {
    STATE
        .lock()
        .ok()
        .and_then(|s| s.runs.first().map(|r| r.icon()))
}

/// Kick off an async fetch into the global cache (ureq is blocking, so the work
/// runs on a blocking task). The returned job callback runs on the main loop,
/// which triggers a redraw once the runs land. Sets `loading` immediately so a
/// per-frame trigger won't spawn duplicates.
pub fn spawn_fetch(jobs: &mut crate::job::Jobs) {
    set_loading(true);
    jobs.callback(async move {
        let runs = tokio::task::spawn_blocking(fetch_blocking)
            .await
            .unwrap_or_else(|e| Err(format!("join error: {e}")));
        let call: crate::job::Callback =
            crate::job::Callback::EditorCompositor(Box::new(move |_editor, _compositor| {
                store(runs);
            }));
        Ok(call)
    });
}

// ── fetching (blocking — run on spawn_blocking) ──────────────────────────────

#[derive(Deserialize)]
struct ApiResp {
    workflow_runs: Vec<ApiRun>,
}

#[derive(Deserialize)]
struct ApiRun {
    name: String,
    #[serde(default)]
    display_title: String,
    status: String,
    conclusion: Option<String>,
    #[serde(default)]
    head_branch: String,
    #[serde(default)]
    head_sha: String,
    created_at: String,
    html_url: String,
}

/// `owner/repo` from the `origin` remote of the repo containing `cwd`.
/// Uses a one-shot `git` invocation only to *locate* the repo (not for CI data).
fn repo_slug() -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err("no git origin remote".into());
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    parse_slug(&url).ok_or_else(|| format!("can't parse owner/repo from {url}"))
}

/// Parse `owner/repo` from an https or ssh GitHub remote URL.
fn parse_slug(url: &str) -> Option<String> {
    let s = url.trim().trim_end_matches(".git");
    let rest = if let Some(r) = s.strip_prefix("git@github.com:") {
        r
    } else if let Some(r) = s.strip_prefix("https://github.com/") {
        r
    } else {
        s.strip_prefix("ssh://git@github.com/")?
    };
    let mut it = rest.split('/');
    let owner = it.next()?;
    let repo = it.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

/// Blocking fetch of recent runs. Call from `tokio::task::spawn_blocking`.
pub fn fetch_blocking() -> Result<Vec<CiRun>, String> {
    let slug = repo_slug()?;
    let url = format!("https://api.github.com/repos/{slug}/actions/runs?per_page=20");
    let mut req = ureq::get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "zmax-ci")
        .set("X-GitHub-Api-Version", "2022-11-28");
    if let Some(tok) = std::env::var("GITHUB_TOKEN")
        .ok()
        .or_else(|| std::env::var("GH_TOKEN").ok())
        .filter(|t| !t.is_empty())
    {
        req = req.set("Authorization", &format!("Bearer {tok}"));
    }
    let body = req
        .call()
        .map_err(|e| format!("{e}"))?
        .into_string()
        .map_err(|e| format!("{e}"))?;
    let resp: ApiResp = serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))?;
    Ok(resp
        .workflow_runs
        .into_iter()
        .map(|r| CiRun {
            status: r.status,
            conclusion: r.conclusion,
            workflow: r.name,
            title: r.display_title,
            branch: r.head_branch,
            sha: r.head_sha,
            created_at: r.created_at,
            url: r.html_url,
        })
        .collect())
}

/// Crude RFC3339 → "Nm ago" without pulling chrono: parse the timestamp to a
/// unix epoch and diff against `SystemTime::now()`.
fn age_of(rfc3339: &str) -> String {
    let Some(epoch) = parse_epoch(rfc3339) else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let d = now - epoch;
    if d < 0 {
        "now".into()
    } else if d < 60 {
        format!("{d}s ago")
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else if d < 86400 {
        format!("{}h ago", d / 3600)
    } else {
        format!("{}d ago", d / 86400)
    }
}

/// Parse `YYYY-MM-DDTHH:MM:SSZ` to a unix timestamp (UTC, no leap seconds).
fn parse_epoch(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, mo, da) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    // days since 1970 via a civil-calendar algorithm (Howard Hinnant's).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_parsing() {
        assert_eq!(
            parse_slug("https://github.com/o/r.git").as_deref(),
            Some("o/r")
        );
        assert_eq!(parse_slug("git@github.com:o/r.git").as_deref(), Some("o/r"));
        assert_eq!(parse_slug("https://gitlab.com/o/r"), None);
    }

    #[test]
    fn epoch_parsing() {
        // 2021-01-01T00:00:00Z == 1609459200
        assert_eq!(parse_epoch("2021-01-01T00:00:00Z"), Some(1609459200));
    }
}
