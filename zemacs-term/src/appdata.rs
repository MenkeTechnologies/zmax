//! Session persistence: drawer layout, open tabs, focused file + cursor, and theme,
//! stored at `<config-dir>/appdata.toml`. Saved on exit, restored on a no-file launch.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IdeLayout {
    pub open: bool,
    pub left_width: u16,
    pub left_collapsed: bool,
    pub fold_project: bool,
    pub fold_structure: bool,
    pub fold_problems: bool,
    /// Whether the right-hand minimap stripe is collapsed to a thin handle.
    pub fold_minimap: bool,
    /// Height of the bottom tool drawer, in rows (0 = use the default).
    pub bottom_height: u16,
    /// Bottom drawer maximized to full height.
    pub bottom_zoom: bool,
    /// The two bottom-drawer column divider positions, as % of drawer width.
    pub bottom_splits: [u16; 2],
    /// Bottom drawer middle column collapsed (two-column layout).
    pub bottom_mid_folded: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppData {
    pub theme: Option<String>,
    /// Absolute paths of the buffers that were open, in tab order.
    pub open_files: Vec<String>,
    /// The buffer that had focus, and its primary cursor (char offset).
    pub focused_file: Option<String>,
    pub cursor: Option<usize>,
    pub ide: IdeLayout,
}

fn store_path() -> PathBuf {
    zemacs_loader::config_dir().join("appdata.toml")
}

pub fn load() -> Option<AppData> {
    let contents = std::fs::read_to_string(store_path()).ok()?;
    toml::from_str(&contents).ok()
}

pub fn save(data: &AppData) {
    let Ok(contents) = toml::to_string_pretty(data) else {
        return;
    };
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, contents);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ide_layout_survives_toml_round_trip() {
        let data = AppData {
            theme: Some("nord".into()),
            ide: IdeLayout {
                open: true,
                left_width: 40,
                left_collapsed: true,
                fold_project: false,
                fold_structure: true,
                fold_problems: false,
                fold_minimap: true,
                bottom_height: 12,
                bottom_zoom: true,
                bottom_splits: [20, 60],
                bottom_mid_folded: true,
            },
            ..Default::default()
        };
        let serialized = toml::to_string_pretty(&data).unwrap();
        let parsed: AppData = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.theme.as_deref(), Some("nord"));
        assert_eq!(parsed.ide.left_width, 40);
        assert!(parsed.ide.left_collapsed);
        assert!(parsed.ide.fold_minimap);
        assert_eq!(parsed.ide.bottom_height, 12);
        assert!(parsed.ide.bottom_zoom);
        assert_eq!(parsed.ide.bottom_splits, [20, 60]);
        assert!(parsed.ide.bottom_mid_folded);
    }

    #[test]
    fn old_appdata_without_new_ide_fields_still_loads() {
        // A pre-existing appdata.toml that predates the bottom-* drawer fields must
        // still parse (serde defaults fill them in), so an upgrade never wipes a
        // user's session.
        let parsed: AppData =
            toml::from_str("theme = \"base16\"\n[ide]\nopen = true\nleft_width = 30\n").unwrap();
        assert!(parsed.ide.open);
        assert_eq!(parsed.ide.left_width, 30);
        assert_eq!(parsed.ide.bottom_height, 0);
        assert_eq!(parsed.ide.bottom_splits, [0, 0]);
        assert!(!parsed.ide.bottom_mid_folded);
    }
}
