use crate::chars::char_is_word;
use crate::movement::Direction;
use crate::RopeSlice;

// TODO: switch to std::str::Pattern when it is stable.
pub trait CharMatcher {
    fn char_match(&self, ch: char) -> bool;
}

impl CharMatcher for char {
    fn char_match(&self, ch: char) -> bool {
        *self == ch
    }
}

impl<F: Fn(&char) -> bool> CharMatcher for F {
    fn char_match(&self, ch: char) -> bool {
        (*self)(&ch)
    }
}

// Finds the positions of the nth matching character in given direction
// starting from the pos gap-index (see Range struct for explanation)
pub fn find_nth_char<M: CharMatcher>(
    mut n: usize,
    text: RopeSlice,
    char_matcher: M,
    mut pos: usize,
    direction: Direction,
) -> Option<usize> {
    if n == 0 {
        return None;
    }

    let mut chars = text.get_chars_at(pos)?;

    match direction {
        Direction::Forward => loop {
            let c = chars.next()?;
            if char_matcher.char_match(c) {
                n -= 1;
                if n == 0 {
                    return Some(pos);
                }
            }
            pos += 1;
        },
        Direction::Backward => loop {
            let c = chars.prev()?;
            pos -= 1;
            if char_matcher.char_match(c) {
                n -= 1;
                if n == 0 {
                    return Some(pos);
                }
            }
        },
    };
}

// ---------------------------------------------------------------------------
// Incremental search (Emacs isearch) — pure, unit-tested helpers
//
// zemacs's live `/` search stores the pattern in the `/` register and matches
// it with the `rope::Regex` engine. These helpers turn a typed isearch string
// (plus the active toggle flags) into the regex to hand that engine, and grab
// the buffer text that `isearch-yank-*` pulls into the search string.
// ---------------------------------------------------------------------------

/// The toggle state of an in-progress incremental search, mirroring the Emacs
/// `isearch-mode` variables that each `isearch-toggle-*` command flips.
///
/// The flags that change matching in zemacs are `regexp`, `word`/`symbol`,
/// `case_fold` (via [`IsearchFlags::build_regex`] and
/// [`IsearchFlags::is_case_insensitive`]), `lax_whitespace` and `char_fold`
/// (both honored for non-regexp searches). `invisible` is tracked for parity but
/// has no matching effect (zemacs has no invisible text), so it is documented as
/// a no-op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsearchFlags {
    /// Interpret the search string as a regexp (`isearch-toggle-regexp`).
    pub regexp: bool,
    /// Word search: match whole words (`isearch-toggle-word`).
    pub word: bool,
    /// Symbol search: match whole symbols (`isearch-toggle-symbol`).
    pub symbol: bool,
    /// Case-fold: match case-insensitively unless the string has an uppercase
    /// letter (`isearch-toggle-case-fold`; smart-case, like Emacs default).
    pub case_fold: bool,
    /// A space matches a run of whitespace (`isearch-toggle-lax-whitespace`).
    pub lax_whitespace: bool,
    /// Character folding, e.g. match `a` against `ä` (`isearch-toggle-char-fold`).
    /// Honored for non-regexp searches via [`char_fold_regexp`].
    pub char_fold: bool,
    /// Match inside invisible/folded text (`isearch-toggle-invisible`). No
    /// matching effect in zemacs; tracked only for parity.
    pub invisible: bool,
}

impl Default for IsearchFlags {
    fn default() -> Self {
        // Emacs defaults: case-fold and lax-whitespace on, everything else off.
        IsearchFlags {
            regexp: false,
            word: false,
            symbol: false,
            case_fold: true,
            lax_whitespace: true,
            char_fold: false,
            invisible: false,
        }
    }
}

impl IsearchFlags {
    /// Whether the search should ignore case for `raw`. With `case_fold` on this
    /// is smart-case (Emacs `search-upper-case` = `not-yanks`): fold unless the
    /// string contains an uppercase letter. With `case_fold` off, never fold.
    pub fn is_case_insensitive(&self, raw: &str) -> bool {
        self.case_fold && !raw.chars().any(|c| c.is_uppercase())
    }

