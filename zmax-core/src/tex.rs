//! Pure, editor-free algorithms backing the TeX/LaTeX editing substrate (the
//! zmax port of GNU Emacs `tex-mode`/`latex-mode`). The command layer in the
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

/// The TeX-escape ↔ Latin-1 pairs from Emacs `iso-cvt.el`'s
/// `iso-tex2iso-trans-tab`. Ordered longest-first so the braced form `{\"a}` is
/// consumed before the bare `\"a` hiding inside it, and so `\ss` is not eaten by
/// a shorter prefix.
const TEX_ISO: &[(&str, &str)] = &[
    ("{\\\"a}", "ä"),
    ("{\\\"o}", "ö"),
    ("{\\\"u}", "ü"),
    ("{\\\"A}", "Ä"),
    ("{\\\"O}", "Ö"),
    ("{\\\"U}", "Ü"),
    ("{\\\"e}", "ë"),
    ("{\\\"i}", "ï"),
    ("{\\`a}", "à"),
    ("{\\`e}", "è"),
    ("{\\`i}", "ì"),
    ("{\\`o}", "ò"),
    ("{\\`u}", "ù"),
    ("{\\'a}", "á"),
    ("{\\'e}", "é"),
    ("{\\'i}", "í"),
    ("{\\'o}", "ó"),
    ("{\\'u}", "ú"),
    ("{\\'c}", "ć"),
    ("{\\^a}", "â"),
    ("{\\^e}", "ê"),
    ("{\\^i}", "î"),
    ("{\\^o}", "ô"),
    ("{\\^u}", "û"),
    ("{\\~n}", "ñ"),
    ("{\\~a}", "ã"),
    ("{\\~o}", "õ"),
    ("{\\c c}", "ç"),
    ("{\\c C}", "Ç"),
    ("{\\ss}", "ß"),
    ("{\\aa}", "å"),
    ("{\\AA}", "Å"),
    ("{\\ae}", "æ"),
    ("{\\AE}", "Æ"),
    ("{\\o}", "ø"),
    ("{\\O}", "Ø"),
    ("\\\"a", "ä"),
    ("\\\"o", "ö"),
    ("\\\"u", "ü"),
    ("\\\"A", "Ä"),
    ("\\\"O", "Ö"),
    ("\\\"U", "Ü"),
    ("\\\"e", "ë"),
    ("\\\"i", "ï"),
    ("\\`a", "à"),
    ("\\`e", "è"),
    ("\\`i", "ì"),
    ("\\`o", "ò"),
    ("\\`u", "ù"),
    ("\\'a", "á"),
    ("\\'e", "é"),
    ("\\'i", "í"),
    ("\\'o", "ó"),
    ("\\'u", "ú"),
    ("\\^a", "â"),
    ("\\^e", "ê"),
    ("\\^i", "î"),
    ("\\^o", "ô"),
    ("\\^u", "û"),
    ("\\~n", "ñ"),
    ("\\c c", "ç"),
    ("\\c C", "Ç"),
    ("!`", "¡"),
    ("?`", "¿"),
];

/// German-TeX (`german.sty`) ↔ Latin-1 pairs, Emacs `iso-gtex2iso-trans-tab`.
/// `"` is active in german.sty, so `"a` is ä and `"s` is ß.
const GTEX_ISO: &[(&str, &str)] = &[
    ("\"a", "ä"),
    ("\"o", "ö"),
    ("\"u", "ü"),
    ("\"A", "Ä"),
    ("\"O", "Ö"),
    ("\"U", "Ü"),
    ("\"s", "ß"),
    ("\\3", "ß"),
    ("\"`", "„"),
    ("\"'", "“"),
];

/// Rewrite `text` by replacing every `from` with `to`, scanning left to right and
/// taking the first pair in `table` that matches at each position. Because the
/// tables are ordered longest-first, one pass converts without a replacement ever
/// being re-scanned — the reason a naive chain of `str::replace` calls is wrong
/// here (`\"a` would fire inside `{\"a}`).
fn translate(text: &str, table: &[(&str, &str)], reverse: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'outer: while !rest.is_empty() {
        for (tex, iso) in table {
            let (from, to) = if reverse { (*iso, *tex) } else { (*tex, *iso) };
            if let Some(tail) = rest.strip_prefix(from) {
                out.push_str(to);
                rest = tail;
                continue 'outer;
            }
        }
        let c = rest.chars().next().expect("rest is non-empty");
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Emacs `iso-tex2iso`: TeX escape sequences (`\"a`, `{\\ss}`, `!\``) become the
/// Latin-1 characters they stand for (`ä`, `ß`, `¡`).
pub fn tex2iso(text: &str) -> String {
    translate(text, TEX_ISO, false)
}

/// Emacs `iso-iso2tex`: the inverse of [`tex2iso`] — accented characters become
/// their TeX escape sequences.
pub fn iso2tex(text: &str) -> String {
    translate(text, TEX_ISO, true)
}

/// Emacs `iso-gtex2iso`: German-TeX shorthands (`"a`, `"s`) become Latin-1.
pub fn gtex2iso(text: &str) -> String {
    translate(text, GTEX_ISO, false)
}

/// Emacs `iso-iso2gtex`: the inverse of [`gtex2iso`].
pub fn iso2gtex(text: &str) -> String {
    translate(text, GTEX_ISO, true)
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

    /// The whole point of the longest-first table: the braced form must be
    /// consumed whole, or `{\"a}` would come out as `{ä}` with stray braces.
    #[test]
    fn tex2iso_prefers_the_braced_form_over_the_bare_escape() {
        assert_eq!(tex2iso("Stra{\\ss}e"), "Straße");
        assert_eq!(tex2iso("M{\\\"u}ller"), "Müller");
        assert_eq!(tex2iso("M\\\"uller"), "Müller");
        assert_eq!(tex2iso("caf\\'e"), "café");
        assert_eq!(tex2iso("!`Hola?`"), "¡Hola¿");
        // Nothing to convert: text passes through untouched.
        assert_eq!(tex2iso("plain ascii"), "plain ascii");
    }

    /// iso2tex is the inverse for the escapes iso-cvt round-trips, and it emits
    /// the braced form so the escape cannot swallow the following letter.
    #[test]
    fn iso2tex_round_trips_through_tex2iso() {
        for s in ["Müller", "Straße", "café", "¡Hola!", "Señor", "Ångström"] {
            let tex = iso2tex(s);
            assert_eq!(tex2iso(&tex), s, "round trip of {s} via {tex}");
        }
        assert_eq!(iso2tex("Müller"), "M{\\\"u}ller");
    }

    /// German TeX: `"` is the active accent char, so `"s` is ß, not a quote.
    #[test]
    fn gtex_converts_german_sty_shorthands_both_ways() {
        assert_eq!(gtex2iso("Stra\"se"), "Straße");
        assert_eq!(gtex2iso("M\"uller gr\"o\"ser"), "Müller größer");
        assert_eq!(gtex2iso("\\3"), "ß");
        assert_eq!(iso2gtex("Müller"), "M\"uller");
        // ß has two German-TeX spellings; the inverse emits the canonical `"s`.
        assert_eq!(iso2gtex("Straße"), "Stra\"se");
        assert_eq!(gtex2iso(&iso2gtex("größer")), "größer");
    }
}
