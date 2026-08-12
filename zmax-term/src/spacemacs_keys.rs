//! State behind the Spacemacs leader keys whose ports had no existing home:
//! the mode-line minor-mode area (`SPC t m …`), the two mode-line lighters
//! Spacemacs draws next to it, the `ggtags`/`yasnippet` minor modes those
//! lighters report, and smeargle's commit-age highlighting (`SPC g H …`).
//!
//! Everything here is process-global because the mode line and the highlight
//! overlay are drawn from render code that has no editor state of its own —
//! the same shape `commands::display_time_flag` and `blame::ENABLED` use.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Minor modes the mode-line lighter area reports.
// ---------------------------------------------------------------------------

/// `SPC t G` (ggtags-mode): when on, `goto_definition` consults the visited
/// etags table before asking a language server, which is what ggtags-mode does
/// to `xref` in Emacs. Off by default, like ggtags-mode itself.
static GGTAGS: AtomicBool = AtomicBool::new(false);

/// `SPC t y` (yasnippet-mode): when off, a snippet key stops expanding. On by
/// default, matching Spacemacs (which enables yasnippet in the auto-completion
/// layer and lists `ⓨ` as a lit lighter).
static YASNIPPET: AtomicBool = AtomicBool::new(true);

pub fn ggtags_enabled() -> bool {
    GGTAGS.load(Ordering::Relaxed)
}

/// Toggle ggtags-mode; returns the new state.
pub fn toggle_ggtags() -> bool {
    !GGTAGS.fetch_xor(true, Ordering::Relaxed)
}

pub fn yasnippet_enabled() -> bool {
    YASNIPPET.load(Ordering::Relaxed)
}

