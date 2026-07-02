//! Pure, editor-free algorithms backing the TeX/LaTeX editing substrate (the
//! zemacs port of GNU Emacs `tex-mode`/`latex-mode`). The command layer in the
//! term crate reads the buffer text, calls these, and applies the result.
//! Everything here is dependency-free and unit-tested. Prior art: Emacs
//! `tex-insert-quote`, `latex-close-block`, `tex-validate-region`.

/// Emacs `tex-insert-quote`: decide the TeX quote to insert given the character
/// immediately before point. An opening `` `` `` is used at the start of the
/// buffer or after whitespace or an opening delimiter; otherwise a closing `''`.
pub fn insert_quote(before: Option<char>) -> &'static str {
    match before {
        None => "``",
        Some(c) if c.is_whitespace() || matches!(c, '(' | '[' | '{' | '`') => "``",
        _ => "''",
    }
}

/// Scan `text` (typically the buffer up to point) for the innermost LaTeX
/// environment that is opened by `\begin{ENV}` but not yet closed by a matching
/// `\end{ENV}`. Returns the environment name, so `latex-close-block` can insert
/// `\end{ENV}`. Nested environments are handled with a stack.
pub fn unclosed_environment(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if let Some(rest) = text[i..].strip_prefix("\\begin{") {
                if let Some(end) = rest.find('}') {
                    stack.push(rest[..end].to_string());
                    i += "\\begin{".len() + end + 1;
                    continue;
                }
            } else if let Some(rest) = text[i..].strip_prefix("\\end{") {
                if let Some(end) = rest.find('}') {
                    let name = &rest[..end];
                    // Pop the matching open (or the top if names disagree).
                    if let Some(pos) = stack.iter().rposition(|e| e == name) {
                        stack.remove(pos);
                    } else {
                        stack.pop();
                    }
                    i += "\\end{".len() + end + 1;
                    continue;
                }
            }
            // Skip the escaped char so "\{" / "\}" don't confuse a later pass.
            i += 2;
            continue;
        }
        i += 1;
    }
    stack.pop()
}

/// A structural problem `tex-validate-region` reports.
#[derive(Debug, PartialEq, Eq)]
pub enum TexError {
    /// An unmatched `}` at this char offset.
    UnmatchedClose(usize),
    /// An unclosed `{` opened at this char offset.
    UnmatchedOpen(usize),
    /// An unclosed inline-math `$` opened at this char offset.
    UnmatchedMath(usize),
    /// A `\begin{ENV}` with no matching `\end`.
    UnclosedEnv(String),
    /// A `\end{ENV}` with no matching `\begin`.
    UnopenedEnv(String),
}

/// Emacs `tex-validate-region`: check `{}` balance (respecting `\{`/`\}`
/// escapes), inline-math `$` pairing, and `\begin`/`\end` environment matching.
/// Returns the first problem found, or `None` if the text is well-formed.
/// Char offsets are counted in `char`s (not bytes) so callers can map to the
/// rope directly.
pub fn validate(text: &str) -> Option<TexError> {
    let chars: Vec<char> = text.chars().collect();
    let mut brace: Vec<usize> = Vec::new();
    let mut math: Option<usize> = None;
    let mut env: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                // Check for \begin{ / \end{ (compare against the remaining text).
                let tail: String = chars[i..].iter().collect();
                if let Some(rest) = tail.strip_prefix("\\begin{") {
                    if let Some(e) = rest.find('}') {
                        env.push(rest[..e].to_string());
                        i += "\\begin{".chars().count() + rest[..e].chars().count() + 1;
                        continue;
                    }
                } else if let Some(rest) = tail.strip_prefix("\\end{") {
                    if let Some(e) = rest.find('}') {
                        let name = rest[..e].to_string();
                        match env.pop() {
                            Some(top) if top == name => {}
                            _ => return Some(TexError::UnopenedEnv(name)),
                        }
                        i += "\\end{".chars().count() + rest[..e].chars().count() + 1;
                        continue;
                    }
                }
                i += 2; // skip the escaped char
                continue;
            }
            '{' => brace.push(i),
            '}' => {
                if brace.pop().is_none() {
                    return Some(TexError::UnmatchedClose(i));
                }
            }
            '$' => {
                math = match math {
                    Some(_) => None,
                    None => Some(i),
                };
            }
            _ => {}
        }
        i += 1;
    }
    if let Some(pos) = brace.first() {
        return Some(TexError::UnmatchedOpen(*pos));
    }
    if let Some(pos) = math {
        return Some(TexError::UnmatchedMath(pos));
    }
    if let Some(name) = env.pop() {
        return Some(TexError::UnclosedEnv(name));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_direction() {
        assert_eq!(insert_quote(None), "``");
        assert_eq!(insert_quote(Some(' ')), "``");
        assert_eq!(insert_quote(Some('(')), "``");
        assert_eq!(insert_quote(Some('a')), "''");
        assert_eq!(insert_quote(Some('.')), "''");
    }

    #[test]
    fn close_innermost_environment() {
        assert_eq!(
            unclosed_environment("\\begin{itemize}\n\\item x"),
            Some("itemize".to_string())
        );
        assert_eq!(
            unclosed_environment("\\begin{a}\\begin{b}"),
            Some("b".to_string())
        );
        // fully closed -> nothing to close
        assert_eq!(unclosed_environment("\\begin{a}\\end{a}"), None);
        // nested, inner closed -> outer remains open
        assert_eq!(
            unclosed_environment("\\begin{a}\\begin{b}\\end{b}"),
            Some("a".to_string())
        );
    }

    #[test]
    fn validate_ok() {
        assert_eq!(validate("\\begin{a} {x} $y$ \\end{a}"), None);
        assert_eq!(validate("plain text, no markup"), None);
        assert_eq!(validate("escaped \\{ and \\}"), None);
    }

    #[test]
    fn validate_catches_problems() {
        assert_eq!(validate("a } b"), Some(TexError::UnmatchedClose(2)));
        assert_eq!(validate("a { b"), Some(TexError::UnmatchedOpen(2)));
        assert_eq!(validate("math $x + y"), Some(TexError::UnmatchedMath(5)));
        assert_eq!(
            validate("\\begin{itemize} x"),
            Some(TexError::UnclosedEnv("itemize".to_string()))
        );
        assert_eq!(
            validate("x \\end{foo}"),
            Some(TexError::UnopenedEnv("foo".to_string()))
        );
    }
}
