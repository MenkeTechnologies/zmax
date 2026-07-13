//! ChangeLog files — the zemacs port of GNU Emacs `add-log.el`.
//!
//! A ChangeLog is a stack of dated entries, newest first:
//!
//! ```text
//! 2026-07-12  A Hacker  <hacker@example.com>
//!
//! \t* src/main.c (main): Fix the off-by-one.
//! \t* src/util.h: Declare it.
//! ```
//!
//! An *entry* is the author/date line plus everything up to the next such line;
//! a *file line* inside it names a source file and, in parentheses, the function
//! the change touched. This module is the pure part: build those lines, read
//! them back, and merge two ChangeLogs into one date-ordered file.

/// Emacs `add-log-full-name` / `add-log-mailing-address` header:
/// `DATE  NAME  <EMAIL>`.
pub fn entry_header(date: &str, name: &str, email: &str) -> String {
    format!("{date}  {name}  <{email}>")
}

/// The file line Emacs inserts for a change: `\t* FILE (SYMBOL): ` — or
/// `\t* FILE: ` when the change is not inside a named function.
pub fn file_line(file: &str, symbol: Option<&str>) -> String {
    match symbol {
        Some(s) if !s.is_empty() => format!("\t* {file} ({s}): "),
        _ => format!("\t* {file}: "),
    }
}

/// Read a file line back: the source file it names and the symbol in parens.
/// This is what `change-log-goto-source` follows to open the changed code.
pub fn source_at(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.trim_start().strip_prefix("* ")?;
    // The file name runs up to the first `(`, `:` or `,` — whichever comes first.
    let end = rest.find(['(', ':', ',']).unwrap_or(rest.len());
    let file = rest[..end].trim();
    if file.is_empty() {
        return None;
    }
    let symbol = rest[end..]
        .strip_prefix('(')
        .and_then(|s| s.split_once(')'))
        .map(|(sym, _)| sym.trim().to_string())
        .filter(|s| !s.is_empty());
    Some((file.to_string(), symbol))
}

/// True when `line` starts a new ChangeLog entry: an unindented line beginning
/// with an ISO date (`YYYY-MM-DD`). Emacs also accepts the old ctime format, but
/// every ChangeLog it writes today uses ISO.
fn is_entry_header(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() >= 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
}

/// Split a ChangeLog into its entries. Anything before the first dated header
/// (a file-local-variables preamble, say) is returned as the first element of
/// the tuple and stays put when merging.
pub fn split_entries(text: &str) -> (String, Vec<String>) {
    let mut preamble = String::new();
    let mut entries: Vec<String> = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.split_inclusive('\n') {
        if is_entry_header(line) {
            if let Some(e) = cur.take() {
                entries.push(e);
            }
            cur = Some(line.to_string());
        } else if let Some(e) = cur.as_mut() {
            e.push_str(line);
        } else {
            preamble.push_str(line);
        }
    }
    if let Some(e) = cur {
        entries.push(e);
    }
    (preamble, entries)
}

/// The date an entry is filed under (its first 10 characters).
fn entry_date(entry: &str) -> &str {
    &entry[..10.min(entry.len())]
}

