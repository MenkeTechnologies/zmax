//! Pure-Rust region / line / structural editing algorithms — a batch of gap-fill
//! commands driving zemacs toward a strict superset of GNU Emacs, VS Code,
//! Neovim/Vim, Sublime Text, JetBrains, Zed and Helix (plus a couple of originals
//! that go beyond all of them).
//!
//! Like `zemacs-term`'s `text_ops`, everything here is a plain function (or small
//! value type) over `&str` / `&[String]` with no editor types leaking in, so each
//! is unit tested in isolation. The command layer extracts the live selection's
//! line span or region, calls one of these, and applies the result as a single
//! undoable transaction.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Line-block transforms
// ---------------------------------------------------------------------------

/// VS Code "Join Lines" (Ctrl+J) / Vim `J` / JetBrains "Join Lines": collapse a
/// block of lines into a single line. Continuation lines always have their
/// leading whitespace dropped (the whole point of a join). With `space_separated`
/// (Vim/VS Code semantics) each boundary becomes exactly one space and every
/// segment is trimmed; otherwise the trimmed continuations are concatenated with
/// no inserted separator.
pub fn join_lines(lines: &[String], space_separated: bool) -> String {
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            out.push_str(line.trim_end());
            continue;
        }
        let seg = if space_separated {
            line.trim()
        } else {
            line.trim_start()
        };
        if space_separated && !seg.is_empty() && !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push_str(seg);
    }
    out
}

/// Options for [`sort_lines`], mirroring GNU `sort` / Vim `:sort` flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SortOptions {
    /// Reverse the final order (`sort -r`, `:sort!`).
    pub reverse: bool,
    /// Case-insensitive comparison (`sort -f`, `:sort i`).
    pub ignore_case: bool,
    /// Compare by the leading numeric value (`sort -n`, `:sort n`).
    pub numeric: bool,
    /// Drop exact duplicate lines after sorting (`sort -u`, `:sort u`).
    pub unique: bool,
}

/// Sublime "Sort Lines" / VS Code "Sort Lines Ascending" / Vim `:sort` / Emacs
/// `sort-lines`: sort a block of lines with the flags in [`SortOptions`].
pub fn sort_lines(lines: &[String], opts: SortOptions) -> Vec<String> {
    let mut v = lines.to_vec();
    v.sort_by(|a, b| line_cmp(a, b, opts));
    if opts.reverse {
        v.reverse();
    }
    if opts.unique {
        v.dedup();
    }
    v
}

fn line_cmp(a: &str, b: &str, opts: SortOptions) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if opts.numeric {
        match (leading_number(a), leading_number(b)) {
            (Some(x), Some(y)) => {
                return x
                    .partial_cmp(&y)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| string_cmp(a, b, opts.ignore_case));
            }
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            (None, None) => {}
        }
    }
    string_cmp(a, b, opts.ignore_case)
}

fn string_cmp(a: &str, b: &str, ignore_case: bool) -> std::cmp::Ordering {
    if ignore_case {
        a.to_lowercase().cmp(&b.to_lowercase())
    } else {
        a.cmp(b)
    }
}

/// Parse the leading (possibly signed / decimal) number of a line, ignoring
/// leading whitespace. Returns `None` when the line does not start with a number.
fn leading_number(s: &str) -> Option<f64> {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
        i += 1;
    }
    let mut seen_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        seen_digit = true;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            seen_digit = true;
        }
    }
    if !seen_digit {
        return None;
    }
    t[..i].parse::<f64>().ok()
}

/// Emacs `reverse-region` / Vim `:g/^/m0`: reverse the order of a block of lines.
pub fn reverse_lines(lines: &[String]) -> Vec<String> {
    lines.iter().rev().cloned().collect()
}

/// coreutils `uniq` / Emacs `delete-duplicate-lines` with the adjacent-only flag:
/// collapse *runs* of identical consecutive lines to one (unlike a global dedup,
/// non-adjacent duplicates are kept), so it composes with a prior [`sort_lines`].
pub fn uniq_adjacent(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if out.last().is_none_or(|p| p != l) {
            out.push(l.clone());
        }
    }
    out
}

