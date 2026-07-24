//! A plugin's optional `zmax-native.toml`, declaring how the plugin is built.
//!
//! Ported/adapted from zshrs's `pkg/manifest.rs`, trimmed to zmax's single
//! plugin kind — **native** (a Rust `cdylib` built against the `zmax-native`
//! SDK). When a plugin repo ships no `zmax-native.toml`, [`detect_native`]
//! infers the build recipe from the tree, so an ordinary cdylib crate installs
//! with no metadata.
//!
//! Schema:
//! ```toml
//! [plugin]
//! name = "hello"
//! version = "0.1.0"
//! description = "example zmax plugin"
//!
//! # Native (Rust cdylib) plugin — dlopened (mmap'd) via `:plugin load`:
//! [native]
//! lib = "hello"        # produces lib<lib>.{dylib,so}
//! # build = true       # run `cargo build`; defaults to true when a Cargo.toml
//!                      # is present and no prebuilt cdylib sits at the root
//! ```

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::{PkgError, PkgResult};

/// Manifest filename, at the root of a plugin's tree.
pub const MANIFEST_FILE: &str = "zmax-native.toml";

/// Parsed `zmax-native.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    /// `[plugin]` metadata.
    #[serde(default)]
    pub plugin: PluginMeta,
    /// `[native]` — the cdylib build spec (optional; inferred when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeSpec>,
}

/// `[plugin]` table.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginMeta {
    /// `name` — defaults to the source basename when absent.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// `version` — defaults to `"0.0.0"` when absent.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// One-line description (shown by `:plugin list`/`info`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// `[native]` — a Rust cdylib plugin using the `zmax-native` SDK.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeSpec {
    /// Library file stem — produces `lib<lib>.{dylib,so}`. When empty the
    /// installer infers it from the built artifact or the package name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lib: String,
    /// When true, run `cargo build` in the staged tree before looking for the
    /// cdylib. Defaults to true when a `Cargo.toml` exists and no prebuilt
    /// `lib*.{dylib,so}` already sits at the tree root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<bool>,
}

impl PluginManifest {
    /// Parse a `zmax-native.toml` string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> PkgResult<PluginManifest> {
        toml::from_str::<PluginManifest>(s)
            .map_err(|e| PkgError::Manifest(format!("zmax-native.toml: {}", e.message())))
    }

    /// Load a plugin's `zmax-native.toml` if present at `dir/zmax-native.toml`.
    pub fn load(dir: &Path) -> PkgResult<Option<PluginManifest>> {
        let path = dir.join(MANIFEST_FILE);
        if !path.is_file() {
            return Ok(None);
        }
        let s = std::fs::read_to_string(&path)
            .map_err(|e| PkgError::Io(format!("read {}: {}", path.display(), e)))?;
        Ok(Some(PluginManifest::from_str(&s)?))
    }
}

/// Determine the native build spec for a staged tree. Prefers an explicit
/// `[native]` table, then falls back to layout detection:
///
/// 1. an explicit `[native]` in `zmax-native.toml`, or
/// 2. a prebuilt `lib*.{dylib,so}` at the tree root, or
/// 3. a `Cargo.toml` whose `[lib] crate-type` mentions `cdylib`.
///
/// Returns [`PkgError::Unknown`] when none matches — the tree is not a zmax
/// native plugin.
pub fn detect_native(dir: &Path, manifest: Option<&PluginManifest>) -> PkgResult<NativeSpec> {
    if let Some(m) = manifest {
        if let Some(n) = &m.native {
            return Ok(n.clone());
        }
    }
    if has_cdylib(dir) || cargo_is_cdylib(dir) {
        return Ok(NativeSpec::default());
    }
    Err(PkgError::Unknown(
        "not a zmax native plugin: no zmax-native.toml [native], no prebuilt \
         lib*.{dylib,so}, and no Cargo.toml declaring crate-type = [\"cdylib\"]"
            .into(),
    ))
}

/// True if a `lib*.{dylib,so}` exists at the tree root (a prebuilt cdylib).
fn has_cdylib(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in rd.flatten() {
        let n = entry.file_name();
        let n = n.to_string_lossy();
        if n.starts_with("lib") && (n.ends_with(".dylib") || n.ends_with(".so")) {
            return true;
        }
    }
    false
}

/// True if `Cargo.toml` declares a `cdylib` crate-type (so `cargo build`
/// produces a dlopen-able library).
fn cargo_is_cdylib(dir: &Path) -> bool {
    let cargo = dir.join("Cargo.toml");
    let Ok(s) = std::fs::read_to_string(&cargo) else {
        return false;
    };
    s.contains("cdylib")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_native_manifest() {
        let m = PluginManifest::from_str(
            "[plugin]\nname='x'\nversion='0.1.0'\n[native]\nlib='foo'\n",
        )
        .unwrap();
        assert_eq!(m.plugin.name, "x");
        assert_eq!(m.native.unwrap().lib, "foo");
    }

    #[test]
    fn detect_prefers_explicit_native() {
        let m = PluginManifest::from_str("[plugin]\nname='x'\n[native]\nlib='bar'\n").unwrap();
        let spec = detect_native(std::path::Path::new("/nonexistent"), Some(&m)).unwrap();
        assert_eq!(spec.lib, "bar");
    }

    #[test]
    fn detect_rejects_non_plugin() {
        let dir = std::env::temp_dir().join(format!("zmaxpkg-detect-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("README.md"), b"hi").unwrap();
        assert!(detect_native(&dir, None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
