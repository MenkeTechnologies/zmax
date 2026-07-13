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

// ===========================================================================
// diff-mode text transforms (the pure logic behind the interactive commands).
//
// These all take the RAW diff text (never the parsed model), because the
// interactive commands must preserve every byte the parser drops — `index …`
// lines, mode-change lines, arbitrary chrome — when they kill, split, reverse
// or convert a hunk. They operate on a line vector and reconstruct the text,
// preserving the trailing-newline state of the input.
// ===========================================================================

/// Split `text` into owned lines plus whether it ended with a newline (so the
/// reconstruction round-trips exactly).
fn to_lines(text: &str) -> (Vec<String>, bool) {
    (
        text.lines().map(|s| s.to_string()).collect(),
        text.ends_with('\n'),
    )
}

/// Rejoin lines produced by [`to_lines`], restoring the trailing newline.
fn from_lines(lines: &[String], trailing_nl: bool) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut s = lines.join("\n");
    if trailing_nl {
        s.push('\n');
    }
    s
}

/// Line indices at which a new file section begins. A section starts at a
/// `diff --git`/`Index:` banner, or at a bare `--- `/`+++ ` header pair not
/// already introduced by such a banner.
fn file_start_lines(lines: &[String]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut header_pending = false;
    let n = lines.len();
    for i in 0..n {
        let l = lines[i].as_str();
        if l.starts_with("diff --git ") || l.starts_with("Index: ") {
            starts.push(i);
            header_pending = true;
        } else if l.starts_with("--- ") && i + 1 < n && lines[i + 1].starts_with("+++ ") {
            if !header_pending {
                starts.push(i);
            }
            header_pending = false;
        } else if l.starts_with("@@") {
            header_pending = false;
        }
    }
    starts
}

/// The `[start, end)` line range of the file section containing line `at`.
fn file_bounds_at(lines: &[String], starts: &[usize], at: usize) -> (usize, usize) {
    if starts.is_empty() {
        return (0, lines.len());
    }
    let idx = starts.iter().rposition(|&s| s <= at).unwrap_or(0);
    let start = starts[idx];
    let end = starts.get(idx + 1).copied().unwrap_or(lines.len());
    (start, end)
}

/// The `[start, end)` line range of the file section containing `at_line`, or
/// `None` if the buffer holds no file section. Used by `diff-restrict-view`
/// with a prefix argument.
pub fn file_line_bounds(text: &str, at_line: usize) -> Option<(usize, usize)> {
    let (lines, _) = to_lines(text);
    if lines.is_empty() {
        return None;
    }
    let at = at_line.min(lines.len() - 1);
    let starts = file_start_lines(&lines);
    if starts.is_empty() {
        return None;
    }
    Some(file_bounds_at(&lines, &starts, at))
}

/// The `[start, end)` line range of the hunk containing `at_line` (its `@@`
/// header through the last body line), or `None` if there is no hunk. Used by
/// `diff-restrict-view` and `diff-split-hunk`.
pub fn hunk_line_bounds(text: &str, at_line: usize) -> Option<(usize, usize)> {
    let (lines, _) = to_lines(text);
    if lines.is_empty() {
        return None;
    }
    let at = at_line.min(lines.len() - 1);
    let starts = file_start_lines(&lines);
    let (fs, fe) = file_bounds_at(&lines, &starts, at);
    let hunk_starts: Vec<usize> = (fs..fe).filter(|&i| lines[i].starts_with("@@")).collect();
    if hunk_starts.is_empty() {
        return None;
    }
    let idx = hunk_starts.iter().rposition(|&s| s <= at).unwrap_or(0);
    let start = hunk_starts[idx];
    let end = hunk_starts.get(idx + 1).copied().unwrap_or(fe);
    Some((start, end))
}

/// `diff-hunk-kill`: delete the hunk at `at_line`. If it is the only hunk of
/// its file the whole file section (headers included) is removed too, matching
/// GNU Emacs `diff-remove-hunk`. Returns the new diff text, or `None` if point
/// is not in any hunk.
pub fn diff_hunk_kill(text: &str, at_line: usize) -> Option<String> {
    let (lines, nl) = to_lines(text);
    if lines.is_empty() {
        return None;
    }
    let at = at_line.min(lines.len() - 1);
    let starts = file_start_lines(&lines);
    let (fs, fe) = file_bounds_at(&lines, &starts, at);
    let hunk_starts: Vec<usize> = (fs..fe).filter(|&i| lines[i].starts_with("@@")).collect();
    if hunk_starts.is_empty() {
        return None;
    }
    let (rs, re) = if hunk_starts.len() == 1 {
        (fs, fe)
    } else {
        let idx = hunk_starts.iter().rposition(|&s| s <= at).unwrap_or(0);
        (
            hunk_starts[idx],
            hunk_starts.get(idx + 1).copied().unwrap_or(fe),
        )
    };
    let mut out = lines;
    out.drain(rs..re);
    Some(from_lines(&out, nl))
}

