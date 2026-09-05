//! CI status cache: the repository's recent GitHub Actions runs, shared by the
//! statusline badge and the IDE's CI panel.
//!
//! Transport, models and parsing live in [`crate::github`] — this module is only
//! the process-global cache and the fetch job, so both readers see the same runs
//! without threading state through the editor. The full browser (`:github`) uses
//! the same layer.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::github::{self, Run};

/// One workflow run, as displayed. The CI panel and the `:github` browser share
/// the model so a run means the same thing in both.
pub type CiRun = Run;

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

/// The 20 most recent runs for the repository containing the working directory.
/// Blocking — call it on a blocking task.
pub fn fetch_blocking() -> Result<Vec<CiRun>, String> {
    let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let slug = github::repo_slug(&dir)?;
    github::api(&format!("repos/{slug}/actions/runs?per_page=20")).map(|v| Run::parse_list(&v))
}

/// Kick off an async fetch into the global cache. The returned job callback runs
/// on the main loop, which triggers a redraw once the runs land. Sets `loading`
/// immediately so a per-frame trigger won't spawn duplicates.
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
