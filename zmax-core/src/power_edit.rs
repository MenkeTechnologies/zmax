//! Round-3 pure-Rust editor-engine batch — a third wave of gap-fill algorithms
//! (plus two zmax originals) pushing the editor toward a strict superset of GNU
//! Emacs, VS Code, Neovim/Vim, Sublime Text, JetBrains, Zed and Helix.
//!
//! Every item here is a plain function (or small value type) over `&str` /
//! `&[String]` with no editor types leaking in, so each is unit-tested in
//! isolation. The command layer extracts the live selection's region or line
//! span, calls one of these, and applies the result as a single undoable
//! transaction. This module is deliberately disjoint from [`crate::region_ops`]
//! (round 1) and [`crate::text_engine`] (round 2): where those cover joins,
//! sorting, rectangles, alignment on a fixed separator, incremental search and
//! sub-word motion, this round covers soft-wrap/visual-line motion,
//! expand-region smart selection, multiple-cursors add-next-match, Vim `g
//! CTRL-A` number sequences, uniq variants, markdown-table/auto-table alignment,
//! comment boxes, indent guides, whitespace normalisation, Emmet tag wrapping,
//! kmacro counters and query-replace planning.
//!
//! Column arithmetic here treats each `char` as one display column (monospace
//! ASCII assumption); the tree-sitter/grapheme modules handle true display
//! width where it matters.

use std::collections::HashSet;

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// ===========================================================================
// 1. Word-wrap / visual-line motion
//    Emacs `visual-line-mode` + `next-line`/`previous-line`, VS Code "Word Wrap"
//    (`editor.wordWrap`), Vim `gj`/`gk`, Sublime "Word Wrap".
// ===========================================================================

/// Break `line` into visual sub-rows no wider than `width` columns, breaking at
/// the last whitespace that fits (word wrap); a word longer than `width` is
/// hard-broken. Returns the char offset within `line` at which each visual row
/// begins (the first is always `0`). `width == 0` disables wrapping.
pub fn soft_wrap_offsets(line: &str, width: usize) -> Vec<usize> {
    let mut offsets = vec![0usize];
    if width == 0 {
        return offsets;
    }
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= width {
        return offsets;
    }
    let mut line_start = 0usize;
    let mut last_break: Option<usize> = None; // char index just after a run of ws
    let mut i = 0usize;
    while i < chars.len() {
        if i - line_start >= width {
            let brk = match last_break {
                Some(b) if b > line_start && b <= i => b,
                _ => i, // no usable break point -> hard break
            };
            offsets.push(brk);
            line_start = brk;
            last_break = None;
            i = brk;
            continue;
        }
        if chars[i] == ' ' || chars[i] == '\t' {
            last_break = Some(i + 1);
        }
        i += 1;
    }
    offsets
}

/// Map an absolute char position within a line to its `(row, col)` on the given
/// visual-row start offsets (as produced by [`soft_wrap_offsets`]).
pub fn visual_row_col(offsets: &[usize], char_pos: usize) -> (usize, usize) {
    let mut row = 0;
    for (r, &start) in offsets.iter().enumerate() {
        if start <= char_pos {
            row = r;
        } else {
            break;
        }
    }
    (row, char_pos - offsets[row])
}

/// Move the cursor one visual row down, preserving the goal column (Vim `gj`,
/// Emacs visual-line `next-line`). `total` is the line's char length. Returns the
/// new absolute char position, clamped to the target row's width / line end.
pub fn visual_move_down(offsets: &[usize], total: usize, char_pos: usize) -> usize {
    let (row, col) = visual_row_col(offsets, char_pos);
    if row + 1 >= offsets.len() {
        return char_pos; // already on the last visual row
    }
    let next_start = offsets[row + 1];
    let next_end = offsets.get(row + 2).copied().unwrap_or(total);
    (next_start + col).min(next_end)
}

/// Move the cursor one visual row up, preserving the goal column (Vim `gk`).
pub fn visual_move_up(offsets: &[usize], char_pos: usize) -> usize {
    let (row, col) = visual_row_col(offsets, char_pos);
    if row == 0 {
        return char_pos;
    }
    let prev_start = offsets[row - 1];
    let prev_end = offsets[row]; // exclusive: start of current row
    (prev_start + col).min(prev_end.saturating_sub(1).max(prev_start))
}

// ===========================================================================
// 2. Expand-region / smart-select
//    `expand-region.el`, JetBrains "Extend Selection" (Ctrl-W), VS Code
//    "Expand Selection" (Shift-Alt-Right), Vim `viw`/`vi(` ladder.
// ===========================================================================

