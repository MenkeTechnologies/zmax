//! Parinfer (Shaun Lebron), the algorithm `parinfer-rust-mode` runs and the
//! spacemacs `+misc/parinfer` layer binds: keep a Lisp buffer's indentation and
//! its parentheses in agreement by inferring one from the other.
//!
//! Three modes, per <https://shaunlebron.github.io/parinfer/>:
//!
//! * [`Mode::Indent`] — indentation is authoritative. Unmatched close-parens
//!   are removed, close-parens at the start and end of a line are removed, and
//!   for every open-paren left unmatched a close-paren is appended to the last
//!   line of its form (the line before the first line indented at or left of
//!   the open-paren's column). Close-parens in the middle of a line are kept.
//! * [`Mode::Paren`] — parens are authoritative. A line's leading close-parens
//!   move to the end of the previous line with code on it, and each line's
//!   indentation is shifted by its parent form's shift and then clamped to at
//!   least one column right of its parent open-paren. Unbalanced input is
//!   refused rather than changed.
//! * [`Mode::Smart`] — indent mode, except that when the caller supplies a
//!   cursor line, the input is balanced, and indent mode would change the
//!   bracket characters on that cursor line (adding a paren the user did not
//!   type, or dropping one), paren mode runs instead so the form keeps its
//!   structure and only its indentation is corrected.
//!
//! What smart mode here does **not** do: upstream parinfer's smart mode diffs
//! the buffer against its previous text to tell "the user moved a whole form"
//! from "the user dedented one line". [`process`] receives one text and no
//! history, so the decision above is the whole of it — with a cursor line on
//! balanced text it behaves as paren mode whenever indent mode would move a
//! paren onto or off the cursor line, and as indent mode otherwise. It never
//! consults the previous buffer state and never restricts the fallback to a
//! single form: the paren-mode pass runs over the whole text.
//!
//! Inert regions are honoured: `"…"` strings with `\` escapes, `\(`-style
//! character literals, and `;` line comments. Brackets inside them are never
//! counted, moved or inserted. All three bracket kinds `()`, `[]` and `{}` are
//! tracked; a bracket closed by the wrong kind is reported in [`Answer::error`]
//! with the text returned unchanged, as is an unterminated string.
//!
//! Columns are counted in characters, so a tab counts as one column — the same
//! simplification `parinfer-rust` makes.

use std::sync::Mutex;

/// Which of the three parinfer modes to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Indentation is authoritative; close-parens are inferred.
    Indent,
    /// Parens are authoritative; indentation is corrected.
    Paren,
    /// Indent mode that falls back to paren mode rather than move a paren onto
    /// or off the cursor line of otherwise balanced text.
    Smart,
}

/// Where the caret is, so the paren trail under it survives editing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Zero-based line the caret is on.
    pub cursor_line: Option<usize>,
    /// Zero-based column of the caret within that line.
    pub cursor_x: Option<usize>,
}

/// The result of one [`process`] run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The processed text, or the input unchanged when `error` is set.
    pub text: String,
    /// The caret column after processing, when the caller supplied one.
    pub cursor_x: Option<usize>,
    /// Set when the text could not be processed; `text` is then the input.
    pub error: Option<String>,
}

/// Run `mode` over `text`. A trailing newline on the input is preserved on the
/// output; no line is ever added or removed.
pub fn process(text: &str, mode: Mode, opts: &Options) -> Answer {
    let had_newline = text.ends_with('\n');
    let body = if had_newline {
        &text[..text.len() - 1]
    } else {
        text
    };
    let lines = scan(body);

    let balanced = match validate(&lines) {
        Ok(b) => b,
        Err(e) => return failed(text, opts, e),
    };

    match mode {
        Mode::Indent => {
            let (out, deltas) = indent_mode(&lines, opts);
            finish(out, deltas, had_newline, opts)
        }
        Mode::Paren => {
            if !balanced {
                return failed(text, opts, "unbalanced parens".to_string());
            }
            let (out, deltas) = paren_mode(&lines);
            finish(out, deltas, had_newline, opts)
        }
        Mode::Smart => {
            let (out, deltas) = indent_mode(&lines, opts);
            let answer = finish(out, deltas, had_newline, opts);
            if balanced && cursor_line_brackets_changed(&lines, &answer.text, opts) {
                let (out, deltas) = paren_mode(&lines);
                return finish(out, deltas, had_newline, opts);
            }
            answer
        }
    }
}

