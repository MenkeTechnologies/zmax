//! `prettify-symbols-mode` and `glyphless-display-mode` — the two Emacs modes
//! that change how a character *displays* without changing the buffer text.
//!
//! Emacs implements both with a `display` text property / composition: the
//! characters are still there, `point` still walks them, the file on disk is
//! untouched — only the glyphs drawn on screen change. zmax draws them with
//! grapheme overlays (`zmax_core::text_annotations::Overlay`), the same
//! mechanism `conceallevel` uses: an overlay replaces the grapheme at a char
//! index, and an empty replacement erases it. A two-character symbol like `->`
//! is therefore one overlay carrying `→` and one carrying nothing.
//!
//! Both scans are pure: they take text and return `(char_index, replacement)`
//! pairs, so the substitutions can be unit tested without a renderer.

/// One display substitution: the grapheme at `char_idx` renders as `text`
/// (which is empty when the character is swallowed by a preceding, wider
/// substitution).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Substitution {
    /// Char index into the scanned text.
    pub char_idx: usize,
    /// What to draw there. Empty means "draw nothing".
    pub text: &'static str,
}

/// A `prettify-symbols-alist` entry: the literal source token and the glyph it
/// draws as.
type Symbol = (&'static str, &'static str);

/// The symbols shared by the C-family / ML-family languages zmax highlights:
/// the comparison and arrow operators, which every `prettify-symbols-alist` in
/// the wild starts with.
const OPERATORS: &[Symbol] = &[
    ("<=", "≤"),
    (">=", "≥"),
    ("!=", "≠"),
    ("->", "→"),
    ("=>", "⇒"),
    ("<-", "←"),
    ("&&", "∧"),
    ("||", "∨"),
];

/// Lisp-family `prettify-symbols-alist`: the classic `lambda` → `λ`.
const LISP: &[Symbol] = &[("lambda", "λ")];

/// Python's `prettify-symbols-alist`, as `python-mode` sets it.
const PYTHON: &[Symbol] = &[
    ("lambda", "λ"),
    ("not", "¬"),
    ("!=", "≠"),
    ("<=", "≤"),
    (">=", "≥"),
];

/// Haskell's, which is the richest of the standard ones.
const HASKELL: &[Symbol] = &[
    ("->", "→"),
    ("<-", "←"),
    ("=>", "⇒"),
    ("::", "∷"),
    ("\\", "λ"),
    ("/=", "≠"),
    ("<=", "≤"),
    (">=", "≥"),
];

/// The `prettify-symbols-alist` for a language, or `None` when the language has
/// none — Emacs' `prettify-symbols-mode` is a no-op in a mode that sets no alist,
/// and so is this.
pub fn symbols_for(language: &str) -> Option<&'static [Symbol]> {
    Some(match language {
        "haskell" | "purescript" | "elm" | "idris" => HASKELL,
        "python" => PYTHON,
        "lisp" | "scheme" | "clojure" | "elisp" | "emacs-lisp" | "racket" | "fennel" => LISP,
        "rust" | "c" | "cpp" | "go" | "java" | "javascript" | "typescript" | "tsx" | "zig"
        | "ocaml" | "scala" | "kotlin" | "swift" | "csharp" | "php" => OPERATORS,
        _ => return None,
    })
}

/// True when `c` can be part of an identifier — a word symbol like `lambda` only
/// prettifies when it stands alone, never inside `lambdas` or `my_lambda`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// The substitutions `prettify-symbols-mode` makes in `text` for `symbols`.
///
/// Longest match wins at each position, so `<=` prettifies before `<`. A symbol
/// made of word characters must stand as a whole word. Returns the overlays
/// ascending by char index, which is the order the renderer requires.
pub fn prettify(text: &str, symbols: &[Symbol]) -> Vec<Substitution> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        // Longest symbol first: `<=` must beat a hypothetical `<`.
        let hit = symbols
            .iter()
            .filter(|(from, _)| {
                let sym: Vec<char> = from.chars().collect();
                if i + sym.len() > chars.len() || chars[i..i + sym.len()] != sym[..] {
                    return false;
                }
                // A word-ish symbol needs word boundaries on both sides.
                let wordish = from.chars().all(is_word);
                if !wordish {
                    return true;
                }
                let before_ok = i == 0 || !is_word(chars[i - 1]);
                let after = i + sym.len();
                let after_ok = after >= chars.len() || !is_word(chars[after]);
                before_ok && after_ok
            })
            .max_by_key(|(from, _)| from.chars().count());

        let Some((from, to)) = hit else {
            i += 1;
            continue;
        };
        let width = from.chars().count();
        out.push(Substitution {
            char_idx: i,
            text: to,
        });
        // The rest of the token is swallowed by the glyph drawn at its start.
        for offset in 1..width {
            out.push(Substitution {
                char_idx: i + offset,
                text: "",
            });
        }
        i += width;
    }
    out
}

