//! Pure line/column logic for GNU Emacs `fortran-mode` (fixed-form) and
//! `f90-mode` (free-form).
//!
//! Fixed-form Fortran is column-sensitive: columns 1-5 hold an optional
//! statement label, column 6 holds a continuation marker, columns 7-72 hold the
//! code, and columns 73+ hold sequence numbers. A `C`, `c`, or `*` in column 1
//! marks a comment line (Emacs also treats a leading `!` as a comment line).
//! Free-form Fortran (`.f90`) is not column-sensitive: `!` starts a comment
//! anywhere, a trailing `&` continues onto the next line, and `;` separates
//! statements.
//!
//! Everything here is pure (operates on borrowed `&str` lines and returns line
//! indices or new `String`s) so the classification, motion, block-matching and
//! editing logic is unit tested without an editor. The behaviour mirrors the
//! documented algorithms of GNU Emacs 30.x `fortran.el` / `f90.el`; where a
//! construct's detection is deliberately restricted to the common forms that is
//! called out on the relevant function.

/// Classification of a fixed-form Fortran source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedLine {
    /// A comment line: column 1 is `C`, `c`, `*`, or `!`.
    Comment,
    /// A line that is empty or only whitespace.
    Blank,
    /// A continuation line: column 6 holds a non-blank, non-`0` marker.
    Continuation,
    /// An initial (non-continuation) line of code.
    Statement,
}

/// True when `line` is a fixed-form comment line (`C`/`c`/`*`/`!` in column 1).
pub fn is_fixed_comment(line: &str) -> bool {
    matches!(line.chars().next(), Some('C' | 'c' | '*' | '!'))
}

/// True when `line` is a fixed-form continuation line: it is not a comment and
/// its 6th column holds a character other than a space or `0`.
pub fn is_fixed_continuation(line: &str) -> bool {
    if is_fixed_comment(line) {
        return false;
    }
    match line.chars().nth(5) {
        Some(c) => c != ' ' && c != '0' && c != '\t',
        None => false,
    }
}

/// Classify a fixed-form source line.
pub fn classify_fixed(line: &str) -> FixedLine {
    if is_fixed_comment(line) {
        FixedLine::Comment
    } else if line.trim().is_empty() {
        FixedLine::Blank
    } else if is_fixed_continuation(line) {
        FixedLine::Continuation
    } else {
        FixedLine::Statement
    }
}

/// True when `line` does not begin a statement of its own — a comment, a blank
/// line, or a continuation of the previous statement.
fn fixed_is_skippable(line: &str) -> bool {
    matches!(
        classify_fixed(line),
        FixedLine::Comment | FixedLine::Blank | FixedLine::Continuation
    )
}

/// Emacs `fortran-next-statement`: return the index of the initial line of the
/// next statement after `cur`, skipping the current statement's remaining
/// continuation lines and any intervening comment/blank lines. Returns `None`
/// at end of buffer.
pub fn fortran_next_statement(lines: &[&str], cur: usize) -> Option<usize> {
    let mut i = cur + 1;
    while i < lines.len() && fixed_is_skippable(lines[i]) {
        i += 1;
    }
    (i < lines.len()).then_some(i)
}

