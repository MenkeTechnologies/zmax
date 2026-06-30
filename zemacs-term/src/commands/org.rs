//! Org-mode support — slice 1: pure outline + TODO helpers.
//!
//! This module holds the engine-agnostic, unit-tested logic for org-style
//! outlines: detecting headings, finding a heading's subtree, cycling its TODO
//! keyword, and promoting/demoting its level. The typable commands that drive
//! the editor (`:org-cycle`, `:org-todo`, `:org-promote`, …) live in
//! `commands/typed.rs` and call into these helpers; folding reuses the
//! document's existing `zemacs_core::fold::Folds` model exactly like the vim
//! `z*` fold commands.
//!
//! Slice 2 adds the **agenda** ([`parse_agenda`] / [`AgendaItem`], driven by the
//! `OrgAgenda` overlay) and heading **priority** cycling ([`priority_cycle`]).
//!
//! Deferred to later slices: recurring timestamps, real date math / a "today"
//! grouping, babel, export, capture, keymap binds, `.org` file-type detection and
//! syntax highlighting.

use std::path::{Path, PathBuf};

/// Org heading depth: the number of leading `*` characters immediately followed
/// by a space (`* ` → 1, `** ` → 2, …). A run of stars not followed by a space
/// (`*notspace`, bare `***`) is not a heading and returns `None`. Headings must
/// start in column 0 — leading whitespace disqualifies the line.
pub fn heading_level(line: &str) -> Option<usize> {
    let stars = line.chars().take_while(|&c| c == '*').count();
    if stars == 0 {
        return None;
    }
    // The char right after the stars must be a space.
    if line[stars..].starts_with(' ') {
        Some(stars)
    } else {
        None
    }
}

/// Last line (0-based, inclusive) of the subtree rooted at `heading_line`: scan
/// forward to the line before the next heading whose level is `<=` this
/// heading's level, or the end of the buffer. If `heading_line` is not a
/// heading (or out of range), returns `heading_line` unchanged.
pub fn subtree_end(lines: &[&str], heading_line: usize) -> usize {
    let Some(&line) = lines.get(heading_line) else {
        return heading_line;
    };
    let Some(level) = heading_level(line) else {
        return heading_line;
    };
    for (i, l) in lines.iter().enumerate().skip(heading_line + 1) {
        if let Some(lvl) = heading_level(l) {
            if lvl <= level {
                return i - 1;
            }
        }
    }
    lines.len().saturating_sub(1)
}

/// Cycle the TODO keyword of a heading line: none → `TODO` → `DONE` → none.
/// The keyword sits right after the stars (`** foo` → `** TODO foo` →
/// `** DONE foo` → `** foo`). The stars and the remaining heading text are
/// preserved. Non-heading lines are returned unchanged.
pub fn cycle_todo(line: &str) -> String {
    let Some(level) = heading_level(line) else {
        return line.to_string();
    };
    let stars = &line[..level]; // the run of `*`
    let rest = &line[level + 1..]; // text after the single space following the stars

    let new_rest = if let Some(body) = strip_keyword(rest, "TODO") {
        // TODO → DONE, keeping the body.
        if body.is_empty() {
            "DONE".to_string()
        } else {
            format!("DONE {body}")
        }
    } else if let Some(body) = strip_keyword(rest, "DONE") {
        // DONE → none.
        body.to_string()
    } else {
        // none → TODO.
        if rest.is_empty() {
            "TODO".to_string()
        } else {
            format!("TODO {rest}")
        }
    };

    format!("{stars} {new_rest}")
}

/// If `rest` begins with `kw` as a whole word (followed by a space or end of
/// string), return the remaining body after it; otherwise `None`.
fn strip_keyword<'a>(rest: &'a str, kw: &str) -> Option<&'a str> {
    if rest == kw {
        Some("")
    } else {
        rest.strip_prefix(kw)
            .and_then(|tail| tail.strip_prefix(' '))
    }
}

/// Promote a heading one level (remove one leading `*`), clamped so a level-1
/// heading stays level 1. The space after the stars is preserved. Non-heading
/// lines are returned unchanged.
pub fn promote(line: &str) -> String {
    match heading_level(line) {
        Some(level) if level > 1 => line[1..].to_string(),
        _ => line.to_string(),
    }
}

/// Demote a heading one level (add one leading `*`). The space after the stars
/// is preserved. Non-heading lines are returned unchanged.
pub fn demote(line: &str) -> String {
    if heading_level(line).is_some() {
        format!("*{line}")
    } else {
        line.to_string()
    }
}

