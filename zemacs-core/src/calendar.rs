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

/// Day-of-month (1-based) of the `n`th occurrence (n = 1..) of `target` weekday
/// (0 = Sunday .. 6 = Saturday) in `month`. Assumes the month has an `n`th such
/// weekday (true for n <= 4, and n = 5 only for the weekdays that occur 5 times).
pub fn nth_weekday(year: i32, month: u32, target: u32, n: u32) -> u32 {
    let first_wd = weekday(Date::new(year, month, 1));
    let offset = (7 + target - first_wd) % 7;
    1 + offset + (n - 1) * 7
}

/// Day-of-month of the last `target` weekday (0 = Sunday .. 6 = Saturday) in
/// `month` (Emacs uses this for Memorial Day = last Monday of May).
pub fn last_weekday(year: i32, month: u32, target: u32) -> u32 {
    let dim = days_in_month(year, month);
    let last_wd = weekday(Date::new(year, month, dim));
    dim - ((7 + last_wd - target) % 7)
}

/// Fixed and easily-computed US holidays that fall in `month` of `year`, as
/// `(day-of-month, name)` sorted by day (Emacs `calendar-holidays`). Covers the
/// fixed-date observances plus the `n`th-weekday floating holidays; deliberately
/// omits astronomically-computed ones (Easter, equinoxes).
pub fn holidays(year: i32, month: u32) -> Vec<(u32, &'static str)> {
    // Fixed-date holidays: (month, day, name).
    const FIXED: &[(u32, u32, &str)] = &[
        (1, 1, "New Year's Day"),
        (2, 2, "Groundhog Day"),
        (2, 14, "Valentine's Day"),
        (3, 17, "St. Patrick's Day"),
        (4, 1, "April Fools' Day"),
        (6, 19, "Juneteenth"),
        (7, 4, "Independence Day"),
        (10, 31, "Halloween"),
        (11, 11, "Veterans Day"),
        (12, 25, "Christmas"),
        (12, 31, "New Year's Eve"),
    ];
    let mut out: Vec<(u32, &'static str)> = FIXED
        .iter()
        .filter(|&&(m, _, _)| m == month)
        .map(|&(_, d, name)| (d, name))
        .collect();
    // Floating (nth-weekday) holidays. Weekday: 0 = Sunday .. 6 = Saturday.
    match month {
        1 => out.push((nth_weekday(year, 1, 1, 3), "Martin Luther King Jr. Day")),
        2 => out.push((nth_weekday(year, 2, 1, 3), "Presidents' Day")),
        5 => out.push((last_weekday(year, 5, 1), "Memorial Day")),
        9 => out.push((nth_weekday(year, 9, 1, 1), "Labor Day")),
        10 => out.push((nth_weekday(year, 10, 1, 2), "Columbus Day")),
        11 => out.push((nth_weekday(year, 11, 4, 4), "Thanksgiving")),
        _ => {}
    }
    out.sort_by_key(|&(d, _)| d);
    out
}

/// The holiday falling exactly on `d`, if any (Emacs `calendar-cursor-holidays`).
pub fn holiday_on(d: Date) -> Option<&'static str> {
    holidays(d.year, d.month)
        .into_iter()
        .find(|&(day, _)| day == d.day)
        .map(|(_, name)| name)
}

/// Parse a date typed at the `calendar-goto-date` prompt. Accepts `Y/M/D`,
/// `Y-M-D`, or space/comma-separated `Y M D`, validating the month and the day
/// against the month's length. Returns `None` on anything malformed.
pub fn parse_ymd(s: &str) -> Option<Date> {
    let nums: Option<Vec<i64>> = s
        .split(|c: char| c == '/' || c == '-' || c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<i64>().ok())
        .collect();
    let nums = nums?;
    if nums.len() != 3 {
        return None;
    }
    let (y, m, d) = (nums[0], nums[1], nums[2]);
    if !(1..=12).contains(&m) {
        return None;
    }
    let year = y as i32;
    let month = m as u32;
    if d < 1 || d as u32 > days_in_month(year, month) {
        return None;
    }
    Some(Date::new(year, month, d as u32))
}

pub const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub const WEEKDAY_ABBR: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

// ===========================================================================
// "Other calendars" — the zemacs port of GNU Emacs cal-julian / cal-hebrew /
// cal-islam / cal-persia / cal-coptic / cal-french / cal-bahai / cal-mayan.
//
// Emacs works in "absolute" dates = R.D. (Rata Die) fixed day numbers, the
// count of days since the imaginary Gregorian date Sunday, December 31, 1 BC
// (so R.D. 1 = Gregorian 0001-01-01). Our [`to_serial`] is days-since-Unix;
// [`rd`]/[`from_rd`] shift that to R.D. so these algorithms match Emacs
// verbatim. All conversion algorithms below are transcribed from GNU Emacs 30's
// calendar library (arithmetic rules), unit-tested against known dates.
// ===========================================================================

/// R.D. (absolute) fixed day number of `d`. `rd(0001-01-01) == 1`,
/// `rd(1970-01-01) == 719163`.
pub fn rd(d: Date) -> i64 {
    to_serial(d) - to_serial(Date::new(1, 1, 1)) + 1
}

/// Inverse of [`rd`]: the proleptic-Gregorian date for R.D. day number `f`.
pub fn from_rd(f: i64) -> Date {
    from_serial(f + to_serial(Date::new(1, 1, 1)) - 1)
}

// --- Julian (Roman) calendar (cal-julian.el) -------------------------------

const JULIAN_EPOCH: i64 = -1; // R.D. of Julian 0001-01-01.

/// True if `year` is a Julian leap year (every 4th year; BCE offset by one).
pub fn julian_leap(year: i32) -> bool {
    (year as i64).rem_euclid(4) == if year > 0 { 0 } else { 3 }
}

/// R.D. of a Julian date `(year, month, day)` (Dershowitz–Reingold).
pub fn fixed_from_julian(year: i32, month: u32, day: u32) -> i64 {
    let y = if year < 0 { year as i64 + 1 } else { year as i64 };
    let m = month as i64;
    JULIAN_EPOCH - 1
        + 365 * (y - 1)
        + (y - 1).div_euclid(4)
        + (367 * m - 362).div_euclid(12)
        + if m <= 2 {
            0
        } else if julian_leap(year) {
            -1
        } else {
            -2
        }
        + day as i64
}

