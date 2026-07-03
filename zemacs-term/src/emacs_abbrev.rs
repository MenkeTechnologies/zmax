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

use std::path::{Path, PathBuf};

use zemacs_loader::config_dir;

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
    let incoming: Vec<(String, String)> = text.lines().filter_map(parse_line).collect();
    let n = incoming.len();
    let mut rows = load();
    for (name, exp) in &incoming {
        rows.retain(|(nn, _)| nn != name);
        rows.push((name.clone(), exp.clone()));
    }
    save(&rows);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

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
