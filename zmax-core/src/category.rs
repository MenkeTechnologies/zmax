//! Emacs character categories — the second, non-exclusive classification of
//! characters that sits alongside the syntax table.
//!
//! A category has a one-character mnemonic (a printing ASCII character) and a
//! docstring; a character belongs to any number of categories at once. The
//! primitives are the three the Emacs Lisp manual documents in its "Categories"
//! node: [`char_category_set`] (which categories a character is in),
//! [`category_set_mnemonics`] (that set rendered as a string) and
//! [`modify_category_entry`] (add or remove a character, or a whole range, from
//! a category).
//!
//! The standard categories and the characters they start out holding are ported
//! from `lisp/international/characters.el` and `lisp/international/kinsoku.el`.
//! Emacs's table is per-buffer (each buffer starts with a copy of the standard
//! table); this one is the standard table alone, shared by every buffer, so
//! `modify-category-entry` here is what Emacs's `(modify-category-entry …
//! (standard-category-table))` is.
//!
//! The real consumer is line breaking: `|` marks a character a line may be
//! broken *at* even without whitespace (CJK), `>` marks one that must not start
//! a line and `<` one that must not end a line — Emacs's kinsoku rules.

use std::sync::{OnceLock, RwLock};

/// One category: its mnemonic and what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Category {
    /// The single ASCII character that names the category.
    pub mnemonic: char,
    /// The first line of the docstring — the short name Emacs shows first.
    pub name: &'static str,
    /// The rest of the docstring, empty when the category has only a name.
    pub doc: &'static str,
}

/// The standard categories, in `characters.el`'s definition order, with their
/// docstrings verbatim.
pub const STANDARD_CATEGORIES: &[Category] = &[
    Category { mnemonic: 'a', name: "ASCII", doc: "ASCII graphic characters 32-126 (ISO646 IRV:1983[4/0])" },
    Category { mnemonic: 'l', name: "Latin", doc: "" },
    Category { mnemonic: 't', name: "Thai", doc: "" },
    Category { mnemonic: 'g', name: "Greek", doc: "" },
    Category { mnemonic: 'b', name: "Arabic", doc: "" },
    Category { mnemonic: 'w', name: "Hebrew", doc: "" },
    Category { mnemonic: 'y', name: "Cyrillic", doc: "" },
    Category { mnemonic: 'k', name: "Katakana", doc: "Japanese katakana" },
    Category { mnemonic: 'r', name: "Roman", doc: "Japanese roman" },
    Category { mnemonic: 'c', name: "Chinese", doc: "" },
    Category { mnemonic: 'j', name: "Japanese", doc: "" },
    Category { mnemonic: 'h', name: "Korean", doc: "" },
    Category { mnemonic: 'e', name: "Ethiopic", doc: "Ethiopic (Ge'ez)" },
    Category { mnemonic: 'v', name: "Viet", doc: "Vietnamese" },
    Category { mnemonic: 'i', name: "Indian", doc: "" },
    Category { mnemonic: 'o', name: "Lao", doc: "" },
    Category { mnemonic: 'q', name: "Tibetan", doc: "" },
    Category { mnemonic: 'A', name: "2-byte alnum", doc: "Alphanumeric characters of 2-byte character sets" },
    Category { mnemonic: 'C', name: "2-byte han", doc: "Chinese (Han) characters of 2-byte character sets" },
    Category { mnemonic: 'G', name: "2-byte Greek", doc: "Greek characters of 2-byte character sets" },
    Category { mnemonic: 'H', name: "2-byte Hiragana", doc: "Japanese Hiragana characters of 2-byte character sets" },
    Category { mnemonic: 'K', name: "2-byte Katakana", doc: "Japanese Katakana characters of 2-byte character sets" },
    Category { mnemonic: 'N', name: "2-byte Korean", doc: "Korean Hangul characters of 2-byte character sets" },
    Category { mnemonic: 'Y', name: "2-byte Cyrillic", doc: "Cyrillic characters of 2-byte character sets" },
    Category { mnemonic: '6', name: "digit", doc: "" },
    Category { mnemonic: '|', name: "line breakable", doc: "While filling, we can break a line at this character." },
    Category { mnemonic: ' ', name: "space for indent", doc: "This character counts as a space for indentation purposes." },
    Category { mnemonic: '>', name: "Not at bol", doc: "A character which can't be placed at beginning of line." },
    Category { mnemonic: '<', name: "Not at eol", doc: "A character which can't be placed at end of line." },
    Category { mnemonic: '.', name: "Base", doc: "Base characters (Unicode General Category L,N,P,S,Zs)" },
    Category { mnemonic: '^', name: "Combining", doc: "Combining diacritic or mark (Unicode General Category M)" },
    Category { mnemonic: 'R', name: "Strong R2L", doc: "Characters with \"strong\" right-to-left directionality, i.e. with R, AL, RLE, or RLO Unicode bidi character type." },
    Category { mnemonic: 'L', name: "Strong L2R", doc: "Characters with \"strong\" left-to-right directionality, i.e. with L, LRE, or LRO Unicode bidi character type." },
];

