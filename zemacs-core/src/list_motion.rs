//! List / s-expression motion — the zemacs port of the GNU Emacs balanced-paren
//! movement commands (`forward-list` C-M-n, `backward-list` C-M-p, `down-list`
//! C-M-d, `up-list`, `backward-up-list` C-M-u, and `forward-sexp` C-M-f which
//! `kill-sexp` uses). Pure and dependency-free: each takes the buffer text and a
//! char cursor and returns the target char position, counting `()`, `[]` and
//! `{}` depth (Emacs's default syntax for these). Returns `None` when the motion
//! would cross an unbalanced delimiter or run off the buffer (Emacs signals a
//! scan error there); the command layer then leaves the cursor put.

fn is_open(c: char) -> bool {
    matches!(c, '(' | '[' | '{')
}

fn is_close(c: char) -> bool {
    matches!(c, ')' | ']' | '}')
}

/// `forward-list`: move forward over the next balanced group, landing just after
/// its closing delimiter.
pub fn forward_list(text: &str, cursor: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0i32;
    let mut i = cursor;
    while i < chars.len() {
        let c = chars[i];
        if is_open(c) {
            depth += 1;
        } else if is_close(c) {
            if depth == 0 {
                return None; // a close before any open — no list ahead
            }
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

/// `backward-list`: move backward over the previous balanced group, landing on
/// its opening delimiter.
pub fn backward_list(text: &str, cursor: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0i32;
    let mut i = cursor.min(chars.len());
    while i > 0 {
        i -= 1;
        let c = chars[i];
        if is_close(c) {
            depth += 1;
        } else if is_open(c) {
            if depth == 0 {
                return None;
            }
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// `down-list`: descend into the next list, landing just after its opening
/// delimiter.
pub fn down_list(text: &str, cursor: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate().skip(cursor) {
        if is_close(c) {
            return None; // hit a closing delimiter first — nothing to descend into
        }
        if is_open(c) {
            return Some(i + 1);
        }
    }
    None
}

/// `up-list`: move forward out of the enclosing list, landing just after its
/// closing delimiter.
pub fn up_list(text: &str, cursor: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0i32;
    for (i, &c) in chars.iter().enumerate().skip(cursor) {
        if is_open(c) {
            depth += 1;
        } else if is_close(c) {
            if depth == 0 {
                return Some(i + 1);
            }
            depth -= 1;
        }
    }
    None
}

/// `backward-up-list`: move backward out of the enclosing list, landing on its
/// opening delimiter.
pub fn backward_up_list(text: &str, cursor: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0i32;
    let mut i = cursor.min(chars.len());
    while i > 0 {
        i -= 1;
        if is_close(chars[i]) {
            depth += 1;
        } else if is_open(chars[i]) {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

/// `forward-sexp`: move over the next s-expression — a whole balanced list if the
/// next non-space char opens one, otherwise a run of atom characters. Used by
/// `kill-sexp` (kill from point to here).
pub fn forward_sexp(text: &str, cursor: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = cursor;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    if is_open(chars[i]) {
        return forward_list(text, i);
    }
    if is_close(chars[i]) {
        return None; // pointing at a stray close
    }
    let start = i;
    while i < chars.len() && !chars[i].is_whitespace() && !is_open(chars[i]) && !is_close(chars[i])
    {
        i += 1;
    }
    (i > start).then_some(i)
}

/// `backward-sexp` (C-M-b): move backward over the previous s-expression — a whole
/// balanced list if the preceding non-space char closes one, otherwise a run of
/// atom characters back to its start. The mirror of [`forward_sexp`]; `kill-sexp`
/// and the backward paredit motions build on it.
pub fn backward_sexp(text: &str, cursor: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = cursor.min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    if i == 0 {
        return None;
    }
    let prev = chars[i - 1];
    if is_close(prev) {
        return backward_list(text, i);
    }
    if is_open(prev) {
        return None; // sitting just after a stray open
    }
    let end = i;
    while i > 0
        && !chars[i - 1].is_whitespace()
        && !is_open(chars[i - 1])
        && !is_close(chars[i - 1])
    {
        i -= 1;
    }
    (i < end).then_some(i)
}

/// The char range `[start, end)` of the top-level form containing `cursor` —
/// Emacs's *defun*: the parenthesised form whose open paren sits in column 0, and
/// its matching close paren. This is what `lisp-eval-defun` (`C-M-x`) sends to the
/// inferior Lisp and what `eval-defun` evaluates.
///
/// The scan is Lisp-aware where it has to be: `"…"` strings (with `\` escapes),
/// `;` line comments and `?(` character literals do not count towards paren
/// depth, so a `(` inside a docstring cannot swallow the rest of the file.
/// `None` when the cursor is not inside a top-level form, or the form is
/// unterminated.
pub fn defun_range(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    // The nearest `(` that starts a line, at or before the cursor.
    let from = cursor.min(chars.len() - 1);
    let start = (0..=from)
        .rev()
        .find(|&i| chars[i] == '(' && (i == 0 || chars[i - 1] == '\n'))?;

    let mut depth = 0i32;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '"' => {
                // Skip the string literal, honouring backslash escapes.
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += if chars[i] == '\\' { 2 } else { 1 };
                }
            }
            ';' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            // A character literal — `?(`, `?)` or the escaped `?\(` — is data, not
            // a delimiter: skip the character it names (and its backslash).
            '?' if i + 1 < chars.len() => i += if chars[i + 1] == '\\' { 2 } else { 1 },
            '\\' => i += 1,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    // The cursor must actually be inside the form we found.
                    return (cursor <= i + 1).then_some((start, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Char-slice `text` over the char range a motion returned.
    fn chars_in(text: &str, r: (usize, usize)) -> String {
        text.chars().skip(r.0).take(r.1 - r.0).collect()
    }

    /// `defun-range` spans the whole top-level form the cursor sits in, from the
    /// column-0 open paren to its matching close.
    #[test]
    fn defun_range_spans_the_top_level_form() {
        let src = "(defun a ()\n  (+ 1 2))\n(defun b () 3)\n";
        // Cursor anywhere inside the first defun (here: inside the nested list).
        assert_eq!(
            chars_in(src, defun_range(src, 15).unwrap()),
            "(defun a ()\n  (+ 1 2))"
        );
        // …and inside the second.
        assert_eq!(
            chars_in(src, defun_range(src, 30).unwrap()),
            "(defun b () 3)"
        );
    }

    /// A `(` inside a string, a comment or a character literal must not open a
    /// level — otherwise the form would never close and the wrong text is sent.
    #[test]
    fn defun_range_ignores_parens_in_strings_comments_and_chars() {
        let src = "(defun a ()\n  \"a ( string\" ; a ( comment\n  ?\\( )\n(defun b () 2)\n";
        assert_eq!(
            chars_in(src, defun_range(src, 5).unwrap()),
            "(defun a ()\n  \"a ( string\" ; a ( comment\n  ?\\( )"
        );
    }

    /// Outside any top-level form, and inside an unterminated one, there is
    /// nothing to send.
    #[test]
    fn defun_range_none_when_not_in_a_form() {
        assert_eq!(defun_range(";; just a comment\n", 3), None);
        assert_eq!(defun_range("(defun a ()\n  (+ 1 2)\n", 5), None);
        assert_eq!(defun_range("", 0), None);
        // Past the end of the only form: the cursor is not inside it.
        assert_eq!(defun_range("(a)\n\nxyz", 7), None);
    }

    // Positions in "(a (b) c)": ( a _ ( b ) _ c )
    //                            0 1 2 3 4 5 6 7 8
    const S: &str = "(a (b) c)";

    #[test]
    fn forward_and_backward_list() {
        // From 0, forward over the whole "(a (b) c)" -> after the final ) = 9.
        assert_eq!(forward_list(S, 0), Some(9));
        // From inside at the inner "(b)" open (index 3) -> after ")" = 6.
        assert_eq!(forward_list(S, 3), Some(6));
        // From end, backward over the whole list -> its open at 0.
        assert_eq!(backward_list(S, 9), Some(0));
        // From just after "(b)" (index 6), backward -> that list's open at 3.
        assert_eq!(backward_list(S, 6), Some(3));
        // No list ahead.
        assert_eq!(forward_list("abc", 0), None);
    }

    #[test]
    fn down_and_up_list() {
        // down-list from 0: enter the outer list -> just after "(" = 1.
        assert_eq!(down_list(S, 0), Some(1));
        // down-list from 1: descend into the inner "(b)" -> just after it = 4.
        assert_eq!(down_list(S, 1), Some(4));
        // up-list from inside "b" (index 4): out of the inner list -> after ")" = 6.
        assert_eq!(up_list(S, 4), Some(6));
        // backward-up-list from index 4: to the inner open "(" at 3.
        assert_eq!(backward_up_list(S, 4), Some(3));
        // up-list from top level with no enclosing list.
        assert_eq!(up_list("a b c", 2), None);
        assert_eq!(backward_up_list("a b c", 2), None);
    }

    #[test]
    fn forward_sexp_atoms_and_lists() {
        // A leading atom.
        assert_eq!(forward_sexp("foo bar", 0), Some(3));
        // Skip whitespace, then the atom.
        assert_eq!(forward_sexp("  foo", 0), Some(5));
        // A list is one sexp.
        assert_eq!(forward_sexp(S, 0), Some(9));
        // Inner list from index 3.
        assert_eq!(forward_sexp(S, 3), Some(6));
        assert_eq!(forward_sexp("   ", 0), None);
    }

    #[test]
    fn backward_sexp_atoms_and_lists() {
        // Trailing atom: from end of "foo bar" (7) back over "bar" -> 4.
        assert_eq!(backward_sexp("foo bar", 7), Some(4));
        // Skip trailing whitespace, then the atom: "foo  " (len 5) -> 0.
        assert_eq!(backward_sexp("foo  ", 5), Some(0));
        // A whole list is one sexp: from end of S (9) -> its open at 0.
        assert_eq!(backward_sexp(S, 9), Some(0));
        // From just after the inner "(b)" (index 6) -> its open at 3.
        assert_eq!(backward_sexp(S, 6), Some(3));
        // Nothing before point.
        assert_eq!(backward_sexp("   ", 3), None);
        // Round-trips with forward_sexp over an atom.
        assert_eq!(
            backward_sexp("foo bar", forward_sexp("foo bar", 4).unwrap()),
            Some(4)
        );
    }
}