/// `diff-file-kill`: delete the whole file section (banner, `---`/`+++` headers
/// and every hunk) containing `at_line`. Returns `None` if there is no file
/// section.
pub fn diff_file_kill(text: &str, at_line: usize) -> Option<String> {
    let (lines, nl) = to_lines(text);
    if lines.is_empty() {
        return None;
    }
    let at = at_line.min(lines.len() - 1);
    let starts = file_start_lines(&lines);
    if starts.is_empty() {
        return None;
    }
    let (fs, fe) = file_bounds_at(&lines, &starts, at);
    let mut out = lines;
    out.drain(fs..fe);
    Some(from_lines(&out, nl))
}

/// Count `(old, new)` lines in a unified-hunk body slice: a context line counts
/// on both sides, a `-` line only old, a `+` line only new, a `\ No newline`
/// marker on neither.
fn count_old_new(body: &[String]) -> (usize, usize) {
    let mut old = 0;
    let mut new = 0;
    for l in body {
        match l.as_bytes().first() {
            Some(b'+') => new += 1,
            Some(b'-') => old += 1,
            Some(b'\\') => {}
            _ => {
                old += 1;
                new += 1;
            }
        }
    }
    (old, new)
}

/// `diff-split-hunk`: split the unified hunk containing `at_line` into two, with
/// the boundary at `at_line`. Both resulting `@@` headers get correct start and
/// length numbers (zemacs recomputes both halves; stock Emacs leaves the second
/// header's lengths as a `,1` placeholder to be fixed up by hand). Returns
/// `None` unless point is on a body line strictly inside a unified hunk.
pub fn diff_split_hunk(text: &str, at_line: usize) -> Option<String> {
    let (lines, nl) = to_lines(text);
    if lines.is_empty() {
        return None;
    }
    let at = at_line.min(lines.len() - 1);
    let (hs, he) = hunk_line_bounds(text, at)?;
    if !lines[hs].starts_with("@@") {
        return None;
    }
    if at <= hs || at >= he {
        return None;
    }
    let (o_s, _o_l, n_s, _n_l) = parse_hunk_header(&lines[hs]);
    let first: Vec<String> = lines[hs + 1..at].to_vec();
    let second: Vec<String> = lines[at..he].to_vec();
    if first.is_empty() || second.is_empty() {
        return None;
    }
    let (fo, fnn) = count_old_new(&first);
    let (so, sn) = count_old_new(&second);
    let h1 = format!("@@ -{},{} +{},{} @@", o_s, fo, n_s, fnn);
    let h2 = format!("@@ -{},{} +{},{} @@", o_s + fo, so, n_s + fnn, sn);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    out.extend_from_slice(&lines[..hs]);
    out.push(h1);
    out.extend(first);
    out.push(h2);
    out.extend(second);
    out.extend_from_slice(&lines[he..]);
    Some(from_lines(&out, nl))
}

/// Parse a `@@ -OLD +NEW @@SUFFIX` header into the raw `OLD`/`NEW` range strings
/// (e.g. `"1,3"`, `"10"`) and the trailing text after the closing `@@`,
/// preserving the exact number format so a reverse round-trips byte-for-byte.
fn split_hunk_header_raw(l: &str) -> Option<(String, String, String)> {
    let after = l.strip_prefix("@@")?;
    let close = after.find("@@")?;
    let mid = &after[..close];
    let suffix = after[close + 2..].to_string();
    let mut old = None;
    let mut new = None;
    for tok in mid.split_whitespace() {
        if let Some(r) = tok.strip_prefix('-') {
            old = Some(r.to_string());
        } else if let Some(r) = tok.strip_prefix('+') {
            new = Some(r.to_string());
        }
    }
    Some((old?, new?, suffix))
}

