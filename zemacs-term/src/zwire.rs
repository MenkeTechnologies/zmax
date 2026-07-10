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
//! `:config-reload`, and live while idle. The scheme name is used verbatim (no
//! hardcoded scheme list), so a new zwire scheme with a matching `zgui-<name>`
//! theme works with no code change; an unknown scheme simply fails to load and
//! the caller keeps the current theme.

use serde::Deserialize;
use std::path::PathBuf;

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
