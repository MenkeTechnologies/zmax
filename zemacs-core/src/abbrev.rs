//! Abbreviations — the zemacs port of the Vim/Neovim abbreviation table
//! (`:abbreviate`, `:iabbrev`, `:cabbrev` and their `un…`/`…clear` siblings).
//!
//! An abbreviation maps a typed word (the *lhs*) to an expansion (the *rhs*).
//! Vim keeps abbreviations per mode: `:iabbrev` is Insert-mode only, `:cabbrev`
//! is Command-line only, and plain `:abbreviate` applies to *both*. This module
//! is the pure, dependency-free state machine behind those commands: it owns the
//! three maps, add/remove/clear/lookup, and — the interesting part — the
//! "should this word expand" trigger classification.
//!
//! Vim recognises an abbreviation only when a non-keyword character is typed
//! after it, and only for one of three lhs shapes (`:help abbreviations`):
//!
//!   * **full-id** — every character is a keyword char (`foo`, `g3`).
//!   * **end-id**  — the last character is a keyword char and every *other*
//!     character is a non-keyword char (`#i`, `..f`).
//!   * **non-id**  — the last character is a non-keyword char; the others may be
//!     anything (`def#`, `4/7$`).
//!
//! Anything else (`a.b`, `#def`, `a b`) is not a valid abbreviation. Each shape
//! carries a rule for the character *in front of* the match at trigger time:
//! full-id and end-id require a non-keyword char (or start-of-line) before them,
//! while non-id requires a keyword char (or start-of-line). That distinction is
//! the whole reason the three shapes exist and is what `should_trigger` encodes.
//!
//! A "keyword character" here is ASCII-alphanumeric or `_` — the common core of
//! Vim's `'iskeyword'`. The command layer owns one `AbbrevTable`; wiring the
//! actual Insert-mode expansion into the typing path is a separate editor hook.

use std::collections::HashMap;

/// The three abbreviation shapes Vim accepts (`:help abbreviations`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbbrevKind {
    /// Every character is a keyword char, e.g. `foo`.
    FullId,
    /// Last char is a keyword char, every other char is non-keyword, e.g. `#i`.
    EndId,
    /// Last char is a non-keyword char; the rest may be anything, e.g. `def#`.
    NonId,
}

/// Which mode(s) an abbreviation belongs to. `Both` is plain `:abbreviate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbbrevMode {
    /// Insert mode only (`:iabbrev`).
    Insert,
    /// Command-line mode only (`:cabbrev`).
    Command,
    /// Both Insert and Command-line mode (plain `:abbreviate`).
    Both,
}

/// True for Vim's default-`'iskeyword'` core: ASCII letters, digits and `_`.
pub fn is_keyword_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Classify an abbreviation lhs, or `None` if it is not a valid abbreviation.
///
/// Empty strings are rejected. Otherwise the last character decides the family:
/// a non-keyword last char is always [`AbbrevKind::NonId`]; a keyword last char
/// is [`AbbrevKind::FullId`] when every char is a keyword char, or
/// [`AbbrevKind::EndId`] when every *other* char is non-keyword — and invalid
/// (mixed keyword/non-keyword prefix, like `a.b` or `#def`) otherwise.
pub fn classify(lhs: &str) -> Option<AbbrevKind> {
    let chars: Vec<char> = lhs.chars().collect();
    let (&last, rest) = chars.split_last()?; // None on empty
    if !is_keyword_char(last) {
        // Ends in a non-keyword char: non-id regardless of the prefix.
        return Some(AbbrevKind::NonId);
    }
    if rest.iter().all(|&c| is_keyword_char(c)) {
        Some(AbbrevKind::FullId)
    } else if rest.iter().all(|&c| !is_keyword_char(c)) {
        Some(AbbrevKind::EndId)
    } else {
        None // mixed keyword/non-keyword prefix before a keyword tail
    }
}

/// Would typing a trigger (non-keyword) char expand `lhs` here, given the
/// character immediately before it (`None` at start-of-line)?
///
/// full-id / end-id fire when the preceding char is a non-keyword char or there
/// is none; non-id fires when the preceding char is a keyword char or there is
/// none. Returns `false` for a non-abbreviation `lhs`.
pub fn should_trigger(lhs: &str, preceding: Option<char>) -> bool {
    match classify(lhs) {
        Some(AbbrevKind::FullId) | Some(AbbrevKind::EndId) => {
            preceding.is_none_or(|c| !is_keyword_char(c))
        }
        Some(AbbrevKind::NonId) => preceding.is_none_or(is_keyword_char),
        None => false,
    }
}