/// `diff-reverse-direction`: swap the diff's direction so the patch applies in
/// reverse. `---`/`+++` file paths swap, each `@@` header's two ranges swap, and
/// every `+` body line becomes `-` and vice versa. Reversing twice restores the
/// original text exactly.
pub fn diff_reverse_direction(text: &str) -> String {
    let (lines, nl) = to_lines(text);
    let n = lines.len();
    let mut out: Vec<String> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let l = &lines[i];
        if l.starts_with("--- ") && i + 1 < n && lines[i + 1].starts_with("+++ ") {
            out.push(format!("--- {}", &lines[i + 1][4..]));
            out.push(format!("+++ {}", &l[4..]));
            i += 2;
            continue;
        }
        if let Some((old_raw, new_raw, suffix)) = split_hunk_header_raw(l) {
            out.push(format!("@@ -{} +{} @@{}", new_raw, old_raw, suffix));
            i += 1;
            continue;
        }
        match l.as_bytes().first() {
            Some(b'+') => out.push(format!("-{}", &l[1..])),
            Some(b'-') => out.push(format!("+{}", &l[1..])),
            _ => out.push(l.clone()),
        }
        i += 1;
    }
    from_lines(&out, nl)
}

/// Format a context-diff range `start,end` (1-based inclusive) from a unified
/// `start,len`, using GNU diff's `end,0` empty form for a zero-length side.
fn ctx_range(start: usize, len: usize) -> String {
    if len == 0 {
        format!("{},0", start.saturating_sub(1))
    } else {
        format!("{},{}", start, start + len - 1)
    }
}

/// Rewrite a unified body line as a context-diff body line: the one-char prefix
/// (` `, `-` or `+`) gains a trailing space (` x` → `  x`, `-x` → `- x`).
fn ctxify(l: &str) -> String {
    let mut ch = l.chars();
    match ch.next() {
        Some(c) => format!("{} {}", c, ch.as_str()),
        None => "  ".to_string(),
    }
}

/// `diff-unified->context`: convert a unified diff to a context diff, mirroring
/// GNU Emacs `diff-unified->context` — `---`/`+++` headers become `***`/`---`,
/// and each `@@` hunk becomes a `***************` / `*** o ****` / `--- n ----`
/// block whose old side keeps context and `-` lines (as `- `) and whose new side
/// keeps context and `+` lines (as `+ `). A side with no changes is emitted with
/// only its range header (no body lines).
pub fn diff_unified_to_context(text: &str) -> String {
    let (lines, nl) = to_lines(text);
    let n = lines.len();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n {
        let l = &lines[i];
        if l.starts_with("--- ") && i + 1 < n && lines[i + 1].starts_with("+++ ") {
            out.push(format!("*** {}", &l[4..]));
            out.push(format!("--- {}", &lines[i + 1][4..]));
            i += 2;
            continue;
        }
        if l.starts_with("@@") {
            let (o_s, o_l, n_s, n_l) = parse_hunk_header(l);
            let mut j = i + 1;
            while j < n
                && !(lines[j].starts_with("@@")
                    || lines[j].starts_with("diff --git ")
                    || lines[j].starts_with("Index: ")
                    || (lines[j].starts_with("--- ")
                        && j + 1 < n
                        && lines[j + 1].starts_with("+++ ")))
            {
                j += 1;
            }
            let body = &lines[i + 1..j];
            out.push("***************".to_string());
            out.push(format!("*** {} ****", ctx_range(o_s, o_l)));
            if body.iter().any(|b| b.starts_with('-')) {
                for b in body {
                    match b.as_bytes().first() {
                        Some(b'+') => {}
                        Some(b'\\') => out.push(b.clone()),
                        _ => out.push(ctxify(b)),
                    }
                }
            }
            out.push(format!("--- {} ----", ctx_range(n_s, n_l)));
            if body.iter().any(|b| b.starts_with('+')) {
                for b in body {
                    match b.as_bytes().first() {
                        Some(b'-') => {}
                        Some(b'\\') => out.push(b.clone()),
                        _ => out.push(ctxify(b)),
                    }
                }
            }
            i = j;
            continue;
        }
        out.push(l.clone());
        i += 1;
    }
    from_lines(&out, nl)
}

/// True for a `***************` hunk separator (a run of 5+ asterisks).
fn is_asterisk_sep(l: &str) -> bool {
    l.len() >= 5 && l.bytes().all(|b| b == b'*')
}

