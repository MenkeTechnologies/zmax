//! Emacs Hi-Lock (`highlight-regexp` / `unhighlight-regexp` / `highlight-phrase`
//! / `highlight-lines-matching-regexp`): persistent, user-defined regexp
//! highlighting drawn as an overlay on top of syntax highlighting.
//!
//! Patterns are process-global (a simplification of emacs's buffer-local
//! `hi-lock-interactive-patterns`), compiled once on add. The render loop calls
//! [`viewport_matches`] with the visible slice to get the char ranges to paint;
//! each pattern is assigned a colour by its index (see `HI_LOCK_SCOPES` in the
//! editor). The match-finding is pure and unit-tested.

use std::sync::Mutex;

use once_cell::sync::Lazy;
use regex::Regex;

/// One active highlight: the compiled regexp, whether it highlights the whole
/// matching line, and the original source (for `unhighlight-regexp` + dedup).
pub struct Pattern {
    pub re: Regex,
    pub whole_line: bool,
    pub src: String,
}

static PATTERNS: Lazy<Mutex<Vec<Pattern>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Add a highlight for `src`. `whole_line` extends each match to its full line
/// (`highlight-lines-matching-regexp`). A duplicate source is ignored. Returns
/// an error if the regexp does not compile.
pub fn add(src: &str, whole_line: bool) -> Result<(), String> {
    let re = Regex::new(src).map_err(|e| e.to_string())?;
    let mut pats = PATTERNS.lock().unwrap();
    if pats.iter().any(|p| p.src == src) {
        return Ok(());
    }
    pats.push(Pattern {
        re,
        whole_line,
        src: src.to_string(),
    });
    Ok(())
}

/// Remove the highlight for `src`. Returns whether one was removed.
pub fn remove(src: &str) -> bool {
    let mut pats = PATTERNS.lock().unwrap();
    let before = pats.len();
    pats.retain(|p| p.src != src);
    pats.len() != before
}

/// Remove every highlight (`unhighlight-regexp` with the "all" answer).
pub fn clear() {
    PATTERNS.lock().unwrap().clear();
}

/// vim `:match` / `:2match` / `:3match` slots. vim gives three independent match
/// groups, each holding at most one pattern, cleared with `:{N}match none`. The
/// pattern itself lives in the shared [`PATTERNS`] set so it renders like any
/// Hi-Lock highlight (colour by index, not by the named highlight group); this
/// array only remembers which source string currently occupies each slot.
static MATCH_GROUPS: Lazy<Mutex<[Option<String>; 3]>> =
    Lazy::new(|| Mutex::new([None, None, None]));

/// Set match group `n` (1..=3) to highlight `src`, replacing whatever the slot
/// held. Errors on an out-of-range slot or an invalid regexp.
pub fn set_match_group(n: usize, src: &str) -> Result<(), String> {
    if !(1..=3).contains(&n) {
        return Err(format!("match group must be 1..3, got {n}"));
    }
    // Validate before mutating any state.
    Regex::new(src).map_err(|e| e.to_string())?;
    let mut groups = MATCH_GROUPS.lock().unwrap();
    if let Some(old) = groups[n - 1].take() {
        // Drop the old highlight unless another slot still references it.
        if old != src && !groups.iter().any(|g| g.as_deref() == Some(old.as_str())) {
            remove(&old);
        }
    }
    add(src, false)?;
    groups[n - 1] = Some(src.to_string());
    Ok(())
}

/// Clear match group `n` (1..=3), removing its highlight unless another slot
/// still references the same source. Returns whether a pattern was cleared.
pub fn clear_match_group(n: usize) -> bool {
    if !(1..=3).contains(&n) {
        return false;
    }
    let mut groups = MATCH_GROUPS.lock().unwrap();
    match groups[n - 1].take() {
        Some(src) => {
            if !groups.iter().any(|g| g.as_deref() == Some(src.as_str())) {
                remove(&src);
            }
            true
        }
        None => false,
    }
}

/// The active pattern sources, for completion and status.
pub fn sources() -> Vec<String> {
    PATTERNS
        .lock()
        .unwrap()
        .iter()
        .map(|p| p.src.clone())
        .collect()
}

/// Whether any highlight is active (lets the render loop skip the work).
pub fn is_empty() -> bool {
    PATTERNS.lock().unwrap().is_empty()
}

/// Run `f` over the active patterns (used by the render loop to find matches
/// without cloning the compiled regexps).
pub fn with_patterns<R>(f: impl FnOnce(&[Pattern]) -> R) -> R {
    f(&PATTERNS.lock().unwrap())
}

