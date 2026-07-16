//! Translate a vim search / substitute pattern into the syntax the
//! `regex`/`regex-automata` engine (via `rope::RegexBuilder`) expects.
//!
//! zmax's search engine is a Rust regex; vim users type vim "magic" patterns.
//! Those two syntaxes disagree on which characters need a backslash. In vim's
//! default *magic* mode `\(`, `\)`, `\|`, `\+`, `\?`, `\{` are the *special*
//! forms and a bare `(` / `|` / `+` is a *literal*; the Rust engine is the
//! opposite. Passing a vim pattern straight through therefore matches literally
//! (`\(foo\|bar\)` looks for the text `(foo|bar)`) — silently, with no error.
//!
//! [`to_rust`] rewrites the pattern so vim muscle-memory works. It honors vim's
//! magic-level switches (`\v` very-magic, `\m` magic, `\M` nomagic, `\V`
//! very-nomagic), the inline case flags `\c`/`\C`, and vim's `\a`/`\l`/`\u`/…
//! character-class aliases.
//!
//! This is gated to the vim/spacemacs presets by the caller (`editor.vim_semantics`);
//! helix/emacs users keep typing native Rust-regex syntax untouched.
//!
//! Known limitation: `\zs` / `\ze` (match-start / match-end) have no Rust-regex
//! equivalent and are left as-is (the engine treats `\z` as an anchor). Mid-pattern
//! literal `^`/`$` are passed through as anchors (the common boundary case is
//! correct; a literal `^` in the middle of a magic pattern is rare and not handled).

use std::borrow::Cow;

/// vim "magic level" — controls how much of the pattern is special without a
/// backslash. Switchable mid-pattern via `\v` `\m` `\M` `\V`.
#[allow(clippy::enum_variant_names)] // Very/Magic/No/VeryNo mirror vim \v \m \M \V
#[derive(Clone, Copy, PartialEq, Eq)]
enum Magic {
    /// `\v` — every non-alphanumeric, non-`_` char is special (closest to Rust).
    Very,
    /// `\m` — vim default: `. * [ ] ^ $ ~` special; `( ) { } | + ?` literal.
    Magic,
    /// `\M` — only `^ $` special; `.` `*` become literal.
    No,
    /// `\V` — only `\`-escaped atoms are special; everything else literal.
    VeryNo,
}

/// Push `c` to `out`, backslash-escaping it if it is special to the Rust engine.
fn push_literal(out: &mut String, c: char) {
    // `}` is intentionally omitted: a bare `}` is a valid literal in the Rust
    // engine, and escaping it (`\}`) would fail to close a `{n,m}` quantifier
    // whose opening `\{` we translated to a bare `{`.
    if matches!(
        c,
        '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '|' | '\\'
    ) {
        out.push('\\');
    }
    out.push(c);
}