    /// Build the regex string to hand the search engine for the typed `raw`
    /// string under these flags. Returns `""` for an empty (or all-separator
    /// under word/symbol search) string.
    pub fn build_regex(&self, raw: &str) -> String {
        if raw.is_empty() {
            return String::new();
        }
        if self.word || self.symbol {
            return token_search_regexp(raw, self.lax_whitespace);
        }
        if self.regexp {
            // Already a regexp; only fold whitespace if asked.
            if self.lax_whitespace {
                lax_whitespace_regexp(raw)
            } else {
                raw.to_string()
            }
        } else {
            // A literal search: regexp-quote it, expanding each character into its
            // char-fold class first when `char_fold` is on (Emacs applies
            // character folding to literal searches only).
            let quoted = if self.char_fold {
                char_fold_regexp(raw)
            } else {
                regex::escape(raw)
            };
            if self.lax_whitespace {
                lax_whitespace_regexp(&quoted)
            } else {
                quoted
            }
        }
    }
}

// --- character folding (Emacs `char-fold-table` / `char-fold-to-regexp`) -----
//
// Emacs builds its fold table from the Unicode canonical decompositions: a
// search for `e` also matches every character that decomposes to `e` plus
// combining marks (`é`, `ê`, `ế`, …). `CHAR_FOLD` is that same table for the
// ASCII letters, computed from the canonical decompositions of U+0080..U+20FF
// (Latin-1 Supplement through Latin Extended Additional, i.e. every precomposed
// Latin letter including the Vietnamese ones). `PUNCT_FOLD` adds the quote and
// dash equivalences Emacs's table also carries.
//
// Not covered (documented, not silently missing): ligature and multi-character
// decompositions (`ﬁ` → `fi`), and the non-Latin scripts, whose folds Emacs
// derives from the same table but which zemacs does not tabulate.

/// For each ASCII letter, the precomposed characters that canonically decompose
/// to it.
const CHAR_FOLD: [(char, &str); 50] = [
    ('A', "ÀÁÂÃÄÅĀĂĄǍǞǠǺȀȂȦḀẠẢẤẦẨẪẬẮẰẲẴẶ"),
    ('B', "ḂḄḆ"),
    ('C', "ÇĆĈĊČḈ"),
    ('D', "ĎḊḌḎḐḒ"),
    ('E', "ÈÉÊËĒĔĖĘĚȄȆȨḔḖḘḚḜẸẺẼẾỀỂỄỆ"),
    ('F', "Ḟ"),
    ('G', "ĜĞĠĢǦǴḠ"),
    ('H', "ĤȞḢḤḦḨḪ"),
    ('I', "ÌÍÎÏĨĪĬĮİǏȈȊḬḮỈỊ"),
    ('J', "Ĵ"),
    ('K', "ĶǨḰḲḴ"),
    ('L', "ĹĻĽḶḸḺḼ"),
    ('M', "ḾṀṂ"),
    ('N', "ÑŃŅŇǸṄṆṈṊ"),
    ('O', "ÒÓÔÕÖŌŎŐƠǑǪǬȌȎȪȬȮȰṌṎṐṒỌỎỐỒỔỖỘỚỜỞỠỢ"),
    ('P', "ṔṖ"),
    ('R', "ŔŖŘȐȒṘṚṜṞ"),
    ('S', "ŚŜŞŠȘṠṢṤṦṨ"),
    ('T', "ŢŤȚṪṬṮṰ"),
    ('U', "ÙÚÛÜŨŪŬŮŰŲƯǓǕǗǙǛȔȖṲṴṶṸṺỤỦỨỪỬỮỰ"),
    ('V', "ṼṾ"),
    ('W', "ŴẀẂẄẆẈ"),
    ('X', "ẊẌ"),
    ('Y', "ÝŶŸȲẎỲỴỶỸ"),
    ('Z', "ŹŻŽẐẒẔ"),
    ('a', "àáâãäåāăąǎǟǡǻȁȃȧḁạảấầẩẫậắằẳẵặ"),
    ('b', "ḃḅḇ"),
    ('c', "çćĉċčḉ"),
    ('d', "ďḋḍḏḑḓ"),
    ('e', "èéêëēĕėęěȅȇȩḕḗḙḛḝẹẻẽếềểễệ"),
    ('f', "ḟ"),
    ('g', "ĝğġģǧǵḡ"),
    ('h', "ĥȟḣḥḧḩḫẖ"),
    ('i', "ìíîïĩīĭįǐȉȋḭḯỉị"),
    ('j', "ĵǰ"),
    ('k', "ķǩḱḳḵ"),
    ('l', "ĺļľḷḹḻḽ"),
    ('m', "ḿṁṃ"),
    ('n', "ñńņňǹṅṇṉṋ"),
    ('o', "òóôõöōŏőơǒǫǭȍȏȫȭȯȱṍṏṑṓọỏốồổỗộớờởỡợ"),
    ('p', "ṕṗ"),
    ('r', "ŕŗřȑȓṙṛṝṟ"),
    ('s', "śŝşšșṡṣṥṧṩ"),
    ('t', "ţťțṫṭṯṱẗ"),
    ('u', "ùúûüũūŭůűųưǔǖǘǚǜȕȗṳṵṷṹṻụủứừửữự"),
    ('v', "ṽṿ"),
    ('w', "ŵẁẃẅẇẉẘ"),
    ('x', "ẋẍ"),
    ('y', "ýÿŷȳẏẙỳỵỷỹ"),
    ('z', "źżžẑẓẕ"),
];