/// The code-point ranges each script category starts out holding, ported from
/// the `modify-category-entry` calls at the head of `characters.el`. The
/// categories that `characters.el` fills in from a legacy charset (`map-charset-chars`)
/// use the equivalent Unicode block here, since zmax has no charset registry.
const RANGES: &[(char, u32, u32)] = &[
    // ASCII and Latin.
    ('a', 32, 127),
    ('l', 32, 127),
    ('l', 0x80, 0x24F),
    ('l', 0x1E00, 0x1EF9),
    ('l', 0x2C60, 0x2C7F),
    ('l', 0xA720, 0xA7FF),
    // Greek (including Coptic, which characters.el also gives `g').
    ('g', 0x0370, 0x03FF),
    ('g', 0x1F00, 0x1FFF),
    ('g', 0x2C80, 0x2CFF),
    // Cyrillic.
    ('y', 0x0400, 0x04FF),
    ('y', 0x1C80, 0x1C8F),
    ('y', 0xA640, 0xA69F),
    // Hebrew, Arabic.
    ('w', 0x0590, 0x05FF),
    ('b', 0x0600, 0x06FF),
    ('b', 0x0870, 0x08FF),
    ('b', 0xFB50, 0xFDCF),
    ('b', 0xFDF0, 0xFDFF),
    ('b', 0xFE70, 0xFEFE),
    // The Brahmic and South-East Asian scripts.
    ('i', 0x0901, 0x0970),
    ('t', 0x0E00, 0x0E7F),
    ('o', 0x0E80, 0x0EFF),
    ('q', 0x0F00, 0x0FFF),
    ('e', 0x1200, 0x1399),
    ('e', 0x2D80, 0x2DDE),
    // Han.
    ('C', 0x3400, 0x4DBF),
    ('C', 0x4E00, 0x9FFF),
    ('C', 0xF900, 0xFAFF),
    ('C', 0x20000, 0x2FFFF),
    ('c', 0x3400, 0x9FFF),
    ('c', 0xF900, 0xFAFF),
    ('c', 0x20000, 0x2FFFF),
    // Kana.
    ('H', 0x3040, 0x309F),
    ('K', 0x3099, 0x309C),
    ('K', 0x30A0, 0x30FF),
    ('K', 0x31F0, 0x31FF),
    ('k', 0xFF66, 0xFF9F),
    ('j', 0x3040, 0x30FF),
    ('j', 0x31F0, 0x31FF),
    ('j', 0xFF01, 0xFF9F),
    // Hangul.
    ('h', 0x1100, 0x11FF),
    ('h', 0xAC00, 0xD7A3),
    ('N', 0xAC00, 0xD7A3),
    // Line-breakable (the `|' category): CJK, kana and the fullwidth forms.
    ('|', 0x2E80, 0x312F),
    ('|', 0x3190, 0x33FF),
    ('|', 0x3400, 0x9FFF),
    ('|', 0xF900, 0xFAFF),
    ('|', 0xFF01, 0xFF60),
    ('|', 0x3040, 0x3096),
    ('|', 0x30A0, 0x30FA),
    ('|', 0x20000, 0x2FFFF),
];