fn failed(text: &str, opts: &Options, error: String) -> Answer {
    Answer {
        text: text.to_string(),
        cursor_x: opts.cursor_x,
        error: Some(error),
    }
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// One input line with each character marked live (bracket-significant) or
/// inert (inside a string, a comment, or escaped).
struct LineScan {
    chars: Vec<char>,
    live: Vec<bool>,
    /// Index of the `;` that starts a line comment, if the line has one.
    comment_at: Option<usize>,
    /// The line begins inside a string opened on an earlier line.
    starts_in_string: bool,
    /// A string is still open at the end of the line.
    ends_in_string: bool,
}

fn scan(text: &str) -> Vec<LineScan> {
    let mut in_string = false;
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let chars: Vec<char> = raw.chars().collect();
        let mut live = vec![false; chars.len()];
        let starts_in_string = in_string;
        let mut comment_at = None;
        let mut escaped = false;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if escaped {
                // The escaped character, `(` in `\(` included, is inert.
                escaped = false;
                i += 1;
                continue;
            }
            match c {
                '\\' => escaped = true,
                '"' => in_string = !in_string,
                ';' if !in_string => {
                    comment_at = Some(i);
                    break;
                }
                _ => live[i] = !in_string,
            }
            i += 1;
        }
        out.push(LineScan {
            chars,
            live,
            comment_at,
            starts_in_string,
            ends_in_string: in_string,
        });
    }
    out
}

fn is_open(c: char) -> bool {
    matches!(c, '(' | '[' | '{')
}

fn is_close(c: char) -> bool {
    matches!(c, ')' | ']' | '}')
}

fn closer_for(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => unreachable!("closer_for on a non-open bracket"),
    }
}

/// Walk every live bracket. `Err` on a bracket closed by the wrong kind or an
/// unterminated string; `Ok(true)` when every bracket pairs up exactly.
fn validate(lines: &[LineScan]) -> Result<bool, String> {
    let mut stack: Vec<char> = Vec::new();
    let mut extra_close = false;
    for (i, l) in lines.iter().enumerate() {
        for (j, &c) in l.chars.iter().enumerate() {
            if !l.live[j] {
                continue;
            }
            if is_open(c) {
                stack.push(c);
            } else if is_close(c) {
                match stack.pop() {
                    Some(o) if closer_for(o) == c => {}
                    Some(o) => {
                        return Err(format!(
                            "line {}: {} opened at depth {} is closed by {}",
                            i + 1,
                            o,
                            stack.len() + 1,
                            c
                        ))
                    }
                    None => extra_close = true,
                }
            }
        }
    }
    if lines.last().is_some_and(|l| l.ends_in_string) {
        return Err("unclosed string".to_string());
    }
    Ok(stack.is_empty() && !extra_close)
}

/// The live bracket characters of one line, in order.
fn line_brackets(l: &LineScan) -> String {
    l.chars
        .iter()
        .enumerate()
        .filter(|(j, c)| l.live[*j] && (is_open(**c) || is_close(**c)))
        .map(|(_, c)| *c)
        .collect()
}