/// Char ranges to highlight within `text` (a viewport slice), as
/// `(char_start, char_end, pattern_index)`. `whole_line` patterns expand each
/// match to the line(s) it covers within `text`. Pure — no global state.
pub fn viewport_matches(text: &str, patterns: &[Pattern]) -> Vec<(usize, usize, usize)> {
    // Byte offset -> char index, computed once for the slice.
    let byte_to_char = |b: usize| text[..b].chars().count();
    let mut out = Vec::new();
    for (idx, p) in patterns.iter().enumerate() {
        for m in p.re.find_iter(text) {
            if m.start() == m.end() {
                continue; // skip empty matches
            }
            let (cs, ce) = if p.whole_line {
                // Expand to the enclosing line(s).
                let line_start = text[..m.start()].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let line_end = text[m.end()..]
                    .find('\n')
                    .map(|i| m.end() + i + 1)
                    .unwrap_or(text.len());
                (byte_to_char(line_start), byte_to_char(line_end))
            } else {
                (byte_to_char(m.start()), byte_to_char(m.end()))
            };
            out.push((cs, ce, idx));
        }
    }
    out
}

/// Default byte limit for scanning a buffer for file-local `Hi-lock:` patterns,
/// mirroring Emacs `hi-lock-file-patterns-range` (10000).
pub const FILE_PATTERNS_RANGE: usize = 10000;

/// The tag Emacs writes/reads for file-local Hi-Lock patterns
/// (`hi-lock-file-patterns-prefix`), always followed by a colon.
pub const FILE_PATTERNS_PREFIX: &str = "Hi-lock";

/// Emacs `highlight-symbol-at-point`: build the regexp that highlights every
/// occurrence of `symbol` as a whole word. Emacs uses
/// `find-tag-default-as-symbol-regexp`, which wraps the regexp-quoted symbol in
/// symbol boundaries (`\_<`…`\_>`); the Rust `regex` crate has no symbol-boundary
/// escape, so the faithful equivalent is a word boundary (`\b`) on each side.
/// Returns `None` for an empty/blank symbol.
pub fn symbol_regexp(symbol: &str) -> Option<String> {
    let symbol = symbol.trim();
    if symbol.is_empty() {
        return None;
    }
    Some(format!(r"\b{}\b", regex::escape(symbol)))
}

