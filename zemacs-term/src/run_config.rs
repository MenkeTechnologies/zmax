//! Named run configurations (JetBrains-style "Run/Debug Configurations").
//!
//! Each config is a name + shell command + working directory (+ optional env).
//! The list and the active selection persist to `<workspace>/.zemacs/run-configs.toml`
//! ("store as project file"). The Run toolbar / keybinding runs the active config;
//! the manager TUI (`ui::run_config::RunConfigPanel`) does CRUD over the list.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunConfig {
    pub name: String,
    /// Full shell command line, e.g. `cargo run --release` or `npm run dev`.
    pub command: String,
    /// Working directory. Empty = workspace root; relative is resolved against it.
    pub dir: String,
    /// Newline-separated `KEY=VALUE` environment overrides.
    pub env: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunConfigs {
    /// Index of the active config in `configs`.
    pub active: usize,
    #[serde(rename = "config", default)]
    pub configs: Vec<RunConfig>,
}

fn store_path() -> PathBuf {
    zemacs_loader::find_workspace()
        .0
        .join(".zemacs")
        .join("run-configs.toml")
}

pub fn load() -> RunConfigs {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|c| toml::from_str(&c).ok())
        .unwrap_or_default()
}

pub fn save(data: &RunConfigs) {
    let Ok(contents) = toml::to_string_pretty(data) else {
        return;
    };
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, contents);
}

/// The currently-selected config, if the list is non-empty.
pub fn active() -> Option<RunConfig> {
    let data = load();
    data.configs.get(data.active).cloned()
}

/// Create (or update) a run configuration by `name`, make it the active config,
/// persist, and return it. The JetBrains "right-click → Run" flow: running a
/// file materializes a reusable configuration rather than a one-shot command.
pub fn upsert_active(name: String, command: String, dir: String) -> RunConfig {
    let mut data = load();
    if let Some(i) = data.configs.iter().position(|c| c.name == name) {
        data.configs[i].command = command;
        data.configs[i].dir = dir;
        data.active = i;
    } else {
        data.configs.push(RunConfig {
            name,
            command,
            dir,
            env: String::new(),
        });
        data.active = data.configs.len() - 1;
    }
    let cfg = data.configs[data.active].clone();
    save(&data);
    cfg
}

/// Resolve a config's `dir` field to an absolute working directory.
pub fn resolve_dir(dir: &str) -> PathBuf {
    let root = zemacs_loader::find_workspace().0;
    if dir.trim().is_empty() {
        root
    } else {
        let p = PathBuf::from(dir);
        if p.is_absolute() {
            p
        } else {
            root.join(p)
        }
    }
}
