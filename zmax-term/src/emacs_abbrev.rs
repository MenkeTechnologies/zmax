//! Emacs abbrevs (`C-x a g` define, `C-x '` expand).
//!
//! A persistent table of abbreviation -> expansion, stored at
//! `<config-dir>/abbrevs` as `name\texpansion` lines. This ports emacs's
//! *explicit* expansion (`expand-abbrev`, `C-x '`) plus a define command;
//! auto-expansion as you type (full `abbrev-mode`) would need an insert-keypress
//! hook and is left for later — explicit expansion is the non-invasive core.
//!
//! Tab in a name is unsupported (names are single words); newline/tab in an
//! expansion are escaped so the one-row-per-line store stays intact.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use zmax_loader::config_dir;

const FILE_NAME: &str = "abbrevs";

fn store_path() -> PathBuf {
    config_dir().join(FILE_NAME)
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse one `name\texpansion` row (expansion is unescaped).
fn parse_line(line: &str) -> Option<(String, String)> {
    let (name, exp) = line.split_once('\t')?;
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), unescape(exp)))
}

fn format_line(name: &str, expansion: &str) -> String {
    format!("{}\t{}", name, escape(expansion))
}

fn load() -> Vec<(String, String)> {
    match std::fs::read_to_string(store_path()) {
        Ok(s) => s.lines().filter_map(parse_line).collect(),
        Err(_) => Vec::new(),
    }
}

fn save(rows: &[(String, String)]) {
    let body: String = rows
        .iter()
        .map(|(n, e)| format_line(n, e))
        .collect::<Vec<_>>()
        .join("\n");
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, body);
}

/// Define (or replace) an abbrev.
pub fn define(name: &str, expansion: &str) {
    let mut rows = load();
    rows.retain(|(n, _)| n != name);
    rows.push((name.to_string(), expansion.to_string()));
    save(&rows);
}

/// `(define-abbrev global-abbrev-table name nil)` — the global-table twin of
/// [`undefine_mode`], reached with a negative prefix argument when
/// `only-global-abbrevs` routes the mode-abbrev commands to the global table.
/// Returns whether the table had it.
pub fn undefine(name: &str) -> bool {
    let mut rows = load();
    let before = rows.len();
    rows.retain(|(n, _)| n != name);
    let removed = rows.len() != before;
    if removed {
        save(&rows);
    }
    removed
}

/// Look up an abbrev's expansion.
pub fn get(name: &str) -> Option<String> {
    load().into_iter().find(|(n, _)| n == name).map(|(_, e)| e)
}

/// `write-abbrev-file`: write every abbrev to `path` in the store's
/// `name\texpansion` format. Returns how many abbrevs were written.
pub fn write_to(path: &Path) -> std::io::Result<usize> {
    let rows = load();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body: String = rows
        .iter()
        .map(|(n, e)| format_line(n, e))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, body)?;
    Ok(rows.len())
}

/// `read-abbrev-file`: read abbrevs from `path` and merge them into the store
/// (a definition replaces a same-named one). Returns how many were read.
pub fn read_from(path: &Path) -> std::io::Result<usize> {
    let text = std::fs::read_to_string(path)?;
    Ok(define_from_text(&text))
}

