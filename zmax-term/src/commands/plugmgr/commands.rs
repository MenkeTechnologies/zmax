//! `:plugin` package-manager subcommand implementations (global model). Ported
//! from zshrs's `pkg/commands.rs`, reduced to native plugins and made
//! **TUI-safe**: every command RETURNS a status string instead of writing to
//! stdout (a `println!` here would corrupt the editor's terminal display). The
//! `:plugin` dispatcher surfaces the returned string via `Editor::set_status`.
//!
//! Native plugins load through zmax's mmap-dlopen host
//! ([`crate::commands::plugin::load`]): `dlopen` maps the `cdylib` into the
//! process address space by mmap — the store copy is never read into a buffer,
//! the loader pages it in.

use std::path::Path;

use super::manifest::{self, NativeSpec, PluginManifest};
use super::store::{InstalledIndex, InstalledPlugin, Store};
use super::{resolver, PkgError, PkgResult};

/// `:plugin add <SOURCE>` — resolve, build if needed, install into the store,
/// record in the index, and load. Returns a one-line summary.
pub fn add(spec: &str) -> PkgResult<String> {
    let store = Store::user_default()?;
    store.ensure_layout()?;

    let staged = resolver::resolve(spec, &store)?;
    let manifest = PluginManifest::load(&staged.dir)?;
    let name = manifest
        .as_ref()
        .map(|m| m.plugin.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| staged.name.clone());
    let version = manifest
        .as_ref()
        .map(|m| m.plugin.version.clone())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "0.0.0".into());
    let description = manifest
        .as_ref()
        .map(|m| m.plugin.description.clone())
        .unwrap_or_default();
    let native = manifest::detect_native(&staged.dir, manifest.as_ref())?;

    // A plugin shipped as source needs a build before the cdylib exists at the
    // tree root (where the store copy will find it).
    prepare_native(&staged.dir, &native, &name)?;

    // Copy the loadable subset into the content-addressed store.
    let store_path = store.install_dir(&name, &version, &staged.dir)?;
    let integrity = super::store_integrity(&store_path)?;

    let entry = InstalledPlugin {
        name: name.clone(),
        version: version.clone(),
        source: staged.source.clone(),
        kind: "native".into(),
        integrity,
        lib: find_cdylib(&store_path)
            .ok_or_else(|| PkgError::Resolve(format!("{}: no cdylib after build", name)))?,
    };

    let mut index = InstalledIndex::load_from(&store)?;
    index.upsert(entry.clone());
    index.save_to(&store)?;

    // Clean the git clone scratch — the store copy is authoritative.
    if staged.source.starts_with("github:") || staged.source.starts_with("git+") {
        let _ = std::fs::remove_dir_all(&staged.dir);
    }

    load_entry(&store, &entry)?;
    let desc = if description.is_empty() {
        String::new()
    } else {
        format!(" — {}", description)
    };
    Ok(format!("added {}@{} (native){}", name, version, desc))
}

/// `:plugin remove <NAME>` — unload (best-effort), drop the store copy + index row.
pub fn remove(name: &str) -> PkgResult<String> {
    let store = Store::user_default()?;
    let mut index = InstalledIndex::load_from(&store)?;
    let Some(entry) = index.remove(name) else {
        return Err(PkgError::Other(format!("{} is not installed", name)));
    };
    let _ = crate::commands::plugin::unload(name);
    let dir = store.package_dir(&entry.name, &entry.version);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| PkgError::Io(format!("remove {}: {}", dir.display(), e)))?;
    }
    index.save_to(&store)?;
    Ok(format!("removed {}", name))
}

/// `:plugin registry` — one line per installed plugin (name version source).
/// Distinct from `:plugin list`, which shows the currently *loaded* plugins.
pub fn registry() -> PkgResult<String> {
    let store = Store::user_default()?;
    let index = InstalledIndex::load_from(&store)?;
    if index.packages.is_empty() {
        return Ok("no plugins installed".into());
    }
    let lines: Vec<String> = index
        .packages
        .iter()
        .map(|p| format!("{} {} {}", p.name, p.version, p.source))
        .collect();
    Ok(lines.join("  |  "))
}

/// Recursive byte size of a directory tree (0 if unreadable). Ported from
/// zshrs's `pkg/commands.rs::dir_size`.
fn dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    total
}

