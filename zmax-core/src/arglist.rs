//! Argument list — the zmax port of the Vim/Neovim argument list (`:args`).
//!
//! The argument list is an ordered list of file names with a "current" index,
//! seeded from the files named on the command line and navigated independently
//! of the buffer list: `:next`/`:previous` walk it, `:argument N` jumps, `:args
//! {files}` replaces it, `:argadd`/`:argdelete`/`:argdedupe` edit it. This module
//! is the pure, dependency-free state machine behind those commands — the command
//! layer owns one instance and opens `current_file()` whenever the index moves.
//! Matches Vim's documented semantics (`:help argument-list`), including the
//! end-of-list errors and glob-based `:argdelete`.

/// The ordered argument list plus the current index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArgList {
    files: Vec<String>,
    current: usize,
}

impl ArgList {
    pub fn new() -> Self {
        ArgList::default()
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The current index (0-based). Meaningless when the list is empty.
    pub fn index(&self) -> usize {
        self.current
    }

    /// The current file, if any.
    pub fn current_file(&self) -> Option<&str> {
        self.files.get(self.current).map(String::as_str)
    }

    /// `:args {files}` — replace the whole list and point at the first entry.
    pub fn set(&mut self, files: Vec<String>) {
        self.files = files;
        self.current = 0;
    }

    /// `:argadd {files}` — insert the names just after the current entry (Vim's
    /// behaviour). On an empty list they become the list. The current index is
    /// left on the same file it was on.
    pub fn add(&mut self, files: Vec<String>) {
        if self.files.is_empty() {
            self.files = files;
            self.current = 0;
            return;
        }
        let at = self.current + 1;
        for (i, f) in files.into_iter().enumerate() {
            self.files.insert(at + i, f);
        }
    }

    /// `:argedit {file}` — add the file after the current entry (if not already
    /// the current file) and make it current. Returns the file to edit.
    pub fn edit(&mut self, file: String) -> String {
        if self.current_file() == Some(file.as_str()) {
            return file;
        }
        if self.files.is_empty() {
            self.files.push(file.clone());
            self.current = 0;
        } else {
            self.files.insert(self.current + 1, file.clone());
            self.current += 1;
        }
        file
    }

    /// `:argdelete {patterns}` — remove every entry matching any glob pattern.
    /// Returns how many were removed. The current index is clamped to stay in
    /// range (Vim keeps it on the entry that followed the deleted block).
    pub fn delete_matching(&mut self, patterns: &[&str]) -> usize {
        let before = self.files.len();
        let mut removed_before_current = 0;
        let mut kept = Vec::with_capacity(before);
        for (i, f) in self.files.iter().enumerate() {
            if patterns.iter().any(|p| glob_match(p, f)) {
                if i < self.current {
                    removed_before_current += 1;
                }
            } else {
                kept.push(f.clone());
            }
        }
        self.files = kept;
        self.current = self
            .current
            .saturating_sub(removed_before_current)
            .min(self.files.len().saturating_sub(1));
        before - self.files.len()
    }

    /// `:argdedupe` — remove later duplicates, keeping the first occurrence.
    pub fn dedupe(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let current_file = self.current_file().map(str::to_string);
        self.files.retain(|f| seen.insert(f.clone()));
        // Re-point the index at the file we were on (its first occurrence).
        if let Some(cf) = current_file {
            if let Some(pos) = self.files.iter().position(|f| f == &cf) {
                self.current = pos;
            }
        }
        self.current = self.current.min(self.files.len().saturating_sub(1));
    }

    /// `:next` — advance by `count` (default 1). Returns the new current file,
    /// or `None` (without moving) if that would go past the last entry — Vim's
    /// "E165: Cannot go beyond last file".
    pub fn next(&mut self, count: usize) -> Option<&str> {
        let count = count.max(1);
        if self.files.is_empty() || self.current + count >= self.files.len() {
            return None;
        }
        self.current += count;
        self.current_file()
    }

    /// `:previous`/`:Next` — retreat by `count` (default 1). `None` at the start.
    pub fn prev(&mut self, count: usize) -> Option<&str> {
        let count = count.max(1);
        if self.current < count {
            return None;
        }
        self.current -= count;
        self.current_file()
    }

    /// `:first`/`:rewind`.
    pub fn first(&mut self) -> Option<&str> {
        self.current = 0;
        self.current_file()
    }

    /// `:last`.
    pub fn last(&mut self) -> Option<&str> {
        self.current = self.files.len().saturating_sub(1);
        self.current_file()
    }

    /// `:argument N` — go to the 1-based Nth entry. Returns `None` if out of
    /// range (leaving the index unchanged).
    pub fn goto(&mut self, n: usize) -> Option<&str> {
        if n == 0 || n > self.files.len() {
            return None;
        }
        self.current = n - 1;
        self.current_file()
    }

    /// `:args` with no argument — the display form: each file space-separated,
    /// the current one wrapped in `[brackets]`.
    pub fn display(&self) -> String {
        self.files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                if i == self.current {
                    format!("[{f}]")
                } else {
                    f.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A classic shell wildcard match supporting `*` (any run, including empty),
/// `?` (any single char), and literals. Anchored to the whole string. Used by
/// `:argdelete {pattern}`.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // Iterative wildcard matcher with backtracking on `*` (O(n*m) worst case).
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut star_ti): (Option<usize>, usize) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn al(files: &[&str]) -> ArgList {
        let mut a = ArgList::new();
        a.set(files.iter().map(|s| s.to_string()).collect());
        a
    }

    #[test]
    fn set_and_current() {
        let a = al(&["a.rs", "b.rs", "c.rs"]);
        assert_eq!(a.len(), 3);
        assert_eq!(a.current_file(), Some("a.rs"));
        assert_eq!(a.display(), "[a.rs] b.rs c.rs");
    }

    #[test]
    fn next_prev_bounds() {
        let mut a = al(&["a", "b", "c"]);
        assert_eq!(a.next(1), Some("b"));
        assert_eq!(a.next(1), Some("c"));
        assert_eq!(a.next(1), None); // past last: no move
        assert_eq!(a.current_file(), Some("c"));
        assert_eq!(a.prev(2), Some("a"));
        assert_eq!(a.prev(1), None); // before first
        assert_eq!(a.current_file(), Some("a"));
    }

    #[test]
    fn first_last_goto() {
        let mut a = al(&["a", "b", "c", "d"]);
        assert_eq!(a.last(), Some("d"));
        assert_eq!(a.index(), 3);
        assert_eq!(a.first(), Some("a"));
        assert_eq!(a.goto(3), Some("c"));
        assert_eq!(a.goto(0), None);
        assert_eq!(a.goto(99), None);
        assert_eq!(a.current_file(), Some("c")); // unchanged on bad goto
    }

    #[test]
    fn add_after_current() {
        let mut a = al(&["a", "b", "c"]);
        a.next(1); // current = b (index 1)
        a.add(vec!["x".into(), "y".into()]);
        assert_eq!(a.files(), &["a", "b", "x", "y", "c"]);
        assert_eq!(a.current_file(), Some("b")); // index unchanged
        let mut empty = ArgList::new();
        empty.add(vec!["only".into()]);
        assert_eq!(empty.current_file(), Some("only"));
    }

    #[test]
    fn edit_adds_and_selects() {
        let mut a = al(&["a", "b"]);
        assert_eq!(a.edit("z".into()), "z");
        assert_eq!(a.files(), &["a", "z", "b"]);
        assert_eq!(a.current_file(), Some("z"));
        // editing the current file again is a no-op re-selection
        assert_eq!(a.edit("z".into()), "z");
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn delete_by_glob() {
        let mut a = al(&["main.rs", "lib.rs", "notes.txt", "read.md"]);
        a.goto(3); // on notes.txt
        let n = a.delete_matching(&["*.rs"]);
        assert_eq!(n, 2);
        assert_eq!(a.files(), &["notes.txt", "read.md"]);
        // current followed the surviving entry it was on
        assert_eq!(a.current_file(), Some("notes.txt"));
    }

    #[test]
    fn dedupe_keeps_first() {
        let mut a = al(&["a", "b", "a", "c", "b"]);
        a.goto(5); // on the second "b"
        a.dedupe();
        assert_eq!(a.files(), &["a", "b", "c"]);
        assert_eq!(a.current_file(), Some("b")); // re-pointed to first "b"
    }

    #[test]
    fn glob_semantics() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("main.*", "main.rs"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a?c", "abc"));
        assert!(glob_match("src/*.rs", "src/lib.rs"));
        assert!(!glob_match("*.rs", "main.txt"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("a*b*c", "azzbzzc"));
        assert!(!glob_match("a*b*c", "azzbzz"));
    }
}
