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
//!
//! [theme.palette]        # RESOLVED live colours, written by the fleet
//! "--accent" = "#7c3aed"
//! "--bg-primary" = "#050510"
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
//! When zwire also records `[theme.palette]` (the resolved var -> hex colours),
//! zemacs grafts those colours onto the `zgui-<scheme>` theme's face structure
//! ([`VAR_TO_PALETTE`] maps the CSS vars to the theme's palette keys). So an
//! EDITED built-in scheme or a fully custom palette — which has no `zgui-*` file —
//! still reproduces exactly in zemacs; a custom scheme falls back to cyberpunk's
//! face structure painted with the live palette.
//!
//! Live sync is a dedicated `notify` watcher (mirroring [`crate::file_watcher`])
//! that owns an OS thread and, on a change to `global.toml`, hops onto the main
//! thread via [`crate::job::dispatch_blocking`] to re-apply the theme. The event
//! loop renders right after each dispatched callback, so the scheme change lands
//! immediately even while zemacs is otherwise idle — no keypress or focus event
//! required.
//!
//! The sync is bidirectional: when the user commits a theme change inside zemacs
//! (`:theme`, the picker, `:theme-toggle`), [`write_back`] reverse-maps the
//! `zgui-*` theme to a zwire `(scheme, light)` and rewrites just those two keys
//! in `global.toml`; zwire's own watcher then fans the change out to the browser
//! /HUD. Non-app-shell themes (no `zgui-` prefix) are ignored, and a write that
//! matches what's already on disk is skipped — so the two watchers can't loop.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use zemacs_view::editor::Editor;
use zemacs_view::Theme;

#[derive(Deserialize)]
struct Global {
    theme: Option<ThemeSection>,
}

#[derive(Deserialize)]
struct ThemeSection {
    scheme: Option<String>,
    ui: Option<ThemeUi>,
    /// zwire's RESOLVED live palette (`[theme.palette]`): CSS-var → colour string.
    /// Present once any app has written colours; absent on an older global.toml.
    palette: Option<HashMap<String, String>>,
}

/// Map zwire's `[theme.palette]` CSS-var keys to the palette keys the `zgui-*`
/// zemacs themes use (their `[palette]` section — same design system, see
/// `runtime/themes/zgui-midnight.toml`). Only solid-colour vars map; zwire's
/// glow/dim `rgba()` vars have no zemacs face and are intentionally omitted.
const VAR_TO_PALETTE: &[(&str, &str)] = &[
    ("--accent", "accent"),
    ("--accent-light", "accent_light"),
    ("--cyan", "cyan"),
    ("--magenta", "magenta"),
    ("--green", "green"),
    ("--yellow", "yellow"),
    ("--orange", "orange"),
    ("--red", "red"),
    ("--text", "text"),
    ("--text-dim", "text_dim"),
    ("--text-muted", "text_muted"),
    ("--bg-primary", "bg"),
    ("--bg-secondary", "bg2"),
    ("--bg-card", "bg_card"),
    ("--bg-hover", "bg_hover"),
    ("--border", "border"),
    ("--border-glow", "border_glow"),
];

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

/// Build the palette overrides (zemacs palette-key → colour) from a `global.toml`
/// body's `[theme.palette]`, mapping via [`VAR_TO_PALETTE`]. Empty when the file
/// carries no palette (older zwire) — the caller then loads the baked theme as-is.
/// Only `#`-hex values pass through, so a stray `rgba()` can't reach the palette.
fn palette_overrides_from_toml(body: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(theme) = toml::from_str::<Global>(body).ok().and_then(|g| g.theme) else {
        return out;
    };
    let Some(palette) = theme.palette else {
        return out;
    };
    for (var, key) in VAR_TO_PALETTE {
        if let Some(hex) = palette.get(*var) {
            if hex.starts_with('#') {
                out.insert((*key).to_string(), hex.clone());
            }
        }
    }
    out
}

/// Reverse of the scheme mapping: turn a zemacs theme name back into a zwire
/// `(scheme, light)` pair, or `None` when the theme isn't an app-shell scheme.
///
/// Only `zgui-<scheme>` / `zgui-<scheme>-light` themes map back — those are the
/// ports of zwire's own colorschemes (`store.rs SCHEMES`: cyberpunk, midnight,
/// matrix, ember, arctic, crimson, toxic, vapor). Any other theme the user
/// picks (dracula, nord, a personal theme, …) has no `zgui-` prefix and is
/// ignored, so a non-app-shell zemacs theme never gets pushed to zwire.
fn scheme_from_theme(theme_name: &str) -> Option<(String, bool)> {
    let rest = theme_name.strip_prefix("zgui-")?;
    match rest.strip_suffix("-light") {
        Some(scheme) => Some((scheme.to_string(), true)),
        None => Some((rest.to_string(), false)),
    }
}