/// Emacs `fortran-previous-statement`: return the index of the initial line of
/// the statement before `cur`. From a continuation line this is the start of the
/// current statement; from a statement start it is the previous statement.
/// Returns `None` before the first statement.
pub fn fortran_previous_statement(lines: &[&str], cur: usize) -> Option<usize> {
    if cur == 0 {
        return None;
    }
    let mut i = cur - 1;
    loop {
        if !fixed_is_skippable(lines[i]) {
            return Some(i);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

/// Whether a code line opens a block, closes one, or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// Opens a block (`DO`, `IF (...) THEN`, `SUBROUTINE`, ...).
    Start,
    /// Closes a block (`END`, `ENDDO`, `END IF`, ...).
    End,
    /// Neither opens nor closes a block.
    Neither,
}

/// Block keywords that both open a construct and appear after `END` to close it.
const BLOCK_WORDS: &[&str] = &[
    "do",
    "if",
    "subroutine",
    "function",
    "program",
    "module",
    "interface",
    "block",
    "associate",
    "select",
    "where",
    "forall",
    "type",
];

/// Reduce a source line to its significant lowercased code: strip a trailing
/// comment, trim surrounding whitespace, drop a leading numeric statement label,
/// and (for continuation lines) the marker column.
fn code_of(line: &str, fixed: bool) -> String {
    // Drop a trailing `!...` comment (ignoring `!` inside quotes).
    let mut end = line.len();
    let mut quote: Option<char> = None;
    for (idx, ch) in line.char_indices() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => {
                if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                } else if ch == '!' {
                    end = idx;
                    break;
                }
            }
        }
    }
    let mut code = &line[..end];
    // For a fixed-form continuation line, the code proper starts after column 6.
    if fixed && is_fixed_continuation(line) && code.len() > 6 {
        code = &code[6..];
    }
    let trimmed = code.trim();
    // Strip a leading statement label (digits) and following whitespace.
    let after_label = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
    after_label.trim_start().to_ascii_lowercase()
}

/// The first whitespace/`(`-delimited word of `code`.
fn first_word(code: &str) -> &str {
    code.split(|c: char| c.is_whitespace() || c == '(')
        .next()
        .unwrap_or("")
}

/// True when `code` contains `word` as a standalone token.
fn contains_word(code: &str, word: &str) -> bool {
    code.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|w| w == word)
}

/// Classify already-reduced `code` (see [`code_of`]) as a block delimiter.
fn classify_code(code: &str) -> BlockKind {
    if code.is_empty() {
        return BlockKind::Neither;
    }
    // END-family: "end", "enddo", "end if", "end subroutine foo", ...
    if let Some(rest) = code.strip_prefix("end") {
        let rest = rest.trim_start();
        if rest.is_empty() || BLOCK_WORDS.contains(&first_word(rest)) {
            return BlockKind::End;
        }
    }
    let word = first_word(code);
    match word {
        "do" | "subroutine" | "function" | "program" | "module" | "interface" | "associate"
        | "select" | "block" => BlockKind::Start,
        // IF / ELSE IF open a block only in the `... THEN` form.
        "if" if code.trim_end().ends_with("then") => BlockKind::Start,
        _ => {
            // Definitions may carry prefixes: "recursive function foo",
            // "pure elemental subroutine bar", "integer function baz".
            if contains_word(code, "subroutine") || contains_word(code, "function") {
                BlockKind::Start
            } else {
                BlockKind::Neither
            }
        }
    }
}

/// Whether a fixed-form line opens/closes/neither a block.
pub fn fixed_block_kind(line: &str) -> BlockKind {
    match classify_fixed(line) {
        FixedLine::Comment | FixedLine::Blank | FixedLine::Continuation => BlockKind::Neither,
        FixedLine::Statement => classify_code(&code_of(line, true)),
    }
}

/// Whether a free-form line opens/closes/neither a block.
pub fn free_block_kind(line: &str) -> BlockKind {
    if is_free_comment(line) {
        return BlockKind::Neither;
    }
    classify_code(&code_of(line, false))
}

/// Forward scan matching the block enclosing (or opened at) `cur`, using
/// `kind` to classify each line. Returns the index of the matching END line.
fn end_of_block_generic(
    lines: &[&str],
    cur: usize,
    kind: impl Fn(&str) -> BlockKind,
) -> Option<usize> {
    let mut depth: i32 = 0;
    for (i, line) in lines.iter().enumerate().skip(cur + 1) {
        match kind(line) {
            BlockKind::Start => depth += 1,
            BlockKind::End => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            BlockKind::Neither => {}
        }
    }
    None
}

