//! iCalendar (RFC 5545) export/import for diary entries — the zemacs port of
//! GNU Emacs `icalendar.el` (`icalendar-export-file`/`-region`,
//! `icalendar-import-file`/`-buffer`).
//!
//! This is the pure, tested core: it turns a slice of [`crate::diary::Entry`]
//! into a `VCALENDAR` string of `VEVENT`s and parses such a string back into
//! diary lines. All I/O (reading the diary file, writing the `.ics` file) is
//! done by the command layer. Recurring specs map to `RRULE`s; specs that have
//! no simple iCalendar equivalent (sexp float/wildcard/other-calendar) are
//! skipped and counted so the caller can report them.

use crate::calendar::{add_days, weekday, Date, MONTH_NAMES};
use crate::diary::{DateSpec, Entry};

/// Emacs `icalendar-uid-format` yields UIDs like `emacs<n>`; zemacs tags its own.
const PRODID: &str = "-//zemacs//NONSGML zemacs diary//EN";

/// The two-letter iCalendar weekday codes, indexed 0 = Sunday … 6 = Saturday
/// (matching [`crate::calendar::weekday`]).
const BYDAY: [&str; 7] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];

/// Escape a summary/description for an iCalendar `TEXT` value (RFC 5545 §3.3.11):
/// backslash, semicolon and comma are escaped, newlines become `\n`.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// Reverse [`escape_text`] for a parsed `TEXT` value.
fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `YYYYMMDD` for a `VALUE=DATE` property.
fn ical_date(d: Date) -> String {
    format!("{:04}{:02}{:02}", d.year, d.month, d.day)
}

/// Fold a content line to 75 octets per RFC 5545 §3.1: continuation lines start
/// with a single space. Folding counts bytes (UTF-8), never splitting a byte.
fn fold_line(line: &str, out: &mut String) {
    let bytes = line.as_bytes();
    if bytes.len() <= 75 {
        out.push_str(line);
        out.push_str("\r\n");
        return;
    }
    let mut start = 0;
    let mut first = true;
    while start < bytes.len() {
        // 75 octets on the first line, 74 on continuations (the leading space
        // counts toward the 75-octet limit). Do not split a UTF-8 code point.
        let budget = if first { 75 } else { 74 };
        let mut end = (start + budget).min(bytes.len());
        while end > start && (bytes[end - 1] & 0xC0) == 0x80 {
            end -= 1;
        }
        if !first {
            out.push(' ');
        }
        out.push_str(&line[start..end]);
        out.push_str("\r\n");
        start = end;
        first = false;
    }
}

/// The result of exporting one batch of entries.
pub struct Export {
    /// The `VCALENDAR` text (CRLF line endings, folded per RFC 5545).
    pub ics: String,
    /// How many entries were exported as `VEVENT`s.
    pub exported: usize,
    /// How many entries had no simple iCalendar mapping and were skipped.
    pub skipped: usize,
}

/// Map a [`DateSpec`] to an iCalendar `(DTSTART, DTEND, RRULE)` triple, anchoring
/// undated recurrences (weekly/yearly) at `anchor`. Returns `None` for specs
/// with no simple iCalendar equivalent.
fn spec_to_ical(spec: &DateSpec, anchor: Date) -> Option<(Date, Option<Date>, Option<String>)> {
    match *spec {
        DateSpec::Specific { year, month, day } => Some((Date::new(year, month, day), None, None)),
        DateSpec::Yearly { month, day } => Some((
            Date::new(anchor.year, month, day),
            None,
            Some("FREQ=YEARLY".to_string()),
        )),
        DateSpec::Anniversary { month, day, year } => Some((
            Date::new(year.unwrap_or(anchor.year), month, day),
            None,
            Some("FREQ=YEARLY".to_string()),
        )),
        DateSpec::Weekly { weekday: wd } => {
            // First occurrence on/after the anchor.
            let mut d = anchor;
            while weekday(d) != wd {
                d = add_days(d, 1);
            }
            Some((
                d,
                None,
                Some(format!("FREQ=WEEKLY;BYDAY={}", BYDAY[(wd % 7) as usize])),
            ))
        }
        DateSpec::Cyclic { n, base } => {
            Some((base, None, Some(format!("FREQ=DAILY;INTERVAL={n}"))))
        }
        DateSpec::Block { start, end } => {
            // A multi-day all-day event: DTEND is exclusive (day after `end`).
            Some((start, Some(add_days(end, 1)), None))
        }
        // Float, DateWild, CalendarDate, Offset, Remind, HebrewYahrzeit have no
        // simple VEVENT mapping.
        _ => None,
    }
}