// --- Slice 2: agenda parsing + priority cycling -----------------------------

/// The TODO keywords recognised on a heading (slice 2 keeps it to the two
/// built-ins; both are treated as agenda keywords, `DONE` rendered as completed).
const KEYWORDS: &[&str] = &["TODO", "DONE"];

/// One agenda entry: a TODO/DONE heading collected from an org file, with its
/// location, level, keyword, optional `[#A]` priority and any `SCHEDULED:` /
/// `DEADLINE:` dates pulled from the heading's body (each `YYYY-MM-DD`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgendaItem {
    pub file: PathBuf,
    /// 0-based line of the heading within its file.
    pub line: usize,
    pub level: usize,
    /// `TODO` / `DONE` (the matched keyword).
    pub keyword: String,
    /// `A` / `B` / `C` from a `[#A]` cookie, if present.
    pub priority: Option<char>,
    /// `YYYY-MM-DD` from a `SCHEDULED:` timestamp in the body.
    pub scheduled: Option<String>,
    /// `YYYY-MM-DD` from a `DEADLINE:` timestamp in the body.
    pub deadline: Option<String>,
    /// The heading text after the keyword + optional priority cookie.
    pub title: String,
}

/// Split a heading's text-after-the-stars into its `(keyword, priority, title)`
/// parts. The keyword (if present) must be the first whole word; an optional
/// `[#A]`/`[#B]`/`[#C]` cookie may follow; the rest is the title. Leading
/// whitespace is ignored. When no keyword matches, `keyword` is `None` (the
/// caller decides whether the heading is an agenda item).
fn parse_heading_rest(rest: &str) -> (Option<&'static str>, Option<char>, &str) {
    let trimmed = rest.trim_start();
    let mut keyword = None;
    let mut r = trimmed;
    for kw in KEYWORDS {
        if trimmed == *kw {
            return (Some(kw), None, "");
        }
        if let Some(tail) = trimmed.strip_prefix(kw).and_then(|t| t.strip_prefix(' ')) {
            keyword = Some(*kw);
            r = tail.trim_start();
            break;
        }
    }
    let mut priority = None;
    if let Some((c, tail)) = parse_priority_cookie(r) {
        priority = Some(c);
        r = tail.trim_start();
    }
    (keyword, priority, r)
}

/// If `s` begins with a `[#X]` priority cookie (X an ASCII letter), return the
/// upper-cased priority char and the remainder after the `]`.
fn parse_priority_cookie(s: &str) -> Option<(char, &str)> {
    let rest = s.strip_prefix("[#")?;
    let mut chars = rest.chars();
    let c = chars.next()?;
    if !c.is_ascii_alphabetic() {
        return None;
    }
    let body = chars.as_str().strip_prefix(']')?;
    Some((c.to_ascii_uppercase(), body))
}

/// Extract the first `YYYY-MM-DD` date out of an org timestamp string (e.g.
/// `<2026-07-01>` or `<2026-07-01 Wed>`), scanning for the date pattern anywhere
/// in `s`. Returns `None` when no such pattern is present.
pub fn org_date(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let is_digit = |b: u8| b.is_ascii_digit();
    for start in 0..bytes.len() {
        let w = bytes.get(start..start + 10)?;
        if is_digit(w[0])
            && is_digit(w[1])
            && is_digit(w[2])
            && is_digit(w[3])
            && w[4] == b'-'
            && is_digit(w[5])
            && is_digit(w[6])
            && w[7] == b'-'
            && is_digit(w[8])
            && is_digit(w[9])
        {
            return Some(s[start..start + 10].to_string());
        }
        if bytes.len() < start + 10 {
            break;
        }
    }
    None
}

/// Find `SCHEDULED:`/`DEADLINE:`-style date in `line`: locate `marker`, then read
/// the first org date after it. Returns `None` if the marker isn't present or no
/// date follows it.
fn body_date(line: &str, marker: &str) -> Option<String> {
    let idx = line.find(marker)?;
    org_date(&line[idx + marker.len()..])
}

