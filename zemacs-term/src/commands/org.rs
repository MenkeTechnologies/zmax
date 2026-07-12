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
//! Slice 3 adds dep-free **date math** ([`civil_from_days`] / [`ymd_from_unix`] /
//! [`today`]) used to bucket dated agenda items ([`date_bucket`] →
//! [`Bucket::Overdue`]/`Today`/`Upcoming`) and **capture** ([`capture_entry`] /
//! [`inbox_path`] / [`append_capture`], driving `:org-capture`).
//!
//! Deferred to later slices: recurring timestamps, refile/archive, babel and
//! export.

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

/// Move the subtree rooted at `heading_line` down past its next sibling (Emacs
/// `org-move-subtree-down`). Returns the new lines and the subtree's new heading
/// line, or `None` when `heading_line` is not a heading or has no next sibling.
pub fn move_subtree_down(lines: &[&str], heading_line: usize) -> Option<(Vec<String>, usize)> {
    let level = heading_level(lines.get(heading_line)?)?;
    let end = subtree_end(lines, heading_line);
    let next_start = end + 1;
    // The next heading must exist and be a sibling (same level); a shallower
    // heading means this is the last child of its parent.
    if heading_level(lines.get(next_start)?)? != level {
        return None;
    }
    let next_end = subtree_end(lines, next_start);
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    out.extend(lines[..heading_line].iter().map(|s| s.to_string()));
    out.extend(lines[next_start..=next_end].iter().map(|s| s.to_string()));
    out.extend(lines[heading_line..=end].iter().map(|s| s.to_string()));
    out.extend(lines[next_end + 1..].iter().map(|s| s.to_string()));
    let new_heading = heading_line + (next_end - next_start + 1);
    Some((out, new_heading))
}

/// Move the subtree rooted at `heading_line` up past its previous sibling (Emacs
/// `org-move-subtree-up`). Equivalent to moving the previous sibling down past
/// this one. Returns the new lines and this subtree's new heading line.
pub fn move_subtree_up(lines: &[&str], heading_line: usize) -> Option<(Vec<String>, usize)> {
    let level = heading_level(lines.get(heading_line)?)?;
    // Walk back to the previous sibling's heading (same level); a shallower
    // heading first means there is no previous sibling.
    let mut prev = None;
    for i in (0..heading_line).rev() {
        if let Some(lvl) = heading_level(lines[i]) {
            if lvl == level {
                prev = Some(i);
                break;
            }
            if lvl < level {
                return None;
            }
        }
    }
    let prev_start = prev?;
    // Moving `heading_line` up past `prev_start` == moving `prev_start` down; the
    // current subtree then begins where the previous sibling did.
    move_subtree_down(lines, prev_start).map(|(out, _)| (out, prev_start))
}

/// Parse the `KEYWORD: <timestamp>` pairs of an org planning line (`SCHEDULED`,
/// `DEADLINE`, `CLOSED`). Timestamps are `<...>` (active) or `[...]` (inactive).
fn parse_planning(line: &str) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for kw in ["CLOSED", "DEADLINE", "SCHEDULED"] {
        if let Some(pos) = line.find(&format!("{kw}:")) {
            let after = line[pos + kw.len() + 1..].trim_start();
            let close = if after.starts_with('<') {
                '>'
            } else if after.starts_with('[') {
                ']'
            } else {
                continue;
            };
            if let Some(end) = after.find(close) {
                out.push((kw, after[..=end].to_string()));
            }
        }
    }
    out
}