/// Export `entries` to an iCalendar `VCALENDAR` string (Emacs
/// `icalendar-export-file`/`-region`). Undated recurrences are anchored at
/// `anchor` (the command layer passes today's date).
pub fn export_entries(entries: &[Entry], anchor: Date) -> Export {
    let mut body = String::new();
    let mut exported = 0usize;
    let mut skipped = 0usize;
    for (i, e) in entries.iter().enumerate() {
        let Some((dtstart, dtend, rrule)) = spec_to_ical(&e.spec, anchor) else {
            skipped += 1;
            continue;
        };
        exported += 1;
        fold_line("BEGIN:VEVENT", &mut body);
        fold_line(&format!("UID:zemacs-{}@diary", i + 1), &mut body);
        fold_line(&format!("SUMMARY:{}", escape_text(&e.text)), &mut body);
        fold_line(
            &format!("DTSTART;VALUE=DATE:{}", ical_date(dtstart)),
            &mut body,
        );
        if let Some(end) = dtend {
            fold_line(&format!("DTEND;VALUE=DATE:{}", ical_date(end)), &mut body);
        }
        if let Some(rule) = rrule {
            fold_line(&format!("RRULE:{rule}"), &mut body);
        }
        fold_line("END:VEVENT", &mut body);
    }
    let mut ics = String::new();
    fold_line("BEGIN:VCALENDAR", &mut ics);
    fold_line("VERSION:2.0", &mut ics);
    fold_line(&format!("PRODID:{PRODID}"), &mut ics);
    ics.push_str(&body);
    fold_line("END:VCALENDAR", &mut ics);
    Export {
        ics,
        exported,
        skipped,
    }
}

/// Unfold RFC 5545 content lines: a line beginning with a space or tab is a
/// continuation of the previous line.
fn unfold(ics: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in ics.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        lines.push(line.to_string());
    }
    lines
}

/// Parse a `YYYYMMDD` (optionally with a trailing `THHMMSS`) date value.
fn parse_ical_date(v: &str) -> Option<Date> {
    let d = v.split('T').next().unwrap_or(v);
    if d.len() < 8 {
        return None;
    }
    let year: i32 = d[0..4].parse().ok()?;
    let month: u32 = d[4..6].parse().ok()?;
    let day: u32 = d[6..8].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(Date::new(year, month, day))
}

/// Import an iCalendar string into diary lines (Emacs
/// `icalendar-import-file`/`-buffer`). Each `VEVENT` becomes one diary line:
/// a plain date `Monthname Day, Year Summary`, or, when the event carries a
/// yearly/weekly `RRULE`, the recurring diary form (`Monthname Day Summary` /
/// weekday name). Events without a `DTSTART` are skipped.
pub fn import_ical(ics: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_event = false;
    let mut summary = String::new();
    let mut dtstart: Option<Date> = None;
    let mut rrule = String::new();
    for line in unfold(ics) {
        let upper = line.to_ascii_uppercase();
        if upper == "BEGIN:VEVENT" {
            in_event = true;
            summary.clear();
            dtstart = None;
            rrule.clear();
            continue;
        }
        if upper == "END:VEVENT" {
            if let Some(d) = dtstart {
                out.push(diary_line(d, &summary, &rrule));
            }
            in_event = false;
            continue;
        }
        if !in_event {
            continue;
        }
        // Split off the property name (up to `:`), ignoring any `;`-params.
        let (name, value) = match line.split_once(':') {
            Some((n, v)) => (n, v),
            None => continue,
        };
        let prop = name.split(';').next().unwrap_or(name).to_ascii_uppercase();
        match prop.as_str() {
            "SUMMARY" => summary = unescape_text(value),
            "DTSTART" => dtstart = parse_ical_date(value),
            "RRULE" => rrule = value.to_ascii_uppercase(),
            _ => {}
        }
    }
    out
}

