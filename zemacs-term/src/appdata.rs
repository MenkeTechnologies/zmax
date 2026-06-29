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