/// Add or update a planning timestamp (`SCHEDULED` / `DEADLINE` / `CLOSED`) on
/// the heading at `heading_line` (Emacs `org-schedule` / `org-deadline`). `date`
/// is wrapped in an active `<...>` timestamp unless already bracketed. The
/// planning line sits directly under the heading, keywords in canonical order
/// (CLOSED, DEADLINE, SCHEDULED). Returns the new lines, or `None` if
/// `heading_line` is not a heading or `keyword` is unknown.
pub fn set_planning(
    lines: &[&str],
    heading_line: usize,
    keyword: &str,
    date: &str,
) -> Option<Vec<String>> {
    heading_level(lines.get(heading_line)?)?;
    if !matches!(keyword, "SCHEDULED" | "DEADLINE" | "CLOSED") {
        return None;
    }
    let ts = if date.starts_with('<') || date.starts_with('[') {
        date.to_string()
    } else {
        format!("<{date}>")
    };
    let planning_idx = heading_line + 1;
    let existing = lines.get(planning_idx).copied().unwrap_or("");
    let is_planning = {
        let t = existing.trim_start();
        t.starts_with("SCHEDULED:") || t.starts_with("DEADLINE:") || t.starts_with("CLOSED:")
    };
    // Collect current keyword→timestamp, then set the requested one.
    let mut map: Vec<(&'static str, String)> =
        if is_planning { parse_planning(existing) } else { Vec::new() };
    let canonical = match keyword {
        "CLOSED" => "CLOSED",
        "DEADLINE" => "DEADLINE",
        _ => "SCHEDULED",
    };
    map.retain(|(k, _)| *k != canonical);
    map.push((canonical, ts));
    let planning_line = ["CLOSED", "DEADLINE", "SCHEDULED"]
        .iter()
        .filter_map(|k| map.iter().find(|(mk, _)| mk == k))
        .map(|(k, t)| format!("{k}: {t}"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    if is_planning {
        out[planning_idx] = planning_line;
    } else {
        out.insert(planning_idx.min(out.len()), planning_line);
    }
    Some(out)
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

// --- Slice 3: date math + capture --------------------------------------------

/// Where a dated agenda item falls relative to "today": before today
/// ([`Bucket::Overdue`]), exactly today ([`Bucket::Today`]) or after
/// ([`Bucket::Upcoming`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Overdue,
    Today,
    Upcoming,
}

/// Convert a count of days since the Unix epoch (1970-01-01 = day 0) into a
/// proleptic-Gregorian `(year, month, day)` via Howard Hinnant's branch-free
/// `civil_from_days` algorithm. Pure and total for any `i64` day count, so it is
/// unit-testable with fixed inputs.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01 so leap days land at the end of the era.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // day-of-era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day-of-year [0, 365]
    let mp = (5 * doy + 2) / 153; // month shifted so March = 0 [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d)
}

/// Format the date of a Unix timestamp (whole seconds since the epoch, UTC) as
/// `YYYY-MM-DD`. Uses [`civil_from_days`], so it is pure and unit-testable
/// (e.g. `0 -> "1970-01-01"`). Negative timestamps (pre-1970) work via floored
/// division.
pub fn ymd_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today's date as `YYYY-MM-DD`. Reads the wall clock via [`std::time::SystemTime`]
/// (no `chrono`/`time` dependency) and formats it with [`ymd_from_unix`]. If the
/// clock is somehow before the epoch it falls back to the epoch date.
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    ymd_from_unix(secs)
}

/// Classify a `YYYY-MM-DD` date against `today` (also `YYYY-MM-DD`). Because that
/// format sorts chronologically as plain strings, a lexical compare suffices.
pub fn date_bucket(date: &str, today: &str) -> Bucket {
    use std::cmp::Ordering;
    match date.cmp(today) {
        Ordering::Less => Bucket::Overdue,
        Ordering::Equal => Bucket::Today,
        Ordering::Greater => Bucket::Upcoming,
    }
}

/// Format a captured note as a top-level org TODO entry: `"* TODO {text}\n"`,
/// with surrounding whitespace trimmed off `text`. Pure and unit-tested.
pub fn capture_entry(text: &str) -> String {
    format!("* TODO {}\n", text.trim())
}

/// Resolve the inbox file a capture appends to. An explicit (non-blank) `arg`
/// path wins — absolute as given, relative resolved against `working_dir`;
/// otherwise the default is `<working_dir>/inbox.org`. Pure and unit-tested.
pub fn inbox_path(arg: Option<&str>, working_dir: &Path) -> PathBuf {
    match arg.map(str::trim).filter(|a| !a.is_empty()) {
        Some(a) => {
            let p = Path::new(a);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                working_dir.join(p)
            }
        }
        None => working_dir.join("inbox.org"),
    }
}

