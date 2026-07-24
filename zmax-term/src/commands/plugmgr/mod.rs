//! zmax native-plugin package manager — a GLOBAL-only package manager for the
//! editor's native (Rust `cdylib`) plugins.
//!
//! Ported from zshrs's `znative` package manager (`src/extensions/pkg/`),
//! retargeted from zsh plugins to zmax plugins and reduced to a single kind:
//! **native**. A zmax plugin is a compiled `cdylib` built against the
//! [`zmax_plugin`] SDK; there is no interpreted-script plugin kind (an editor has
//! no shell to `source` into), so the zshrs "script" path is dropped here.
//!
//! World's first for an editor of this lineage: Helix — which zmax forks — ships
//! **no plugin system at all**. zmax adds a package manager that installs a
//! third-party compiled plugin from `owner/repo` into a content-addressed global
//! store and loads it at runtime by **mmap** (`dlopen`), with SHA-256 integrity
//! pinning and zero editor recompile.
//!
//! Surface:
//! - [`manifest`] — a plugin's optional `zmax-plugin.toml` (`[plugin]`/
//!   `[native]`); auto-detected from the tree when absent.
//! - [`store`]    — `~/.zmax/pkg/{store,cache,git,bin}/` layout + the
//!   `installed.toml` global index (source of truth).
//! - [`resolver`] — turn a source spec (`owner/repo`, `git+URL`, `path:DIR`)
//!   into a staged directory ready to install.
//! - [`commands`] — `add/remove/list/info/load/update/gc/clean`
//!   implementations. Each **returns** a status string (never prints) so the
//!   `:plugin` dispatcher can surface it in the editor status line — writing to
//!   stdout would corrupt the TUI.

pub mod commands;
pub mod manifest;
pub mod resolver;
pub mod store;

/// Result alias used throughout the package manager. Errors are stringly-typed
/// (one user-facing diagnostic per failure path), surfaced to the editor as
/// `plugin: <reason>` — matching zmax's terse command-error style.
pub type PkgResult<T> = Result<T, PkgError>;

/// Errors emitted by the package manager. `Display` produces the one-line
/// reason (no prefix — the caller adds `plugin:`).
#[derive(Debug)]
pub enum PkgError {
    /// File I/O — read/write/create/copy.
    Io(String),
    /// Manifest parse error (bad TOML in a plugin's `zmax-plugin.toml`).
    Manifest(String),
    /// Resolver error — unknown source form, clone/build/download failure.
    Resolve(String),
    /// The plugin could not be recognized as a native cdylib plugin (no
    /// `zmax-plugin.toml`, no prebuilt `lib*.{dylib,so}`, no cdylib `Cargo.toml`).
    Unknown(String),
    /// Generic runtime error.
    Other(String),
}

impl std::fmt::Display for PkgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PkgError::Io(s)
            | PkgError::Manifest(s)
            | PkgError::Resolve(s)
            | PkgError::Unknown(s)
            | PkgError::Other(s) => write!(f, "{}", s),
        }
    }
}

impl From<std::io::Error> for PkgError {
    fn from(e: std::io::Error) -> Self {
        PkgError::Io(e.to_string())
    }
}

/// Deterministic SHA-256 of a directory tree, `sha256-<hex>`. Ported from zshrs
/// `pkg/mod.rs::store_integrity`. Files are walked in sorted order so the hash
/// is stable regardless of filesystem iteration; each file contributes
/// `<relpath>\0F\0<len>\n<bytes>\n`, symlinks their target. Recorded in the
/// install index for change detection / audit.
pub fn store_integrity(root: &std::path::Path) -> PkgResult<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut entries: Vec<std::path::PathBuf> = Vec::new();
    fn walk(
        root: &std::path::Path,
        cur: &std::path::Path,
        out: &mut Vec<std::path::PathBuf>,
    ) -> PkgResult<()> {
        for entry in std::fs::read_dir(cur)? {
            let entry = entry?;
            let path = entry.path();
            let meta = entry.metadata()?;
            if meta.is_dir() && !meta.file_type().is_symlink() {
                walk(root, &path, out)?;
            } else {
                out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
            }
        }
        Ok(())
    }
    walk(root, root, &mut entries)?;
    entries.sort();
    for rel in &entries {
        let abs = root.join(rel);
        let meta = std::fs::symlink_metadata(&abs)?;
        let rel_s = rel.to_string_lossy();
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs)?;
            hasher.update(rel_s.as_bytes());
            hasher.update(b"\0L\0");
            hasher.update(target.to_string_lossy().as_bytes());
            hasher.update(b"\n");
        } else if meta.is_file() {
            let bytes = std::fs::read(&abs)?;
            hasher.update(rel_s.as_bytes());
            hasher.update(b"\0F\0");
            hasher.update(bytes.len().to_string().as_bytes());
            hasher.update(b"\n");
            hasher.update(&bytes);
            hasher.update(b"\n");
        }
    }
    Ok(format!("sha256-{:x}", hasher.finalize()))
}
