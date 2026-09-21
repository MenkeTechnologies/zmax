//! Changelists — JetBrains' grouping of uncommitted files (`ChangesView.*`).
//!
//! The IDE lets you split your working changes into named lists and commit one
//! of them, which is how a refactor and the bug fix riding along with it become
//! two commits without stashing or staging by hand. Git has an index, not named
//! groups, so the grouping lives here: a file per repository under
//! `.git/zmax-changelists`, holding names and the paths assigned to them.
//!
//! Paths are stored relative to the repository root, so a changelist survives
//! the repository being moved or checked out somewhere else, and the store is
//! plain text — one `[name]` header per list, one path per line — so it can be
//! read and fixed with the editor it belongs to.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The changelists of one repository: name -> the paths assigned to it.
pub type Store = BTreeMap<String, Vec<String>>;

/// Where the store lives for the repository containing `dir`.
pub fn store_path(git_dir: &Path) -> PathBuf {
    git_dir.join("zmax-changelists")
}

/// Parse the store's text. Unknown junk is skipped rather than failing: this
/// file is meant to be hand-editable, and refusing to load it because of a
/// stray blank line would lose the grouping. Pure — unit tested.
pub fn parse(text: &str) -> Store {
    let mut store = Store::new();
    let mut current: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = name.trim().to_string();
            if !name.is_empty() {
                store.entry(name.clone()).or_default();
                current = Some(name);
            }
            continue;
        }
        if let Some(name) = &current {
            store.entry(name.clone()).or_default().push(line.to_string());
        }
    }
    store
}

/// Render the store back to text, sorted by name and by path so a diff of the
/// file shows what changed rather than a reordering. Pure — unit tested.
pub fn render(store: &Store) -> String {
    let mut out = String::new();
    for (name, paths) in store {
        out.push_str(&format!("[{name}]\n"));
        let mut paths = paths.clone();
        paths.sort();
        paths.dedup();
        for path in paths {
            out.push_str(&path);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// Put `path` in `name`, taking it out of any other list — a file belongs to
/// one changelist, as in the IDE. Returns the list it came from, if any. Pure
/// — unit tested.
pub fn assign(store: &mut Store, name: &str, path: &str) -> Option<String> {
    let mut previous = None;
    for (list, paths) in store.iter_mut() {
        if list != name && paths.iter().any(|p| p == path) {
            paths.retain(|p| p != path);
            previous = Some(list.clone());
        }
    }
    let paths = store.entry(name.to_string()).or_default();
    if !paths.iter().any(|p| p == path) {
        paths.push(path.to_string());
    }
    previous
}

/// Which list holds `path`, if any. Pure — unit tested.
pub fn list_of<'a>(store: &'a Store, path: &str) -> Option<&'a str> {
    store
        .iter()
        .find(|(_, paths)| paths.iter().any(|p| p == path))
        .map(|(name, _)| name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_render_round_trip_sorted() {
        // Written in the order they were made…
        let written = "[refactor]\nsrc/b.rs\nsrc/a.rs\n\n[bugfix]\nsrc/c.rs\n\n";
        let store = parse(written);
        assert_eq!(store["refactor"], vec!["src/b.rs", "src/a.rs"]);
        assert_eq!(store["bugfix"], vec!["src/c.rs"]);
        // …and read back sorted by list and by path, so a diff of the file
        // shows a real change rather than a reordering.
        let rendered = render(&store);
        assert_eq!(
            rendered,
            "[bugfix]\nsrc/c.rs\n\n[refactor]\nsrc/a.rs\nsrc/b.rs\n\n"
        );
        // Sorted output parses back to the same store: the round trip is
        // stable from the second write on.
        assert_eq!(render(&parse(&rendered)), rendered);
    }

    #[test]
    fn parse_skips_junk_rather_than_losing_the_grouping() {
        let store = parse("# a comment\n\nloose/path/before/any/header\n[work]\nsrc/a.rs\n[]\n");
        assert_eq!(store.len(), 1, "the empty header is not a list: {store:?}");
        assert_eq!(store["work"], vec!["src/a.rs"]);
    }

    #[test]
    fn assign_moves_a_file_between_lists() {
        let mut store = parse("[a]\nsrc/x.rs\n");
        assert_eq!(assign(&mut store, "b", "src/x.rs").as_deref(), Some("a"));
        assert!(store["a"].is_empty(), "it left the old list");
        assert_eq!(store["b"], vec!["src/x.rs"]);
        assert_eq!(list_of(&store, "src/x.rs"), Some("b"));
        // Assigning twice is not a duplicate.
        assert_eq!(assign(&mut store, "b", "src/x.rs"), None);
        assert_eq!(store["b"], vec!["src/x.rs"]);
    }
}