/// Emacs `change-log-merge`: fold `other`'s entries into `into`, keeping the
/// result sorted newest-first and dropping entries that are already present
/// verbatim. The merge is stable, so same-date entries keep their relative order
/// with `into`'s ahead of `other`'s.
pub fn merge(into: &str, other: &str) -> String {
    let (preamble, mut entries) = split_entries(into);
    let (_, incoming) = split_entries(other);
    for e in incoming {
        if !entries.iter().any(|x| x.trim_end() == e.trim_end()) {
            entries.push(e);
        }
    }
    // Newest first; `sort_by` is stable so equal dates keep insertion order.
    entries.sort_by(|a, b| entry_date(b).cmp(entry_date(a)));
    let mut out = preamble;
    for e in entries {
        out.push_str(&e);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Where a new file line goes: if the newest entry already has `header`, the new
/// line joins it; otherwise a fresh entry is opened at the top. Returns the text
/// to insert at char offset 0 of the ChangeLog.
pub fn insert_entry(existing: &str, header: &str, file_line: &str) -> String {
    let (_, entries) = split_entries(existing);
    let same_author_today = entries
        .first()
        .is_some_and(|e| e.lines().next() == Some(header));
    if same_author_today {
        // Slot the file line in directly under the existing header.
        format!("{file_line}\n")
    } else {
        format!("{header}\n\n{file_line}\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "2026-07-12  A Hacker  <a@example.com>\n\n\t* src/main.c (main): Fix.\n\n2026-07-01  B Hacker  <b@example.com>\n\n\t* src/util.h: Declare.\n\n";

    /// The file line is what every other command reads back, so its shape and
    /// its inverse must agree.
    #[test]
    fn file_line_round_trips_through_source_at() {
        assert_eq!(
            file_line("src/main.c", Some("main")),
            "\t* src/main.c (main): "
        );
        assert_eq!(file_line("README", None), "\t* README: ");
        assert_eq!(file_line("README", Some("")), "\t* README: ");

        let (f, s) = source_at("\t* src/main.c (main): Fix.").unwrap();
        assert_eq!((f.as_str(), s.as_deref()), ("src/main.c", Some("main")));

        let (f, s) = source_at("\t* README: Update.").unwrap();
        assert_eq!((f.as_str(), s.as_deref()), ("README", None));

        // Not a file line.
        assert_eq!(source_at("2026-07-12  A Hacker  <a@example.com>"), None);
        assert_eq!(source_at("\tPlain prose."), None);
    }

    /// Entries are split on unindented ISO dates — a date inside the prose of an
    /// entry (indented) must not start a new one.
    #[test]
    fn splits_on_unindented_dates_only() {
        let (pre, entries) = split_entries(LOG);
        assert_eq!(pre, "");
        assert_eq!(entries.len(), 2);
        assert!(entries[0].starts_with("2026-07-12"));
        assert!(entries[0].contains("src/main.c"));
        assert!(entries[1].starts_with("2026-07-01"));

        let indented = "2026-07-12  X  <x@e.com>\n\n\t* a.c: See 2020-01-01 for why.\n";
        let (_, entries) = split_entries(indented);
        assert_eq!(entries.len(), 1, "the indented date is prose, not a header");
    }

    /// Merging is the whole feature of `change-log-merge`: newest first, no
    /// duplicates, and the other file's entries interleave by date.
    #[test]
    fn merge_interleaves_by_date_and_drops_duplicates() {
        let other = "2026-07-05  C Hacker  <c@example.com>\n\n\t* src/z.c: New.\n\n2026-07-12  A Hacker  <a@example.com>\n\n\t* src/main.c (main): Fix.\n\n";
        let merged = merge(LOG, other);
        let (_, entries) = split_entries(&merged);
        let dates: Vec<&str> = entries.iter().map(|e| &e[..10]).collect();
        assert_eq!(
            dates,
            vec!["2026-07-12", "2026-07-05", "2026-07-01"],
            "newest first, and the duplicate 07-12 entry appears once"
        );
        assert!(merged.contains("src/z.c"));
    }

    /// A second change on the same day by the same author extends the open entry
    /// instead of opening a new one — the behaviour that keeps ChangeLogs tidy.
    #[test]
    fn insert_entry_reuses_todays_header() {
        let header = "2026-07-12  A Hacker  <a@example.com>";
        let line = file_line("src/new.c", Some("f"));
        assert_eq!(insert_entry(LOG, header, &line), format!("{line}\n"));

        let other = "2026-07-12  B Hacker  <b@example.com>";
        let ins = insert_entry(LOG, other, &line);
        assert!(
            ins.starts_with(other),
            "a different author opens a new entry"
        );
        assert!(ins.contains(&line));

        // An empty ChangeLog always opens a fresh entry.
        assert!(insert_entry("", header, &line).starts_with(header));
    }
}
