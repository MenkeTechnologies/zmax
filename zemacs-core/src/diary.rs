//! Diary — the zemacs port of the GNU Emacs diary (the dated-entries file the
//! Calendar reads, `diary-file`, default `~/diary`).
//!
//! A diary file is a list of entries, each introduced by a date specification at
//! the start of a line; the rest of that line is the entry text. This module is
//! the pure, dependency-free, tested core: it parses the common `diary-date-forms`
//! into a [`DateSpec`], decides whether a spec applies to a given [`Date`], and
//! formats the date/weekly headers the `insert-*-diary-entry` commands write.
//! It performs no I/O — the command layer reads/writes the file and drives the
//! Calendar. Date arithmetic reuses [`crate::calendar`].

use crate::calendar::{weekday, Date, MONTH_NAMES};

/// A parsed diary date specification (the faithful default `diary-date-forms`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateSpec {
    /// A specific calendar date: `October 12, 2024` or `10/12/2024`.
    Specific { year: i32, month: u32, day: u32 },
    /// Every year on this month/day: `October 12` or `10/12`.
    Yearly { month: u32, day: u32 },
    /// Every week on this weekday (0 = Sunday): `Monday`.
    Weekly { weekday: u32 },
}

/// One diary entry: the date spec plus its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub spec: DateSpec,
    pub text: String,
}

impl DateSpec {
    /// Does this spec apply on `date`?
    pub fn matches(&self, date: Date) -> bool {
        match *self {
            DateSpec::Specific { year, month, day } => {
                date.year == year && date.month == month && date.day == day
            }
            DateSpec::Yearly { month, day } => date.month == month && date.day == day,
            DateSpec::Weekly { weekday: wd } => weekday(date) == wd,
        }
    }
}

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Full month name -> 1-based month number (case-insensitive, 3+ char prefix).
fn parse_month_name(word: &str) -> Option<u32> {
    let w = word.trim().to_ascii_lowercase();
    if w.len() < 3 {
        return None;
    }
    MONTH_NAMES
        .iter()
        .position(|m| {
            m.to_ascii_lowercase().starts_with(&w) || w.starts_with(&m.to_ascii_lowercase())
        })
        .map(|i| i as u32 + 1)
}

fn parse_weekday_name(word: &str) -> Option<u32> {
    let w = word.trim().to_ascii_lowercase();
    if w.len() < 3 {
        return None;
    }
    WEEKDAYS
        .iter()
        .position(|d| d.to_ascii_lowercase().starts_with(&w))
        .map(|i| i as u32)
}

/// Parse the leading date spec of a diary line, returning the spec and the
/// remaining entry text. Recognises (default American `diary-date-forms`):
///   `Monthname Day[, Year]`  ·  `M/D[/Year]`  ·  `Weekdayname`
pub fn parse_line(line: &str) -> Option<(DateSpec, String)> {
    let line = line.trim_start();
    if line.is_empty() {
        return None;
    }
    let mut it = line.splitn(2, char::is_whitespace);
    let first = it.next()?;
    let rest = it.next().unwrap_or("").trim_start();

    // Weekday name: `Monday ...`
    if let Some(wd) = parse_weekday_name(first) {
        return Some((DateSpec::Weekly { weekday: wd }, rest.to_string()));
    }

    // Numeric `M/D` or `M/D/Y`.
    if first.contains('/') {
        let parts: Vec<&str> = first.split('/').collect();
        if parts.len() == 2 || parts.len() == 3 {
            let month: u32 = parts[0].parse().ok()?;
            let day: u32 = parts[1].parse().ok()?;
            if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
                return None;
            }
            let spec = if parts.len() == 3 {
                DateSpec::Specific {
                    year: parts[2].parse().ok()?,
                    month,
                    day,
                }
            } else {
                DateSpec::Yearly { month, day }
            };
            return Some((spec, rest.to_string()));
        }
        return None;
    }

    // `Monthname Day[, Year] ...`
    if let Some(month) = parse_month_name(first) {
        // The next token is the day (possibly with a trailing comma).
        let mut rest_it = rest.splitn(2, char::is_whitespace);
        let day_tok = rest_it.next()?;
        let after_day = rest_it.next().unwrap_or("").trim_start();
        let had_comma = day_tok.ends_with(',');
        let day: u32 = day_tok.trim_end_matches(',').parse().ok()?;
        if !(1..=31).contains(&day) {
            return None;
        }
        // If a comma followed the day, the next token may be a year.
        if had_comma {
            let mut ay = after_day.splitn(2, char::is_whitespace);
            let ytok = ay.next().unwrap_or("");
            if let Ok(year) = ytok.parse::<i32>() {
                let text = ay.next().unwrap_or("").trim_start();
                return Some((DateSpec::Specific { year, month, day }, text.to_string()));
            }
        }
        return Some((DateSpec::Yearly { month, day }, after_day.to_string()));
    }

    None
}