/// Push a committed zemacs theme change back to the zwire host by updating
/// `~/.zwire/global.toml`. zwire's own file watcher picks the change up and
/// fans it out to the browser/HUD. Only the `[theme] scheme` and `[theme.ui]
/// light` values are rewritten (format-preserving) — every other key zwire
/// keeps there is left untouched.
///
/// No-ops when the theme isn't an app-shell `zgui-*` scheme, or when
/// `global.toml` already holds these values (which also breaks the echo loop
/// with our own read-side watcher). Called only for committed `set_theme`s.
pub fn write_back(theme_name: &str) {
    if let Some(path) = global_toml_path() {
        write_back_to(&path, theme_name);
    }
}

/// Core of [`write_back`] against an explicit path (so it's testable without
/// touching the real `~/.zwire`). Returns `true` when it wrote the file, `false`
/// when it skipped (non-app-shell theme, or values already current).
fn write_back_to(path: &Path, theme_name: &str) -> bool {
    let Some((scheme, light)) = scheme_from_theme(theme_name) else {
        return false;
    };
    // Edit the existing document in place; if it's missing/unreadable start from
    // an empty document so the `[theme]` table is created.
    let mut doc = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.parse::<toml_edit::DocumentMut>().ok())
        .unwrap_or_default();

    // Skip the write when nothing changes — avoids churning the file (and waking
    // both zwire's watcher and ours) on every no-op theme reselection.
    let cur_scheme = doc["theme"].get("scheme").and_then(|v| v.as_str());
    let cur_light = doc["theme"]
        .get("ui")
        .and_then(|ui| ui.get("light"))
        .and_then(|v| v.as_bool());
    if cur_scheme == Some(scheme.as_str()) && cur_light == Some(light) {
        return false;
    }

    // `implicit(false)` keeps `[theme]` / `[theme.ui]` written as real headers.
    let theme = doc["theme"].or_insert(toml_edit::table());
    if let Some(t) = theme.as_table_mut() {
        t.set_implicit(false);
    }
    theme["scheme"] = toml_edit::value(scheme);
    let ui = theme["ui"].or_insert(toml_edit::table());
    if let Some(t) = ui.as_table_mut() {
        t.set_implicit(false);
    }
    ui["light"] = toml_edit::value(light);

    match write_atomic(path, doc.to_string().as_bytes()) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("zwire write-back to {} failed: {}", path.display(), e);
            false
        }
    }
}

/// Atomically replace `path`: write a sibling temp file then rename over the
/// target, so zwire's watcher never observes a half-written `global.toml`.
/// Mirrors zwire-host's own `write_atomic`.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("toml.zemacs-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Re-apply the zwire scheme to `editor` when `sync-zwire-theme` is on and
/// `global.toml` now names a different, loadable theme. Runs on the main thread
/// (via the watcher's [`crate::job::dispatch_blocking`]) so it may touch the
/// editor directly. A no-op when the setting is off, the file names no usable
/// scheme, or the theme is already active.
/// `(theme name, palette overrides)` from the current `global.toml`, or `None`
/// when the file is absent/unreadable or names no scheme.
fn theme_spec() -> Option<(String, HashMap<String, String>)> {
    let body = global_toml_path().and_then(|p| std::fs::read_to_string(p).ok())?;
    let name = theme_name_from_toml(&body)?;
    Some((name, palette_overrides_from_toml(&body)))
}

/// Load the `zgui-<scheme>` structure painted with the live palette, falling back
/// to cyberpunk's structure for a custom scheme that ships no theme file.
fn load_scheme_theme(
    editor: &Editor,
    name: &str,
    overrides: &HashMap<String, String>,
) -> anyhow::Result<Theme> {
    editor
        .theme_loader
        .load_with_palette_overrides(name, overrides)
        .or_else(|e| {
            if overrides.is_empty() {
                Err(e)
            } else {
                let base = if name.ends_with("-light") {
                    "zgui-cyberpunk-light"
                } else {
                    "zgui-cyberpunk"
                };
                editor
                    .theme_loader
                    .load_with_palette_overrides(base, overrides)
            }
        })
}

/// Resolve the theme to apply (structure + live palette), honouring the
/// terminal's colour support. Shared by startup and the live watcher so the two
/// never diverge. `None` when nothing usable is named or true colour is lacking.
pub fn resolve_theme(editor: &Editor, true_color: bool) -> Option<Theme> {
    let (name, overrides) = theme_spec()?;
    let theme = load_scheme_theme(editor, &name, &overrides).ok()?;
    (true_color || theme.is_16_color()).then_some(theme)
}

