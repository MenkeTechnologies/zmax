//! Named bookmarks — the zemacs port of GNU Emacs `bookmark.el` (the
//! `C-x r m` / `C-x r b` / `C-x r l` family).
//!
//! An Emacs bookmark is a string name pointing at a saved location: a file and
//! a position within it. This module is the pure, dependency-free store behind
//! those commands — an ordered `name -> {file, line, column}` table with
//! set/overwrite, jump (lookup), delete, rename, and a stable text
//! serialization for `bookmark-save` / `bookmark-load`. It performs no IO: the
//! term-crate command layer owns one instance, reads/writes the on-disk file,
//! and translates the stored `(line, column)` to a buffer position.
//!
//! Line and column are 0-based (matching zemacs's rope indexing); `column` is
//! optional so a bookmark can pin a whole line. Names are single-line tokens
//! (leading/trailing whitespace trimmed) — the serialization is one
//! tab-separated record per line, so embedded tabs/newlines are not allowed in
//! a name, mirroring the deliberately simple store format. Ordering follows
//! Emacs's `bookmark-alist`: newest-set bookmarks move to the front, so the
//! most recently touched name is offered first.

/// A saved location: a file plus a position within it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    /// The bookmarked file, stored verbatim (the command layer expands `~`).
    pub file: String,
    /// 0-based line number.
    pub line: usize,
    /// 0-based column within the line, or `None` to pin the whole line.
    pub column: Option<usize>,
}

impl Bookmark {
    pub fn new(file: impl Into<String>, line: usize, column: Option<usize>) -> Bookmark {
        Bookmark {
            file: file.into(),
            line,
            column,
        }
    }
}

/// An ordered map of bookmark name -> location, front == most recently set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BookmarkStore {
    /// `(name, bookmark)` pairs, most-recently-set first (Emacs `bookmark-alist`
    /// order). Names are unique.
    entries: Vec<(String, Bookmark)>,
}

impl BookmarkStore {
    pub fn new() -> BookmarkStore {
        BookmarkStore::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `bookmark-set` (`C-x r m`): create or overwrite `name`, moving it to the
    /// front. Returns `true` if an existing bookmark of that name was replaced.
    /// A blank name (empty after trimming) is rejected and returns `false`.
    pub fn set(&mut self, name: &str, mark: Bookmark) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let replaced = self.remove_named(name).is_some();
        self.entries.insert(0, (name.to_string(), mark));
        replaced
    }

    /// `bookmark-set-no-overwrite` (`C-x r M`): create `name` only if it does
    /// not already exist. Returns `true` if it was inserted, `false` if a
    /// bookmark of that name was already present (or the name is blank).
    pub fn set_no_overwrite(&mut self, name: &str, mark: Bookmark) -> bool {
        let name = name.trim();
        if name.is_empty() || self.get(name).is_some() {
            return false;
        }
        self.entries.insert(0, (name.to_string(), mark));
        true
    }

    /// `bookmark-jump` lookup (`C-x r b`): the location named `name`, if any.
    pub fn get(&self, name: &str) -> Option<&Bookmark> {
        let name = name.trim();
        self.entries.iter().find(|(n, _)| n == name).map(|(_, b)| b)
    }

    /// `bookmark-delete`: remove `name`. Returns the removed bookmark, if any.
    pub fn delete(&mut self, name: &str) -> Option<Bookmark> {
        self.remove_named(name.trim())
    }