/// The punctuation folds: typographic quotes and dashes match their ASCII form.
const PUNCT_FOLD: [(char, &str); 3] = [('\'', "‘’‚‛′"), ('"', "“”„‟″"), ('-', "‐‑‒–—―")];

/// The characters `c` also matches under character folding (Emacs
/// `char-fold-table`), or `None` when `c` has no fold equivalents.
pub fn char_fold_variants(c: char) -> Option<&'static str> {
    CHAR_FOLD
        .iter()
        .chain(PUNCT_FOLD.iter())
        .find(|(base, _)| *base == c)
        .map(|(_, v)| *v)
}

/// Regexp-quote `raw`, expanding every character that has fold equivalents into
/// an alternation of itself and them (Emacs `char-fold-to-regexp`). A character
/// with no equivalents is quoted as-is, so the result is always a literal search
/// widened by folding — never a looser pattern.
pub fn char_fold_regexp(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    for c in raw.chars() {
        match char_fold_variants(c) {
            Some(variants) => {
                out.push_str("(?:");
                out.push_str(&regex::escape(&c.to_string()));
                for v in variants.chars() {
                    out.push('|');
                    out.push_str(&regex::escape(&v.to_string()));
                }
                out.push(')');
            }
            None => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out
}

/// Replace each run of spaces in `pat` with a "match any whitespace run" class,
/// implementing `isearch-lax-whitespace` (`search-whitespace-regexp`). Leading
/// and trailing spaces are preserved as literal single-space classes so an
/// intentional edge space still requires whitespace there.
fn lax_whitespace_regexp(pat: &str) -> String {
    let mut out = String::with_capacity(pat.len());
    let mut chars = pat.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' ' {
            while chars.peek() == Some(&' ') {
                chars.next();
            }
            out.push_str("[ \\t]+");
        } else {
            out.push(c);
        }
    }
    out
}

/// Build the regexp for `isearch-forward-word` / `isearch-forward-symbol`: split
/// `raw` into its word/symbol tokens (runs of word constituents), regexp-quote
/// each, join them so intervening separators are matched loosely, and (unless
/// `lax`) anchor the whole thing at word boundaries so only whole words match.
///
/// zemacs's regex engine has no Emacs symbol-boundary escape (`\_<`/`\_>`), so
/// both word and symbol search use `\b` word boundaries over word constituents
/// (`char_is_word`, i.e. alphanumerics and `_`); the two therefore match the
/// same whole tokens here.
pub fn token_search_regexp(raw: &str, lax: bool) -> String {
    let tokens: Vec<&str> = raw
        .split(|c| !char_is_word(c))
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.is_empty() {
        return String::new();
    }
    let body = tokens
        .iter()
        .map(|t| regex::escape(t))
        .collect::<Vec<_>>()
        .join("\\W+");
    if lax {
        body
    } else {
        format!("\\b{body}\\b")
    }
}

/// The single character at char index `pos`, as `isearch-yank-char` would pull
/// it into the search string. `None` past the end of `text`.
pub fn grab_char(text: RopeSlice, pos: usize) -> Option<String> {
    if pos >= text.len_chars() {
        None
    } else {
        Some(text.char(pos).to_string())
    }
}

/// The text `isearch-yank-word-or-char` pulls in at char index `pos`: the whole
/// word constituent run starting at `pos` if `pos` is on one, otherwise the
/// single character there. Empty at/after end of buffer.
pub fn grab_word_or_char(text: RopeSlice, pos: usize) -> String {
    let len = text.len_chars();
    if pos >= len {
        return String::new();
    }
    if char_is_word(text.char(pos)) {
        let mut end = pos;
        while end < len && char_is_word(text.char(end)) {
            end += 1;
        }
        text.slice(pos..end).to_string()
    } else {
        text.char(pos).to_string()
    }
}

/// The text `isearch-yank-word` pulls in at char index `pos`: skip any leading
/// non-word characters, then take the following whole word. Empty if no word
/// remains.
pub fn grab_word(text: RopeSlice, pos: usize) -> String {
    let len = text.len_chars();
    let mut start = pos;
    while start < len && !char_is_word(text.char(start)) {
        start += 1;
    }
    let mut end = start;
    while end < len && char_is_word(text.char(end)) {
        end += 1;
    }
    text.slice(start..end).to_string()
}

/// The text `isearch-yank-line` pulls in at char index `pos`: from `pos` to the
/// end of its line, excluding the trailing newline.
pub fn grab_line(text: RopeSlice, pos: usize) -> String {
    let len = text.len_chars();
    let mut end = pos;
    while end < len {
        let c = text.char(end);
        if c == '\n' || c == '\r' {
            break;
        }
        end += 1;
    }
    if pos >= len {
        String::new()
    } else {
        text.slice(pos..end).to_string()
    }
}

/// The text `isearch-yank-until-char` pulls in at char index `pos`: from `pos`
/// up to (but not including) the first occurrence of `target`. If `target` is
/// not found before end of buffer, grabs to the end.
pub fn grab_until_char(text: RopeSlice, pos: usize, target: char) -> String {
    let len = text.len_chars();
    let mut end = pos;
    while end < len && text.char(end) != target {
        end += 1;
    }
    if pos >= len {
        String::new()
    } else {
        text.slice(pos..end).to_string()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::movement::Direction;

    #[test]
    fn test_find_nth_char() {
        let text = RopeSlice::from("aa ⌚aa \r\n aa");

        // Forward direction
        assert_eq!(find_nth_char(1, text, 'a', 5, Direction::Forward), Some(5));
        assert_eq!(find_nth_char(2, text, 'a', 5, Direction::Forward), Some(10));
        assert_eq!(find_nth_char(3, text, 'a', 5, Direction::Forward), Some(11));
        assert_eq!(find_nth_char(4, text, 'a', 5, Direction::Forward), None);

        // Backward direction
        assert_eq!(find_nth_char(1, text, 'a', 5, Direction::Backward), Some(4));
        assert_eq!(find_nth_char(2, text, 'a', 5, Direction::Backward), Some(1));
        assert_eq!(find_nth_char(3, text, 'a', 5, Direction::Backward), Some(0));
        assert_eq!(find_nth_char(4, text, 'a', 5, Direction::Backward), None);

        // Edge cases
        assert_eq!(find_nth_char(0, text, 'a', 5, Direction::Forward), None); // n = 0
        assert_eq!(find_nth_char(1, text, 'x', 5, Direction::Forward), None); // Not found
        assert_eq!(find_nth_char(1, text, 'a', 20, Direction::Forward), None); // Beyond text
        assert_eq!(find_nth_char(1, text, 'a', 0, Direction::Backward), None); // At start going backward
    }

    #[test]
    fn test_isearch_build_regex_plain() {
        let f = IsearchFlags::default();
        // Plain, non-regexp search regexp-quotes metacharacters.
        assert_eq!(f.build_regex("a.b*"), "a\\.b\\*");
        assert_eq!(f.build_regex(""), "");
    }

    #[test]
    fn test_isearch_build_regex_flags() {
        // Regexp search passes the pattern through (whitespace kept literal when
        // lax-whitespace is off).
        let f = IsearchFlags {
            regexp: true,
            lax_whitespace: false,
            ..Default::default()
        };
        assert_eq!(f.build_regex("a.b"), "a.b");

        // Word/symbol search anchors whole tokens at word boundaries.
        let w = IsearchFlags {
            word: true,
            lax_whitespace: false,
            ..Default::default()
        };
        assert_eq!(w.build_regex("foo bar"), "\\bfoo\\W+bar\\b");
        // Symbol search behaves the same over word constituents.
        let s = IsearchFlags {
            symbol: true,
            lax_whitespace: false,
            ..Default::default()
        };
        assert_eq!(s.build_regex("foo_bar"), "\\bfoo_bar\\b");
        // A metacharacter inside a token is quoted.
        assert_eq!(w.build_regex("a.b c"), "\\ba\\W+b\\W+c\\b");
    }

    #[test]
    fn test_isearch_lax_whitespace() {
        let f = IsearchFlags::default(); // lax_whitespace on
        assert_eq!(f.build_regex("a b"), "a[ \\t]+b");
        // Collapsed runs of spaces.
        assert_eq!(f.build_regex("a   b"), "a[ \\t]+b");
    }

    #[test]
    fn test_isearch_case_fold() {
        let f = IsearchFlags::default(); // case_fold on
        assert!(f.is_case_insensitive("foo")); // no uppercase -> fold
        assert!(!f.is_case_insensitive("Foo")); // uppercase -> no fold (smart case)
        let off = IsearchFlags {
            case_fold: false,
            ..Default::default()
        };
        assert!(!off.is_case_insensitive("foo")); // folding disabled
    }

    #[test]
    fn test_char_fold_regexp_matches_accented_forms() {
        // The folded pattern for a plain letter matches the letter and every
        // precomposed form that decomposes to it.
        let re = regex::Regex::new(&char_fold_regexp("resume")).unwrap();
        assert!(re.is_match("resume"));
        assert!(re.is_match("résumé"));
        assert!(re.is_match("rêsumè"));
        // Vietnamese multi-mark forms decompose to the same base letter.
        assert!(regex::Regex::new(&char_fold_regexp("e"))
            .unwrap()
            .is_match("ế"));
        // Folding never widens beyond the fold classes: a different letter still
        // does not match.
        assert!(!re.is_match("resome"));
    }

    #[test]
    fn test_char_fold_quotes_and_unfoldable_chars() {
        // Typographic quotes fold onto the ASCII ones.
        let re = regex::Regex::new(&char_fold_regexp("don't")).unwrap();
        assert!(re.is_match("don’t"));
        assert!(re.is_match("don't"));
        // A character with no equivalents is quoted literally (regexp metachars
        // must not leak through as syntax).
        assert_eq!(char_fold_variants('7'), None);
        let dot = regex::Regex::new(&char_fold_regexp("a.b")).unwrap();
        assert!(dot.is_match("a.b"));
        assert!(!dot.is_match("axb"));
    }

    #[test]
    fn test_isearch_build_regex_char_fold_flag() {
        let folded = IsearchFlags {
            char_fold: true,
            lax_whitespace: false,
            ..Default::default()
        };
        let re = regex::Regex::new(&folded.build_regex("cafe")).unwrap();
        assert!(re.is_match("café"));
        // With the flag off the same string is a plain literal search.
        let plain = IsearchFlags {
            lax_whitespace: false,
            ..Default::default()
        };
        assert_eq!(plain.build_regex("cafe"), "cafe");
        // char-fold applies to literal searches only — a regexp search is passed
        // through untouched (Emacs folds `char-fold-to-regexp` on the literal).
        let rx = IsearchFlags {
            char_fold: true,
            regexp: true,
            lax_whitespace: false,
            ..Default::default()
        };
        assert_eq!(rx.build_regex("a.b"), "a.b");
    }

    #[test]
    fn test_isearch_yank_grabs() {
        let text = RopeSlice::from("foo_bar baz\nnext");
        // char
        assert_eq!(grab_char(text, 0), Some("f".to_string()));
        assert_eq!(grab_char(text, 16), None);
        // word-or-char: on a word constituent grabs the whole token (incl `_`)
        assert_eq!(grab_word_or_char(text, 0), "foo_bar");
        // on a separator grabs just that char
        assert_eq!(grab_word_or_char(text, 7), " ");
        // word: skip leading separators then grab the word
        assert_eq!(grab_word(text, 7), "baz");
        assert_eq!(grab_word(text, 0), "foo_bar");
        // line: to end of line, excluding newline
        assert_eq!(grab_line(text, 0), "foo_bar baz");
        assert_eq!(grab_line(text, 8), "baz");
        // until-char
        assert_eq!(grab_until_char(text, 0, '_'), "foo");
        assert_eq!(grab_until_char(text, 0, 'z'), "foo_bar ba");
        assert_eq!(grab_until_char(text, 0, 'X'), "foo_bar baz\nnext");
    }
}
