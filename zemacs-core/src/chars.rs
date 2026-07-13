//! Utility functions to categorize a `char`.

use crate::LineEnding;

#[derive(Debug, Eq, PartialEq)]
pub enum CharCategory {
    Whitespace,
    Eol,
    Word,
    Punctuation,
    Unknown,
}

thread_local! {
    /// vim `iskeyword`: extra characters (beyond alphanumerics and `_`) that count
    /// as word/keyword characters for word motions and text objects. Empty by
    /// default, so behaviour is unchanged unless `:set iskeyword` opts in.
    static EXTRA_KEYWORD_CHARS: std::cell::RefCell<Vec<char>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// vim `iskeyword`: replace the set of extra word/keyword characters.
pub fn set_extra_keyword_chars(chars: Vec<char>) {
    EXTRA_KEYWORD_CHARS.with(|c| *c.borrow_mut() = chars);
}

#[inline]
fn is_extra_keyword_char(ch: char) -> bool {
    EXTRA_KEYWORD_CHARS.with(|c| c.borrow().contains(&ch))
}

#[inline]
pub fn categorize_char(ch: char) -> CharCategory {
    if char_is_line_ending(ch) {
        CharCategory::Eol
    } else if ch.is_whitespace() {
        CharCategory::Whitespace
    } else if char_is_word(ch) || is_extra_keyword_char(ch) {
        CharCategory::Word
    } else if char_is_punctuation(ch) {
        CharCategory::Punctuation
    } else {
        CharCategory::Unknown
    }
}

/// Determine whether a character is a line ending.
#[inline]
pub fn char_is_line_ending(ch: char) -> bool {
    LineEnding::from_char(ch).is_some()
}

// ---------------------------------------------------------------------------
// vim `CTRL-V {number}` — literal character codes (`i_CTRL-V_digit`, `c_CTRL-V`).
// ---------------------------------------------------------------------------

/// The numeric form a vim `CTRL-V` code is typed in. A bare digit after `CTRL-V`
/// starts a [`Decimal`](LiteralRadix::Decimal) code; the other forms are opened
/// by a one-character introducer (`o`/`x`/`u`/`U`/`b`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralRadix {
    /// `CTRL-V 065` — up to three decimal digits, one byte.
    Decimal,
    /// `CTRL-V o101` — up to three octal digits, one byte.
    Octal,
    /// `CTRL-V x41` — up to two hex digits, one byte.
    Hex,
    /// `CTRL-V u00e9` — up to four hex digits, a codepoint.
    Unicode4,
    /// `CTRL-V U0001f600` — up to eight hex digits, a codepoint.
    Unicode8,
    /// `CTRL-V b01000001` — up to eight binary digits, one byte.
    Binary,
}

impl LiteralRadix {
    /// The form opened by the character typed right after `CTRL-V`, or `None`
    /// when it does not open one (a decimal digit is handled by the caller: it is
    /// already the first digit of a [`Decimal`](LiteralRadix::Decimal) code).
    pub fn from_introducer(c: char) -> Option<Self> {
        match c {
            'o' | 'O' => Some(Self::Octal),
            'x' | 'X' => Some(Self::Hex),
            'u' => Some(Self::Unicode4),
            'U' => Some(Self::Unicode8),
            'b' | 'B' => Some(Self::Binary),
            _ => None,
        }
    }

    /// The number of digits the form accepts. vim inserts the character as soon
    /// as that many digits have been typed (an invalid digit ends it early).
    pub fn max_digits(self) -> usize {
        match self {
            Self::Decimal | Self::Octal => 3,
            Self::Hex => 2,
            Self::Unicode4 => 4,
            Self::Unicode8 | Self::Binary => 8,
        }
    }

    /// The numeric base of the form.
    fn base(self) -> u32 {
        match self {
            Self::Binary => 2,
            Self::Octal => 8,
            Self::Decimal => 10,
            Self::Hex | Self::Unicode4 | Self::Unicode8 => 16,
        }
    }

    /// Does `c` continue a code in this form?
    pub fn is_digit(self, c: char) -> bool {
        c.is_digit(self.base())
    }
}

