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

use crate::calendar::{
    days_in_month, from_serial, is_leap, to_serial, weekday, Date, MONTH_NAMES,
};

/// A parsed diary date specification (the faithful default `diary-date-forms`
/// plus the `%%(diary-...)` sexp entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateSpec {
    /// A specific calendar date: `October 12, 2024` or `10/12/2024`.
    Specific { year: i32, month: u32, day: u32 },
    /// Every year on this month/day: `October 12` or `10/12`.
    Yearly { month: u32, day: u32 },
    /// Every week on this weekday (0 = Sunday): `Monday`.
    Weekly { weekday: u32 },
    /// `%%(diary-anniversary MONTH DAY [YEAR])`.
    Anniversary {
        month: u32,
        day: u32,
        year: Option<i32>,
    },
    /// `%%(diary-block M1 D1 Y1 M2 D2 Y2)`.
    Block { start: Date, end: Date },
    /// `%%(diary-cyclic N MONTH DAY YEAR)`.
    Cyclic { n: i64, base: Date },
    /// `%%(diary-float MONTH DAYNAME N [DAY])`. `month = None` means any month.
    Float {
        month: Option<u32>,
        dayname: u32,
        n: i32,
        day: Option<u32>,
    },
    /// `%%(diary-date MONTH DAY YEAR)` with `t`/`*` wildcards (`None`).
    DateWild {
        month: Option<u32>,
        day: Option<u32>,
        year: Option<i32>,
    },
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
            DateSpec::Anniversary { month, day, year } => {
                anniversary(month, day, year, date).is_some()
            }
            DateSpec::Block { start, end } => block(start, end, date),
            DateSpec::Cyclic { n, base } => cyclic(n, base, date).is_some(),
            DateSpec::Float {
                month,
                dayname,
                n,
                day,
            } => {
                let ms = month.map(|m| [m]);
                float_match(ms.as_ref().map(|a| a.as_slice()), dayname, n, day, date)
            }
            DateSpec::DateWild { month, day, year } => date_wildcard(month, day, year, date),
        }
    }
}

/// Parse a signed integer, or `None` for the Emacs wildcard tokens `t`/`*`
/// (`nil` is treated as wildcard too).
fn parse_arg_opt(tok: &str) -> Option<Option<i64>> {
    match tok {
        "t" | "*" | "nil" => Some(None),
        _ => tok.parse::<i64>().ok().map(Some),
    }
}