/// Parse a context range token like `1,4` / `5` / `4,0` (ignoring any trailing
/// `****`/`----`) into `(start, end)`; a missing end defaults to `start`.
fn parse_ctx_range(s: &str) -> (usize, usize) {
    let tok = s.split_whitespace().next().unwrap_or("");
    let mut it = tok.split(',');
    let a = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let b = it.next().and_then(|x| x.parse().ok()).unwrap_or(a);
    (a, b)
}

/// Convert a context range `(start, end)` back to a unified `(start, len)`,
/// undoing the `end,0` empty form.
fn unified_from_ctx_range((a, b): (usize, usize)) -> (usize, usize) {
    if b >= a {
        (a, b - a + 1)
    } else {
        (a + 1, 0)
    }
}

/// Classify a context-diff body line into a `(kind, content)` pair; `kind` is
/// the leading byte (` `, `-`, `+`, `!`) and `content` is the text after its
/// two-char prefix.
fn ctx_entry(l: &str) -> (u8, String) {
    let content = if l.len() >= 2 {
        l[2..].to_string()
    } else {
        String::new()
    };
    let kind = match l.as_bytes().first() {
        Some(b'-') => b'-',
        Some(b'+') => b'+',
        Some(b'!') => b'!',
        _ => b' ',
    };
    (kind, content)
}