/// The character vim inserts for the `digits` typed after `CTRL-V` in `radix`.
///
/// The byte forms (decimal/octal/hex/binary) name a value in `0..=255` — vim
/// inserts it as a single byte, which in a UTF-8 document is the codepoint of the
/// same value (so `CTRL-V 233` gives `é`). The `u`/`U` forms name a codepoint
/// directly. Out-of-range or unpaired-surrogate values yield `None`, as does an
/// empty digit run (`CTRL-V` followed by nothing typed).
pub fn literal_code_char(radix: LiteralRadix, digits: &str) -> Option<char> {
    if digits.is_empty() {
        return None;
    }
    let value = u32::from_str_radix(digits, radix.base()).ok()?;
    match radix {
        LiteralRadix::Unicode4 | LiteralRadix::Unicode8 => char::from_u32(value),
        _ => (value <= 0xff).then(|| char::from(value as u8)),
    }
}

/// Determine whether a character qualifies as (non-line-break)
/// whitespace.
#[inline]
pub fn char_is_whitespace(ch: char) -> bool {
    // TODO: this is a naive binary categorization of whitespace
    // characters.  For display, word wrapping, etc. we'll need a better
    // categorization based on e.g. breaking vs non-breaking spaces
    // and whether they're zero-width or not.
    match ch {
        //'\u{1680}' | // Ogham Space Mark (here for completeness, but usually displayed as a dash, not as whitespace)        '\u{0009}' | // Character Tabulation
        '\u{0020}' | // Space
        '\u{00A0}' | // No-break Space
        '\u{180E}' | // Mongolian Vowel Separator
        '\u{202F}' | // Narrow No-break Space
        '\u{205F}' | // Medium Mathematical Space
        '\u{3000}' | // Ideographic Space
        '\u{FEFF}'   // Zero Width No-break Space
        => true,

        // En Quad, Em Quad, En Space, Em Space, Three-per-em Space,
        // Four-per-em Space, Six-per-em Space, Figure Space,
        // Punctuation Space, Thin Space, Hair Space, Zero Width Space.
        ch if ('\u{2000}' ..= '\u{200B}').contains(&ch) => true,

        _ => false,
    }
}

#[inline]
pub fn char_is_punctuation(ch: char) -> bool {
    use unicode_general_category::{get_general_category, GeneralCategory};

    matches!(
        get_general_category(ch),
        GeneralCategory::OtherPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::MathSymbol
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
    )
}

#[inline]
pub fn char_is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// The Unicode **general category** of `ch` as Emacs `describe-char` reports it:
/// the two-letter abbreviation plus the long name, e.g. `('A')` → `("Lu", "Letter,
/// Uppercase")`. The mapping mirrors the Unicode standard's category names (the same
/// text Emacs shows for `general-category`).
pub fn general_category_name(ch: char) -> (&'static str, &'static str) {
    use unicode_general_category::{get_general_category, GeneralCategory as G};
    match get_general_category(ch) {
        G::UppercaseLetter => ("Lu", "Letter, Uppercase"),
        G::LowercaseLetter => ("Ll", "Letter, Lowercase"),
        G::TitlecaseLetter => ("Lt", "Letter, Titlecase"),
        G::ModifierLetter => ("Lm", "Letter, Modifier"),
        G::OtherLetter => ("Lo", "Letter, Other"),
        G::NonspacingMark => ("Mn", "Mark, Nonspacing"),
        G::SpacingMark => ("Mc", "Mark, Spacing Combining"),
        G::EnclosingMark => ("Me", "Mark, Enclosing"),
        G::DecimalNumber => ("Nd", "Number, Decimal Digit"),
        G::LetterNumber => ("Nl", "Number, Letter"),
        G::OtherNumber => ("No", "Number, Other"),
        G::ConnectorPunctuation => ("Pc", "Punctuation, Connector"),
        G::DashPunctuation => ("Pd", "Punctuation, Dash"),
        G::OpenPunctuation => ("Ps", "Punctuation, Open"),
        G::ClosePunctuation => ("Pe", "Punctuation, Close"),
        G::InitialPunctuation => ("Pi", "Punctuation, Initial quote"),
        G::FinalPunctuation => ("Pf", "Punctuation, Final quote"),
        G::OtherPunctuation => ("Po", "Punctuation, Other"),
        G::MathSymbol => ("Sm", "Symbol, Math"),
        G::CurrencySymbol => ("Sc", "Symbol, Currency"),
        G::ModifierSymbol => ("Sk", "Symbol, Modifier"),
        G::OtherSymbol => ("So", "Symbol, Other"),
        G::SpaceSeparator => ("Zs", "Separator, Space"),
        G::LineSeparator => ("Zl", "Separator, Line"),
        G::ParagraphSeparator => ("Zp", "Separator, Paragraph"),
        G::Control => ("Cc", "Other, Control"),
        G::Format => ("Cf", "Other, Format"),
        G::Surrogate => ("Cs", "Other, Surrogate"),
        G::PrivateUse => ("Co", "Other, Private Use"),
        G::Unassigned => ("Cn", "Other, Not Assigned"),
        // `GeneralCategory` is `#[non_exhaustive]`; anything unmapped is unassigned.
        _ => ("Cn", "Other, Not Assigned"),
    }
}

