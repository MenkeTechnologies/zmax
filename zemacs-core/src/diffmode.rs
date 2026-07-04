//! Diff-mode substrate — the pure, filesystem-free parser behind the zemacs port
//! of GNU Emacs `diff-mode`.
//!
//! It turns a **unified diff** (as produced by `git diff` or `diff -u`) into a
//! structured [`Diff`] model — a list of [`FileDiff`]s, each holding its old/new
//! path and a list of [`Hunk`]s, each hunk holding its `@@ … @@` header numbers
//! and body [`DiffLine`]s classified by [`LineKind`]. It also offers a flat
//! rendering ([`flatten`]) plus the hunk-count, stats and hunk-navigation helpers
//! the interactive overlay needs. No I/O and no terminal types live here, so every
//! bit of it is unit-tested below.

/// The role a single displayed diff line plays, which drives its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// An unchanged context line (` ` prefix).
    Context,
    /// An added line (`+` prefix).
    Added,
    /// A removed line (`-` prefix).
    Removed,
    /// A secondary file header such as `--- a/f` / `+++ b/f`.
    Header,
    /// The `diff --git …` / per-file banner starting a [`FileDiff`].
    FileHeader,
    /// A `@@ -a,b +c,d @@` hunk header.
    HunkHeader,
}

/// One rendered diff line: its [`LineKind`] and full text (body lines keep their
/// leading `+`/`-`/space so the overlay can show the glyph).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub text: String,
}

/// A single hunk: its `@@` header numbers, the raw header text and its body lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// All hunks touching one file, with the old and new (post-`a/`/`b/`-strip) paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub old_path: String,
    pub new_path: String,
    pub hunks: Vec<Hunk>,
}

/// A parsed unified diff: an ordered list of per-file diffs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub files: Vec<FileDiff>,
}

/// Strip a `a/` or `b/` prefix and any trailing `\t<timestamp>` (as `diff -u`
/// appends), yielding a bare path.
fn clean_path(raw: &str) -> String {
    let path = raw.split('\t').next().unwrap_or(raw).trim();
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    path.to_string()
}

/// Parse `start[,len]` from one side of a hunk header; a missing length is 1.
fn parse_range(s: &str) -> (usize, usize) {
    let mut it = s.split(',');
    let start = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let len = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);
    (start, len)
}

/// Parse the four numbers out of a `@@ -old_start,old_len +new_start,new_len @@`
/// header. Missing lengths default to 1; a malformed header yields zeros.
pub fn parse_hunk_header(line: &str) -> (usize, usize, usize, usize) {
    let inner = line.split("@@").nth(1).unwrap_or("").trim();
    let (mut old, mut new) = ((0usize, 1usize), (0usize, 1usize));
    for tok in inner.split_whitespace() {
        if let Some(r) = tok.strip_prefix('-') {
            old = parse_range(r);
        } else if let Some(r) = tok.strip_prefix('+') {
            new = parse_range(r);
        }
    }
    (old.0, old.1, new.0, new.1)
}

fn classify_body(line: &str) -> LineKind {
    match line.as_bytes().first() {
        Some(b'+') => LineKind::Added,
        Some(b'-') => LineKind::Removed,
        _ => LineKind::Context, // ' ', '\' (no-newline marker), or empty
    }
}