/// Given `text` and the current selection `(start, end)` as char offsets, return
/// the smallest semantically meaningful region that *strictly contains* the
/// current selection: word -> enclosing bracket/quote pair -> line -> paragraph
/// -> whole buffer. Returns `None` when the selection already spans everything.
pub fn expand_region(text: &str, sel: (usize, usize)) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let (s, e) = (sel.0.min(n), sel.1.min(n));
    let mut candidates: Vec<(usize, usize)> = Vec::new();

    // Word around the caret / selection start.
    if let Some(w) = word_bounds(&chars, s, e) {
        candidates.push(w);
    }
    // Innermost enclosing bracket / quote pair (inside, then including delimiters).
    if let Some((inner, outer)) = enclosing_pair(&chars, s, e) {
        candidates.push(inner);
        candidates.push(outer);
    }
    // Current line (excluding the trailing newline).
    candidates.push(line_bounds(&chars, s, e));
    // Current paragraph (blank-line delimited).
    candidates.push(paragraph_bounds(&chars, s, e));
    // Whole buffer.
    candidates.push((0, n));

    // Pick the smallest candidate that strictly contains the current selection.
    candidates
        .into_iter()
        .filter(|&(cs, ce)| cs <= s && ce >= e && (cs < s || ce > e))
        .min_by_key(|&(cs, ce)| ce - cs)
}

fn word_bounds(chars: &[char], s: usize, e: usize) -> Option<(usize, usize)> {
    let mut start = s;
    let mut end = e.max(s);
    // If caret sits just past a word, or between words, anchor onto a word char.
    if start >= chars.len() || !is_word_char(chars[start]) {
        if start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
            end = start;
        } else {
            return None;
        }
    }
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    if start < s || end > e {
        Some((start, end))
    } else {
        None
    }
}