/// The Unicode **block** table: `(start, end_inclusive, name)`, sorted by `start`.
/// A curated best-effort subset covering the blocks a working editor actually
/// meets (Latin, punctuation, currency, arrows, math, box drawing, the common CJK
/// and Indic scripts, emoji, …). Names match the official Unicode block names Emacs
/// `describe-char` prints. Not exhaustive — codepoints outside every listed range
/// return [`NO_BLOCK`].
pub const NO_BLOCK: &str = "No_Block";

const UNICODE_BLOCKS: &[(u32, u32, &str)] = &[
    (0x0000, 0x007F, "Basic Latin"),
    (0x0080, 0x00FF, "Latin-1 Supplement"),
    (0x0100, 0x017F, "Latin Extended-A"),
    (0x0180, 0x024F, "Latin Extended-B"),
    (0x0250, 0x02AF, "IPA Extensions"),
    (0x02B0, 0x02FF, "Spacing Modifier Letters"),
    (0x0300, 0x036F, "Combining Diacritical Marks"),
    (0x0370, 0x03FF, "Greek and Coptic"),
    (0x0400, 0x04FF, "Cyrillic"),
    (0x0500, 0x052F, "Cyrillic Supplement"),
    (0x0530, 0x058F, "Armenian"),
    (0x0590, 0x05FF, "Hebrew"),
    (0x0600, 0x06FF, "Arabic"),
    (0x0700, 0x074F, "Syriac"),
    (0x0900, 0x097F, "Devanagari"),
    (0x0980, 0x09FF, "Bengali"),
    (0x0A00, 0x0A7F, "Gurmukhi"),
    (0x0B80, 0x0BFF, "Tamil"),
    (0x0E00, 0x0E7F, "Thai"),
    (0x0E80, 0x0EFF, "Lao"),
    (0x1000, 0x109F, "Myanmar"),
    (0x10A0, 0x10FF, "Georgian"),
    (0x1100, 0x11FF, "Hangul Jamo"),
    (0x1E00, 0x1EFF, "Latin Extended Additional"),
    (0x1F00, 0x1FFF, "Greek Extended"),
    (0x2000, 0x206F, "General Punctuation"),
    (0x2070, 0x209F, "Superscripts and Subscripts"),
    (0x20A0, 0x20CF, "Currency Symbols"),
    (0x2100, 0x214F, "Letterlike Symbols"),
    (0x2150, 0x218F, "Number Forms"),
    (0x2190, 0x21FF, "Arrows"),
    (0x2200, 0x22FF, "Mathematical Operators"),
    (0x2300, 0x23FF, "Miscellaneous Technical"),
    (0x2400, 0x243F, "Control Pictures"),
    (0x2500, 0x257F, "Box Drawing"),
    (0x2580, 0x259F, "Block Elements"),
    (0x25A0, 0x25FF, "Geometric Shapes"),
    (0x2600, 0x26FF, "Miscellaneous Symbols"),
    (0x2700, 0x27BF, "Dingbats"),
    (0x2B00, 0x2BFF, "Miscellaneous Symbols and Arrows"),
    (0x3000, 0x303F, "CJK Symbols and Punctuation"),
    (0x3040, 0x309F, "Hiragana"),
    (0x30A0, 0x30FF, "Katakana"),
    (0x3400, 0x4DBF, "CJK Unified Ideographs Extension A"),
    (0x4E00, 0x9FFF, "CJK Unified Ideographs"),
    (0xA000, 0xA48F, "Yi Syllables"),
    (0xAC00, 0xD7AF, "Hangul Syllables"),
    (0xE000, 0xF8FF, "Private Use Area"),
    (0xF900, 0xFAFF, "CJK Compatibility Ideographs"),
    (0xFB00, 0xFB4F, "Alphabetic Presentation Forms"),
    (0xFE30, 0xFE4F, "CJK Compatibility Forms"),
    (0xFF00, 0xFFEF, "Halfwidth and Fullwidth Forms"),
    (0x1D400, 0x1D7FF, "Mathematical Alphanumeric Symbols"),
    (0x1F300, 0x1F5FF, "Miscellaneous Symbols and Pictographs"),
    (0x1F600, 0x1F64F, "Emoticons"),
    (0x1F680, 0x1F6FF, "Transport and Map Symbols"),
    (0x1F900, 0x1F9FF, "Supplemental Symbols and Pictographs"),
    (0x20000, 0x2A6DF, "CJK Unified Ideographs Extension B"),
];