/// `:plugin gc [--dry-run]` — remove every `store/<name>@<version>/` directory
/// not pinned by `installed.toml` (orphans left by old versions or upgrades),
/// plus the `git/` clone scratch. Ported from zshrs's `gc`.
pub fn gc(dry_run: bool) -> PkgResult<String> {
    let store = Store::user_default()?;
    let index = InstalledIndex::load_from(&store)?;
    let pinned: std::collections::HashSet<String> = index
        .packages
        .iter()
        .map(|p| format!("{}@{}", p.name, p.version))
        .collect();

    let mut freed: u64 = 0;
    let mut count: usize = 0;

    // 1. Orphan store/<name>@<version> directories.
    if let Ok(entries) = std::fs::read_dir(store.store_dir()) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() && !pinned.contains(&name) {
                let bytes = dir_size(&entry.path());
                if !dry_run {
                    std::fs::remove_dir_all(entry.path())
                        .map_err(|e| PkgError::Io(format!("remove {}: {}", name, e)))?;
                }
                freed += bytes;
                count += 1;
            }
        }
    }

    // 2. git/ clone scratch — the store holds the copied working tree, so the
    //    clone under git/ is dead weight after install.
    let git = store.git_dir();
    let git_bytes = dir_size(&git);
    if git_bytes > 0 {
        if !dry_run {
            let _ = std::fs::remove_dir_all(&git);
        }
        freed += git_bytes;
        count += 1;
    }

    if count == 0 {
        return Ok("gc: nothing to collect".into());
    }
    let verb = if dry_run { "would free" } else { "freed" };
    Ok(format!(
        "gc: {} {} item(s), {} KB",
        verb,
        count,
        (freed + 512) / 1024
    ))
}

/// `:plugin clean` — clear the scratch directories (`git/`, `cache/`, `bin/`).
/// The store and index are left intact. Ported from zshrs's `clean`.
pub fn clean() -> PkgResult<String> {
    let store = Store::user_default()?;
    let mut freed: u64 = 0;
    for d in [store.git_dir(), store.cache_dir(), store.bin_dir()] {
        if d.exists() {
            freed += dir_size(&d);
            std::fs::remove_dir_all(&d)
                .map_err(|e| PkgError::Io(format!("remove {}: {}", d.display(), e)))?;
        }
    }
    Ok(format!("clean: cleared {} KB of scratch", (freed + 512) / 1024))
}

/// `:plugin info <NAME>` — full record for one plugin, as a one-line summary.
pub fn info(name: &str) -> PkgResult<String> {
    let store = Store::user_default()?;
    let index = InstalledIndex::load_from(&store)?;
    let Some(p) = index.find(name) else {
        return Err(PkgError::Other(format!("{} is not installed", name)));
    };
    let mut parts = vec![
        format!("name={}", p.name),
        format!("version={}", p.version),
        format!("source={}", p.source),
        format!("store={}", store.package_dir(&p.name, &p.version).display()),
    ];
    if !p.lib.is_empty() {
        parts.push(format!("lib={}", p.lib));
    }
    if !p.integrity.is_empty() {
        parts.push(format!("integrity={}", p.integrity));
    }
    Ok(parts.join("  "))
}

/// `:plugin load [NAME_or_SOURCE]` package-manager form — load one installed
/// plugin from the store, or every installed plugin when no name is given. A
/// SOURCE not yet in the store is installed first, then loaded (self-installing
/// startup line). Zero network once stored. Returns a summary.
pub fn load(name: Option<&str>) -> PkgResult<String> {
    let store = Store::user_default()?;
    let index = InstalledIndex::load_from(&store)?;
    match name {
        Some(n) => {
            // 1. Already installed under this name → load from the store
            //    (native = dlopen/mmap the cdylib; no reinstall).
            if let Some(entry) = index.find(n) {
                load_entry(&store, entry)?;
                return Ok(format!("loaded {}", entry.name));
            }
            // 2. `n` is a SOURCE spec — is a plugin from that source already
            //    installed? The index keys on the source label, since a repo
            //    basename usually differs from the plugin's manifest name.
            if let Some(label) = resolver::source_label(n) {
                if let Some(entry) = index.packages.iter().find(|p| p.source == label) {
                    load_entry(&store, entry)?;
                    return Ok(format!("loaded {}", entry.name));
                }
                // 3. Not in the store yet → install-on-first-use, then load.
                return add(n);
            }
            Err(PkgError::Other(format!("{} is not installed", n)))
        }
        None => {
            let mut loaded = Vec::new();
            let mut errs = Vec::new();
            for p in &index.packages {
                match load_entry(&store, p) {
                    Ok(()) => loaded.push(p.name.clone()),
                    Err(e) => errs.push(format!("{}: {}", p.name, e)),
                }
            }
            if !errs.is_empty() {
                return Err(PkgError::Other(errs.join("; ")));
            }
            if loaded.is_empty() {
                Ok("no plugins installed".into())
            } else {
                Ok(format!("loaded {} plugin(s): {}", loaded.len(), loaded.join(", ")))
            }
        }
    }
}