fn enclosing_pair(chars: &[char], s: usize, e: usize) -> Option<((usize, usize), (usize, usize))> {
    const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
    // Scan left for an unmatched opener before `s`.
    for &(open, close) in &PAIRS {
        let mut depth = 0i32;
        let mut i = s;
        let mut open_at = None;
        while i > 0 {
            i -= 1;
            if chars[i] == close {
                depth += 1;
            } else if chars[i] == open {
                if depth == 0 {
                    open_at = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
        let Some(op) = open_at else { continue };
        // Find the matching close at or after `e`.
        let mut depth = 0i32;
        let mut j = op + 1;
        let mut close_at = None;
        while j < chars.len() {
            if chars[j] == open {
                depth += 1;
            } else if chars[j] == close {
                if depth == 0 {
                    close_at = Some(j);
                    break;
                }
                depth -= 1;
            }
            j += 1;
        }
        if let Some(cl) = close_at {
            if cl + 1 >= e {
                return Some(((op + 1, cl), (op, cl + 1)));
            }
        }
    }
    None
}

fn line_bounds(chars: &[char], s: usize, e: usize) -> (usize, usize) {
    let mut start = s.min(chars.len());
    while start > 0 && chars[start - 1] != '\n' {
        start -= 1;
    }
    let mut end = e.min(chars.len());
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    (start, end)
}

fn paragraph_bounds(chars: &[char], s: usize, e: usize) -> (usize, usize) {
    let is_blank_line_start = |idx: usize| -> bool {
        // True if the line beginning at `idx` is empty (idx == '\n' or eof).
        idx >= chars.len() || chars[idx] == '\n'
    };
    // Walk up to the start of a line, then past preceding non-blank lines.
    let mut start = s.min(chars.len());
    while start > 0 && chars[start - 1] != '\n' {
        start -= 1;
    }
    loop {
        if start == 0 {
            break;
        }
        // start-1 is a '\n'; find the start of the previous line.
        let mut prev = start - 1;
        while prev > 0 && chars[prev - 1] != '\n' {
            prev -= 1;
        }
        if is_blank_line_start(prev) {
            break;
        }
        start = prev;
    }
    let mut end = e.min(chars.len());
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    loop {
        if end >= chars.len() {
            break;
        }
        // end is a '\n'; peek the following line.
        let next = end + 1;
        if is_blank_line_start(next) {
            break;
        }
        let mut ln_end = next;
        while ln_end < chars.len() && chars[ln_end] != '\n' {
            ln_end += 1;
        }
        end = ln_end;
    }
    (start, end)
}

// ===========================================================================
// 3. Multiple cursors — add next match
//    VS Code "Add Next Occurrence" (Ctrl-D), Sublime Ctrl-D, Vim visual-multi.
// ===========================================================================

/// A set of cursors selecting identical text, grown by matching the primary
/// selection's text against the buffer. Offsets are byte offsets into the
/// haystack (matching [`str::find`]); ranges are kept sorted and disjoint.
#[derive(Debug, Clone)]
pub struct MultiCursor {
    needle: String,
    cursors: Vec<(usize, usize)>,
}

impl MultiCursor {
    /// Start from the primary selection `(start, end)` whose text is `needle`.
    pub fn new(start: usize, end: usize, needle: impl Into<String>) -> Self {
        MultiCursor {
            needle: needle.into(),
            cursors: vec![(start, end)],
        }
    }

    /// Current cursors, sorted by start offset.
    pub fn cursors(&self) -> &[(usize, usize)] {
        &self.cursors
    }

    /// The matched text every cursor selects.
    pub fn needle(&self) -> &str {
        &self.needle
    }

    /// Add the next occurrence of the needle after the last cursor, wrapping to
    /// the start of the buffer. Returns the added range, or `None` when the
    /// needle is empty or every occurrence is already selected.
    pub fn add_next_match(&mut self, haystack: &str) -> Option<(usize, usize)> {
        if self.needle.is_empty() {
            return None;
        }
        let existing: HashSet<usize> = self.cursors.iter().map(|&(s, _)| s).collect();
        let last_end = self.cursors.iter().map(|&(_, e)| e).max().unwrap_or(0);
        // Search from last_end to end, then wrap from 0.
        let found = find_from(haystack, &self.needle, last_end, &existing)
            .or_else(|| find_from(haystack, &self.needle, 0, &existing));
        if let Some(start) = found {
            let range = (start, start + self.needle.len());
            self.cursors.push(range);
            self.cursors.sort_unstable();
            Some(range)
        } else {
            None
        }
    }

    /// Select *every* occurrence of the needle at once (Ctrl-Shift-L / Alt-Enter).
    pub fn add_all_matches(&mut self, haystack: &str) -> usize {
        if self.needle.is_empty() {
            return 0;
        }
        let mut seen: HashSet<usize> = self.cursors.iter().map(|&(s, _)| s).collect();
        let mut added = 0;
        let mut from = 0;
        while let Some(rel) = haystack[from..].find(&self.needle) {
            let start = from + rel;
            if seen.insert(start) {
                self.cursors.push((start, start + self.needle.len()));
                added += 1;
            }
            from = start + self.needle.len().max(1);
            if from > haystack.len() {
                break;
            }
        }
        self.cursors.sort_unstable();
        added
    }
}

fn find_from(haystack: &str, needle: &str, from: usize, skip: &HashSet<usize>) -> Option<usize> {
    let mut i = from.min(haystack.len());
    while let Some(rel) = haystack.get(i..)?.find(needle) {
        let start = i + rel;
        if !skip.contains(&start) {
            return Some(start);
        }
        i = start + needle.len().max(1);
    }
    None
}

// ===========================================================================
// 4. Join with an arbitrary separator
//    JetBrains "Join Lines", VS Code "Join Lines", Emacs `join-line` — but with a
//    caller-chosen separator (region_ops::join_lines only offers space/none).
// ===========================================================================

/// Join `lines` with `sep`; when `trim` is set, each line's surrounding
/// whitespace is stripped first so runs of indentation collapse cleanly.
pub fn join_with_separator(lines: &[String], sep: &str, trim: bool) -> String {
    let parts: Vec<&str> = lines
        .iter()
        .map(|l| if trim { l.trim() } else { l.as_str() })
        .collect();
    parts.join(sep)
}

// ===========================================================================
// 5. Number sequences — Vim `g CTRL-A`
//    Turn a column of identical/arbitrary numbers into an incrementing sequence.
// ===========================================================================

/// For each line containing an integer, increment its first integer by
/// `step * k`, where `k` is the 1-based index among lines that contain a number
/// (Vim visual-block `g CTRL-A`). Lines without a number pass through unchanged.
pub fn sequence_increment(lines: &[String], step: i64) -> Vec<String> {
    let mut k: i64 = 0;
    lines
        .iter()
        .map(|line| match first_int_span(line) {
            Some((a, b)) => {
                k += 1;
                let val: i64 = line[a..b].parse().unwrap_or(0);
                let new = val + step * k;
                format!("{}{}{}", &line[..a], new, &line[b..])
            }
            None => line.clone(),
        })
        .collect()
}

/// Byte span of the first (optionally signed) integer in `s`.
fn first_int_span(s: &str) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let mut start = i;
            // Absorb a leading sign directly attached to the digit run.
            if start > 0 && (bytes[start - 1] == b'-' || bytes[start - 1] == b'+') {
                // Only treat as sign when it is not part of a preceding number/word.
                if start == 1 || !bytes[start - 2].is_ascii_alphanumeric() {
                    start -= 1;
                }
            }
            let mut end = i;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            return Some((start, end));
        }
        i += 1;
    }
    None
}

// ===========================================================================
// 6. Uniq variants
//    Emacs `delete-duplicate-lines`, coreutils `uniq`/`uniq -c`.
// ===========================================================================

/// Remove *all* duplicate lines, keeping the first occurrence and preserving
/// order (Emacs `delete-duplicate-lines`). Distinct from `region_ops::uniq_adjacent`
/// which only collapses runs.
pub fn uniq_all(lines: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    lines
        .iter()
        .filter(|l| seen.insert((*l).clone()))
        .cloned()
        .collect()
}

/// Collapse each run of adjacent identical lines to a single line prefixed with
/// its repeat count, right-aligned in a 4-column field (coreutils `uniq -c`).
pub fn uniq_count(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() && lines[j] == lines[i] {
            j += 1;
        }
        out.push(format!("{:>4} {}", j - i, lines[i]));
        i = j;
    }
    out
}

