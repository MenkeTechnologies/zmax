//! Ispell — the zmax port of the GNU Emacs `ispell` interface to an external
//! spell checker (`ispell` / `aspell` / `hunspell`, via `ispell-program-name`).
//!
//! Emacs `ispell` does not implement spell checking itself; it drives an external
//! program in "pipe" mode (`<prog> -a`) and parses that program's terse output
//! protocol. This module is the pure, dependency-free, tested core of that: the
//! parser for the ispell/aspell `-a` result protocol. The command layer spawns
//! the program, feeds it text, and passes the output here; nothing in this file
//! does I/O, so it is fully unit-testable without the binary installed.
//!
//! The `-a` protocol (see `aspell` / `ispell` man pages): the program emits a
//! version banner line first (`@(#) ...`), then, for each input line checked,
//! one result line per word followed by a blank line. Result lines:
//!
//! ```text
//!   *                      the word was found (correct)
//!   -                      the word is a run-together / compound, accepted
//!   + ROOT                 found as a derivative of ROOT (correct)
//!   & ORIG N OFFSET: a, b  ORIG is wrong; N near-misses follow after the colon
//!   ? ORIG N OFFSET: a, b  ORIG is wrong; N guesses (weaker) follow
//!   # ORIG OFFSET          ORIG is wrong; no suggestions
//! ```
//!
//! `OFFSET` is the 1-based character column of the word within the checked line.

/// One word's verdict from the checker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WordCheck {
    /// The word is spelled correctly (`*`, `-`, `+`).
    Correct,
    /// The word is misspelled; `offset` is 0-based into the checked line.
    Misspelled {
        word: String,
        offset: usize,
        suggestions: Vec<String>,
    },
}

/// A misspelling within a checked line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Misspelling {
    pub word: String,
    /// 0-based character offset of the word in the line that was checked.
    pub offset: usize,
    pub suggestions: Vec<String>,
}

/// Parse a single `-a` result line. Returns `None` for the version banner, a
/// blank end-of-line marker, or any unrecognised line.
pub fn parse_line(line: &str) -> Option<WordCheck> {
    let line = line.trim_end_matches(['\r', '\n']);
    let mut chars = line.chars();
    match chars.next() {
        // Correct forms.
        Some('*') | Some('-') | Some('+') => Some(WordCheck::Correct),
        // Misspelled with suggestions: `& word count offset: s1, s2, ...`
        // or guesses: `? word count offset: ...`.
        Some('&') | Some('?') => parse_miss_with_suggestions(&line[1..]),
        // Misspelled, no suggestions: `# word offset`.
        Some('#') => parse_miss_no_suggestions(&line[1..]),
        _ => None,
    }
}

fn parse_miss_with_suggestions(rest: &str) -> Option<WordCheck> {
    // rest = " word count offset: s1, s2, ..."
    let (head, tail) = rest.split_once(':')?;
    let mut it = head.split_whitespace();
    let word = it.next()?.to_string();
    let _count: usize = it.next()?.parse().ok()?;
    let offset: usize = it.next()?.parse().ok()?;
    let suggestions = tail
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    Some(WordCheck::Misspelled {
        word,
        offset: offset.saturating_sub(1),
        suggestions,
    })
}

fn parse_miss_no_suggestions(rest: &str) -> Option<WordCheck> {
    // rest = " word offset"
    let mut it = rest.split_whitespace();
    let word = it.next()?.to_string();
    let offset: usize = it.next()?.parse().ok()?;
    Some(WordCheck::Misspelled {
        word,
        offset: offset.saturating_sub(1),
        suggestions: Vec::new(),
    })
}

/// Parse a whole `-a` output block into just the misspellings, in order. The
/// banner line and `Correct` verdicts are dropped. This is what a caller who
/// fed one line of text and wants the errors in it uses.
pub fn parse_output(block: &str) -> Vec<Misspelling> {
    block
        .lines()
        .filter_map(parse_line)
        .filter_map(|c| match c {
            WordCheck::Misspelled {
                word,
                offset,
                suggestions,
            } => Some(Misspelling {
                word,
                offset,
                suggestions,
            }),
            WordCheck::Correct => None,
        })
        .collect()
}

/// Escape a line of text for the `-a` protocol: a leading `^` forces the whole
/// line to be treated as words to check (bypassing the program's command chars
/// like `*`, `@`, `#`, `!`). Newlines are stripped (each line is fed separately).
pub fn escape_line(line: &str) -> String {
    format!("^{}", line.replace(['\r', '\n'], " "))
}

/// Resolve which checker to run and its base arguments, honouring an optional
/// dictionary. Mirrors Emacs's `ispell-program-name` search order
/// (aspell, then hunspell, then ispell). `program` is the resolved binary name;
/// the returned args put it in `-a` pipe mode with the dictionary if given.
pub fn pipe_args(dictionary: Option<&str>) -> Vec<String> {
    let mut args = vec!["-a".to_string()];
    if let Some(dict) = dictionary {
        if !dict.is_empty() {
            args.push("-d".to_string());
            args.push(dict.to_string());
        }
    }
    args
}

