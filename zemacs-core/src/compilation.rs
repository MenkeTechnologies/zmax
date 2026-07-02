//! Compilation error list — the zemacs port of GNU Emacs `compile` /
//! Compilation mode error navigation.
//!
//! Emacs' `compile` runs a shell command, collects its output into a
//! `*compilation*` buffer, scans that output with `compilation-error-regexp-alist`
//! for lines that name a source location, and lets you walk those locations with
//! `next-error` (`M-g n`) / `previous-error` (`M-g p`) / `first-error`, visiting
//! each file at its line (and column, when the tool reports one). This module is
//! the pure, dependency-free state machine behind those commands: a parser that
//! turns raw process output into an ordered list of [`CompileEntry`] locations,
//! plus a current-index cursor with next/previous/first navigation (the same
//! quickfix-style motion Vim's `:cnext`/`:cprev` provides, kept separate from the
//! Vim quickfix list which lives in `quickfix.rs`).
//!
//! The parser recognises the everyday location formats that
//! `compilation-error-regexp-alist` covers in practice:
//!
//!   * `file:line:col: error: message`  — GCC/Clang/rustc with a column.
//!   * `file:line:col: warning: message`
//!   * `file:line: message`             — GNU-style without a column.
//!   * `file:line:col:message`          — colon-joined (e.g. some linters).
//!   * `file:line:message`              — grep -n matches (classified as info).
//!
//! Lines that do not name a `file:line` location (banners, progress spinners,
//! blank lines, a bare summary such as `error: aborting due to 2 errors`) are
//! ignored, matching Emacs' behaviour of only stopping on lines the regexps hit.
//! The command layer owns one [`CompilationList`], fills it from the captured
//! output, and opens the entry the cursor lands on.

/// The severity Emacs assigns a compilation location. Emacs distinguishes error
/// (the default stop), warning, and info (`note:` lines, grep matches); only the
/// first two are stops for the plain `next-error`, but we keep all three so the
/// command layer / a compilation buffer can colour them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Error,
    Warning,
    Info,
}

/// One parsed source location from the compilation output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileEntry {
    /// The file named by the line (verbatim, as the tool printed it — the command
    /// layer expands `~` and resolves it against the working directory).
    pub file: String,
    /// 1-based line number, exactly as the tool reported it.
    pub line: usize,
    /// 1-based column, when the tool reported one (`file:line:col:`).
    pub col: Option<usize>,
    /// The severity inferred from the message text.
    pub kind: ErrorKind,
    /// The message following the location (already stripped of the leading colon
    /// and surrounding whitespace).
    pub text: String,
}

/// An ordered compilation error list with a current-entry cursor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompilationList {
    entries: Vec<CompileEntry>,
    /// Index of the entry `next-error` last visited; `None` before the first move.
    current: Option<usize>,
}

impl CompilationList {
    pub fn new() -> Self {
        CompilationList::default()
    }

    /// Replace the list with the entries parsed from `output`, resetting the
    /// cursor to "nothing visited yet" (so the next `next-error` lands on the
    /// first entry, matching Emacs after a fresh `compile`).
    pub fn set_output(&mut self, output: &str) {
        self.entries = parse_output(output);
        self.current = None;
    }