/// Julian `(year, month, day)` for R.D. `f`.
pub fn julian_from_fixed(f: i64) -> (i32, u32, u32) {
    let approx = (4 * (f - JULIAN_EPOCH) + 1464).div_euclid(1461);
    let year = if approx <= 0 {
        (approx - 1) as i32
    } else {
        approx as i32
    };
    let prior_days = f - fixed_from_julian(year, 1, 1);
    let correction = if f < fixed_from_julian(year, 3, 1) {
        0
    } else if julian_leap(year) {
        1
    } else {
        2
    };
    let month = ((12 * (prior_days + correction) + 373).div_euclid(367)) as u32;
    let day = (f - fixed_from_julian(year, month, 1) + 1) as u32;
    (year, month, day)
}

/// Julian date of a Gregorian date, e.g. `"December 19, 1999"`.
pub fn julian_string(d: Date) -> String {
    let (y, m, day) = julian_from_fixed(rd(d));
    format!("{} {}, {}", MONTH_NAMES[(m - 1) as usize], day, y)
}

// --- Coptic and Ethiopic calendars (cal-coptic.el) -------------------------

const COPTIC_EPOCH: i64 = 103605; // R.D. of Coptic 0001-01-01.
const ETHIOPIC_EPOCH: i64 = 2796; // R.D. of Ethiopic 0001-01-01.

pub const COPTIC_MONTH_NAMES: [&str; 13] = [
    "Tut", "Babah", "Hatur", "Kiyahk", "Tubah", "Amshir", "Baramhat", "Baramundah", "Bashans",
    "Baunah", "Abib", "Misra", "Nasi",
];
pub const ETHIOPIC_MONTH_NAMES: [&str; 13] = [
    "Maskaram", "Teqemt", "Khedar", "Takhsas", "Ter", "Yakatit", "Magabit", "Miyazya", "Genbot",
    "Sanni", "Hamli", "Nahasi", "Paguemen",
];

fn coptic_like_to_fixed(epoch: i64, year: i32, month: u32, day: u32) -> i64 {
    epoch - 1
        + 365 * (year as i64 - 1)
        + (year as i64).div_euclid(4)
        + 30 * (month as i64 - 1)
        + day as i64
}

fn coptic_like_from_fixed(epoch: i64, f: i64) -> (i32, u32, u32) {
    let year = ((4 * (f - epoch) + 1463).div_euclid(1461)) as i32;
    let month = (1 + (f - coptic_like_to_fixed(epoch, year, 1, 1)).div_euclid(30)) as u32;
    let day = (f + 1 - coptic_like_to_fixed(epoch, year, month, 1)) as u32;
    (year, month, day)
}

pub fn coptic_from_fixed(f: i64) -> (i32, u32, u32) {
    coptic_like_from_fixed(COPTIC_EPOCH, f)
}
pub fn fixed_from_coptic(year: i32, month: u32, day: u32) -> i64 {
    coptic_like_to_fixed(COPTIC_EPOCH, year, month, day)
}
pub fn ethiopic_from_fixed(f: i64) -> (i32, u32, u32) {
    coptic_like_from_fixed(ETHIOPIC_EPOCH, f)
}
pub fn fixed_from_ethiopic(year: i32, month: u32, day: u32) -> i64 {
    coptic_like_to_fixed(ETHIOPIC_EPOCH, year, month, day)
}

pub fn coptic_string(d: Date) -> String {
    let (y, m, day) = coptic_from_fixed(rd(d));
    format!("{} {}, {}", COPTIC_MONTH_NAMES[(m - 1) as usize], day, y)
}
pub fn ethiopic_string(d: Date) -> String {
    let (y, m, day) = ethiopic_from_fixed(rd(d));
    format!("{} {}, {}", ETHIOPIC_MONTH_NAMES[(m - 1) as usize], day, y)
}

// --- Hebrew calendar (cal-hebrew.el) ---------------------------------------

pub const HEBREW_MONTH_NAMES_COMMON: [&str; 12] = [
    "Nisan", "Iyar", "Sivan", "Tammuz", "Av", "Elul", "Tishri", "Heshvan", "Kislev", "Teveth",
    "Shevat", "Adar",
];
pub const HEBREW_MONTH_NAMES_LEAP: [&str; 13] = [
    "Nisan", "Iyar", "Sivan", "Tammuz", "Av", "Elul", "Tishri", "Heshvan", "Kislev", "Teveth",
    "Shevat", "Adar I", "Adar II",
];

pub fn hebrew_leap(year: i64) -> bool {
    (7 * year + 1).rem_euclid(19) < 7
}
pub fn hebrew_last_month_of_year(year: i64) -> u32 {
    if hebrew_leap(year) {
        13
    } else {
        12
    }
}

/// Days from the Hebrew epoch to the New Year (1 Tishri) of `year`
/// (`calendar-hebrew-elapsed-days`).
fn hebrew_elapsed_days(year: i64) -> i64 {
    let cy = year - 1;
    let months_elapsed = 235 * (cy / 19) + 12 * (cy % 19) + (7 * (cy % 19) + 1) / 19;
    let parts_elapsed = 204 + 793 * (months_elapsed % 1080);
    let hours_elapsed =
        5 + 12 * months_elapsed + 793 * (months_elapsed / 1080) + parts_elapsed / 1080;
    let day = 1 + 29 * months_elapsed + hours_elapsed / 24;
    let parts = 1080 * (hours_elapsed % 24) + parts_elapsed % 1080;
    let day = if parts >= 19440 {
        day + 1
    } else if day % 7 == 2 && parts >= 9924 && !hebrew_leap(year) {
        day + 1
    } else if day % 7 == 1 && parts >= 16789 && hebrew_leap(year - 1) {
        day + 1
    } else {
        day
    };
    if matches!(day % 7, 0 | 3 | 5) {
        day + 1
    } else {
        day
    }
}