/// Append a captured TODO entry to `path`, creating any missing parent
/// directories and the file itself. Always appends (never clobbers existing
/// content). Returns the formatted entry on success.
pub fn append_capture(path: &Path, text: &str) -> std::io::Result<String> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let entry = capture_entry(text);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(entry.as_bytes())?;
    Ok(entry)
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
    fn set_planning_inserts_and_updates() {
        let lines = ["* TODO Task", "body"];
        // Insert a new planning line under the heading.
        let out = set_planning(&lines, 0, "SCHEDULED", "2026-07-15").unwrap();
        assert_eq!(out, vec!["* TODO Task", "SCHEDULED: <2026-07-15>", "body"]);
        // Add DEADLINE to the existing planning line (canonical order).
        let r1: Vec<&str> = out.iter().map(|s| s.as_str()).collect();
        let out2 = set_planning(&r1, 0, "DEADLINE", "2026-07-20").unwrap();
        assert_eq!(out2[1], "DEADLINE: <2026-07-20> SCHEDULED: <2026-07-15>");
        // Update the existing SCHEDULED, keeping DEADLINE.
        let r2: Vec<&str> = out2.iter().map(|s| s.as_str()).collect();
        let out3 = set_planning(&r2, 0, "SCHEDULED", "2026-08-01").unwrap();
        assert_eq!(out3[1], "DEADLINE: <2026-07-20> SCHEDULED: <2026-08-01>");
        // Already-bracketed date is used verbatim; non-heading yields None.
        let out4 = set_planning(&lines, 0, "DEADLINE", "[2026-01-01]").unwrap();
        assert_eq!(out4[1], "DEADLINE: [2026-01-01]");
        assert!(set_planning(&lines, 1, "SCHEDULED", "x").is_none());
    }

    #[test]
    fn move_subtree_down_and_up_swap_siblings() {
        let lines = ["* A", "a1", "* B", "b1", "* C"];
        // A moves down past B.
        let (out, h) = move_subtree_down(&lines, 0).unwrap();
        assert_eq!(out, vec!["* B", "b1", "* A", "a1", "* C"]);
        assert_eq!(h, 2);
        // B (index 2) moves up past A -> same result, B back at top.
        let (out2, h2) = move_subtree_up(&lines, 2).unwrap();
        assert_eq!(out2, vec!["* B", "b1", "* A", "a1", "* C"]);
        assert_eq!(h2, 0);
        // The last sibling cannot move down; the first cannot move up.
        assert!(move_subtree_down(&lines, 4).is_none());
        assert!(move_subtree_up(&lines, 0).is_none());
        // A non-heading line yields None.
        assert!(move_subtree_down(&lines, 1).is_none());
    }

    #[test]
    fn move_subtree_carries_nested_children() {
        let lines = ["* A", "** A1", "a1body", "* B", "b1"];
        // A (with child A1) moves down past B; the whole subtree travels.
        let (out, h) = move_subtree_down(&lines, 0).unwrap();
        assert_eq!(out, vec!["* B", "b1", "* A", "** A1", "a1body"]);
        assert_eq!(h, 2);
        // A shallower following heading means no sibling below: [** A1] under A,
        // then A is last top-level after the move — moving A1 down past nothing.
        let lines2 = ["* A", "** A1", "** A2"];
        let (out2, h2) = move_subtree_down(&lines2, 1).unwrap();
        assert_eq!(out2, vec!["* A", "** A2", "** A1"]);
        assert_eq!(h2, 2);
    }

    #[test]
    fn subtree_end_stops_at_same_or_higher_level() {
        let lines = [
            "* a",   // 0
            "body",  // 1
            "** b",  // 2
            "body",  // 3
            "*** c", // 4
            "body",  // 5
            "** d",  // 6
            "* e",   // 7
            "body",  // 8
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
        assert_eq!(org_date("<2026-07-01 Wed>"), Some("2026-07-01".to_string()));
        assert_eq!(
            org_date("[2026-12-31 Thu 09:00]"),
            Some("2026-12-31".to_string())
        );
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

    // --- Slice 3 ----------------------------------------------------------

    #[test]
    fn civil_from_days_known_points() {
        // Epoch.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // First day after the epoch and end of that year.
        assert_eq!(civil_from_days(1), (1970, 1, 2));
        assert_eq!(civil_from_days(364), (1970, 12, 31));
        assert_eq!(civil_from_days(365), (1971, 1, 1));
        // A leap day: 2000-02-29 is day 11016.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // Pre-epoch day rolls back into 1969.
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn ymd_from_unix_formats_dates() {
        assert_eq!(ymd_from_unix(0), "1970-01-01");
        // Mid-day of the epoch still formats as the same date.
        assert_eq!(ymd_from_unix(86_399), "1970-01-01");
        assert_eq!(ymd_from_unix(86_400), "1970-01-02");
        // 2021-01-01 00:00:00 UTC = 1_609_459_200.
        assert_eq!(ymd_from_unix(1_609_459_200), "2021-01-01");
        // 2026-06-29 00:00:00 UTC = 1_782_734_400.
        assert_eq!(ymd_from_unix(1_782_734_400), "2026-06-29");
    }

    #[test]
    fn today_is_well_formed() {
        let t = today();
        assert_eq!(t.len(), 10);
        let bytes = t.as_bytes();
        assert_eq!(bytes[4], b'-');
        assert_eq!(bytes[7], b'-');
        assert!(t
            .chars()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()));
    }

    #[test]
    fn date_bucket_by_string_compare() {
        assert_eq!(date_bucket("2026-06-28", "2026-06-29"), Bucket::Overdue);
        assert_eq!(date_bucket("2026-06-29", "2026-06-29"), Bucket::Today);
        assert_eq!(date_bucket("2026-06-30", "2026-06-29"), Bucket::Upcoming);
        // Year/month boundaries still sort chronologically as strings.
        assert_eq!(date_bucket("2025-12-31", "2026-01-01"), Bucket::Overdue);
        assert_eq!(date_bucket("2026-02-01", "2026-01-31"), Bucket::Upcoming);
    }

    #[test]
    fn capture_entry_trims_and_formats() {
        assert_eq!(capture_entry("buy milk"), "* TODO buy milk\n");
        assert_eq!(capture_entry("  padded  "), "* TODO padded\n");
        assert_eq!(capture_entry(""), "* TODO \n");
    }

    #[test]
    fn inbox_path_resolution() {
        let wd = Path::new("/work/dir");
        // Default.
        assert_eq!(inbox_path(None, wd), PathBuf::from("/work/dir/inbox.org"));
        assert_eq!(
            inbox_path(Some("   "), wd),
            PathBuf::from("/work/dir/inbox.org")
        );
        // Relative arg joins the working dir.
        assert_eq!(
            inbox_path(Some("notes/todo.org"), wd),
            PathBuf::from("/work/dir/notes/todo.org")
        );
        // Absolute arg is used verbatim.
        assert_eq!(
            inbox_path(Some("/abs/in.org"), wd),
            PathBuf::from("/abs/in.org")
        );
    }
}
