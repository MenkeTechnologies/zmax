//! Pure, dependency-free date arithmetic backing the Calendar substrate (the
//! zemacs port of GNU Emacs `calendar-mode`). The term-crate Component holds a
//! "point date" and calls these to move it and lay out the month grid. Uses
//! Howard Hinnant's `days_from_civil` / `civil_from_days` (proleptic Gregorian),
//! which are exact for any year. Unit-tested against known dates.

/// A calendar date. Months and days are 1-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Date {
        Date { year, month, day }
    }
}

/// Days since the Unix epoch (1970-01-01 = 0) for a proleptic-Gregorian date.
pub fn to_serial(d: Date) -> i64 {
    let (mut y, m, day) = (d.year as i64, d.month as i64, d.day as i64);
    y -= (m <= 2) as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Inverse of [`to_serial`].
pub fn from_serial(z: i64) -> Date {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    Date {
        year: (y + (month <= 2) as i64) as i32,
        month: month as u32,
        day: day as u32,
    }
}

/// Day of week, 0 = Sunday .. 6 = Saturday.
pub fn weekday(d: Date) -> u32 {
    ((to_serial(d) % 7 + 4).rem_euclid(7)) as u32
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// 1-based day number within the year (Jan 1 = 1).
pub fn day_of_year(d: Date) -> u32 {
    (to_serial(d) - to_serial(Date::new(d.year, 1, 1)) + 1) as u32
}

/// Add `n` days (may be negative), crossing month/year boundaries correctly.
pub fn add_days(d: Date, n: i64) -> Date {
    from_serial(to_serial(d) + n)
}

/// Add `n` months (may be negative), clamping the day to the target month's
/// length (Emacs `calendar-forward-month` behaviour: Jan 31 + 1mo = Feb 28/29).
pub fn add_months(d: Date, n: i64) -> Date {
    let total = (d.year as i64) * 12 + (d.month as i64 - 1) + n;
    let year = total.div_euclid(12) as i32;
    let month = (total.rem_euclid(12) + 1) as u32;
    let day = d.day.min(days_in_month(year, month));
    Date::new(year, month, day)
}

pub fn add_years(d: Date, n: i64) -> Date {
    add_months(d, n * 12)
}

/// Sunday that begins the week containing `d` (Emacs `calendar-beginning-of-week`
/// with the default Sunday start).
pub fn beginning_of_week(d: Date) -> Date {
    add_days(d, -(weekday(d) as i64))
}

/// Saturday that ends the week containing `d`.
pub fn end_of_week(d: Date) -> Date {
    add_days(d, 6 - weekday(d) as i64)
}

/// Inclusive day count between two dates (Emacs `calendar-count-days-region`).
pub fn count_days(a: Date, b: Date) -> i64 {
    (to_serial(b) - to_serial(a)).abs() + 1
}

/// First day of `d`'s month (Emacs `calendar-beginning-of-month`).
pub fn beginning_of_month(d: Date) -> Date {
    Date::new(d.year, d.month, 1)
}

/// Last day of `d`'s month (Emacs `calendar-end-of-month`).
pub fn end_of_month(d: Date) -> Date {
    Date::new(d.year, d.month, days_in_month(d.year, d.month))
}

/// January 1 of `d`'s year (Emacs `calendar-beginning-of-year`).
pub fn beginning_of_year(d: Date) -> Date {
    Date::new(d.year, 1, 1)
}

/// December 31 of `d`'s year (Emacs `calendar-end-of-year`).
pub fn end_of_year(d: Date) -> Date {
    Date::new(d.year, 12, 31)
}

/// The Julian Day Number of `d` (Emacs `calendar-julian-print-date` uses the
/// astronomical day count). JDN of 1970-01-01 is 2440588.
pub fn julian_day(d: Date) -> i64 {
    to_serial(d) + 2440588
}

/// The ISO 8601 week date of `d`: `(iso_year, week 1..=53, weekday 1=Mon..=7=Sun)`
/// (Emacs `calendar-iso-print-date`). The ISO year can differ from the calendar
/// year for days in the first/last week.
pub fn iso_week(d: Date) -> (i32, u32, u32) {
    // ISO weekday: Monday = 1 .. Sunday = 7 (our weekday is 0 = Sunday).
    let iso_dow = ((weekday(d) + 6) % 7) + 1;
    // The Thursday of this week determines the ISO year and week number.
    let thursday = add_days(d, 4 - iso_dow as i64);
    let iso_year = thursday.year;
    let jan1 = Date::new(iso_year, 1, 1);
    let week = ((to_serial(thursday) - to_serial(jan1)) / 7 + 1) as u32;
    (iso_year, week, iso_dow)
}

pub const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August",
    "September", "October", "November", "December",
];

pub const WEEKDAY_ABBR: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_weekdays() {
        assert_eq!(to_serial(Date::new(1970, 1, 1)), 0);
        assert_eq!(weekday(Date::new(1970, 1, 1)), 4); // Thursday
        assert_eq!(weekday(Date::new(2000, 1, 1)), 6); // Saturday
        assert_eq!(weekday(Date::new(2026, 7, 2)), 4); // Thursday
    }

    #[test]
    fn serial_roundtrips() {
        for (y, m, d) in [(1900, 2, 28), (2000, 2, 29), (2024, 12, 31), (1969, 12, 31)] {
            let date = Date::new(y, m, d);
            assert_eq!(from_serial(to_serial(date)), date);
        }
    }

    #[test]
    fn leap_and_month_lengths() {
        assert!(is_leap(2000) && is_leap(2024) && !is_leap(1900) && !is_leap(2023));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2026, 4), 30);
    }

    #[test]
    fn add_days_crosses_boundaries() {
        assert_eq!(add_days(Date::new(2026, 1, 31), 1), Date::new(2026, 2, 1));
        assert_eq!(add_days(Date::new(2026, 3, 1), -1), Date::new(2026, 2, 28));
        assert_eq!(add_days(Date::new(2024, 12, 31), 1), Date::new(2025, 1, 1));
    }

    #[test]
    fn add_months_clamps_day() {
        assert_eq!(add_months(Date::new(2026, 1, 31), 1), Date::new(2026, 2, 28));
        assert_eq!(add_months(Date::new(2024, 1, 31), 1), Date::new(2024, 2, 29));
        assert_eq!(add_months(Date::new(2026, 12, 15), 1), Date::new(2027, 1, 15));
        assert_eq!(add_months(Date::new(2026, 1, 15), -1), Date::new(2025, 12, 15));
    }

    #[test]
    fn week_bounds_and_counts() {
        // 2026-07-02 is a Thursday; its week runs Sun 06-28 .. Sat 07-04.
        assert_eq!(beginning_of_week(Date::new(2026, 7, 2)), Date::new(2026, 6, 28));
        assert_eq!(end_of_week(Date::new(2026, 7, 2)), Date::new(2026, 7, 4));
        assert_eq!(count_days(Date::new(2026, 7, 1), Date::new(2026, 7, 1)), 1);
        assert_eq!(count_days(Date::new(2026, 7, 1), Date::new(2026, 7, 10)), 10);
    }

    #[test]
    fn day_of_year_works() {
        assert_eq!(day_of_year(Date::new(2026, 1, 1)), 1);
        assert_eq!(day_of_year(Date::new(2024, 12, 31)), 366); // leap
        assert_eq!(day_of_year(Date::new(2023, 12, 31)), 365);
    }

    #[test]
    fn month_and_year_bounds() {
        assert_eq!(beginning_of_month(Date::new(2024, 2, 15)), Date::new(2024, 2, 1));
        assert_eq!(end_of_month(Date::new(2024, 2, 15)), Date::new(2024, 2, 29)); // leap Feb
        assert_eq!(end_of_month(Date::new(2023, 2, 15)), Date::new(2023, 2, 28));
        assert_eq!(beginning_of_year(Date::new(2026, 7, 2)), Date::new(2026, 1, 1));
        assert_eq!(end_of_year(Date::new(2026, 7, 2)), Date::new(2026, 12, 31));
    }

    #[test]
    fn julian_day_number() {
        assert_eq!(julian_day(Date::new(1970, 1, 1)), 2440588);
        assert_eq!(julian_day(Date::new(2000, 1, 1)), 2451545);
    }

    #[test]
    fn iso_week_date() {
        // 2026-07-02 is a Thursday -> ISO weekday 4.
        assert_eq!(iso_week(Date::new(2026, 7, 2)).2, 4);
        // Well-known ISO boundary cases:
        // 2021-01-01 (Friday) belongs to ISO week 53 of 2020.
        assert_eq!(iso_week(Date::new(2021, 1, 1)), (2020, 53, 5));
        // 2024-12-30 (Monday) belongs to ISO week 1 of 2025.
        assert_eq!(iso_week(Date::new(2024, 12, 30)), (2025, 1, 1));
        // A mid-year date: 2023-01-02 is ISO 2023-W01-1 (Monday).
        assert_eq!(iso_week(Date::new(2023, 1, 2)), (2023, 1, 1));
    }
}