/// Backward scan matching the block enclosing (or closed at) `cur`. Returns the
/// index of the matching Start line.
fn beginning_of_block_generic(
    lines: &[&str],
    cur: usize,
    kind: impl Fn(&str) -> BlockKind,
) -> Option<usize> {
    if cur == 0 {
        return None;
    }
    let mut depth: i32 = 0;
    let mut i = cur - 1;
    loop {
        match kind(lines[i]) {
            BlockKind::End => depth += 1,
            BlockKind::Start => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            BlockKind::Neither => {}
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

/// Emacs `fortran-end-of-block`: index of the `END` matching the block at (or
/// enclosing) `cur`.
pub fn fortran_end_of_block(lines: &[&str], cur: usize) -> Option<usize> {
    end_of_block_generic(lines, cur, fixed_block_kind)
}

/// Emacs `fortran-beginning-of-block`: index of the block-opening statement
/// matching (or enclosing) `cur`.
pub fn fortran_beginning_of_block(lines: &[&str], cur: usize) -> Option<usize> {
    beginning_of_block_generic(lines, cur, fixed_block_kind)
}

// ---------------------------------------------------------------------------
// Free-form (f90)
// ---------------------------------------------------------------------------

/// True when a free-form line is empty or a pure comment (first non-blank is
/// `!`).
pub fn is_free_comment(line: &str) -> bool {
    matches!(line.trim_start().chars().next(), Some('!'))
}

/// True when `line` is blank or a pure comment (skipped by statement motion).
fn free_is_skippable(line: &str) -> bool {
    line.trim().is_empty() || is_free_comment(line)
}

/// True when a free-form line continues onto the next line: after stripping a
/// trailing comment and whitespace, its last character is `&`.
pub fn free_continues(line: &str) -> bool {
    // Reuse code_of to drop the trailing comment, but keep the `&`.
    let mut end = line.len();
    let mut quote: Option<char> = None;
    for (idx, ch) in line.char_indices() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => {
                if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                } else if ch == '!' {
                    end = idx;
                    break;
                }
            }
        }
    }
    line[..end].trim_end().ends_with('&')
}

/// Emacs `f90-next-statement`: index of the start of the next statement after
/// `cur`, honouring `&` continuation and skipping blank/comment lines.
pub fn f90_next_statement(lines: &[&str], cur: usize) -> Option<usize> {
    // Advance past the rest of the current statement's continuation lines.
    let mut i = cur;
    while i < lines.len() && free_continues(lines[i]) {
        i += 1;
    }
    i += 1;
    while i < lines.len() && free_is_skippable(lines[i]) {
        i += 1;
    }
    (i < lines.len()).then_some(i)
}

/// Emacs `f90-previous-statement`: index of the start of the statement before
/// `cur`, honouring `&` continuation and skipping blank/comment lines.
pub fn f90_previous_statement(lines: &[&str], cur: usize) -> Option<usize> {
    if cur == 0 {
        return None;
    }
    let mut i = cur - 1;
    // Skip blank/comment lines back to the previous statement's last line.
    while free_is_skippable(lines[i]) {
        if i == 0 {
            return None;
        }
        i -= 1;
    }
    // Walk back to the beginning of that statement's continuation chain.
    while i > 0 && free_continues(lines[i - 1]) {
        i -= 1;
    }
    Some(i)
}

/// Emacs `f90-end-of-block`: index of the `end` matching the block at/enclosing
/// `cur`.
pub fn f90_end_of_block(lines: &[&str], cur: usize) -> Option<usize> {
    end_of_block_generic(lines, cur, free_block_kind)
}

/// Emacs `f90-beginning-of-block`: index of the block-opening line matching/
/// enclosing `cur`.
pub fn f90_beginning_of_block(lines: &[&str], cur: usize) -> Option<usize> {
    beginning_of_block_generic(lines, cur, free_block_kind)
}

/// Emacs `f90-next-block`: index of the next block-opening line after `cur`.
pub fn f90_next_block(lines: &[&str], cur: usize) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(cur + 1)
        .find(|(_, l)| free_block_kind(l) == BlockKind::Start)
        .map(|(i, _)| i)
}

/// Emacs `f90-previous-block`: index of the previous block-opening line before
/// `cur`.
pub fn f90_previous_block(lines: &[&str], cur: usize) -> Option<usize> {
    if cur == 0 {
        return None;
    }
    (0..cur)
        .rev()
        .find(|&i| free_block_kind(lines[i]) == BlockKind::Start)
}

// ---------------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------------

/// Default fixed-form continuation marker (Emacs `fortran-continuation-string`).
pub const CONTINUATION_CHAR: char = '$';

