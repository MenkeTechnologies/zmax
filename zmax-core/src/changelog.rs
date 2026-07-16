//! ChangeLog files — the zmax port of GNU Emacs `add-log.el`.
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

/// Emacs `log-edit-generate-changelog-from-diff` (which is `change-log-insert-entries`
/// over `diff-add-log-current-defuns`): read a unified diff and produce the
/// ChangeLog file lines for it — one per changed file, naming the functions the
/// hunks fall in.
///
/// The function names come from the hunk headers: git writes the enclosing
/// definition after the `@@ … @@` marker (its "funcname" hunk header), which is
/// exactly what `diff-add-log-current-defuns` reads. A hunk with no such context
/// contributes no name, so a file changed only outside any function gets the
/// plain `\t* FILE: ` line.
pub fn entries_from_diff(diff: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // (file, functions) in first-seen order.
    let mut files: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<usize> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            // `+++ b/src/main.c` — the post-image path. `/dev/null` is a deletion,
            // whose pre-image name was already taken from the `---` line.
            let path = rest.split('\t').next().unwrap_or(rest).trim();
            if path == "/dev/null" {
                continue;
            }
            let path = path.strip_prefix("b/").unwrap_or(path).to_string();
            current = Some(match files.iter().position(|(f, _)| *f == path) {
                Some(i) => i,
                None => {
                    files.push((path, Vec::new()));
                    files.len() - 1
                }
            });
        } else if line.starts_with("@@") {
            let Some(i) = current else { continue };
            // `@@ -1,7 +1,9 @@ fn parse(input: &str)` — everything after the second
            // `@@` is git's funcname context.
            let Some(after) = line.splitn(3, "@@").nth(2) else {
                continue;
            };
            if let Some(name) = defun_name(after.trim()) {
                if !files[i].1.contains(&name) {
                    files[i].1.push(name);
                }
            }
        }
    }
    for (file, funcs) in files {
        let symbol = if funcs.is_empty() {
            None
        } else {
            Some(funcs.join(", "))
        };
        out.push(file_line(&file, symbol.as_deref()));
    }
    out
}

/// The name of the definition a hunk's funcname context describes: the last
/// identifier before the argument list, so `pub fn parse_status(porcelain: &str)`
/// yields `parse_status` and `impl Component for MagitStatus {` yields
/// `MagitStatus`. Returns `None` for context that names nothing.
fn defun_name(context: &str) -> Option<String> {
    if context.is_empty() {
        return None;
    }
    // Cut the argument list / body brace off, then take the last identifier of
    // what remains — that is the name in every brace language's declaration.
    let head = context
        .split(['(', '{', '<', '='])
        .next()
        .unwrap_or(context)
        .trim();
    let name = head
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .find(|t| !t.is_empty())?;
    // A bare keyword is not a name (`else`, `impl` on its own line, …).
    if matches!(
        name,
        "if" | "else" | "impl" | "struct" | "enum" | "fn" | "def" | "class" | "match" | "for"
    ) {
        return None;
    }
    Some(name.to_string())
}