/// Parse an org buffer's text into agenda items. A heading line (one with a
/// non-zero [`heading_level`]) whose first word after the stars is a TODO keyword
/// becomes an item; its optional `[#A]` priority cookie and title are split out,
/// and the heading's body (every following line up to the next heading) is
/// scanned for `SCHEDULED:` and `DEADLINE:` dates. Pure and unit-tested.
pub fn parse_agenda(path: &Path, text: &str) -> Vec<AgendaItem> {
    let lines: Vec<&str> = text.lines().collect();
    let mut items = Vec::new();
    for (i, &line) in lines.iter().enumerate() {
        let Some(level) = heading_level(line) else {
            continue;
        };
        // Text after the stars (and their following space).
        let rest = &line[level..];
        let (keyword, priority, title) = parse_heading_rest(rest);
        let Some(keyword) = keyword else {
            continue;
        };

        let mut scheduled = None;
        let mut deadline = None;
        for body in lines.iter().skip(i + 1) {
            if heading_level(body).is_some() {
                break;
            }
            if scheduled.is_none() {
                scheduled = body_date(body, "SCHEDULED:");
            }
            if deadline.is_none() {
                deadline = body_date(body, "DEADLINE:");
            }
        }

        items.push(AgendaItem {
            file: path.to_path_buf(),
            line: i,
            level,
            keyword: keyword.to_string(),
            priority,
            scheduled,
            deadline,
            title: title.to_string(),
        });
    }
    items
}