fn hebrew_days_in_year(year: i64) -> i64 {
    hebrew_elapsed_days(year + 1) - hebrew_elapsed_days(year)
}
fn hebrew_long_heshvan(year: i64) -> bool {
    hebrew_days_in_year(year) % 10 == 5
}
fn hebrew_short_kislev(year: i64) -> bool {
    hebrew_days_in_year(year) % 10 == 3
}

pub fn hebrew_last_day_of_month(month: u32, year: i64) -> u32 {
    if matches!(month, 2 | 4 | 6 | 10 | 13)
        || (month == 8 && !hebrew_long_heshvan(year))
        || (month == 9 && hebrew_short_kislev(year))
        || (month == 12 && !hebrew_leap(year))
    {
        29
    } else {
        30
    }
}

/// R.D. of a Hebrew date (`calendar-hebrew-to-absolute`).
pub fn fixed_from_hebrew(year: i64, month: u32, day: u32) -> i64 {
    let mut total = day as i64;
    if month < 7 {
        for m in 7..=hebrew_last_month_of_year(year) {
            total += hebrew_last_day_of_month(m, year) as i64;
        }
        for m in 1..month {
            total += hebrew_last_day_of_month(m, year) as i64;
        }
    } else {
        for m in 7..month {
            total += hebrew_last_day_of_month(m, year) as i64;
        }
    }
    total + hebrew_elapsed_days(year) - 1373429
}

/// Hebrew `(year, month, day)` for R.D. `f` (`calendar-hebrew-from-absolute`).
pub fn hebrew_from_fixed(f: i64) -> (i64, u32, u32) {
    let approx = (f + 1373429).div_euclid(366);
    let mut year = approx;
    while f >= fixed_from_hebrew(year + 1, 7, 1) {
        year += 1;
    }
    let start = if f < fixed_from_hebrew(year, 1, 1) { 7 } else { 1 };
    let mut month = start;
    while f > fixed_from_hebrew(year, month, hebrew_last_day_of_month(month, year)) {
        month += 1;
    }
    let day = (f - fixed_from_hebrew(year, month, 1) + 1) as u32;
    (year, month, day)
}

pub fn hebrew_string(d: Date) -> String {
    let (y, m, day) = hebrew_from_fixed(rd(d));
    let name = if hebrew_last_month_of_year(y) == 12 {
        HEBREW_MONTH_NAMES_COMMON[(m - 1) as usize]
    } else {
        HEBREW_MONTH_NAMES_LEAP[(m - 1) as usize]
    };
    format!("{} {}, {}", name, day, y)
}

// --- Islamic calendar (arithmetic/civil; cal-islam.el) ---------------------

pub const ISLAMIC_MONTH_NAMES: [&str; 12] = [
    "Muharram", "Safar", "Rabi I", "Rabi II", "Jumada I", "Jumada II", "Rajab", "Sha'ban",
    "Ramadan", "Shawwal", "Dhu al-Qada", "Dhu al-Hijjah",
];

/// R.D. of the Islamic epoch (Julian 16 July 622).
fn islamic_epoch() -> i64 {
    fixed_from_julian(622, 7, 16)
}

pub fn islamic_leap(year: i64) -> bool {
    matches!(year.rem_euclid(30), 2 | 5 | 7 | 10 | 13 | 16 | 18 | 21 | 24 | 26 | 29)
}
pub fn islamic_last_day_of_month(month: u32, year: i64) -> u32 {
    if month % 2 == 1 {
        30
    } else if month == 12 && islamic_leap(year) {
        30
    } else {
        29
    }
}
fn islamic_day_number(month: u32, day: u32) -> i64 {
    29 * (month as i64 - 1) + (month as i64) / 2 + day as i64
}

/// R.D. of an Islamic date (`calendar-islamic-to-absolute`).
pub fn fixed_from_islamic(year: i64, month: u32, day: u32) -> i64 {
    islamic_day_number(month, day) + (year - 1) * 354 + (3 + 11 * year) / 30 + islamic_epoch() - 1
}

/// Islamic `(year, month, day)` for R.D. `f`, or `None` before the epoch.
pub fn islamic_from_fixed(f: i64) -> Option<(i64, u32, u32)> {
    if f < islamic_epoch() {
        return None;
    }
    let approx = (f - islamic_epoch()).div_euclid(355);
    let mut year = approx;
    while f >= fixed_from_islamic(year + 1, 1, 1) {
        year += 1;
    }
    let mut month = 1u32;
    while f > fixed_from_islamic(year, month, islamic_last_day_of_month(month, year)) {
        month += 1;
    }
    let day = (f - fixed_from_islamic(year, month, 1) + 1) as u32;
    Some((year, month, day))
}

pub fn islamic_string(d: Date) -> Option<String> {
    let (y, m, day) = islamic_from_fixed(rd(d))?;
    Some(format!(
        "{} {}, {}",
        ISLAMIC_MONTH_NAMES[(m - 1) as usize],
        day,
        y
    ))
}

// --- Persian calendar (arithmetic 2820-year; cal-persia.el) ----------------

pub const PERSIAN_MONTH_NAMES: [&str; 12] = [
    "Farvardin",
    "Ordibehesht",
    "Khordad",
    "Tir",
    "Mordad",
    "Shahrivar",
    "Mehr",
    "Aban",
    "Azar",
    "Dey",
    "Bahman",
    "Esfand",
];

fn persian_epoch() -> i64 {
    fixed_from_julian(622, 3, 19)
}

pub fn persian_leap(year: i64) -> bool {
    let a = if year >= 0 { year + 2346 } else { year + 2347 };
    let inner = (a.rem_euclid(2820)).rem_euclid(768);
    (inner * 683).rem_euclid(2820) < 683
}
pub fn persian_last_day_of_month(month: u32, year: i64) -> u32 {
    if month < 7 {
        31
    } else if month < 12 {
        30
    } else if persian_leap(year) {
        30
    } else {
        29
    }
}

