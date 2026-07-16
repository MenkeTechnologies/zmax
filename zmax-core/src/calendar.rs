//! Pure, dependency-free date arithmetic backing the Calendar substrate (the
//! zmax port of GNU Emacs `calendar-mode`). The term-crate Component holds a
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
    pub const fn new(year: i32, month: u32, day: u32) -> Date {
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
// "Other calendars" — the zmax port of GNU Emacs cal-julian / cal-hebrew /
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
    let y = if year < 0 {
        year as i64 + 1
    } else {
        year as i64
    };
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
    "Tut",
    "Babah",
    "Hatur",
    "Kiyahk",
    "Tubah",
    "Amshir",
    "Baramhat",
    "Baramundah",
    "Bashans",
    "Baunah",
    "Abib",
    "Misra",
    "Nasi",
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
    // The three postponement (dehiyyah) conditions each push the New Year one
    // day later; kept as a single OR so the value-equal branches don't repeat.
    let day = if parts >= 19440
        || (day % 7 == 2 && parts >= 9924 && !hebrew_leap(year))
        || (day % 7 == 1 && parts >= 16789 && hebrew_leap(year - 1))
    {
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
    let start = if f < fixed_from_hebrew(year, 1, 1) {
        7
    } else {
        1
    };
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
    "Muharram",
    "Safar",
    "Rabi I",
    "Rabi II",
    "Jumada I",
    "Jumada II",
    "Rajab",
    "Sha'ban",
    "Ramadan",
    "Shawwal",
    "Dhu al-Qada",
    "Dhu al-Hijjah",
];

/// R.D. of the Islamic epoch (Julian 16 July 622).
fn islamic_epoch() -> i64 {
    fixed_from_julian(622, 7, 16)
}

pub fn islamic_leap(year: i64) -> bool {
    matches!(
        year.rem_euclid(30),
        2 | 5 | 7 | 10 | 13 | 16 | 18 | 21 | 24 | 26 | 29
    )
}
pub fn islamic_last_day_of_month(month: u32, year: i64) -> u32 {
    if month % 2 == 1 || (month == 12 && islamic_leap(year)) {
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
    } else if month < 12 || persian_leap(year) {
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
    "Baha",
    "Jalal",
    "Jamal",
    "'Azamat",
    "Nur",
    "Rahmat",
    "Kalimat",
    "Kamal",
    "Asma'",
    "'Izzat",
    "Mashiyyat",
    "'Ilm",
    "Qudrat",
    "Qawl",
    "Masa'il",
    "Sharaf",
    "Sultan",
    "Mulk",
    "'Ala'",
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
    baktun * 144000 + katun * 7200 + tun * 360 + uinal * 20 + kin - MAYAN_DAYS_BEFORE_ABSOLUTE_ZERO
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

/// The next (`forward`) or previous R.D. day, strictly past `from`, whose Mayan
/// haab is `target = (day 0..=19, month 1..=19)` — Emacs `calendar-mayan-next-haab-
/// date`/`-previous-haab-date`. The haab cycle is 365 days, so a valid target is
/// found within one cycle; an unreachable target returns `from`.
pub fn mayan_next_haab(from: i64, target: (i64, u32), forward: bool) -> i64 {
    let step = if forward { 1 } else { -1 };
    let mut f = from + step;
    for _ in 0..365 {
        if mayan_haab_from_fixed(f) == target {
            return f;
        }
        f += step;
    }
    from
}

/// The next/previous R.D. day past `from` whose Mayan tzolkin is `target =
/// (number 1..=13, name 1..=20)` — cycle length 260.
pub fn mayan_next_tzolkin(from: i64, target: (i64, u32), forward: bool) -> i64 {
    let step = if forward { 1 } else { -1 };
    let mut f = from + step;
    for _ in 0..260 {
        if mayan_tzolkin_from_fixed(f) == target {
            return f;
        }
        f += step;
    }
    from
}

/// The next/previous R.D. day past `from` matching both a haab and a tzolkin — the
/// Mayan "calendar round", which repeats every 18980 days (52 years). Returns
/// `from` if the (haab, tzolkin) pair never co-occurs.
pub fn mayan_next_round(from: i64, haab: (i64, u32), tzolkin: (i64, u32), forward: bool) -> i64 {
    let step = if forward { 1 } else { -1 };
    let mut f = from + step;
    for _ in 0..18980 {
        if mayan_haab_from_fixed(f) == haab && mayan_tzolkin_from_fixed(f) == tzolkin {
            return f;
        }
        f += step;
    }
    from
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
    let c =
        1.9148 * (m * rad).sin() + 0.02 * (2.0 * m * rad).sin() + 0.0003 * (3.0 * m * rad).sin();
    let lambda = (m + c + 180.0 + 102.9372).rem_euclid(360.0);
    let j_transit =
        2451545.0 + j_star + 0.0053 * (m * rad).sin() - 0.0069 * (2.0 * lambda * rad).sin();
    let sin_decl = (lambda * rad).sin() * (23.44 * rad).sin();
    let decl = sin_decl.asin();
    let cos_omega = ((-0.833 * rad).sin() - (lat_deg * rad).sin() * sin_decl)
        / ((lat_deg * rad).cos() * decl.cos());
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

// ===========================================================================
// Astronomy — the true solar longitude and true new moon, ported from GNU
// Emacs 30.2's `solar.el` (`solar-longitude`, `solar-date-next-longitude`,
// `solar-ephemeris-correction`, `solar-data-list`) and `lunar.el`
// (`lunar-new-moon-time`, `lunar-new-moon-on-or-after`).
//
// These are the two astronomical primitives `cal-china.el` is built on: a
// Chinese month begins at the new moon, and the solar terms (zodiac signs) are
// the instants the sun's apparent longitude crosses a multiple of 30°. The
// mean-lunation approximation used by `lunar_phases_in_month` above is not good
// enough for that — a half-day error moves a month boundary by a whole day — so
// the Chinese calendar uses these instead.
//
// Times are "astronomical (Julian) day numbers": R.D. + 1721424.5, exactly what
// Emacs's `calendar-astro-from-absolute` produces, and moments are fractional.
// ===========================================================================

/// Emacs `calendar-astro-from-absolute` (cal-julian.el): the astronomical
/// (Julian) day number of the R.D. moment `f`.
pub fn astro_from_rd(f: f64) -> f64 {
    f + 1_721_424.5
}

/// Emacs `calendar-astro-to-absolute`: the R.D. moment of an astronomical
/// (Julian) day number.
pub fn rd_from_astro(d: f64) -> f64 {
    d - 1_721_424.5
}

fn sin_deg(d: f64) -> f64 {
    d.to_radians().sin()
}

/// The Gregorian year the R.D. moment `f` falls in.
fn year_of_rd(f: f64) -> i32 {
    from_rd(f.floor() as i64).year
}

/// Emacs `solar-ephemeris-correction`: Ephemeris Time minus Universal Time
/// during Gregorian `year`, in days.
pub fn ephemeris_correction(year: i32) -> f64 {
    // Julian centuries from 1900-01-01 to July 1 of `year` (the 1721424.5 offset
    // Emacs applies to both endpoints cancels in the difference).
    let theta = || (rd(Date::new(year, 7, 1)) - rd(Date::new(1900, 1, 1))) as f64 / 36525.0;
    if (1988..2020).contains(&year) {
        (year as f64 - 2000.0 + 67.0) / 60.0 / 60.0 / 24.0
    } else if (1900..1988).contains(&year) {
        let t = theta();
        let (t2, t3, t4, t5) = (t * t, t * t * t, t.powi(4), t.powi(5));
        -0.00002
            + 0.000297 * t
            + 0.025184 * t2
            + -0.181133 * t3
            + 0.553040 * t4
            + -0.861938 * t5
            + 0.677066 * t3 * t3
            + -0.212591 * t4 * t3
    } else if (1800..1900).contains(&year) {
        let t = theta();
        let (t2, t3, t4, t5) = (t * t, t * t * t, t.powi(4), t.powi(5));
        -0.000009
            + 0.003844 * t
            + 0.083563 * t2
            + 0.865736 * t3
            + 4.867575 * t4
            + 15.845535 * t5
            + 31.332267 * t3 * t3
            + 38.291999 * t4 * t3
            + 28.316289 * t4 * t4
            + 11.636204 * t4 * t5
            + 2.043794 * t5 * t5
    } else if (1620..1800).contains(&year) {
        let x = (year - 1600) as f64 / 10.0;
        (2.19167 * x * x - 40.675 * x + 196.58333) / 60.0 / 60.0 / 24.0
    } else {
        let tmp = astro_from_rd(rd(Date::new(year, 1, 1)) as f64) - 2_382_148.0;
        let second = tmp * tmp / 41_048_480.0 - 15.0;
        second / 60.0 / 60.0 / 24.0
    }
}

/// The periodic terms of the sun's longitude (Emacs `solar-data-list`):
/// `(amplitude, phase, frequency)`, phases and frequencies in radians.
const SOLAR_DATA: [(f64, f64, f64); 49] = [
    (403406.0, 4.721964, 1.621043),
    (195207.0, 5.937458, 62830.348067),
    (119433.0, 1.115589, 62830.821524),
    (112392.0, 5.781616, 62829.634302),
    (3891.0, 5.5474, 125660.5691),
    (2819.0, 1.5120, 125660.984),
    (1721.0, 4.1897, 62832.4766),
    (0.0, 1.163, 0.813),
    (660.0, 5.415, 125659.31),
    (350.0, 4.315, 57533.85),
    (334.0, 4.553, -33.931),
    (314.0, 5.198, 777137.715),
    (268.0, 5.989, 78604.191),
    (242.0, 2.911, 5.412),
    (234.0, 1.423, 39302.098),
    (158.0, 0.061, -34.861),
    (132.0, 2.317, 115067.698),
    (129.0, 3.193, 15774.337),
    (114.0, 2.828, 5296.670),
    (99.0, 0.52, 58849.27),
    (93.0, 4.65, 5296.11),
    (86.0, 4.35, -3980.70),
    (78.0, 2.75, 52237.69),
    (72.0, 4.50, 55076.47),
    (68.0, 3.23, 261.08),
    (64.0, 1.22, 15773.85),
    (46.0, 0.14, 188491.03),
    (38.0, 3.44, -7756.55),
    (37.0, 4.37, 264.89),
    (32.0, 1.14, 117906.27),
    (29.0, 2.84, 55075.75),
    (28.0, 5.96, -7961.39),
    (27.0, 5.09, 188489.81),
    (27.0, 1.72, 2132.19),
    (25.0, 2.56, 109771.03),
    (24.0, 1.92, 54868.56),
    (21.0, 0.09, 25443.93),
    (21.0, 5.98, -55731.43),
    (20.0, 4.03, 60697.74),
    (18.0, 4.47, 2132.79),
    (17.0, 0.79, 109771.63),
    (14.0, 4.24, -7752.82),
    (13.0, 2.01, 188491.91),
    (13.0, 2.65, 207.81),
    (13.0, 4.98, 29424.63),
    (12.0, 0.93, -7.99),
    (10.0, 2.21, 46941.14),
    (10.0, 3.59, -68.29),
    (10.0, 1.50, 21463.25),
];

/// Emacs `solar-longitude`: the sun's apparent longitude, in degrees, at the
/// astronomical day number `d`, read in the local time of a zone `tz_minutes`
/// east of UTC. Accurate to about 0.0006° (≈ 1 minute of time).
pub fn solar_longitude(d: f64, tz_minutes: f64) -> f64 {
    // Universal Time (no daylight saving: the Chinese calendrical authorities do
    // not use it, and `calendar-chinese-daylight-time-offset` is 0).
    let mut date = astro_from_rd(rd_from_astro(d) - tz_minutes / 60.0 / 24.0);
    // Ephemeris Time.
    date += ephemeris_correction(year_of_rd(rd_from_astro(date)));
    let u = (date - 2_451_545.0) / 3_652_500.0;
    let tau = std::f64::consts::TAU;
    let sum: f64 = SOLAR_DATA
        .iter()
        .map(|&(x, y, z)| x * (y + z * u).rem_euclid(tau).sin())
        .sum();
    let longitude = 4.9353929 + 62833.1961680 * u + 0.0000001 * sum;
    let aberration = 0.0000001 * (17.0 * (3.10 + 62830.14 * u).cos() - 973.0);
    let a1 = (2.18 + u * (-3375.70 + 0.36 * u)).rem_euclid(tau);
    let a2 = (3.51 + u * (125666.39 + 0.10 * u)).rem_euclid(tau);
    let nutation = -0.0000001 * (834.0 * a1.sin() + 64.0 * a2.sin());
    (longitude + aberration + nutation)
        .to_degrees()
        .rem_euclid(360.0)
}

/// Emacs `solar-date-next-longitude`: the first moment on or after the
/// astronomical day number `d` at which the sun's longitude is a multiple of
/// `l` degrees (`l` must divide 360). Bisection to the nearest minute, exactly
/// as Emacs does it.
pub fn solar_date_next_longitude(d: f64, l: f64, tz_minutes: f64) -> f64 {
    let mut start = d;
    let mut end = d + (l / 360.0) * 400.0;
    // The next multiple of `l` the longitude will reach (0 = the 360° wrap).
    let next = (l * ((solar_longitude(d, tz_minutes) / l).floor() + 1.0)).rem_euclid(360.0);
    while 0.00001 < end - start {
        let mid = (start + end) / 2.0;
        let long = solar_longitude(mid, tz_minutes);
        // Before the crossing when the longitude has not yet reached `next` — or,
        // at the wrap, while it is still past `l`.
        if (next != 0.0 && long < next) || (next == 0.0 && l < long) {
            start = mid;
        } else {
            end = mid;
        }
    }
    (start + end) / 2.0
}

/// Mean lunations per 365.25-day year (Emacs `lunar-cycles-per-year`).
const LUNAR_CYCLES_PER_YEAR: f64 = 12.3685;

/// Emacs `lunar-new-moon-time`: the astronomical day number of the `k`th new
/// moon counted from the new moon of January 2000, in the local time of a zone
/// `tz_minutes` east of UTC. Meeus's periodic series, as Emacs codes it.
pub fn lunar_new_moon_time(k: f64, tz_minutes: f64) -> f64 {
    let t = k / 1236.85;
    let (t2, t3, t4) = (t * t, t * t * t, t.powi(4));
    let jde = 2_451_550.097_65 + 29.530588853 * k + 0.0001337 * t2 - 0.000000150 * t3
        + 0.00000000073 * t4;
    let e = 1.0 - 0.002516 * t - 0.0000074 * t2;
    let sun = 2.5534 + 29.10535669 * k - 0.0000218 * t2 - 0.00000011 * t3;
    let moon = 201.5643 + 385.81693528 * k + 0.0107438 * t2 + 0.00001239 * t3 - 0.000000058 * t4;
    let arg = 160.7108 + 390.67050274 * k - 0.0016341 * t2 - 0.00000227 * t3 + 0.000000011 * t4;
    let omega = 124.7746 - 1.56375580 * k + 0.0020691 * t2 + 0.00000215 * t3;
    let correction = -0.40720 * sin_deg(moon)
        + 0.17241 * e * sin_deg(sun)
        + 0.01608 * sin_deg(2.0 * moon)
        + 0.01039 * sin_deg(2.0 * arg)
        + 0.00739 * e * sin_deg(moon - sun)
        + -0.00514 * e * sin_deg(moon + sun)
        + 0.00208 * e * e * sin_deg(2.0 * sun)
        + -0.00111 * sin_deg(moon - 2.0 * arg)
        + -0.00057 * sin_deg(moon + 2.0 * arg)
        + 0.00056 * e * sin_deg(2.0 * moon + sun)
        + -0.00042 * sin_deg(3.0 * moon)
        + 0.00042 * e * sin_deg(sun + 2.0 * arg)
        + 0.00038 * e * sin_deg(sun - 2.0 * arg)
        + -0.00024 * e * sin_deg(2.0 * moon - sun)
        + -0.00017 * sin_deg(omega)
        + -0.00007 * sin_deg(moon + 2.0 * sun)
        + 0.00004 * sin_deg(2.0 * moon - 2.0 * arg)
        + 0.00004 * sin_deg(3.0 * sun)
        + 0.00003 * sin_deg(moon + sun - 2.0 * arg)
        + 0.00003 * sin_deg(2.0 * moon + 2.0 * arg)
        + -0.00003 * sin_deg(moon + sun + 2.0 * arg)
        + 0.00003 * sin_deg(moon - sun + 2.0 * arg)
        + -0.00002 * sin_deg(moon - sun - 2.0 * arg)
        + -0.00002 * sin_deg(3.0 * moon + sun)
        + 0.00002 * sin_deg(4.0 * moon);
    // The 14 "additional" planetary/long-period corrections (Emacs A1..A14).
    let additional = 0.000325 * sin_deg(299.77 + 0.107408 * k - 0.009173 * t2)
        + 0.000165 * sin_deg(251.88 + 0.016321 * k)
        + 0.000164 * sin_deg(251.83 + 26.641886 * k)
        + 0.000126 * sin_deg(349.42 + 36.412478 * k)
        + 0.000110 * sin_deg(84.66 + 18.206239 * k)
        + 0.000062 * sin_deg(141.74 + 53.303771 * k)
        + 0.000060 * sin_deg(207.14 + 2.453732 * k)
        + 0.000056 * sin_deg(154.84 + 7.306860 * k)
        + 0.000047 * sin_deg(34.52 + 27.261239 * k)
        + 0.000042 * sin_deg(207.19 + 0.121824 * k)
        + 0.000040 * sin_deg(291.34 + 1.844379 * k)
        + 0.000037 * sin_deg(161.72 + 24.198154 * k)
        + 0.000035 * sin_deg(239.56 + 25.513099 * k)
        + 0.000023 * sin_deg(331.55 + 3.592518 * k);
    let new_jde = jde + correction + additional;
    new_jde - ephemeris_correction(year_of_rd(rd_from_astro(new_jde))) + tz_minutes / 60.0 / 24.0
}

/// Emacs `lunar-new-moon-on-or-after`: the astronomical day number of the first
/// new moon at or after the moment `d`, in the local time of a zone
/// `tz_minutes` east of UTC.
pub fn lunar_new_moon_on_or_after(d: f64, tz_minutes: f64) -> f64 {
    let date = from_rd(rd_from_astro(d).floor() as i64);
    let year = date.year as f64 + day_of_year(date) as f64 / 365.25;
    let mut k = ((year - 2000.0) * LUNAR_CYCLES_PER_YEAR).floor();
    let mut moon = lunar_new_moon_time(k, tz_minutes);
    while moon < d {
        k += 1.0;
        moon = lunar_new_moon_time(k, tz_minutes);
    }
    moon
}

// ===========================================================================
// Chinese calendar — the zmax port of GNU Emacs 30.2's `cal-china.el`
// (Reingold's implementation of Baolin Liu's rules, the calendar as revised at
// the start of the Qing dynasty in 1644).
//
// A Chinese month runs from one new moon to the next, in Beijing local time. A
// year has 12 or 13 of them; the leap month is the one that carries no "major
// solar term" (zodiac-sign crossing). The months of one year are computed as a
// block (`chinese_year`) between two winter solstices, exactly as Emacs does,
// because a month's number depends on where the solstice falls in the sequence.
// ===========================================================================

/// The ten celestial stems (Emacs `calendar-chinese-celestial-stem`).
pub const CHINESE_CELESTIAL_STEM: [&str; 10] = [
    "Jia", "Yi", "Bing", "Ding", "Wu", "Ji", "Geng", "Xin", "Ren", "Gui",
];

/// The twelve terrestrial branches (Emacs `calendar-chinese-terrestrial-branch`).
pub const CHINESE_TERRESTRIAL_BRANCH: [&str; 12] = [
    "Zi", "Chou", "Yin", "Mao", "Chen", "Si", "Wu", "Wei", "Shen", "You", "Xu", "Hai",
];

/// The Chinese month names a diary entry is dated with (Emacs
/// `calendar-chinese-month-name-array`).
pub const CHINESE_MONTH_NAMES: [&str; 12] = [
    "正月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "冬月", "臘月",
];

/// The `n`th name of the 60-name sexagesimal cycle (Emacs
/// `calendar-chinese-sexagesimal-name`): stem-branch, e.g. `"Jia-Zi"`.
pub fn chinese_sexagesimal_name(n: i64) -> String {
    format!(
        "{}-{}",
        CHINESE_CELESTIAL_STEM[(n - 1).rem_euclid(10) as usize],
        CHINESE_TERRESTRIAL_BRANCH[(n - 1).rem_euclid(12) as usize]
    )
}

/// Minutes east of UTC that Beijing keeps for calendrical purposes (Emacs
/// `calendar-chinese-time-zone`): UT+7:45:40 before 1928, UT+8 after.
fn chinese_time_zone(year: i32) -> f64 {
    if year < 1928 {
        465.0 + 40.0 / 60.0
    } else {
        480.0
    }
}

/// Emacs `calendar-chinese-zodiac-sign-on-or-after`: R.D. of the first day on or
/// after `d` on which the sun's longitude reaches a multiple of 30° (a "major
/// solar term"), in Beijing time.
pub fn chinese_zodiac_sign_on_or_after(d: i64) -> i64 {
    let tz = chinese_time_zone(from_rd(d).year);
    rd_from_astro(solar_date_next_longitude(astro_from_rd(d as f64), 30.0, tz)).floor() as i64
}

/// Emacs `calendar-chinese-new-moon-on-or-after`: R.D. of the first new moon on
/// or after `d`, in Beijing time — the first day of a Chinese month.
pub fn chinese_new_moon_on_or_after(d: i64) -> i64 {
    let tz = chinese_time_zone(from_rd(d).year);
    rd_from_astro(lunar_new_moon_on_or_after(astro_from_rd(d as f64), tz)).floor() as i64
}

/// Emacs `calendar-chinese-month-list`: the R.D. start days of the Chinese
/// months beginning in `start..=end`.
fn chinese_month_list(start: i64, end: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let mut d = start;
    while d <= end {
        let new_moon = chinese_new_moon_on_or_after(d);
        if new_moon > end {
            break;
        }
        out.push(new_moon);
        d = new_moon + 1;
    }
    out
}

/// Emacs `calendar-chinese-number-months`: number the months in `list`
/// sequentially from `start`, giving a leap month the half number of the month
/// it follows. A month is a leap month when it contains no zodiac-sign crossing
/// — i.e. when the next month starts on or before the next crossing. The first
/// and last months of the list are never leap months.
fn chinese_number_months(list: &[i64], start: f64) -> Vec<(f64, i64)> {
    let mut out = Vec::new();
    let mut rest = list;
    let mut n = start;
    while let Some(&first) = rest.first() {
        out.push((n, first));
        // Too few months left for a leap month: number them straight through.
        let leap_possible = 12.0 - n - rest.len() as f64 != 0.0;
        if leap_possible && rest.len() >= 3 && rest[2] <= chinese_zodiac_sign_on_or_after(rest[1]) {
            out.push((n + 0.5, rest[1]));
            rest = &rest[2..];
        } else {
            rest = &rest[1..];
        }
        n += 1.0;
    }
    out
}

/// Emacs `calendar-chinese-compute-year`: the months of the Chinese year that
/// sits inside Gregorian year `y`, as `(month-number, R.D. of its first day)`
/// pairs running from the month after the solstice of `y-1` to the month of the
/// solstice of `y`. A `.5` month number is a leap month.
fn chinese_compute_year(y: i32) -> Vec<(f64, i64)> {
    let next_solstice = chinese_zodiac_sign_on_or_after(rd(Date::new(y, 12, 15)));
    let list = chinese_month_list(
        1 + chinese_zodiac_sign_on_or_after(rd(Date::new(y - 1, 12, 15))),
        next_solstice,
    );
    let next_sign = chinese_zodiac_sign_on_or_after(list[0]);
    let mut out = Vec::new();
    if list.len() == 12 {
        // No room for a leap month: 12, 1, 2, …, 11.
        out.push((12.0, list[0]));
        out.extend(chinese_number_months(&list[1..], 1.0));
    } else if list[0] > next_sign || next_sign >= list[1] {
        // The first month of the list is a leap month, the second is not.
        out.push((11.5, list[0]));
        out.push((12.0, list[1]));
        out.extend(chinese_number_months(&list[2..], 1.0));
    } else {
        out.push((12.0, list[0]));
        if chinese_zodiac_sign_on_or_after(list[1]) >= list[2] {
            // The second month of the list is a leap month.
            out.push((12.5, list[1]));
            out.extend(chinese_number_months(&list[2..], 1.0));
        } else {
            out.extend(chinese_number_months(&list[1..], 1.0));
        }
    }
    out
}

thread_local! {
    /// Emacs caches each computed year in `calendar-chinese-year-cache`; the
    /// month structure costs a few dozen bisections of the solar longitude, and
    /// every date conversion needs three years of it.
    static CHINESE_YEAR_CACHE: std::cell::RefCell<std::collections::HashMap<i32, Vec<(f64, i64)>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Emacs `calendar-chinese-year`: the month structure of the Chinese year inside
/// Gregorian year `y` (cached).
pub fn chinese_year(y: i32) -> Vec<(f64, i64)> {
    if let Some(hit) = CHINESE_YEAR_CACHE.with(|c| c.borrow().get(&y).cloned()) {
        return hit;
    }
    let computed = chinese_compute_year(y);
    CHINESE_YEAR_CACHE.with(|c| c.borrow_mut().insert(y, computed.clone()));
    computed
}

/// A date on the Chinese calendar: the 60-year `cycle`, the `year` within it
/// (1..=60), the `month` (1..=12, `leap` marking the second month of that
/// number) and the `day` of the month.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChineseDate {
    pub cycle: i64,
    pub year: i64,
    pub month: u32,
    pub leap: bool,
    pub day: u32,
}

impl ChineseDate {
    pub const fn new(cycle: i64, year: i64, month: u32, leap: bool, day: u32) -> ChineseDate {
        ChineseDate {
            cycle,
            year,
            month,
            leap,
            day,
        }
    }

    /// The month as Emacs numbers it: `5` for the 5th month, `5.5` for the leap
    /// month that follows it.
    fn month_number(self) -> f64 {
        self.month as f64 + if self.leap { 0.5 } else { 0.0 }
    }
}

/// Emacs `calendar-chinese-from-absolute`: the Chinese date of R.D. day `f`.
pub fn chinese_from_fixed(f: i64) -> ChineseDate {
    let g_year = from_rd(f).year;
    let mut c_year = g_year as i64 + 2695;
    let mut months = chinese_year(g_year - 1);
    months.extend(chinese_year(g_year));
    months.extend(chinese_year(g_year + 1));
    // Walk forward while the *next* month has already begun; crossing a month 1
    // enters the next Chinese year.
    let mut i = 0usize;
    while i + 1 < months.len() && months[i + 1].1 <= f {
        if months[i + 1].0 == 1.0 {
            c_year += 1;
        }
        i += 1;
    }
    let (month, start) = months[i];
    ChineseDate {
        cycle: (c_year - 1).div_euclid(60),
        year: (c_year - 1).rem_euclid(60) + 1,
        month: month.floor() as u32,
        leap: month.fract() != 0.0,
        day: (f - start + 1) as u32,
    }
}

/// Emacs `calendar-chinese-to-absolute`: the R.D. day of a Chinese date, or
/// `None` when that year has no such month (asking for a leap month that does
/// not exist) or the day runs past the month.
pub fn fixed_from_chinese(c: ChineseDate) -> Option<i64> {
    let g_year = ((c.cycle - 1) * 60 + (c.year - 1) - 2636) as i32;
    let this = chinese_year(g_year);
    // The year runs from its month 1 into the head of the next year's structure
    // (which carries months 12 / 12.5 and any leap 11).
    let start_at = this.iter().position(|&(m, _)| m == 1.0)?;
    let next = chinese_year(g_year + 1);
    // Only the months *of this Chinese year*: its 1..11 (with any leap), then the
    // 12th (and any leap 11 or 12) that open the next structure. Emacs looks the
    // month up in the whole of the next year as well, which silently answers with
    // the wrong year's month when asked for one this year does not have; stopping
    // at the year boundary makes that an error instead, and is identical for every
    // month the year really has.
    let months = this[start_at..]
        .iter()
        .chain(next.iter().take_while(|&&(m, _)| m != 1.0));
    let want = c.month_number();
    let start = months.into_iter().find(|&&(m, _)| m == want)?.1;
    if c.day == 0 || c.day > 30 {
        return None;
    }
    Some(start + c.day as i64 - 1)
}

/// Emacs `calendar-chinese-months`: the months of Chinese year `year` of `cycle`,
/// in order, as `(number, is-leap)` — what `calendar-chinese-goto-date` offers
/// for completion and validates the typed month against.
pub fn chinese_months(cycle: i64, year: i64) -> Vec<(u32, bool)> {
    let g_year = ((cycle - 1) * 60 + (year - 1) - 2636) as i32;
    let this = chinese_year(g_year);
    let Some(start_at) = this.iter().position(|&(m, _)| m == 1.0) else {
        return Vec::new();
    };
    let next = chinese_year(g_year + 1);
    // Months 1..11 (with any leap) from this structure, then the tail months
    // (12, and any leap 11 or 12) that open the next one.
    let tail = next.iter().take_while(|&&(m, _)| m != 1.0);
    this[start_at..]
        .iter()
        .chain(tail)
        .map(|&(m, _)| (m.floor() as u32, m.fract() != 0.0))
        .collect()
}

/// The number of days in the Chinese month of `c` (29 or 30).
pub fn chinese_last_day_of_month(c: ChineseDate) -> u32 {
    let Some(first) = fixed_from_chinese(ChineseDate { day: 1, ..c }) else {
        return 0;
    };
    // The next month begins at the next new moon after this month's first day.
    (chinese_new_moon_on_or_after(first + 1) - first) as u32
}

/// Emacs `calendar-chinese-date-string`: the Chinese date of Gregorian `d`, in
/// Emacs's own phrasing — `"Cycle 78, year 43 (Bing-Wu), month 5 (Jia-Wu), day
/// 29 (Wu-Zi)"`, with `first`/`second` distinguishing a leap month from the
/// ordinary month it doubles.
pub fn chinese_string(d: Date) -> String {
    let abs = rd(d);
    let c = chinese_from_fixed(abs);
    // An ordinary month is the "first" of its number when the year also holds the
    // leap month that doubles it.
    let doubled = chinese_months(c.cycle, c.year)
        .iter()
        .any(|&(m, leap)| leap && m == c.month);
    let prefix = if c.leap {
        "second "
    } else if doubled {
        "first "
    } else {
        ""
    };
    // A leap month has no sexagesimal name of its own.
    let month_name = if c.leap {
        String::new()
    } else {
        format!(
            " ({})",
            chinese_sexagesimal_name(12 * c.year + c.month as i64 + 50)
        )
    };
    format!(
        "Cycle {}, year {} ({}), {}month {}{}, day {} ({})",
        c.cycle,
        c.year,
        chinese_sexagesimal_name(c.year),
        prefix,
        c.month,
        month_name,
        c.day,
        chinese_sexagesimal_name(abs + 15)
    )
}

/// The Chinese date of `d` as a *diary entry* spells it — `"二月 15, 7842"`:
/// the month name, the day, and the year Emacs's Chinese diary packs as
/// `cycle * 100 + year` (`calendar-chinese-from-absolute-for-diary`). A leap
/// month is written with the name of the month it doubles, as Emacs does.
pub fn chinese_diary_string(d: Date) -> String {
    let c = chinese_from_fixed(rd(d));
    format!(
        "{} {}, {}",
        CHINESE_MONTH_NAMES[(c.month - 1) as usize],
        c.day,
        c.cycle * 100 + c.year
    )
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
        assert_eq!(
            persian_from_fixed(rd(Date::new(2000, 1, 1))),
            (1378, 10, 11)
        );
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
    fn mayan_next_prev_search() {
        let f = rd(Date::new(2012, 12, 21)); // 4 Ahau 3 Kankin
                                             // The same haab recurs exactly one 365-day cycle later / earlier.
        let haab = mayan_haab_from_fixed(f); // (3, 14)
        assert_eq!(mayan_next_haab(f, haab, true), f + 365);
        assert_eq!(mayan_next_haab(f, haab, false), f - 365);
        // The same tzolkin recurs every 260 days.
        let tz = mayan_tzolkin_from_fixed(f); // (4, 20)
        assert_eq!(mayan_next_tzolkin(f, tz, true), f + 260);
        assert_eq!(mayan_next_tzolkin(f, tz, false), f - 260);
        // Both together = one calendar round (18980 days).
        assert_eq!(mayan_next_round(f, haab, tz, true), f + 18980);
        // Each search lands on a day that really has the target values.
        let nf = mayan_next_haab(f, (5, 3), true);
        assert_eq!(mayan_haab_from_fixed(nf), (5, 3));
    }

    #[test]
    fn astro_and_iso_strings() {
        assert_eq!(astro_day_number(Date::new(2000, 1, 1)), 2451545);
        // 1999-12-31 (Friday) is ISO Day 5 of week 52 of 1999.
        assert_eq!(
            iso_string(Date::new(1999, 12, 31)),
            "Day 5 of week 52 of 1999"
        );
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

#[cfg(test)]
mod chinese_tests {
    use super::*;

    /// Every expected value below was produced by GNU Emacs 30.2 itself
    /// (`calendar-chinese-from-absolute` / `calendar-chinese-date-string` /
    /// `calendar-chinese-year`), so these pin the port to Emacs's answers, not to
    /// our own arithmetic.
    /// `(gregorian, cycle, year, month, leap, day)`.
    const VECTORS: [(Date, i64, i64, u32, bool, u32); 18] = [
        (Date::new(1900, 1, 1), 76, 36, 12, false, 1),
        (Date::new(1924, 2, 5), 77, 1, 1, false, 1), // start of cycle 77
        (Date::new(2026, 7, 13), 78, 43, 5, false, 29),
        (Date::new(2000, 1, 1), 78, 16, 11, false, 25),
        (Date::new(1999, 12, 31), 78, 16, 11, false, 24),
        (Date::new(2026, 2, 17), 78, 43, 1, false, 1), // Chinese New Year 2026
        (Date::new(2025, 6, 25), 78, 42, 6, false, 1), // the "first" 6th month
        (Date::new(2033, 9, 30), 78, 50, 9, false, 8), // Liu's contested 2033
        (Date::new(2025, 1, 29), 78, 42, 1, false, 1),
        (Date::new(2024, 2, 10), 78, 41, 1, false, 1),
        (Date::new(2023, 4, 20), 78, 40, 3, false, 1),
        (Date::new(2023, 3, 22), 78, 40, 2, true, 1), // leap 2nd month of 2023
        (Date::new(2020, 8, 22), 78, 37, 7, false, 4),
        (Date::new(2026, 12, 21), 78, 43, 11, false, 13),
        (Date::new(2050, 1, 1), 79, 6, 12, false, 8),
        (Date::new(1776, 7, 4), 74, 33, 5, false, 19), // before the 1928 zone change
        (Date::new(1990, 5, 5), 78, 7, 4, false, 11),
        (Date::new(1949, 10, 1), 77, 26, 8, false, 10),
    ];

    #[test]
    fn chinese_from_fixed_matches_emacs() {
        for &(g, cycle, year, month, leap, day) in VECTORS.iter() {
            assert_eq!(
                chinese_from_fixed(rd(g)),
                ChineseDate::new(cycle, year, month, leap, day),
                "chinese date of {g:?}"
            );
        }
    }

    #[test]
    fn fixed_from_chinese_inverts() {
        for &(g, cycle, year, month, leap, day) in VECTORS.iter() {
            assert_eq!(
                fixed_from_chinese(ChineseDate::new(cycle, year, month, leap, day)),
                Some(rd(g)),
                "gregorian of chinese {cycle}/{year}/{month}{}/{day}",
                if leap { "+leap" } else { "" }
            );
        }
    }

    #[test]
    fn round_trip_over_a_decade() {
        // Every day of 2020..2030 converts to a Chinese date and back.
        for f in rd(Date::new(2020, 1, 1))..rd(Date::new(2030, 1, 1)) {
            let c = chinese_from_fixed(f);
            assert_eq!(fixed_from_chinese(c), Some(f), "round trip at R.D. {f}");
            assert!((1..=30).contains(&c.day));
            assert!((1..=12).contains(&c.month));
        }
    }

    #[test]
    fn chinese_new_year_matches_emacs() {
        // `(cadr (assoc 1 (calendar-chinese-year Y)))` in Emacs 30.2.
        const CNY: [(i32, Date); 13] = [
            (1912, Date::new(1912, 2, 18)),
            (1949, Date::new(1949, 1, 29)),
            (1990, Date::new(1990, 1, 27)),
            (2000, Date::new(2000, 2, 5)),
            (2020, Date::new(2020, 1, 25)),
            (2023, Date::new(2023, 1, 22)),
            (2024, Date::new(2024, 2, 10)),
            (2025, Date::new(2025, 1, 29)),
            (2026, Date::new(2026, 2, 17)),
            (2027, Date::new(2027, 2, 6)),
            (2030, Date::new(2030, 2, 3)),
            (2033, Date::new(2033, 1, 31)),
            (2044, Date::new(2044, 1, 30)),
        ];
        for (y, expect) in CNY {
            let new_year = chinese_year(y)
                .into_iter()
                .find(|&(m, _)| m == 1.0)
                .expect("every Chinese year has a month 1");
            assert_eq!(from_rd(new_year.1), expect, "Chinese New Year of {y}");
        }
    }

    #[test]
    fn leap_months_are_where_emacs_puts_them() {
        // Emacs's own `calendar-chinese-year-cache`: 2023 has a leap 2nd month,
        // 2025 a leap 6th, 2020 a leap 4th; 2024 has none.
        let leaps = |y: i32| -> Vec<f64> {
            chinese_year(y)
                .into_iter()
                .map(|(m, _)| m)
                .filter(|m| m.fract() != 0.0)
                .collect()
        };
        assert_eq!(leaps(2023), vec![2.5]);
        assert_eq!(leaps(2025), vec![6.5]);
        assert_eq!(leaps(2020), vec![4.5]);
        assert!(leaps(2024).is_empty());
        // A leap year has 13 months, a common year 12.
        assert_eq!(chinese_year(2023).len(), 13);
        assert_eq!(chinese_year(2024).len(), 12);
    }

    #[test]
    fn date_string_matches_emacs() {
        assert_eq!(
            chinese_string(Date::new(2026, 7, 13)),
            "Cycle 78, year 43 (Bing-Wu), month 5 (Jia-Wu), day 29 (Wu-Zi)"
        );
        // A month doubled by a leap month is announced as the "first" one…
        assert_eq!(
            chinese_string(Date::new(2025, 6, 25)),
            "Cycle 78, year 42 (Yi-Si), first month 6 (Gui-Wei), day 1 (Yi-Chou)"
        );
        // …and the leap month itself as the "second", with no sexagesimal name.
        assert_eq!(
            chinese_string(Date::new(2023, 3, 22)),
            "Cycle 78, year 40 (Gui-Mao), second month 2, day 1 (Ji-Mao)"
        );
        assert_eq!(
            chinese_string(Date::new(1900, 1, 1)),
            "Cycle 76, year 36 (Ji-Hai), month 12 (Ding-Chou), day 1 (Jia-Xu)"
        );
    }

    #[test]
    fn sexagesimal_names() {
        assert_eq!(chinese_sexagesimal_name(1), "Jia-Zi");
        assert_eq!(chinese_sexagesimal_name(60), "Gui-Hai");
        assert_eq!(chinese_sexagesimal_name(61), "Jia-Zi"); // the cycle repeats
    }

    #[test]
    fn months_of_a_year() {
        // Chinese year 42 of cycle 78 (2025) has 13 months: 1..12 with a leap 6.
        let months = chinese_months(78, 42);
        assert_eq!(months.len(), 13);
        assert!(months.contains(&(6, false)) && months.contains(&(6, true)));
        assert!(months.contains(&(12, false)));
        // A common year has exactly 12, none of them leap.
        let months = chinese_months(78, 41); // 2024
        assert_eq!(months.len(), 12);
        assert!(months.iter().all(|&(_, leap)| !leap));
        // Asking for a leap month a year does not have is an error, not a guess.
        assert_eq!(
            fixed_from_chinese(ChineseDate::new(78, 41, 6, true, 1)),
            None
        );
    }

    #[test]
    fn month_lengths_are_lunar() {
        // Every Chinese month is 29 or 30 days long.
        for month in 1..=12 {
            let len = chinese_last_day_of_month(ChineseDate::new(78, 43, month, false, 1));
            assert!((29..=30).contains(&len), "month {month} was {len} days");
        }
    }

    #[test]
    fn astronomy_primitives() {
        // R.D. 730120 is 2000-01-01; astronomical day numbers start at noon, so
        // midnight of that day is 2451544.5 and the J2000 epoch (2451545.0) is its
        // noon.
        assert_eq!(astro_from_rd(730120.0), 2_451_544.5);
        assert_eq!(rd_from_astro(2_451_544.5), 730120.0);
        // The sun crosses 0° (the vernal equinox) around March 20 each year, and
        // 270° (the winter solstice) around December 21.
        let tz = 0.0;
        let equinox =
            solar_date_next_longitude(astro_from_rd(rd(Date::new(2026, 3, 1)) as f64), 30.0, tz);
        let day = from_rd(rd_from_astro(equinox).floor() as i64);
        assert_eq!((day.month, day.day), (3, 20), "2026 vernal equinox");
        // The new moon of 2026-02-17 (Chinese New Year) is the start of a month.
        let moon = chinese_new_moon_on_or_after(rd(Date::new(2026, 2, 10)));
        assert_eq!(from_rd(moon), Date::new(2026, 2, 17));
        // A new moon really is ~29.53 days after the previous one.
        let next = chinese_new_moon_on_or_after(moon + 1);
        assert!((29..=30).contains(&(next - moon)));
    }
}