// ===========================================================================
// 7. Markdown table formatting from rows
//    Org-mode / markdown table re-align, VS Code "Markdown All in One".
// ===========================================================================

/// Render `rows` of cells as an aligned GitHub-flavoured markdown table. The
/// first row is the header; a `| --- | ... |` separator is inserted after it and
/// every column is padded to its widest cell.
pub fn format_markdown_table(rows: &[Vec<String>]) -> Vec<String> {
    if rows.is_empty() {
        return Vec::new();
    }
    let ncol = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![3usize; ncol]; // min 3 so the "---" rule fits
    for row in rows {
        for (c, cell) in row.iter().enumerate() {
            widths[c] = widths[c].max(cell.chars().count());
        }
    }
    let fmt_row = |row: &[String]| -> String {
        let mut s = String::from("|");
        for (c, &w) in widths.iter().enumerate() {
            let cell = row.get(c).map(String::as_str).unwrap_or("");
            let pad = w - cell.chars().count();
            s.push(' ');
            s.push_str(cell);
            s.push_str(&" ".repeat(pad + 1));
            s.push('|');
        }
        s
    };
    let mut out = Vec::with_capacity(rows.len() + 1);
    out.push(fmt_row(&rows[0]));
    let mut sep = String::from("|");
    for &w in &widths {
        sep.push(' ');
        sep.push_str(&"-".repeat(w));
        sep.push(' ');
        sep.push('|');
    }
    out.push(sep);
    for row in &rows[1..] {
        out.push(fmt_row(row));
    }
    out
}

// ===========================================================================
// 8. Comment box / banner
//    Emacs `comment-box`, banner/figlet-style section headers.
// ===========================================================================

/// Wrap `text`'s lines in a box drawn with the given `comment` prefix and `fill`
/// border char (Emacs `comment-box`). Border rows are `<comment> <fill*>`; body
/// rows are `<comment> <line padded>`.
pub fn comment_box(text: &str, comment: &str, fill: char) -> Vec<String> {
    let body: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.lines().collect()
    };
    let width = body.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let border: String = fill.to_string().repeat(width + 2);
    let mut out = Vec::with_capacity(body.len() + 2);
    out.push(format!("{comment} {border}"));
    for line in &body {
        let pad = width - line.chars().count();
        let spaces = " ".repeat(pad);
        out.push(format!("{comment} {fill} {line}{spaces} {fill}"));
    }
    out.push(format!("{comment} {border}"));
    out
}

// ===========================================================================
// 9. Indent guide columns
//    VS Code / Sublime / JetBrains vertical indent guides, incl. blank-line
//    inheritance from surrounding context.
// ===========================================================================

/// For each line, the set of columns (0-based, in `tab_width` steps) at which a
/// vertical indent guide should be drawn. Blank lines inherit the *smaller* of
/// the surrounding non-blank indents (VS Code behaviour) so guides stay
/// continuous across gaps.
pub fn indent_guide_columns(lines: &[String], tab_width: usize) -> Vec<Vec<usize>> {
    let tw = tab_width.max(1);
    let indent_of = |line: &str| -> Option<usize> {
        if line.trim().is_empty() {
            return None; // blank
        }
        let mut col = 0usize;
        for c in line.chars() {
            match c {
                ' ' => col += 1,
                '\t' => col += tw - (col % tw),
                _ => break,
            }
        }
        Some(col)
    };
    let indents: Vec<Option<usize>> = lines.iter().map(|l| indent_of(l)).collect();
    (0..lines.len())
        .map(|i| {
            let ind = match indents[i] {
                Some(v) => v,
                None => {
                    // Blank line: inherit min(prev non-blank, next non-blank).
                    let prev = indents[..i].iter().rev().flatten().next().copied();
                    let next = indents[i + 1..].iter().flatten().next().copied();
                    match (prev, next) {
                        (Some(a), Some(b)) => a.min(b),
                        (Some(a), None) | (None, Some(a)) => a,
                        (None, None) => 0,
                    }
                }
            };
            (1..).map(|k| k * tw).take_while(|&col| col < ind).collect()
        })
        .collect()
}

// ===========================================================================
// 10. Whitespace normalisation
//     Emacs `just-one-space`/`cycle-spacing`, `delete-blank-lines` (`C-x C-o`),
//     coreutils `cat -s`.
// ===========================================================================

