//! The matching half of Emacs's `query-replace` / `query-replace-regexp`.
//!
//! `query-replace` is interactive — it stops at every match and asks — but the
//! part that decides *what* the matches are and *what* each one turns into is
//! pure, so it lives here and the interactive loop (the `QueryReplace` overlay in
//! `zmax-term`) only has to drive it.
//!
//! Two things need care:
//!
//! * **Offsets.** The matches are reported in character offsets, because that is
//!   what the editor's rope and selections use, while `regex` works in bytes.
//! * **Replacement syntax.** Emacs writes back-references as `\&` (the whole
//!   match), `\1`..`\9` (a group) and `\\` (a literal backslash); the `regex`
//!   crate writes them `${0}`, `${1}`, `$$`. [`expand_template`] translates.

use regex::Regex;

/// One match: `(start_char, end_char, replacement)`, offsets into the text the
/// match was found in.
pub type Match = (usize, usize, String);

/// Translate an Emacs replacement string into the `regex` crate's expansion
/// syntax: `\&` → `${0}`, `\N` → `${N}`, `\\` → a literal backslash, and any
/// literal `$` is escaped so it is not read as a group reference.
///
/// A backslash before anything else (`\n` in a replacement, say) is dropped, as
/// Emacs does — the escape exists only to protect the character that follows.
pub fn expand_template(replacement: &str) -> String {
    let mut out = String::with_capacity(replacement.len());
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '$' => out.push_str("$$"),
            '\\' => match chars.next() {
                Some('&') => out.push_str("${0}"),
                Some(d @ '0'..='9') => {
                    out.push_str("${");
                    out.push(d);
                    out.push('}');
                }
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            _ => out.push(c),
        }
    }
    out
}

/// Every match of `re` in `text`, in document order, each paired with the text it
/// should be replaced by.
///
/// `template` is the Emacs-syntax replacement. In regexp mode its
/// back-references are expanded against the match's capture groups; in literal
/// mode (plain `query-replace`) it is inserted verbatim, exactly as Emacs does —
/// a `\1` in a literal replacement is just those two characters.
///
/// Empty matches are skipped: Emacs's query-replace never offers a zero-width
/// match, and offering one would loop forever.
pub fn matches(text: &str, re: &Regex, template: &str, regexp: bool) -> Vec<Match> {
    let expanded = regexp.then(|| expand_template(template));
    let mut out = Vec::new();
    // Byte→char conversion is done incrementally, so the whole text is walked
    // once rather than re-counted from the start for every match.
    let mut last_byte = 0usize;
    let mut last_char = 0usize;
    for caps in re.captures_iter(text) {
        let m = caps.get(0).expect("group 0 always matches");
        if m.is_empty() {
            continue;
        }
        let start_char = last_char + text[last_byte..m.start()].chars().count();
        let end_char = start_char + text[m.start()..m.end()].chars().count();
        last_byte = m.end();
        last_char = end_char;

        let replacement = match &expanded {
            Some(t) => {
                let mut buf = String::new();
                caps.expand(t, &mut buf);
                buf
            }
            None => template.to_string(),
        };
        out.push((start_char, end_char, replacement));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_translates_emacs_backreferences() {
        assert_eq!(expand_template("<\\&>"), "<${0}>");
        assert_eq!(expand_template("\\2-\\1"), "${2}-${1}");
        assert_eq!(expand_template("a\\\\b"), "a\\b");
        // A literal `$` must survive: `regex` would otherwise read `$name`.
        assert_eq!(expand_template("$5.00"), "$$5.00");
        assert_eq!(expand_template("plain"), "plain");
    }

    #[test]
    fn literal_mode_inserts_the_replacement_verbatim() {
        let re = Regex::new(&regex::escape("a.b")).unwrap();
        let found = matches("x a.b y aXb", &re, "\\1$", false);
        // Only the literal `a.b` matches (the `.` is escaped), and the
        // replacement is not expanded in literal mode.
        assert_eq!(found, vec![(2, 5, "\\1$".to_string())]);
    }

    #[test]
    fn regexp_mode_expands_groups_and_whole_match() {
        let re = Regex::new(r"(\w+)@(\w+)").unwrap();
        let found = matches("mail alice@example bob@host", &re, "\\2:\\1 [\\&]", true);
        assert_eq!(
            found,
            vec![
                (5, 18, "example:alice [alice@example]".to_string()),
                (19, 27, "host:bob [bob@host]".to_string()),
            ]
        );
    }

    #[test]
    fn offsets_are_characters_not_bytes() {
        // Each `é` is two bytes: a byte-offset match list would be wrong here.
        let re = Regex::new("cat").unwrap();
        let found = matches("café… cat", &re, "dog", false);
        assert_eq!(found, vec![(6, 9, "dog".to_string())]);
        let text: String = "café… cat".chars().collect();
        assert_eq!(&text.chars().skip(6).take(3).collect::<String>(), "cat");
    }

    #[test]
    fn zero_width_matches_are_skipped() {
        let re = Regex::new("x*").unwrap();
        let found = matches("abxxc", &re, "-", true);
        assert_eq!(
            found,
            vec![(2, 4, "-".to_string())],
            "only the real `xx` match"
        );
    }
}