/// Parse a unified diff into a [`Diff`]. Understands `diff --git …` banners,
/// `--- a/…` / `+++ b/…` path headers (git or plain `diff -u`), `@@ … @@` hunk
/// headers and `+`/`-`/space body lines. Robust to leading junk and to files
/// with or without a `diff --git` banner.
pub fn parse(diff: &str) -> Diff {
    let lines: Vec<&str> = diff.lines().collect();
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cur_file: Option<FileDiff> = None;
    let mut cur_hunk: Option<Hunk> = None;

    // Flush the in-progress hunk into the current file.
    fn flush_hunk(file: &mut Option<FileDiff>, hunk: &mut Option<Hunk>) {
        if let (Some(f), Some(h)) = (file.as_mut(), hunk.take()) {
            f.hunks.push(h);
        }
    }
    // Flush the in-progress file into the file list.
    fn flush_file(files: &mut Vec<FileDiff>, file: &mut Option<FileDiff>) {
        if let Some(f) = file.take() {
            files.push(f);
        }
    }

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush_hunk(&mut cur_file, &mut cur_hunk);
            flush_file(&mut files, &mut cur_file);
            // Fallback paths from the banner (overwritten by ---/+++ if present).
            let mut parts = rest.split_whitespace();
            let old = parts.next().map(clean_path).unwrap_or_default();
            let new = parts.next().map(clean_path).unwrap_or_else(|| old.clone());
            cur_file = Some(FileDiff {
                old_path: old,
                new_path: new,
                hunks: Vec::new(),
            });
            i += 1;
        } else if line.starts_with("--- ")
            && i + 1 < lines.len()
            && lines[i + 1].starts_with("+++ ")
        {
            let old = clean_path(&line[4..]);
            let new = clean_path(&lines[i + 1][4..]);
            flush_hunk(&mut cur_file, &mut cur_hunk);
            match cur_file.as_mut() {
                // Freshly opened by a `diff --git` banner: refine its paths.
                Some(f) if f.hunks.is_empty() => {
                    f.old_path = old;
                    f.new_path = new;
                }
                // A plain (bannerless) or a subsequent file: start a new one.
                _ => {
                    flush_file(&mut files, &mut cur_file);
                    cur_file = Some(FileDiff {
                        old_path: old,
                        new_path: new,
                        hunks: Vec::new(),
                    });
                }
            }
            i += 2;
        } else if line.starts_with("@@") {
            flush_hunk(&mut cur_file, &mut cur_hunk);
            if cur_file.is_none() {
                // A hunk with no preceding file header: synthesise a file.
                cur_file = Some(FileDiff {
                    old_path: String::new(),
                    new_path: String::new(),
                    hunks: Vec::new(),
                });
            }
            let (os, ol, ns, nl) = parse_hunk_header(line);
            cur_hunk = Some(Hunk {
                old_start: os,
                old_len: ol,
                new_start: ns,
                new_len: nl,
                header: line.to_string(),
                lines: Vec::new(),
            });
            i += 1;
        } else {
            if let Some(h) = cur_hunk.as_mut() {
                h.lines.push(DiffLine {
                    kind: classify_body(line),
                    text: line.to_string(),
                });
            }
            // Non-hunk chrome (index …, similarity …, mode …) is ignored.
            i += 1;
        }
    }
    flush_hunk(&mut cur_file, &mut cur_hunk);
    flush_file(&mut files, &mut cur_file);

    Diff { files }
}

/// Total number of hunks across every file.
pub fn hunk_count(diff: &Diff) -> usize {
    diff.files.iter().map(|f| f.hunks.len()).sum()
}

/// Count of `(added, removed)` body lines across the whole diff.
pub fn stats(diff: &Diff) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for f in &diff.files {
        for h in &f.hunks {
            for l in &h.lines {
                match l.kind {
                    LineKind::Added => added += 1,
                    LineKind::Removed => removed += 1,
                    _ => {}
                }
            }
        }
    }
    (added, removed)
}

/// Flatten a parsed diff into the linear list of renderable lines the overlay
/// scrolls: one [`LineKind::FileHeader`] per file, its `---`/`+++` [`LineKind::Header`]
/// lines, then each hunk's [`LineKind::HunkHeader`] followed by its body lines.
pub fn flatten(diff: &Diff) -> Vec<DiffLine> {
    let mut out = Vec::new();
    for f in &diff.files {
        out.push(DiffLine {
            kind: LineKind::FileHeader,
            text: format!("diff  {}  →  {}", f.old_path, f.new_path),
        });
        out.push(DiffLine {
            kind: LineKind::Header,
            text: format!("--- {}", f.old_path),
        });
        out.push(DiffLine {
            kind: LineKind::Header,
            text: format!("+++ {}", f.new_path),
        });
        for h in &f.hunks {
            out.push(DiffLine {
                kind: LineKind::HunkHeader,
                text: h.header.clone(),
            });
            out.extend(h.lines.iter().cloned());
        }
    }
    out
}

/// Index of the first [`LineKind::HunkHeader`] strictly after `from`, if any.
pub fn next_hunk_line(lines_flat: &[LineKind], from: usize) -> Option<usize> {
    lines_flat
        .iter()
        .enumerate()
        .skip(from.saturating_add(1))
        .find(|(_, k)| **k == LineKind::HunkHeader)
        .map(|(i, _)| i)
}