/// The Unicode block name for `ch`, or [`NO_BLOCK`] when it falls outside every
/// range in the curated `UNICODE_BLOCKS` table.
pub fn unicode_block(ch: char) -> &'static str {
    let cp = ch as u32;
    // Table is sorted and non-overlapping; a short linear scan is fine.
    for &(start, end, name) in UNICODE_BLOCKS {
        if cp < start {
            break;
        }
        if cp <= end {
            return name;
        }
    }
    NO_BLOCK
}

/// Return every `(start, end, name)` block in the curated table — used by
/// `list-character-sets` / `list-charset-chars` to enumerate what zemacs can name.
pub fn unicode_blocks() -> &'static [(u32, u32, &'static str)] {
    UNICODE_BLOCKS
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn literal_code_decodes_every_vim_ctrl_v_form() {
        // i_CTRL-V_digit: three decimal digits are one byte.
        assert_eq!(literal_code_char(LiteralRadix::Decimal, "065"), Some('A'));
        assert_eq!(literal_code_char(LiteralRadix::Decimal, "233"), Some('é'));
        // The other introducers vim accepts after CTRL-V.
        assert_eq!(literal_code_char(LiteralRadix::Octal, "101"), Some('A'));
        assert_eq!(literal_code_char(LiteralRadix::Hex, "41"), Some('A'));
        assert_eq!(
            literal_code_char(LiteralRadix::Binary, "01000001"),
            Some('A')
        );
        assert_eq!(literal_code_char(LiteralRadix::Unicode4, "00e9"), Some('é'));
        assert_eq!(
            literal_code_char(LiteralRadix::Unicode8, "0001f600"),
            Some('😀')
        );
        // A short run is decoded as typed (vim inserts what you have when the
        // form is ended early by a non-digit).
        assert_eq!(literal_code_char(LiteralRadix::Decimal, "65"), Some('A'));
        // Byte forms cap at 255; nothing typed decodes to nothing.
        assert_eq!(literal_code_char(LiteralRadix::Decimal, "999"), None);
        assert_eq!(literal_code_char(LiteralRadix::Decimal, ""), None);
        // Lone surrogates are not characters.
        assert_eq!(literal_code_char(LiteralRadix::Unicode4, "d800"), None);
    }

    #[test]
    fn literal_radix_introducers_and_digit_limits() {
        assert_eq!(LiteralRadix::from_introducer('x'), Some(LiteralRadix::Hex));
        assert_eq!(
            LiteralRadix::from_introducer('U'),
            Some(LiteralRadix::Unicode8)
        );
        // A digit does not introduce a form — it is already the decimal code.
        assert_eq!(LiteralRadix::from_introducer('0'), None);
        assert_eq!(LiteralRadix::from_introducer('q'), None);

        assert_eq!(LiteralRadix::Decimal.max_digits(), 3);
        assert_eq!(LiteralRadix::Hex.max_digits(), 2);
        assert_eq!(LiteralRadix::Unicode8.max_digits(), 8);

        assert!(LiteralRadix::Hex.is_digit('f'));
        assert!(!LiteralRadix::Decimal.is_digit('f'));
        assert!(!LiteralRadix::Binary.is_digit('2'));
    }

    #[test]
    fn test_categorize() {
        #[cfg(not(feature = "unicode-lines"))]
        const EOL_TEST_CASE: &str = "\n";
        #[cfg(feature = "unicode-lines")]
        const EOL_TEST_CASE: &str = "\n\u{000B}\u{000C}\u{0085}\u{2028}\u{2029}";
        const WORD_TEST_CASE: &str = "_hello_world_あいうえおー1234567890１２３４５６７８９０";
        const PUNCTUATION_TEST_CASE: &str =
            "!\"#$%&\'()*+,-./:;<=>?@[\\]^`{|}~！”＃＄％＆’（）＊＋、。：；＜＝＞？＠「」＾｀｛｜｝～";
        const WHITESPACE_TEST_CASE: &str = "  　   ";

        for ch in EOL_TEST_CASE.chars() {
            assert_eq!(CharCategory::Eol, categorize_char(ch));
        }

        for ch in WHITESPACE_TEST_CASE.chars() {
            assert_eq!(
                CharCategory::Whitespace,
                categorize_char(ch),
                "Testing '{}', but got `{:?}` instead of `Category::Whitespace`",
                ch,
                categorize_char(ch)
            );
        }

        for ch in WORD_TEST_CASE.chars() {
            assert_eq!(
                CharCategory::Word,
                categorize_char(ch),
                "Testing '{}', but got `{:?}` instead of `Category::Word`",
                ch,
                categorize_char(ch)
            );
        }

        for ch in PUNCTUATION_TEST_CASE.chars() {
            assert_eq!(
                CharCategory::Punctuation,
                categorize_char(ch),
                "Testing '{}', but got `{:?}` instead of `Category::Punctuation`",
                ch,
                categorize_char(ch)
            );
        }
    }

    // Pinned against GNU Emacs 30 `describe-char` `general-category` field, which
    // shows the Unicode two-letter code + long name.
    #[test]
    fn general_category_matches_unicode() {
        assert_eq!(general_category_name('A'), ("Lu", "Letter, Uppercase"));
        assert_eq!(general_category_name('a'), ("Ll", "Letter, Lowercase"));
        assert_eq!(general_category_name('5'), ("Nd", "Number, Decimal Digit"));
        assert_eq!(general_category_name(' '), ("Zs", "Separator, Space"));
        assert_eq!(general_category_name('.'), ("Po", "Punctuation, Other"));
        assert_eq!(general_category_name('('), ("Ps", "Punctuation, Open"));
        assert_eq!(general_category_name('+'), ("Sm", "Symbol, Math"));
        assert_eq!(general_category_name('$'), ("Sc", "Symbol, Currency"));
        assert_eq!(general_category_name('\t'), ("Cc", "Other, Control"));
        assert_eq!(general_category_name('λ'), ("Ll", "Letter, Lowercase"));
    }

    // Pinned against the official Unicode block names (as Emacs `describe-char`
    // prints under "Unicode block").
    #[test]
    fn unicode_block_lookup() {
        assert_eq!(unicode_block('A'), "Basic Latin");
        assert_eq!(unicode_block('é'), "Latin-1 Supplement");
        assert_eq!(unicode_block('λ'), "Greek and Coptic");
        assert_eq!(unicode_block('Ж'), "Cyrillic");
        assert_eq!(unicode_block('→'), "Arrows");
        assert_eq!(unicode_block('∑'), "Mathematical Operators");
        assert_eq!(unicode_block('中'), "CJK Unified Ideographs");
        assert_eq!(unicode_block('あ'), "Hiragana");
        assert_eq!(unicode_block('€'), "Currency Symbols");
        assert_eq!(unicode_block('😀'), "Emoticons");
    }

    #[test]
    fn unicode_block_gap_returns_no_block() {
        // U+0870 sits in the Arabic-to-Syriac gap not covered by the curated table.
        assert_eq!(unicode_block('\u{2FE0}'), NO_BLOCK);
    }

    #[test]
    fn unicode_blocks_table_is_sorted_and_nonoverlapping() {
        let t = unicode_blocks();
        for w in t.windows(2) {
            assert!(w[0].0 <= w[0].1, "block start after end: {:?}", w[0]);
            assert!(
                w[0].1 < w[1].0,
                "blocks overlap or unsorted: {:?} then {:?}",
                w[0],
                w[1]
            );
        }
    }
}