/// `:plugin update [NAME]` — re-resolve + reinstall from the recorded source.
pub fn update(name: Option<&str>) -> PkgResult<String> {
    let store = Store::user_default()?;
    let index = InstalledIndex::load_from(&store)?;
    let targets: Vec<String> = match name {
        Some(n) => vec![n.to_string()],
        None => index.packages.iter().map(|p| p.name.clone()).collect(),
    };
    if targets.is_empty() {
        return Ok("no plugins installed".into());
    }
    let mut done = Vec::new();
    for n in targets {
        let Some(p) = index.find(&n) else {
            return Err(PkgError::Other(format!("{} is not installed", n)));
        };
        let spec = source_to_spec(&p.source);
        // A reinstall of a loaded native plugin must unload the old cdylib
        // first — the dlopen'd library keeps live fn pointers into the store
        // copy we are about to overwrite.
        let _ = crate::commands::plugin::unload(&p.name);
        add(&spec)?;
        done.push(n);
    }
    Ok(format!("updated {} plugin(s): {}", done.len(), done.join(", ")))
}

/// Convert a recorded provenance label back to a `:plugin add` spec.
fn source_to_spec(source: &str) -> String {
    if let Some(rest) = source.strip_prefix("path+file://") {
        format!("path:{}", rest)
    } else {
        // `github:owner/repo` and `git+URL` are already valid `add` specs.
        source.to_string()
    }
}

/// Load one installed native plugin via the mmap-dlopen host.
fn load_entry(store: &Store, p: &InstalledPlugin) -> PkgResult<()> {
    let dir = store.package_dir(&p.name, &p.version);
    let lib = dir.join(&p.lib);
    crate::commands::plugin::load(&lib.to_string_lossy())
        .map(|_| ())
        .map_err(PkgError::Resolve)
}