/// The abbreviation table: three maps keyed by lhs, one per [`AbbrevMode`].
#[derive(Clone, Debug, Default)]
pub struct AbbrevTable {
    insert: HashMap<String, String>,
    command: HashMap<String, String>,
    both: HashMap<String, String>,
}

impl AbbrevTable {
    pub fn new() -> Self {
        AbbrevTable::default()
    }

    /// Add (or overwrite) `lhs → rhs` for `mode`. Returns `false` without
    /// storing anything when `lhs` is not a valid abbreviation.
    pub fn add(&mut self, mode: AbbrevMode, lhs: &str, rhs: &str) -> bool {
        if classify(lhs).is_none() {
            return false;
        }
        // A word can only live in one mode-map at a time; re-defining it in a
        // different mode moves it (mirrors Vim replacing the old definition).
        self.insert.remove(lhs);
        self.command.remove(lhs);
        self.both.remove(lhs);
        self.map_mut(mode).insert(lhs.to_string(), rhs.to_string());
        true
    }

    /// Remove `lhs` for `mode`. `Insert`/`Command` also drop a `Both` entry
    /// (since a both-mode abbreviation is active in that mode); `Both` drops the
    /// word from every map. Returns whether anything was removed.
    pub fn remove(&mut self, mode: AbbrevMode, lhs: &str) -> bool {
        let both = self.both.remove(lhs).is_some();
        match mode {
            AbbrevMode::Insert => self.insert.remove(lhs).is_some() | both,
            AbbrevMode::Command => self.command.remove(lhs).is_some() | both,
            AbbrevMode::Both => {
                let i = self.insert.remove(lhs).is_some();
                let c = self.command.remove(lhs).is_some();
                i | c | both
            }
        }
    }

    /// Clear abbreviations for `mode`. `Insert`/`Command` also clear the shared
    /// `Both` map (its entries are active in that mode); `Both` clears all three.
    pub fn clear(&mut self, mode: AbbrevMode) {
        self.both.clear();
        match mode {
            AbbrevMode::Insert => self.insert.clear(),
            AbbrevMode::Command => self.command.clear(),
            AbbrevMode::Both => {
                self.insert.clear();
                self.command.clear();
            }
        }
    }

    /// Look up an exact `lhs` for `mode`: the mode-specific map first, then the
    /// shared `Both` map. `Both` searches all three (specific defs win).
    pub fn lookup(&self, mode: AbbrevMode, lhs: &str) -> Option<&str> {
        let chain: &[&HashMap<String, String>] = match mode {
            AbbrevMode::Insert => &[&self.insert, &self.both],
            AbbrevMode::Command => &[&self.command, &self.both],
            AbbrevMode::Both => &[&self.insert, &self.command, &self.both],
        };
        chain.iter().find_map(|m| m.get(lhs).map(String::as_str))
    }

    /// Find the abbreviation to expand at a cursor position in `mode`: `before`
    /// is the line text up to (not including) the just-typed trigger char. Picks
    /// the longest lhs that is a suffix of `before` and whose preceding-char rule
    /// ([`should_trigger`]) holds. Returns `(lhs, rhs)`.
    pub fn find_expansion(&self, mode: AbbrevMode, before: &str) -> Option<(String, String)> {
        let chars: Vec<char> = before.chars().collect();
        let mut best: Option<(String, String)> = None;
        for lhs in self.keys(mode) {
            let lhs_chars: Vec<char> = lhs.chars().collect();
            if lhs_chars.len() > chars.len() {
                continue;
            }
            let start = chars.len() - lhs_chars.len();
            if chars[start..] != lhs_chars[..] {
                continue;
            }
            let preceding = if start == 0 {
                None
            } else {
                Some(chars[start - 1])
            };
            if !should_trigger(&lhs, preceding) {
                continue;
            }
            let rhs = self.lookup(mode, &lhs)?.to_string();
            if best
                .as_ref()
                .is_none_or(|(b, _)| lhs.chars().count() > b.chars().count())
            {
                best = Some((lhs, rhs));
            }
        }
        best
    }

