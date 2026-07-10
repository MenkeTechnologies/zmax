//! Sync the zemacs colorscheme to the zwire terminal host's active scheme.
//!
//! zwire records its current UI theme in `~/.zwire/global.toml`:
//!
//! ```toml
//! [theme]
//! scheme = "midnight"
//!
//! [theme.ui]
//! light = true
//! ```
//!
//! zemacs ships a `zgui-<scheme>` theme (plus a `zgui-<scheme>-light` variant)
//! for every zwire scheme, so the mapping is `scheme` -> `zgui-<scheme>`, with a
//! `-light` suffix when zwire's UI is in light mode. When the `sync-zwire-theme`
//! editor setting is on, zemacs follows that scheme at startup, on
//! `:config-reload`, and live via a filesystem watcher on `global.toml`. The
//! scheme name is used verbatim (no hardcoded scheme list), so a new zwire
//! scheme with a matching `zgui-<name>` theme works with no code change; an
//! unknown scheme simply fails to load and the caller keeps the current theme.
//!
//! Live sync is a dedicated `notify` watcher (mirroring [`crate::file_watcher`])
//! that owns an OS thread and, on a change to `global.toml`, hops onto the main
//! thread via [`crate::job::dispatch_blocking`] to re-apply the theme. The event
//! loop renders right after each dispatched callback, so the scheme change lands
//! immediately even while zemacs is otherwise idle — no keypress or focus event
//! required.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use zemacs_view::editor::Editor;

#[derive(Deserialize)]
struct Global {
    theme: Option<ThemeSection>,
}

#[derive(Deserialize)]
struct ThemeSection {
    scheme: Option<String>,
    ui: Option<ThemeUi>,
}

#[derive(Deserialize)]
struct ThemeUi {
    #[serde(default)]
    light: bool,
}

fn global_toml_path() -> Option<PathBuf> {
    Some(
        zemacs_stdx::path::home_dir()
            .ok()?
            .join(".zwire")
            .join("global.toml"),
    )
}

/// Resolve the zemacs theme name that mirrors zwire's active scheme, or `None`
/// if `~/.zwire/global.toml` is absent/unreadable or names no scheme.
pub fn theme_name() -> Option<String> {
    let body = std::fs::read_to_string(global_toml_path()?).ok()?;
    theme_name_from_toml(&body)
}

/// Map a `~/.zwire/global.toml` body to the zemacs theme name. Split from the
/// file read so the scheme -> `zgui-*` mapping is unit-testable.
fn theme_name_from_toml(body: &str) -> Option<String> {
    let theme = toml::from_str::<Global>(body).ok()?.theme?;
    let scheme = theme.scheme?;
    // Reject anything that isn't a plain scheme token so a hand-edited
    // global.toml can't steer the theme loader outside its theme dirs (e.g. a
    // `../` path). Theme names are otherwise loaded by filename.
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let light = theme.ui.map(|ui| ui.light).unwrap_or(false);
    Some(if light {
        format!("zgui-{scheme}-light")
    } else {
        format!("zgui-{scheme}")
    })
}

/// Re-apply the zwire scheme to `editor` when `sync-zwire-theme` is on and
/// `global.toml` now names a different, loadable theme. Runs on the main thread
/// (via the watcher's [`crate::job::dispatch_blocking`]) so it may touch the
/// editor directly. A no-op when the setting is off, the file names no usable
/// scheme, or the theme is already active.
pub fn apply(editor: &mut Editor) {
    if !editor.config().sync_zwire_theme {
        return;
    }
    let Some(name) = theme_name() else {
        return;
    };
    if editor.theme.name() == name {
        return;
    }
    let true_color = editor.config().true_color || crate::true_color();
    match editor.theme_loader.load(&name) {
        Ok(theme) if true_color || theme.is_16_color() => {
            let _ = editor.set_theme(theme);
        }
        // Scheme names no shipped `zgui-*` theme, or true color is unavailable:
        // ignore it and keep the current theme.
        Ok(_) => {}
        Err(e) => log::debug!("zwire theme `{}` not loadable, keeping current: {}", name, e),
    }
}

/// Ensures we only ever spawn a single zwire watcher for the process.
static SPAWNED: AtomicBool = AtomicBool::new(false);

/// Start the live watcher on `~/.zwire/global.toml`. Idempotent: only the first
/// call spawns the OS thread. Spawned unconditionally at startup; the dispatched
/// [`apply`] gates on the `sync-zwire-theme` setting, so toggling it via
/// `:config-reload` takes effect without restarting the watcher.
pub fn spawn_watcher() {
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(path) = global_toml_path() else {
        return;
    };
    std::thread::Builder::new()
        .name("zwire-theme-watcher".into())
        .spawn(move || run_watcher(path))
        .ok();
}

fn run_watcher(global_toml: PathBuf) {
    // Watch the containing dir (not the file) non-recursively so the watch
    // survives zwire replacing `global.toml` via a temp-file rename, then filter
    // events down to `global.toml` — `~/.zwire` also holds a busy `hostlog.jsonl`
    // whose churn must not drive theme reloads.
    let Some(dir) = global_toml.parent().map(Path::to_path_buf) else {
        return;
    };
    let (tx, rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            log::warn!("zwire theme watcher unavailable: {err}");
            return;
        }
    };
    if let Err(err) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        log::warn!("could not watch {}: {err}", dir.display());
        return;
    }

    let touches_global_toml = |res: &notify::Result<notify::Event>| -> bool {
        matches!(res, Ok(event) if event.paths.iter().any(|p| *p == global_toml))
    };

    loop {
        let first = match rx.recv() {
            Ok(event) => event,
            Err(_) => return, // sender dropped — watcher gone
        };
        let mut relevant = touches_global_toml(&first);
        // Coalesce a burst (an editor writing scheme + light in quick succession)
        // into a single re-apply.
        while let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
            relevant |= touches_global_toml(&event);
        }
        if relevant {
            crate::job::dispatch_blocking(|editor, _compositor| apply(editor));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::theme_name_from_toml;

    #[test]
    fn light_ui_selects_light_variant() {
        let toml = "[theme]\nscheme = \"midnight\"\n\n[theme.ui]\nlight = true\n";
        assert_eq!(theme_name_from_toml(toml).as_deref(), Some("zgui-midnight-light"));
    }

    #[test]
    fn dark_ui_selects_base_variant() {
        let toml = "[theme]\nscheme = \"cyberpunk\"\n\n[theme.ui]\nlight = false\n";
        assert_eq!(theme_name_from_toml(toml).as_deref(), Some("zgui-cyberpunk"));
    }

    #[test]
    fn missing_ui_section_defaults_to_dark() {
        assert_eq!(
            theme_name_from_toml("[theme]\nscheme = \"ember\"\n").as_deref(),
            Some("zgui-ember"),
        );
    }

    #[test]
    fn no_scheme_yields_none() {
        assert_eq!(theme_name_from_toml("[theme.ui]\nlight = true\n"), None);
        assert_eq!(theme_name_from_toml(""), None);
    }

    #[test]
    fn path_traversal_scheme_is_rejected() {
        let toml = "[theme]\nscheme = \"../../etc/passwd\"\n";
        assert_eq!(theme_name_from_toml(toml), None);
    }
}