/// `kinsoku-bol` from `kinsoku.el` — the characters that get category `>`
/// because a line must not *begin* with them.
const KINSOKU_BOL: &str = concat!(
    // ASCII
    "!)-_~}]:;',.?",
    // Katakana JISX0201 (halfwidth)
    "｡｣ｧｨｩｪｫｬｭｮｯｰﾞﾟ",
    // Japanese JISX0208
    "、。，．・：；？！゛゜´｀¨＾￣＿ヽヾゝゞ〃仝々〆〇ー—‐",
    "／＼〜‖｜…‥’”）〕］｝〉》」』】°′″℃",
    "ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ",
    // Chinese GB2312
    "、。．，・ˉˇ¨〃々―～‖…’”）〕〉》」』〗",
    "】；：？！±×÷∶°′″℃／＼＂＿￣｜ㄥ",
    // Chinese BIG5
    "，、。．‧；：？！︰…‥﹐﹑﹒·﹔",
    "﹕﹖﹗｜–︱—︳╴︴﹏）︶｝︸〕︺】",
    "︼》︾〉﹀」﹂』﹄﹚﹜﹞’”〞′〃",
    "¯￣＿ˍ﹉﹊﹍﹎﹋﹌×÷±℃℉﹩°ㄥ",
);

/// `kinsoku-eol` from `kinsoku.el` — the characters that get category `<`
/// because a line must not *end* with them.
const KINSOKU_EOL: &str = concat!(
    // ASCII
    "({[`",
    // JISX0201 Katakana
    "｢",
    // Japanese JISX0208
    "‘“（〔［｛〈《「『【°′″℃＠§",
    // Chinese GB2312
    "‘“＂（〔〈《「『〖【°′″＠℃§",
    "ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙㄨ",
    "（︵｛︷〔︹【︻《︽〈︿「﹁『﹃﹙﹛﹝",
    // Chinese BIG5
    "‘“〝‵′〃§＠℃℉﹫°ㄅㄆㄇㄈㄉㄊㄋ",
    "ㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙㄨ",
);

/// One `modify-category-entry` call: a code-point range, the category, and
/// whether it was an addition or a removal (`reset`).
#[derive(Debug, Clone, Copy)]
struct Change {
    lo: u32,
    hi: u32,
    category: char,
    add: bool,
}

/// Everything a user has changed at run time: the categories they defined and
/// the entries they modified, applied over the standard table in order.
#[derive(Default)]
struct Table {
    /// `define-category`'s additions: mnemonic → docstring, in definition order.
    defined: Vec<(char, String)>,
    changes: Vec<Change>,
}

fn table() -> &'static RwLock<Table> {
    static T: OnceLock<RwLock<Table>> = OnceLock::new();
    T.get_or_init(Default::default)
}