    pub fn entries(&self) -> &[CompileEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The current index, if `next-error`/`first-error` has moved the cursor.
    pub fn index(&self) -> Option<usize> {
        self.current
    }

    /// The entry the cursor is on, if any.
    pub fn current(&self) -> Option<&CompileEntry> {
        self.current.and_then(|i| self.entries.get(i))
    }

    /// `next-error` (`M-g n`) — advance to the next entry and return it. From the
    /// fresh state (nothing visited) this lands on the first entry. Returns `None`
    /// without moving when already on the last entry (Emacs' "Moved past last
    /// error").
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&CompileEntry> {
        let n = match self.current {
            None => 0,
            Some(i) => i + 1,
        };
        if n < self.entries.len() {
            self.current = Some(n);
            self.entries.get(n)
        } else {
            None
        }
    }

    /// `previous-error` (`M-g p`) — retreat to the previous entry. Returns `None`
    /// without moving when already on the first entry, or when nothing has been
    /// visited yet ("Moved back before first error").
    pub fn previous(&mut self) -> Option<&CompileEntry> {
        match self.current {
            Some(i) if i > 0 => {
                self.current = Some(i - 1);
                self.entries.get(i - 1)
            }
            _ => None,
        }
    }

    /// `first-error` — jump the cursor to the first entry and return it.
    pub fn first(&mut self) -> Option<&CompileEntry> {
        if self.entries.is_empty() {
            self.current = None;
            None
        } else {
            self.current = Some(0);
            self.entries.first()
        }
    }
}

/// Parse raw compilation output into an ordered list of source locations,
/// dropping every line that does not name a `file:line` position.
pub fn parse_output(output: &str) -> Vec<CompileEntry> {
    output.lines().filter_map(parse_line).collect()
}

/// Parse a single output line into a [`CompileEntry`], or `None` if it names no
/// `file:line` location. See the module docs for the recognised formats.
pub fn parse_line(raw: &str) -> Option<CompileEntry> {
    let line = raw.trim_end_matches(['\n', '\r']);
    // Find the `<file>:<line>` split: the first ':' whose left side is a
    // non-blank file name and whose right side begins with digits. Scanning past
    // false colons (e.g. the drive letter in `C:\path`) lands on the real one.
    let mut from = 0;
    loop {
        let colon = line[from..].find(':')? + from;
        let file = &line[..colon];
        let rest = &line[colon + 1..];

        if file.trim().is_empty() {
            from = colon + 1;
            continue;
        }
        let line_digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if line_digits.is_empty() {
            from = colon + 1;
            continue;
        }

        let line_no: usize = line_digits.parse().ok()?;
        let after_line = &rest[line_digits.len()..];

        // Optional `:col` — only when the ':' is followed by digits. A ':' before
        // non-digit text (grep's `file:line:text`) is not a column.
        let (col, after_col) = parse_optional_col(after_line);

        let text = strip_leading_colon(after_col).trim().to_string();
        let kind = classify(&text);
        return Some(CompileEntry {
            file: file.to_string(),
            line: line_no,
            col,
            kind,
            text,
        });
    }
}

/// If `s` starts with `:<digits>`, split off the column and return the remainder;
/// otherwise report no column and hand back `s` unchanged.
fn parse_optional_col(s: &str) -> (Option<usize>, &str) {
    let Some(after_colon) = s.strip_prefix(':') else {
        return (None, s);
    };
    let digits: String = after_colon
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return (None, s);
    }
    match digits.parse::<usize>() {
        Ok(col) => (Some(col), &after_colon[digits.len()..]),
        Err(_) => (None, s),
    }
}

/// Drop a single leading `:` (the separator before the message).
fn strip_leading_colon(s: &str) -> &str {
    s.strip_prefix(':').unwrap_or(s)
}