/// The 0-based indices of the lines an `ispell-message` run should spell-check:
/// the message body only, skipping the mail header block, cited/quoted lines
/// (leading `>` after optional whitespace) and the signature (everything from a
/// lone `-- ` separator onward). Emacs `ispell-message` skips exactly these.
///
/// The header block is every line up to and including the first empty line (mail
/// headers are `Key: value` lines terminated by a blank line). If there is no
/// blank line, the whole message is treated as body (no recognizable headers).
pub fn message_checkable_lines(lines: &[&str]) -> Vec<usize> {
    // Find the header/body separator: the first empty line.
    let body_start = match lines.iter().position(|l| l.trim().is_empty()) {
        Some(sep) => sep + 1,
        None => 0,
    };
    let mut out = Vec::new();
    for (i, &line) in lines.iter().enumerate().skip(body_start) {
        // Signature separator `-- ` ends the checkable region.
        if line == "-- " {
            break;
        }
        // Skip cited/quoted lines.
        if line.trim_start().starts_with('>') {
            continue;
        }
        // Skip blank lines (nothing to check).
        if line.trim().is_empty() {
            continue;
        }
        out.push(i);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_body_skips_headers_citations_signature() {
        let lines = [
            "To: bob",             // 0 header
            "Subject: hi",         // 1 header
            "",                    // 2 separator
            "Helo wrld",           // 3 body -> check
            "> quoted misspeling", // 4 citation -> skip
            "More txet",           // 5 body -> check
            "",                    // 6 blank -> skip
            "-- ",                 // 7 sig separator -> stop
            "sig speling",         // 8 signature -> skip
        ];
        assert_eq!(message_checkable_lines(&lines), vec![3, 5]);
        // No header separator -> whole message is body (minus citations/sig).
        let no_hdr = ["hello wrld", "> cited", "bye"];
        assert_eq!(message_checkable_lines(&no_hdr), vec![0, 2]);
    }

    #[test]
    fn correct_forms() {
        assert_eq!(parse_line("*"), Some(WordCheck::Correct));
        assert_eq!(parse_line("-"), Some(WordCheck::Correct));
        assert_eq!(parse_line("+ WALK"), Some(WordCheck::Correct));
    }

    #[test]
    fn banner_and_blank_are_none() {
        assert_eq!(parse_line("@(#) International Ispell Version 3.1"), None);
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("   "), None);
    }

    #[test]
    fn miss_with_suggestions() {
        let c = parse_line("& teh 3 1: the, ten, tea").unwrap();
        assert_eq!(
            c,
            WordCheck::Misspelled {
                word: "teh".to_string(),
                offset: 0, // 1-based 1 -> 0-based 0
                suggestions: vec!["the".into(), "ten".into(), "tea".into()],
            }
        );
    }

    #[test]
    fn guesses_parse_like_suggestions() {
        let c = parse_line("? wrdz 2 7: words, wards").unwrap();
        match c {
            WordCheck::Misspelled {
                word,
                offset,
                suggestions,
            } => {
                assert_eq!(word, "wrdz");
                assert_eq!(offset, 6);
                assert_eq!(suggestions, vec!["words", "wards"]);
            }
            _ => panic!("expected misspelled"),
        }
    }

    #[test]
    fn miss_without_suggestions() {
        let c = parse_line("# xyzzyx 10").unwrap();
        assert_eq!(
            c,
            WordCheck::Misspelled {
                word: "xyzzyx".to_string(),
                offset: 9,
                suggestions: vec![],
            }
        );
    }

    #[test]
    fn parse_block_collects_misspellings_in_order() {
        let block = "@(#) International Ispell Version 3.1\n\
                     *\n\
                     & teh 2 5: the, ten\n\
                     *\n\
                     # zzz 12\n\
                     \n";
        let miss = parse_output(block);
        assert_eq!(miss.len(), 2);
        assert_eq!(miss[0].word, "teh");
        assert_eq!(miss[0].offset, 4);
        assert_eq!(miss[0].suggestions, vec!["the", "ten"]);
        assert_eq!(miss[1].word, "zzz");
        assert_eq!(miss[1].offset, 11);
        assert!(miss[1].suggestions.is_empty());
    }

    #[test]
    fn escape_prefixes_caret_and_flattens_newlines() {
        assert_eq!(escape_line("hello world"), "^hello world");
        assert_eq!(escape_line("*command"), "^*command");
        assert_eq!(escape_line("a\nb"), "^a b");
    }

    #[test]
    fn pipe_args_with_and_without_dict() {
        assert_eq!(pipe_args(None), vec!["-a"]);
        assert_eq!(pipe_args(Some("en_GB")), vec!["-a", "-d", "en_GB"]);
        assert_eq!(pipe_args(Some("")), vec!["-a"]);
    }
}