/// Translate a vim magic-mode pattern into Rust-regex syntax. Returns the input
/// unchanged when it is already valid Rust (no vim-only atoms), which keeps the
/// common `\w`/`\d`/`foo.*bar` cases allocation-cheap in practice.
pub fn to_rust(pat: &str) -> String {
    let mut out = String::with_capacity(pat.len() + 8);
    let mut level = Magic::Magic;
    let mut chars = pat.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            let Some(&n) = chars.peek() else {
                // trailing backslash — emit an escaped literal backslash
                out.push_str("\\\\");
                break;
            };
            chars.next();
            match n {
                // magic-level switches (consume, emit nothing)
                'v' => level = Magic::Very,
                'm' => level = Magic::Magic,
                'M' => level = Magic::No,
                'V' => level = Magic::VeryNo,
                // inline case control
                'c' => out.push_str("(?i)"),
                'C' => out.push_str("(?-i)"),
                // vim's backslash-special grouping / alternation / quantifiers →
                // the engine's bare forms.
                '(' => out.push('('),
                ')' => out.push(')'),
                '{' => out.push('{'),
                '}' => out.push('}'),
                '|' => out.push('|'),
                '+' => out.push('+'),
                '?' | '=' => out.push('?'),
                // word boundaries — the engine supports `\<` / `\>` natively.
                '<' => out.push_str("\\<"),
                '>' => out.push_str("\\>"),
                // vim character-class aliases with no Rust escape → POSIX classes.
                'a' => out.push_str("[[:alpha:]]"),
                'A' => out.push_str("[^[:alpha:]]"),
                'l' => out.push_str("[[:lower:]]"),
                'L' => out.push_str("[^[:lower:]]"),
                'u' => out.push_str("[[:upper:]]"),
                'U' => out.push_str("[^[:upper:]]"),
                'x' => out.push_str("[[:xdigit:]]"),
                'X' => out.push_str("[^[:xdigit:]]"),
                'o' => out.push_str("[0-7]"),
                'O' => out.push_str("[^0-7]"),
                'h' => out.push_str("[[:alpha:]_]"),
                'H' => out.push_str("[^[:alpha:]_]"),
                // escapes the engine already understands — keep verbatim.
                'd' | 'D' | 's' | 'S' | 'w' | 'W' | 'b' | 'B' | 'n' | 't' | 'r' | 'f' | '.'
                | '*' | '[' | ']' | '^' | '$' | '/' | '\\' => {
                    out.push('\\');
                    out.push(n);
                }
                // any other escaped char: emit as an escaped literal so it matches
                // itself rather than being reinterpreted by the engine.
                other => push_literal(&mut out, other),
            }
            continue;
        }

        // Unescaped char — meaning depends on the current magic level.
        match level {
            Magic::Very => match c {
                // very-magic: `<`/`>` are word boundaries; `%(` is a non-capturing
                // group; the rest of the Rust metacharacters pass straight through.
                '<' => out.push_str("\\<"),
                '>' => out.push_str("\\>"),
                '%' if chars.peek() == Some(&'(') => {
                    chars.next();
                    out.push_str("(?:");
                }
                _ => out.push(c),
            },
            Magic::Magic => match c {
                // special in both vim-magic and Rust — pass through.
                '.' | '*' | '[' | ']' | '^' | '$' => out.push(c),
                // a bare `}` is a literal in vim-magic and a valid literal (or a
                // quantifier close) in Rust — emit it unescaped either way.
                '}' => out.push('}'),
                // literal in vim-magic but special in Rust — escape.
                '(' | ')' | '{' | '|' | '+' | '?' => {
                    out.push('\\');
                    out.push(c);
                }
                _ => push_literal(&mut out, c),
            },
            Magic::No => match c {
                // nomagic: only anchors stay special.
                '^' | '$' => out.push(c),
                _ => push_literal(&mut out, c),
            },
            Magic::VeryNo => push_literal(&mut out, c),
        }
    }

    out
}