/// Interleave a context diff's old and new side entries into unified body lines:
/// shared context lines emit once as ` x`, a change block emits its removed run
/// (`-x`, from `-`/`!` old entries) then its added run (`+x`, from `+`/`!` new
/// entries).
fn merge_context_sides(old: &[(u8, String)], new: &[(u8, String)], out: &mut Vec<String>) {
    let is_ctx = |e: &(u8, String)| e.0 == b' ';
    let mut i = 0;
    let mut j = 0;
    while i < old.len() || j < new.len() {
        let o_ctx = old.get(i).is_some_and(is_ctx);
        let n_ctx = new.get(j).is_some_and(is_ctx);
        if o_ctx && (j >= new.len() || n_ctx) {
            out.push(format!(" {}", old[i].1));
            i += 1;
            if n_ctx {
                j += 1;
            }
        } else if n_ctx && i >= old.len() {
            out.push(format!(" {}", new[j].1));
            j += 1;
        } else {
            let mut progressed = false;
            while i < old.len() && !is_ctx(&old[i]) {
                out.push(format!("-{}", old[i].1));
                i += 1;
                progressed = true;
            }
            while j < new.len() && !is_ctx(&new[j]) {
                out.push(format!("+{}", new[j].1));
                j += 1;
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
    }
}

/// `diff-context->unified`: convert a context diff back to a unified diff,
/// mirroring GNU Emacs `diff-context->unified`. `***`/`---` file headers become
/// `---`/`+++`, and each `*** o ****` / `--- n ----` block becomes one `@@` hunk
/// whose body interleaves the two sides (change lines marked `!` are read as
/// removed on the old side and added on the new side).
pub fn diff_context_to_unified(text: &str) -> String {
    let (lines, nl) = to_lines(text);
    let n = lines.len();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n {
        let l = &lines[i];
        // File header pair: `*** path` / `--- path` (neither is a range header).
        if l.starts_with("*** ")
            && !l.trim_end().ends_with("****")
            && i + 1 < n
            && lines[i + 1].starts_with("--- ")
            && !lines[i + 1].trim_end().ends_with("----")
        {
            out.push(format!("--- {}", &l[4..]));
            out.push(format!("+++ {}", &lines[i + 1][4..]));
            i += 2;
            continue;
        }
        if is_asterisk_sep(l) {
            i += 1;
            continue;
        }
        // Old range header `*** start,end ****`.
        if l.starts_with("*** ") && l.trim_end().ends_with("****") {
            let old_r = parse_ctx_range(&l[4..]);
            let mut j = i + 1;
            let mut old_side: Vec<(u8, String)> = Vec::new();
            while j < n && !(lines[j].starts_with("--- ") && lines[j].trim_end().ends_with("----"))
            {
                old_side.push(ctx_entry(&lines[j]));
                j += 1;
            }
            let new_r = if j < n {
                parse_ctx_range(&lines[j][4..])
            } else {
                (0, 0)
            };
            let mut k = j + 1;
            let mut new_side: Vec<(u8, String)> = Vec::new();
            while k < n
                && !is_asterisk_sep(&lines[k])
                && !lines[k].starts_with("*** ")
                && !lines[k].starts_with("diff --git ")
                && !lines[k].starts_with("Index: ")
            {
                new_side.push(ctx_entry(&lines[k]));
                k += 1;
            }
            let (us1, ul1) = unified_from_ctx_range(old_r);
            let (us3, ul3) = unified_from_ctx_range(new_r);
            out.push(format!("@@ -{},{} +{},{} @@", us1, ul1, us3, ul3));
            merge_context_sides(&old_side, &new_side, &mut out);
            i = k;
            continue;
        }
        out.push(l.clone());
        i += 1;
    }
    from_lines(&out, nl)
}

/// `diff-delete-trailing-whitespace` (in-buffer variant): strip trailing spaces
/// and tabs from the content of every added (`+`) line, leaving the `+++` header
/// alone. Stock Emacs edits the *patched source files* on disk; zemacs only
/// rewrites the added lines within the diff buffer, hence this is a partial port.
pub fn diff_delete_trailing_whitespace(text: &str) -> String {
    let (lines, nl) = to_lines(text);
    let out: Vec<String> = lines
        .iter()
        .map(|l| {
            if l.starts_with('+') && !l.starts_with("+++") {
                format!("+{}", l[1..].trim_end_matches([' ', '\t']))
            } else {
                l.clone()
            }
        })
        .collect();
    from_lines(&out, nl)
}

/// `diff-ignore-whitespace-hunk`: re-diff the hunk at `at_line` ignoring
/// whitespace, i.e. drop from it every change that is only a change of
/// whitespace.
///
/// A `-`/`+` pair whose two lines are equal once whitespace is removed is not a
/// real change: both lines are replaced by a single context line carrying the
/// `-` (old file's) text, which is what `diff -b` prints for a line it decided
/// to ignore. The `@@` header's old/new lengths are recomputed from the
/// surviving body; the start lines are untouched.
///
/// Pairs are matched within each maximal run of `-` lines followed by `+` lines,
/// positionally (the i-th `-` against the i-th `+`), which is what `diff -b`
/// yields for a hunk that only re-indented lines. A run whose ignored pair sits
/// between real changes is split around the context line, keeping unified order
/// (context lines appear where they belong in both files). Returns `None` when
/// point is not in a unified hunk, and the text unchanged when the hunk holds no
/// whitespace-only change.
pub fn diff_ignore_whitespace_hunk(text: &str, at_line: usize) -> Option<String> {
    let (lines, nl) = to_lines(text);
    if lines.is_empty() {
        return None;
    }
    let at = at_line.min(lines.len() - 1);
    let (hs, he) = hunk_line_bounds(text, at)?;
    if !lines[hs].starts_with("@@") {
        return None;
    }
    let (o_s, _, n_s, _) = parse_hunk_header(&lines[hs]);
    let body = &lines[hs + 1..he];

    // Rewrite the body run by run: a run is `-`* followed by `+`*.
    let squeeze = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).collect() };
    let mut out_body: Vec<String> = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if !body[i].starts_with('-') {
            out_body.push(body[i].clone());
            i += 1;
            continue;
        }
        let del_start = i;
        while i < body.len() && body[i].starts_with('-') {
            i += 1;
        }
        let add_start = i;
        while i < body.len() && body[i].starts_with('+') {
            i += 1;
        }
        let dels = &body[del_start..add_start];
        let adds = &body[add_start..i];
        let paired = dels.len().min(adds.len());
        // Real changes accumulate into a pending -/+ run; an ignored pair flushes
        // it and emits the context line, so the context stays in file order.
        let (mut pend_del, mut pend_add): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        let flush = |out: &mut Vec<String>, d: &mut Vec<String>, a: &mut Vec<String>| {
            out.append(d);
            out.append(a);
        };
        for k in 0..paired {
            let d = dels[k].get(1..).unwrap_or("");
            let a = adds[k].get(1..).unwrap_or("");
            if d != a && squeeze(d) == squeeze(a) {
                flush(&mut out_body, &mut pend_del, &mut pend_add);
                out_body.push(format!(" {d}"));
            } else {
                pend_del.push(dels[k].clone());
                pend_add.push(adds[k].clone());
            }
        }
        // Unpaired lines are real one-sided changes; they keep their side.
        for d in dels.iter().skip(paired) {
            pend_del.push(d.clone());
        }
        for a in adds.iter().skip(paired) {
            pend_add.push(a.clone());
        }
        flush(&mut out_body, &mut pend_del, &mut pend_add);
    }
    if out_body == body {
        return Some(text.to_string());
    }
    let (old, new) = count_old_new(&out_body);
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    out.extend_from_slice(&lines[..hs]);
    out.push(format!("@@ -{o_s},{old} +{n_s},{new} @@"));
    out.extend(out_body);
    out.extend_from_slice(&lines[he..]);
    Some(from_lines(&out, nl))
}