/// Whether indent mode's output changed which brackets sit on the cursor line.
fn cursor_line_brackets_changed(lines: &[LineScan], out_text: &str, opts: &Options) -> bool {
    let Some(l) = opts.cursor_line else {
        return false;
    };
    let out_lines = scan(out_text.strip_suffix('\n').unwrap_or(out_text));
    match (lines.get(l), out_lines.get(l)) {
        (Some(a), Some(b)) => line_brackets(a) != line_brackets(b),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Output lines
// ---------------------------------------------------------------------------

/// One output line, kept in pieces so inferred close-parens can be inserted at
/// the end of the code and before any trailing comment.
#[derive(Default, Clone)]
struct OutLine {
    indent: String,
    code: String,
    /// Close-parens inferred (indent mode) or moved up (paren mode).
    closers: String,
    /// Whitespace between the code and a trailing comment.
    gap: String,
    /// The trailing comment including its `;`, or empty.
    comment: String,
    raw: String,
    changed: bool,
}

impl OutLine {
    fn new(l: &LineScan) -> Self {
        OutLine {
            raw: l.chars.iter().collect(),
            ..Default::default()
        }
    }

    fn render(&self) -> String {
        if !self.changed && self.closers.is_empty() {
            return self.raw.clone();
        }
        if self.code.is_empty() && self.closers.is_empty() && self.comment.is_empty() {
            return String::new();
        }
        let mut s = String::new();
        s.push_str(&self.indent);
        s.push_str(&self.code);
        s.push_str(&self.closers);
        s.push_str(&self.gap);
        s.push_str(&self.comment);
        s
    }
}

/// A line split into indentation, code, the whitespace before a comment, and
/// the comment.
struct Split {
    indent_len: usize,
    /// One past the last code character, trailing whitespace excluded.
    body_end: usize,
    gap: String,
    comment: String,
}

fn split_line(l: &LineScan) -> Split {
    let n = l.chars.len();
    let code_end = l.comment_at.unwrap_or(n);
    let comment: String = l.chars[code_end..].iter().collect();
    let indent_len = l.chars[..code_end]
        .iter()
        .position(|c| !c.is_whitespace())
        .unwrap_or(code_end);
    let mut body_end = code_end;
    while body_end > indent_len && l.chars[body_end - 1].is_whitespace() {
        body_end -= 1;
    }
    let gap: String = l.chars[body_end..code_end].iter().collect();
    Split {
        indent_len,
        body_end,
        gap,
        comment,
    }
}

/// Copy a line that begins inside a multi-line string verbatim: its leading
/// whitespace belongs to the string, so it is never treated as indentation.
/// Returns whether the line can receive inferred close-parens.
fn take_string_line(out: &mut OutLine, l: &LineScan) -> bool {
    let n = l.chars.len();
    let code_end = l.comment_at.unwrap_or(n);
    let mut body_end = code_end;
    while body_end > 0 && l.chars[body_end - 1].is_whitespace() {
        body_end -= 1;
    }
    out.indent = String::new();
    out.code = l.chars[..body_end].iter().collect();
    out.gap = l.chars[body_end..code_end].iter().collect();
    out.comment = l.chars[code_end..].iter().collect();
    out.changed = false;
    !l.ends_in_string && !out.code.is_empty()
}

// ---------------------------------------------------------------------------
// Indent mode
// ---------------------------------------------------------------------------

fn indent_mode(lines: &[LineScan], opts: &Options) -> (Vec<OutLine>, Vec<i64>) {
    let mut out: Vec<OutLine> = lines.iter().map(OutLine::new).collect();
    let deltas = vec![0i64; lines.len()];
    // (open bracket, column)
    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut last_code: Option<usize> = None;

    for (i, l) in lines.iter().enumerate() {
        if l.starts_in_string {
            if take_string_line(&mut out[i], l) {
                last_code = Some(i);
            }
            for (j, &c) in l.chars.iter().enumerate() {
                if !l.live[j] {
                    continue;
                }
                if is_open(c) {
                    stack.push((c, j));
                } else if is_close(c) {
                    stack.pop();
                }
            }
            continue;
        }

        let sp = split_line(l);
        let (start, stripped) = strip_leading_closers(l, sp.indent_len, sp.body_end);

        // A line left with nothing but whitespace never moves any paren.
        if start >= sp.body_end && sp.comment.is_empty() {
            out[i].changed = stripped;
            continue;
        }

        // Close every form whose open bracket sits at or right of this
        // line's indentation.
        let mut closers = String::new();
        while let Some(&(oc, ox)) = stack.last() {
            if ox >= start {
                closers.push(closer_for(oc));
                stack.pop();
            } else {
                break;
            }
        }
        if !closers.is_empty() {
            if let Some(idx) = last_code {
                out[idx].closers.push_str(&closers);
            }
        }

        // The paren trail: the run of close-parens (and the whitespace among
        // them) that ends the code part of the line. It is dropped and
        // re-inferred, except for the part left of the caret.
        let mut trail_start = sp.body_end;
        while trail_start > start {
            let k = trail_start - 1;
            let c = l.chars[k];
            if c.is_whitespace() || (l.live[k] && is_close(c)) {
                trail_start -= 1;
            } else {
                break;
            }
        }
        if !(trail_start..sp.body_end).any(|k| l.live[k] && is_close(l.chars[k])) {
            trail_start = sp.body_end;
        }
        if opts.cursor_line == Some(i) {
            if let Some(cx) = opts.cursor_x {
                if cx > trail_start {
                    trail_start = cx.min(sp.body_end);
                }
            }
        }

        let mut code = String::new();
        let mut dropped = false;
        for k in start..trail_start {
            let c = l.chars[k];
            if l.live[k] && is_open(c) {
                stack.push((c, k));
                code.push(c);
            } else if l.live[k] && is_close(c) {
                match stack.last() {
                    Some(&(oc, _)) if closer_for(oc) == c => {
                        stack.pop();
                        code.push(c);
                    }
                    // validate() already rejected mismatches, so the only
                    // remaining case is a close-paren with nothing open.
                    _ => dropped = true,
                }
            } else {
                code.push(c);
            }
        }
        while code.ends_with(' ') || code.ends_with('\t') {
            code.pop();
        }

        out[i].indent = if stripped {
            " ".repeat(start)
        } else {
            l.chars[..sp.indent_len].iter().collect()
        };
        out[i].code = code;
        out[i].gap = sp.gap;
        out[i].comment = sp.comment;
        out[i].changed = stripped || dropped || trail_start < sp.body_end;
        if !out[i].code.is_empty() {
            last_code = Some(i);
        }
    }

    let mut closers = String::new();
    while let Some((oc, _)) = stack.pop() {
        closers.push(closer_for(oc));
    }
    if !closers.is_empty() {
        if let Some(idx) = last_code {
            out[idx].closers.push_str(&closers);
        }
    }

    (out, deltas)
}

/// Skip the run of close-parens that opens a line's code (and the whitespace
/// after it). Returns the column where the rest of the line starts and whether
/// anything was skipped.
fn strip_leading_closers(l: &LineScan, indent_len: usize, body_end: usize) -> (usize, bool) {
    let mut start = indent_len;
    let mut stripped = false;
    loop {
        let mut k = start;
        while k < body_end && l.chars[k].is_whitespace() {
            k += 1;
        }
        if k < body_end && l.live[k] && is_close(l.chars[k]) {
            start = k + 1;
            stripped = true;
        } else {
            break;
        }
    }
    if stripped {
        while start < body_end && l.chars[start].is_whitespace() {
            start += 1;
        }
    }
    (start, stripped)
}

// ---------------------------------------------------------------------------
// Paren mode
// ---------------------------------------------------------------------------

/// An open bracket on the stack: the column it ends up in and the shift
/// applied to the line that opened it. Paren mode runs only on text
/// [`validate`] found balanced, so the bracket kind never has to be re-checked
/// here.
struct Open {
    x_new: i64,
    delta: i64,
}

fn paren_mode(lines: &[LineScan]) -> (Vec<OutLine>, Vec<i64>) {
    let mut out: Vec<OutLine> = lines.iter().map(OutLine::new).collect();
    let mut deltas = vec![0i64; lines.len()];
    let mut stack: Vec<Open> = Vec::new();
    let mut last_code: Option<usize> = None;

    for (i, l) in lines.iter().enumerate() {
        if l.starts_in_string {
            if take_string_line(&mut out[i], l) {
                last_code = Some(i);
            }
            for (j, &c) in l.chars.iter().enumerate() {
                if !l.live[j] {
                    continue;
                }
                if is_open(c) {
                    stack.push(Open {
                        x_new: j as i64,
                        delta: 0,
                    });
                } else if is_close(c) {
                    stack.pop();
                }
            }
            continue;
        }

        let sp = split_line(l);

        // Leading close-parens belong at the end of the previous code line.
        let mut start = sp.indent_len;
        let mut moved = String::new();
        loop {
            let mut k = start;
            while k < sp.body_end && l.chars[k].is_whitespace() {
                k += 1;
            }
            if k < sp.body_end && l.live[k] && is_close(l.chars[k]) {
                moved.push(l.chars[k]);
                stack.pop();
                start = k + 1;
            } else {
                break;
            }
        }
        let stripped = !moved.is_empty();
        if stripped {
            while start < sp.body_end && l.chars[start].is_whitespace() {
                start += 1;
            }
            if let Some(idx) = last_code {
                out[idx].closers.push_str(&moved);
            }
        }

        if start >= sp.body_end && sp.comment.is_empty() {
            out[i].changed = stripped;
            continue;
        }

        let c = start as i64;
        let new_indent = match stack.last() {
            Some(p) => std::cmp::max(c + p.delta, p.x_new + 1),
            None => c,
        };
        let delta = new_indent - c;
        deltas[i] = delta;

        let mut code = String::new();
        for k in start..sp.body_end {
            let ch = l.chars[k];
            if l.live[k] && is_open(ch) {
                stack.push(Open {
                    x_new: k as i64 + delta,
                    delta,
                });
            } else if l.live[k] && is_close(ch) {
                stack.pop();
            }
            code.push(ch);
        }

        out[i].indent = " ".repeat(new_indent.max(0) as usize);
        out[i].code = code;
        out[i].gap = sp.gap;
        out[i].comment = sp.comment;
        out[i].changed = stripped || delta != 0;
        if !out[i].code.is_empty() {
            last_code = Some(i);
        }
    }

    (out, deltas)
}

// ---------------------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------------------

fn finish(out: Vec<OutLine>, deltas: Vec<i64>, had_newline: bool, opts: &Options) -> Answer {
    let rendered: Vec<String> = out.iter().map(OutLine::render).collect();
    let mut text = rendered.join("\n");
    if had_newline {
        text.push('\n');
    }
    let cursor_x = match (opts.cursor_line, opts.cursor_x) {
        (Some(l), Some(x)) => {
            let d = deltas.get(l).copied().unwrap_or(0);
            let moved = (x as i64 + d).max(0) as usize;
            let len = rendered.get(l).map(|s| s.chars().count()).unwrap_or(moved);
            Some(moved.min(len))
        }
        (_, x) => x,
    };
    Answer {
        text,
        cursor_x,
        error: None,
    }
}

// ---------------------------------------------------------------------------
// Per-buffer mode registry
// ---------------------------------------------------------------------------

/// `doc_key` -> the mode parinfer runs for that buffer. A key that is absent
/// means parinfer is off for the buffer.
static MODES: Mutex<Vec<(usize, Mode)>> = Mutex::new(Vec::new());

/// The mode parinfer is running in for `doc_key`, or `None` when it is off.
pub fn mode_of(doc_key: usize) -> Option<Mode> {
    MODES
        .lock()
        .unwrap()
        .iter()
        .find(|(k, _)| *k == doc_key)
        .map(|(_, m)| *m)
}

/// Turn parinfer on for `doc_key` in `mode`, replacing any current mode.
pub fn set_mode(doc_key: usize, mode: Mode) {
    let mut modes = MODES.lock().unwrap();
    match modes.iter_mut().find(|(k, _)| *k == doc_key) {
        Some(entry) => entry.1 = mode,
        None => modes.push((doc_key, mode)),
    }
}

/// Turn parinfer off for `doc_key`.
pub fn disable(doc_key: usize) {
    MODES.lock().unwrap().retain(|(k, _)| *k != doc_key);
}

/// Cycle `doc_key` through Indent -> Paren -> Smart -> Indent, the order
/// `parinfer-rust-mode` switches modes in. Turning parinfer on for a buffer
/// that did not have it starts in [`Mode::Smart`], `parinfer-rust-mode`'s
/// documented default. Returns the mode now in effect.
pub fn toggle_mode(doc_key: usize) -> Mode {
    let next = match mode_of(doc_key) {
        None => Mode::Smart,
        Some(Mode::Indent) => Mode::Paren,
        Some(Mode::Paren) => Mode::Smart,
        Some(Mode::Smart) => Mode::Indent,
    };
    set_mode(doc_key, next);
    next
}

/// Flip `doc_key` between [`Mode::Smart`] and [`Mode::Paren`] only — what
/// spacemacs binds to `SPC t P`. From [`Mode::Indent`], or from parinfer being
/// off, this lands on [`Mode::Paren`] and [`Mode::Smart`] respectively.
/// Returns the mode now in effect.
pub fn toggle_smart_paren(doc_key: usize) -> Mode {
    let next = match mode_of(doc_key) {
        None => Mode::Smart,
        Some(Mode::Paren) => Mode::Smart,
        Some(_) => Mode::Paren,
    };
    set_mode(doc_key, next);
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, mode: Mode) -> String {
        let a = process(text, mode, &Options::default());
        assert_eq!(a.error, None, "unexpected error for {text:?}");
        a.text
    }

    /// Every character that is not a bracket and not indentation, in order.
    fn payload(text: &str) -> String {
        text.lines()
            .map(|l| {
                l.trim_start()
                    .chars()
                    .filter(|c| !is_open(*c) && !is_close(*c))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn indent_mode_closes_a_form_when_a_child_line_is_dedented() {
        assert_eq!(run("(foo\nbar)\n", Mode::Indent), "(foo)\nbar\n");
    }

    #[test]
    fn indent_mode_opens_a_form_when_a_line_is_indented_under_it() {
        assert_eq!(run("(foo)\n  bar\n", Mode::Indent), "(foo\n  bar)\n");
    }

    #[test]
    fn indent_mode_closes_at_the_last_line_of_the_form() {
        let src = "(defn f\n  [a b]\n  (+ a b\n";
        assert_eq!(run(src, Mode::Indent), "(defn f\n  [a b]\n  (+ a b))\n");
    }

    #[test]
    fn indent_mode_keeps_close_parens_inside_a_line() {
        // The `)` after `b` is followed by code, so it is not part of the
        // paren trail and survives untouched.
        assert_eq!(run("(a (b) c\n", Mode::Indent), "(a (b) c)\n");
    }

    #[test]
    fn indent_mode_removes_an_unmatched_close_paren() {
        assert_eq!(run("foo)\n", Mode::Indent), "foo\n");
        assert_eq!(run("(a) b)\n", Mode::Indent), "(a) b\n");
    }

    #[test]
    fn indent_mode_pulls_up_a_dangling_close_paren_line() {
        assert_eq!(run("(foo\n  bar\n  )\n", Mode::Indent), "(foo\n  bar)\n\n");
    }

    #[test]
    fn paren_mode_reindents_an_underindented_balanced_form() {
        // Paren mode clamps a child line to one column right of its parent
        // open paren; it does not otherwise move indentation.
        assert_eq!(
            run("(defn f [a b]\n(+ a b))\n", Mode::Paren),
            "(defn f [a b]\n (+ a b))\n"
        );
    }

    #[test]
    fn paren_mode_keeps_relative_indentation_when_a_parent_shifts() {
        let src = "(foo (bar\n   baz))\n";
        assert_eq!(run(src, Mode::Paren), "(foo (bar\n      baz))\n");
    }

    #[test]
    fn paren_mode_refuses_unbalanced_text() {
        let a = process("(foo\n", Mode::Paren, &Options::default());
        assert_eq!(a.text, "(foo\n");
        assert_eq!(a.error.as_deref(), Some("unbalanced parens"));
    }

    #[test]
    fn strings_with_unbalanced_parens_are_left_alone() {
        let src = "(println \"a ) b (\")\n";
        assert_eq!(run(src, Mode::Indent), src);
        assert_eq!(run(src, Mode::Paren), src);
    }

    #[test]
    fn multi_line_strings_keep_their_own_indentation() {
        let src = "(def s \"line one\n     line two\")\n";
        assert_eq!(run(src, Mode::Indent), src);
        assert_eq!(run(src, Mode::Paren), src);
    }

    #[test]
    fn comments_with_parens_are_left_alone() {
        let src = "(foo) ; a ) b ( c\n";
        assert_eq!(run(src, Mode::Indent), src);
    }

    #[test]
    fn inferred_close_parens_land_before_a_trailing_comment() {
        assert_eq!(
            run("(foo)\n  bar ; note\n", Mode::Indent),
            "(foo\n  bar) ; note\n"
        );
    }

    #[test]
    fn escaped_character_literals_are_not_brackets() {
        let src = "(list \\( \\))\n";
        assert_eq!(run(src, Mode::Indent), src);
        assert_eq!(run(src, Mode::Paren), src);
    }

    #[test]
    fn square_and_curly_brackets_are_inferred_too() {
        assert_eq!(run("[1 2\n 3\n", Mode::Indent), "[1 2\n 3]\n");
        assert_eq!(run("{:a 1\n :b 2\n", Mode::Indent), "{:a 1\n :b 2}\n");
        assert_eq!(
            run("(f [a\n    b] {:c 1\n", Mode::Indent),
            "(f [a\n    b] {:c 1})\n"
        );
    }

    #[test]
    fn a_mismatched_bracket_is_an_error_and_changes_nothing() {
        for mode in [Mode::Indent, Mode::Paren, Mode::Smart] {
            let a = process("(foo]\n", mode, &Options::default());
            assert_eq!(a.text, "(foo]\n", "{mode:?}");
            assert!(a.error.is_some(), "{mode:?} accepted a mismatched bracket");
        }
    }

    #[test]
    fn an_unclosed_string_is_an_error_and_changes_nothing() {
        let a = process("(foo \"bar\n", Mode::Indent, &Options::default());
        assert_eq!(a.text, "(foo \"bar\n");
        assert_eq!(a.error.as_deref(), Some("unclosed string"));
    }

    #[test]
    fn every_mode_is_idempotent() {
        let cases = [
            "(foo\nbar)\n",
            "(foo)\n  bar\n",
            "(defn f\n  [a b]\n  (+ a b\n",
            "(foo (bar\n   baz))\n",
            "(a) b)\n",
            "(def s \"x\n y\")\n",
        ];
        for src in cases {
            for mode in [Mode::Indent, Mode::Paren, Mode::Smart] {
                let a = process(src, mode, &Options::default());
                if a.error.is_some() {
                    continue;
                }
                let b = process(&a.text, mode, &Options::default());
                assert_eq!(a.text, b.text, "{mode:?} not idempotent on {src:?}");
            }
        }
    }

    #[test]
    fn text_without_parens_comes_back_byte_identical() {
        let src = "hello world\n  indented text\n\n   \nlast line\n";
        for mode in [Mode::Indent, Mode::Paren, Mode::Smart] {
            assert_eq!(run(src, mode), src, "{mode:?}");
        }
        assert_eq!(
            run("no trailing newline", Mode::Indent),
            "no trailing newline"
        );
        assert_eq!(run("", Mode::Indent), "");
    }

    #[test]
    fn non_bracket_text_is_never_lost() {
        let cases = [
            "(defn f\n  [a b] ; add\n  (+ a b)\n",
            "(a) b)\n",
            "(foo)\n    bar\nbaz\n",
            "(def s \"a ) b\"\n  more)\n",
        ];
        for src in cases {
            for mode in [Mode::Indent, Mode::Paren, Mode::Smart] {
                let a = process(src, mode, &Options::default());
                if a.error.is_some() {
                    continue;
                }
                assert_eq!(
                    payload(&a.text),
                    payload(src),
                    "{mode:?} changed the text of {src:?}"
                );
            }
        }
    }

    #[test]
    fn a_trailing_newline_is_preserved_either_way() {
        assert_eq!(run("(foo)\n  bar\n", Mode::Indent), "(foo\n  bar)\n");
        assert_eq!(run("(foo)\n  bar", Mode::Indent), "(foo\n  bar)");
    }

    #[test]
    fn the_caret_protects_the_paren_trail_it_sits_in() {
        let opts = Options {
            cursor_line: Some(0),
            cursor_x: Some(5),
        };
        // Without the caret this becomes "(foo\n  bar)".
        let a = process("(foo)\n  bar\n", Mode::Indent, &opts);
        assert_eq!(a.text, "(foo)\n  bar\n");
        assert_eq!(a.cursor_x, Some(5));
    }

    #[test]
    fn smart_mode_keeps_structure_when_indent_mode_would_move_a_paren() {
        let opts = Options {
            cursor_line: Some(1),
            cursor_x: Some(2),
        };
        let src = "(a)\n  b\n";
        // Indent mode moves the close paren onto the cursor line.
        assert_eq!(process(src, Mode::Indent, &opts).text, "(a\n  b)\n");
        // Smart mode refuses to create a paren the user did not type.
        assert_eq!(process(src, Mode::Smart, &opts).text, src);
    }

    #[test]
    fn smart_mode_is_indent_mode_when_the_cursor_line_is_unaffected() {
        let opts = Options {
            cursor_line: Some(1),
            cursor_x: Some(3),
        };
        let src = "(a\n  b)\nc\n";
        assert_eq!(process(src, Mode::Smart, &opts).text, src);
        // With no cursor at all smart mode is plain indent mode.
        assert_eq!(run("(a)\n  b\n", Mode::Smart), "(a\n  b)\n");
    }

    #[test]
    fn mode_registry_toggles_and_clears() {
        let key = 0xbeef;
        disable(key);
        assert_eq!(mode_of(key), None);
        // Turning it on starts in Smart.
        assert_eq!(toggle_mode(key), Mode::Smart);
        assert_eq!(toggle_mode(key), Mode::Indent);
        assert_eq!(toggle_mode(key), Mode::Paren);
        assert_eq!(toggle_mode(key), Mode::Smart);
        assert_eq!(mode_of(key), Some(Mode::Smart));
        // SPC t P flips Smart <-> Paren only.
        assert_eq!(toggle_smart_paren(key), Mode::Paren);
        assert_eq!(toggle_smart_paren(key), Mode::Smart);
        set_mode(key, Mode::Indent);
        assert_eq!(toggle_smart_paren(key), Mode::Paren);
        disable(key);
        assert_eq!(mode_of(key), None);
        assert_eq!(toggle_smart_paren(key), Mode::Smart);
        disable(key);
    }

    #[test]
    fn registry_keys_are_independent() {
        disable(1);
        disable(2);
        set_mode(1, Mode::Paren);
        set_mode(2, Mode::Indent);
        assert_eq!(mode_of(1), Some(Mode::Paren));
        assert_eq!(mode_of(2), Some(Mode::Indent));
        disable(1);
        assert_eq!(mode_of(1), None);
        assert_eq!(mode_of(2), Some(Mode::Indent));
        disable(2);
    }
}
