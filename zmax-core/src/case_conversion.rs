use crate::Tendril;

// todo: should this be grapheme aware?

pub fn to_pascal_case(text: impl Iterator<Item = char>) -> Tendril {
    let mut res = Tendril::new();
    to_pascal_case_with(text, &mut res);
    res
}

pub fn to_pascal_case_with(text: impl Iterator<Item = char>, buf: &mut Tendril) {
    let mut at_word_start = true;
    for c in text {
        // we don't count _ as a word char here so case conversions work well
        if !c.is_alphanumeric() {
            at_word_start = true;
            continue;
        }
        if at_word_start {
            at_word_start = false;
            buf.extend(c.to_uppercase());
        } else {
            buf.push(c)
        }
    }
}

pub fn to_upper_case_with(text: impl Iterator<Item = char>, buf: &mut Tendril) {
    for c in text {
        for c in c.to_uppercase() {
            buf.push(c)
        }
    }
}

pub fn to_lower_case_with(text: impl Iterator<Item = char>, buf: &mut Tendril) {
    for c in text {
        for c in c.to_lowercase() {
            buf.push(c)
        }
    }
}

/// Emacs `capitalize-region` / `capitalize-word`: title-case each word — the
/// first alphanumeric of every word upper-cased, the rest lower-cased. A "word"
/// is a maximal run of alphanumeric characters; every other character (spaces,
/// punctuation, `_`) is preserved verbatim (unlike [`to_pascal_case_with`], which
/// drops them for identifier conversion).
pub fn capitalize_words_with(text: impl Iterator<Item = char>, buf: &mut Tendril) {
    let mut at_word_start = true;
    for c in text {
        if !c.is_alphanumeric() {
            buf.push(c);
            at_word_start = true;
        } else if at_word_start {
            buf.extend(c.to_uppercase());
            at_word_start = false;
        } else {
            buf.extend(c.to_lowercase());
        }
    }
}

/// Emacs `capitalize-region` / `capitalize-word` — see [`capitalize_words_with`].
pub fn capitalize_words(text: impl Iterator<Item = char>) -> Tendril {
    let mut res = Tendril::new();
    capitalize_words_with(text, &mut res);
    res
}

/// Emacs `upcase-initials-region`: upper-case the first letter of each word,
/// leaving the remaining letters of each word (and all non-word characters)
/// unchanged.
pub fn upcase_initials_with(text: impl Iterator<Item = char>, buf: &mut Tendril) {
    let mut at_word_start = true;
    for c in text {
        if !c.is_alphanumeric() {
            buf.push(c);
            at_word_start = true;
        } else if at_word_start {
            buf.extend(c.to_uppercase());
            at_word_start = false;
        } else {
            buf.push(c);
        }
    }
}

/// Emacs `upcase-initials-region` — see [`upcase_initials_with`].
pub fn upcase_initials(text: impl Iterator<Item = char>) -> Tendril {
    let mut res = Tendril::new();
    upcase_initials_with(text, &mut res);
    res
}

pub fn to_camel_case(text: impl Iterator<Item = char>) -> Tendril {
    let mut res = Tendril::new();
    to_camel_case_with(text, &mut res);
    res
}
pub fn to_camel_case_with(mut text: impl Iterator<Item = char>, buf: &mut Tendril) {
    for c in &mut text {
        if c.is_alphanumeric() {
            buf.extend(c.to_lowercase())
        }
    }
    let mut at_word_start = false;
    for c in text {
        // we don't count _ as a word char here so case conversions work well
        if !c.is_alphanumeric() {
            at_word_start = true;
            continue;
        }
        if at_word_start {
            at_word_start = false;
            buf.extend(c.to_uppercase());
        } else {
            buf.push(c)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(s: &str) -> String {
        capitalize_words(s.chars()).to_string()
    }
    fn ini(s: &str) -> String {
        upcase_initials(s.chars()).to_string()
    }

    #[test]
    fn capitalize_words_titlecases_and_preserves_structure() {
        assert_eq!(cap("hello WORLD foo"), "Hello World Foo");
        // Non-word characters (spaces, punctuation, underscores) are preserved.
        assert_eq!(cap("foo-bar_baz.qux"), "Foo-Bar_Baz.Qux");
        assert_eq!(cap("  spaced  out  "), "  Spaced  Out  ");
        assert_eq!(cap(""), "");
    }

    #[test]
    fn upcase_initials_upper_first_only() {
        // First letter of each word upper-cased; the rest left unchanged.
        assert_eq!(ini("hELLO wORLD"), "HELLO WORLD");
        assert_eq!(ini("foo bAR"), "Foo BAR");
        assert_eq!(ini("a-b c"), "A-B C");
    }
}