/// Apply a single unified [`Hunk`] to `target` (the pre-image file text),
/// returning the patched text. The hunk's old image (context + removed lines) is
/// located at `old_start` (falling back to a scan of the whole file), then
/// replaced by the new image (context + added lines). `Err` if the context does
/// not match anywhere.
pub fn apply_hunk(target: &str, hunk: &Hunk) -> Result<String, String> {
    let (mut tlines, nl) = to_lines(target);
    let mut old_img: Vec<String> = Vec::new();
    let mut new_img: Vec<String> = Vec::new();
    for dl in &hunk.lines {
        if dl.text.starts_with('\\') {
            continue; // "\ No newline at end of file"
        }
        let content = dl.text.get(1..).unwrap_or("").to_string();
        match dl.kind {
            LineKind::Context => {
                old_img.push(content.clone());
                new_img.push(content);
            }
            LineKind::Removed => old_img.push(content),
            LineKind::Added => new_img.push(content),
            _ => {}
        }
    }

    if old_img.is_empty() {
        let pos = hunk.old_start.min(tlines.len());
        tlines.splice(pos..pos, new_img);
        return Ok(from_lines(&tlines, nl));
    }

    let matches_at = |lines: &[String], p: usize| -> bool {
        p + old_img.len() <= lines.len() && lines[p..p + old_img.len()] == old_img[..]
    };
    let want = hunk.old_start.saturating_sub(1);
    let pos = if matches_at(&tlines, want) {
        Some(want)
    } else {
        (0..=tlines.len().saturating_sub(old_img.len())).find(|&p| matches_at(&tlines, p))
    };
    let pos = pos.ok_or_else(|| "hunk does not apply (context mismatch)".to_string())?;
    tlines.splice(pos..pos + old_img.len(), new_img);
    Ok(from_lines(&tlines, nl))
}