/// Emacs `fortran-split-line`: break `line` at column `col` (a character index),
/// returning the text before point and a new continuation line carrying the
/// remainder. The new line has five spaces (columns 1-5), the continuation
/// marker in column 6, then the trailing code.
pub fn split_line(line: &str, col: usize, cont: char) -> (String, String) {
    // `col` is a character index; convert to a byte offset on a char boundary.
    let idx = line
        .char_indices()
        .nth(col)
        .map(|(b, _)| b)
        .unwrap_or(line.len());
    let first = line[..idx].to_string();
    let rest = line[idx..].trim_start();
    let second = format!("     {cont}{rest}");
    (first, second)
}

/// Emacs `fortran-join-line`: join `first` with its following continuation line
/// `cont_line`, dropping the label/marker columns (1-6) of the continuation line.
pub fn join_continuation(first: &str, cont_line: &str) -> String {
    // Skip up to the first 6 columns (label field + continuation marker).
    let tail: String = cont_line.chars().skip(6).collect();
    let code = tail.trim_start();
    format!("{}{}", first.trim_end(), code)
}

/// Default Emacs `fortran-comment-region` marker.
pub const COMMENT_REGION_MARKER: &str = "c$$$";

/// True when `line` is already commented with `marker`.
pub fn is_commented(line: &str, marker: &str) -> bool {
    line.starts_with(marker)
}

/// Prefix `line` with the comment `marker` (Emacs `fortran-comment-region`).
pub fn comment_region_line(line: &str, marker: &str) -> String {
    format!("{marker}{line}")
}

/// Remove a leading comment `marker` from `line`, if present (the inverse of
/// [`comment_region_line`]).
pub fn uncomment_region_line(line: &str, marker: &str) -> String {
    line.strip_prefix(marker).unwrap_or(line).to_string()
}

/// Emacs `fortran-strip-sequence-nos`: delete the sequence-number field in
/// columns 73+ (character indices 72+), returning columns 1-72.
pub fn strip_sequence_nos(line: &str) -> String {
    line.chars().take(72).collect()
}

/// True when reduced `code` is a mid-block keyword (`else`, `else if`, `case`,
/// `contains`) that dedents itself but does not change block depth.
fn is_block_mid(code: &str) -> bool {
    matches!(first_word(code), "else" | "elseif" | "case" | "contains")
}

/// Minimum fixed-form statement indent (Emacs `fortran-minimum-statement-indent-fixed`:
/// code begins in column 7, i.e. six leading blanks).
const MIN_STATEMENT_INDENT: usize = 6;
/// Per-nesting-level indent (Emacs `fortran-do-indent` / `fortran-if-indent`).
const BLOCK_INDENT: usize = 3;

/// Pure fixed-form re-indent of a subprogram (approximates
/// `fortran-indent-subprogram`). Comment and continuation lines are left as-is;
/// each initial statement line is re-indented by block nesting depth, keeping a
/// numeric label left-justified in columns 1-5 and code at column
/// `7 + depth*3`. This reproduces the block-nesting indentation of
/// `fortran-indent-line` but not its every corner case.
pub fn indent_subprogram(lines: &[&str]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut depth: i32 = 0;
    for line in lines {
        match classify_fixed(line) {
            FixedLine::Comment | FixedLine::Blank | FixedLine::Continuation => {
                out.push((*line).to_string());
                continue;
            }
            FixedLine::Statement => {}
        }
        let code = code_of(line, true);
        let kind = classify_code(&code);
        // A line's own indent dedents for END and mid-block keywords.
        let own_depth = if kind == BlockKind::End || is_block_mid(&code) {
            (depth - 1).max(0)
        } else {
            depth.max(0)
        };
        // Preserve a leading numeric label.
        let trimmed = line.trim_start();
        let label: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        let body = trimmed
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start();
        let indent = MIN_STATEMENT_INDENT + own_depth as usize * BLOCK_INDENT;
        let rebuilt = if label.is_empty() {
            format!("{}{}", " ".repeat(indent), body)
        } else {
            // Label left-justified in columns 1-5, code padded to `indent`.
            let pad = indent.saturating_sub(label.len());
            format!("{}{}{}", label, " ".repeat(pad.max(1)), body)
        };
        out.push(rebuilt);
        match kind {
            BlockKind::Start => depth += 1,
            BlockKind::End => depth = (depth - 1).max(0),
            BlockKind::Neither => {}
        }
    }
    out
}