/// Parse a whole diary file into entries, skipping lines that do not begin with
/// a recognised date spec (comments/blank lines).
pub fn parse_file(contents: &str) -> Vec<Entry> {
    contents
        .lines()
        .filter_map(parse_line)
        .map(|(spec, text)| Entry { spec, text })
        .collect()
}

/// The entries that apply on `date` (Emacs `diary-list-entries`).
pub fn entries_for<'a>(entries: &'a [Entry], date: Date) -> Vec<&'a Entry> {
    entries.iter().filter(|e| e.spec.matches(date)).collect()
}

/// Whether any entry applies on `date` (used to mark Calendar dates).
pub fn has_entry(entries: &[Entry], date: Date) -> bool {
    entries.iter().any(|e| e.spec.matches(date))
}

/// The header `insert-diary-entry` writes for a specific date:
/// `Monthname Day, Year `.
pub fn format_daily(date: Date) -> String {
    format!(
        "{} {}, {} ",
        MONTH_NAMES[(date.month - 1) as usize],
        date.day,
        date.year
    )
}

/// The header `insert-weekly-diary-entry` writes: the weekday name of `date`.
pub fn format_weekly(date: Date) -> String {
    format!("{} ", WEEKDAYS[weekday(date) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_monthname_forms() {
        assert_eq!(
            parse_line("October 12, 2024 Dentist"),
            Some((
                DateSpec::Specific {
                    year: 2024,
                    month: 10,
                    day: 12
                },
                "Dentist".to_string()
            ))
        );
        assert_eq!(
            parse_line("October 12 Payday"),
            Some((
                DateSpec::Yearly { month: 10, day: 12 },
                "Payday".to_string()
            ))
        );
        // 3-letter month abbreviation.
        assert_eq!(
            parse_line("Dec 25 Christmas"),
            Some((
                DateSpec::Yearly { month: 12, day: 25 },
                "Christmas".to_string()
            ))
        );
    }

    #[test]
    fn parses_numeric_and_weekday_forms() {
        assert_eq!(
            parse_line("10/31 Halloween"),
            Some((
                DateSpec::Yearly { month: 10, day: 31 },
                "Halloween".to_string()
            ))
        );
        assert_eq!(
            parse_line("12/25/2024 Xmas"),
            Some((
                DateSpec::Specific {
                    year: 2024,
                    month: 12,
                    day: 25
                },
                "Xmas".to_string()
            ))
        );
        assert_eq!(
            parse_line("Monday Standup"),
            Some((DateSpec::Weekly { weekday: 1 }, "Standup".to_string()))
        );
    }

    #[test]
    fn rejects_non_entries() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("just some prose"), None);
        assert_eq!(parse_line("13/40 bad date"), None);
        assert_eq!(parse_line("# a comment"), None);
    }

    #[test]
    fn matching_by_kind() {
        // 2024-12-25 is a Wednesday.
        let d = Date::new(2024, 12, 25);
        assert!(DateSpec::Specific {
            year: 2024,
            month: 12,
            day: 25
        }
        .matches(d));
        assert!(!DateSpec::Specific {
            year: 2023,
            month: 12,
            day: 25
        }
        .matches(d));
        assert!(DateSpec::Yearly { month: 12, day: 25 }.matches(d));
        assert!(DateSpec::Yearly { month: 12, day: 25 }.matches(Date::new(2030, 12, 25)));
        assert_eq!(weekday(d), 3); // Wednesday
        assert!(DateSpec::Weekly { weekday: 3 }.matches(d));
        assert!(!DateSpec::Weekly { weekday: 1 }.matches(d));
    }

    #[test]
    fn entries_for_a_date() {
        let file = "December 25 Christmas\n\
                    12/25/2024 Family dinner\n\
                    Wednesday Trash day\n\
                    January 1 New Year\n";
        let entries = parse_file(file);
        assert_eq!(entries.len(), 4);
        let hits = entries_for(&entries, Date::new(2024, 12, 25)); // Wed
        let texts: Vec<&str> = hits.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["Christmas", "Family dinner", "Trash day"]);
        assert!(has_entry(&entries, Date::new(2025, 1, 1)));
        assert!(!has_entry(&entries, Date::new(2024, 7, 4)));
    }

    #[test]
    fn insert_headers() {
        assert_eq!(format_daily(Date::new(2024, 12, 25)), "December 25, 2024 ");
        // 2024-12-25 is a Wednesday.
        assert_eq!(format_weekly(Date::new(2024, 12, 25)), "Wednesday ");
    }
}
