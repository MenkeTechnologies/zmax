//! Named run configurations (JetBrains-style "Run/Debug Configurations").
//!
//! Each config is a name + shell command + working directory (+ optional env).
//! The list and the active selection persist to `<workspace>/.zmax/run-configs.toml`
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

/// URL-safe base64 (no padding), dependency-free — for encoding a workspace path
/// into a single filesystem-safe directory component.
fn b64_url(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b1 = chunk[0] as u32;
        let b2 = *chunk.get(1).unwrap_or(&0) as u32;
        let b3 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b1 << 16) | (b2 << 8) | b3;
        out.push(A[((n >> 18) & 63) as usize] as char);
        out.push(A[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(A[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(A[(n & 63) as usize] as char);
        }
    }
    out
}

/// Decode a [`b64_url`] string back to bytes. `None` on a character outside the
/// alphabet or a truncated group — a stray directory under `projects/` should be
/// skipped, not guessed at.
fn b64_url_decode(input: &str) -> Option<Vec<u8>> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bits = 0u32;
    let mut nbits = 0u32;
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for c in input.bytes() {
        let v = A.iter().position(|&a| a == c)? as u32;
        bits = (bits << 6) | v;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
        }
    }
    Some(out)
}

/// The projects zmax has state for, newest first: the roots encoded in the
/// `projects/<name>-<b64(path)>` directory names, ordered by the state
/// directory's mtime. A root that no longer exists on disk is dropped — the
/// state directory outlives a deleted checkout.
pub fn recent_projects() -> Vec<PathBuf> {
    let mut rows: Vec<(std::time::SystemTime, PathBuf)> =
        std::fs::read_dir(zmax_loader::config_dir().join("projects"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let (_, key) = name.rsplit_once('-')?;
                let path = PathBuf::from(String::from_utf8(b64_url_decode(key)?).ok()?);
                if !path.is_dir() {
                    return None;
                }
                let when = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                Some((when, path))
            })
            .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    rows.into_iter().map(|(_, path)| path).collect()
}

/// The per-project state directory under the global config dir — where all
/// project-specific files live so the project tree isn't polluted with a
/// `.zmax/`. Named `<project>-<base64(full path)>` so it's readable AND unique
/// (same-named projects never collide): `~/.zmax/projects/<project>-<b64>/`.
pub fn project_dir() -> PathBuf {
    let root = zmax_loader::find_workspace().0;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "global".to_string());
    let key = b64_url(root.to_string_lossy().as_bytes());
    zmax_loader::config_dir()
        .join("projects")
        .join(format!("{name}-{key}"))
}

fn store_path() -> PathBuf {
    project_dir().join("run-configs.toml")
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
    let root = zmax_loader::find_workspace().0;
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

#[cfg(test)]
mod tests {
    use super::{b64_url, b64_url_decode};

    #[test]
    fn the_project_key_round_trips() {
        for path in ["/Users/someone/code/zmax", "/tmp/a", "/x/caf\u{e9}", "/"] {
            let key = b64_url(path.as_bytes());
            assert_eq!(
                b64_url_decode(&key).and_then(|b| String::from_utf8(b).ok()),
                Some(path.to_string()),
                "key {key} did not decode back to {path}"
            );
        }
    }

    #[test]
    fn a_key_with_a_foreign_character_decodes_to_nothing() {
        assert!(b64_url_decode("abc*def").is_none());
    }
}
