//! Pure, editor-type-free substrate backing the Xref cross-reference mode
//! (`crate::ui::xref` in the term crate). Everything here is filesystem-free and
//! unit-tested in isolation: the term layer walks the directory tree, reads each
//! text file into a `(path, contents)` pair, and hands the whole set to
//! [`find_matches`], which scans for whole-word occurrences of a symbol. The UI
//! then [`group_by_file`]s the hits for display and uses [`looks_like_definition`]
//! to tell a likely definition line apart from a plain reference.
//!
//! Prior art: GNU Emacs `xref` (find-definitions / find-references). Since zemacs
//! has no semantic index, this is a grep-style match rather than a tags lookup.

/// One cross-reference hit: a whole-word occurrence of the searched symbol.
/// `line` is 1-based (natural for `path:line:` display); `col` is a 0-based
/// character column into that line. `path` is whatever string the caller keyed
/// the file by (the term layer uses an absolute path so it can reopen the file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XrefHit {
    pub path: String,
    pub line: usize,
    pub col: usize,
    pub text: String,
}

/// A "word" (identifier) character: what a symbol boundary must *not* touch.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The 0-based character column of the first *whole-word* occurrence of `symbol`
/// in `line`, or `None`. A match is whole-word when the characters immediately
/// before and after it are not identifier characters (so `foo` matches in
/// `foo()` but not in `foobar` or `a_foo`).
fn find_word_col(line: &str, symbol: &str) -> Option<usize> {
    if symbol.is_empty() {
        return None;
    }
    let mut start = 0;
    while let Some(rel) = line[start..].find(symbol) {
        let i = start + rel;
        let end = i + symbol.len();
        let before_ok = line[..i].chars().next_back().map_or(true, |c| !is_word_char(c));
        let after_ok = line[end..].chars().next().map_or(true, |c| !is_word_char(c));
        if before_ok && after_ok {
            return Some(line[..i].chars().count());
        }
        start = i + 1;
    }
    None
}

/// Scan every `(path, contents)` file for whole-word occurrences of `symbol`,
/// returning one [`XrefHit`] per matching line (the first match on that line).
/// An empty `symbol` yields no hits. Files are visited in the given order, lines
/// top-to-bottom, so the result is already in a natural display order.
pub fn find_matches(files: &[(String, String)], symbol: &str) -> Vec<XrefHit> {
    let mut hits = Vec::new();
    if symbol.is_empty() {
        return hits;
    }
    for (path, contents) in files {
        for (idx, line) in contents.lines().enumerate() {
            if let Some(col) = find_word_col(line, symbol) {
                hits.push(XrefHit {
                    path: path.clone(),
                    line: idx + 1,
                    col,
                    text: line.trim_end().to_string(),
                });
            }
        }
    }
    hits
}

/// Group hits by their `path`, preserving first-seen order of both the files and
/// the hits within each file — the shape the overlay renders (a file header
/// followed by its lines).
pub fn group_by_file(hits: &[XrefHit]) -> Vec<(String, Vec<&XrefHit>)> {
    let mut groups: Vec<(String, Vec<&XrefHit>)> = Vec::new();
    for h in hits {
        if let Some(g) = groups.iter_mut().find(|(p, _)| *p == h.path) {
            g.1.push(h);
        } else {
            groups.push((h.path.clone(), vec![h]));
        }
    }
    groups
}

