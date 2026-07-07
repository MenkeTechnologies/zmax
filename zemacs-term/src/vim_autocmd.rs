//! vim autocommands (`:autocmd`). A registry of (events, file-pattern, command)
//! entries; when a lifecycle event fires (from the command layer, which has a
//! `Context` to run the command), every entry whose event and file pattern match
//! runs its `:command`. Registration/parsing/matching are pure and unit-tested;
//! firing runs each command through `run_command_line`.

use std::cell::RefCell;

thread_local! {
    static AUTOCMDS: RefCell<Vec<Autocmd>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Autocmd {
    /// Event names this entry fires on, normalized to lowercase (e.g. `bufwritepost`).
    pub events: Vec<String>,
    /// File-name glob (`*`, `*.rs`, `Makefile`); `*` matches everything.
    pub pattern: String,
    /// The `:command` line to run when the entry fires.
    pub command: String,
}

/// Parse a `:autocmd` argument line: `{events} {pattern} {command...}`. Events
/// are comma-separated; the command is the remainder. Returns None if any of the
/// three parts is missing.
pub fn parse_autocmd(args: &str) -> Option<Autocmd> {
    let args = args.trim();
    let mut parts = args.splitn(3, char::is_whitespace);
    let events = parts.next()?.trim();
    let pattern = parts.next()?.trim();
    let command = parts.next()?.trim();
    if events.is_empty() || pattern.is_empty() || command.is_empty() {
        return None;
    }
    let events: Vec<String> = events
        .split(',')
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect();
    if events.is_empty() {
        return None;
    }
    Some(Autocmd {
        events,
        pattern: pattern.to_string(),
        command: command.to_string(),
    })
}

/// Glob match for autocmd patterns: `*` is a wildcard (any run, including empty),
/// everything else is literal. `*` alone matches anything. Matched against the
/// file name (and, for patterns containing `/`, the full path by the caller).
pub fn pattern_matches(pattern: &str, name: &str) -> bool {
    fn m(p: &[u8], s: &[u8]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some(b'*') => m(&p[1..], s) || (!s.is_empty() && m(p, &s[1..])),
            Some(&c) => !s.is_empty() && s[0] == c && m(&p[1..], &s[1..]),
        }
    }
    m(pattern.as_bytes(), name.as_bytes())
}

/// Register an autocmd (vim `:autocmd {events} {pat} {cmd}`).
pub fn register(entry: Autocmd) {
    AUTOCMDS.with(|a| a.borrow_mut().push(entry));
}

/// Clear autocmds (vim `:autocmd!`). `None` clears all; `Some(event)` clears the
/// entries that fire on that (lowercased) event.
pub fn clear(event: Option<&str>) {
    AUTOCMDS.with(|a| {
        let mut list = a.borrow_mut();
        match event {
            None => list.clear(),
            Some(ev) => {
                let ev = ev.to_ascii_lowercase();
                list.retain(|e| !e.events.contains(&ev));
            }
        }
    });
}

/// The commands to run for `event` on a buffer named `name` (in registration
/// order). `name` is the file name (or `""` for an unnamed buffer).
pub fn matching_commands(event: &str, name: &str) -> Vec<String> {
    let event = event.to_ascii_lowercase();
    AUTOCMDS.with(|a| {
        a.borrow()
            .iter()
            .filter(|e| e.events.contains(&event) && pattern_matches(&e.pattern, name))
            .map(|e| e.command.clone())
            .collect()
    })
}

/// Number of registered autocmds (for `:autocmd` listing / tests).
pub fn len() -> usize {
    AUTOCMDS.with(|a| a.borrow().len())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_events_pattern_command() {
        let a = parse_autocmd("BufWritePost,BufRead *.rs setlocal sw=4").unwrap();
        assert_eq!(a.events, vec!["bufwritepost", "bufread"]);
        assert_eq!(a.pattern, "*.rs");
        assert_eq!(a.command, "setlocal sw=4");
        assert!(parse_autocmd("BufRead *.rs").is_none()); // no command
    }

    #[test]
    fn globs_match() {
        assert!(pattern_matches("*", "anything.txt"));
        assert!(pattern_matches("*.rs", "main.rs"));
        assert!(!pattern_matches("*.rs", "main.py"));
        assert!(pattern_matches("Makefile", "Makefile"));
        assert!(pattern_matches("*test*", "my_test_file"));
        assert!(!pattern_matches("*.rs", "rs"));
    }

    #[test]
    fn register_match_clear() {
        clear(None);
        register(parse_autocmd("BufWritePost *.rs echo saved").unwrap());
        register(parse_autocmd("BufRead *.py echo py").unwrap());
        assert_eq!(
            matching_commands("bufwritepost", "a.rs"),
            vec!["echo saved"]
        );
        assert_eq!(
            matching_commands("BufWritePost", "a.py"),
            Vec::<String>::new()
        );
        assert_eq!(matching_commands("bufread", "x.py"), vec!["echo py"]);
        clear(Some("BufRead"));
        assert!(matching_commands("bufread", "x.py").is_empty());
        assert_eq!(len(), 1);
        clear(None);
    }
}