/// Apply every hunk of a [`FileDiff`] to `target`, from the last hunk to the
/// first so earlier hunks' line numbers stay valid. `Err` if any hunk fails.
pub fn apply_file_diff(target: &str, file: &FileDiff) -> Result<String, String> {
    let mut cur = target.to_string();
    for hunk in file.hunks.iter().rev() {
        cur = apply_hunk(&cur, hunk)?;
    }
    Ok(cur)
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

    // A one-file unified diff with two changed lines, used by the transform tests.
    const UNIFIED: &str = "\
--- a/f.txt
+++ b/f.txt
@@ -1,4 +1,4 @@
 line1
-line2
+line2new
 line3
-line4
+line4new
";

    // Its exact `diff -c` equivalent as produced by `diff-unified->context`.
    const CONTEXT: &str = "\
*** a/f.txt
--- b/f.txt
***************
*** 1,4 ****
  line1
- line2
  line3
- line4
--- 1,4 ----
  line1
+ line2new
  line3
+ line4new
";

    #[test]
    fn hunk_kill_removes_only_the_targeted_hunk() {
        // Point inside the SECOND hunk of foo.rs (the "-gone" line).
        let flat_line = TWO_FILE.lines().position(|l| l == "-gone").unwrap();
        let out = diff_hunk_kill(TWO_FILE, flat_line).expect("killed");
        // The second hunk is gone; the first hunk and the README file remain.
        assert!(!out.contains("@@ -10,2 +11,2 @@"));
        assert!(out.contains("@@ -1,3 +1,4 @@"));
        assert!(out.contains("README.md"));
        assert_eq!(hunk_count(&parse(&out)), 2);
    }

    #[test]
    fn hunk_kill_of_lone_hunk_removes_the_file_header() {
        // README.md has a single hunk; killing it removes the whole file section.
        let line = TWO_FILE.lines().position(|l| l == "-# Title").unwrap();
        let out = diff_hunk_kill(TWO_FILE, line).expect("killed");
        assert!(
            !out.contains("README.md"),
            "file header removed with lone hunk"
        );
        assert!(out.contains("src/foo.rs"));
        assert_eq!(parse(&out).files.len(), 1);
    }

    #[test]
    fn file_kill_removes_the_whole_file_section() {
        let line = TWO_FILE.lines().position(|l| l == "-    old();").unwrap();
        let out = diff_file_kill(TWO_FILE, line).expect("killed");
        assert!(!out.contains("src/foo.rs"));
        assert!(out.contains("README.md"));
        let d = parse(&out);
        assert_eq!(d.files.len(), 1);
        assert_eq!(d.files[0].new_path, "README.md");
    }

    #[test]
    fn split_hunk_produces_two_valid_hunks() {
        // Split the first foo.rs hunk at the "+    new();" line.
        let at = TWO_FILE.lines().position(|l| l == "+    new();").unwrap();
        let out = diff_split_hunk(TWO_FILE, at).expect("split");
        assert!(out.contains("@@ -1,2 +1,1 @@"));
        assert!(out.contains("@@ -3,1 +2,3 @@"));
        // Now three hunks in foo.rs plus README's one.
        assert_eq!(hunk_count(&parse(&out)), 4);
        // Reparsing keeps the body intact.
        assert!(out.contains("+    extra();"));
    }

    #[test]
    fn split_hunk_rejects_the_header_line() {
        let hdr = TWO_FILE
            .lines()
            .position(|l| l.starts_with("@@ -1,3"))
            .unwrap();
        assert_eq!(diff_split_hunk(TWO_FILE, hdr), None);
    }

    #[test]
    fn reverse_direction_swaps_headers_and_signs() {
        let out = diff_reverse_direction(UNIFIED);
        let expected = "\
--- b/f.txt
+++ a/f.txt
@@ -1,4 +1,4 @@
 line1
+line2
-line2new
 line3
+line4
-line4new
";
        assert_eq!(out, expected);
    }

    #[test]
    fn reverse_direction_round_trips() {
        assert_eq!(
            diff_reverse_direction(&diff_reverse_direction(TWO_FILE)),
            TWO_FILE
        );
        assert_eq!(
            diff_reverse_direction(&diff_reverse_direction(UNIFIED)),
            UNIFIED
        );
    }

    #[test]
    fn reverse_preserves_omitted_hunk_lengths() {
        let src = "@@ -10 +11 @@\n context\n";
        assert_eq!(diff_reverse_direction(src), "@@ -11 +10 @@\n context\n");
    }

    #[test]
    fn unified_to_context_matches_diff_c() {
        assert_eq!(diff_unified_to_context(UNIFIED), CONTEXT);
    }

    #[test]
    fn context_to_unified_inverts() {
        assert_eq!(diff_context_to_unified(CONTEXT), UNIFIED);
    }

    #[test]
    fn unified_context_round_trips() {
        assert_eq!(
            diff_context_to_unified(&diff_unified_to_context(UNIFIED)),
            UNIFIED
        );
    }

    #[test]
    fn unified_to_context_pure_insertion_omits_old_body() {
        let ins = "\
--- a/f.txt
+++ b/f.txt
@@ -5,0 +6,3 @@
+alpha
+beta
+gamma
";
        let out = diff_unified_to_context(ins);
        let expected = "\
*** a/f.txt
--- b/f.txt
***************
*** 4,0 ****
--- 6,8 ----
+ alpha
+ beta
+ gamma
";
        assert_eq!(out, expected);
        // And it converts straight back.
        assert_eq!(diff_context_to_unified(&out), ins);
    }

    #[test]
    fn context_to_unified_reads_bang_change_lines() {
        // Real `diff -c` output marks a change block with `!` on both sides.
        let ctx = "\
*** a/f.txt
--- b/f.txt
***************
*** 1,3 ****
  keep
! old
  tail
--- 1,3 ----
  keep
! new
  tail
";
        let out = diff_context_to_unified(ctx);
        let expected = "\
--- a/f.txt
+++ b/f.txt
@@ -1,3 +1,3 @@
 keep
-old
+new
 tail
";
        assert_eq!(out, expected);
    }

    #[test]
    fn delete_trailing_whitespace_strips_added_lines_only() {
        let src = "@@ -1,2 +1,2 @@\n context  \n+added   \n-removed\t\n";
        let out = diff_delete_trailing_whitespace(src);
        // The added line loses its trailing spaces; context/removed untouched.
        assert_eq!(out, "@@ -1,2 +1,2 @@\n context  \n+added\n-removed\t\n");
    }

    #[test]
    fn apply_hunk_patches_the_target() {
        let target = "line1\nline2\nline3\nline4\n";
        let d = parse(UNIFIED);
        let hunk = &d.files[0].hunks[0];
        let out = apply_hunk(target, hunk).expect("applies");
        assert_eq!(out, "line1\nline2new\nline3\nline4new\n");
    }

    #[test]
    fn apply_hunk_scans_when_line_number_is_off() {
        // Same change but the file has an extra leading line, so old_start is wrong.
        let target = "header\nline1\nline2\nline3\nline4\n";
        let hunk = &parse(UNIFIED).files[0].hunks[0];
        let out = apply_hunk(target, hunk).expect("applies via scan");
        assert_eq!(out, "header\nline1\nline2new\nline3\nline4new\n");
    }

    #[test]
    fn apply_hunk_rejects_mismatched_context() {
        let target = "totally\ndifferent\ncontent\n";
        let hunk = &parse(UNIFIED).files[0].hunks[0];
        assert!(apply_hunk(target, hunk).is_err());
    }

    #[test]
    fn apply_file_diff_applies_every_hunk() {
        // foo.rs old file reconstructed from the two hunks' old images.
        let target = "fn foo() {\n    old();\n}\n\n\n\n\n\n\ntail\ngone\n";
        let file = &parse(TWO_FILE).files[0];
        let out = apply_file_diff(target, file).expect("applies");
        assert!(out.contains("    new();"));
        assert!(out.contains("    extra();"));
        assert!(out.contains("kept"));
        assert!(!out.contains("gone"));
        assert!(!out.contains("old();"));
    }

    #[test]
    fn restrict_bounds_cover_the_hunk() {
        let (hs, he) = hunk_line_bounds(TWO_FILE, 6).expect("a hunk");
        assert!(TWO_FILE.lines().nth(hs).unwrap().starts_with("@@"));
        assert!(he > hs);
        let (fs, fe) = file_line_bounds(TWO_FILE, 6).expect("a file");
        assert!(TWO_FILE.lines().nth(fs).unwrap().starts_with("diff --git"));
        assert!(fe > fs && fs <= hs);
    }

    /// A hunk whose only change is re-indentation collapses to all-context, and
    /// the `@@` lengths are recomputed to match.
    #[test]
    fn ignore_whitespace_hunk_drops_indent_only_changes() {
        let d = "--- a/x\n+++ b/x\n@@ -1,4 +1,4 @@\n ctx\n-    tabbed()\n+\ttabbed()\n after\n";
        let out = diff_ignore_whitespace_hunk(d, 3).expect("in a hunk");
        assert_eq!(
            out, "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n ctx\n     tabbed()\n after\n",
            "the -/+ pair becomes one context line carrying the old text"
        );
    }

    /// A real change in the same run survives, and a whitespace-only pair before
    /// it becomes a context line *above* the surviving change (file order).
    #[test]
    fn ignore_whitespace_hunk_keeps_real_changes_in_order() {
        let d = concat!(
            "--- a/x\n+++ b/x\n@@ -1,5 +1,5 @@\n",
            " ctx\n",
            "-  foo()\n",
            "-  bar()\n",
            "+foo()\n",
            "+baz()\n",
            " tail\n",
        );
        let out = diff_ignore_whitespace_hunk(d, 4).expect("in a hunk");
        assert_eq!(
            out,
            concat!(
                "--- a/x\n+++ b/x\n@@ -1,4 +1,4 @@\n",
                " ctx\n",
                "   foo()\n",
                "-  bar()\n",
                "+baz()\n",
                " tail\n",
            )
        );
    }

    /// Nothing to ignore: the hunk (and so the whole diff) is returned unchanged.
    #[test]
    fn ignore_whitespace_hunk_is_a_no_op_without_whitespace_changes() {
        let d = "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n ctx\n-old()\n+new()\n";
        assert_eq!(diff_ignore_whitespace_hunk(d, 3).unwrap(), d);
        // Not inside a hunk at all.
        assert_eq!(diff_ignore_whitespace_hunk("no diff here\n", 0), None);
    }

    /// An unpaired deletion (more `-` than `+`) stays a deletion.
    #[test]
    fn ignore_whitespace_hunk_keeps_unpaired_deletions() {
        let d = "--- a/x\n+++ b/x\n@@ -1,3 +1,2 @@\n ctx\n-  a()\n-gone()\n+a()\n";
        let out = diff_ignore_whitespace_hunk(d, 4).expect("in a hunk");
        assert_eq!(
            out,
            "--- a/x\n+++ b/x\n@@ -1,3 +1,2 @@\n ctx\n   a()\n-gone()\n"
        );
    }
}