/// Parse a `%%(diary-FUNC ARG...)` sexp entry into a [`DateSpec`]. Recognises
/// `diary-anniversary`, `diary-block`, `diary-cyclic`, `diary-float` and
/// `diary-date`. Returns the spec and the remaining entry text.
pub fn parse_sexp(line: &str) -> Option<(DateSpec, String)> {
    let line = line.trim_start();
    let inner_start = line.find("%%(")? + 3;
    // Find the matching close paren for the opening one.
    let bytes = line.as_bytes();
    let mut depth = 1i32;
    let mut i = inner_start;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if depth != 0 {
        return None;
    }
    let sexp = &line[inner_start..i];
    let text = line[i + 1..].trim_start().to_string();
    let mut toks = sexp.split_whitespace();
    let func = toks.next()?;
    let args: Vec<&str> = toks.collect();
    let num = |k: usize| -> Option<i64> { args.get(k).and_then(|t| t.parse::<i64>().ok()) };
    let spec = match func {
        "diary-anniversary" => DateSpec::Anniversary {
            month: num(0)? as u32,
            day: num(1)? as u32,
            year: args.get(2).and_then(|t| t.parse::<i32>().ok()),
        },
        "diary-block" => DateSpec::Block {
            start: Date::new(num(2)? as i32, num(0)? as u32, num(1)? as u32),
            end: Date::new(num(5)? as i32, num(3)? as u32, num(4)? as u32),
        },
        "diary-cyclic" => DateSpec::Cyclic {
            n: num(0)?,
            base: Date::new(num(3)? as i32, num(1)? as u32, num(2)? as u32),
        },
        "diary-float" => DateSpec::Float {
            month: parse_arg_opt(args.first()?)?.map(|v| v as u32),
            dayname: num(1)? as u32,
            n: num(2)? as i32,
            day: args.get(3).and_then(|t| t.parse::<u32>().ok()),
        },
        "diary-date" => DateSpec::DateWild {
            month: parse_arg_opt(args.first()?)?.map(|v| v as u32),
            day: parse_arg_opt(args.get(1)?)?.map(|v| v as u32),
            year: parse_arg_opt(args.get(2)?)?.map(|v| v as i32),
        },
        _ => return None,
    };
    Some((spec, text))
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
    // `%%(diary-...)` sexp entries.
    if line.starts_with("%%(") {
        return parse_sexp(line);
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
pub fn entries_for(entries: &[Entry], date: Date) -> Vec<&Entry> {
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

// ===========================================================================
// Sexp diary entries — the pure date predicates behind Emacs's "sexp" diary
// entries (`%%(diary-anniversary ...)` etc.). Each is a total, dependency-free
// function on Gregorian [`Date`]s, transcribed from GNU Emacs 30's
// `diary-lib.el` / `cal-hebrew.el`. The command layer evaluates them against a
// target date and formats matches; the insert-* commands write the sexp lines.
// ===========================================================================

/// The English ordinal suffix for `n` (`"st"`/`"nd"`/`"rd"`/`"th"`), matching
/// Emacs `diary-ordinal-suffix` (11/12/13 → "th").
pub fn ordinal_suffix(n: i64) -> &'static str {
    let n = n.abs();
    if (11..=13).contains(&(n % 100)) {
        "th"
    } else {
        match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    }
}

/// `diary-anniversary MONTH DAY [YEAR]`: does the anniversary fall on `on`?
/// Returns the number of elapsed years (Emacs `diff`) so the caller can format
/// "Nth anniversary". Matches when the month/day agree and `diff > 0`; with no
/// base `year`, Emacs uses a nominal `diff = 100` so it always applies.
pub fn anniversary(month: u32, day: u32, year: Option<i32>, on: Date) -> Option<i64> {
    if on.month != month || on.day != day {
        return None;
    }
    let diff = match year {
        Some(y) => on.year as i64 - y as i64,
        None => 100,
    };
    if diff > 0 {
        Some(diff)
    } else {
        None
    }
}

/// `diary-block M1 D1 Y1 M2 D2 Y2`: is `on` within the inclusive date range?
pub fn block(start: Date, end: Date, on: Date) -> bool {
    start <= on && on <= end
}

/// `diary-cyclic N MONTH DAY YEAR`: an entry every `n` days from the base date.
/// Returns the 1-based occurrence number when `on` is a repetition (Emacs's
/// `(1+ (/ diff n))`), or `None`.
pub fn cyclic(n: i64, base: Date, on: Date) -> Option<i64> {
    if n <= 0 {
        return None;
    }
    let diff = to_serial(on) - to_serial(base);
    if diff >= 0 && diff % n == 0 {
        Some(1 + diff / n)
    } else {
        None
    }
}

/// The absolute (serial) day of the `dayname` weekday on or before `abs`
/// (Emacs `calendar-dayname-on-or-before`). `dayname`: 0 = Sunday .. 6 = Sat.
fn dayname_on_or_before(dayname: u32, abs: i64) -> i64 {
    let wd = weekday(from_serial(abs)) as i64;
    abs - (wd - dayname as i64).rem_euclid(7)
}

/// Serial day of the `n`th `dayname` of (`month`,`year`), optionally offset from
/// `day` (Emacs `calendar-nth-named-absday`). `n > 0` counts from the start of
/// the month, `n < 0` from the end.
fn nth_named_day_serial(n: i32, dayname: u32, month: u32, year: i32, day: Option<u32>) -> i64 {
    if n > 0 {
        let base = to_serial(Date::new(year, month, day.unwrap_or(1)));
        dayname_on_or_before(dayname, base + 6) + 7 * (n as i64 - 1)
    } else {
        let d = day.unwrap_or_else(|| days_in_month(year, month));
        let base = to_serial(Date::new(year, month, d));
        dayname_on_or_before(dayname, base) - 7 * ((-n) as i64 - 1)
    }
}

/// `diary-float MONTH DAYNAME N [DAY]`: the Nth (from start if `n>0`, from end
/// if `n<0`) `dayname` weekday of a month. `months = None` means "any month"
/// (Emacs `t`); otherwise `on`'s month must be listed. `dayname`: 0 = Sunday.
pub fn float_match(months: Option<&[u32]>, dayname: u32, n: i32, day: Option<u32>, on: Date) -> bool {
    if n == 0 {
        return false;
    }
    if let Some(ms) = months {
        if !ms.contains(&on.month) {
            return false;
        }
    }
    to_serial(on) == nth_named_day_serial(n, dayname, on.month, on.year, day)
}

/// `diary-date MONTH DAY YEAR` with wildcards: each of month/day/year may be a
/// specific value or `None` (Emacs `t`, "any"). Matches when every specified
/// component agrees with `on`.
pub fn date_wildcard(month: Option<u32>, day: Option<u32>, year: Option<i32>, on: Date) -> bool {
    month.map_or(true, |m| m == on.month)
        && day.map_or(true, |d| d == on.day)
        && year.map_or(true, |y| y == on.year)
}

/// The day-of-year string Emacs `diary-day-of-year` / `calendar-day-of-year-string`
/// produces, e.g. `"Day 141 of 366; 225 days remaining"`.
pub fn day_of_year_string(on: Date) -> String {
    let day = crate::calendar::day_of_year(on) as i64;
    let total: i64 = if is_leap(on.year) { 366 } else { 365 };
    let remaining = total - day;
    format!(
        "Day {} of {}; {} day{} remaining",
        day,
        total,
        remaining,
        if remaining == 1 { "" } else { "s" }
    )
}

// --- insert-entry line builders (the sexp/date forms the insert-* commands
//     append to the diary file; American `calendar-date-style`) ----------------

/// `diary-insert-monthly-entry` header (American form `"* DAY "`).
pub fn format_monthly(day: u32) -> String {
    format!("* {} ", day)
}

/// `diary-insert-yearly-entry` header (American form `"Monthname Day "`).
pub fn format_yearly(date: Date) -> String {
    format!("{} {} ", MONTH_NAMES[(date.month - 1) as usize], date.day)
}

/// `diary-insert-anniversary-entry`: `"%%(diary-anniversary M D Y) "`.
pub fn format_anniversary_sexp(date: Date) -> String {
    format!(
        "%%(diary-anniversary {} {} {}) ",
        date.month, date.day, date.year
    )
}

/// `diary-insert-block-entry`: `"%%(diary-block M1 D1 Y1 M2 D2 Y2) "`.
pub fn format_block_sexp(start: Date, end: Date) -> String {
    format!(
        "%%(diary-block {} {} {} {} {} {}) ",
        start.month, start.day, start.year, end.month, end.day, end.year
    )
}

/// `diary-insert-cyclic-entry`: `"%%(diary-cyclic N M D Y) "`.
pub fn format_cyclic_sexp(n: i64, date: Date) -> String {
    format!(
        "%%(diary-cyclic {} {} {} {}) ",
        n, date.month, date.day, date.year
    )
}

/// A non-Gregorian insert header: the `prefix` letter (`H`/`I`/`B`) followed by
/// the given calendar's date string (Emacs `diary-*-insert-entry`).
pub fn format_other_entry(prefix: char, date_string: &str) -> String {
    format!("{}{} ", prefix, date_string)
}

/// A non-Gregorian *yearly* header: `"H Monthname Day "` (recurs on that
/// calendar month/day every year; Emacs `diary-*-insert-yearly-entry`).
pub fn format_other_yearly(prefix: char, month_name: &str, day: u32) -> String {
    format!("{}{} {} ", prefix, month_name, day)
}

/// A non-Gregorian *monthly* header: `"H* Day "` (Emacs
/// `diary-*-insert-monthly-entry`, American `("* " day)` form).
pub fn format_other_monthly(prefix: char, day: u32) -> String {
    format!("{}* {} ", prefix, day)
}

/// A non-Gregorian anniversary sexp: `"%%(diary-KIND-anniversary M D Y) "`
/// (Emacs `diary-*-insert-anniversary-entry`).
pub fn format_other_anniversary_sexp(kind: &str, month: u32, day: u32, year: i64) -> String {
    format!("%%(diary-{}-anniversary {} {} {}) ", kind, month, day, year)
}

// --- Hebrew-calendar diary predicates (cal-hebrew.el) -----------------------

/// `diary-hebrew-omer`: the Sefirat HaOmer count for `on`, or `None` outside the
/// 49-day count. Returns `(omer 1..=49, week, day-in-week)`; omer is measured
/// from 15 Nisan (Passover) of the Hebrew year containing `on`.
pub fn hebrew_omer(on: Date) -> Option<(i64, i64, i64)> {
    let abs = crate::calendar::rd(on);
    let hy = crate::calendar::hebrew_from_fixed(abs).0;
    let passover = crate::calendar::fixed_from_hebrew(hy, 1, 15);
    let omer = abs - passover;
    if omer > 0 && omer < 50 {
        Some((omer, omer / 7, omer % 7))
    } else {
        None
    }
}

/// The formatted Emacs `diary-hebrew-omer` string for an omer count.
pub fn hebrew_omer_string(omer: i64, week: i64, day: i64) -> String {
    let which = if week == 0 {
        format!("{} day{}", omer, if omer == 1 { "" } else { "s" })
    } else if day == 0 {
        format!("{} week{}", week, if week == 1 { "" } else { "s" })
    } else {
        format!(
            "{} week{} and {} day{}",
            week,
            if week == 1 { "" } else { "s" },
            day,
            if day == 1 { "" } else { "s" }
        )
    };
    format!(
        "Day {}{} of the omer (which is {})",
        omer,
        ordinal_suffix(omer),
        which
    )
}

/// `diary-hebrew-rosh-hodesh`: is `on` a day of Rosh Hodesh (the New Moon
/// festival)? Rosh Hodesh is the 1st of a Hebrew month, and also the 30th of the
/// preceding month when that month is 30 days long. Returns the name of the new
/// month, or `None`. (Emacs additionally reports Shabbat Mevarchim / the coming
/// month's molad; that requires the parashah tables, so is not included.)
pub fn hebrew_rosh_hodesh(on: Date) -> Option<String> {
    let abs = crate::calendar::rd(on);
    let (hy, hm, hd) = crate::calendar::hebrew_from_fixed(abs);
    let month_name = |year: i64, m: u32| -> &'static str {
        let idx = (m - 1) as usize;
        if crate::calendar::hebrew_last_month_of_year(year) == 12 {
            crate::calendar::HEBREW_MONTH_NAMES_COMMON[idx]
        } else {
            crate::calendar::HEBREW_MONTH_NAMES_LEAP[idx]
        }
    };
    if hd == 1 {
        Some(month_name(hy, hm).to_string())
    } else if hd == 30 {
        // 30th of a month → also Rosh Hodesh for the following month.
        let (ny, nm, _) = crate::calendar::hebrew_from_fixed(abs + 1);
        Some(month_name(ny, nm).to_string())
    } else {
        None
    }
}

/// `diary-hebrew-birthday MONTH DAY YEAR`: does the Hebrew birthday recur on
/// `on`? Returns the number of Hebrew years elapsed. This is the simple
/// month/day recurrence in the current Hebrew year; Emacs additionally special-
/// cases births on 30 Heshvan/Kislev and in Adar of a leap year, which need the
/// month-length tables and are not handled here.
pub fn hebrew_birthday(bmonth: u32, bday: u32, byear: i64, on: Date) -> Option<i64> {
    let abs = crate::calendar::rd(on);
    let cur_hy = crate::calendar::hebrew_from_fixed(abs).0;
    let birthday = crate::calendar::fixed_from_hebrew(cur_hy, bmonth, bday);
    let diff = cur_hy - byear;
    if birthday == abs && diff > 0 {
        Some(diff)
    } else {
        None
    }
}

// ===========================================================================
// appt — appointment reminders (appt.el). A sorted in-memory list of
// (minutes-since-midnight, message) appointments. The pure model handles time
// parsing and add/delete; the command layer holds the live list and (would)
// check it against the clock. zemacs has no idle timer, so the timed pop-up
// reminder is not delivered — only the list management is faithful.
// ===========================================================================

/// Parse an appt time string into minutes since midnight. Accepts `"HH:MM"`
/// (24-hour) and 12-hour `"H:MMam"`/`"H:MMpm"` (Emacs `appt-convert-time`).
pub fn parse_appt_time(s: &str) -> Option<u32> {
    let s = s.trim().to_ascii_lowercase();
    let (body, pm, has_ampm) = if let Some(b) = s.strip_suffix("pm") {
        (b.trim(), true, true)
    } else if let Some(b) = s.strip_suffix("am") {
        (b.trim(), false, true)
    } else {
        (s.as_str(), false, false)
    };
    let (h, m) = body.split_once(':')?;
    let mut hour: u32 = h.trim().parse().ok()?;
    let min: u32 = m.trim().parse().ok()?;
    if min > 59 {
        return None;
    }
    if has_ampm {
        if !(1..=12).contains(&hour) {
            return None;
        }
        if hour == 12 {
            hour = 0;
        }
        if pm {
            hour += 12;
        }
    } else if hour > 23 {
        return None;
    }
    Some(hour * 60 + min)
}

/// Format minutes-since-midnight back to `"HH:MM"`.
pub fn format_appt_time(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// One appointment: time (minutes since midnight) and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Appt {
    pub minutes: u32,
    pub message: String,
}

/// Insert `appt` keeping the list sorted by time (Emacs `appt-add`). Duplicates
/// (same time and message) are ignored, matching `appt.el`.
pub fn appt_add(list: &mut Vec<Appt>, appt: Appt) -> bool {
    if list.iter().any(|a| *a == appt) {
        return false;
    }
    let pos = list.partition_point(|a| a.minutes <= appt.minutes);
    list.insert(pos, appt);
    true
}

/// Delete every appointment whose message contains `needle` (Emacs
/// `appt-delete` prompts per entry; here we match by substring). Returns the
/// number removed.
pub fn appt_delete(list: &mut Vec<Appt>, needle: &str) -> usize {
    let before = list.len();
    list.retain(|a| !a.message.contains(needle));
    before - list.len()
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

    #[test]
    fn ordinal_suffixes() {
        assert_eq!(ordinal_suffix(1), "st");
        assert_eq!(ordinal_suffix(2), "nd");
        assert_eq!(ordinal_suffix(3), "rd");
        assert_eq!(ordinal_suffix(4), "th");
        assert_eq!(ordinal_suffix(11), "th");
        assert_eq!(ordinal_suffix(12), "th");
        assert_eq!(ordinal_suffix(13), "th");
        assert_eq!(ordinal_suffix(21), "st");
        assert_eq!(ordinal_suffix(112), "th");
    }

    #[test]
    fn anniversary_predicate() {
        // Born 2000-06-15; on 2024-06-15 that is the 24th anniversary.
        assert_eq!(
            anniversary(6, 15, Some(2000), Date::new(2024, 6, 15)),
            Some(24)
        );
        // Wrong day / month → no match.
        assert_eq!(anniversary(6, 15, Some(2000), Date::new(2024, 6, 16)), None);
        // Same year as base or earlier → diff not > 0.
        assert_eq!(anniversary(6, 15, Some(2000), Date::new(2000, 6, 15)), None);
        assert_eq!(anniversary(6, 15, Some(2000), Date::new(1999, 6, 15)), None);
        // No base year → always applies on that month/day (Emacs diff = 100).
        assert_eq!(anniversary(6, 15, None, Date::new(2024, 6, 15)), Some(100));
    }

    #[test]
    fn block_range() {
        let a = Date::new(2024, 6, 1);
        let b = Date::new(2024, 6, 10);
        assert!(block(a, b, Date::new(2024, 6, 1))); // inclusive start
        assert!(block(a, b, Date::new(2024, 6, 10))); // inclusive end
        assert!(block(a, b, Date::new(2024, 6, 5)));
        assert!(!block(a, b, Date::new(2024, 5, 31)));
        assert!(!block(a, b, Date::new(2024, 6, 11)));
    }

    #[test]
    fn cyclic_every_n_days() {
        let base = Date::new(2024, 1, 1);
        assert_eq!(cyclic(3, base, Date::new(2024, 1, 1)), Some(1)); // occurrence 1
        assert_eq!(cyclic(3, base, Date::new(2024, 1, 4)), Some(2)); // +3 days
        assert_eq!(cyclic(3, base, Date::new(2024, 1, 7)), Some(3)); // +6 days
        assert_eq!(cyclic(3, base, Date::new(2024, 1, 2)), None); // not a multiple
        assert_eq!(cyclic(3, base, Date::new(2023, 12, 31)), None); // before base
    }

    #[test]
    fn float_nth_weekday() {
        // 3rd Thursday (dayname 4) of November 2024 is the 21st.
        assert!(float_match(Some(&[11]), 4, 3, None, Date::new(2024, 11, 21)));
        assert!(!float_match(Some(&[11]), 4, 3, None, Date::new(2024, 11, 14)));
        // Wrong month is rejected by the month filter.
        assert!(!float_match(Some(&[11]), 4, 3, None, Date::new(2024, 10, 17)));
        // "any month" (t): 3rd Thursday of October 2024 is the 17th.
        assert!(float_match(None, 4, 3, None, Date::new(2024, 10, 17)));
        // Last Monday (dayname 1) of May 2024 (Memorial Day) is the 27th.
        assert!(float_match(Some(&[5]), 1, -1, None, Date::new(2024, 5, 27)));
        assert!(!float_match(Some(&[5]), 1, -1, None, Date::new(2024, 5, 20)));
        // 1st Monday of September 2024 (Labor Day) is the 2nd.
        assert!(float_match(Some(&[9]), 1, 1, None, Date::new(2024, 9, 2)));
    }

    #[test]
    fn date_with_wildcards() {
        let d = Date::new(2024, 7, 4);
        assert!(date_wildcard(Some(7), Some(4), Some(2024), d));
        assert!(date_wildcard(Some(7), Some(4), None, d)); // any year
        assert!(date_wildcard(None, Some(4), None, d)); // the 4th of any month
        assert!(!date_wildcard(Some(7), Some(5), None, d));
        assert!(!date_wildcard(Some(8), None, None, d));
    }

    #[test]
    fn day_of_year_strings() {
        assert_eq!(
            day_of_year_string(Date::new(2024, 1, 1)),
            "Day 1 of 366; 365 days remaining"
        );
        assert_eq!(
            day_of_year_string(Date::new(2024, 12, 31)),
            "Day 366 of 366; 0 days remaining"
        );
        assert_eq!(
            day_of_year_string(Date::new(2023, 12, 30)),
            "Day 364 of 365; 1 day remaining"
        );
    }

    #[test]
    fn insert_line_builders() {
        let d = Date::new(2024, 12, 25);
        assert_eq!(format_monthly(25), "* 25 ");
        assert_eq!(format_yearly(d), "December 25 ");
        assert_eq!(
            format_anniversary_sexp(d),
            "%%(diary-anniversary 12 25 2024) "
        );
        assert_eq!(
            format_block_sexp(Date::new(2024, 6, 1), Date::new(2024, 6, 10)),
            "%%(diary-block 6 1 2024 6 10 2024) "
        );
        assert_eq!(
            format_cyclic_sexp(7, d),
            "%%(diary-cyclic 7 12 25 2024) "
        );
        assert_eq!(format_other_entry('H', "Tishri 5, 5785"), "HTishri 5, 5785 ");
        assert_eq!(format_other_yearly('H', "Tishri", 5), "HTishri 5 ");
        assert_eq!(format_other_monthly('I', 12), "I* 12 ");
        assert_eq!(
            format_other_anniversary_sexp("hebrew", 7, 10, 5750),
            "%%(diary-hebrew-anniversary 7 10 5750) "
        );
    }

    #[test]
    fn hebrew_omer_count() {
        // 16 Nisan is the 1st day of the omer, everywhere self-consistent with
        // the Hebrew calendar conversions. Pick Hebrew year 5784.
        let greg16 = crate::calendar::from_rd(crate::calendar::fixed_from_hebrew(5784, 1, 16));
        assert_eq!(hebrew_omer(greg16), Some((1, 0, 1)));
        // 15 Nisan (Passover) itself is omer 0 → outside the count.
        let greg15 = crate::calendar::from_rd(crate::calendar::fixed_from_hebrew(5784, 1, 15));
        assert_eq!(hebrew_omer(greg15), None);
        // The 8th day is week 1, day 1.
        let greg23 = crate::calendar::from_rd(crate::calendar::fixed_from_hebrew(5784, 1, 23));
        assert_eq!(hebrew_omer(greg23), Some((8, 1, 1)));
        assert_eq!(
            hebrew_omer_string(8, 1, 1),
            "Day 8th of the omer (which is 1 week and 1 day)"
        );
        assert_eq!(
            hebrew_omer_string(1, 0, 1),
            "Day 1st of the omer (which is 1 day)"
        );
        assert_eq!(
            hebrew_omer_string(7, 1, 0),
            "Day 7th of the omer (which is 1 week)"
        );
    }

    #[test]
    fn hebrew_rosh_hodesh_days() {
        // 1 Tishri (Rosh Hashanah) is Rosh Hodesh Tishri.
        let d = crate::calendar::from_rd(crate::calendar::fixed_from_hebrew(5785, 7, 1));
        assert_eq!(hebrew_rosh_hodesh(d), Some("Tishri".to_string()));
        // A mid-month day is not Rosh Hodesh.
        let mid = crate::calendar::from_rd(crate::calendar::fixed_from_hebrew(5785, 7, 15));
        assert_eq!(hebrew_rosh_hodesh(mid), None);
    }

    #[test]
    fn hebrew_birthday_recurs() {
        // Someone born 5750 Tishri 10; on the same Hebrew date in 5785 it is the
        // 35th Hebrew birthday.
        let d = crate::calendar::from_rd(crate::calendar::fixed_from_hebrew(5785, 7, 10));
        assert_eq!(hebrew_birthday(7, 10, 5750, d), Some(35));
        // A different Hebrew date → no match.
        let other = crate::calendar::from_rd(crate::calendar::fixed_from_hebrew(5785, 7, 11));
        assert_eq!(hebrew_birthday(7, 10, 5750, other), None);
    }

    #[test]
    fn parses_and_matches_sexp_entries() {
        // Anniversary.
        let (spec, text) =
            parse_line("%%(diary-anniversary 10 31 1948) Arthur's birthday").unwrap();
        assert_eq!(text, "Arthur's birthday");
        assert!(spec.matches(Date::new(2024, 10, 31)));
        assert!(!spec.matches(Date::new(2024, 10, 30)));
        assert!(!spec.matches(Date::new(1948, 10, 31))); // same year → diff not > 0

        // Block.
        let (spec, _) = parse_line("%%(diary-block 6 1 2024 6 10 2024) Vacation").unwrap();
        assert!(spec.matches(Date::new(2024, 6, 5)));
        assert!(!spec.matches(Date::new(2024, 6, 11)));

        // Cyclic.
        let (spec, _) = parse_line("%%(diary-cyclic 3 1 1 2024) Meds").unwrap();
        assert!(spec.matches(Date::new(2024, 1, 4)));
        assert!(!spec.matches(Date::new(2024, 1, 5)));

        // Float: 3rd Thursday of November.
        let (spec, _) = parse_line("%%(diary-float 11 4 3) Meeting").unwrap();
        assert!(spec.matches(Date::new(2024, 11, 21)));
        assert!(!spec.matches(Date::new(2024, 11, 14)));
        // Float with wildcard month t: 1st Monday of any month.
        let (spec, _) = parse_line("%%(diary-float t 1 1) First Monday").unwrap();
        assert!(spec.matches(Date::new(2024, 9, 2)));
        assert!(spec.matches(Date::new(2024, 7, 1)));

        // Date wildcards: the 15th of any month, any year.
        let (spec, _) = parse_line("%%(diary-date t 15 t) Payday").unwrap();
        assert!(spec.matches(Date::new(2024, 3, 15)));
        assert!(!spec.matches(Date::new(2024, 3, 16)));

        // A whole file with a mix of plain and sexp entries.
        let entries = parse_file(
            "10/31 Halloween\n%%(diary-cyclic 7 1 1 2024) Weekly\nnot an entry\n",
        );
        assert_eq!(entries.len(), 2);
        assert!(has_entry(&entries, Date::new(2024, 1, 8))); // +7 days
    }

    #[test]
    fn appt_time_parsing() {
        assert_eq!(parse_appt_time("09:30"), Some(9 * 60 + 30));
        assert_eq!(parse_appt_time("9:30am"), Some(9 * 60 + 30));
        assert_eq!(parse_appt_time("12:00am"), Some(0)); // midnight
        assert_eq!(parse_appt_time("12:00pm"), Some(12 * 60)); // noon
        assert_eq!(parse_appt_time("1:15pm"), Some(13 * 60 + 15));
        assert_eq!(parse_appt_time("23:59"), Some(23 * 60 + 59));
        assert_eq!(parse_appt_time("24:00"), None);
        assert_eq!(parse_appt_time("nope"), None);
        assert_eq!(format_appt_time(13 * 60 + 15), "13:15");
    }

    #[test]
    fn appt_add_and_delete() {
        let mut list = Vec::new();
        assert!(appt_add(
            &mut list,
            Appt {
                minutes: 600,
                message: "Lunch".into()
            }
        ));
        assert!(appt_add(
            &mut list,
            Appt {
                minutes: 540,
                message: "Standup".into()
            }
        ));
        // Sorted by time: 09:00 Standup before 10:00 Lunch.
        assert_eq!(list[0].message, "Standup");
        assert_eq!(list[1].message, "Lunch");
        // Duplicate ignored.
        assert!(!appt_add(
            &mut list,
            Appt {
                minutes: 540,
                message: "Standup".into()
            }
        ));
        assert_eq!(list.len(), 2);
        assert_eq!(appt_delete(&mut list, "Lunch"), 1);
        assert_eq!(list.len(), 1);
    }
}