    /// Every `(lhs, rhs, mode)` triple, for `:abbreviate` with no args (listing).
    /// `Insert`/`Command` list their own map plus `Both`; `Both` lists all.
    pub fn entries(&self, mode: AbbrevMode) -> Vec<(String, String, AbbrevMode)> {
        let mut out = Vec::new();
        let mut push = |m: &HashMap<String, String>, tag: AbbrevMode| {
            for (k, v) in m {
                out.push((k.clone(), v.clone(), tag));
            }
        };
        match mode {
            AbbrevMode::Insert => {
                push(&self.insert, AbbrevMode::Insert);
                push(&self.both, AbbrevMode::Both);
            }
            AbbrevMode::Command => {
                push(&self.command, AbbrevMode::Command);
                push(&self.both, AbbrevMode::Both);
            }
            AbbrevMode::Both => {
                push(&self.insert, AbbrevMode::Insert);
                push(&self.command, AbbrevMode::Command);
                push(&self.both, AbbrevMode::Both);
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// True when no abbreviations are defined for `mode`.
    pub fn is_empty(&self, mode: AbbrevMode) -> bool {
        self.entries(mode).is_empty()
    }

    fn map_mut(&mut self, mode: AbbrevMode) -> &mut HashMap<String, String> {
        match mode {
            AbbrevMode::Insert => &mut self.insert,
            AbbrevMode::Command => &mut self.command,
            AbbrevMode::Both => &mut self.both,
        }
    }

    /// The lhs keys visible in `mode` (mode-specific map plus `Both`).
    fn keys(&self, mode: AbbrevMode) -> Vec<String> {
        let mut keys: Vec<String> = self.both.keys().cloned().collect();
        match mode {
            AbbrevMode::Insert => keys.extend(self.insert.keys().cloned()),
            AbbrevMode::Command => keys.extend(self.command.keys().cloned()),
            AbbrevMode::Both => {
                keys.extend(self.insert.keys().cloned());
                keys.extend(self.command.keys().cloned());
            }
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_full_id() {
        assert_eq!(classify("foo"), Some(AbbrevKind::FullId));
        assert_eq!(classify("g3"), Some(AbbrevKind::FullId));
        assert_eq!(classify("teh"), Some(AbbrevKind::FullId));
        assert_eq!(classify("_x"), Some(AbbrevKind::FullId));
    }

    #[test]
    fn classify_end_id() {
        // last char keyword, every other char non-keyword
        assert_eq!(classify("#i"), Some(AbbrevKind::EndId));
        assert_eq!(classify("..f"), Some(AbbrevKind::EndId));
        assert_eq!(classify("$/7"), Some(AbbrevKind::EndId));
    }

    #[test]
    fn classify_non_id() {
        // ends in a non-keyword char; prefix may be anything
        assert_eq!(classify("def#"), Some(AbbrevKind::NonId));
        assert_eq!(classify("4/7$"), Some(AbbrevKind::NonId));
        assert_eq!(classify("#"), Some(AbbrevKind::NonId));
        assert_eq!(classify("a.b."), Some(AbbrevKind::NonId));
    }

    #[test]
    fn classify_invalid() {
        assert_eq!(classify(""), None);
        assert_eq!(classify("a.b"), None); // keyword prefix before a non-kw then kw tail
        assert_eq!(classify("#def"), None); // mixed prefix before keyword tail
        assert_eq!(classify("a b"), None); // space in the middle before keyword tail
    }

    #[test]
    fn trigger_full_id_needs_non_keyword_before() {
        // "foo" is full-id: expands when preceded by a non-keyword char or none.
        assert!(should_trigger("foo", None));
        assert!(should_trigger("foo", Some(' ')));
        assert!(should_trigger("foo", Some('.')));
        assert!(!should_trigger("foo", Some('a'))); // keyword char before -> part of a longer word
        assert!(!should_trigger("foo", Some('9')));
    }

    #[test]
    fn trigger_non_id_needs_keyword_before() {
        // "def#" is non-id: expands when preceded by a keyword char or none.
        assert!(should_trigger("def#", None));
        assert!(should_trigger("def#", Some('a')));
        assert!(!should_trigger("def#", Some(' ')));
        assert!(!should_trigger("def#", Some('.')));
    }

    #[test]
    fn trigger_rejects_non_abbrev() {
        assert!(!should_trigger("a.b", None));
        assert!(!should_trigger("", None));
    }

    #[test]
    fn add_lookup_by_mode() {
        let mut t = AbbrevTable::new();
        assert!(t.add(AbbrevMode::Insert, "teh", "the"));
        assert!(t.add(AbbrevMode::Command, "wq", "write | quit"));
        assert!(t.add(AbbrevMode::Both, "adn", "and"));

        // insert sees its own map + both, not the command-only entry
        assert_eq!(t.lookup(AbbrevMode::Insert, "teh"), Some("the"));
        assert_eq!(t.lookup(AbbrevMode::Insert, "adn"), Some("and"));
        assert_eq!(t.lookup(AbbrevMode::Insert, "wq"), None);
        // command sees its own map + both, not the insert-only entry
        assert_eq!(t.lookup(AbbrevMode::Command, "wq"), Some("write | quit"));
        assert_eq!(t.lookup(AbbrevMode::Command, "adn"), Some("and"));
        assert_eq!(t.lookup(AbbrevMode::Command, "teh"), None);
    }

    #[test]
    fn add_rejects_invalid_lhs() {
        let mut t = AbbrevTable::new();
        assert!(!t.add(AbbrevMode::Insert, "a.b", "x"));
        assert_eq!(t.lookup(AbbrevMode::Insert, "a.b"), None);
    }

    #[test]
    fn redefining_moves_between_modes() {
        let mut t = AbbrevTable::new();
        t.add(AbbrevMode::Insert, "teh", "the");
        // redefining as command-only removes the insert entry
        t.add(AbbrevMode::Command, "teh", "THE");
        assert_eq!(t.lookup(AbbrevMode::Insert, "teh"), None);
        assert_eq!(t.lookup(AbbrevMode::Command, "teh"), Some("THE"));
    }

    #[test]
    fn remove_semantics() {
        let mut t = AbbrevTable::new();
        t.add(AbbrevMode::Both, "adn", "and");
        // :iunabbrev drops a both-mode entry (active in insert)
        assert!(t.remove(AbbrevMode::Insert, "adn"));
        assert_eq!(t.lookup(AbbrevMode::Command, "adn"), None);
        assert!(!t.remove(AbbrevMode::Insert, "adn")); // already gone

        t.add(AbbrevMode::Insert, "teh", "the");
        // :cunabbrev must not touch an insert-only entry
        assert!(!t.remove(AbbrevMode::Command, "teh"));
        assert_eq!(t.lookup(AbbrevMode::Insert, "teh"), Some("the"));
        assert!(t.remove(AbbrevMode::Both, "teh"));
        assert_eq!(t.lookup(AbbrevMode::Insert, "teh"), None);
    }

    #[test]
    fn clear_semantics() {
        let mut t = AbbrevTable::new();
        t.add(AbbrevMode::Insert, "teh", "the");
        t.add(AbbrevMode::Command, "wq", "wq!");
        t.add(AbbrevMode::Both, "adn", "and");
        // :iabclear clears insert + both, leaves command intact
        t.clear(AbbrevMode::Insert);
        assert_eq!(t.lookup(AbbrevMode::Insert, "teh"), None);
        assert_eq!(t.lookup(AbbrevMode::Insert, "adn"), None);
        assert_eq!(t.lookup(AbbrevMode::Command, "wq"), Some("wq!"));
        // :abclear clears everything
        t.clear(AbbrevMode::Both);
        assert!(t.is_empty(AbbrevMode::Both));
    }

    #[test]
    fn find_expansion_picks_valid_and_longest() {
        let mut t = AbbrevTable::new();
        t.add(AbbrevMode::Insert, "teh", "the");
        t.add(AbbrevMode::Insert, "adn", "and");

        // typed "teh" then a space -> before == "teh"
        assert_eq!(
            t.find_expansion(AbbrevMode::Insert, "teh"),
            Some(("teh".to_string(), "the".to_string()))
        );
        // "xteh": preceding char 'x' is a keyword char, full-id must NOT fire
        assert_eq!(t.find_expansion(AbbrevMode::Insert, "xteh"), None);
        // "say teh": preceding char ' ' is fine
        assert_eq!(
            t.find_expansion(AbbrevMode::Insert, "say teh"),
            Some(("teh".to_string(), "the".to_string()))
        );
        // no matching abbreviation at the end
        assert_eq!(t.find_expansion(AbbrevMode::Insert, "hello"), None);
    }

    #[test]
    fn find_expansion_longest_wins() {
        let mut t = AbbrevTable::new();
        t.add(AbbrevMode::Insert, "def#", "define#");
        t.add(AbbrevMode::Insert, "abcdef#", "ABCDEF#");
        // both are suffixes of "abcdef#"; longest lhs should win
        assert_eq!(
            t.find_expansion(AbbrevMode::Insert, "abcdef#"),
            Some(("abcdef#".to_string(), "ABCDEF#".to_string()))
        );
    }

    #[test]
    fn entries_sorted_and_scoped() {
        let mut t = AbbrevTable::new();
        t.add(AbbrevMode::Insert, "teh", "the");
        t.add(AbbrevMode::Command, "wq", "wq!");
        t.add(AbbrevMode::Both, "adn", "and");
        let ins = t.entries(AbbrevMode::Insert);
        assert_eq!(
            ins,
            vec![
                ("adn".to_string(), "and".to_string(), AbbrevMode::Both),
                ("teh".to_string(), "the".to_string(), AbbrevMode::Insert),
            ]
        );
        assert_eq!(t.entries(AbbrevMode::Both).len(), 3);
    }
}