/// Infer severity from the message text. GCC/Clang/rustc lead the message with
/// `error:` / `warning:` / `note:`; anything else (a grep match) is info.
fn classify(text: &str) -> ErrorKind {
    let lower = text.to_ascii_lowercase();
    if lower.contains("error") {
        ErrorKind::Error
    } else if lower.contains("warning") {
        ErrorKind::Warning
    } else {
        // `note:`/`info:` and any unrecognised text (e.g. a grep match) are info.
        ErrorKind::Info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gcc_error_with_column() {
        let e = parse_line("src/main.c:42:7: error: expected ';' before '}'").unwrap();
        assert_eq!(e.file, "src/main.c");
        assert_eq!(e.line, 42);
        assert_eq!(e.col, Some(7));
        assert_eq!(e.kind, ErrorKind::Error);
        assert_eq!(e.text, "error: expected ';' before '}'");
    }

    #[test]
    fn parses_rustc_warning_with_column() {
        let e = parse_line("zemacs-core/src/lib.rs:10:5: warning: unused variable: `x`").unwrap();
        assert_eq!(e.file, "zemacs-core/src/lib.rs");
        assert_eq!(e.line, 10);
        assert_eq!(e.col, Some(5));
        assert_eq!(e.kind, ErrorKind::Warning);
    }

    #[test]
    fn parses_gnu_style_no_column() {
        // `file:line: message` — no column group.
        let e = parse_line("Makefile:12: *** missing separator.  Stop.").unwrap();
        assert_eq!(e.file, "Makefile");
        assert_eq!(e.line, 12);
        assert_eq!(e.col, None);
        assert_eq!(e.text, "*** missing separator.  Stop.");
    }

    #[test]
    fn parses_grep_match_as_info() {
        // grep -n: `file:line:matched text` — the second colon is not a column.
        let e = parse_line("notes.txt:3:the quick brown fox").unwrap();
        assert_eq!(e.file, "notes.txt");
        assert_eq!(e.line, 3);
        assert_eq!(e.col, None);
        assert_eq!(e.kind, ErrorKind::Info);
        assert_eq!(e.text, "the quick brown fox");
    }

    #[test]
    fn parses_note_line_as_info() {
        let e = parse_line("src/x.rs:1:1: note: consider borrowing here").unwrap();
        assert_eq!(e.kind, ErrorKind::Info);
        assert_eq!(e.col, Some(1));
    }

    #[test]
    fn windows_drive_letter_is_not_the_location_colon() {
        let e = parse_line(r"C:\proj\a.c:5:3: error: boom").unwrap();
        assert_eq!(e.file, r"C:\proj\a.c");
        assert_eq!(e.line, 5);
        assert_eq!(e.col, Some(3));
    }

    #[test]
    fn ignores_noise_lines() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   Compiling zemacs-core v0.1.0").is_none());
        assert!(parse_line("error: aborting due to 2 previous errors").is_none());
        assert!(parse_line("make: *** [all] Error 1").is_none());
        assert!(parse_line("Finished in 3.2s").is_none());
        // A colon with no line number after it is not a location.
        assert!(parse_line("warning: something happened").is_none());
    }

    #[test]
    fn parse_output_drops_noise_and_keeps_locations() {
        let out = "\
   Compiling foo v0.1.0
src/a.rs:1:1: error: first
random progress line
src/b.rs:2: warning: second
grep.txt:9:hit
error: aborting due to previous error
";
        let list = parse_output(out);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].file, "src/a.rs");
        assert_eq!(list[1].file, "src/b.rs");
        assert_eq!(list[1].col, None);
        assert_eq!(list[2].kind, ErrorKind::Info);
    }

    fn sample() -> CompilationList {
        let mut c = CompilationList::new();
        c.set_output("a.rs:1:1: error: one\nb.rs:2:1: warning: two\nc.rs:3: error: three\n");
        c
    }

    #[test]
    fn next_walks_from_fresh_state() {
        let mut c = sample();
        assert_eq!(c.index(), None);
        assert_eq!(c.next().unwrap().file, "a.rs");
        assert_eq!(c.next().unwrap().file, "b.rs");
        assert_eq!(c.next().unwrap().file, "c.rs");
        assert!(c.next().is_none()); // past last: no move
        assert_eq!(c.current().unwrap().file, "c.rs");
    }

    #[test]
    fn previous_stops_before_first() {
        let mut c = sample();
        c.next(); // a.rs
        c.next(); // b.rs
        assert_eq!(c.previous().unwrap().file, "a.rs");
        assert!(c.previous().is_none()); // before first: no move
        assert_eq!(c.current().unwrap().file, "a.rs");
    }

    #[test]
    fn previous_from_fresh_state_is_none() {
        let mut c = sample();
        assert!(c.previous().is_none());
        assert_eq!(c.index(), None);
    }

    #[test]
    fn first_jumps_to_start() {
        let mut c = sample();
        c.next();
        c.next();
        assert_eq!(c.first().unwrap().file, "a.rs");
        assert_eq!(c.index(), Some(0));
    }

    #[test]
    fn navigation_on_empty_list() {
        let mut c = CompilationList::new();
        c.set_output("no locations here\njust text\n");
        assert!(c.is_empty());
        assert!(c.next().is_none());
        assert!(c.previous().is_none());
        assert!(c.first().is_none());
        assert_eq!(c.index(), None);
    }

    #[test]
    fn set_output_resets_cursor() {
        let mut c = sample();
        c.next();
        c.next();
        assert_eq!(c.index(), Some(1));
        c.set_output("x.rs:5:5: error: fresh\n");
        assert_eq!(c.index(), None);
        assert_eq!(c.next().unwrap().file, "x.rs");
    }
}