/// Collapse every run of spaces/tabs within `line` to a single space and trim
/// both ends (Emacs `just-one-space` applied line-wide).
pub fn normalize_whitespace(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Collapse runs of two or more blank lines into a single blank line
/// (`delete-blank-lines` / `cat -s`).
pub fn squeeze_blank_lines(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut prev_blank = false;
    for line in lines {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        out.push(line.clone());
        prev_blank = blank;
    }
    out
}

// ===========================================================================
// 11. Emmet-style wrap in tag
//     Emmet "Wrap with Abbreviation" (VS Code / Sublime / JetBrains).
// ===========================================================================

/// Wrap `inner` in an HTML tag described by a minimal Emmet abbreviation:
/// `tag#id.class1.class2`. Missing tag defaults to `div`.
pub fn wrap_in_tag(inner: &str, abbr: &str) -> String {
    let mut tag = String::new();
    let mut id = String::new();
    let mut classes: Vec<String> = Vec::new();
    let mut mode = 't'; // t=tag, i=id, c=class
    let mut buf = String::new();
    let flush = |mode: char,
                 buf: &mut String,
                 tag: &mut String,
                 id: &mut String,
                 classes: &mut Vec<String>| {
        if buf.is_empty() {
            return;
        }
        match mode {
            't' => *tag = std::mem::take(buf),
            'i' => *id = std::mem::take(buf),
            'c' => classes.push(std::mem::take(buf)),
            _ => {}
        }
    };
    for c in abbr.chars() {
        match c {
            '#' => {
                flush(mode, &mut buf, &mut tag, &mut id, &mut classes);
                mode = 'i';
            }
            '.' => {
                flush(mode, &mut buf, &mut tag, &mut id, &mut classes);
                mode = 'c';
            }
            _ => buf.push(c),
        }
    }
    flush(mode, &mut buf, &mut tag, &mut id, &mut classes);
    if tag.is_empty() {
        tag = "div".into();
    }
    let mut attrs = String::new();
    if !id.is_empty() {
        attrs.push_str(&format!(" id=\"{id}\""));
    }
    if !classes.is_empty() {
        attrs.push_str(&format!(" class=\"{}\"", classes.join(" ")));
    }
    format!("<{tag}{attrs}>{inner}</{tag}>")
}

// ===========================================================================
// 12. Keyboard-macro counter
//     Emacs `kmacro-insert-counter` (`C-x C-k C-i`) + format / `kmacro-add-counter`.
// ===========================================================================

/// A keyboard-macro counter with a step and a printf-style format supporting
/// `%d` and zero-padded `%0Nd` (Emacs `kmacro-set-format`).
#[derive(Debug, Clone)]
pub struct KmacroCounter {
    value: i64,
    step: i64,
    format: String,
}

impl KmacroCounter {
    pub fn new(start: i64, step: i64) -> Self {
        KmacroCounter {
            value: start,
            step,
            format: "%d".into(),
        }
    }

    pub fn set_format(&mut self, format: impl Into<String>) {
        self.format = format.into();
    }

    pub fn value(&self) -> i64 {
        self.value
    }

    /// Add `n` to the counter without inserting (Emacs `kmacro-add-counter`).
    pub fn add(&mut self, n: i64) {
        self.value += n;
    }

    /// Render the current value through the format string.
    pub fn render(&self) -> String {
        render_counter(&self.format, self.value)
    }

    /// Render the current value, then advance by `step` (Emacs
    /// `kmacro-insert-counter`).
    pub fn insert_and_advance(&mut self) -> String {
        let s = self.render();
        self.value += self.step;
        s
    }
}

fn render_counter(format: &str, value: i64) -> String {
    // Find a %d / %0Nd directive; anything else is literal.
    if let Some(pct) = format.find('%') {
        let rest = &format[pct + 1..];
        let mut chars = rest.char_indices();
        let mut width = 0usize;
        let mut zero = false;
        for (idx, c) in chars.by_ref() {
            if c == '0' && width == 0 && !zero {
                zero = true;
            } else if c.is_ascii_digit() {
                width = width * 10 + (c as u8 - b'0') as usize;
            } else if c == 'd' {
                let consumed = idx + 1;
                let body = if zero {
                    format!("{:0>width$}", value, width = width)
                } else {
                    format!("{:>width$}", value, width = width)
                };
                return format!("{}{}{}", &format[..pct], body, &rest[consumed..]);
            } else {
                break;
            }
        }
    }
    format.to_string()
}

// ===========================================================================
// 13. Query-replace planning
//     Emacs `query-replace` / `query-replace-regexp` interactive y/n; the
//     replacement may be pulled from a register.
// ===========================================================================

/// Byte spans of every occurrence of `needle` in `haystack` — the match list an
/// interactive query-replace steps through.
pub fn query_replace_matches(
    haystack: &str,
    needle: &str,
    ignore_case: bool,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    if ignore_case {
        let hl = haystack.to_lowercase();
        let nl = needle.to_lowercase();
        // Lower-casing can change byte lengths; map matches back conservatively by
        // scanning the original for case-insensitive equality at char boundaries.
        let mut from = 0;
        while let Some(rel) = hl[from..].find(&nl) {
            let start_l = from + rel;
            // Translate lowercased byte offset to an original char boundary count.
            let char_start = hl[..start_l].chars().count();
            if let Some((bs, be)) = nth_ci_span(haystack, needle, char_start) {
                out.push((bs, be));
            }
            from = start_l + nl.len().max(1);
        }
    } else {
        let mut from = 0;
        while let Some(rel) = haystack[from..].find(needle) {
            let start = from + rel;
            out.push((start, start + needle.len()));
            from = start + needle.len().max(1);
        }
    }
    out
}

fn nth_ci_span(haystack: &str, needle: &str, char_start: usize) -> Option<(usize, usize)> {
    let byte_start = haystack.char_indices().nth(char_start).map(|(b, _)| b)?;
    let end_char = char_start + needle.chars().count();
    let byte_end = haystack
        .char_indices()
        .nth(end_char)
        .map(|(b, _)| b)
        .unwrap_or(haystack.len());
    Some((byte_start, byte_end))
}

/// Apply a query-replace given a per-match `accept` decision (models interactive
/// y/n / the register-sourced replacement). `accept(i)` decides whether the
/// i-th match (from [`query_replace_matches`]) is replaced with `replacement`.
pub fn query_replace<F: Fn(usize) -> bool>(
    haystack: &str,
    needle: &str,
    replacement: &str,
    ignore_case: bool,
    accept: F,
) -> String {
    let matches = query_replace_matches(haystack, needle, ignore_case);
    let mut out = String::with_capacity(haystack.len());
    let mut last = 0;
    for (i, (s, e)) in matches.into_iter().enumerate() {
        if s < last {
            continue; // overlapping guard
        }
        out.push_str(&haystack[last..s]);
        if accept(i) {
            out.push_str(replacement);
        } else {
            out.push_str(&haystack[s..e]);
        }
        last = e;
    }
    out.push_str(&haystack[last..]);
    out
}

// ===========================================================================
// 14. ⭐ zmax original — grid transpose (beyond Emacs / VS Code / Vim)
//     No mainstream editor transposes a delimited grid (rows <-> columns) as a
//     first-class edit; Emacs `transpose-lines`/`transpose-chars` are 1-D only.
// ===========================================================================

/// Transpose a grid of cells so rows become columns and vice-versa. Short rows
/// are padded with empty cells so the result is rectangular. zmax original —
/// beyond Emacs `transpose-lines`, Vim, VS Code and Sublime (all 1-dimensional).
pub fn transpose_grid(rows: &[Vec<String>]) -> Vec<Vec<String>> {
    let ncol = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    (0..ncol)
        .map(|c| {
            rows.iter()
                .map(|r| r.get(c).cloned().unwrap_or_default())
                .collect()
        })
        .collect()
}

// ===========================================================================
// 15. ⭐ zmax original — auto-delimiter table align
//     `text_engine::align_on_separator` needs you to name the separator; this
//     auto-detects the dominant delimiter and pads every column. Beyond Emacs
//     `align-regexp` and Vim easy-align, which both require an explicit pattern.
// ===========================================================================

/// Auto-detect the dominant single-char delimiter among `, \t | : ;` across
/// `lines`, then split every line on it and pad each column to its widest cell,
/// rejoining with `" <delim> "`. Lines are left untouched if no delimiter is
/// shared. zmax original — beyond Emacs `align-regexp` / Vim easy-align.
pub fn align_table_auto(lines: &[String]) -> Vec<String> {
    align_auto(lines, &[',', '\t', '|', ':', ';'])
}

/// Align `lines` on the dominant delimiter among `cands` (the one appearing on
/// the most lines), padding each column to its widest cell. Lines are left
/// untouched if no candidate is shared by at least two lines. Backs both
/// [`align_table_auto`] and the Emacs `align-current` / `align-entire` commands
/// (which pass a set that includes `=`).
pub fn align_auto(lines: &[String], cands: &[char]) -> Vec<String> {
    // Score = number of lines that contain the delimiter; pick the max.
    let mut best = None;
    let mut best_score = 0usize;
    for &d in cands {
        let score = lines.iter().filter(|l| l.contains(d)).count();
        if score > best_score {
            best_score = score;
            best = Some(d);
        }
    }
    let Some(delim) = best else {
        return lines.to_vec();
    };
    if best_score < 2 {
        return lines.to_vec();
    }
    // Split into trimmed cells.
    let grid: Vec<Vec<String>> = lines
        .iter()
        .map(|l| l.split(delim).map(|c| c.trim().to_string()).collect())
        .collect();
    let ncol = grid.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0usize; ncol];
    for row in &grid {
        for (c, cell) in row.iter().enumerate() {
            widths[c] = widths[c].max(cell.chars().count());
        }
    }
    grid.iter()
        .map(|row| {
            let joined = row
                .iter()
                .enumerate()
                .map(|(c, cell)| {
                    let pad = widths[c] - cell.chars().count();
                    format!("{cell}{}", " ".repeat(pad))
                })
                .collect::<Vec<_>>()
                .join(&format!(" {delim} "));
            joined.trim_end().to_string()
        })
        .collect()
}

