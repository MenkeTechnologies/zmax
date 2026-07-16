//! Session persistence, backing the Emacs `desktop` family (`desktop-save`,
//! `desktop-read`, `desktop-clear`, `desktop-change-dir`, `desktop-revert`).
//! Emacs' `desktop-save` records the list of file-visiting buffers plus point
//! for each so a later `desktop-read` reopens the same working set.
//!
//! This module owns the **on-disk desktop-file format** and nothing else: the
//! editor-facing commands enumerate open buffers and drive file opening, but the
//! serialize/parse round-trip lives here as pure functions so it can be unit
//! tested without an editor.
//!
//! ## Format
//!
//! A line-oriented text file. Lines beginning with `;;` (comments) and blank
//! lines are ignored. Each data line is four TAB-separated fields:
//!
//! ```text
//! flag<TAB>line<TAB>column<TAB>path
//! ```
//!
//! - `flag` is `*` for the buffer that was current when the desktop was saved,
//!   `-` otherwise.
//! - `line` / `column` are the zero-based point position (decimal).
//! - `path` is the file path and comes last so it may itself contain TABs.
//!
//! Parsing is tolerant: a data line with no TABs is treated as a bare path
//! (flag `-`, point `0,0`), so hand-written desktop files listing one path per
//! line work.

/// One saved buffer: a visited file plus its point position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// Whether this buffer was the current one when the desktop was saved.
    pub current: bool,
    /// Zero-based line of point.
    pub line: usize,
    /// Zero-based column of point.
    pub column: usize,
    /// Path of the visited file.
    pub path: String,
}

impl DesktopEntry {
    /// Construct a non-current entry at point `(line, column)`.
    pub fn new(path: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            current: false,
            line,
            column,
            path: path.into(),
        }
    }
}

/// Default desktop-file basename, stored inside the desktop directory. Emacs uses
/// `.emacs.desktop`; zmax keeps its own name so the two never collide.
pub const FILE_NAME: &str = ".zmax.desktop";

const HEADER: &str = "\
;; -*- mode: emacs-desktop -*-
;; zmax desktop file — do not edit while zmax is running.
;; fields: flag<TAB>line<TAB>column<TAB>path  (flag `*` = current buffer)";

/// Serialize `entries` to desktop-file text (trailing newline included). Entries
/// are written in the given order; the caller decides ordering.
pub fn serialize(entries: &[DesktopEntry]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for e in entries {
        let flag = if e.current { '*' } else { '-' };
        out.push_str(&format!("{flag}\t{}\t{}\t{}\n", e.line, e.column, e.path));
    }
    out
}

/// Parse desktop-file `contents` into entries. Comment (`;;`) and blank lines are
/// skipped. Malformed numeric fields degrade to `0`; a line with no TAB is taken
/// as a bare path. Lines whose path field is empty are dropped.
pub fn parse(contents: &str) -> Vec<DesktopEntry> {
    let mut entries = Vec::new();
    for raw in contents.lines() {
        let line = raw.trim_end_matches(['\r']);
        if line.trim().is_empty() || line.starts_with(";;") {
            continue;
        }
        // Split into at most 4 fields so a TAB inside a path is preserved.
        let mut parts = line.splitn(4, '\t');
        let first = parts.next().unwrap_or("");
        match (parts.next(), parts.next(), parts.next()) {
            (Some(line_s), Some(col_s), Some(path)) => {
                if path.is_empty() {
                    continue;
                }
                entries.push(DesktopEntry {
                    current: first == "*",
                    line: line_s.trim().parse().unwrap_or(0),
                    column: col_s.trim().parse().unwrap_or(0),
                    path: path.to_string(),
                });
            }
            // Fewer than four fields: treat the whole line as a bare path.
            _ => {
                let path = line.trim();
                if !path.is_empty() {
                    entries.push(DesktopEntry::new(path, 0, 0));
                }
            }
        }
    }
    entries
}

/// Index of the entry marked current, if any. When several are marked (a
/// hand-edited file), the first wins.
pub fn current_index(entries: &[DesktopEntry]) -> Option<usize> {
    entries.iter().position(|e| e.current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_entries_and_current_flag() {
        let entries = vec![
            DesktopEntry {
                current: false,
                line: 0,
                column: 0,
                path: "/home/x/a.rs".into(),
            },
            DesktopEntry {
                current: true,
                line: 12,
                column: 4,
                path: "/home/x/b.rs".into(),
            },
        ];
        let text = serialize(&entries);
        assert_eq!(parse(&text), entries);
        assert_eq!(current_index(&parse(&text)), Some(1));
    }

    #[test]
    fn header_is_emitted_and_ignored_on_read() {
        let text = serialize(&[DesktopEntry::new("/a", 0, 0)]);
        assert!(text.starts_with(";;"));
        // Three comment lines + one data line, all round-trip to one entry.
        assert_eq!(parse(&text).len(), 1);
    }

    #[test]
    fn blank_and_comment_lines_are_skipped() {
        let text = "\
;; a comment
-\t3\t7\t/etc/hosts

;; another
*\t0\t0\t/tmp/f
";
        let got = parse(text);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], DesktopEntry::new("/etc/hosts", 3, 7));
        assert!(got[1].current);
    }

    #[test]
    fn path_may_contain_tabs() {
        let weird = "/tmp/has\ttab.txt";
        let entries = vec![DesktopEntry::new(weird, 1, 2)];
        let parsed = parse(&serialize(&entries));
        assert_eq!(parsed[0].path, weird);
        assert_eq!((parsed[0].line, parsed[0].column), (1, 2));
    }

    #[test]
    fn bare_path_line_tolerated() {
        let got = parse("/home/me/notes.md\n/home/me/todo.md\n");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], DesktopEntry::new("/home/me/notes.md", 0, 0));
        assert!(!got[0].current);
    }

    #[test]
    fn malformed_numeric_fields_degrade_to_zero() {
        let got = parse("-\txx\t-\t/a/b\n");
        assert_eq!(got, vec![DesktopEntry::new("/a/b", 0, 0)]);
    }

    #[test]
    fn empty_path_field_dropped() {
        let got = parse("-\t0\t0\t\n");
        assert!(got.is_empty());
    }

    #[test]
    fn empty_input_yields_no_entries() {
        assert!(parse("").is_empty());
        assert_eq!(serialize(&[]), format!("{HEADER}\n"));
    }
}