/// R.D. of a Persian date (`calendar-persian-to-absolute`).
pub fn fixed_from_persian(year: i64, month: u32, day: u32) -> i64 {
    if year < 0 {
        return fixed_from_persian(1 + year.rem_euclid(2820), month, day)
            + 1029983 * year.div_euclid(2820);
    }
    let mut prior = 0i64;
    for m in 1..month {
        prior += persian_last_day_of_month(m, year) as i64;
    }
    persian_epoch() - 1
        + 365 * (year - 1)
        + 683 * (year + 2345).div_euclid(2820)
        + 186 * ((year + 2345).rem_euclid(2820)).div_euclid(768)
        + (683 * ((year + 2345).rem_euclid(2820)).rem_euclid(768)).div_euclid(2820)
        - 568
        + prior
        + day as i64
}

fn persian_year_from_fixed(f: i64) -> i64 {
    let d0 = f - fixed_from_persian(-2345, 1, 1);
    let n2820 = d0.div_euclid(1029983);
    let d1 = d0.rem_euclid(1029983);
    let n768 = d1.div_euclid(280506);
    let d2 = d1.rem_euclid(280506);
    let n1 = (2820 * (d2 + 366)).div_euclid(1029983);
    let year = 2820 * n2820 + 768 * n768 + if d1 == 1029617 { n1 - 1 } else { n1 } - 2345;
    if year < 1 {
        year - 1
    } else {
        year
    }
}

/// Persian `(year, month, day)` for R.D. `f` (`calendar-persian-from-absolute`).
pub fn persian_from_fixed(f: i64) -> (i64, u32, u32) {
    let year = persian_year_from_fixed(f);
    let mut month = 1u32;
    while f > fixed_from_persian(year, month, persian_last_day_of_month(month, year)) {
        month += 1;
    }
    let day = (f - fixed_from_persian(year, month, 1) + 1) as u32;
    (year, month, day)
}

pub fn persian_string(d: Date) -> String {
    let (y, m, day) = persian_from_fixed(rd(d));
    format!("{} {}, {}", PERSIAN_MONTH_NAMES[(m - 1) as usize], day, y)
}

// --- French Revolutionary calendar (Romme arithmetic; cal-french.el) -------

pub const FRENCH_MONTH_NAMES: [&str; 12] = [
    "Vendemiaire",
    "Brumaire",
    "Frimaire",
    "Nivose",
    "Pluviose",
    "Ventose",
    "Germinal",
    "Floreal",
    "Prairial",
    "Messidor",
    "Thermidor",
    "Fructidor",
];
pub const FRENCH_SANSCULOTTIDES: [&str; 6] = [
    "Jour de la Vertu",
    "Jour du Genie",
    "Jour du Travail",
    "Jour de la Raison",
    "Jour de la Recompense",
    "Jour de la Revolution",
];

fn french_epoch() -> i64 {
    rd(Date::new(1792, 9, 22))
}

pub fn french_leap(year: i64) -> bool {
    matches!(year, 3 | 7 | 11 | 15 | 20)
        || (year > 20
            && year % 4 == 0
            && !matches!(year % 400, 100 | 200 | 300)
            && year % 4000 != 0)
}
pub fn french_last_day_of_month(month: u32, year: i64) -> u32 {
    if month < 13 {
        30
    } else if french_leap(year) {
        6
    } else {
        5
    }
}

/// R.D. of a French Revolutionary date (`calendar-french-to-absolute`).
pub fn fixed_from_french(year: i64, month: u32, day: u32) -> i64 {
    365 * (year - 1)
        + if year < 20 {
            year / 4
        } else {
            (year - 1) / 4 - (year - 1) / 100 + (year - 1) / 400 - (year - 1) / 4000
        }
        + 30 * (month as i64 - 1)
        + day as i64
        + french_epoch()
        - 1
}

/// French Revolutionary `(year, month, day)` for R.D. `f`, or `None` before the
/// epoch. Month 13 is the 5/6 `sansculottides` at year's end.
pub fn french_from_fixed(f: i64) -> Option<(i64, u32, u32)> {
    if f < french_epoch() {
        return None;
    }
    let approx = (f - french_epoch()).div_euclid(366);
    let mut year = approx;
    while f >= fixed_from_french(year + 1, 1, 1) {
        year += 1;
    }
    let mut month = 1u32;
    while f > fixed_from_french(year, month, french_last_day_of_month(month, year)) {
        month += 1;
    }
    let day = (f - fixed_from_french(year, month, 1) + 1) as u32;
    Some((year, month, day))
}

pub fn french_string(d: Date) -> Option<String> {
    let (y, m, day) = french_from_fixed(rd(d))?;
    if m == 13 {
        Some(format!(
            "{} de l'Annee {} de la Revolution",
            FRENCH_SANSCULOTTIDES[(day - 1) as usize],
            y
        ))
    } else {
        Some(format!(
            "{} {} an {} de la Revolution",
            day,
            FRENCH_MONTH_NAMES[(m - 1) as usize],
            y
        ))
    }
}

// --- Baha'i calendar (arithmetic; approximation of cal-bahai.el) -----------
// NOTE: modern GNU Emacs computes Naw-Ruz astronomically (sunset at the vernal
// equinox in Tehran) for Baha'i years >= 172 (Gregorian >= 2015). This port
// fixes Naw-Ruz at Gregorian March 21, matching Emacs's older arithmetic rule
// for pre-2015 dates; for later years it can differ by a day. Marked PARTIAL.

pub const BAHAI_MONTH_NAMES: [&str; 19] = [
    "Baha", "Jalal", "Jamal", "'Azamat", "Nur", "Rahmat", "Kalimat", "Kamal", "Asma'", "'Izzat",
    "Mashiyyat", "'Ilm", "Qudrat", "Qawl", "Masa'il", "Sharaf", "Sultan", "Mulk", "'Ala'",
];

/// R.D. of Naw-Ruz beginning Baha'i `year` (fixed at Gregorian March 21).
fn bahai_nawruz(year: i64) -> i64 {
    rd(Date::new((1843 + year) as i32, 3, 21))
}

/// Number of Ayyam-i-Ha (intercalary) days in Baha'i `year` (4 or 5).
fn bahai_ayyam(year: i64) -> i64 {
    bahai_nawruz(year + 1) - bahai_nawruz(year) - 361
}

