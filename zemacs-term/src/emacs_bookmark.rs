//! Emacs bookmarks (`C-x r m` / `C-x r b` / `C-x r l`).
//!
//! Named, persistent positions — the string-named, cross-session cousin of the
//! char-keyed point registers (`emacs_register`). Persisted at
//! `<config-dir>/bookmarks` as `name\tfile\tcharpos` lines (mirroring the
//! harpoon store), so bookmarks survive restarts. `commands.rs` prompts for the
//! name on set and offers a picker on jump.

use std::path::{Path, PathBuf};

use zemacs_loader::config_dir;

const FILE_NAME: &str = "bookmarks";

fn store_path() -> PathBuf {
    config_dir().join(FILE_NAME)
}

/// Parse one `name\tfile\tpos` row. Pure; tab-in-name is not supported (names
/// are single tokens), matching the simple store format.
fn parse_line(line: &str) -> Option<(String, PathBuf, usize)> {
    let mut parts = line.splitn(3, '\t');
    let name = parts.next()?.to_string();
    let path = parts.next()?;
    let pos = parts.next()?.parse::<usize>().ok()?;
    if name.is_empty() {
        return None;
    }
    Some((name, PathBuf::from(path), pos))
}

fn format_line(name: &str, file: &Path, pos: usize) -> String {
    format!("{}\t{}\t{}", name, file.to_string_lossy(), pos)
}

fn load() -> Vec<(String, PathBuf, usize)> {
    match std::fs::read_to_string(store_path()) {
        Ok(s) => s.lines().filter_map(parse_line).collect(),
        Err(_) => Vec::new(),
    }
}

fn save(rows: &[(String, PathBuf, usize)]) {
    let body: String = rows
        .iter()
        .map(|(n, p, o)| format_line(n, p, *o))
        .collect::<Vec<_>>()
        .join("\n");
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, body);
}

/// Set (or replace) a named bookmark.
pub fn set(name: &str, file: &Path, pos: usize) {
    let mut rows = load();
    rows.retain(|(n, _, _)| n != name);
    rows.push((name.to_string(), file.to_path_buf(), pos));
    save(&rows);
}

/// All bookmarks, in insertion order.
pub fn list() -> Vec<(String, PathBuf, usize)> {
    load()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_row() {
        let line = format_line("todo", Path::new("/src/lib.rs"), 1234);
        assert_eq!(line, "todo\t/src/lib.rs\t1234");
        let (name, path, pos) = parse_line(&line).unwrap();
        assert_eq!(name, "todo");
        assert_eq!(path, PathBuf::from("/src/lib.rs"));
        assert_eq!(pos, 1234);
    }

    #[test]
    fn rejects_malformed_rows() {
        assert!(parse_line("no-tabs-here").is_none());
        assert!(parse_line("name\t/only/two/fields").is_none());
        assert!(parse_line("name\t/path\tnotanumber").is_none());
        assert!(parse_line("\t/path\t5").is_none()); // empty name
    }
}
