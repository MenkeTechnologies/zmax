//! Emacs abbrevs (`C-x a g` define, `C-x '` expand).
//!
//! A persistent table of abbreviation -> expansion, stored at
//! `<config-dir>/abbrevs` as `name\texpansion` lines. This ports emacs's
//! *explicit* expansion (`expand-abbrev`, `C-x '`) plus a define command;
//! auto-expansion as you type (full `abbrev-mode`) would need an insert-keypress
//! hook and is left for later — explicit expansion is the non-invasive core.
//!
//! Tab in a name is unsupported (names are single words); newline/tab in an
//! expansion are escaped so the one-row-per-line store stays intact.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use zmax_loader::config_dir;

const FILE_NAME: &str = "abbrevs";

fn store_path() -> PathBuf {
    config_dir().join(FILE_NAME)
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse one `name\texpansion` row (expansion is unescaped).
fn parse_line(line: &str) -> Option<(String, String)> {
    let (name, exp) = line.split_once('\t')?;
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), unescape(exp)))
}

fn format_line(name: &str, expansion: &str) -> String {
    format!("{}\t{}", name, escape(expansion))
}

fn load() -> Vec<(String, String)> {
    match std::fs::read_to_string(store_path()) {
        Ok(s) => s.lines().filter_map(parse_line).collect(),
        Err(_) => Vec::new(),
    }
}

fn save(rows: &[(String, String)]) {
    let body: String = rows
        .iter()
        .map(|(n, e)| format_line(n, e))
        .collect::<Vec<_>>()
        .join("\n");
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, body);
}

/// Define (or replace) an abbrev.
pub fn define(name: &str, expansion: &str) {
    let mut rows = load();
    rows.retain(|(n, _)| n != name);
    rows.push((name.to_string(), expansion.to_string()));
    save(&rows);
}

/// Look up an abbrev's expansion.
pub fn get(name: &str) -> Option<String> {
    load().into_iter().find(|(n, _)| n == name).map(|(_, e)| e)
}

/// `write-abbrev-file`: write every abbrev to `path` in the store's
/// `name\texpansion` format. Returns how many abbrevs were written.
pub fn write_to(path: &Path) -> std::io::Result<usize> {
    let rows = load();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body: String = rows
        .iter()
        .map(|(n, e)| format_line(n, e))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, body)?;
    Ok(rows.len())
}

/// `read-abbrev-file`: read abbrevs from `path` and merge them into the store
/// (a definition replaces a same-named one). Returns how many were read.
pub fn read_from(path: &Path) -> std::io::Result<usize> {
    let text = std::fs::read_to_string(path)?;
    Ok(define_from_text(&text))
}

/// Every abbrev as a `name\texpansion` block, for `insert-abbrevs`.
pub fn serialize() -> String {
    load()
        .iter()
        .map(|(n, e)| format_line(n, e))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Define abbrevs from a `name\texpansion` block (`define-abbrevs`,
/// `read-abbrev-file`), merging into the store. Returns how many were defined.
pub fn define_from_text(text: &str) -> usize {
    let incoming: Vec<(String, String)> = text.lines().filter_map(parse_line).collect();
    let n = incoming.len();
    let mut rows = load();
    for (name, exp) in &incoming {
        rows.retain(|(nn, _)| nn != name);
        rows.push((name.clone(), exp.clone()));
    }
    save(&rows);
    n
}

// --- Mode-local (major-mode) abbrev tables ---------------------------------
//
// Emacs keeps a per-major-mode `*-mode-abbrev-table` alongside the
// `global-abbrev-table`; `expand-abbrev` searches the buffer's local table
// before the global one. These tables are keyed by the mode name (zmax's
// document language, e.g. `rust`), and are in-memory for the session — the
// global table above stays file-backed, matching the `abbrevs` file emacs
// persists by default while mode abbrevs are typically (re)defined per session.

static MODE_TABLES: Lazy<Mutex<HashMap<String, HashMap<String, String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// `define-mode-abbrev` — define (or replace) `name → expansion` in `mode`'s
/// local abbrev table.
pub fn define_mode(mode: &str, name: &str, expansion: &str) {
    MODE_TABLES
        .lock()
        .unwrap()
        .entry(mode.to_string())
        .or_default()
        .insert(name.to_string(), expansion.to_string());
}

/// Look up `name` in `mode`'s local abbrev table only.
pub fn get_mode(mode: &str, name: &str) -> Option<String> {
    MODE_TABLES
        .lock()
        .unwrap()
        .get(mode)
        .and_then(|t| t.get(name).cloned())
}

/// The lookup `expand-abbrev` uses: the buffer's mode-local table (when `mode`
/// is set) wins over the global table, mirroring emacs's local-then-global
/// search order.
pub fn get_effective(mode: Option<&str>, name: &str) -> Option<String> {
    if let Some(m) = mode {
        if let Some(exp) = get_mode(m, name) {
            return Some(exp);
        }
    }
    get(name)
}

/// All mode-local abbrevs for `mode`, sorted by name (for `list-abbrevs`).
pub fn mode_entries(mode: &str) -> Vec<(String, String)> {
    let tables = MODE_TABLES.lock().unwrap();
    let Some(t) = tables.get(mode) else {
        return Vec::new();
    };
    let mut v: Vec<(String, String)> = t.iter().map(|(n, e)| (n.clone(), e.clone())).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_tables_are_local_to_their_mode() {
        // Hermetic: touches only the in-memory MODE_TABLES, never the file store,
        // so it can't pollute the user's `abbrevs` file. Names are unique to this
        // test to survive the shared process-global map.
        define_mode("mtl_rust", "mtl_only", "rust-only");
        define_mode("mtl_rust", "mtl_two", "rust-two");
        // Resolvable only within its own mode.
        assert_eq!(
            get_mode("mtl_rust", "mtl_only").as_deref(),
            Some("rust-only")
        );
        assert_eq!(
            get_effective(Some("mtl_rust"), "mtl_only").as_deref(),
            Some("rust-only")
        );
        // Another mode and the global-only lookup (None) don't see it. `get(None)`
        // falls through to the (empty-for-this-name) global store.
        assert!(get_effective(Some("mtl_python"), "mtl_only").is_none());
        assert!(get_effective(None, "mtl_only").is_none());
        // A later define_mode for the same key replaces the expansion.
        define_mode("mtl_rust", "mtl_only", "rust-only-v2");
        assert_eq!(
            get_mode("mtl_rust", "mtl_only").as_deref(),
            Some("rust-only-v2")
        );
        // mode_entries lists that mode's abbrevs, sorted by name.
        let e = mode_entries("mtl_rust");
        let names: Vec<&str> = e.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["mtl_only", "mtl_two"]);
        assert!(mode_entries("mtl_nonexistent").is_empty());
        // Clean up this test's mode keys.
        let mut t = MODE_TABLES.lock().unwrap();
        t.remove("mtl_rust");
    }

    #[test]
    fn round_trips_with_escaping() {
        let line = format_line("teh", "the\ttab\nand newline");
        assert_eq!(line, "teh\tthe\\ttab\\nand newline");
        let (name, exp) = parse_line(&line).unwrap();
        assert_eq!(name, "teh");
        assert_eq!(exp, "the\ttab\nand newline");
    }

    #[test]
    fn rejects_nameless_or_tabless() {
        assert!(parse_line("no-tab").is_none());
        assert!(parse_line("\texpansion").is_none());
    }

    #[test]
    fn unescape_handles_trailing_and_unknown_backslash() {
        assert_eq!(unescape("a\\"), "a\\");
        assert_eq!(unescape("a\\x"), "a\\x");
    }
}