/// Emacs `rectangle-number-lines` / JetBrains sequential-number insert: prefix each
/// line with a right-aligned, zero-free sequential number starting at `start`,
/// followed by `sep`. All numbers are padded to a common width for tidy columns.
pub fn number_lines(lines: &[String], start: i64, sep: &str) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }
    let last = start + lines.len() as i64 - 1;
    let width = start.to_string().len().max(last.to_string().len());
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:>width$}{}{}", start + i as i64, sep, l, width = width))
        .collect()
}

/// Universal "delete trailing whitespace" (Emacs `delete-trailing-whitespace`,
/// VS Code `files.trimTrailingWhitespace`): strip trailing spaces/tabs from every
/// line without touching interior content.
pub fn trim_trailing_whitespace(lines: &[String]) -> Vec<String> {
    lines.iter().map(|l| l.trim_end().to_string()).collect()
}

/// Emacs `occur` / Vim `:g/pat/p`: return every matching line with its 1-based
/// line number, leaving the buffer untouched (the caller renders the hit list).
pub fn occur(lines: &[String], matches: impl Fn(&str) -> bool) -> Vec<(usize, String)> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| matches(l))
        .map(|(i, l)| (i + 1, l.clone()))
        .collect()
}

/// Emacs `C-x C-t` `transpose-lines` / VS Code "Move Line Down": swap line `i`
/// with the following line. A no-op when `i` is the last line.
pub fn transpose_lines(lines: &[String], i: usize) -> Vec<String> {
    let mut v = lines.to_vec();
    if i + 1 < v.len() {
        v.swap(i, i + 1);
    }
    v
}

/// Emacs `transpose-regions`: swap the text of char-range `[s1,e1)` with
/// char-range `[s2,e2)` in `text`. The regions may differ in length; the text
/// between them is preserved. Returns the transposed string, or `None` if the
/// ranges are ill-formed (each must satisfy `start <= end`), overlap, or fall
/// outside the text. Ranges given in either order are normalised so the earlier
/// region comes first.
pub fn transpose_regions(
    text: &str,
    s1: usize,
    e1: usize,
    s2: usize,
    e2: usize,
) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    // Each range must be well-formed and in bounds.
    if s1 > e1 || s2 > e2 || e1 > n || e2 > n {
        return None;
    }
    // Order the two regions so region A precedes region B.
    let ((as_, ae), (bs, be)) = if s1 <= s2 {
        ((s1, e1), (s2, e2))
    } else {
        ((s2, e2), (s1, e1))
    };
    // They must not overlap.
    if ae > bs {
        return None;
    }
    let slice = |a: usize, b: usize| -> String { chars[a..b].iter().collect() };
    Some(format!(
        "{}{}{}{}{}",
        slice(0, as_),   // prefix before region A
        slice(bs, be),   // region B in A's place
        slice(ae, bs),   // the untouched middle
        slice(as_, ae),  // region A in B's place
        slice(be, n),    // suffix after region B
    ))
}

/// ⭐ zemacs original — beyond GNU Emacs, VS Code, Vim, Sublime, JetBrains, Zed and
/// Helix: cyclically rotate a block of lines by `n` (positive rotates the block
/// *down*, so the last `n` lines wrap to the top). None of the competitors offer a
/// single wrap-around line-rotate command.
pub fn rotate_lines(lines: &[String], n: isize) -> Vec<String> {
    let len = lines.len();
    if len == 0 {
        return Vec::new();
    }
    let len_i = len as isize;
    (0..len_i)
        .map(|i| lines[(i - n).rem_euclid(len_i) as usize].clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Character / case transforms
// ---------------------------------------------------------------------------

/// Emacs `rot13-region` / Vim `g?`: apply the ROT13 cipher to ASCII letters,
/// leaving every other character (digits, punctuation, non-ASCII) untouched.
pub fn rot13(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a' + 13) % 26) + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A' + 13) % 26) + b'A') as char,
            _ => c,
        })
        .collect()
}