/// Every abbrev as a `name\texpansion` block, for `insert-abbrevs`.
pub fn serialize() -> String {
    load()
        .iter()
        .map(|(n, e)| format_line(n, e))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Define abbrevs from a `name\texpansion` block (`define-abbrevs`,
/// `read-abbrev-file`), merging into the store. Returns how many were defined.
pub fn define_from_text(text: &str) -> usize {
    let incoming: Vec<(String, String)> = text.lines().filter_map(parse_line).collect();
    let n = incoming.len();
    let mut rows = load();
    for (name, exp) in &incoming {
        rows.retain(|(nn, _)| nn != name);
        rows.push((name.clone(), exp.clone()));
    }
    save(&rows);
    n
}

// --- Mode-local (major-mode) abbrev tables ---------------------------------
//
// Emacs keeps a per-major-mode `*-mode-abbrev-table` alongside the
// `global-abbrev-table`; `expand-abbrev` searches the buffer's local table
// before the global one. These tables are keyed by the mode name (zmax's
// document language, e.g. `rust`), and are in-memory for the session — the
// global table above stays file-backed, matching the `abbrevs` file emacs
// persists by default while mode abbrevs are typically (re)defined per session.

static MODE_TABLES: Lazy<Mutex<HashMap<String, HashMap<String, String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// `define-mode-abbrev` — define (or replace) `name → expansion` in `mode`'s
/// local abbrev table.
pub fn define_mode(mode: &str, name: &str, expansion: &str) {
    MODE_TABLES
        .lock()
        .unwrap()
        .entry(mode.to_string())
        .or_default()
        .insert(name.to_string(), expansion.to_string());
}

/// `(define-abbrev table name nil)` with a nil expansion — emacs's way of
/// undefining an abbrev, which `add-mode-abbrev` reaches with a negative prefix
/// argument. Returns whether the mode's table had it.
pub fn undefine_mode(mode: &str, name: &str) -> bool {
    MODE_TABLES
        .lock()
        .unwrap()
        .get_mut(mode)
        .is_some_and(|t| t.remove(name).is_some())
}

/// Look up `name` in `mode`'s local abbrev table only.
pub fn get_mode(mode: &str, name: &str) -> Option<String> {
    MODE_TABLES
        .lock()
        .unwrap()
        .get(mode)
        .and_then(|t| t.get(name).cloned())
}

/// The lookup `expand-abbrev` uses: the buffer's mode-local table (when `mode`
/// is set) wins over the global table, mirroring emacs's local-then-global
/// search order.
pub fn get_effective(mode: Option<&str>, name: &str) -> Option<String> {
    if let Some(m) = mode {
        if let Some(exp) = get_mode(m, name) {
            return Some(exp);
        }
    }
    get(name)
}

/// All mode-local abbrevs for `mode`, sorted by name (for `list-abbrevs`).
pub fn mode_entries(mode: &str) -> Vec<(String, String)> {
    let tables = MODE_TABLES.lock().unwrap();
    let Some(t) = tables.get(mode) else {
        return Vec::new();
    };
    let mut v: Vec<(String, String)> = t.iter().map(|(n, e)| (n.clone(), e.clone())).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

// --- abbrev-suggest (lisp/abbrev.el) ---------------------------------------
//
// Emacs 28.1 added an opt-in nag: with `abbrev-suggest' non-nil, after each
// self-insert emacs checks whether the words just typed match the *expansion*
// of a defined abbrev and, if using the abbrev would have saved at least
// `abbrev-suggest-hint-threshold' characters, tells you so and remembers the
// miss in `abbrev--suggest-saved-recommendations'. `abbrev-suggest-show-report'
// then tallies that list by expansion.
//
// This is the whole of that machinery except the two hooks into the editor: the
// per-keypress call to [`suggest_maybe_suggest`] and the command that renders
// [`suggest_show_report`] — both live in commands.rs.

/// `abbrev-suggest` — whether misses are detected and recorded at all.
static SUGGEST: Mutex<bool> = Mutex::new(false);

/// `abbrev-suggest-hint-threshold` (defcustom, default 3): how many characters
/// the abbrev must save before the miss is worth reporting.
static SUGGEST_HINT_THRESHOLD: Mutex<i64> = Mutex::new(3);

/// `abbrev--suggest-saved-recommendations`: every recorded miss, as
/// `(expansion, abbrev)`. Duplicates accumulate — the count *is* the tally.
static SAVED_RECOMMENDATIONS: Lazy<Mutex<Vec<(String, String)>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// Read `abbrev-suggest`.
pub fn suggest() -> bool {
    *SUGGEST.lock().unwrap()
}

/// Set `abbrev-suggest`.
pub fn set_suggest(on: bool) {
    *SUGGEST.lock().unwrap() = on;
}

/// Read `abbrev-suggest-hint-threshold`.
pub fn suggest_hint_threshold() -> i64 {
    *SUGGEST_HINT_THRESHOLD.lock().unwrap()
}

/// Set `abbrev-suggest-hint-threshold`.
pub fn set_suggest_hint_threshold(n: i64) {
    *SUGGEST_HINT_THRESHOLD.lock().unwrap() = n;
}

/// `abbrev--suggest-count-words`: `(split-string expansion " " t)` — runs of
/// spaces separate, empties dropped.
fn suggest_count_words(expansion: &str) -> usize {
    expansion.split(' ').filter(|w| !w.is_empty()).count()
}

/// `abbrev--suggest-above-threshold`: the abbrev has to be at least
/// `abbrev-suggest-hint-threshold` characters shorter than its expansion.
fn suggest_above_threshold(expansion: &str, abbrev: &str) -> bool {
    (expansion.chars().count() as i64 - abbrev.chars().count() as i64) >= suggest_hint_threshold()
}

/// `abbrev--suggest-get-active-abbrev-expansions`: every abbrev reachable from
/// the buffer, as `(expansion, name)`. zmax's two tables (mode-local, then
/// global) stand in for emacs's active-tables-plus-parents walk.
fn suggest_active_expansions(mode: Option<&str>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if let Some(m) = mode {
        out.extend(mode_entries(m).into_iter().map(|(n, e)| (e, n)));
    }
    out.extend(load().into_iter().map(|(n, e)| (e, n)));
    out
}

/// `abbrev--suggest-shortest-abbrev`: shorter abbrev wins, ties keep the
/// incumbent.
fn suggest_shortest_abbrev(
    new: (String, String),
    current: Option<(String, String)>,
) -> (String, String) {
    match current {
        None => new,
        Some(cur) => {
            if new.1.chars().count() < cur.1.chars().count() {
                new
            } else {
                cur
            }
        }
    }
}

/// Emacs matches the expansion against the typed words with `string-match`,
/// i.e. as a *regexp*, case-insensitively. Expansions are usually literal text,
/// so an expansion that isn't valid regexp syntax falls back to a
/// case-insensitive substring search rather than being silently skipped.
fn suggest_matches(expansion: &str, words: &str) -> bool {
    match regex::RegexBuilder::new(expansion)
        .case_insensitive(true)
        .build()
    {
        Ok(re) => re.is_match(words),
        Err(_) => words.to_lowercase().contains(&expansion.to_lowercase()),
    }
}

/// `abbrev--suggest-inform-user`: record the miss and hand back the message
/// emacs shows in the echo area.
fn suggest_inform_user(expansion: &str, abbrev: &str) -> String {
    SAVED_RECOMMENDATIONS
        .lock()
        .unwrap()
        .push((expansion.to_string(), abbrev.to_string()));
    format!(
        "You can write `{}' using the abbrev `{}'.",
        expansion, abbrev
    )
}

/// `abbrev--suggest-maybe-suggest`: for each active abbrev, compare its
/// expansion against the same number of words before point (supplied by
/// `previous_words`, emacs's `abbrev--suggest-get-previous-words`, which
/// squashes whitespace to single spaces). The shortest qualifying abbrev is
/// recorded; the returned string is the echo-area message, if any.
pub fn suggest_maybe_suggest<F>(mode: Option<&str>, previous_words: F) -> Option<String>
where
    F: Fn(usize) -> String,
{
    let mut found: Option<(String, String)> = None;
    for (expansion, abbrev) in suggest_active_expansions(mode) {
        let word_count = suggest_count_words(&expansion);
        if word_count == 0 {
            continue;
        }
        let words = previous_words(word_count);
        if suggest_matches(&expansion, &words) && suggest_above_threshold(&expansion, &abbrev) {
            found = Some(suggest_shortest_abbrev((expansion, abbrev), found));
        }
    }
    let (expansion, abbrev) = found?;
    Some(suggest_inform_user(&expansion, &abbrev))
}

/// `abbrev--suggest-get-totals`: tally the recorded misses by expansion. Both
/// emacs lists are built with `push`, so it walks the misses newest-first and
/// prepends each new expansion — leaving the most recently missed expansion
/// last. The report prints the result as-is, so the order is part of the port.
pub fn suggest_get_totals() -> Vec<(String, usize)> {
    let mut totals: Vec<(String, usize)> = Vec::new();
    for (expansion, _) in SAVED_RECOMMENDATIONS.lock().unwrap().iter().rev() {
        match totals.iter_mut().find(|(e, _)| e == expansion) {
            Some((_, count)) => *count += 1,
            None => totals.insert(0, (expansion.clone(), 1)),
        }
    }
    totals
}

/// `abbrev-suggest-show-report`: the text of the `*abbrev-suggest*` buffer —
/// the header verbatim, then ` EXPANSION: COUNT` per missed expansion. The
/// header's `\\[edit-abbrevs]` renders as `M-x edit-abbrevs` because, as in
/// emacs, the command has no key binding.
pub fn suggest_show_report() -> String {
    let mut out = String::from(
        "** Abbrev expansion usage **\n\n\
         Below is a list of expansions for which abbrevs are defined, and\n\
         the number of times the expansion was typed manually.  To display\n\
         and edit all abbrevs, type M-x edit-abbrevs.\n\n",
    );
    for (expansion, count) in suggest_get_totals() {
        out.push_str(&format!(" {}: {}\n", expansion, count));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_tables_are_local_to_their_mode() {
        // Hermetic: touches only the in-memory MODE_TABLES, never the file store,
        // so it can't pollute the user's `abbrevs` file. Names are unique to this
        // test to survive the shared process-global map.
        define_mode("mtl_rust", "mtl_only", "rust-only");
        define_mode("mtl_rust", "mtl_two", "rust-two");
        // Resolvable only within its own mode.
        assert_eq!(
            get_mode("mtl_rust", "mtl_only").as_deref(),
            Some("rust-only")
        );
        assert_eq!(
            get_effective(Some("mtl_rust"), "mtl_only").as_deref(),
            Some("rust-only")
        );
        // Another mode and the global-only lookup (None) don't see it. `get(None)`
        // falls through to the (empty-for-this-name) global store.
        assert!(get_effective(Some("mtl_python"), "mtl_only").is_none());
        assert!(get_effective(None, "mtl_only").is_none());
        // A later define_mode for the same key replaces the expansion.
        define_mode("mtl_rust", "mtl_only", "rust-only-v2");
        assert_eq!(
            get_mode("mtl_rust", "mtl_only").as_deref(),
            Some("rust-only-v2")
        );
        // mode_entries lists that mode's abbrevs, sorted by name.
        let e = mode_entries("mtl_rust");
        let names: Vec<&str> = e.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["mtl_only", "mtl_two"]);
        assert!(mode_entries("mtl_nonexistent").is_empty());
        // Clean up this test's mode keys.
        let mut t = MODE_TABLES.lock().unwrap();
        t.remove("mtl_rust");
    }

    #[test]
    fn suggest_threshold_shortest_abbrev_and_report() {
        // Hermetic: only the pure helpers plus the in-memory recommendation
        // list, never the file store.
        assert_eq!(suggest_count_words("  the  quick brown "), 3);
        assert_eq!(suggest_count_words(""), 0);
        // Default threshold 3: saving exactly 3 characters qualifies, 2 doesn't.
        set_suggest_hint_threshold(3);
        assert!(suggest_above_threshold("abcd", "a"));
        assert!(!suggest_above_threshold("abc", "a"));
        // A zero threshold reports every abbrev, however marginal.
        set_suggest_hint_threshold(0);
        assert!(suggest_above_threshold("ab", "ab"));
        set_suggest_hint_threshold(3);
        // The shortest abbrev wins; an equal-length rival keeps the incumbent.
        let cur = ("expansion".to_string(), "exp".to_string());
        let short = suggest_shortest_abbrev(("expansion".to_string(), "e".to_string()), Some(cur));
        assert_eq!(short.1, "e");
        let tie = suggest_shortest_abbrev(
            ("expansion".to_string(), "xx".to_string()),
            Some(("expansion".to_string(), "yy".to_string())),
        );
        assert_eq!(tie.1, "yy");
        // Matching is case-insensitive, and a non-regexp expansion still matches
        // literally rather than being dropped.
        assert!(suggest_matches("Hello There", "hello there"));
        assert!(suggest_matches("a(b", "typed a(b just now"));
        // Two misses of one expansion and one of another tally as 2 and 1, and
        // the report prints the emacs header above them.
        SAVED_RECOMMENDATIONS.lock().unwrap().clear();
        suggest_inform_user("for example", "feg");
        let msg = suggest_inform_user("for example", "feg");
        assert_eq!(msg, "You can write `for example' using the abbrev `feg'.");
        suggest_inform_user("in other words", "iow");
        let totals = suggest_get_totals();
        assert_eq!(
            totals,
            vec![
                ("for example".to_string(), 2),
                ("in other words".to_string(), 1)
            ]
        );
        let report = suggest_show_report();
        assert!(report.starts_with("** Abbrev expansion usage **\n\n"));
        assert!(report.ends_with(" for example: 2\n in other words: 1\n"));
        SAVED_RECOMMENDATIONS.lock().unwrap().clear();
    }

    #[test]
    fn round_trips_with_escaping() {
        let line = format_line("teh", "the\ttab\nand newline");
        assert_eq!(line, "teh\tthe\\ttab\\nand newline");
        let (name, exp) = parse_line(&line).unwrap();
        assert_eq!(name, "teh");
        assert_eq!(exp, "the\ttab\nand newline");
    }

    #[test]
    fn rejects_nameless_or_tabless() {
        assert!(parse_line("no-tab").is_none());
        assert!(parse_line("\texpansion").is_none());
    }

    #[test]
    fn unescape_handles_trailing_and_unknown_backslash() {
        assert_eq!(unescape("a\\"), "a\\");
        assert_eq!(unescape("a\\x"), "a\\x");
    }
}