/// Re-apply the zwire scheme to `editor` when `sync-zwire-theme` is on and
/// `global.toml` now names a different theme or carries an edited palette. Runs
/// on the main thread (via the watcher's [`crate::job::dispatch_blocking`]) so it
/// may touch the editor directly.
pub fn apply(editor: &mut Editor) {
    if !editor.config().sync_zwire_theme {
        return;
    }
    let Some((name, overrides)) = theme_spec() else {
        return;
    };
    // No live palette + already the active theme -> nothing to do. With a palette
    // we re-apply on a name match too, since the colours may have been edited.
    if overrides.is_empty() && editor.theme.name() == name {
        return;
    }
    let true_color = editor.config().true_color || crate::true_color();
    match load_scheme_theme(editor, &name, &overrides) {
        Ok(theme) if true_color || theme.is_16_color() => {
            let _ = editor.set_theme(theme);
        }
        // Scheme names no shipped `zgui-*` theme (and no palette to graft onto a
        // base), or true color is unavailable: keep the current theme.
        Ok(_) => {}
        Err(e) => log::debug!(
            "zwire theme `{}` not loadable, keeping current: {}",
            name,
            e
        ),
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
        matches!(res, Ok(event) if event.paths.contains(&global_toml))
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
        assert_eq!(
            theme_name_from_toml(toml).as_deref(),
            Some("zgui-midnight-light")
        );
    }

    #[test]
    fn dark_ui_selects_base_variant() {
        let toml = "[theme]\nscheme = \"cyberpunk\"\n\n[theme.ui]\nlight = false\n";
        assert_eq!(
            theme_name_from_toml(toml).as_deref(),
            Some("zgui-cyberpunk")
        );
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

    use super::palette_overrides_from_toml;

    #[test]
    fn palette_overrides_map_vars_to_theme_keys() {
        let toml = "[theme]\nscheme = \"midnight\"\n\n[theme.palette]\n\"--accent\" = \"#7c3aed\"\n\"--bg-primary\" = \"#050510\"\n\"--border-glow\" = \"#2e1e5a\"\n";
        let o = palette_overrides_from_toml(toml);
        assert_eq!(o.get("accent").map(String::as_str), Some("#7c3aed"));
        assert_eq!(o.get("bg").map(String::as_str), Some("#050510")); // --bg-primary -> bg
        assert_eq!(o.get("border_glow").map(String::as_str), Some("#2e1e5a"));
        assert_eq!(o.len(), 3);
    }

    #[test]
    fn palette_overrides_skip_non_hex_and_absent() {
        // A non-`#` value (e.g. a stray rgba) must not reach the palette.
        let toml = "[theme]\nscheme = \"midnight\"\n\n[theme.palette]\n\"--accent\" = \"rgba(124, 58, 237, 0.4)\"\n";
        assert!(palette_overrides_from_toml(toml).is_empty());
        // No [theme.palette] at all -> empty overrides (older global.toml).
        assert!(palette_overrides_from_toml("[theme]\nscheme = \"midnight\"\n").is_empty());
    }

    use super::scheme_from_theme;

    #[test]
    fn reverse_map_dark_and_light() {
        assert_eq!(
            scheme_from_theme("zgui-midnight"),
            Some(("midnight".into(), false))
        );
        assert_eq!(
            scheme_from_theme("zgui-matrix-light"),
            Some(("matrix".into(), true)),
        );
    }

    #[test]
    fn reverse_map_ignores_non_app_shell_themes() {
        // No `zgui-` prefix -> not an app-shell scheme -> never pushed to zwire.
        assert_eq!(scheme_from_theme("dracula_at_night"), None);
        assert_eq!(scheme_from_theme("nord"), None);
        assert_eq!(scheme_from_theme("ataraxia"), None);
    }

    #[test]
    fn map_and_reverse_map_round_trip() {
        // theme_name_from_toml -> scheme_from_theme recovers (scheme, light).
        for (scheme, light) in [("cyberpunk", false), ("vapor", true)] {
            let toml = format!("[theme]\nscheme = \"{scheme}\"\n\n[theme.ui]\nlight = {light}\n");
            let name = theme_name_from_toml(&toml).unwrap();
            assert_eq!(scheme_from_theme(&name), Some((scheme.to_string(), light)));
        }
    }

    use super::write_back_to;

    fn tmp_global(body: &str) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{body}").unwrap();
        f.flush().unwrap();
        f
    }

    const FULL: &str = "[theme]\nscheme = \"midnight\"\n\n[theme.ui]\nanim = true\nglow = true\nlight = false\nscanlines = true\nvignette = true\n";

    #[test]
    fn write_back_updates_scheme_and_light_preserving_other_keys() {
        let f = tmp_global(FULL);
        assert!(write_back_to(f.path(), "zgui-matrix-light"));
        let out = std::fs::read_to_string(f.path()).unwrap();
        // scheme + light rewritten...
        assert!(
            out.contains("scheme = \"matrix\""),
            "scheme not updated: {out}"
        );
        assert!(out.contains("light = true"), "light not updated: {out}");
        // ...and zwire's other keys survive untouched.
        for k in [
            "anim = true",
            "glow = true",
            "scanlines = true",
            "vignette = true",
        ] {
            assert!(out.contains(k), "clobbered `{k}`: {out}");
        }
    }

    #[test]
    fn write_back_ignores_non_app_shell_theme() {
        let f = tmp_global(FULL);
        assert!(!write_back_to(f.path(), "dracula_at_night"));
        // File is left byte-for-byte unchanged.
        assert_eq!(std::fs::read_to_string(f.path()).unwrap(), FULL);
    }

    #[test]
    fn write_back_is_idempotent_when_already_current() {
        // midnight + light=false is already what FULL holds -> no write.
        let f = tmp_global(FULL);
        assert!(!write_back_to(f.path(), "zgui-midnight"));
        assert_eq!(std::fs::read_to_string(f.path()).unwrap(), FULL);
    }
}
