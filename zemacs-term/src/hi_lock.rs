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

/// The active pattern sources, for completion and status.
pub fn sources() -> Vec<String> {
    PATTERNS.lock().unwrap().iter().map(|p| p.src.clone()).collect()
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
        assert_eq!(
            viewport_matches(text, &pats),
            vec![(0, 3, 0), (8, 11, 0)]
        );
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
}
