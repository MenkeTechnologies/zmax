//! Comint — the filesystem/process-free core of the zemacs port of GNU Emacs
//! `comint-mode`, the base mode for interactive subprocess buffers (`M-x shell`,
//! inferior-lisp, `gud`).
//!
//! This module holds only the pure, unit-tested logic: the **input ring**
//! (`comint-input-ring`) that records submitted inputs and cycles through them
//! for `comint-previous-input` / `comint-next-input` /
//! `comint-previous-matching-input`. All process I/O, rendering and key handling
//! lives in `zemacs-term::ui::comint`.

/// Emacs `comint-input-ring-size` default.
pub const DEFAULT_RING_SIZE: usize = 500;

/// A bounded history of submitted inputs, newest first — the port of
/// `comint-input-ring` plus its navigation index (`comint-input-ring-index`).
#[derive(Debug, Clone)]
pub struct InputRing {
    /// Newest input at index 0.
    items: Vec<String>,
    /// Current navigation position into `items`, or `None` when not navigating
    /// (the caret is on a fresh, unsubmitted input line).
    index: Option<usize>,
    max: usize,
    /// Port of `comint-input-ignoredups`: skip adding an input identical to the
    /// most recent one.
    ignoredups: bool,
}

impl Default for InputRing {
    fn default() -> Self {
        Self::new(DEFAULT_RING_SIZE)
    }
}

impl InputRing {
    pub fn new(max: usize) -> Self {
        Self {
            items: Vec::new(),
            index: None,
            max: max.max(1),
            ignoredups: true,
        }
    }

    pub fn with_ignoredups(mut self, ignoredups: bool) -> Self {
        self.ignoredups = ignoredups;
        self
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Record a submitted input (`comint-add-to-input-history`). Empty inputs are
    /// ignored; with `ignoredups`, an input equal to the newest is skipped. Adding
    /// always ends any in-progress history navigation.
    pub fn add(&mut self, input: &str) {
        self.index = None;
        if input.is_empty() {
            return;
        }
        if self.ignoredups && self.items.first().map(String::as_str) == Some(input) {
            return;
        }
        self.items.insert(0, input.to_string());
        if self.items.len() > self.max {
            self.items.truncate(self.max);
        }
    }

    /// `comint-previous-input`: step to an older entry. From a fresh line the
    /// first call yields the most-recent input; further calls walk back, clamping
    /// at the oldest. Returns the entry now under the caret, or `None` if empty.
    pub fn previous(&mut self) -> Option<&str> {
        if self.items.is_empty() {
            return None;
        }
        self.index = Some(match self.index {
            None => 0,
            Some(i) => (i + 1).min(self.items.len() - 1),
        });
        self.index.map(|i| self.items[i].as_str())
    }

    /// `comint-next-input`: step to a newer entry. Stepping past the newest entry
    /// stops navigation and returns `None` (the caller restores the fresh input).
    pub fn next(&mut self) -> Option<&str> {
        match self.index {
            None | Some(0) => {
                self.index = None;
                None
            }
            Some(i) => {
                let ni = i - 1;
                self.index = Some(ni);
                Some(self.items[ni].as_str())
            }
        }
    }

    /// End history navigation without moving (called when the caret leaves the
    /// input line or a new input is edited).
    pub fn reset(&mut self) {
        self.index = None;
    }

    /// Whether the ring is currently mid-navigation.
    pub fn navigating(&self) -> bool {
        self.index.is_some()
    }

    /// `comint-previous-matching-input`: the first entry strictly older than the
    /// current position that contains `needle` as a substring. Advances the index
    /// to that entry when found. `None` leaves the index unchanged.
    pub fn previous_matching(&mut self, needle: &str) -> Option<&str> {
        if self.items.is_empty() {
            return None;
        }
        let start = match self.index {
            None => 0,
            Some(i) => i + 1,
        };
        let found = (start..self.items.len()).find(|&i| self.items[i].contains(needle));
        if let Some(i) = found {
            self.index = Some(i);
            Some(self.items[i].as_str())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_ignores_empty_and_dups() {
        let mut r = InputRing::new(10);
        r.add("ls");
        r.add("");
        r.add("ls"); // dup of newest -> skipped
        r.add("pwd");
        assert_eq!(r.len(), 2);
        // newest first
        assert_eq!(r.previous(), Some("pwd"));
        assert_eq!(r.previous(), Some("ls"));
    }

    #[test]
    fn dups_allowed_when_disabled() {
        let mut r = InputRing::new(10).with_ignoredups(false);
        r.add("ls");
        r.add("ls");
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn previous_next_navigation_clamps_and_returns_to_fresh() {
        let mut r = InputRing::new(10);
        for cmd in ["a", "b", "c"] {
            r.add(cmd);
        }
        // items: [c, b, a]
        assert_eq!(r.previous(), Some("c"));
        assert_eq!(r.previous(), Some("b"));
        assert_eq!(r.previous(), Some("a"));
        assert_eq!(r.previous(), Some("a")); // clamp at oldest
        assert_eq!(r.next(), Some("b"));
        assert_eq!(r.next(), Some("c"));
        assert!(r.navigating());
        assert_eq!(r.next(), None); // past newest -> fresh line
        assert!(!r.navigating());
    }

    #[test]
    fn ring_is_capped_to_max() {
        let mut r = InputRing::new(2);
        for cmd in ["a", "b", "c", "d"] {
            r.add(cmd);
        }
        assert_eq!(r.len(), 2);
        assert_eq!(r.previous(), Some("d"));
        assert_eq!(r.previous(), Some("c"));
        assert_eq!(r.previous(), Some("c")); // "a"/"b" evicted
    }

    #[test]
    fn previous_matching_finds_older_substring() {
        let mut r = InputRing::new(10);
        for cmd in ["git status", "ls -l", "git commit", "cd .."] {
            r.add(cmd);
        }
        // items: [cd .., git commit, ls -l, git status]
        assert_eq!(r.previous_matching("git"), Some("git commit"));
        assert_eq!(r.previous_matching("git"), Some("git status"));
        assert_eq!(r.previous_matching("git"), None); // no older match; index unchanged
    }
}