/// Whether `line` looks like it *defines* `symbol` rather than merely referencing
/// it — a cheap heuristic (no parser): a definition keyword directly followed by
/// the symbol (`fn`/`let`/`struct`/`enum`/`def`/`class`/`const`/`type`/`trait`
/// `symbol`), or a top-level assignment (`symbol =`, but not the `==` comparison).
/// Both the keyword and the symbol are checked at word boundaries.
pub fn looks_like_definition(line: &str, symbol: &str) -> bool {
    if symbol.is_empty() {
        return false;
    }
    for kw in [
        "fn", "let", "struct", "enum", "def", "class", "const", "type", "trait",
    ] {
        let needle = format!("{kw} {symbol}");
        let mut start = 0;
        while let Some(rel) = line[start..].find(&needle) {
            let i = start + rel;
            let end = i + needle.len();
            let before_ok = line[..i].chars().next_back().map_or(true, |c| !is_word_char(c));
            let after_ok = line[end..].chars().next().map_or(true, |c| !is_word_char(c));
            if before_ok && after_ok {
                return true;
            }
            start = i + 1;
        }
    }
    // `symbol = ...` assignment, excluding the `symbol == ...` comparison.
    let mut start = 0;
    while let Some(rel) = line[start..].find(symbol) {
        let i = start + rel;
        let end = i + symbol.len();
        let before_ok = line[..i].chars().next_back().map_or(true, |c| !is_word_char(c));
        if before_ok {
            let rest = line[end..].trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                if !after_eq.starts_with('=') {
                    return true;
                }
            }
        }
        start = i + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, String)> {
        vec![
            (
                "a.rs".to_string(),
                "fn foo() {}\n    foo();\nlet foobar = 1;\n".to_string(),
            ),
            (
                "b.rs".to_string(),
                "// call foo here\nbar(foo, baz);\n".to_string(),
            ),
        ]
    }

    #[test]
    fn finds_whole_word_matches_with_line_and_col() {
        let hits = find_matches(&files(), "foo");
        // a.rs line 1 (fn foo), a.rs line 2 (foo();), b.rs line 1, b.rs line 2.
        // "foobar" on a.rs line 3 must NOT match.
        assert_eq!(hits.len(), 4);
        assert_eq!(hits[0].path, "a.rs");
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[0].col, 3); // "fn " is 3 chars
        assert_eq!(hits[1].line, 2);
        assert_eq!(hits[1].col, 4); // four leading spaces
    }

    #[test]
    fn respects_word_boundaries() {
        // Neither a substring inside a longer identifier nor one glued by `_`.
        assert!(find_word_col("foobar", "foo").is_none());
        assert!(find_word_col("a_foo", "foo").is_none());
        assert!(find_word_col("foo_bar", "foo").is_none());
        assert_eq!(find_word_col("x foo y", "foo"), Some(2));
        assert_eq!(find_word_col("foo()", "foo"), Some(0));
    }

    #[test]
    fn empty_symbol_finds_nothing() {
        assert!(find_matches(&files(), "").is_empty());
        assert!(find_word_col("anything", "").is_none());
    }

    #[test]
    fn groups_preserve_file_and_hit_order() {
        let hits = find_matches(&files(), "foo");
        let groups = group_by_file(&hits);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "a.rs");
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[1].0, "b.rs");
        assert_eq!(groups[1].1.len(), 2);
        // Within a group, hits stay in line order.
        assert_eq!(groups[0].1[0].line, 1);
        assert_eq!(groups[0].1[1].line, 2);
    }

    #[test]
    fn definition_heuristic_accepts_definitions() {
        assert!(looks_like_definition("fn foo() {}", "foo"));
        assert!(looks_like_definition("    let foo = 1;", "foo"));
        assert!(looks_like_definition("pub struct foo;", "foo"));
        assert!(looks_like_definition("class foo:", "foo"));
        assert!(looks_like_definition("def foo():", "foo"));
        assert!(looks_like_definition("foo = 42", "foo"));
        assert!(looks_like_definition("enum foo {", "foo"));
    }

    #[test]
    fn definition_heuristic_rejects_references() {
        assert!(!looks_like_definition("    foo();", "foo"));
        assert!(!looks_like_definition("bar(foo, baz);", "foo"));
        assert!(!looks_like_definition("if foo == 1 {", "foo")); // comparison, not assignment
        assert!(!looks_like_definition("// mention foo", "foo"));
        assert!(!looks_like_definition("myfn foobar", "foo")); // boundaries on both sides
    }

    #[test]
    fn col_counts_characters_not_bytes() {
        // A multibyte prefix: the column is a character index, not a byte offset.
        let col = find_word_col("héllo foo", "foo");
        assert_eq!(col, Some(6)); // h é l l o <space> = 6 chars before foo
    }
}