/// Toggle yasnippet-mode; returns the new state.
pub fn toggle_yasnippet() -> bool {
    !YASNIPPET.fetch_xor(true, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Mode-line constructs (`SPC t m r`, `SPC t m V`, `SPC t m c`).
// ---------------------------------------------------------------------------

/// `SPC t m r` (spaceline's `spaceline-toggle-*-responsive`): when on, the
/// status line drops its optional right-hand constructs once they no longer fit
/// the window, instead of letting them push the file name off the left.
static MODELINE_RESPONSIVE: AtomicBool = AtomicBool::new(false);

/// `SPC t m V` (`spacemacs/toggle-mode-line-new-version`): show a lighter when a
/// newer release than the running one is known.
static NEW_VERSION_LIGHTER: AtomicBool = AtomicBool::new(false);

/// `SPC t m c` (`spacemacs/toggle-mode-line-org-clock`): show the running org
/// task clock in the mode line.
static MODELINE_ORG_CLOCK: AtomicBool = AtomicBool::new(false);

/// The heading `org-clock-in` is currently timing and the unix time it started,
/// or `None` when no clock runs. Written by `org_clock_toggle`.
static ORG_CLOCK: Mutex<Option<(String, u64)>> = Mutex::new(None);

/// The newest release seen by an update check, when it is newer than the running
/// build. `None` until something records one.
static NEW_VERSION: Mutex<Option<String>> = Mutex::new(None);

pub fn modeline_responsive() -> bool {
    MODELINE_RESPONSIVE.load(Ordering::Relaxed)
}

/// Toggle mode-line responsiveness; returns the new state.
pub fn toggle_modeline_responsive() -> bool {
    !MODELINE_RESPONSIVE.fetch_xor(true, Ordering::Relaxed)
}

/// Toggle the new-version lighter; returns the new state.
pub fn toggle_new_version_lighter() -> bool {
    !NEW_VERSION_LIGHTER.fetch_xor(true, Ordering::Relaxed)
}

/// Record the newest known release (an update check's result). Clearing it with
/// `None` takes the lighter down again.
pub fn set_new_version(version: Option<String>) {
    if let Ok(mut g) = NEW_VERSION.lock() {
        *g = version;
    }
}

/// The new-version lighter's text, or `None` when the lighter is off or no newer
/// release is known.
pub fn new_version_text() -> Option<String> {
    if !NEW_VERSION_LIGHTER.load(Ordering::Relaxed) {
        return None;
    }
    let newest = NEW_VERSION.lock().ok()?.clone()?;
    Some(format!("⇪{newest}"))
}

/// Toggle the org-clock mode-line construct; returns the new state.
pub fn toggle_modeline_org_clock() -> bool {
    !MODELINE_ORG_CLOCK.fetch_xor(true, Ordering::Relaxed)
}

/// Start (`Some(heading)`) or stop (`None`) the mode line's org clock. Called by
/// `org-clock-in` / `org-clock-out` so the construct tracks the real clock.
pub fn set_org_clock(heading: Option<String>) {
    if let Ok(mut g) = ORG_CLOCK.lock() {
        *g = heading.map(|h| (h, now_secs()));
    }
}

/// The org-clock construct's text (`⏱ 0:12 heading`), or `None` when the
/// construct is off or no clock is running.
pub fn org_clock_text() -> Option<String> {
    if !MODELINE_ORG_CLOCK.load(Ordering::Relaxed) {
        return None;
    }
    let (heading, started) = ORG_CLOCK.lock().ok()?.clone()?;
    let elapsed = now_secs().saturating_sub(started);
    Some(format!(
        "⏱ {}:{:02} {heading}",
        elapsed / 3600,
        (elapsed % 3600) / 60
    ))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// smeargle (`SPC g H h` / `SPC g H t` / `SPC g H c`).
// ---------------------------------------------------------------------------

/// Which of smeargle's two colourings is painting, if either.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Smeargle {
    /// `smeargle-commits`: rank the commits that touched the file by age and
    /// colour each line by its commit's rank, so the oldest commit and the
    /// newest are always at the ends of the palette.
    Commits,
    /// `smeargle`: colour each line by how long ago it was last updated, in
    /// fixed time bands, so two files can be compared against each other.
    Time,
}

/// The active colouring; `None` after `smeargle-clear`.
static SMEARGLE: Mutex<Option<Smeargle>> = Mutex::new(None);

pub fn smeargle_mode() -> Option<Smeargle> {
    SMEARGLE.lock().ok().and_then(|g| *g)
}

/// Turn a colouring on (replacing whichever was on) or, with `None`, clear it.
pub fn set_smeargle(mode: Option<Smeargle>) {
    if let Ok(mut g) = SMEARGLE.lock() {
        *g = mode;
    }
}

/// smeargle's palette: 6 bands, oldest first. Theme scopes rather than literal
/// colours so the bands stay legible in every theme.
pub const SMEARGLE_SCOPES: [&str; 6] = [
    "ui.background.separator",
    "ui.statusline.inactive",
    "ui.bufferline.background",
    "ui.statusline",
    "ui.selection",
    "ui.cursorline.primary",
];

/// The band (index into [`SMEARGLE_SCOPES`]) each line of `path` falls in, or an
/// empty vector when the file has no blame data. Index 0 is line 1; a line whose
/// change is not committed yet gets no band (`None`) and stays unpainted, which
/// is what smeargle does with `Not Committed Yet` lines.
pub fn line_bands(path: &Path, mode: Smeargle) -> Vec<Option<usize>> {
    let times = crate::blame::line_times(path);
    if times.is_empty() {
        return Vec::new();
    }
    let bands = SMEARGLE_SCOPES.len();
    match mode {
        // Rank the distinct commit times oldest-first, then spread the ranks
        // across the palette — `smeargle-commits`' "by age of commits".
        Smeargle::Commits => {
            let mut distinct: Vec<i64> = times
                .iter()
                .filter(|(_, uncommitted)| !uncommitted)
                .map(|(t, _)| *t)
                .collect();
            distinct.sort_unstable();
            distinct.dedup();
            if distinct.is_empty() {
                return vec![None; times.len()];
            }
            let spread = distinct.len().saturating_sub(1);
            times
                .iter()
                .map(|(t, uncommitted)| {
                    if *uncommitted {
                        return None;
                    }
                    // rank 0 is the oldest commit, `spread` the newest; a file
                    // touched by a single commit is all-newest.
                    let rank = distinct.partition_point(|d| d < t);
                    Some(if spread == 0 {
                        bands - 1
                    } else {
                        (rank * (bands - 1) / spread).min(bands - 1)
                    })
                })
                .collect()
        }
        // Fixed bands of wall-clock age: a day, a week, a month, a quarter, a
        // year, older — smeargle's "by last updated time".
        Smeargle::Time => {
            const DAY: u64 = 86_400;
            let cutoffs = [DAY, 7 * DAY, 30 * DAY, 90 * DAY, 365 * DAY];
            let now = now_secs();
            times
                .iter()
                .map(|(t, uncommitted)| {
                    if *uncommitted {
                        return None;
                    }
                    let age = now.saturating_sub((*t).max(0) as u64);
                    // Newest band last, so the palette runs oldest -> newest the
                    // same way the commit ranking does.
                    let older_than = cutoffs.iter().filter(|c| age >= **c).count();
                    Some(bands - 1 - older_than.min(bands - 1))
                })
                .collect()
        }
    }
}

// ---------------------------------------------------------------------------
// The ediff registry (`SPC D s`) and the git-link remote (`SPC u` + `SPC g l …`).
// ---------------------------------------------------------------------------

/// Every ediff/merge session started in this run, oldest first — Emacs's
/// `ediff-session-registry`. Recorded by `ui::merge::DiffView::new`, which every
/// `SPC D …` session goes through.
static EDIFF_SESSIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Register a session under the title its view carries.
pub fn record_ediff_session(title: &str) {
    if let Ok(mut g) = EDIFF_SESSIONS.lock() {
        g.push(title.to_string());
    }
}

/// The registered sessions, oldest first.
pub fn ediff_sessions() -> Vec<String> {
    EDIFF_SESSIONS.lock().map(|g| g.clone()).unwrap_or_default()
}

/// The remote the git-link commands build their URLs from. `origin` until
/// `git_link_select_remote` picks another.
static GIT_LINK_REMOTE: Mutex<Option<String>> = Mutex::new(None);

pub fn git_link_remote() -> String {
    GIT_LINK_REMOTE
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| "origin".to_string())
}

pub fn set_git_link_remote(name: String) {
    if let Ok(mut g) = GIT_LINK_REMOTE.lock() {
        *g = Some(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_clock_text_is_none_until_both_the_toggle_and_a_clock_are_on() {
        set_org_clock(None);
        assert_eq!(org_clock_text(), None, "no clock, construct off");
        set_org_clock(Some("Write the port".into()));
        assert_eq!(org_clock_text(), None, "clock runs but the construct is off");
        assert!(toggle_modeline_org_clock());
        assert!(org_clock_text().unwrap().ends_with("Write the port"));
        set_org_clock(None);
        assert_eq!(org_clock_text(), None, "clocked out");
        assert!(!toggle_modeline_org_clock());
    }

    #[test]
    fn new_version_lighter_needs_a_recorded_release() {
        set_new_version(None);
        assert!(toggle_new_version_lighter());
        assert_eq!(new_version_text(), None, "nothing newer is known");
        set_new_version(Some("25.10".into()));
        assert_eq!(new_version_text().as_deref(), Some("⇪25.10"));
        assert!(!toggle_new_version_lighter());
        assert_eq!(new_version_text(), None, "lighter off");
        set_new_version(None);
    }
}