/// Whether `ch` is in category `mnemonic` in the *standard* table, before any
/// `modify-category-entry` the user has made.
fn standard_member(ch: char, mnemonic: char) -> bool {
    use unicode_general_category::{get_general_category, GeneralCategory as G};
    let code = ch as u32;
    match mnemonic {
        // Computed categories: these are properties of the character, which is
        // how characters.el fills them in (it walks the Unicode tables).
        '.' => matches!(
            get_general_category(ch),
            G::UppercaseLetter
                | G::LowercaseLetter
                | G::TitlecaseLetter
                | G::ModifierLetter
                | G::OtherLetter
                | G::DecimalNumber
                | G::LetterNumber
                | G::OtherNumber
                | G::ConnectorPunctuation
                | G::DashPunctuation
                | G::OpenPunctuation
                | G::ClosePunctuation
                | G::InitialPunctuation
                | G::FinalPunctuation
                | G::OtherPunctuation
                | G::MathSymbol
                | G::CurrencySymbol
                | G::ModifierSymbol
                | G::OtherSymbol
                | G::SpaceSeparator
        ),
        '^' => matches!(
            get_general_category(ch),
            G::NonspacingMark | G::SpacingMark | G::EnclosingMark
        ),
        '6' => get_general_category(ch) == G::DecimalNumber,
        'R' => crate::bidi::is_strong_rtl(ch),
        'L' => crate::bidi::is_strong_ltr(ch),
        ' ' => ch == ' ' || ch == '\t',
        '>' => KINSOKU_BOL.contains(ch),
        '<' => KINSOKU_EOL.contains(ch),
        // Everything else is a plain range table.
        _ => RANGES
            .iter()
            .any(|&(cat, lo, hi)| cat == mnemonic && (lo..=hi).contains(&code)),
    }
}

