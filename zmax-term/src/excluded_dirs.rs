//! Excluded directories — JetBrains "Mark Directory as > Excluded"
//! (`MarkExcludeRoot`) and its undo, "Unmark as Excluded" (`UnmarkRoot`,
//! `MarkAsContentRoot`).
//!
//! The IDE keeps an excluded folder out of indexing, Go to File and Find in
//! Files while still showing it, greyed, in the Project view. zmax's file
//! picker, project search and file explorer already read the workspace's
//! `.zmax/ignore` (see `ui::workspace_walk`), so an excluded directory is one
//! anchored line there, `/path/from/root/`. The project tree reads its own
//! directory listings, so it keeps showing the folder and dims it.
//!
//! Only lines this module writes — a leading and a trailing `/` — count as
//! excluded directories; any other pattern a user put in the file is left
//! alone and not reported.

use std::io;
use std::path::{Component, Path, PathBuf};

/// The workspace ignore file the walkers read.
pub fn ignore_file(root: &Path) -> PathBuf {
    root.join(".zmax").join("ignore")
}

/// The `/rel/` line that excludes `dir`, or `None` when `dir` is not a
/// directory strictly below `root`.
fn line_for(root: &Path, dir: &Path) -> Option<String> {
    let rel = dir.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if parts.is_empty() {
        return None;
    }
    Some(format!("/{}/", parts.join("/")))
}

fn read_lines(root: &Path) -> io::Result<Vec<String>> {
    match std::fs::read_to_string(ignore_file(root)) {
        Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

fn write_lines(root: &Path, lines: &[String]) -> io::Result<()> {
    let path = ignore_file(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(path, text)
}

/// The directories excluded under `root`, in file order.
pub fn excluded(root: &Path) -> Vec<PathBuf> {
    read_lines(root)
        .unwrap_or_default()
        .iter()
        .filter_map(|line| {
            let inner = line.trim().strip_prefix('/')?.strip_suffix('/')?;
            (!inner.is_empty()).then(|| root.join(inner))
        })
        .collect()
}

/// Exclude `dir`. Returns false when it already was.
pub fn exclude(root: &Path, dir: &Path) -> io::Result<bool> {
    let line = line_for(root, dir).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a directory inside the workspace",
        )
    })?;
    let mut lines = read_lines(root)?;
    if lines.iter().any(|l| l.trim() == line) {
        return Ok(false);
    }
    lines.push(line);
    write_lines(root, &lines)?;
    Ok(true)
}

/// Cancel the exclusion of `dir`. Returns false when it was not excluded.
pub fn include(root: &Path, dir: &Path) -> io::Result<bool> {
    let Some(line) = line_for(root, dir) else {
        return Ok(false);
    };
    let mut lines = read_lines(root)?;
    let before = lines.len();
    lines.retain(|l| l.trim() != line);
    if lines.len() == before {
        return Ok(false);
    }
    write_lines(root, &lines)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The files a walk built the way `ui::workspace_walk` reads the workspace
    /// ignore file finds, relative to `root`.
    fn walked(root: &Path) -> Vec<String> {
        let mut files: Vec<String> = ignore::WalkBuilder::new(root)
            .hidden(false)
            .add_custom_ignore_filename(".zmax/ignore")
            .build()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .filter(|e| !e.path().starts_with(root.join(".zmax")))
            .map(|e| {
                e.path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        files.sort();
        files
    }

    #[test]
    fn an_excluded_directory_leaves_the_walk_and_comes_back_when_unmarked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for f in ["src/main.rs", "target/debug/out", "docs/target/page.md"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        let all = walked(root);

        assert!(exclude(root, &root.join("target")).unwrap());
        assert!(!exclude(root, &root.join("target")).unwrap());
        // Anchored: the nested `docs/target` is a different directory.
        assert_eq!(walked(root), ["docs/target/page.md", "src/main.rs"]);
        assert_eq!(excluded(root), [root.join("target")]);

        assert!(include(root, &root.join("target")).unwrap());
        assert!(!include(root, &root.join("target")).unwrap());
        assert_eq!(walked(root), all);
        assert!(excluded(root).is_empty());
    }

    #[test]
    fn the_users_own_patterns_survive_and_are_not_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".zmax")).unwrap();
        std::fs::write(ignore_file(root), "*.log\n!keep.log\n").unwrap();

        exclude(root, &root.join("a b/c")).unwrap();
        assert_eq!(excluded(root), [root.join("a b/c")]);
        include(root, &root.join("a b/c")).unwrap();
        assert_eq!(
            std::fs::read_to_string(ignore_file(root)).unwrap(),
            "*.log\n!keep.log\n"
        );
    }

    #[test]
    fn only_directories_below_the_root_can_be_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ws");
        assert!(exclude(&root, &root).is_err());
        assert!(exclude(&root, tmp.path()).is_err());
        assert!(exclude(&root, &root.join("../x")).is_err());
        assert!(!ignore_file(&root).exists());
    }
}