    /// `bookmark-rename`: rename `from` to `to`, keeping the same location and
    /// list position. Fails (returns `false`) if `from` is missing, `to` is
    /// blank, or `to` already names a different bookmark.
    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        let (from, to) = (from.trim(), to.trim());
        if to.is_empty() {
            return false;
        }
        if from == to {
            return self.get(from).is_some();
        }
        if self.get(to).is_some() {
            return false;
        }
        match self.entries.iter_mut().find(|(n, _)| n == from) {
            Some(slot) => {
                slot.0 = to.to_string();
                true
            }
            None => false,
        }
    }

    /// `bookmark-bmenu-list` / `list-bookmarks` (`C-x r l`): every `(name,
    /// bookmark)` in list order (most recent first).
    pub fn list(&self) -> &[(String, Bookmark)] {
        &self.entries
    }

    /// The bookmark names, in list order — used to offer completions.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(n, _)| n.as_str())
    }

    fn remove_named(&mut self, name: &str) -> Option<Bookmark> {
        let pos = self.entries.iter().position(|(n, _)| n == name)?;
        Some(self.entries.remove(pos).1)
    }

    /// Serialize to the simple text format: one `name\tfile\tline[\tcolumn]`
    /// record per line, in list order. A column is written only when present.
    /// Round-trips through [`deserialize`](BookmarkStore::deserialize).
    pub fn serialize(&self) -> String {
        self.entries
            .iter()
            .map(|(name, b)| match b.column {
                Some(col) => format!("{}\t{}\t{}\t{}", name, b.file, b.line, col),
                None => format!("{}\t{}\t{}", name, b.file, b.line),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Parse the text format produced by [`serialize`](BookmarkStore::serialize).
    /// Malformed lines (missing fields, empty name, non-numeric line/column) are
    /// skipped. A 3-field record has no column; a 4th field sets the column.
    pub fn deserialize(text: &str) -> BookmarkStore {
        let mut store = BookmarkStore::new();
        for line in text.lines() {
            if let Some((name, mark)) = parse_record(line) {
                // Preserve file order (front == first line): push to the back so
                // the serialized order is reproduced exactly.
                if store.get(&name).is_none() {
                    store.entries.push((name, mark));
                }
            }
        }
        store
    }
}

/// Parse one serialized record into `(name, Bookmark)`.
fn parse_record(line: &str) -> Option<(String, Bookmark)> {
    let mut parts = line.split('\t');
    let name = parts.next()?.trim().to_string();
    let file = parts.next()?;
    let line_no = parts.next()?.parse::<usize>().ok()?;
    if name.is_empty() || file.is_empty() {
        return None;
    }
    let column = match parts.next() {
        Some(c) => Some(c.parse::<usize>().ok()?),
        None => None,
    };
    Some((name, Bookmark::new(file, line_no, column)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(file: &str, line: usize, col: Option<usize>) -> Bookmark {
        Bookmark::new(file, line, col)
    }

    #[test]
    fn set_get_and_front_ordering() {
        let mut s = BookmarkStore::new();
        assert!(!s.set("a", mk("/a.rs", 1, Some(2)))); // no prior => not replaced
        assert!(!s.set("b", mk("/b.rs", 3, None)));
        assert_eq!(s.len(), 2);
        // Newest set is first (bookmark-alist order).
        assert_eq!(s.list()[0].0, "b");
        assert_eq!(s.get("a"), Some(&mk("/a.rs", 1, Some(2))));
        assert_eq!(s.get("missing"), None);
    }

    #[test]
    fn set_overwrites_and_moves_to_front() {
        let mut s = BookmarkStore::new();
        s.set("a", mk("/a.rs", 1, None));
        s.set("b", mk("/b.rs", 2, None));
        assert!(s.set("a", mk("/a.rs", 9, Some(4)))); // replaced
        assert_eq!(s.len(), 2);
        assert_eq!(s.list()[0].0, "a"); // moved to front
        assert_eq!(s.get("a"), Some(&mk("/a.rs", 9, Some(4))));
    }

    #[test]
    fn blank_name_rejected() {
        let mut s = BookmarkStore::new();
        assert!(!s.set("   ", mk("/x", 0, None)));
        assert!(s.is_empty());
        // Names are trimmed on set and lookup.
        s.set("  todo  ", mk("/x", 5, None));
        assert_eq!(s.get("todo").map(|b| b.line), Some(5));
    }

    #[test]
    fn set_no_overwrite() {
        let mut s = BookmarkStore::new();
        assert!(s.set_no_overwrite("a", mk("/a", 1, None)));
        assert!(!s.set_no_overwrite("a", mk("/a", 2, None))); // already exists
        assert_eq!(s.get("a").map(|b| b.line), Some(1)); // unchanged
    }

    #[test]
    fn delete_removes() {
        let mut s = BookmarkStore::new();
        s.set("a", mk("/a", 1, None));
        s.set("b", mk("/b", 2, None));
        assert_eq!(s.delete("a"), Some(mk("/a", 1, None)));
        assert_eq!(s.delete("a"), None); // already gone
        assert_eq!(s.len(), 1);
        assert_eq!(s.list()[0].0, "b");
    }

    #[test]
    fn rename_keeps_position_and_location() {
        let mut s = BookmarkStore::new();
        s.set("a", mk("/a", 1, None));
        s.set("b", mk("/b", 2, None)); // b is front
        assert!(s.rename("a", "z"));
        assert_eq!(s.get("z"), Some(&mk("/a", 1, None)));
        assert_eq!(s.get("a"), None);
        // b stayed at the front; z kept a's slot.
        assert_eq!(s.list()[0].0, "b");
        assert_eq!(s.list()[1].0, "z");
    }

    #[test]
    fn rename_edge_cases() {
        let mut s = BookmarkStore::new();
        s.set("a", mk("/a", 1, None));
        s.set("b", mk("/b", 2, None));
        assert!(!s.rename("missing", "x")); // source absent
        assert!(!s.rename("a", "b")); // target already exists
        assert!(!s.rename("a", "  ")); // blank target
        assert!(s.rename("a", "a")); // rename to self is a no-op success
        assert_eq!(s.get("a").map(|b| b.line), Some(1));
    }

    #[test]
    fn serialize_roundtrip() {
        let mut s = BookmarkStore::new();
        s.set("first", mk("/one.rs", 10, Some(3)));
        s.set("second", mk("/two with space.rs", 0, None));
        let text = s.serialize();
        // second is front (most recent) and has no column.
        assert_eq!(text, "second\t/two with space.rs\t0\nfirst\t/one.rs\t10\t3");
        let back = BookmarkStore::deserialize(&text);
        assert_eq!(back, s);
    }

    #[test]
    fn deserialize_skips_malformed() {
        let text = "\
good\t/a.rs\t4\t1
missing-fields
\t/no-name.rs\t0
name\t/bad-line.rs\tNaN
alsogood\t/b.rs\t7";
        let s = BookmarkStore::deserialize(text);
        assert_eq!(s.len(), 2);
        assert_eq!(s.get("good"), Some(&mk("/a.rs", 4, Some(1))));
        assert_eq!(s.get("alsogood"), Some(&mk("/b.rs", 7, None)));
    }

    #[test]
    fn deserialize_dedups_keeping_first() {
        let text = "dup\t/a\t1\ndup\t/b\t2";
        let s = BookmarkStore::deserialize(text);
        assert_eq!(s.len(), 1);
        assert_eq!(s.get("dup"), Some(&mk("/a", 1, None)));
    }
}