/// Emacs `hi-lock-write-interactive-patterns`: serialize each active pattern
/// `src` to a `Hi-lock:` line in the font-lock-keyword form Emacs writes, e.g.
/// `Hi-lock: (("foo" (0 (quote hi-yellow) t)))`. The caller comments the lines
/// with the buffer's comment token before inserting them; [`find_patterns`]
/// reads them back. Faces are fixed to `hi-yellow` because zmax assigns each
/// highlight a colour by index rather than by a stored face.
pub fn write_patterns_lines(sources: &[String]) -> Vec<String> {
    sources
        .iter()
        .map(|src| {
            // prin1 escapes backslashes and double quotes inside the string.
            let escaped = src.replace('\\', r"\\").replace('"', r#"\""#);
            format!("{FILE_PATTERNS_PREFIX}: ((\"{escaped}\" (0 (quote hi-yellow) t)))")
        })
        .collect()
}

/// Extract the first double-quoted string that follows a `Hi-lock:` tag on a
/// line (the regexp inside the font-lock keyword), un-escaping `\"` and `\\`.
fn read_quoted_regexp(after_tag: &str) -> Option<String> {
    let bytes = after_tag.as_bytes();
    let start = after_tag.find('"')? + 1;
    let mut out = String::new();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => {
                // Preserve the escape for regex metacharacters, but collapse the
                // prin1 escapes for a literal quote / backslash.
                let next = bytes[i + 1];
                if next == b'"' || next == b'\\' {
                    out.push(next as char);
                } else {
                    out.push('\\');
                    out.push(next as char);
                }
                i += 2;
            }
            b'"' => return Some(out),
            _ => {
                // Copy the whole UTF-8 char starting at i.
                let ch = after_tag[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    None
}

/// Emacs `hi-lock-find-patterns`: scan the head of `text` for file-local
/// `Hi-lock:` pattern lines and return the regexps they carry. Faithful to the
/// Emacs reader: the first `Hi-lock:` tag must begin within `range` bytes of the
/// buffer start; each subsequent tag must begin within 100 bytes of the previous
/// one; scanning stops at a `Hi-lock:` tag whose remainder is `end`.
pub fn find_patterns(text: &str, range: usize) -> Vec<String> {
    let tag = format!("{FILE_PATTERNS_PREFIX}:");
    let mut out = Vec::new();
    let Some(mut pos) = text.find(&tag) else {
        return out;
    };
    if pos >= range {
        return out;
    }
    loop {
        let after = &text[pos + tag.len()..];
        // `looking-at "\\s-*end"`: the region after the colon is the terminator.
        if after.trim_start().starts_with("end") {
            break;
        }
        if let Some(rx) = read_quoted_regexp(after) {
            out.push(rx);
        }
        // Next tag must appear within 100 bytes of this one (Emacs `(+ (point) 100)`).
        let search_from = pos + tag.len();
        match text[search_from..].find(&tag) {
            Some(rel) if rel <= 100 => pos = search_from + rel,
            _ => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pat(src: &str, whole_line: bool) -> Pattern {
        Pattern {
            re: Regex::new(src).unwrap(),
            whole_line,
            src: src.to_string(),
        }
    }

    #[test]
    fn finds_char_ranges_for_each_match() {
        let text = "foo bar foo";
        let pats = [pat("foo", false)];
        assert_eq!(viewport_matches(text, &pats), vec![(0, 3, 0), (8, 11, 0)]);
    }

    #[test]
    fn whole_line_expands_to_the_line() {
        // "alpha\nBUG here\ngamma\n": the pattern BUG expands to its whole line
        // "BUG here\n" = chars 6..=14 (through the trailing newline) -> [6, 15).
        let text = "alpha\nBUG here\ngamma\n";
        let pats = [pat("BUG", true)];
        assert_eq!(viewport_matches(text, &pats), vec![(6, 15, 0)]);
    }

    #[test]
    fn multiple_patterns_carry_their_index() {
        let text = "cat dog";
        let pats = [pat("cat", false), pat("dog", false)];
        let m = viewport_matches(text, &pats);
        assert_eq!(m, vec![(0, 3, 0), (4, 7, 1)]);
    }

    #[test]
    fn symbol_regexp_word_bounds_and_escapes() {
        assert_eq!(symbol_regexp("foo").as_deref(), Some(r"\bfoo\b"));
        // Regex metacharacters in the symbol are quoted.
        assert_eq!(symbol_regexp("a.b+").as_deref(), Some(r"\ba\.b\+\b"));
        assert_eq!(symbol_regexp("   ").as_deref(), None);
        assert_eq!(symbol_regexp("").as_deref(), None);
    }

    #[test]
    fn write_then_find_round_trips() {
        let srcs = vec![r"\bTODO\b".to_string(), "foo".to_string()];
        let lines = write_patterns_lines(&srcs);
        assert_eq!(
            lines[0],
            r#"Hi-lock: (("\\bTODO\\b" (0 (quote hi-yellow) t)))"#
        );
        // The written lines, read back, recover the original regexps.
        let doc = lines.join("\n");
        assert_eq!(find_patterns(&doc, FILE_PATTERNS_RANGE), srcs);
    }

    #[test]
    fn find_patterns_stops_at_end_and_honours_range() {
        let text = "// Hi-lock: ((\"aaa\" (0 (quote hi-yellow) t)))\n\
                    // Hi-lock: ((\"bbb\" (0 (quote hi-yellow) t)))\n\
                    // Hi-lock: end\n\
                    // Hi-lock: ((\"ccc\" (0 (quote hi-yellow) t)))\n";
        assert_eq!(
            find_patterns(text, FILE_PATTERNS_RANGE),
            vec!["aaa".to_string(), "bbb".to_string()]
        );
        // First tag past the range is ignored entirely.
        assert!(find_patterns(text, 1).is_empty());
    }

    #[test]
    fn match_groups_are_independent_slots() {
        // Uses the process-global PATTERNS/MATCH_GROUPS; run in isolation.
        clear();
        assert!(!clear_match_group(1)); // nothing to clear yet
        set_match_group(1, "foo").unwrap();
        set_match_group(2, "bar").unwrap();
        assert_eq!(sources(), vec!["foo".to_string(), "bar".to_string()]);
        // Replacing a slot drops its previous pattern but leaves the other.
        set_match_group(1, "baz").unwrap();
        assert_eq!(sources(), vec!["bar".to_string(), "baz".to_string()]);
        // Clearing slot 2 removes only its pattern.
        assert!(clear_match_group(2));
        assert_eq!(sources(), vec!["baz".to_string()]);
        // Out-of-range slots are rejected.
        assert!(set_match_group(4, "x").is_err());
        clear();
    }

    #[test]
    fn find_patterns_unescapes_quotes_and_backslashes() {
        let line = r#"; Hi-lock: (("a\"b\\c" (0 (quote hi-yellow) t)))"#;
        assert_eq!(find_patterns(line, FILE_PATTERNS_RANGE), vec![r#"a"b\c"#]);
    }
}