/// Baha'i `(year, month, day)` for R.D. `f`. `month == 0` denotes the
/// Ayyam-i-Ha intercalary days; `month == 19` is the final month (`'Ala'`).
pub fn bahai_from_fixed(f: i64) -> (i64, u32, u32) {
    // Estimate the Baha'i year, then correct.
    let mut year = f - rd(Date::new(1844, 3, 21)) - 1;
    year = year.div_euclid(366) + 1;
    while f >= bahai_nawruz(year + 1) {
        year += 1;
    }
    while f < bahai_nawruz(year) {
        year -= 1;
    }
    let doy = f - bahai_nawruz(year); // 0-based day of Baha'i year
    let ayyam = bahai_ayyam(year);
    if doy < 342 {
        ((year), (doy / 19 + 1) as u32, (doy % 19 + 1) as u32)
    } else if doy < 342 + ayyam {
        (year, 0, (doy - 342 + 1) as u32)
    } else {
        (year, 19, (doy - 342 - ayyam + 1) as u32)
    }
}

/// R.D. of a Baha'i date. `month == 0` = Ayyam-i-Ha; `month == 19` = `'Ala'`.
pub fn fixed_from_bahai(year: i64, month: u32, day: u32) -> i64 {
    let base = bahai_nawruz(year);
    let ayyam = bahai_ayyam(year);
    let off = match month {
        0 => 342 + (day as i64 - 1),
        19 => 342 + ayyam + (day as i64 - 1),
        _ => (month as i64 - 1) * 19 + (day as i64 - 1),
    };
    base + off
}

pub fn bahai_string(d: Date) -> String {
    let (y, m, day) = bahai_from_fixed(rd(d));
    let name = if m == 0 {
        "Ayyam-i-Ha".to_string()
    } else {
        BAHAI_MONTH_NAMES[(m - 1) as usize].to_string()
    };
    format!("{} {}, {}", name, day, y)
}

// --- Mayan calendar (cal-mayan.el) -----------------------------------------

const MAYAN_DAYS_BEFORE_ABSOLUTE_ZERO: i64 = 1137142; // GMT correlation 584283.

pub const MAYAN_HAAB_MONTHS: [&str; 19] = [
    "Pop", "Uo", "Zip", "Zotz", "Tzec", "Xul", "Yaxkin", "Mol", "Chen", "Yax", "Zac", "Ceh", "Mac",
    "Kankin", "Muan", "Pax", "Kayab", "Cumku", "Uayeb",
];
pub const MAYAN_TZOLKIN_NAMES: [&str; 20] = [
    "Imix", "Ik", "Akbal", "Kan", "Chicchan", "Cimi", "Manik", "Lamat", "Muluc", "Oc", "Chuen",
    "Eb", "Ben", "Ix", "Men", "Cib", "Caban", "Etznab", "Cauac", "Ahau",
];

/// Mayan long count `(baktun, katun, tun, uinal, kin)` for R.D. `f`.
pub fn mayan_long_count_from_fixed(f: i64) -> (i64, i64, i64, i64, i64) {
    let lc = f + MAYAN_DAYS_BEFORE_ABSOLUTE_ZERO;
    let baktun = lc.div_euclid(144000);
    let day_of_baktun = lc.rem_euclid(144000);
    let katun = day_of_baktun.div_euclid(7200);
    let day_of_katun = day_of_baktun.rem_euclid(7200);
    let tun = day_of_katun.div_euclid(360);
    let day_of_tun = day_of_katun.rem_euclid(360);
    let uinal = day_of_tun.div_euclid(20);
    let kin = day_of_tun.rem_euclid(20);
    (baktun, katun, tun, uinal, kin)
}

/// R.D. of a Mayan long count.
pub fn fixed_from_mayan_long_count(baktun: i64, katun: i64, tun: i64, uinal: i64, kin: i64) -> i64 {
    baktun * 144000 + katun * 7200 + tun * 360 + uinal * 20 + kin
        - MAYAN_DAYS_BEFORE_ABSOLUTE_ZERO
}

/// Mayan haab `(day, month_index_1based)` for R.D. `f`.
pub fn mayan_haab_from_fixed(f: i64) -> (i64, u32) {
    let lc = f + MAYAN_DAYS_BEFORE_ABSOLUTE_ZERO;
    // Haab at epoch = day 8 of month 18 (Cumku) -> day number 8 + 20*(18-1).
    let day_of_haab = (lc + 8 + 20 * (18 - 1)).rem_euclid(365);
    let day = day_of_haab.rem_euclid(20);
    let month = (day_of_haab.div_euclid(20) + 1) as u32;
    (day, month)
}

/// Mayan tzolkin `(number 1..=13, name_index 1..=20)` for R.D. `f`.
pub fn mayan_tzolkin_from_fixed(f: i64) -> (i64, u32) {
    let lc = f + MAYAN_DAYS_BEFORE_ABSOLUTE_ZERO;
    let number = (lc + 4 - 1).rem_euclid(13) + 1; // tzolkin count at epoch = 4
    let name = ((lc + 20 - 1).rem_euclid(20) + 1) as u32; // tzolkin name at epoch = 20
    (number, name)
}

pub fn mayan_string(d: Date) -> String {
    let f = rd(d);
    let (b, k, t, u, kin) = mayan_long_count_from_fixed(f);
    let (tz_num, tz_name) = mayan_tzolkin_from_fixed(f);
    let (h_day, h_month) = mayan_haab_from_fixed(f);
    format!(
        "Long count = {b}.{k}.{t}.{u}.{kin}; tzolkin = {tz_num} {}; haab = {h_day} {}",
        MAYAN_TZOLKIN_NAMES[(tz_name - 1) as usize],
        MAYAN_HAAB_MONTHS[(h_month - 1) as usize],
    )
}

// --- Astronomical (Julian) day number --------------------------------------

/// Astronomical (Julian) day number at noon UTC for `d` — the integer Julian
/// Day Number (Emacs `calendar-astro-print-day-number`). Same as [`julian_day`].
pub fn astro_day_number(d: Date) -> i64 {
    julian_day(d)
}