/// Emacs `char-category-set`: the categories `ch` belongs to, as their mnemonics
/// in ascending order. Emacs returns a bool-vector indexed by category; this
/// returns the same information as the set of mnemonics that are `t` in it.
pub fn char_category_set(ch: char) -> Vec<char> {
    let t = table().read().unwrap();
    let mut out: Vec<char> = STANDARD_CATEGORIES
        .iter()
        .map(|c| c.mnemonic)
        .chain(t.defined.iter().map(|(m, _)| *m))
        .filter(|&m| {
            let mut member = standard_member(ch, m);
            for change in &t.changes {
                if change.category == m && (change.lo..=change.hi).contains(&(ch as u32)) {
                    member = change.add;
                }
            }
            member
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Emacs `category-set-mnemonics`: a category set rendered as the string of the
/// mnemonics it contains.
pub fn category_set_mnemonics(set: &[char]) -> String {
    set.iter().collect()
}

/// True when `ch` is in the single category `mnemonic`. This is the query the
/// line-breaking code makes; it avoids building the whole set.
pub fn in_category(ch: char, mnemonic: char) -> bool {
    let t = table().read().unwrap();
    let mut member = standard_member(ch, mnemonic);
    for change in &t.changes {
        if change.category == mnemonic && (change.lo..=change.hi).contains(&(ch as u32)) {
            member = change.add;
        }
    }
    member
}

/// Emacs `modify-category-entry`: add every character in `lo..=hi` to
/// `category`, or remove them from it when `reset` is true.
pub fn modify_category_entry(lo: char, hi: char, category: char, reset: bool) {
    let mut t = table().write().unwrap();
    t.changes.push(Change {
        lo: lo as u32,
        hi: hi as u32,
        category,
        add: !reset,
    });
}

/// Emacs `define-category`: register a new category with `mnemonic` and
/// `docstring`. Returns `Err` when the mnemonic is already taken or is not a
/// printing ASCII character, which is what Emacs signals.
pub fn define_category(mnemonic: char, docstring: &str) -> Result<(), String> {
    if !(' '..='~').contains(&mnemonic) {
        return Err(format!(
            "Category name must be a printing ASCII character, not {mnemonic:?}"
        ));
    }
    if category_docstring(mnemonic).is_some() {
        return Err(format!("Category `{mnemonic}' is already defined"));
    }
    table()
        .write()
        .unwrap()
        .defined
        .push((mnemonic, docstring.to_string()));
    Ok(())
}

/// Emacs `category-docstring`: what a category means, or `None` when nothing has
/// defined that mnemonic.
pub fn category_docstring(mnemonic: char) -> Option<String> {
    if let Some(c) = STANDARD_CATEGORIES.iter().find(|c| c.mnemonic == mnemonic) {
        return Some(if c.doc.is_empty() {
            c.name.to_string()
        } else {
            format!("{}\n{}", c.name, c.doc)
        });
    }
    let t = table().read().unwrap();
    t.defined
        .iter()
        .find(|(m, _)| *m == mnemonic)
        .map(|(_, doc)| doc.clone())
}

/// Every defined category — the standard ones plus whatever `define-category`
/// has added — as (mnemonic, docstring) pairs, for `describe-categories`.
pub fn categories() -> Vec<(char, String)> {
    let t = table().read().unwrap();
    STANDARD_CATEGORIES
        .iter()
        .map(|c| {
            (
                c.mnemonic,
                if c.doc.is_empty() {
                    c.name.to_string()
                } else {
                    format!("{}\n{}", c.name, c.doc)
                },
            )
        })
        .chain(t.defined.iter().cloned())
        .collect()
}

/// Emacs `get-unused-category`: a mnemonic no category is using yet.
pub fn unused_category() -> Option<char> {
    (' '..='~').find(|&c| category_docstring(c).is_none())
}

// ---------------------------------------------------------------------------
// The consumer: line breaking (Emacs's `fill-find-break-point` / kinsoku.el).
// ---------------------------------------------------------------------------

/// Whether a line may be broken *between* `before` and `after` with no
/// whitespace there, which is true for the CJK characters in category `|`.
/// The kinsoku rules veto the break when it would leave a character that may not
/// end a line (`<`) at the end, or put one that may not begin a line (`>`) at
/// the start.
pub fn can_break_between(before: char, after: char) -> bool {
    (in_category(before, '|') || in_category(after, '|'))
        && !in_category(before, '<')
        && !in_category(after, '>')
}

/// Whether a break at a whitespace character is allowed by the kinsoku rules —
/// i.e. the character that would start the new line may begin a line.
pub fn can_break_before(after: char) -> bool {
    !in_category(after, '>')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_letters_are_ascii_and_latin() {
        // The Emacs Lisp manual's own example: (category-set-mnemonics
        // (char-category-set ?a)) => "al".
        let set = char_category_set('a');
        assert!(set.contains(&'a'), "{set:?}");
        assert!(set.contains(&'l'), "{set:?}");
        // …plus the computed categories a Latin letter really is in.
        assert!(set.contains(&'.'), "base: {set:?}");
        assert!(set.contains(&'L'), "strong L2R: {set:?}");
    }

    #[test]
    fn han_is_chinese_and_line_breakable() {
        let set = char_category_set('漢');
        assert!(set.contains(&'c'), "{set:?}");
        assert!(set.contains(&'C'), "{set:?}");
        assert!(set.contains(&'|'), "{set:?}");
        assert_eq!(
            category_set_mnemonics(&char_category_set('漢')),
            set.iter().collect::<String>()
        );
    }

    #[test]
    fn hebrew_is_strong_rtl_and_latin_is_not() {
        assert!(char_category_set('ש').contains(&'R'));
        assert!(char_category_set('ש').contains(&'w'));
        assert!(!char_category_set('a').contains(&'R'));
    }

    #[test]
    fn kinsoku_rules_veto_breaks() {
        // A Japanese full stop may not start a line…
        assert!(in_category('。', '>'));
        assert!(!can_break_before('。'));
        // …and an opening bracket may not end one.
        assert!(in_category('「', '<'));
        assert!(!can_break_between('「', '漢'));
        // Between two han characters a break is fine with no whitespace.
        assert!(can_break_between('漢', '字'));
    }

    #[test]
    fn modify_category_entry_adds_and_removes() {
        // `~` is not line breakable to start with.
        assert!(!in_category('~', '|'));
        modify_category_entry('~', '~', '|', false);
        assert!(in_category('~', '|'));
        modify_category_entry('~', '~', '|', true);
        assert!(!in_category('~', '|'));
        // A removal of a standard entry sticks too: ASCII `a` out of category `a`.
        modify_category_entry('a', 'a', 'a', true);
        assert!(!char_category_set('a').contains(&'a'));
        modify_category_entry('a', 'a', 'a', false);
        assert!(char_category_set('a').contains(&'a'));
    }
}
