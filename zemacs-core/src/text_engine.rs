//! Round-2 pure-Rust editor-engine batch — a second wave of gap-fill algorithms
//! (plus a couple of zemacs originals) pushing the editor toward a strict superset
//! of GNU Emacs, VS Code, Neovim/Vim, Sublime Text, JetBrains, Zed and Helix.
//!
//! Every item here is a plain function (or small value type) over `&str` /
//! `&[String]` with no editor types leaking in, so each is unit-tested in
//! isolation. The command layer extracts the live selection's region or line span,
//! calls one of these, and applies the result as a single undoable transaction.
//! This is deliberately disjoint from [`crate::region_ops`] (round 1) and the
//! tree-sitter-driven modules (`object`, `fold`, `indent`, `match_brackets`,
//! `comment`, `surround`): everything here is language-agnostic and syntax-free.

use std::collections::HashMap;

fn chars_of(s: &str) -> Vec<char> {
    s.chars().collect()
}

// ---------------------------------------------------------------------------
// Alignment — Emacs `align-regexp`, Vim `vim-easy-align`/`Tabular`, Sublime
// "Alignment" package: line up a block of lines on their first separator.
// ---------------------------------------------------------------------------

/// Align a block of lines on the first occurrence of `sep`, padding the left part
/// to a common width so the separators form a column (one space on each side of
/// the separator). Lines without the separator are returned unchanged.
pub fn align_on_separator(lines: &[String], sep: &str) -> Vec<String> {
    if sep.is_empty() {
        return lines.to_vec();
    }
    // Widest left-hand part (in chars) among lines that contain the separator.
    let width = lines
        .iter()
        .filter_map(|l| l.find(sep).map(|i| l[..i].trim_end().chars().count()))
        .max();
    let Some(width) = width else {
        return lines.to_vec();
    };
    lines
        .iter()
        .map(|l| match l.find(sep) {
            Some(i) => {
                let left = l[..i].trim_end();
                let right = l[i + sep.len()..].trim_start();
                let pad = width - left.chars().count();
                // pad left to the common width, then exactly one space before `sep`.
                format!("{left}{}{sep} {right}", " ".repeat(pad + 1))
            }
            None => l.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Hard fill / rewrap — Emacs `fill-paragraph` (M-q), VS Code "Rewrap Comment /
// Text", the Rewrap extension: greedily word-wrap text to a column with a prefix.
// (Distinct from `wrap`/`doc_formatter`, which do *visual* soft-wrap only.)
// ---------------------------------------------------------------------------

/// Greedily word-wrap `text` so that each produced line (including `prefix`) is at
/// most `width` columns wide. Runs of whitespace — including existing newlines —
/// are collapsed to single spaces. Words longer than the budget are still emitted
/// alone on their own line rather than being split.
pub fn fill_paragraph(text: &str, width: usize, prefix: &str) -> String {
    let plen = prefix.chars().count();
    let mut out: Vec<String> = Vec::new();
    let mut line = String::from(prefix);
    let mut line_len = plen;
    let mut empty = true;
    for word in text.split_whitespace() {
        let wlen = word.chars().count();
        let extra = if empty { wlen } else { wlen + 1 };
        if !empty && line_len + extra > width {
            out.push(std::mem::replace(&mut line, String::from(prefix)));
            line_len = plen;
            empty = true;
        }
        if !empty {
            line.push(' ');
            line_len += 1;
        }
        line.push_str(word);
        line_len += wlen;
        empty = false;
    }
    out.push(line);
    out.join("\n")
}

// ---------------------------------------------------------------------------
// Tabs <-> spaces — Emacs `untabify`/`tabify`, VS Code "Convert Indentation to
// Tabs/Spaces", Vim `:retab`.
// ---------------------------------------------------------------------------

/// Expand every tab in `line` to spaces, honoring column stops of width
/// `tab_width` (a tab advances to the next multiple of `tab_width`).
pub fn untabify(line: &str, tab_width: usize) -> String {
    let tw = tab_width.max(1);
    let mut out = String::new();
    let mut col = 0usize;
    for c in line.chars() {
        if c == '\t' {
            let n = tw - (col % tw);
            for _ in 0..n {
                out.push(' ');
            }
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

/// Convert the leading indentation of `line` from spaces to tabs (+ a remainder of
/// spaces), the "Convert Indentation to Tabs" transform. Interior spaces are left
/// untouched, which keeps alignment inside the line intact.
pub fn tabify_indent(line: &str, tab_width: usize) -> String {
    let tw = tab_width.max(1);
    let spaces = line.chars().take_while(|&c| c == ' ').count();
    let rest: String = line.chars().skip(spaces).collect();
    let tabs = spaces / tw;
    let rem = spaces % tw;
    format!("{}{}{}", "\t".repeat(tabs), " ".repeat(rem), rest)
}

// ---------------------------------------------------------------------------
// Transpose words — Emacs `transpose-words` (M-t).
// ---------------------------------------------------------------------------

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn word_spans(chars: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if is_word_char(chars[i]) {
            let start = i;
            while i < chars.len() && is_word_char(chars[i]) {
                i += 1;
            }
            spans.push((start, i));
        } else {
            i += 1;
        }
    }
    spans
}

/// Emacs `transpose-words` (M-t): swap the word at/before `cursor` (a char index)
/// with the following word, preserving the whitespace between them, and return
/// `(new_text, new_cursor)` with the cursor left after the word that moved right.
/// Returns `None` when there are fewer than two words.
pub fn transpose_words(text: &str, cursor: usize) -> Option<(String, usize)> {
    let chars = chars_of(text);
    let spans = word_spans(&chars);
    if spans.len() < 2 {
        return None;
    }
    let mut i = spans.iter().rposition(|s| s.0 <= cursor).unwrap_or(0);
    if i + 1 >= spans.len() {
        i = spans.len() - 2;
    }
    let (a0, a1) = spans[i];
    let (b0, b1) = spans[i + 1];
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    out.extend_from_slice(&chars[..a0]);
    out.extend_from_slice(&chars[b0..b1]); // second word first
    out.extend_from_slice(&chars[a1..b0]); // untouched gap
    out.extend_from_slice(&chars[a0..a1]); // first word last
    out.extend_from_slice(&chars[b1..]);
    let new_cursor = a0 + (b1 - b0) + (b0 - a1) + (a1 - a0);
    Some((out.into_iter().collect(), new_cursor))
}

// ---------------------------------------------------------------------------
// Sort by field / column — Emacs `sort-fields`, Vim `:sort /re/`, `sort -k -t`.
// ---------------------------------------------------------------------------

/// Stable-sort `lines` by their `field`-th `sep`-delimited column (0-based). A
/// line with fewer than `field + 1` columns sorts as the empty string. Complements
/// [`crate::region_ops::sort_lines`], which only sorts by whole-line value.
pub fn sort_by_field(lines: &[String], field: usize, sep: &str) -> Vec<String> {
    let key = |l: &String| -> String {
        if sep.is_empty() {
            return l.clone();
        }
        l.split(sep).nth(field).unwrap_or("").to_string()
    };
    let mut v = lines.to_vec();
    v.sort_by_key(|l| key(l));
    v
}

// ---------------------------------------------------------------------------
// Rectangle operations — CUA-rect / Emacs `rectangle-mark-mode` family
// (`C-x r k`, `C-x r t`, `C-x r o`), Sublime/VS Code column selection edits.
// Columns are char columns; the left column `c1` is inclusive, `c2` exclusive.
// ---------------------------------------------------------------------------

fn split_at_col(chars: &[char], col: usize) -> (String, String) {
    if col >= chars.len() {
        let pad = col - chars.len();
        let mut left: String = chars.iter().collect();
        left.push_str(&" ".repeat(pad));
        (left, String::new())
    } else {
        (
            chars[..col].iter().collect(),
            chars[col..].iter().collect(),
        )
    }
}

/// Emacs `extract-rectangle`: the char columns `[c1, c2)` of every line, padded
/// with spaces where a line is shorter than the rectangle.
pub fn extract_rectangle(lines: &[String], c1: usize, c2: usize) -> Vec<String> {
    let (lo, hi) = (c1.min(c2), c1.max(c2));
    lines
        .iter()
        .map(|l| {
            let chars = chars_of(l);
            let (_, right) = split_at_col(&chars, lo);
            let rc = chars_of(&right);
            let (mid, _) = split_at_col(&rc, hi - lo);
            mid
        })
        .collect()
}

/// Emacs `kill-rectangle` (`C-x r k`): remove columns `[c1, c2)` from every line,
/// returning `(remaining_lines, killed_rectangle)`.
pub fn kill_rectangle(lines: &[String], c1: usize, c2: usize) -> (Vec<String>, Vec<String>) {
    let (lo, hi) = (c1.min(c2), c1.max(c2));
    let killed = extract_rectangle(lines, lo, hi);
    let remaining = lines
        .iter()
        .map(|l| {
            let chars = chars_of(l);
            let left: String = chars.iter().take(lo).collect();
            let right: String = chars.iter().skip(hi).collect();
            format!("{left}{right}")
        })
        .collect();
    (remaining, killed)
}

/// Emacs `string-rectangle` (`C-x r t`): replace columns `[c1, c2)` on every line
/// with `s`, padding short lines out to `c1` with spaces first.
pub fn string_rectangle(lines: &[String], c1: usize, c2: usize, s: &str) -> Vec<String> {
    let (lo, hi) = (c1.min(c2), c1.max(c2));
    lines
        .iter()
        .map(|l| {
            let chars = chars_of(l);
            let (left, _) = split_at_col(&chars, lo);
            let right: String = chars.iter().skip(hi).collect();
            format!("{left}{s}{right}")
        })
        .collect()
}

/// Emacs `open-rectangle` (`C-x r o`): insert `c2 - c1` blank columns at column
/// `c1` on every line, shifting the remainder rightward.
pub fn open_rectangle(lines: &[String], c1: usize, c2: usize) -> Vec<String> {
    let (lo, hi) = (c1.min(c2), c1.max(c2));
    string_rectangle(lines, lo, lo, &" ".repeat(hi - lo))
}

/// Emacs `string-insert-rectangle`: insert `s` at column `col` on every line
/// *without* replacing anything, shifting the remainder rightward. Short lines
/// are padded with spaces out to `col` first. Unlike [`string_rectangle`], which
/// overwrites the `[c1, c2)` span, this inserts into the zero-width span
/// `[col, col)`.
pub fn string_insert_rectangle(lines: &[String], col: usize, s: &str) -> Vec<String> {
    string_rectangle(lines, col, col, s)
}

// ---------------------------------------------------------------------------
// Multiple-cursor / selection algebra — VS Code & Sublime multi-cursor edits,
// Helix selection manipulation. Ranges are half-open `[start, end)` char offsets.
// ---------------------------------------------------------------------------

/// Normalize a set of selections: sort by start and merge every overlapping or
/// touching pair, exactly what an editor does after multi-cursor edits produce
/// adjacent/overlapping regions. Zero-width ranges are preserved as cursors.
pub fn merge_ranges(ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = ranges
        .iter()
        .map(|&(a, b)| (a.min(b), a.max(b)))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (s, e) in v {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Subtract the hole `[h0, h1)` from every range in `ranges`, splitting a range in
/// two when the hole falls in its interior — the "remove a region from the current
/// selection set" primitive. Empty results are dropped.
pub fn subtract_range(ranges: &[(usize, usize)], hole: (usize, usize)) -> Vec<(usize, usize)> {
    let (h0, h1) = (hole.0.min(hole.1), hole.0.max(hole.1));
    let mut out = Vec::new();
    for &(a, b) in ranges {
        let (s, e) = (a.min(b), a.max(b));
        if h1 <= s || h0 >= e || h0 == h1 {
            out.push((s, e)); // no overlap
            continue;
        }
        if s < h0 {
            out.push((s, h0));
        }
        if h1 < e {
            out.push((h1, e));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Indentation-based code folding — VS Code default folding, Vim
// `foldmethod=indent`, Sublime. Computed purely from leading whitespace.
// ---------------------------------------------------------------------------

fn indent_of(line: &str) -> Option<usize> {
    if line.trim().is_empty() {
        None
    } else {
        Some(line.chars().take_while(|c| *c == ' ' || *c == '\t').count())
    }
}

/// Compute fold ranges `(start_line, end_line)` (inclusive, 0-based) from
/// indentation: each line whose block contains at least one more-indented line
/// becomes a foldable header spanning down to the last such line. Blank lines
/// inside a block do not break it; trailing blanks are excluded.
pub fn compute_indent_folds(lines: &[String]) -> Vec<(usize, usize)> {
    let indents: Vec<Option<usize>> = lines.iter().map(|l| indent_of(l)).collect();
    let mut folds = Vec::new();
    for (i, di) in indents.iter().enumerate() {
        let Some(di) = di else { continue };
        let mut end = i;
        for (k, dk) in indents.iter().enumerate().skip(i + 1) {
            match dk {
                None => {}
                Some(d) if *d > *di => end = k,
                Some(_) => break,
            }
        }
        if end > i {
            folds.push((i, end));
        }
    }
    folds
}

// ---------------------------------------------------------------------------
// HTML/XML tag matching — Emacs `sgml-mode` tag match, VS Code / JetBrains
// "matching tag" highlight and jump.
// ---------------------------------------------------------------------------

struct Tag {
    start: usize,
    end: usize, // exclusive, char offsets
    name: String,
    closing: bool,
    self_closing: bool,
}

fn scan_tags(chars: &[char]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let start = i;
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '>' {
                j += 1;
            }
            if j >= chars.len() {
                break;
            }
            let end = j + 1;
            let inner: String = chars[start + 1..j].iter().collect();
            let closing = inner.starts_with('/');
            let self_closing = inner.trim_end().ends_with('/');
            let name: String = inner
                .trim_start_matches('/')
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '/')
                .collect();
            tags.push(Tag {
                start,
                end,
                name,
                closing,
                self_closing,
            });
            i = end;
        } else {
            i += 1;
        }
    }
    tags
}

/// Given a cursor char offset inside an HTML/XML tag, return the char spans of the
/// tag and its matching partner as `((open_start, open_end), (close_start,
/// close_end))`. Works from either the opening or the closing tag; returns `None`
/// for self-closing tags or unbalanced markup.
pub fn match_tag(text: &str, cursor: usize) -> Option<((usize, usize), (usize, usize))> {
    let chars = chars_of(text);
    let tags = scan_tags(&chars);
    let idx = tags
        .iter()
        .position(|t| cursor >= t.start && cursor < t.end)?;
    let here = &tags[idx];
    if here.self_closing {
        return None;
    }
    if !here.closing {
        // walk forward for the matching close of the same name
        let mut depth = 0i32;
        for t in &tags[idx + 1..] {
            if t.self_closing || t.name != here.name {
                continue;
            }
            if t.closing {
                if depth == 0 {
                    return Some(((here.start, here.end), (t.start, t.end)));
                }
                depth -= 1;
            } else {
                depth += 1;
            }
        }
        None
    } else {
        // walk backward for the matching open of the same name
        let mut depth = 0i32;
        for t in tags[..idx].iter().rev() {
            if t.self_closing || t.name != here.name {
                continue;
            }
            if !t.closing {
                if depth == 0 {
                    return Some(((t.start, t.end), (here.start, here.end)));
                }
                depth -= 1;
            } else {
                depth += 1;
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Subword / camelCase navigation — Emacs `subword-mode`, VS Code camelCase
// cursor motion, JetBrains "words in CamelHump".
// ---------------------------------------------------------------------------

/// Char indices at which a new sub-word begins inside an identifier, splitting on
/// `_`/`-`/space separators, lower→upper transitions, letter→digit transitions,
/// and the acronym boundary in runs like `HTMLParser` (`HTML` | `Parser`).
pub fn subword_boundaries(ident: &str) -> Vec<usize> {
    let chars = chars_of(ident);
    let n = chars.len();
    let mut out = Vec::new();
    let is_sep = |c: char| c == '_' || c == '-' || c == ' ';
    for i in 0..n {
        let c = chars[i];
        if is_sep(c) {
            continue;
        }
        let boundary = if i == 0 {
            true
        } else {
            let p = chars[i - 1];
            is_sep(p)
                || (!p.is_uppercase() && c.is_uppercase())
                || (p.is_uppercase() && c.is_uppercase() && i + 1 < n && chars[i + 1].is_lowercase())
                || (!p.is_ascii_digit() && c.is_ascii_digit())
        };
        if boundary {
            out.push(i);
        }
    }
    out
}

/// The next sub-word start strictly after `pos` in arbitrary text (crossing
/// separators and punctuation), or the text length when none remains — the target
/// of a camelCase-aware "forward word" motion.
pub fn next_subword_start(text: &str, pos: usize) -> usize {
    let chars = chars_of(text);
    let n = chars.len();
    let is_word = |c: char| c.is_alphanumeric();
    for i in (pos + 1)..n {
        let c = chars[i];
        if !is_word(c) {
            continue;
        }
        let p = chars[i - 1];
        let boundary = !is_word(p)
            || (!p.is_uppercase() && c.is_uppercase())
            || (p.is_uppercase() && c.is_uppercase() && i + 1 < n && chars[i + 1].is_lowercase())
            || (!p.is_ascii_digit() && c.is_ascii_digit());
        if boundary {
            return i;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// Incremental search — Emacs `isearch` (C-s/C-r), and the match-cycling every
// editor's find bar exposes, with wrap-around.
// ---------------------------------------------------------------------------

/// Char offsets of every non-overlapping occurrence of `needle` in `haystack`
/// (ASCII-case-insensitive when `ignore_case`, preserving char indices). Empty
/// when `needle` is empty.
pub fn search_all(haystack: &str, needle: &str, ignore_case: bool) -> Vec<usize> {
    let hs = chars_of(haystack);
    let nd = chars_of(needle);
    if nd.is_empty() || nd.len() > hs.len() {
        return Vec::new();
    }
    let eq = |a: char, b: char| {
        if ignore_case {
            a.eq_ignore_ascii_case(&b)
        } else {
            a == b
        }
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i + nd.len() <= hs.len() {
        if (0..nd.len()).all(|k| eq(hs[i + k], nd[k])) {
            out.push(i);
            i += nd.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Emacs `isearch` match cursor: holds the ordered match offsets and cycles
/// forward/backward through them with wrap-around.
#[derive(Clone, Debug)]
pub struct IncrementalSearch {
    matches: Vec<usize>,
    pos: usize, // index into `matches`; == len() means "before first"
}

impl IncrementalSearch {
    pub fn new(haystack: &str, needle: &str, ignore_case: bool) -> Self {
        let matches = search_all(haystack, needle, ignore_case);
        let pos = matches.len();
        Self { matches, pos }
    }

    pub fn matches(&self) -> &[usize] {
        &self.matches
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// Advance to the next match (C-s), wrapping past the end back to the first.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<usize> {
        let len = self.matches.len();
        if len == 0 {
            return None;
        }
        self.pos = if self.pos + 1 >= len { 0 } else { self.pos + 1 };
        Some(self.matches[self.pos])
    }

    /// Retreat to the previous match (C-r), wrapping past the start to the last.
    pub fn prev(&mut self) -> Option<usize> {
        let len = self.matches.len();
        if len == 0 {
            return None;
        }
        self.pos = if self.pos == 0 || self.pos >= len {
            len - 1
        } else {
            self.pos - 1
        };
        Some(self.matches[self.pos])
    }
}

// ---------------------------------------------------------------------------
// Undo tree — `undo-tree.el` and Vim's persistent branching undo. Records a tree
// of states where undo/redo navigate parent/child links, so an edit after an undo
// creates a *branch* instead of clobbering the redo history.
// ---------------------------------------------------------------------------

struct UndoNode<T> {
    state: T,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// A branching undo history (`undo-tree`). Each recorded state is a node; `undo`
/// walks to the parent, `redo` walks to the most-recently-created child branch.
pub struct UndoTree<T> {
    nodes: Vec<UndoNode<T>>,
    current: usize,
}

impl<T> UndoTree<T> {
    pub fn new(root: T) -> Self {
        Self {
            nodes: vec![UndoNode {
                state: root,
                parent: None,
                children: Vec::new(),
            }],
            current: 0,
        }
    }

    /// Record a new state as a child of the current node and make it current.
    /// If invoked after an `undo`, this starts a new branch rather than discarding
    /// the existing redo path.
    pub fn record(&mut self, state: T) {
        let id = self.nodes.len();
        self.nodes.push(UndoNode {
            state,
            parent: Some(self.current),
            children: Vec::new(),
        });
        self.nodes[self.current].children.push(id);
        self.current = id;
    }

    pub fn current_state(&self) -> &T {
        &self.nodes[self.current].state
    }

    /// Move to the parent state, or `None` at the root.
    pub fn undo(&mut self) -> Option<&T> {
        let parent = self.nodes[self.current].parent?;
        self.current = parent;
        Some(&self.nodes[self.current].state)
    }

    /// Move to the newest child branch, or `None` at a leaf.
    pub fn redo(&mut self) -> Option<&T> {
        let child = *self.nodes[self.current].children.last()?;
        self.current = child;
        Some(&self.nodes[self.current].state)
    }

    /// Number of forward branches available from the current node.
    pub fn branch_count(&self) -> usize {
        self.nodes[self.current].children.len()
    }
}

// ---------------------------------------------------------------------------
// Common-indent strip (dedent) — Python `textwrap.dedent`; the "remove shared
// leading indentation" transform editors lack as a single command.
// ---------------------------------------------------------------------------

/// Remove the longest common leading-whitespace prefix shared by every non-blank
/// line in the block (blank lines are ignored when measuring, and cleared of any
/// residual whitespace so a dedent leaves them truly empty).
pub fn strip_common_indent(lines: &[String]) -> Vec<String> {
    let common = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').count())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                l.chars().skip(common).collect()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Balance check — Emacs `check-parens`.
// ---------------------------------------------------------------------------

fn bracket_close(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

/// Emacs `check-parens`: return the char index of the first delimiter that breaks
/// bracket balance — an unmatched or mismatched closer, or (at end of input) the
/// innermost still-open opener. `None` when `(`/`[`/`{` are perfectly balanced.
/// String and comment contexts are not tracked (a plain structural scan).
pub fn first_unbalanced(text: &str) -> Option<usize> {
    let mut stack: Vec<(char, usize)> = Vec::new();
    for (i, c) in text.chars().enumerate() {
        if bracket_close(c).is_some() {
            stack.push((c, i));
        } else if matches!(c, ')' | ']' | '}') {
            match stack.pop() {
                Some((open, _)) if bracket_close(open) == Some(c) => {}
                _ => return Some(i),
            }
        }
    }
    stack.last().map(|&(_, i)| i)
}

// ---------------------------------------------------------------------------
// ⭐ zemacs original — beyond GNU Emacs, VS Code, Vim, Sublime, JetBrains, Zed
// and Helix: cycle an identifier through naming conventions with one keystroke.
// (Editors offer discrete "to snake_case" commands; none *cycle* through the
// family, so you can hammer one key until the identifier reads how you want.)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaseStyle {
    Snake,
    Kebab,
    Camel,
    Pascal,
    Screaming,
}

fn detect_case(ident: &str) -> CaseStyle {
    if ident.contains('-') {
        CaseStyle::Kebab
    } else if ident.contains('_') {
        let letters: Vec<char> = ident.chars().filter(|c| c.is_alphabetic()).collect();
        if !letters.is_empty() && letters.iter().all(|c| c.is_uppercase()) {
            CaseStyle::Screaming
        } else {
            CaseStyle::Snake
        }
    } else if ident.chars().next().is_some_and(|c| c.is_uppercase()) {
        CaseStyle::Pascal
    } else if ident.chars().any(|c| c.is_uppercase()) {
        CaseStyle::Camel
    } else {
        CaseStyle::Snake
    }
}

/// Split an identifier into its lower-cased component words, understanding
/// separators and camel/acronym boundaries via [`subword_boundaries`].
fn split_words(ident: &str) -> Vec<String> {
    let chars = chars_of(ident);
    let bounds = subword_boundaries(ident);
    let mut words = Vec::new();
    for (k, &start) in bounds.iter().enumerate() {
        let end = bounds.get(k + 1).copied().unwrap_or(chars.len());
        let word: String = chars[start..end]
            .iter()
            .filter(|c| **c != '_' && **c != '-' && **c != ' ')
            .flat_map(|c| c.to_lowercase())
            .collect();
        if !word.is_empty() {
            words.push(word);
        }
    }
    words
}

fn capitalize(w: &str) -> String {
    let mut cs = w.chars();
    match cs.next() {
        Some(first) => first.to_uppercase().collect::<String>() + cs.as_str(),
        None => String::new(),
    }
}

fn render_case(words: &[String], style: CaseStyle) -> String {
    match style {
        CaseStyle::Snake => words.join("_"),
        CaseStyle::Kebab => words.join("-"),
        CaseStyle::Screaming => words
            .iter()
            .map(|w| w.to_uppercase())
            .collect::<Vec<_>>()
            .join("_"),
        CaseStyle::Camel => words
            .iter()
            .enumerate()
            .map(|(i, w)| if i == 0 { w.clone() } else { capitalize(w) })
            .collect(),
        CaseStyle::Pascal => words.iter().map(|w| capitalize(w)).collect(),
    }
}

/// ⭐ zemacs original: cycle an identifier's naming convention
/// `snake_case → kebab-case → camelCase → PascalCase → SCREAMING_SNAKE → …`,
/// preserving its word decomposition. Idempotent over five presses.
pub fn cycle_identifier_case(ident: &str) -> String {
    let words = split_words(ident);
    if words.is_empty() {
        return ident.to_string();
    }
    let next = match detect_case(ident) {
        CaseStyle::Snake => CaseStyle::Kebab,
        CaseStyle::Kebab => CaseStyle::Camel,
        CaseStyle::Camel => CaseStyle::Pascal,
        CaseStyle::Pascal => CaseStyle::Screaming,
        CaseStyle::Screaming => CaseStyle::Snake,
    };
    render_case(&words, next)
}

// ---------------------------------------------------------------------------
// ⭐ zemacs original — beyond all listed editors: sum a numeric column across a
// block of lines (a spreadsheet-style total). Emacs/VS Code/etc. have no built-in
// "sum the selected column" command; here it is as a first-class engine op.
// ---------------------------------------------------------------------------

/// ⭐ zemacs original: sum the `field`-th `sep`-delimited column over `lines`,
/// treating non-numeric / missing cells as zero, and return `(total, counted)`
/// where `counted` is the number of lines that contributed a real number. Handy
/// for "select a column of numbers and see the total" without leaving the editor.
pub fn sum_column(lines: &[String], field: usize, sep: &str) -> (f64, usize) {
    let mut total = 0.0;
    let mut counted = 0usize;
    for l in lines {
        let cell = if sep.is_empty() {
            l.trim()
        } else {
            l.split(sep).nth(field).unwrap_or("").trim()
        };
        if let Ok(n) = cell.parse::<f64>() {
            total += n;
            counted += 1;
        }
    }
    (total, counted)
}

// ---------------------------------------------------------------------------
// Registers for rectangles / named text is already covered by region_ops; keep
// a small typed helper for column-clip metadata so command wiring stays honest.
// ---------------------------------------------------------------------------

/// Rectangular clipboard payload: the killed columns plus the originating column
/// span, so a later yank can reinsert the block at the same width.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RectClip {
    pub lines: Vec<String>,
    pub width: usize,
}

impl RectClip {
    pub fn from_rectangle(lines: Vec<String>) -> Self {
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        Self { lines, width }
    }
}

/// Named store for [`RectClip`]s (rectangle registers, Emacs `C-x r r`).
#[derive(Clone, Debug, Default)]
pub struct RectRegisters {
    map: HashMap<char, RectClip>,
}

impl RectRegisters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: char, clip: RectClip) {
        self.map.insert(name, clip);
    }

    pub fn get(&self, name: char) -> Option<&RectClip> {
        self.map.get(&name)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn v(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn align_on_separator_pads_left() {
        assert_eq!(
            align_on_separator(&v(&["a = 1", "bb = 2", "no sep here"]), "="),
            v(&["a  = 1", "bb = 2", "no sep here"])
        );
    }

    #[test]
    fn fill_paragraph_wraps_greedily() {
        assert_eq!(
            fill_paragraph("the quick brown fox", 9, ""),
            "the quick\nbrown fox"
        );
        // collapses existing newlines and applies the prefix
        assert_eq!(
            fill_paragraph("aa\n  bb   cc", 8, "// "),
            "// aa bb\n// cc"
        );
        // over-long word still emitted alone
        assert_eq!(fill_paragraph("supercalifragilistic hi", 5, ""), "supercalifragilistic\nhi");
    }

    #[test]
    fn untabify_and_tabify_indent() {
        assert_eq!(untabify("\tx", 4), "    x");
        assert_eq!(untabify("a\tb", 4), "a   b"); // tab to next stop from col 1
        assert_eq!(tabify_indent("        x", 4), "\t\tx");
        assert_eq!(tabify_indent("     x", 4), "\t x"); // 5 spaces = 1 tab + 1 space
    }

    #[test]
    fn transpose_words_swaps_around_cursor() {
        assert_eq!(
            transpose_words("foo bar", 1),
            Some(("bar foo".to_string(), 7))
        );
        assert_eq!(
            transpose_words("a, b", 0).map(|(s, _)| s),
            Some("b, a".to_string())
        );
        assert_eq!(transpose_words("solo", 1), None);
    }

    #[test]
    fn sort_by_field_uses_column_key() {
        assert_eq!(
            sort_by_field(&v(&["x 3", "y 1", "z 2"]), 1, " "),
            v(&["y 1", "z 2", "x 3"])
        );
        assert_eq!(
            sort_by_field(&v(&["b:2", "a:1"]), 0, ":"),
            v(&["a:1", "b:2"])
        );
    }

    #[test]
    fn rectangle_extract_kill_string_open() {
        let lines = v(&["abcdef", "ABCDEF", "xy"]);
        assert_eq!(extract_rectangle(&lines, 2, 4), v(&["cd", "CD", "  "]));
        let (rem, killed) = kill_rectangle(&lines, 2, 4);
        assert_eq!(rem, v(&["abef", "ABEF", "xy"]));
        assert_eq!(killed, v(&["cd", "CD", "  "]));
        assert_eq!(
            string_rectangle(&lines, 2, 4, "**"),
            v(&["ab**ef", "AB**EF", "xy**"])
        );
        assert_eq!(
            open_rectangle(&lines, 2, 4),
            v(&["ab  cdef", "AB  CDEF", "xy  "])
        );
    }

    #[test]
    fn string_insert_rectangle_shifts_without_overwriting() {
        let lines = v(&["abcdef", "ABCDEF", "x"]);
        // insert ">>" at column 2 on each line; the short "x" is padded to col 2
        assert_eq!(
            string_insert_rectangle(&lines, 2, ">>"),
            v(&["ab>>cdef", "AB>>CDEF", "x >>"])
        );
        // inserting the empty string is a no-op that still pads short lines out
        assert_eq!(
            string_insert_rectangle(&lines, 0, ""),
            v(&["abcdef", "ABCDEF", "x"])
        );
    }

    #[test]
    fn merge_and_subtract_ranges() {
        assert_eq!(
            merge_ranges(&[(5, 8), (0, 3), (2, 6)]),
            vec![(0, 8)]
        );
        assert_eq!(
            merge_ranges(&[(0, 2), (2, 4), (10, 12)]),
            vec![(0, 4), (10, 12)]
        );
        // hole splits a range in two
        assert_eq!(subtract_range(&[(0, 10)], (3, 6)), vec![(0, 3), (6, 10)]);
        assert_eq!(subtract_range(&[(0, 4)], (10, 12)), vec![(0, 4)]);
    }

    #[test]
    fn indent_folds_from_whitespace() {
        let lines = v(&["def f():", "    a = 1", "    b = 2", "c = 3"]);
        assert_eq!(compute_indent_folds(&lines), vec![(0, 2)]);
        let nested = v(&["a", "  b", "    c", "  d", "e"]);
        assert_eq!(compute_indent_folds(&nested), vec![(0, 3), (1, 2)]);
    }

    #[test]
    fn match_tag_pairs() {
        let s = "<a><b>x</b></a>";
        assert_eq!(match_tag(s, 0), Some(((0, 3), (11, 15))));
        assert_eq!(match_tag(s, 3), Some(((3, 6), (7, 11))));
        // from the closing side
        assert_eq!(match_tag(s, 12), Some(((0, 3), (11, 15))));
        assert_eq!(match_tag("<br/>", 1), None); // self-closing
    }

    #[test]
    fn subword_boundaries_and_next() {
        assert_eq!(subword_boundaries("fooBarBaz"), vec![0, 3, 6]);
        assert_eq!(subword_boundaries("HTMLParser"), vec![0, 4]);
        assert_eq!(subword_boundaries("foo_bar"), vec![0, 4]);
        assert_eq!(next_subword_start("fooBar baz", 0), 3);
        assert_eq!(next_subword_start("fooBar baz", 3), 7);
        assert_eq!(next_subword_start("end", 0), 3); // no more -> len
    }

    #[test]
    fn search_all_and_incremental_cycle() {
        assert_eq!(search_all("abababab", "ab", false), vec![0, 2, 4, 6]);
        assert_eq!(search_all("AbaBA", "ba", true), vec![1, 3]);
        let mut s = IncrementalSearch::new("a.a.a", "a", false);
        assert_eq!(s.matches(), &[0, 2, 4]);
        assert_eq!(s.next(), Some(0));
        assert_eq!(s.next(), Some(2));
        assert_eq!(s.next(), Some(4));
        assert_eq!(s.next(), Some(0)); // wrap
        assert_eq!(s.prev(), Some(4)); // wrap backward
    }

    #[test]
    fn undo_tree_branches() {
        let mut t = UndoTree::new("root");
        t.record("a");
        t.record("b");
        assert_eq!(*t.current_state(), "b");
        assert_eq!(t.undo(), Some(&"a"));
        assert_eq!(t.undo(), Some(&"root"));
        assert_eq!(t.undo(), None); // at root
        // create a second branch off the root
        t.record("c");
        assert_eq!(*t.current_state(), "c");
        assert_eq!(t.undo(), Some(&"root"));
        assert_eq!(t.branch_count(), 2); // "a" branch + "c" branch
        assert_eq!(t.redo(), Some(&"c")); // newest child
    }

    #[test]
    fn strip_common_indent_dedents() {
        assert_eq!(
            strip_common_indent(&v(&["    a", "      b", "    c"])),
            v(&["a", "  b", "c"])
        );
        // blank line ignored when measuring and cleared in output
        assert_eq!(
            strip_common_indent(&v(&["  a", "   ", "  b"])),
            v(&["a", "", "b"])
        );
    }

    #[test]
    fn first_unbalanced_reports_offender() {
        assert_eq!(first_unbalanced("(a[b]{c})"), None);
        assert_eq!(first_unbalanced("(a]"), Some(2)); // mismatched close
        assert_eq!(first_unbalanced(")"), Some(0)); // stray close
        assert_eq!(first_unbalanced("(("), Some(1)); // innermost unclosed
    }

    #[test]
    fn cycle_identifier_case_full_loop() {
        assert_eq!(cycle_identifier_case("foo_bar"), "foo-bar");
        assert_eq!(cycle_identifier_case("foo-bar"), "fooBar");
        assert_eq!(cycle_identifier_case("fooBar"), "FooBar");
        assert_eq!(cycle_identifier_case("FooBar"), "FOO_BAR");
        assert_eq!(cycle_identifier_case("FOO_BAR"), "foo_bar");
        // acronym-aware decomposition survives the round trip
        assert_eq!(cycle_identifier_case("HTMLParser"), "HTML_PARSER");
    }

    #[test]
    fn sum_column_totals_numeric_cells() {
        let lines = v(&["item 10", "item 20", "header n/a", "item 5"]);
        assert_eq!(sum_column(&lines, 1, " "), (35.0, 3));
        assert_eq!(sum_column(&v(&["1.5", "2.5", "x"]), 0, ""), (4.0, 2));
    }

    #[test]
    fn rect_clip_and_registers() {
        let clip = RectClip::from_rectangle(v(&["cd", "C", "eee"]));
        assert_eq!(clip.width, 3);
        let mut regs = RectRegisters::new();
        regs.set('a', clip.clone());
        assert_eq!(regs.get('a'), Some(&clip));
        assert_eq!(regs.get('z'), None);
    }
}