/// The ISO 8601 date string in Emacs's phrasing: `"Day 6 of week 52 of 1999"`.
pub fn iso_string(d: Date) -> String {
    let (y, w, dow) = iso_week(d);
    format!("Day {dow} of week {w} of {y}")
}

/// The Gregorian date for an ISO week date `(iso_year, week, weekday)` where
/// weekday is 1=Monday..=7=Sunday (inverse of [`iso_week`];
/// `calendar-iso-goto-week`/`calendar-iso-goto-date`).
pub fn date_from_iso(iso_year: i32, week: u32, weekday: u32) -> Date {
    let jan4 = Date::new(iso_year, 1, 4);
    // ISO weekday of Jan 4 (1=Mon..7=Sun); the Monday of ISO week 1.
    let jan4_iso = ((weekday_of(jan4) + 6) % 7) + 1;
    let week1_monday = to_serial(jan4) - (jan4_iso as i64 - 1);
    from_serial(week1_monday + (week as i64 - 1) * 7 + (weekday as i64 - 1))
}

// Small alias so `date_from_iso` reads clearly next to `weekday`.
fn weekday_of(d: Date) -> u32 {
    weekday(d)
}

// --- Lunar phases (mean-lunation approximation; cal-dst/lunar.el) ----------
// NOTE: GNU Emacs computes moon phases with a full periodic (Meeus) series,
// accurate to a minute. This is a mean-synodic-month approximation off a known
// new-moon epoch: phase *dates* are usually right but can slip a day near a
// phase boundary. Marked PARTIAL.