/// Build the diary line for an imported event.
fn diary_line(d: Date, summary: &str, rrule: &str) -> String {
    let month = MONTH_NAMES[(d.month - 1) as usize];
    let text = if summary.is_empty() { "" } else { summary };
    if rrule.contains("FREQ=YEARLY") {
        // Recurs every year on this month/day.
        format!("{} {} {}", month, d.day, text)
            .trim_end()
            .to_string()
    } else if rrule.contains("FREQ=WEEKLY") {
        // Recurs every week on this weekday.
        let wd = weekday(d) as usize;
        format!("{} {}", WEEKDAY_NAMES[wd], text)
            .trim_end()
            .to_string()
    } else {
        // A specific date.
        format!("{} {}, {} {}", month, d.day, d.year, text)
            .trim_end()
            .to_string()
    }
}

/// Full weekday names, indexed 0 = Sunday … 6 = Saturday.
const WEEKDAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diary::parse_file;

    #[test]
    fn escaping_round_trips() {
        let s = "Meet Bob; bring A, B\\C\nline2";
        assert_eq!(unescape_text(&escape_text(s)), s);
        assert_eq!(escape_text("a;b,c"), "a\\;b\\,c");
    }

    #[test]
    fn folds_long_lines_at_75_octets() {
        let mut out = String::new();
        let long = format!("SUMMARY:{}", "x".repeat(100));
        fold_line(&long, &mut out);
        for line in out.trim_end().split("\r\n") {
            assert!(line.len() <= 75, "line too long: {}", line.len());
        }
        // Unfolding restores the original content line (ignoring the trailing
        // empty line left by the final CRLF).
        let unfolded: Vec<String> = unfold(&out).into_iter().filter(|l| !l.is_empty()).collect();
        assert_eq!(unfolded, vec![long]);
    }

    #[test]
    fn exports_specific_and_recurring() {
        let entries =
            parse_file("October 12, 2024 Dentist\nOctober 31 Halloween\nMonday Standup\n");
        let anchor = Date::new(2024, 1, 1);
        let ex = export_entries(&entries, anchor);
        assert_eq!(ex.exported, 3);
        assert_eq!(ex.skipped, 0);
        assert!(ex.ics.contains("BEGIN:VCALENDAR"));
        assert!(ex.ics.contains("DTSTART;VALUE=DATE:20241012"));
        assert!(ex.ics.contains("SUMMARY:Dentist"));
        // Yearly recurrence for the undated Oct 31 entry.
        assert!(ex.ics.contains("RRULE:FREQ=YEARLY"));
        // Weekly recurrence anchored on the first Monday on/after the anchor.
        assert!(ex.ics.contains("RRULE:FREQ=WEEKLY;BYDAY=MO"));
    }

    #[test]
    fn import_reads_events() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:Dentist\r\n\
             DTSTART;VALUE=DATE:20241012\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\n\
             SUMMARY:Birthday\r\nDTSTART;VALUE=DATE:20240315\r\n\
             RRULE:FREQ=YEARLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let lines = import_ical(ics);
        assert_eq!(
            lines,
            vec![
                "October 12, 2024 Dentist".to_string(),
                "March 15 Birthday".to_string(),
            ]
        );
    }

    #[test]
    fn export_import_round_trip_specific() {
        let entries = parse_file("July 4, 2026 Fireworks\n");
        let ex = export_entries(&entries, Date::new(2026, 1, 1));
        let lines = import_ical(&ex.ics);
        assert_eq!(lines, vec!["July 4, 2026 Fireworks".to_string()]);
    }

    #[test]
    fn skips_unmappable_specs() {
        // A float sexp has no simple VEVENT mapping.
        let entries = parse_file("%%(diary-float 1 1 2) MLK-ish\n");
        let ex = export_entries(&entries, Date::new(2024, 1, 1));
        assert_eq!(ex.exported, 0);
        assert_eq!(ex.skipped, 1);
    }
}