/// Emacs `fortran-column-ruler` header (`fortran-column-ruler-fixed`): the
/// column-tens guide for fixed-form source. Column 1 begins the label field,
/// column 6 is the continuation column, columns 7-72 are code, and 73+ are
/// sequence numbers.
pub const FORTRAN_COLUMN_RULER: &str =
    "0   4 6  10        20        30        40        50        60        70";

/// A numeric 1-72 ruler line matching [`FORTRAN_COLUMN_RULER`], repeating the
/// digit pattern so each column's ones digit is visible.
pub fn fortran_column_ruler_numeric() -> String {
    (1..=72)
        .map(|c| char::from(b'0' + (c % 10) as u8))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<&str> {
        s.lines().collect()
    }

    #[test]
    fn classify_fixed_lines() {
        assert_eq!(classify_fixed("C this is a comment"), FixedLine::Comment);
        assert_eq!(classify_fixed("c lower comment"), FixedLine::Comment);
        assert_eq!(classify_fixed("* star comment"), FixedLine::Comment);
        assert_eq!(classify_fixed("! bang comment"), FixedLine::Comment);
        assert_eq!(classify_fixed("      x = 1"), FixedLine::Statement);
        assert_eq!(classify_fixed("     $  y = 2"), FixedLine::Continuation);
        assert_eq!(classify_fixed("     0  z = 3"), FixedLine::Statement); // '0' is not cont
        assert_eq!(classify_fixed("   "), FixedLine::Blank);
        assert_eq!(classify_fixed(""), FixedLine::Blank);
        assert_eq!(classify_fixed("  100 continue"), FixedLine::Statement);
    }

    #[test]
    fn next_and_previous_statement_skip_continuation_and_comments() {
        // 0:      program p
        // 1: C     a comment
        // 2:       x = 1 +
        // 3:      $     2
        // 4:
        // 5:       y = 2
        // 6:       end
        let src = "      program p\n\
                   C     a comment\n\
                   \x20     x = 1 +\n\
                   \x20    $     2\n\
                   \n\
                   \x20     y = 2\n\
                   \x20     end";
        let l = lines(src);
        // From the program line, the next statement skips the comment.
        assert_eq!(fortran_next_statement(&l, 0), Some(2));
        // From the `x = 1 +` line, next skips its continuation and the blank.
        assert_eq!(fortran_next_statement(&l, 2), Some(5));
        // From the continuation line, next still lands on `y = 2`.
        assert_eq!(fortran_next_statement(&l, 3), Some(5));
        // Previous from `y = 2` goes back to the start of the x statement.
        assert_eq!(fortran_previous_statement(&l, 5), Some(2));
        // Previous from a continuation line goes to its statement head.
        assert_eq!(fortran_previous_statement(&l, 3), Some(2));
        // Previous from the first statement is None.
        assert_eq!(fortran_previous_statement(&l, 0), None);
        // Next past the end is None.
        assert_eq!(fortran_next_statement(&l, 6), None);
    }

    #[test]
    fn fixed_block_matching() {
        // 0:       subroutine s
        // 1:       do 10 i = 1, n
        // 2:       if (x .gt. 0) then
        // 3:       y = 1
        // 4:       endif
        // 5: 10    continue           <- not a block end
        // 6:       enddo
        // 7:       end
        let src = "      subroutine s\n\
                   \x20     do 10 i = 1, n\n\
                   \x20     if (x .gt. 0) then\n\
                   \x20     y = 1\n\
                   \x20     endif\n\
                   10    continue\n\
                   \x20     enddo\n\
                   \x20     end";
        let l = lines(src);
        assert_eq!(fixed_block_kind(l[0]), BlockKind::Start);
        assert_eq!(fixed_block_kind(l[1]), BlockKind::Start);
        assert_eq!(fixed_block_kind(l[2]), BlockKind::Start);
        assert_eq!(fixed_block_kind(l[4]), BlockKind::End);
        assert_eq!(fixed_block_kind(l[5]), BlockKind::Neither); // continue
        assert_eq!(fixed_block_kind(l[6]), BlockKind::End);
        assert_eq!(fixed_block_kind(l[7]), BlockKind::End);
        // if...then at 2 matches endif at 4.
        assert_eq!(fortran_end_of_block(&l, 2), Some(4));
        assert_eq!(fortran_beginning_of_block(&l, 4), Some(2));
        // do at 1 matches enddo at 6 (nested if is balanced).
        assert_eq!(fortran_end_of_block(&l, 1), Some(6));
        assert_eq!(fortran_beginning_of_block(&l, 6), Some(1));
        // subroutine at 0 matches end at 7.
        assert_eq!(fortran_end_of_block(&l, 0), Some(7));
        assert_eq!(fortran_beginning_of_block(&l, 7), Some(0));
        // From inside the do body (line 3), the enclosing end is enddo.
        assert_eq!(fortran_end_of_block(&l, 3), Some(4)); // innermost = endif
    }

    #[test]
    fn logical_if_is_not_a_block() {
        assert_eq!(
            fixed_block_kind("      if (x .eq. 1) y = 2"),
            BlockKind::Neither
        );
        assert_eq!(
            fixed_block_kind("      if (x .eq. 1) then"),
            BlockKind::Start
        );
        // ENDFILE must not be mistaken for a block END.
        assert_eq!(fixed_block_kind("      endfile 7"), BlockKind::Neither);
    }

    #[test]
    fn function_prefixes_detected() {
        assert_eq!(
            fixed_block_kind("      recursive function fib(n)"),
            BlockKind::Start
        );
        assert_eq!(
            fixed_block_kind("      integer function g(x)"),
            BlockKind::Start
        );
        assert_eq!(fixed_block_kind("      end function"), BlockKind::End);
    }

    #[test]
    fn f90_statement_motion() {
        // 0: program p
        // 1: ! comment
        // 2: x = 1 + &
        // 3:     2
        // 4:
        // 5: y = 2
        // 6: end program p
        let src = "program p\n\
                   ! comment\n\
                   x = 1 + &\n\
                   \x20    2\n\
                   \n\
                   y = 2\n\
                   end program p";
        let l = lines(src);
        assert_eq!(f90_next_statement(&l, 0), Some(2));
        assert_eq!(f90_next_statement(&l, 2), Some(5)); // skip continuation + blank
        assert_eq!(f90_next_statement(&l, 3), Some(5));
        assert_eq!(f90_previous_statement(&l, 5), Some(2));
        assert_eq!(f90_previous_statement(&l, 3), Some(2)); // to head of chain
        assert_eq!(f90_previous_statement(&l, 0), None);
        assert_eq!(f90_next_statement(&l, 6), None);
    }

    #[test]
    fn f90_block_matching() {
        // 0: subroutine s
        // 1:   do i = 1, n
        // 2:     if (x > 0) then
        // 3:       y = 1
        // 4:     end if
        // 5:   end do
        // 6: end subroutine s
        let src = "subroutine s\n\
                   \x20 do i = 1, n\n\
                   \x20   if (x > 0) then\n\
                   \x20     y = 1\n\
                   \x20   end if\n\
                   \x20 end do\n\
                   end subroutine s";
        let l = lines(src);
        assert_eq!(free_block_kind(l[2]), BlockKind::Start);
        assert_eq!(free_block_kind(l[4]), BlockKind::End);
        assert_eq!(f90_end_of_block(&l, 2), Some(4));
        assert_eq!(f90_beginning_of_block(&l, 4), Some(2));
        assert_eq!(f90_end_of_block(&l, 1), Some(5));
        assert_eq!(f90_beginning_of_block(&l, 5), Some(1));
        assert_eq!(f90_end_of_block(&l, 0), Some(6));
        // next/previous block walk between block openers.
        assert_eq!(f90_next_block(&l, 0), Some(1));
        assert_eq!(f90_next_block(&l, 1), Some(2));
        assert_eq!(f90_next_block(&l, 2), None);
        assert_eq!(f90_previous_block(&l, 6), Some(2));
        assert_eq!(f90_previous_block(&l, 2), Some(1));
        assert_eq!(f90_previous_block(&l, 1), Some(0));
        assert_eq!(f90_previous_block(&l, 0), None);
    }

    #[test]
    fn split_line_inserts_continuation() {
        let (a, b) = split_line("      x = alpha + beta", 15, CONTINUATION_CHAR);
        assert_eq!(a, "      x = alpha");
        assert_eq!(b, "     $+ beta");
    }

    #[test]
    fn join_line_removes_marker() {
        let joined = join_continuation("      x = alpha", "     $+ beta");
        assert_eq!(joined, "      x = alpha+ beta");
        // Split then join is a round trip (modulo the space we trimmed).
        let (a, b) = split_line("      x = alpha +beta", 16, CONTINUATION_CHAR);
        assert_eq!(a, "      x = alpha ");
        assert_eq!(b, "     $+beta");
        assert_eq!(join_continuation(&a, &b), "      x = alpha+beta");
    }

    #[test]
    fn comment_region_roundtrip() {
        let l = "      x = 1";
        let c = comment_region_line(l, COMMENT_REGION_MARKER);
        assert_eq!(c, "c$$$      x = 1");
        assert!(is_commented(&c, COMMENT_REGION_MARKER));
        assert_eq!(uncomment_region_line(&c, COMMENT_REGION_MARKER), l);
        // Also works with a plain `C` marker.
        assert_eq!(comment_region_line("y = 2", "C"), "Cy = 2");
    }

    #[test]
    fn strip_sequence_numbers() {
        let mut line = String::new();
        line.push_str("      x = 1");
        line.push_str(&" ".repeat(72 - line.len()));
        line.push_str("SEQ00010");
        assert_eq!(line.len(), 80);
        let stripped = strip_sequence_nos(&line);
        assert_eq!(stripped.len(), 72);
        assert!(stripped.trim_end().ends_with("x = 1"));
        assert!(!stripped.contains("SEQ"));
        // Short lines are unchanged.
        assert_eq!(strip_sequence_nos("      y = 2"), "      y = 2");
    }

    #[test]
    fn indent_subprogram_nests() {
        // Valid fixed-form (code at column 7) but flat; re-indent by nesting.
        let src = "      program p\n\
                   \x20     do i = 1, n\n\
                   \x20     if (x > 0) then\n\
                   \x20     y = 1\n\
                   \x20     else\n\
                   \x20     y = 2\n\
                   \x20     endif\n\
                   \x20     enddo\n\
                   \x20     end";
        let l = lines(src);
        let out = indent_subprogram(&l);
        assert_eq!(out[0], "      program p"); // depth 0 -> col 7
        assert_eq!(out[1], "         do i = 1, n"); // depth 1 -> col 10
        assert_eq!(out[2], "            if (x > 0) then"); // depth 2 -> col 13
        assert_eq!(out[3], "               y = 1"); // depth 3 -> col 16
        assert_eq!(out[4], "            else"); // dedent to depth 2
        assert_eq!(out[5], "               y = 2");
        assert_eq!(out[6], "            endif"); // dedent
        assert_eq!(out[7], "         enddo");
        assert_eq!(out[8], "      end");
    }

    #[test]
    fn indent_subprogram_keeps_label() {
        let src = "      do 10 i = 1, n\n\
                   x = 1\n\
                   10    continue";
        let l = lines(src);
        let out = indent_subprogram(&l);
        assert_eq!(out[0], "      do 10 i = 1, n");
        assert_eq!(out[1], "         x = 1");
        // Label 10 stays in the label field; continue dedents (not a block end).
        assert_eq!(out[2], "10       continue");
    }

    #[test]
    fn column_ruler_shape() {
        assert!(FORTRAN_COLUMN_RULER.starts_with('0'));
        let num = fortran_column_ruler_numeric();
        assert_eq!(num.len(), 72);
        assert_eq!(&num[..10], "1234567890");
        assert_eq!(num.chars().nth(71), Some('2')); // column 72 -> ones digit 2
    }
}
