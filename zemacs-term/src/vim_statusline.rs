//! vim `statusline` / `rulerformat`: translate a vim format string into the
//! statusline elements zemacs renders.
//!
//! zemacs's statusline is a list of typed elements (`StatusLineElement`) split
//! into left / center / right groups, not a printf-style string, so a vim format
//! string is parsed into that list rather than interpreted at render time. The
//! `%=` separator splits left from right, exactly as it does in vim.
//!
//! Only the `%`-codes that have a real element behind them are honored; a code
//! zemacs has no element for (`%{expr}` evaluation, `%<` truncation markers,
//! field widths) is dropped rather than faked. Literal text between codes is not
//! representable either — the element list has no free-text element — so it is
//! dropped too, and [`parse`] reports what it could not keep so `:set` can say
//! so instead of silently pretending.

use zemacs_view::editor::StatusLineElement as El;

/// What a vim statusline format string translated to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct VimStatusLine {
    /// Elements before `%=`.
    pub left: Vec<El>,
    /// Elements after `%=`.
    pub right: Vec<El>,
    /// The `%`-codes and literals that had no zemacs element, in order. `:set`
    /// reports these instead of silently dropping them.
    pub unsupported: Vec<String>,
}

/// The element a vim `%`-code maps to, or `None` when zemacs has no equivalent.
fn element_for(code: char) -> Option<El> {
    Some(match code {
        // File identification.
        'f' => El::FileName,                        // path relative to the cwd
        'F' => El::FileAbsolutePath,                // full path
        't' => El::FileBaseName,                    // basename
        'm' | '+' => El::FileModificationIndicator, // [+] modified
        'r' | 'R' => El::ReadOnlyIndicator,         // [RO]
        'y' | 'Y' => El::FileType,                  // [rust]
        // Position.
        'l' | 'c' | 'v' | 'V' | 'o' | 'O' => El::Position, // line/col — one element covers both
        'L' => El::TotalLineNumbers,
        'p' | 'P' => El::PositionPercentage,
        // Everything else has no element behind it.
        _ => return None,
    })
}

/// Parse a vim `statusline`/`rulerformat` string.
///
/// `%%` is a literal percent (dropped, as literals are), `%=` splits left from
/// right. Field widths and truncation (`%-14.20f`, `%<`) are consumed and the
/// element kept; the width itself is not honored.
pub fn parse(format: &str) -> VimStatusLine {
    let mut out = VimStatusLine::default();
    let mut right = false;
    let mut literal = String::new();
    let mut chars = format.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            literal.push(ch);
            continue;
        }
        // Consume the field width / justification vim allows between `%` and the
        // code: `-`, digits, `.`, and the truncation marker `<`.
        let mut spec = String::new();
        while let Some(&c) = chars.peek() {
            if c == '-' || c == '.' || c == '<' || c.is_ascii_digit() {
                spec.push(c);
                chars.next();
            } else {
                break;
            }
        }
        let Some(code) = chars.next() else {
            out.unsupported.push("%".to_string());
            break;
        };
        match code {
            '%' => literal.push('%'),
            '=' => right = true,
            // `%{expr}` / `%(…%)` / `%#Group#`: no evaluation, no highlight groups.
            '{' | '(' | '#' => {
                let mut skipped = String::from(code);
                let closer = match code {
                    '{' => '}',
                    '(' => ')',
                    _ => '#',
                };
                for c in chars.by_ref() {
                    skipped.push(c);
                    if c == closer {
                        break;
                    }
                }
                out.unsupported.push(format!("%{skipped}"));
            }
            _ => match element_for(code) {
                Some(el) => {
                    let group = if right { &mut out.right } else { &mut out.left };
                    // Vim strings run codes together (`%l,%c`); the separator
                    // between them is the literal text, which zemacs cannot
                    // render, so elements are simply spaced.
                    if !group.is_empty() {
                        group.push(El::Spacer);
                    }
                    group.push(el);
                }
                None => out.unsupported.push(format!("%{spec}{code}")),
            },
        }
    }
    if !literal.trim().is_empty() {
        out.unsupported.push(literal.trim().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_left_and_right_at_percent_equals() {
        // vim's own default-ish statusline.
        let s = parse("%f %m%r%= %l,%c %P");
        assert_eq!(
            s.left,
            vec![
                El::FileName,
                El::Spacer,
                El::FileModificationIndicator,
                El::Spacer,
                El::ReadOnlyIndicator,
            ]
        );
        assert_eq!(
            s.right,
            vec![
                El::Position,
                El::Spacer,
                El::Position,
                El::Spacer,
                El::PositionPercentage,
            ]
        );
    }

    #[test]
    fn field_widths_are_consumed_and_the_element_kept() {
        let s = parse("%-14.20f");
        assert_eq!(s.left, vec![El::FileName]);
        assert!(s.unsupported.is_empty(), "width is not an unsupported code");
    }

    /// Codes with nothing behind them are reported, never silently dropped —
    /// that is what lets `:set statusline=…` tell the truth about what it kept.
    #[test]
    fn unsupported_codes_are_reported() {
        let s = parse("%{FugitiveHead()} %f %#Error# %q");
        assert_eq!(s.left, vec![El::FileName]);
        assert_eq!(
            s.unsupported,
            vec![
                "%{FugitiveHead()}".to_string(),
                "%#Error#".to_string(),
                "%q".to_string()
            ]
        );
    }

    #[test]
    fn double_percent_is_a_literal_not_a_code() {
        let s = parse("%f%%");
        assert_eq!(s.left, vec![El::FileName]);
        assert_eq!(s.unsupported, vec!["%".to_string()]);
    }
}