/// Index of the last [`LineKind::HunkHeader`] strictly before `from`, if any.
pub fn prev_hunk_line(lines_flat: &[LineKind], from: usize) -> Option<usize> {
    lines_flat
        .iter()
        .enumerate()
        .take(from.min(lines_flat.len()))
        .rev()
        .find(|(_, k)| **k == LineKind::HunkHeader)
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_FILE: &str = "\
diff --git a/src/foo.rs b/src/foo.rs
index 111..222 100644
--- a/src/foo.rs
+++ b/src/foo.rs
@@ -1,3 +1,4 @@
 fn foo() {
-    old();
+    new();
+    extra();
 }
@@ -10,2 +11,2 @@
 tail
-gone
+kept
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1 +1 @@
-# Title
+# New Title
";

    #[test]
    fn parses_two_file_multi_hunk_diff() {
        let d = parse(TWO_FILE);
        assert_eq!(d.files.len(), 2, "two files parsed");
        assert_eq!(d.files[0].new_path, "src/foo.rs");
        assert_eq!(d.files[1].new_path, "README.md");
        assert_eq!(d.files[0].hunks.len(), 2, "first file has two hunks");
        assert_eq!(d.files[1].hunks.len(), 1, "second file has one hunk");
    }

    #[test]
    fn counts_hunks() {
        let d = parse(TWO_FILE);
        assert_eq!(hunk_count(&d), 3);
    }

    #[test]
    fn parses_hunk_header_numbers() {
        let (os, ol, ns, nl) = parse_hunk_header("@@ -1,3 +1,4 @@ fn foo()");
        assert_eq!((os, ol, ns, nl), (1, 3, 1, 4));
        // Omitted lengths default to 1.
        let (os, ol, ns, nl) = parse_hunk_header("@@ -10 +11 @@");
        assert_eq!((os, ol, ns, nl), (10, 1, 11, 1));
        // Reflected on the parsed model.
        let d = parse(TWO_FILE);
        let h = &d.files[0].hunks[0];
        assert_eq!(
            (h.old_start, h.old_len, h.new_start, h.new_len),
            (1, 3, 1, 4)
        );
    }

    #[test]
    fn classifies_body_lines() {
        let d = parse(TWO_FILE);
        let kinds: Vec<LineKind> = d.files[0].hunks[0].lines.iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            vec![
                LineKind::Context, // " fn foo() {"
                LineKind::Removed, // "-    old();"
                LineKind::Added,   // "+    new();"
                LineKind::Added,   // "+    extra();"
                LineKind::Context, // " }"
            ]
        );
    }

    #[test]
    fn computes_stats() {
        let d = parse(TWO_FILE);
        // added: new(), extra(), kept, "# New Title" = 4; removed: old(), gone, "# Title" = 3.
        assert_eq!(stats(&d), (4, 3));
    }

    #[test]
    fn navigates_hunks() {
        let d = parse(TWO_FILE);
        let flat = flatten(&d);
        let kinds: Vec<LineKind> = flat.iter().map(|l| l.kind).collect();
        let first = next_hunk_line(&kinds, 0).expect("a first hunk");
        assert_eq!(kinds[first], LineKind::HunkHeader);
        let second = next_hunk_line(&kinds, first).expect("a second hunk");
        assert!(second > first);
        let third = next_hunk_line(&kinds, second).expect("a third hunk");
        assert!(third > second);
        assert_eq!(next_hunk_line(&kinds, third), None, "only three hunks");
        // prev walks back.
        assert_eq!(prev_hunk_line(&kinds, third), Some(second));
        assert_eq!(prev_hunk_line(&kinds, first), None);
    }

    #[test]
    fn parses_plain_unified_diff_without_git_banner() {
        let plain = "\
--- old.txt\t2024-01-01
+++ new.txt\t2024-01-02
@@ -1,2 +1,2 @@
 keep
-drop
+add
";
        let d = parse(plain);
        assert_eq!(d.files.len(), 1);
        assert_eq!(d.files[0].old_path, "old.txt");
        assert_eq!(d.files[0].new_path, "new.txt");
        assert_eq!(hunk_count(&d), 1);
        assert_eq!(stats(&d), (1, 1));
    }
}