/// Emacs `log-edit-insert-changelog`: the ChangeLog text to seed a commit
/// message with — the newest entry's file lines that mention one of `files`,
/// each with the prose that follows it.
///
/// Emacs writes the ChangeLog first and commits with it; this reads the same
/// lines back. Only the newest matching entry for a file is taken (an older one
/// describes an older change).
pub fn entries_for_files(changelog: &str, files: &[String]) -> Vec<String> {
    let (_, entries) = split_entries(changelog);
    let mut out: Vec<String> = Vec::new();
    let mut claimed: Vec<String> = Vec::new();
    for entry in entries {
        // The file lines of this entry, each with any continuation lines.
        let mut blocks: Vec<Vec<&str>> = Vec::new();
        for line in entry.lines().skip(1) {
            if source_at(line).is_some() {
                blocks.push(vec![line]);
            } else if let Some(last) = blocks.last_mut() {
                if !line.trim().is_empty() {
                    last.push(line);
                }
            }
        }
        for block in blocks {
            let Some((file, _)) = source_at(block[0]) else {
                continue;
            };
            // Newest entry wins: once a file has contributed a block, older
            // entries for it are history, not this commit's message.
            if !files.contains(&file) || claimed.contains(&file) {
                continue;
            }
            claimed.push(file);
            let text = block
                .iter()
                .map(|l| l.trim_start_matches('\t').trim_end())
                .collect::<Vec<_>>()
                .join(" ");
            out.push(text.trim().to_string());
        }
    }
    out
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

    /// A real `git diff` (with its funcname hunk headers) is the input
    /// `log-edit-generate-changelog-from-diff` works from: one line per file,
    /// naming every function a hunk touched, in first-seen order.
    #[test]
    fn generates_changelog_entries_from_a_diff() {
        const DIFF: &str = "\
diff --git a/src/main.rs b/src/main.rs
index 1234567..89abcde 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,7 +10,7 @@ fn parse_status(porcelain: &str) -> Vec<Entry> {
-    let x = 1;
+    let x = 2;
@@ -40,6 +40,7 @@ pub fn render(area: Rect) {
+    draw();
@@ -80,3 +81,3 @@ fn parse_status(porcelain: &str) -> Vec<Entry> {
-    old();
+    new();
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1,2 +1,3 @@
+A new line outside any function.
";
        let lines = entries_from_diff(DIFF);
        assert_eq!(
            lines,
            vec![
                // Both functions, each once (the repeated hunk is not repeated),
                // in the order the hunks appear.
                "\t* src/main.rs (parse_status, render): ".to_string(),
                // No funcname context at all: the plain file line.
                "\t* README.md: ".to_string(),
            ]
        );
        // Every generated line reads back as a file line.
        for line in &lines {
            assert!(source_at(line).is_some(), "{line:?} is not a file line");
        }
        assert!(entries_from_diff("").is_empty());
    }

    /// The funcname context git writes is a whole declaration; the entry needs
    /// just the name out of it.
    #[test]
    fn defun_names_come_out_of_declaration_context() {
        assert_eq!(
            defun_name("pub fn parse_status(porcelain: &str)").as_deref(),
            Some("parse_status")
        );
        assert_eq!(
            defun_name("impl Component for MagitStatus {").as_deref(),
            Some("MagitStatus")
        );
        assert_eq!(defun_name("def handle(self):").as_deref(), Some("handle"));
        assert_eq!(
            defun_name("static int main(int argc)").as_deref(),
            Some("main")
        );
        // Nothing to name.
        assert_eq!(defun_name(""), None);
        assert_eq!(defun_name("else {"), None);
    }

    /// `log-edit-insert-changelog` seeds the commit message from the ChangeLog
    /// lines for the files being committed — the newest entry for each, and
    /// nothing about files that are not in the commit.
    #[test]
    fn insert_changelog_takes_the_newest_entry_per_file() {
        const HISTORY: &str = "2026-07-12  A  <a@example.com>\n\n\t* src/main.c (main): Fix the off-by-one.\n\tAlso tidy the loop.\n\n2026-07-01  A  <a@example.com>\n\n\t* src/main.c (main): An older change.\n\t* src/util.h: Declare it.\n\n";
        let files = vec!["src/main.c".to_string()];
        let entries = entries_for_files(HISTORY, &files);
        assert_eq!(
            entries,
            vec!["* src/main.c (main): Fix the off-by-one. Also tidy the loop.".to_string()],
            "the newest entry wins, with its continuation line folded in"
        );
        // A file not being committed contributes nothing…
        assert!(entries_for_files(HISTORY, &["src/other.c".to_string()]).is_empty());
        // …and a file whose only entry is in the older block still comes through.
        let entries = entries_for_files(HISTORY, &["src/util.h".to_string()]);
        assert_eq!(entries, vec!["* src/util.h: Declare it.".to_string()]);
    }
}
