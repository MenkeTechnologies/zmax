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
    /// Bottom drawer left column collapsed.
    pub bottom_left_folded: bool,
    /// Bottom drawer right column collapsed.
    pub bottom_right_folded: bool,
    /// "Always select opened file" (auto-reveal the current buffer in the tree).
    pub auto_reveal: bool,
    /// The active tool-window tab in each of the three bottom columns, by name
    /// (e.g. `["problems", "run", "git"]`). Empty = use the defaults.
    pub bottom_tabs: Vec<String>,
    /// Which bottom column had keyboard focus (0..3).
    pub bottom_focus_col: usize,
    /// Absolute paths of the expanded folders in the PROJECT tree.
    pub expanded_dirs: Vec<String>,
}

/// A persisted debugger breakpoint (the user-set fields; runtime state like the
/// DAP id and `verified` flag is re-established when a debug session attaches).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BreakpointData {
    pub line: usize,
    pub column: Option<usize>,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
}

/// All breakpoints set in one file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileBreakpoints {
    pub path: String,
    pub breakpoints: Vec<BreakpointData>,
}

/// One node of the persisted window split tree. A leaf holds a file (+ its
/// cursor); a split (`kind` = "h"/"v") holds weighted children. Mirrors
/// [`zmax_view::tree::TreeShape`] with paths instead of live `DocumentId`s.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SplitNode {
    /// "leaf", "h" (horizontal split), or "v" (vertical split).
    pub kind: String,
    /// Size weight within the parent split.
    pub weight: f32,
    // leaf fields:
    pub path: Option<String>,
    pub focused: bool,
    pub cursor: Option<usize>,
    // split fields:
    pub children: Vec<SplitNode>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppData {
    pub theme: Option<String>,
    /// Absolute paths of the buffers that were open, in tab order. Kept as a
    /// fallback for when there is no `splits` tree (older sessions).
    pub open_files: Vec<String>,
    /// The buffer that had focus, and its primary cursor (char offset).
    pub focused_file: Option<String>,
    pub cursor: Option<usize>,
    /// The full window split layout (files + horizontal/vertical arrangement +
    /// per-view cursor). When present it supersedes `open_files`/`focused_file`.
    pub splits: Option<SplitNode>,
    /// Debugger breakpoints, keyed by file.
    pub breakpoints: Vec<FileBreakpoints>,
    pub ide: IdeLayout,
}

fn store_path() -> PathBuf {
    zmax_loader::config_dir().join("appdata.toml")
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
                ..Default::default()
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
    fn splits_breakpoints_and_ide_settings_round_trip() {
        let data = AppData {
            breakpoints: vec![FileBreakpoints {
                path: "/p/src/main.rs".into(),
                breakpoints: vec![
                    BreakpointData {
                        line: 12,
                        ..Default::default()
                    },
                    BreakpointData {
                        line: 40,
                        condition: Some("x > 3".into()),
                        ..Default::default()
                    },
                ],
            }],
            splits: Some(SplitNode {
                kind: "h".into(),
                weight: 1.0,
                children: vec![
                    SplitNode {
                        kind: "leaf".into(),
                        weight: 1.0,
                        path: Some("/p/a.rs".into()),
                        focused: true,
                        cursor: Some(10),
                        ..Default::default()
                    },
                    SplitNode {
                        kind: "v".into(),
                        weight: 2.0,
                        children: vec![SplitNode {
                            kind: "leaf".into(),
                            weight: 1.0,
                            path: Some("/p/b.rs".into()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ide: IdeLayout {
                auto_reveal: true,
                bottom_tabs: vec!["run".into(), "git".into(), "debug".into()],
                bottom_focus_col: 2,
                expanded_dirs: vec!["/p/src".into(), "/p/tests".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let s = toml::to_string_pretty(&data).unwrap();
        let p: AppData = toml::from_str(&s).unwrap();
        assert_eq!(p.breakpoints.len(), 1);
        assert_eq!(
            p.breakpoints[0].breakpoints[1].condition.as_deref(),
            Some("x > 3")
        );
        let splits = p.splits.unwrap();
        assert_eq!(splits.kind, "h");
        assert_eq!(splits.children.len(), 2);
        assert_eq!(splits.children[0].path.as_deref(), Some("/p/a.rs"));
        assert!(splits.children[0].focused);
        assert_eq!(splits.children[1].kind, "v");
        assert_eq!(
            splits.children[1].children[0].path.as_deref(),
            Some("/p/b.rs")
        );
        assert!(p.ide.auto_reveal);
        assert_eq!(p.ide.bottom_tabs, vec!["run", "git", "debug"]);
        assert_eq!(p.ide.bottom_focus_col, 2);
        assert_eq!(p.ide.expanded_dirs, vec!["/p/src", "/p/tests"]);
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