/// Translate `input` for the search/substitute engine when vim semantics are
/// active; otherwise return it untouched (helix/emacs presets type Rust regex).
pub fn search_pattern<'a>(vim_semantics: bool, input: &'a str) -> Cow<'a, str> {
    if vim_semantics {
        Cow::Owned(to_rust(input))
    } else {
        Cow::Borrowed(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_and_alternation() {
        // The headline case: `\(foo\|bar\)\+` must become a real group +
        // alternation + quantifier, not a literal-character search.
        assert_eq!(to_rust(r"\(foo\|bar\)\+"), "(foo|bar)+");
    }

    #[test]
    fn counted_quantifier() {
        assert_eq!(to_rust(r"a\{2,3}"), "a{2,3}");
    }

    #[test]
    fn optional_backslash_forms() {
        assert_eq!(to_rust(r"colou\?r"), "colou?r");
        assert_eq!(to_rust(r"ab\=c"), "ab?c");
    }

    #[test]
    fn bare_metacharacters_are_literal_in_magic() {
        // In magic mode these are literals; the engine needs them escaped.
        assert_eq!(to_rust(r"a(b"), r"a\(b");
        assert_eq!(to_rust(r"a|b"), r"a\|b");
        assert_eq!(to_rust(r"a+b"), r"a\+b");
        assert_eq!(to_rust(r"a?b"), r"a\?b");
        assert_eq!(to_rust(r"a{b"), r"a\{b");
    }

    #[test]
    fn magic_specials_pass_through() {
        assert_eq!(to_rust(r"foo.*bar"), "foo.*bar");
        assert_eq!(to_rust(r"^start"), "^start");
        assert_eq!(to_rust(r"end$"), "end$");
        assert_eq!(to_rust(r"[abc]"), "[abc]");
    }

    #[test]
    fn word_boundaries_preserved() {
        assert_eq!(to_rust(r"\<word\>"), r"\<word\>");
    }

    #[test]
    fn engine_escapes_pass_through() {
        assert_eq!(to_rust(r"\d\+"), r"\d+");
        assert_eq!(to_rust(r"\w\{3}"), r"\w{3}");
        assert_eq!(to_rust(r"\s"), r"\s");
    }

    #[test]
    fn very_magic() {
        assert_eq!(to_rust(r"\vfoo(bar|baz)+"), "foo(bar|baz)+");
        assert_eq!(to_rust(r"\v\d+"), r"\d+");
        assert_eq!(to_rust(r"\v<word>"), r"\<word\>");
        assert_eq!(to_rust(r"\v%(ab)+"), "(?:ab)+");
    }

    #[test]
    fn very_nomagic() {
        // `\V` — everything literal; `.` matches a dot, not any char.
        assert_eq!(to_rust(r"\Va.b"), r"a\.b");
        assert_eq!(to_rust(r"\Vfoo(bar)"), r"foo\(bar\)");
    }

    #[test]
    fn nomagic() {
        // `\M` — `.`/`*` literal, anchors still special.
        assert_eq!(to_rust(r"\Ma.b"), r"a\.b");
        assert_eq!(to_rust(r"\M^ab$"), "^ab$");
    }

    #[test]
    fn inline_case_flags() {
        assert_eq!(to_rust(r"\cfoo"), "(?i)foo");
        assert_eq!(to_rust(r"\Cfoo"), "(?-i)foo");
    }

    #[test]
    fn char_class_aliases() {
        assert_eq!(to_rust(r"\a\+"), "[[:alpha:]]+");
        assert_eq!(to_rust(r"\l"), "[[:lower:]]");
        assert_eq!(to_rust(r"\u"), "[[:upper:]]");
        assert_eq!(to_rust(r"\x\+"), "[[:xdigit:]]+");
        assert_eq!(to_rust(r"\h\w*"), r"[[:alpha:]_]\w*");
    }

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(to_rust("hello world"), "hello world");
        assert_eq!(to_rust("TODO"), "TODO");
    }

    #[test]
    fn trailing_backslash_is_safe() {
        // Must not panic and must not leave a dangling escape for the engine.
        assert_eq!(to_rust(r"abc\"), r"abc\\");
    }

    #[test]
    fn translated_patterns_compile() {
        // Every tricky translation must produce syntax the real search engine
        // (`rope::RegexBuilder`, i.e. regex-automata) accepts — including `\<`/`\>`
        // word boundaries, which the key-sequence test harness can't express.
        for pat in [
            r"\<foo\>",
            r"\(a\|b\)\+",
            r"a\{2,3}",
            r"\v(x|y)+",
            r"\v%(ab)+",
            r"\ccase",
            r"\Cfoo",
            r"\a\+",
            r"\h\w*",
            r"\x\{2}",
            r"\Va.b",
            r"foo.*bar",
        ] {
            let translated = to_rust(pat);
            let built = zmax_stdx::rope::RegexBuilder::new().build(&translated);
            assert!(
                built.is_ok(),
                "vim pattern {pat:?} translated to {translated:?} must compile: {built:?}"
            );
        }
    }

    #[test]
    fn translated_patterns_compile_in_substitute_engine() {
        // `:s`/`:g` build with the `regex` crate (not `rope`); confirm the same
        // translations compile there too, including `\<`/`\>` word boundaries.
        for pat in [
            r"\<foo\>",
            r"\(a\|b\)\+",
            r"a\{2,3}",
            r"\v(x|y)+",
            r"\ccase",
            r"\a\+",
        ] {
            let translated = to_rust(pat);
            let built = regex::Regex::new(&translated);
            assert!(
                built.is_ok(),
                "vim pattern {pat:?} -> {translated:?} must compile in the regex crate: {built:?}"
            );
        }
    }

    #[test]
    fn gate_off_returns_input_untouched() {
        // With vim semantics off, a Rust-regex pattern is passed through verbatim.
        assert_eq!(search_pattern(false, r"(foo|bar)+"), r"(foo|bar)+");
        // With it on, a vim pattern is translated.
        assert_eq!(search_pattern(true, r"\(foo\|bar\)"), "(foo|bar)");
    }
}