/// Build a native plugin's cdylib into the tree root so the store copy carries
/// it (the store copy skips `target/`). If a `lib*.{dylib,so}` already sits at
/// the root, use it as-is. Runs `cargo build --release` when a `Cargo.toml`
/// exists and building isn't disabled. Ported from zshrs's `prepare_native`.
fn prepare_native(dir: &Path, spec: &NativeSpec, name: &str) -> PkgResult<()> {
    if find_cdylib(dir).is_some() {
        return Ok(()); // prebuilt cdylib already at the root.
    }
    let has_cargo = dir.join("Cargo.toml").is_file();
    let want_build = spec.build.unwrap_or(has_cargo);
    if !want_build {
        return Err(PkgError::Resolve(format!(
            "{}: native plugin has no prebuilt cdylib and build is disabled",
            name
        )));
    }
    if !has_cargo {
        return Err(PkgError::Resolve(format!(
            "{}: native plugin has neither a cdylib nor a Cargo.toml to build",
            name
        )));
    }
    let out = std::process::Command::new("cargo")
        .current_dir(dir)
        .arg("build")
        .arg("--release")
        .output()
        .map_err(|e| PkgError::Resolve(format!("cargo build: {} (is cargo installed?)", e)))?;
    if !out.status.success() {
        return Err(PkgError::Resolve(format!(
            "{}: cargo build failed:\n{}",
            name,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    // Copy the built cdylib from target/release to the tree root.
    let rel = dir.join("target").join("release");
    let built = find_cdylib(&rel).ok_or_else(|| {
        PkgError::Resolve(format!(
            "{}: cargo build produced no cdylib in {} (need crate-type=[\"cdylib\"])",
            name,
            rel.display()
        ))
    })?;
    std::fs::copy(rel.join(&built), dir.join(&built))
        .map_err(|e| PkgError::Io(format!("stage cdylib: {}", e)))?;
    Ok(())
}

/// Find a `lib*.{dylib,so}` (or `*.dll`) filename directly inside `dir`.
fn find_cdylib(dir: &Path) -> Option<String> {
    let suffix = std::env::consts::DLL_SUFFIX; // .dylib / .so / .dll
    let prefix = std::env::consts::DLL_PREFIX; // lib / (empty on Windows)
    let rd = std::fs::read_dir(dir).ok()?;
    for entry in rd.flatten() {
        let n = entry.file_name().to_string_lossy().into_owned();
        if n.ends_with(suffix) && n.starts_with(prefix) {
            return Some(n);
        }
    }
    None
}

#[cfg(test)]
mod install_tests {
    use super::*;
    use super::super::store::{InstalledIndex, Store};
    use std::path::PathBuf;

    /// End-to-end install through the real package manager: resolve → `cargo
    /// build` → content-addressed store → index → mmap-`dlopen` load. Asserts the
    /// index record, the store cdylib on disk, and that the plugin's command
    /// registered in the live host.
    ///
    /// `#[ignore]` because it shells out to `cargo build` (slow). By default it
    /// generates a local cdylib plugin depending on the in-repo `zmax-native` SDK
    /// by path — **no network**, so CI can run it on demand with `--ignored`.
    /// Point it at a published repo to prove a real GitHub install:
    /// ```text
    /// ZMAX_TEST_PLUGIN_SRC=MenkeTechnologies/zmax-native-wc \
    ///   cargo test -p zmax-term --lib -- --ignored plugin_install_end_to_end
    /// ```
    #[test]
    #[ignore]
    fn plugin_install_end_to_end() {
        let tmp = std::env::temp_dir().join(format!("zmax-pkg-it-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("ZMAX_PKG_DIR", tmp.join("pkg"));

        // (add-spec, loaded plugin name from declare_plugin!, index/store name).
        let (spec, want_plugin, want_index_name) = match std::env::var("ZMAX_TEST_PLUGIN_SRC") {
            Ok(s) => {
                // `owner/zmax-native-wc` → loaded name "wc", index name "zmax-native-wc".
                let repo = s.rsplit('/').next().unwrap_or(&s).to_string();
                let plugin = repo
                    .strip_prefix("zmax-native-")
                    .unwrap_or(&repo)
                    .to_string();
                (s.clone(), plugin, repo)
            }
            Err(_) => {
                // Generate a minimal local cdylib plugin against the in-repo SDK.
                let sdk = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../zmax-native")
                    .canonicalize()
                    .expect("locate in-repo zmax-native SDK");
                let dir = tmp.join("localplug");
                std::fs::create_dir_all(dir.join("src")).unwrap();
                std::fs::write(
                    dir.join("Cargo.toml"),
                    format!(
                        "[package]\nname = \"localplug\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
                         [lib]\ncrate-type = [\"cdylib\"]\n[dependencies]\n\
                         zmax-native = {{ path = \"{}\" }}\n",
                        sdk.display()
                    ),
                )
                .unwrap();
                std::fs::write(
                    dir.join("src/lib.rs"),
                    "use std::os::raw::c_int;\n\
                     use zmax_native::{declare_plugin, Args, Host};\n\
                     fn hi(h: &Host, _: &Args) -> c_int { h.message(\"hi\"); 0 }\n\
                     declare_plugin! { name: \"localplug\", version: \"0.1.0\", \
                     commands: { \"localplug-hi\" => hi } }\n",
                )
                .unwrap();
                (
                    format!("path:{}", dir.display()),
                    "localplug".to_string(),
                    "localplug".to_string(),
                )
            }
        };

        // A prior run may have left the plugin registered in the process-global host.
        let _ = crate::commands::plugin::unload(&want_plugin);

        let msg = add(&spec).expect("add should succeed");
        assert!(msg.contains("added"), "unexpected add message: {msg}");

        // The index recorded a native package with a real cdylib + integrity.
        let store = Store::user_default().unwrap();
        let index = InstalledIndex::load_from(&store).unwrap();
        let entry = index.find(&want_index_name).unwrap_or_else(|| {
            panic!(
                "index missing {want_index_name}; have {:?}",
                index.packages.iter().map(|p| &p.name).collect::<Vec<_>>()
            )
        });
        assert_eq!(entry.kind, "native");
        assert!(
            entry.lib.starts_with("lib")
                && (entry.lib.ends_with(".dylib") || entry.lib.ends_with(".so")),
            "bad lib name: {}",
            entry.lib
        );
        assert!(entry.integrity.starts_with("sha256-"), "no integrity");

        // The cdylib is really on disk in the store.
        let libpath = store
            .package_dir(&entry.name, &entry.version)
            .join(&entry.lib);
        assert!(libpath.is_file(), "store cdylib missing: {}", libpath.display());

        // It loaded (dlopen/mmap) and registered its command in the host.
        let loaded = crate::commands::plugin::list();
        assert!(
            loaded.iter().any(|(n, _, _)| n == &want_plugin),
            "plugin {want_plugin} not loaded; have {:?}",
            loaded
        );

        // Cleanup.
        let _ = crate::commands::plugin::unload(&want_plugin);
        std::env::remove_var("ZMAX_PKG_DIR");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