/// The glyph `glyphless-display-mode` draws for a character it considers
/// glyphless, or `None` when the character has a glyph of its own.
///
/// Emacs' `glyphless-char-display` covers control characters, C1 controls, the
/// format/bidi controls and unassigned code points. This covers the ones a
/// terminal actually mangles: the C0 controls (drawn as their Unicode Control
/// Pictures, which is Emacs' `acronym` style rendered properly), `DEL`, the C1
/// controls, and the zero-width / bidi format characters that are invisible and
/// therefore dangerous.
///
/// TAB and NEWLINE are excluded: the renderer already has its own handling for
/// them (`whitespace` rendering), and hiding that behind a box would fight it.
pub fn glyphless(c: char) -> Option<&'static str> {
    const PICTURES: [&str; 32] = [
        "␀", "␁", "␂", "␃", "␄", "␅", "␆", "␇", "␈", "␉", "␊", "␋", "␌", "␍", "␎", "␏", "␐", "␑",
        "␒", "␓", "␔", "␕", "␖", "␗", "␘", "␙", "␚", "␛", "␜", "␝", "␞", "␟",
    ];
    match c {
        // The renderer draws these itself.
        '\t' | '\n' | '\r' => None,
        c if (c as u32) < 32 => Some(PICTURES[c as usize]),
        '\u{7f}' => Some("␡"),
        // C1 controls — invisible and almost always a mistake in a text file.
        c if ('\u{80}'..='\u{9f}').contains(&c) => Some("¿"),
        // Zero-width and bidi format characters: invisible by design, which is
        // exactly why Emacs shows them.
        '\u{200b}'..='\u{200f}' => Some("·"),
        '\u{2028}' | '\u{2029}' => Some("¶"),
        '\u{202a}'..='\u{202e}' => Some("¤"),
        '\u{2066}'..='\u{2069}' => Some("¤"),
        '\u{feff}' => Some("·"),
        _ => None,
    }
}

/// The substitutions `glyphless-display-mode` makes in `text`.
pub fn glyphless_scan(text: &str) -> Vec<Substitution> {
    text.chars()
        .enumerate()
        .filter_map(|(i, c)| glyphless(c).map(|text| Substitution { char_idx: i, text }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the line renders as once the substitutions are applied — the check
    /// that actually matters, since a substitution list is only correct if it
    /// draws the right thing.
    fn rendered(text: &str, subs: &[Substitution]) -> String {
        let mut out = String::new();
        for (i, c) in text.chars().enumerate() {
            match subs.iter().find(|s| s.char_idx == i) {
                Some(sub) => out.push_str(sub.text),
                None => out.push(c),
            }
        }
        out
    }

    #[test]
    fn a_two_char_operator_draws_as_one_glyph_and_erases_its_tail() {
        let subs = prettify("a -> b", OPERATORS);
        assert_eq!(
            subs,
            vec![
                Substitution {
                    char_idx: 2,
                    text: "→"
                },
                Substitution {
                    char_idx: 3,
                    text: ""
                },
            ]
        );
        assert_eq!(rendered("a -> b", &subs), "a → b");
    }

    #[test]
    fn the_buffer_text_is_untouched_only_the_glyphs_differ() {
        let src = "if a <= b && c >= d";
        let subs = prettify(src, OPERATORS);
        // Each two-char operator collapses to one glyph plus an erased slot.
        assert_eq!(rendered(src, &subs), "if a ≤ b ∧ c ≥ d");
        // No substitution ever points past the text: prettify only *replaces*
        // graphemes, which is what keeps point and every char offset correct.
        assert!(subs.iter().all(|s| s.char_idx < src.chars().count()));
    }

    #[test]
    fn a_word_symbol_only_prettifies_as_a_whole_word() {
        let subs = prettify("lambda x: x", LISP);
        assert_eq!(rendered("lambda x: x", &subs), "λ x: x");
        // Six chars of `lambda` -> one glyph and five erased slots.
        assert_eq!(subs.len(), 6);
        assert!(
            prettify("lambdas", LISP).is_empty(),
            "`lambdas` is not `lambda`"
        );
        assert!(prettify("my_lambda", LISP).is_empty());
    }

    #[test]
    fn the_longest_symbol_wins() {
        // `<-` and `<=` share a prefix; each must match itself, not the other.
        let subs = prettify("<- <=", OPERATORS);
        assert_eq!(rendered("<- <=", &subs), "← ≤");
    }

    #[test]
    fn substitutions_come_out_ascending() {
        let subs = prettify("a -> b => c", OPERATORS);
        assert!(subs.windows(2).all(|w| w[0].char_idx < w[1].char_idx));
    }

    #[test]
    fn a_language_with_no_alist_prettifies_nothing() {
        assert!(symbols_for("markdown").is_none());
        assert!(symbols_for("toml").is_none());
        assert!(symbols_for("rust").is_some());
        assert!(symbols_for("haskell").is_some());
    }

    #[test]
    fn glyphless_draws_control_pictures_and_leaves_text_alone() {
        assert_eq!(glyphless('\u{1}'), Some("␁"));
        assert_eq!(glyphless('\u{7f}'), Some("␡"));
        assert_eq!(glyphless('a'), None);
        assert_eq!(glyphless('λ'), None);
    }

    #[test]
    fn glyphless_leaves_tab_and_newline_to_the_renderer() {
        assert_eq!(glyphless('\t'), None);
        assert_eq!(glyphless('\n'), None);
        assert_eq!(glyphless('\r'), None);
    }

    #[test]
    fn glyphless_reveals_a_zero_width_space() {
        let src = "a\u{200b}b";
        let subs = glyphless_scan(src);
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].char_idx, 1);
        assert_eq!(rendered(src, &subs), "a·b");
    }

    #[test]
    fn glyphless_scan_of_plain_text_is_empty() {
        assert!(glyphless_scan("plain ascii\tand a newline\n").is_empty());
    }
}
