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

    /// The `i`-th entry counting back from the newest (0 = most recent).
    pub fn get(&self, i: usize) -> Option<&str> {
        self.items.get(i).map(String::as_str)
    }

    /// The most recently submitted input, if any (`comint-input-ring` head).
    pub fn newest(&self) -> Option<&str> {
        self.items.first().map(String::as_str)
    }

    /// All entries, newest-first — used by `comint-dynamic-list-input-ring`.
    pub fn items(&self) -> &[String] {
        &self.items
    }

    /// The current navigation index (0 = newest), or `None` when on a fresh line.
    pub fn index(&self) -> Option<usize> {
        self.index
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

// --------------------------------------------------------------------------
// Pure, unit-tested helpers for the comint editing/navigation commands. These
// hold no process or buffer state; the `zemacs-term::ui::comint` component calls
// them and applies the result to its real input line / scrollback / child.
// --------------------------------------------------------------------------

/// Port of `comint-bol-or-process-mark` (bound to `C-a`): the target column when
/// the caret is at `caret` and the process mark (start of the editable input) is
/// at `process_mark`. If the caret is not already at the process mark, go there;
/// if it is, go to the true beginning of line (column 0, before the prompt). In
/// zemacs's single-line input model `process_mark` is normally 0, so both cases
/// collapse to 0 — but the helper keeps Emacs's two-step semantics faithfully.
pub fn bol_or_process_mark_target(caret: usize, process_mark: usize) -> usize {
    if caret == process_mark {
        0
    } else {
        process_mark
    }
}

/// Port of `comint-strip-ctrl-m`: remove carriage returns (`^M`, `\r`) from a
/// line of subprocess output (DOS/telnet line endings).
pub fn strip_ctrl_m(s: &str) -> String {
    s.replace('\r', "")
}

/// The last whitespace-delimited argument of `cmd` — the value inserted by
/// `comint-insert-previous-argument` (`M-.`, i.e. `!$`). Returns `None` for an
/// empty/blank command.
pub fn last_argument(cmd: &str) -> Option<&str> {
    cmd.split_whitespace().next_back()
}

/// Everything after the first whitespace-delimited word of `cmd` (the `!*`
/// history designator: all the arguments). `None` when there are no arguments.
pub fn all_arguments(cmd: &str) -> Option<String> {
    let trimmed = cmd.trim_start();
    let rest = trimmed.split_whitespace().skip(1).collect::<Vec<_>>();
    if rest.is_empty() {
        None
    } else {
        Some(rest.join(" "))
    }
}

/// Port of the csh/bash history expansion behind `comint-magic-space` /
/// `comint-replace-by-expanded-history`. Expands the history designators that map
/// cleanly onto the input ring, scanning `input` for `!`-events:
///   `!!`        the previous command
///   `!$`        the last argument of the previous command
///   `!*`        all arguments of the previous command
///   `!-N`       the N-th previous command
///   `!STR`      the most recent command starting with STR
/// Returns `Some(expanded)` when at least one designator was replaced, else
/// `None` (nothing to expand — the input is unchanged). A lone `!` or an event
/// with no matching history entry is left verbatim.
pub fn expand_history(input: &str, ring: &InputRing) -> Option<String> {
    let bytes: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut expanded = false;
    while i < bytes.len() {
        if bytes[i] != '!' || i + 1 >= bytes.len() {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let next = bytes[i + 1];
        // `!!`, `!$`, `!*`
        if next == '!' {
            if let Some(cmd) = ring.newest() {
                out.push_str(cmd);
                expanded = true;
            } else {
                out.push('!');
                out.push('!');
            }
            i += 2;
            continue;
        }
        if next == '$' {
            match ring.newest().and_then(last_argument) {
                Some(arg) => {
                    out.push_str(arg);
                    expanded = true;
                }
                None => {
                    out.push('!');
                    out.push('$');
                }
            }
            i += 2;
            continue;
        }
        if next == '*' {
            match ring.newest().and_then(all_arguments) {
                Some(args) => {
                    out.push_str(&args);
                    expanded = true;
                }
                None => {
                    out.push('!');
                    out.push('*');
                }
            }
            i += 2;
            continue;
        }
        // `!-N` : N-th previous command.
        if next == '-' {
            let mut j = i + 2;
            let mut num = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                num.push(bytes[j]);
                j += 1;
            }
            if let Ok(n) = num.parse::<usize>() {
                if n >= 1 {
                    if let Some(cmd) = ring.get(n - 1) {
                        out.push_str(cmd);
                        expanded = true;
                        i = j;
                        continue;
                    }
                }
            }
            // Not a valid designator: emit literally and move past the `!`.
            out.push('!');
            i += 1;
            continue;
        }
        // `!STR` : most recent command starting with STR (STR = word chars).
        if next.is_alphanumeric() || next == '_' || next == '/' || next == '.' {
            let mut j = i + 1;
            let mut pfx = String::new();
            while j < bytes.len()
                && (bytes[j].is_alphanumeric()
                    || bytes[j] == '_'
                    || bytes[j] == '/'
                    || bytes[j] == '.'
                    || bytes[j] == '-')
            {
                pfx.push(bytes[j]);
                j += 1;
            }
            let found = ring.items().iter().find(|c| c.starts_with(&pfx));
            if let Some(cmd) = found {
                out.push_str(cmd);
                expanded = true;
                i = j;
                continue;
            }
            // No match: leave the whole `!STR` verbatim.
            out.push('!');
            i += 1;
            continue;
        }
        // Unknown designator: keep the `!` literally.
        out.push('!');
        i += 1;
    }
    expanded.then_some(out)
}

/// Port of `comint-delete-output` / `comint-show-output`'s notion of "the last
/// command's output": given a per-scrollback-line flag marking which lines are
/// echoed prompt/input lines, return the `[start, end)` range of the output that
/// followed the most recent prompt line (exclusive of the prompt itself). `None`
/// when there is no prompt line or no output after it.
pub fn last_output_range(is_prompt: &[bool]) -> Option<(usize, usize)> {
    let last_prompt = is_prompt.iter().rposition(|&p| p)?;
    let start = last_prompt + 1;
    let end = is_prompt.len();
    if start < end {
        Some((start, end))
    } else {
        None
    }
}

/// The scrollback line index of the last prompt (input) line — the anchor
/// `comint-show-output` scrolls to. `None` when no prompt has been echoed.
pub fn last_prompt_line(is_prompt: &[bool]) -> Option<usize> {
    is_prompt.iter().rposition(|&p| p)
}

/// Shell-command word boundaries for `shell-forward-command` (`M-f`) and
/// `shell-backward-command` (`M-b`): treat `;`, `|`, `&`, and newlines as the
/// separators between shell commands on the input line. `forward` returns the
/// caret column just past the end of the command at/after `caret`; `!forward`
/// returns the start column of the command at/before `caret`. Operates on char
/// columns.
pub fn forward_command(input: &str, caret: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let is_sep = |c: char| matches!(c, ';' | '|' | '&' | '\n');
    let mut i = caret.min(n);
    // Skip leading separators/space.
    while i < n && (is_sep(chars[i]) || chars[i].is_whitespace()) {
        i += 1;
    }
    // Advance to the next separator.
    while i < n && !is_sep(chars[i]) {
        i += 1;
    }
    i
}

/// Backward counterpart of [`forward_command`].
pub fn backward_command(input: &str, caret: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let is_sep = |c: char| matches!(c, ';' | '|' | '&' | '\n');
    let mut i = caret.min(n);
    // Step back over separators/space immediately before the caret.
    while i > 0 && (is_sep(chars[i - 1]) || chars[i - 1].is_whitespace()) {
        i -= 1;
    }
    // Step back to the start of this command (just past a separator or bol).
    while i > 0 && !is_sep(chars[i - 1]) {
        i -= 1;
    }
    // Skip forward over the separator and leading whitespace to the command's
    // first non-blank character.
    while i < n && (is_sep(chars[i]) || chars[i].is_whitespace()) {
        i += 1;
    }
    i
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

    #[test]
    fn bol_or_process_mark_two_step() {
        // Not at the process mark -> go to the process mark.
        assert_eq!(bol_or_process_mark_target(6, 2), 2);
        // Already at the process mark -> go to true beginning of line (0).
        assert_eq!(bol_or_process_mark_target(2, 2), 0);
        // Single-line model (process_mark = 0): always column 0.
        assert_eq!(bol_or_process_mark_target(5, 0), 0);
    }

    #[test]
    fn strip_ctrl_m_removes_carriage_returns() {
        assert_eq!(strip_ctrl_m("hello\r"), "hello");
        assert_eq!(strip_ctrl_m("a\rb\r\n"), "ab\n");
        assert_eq!(strip_ctrl_m("clean"), "clean");
    }

    #[test]
    fn last_and_all_arguments() {
        assert_eq!(last_argument("git commit -m msg"), Some("msg"));
        assert_eq!(last_argument("ls"), Some("ls"));
        assert_eq!(last_argument("   "), None);
        assert_eq!(all_arguments("git commit -m msg"), Some("commit -m msg".into()));
        assert_eq!(all_arguments("ls"), None);
    }

    #[test]
    fn expand_history_designators() {
        let mut r = InputRing::new(10);
        for cmd in ["make clean", "git status", "cargo build --release"] {
            r.add(cmd);
        }
        // items newest-first: [cargo build --release, git status, make clean]
        assert_eq!(
            expand_history("!!", &r),
            Some("cargo build --release".into())
        );
        assert_eq!(expand_history("run !$", &r), Some("run --release".into()));
        assert_eq!(expand_history("echo !*", &r), Some("echo build --release".into()));
        assert_eq!(expand_history("!-2", &r), Some("git status".into()));
        assert_eq!(expand_history("!git", &r), Some("git status".into()));
        // No designator -> None (unchanged).
        assert_eq!(expand_history("plain text", &r), None);
        // Unknown/unmatched -> left verbatim, treated as no expansion.
        assert_eq!(expand_history("!nope", &r), None);
    }

    #[test]
    fn expand_history_empty_ring_is_literal() {
        let r = InputRing::new(10);
        assert_eq!(expand_history("!!", &r), None);
        assert_eq!(expand_history("!$", &r), None);
    }

    #[test]
    fn last_output_range_after_last_prompt() {
        // lines: prompt, out, out, prompt, out  (indices 0..5)
        let flags = [true, false, false, true, false];
        assert_eq!(last_output_range(&flags), Some((4, 5)));
        assert_eq!(last_prompt_line(&flags), Some(3));
        // A trailing prompt with no output yet.
        let flags2 = [true, false, true];
        assert_eq!(last_output_range(&flags2), None);
        // No prompts at all.
        assert_eq!(last_output_range(&[false, false]), None);
        assert_eq!(last_prompt_line(&[false, false]), None);
    }

    #[test]
    fn shell_forward_backward_command() {
        let line = "echo hi | grep x ; ls -l";
        // From column 0, forward stops at end of "echo hi " -> just before '|'.
        let f1 = forward_command(line, 0);
        assert_eq!(&line[..f1], "echo hi ");
        // Continuing forward skips the '|' and stops before ';'.
        let f2 = forward_command(line, f1);
        assert_eq!(&line[..f2], "echo hi | grep x ");
        // Backward from the very end lands at the start of "ls -l".
        let b = backward_command(line, line.chars().count());
        assert_eq!(&line[b..], "ls -l");
    }
}