/// Delimiters `align-current` / `align-entire` auto-detect (assignments first).
const ALIGN_CANDS: [char; 6] = ['=', ':', ',', '|', ';', '\t'];

/// Emacs `align-entire`: align the whole region as one section on the dominant
/// delimiter (including `=`, so assignments line up).
pub fn align_entire(lines: &[String]) -> Vec<String> {
    align_auto(lines, &ALIGN_CANDS)
}

/// Emacs `align-current`: align each blank-line-separated section of the region
/// independently (blank lines are section boundaries and pass through unchanged).
pub fn align_current(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut section: Vec<String> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            out.extend(align_auto(&section, &ALIGN_CANDS));
            section.clear();
            out.push(line.clone());
        } else {
            section.push(line.clone());
        }
    }
    out.extend(align_auto(&section, &ALIGN_CANDS));
    out
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn soft_wrap_breaks_on_words() {
        // width 10: "the quick ", "brown fox ", "jumps"
        let offs = soft_wrap_offsets("the quick brown fox jumps", 10);
        assert_eq!(offs, vec![0, 10, 20]);
    }

    #[test]
    fn soft_wrap_hard_breaks_long_word() {
        let offs = soft_wrap_offsets("abcdefghijklmno", 5);
        assert_eq!(offs, vec![0, 5, 10]);
    }

    #[test]
    fn soft_wrap_short_line_is_single_row() {
        assert_eq!(soft_wrap_offsets("hi", 10), vec![0]);
        assert_eq!(soft_wrap_offsets("anything", 0), vec![0]);
    }

    #[test]
    fn visual_motion_down_and_up() {
        let offs = soft_wrap_offsets("the quick brown fox jumps", 10); // [0,10,20]
                                                                       // caret at col 2 of row 0 (char 2) -> down to row 1 col 2 -> char 12
        assert_eq!(visual_move_down(&offs, 25, 2), 12);
        // and back up
        assert_eq!(visual_move_up(&offs, 12), 2);
        // last row cannot go down
        assert_eq!(visual_move_down(&offs, 25, 21), 21);
        // first row cannot go up
        assert_eq!(visual_move_up(&offs, 3), 3);
    }

    #[test]
    fn expand_region_ladder() {
        //            0         1
        //            0123456789012345
        let text = "foo(bar baz)\nnext";
        // caret inside "bar" -> word
        let w = expand_region(text, (4, 4)).unwrap();
        assert_eq!(&text[w.0..w.1], "bar");
        // from the word -> inside the parens
        let inner = expand_region(text, w).unwrap();
        assert_eq!(&text[inner.0..inner.1], "bar baz");
        // from inside -> including the parens
        let outer = expand_region(text, inner).unwrap();
        assert_eq!(&text[outer.0..outer.1], "(bar baz)");
        // then -> the line
        let line = expand_region(text, outer).unwrap();
        assert_eq!(&text[line.0..line.1], "foo(bar baz)");
    }

    #[test]
    fn expand_region_whole_buffer_terminal() {
        let text = "abc";
        let whole = expand_region(text, (0, 3));
        assert!(whole.is_none());
    }

    #[test]
    fn multi_cursor_add_next_and_all() {
        let hay = "foo bar foo baz foo";
        let mut mc = MultiCursor::new(0, 3, "foo");
        let a = mc.add_next_match(hay).unwrap();
        assert_eq!(a, (8, 11));
        let b = mc.add_next_match(hay).unwrap();
        assert_eq!(b, (16, 19));
        // wraps: nothing new left -> None
        assert!(mc.add_next_match(hay).is_none());
        assert_eq!(mc.cursors(), &[(0, 3), (8, 11), (16, 19)]);

        let mut mc2 = MultiCursor::new(0, 3, "foo");
        assert_eq!(mc2.add_all_matches(hay), 2);
        assert_eq!(mc2.cursors().len(), 3);
    }

    #[test]
    fn join_with_separator_trims() {
        let lines = v(&["  a ", "b", " c"]);
        assert_eq!(join_with_separator(&lines, ", ", true), "a, b, c");
        assert_eq!(join_with_separator(&v(&["a", "b"]), "", false), "ab");
    }

    #[test]
    fn sequence_increment_vim_g_ctrl_a() {
        let lines = v(&["0", "0", "0", "no number", "0"]);
        let out = sequence_increment(&lines, 1);
        assert_eq!(out, v(&["1", "2", "3", "no number", "4"]));
        // step 10 with embedded numbers and prefixes
        let lines2 = v(&["item 5", "item 5"]);
        assert_eq!(sequence_increment(&lines2, 10), v(&["item 15", "item 25"]));
    }

    #[test]
    fn sequence_increment_signed() {
        let lines = v(&["x=-1", "x=-1"]);
        assert_eq!(sequence_increment(&lines, 1), v(&["x=0", "x=1"]));
    }

    #[test]
    fn uniq_variants() {
        let lines = v(&["a", "b", "a", "b", "b", "c"]);
        assert_eq!(uniq_all(&lines), v(&["a", "b", "c"]));
        let adj = v(&["a", "a", "b", "a"]);
        assert_eq!(uniq_count(&adj), v(&["   2 a", "   1 b", "   1 a"]));
    }

    #[test]
    fn markdown_table_align() {
        let rows = vec![v(&["name", "age"]), v(&["alice", "30"]), v(&["bob", "7"])];
        let out = format_markdown_table(&rows);
        assert_eq!(out[0], "| name  | age |");
        assert_eq!(out[1], "| ----- | --- |");
        assert_eq!(out[2], "| alice | 30  |");
        assert_eq!(out[3], "| bob   | 7   |");
    }

    #[test]
    fn comment_box_draws_border() {
        let out = comment_box("hi\nthere", "//", '*');
        assert_eq!(out[0], "// *******");
        assert_eq!(out[1], "// * hi    *");
        assert_eq!(out[2], "// * there *");
        assert_eq!(out[3], "// *******");
    }

    #[test]
    fn indent_guides_with_blank_inheritance() {
        let lines = v(&["def f():", "    a = 1", "", "    b = 2", "c = 3"]);
        let guides = indent_guide_columns(&lines, 4);
        let empty = Vec::<usize>::new();
        assert_eq!(guides[0], empty); // no indent
        assert_eq!(guides[1], empty); // indent 4 -> no guide before col 4
                                      // blank line inherits min(4,4)=4 -> still no guide (col 4 not < 4)
        assert_eq!(guides[2], empty);
        // deeper indent shows a guide
        let deep = v(&["a", "        x"]);
        let g = indent_guide_columns(&deep, 4);
        assert_eq!(g[1], vec![4]); // indent 8 -> guide at col 4
    }

    #[test]
    fn normalize_and_squeeze_whitespace() {
        assert_eq!(normalize_whitespace("  a\t b   c "), "a b c");
        let lines = v(&["a", "", "", "b", "", "c"]);
        assert_eq!(squeeze_blank_lines(&lines), v(&["a", "", "b", "", "c"]));
    }

    #[test]
    fn wrap_in_tag_emmet() {
        assert_eq!(wrap_in_tag("x", "div"), "<div>x</div>");
        assert_eq!(
            wrap_in_tag("x", "section#main.box.warn"),
            "<section id=\"main\" class=\"box warn\">x</section>"
        );
        assert_eq!(wrap_in_tag("x", ".card"), "<div class=\"card\">x</div>");
    }

    #[test]
    fn kmacro_counter_advances_and_formats() {
        let mut c = KmacroCounter::new(1, 1);
        assert_eq!(c.insert_and_advance(), "1");
        assert_eq!(c.insert_and_advance(), "2");
        assert_eq!(c.value(), 3);
        c.set_format("item-%03d:");
        assert_eq!(c.render(), "item-003:");
        c.add(7);
        assert_eq!(c.value(), 10);
    }

    #[test]
    fn query_replace_selective() {
        let hay = "cat cat cat";
        assert_eq!(
            query_replace_matches(hay, "cat", false),
            vec![(0, 3), (4, 7), (8, 11)]
        );
        // replace only the middle match
        let out = query_replace(hay, "cat", "dog", false, |i| i == 1);
        assert_eq!(out, "cat dog cat");
        // replace all
        assert_eq!(
            query_replace(hay, "cat", "dog", false, |_| true),
            "dog dog dog"
        );
    }

    #[test]
    fn query_replace_case_insensitive() {
        let hay = "Foo foo FOO";
        let out = query_replace(hay, "foo", "bar", true, |_| true);
        assert_eq!(out, "bar bar bar");
    }

    #[test]
    fn transpose_grid_original() {
        let rows = vec![v(&["a", "b", "c"]), v(&["1", "2", "3"])];
        let t = transpose_grid(&rows);
        assert_eq!(t, vec![v(&["a", "1"]), v(&["b", "2"]), v(&["c", "3"])]);
        // ragged rows pad with empty
        let ragged = vec![v(&["a", "b"]), v(&["1"])];
        assert_eq!(transpose_grid(&ragged), vec![v(&["a", "1"]), v(&["b", ""])]);
    }

    #[test]
    fn align_table_auto_original() {
        let lines = v(&["name,age,city", "alice,30,nyc", "bob,7,la"]);
        let out = align_table_auto(&lines);
        assert_eq!(out[0], "name  , age , city");
        assert_eq!(out[1], "alice , 30  , nyc");
        assert_eq!(out[2], "bob   , 7   , la");
        // no shared delimiter -> untouched
        let plain = v(&["hello", "world"]);
        assert_eq!(align_table_auto(&plain), plain);
    }

    #[test]
    fn align_entire_lines_up_assignments() {
        // align-entire aligns the whole region on `=` (assignments).
        let out = align_entire(&v(&["a = 1", "bb = 2", "ccc = 3"]));
        assert_eq!(out, v(&["a   = 1", "bb  = 2", "ccc = 3"]));
    }

    #[test]
    fn align_current_respects_blank_line_sections() {
        // Two sections separated by a blank line align independently; the blank
        // line passes through.
        let input = v(&["a = 1", "bbb = 2", "", "xx = 10", "y = 2"]);
        let out = align_current(&input);
        assert_eq!(out, v(&["a   = 1", "bbb = 2", "", "xx = 10", "y  = 2"]));
    }
}