/// Cycle a heading's priority cookie: none → `[#A]` → `[#B]` → `[#C]` → none.
/// The cookie sits right after the keyword (or after the stars if there is no
/// keyword). The stars, keyword and title are preserved; spacing is normalised
/// to single spaces. Non-heading lines are returned unchanged.
pub fn priority_cycle(line: &str) -> String {
    let Some(level) = heading_level(line) else {
        return line.to_string();
    };
    let stars = &line[..level];
    let rest = &line[level..];
    let (keyword, priority, title) = parse_heading_rest(rest);

    let next = match priority {
        None => Some('A'),
        Some('A') => Some('B'),
        Some('B') => Some('C'),
        _ => None,
    };

    let mut parts: Vec<String> = Vec::new();
    if let Some(kw) = keyword {
        parts.push(kw.to_string());
    }
    if let Some(p) = next {
        parts.push(format!("[#{p}]"));
    }
    if !title.is_empty() {
        parts.push(title.to_string());
    }
    if parts.is_empty() {
        stars.to_string()
    } else {
        format!("{} {}", stars, parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_level_detects_stars_then_space() {
        assert_eq!(heading_level("not a heading"), None);
        assert_eq!(heading_level("* one"), Some(1));
        assert_eq!(heading_level("*** three"), Some(3));
        assert_eq!(heading_level("* "), Some(1)); // empty heading still a heading
        assert_eq!(heading_level("*notspace"), None); // stars not followed by space
        assert_eq!(heading_level("***"), None); // bare stars, no space
        assert_eq!(heading_level(" * indented"), None); // must start in column 0
        assert_eq!(heading_level(""), None);
    }

    #[test]
    fn subtree_end_stops_at_same_or_higher_level() {
        let lines = [
            "* a",      // 0
            "body",     // 1
            "** b",     // 2
            "body",     // 3
            "*** c",    // 4
            "body",     // 5
            "** d",     // 6
            "* e",      // 7
            "body",     // 8
        ];
        // subtree of "* a" runs until just before the next level-1 heading "* e".
        assert_eq!(subtree_end(&lines, 0), 6);
        // subtree of "** b" stops at the next same-level heading "** d".
        assert_eq!(subtree_end(&lines, 2), 5);
        // deepest heading takes its body up to the next shallower heading.
        assert_eq!(subtree_end(&lines, 4), 5);
        // last heading runs to end of buffer.
        assert_eq!(subtree_end(&lines, 7), 8);
        // non-heading line is its own end.
        assert_eq!(subtree_end(&lines, 1), 1);
    }

    #[test]
    fn cycle_todo_full_cycle_preserves_stars_and_text() {
        let none = "** foo bar";
        let todo = cycle_todo(none);
        assert_eq!(todo, "** TODO foo bar");
        let done = cycle_todo(&todo);
        assert_eq!(done, "** DONE foo bar");
        let back = cycle_todo(&done);
        assert_eq!(back, "** foo bar");
        // single-star heading cycles too.
        assert_eq!(cycle_todo("* x"), "* TODO x");
        // non-heading untouched.
        assert_eq!(cycle_todo("plain text"), "plain text");
    }

    #[test]
    fn promote_demote_clamp_levels() {
        assert_eq!(demote("* a"), "** a");
        assert_eq!(demote("** a"), "*** a");
        assert_eq!(promote("** a"), "* a");
        assert_eq!(promote("*** a"), "** a");
        // level-1 heading cannot be promoted further.
        assert_eq!(promote("* a"), "* a");
        // non-heading lines untouched by either.
        assert_eq!(promote("plain"), "plain");
        assert_eq!(demote("plain"), "plain");
    }

    #[test]
    fn org_date_with_and_without_weekday() {
        assert_eq!(org_date("<2026-07-01>"), Some("2026-07-01".to_string()));
        assert_eq!(
            org_date("<2026-07-01 Wed>"),
            Some("2026-07-01".to_string())
        );
        assert_eq!(org_date("[2026-12-31 Thu 09:00]"), Some("2026-12-31".to_string()));
        // no date pattern → None.
        assert_eq!(org_date("not a date"), None);
        assert_eq!(org_date("<2026/07/01>"), None);
        assert_eq!(org_date(""), None);
        assert_eq!(org_date("12-3"), None);
    }

    #[test]
    fn parse_agenda_todo_with_priority_and_scheduled() {
        let text = "\
* Project
** TODO [#A] Write the report
   SCHEDULED: <2026-07-01 Wed>
   some notes
";
        let items = parse_agenda(Path::new("/tmp/a.org"), text);
        assert_eq!(items.len(), 1);
        let it = &items[0];
        assert_eq!(it.line, 1);
        assert_eq!(it.level, 2);
        assert_eq!(it.keyword, "TODO");
        assert_eq!(it.priority, Some('A'));
        assert_eq!(it.title, "Write the report");
        assert_eq!(it.scheduled.as_deref(), Some("2026-07-01"));
        assert_eq!(it.deadline, None);
        assert_eq!(it.file, Path::new("/tmp/a.org"));
    }

    #[test]
    fn parse_agenda_plain_heading_is_not_an_item() {
        let items = parse_agenda(Path::new("x.org"), "* Just a heading\nbody\n");
        assert!(items.is_empty());
    }

    #[test]
    fn parse_agenda_done_is_kept_as_keyword() {
        let items = parse_agenda(Path::new("x.org"), "* DONE shipped it\n");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].keyword, "DONE");
        assert_eq!(items[0].priority, None);
        assert_eq!(items[0].title, "shipped it");
    }

    #[test]
    fn parse_agenda_deadline_on_body_line() {
        let text = "* TODO ship\n  DEADLINE: <2026-08-15>\n";
        let items = parse_agenda(Path::new("x.org"), text);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].deadline.as_deref(), Some("2026-08-15"));
        assert_eq!(items[0].scheduled, None);
    }

    #[test]
    fn parse_agenda_multiple_items_and_body_scoping() {
        let text = "\
* TODO first
  SCHEDULED: <2026-07-01>
* notes heading
  DEADLINE: <2026-09-09>
** TODO [#B] second
* DONE third
";
        let items = parse_agenda(Path::new("x.org"), text);
        // "notes heading" is not a keyword heading; its DEADLINE must NOT attach
        // to "first" (a heading line stops the body scan).
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].keyword, "TODO");
        assert_eq!(items[0].scheduled.as_deref(), Some("2026-07-01"));
        assert_eq!(items[0].deadline, None);
        assert_eq!(items[1].keyword, "TODO");
        assert_eq!(items[1].priority, Some('B'));
        assert_eq!(items[1].title, "second");
        assert_eq!(items[2].keyword, "DONE");
        assert_eq!(items[2].title, "third");
    }

    #[test]
    fn priority_cycle_full_cycle_preserves_keyword_and_title() {
        let none = "** TODO foo bar";
        let a = priority_cycle(none);
        assert_eq!(a, "** TODO [#A] foo bar");
        let b = priority_cycle(&a);
        assert_eq!(b, "** TODO [#B] foo bar");
        let c = priority_cycle(&b);
        assert_eq!(c, "** TODO [#C] foo bar");
        let back = priority_cycle(&c);
        assert_eq!(back, "** TODO foo bar");
    }

    #[test]
    fn priority_cycle_without_keyword_and_non_heading() {
        assert_eq!(priority_cycle("* foo"), "* [#A] foo");
        assert_eq!(priority_cycle("* [#C] foo"), "* foo");
        // non-heading untouched.
        assert_eq!(priority_cycle("plain text"), "plain text");
    }
}