/// Vim `g~` `invert case`: swap the case of every cased character, honoring
/// multi-character Unicode case mappings.
pub fn invert_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_uppercase() {
            out.extend(c.to_lowercase());
        } else if c.is_lowercase() {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Emacs `C-t` `transpose-chars`: swap the two characters straddling `cursor`
/// (a char index) and advance the cursor past them, returning `(new_text,
/// new_cursor)`. At end-of-text the last two characters are transposed (Emacs
/// behavior). Returns `None` when there is nothing to transpose.
pub fn transpose_chars(text: &str, cursor: usize) -> Option<(String, usize)> {
    let mut chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n < 2 {
        return None;
    }
    let (a, b) = if cursor >= n {
        (n - 2, n - 1)
    } else if cursor == 0 {
        return None;
    } else {
        (cursor - 1, cursor)
    };
    chars.swap(a, b);
    Some((chars.into_iter().collect(), (b + 1).min(n)))
}

// ---------------------------------------------------------------------------
// Arithmetic evaluation (Emacs `calc-eval` / VS Code "Calculate" family)
// ---------------------------------------------------------------------------

/// Emacs `calc-eval` / VS Code "Calculate" extensions: evaluate an arithmetic
/// expression string, returning `f64` or `None` on a parse/eval error (including
/// division/modulo by zero). Supports `+ - * / % ^`, parenthesized grouping,
/// unary minus/plus, and decimal literals with the usual precedence (`^` is
/// right-associative). Handy for "replace the selected expression with its value".
pub fn eval_arithmetic(expr: &str) -> Option<f64> {
    let toks = tokenize(expr)?;
    let mut p = Parser { toks, pos: 0 };
    let v = p.expr()?;
    if p.pos == p.toks.len() {
        Some(v)
    } else {
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tok {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
}

fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let chars: Vec<char> = s.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => {
                toks.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                toks.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                toks.push(Tok::Star);
                i += 1;
            }
            '/' => {
                toks.push(Tok::Slash);
                i += 1;
            }
            '%' => {
                toks.push(Tok::Percent);
                i += 1;
            }
            '^' => {
                toks.push(Tok::Caret);
                i += 1;
            }
            '(' => {
                toks.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                toks.push(Tok::RParen);
                i += 1;
            }
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let lit: String = chars[start..i].iter().collect();
                toks.push(Tok::Num(lit.parse::<f64>().ok()?));
            }
            _ => return None,
        }
    }
    Some(toks)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<Tok> {
        self.toks.get(self.pos).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            match self.peek() {
                Some(Tok::Plus) => {
                    self.pos += 1;
                    v += self.term()?;
                }
                Some(Tok::Minus) => {
                    self.pos += 1;
                    v -= self.term()?;
                }
                _ => break,
            }
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(Tok::Star) => {
                    self.pos += 1;
                    v *= self.power()?;
                }
                Some(Tok::Slash) => {
                    self.pos += 1;
                    let d = self.power()?;
                    if d == 0.0 {
                        return None;
                    }
                    v /= d;
                }
                Some(Tok::Percent) => {
                    self.pos += 1;
                    let d = self.power()?;
                    if d == 0.0 {
                        return None;
                    }
                    v %= d;
                }
                _ => break,
            }
        }
        Some(v)
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if let Some(Tok::Caret) = self.peek() {
            self.pos += 1;
            let exp = self.power()?;
            Some(base.powf(exp))
        } else {
            Some(base)
        }
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(Tok::Minus) => {
                self.pos += 1;
                Some(-self.unary()?)
            }
            Some(Tok::Plus) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(Tok::Num(n)) => {
                self.pos += 1;
                Some(n)
            }
            Some(Tok::LParen) => {
                self.pos += 1;
                let v = self.expr()?;
                if let Some(Tok::RParen) = self.peek() {
                    self.pos += 1;
                    Some(v)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Structural / paredit-family ops (Emacs `paredit`)
// ---------------------------------------------------------------------------

fn close_for(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

fn is_close(c: char) -> bool {
    matches!(c, ')' | ']' | '}')
}

/// Index of the delimiter that closes the bracket opened at `open`, matching by
/// depth (assumes well-formed nesting).
fn matching_close(chars: &[char], open: usize) -> Option<usize> {
    close_for(chars[open])?;
    let mut depth = 0i32;
    for (i, &c) in chars.iter().enumerate().skip(open) {
        if close_for(c).is_some() {
            depth += 1;
        } else if is_close(c) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// The innermost open/close bracket pair enclosing `cursor` (a char index).
fn enclosing(chars: &[char], cursor: usize) -> Option<(usize, usize)> {
    let mut depth = 0i32;
    let mut open = None;
    let mut i = cursor.min(chars.len());
    while i > 0 {
        i -= 1;
        let c = chars[i];
        if is_close(c) {
            depth += 1;
        } else if close_for(c).is_some() {
            if depth == 0 {
                open = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let o = open?;
    let c = matching_close(chars, o)?;
    Some((o, c))
}

/// Emacs paredit `splice-sexp` (`M-s`): remove the pair of delimiters enclosing
/// `cursor`, lifting the list's contents into its parent. Returns `None` when the
/// cursor is not inside a bracketed form.
pub fn splice_sexp(text: &str, cursor: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let (o, c) = enclosing(&chars, cursor)?;
    Some(
        chars
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != o && *i != c)
            .map(|(_, &ch)| ch)
            .collect(),
    )
}

/// Emacs paredit `slurp-forward` (`C-)`): move the closing delimiter of the list
/// enclosing `cursor` rightward past the next sibling form (an atom or a balanced
/// bracketed expression), pulling it inside the list. Returns `None` when the
/// cursor is not inside a form or there is nothing to slurp.
pub fn slurp_forward(text: &str, cursor: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let (_open, close) = enclosing(&chars, cursor)?;
    let mut i = close + 1;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    let end = if close_for(chars[i]).is_some() {
        matching_close(&chars, i)?
    } else {
        let mut j = i;
        while j < chars.len()
            && !chars[j].is_whitespace()
            && close_for(chars[j]).is_none()
            && !is_close(chars[j])
        {
            j += 1;
        }
        j - 1
    };
    let mut out = String::new();
    for (idx, &ch) in chars.iter().enumerate() {
        if idx == close {
            continue;
        }
        out.push(ch);
        if idx == end {
            out.push(chars[close]);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Stateful editing surfaces: kill-ring & registers
// ---------------------------------------------------------------------------

/// GNU Emacs `kill-ring`: a bounded most-recent-first ring of killed text with a
/// rotating yank pointer. `kill` pushes a new entry (dropping the oldest past the
/// cap) and resets the pointer; `yank_pop` rotates to the next-older entry, exactly
/// like Emacs `M-y` after a `C-y`.
#[derive(Clone, Debug)]
pub struct KillRing {
    entries: Vec<String>,
    max: usize,
    ptr: usize,
}

impl KillRing {
    pub fn new(max: usize) -> Self {
        Self {
            entries: Vec::new(),
            max: max.max(1),
            ptr: 0,
        }
    }

    /// Push freshly killed text to the front, resetting the yank pointer.
    pub fn kill(&mut self, text: impl Into<String>) {
        self.entries.insert(0, text.into());
        self.entries.truncate(self.max);
        self.ptr = 0;
    }

    /// The current yank target (front, or wherever `yank_pop` has rotated to).
    pub fn front(&self) -> Option<&str> {
        self.entries.get(self.ptr).map(String::as_str)
    }

    /// Emacs `M-y`: rotate to the next-older entry and return it.
    pub fn yank_pop(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        self.ptr = (self.ptr + 1) % self.entries.len();
        self.entries.get(self.ptr).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Vim / Emacs named registers: `set(name, text)` stores into a named slot; a Vim
/// uppercase name (`"A`) *appends* to the matching lowercase register instead of
/// overwriting, and lookups are case-insensitive.
#[derive(Clone, Debug, Default)]
pub struct Registers {
    map: HashMap<char, String>,
}

impl Registers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: char, text: &str) {
        if name.is_ascii_uppercase() {
            self.map
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push_str(text);
        } else {
            self.map.insert(name, text.to_string());
        }
    }

    pub fn get(&self, name: char) -> Option<&str> {
        self.map.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn join_lines_space_separated_and_raw() {
        assert_eq!(
            join_lines(&v(&["foo  ", "  bar", "baz"]), true),
            "foo bar baz"
        );
        assert_eq!(join_lines(&v(&["foo", "  bar"]), false), "foobar");
    }

    #[test]
    fn sort_lines_variants() {
        assert_eq!(
            sort_lines(&v(&["b", "a", "c"]), SortOptions::default()),
            v(&["a", "b", "c"])
        );
        let numeric = SortOptions {
            numeric: true,
            ..Default::default()
        };
        assert_eq!(
            sort_lines(&v(&["10", "2", "1"]), numeric),
            v(&["1", "2", "10"])
        );
        let ci_rev = SortOptions {
            ignore_case: true,
            reverse: true,
            ..Default::default()
        };
        assert_eq!(
            sort_lines(&v(&["a", "B", "c"]), ci_rev),
            v(&["c", "B", "a"])
        );
        let uniq = SortOptions {
            unique: true,
            ..Default::default()
        };
        assert_eq!(sort_lines(&v(&["b", "a", "b", "a"]), uniq), v(&["a", "b"]));
    }

    #[test]
    fn reverse_and_uniq() {
        assert_eq!(reverse_lines(&v(&["a", "b", "c"])), v(&["c", "b", "a"]));
        assert_eq!(
            uniq_adjacent(&v(&["a", "a", "b", "a", "a"])),
            v(&["a", "b", "a"])
        );
    }

    #[test]
    fn number_lines_pads_width() {
        assert_eq!(
            number_lines(&v(&["a", "b"]), 9, ": "),
            v(&[" 9: a", "10: b"])
        );
    }

    #[test]
    fn trim_trailing() {
        assert_eq!(
            trim_trailing_whitespace(&v(&["a  ", "b\t", "c"])),
            v(&["a", "b", "c"])
        );
    }

    #[test]
    fn occur_reports_line_numbers() {
        let got = occur(&v(&["fn a", "let x", "fn b"]), |l| l.starts_with("fn"));
        assert_eq!(got, vec![(1, "fn a".to_string()), (3, "fn b".to_string())]);
    }

    #[test]
    fn transpose_lines_swaps() {
        assert_eq!(
            transpose_lines(&v(&["a", "b", "c"]), 0),
            v(&["b", "a", "c"])
        );
        // no-op on last line
        assert_eq!(transpose_lines(&v(&["a", "b"]), 1), v(&["a", "b"]));
    }

    #[test]
    fn transpose_regions_swaps_text() {
        // "abcXXdefYYghi": swap "XX" [3,5) with "YY" [8,10) -> "abcYYdefXXghi".
        let t = "abcXXdefYYghi";
        assert_eq!(
            transpose_regions(t, 3, 5, 8, 10).as_deref(),
            Some("abcYYdefXXghi")
        );
        // Given in reverse order, normalised to the same result.
        assert_eq!(
            transpose_regions(t, 8, 10, 3, 5).as_deref(),
            Some("abcYYdefXXghi")
        );
    }

    #[test]
    fn transpose_regions_different_lengths_and_guards() {
        // "aWbXYc": swap "W" [1,2) with "XY" [3,5) -> "aXYbWc".
        assert_eq!(
            transpose_regions("aWbXYc", 1, 2, 3, 5).as_deref(),
            Some("aXYbWc")
        );
        // Overlapping ranges are rejected.
        assert_eq!(transpose_regions("abcdef", 1, 4, 3, 5), None);
        // Ill-formed (start > end) rejected.
        assert_eq!(transpose_regions("abcdef", 4, 1, 0, 0), None);
        // Out of bounds rejected.
        assert_eq!(transpose_regions("abc", 0, 1, 2, 9), None);
    }

    #[test]
    fn rotate_lines_wraps() {
        assert_eq!(rotate_lines(&v(&["a", "b", "c"]), 1), v(&["c", "a", "b"]));
        assert_eq!(rotate_lines(&v(&["a", "b", "c"]), -1), v(&["b", "c", "a"]));
        assert_eq!(rotate_lines(&v(&["a", "b", "c"]), 3), v(&["a", "b", "c"]));
    }

    #[test]
    fn rot13_is_involutive() {
        assert_eq!(rot13("Hello, World! 42"), "Uryyb, Jbeyq! 42");
        assert_eq!(rot13(&rot13("Hello")), "Hello");
    }

    #[test]
    fn invert_case_swaps() {
        assert_eq!(invert_case("Hello World 123"), "hELLO wORLD 123");
    }

    #[test]
    fn transpose_chars_mid_and_end() {
        assert_eq!(transpose_chars("abcd", 2), Some(("acbd".to_string(), 3)));
        // at end: swap last two
        assert_eq!(transpose_chars("abcd", 4), Some(("abdc".to_string(), 4)));
        assert_eq!(transpose_chars("a", 1), None);
        assert_eq!(transpose_chars("ab", 0), None);
    }

    #[test]
    fn eval_arithmetic_precedence_and_errors() {
        assert_eq!(eval_arithmetic("1 + 2 * 3"), Some(7.0));
        assert_eq!(eval_arithmetic("(1 + 2) * 3"), Some(9.0));
        assert_eq!(eval_arithmetic("2 ^ 3 ^ 2"), Some(512.0)); // right-assoc
        assert_eq!(eval_arithmetic("-3 + 4"), Some(1.0));
        assert_eq!(eval_arithmetic("7 % 3"), Some(1.0));
        assert_eq!(eval_arithmetic("10 / 4"), Some(2.5));
        assert_eq!(eval_arithmetic("1 / 0"), None);
        assert_eq!(eval_arithmetic("1 +"), None);
        assert_eq!(eval_arithmetic("(1 + 2"), None);
        assert_eq!(eval_arithmetic("1 2"), None);
    }

    #[test]
    fn splice_sexp_removes_enclosing() {
        assert_eq!(splice_sexp("(a (b c) d)", 5).as_deref(), Some("(a b c d)"));
        assert_eq!(splice_sexp("no parens", 3), None);
    }

    #[test]
    fn slurp_forward_pulls_next_form() {
        // atom slurp
        assert_eq!(slurp_forward("(a) b", 1).as_deref(), Some("(a b)"));
        // balanced-form slurp
        assert_eq!(slurp_forward("(a) (b c)", 1).as_deref(), Some("(a (b c))"));
        // nothing to slurp
        assert_eq!(slurp_forward("(a)", 1), None);
    }

    #[test]
    fn kill_ring_rotates_and_caps() {
        let mut kr = KillRing::new(3);
        kr.kill("one");
        kr.kill("two");
        kr.kill("three");
        assert_eq!(kr.front(), Some("three"));
        assert_eq!(kr.yank_pop(), Some("two"));
        assert_eq!(kr.yank_pop(), Some("one"));
        assert_eq!(kr.yank_pop(), Some("three")); // wraps
        kr.kill("four"); // drops "one" (cap 3), resets pointer
        assert_eq!(kr.len(), 3);
        assert_eq!(kr.front(), Some("four"));
    }

    #[test]
    fn registers_set_get_and_append() {
        let mut regs = Registers::new();
        regs.set('a', "hello");
        assert_eq!(regs.get('a'), Some("hello"));
        regs.set('A', " world"); // uppercase appends
        assert_eq!(regs.get('a'), Some("hello world"));
        assert_eq!(regs.get('z'), None);
    }
}