/// The four principal moon phases occurring within `(year, month)`, each as
/// `(date, phase-name)`, using the mean synodic month. Approximate.
pub fn lunar_phases_in_month(year: i32, month: u32) -> Vec<(Date, &'static str)> {
    const SYNODIC: f64 = 29.530_588_861;
    // R.D. (UTC) of a known new moon: 2000-01-06 18:14 UTC.
    const NEW_MOON_REF: f64 = 730_125.0 + (18.0 * 60.0 + 14.0) / 1440.0;
    const NAMES: [&str; 4] = ["New Moon", "First Quarter", "Full Moon", "Last Quarter"];
    let start = rd(Date::new(year, month, 1)) as f64;
    let end = start + days_in_month(year, month) as f64;
    let k0 = ((start - NEW_MOON_REF) / SYNODIC).floor() as i64 - 1;
    let mut out: Vec<(f64, Date, &'static str)> = Vec::new();
    for k in k0..=k0 + 3 {
        for (i, frac) in [0.0f64, 0.25, 0.5, 0.75].iter().enumerate() {
            let t = NEW_MOON_REF + SYNODIC * (k as f64 + frac);
            if t >= start && t < end {
                out.push((t, from_rd(t.floor() as i64), NAMES[i]));
            }
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    out.into_iter().map(|(_, d, n)| (d, n)).collect()
}

// --- Sunrise / sunset (Wikipedia "sunrise equation"; cal-dst.el / solar.el) -
// NOTE: Emacs's solar.el uses a fuller model and the user's configured
// `calendar-latitude`/`calendar-longitude`/`calendar-time-zone`. This is the
// standard low-precision sunrise equation; times can differ from Emacs by a few
// minutes and it needs a location. Marked PARTIAL.

/// Sunrise and sunset for date `d` at `lat_deg`/`lon_deg` (east positive), as
/// fractional hours in UTC. Returns `None` on polar day/night (sun never
/// crosses the horizon that day). Approximate.
pub fn sunrise_sunset_utc(d: Date, lat_deg: f64, lon_deg: f64) -> Option<(f64, f64)> {
    let rad = std::f64::consts::PI / 180.0;
    // Julian day at the given date (noon), then current Julian date.
    let jd = julian_day(d) as f64;
    let n = (jd - 2451545.0 + 0.0008).ceil();
    let j_star = n - lon_deg / 360.0;
    let m = (357.5291 + 0.98560028 * j_star).rem_euclid(360.0);
    let c = 1.9148 * (m * rad).sin() + 0.02 * (2.0 * m * rad).sin() + 0.0003 * (3.0 * m * rad).sin();
    let lambda = (m + c + 180.0 + 102.9372).rem_euclid(360.0);
    let j_transit =
        2451545.0 + j_star + 0.0053 * (m * rad).sin() - 0.0069 * (2.0 * lambda * rad).sin();
    let sin_decl = (lambda * rad).sin() * (23.44 * rad).sin();
    let decl = sin_decl.asin();
    let cos_omega =
        ((-0.833 * rad).sin() - (lat_deg * rad).sin() * sin_decl) / ((lat_deg * rad).cos() * decl.cos());
    if !(-1.0..=1.0).contains(&cos_omega) {
        return None; // polar day or night
    }
    let omega = cos_omega.acos() / rad; // degrees
    let j_rise = j_transit - omega / 360.0;
    let j_set = j_transit + omega / 360.0;
    // Convert a Julian date to UTC fractional hours.
    let to_hours = |j: f64| ((j + 0.5).fract() * 24.0).rem_euclid(24.0);
    Some((to_hours(j_rise), to_hours(j_set)))
}

/// Format fractional `hours` (0..24) as `"HH:MM"`.
pub fn format_hm(hours: f64) -> String {
    let total = (hours * 60.0).round() as i64;
    let (h, m) = (total.div_euclid(60).rem_euclid(24), total.rem_euclid(60));
    format!("{h:02}:{m:02}")
}

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
        assert_eq!(
            add_months(Date::new(2026, 1, 31), 1),
            Date::new(2026, 2, 28)
        );
        assert_eq!(
            add_months(Date::new(2024, 1, 31), 1),
            Date::new(2024, 2, 29)
        );
        assert_eq!(
            add_months(Date::new(2026, 12, 15), 1),
            Date::new(2027, 1, 15)
        );
        assert_eq!(
            add_months(Date::new(2026, 1, 15), -1),
            Date::new(2025, 12, 15)
        );
    }

    #[test]
    fn week_bounds_and_counts() {
        // 2026-07-02 is a Thursday; its week runs Sun 06-28 .. Sat 07-04.
        assert_eq!(
            beginning_of_week(Date::new(2026, 7, 2)),
            Date::new(2026, 6, 28)
        );
        assert_eq!(end_of_week(Date::new(2026, 7, 2)), Date::new(2026, 7, 4));
        assert_eq!(count_days(Date::new(2026, 7, 1), Date::new(2026, 7, 1)), 1);
        assert_eq!(
            count_days(Date::new(2026, 7, 1), Date::new(2026, 7, 10)),
            10
        );
    }

    #[test]
    fn day_of_year_works() {
        assert_eq!(day_of_year(Date::new(2026, 1, 1)), 1);
        assert_eq!(day_of_year(Date::new(2024, 12, 31)), 366); // leap
        assert_eq!(day_of_year(Date::new(2023, 12, 31)), 365);
    }

    #[test]
    fn month_and_year_bounds() {
        assert_eq!(
            beginning_of_month(Date::new(2024, 2, 15)),
            Date::new(2024, 2, 1)
        );
        assert_eq!(end_of_month(Date::new(2024, 2, 15)), Date::new(2024, 2, 29)); // leap Feb
        assert_eq!(end_of_month(Date::new(2023, 2, 15)), Date::new(2023, 2, 28));
        assert_eq!(
            beginning_of_year(Date::new(2026, 7, 2)),
            Date::new(2026, 1, 1)
        );
        assert_eq!(end_of_year(Date::new(2026, 7, 2)), Date::new(2026, 12, 31));
    }

    #[test]
    fn julian_day_number() {
        assert_eq!(julian_day(Date::new(1970, 1, 1)), 2440588);
        assert_eq!(julian_day(Date::new(2000, 1, 1)), 2451545);
    }

    #[test]
    fn nth_and_last_weekday() {
        // July 2026: Jul 1 is a Wednesday. The 1st Monday is Jul 6.
        assert_eq!(weekday(Date::new(2026, 7, 1)), 3);
        assert_eq!(nth_weekday(2026, 7, 1, 1), 6);
        // 2026 MLK Day = 3rd Monday of January = Jan 19.
        assert_eq!(nth_weekday(2026, 1, 1, 3), 19);
        // 2026 Thanksgiving = 4th Thursday of November = Nov 26.
        assert_eq!(nth_weekday(2026, 11, 4, 4), 26);
        // 2026 Memorial Day = last Monday of May = May 25.
        assert_eq!(last_weekday(2026, 5, 1), 25);
        // Last Friday of Feb 2024 (leap) = Feb 23.
        assert_eq!(last_weekday(2024, 2, 5), 23);
    }

    #[test]
    fn holidays_fixed_and_floating() {
        let jul = holidays(2026, 7);
        assert!(jul.contains(&(4, "Independence Day")));
        let dec = holidays(2026, 12);
        assert!(dec.contains(&(25, "Christmas")));
        assert!(dec.contains(&(31, "New Year's Eve")));
        // Floating holidays land on the right days in 2026.
        assert!(holidays(2026, 11).contains(&(26, "Thanksgiving")));
        assert!(holidays(2026, 5).contains(&(25, "Memorial Day")));
        assert!(holidays(2026, 1).contains(&(19, "Martin Luther King Jr. Day")));
        // Output is sorted by day, and February has three holidays in 2026.
        let feb = holidays(2026, 2);
        assert!(feb.windows(2).all(|w| w[0].0 <= w[1].0));
        assert_eq!(feb.len(), 3); // Groundhog, Valentine, Presidents' Day
    }

    #[test]
    fn holiday_on_a_date() {
        assert_eq!(holiday_on(Date::new(2026, 7, 4)), Some("Independence Day"));
        assert_eq!(holiday_on(Date::new(2026, 12, 25)), Some("Christmas"));
        assert_eq!(holiday_on(Date::new(2026, 7, 5)), None);
    }

    #[test]
    fn parse_ymd_forms() {
        assert_eq!(parse_ymd("2026/7/4"), Some(Date::new(2026, 7, 4)));
        assert_eq!(parse_ymd("2026-12-25"), Some(Date::new(2026, 12, 25)));
        assert_eq!(parse_ymd("  2024 2 29 "), Some(Date::new(2024, 2, 29)));
        // Invalid: Feb 29 in a non-leap year, bad month, wrong arity.
        assert_eq!(parse_ymd("2023/2/29"), None);
        assert_eq!(parse_ymd("2026/13/1"), None);
        assert_eq!(parse_ymd("2026/7"), None);
        assert_eq!(parse_ymd("nonsense"), None);
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

#[cfg(test)]
mod other_calendar_tests {
    use super::*;

    #[test]
    fn rd_epoch_offsets() {
        assert_eq!(rd(Date::new(1, 1, 1)), 1);
        assert_eq!(rd(Date::new(1970, 1, 1)), 719163);
        assert_eq!(rd(Date::new(2000, 1, 1)), 730120);
        assert_eq!(from_rd(730120), Date::new(2000, 1, 1));
        assert_eq!(islamic_epoch(), 227015);
        assert_eq!(persian_epoch(), 226896);
        assert_eq!(french_epoch(), 654415);
        assert_eq!(fixed_from_julian(1, 1, 1), -1); // Julian epoch
    }

    #[test]
    fn julian_known() {
        // 2000-01-01 Gregorian = 1999-12-19 Julian (13 days behind).
        assert_eq!(julian_from_fixed(rd(Date::new(2000, 1, 1))), (1999, 12, 19));
        assert_eq!(julian_string(Date::new(2000, 1, 1)), "December 19, 1999");
        // Round-trip across a range.
        for f in 700000..700400 {
            let (y, m, d) = julian_from_fixed(f);
            assert_eq!(fixed_from_julian(y, m, d), f);
        }
    }

    #[test]
    fn hebrew_known() {
        // 2000-01-01 Gregorian = 23 Teveth 5760.
        assert_eq!(hebrew_from_fixed(rd(Date::new(2000, 1, 1))), (5760, 10, 23));
        assert_eq!(hebrew_string(Date::new(2000, 1, 1)), "Teveth 23, 5760");
        for f in 725000..725730 {
            let (y, m, d) = hebrew_from_fixed(f);
            assert_eq!(fixed_from_hebrew(y, m, d), f);
        }
    }

    #[test]
    fn islamic_known() {
        // 2000-01-01 Gregorian = 24 Ramadan 1420.
        assert_eq!(
            islamic_from_fixed(rd(Date::new(2000, 1, 1))),
            Some((1420, 9, 24))
        );
        assert_eq!(
            islamic_string(Date::new(2000, 1, 1)).as_deref(),
            Some("Ramadan 24, 1420")
        );
        for f in 730000..730700 {
            let (y, m, d) = islamic_from_fixed(f).unwrap();
            assert_eq!(fixed_from_islamic(y, m, d), f);
        }
    }

    #[test]
    fn persian_known() {
        // 2000-01-01 Gregorian = 11 Dey 1378.
        assert_eq!(persian_from_fixed(rd(Date::new(2000, 1, 1))), (1378, 10, 11));
        assert_eq!(persian_string(Date::new(2000, 1, 1)), "Dey 11, 1378");
        for f in 725000..725730 {
            let (y, m, d) = persian_from_fixed(f);
            assert_eq!(fixed_from_persian(y, m, d), f);
        }
    }

    #[test]
    fn coptic_ethiopic_roundtrip() {
        // 2000-01-01 Gregorian = 22 Kiyahk 1716 (Coptic).
        assert_eq!(coptic_from_fixed(rd(Date::new(2000, 1, 1))), (1716, 4, 22));
        assert_eq!(coptic_string(Date::new(2000, 1, 1)), "Kiyahk 22, 1716");
        for f in 725000..725730 {
            let (y, m, d) = coptic_from_fixed(f);
            assert_eq!(fixed_from_coptic(y, m, d), f);
            let (y, m, d) = ethiopic_from_fixed(f);
            assert_eq!(fixed_from_ethiopic(y, m, d), f);
        }
    }

    #[test]
    fn french_roundtrip() {
        // 2000-01-01 Gregorian falls in year 208 of the Revolution.
        let (y, _m, _d) = french_from_fixed(rd(Date::new(2000, 1, 1))).unwrap();
        assert_eq!(y, 208);
        for f in (french_epoch() + 1)..(french_epoch() + 4000) {
            let (y, m, d) = french_from_fixed(f).unwrap();
            assert_eq!(fixed_from_french(y, m, d), f);
        }
        assert_eq!(french_from_fixed(french_epoch() - 1), None);
    }

    #[test]
    fn bahai_roundtrip() {
        for f in 725000..725730 {
            let (y, m, d) = bahai_from_fixed(f);
            assert_eq!(fixed_from_bahai(y, m, d), f, "bahai roundtrip at {f}");
        }
    }

    #[test]
    fn mayan_known() {
        // The famous "end of the 13th baktun": 2012-12-21 = 13.0.0.0.0,
        // 4 Ahau 3 Kankin (Goodman-Martinez-Thompson correlation 584283).
        let f = rd(Date::new(2012, 12, 21));
        assert_eq!(mayan_long_count_from_fixed(f), (13, 0, 0, 0, 0));
        assert_eq!(mayan_tzolkin_from_fixed(f), (4, 20)); // 4 Ahau
        assert_eq!(mayan_haab_from_fixed(f), (3, 14)); // 3 Kankin
        assert_eq!(MAYAN_TZOLKIN_NAMES[19], "Ahau");
        assert_eq!(MAYAN_HAAB_MONTHS[13], "Kankin");
        // Long-count round-trip.
        for f in 725000..725400 {
            let (b, k, t, u, kin) = mayan_long_count_from_fixed(f);
            assert_eq!(fixed_from_mayan_long_count(b, k, t, u, kin), f);
        }
    }

    #[test]
    fn astro_and_iso_strings() {
        assert_eq!(astro_day_number(Date::new(2000, 1, 1)), 2451545);
        // 1999-12-31 (Friday) is ISO Day 5 of week 52 of 1999.
        assert_eq!(iso_string(Date::new(1999, 12, 31)), "Day 5 of week 52 of 1999");
    }

    #[test]
    fn iso_reverse() {
        assert_eq!(date_from_iso(1999, 52, 5), Date::new(1999, 12, 31));
        assert_eq!(date_from_iso(2020, 53, 5), Date::new(2021, 1, 1));
        assert_eq!(date_from_iso(2025, 1, 1), Date::new(2024, 12, 30));
        // Round-trip against iso_week over a range.
        for f in 729000..730100 {
            let d = from_rd(f);
            let (y, w, dow) = iso_week(d);
            assert_eq!(date_from_iso(y, w, dow), d);
        }
    }

    #[test]
    fn lunar_phases_approx() {
        // There was a new moon on 2000-01-06; the mean approximation must place
        // a New Moon in January 2000 on the 6th (its reference epoch).
        let jan = lunar_phases_in_month(2000, 1);
        assert!(jan
            .iter()
            .any(|&(d, name)| name == "New Moon" && d == Date::new(2000, 1, 6)));
        // A month yields at most four principal phases, all within the month.
        assert!(jan.len() <= 4);
        assert!(jan.iter().all(|&(d, _)| d.year == 2000 && d.month == 1));
    }

    #[test]
    fn sunrise_sunset_plausible() {
        // New York City on 2000-01-01: sunrise ~07:20 EST (12:20 UTC),
        // sunset ~16:39 EST (21:39 UTC).
        let (rise, set) = sunrise_sunset_utc(Date::new(2000, 1, 1), 40.7128, -74.0060).unwrap();
        assert!(rise < set);
        assert!((12.0..12.7).contains(&rise), "sunrise UTC was {rise}");
        assert!((21.3..22.0).contains(&set), "sunset UTC was {set}");
        assert_eq!(format_hm(12.5), "12:30");
        // The poles have no sunrise at the solstice.
        assert!(sunrise_sunset_utc(Date::new(2000, 12, 21), 85.0, 0.0).is_none());
    }
}
