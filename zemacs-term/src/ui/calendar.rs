//! Calendar — the zemacs port of GNU Emacs `calendar-mode`.
//!
//! A full-screen [`Component`] showing a month grid with a movable "point date".
//! All date arithmetic is the pure, unit-tested [`zemacs_core::calendar`]; this
//! module renders the grid and maps keys to date motion.
//!
//! Keys — the real `calendar-mode-map` (checked against Emacs 30's `C-h b` dump),
//! including its `g` / `i` / `p` / `t` / `H` / `C-x` / `C-c` prefix maps, which
//! this component walks with a one-key [`Prefix`] state:
//!   C-f/Right, C-b/Left — forward/backward one day
//!   C-n/Down, C-p/Up   — forward/backward one week
//!   C-a, C-e           — beginning / end of week
//!   M-a, M-e           — beginning / end of month; M-< / M-> — of year
//!   M-}, `>`, M-{, `<` — forward / backward one month
//!   C-v / PageDown / M-v / PageUp — scroll forward / backward three months
//!   C-x [ / C-x ]      — backward / forward one year
//!   C-SPC / C-@        — set the mark; C-x C-x — exchange point and mark
//!   M-=                — count the days in the region (mark → point)
//!   `.`                — go to today; SPC / DEL — scroll the output pane
//!   o                  — other month (calendar-other-month)
//!   a / h / x / u      — list holidays / holidays at point / mark them / unmark
//!   M / S              — lunar phases / sunrise-sunset for point
//!   d / s / m          — diary: entries for point / every entry / mark the dates
//!   C-c C-l            — redraw (re-read the diary file)
//!   g …                — goto: `g d` a date, `g D` a day-of-year, `g w` an ISO
//!                        week, `g m …` the Mayan calendars, and `g c/j/h/i/p/k/e/f/b/a`
//!                        a date on the ISO / Julian / Hebrew / Islamic / Persian /
//!                        Coptic / Ethiopic / French / Baha'i / astronomical calendar
//!   p …                — print the date at point on one of those calendars
//!                        (`p d` day-of-year, `p o` every calendar at once)
//!   i …                — insert a diary entry for point: `i d` one-off, `i w`
//!                        weekly, `i m` monthly, `i y` yearly, `i a` anniversary,
//!                        `i b` a block (mark → point), `i c` cyclic (every N days)
//!   H m / H y          — write an HTML calendar for the month / year (cal-html)
//!   t d / t m / t y    — write a LaTeX calendar for the day / month / year (cal-tex)
//!   q/Esc              — exit
//! zemacs aliases kept on keys Emacs leaves free: `I` insert a diary entry,
//! `J` print the Julian date, `B`/`H`… → the Mayan jumps now live under `g m`,
//! and j/k/l move like the arrows.

use std::time::{SystemTime, UNIX_EPOCH};

use tui::buffer::Buffer as Surface;
use zemacs_core::calendar::{
    add_days, add_months, add_years, beginning_of_month, beginning_of_week, beginning_of_year,
    day_of_year, end_of_month, end_of_week, end_of_year, format_hm, from_serial, holiday_on,
    holidays, iso_week, lunar_phases_in_month, parse_ymd, sunrise_sunset_utc, weekday, Date,
    MONTH_NAMES, WEEKDAY_ABBR,
};
use zemacs_view::graphics::Rect;
use zemacs_view::input::KeyEvent;
use zemacs_view::keyboard::KeyCode;

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key, shift,
};

/// Weekday names in full, as a `diary-insert-weekly-entry` line spells them
/// (`WEEKDAY_ABBR` in `zemacs_core::calendar` is the two-letter grid header).
const WEEKDAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// The ISO weekday number of `d`: 1 = Monday … 7 = Sunday (the calendar module's
/// `weekday` is 0 = Sunday).
fn weekday_iso(d: Date) -> u32 {
    (weekday(d) + 6) % 7 + 1
}

/// Today's date in local-ish (UTC) terms, from the system clock.
fn today() -> Date {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    from_serial(secs / 86_400)
}

/// A pending Emacs calendar prefix chord: the first key has been typed and the
/// next key selects the command inside that prefix map. `calendar-mode-map` binds
/// `g`, `i`, `p`, `t`, `H`, `C-x` and `C-c` as prefixes (and `g m n` / `g m p`
/// two levels deep), which a single-key match cannot express.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prefix {
    /// `g` — the goto map.
    Goto,
    /// `g m` — the Mayan goto map.
    GotoMayan,
    /// `g m n` / `g m p` — next / previous Mayan haab / tzolkin / round date.
    GotoMayanNext,
    GotoMayanPrev,
    /// `i` — the diary-insert map.
    Insert,
    /// `p` — the print map (the date at point on another calendar).
    Print,
    /// `t` — the cal-tex map.
    Tex,
    /// `H` — the cal-html map.
    Html,
    /// `C-x` — year motion and `C-x C-x` (exchange point and mark).
    CtrlX,
    /// `C-c` — `C-c C-l` (calendar-redraw).
    CtrlC,
}

/// The calendars `g <char>` jumps to and `p <char>` prints. `Gregorian` is the
/// day-of-year form (`g D` / `p d`), which is a Gregorian reading, not a separate
/// calendar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cal {
    Iso,
    IsoWeek,
    Julian,
    Hebrew,
    Islamic,
    Persian,
    Coptic,
    Ethiopic,
    French,
    Bahai,
    Astro,
    Mayan,
    DayOfYear,
    Other,
}

/// Which kind of diary entry the `i` map is inserting for the date at point.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DiaryKind {
    /// `i d` — a one-off entry on this date.
    Day,
    /// `i w` — every week on this weekday.
    Weekly,
    /// `i m` — this day of every month.
    Monthly,
    /// `i y` — this month/day of every year.
    Yearly,
    /// `i a` — an anniversary of this date.
    Anniversary,
    /// `i b` — a block running from the mark to point.
    Block,
    /// `i c` — a cyclic entry, every N days from this date (the line is read as
    /// `N TEXT`, the way Emacs reads the interval and then the text).
    Cyclic,
}

/// Which single-line prompt (if any) is active at the foot of the overlay.
#[derive(Clone, Copy)]
enum InputMode {
    /// `calendar-goto-date`: parse a typed `Y/M/D` and jump point there.
    Goto,
    /// `diary-insert-entry` and friends: capture entry text for the date at point.
    Diary(DiaryKind),
    /// `calendar-goto-day-of-year` / `calendar-iso-goto-week` / the
    /// `calendar-*-goto-date` family: read a date on `Cal` and jump there.
    GotoCal(Cal),
    /// `calendar-mayan-goto-long-count-date`: parse `b.k.t.u.kin` and jump there.
    MayanLongCount,
    /// `calendar-mayan-next-haab-date`/`-previous-haab-date`: read a haab
    /// `day month` and jump to the next/previous date with it.
    MayanHaab { forward: bool },
    /// `calendar-mayan-next-tzolkin-date`/`-previous-tzolkin-date`: read a tzolkin
    /// `number name` (both 1-based) and jump to the next/previous match.
    MayanTzolkin { forward: bool },
    /// `calendar-mayan-next-calendar-round-date`: read `haab-day haab-month
    /// tzolkin-number tzolkin-name` and jump to the next date matching all four.
    MayanRound,
    /// `calendar-other-month` (`o`): read a `MONTH YEAR` and display that month,
    /// leaving point on its first day.
    OtherMonth,
}

/// The location sunrise/sunset is computed for. Emacs reads the
/// `calendar-latitude` / `calendar-longitude` variables; zemacs has no such
/// variable yet, so — like `commands::calendar_sunrise_sunset` — this uses a
/// fixed default and says so in the status line.
const DEFAULT_LAT: f64 = 40.7128;
const DEFAULT_LON: f64 = -74.0060;

/// The interactive Calendar overlay.
pub struct Calendar {
    point: Date,
    today: Date,
    /// Diary entries loaded from `~/diary`, used to mark dates and show entries.
    /// `diary-insert-entry` appends new entries here in memory.
    diary: Vec<zemacs_core::diary::Entry>,
    /// Active foot-of-screen prompt and the text typed into it so far.
    input: Option<(InputMode, String)>,
    /// Explicitly marked dates (Emacs `calendar-mark-*`; `calendar-unmark`
    /// clears them). Diary and holiday days are highlighted from the data
    /// itself, so only these need remembering.
    marks: std::collections::BTreeSet<Date>,
    /// The date replaced by asterisks (Emacs `calendar-star-date`).
    starred: Option<Date>,
    /// A prefix chord in flight (`g`, `i`, `p`, `t`, `H`, `C-x`, `C-c`).
    prefix: Option<Prefix>,
    /// The region mark (`C-SPC` / `C-@`), used by `M-=` (count the days in the
    /// region), `C-x C-x` and `i b` (a diary block entry over the region).
    mark: Option<Date>,
    /// The lines Emacs would show in the other window (`*Holidays*`, `*Diary*`,
    /// the `p`-family conversions): rendered in a pane at the foot of the overlay
    /// and scrolled by SPC / DEL (`scroll-other-window`).
    output: Vec<String>,
    /// First visible line of `output`.
    out_scroll: usize,
}

impl Calendar {
    pub fn new() -> Self {
        Self::at(today())
    }

    /// Open the Calendar with point at `date` (Emacs `calendar-other-month`).
    pub fn at(date: Date) -> Self {
        Calendar {
            point: date,
            today: today(),
            diary: crate::commands::diary_entries(),
            input: None,
            marks: std::collections::BTreeSet::new(),
            starred: None,
            prefix: None,
            mark: None,
            output: Vec::new(),
            out_scroll: 0,
        }
    }

    /// Show `lines` in the output pane (Emacs pops these up in another window;
    /// SPC / DEL scroll them here).
    fn show(&mut self, lines: Vec<String>) {
        self.output = lines;
        self.out_scroll = 0;
    }

    /// The date under the cursor.
    pub fn point(&self) -> Date {
        self.point
    }

    /// Emacs `calendar-mark-today`: mark today's date in the calendar. `false`
    /// when today is not in the displayed month (nothing to mark there).
    pub fn mark_today(&mut self) -> bool {
        self.marks.insert(self.today);
        self.today.year == self.point.year && self.today.month == self.point.month
    }

    /// The `(year, month)` the grid is showing.
    pub fn displayed_month(&self) -> (i32, u32) {
        (self.point.year, self.point.month)
    }

    /// The diary entries this calendar loaded (`diary-*-mark-entries` picks the
    /// dates to mark out of these).
    pub fn diary(&self) -> &[zemacs_core::diary::Entry] {
        &self.diary
    }

    /// Mark `dates` in the calendar (Emacs `diary-mark-entries` and the
    /// `calendar-mark-*` family). Returns how many marks were added.
    pub fn mark_dates(&mut self, dates: impl IntoIterator<Item = Date>) -> usize {
        let before = self.marks.len();
        self.marks.extend(dates);
        self.marks.len() - before
    }

    /// Emacs `calendar-unmark` (`u`): remove every mark (and the star) from the
    /// calendar. Returns how many marks were removed.
    pub fn unmark(&mut self) -> usize {
        let n = self.marks.len() + usize::from(self.starred.is_some());
        self.marks.clear();
        self.starred = None;
        n
    }

    /// Emacs `calendar-star-date`: replace the date under the cursor with
    /// asterisks.
    pub fn star_date(&mut self) -> Date {
        self.starred = Some(self.point);
        self.point
    }

    /// Emacs `calendar-redraw`: regenerate the calendar, re-reading the diary
    /// file (so entries added outside the overlay show up) and dropping the
    /// marks the old drawing carried. Returns the number of diary entries read.
    pub fn redraw(&mut self) -> usize {
        self.diary = crate::commands::diary_entries();
        self.marks.clear();
        self.starred = None;
        self.diary.len()
    }

    /// Emacs `calendar-scroll-left` / `-right`: move the displayed month `n`
    /// months forward (negative = back). zemacs shows a single month, whose month
    /// is the month of point, so scrolling moves point by whole months (the day
    /// is clamped into the new month, as Emacs's cursor is). Returns the new
    /// point.
    pub fn scroll_months(&mut self, n: i64) -> Date {
        self.point = zemacs_core::calendar::add_months(self.point, n);
        self.point
    }

    /// Feed a key to the active goto/diary prompt. Returns `true` while the
    /// prompt is (still) consuming keys, so the caller stops further handling.
    fn handle_input_key(&mut self, event: Event, cx: &mut Context) -> bool {
        let mode = match &self.input {
            Some((m, _)) => *m,
            None => return false,
        };
        let key = match event {
            Event::Key(k) => k,
            _ => return true, // swallow non-key events while a prompt is open
        };
        match key {
            key!(Esc) | ctrl!('c') | ctrl!('g') => {
                self.input = None;
                cx.editor.set_status("Cancelled");
            }
            key!(Backspace) => {
                if let Some((_, buf)) = &mut self.input {
                    buf.pop();
                }
            }
            key!(Enter) => {
                let text = self
                    .input
                    .as_ref()
                    .map(|(_, b)| b.clone())
                    .unwrap_or_default();
                self.input = None;
                match mode {
                    InputMode::Goto => match parse_ymd(&text) {
                        Some(d) => {
                            self.point = d;
                            cx.editor.set_status(format!(
                                "Goto {} {}, {}",
                                MONTH_NAMES[(d.month - 1) as usize],
                                d.day,
                                d.year
                            ));
                        }
                        None => cx
                            .editor
                            .set_error(format!("Invalid date: {text:?} (use Y/M/D)")),
                    },
                    InputMode::Diary(kind) => self.insert_diary_entry(kind, &text, cx),
                    InputMode::GotoCal(cal) => self.goto_cal(cal, &text, cx),
                    InputMode::OtherMonth => self.goto_other_month(&text, cx),
                    InputMode::MayanLongCount => self.mayan_goto_long_count(&text, cx),
                    InputMode::MayanHaab { forward } => self.mayan_goto_haab(&text, forward, cx),
                    InputMode::MayanTzolkin { forward } => {
                        self.mayan_goto_tzolkin(&text, forward, cx)
                    }
                    InputMode::MayanRound => self.mayan_goto_round(&text, cx),
                }
            }
            _ => {
                if let Some(ch) = key.char() {
                    if let Some((_, buf)) = &mut self.input {
                        buf.push(ch);
                    }
                }
            }
        }
        true
    }

    /// Emacs `calendar-other-month` (`o`): display `MONTH YEAR`, with point on
    /// the 1st of it. The month may be a number (`3 2027`) or a name prefix
    /// (`mar 2027`), matching Emacs's completing read.
    fn goto_other_month(&mut self, text: &str, cx: &mut Context) {
        let mut it = text.split(['/', '-', ' ']).filter(|s| !s.is_empty());
        let (Some(m), Some(y)) = (it.next(), it.next()) else {
            cx.editor.set_error("Other month: expected `MONTH YEAR`");
            return;
        };
        let month = m.parse::<u32>().ok().or_else(|| {
            let m = m.to_ascii_lowercase();
            MONTH_NAMES
                .iter()
                .position(|name| name.to_ascii_lowercase().starts_with(&m))
                .map(|i| i as u32 + 1)
        });
        match (month, y.parse::<i32>()) {
            (Some(month), Ok(year)) if (1..=12).contains(&month) => {
                self.point = Date::new(year, month, 1);
                cx.editor
                    .set_status(format!("{} {}", MONTH_NAMES[(month - 1) as usize], year));
            }
            _ => cx
                .editor
                .set_error(format!("Other month: cannot read {text:?} as MONTH YEAR")),
        }
    }

    /// The date at point on `cal`, as Emacs's `p <char>` prints it.
    fn print_on(&self, cal: Cal) -> String {
        use zemacs_core::calendar as c;
        let p = self.point;
        match cal {
            Cal::Iso => format!("ISO date: {}", c::iso_string(p)),
            Cal::IsoWeek => {
                let (y, w, dow) = iso_week(p);
                format!("ISO week: {y}-W{w:02}-{dow}")
            }
            Cal::Julian => format!("Julian date: {}", c::julian_string(p)),
            Cal::Hebrew => format!("Hebrew date: {}", c::hebrew_string(p)),
            Cal::Islamic => match c::islamic_string(p) {
                Some(s) => format!("Islamic date: {s}"),
                None => "Islamic date: pre-Islamic".to_string(),
            },
            Cal::Persian => format!("Persian date: {}", c::persian_string(p)),
            Cal::Coptic => format!("Coptic date: {}", c::coptic_string(p)),
            Cal::Ethiopic => format!("Ethiopic date: {}", c::ethiopic_string(p)),
            Cal::French => match c::french_string(p) {
                Some(s) => format!("French Revolutionary date: {s}"),
                None => "French Revolutionary date: pre-Revolution".to_string(),
            },
            Cal::Bahai => format!("Baha'i date: {}", c::bahai_string(p)),
            Cal::Astro => format!(
                "Astronomical (Julian) day number: {}",
                c::astro_day_number(p)
            ),
            Cal::Mayan => format!("Mayan date: {}", c::mayan_string(p)),
            Cal::DayOfYear => format!("Day {} of {}", day_of_year(p), p.year),
            Cal::Other => format!("Day {} of {}", day_of_year(p), p.year),
        }
    }

    /// The prompt `g <char>` reads the date with, and what it means.
    fn goto_prompt(cal: Cal) -> &'static str {
        match cal {
            Cal::Iso => "ISO date (year month day): ",
            Cal::IsoWeek => "ISO week (year week [weekday]): ",
            Cal::Julian => "Julian date (year month day): ",
            Cal::Hebrew => "Hebrew date (year month day): ",
            Cal::Islamic => "Islamic date (year month day): ",
            Cal::Persian => "Persian date (year month day): ",
            Cal::Coptic => "Coptic date (year month day): ",
            Cal::Ethiopic => "Ethiopic date (year month day): ",
            Cal::French => "French Revolutionary date (year month day): ",
            Cal::Bahai => "Baha'i date (year month day): ",
            Cal::Astro => "Astronomical (Julian) day number: ",
            Cal::DayOfYear => "Day of year (day [year]): ",
            Cal::Mayan | Cal::Other => "Date: ",
        }
    }

    /// `calendar-<cal>-goto-date` (`g <char>`), `calendar-goto-day-of-year`
    /// (`g D`) and `calendar-iso-goto-week` (`g w`): read the numbers the prompt
    /// asked for and move point to the Gregorian date they name.
    fn goto_cal(&mut self, cal: Cal, text: &str, cx: &mut Context) {
        use zemacs_core::calendar as c;
        let n: Vec<i64> = text
            .split(['/', '-', ' ', ','])
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        // Every calendar below is `year month day` except the one-number forms.
        let fixed = match cal {
            Cal::Astro if n.len() == 1 => {
                // The astronomical day number counts from the same epoch every day
                // of the R.D. does, so its offset is fixed: read it off day 0.
                let offset = c::astro_day_number(c::from_rd(0));
                Some(n[0] - offset)
            }
            Cal::DayOfYear if !n.is_empty() => {
                let year = n.get(1).copied().unwrap_or(self.point.year as i64) as i32;
                Some(c::rd(Date::new(year, 1, 1)) + n[0] - 1)
            }
            Cal::IsoWeek if n.len() >= 2 => {
                // The ISO week's Monday, then the requested weekday within it.
                let (year, week) = (n[0] as i32, n[1]);
                let weekday = n.get(2).copied().unwrap_or(1);
                // Walk from Jan 4 (always in ISO week 1) to that week's Monday.
                let jan4 = Date::new(year, 1, 4);
                let monday_of_w1 = c::rd(jan4) - ((weekday_iso(jan4) as i64) - 1);
                Some(monday_of_w1 + (week - 1) * 7 + (weekday - 1))
            }
            _ if n.len() >= 3 => {
                let (y, m, d) = (n[0], n[1] as u32, n[2] as u32);
                match cal {
                    Cal::Iso => Some(c::rd(Date::new(y as i32, m, d))),
                    Cal::Julian => Some(c::fixed_from_julian(y as i32, m, d)),
                    Cal::Hebrew => Some(c::fixed_from_hebrew(y, m, d)),
                    Cal::Islamic => Some(c::fixed_from_islamic(y, m, d)),
                    Cal::Persian => Some(c::fixed_from_persian(y, m, d)),
                    Cal::Coptic => Some(c::fixed_from_coptic(y as i32, m, d)),
                    Cal::Ethiopic => Some(c::fixed_from_ethiopic(y as i32, m, d)),
                    Cal::French => Some(c::fixed_from_french(y, m, d)),
                    Cal::Bahai => Some(c::fixed_from_bahai(y, m, d)),
                    _ => None,
                }
            }
            _ => None,
        };
        match fixed {
            Some(f) => {
                self.point = c::from_rd(f);
                let p = self.point;
                cx.editor.set_status(format!(
                    "{} {}, {}",
                    MONTH_NAMES[(p.month - 1) as usize],
                    p.day,
                    p.year
                ));
            }
            None => cx.editor.set_error(format!(
                "Cannot read {text:?} as `{}`",
                Self::goto_prompt(cal).trim_end_matches(": ")
            )),
        }
    }

    /// `M-=` (`calendar-count-days-region`): how many days the region spans,
    /// counting both ends, as Emacs reports it.
    fn count_days_region(&mut self, cx: &mut Context) {
        let Some(mark) = self.mark else {
            cx.editor.set_error("No mark set (C-SPC sets it)");
            return;
        };
        // `count_days` is already inclusive of both ends, as Emacs's count is.
        let n = zemacs_core::calendar::count_days(mark, self.point);
        cx.editor.set_status(format!("Region has {n} day(s)"));
    }

    /// The diary line an `i <char>` entry writes, in the diary file's own syntax
    /// (the plain date forms for one-off / weekly / yearly entries, and the
    /// `%%(diary-…)` sexp forms for the rest — exactly what the Emacs
    /// `diary-insert-*-entry` commands append).
    fn diary_line(&self, kind: DiaryKind, text: &str) -> Option<String> {
        use zemacs_core::diary::DateStyle;
        let style = DateStyle::default();
        let p = self.point;
        let line = match kind {
            DiaryKind::Day => format!("{} {text}", style.date_string(p)),
            DiaryKind::Weekly => {
                format!("{} {text}", WEEKDAY_NAMES[weekday(p) as usize])
            }
            // Emacs's monthly entry is the `*` day-of-every-month form; the sexp
            // `diary-date` with a `t` wildcard month is the portable spelling of it.
            DiaryKind::Monthly => format!("%%(diary-date t {} t) {text}", p.day),
            DiaryKind::Yearly => format!("{} {text}", style.yearly_string(p)),
            DiaryKind::Anniversary => {
                format!("%%(diary-anniversary {}) {text}", style.sexp_args(p))
            }
            DiaryKind::Block => {
                let start = self.mark?;
                let (a, b) = if zemacs_core::calendar::count_days(start, p) <= 0 {
                    (start, p)
                } else {
                    (p, start)
                };
                format!(
                    "%%(diary-block {} {}) {text}",
                    style.sexp_args(a),
                    style.sexp_args(b)
                )
            }
            DiaryKind::Cyclic => {
                // Emacs reads the interval, then the text: `N TEXT`.
                let (n, rest) = text.split_once(char::is_whitespace)?;
                let n: i64 = n.parse().ok()?;
                format!(
                    "%%(diary-cyclic {n} {}) {}",
                    style.sexp_args(p),
                    rest.trim()
                )
            }
        };
        Some(line)
    }

    /// `diary-insert-entry` and its cyclic/block/anniversary/weekly/monthly/yearly
    /// siblings: append the entry to the diary file and to the loaded entries, so
    /// the grid marks it immediately.
    fn insert_diary_entry(&mut self, kind: DiaryKind, text: &str, cx: &mut Context) {
        let text = text.trim();
        if text.is_empty() {
            cx.editor.set_error("Diary: empty entry, nothing added");
            return;
        }
        let Some(line) = self.diary_line(kind, text) else {
            cx.editor.set_error(match kind {
                DiaryKind::Block => "Diary block: no mark set (C-SPC sets it)",
                DiaryKind::Cyclic => "Diary cyclic: expected `N TEXT` (the day interval first)",
                _ => "Diary: cannot build that entry",
            });
            return;
        };
        let path = crate::commands::diary_path();
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let mut body = existing;
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&line);
        body.push('\n');
        if let Err(e) = std::fs::write(&path, &body) {
            cx.editor
                .set_error(format!("diary: cannot write {}: {e}", path.display()));
            return;
        }
        // Re-read so the new entry marks the grid and shows up under `d`.
        self.diary = crate::commands::diary_entries();
        cx.editor
            .set_status(format!("Added to {}: {line}", path.display()));
    }

    /// `cal-html-cursor-month` / `-year` (`H m` / `H y`): write a browsable HTML
    /// calendar of the month (or the whole year) at point, holidays marked, and
    /// report the file it wrote.
    fn write_html(&mut self, whole_year: bool, cx: &mut Context) {
        let p = self.point;
        let months: Vec<(i32, u32)> = if whole_year {
            (1..=12).map(|m| (p.year, m)).collect()
        } else {
            vec![(p.year, p.month)]
        };
        let mut html = String::from(
            "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\">\
             <title>Calendar</title>\n<style>\n\
             table{border-collapse:collapse;margin:1em}\
             td,th{border:1px solid #999;padding:4px 8px;text-align:right}\
             .holiday{background:#ffe0e0}\n</style>\n</head>\n<body>\n",
        );
        for (year, month) in months {
            html.push_str(&format!(
                "<table>\n<caption>{} {year}</caption>\n<tr>",
                MONTH_NAMES[(month - 1) as usize]
            ));
            for w in WEEKDAY_ABBR {
                html.push_str(&format!("<th>{w}</th>"));
            }
            html.push_str("</tr>\n<tr>");
            let lead = weekday(Date::new(year, month, 1));
            for _ in 0..lead {
                html.push_str("<td></td>");
            }
            let dim = zemacs_core::calendar::days_in_month(year, month);
            for day in 1..=dim {
                let date = Date::new(year, month, day);
                let cell = (lead + day - 1) % 7;
                match holiday_on(date) {
                    Some(name) => html.push_str(&format!(
                        "<td class=\"holiday\" title=\"{name}\">{day}</td>"
                    )),
                    None => html.push_str(&format!("<td>{day}</td>")),
                }
                if cell == 6 && day != dim {
                    html.push_str("</tr>\n<tr>");
                }
            }
            html.push_str("</tr>\n</table>\n");
        }
        html.push_str("</body>\n</html>\n");

        let name = if whole_year {
            format!("calendar-{}.html", p.year)
        } else {
            format!("calendar-{}-{:02}.html", p.year, p.month)
        };
        self.write_calendar_file(&name, &html, cx);
    }

    /// `cal-tex-cursor-day` / `-month` / `-year` (`t d` / `t m` / `t y`): write the
    /// LaTeX source of a printable calendar. Emacs then runs LaTeX on it; zemacs
    /// writes the `.tex` and reports where, leaving the typesetting to the user's
    /// own toolchain.
    fn write_tex(&mut self, span: char, cx: &mut Context) {
        let p = self.point;
        let mut tex = String::from(
            "\\documentclass[11pt]{article}\n\
             \\usepackage[margin=1in]{geometry}\n\
             \\pagestyle{empty}\n\\begin{document}\n",
        );
        let months: Vec<(i32, u32)> = match span {
            'y' => (1..=12).map(|m| (p.year, m)).collect(),
            'd' => Vec::new(),
            _ => vec![(p.year, p.month)],
        };
        if span == 'd' {
            tex.push_str(&format!(
                "\\section*{{{} {}, {}}}\n\\vspace{{2in}}\n",
                MONTH_NAMES[(p.month - 1) as usize],
                p.day,
                p.year
            ));
        }
        for (year, month) in months {
            tex.push_str(&format!(
                "\\section*{{{} {year}}}\n\\begin{{tabular}}{{|r|r|r|r|r|r|r|}}\n\\hline\n",
                MONTH_NAMES[(month - 1) as usize]
            ));
            tex.push_str(&WEEKDAY_ABBR.join(" & "));
            tex.push_str(" \\\\\n\\hline\n");
            let lead = weekday(Date::new(year, month, 1));
            let dim = zemacs_core::calendar::days_in_month(year, month);
            let mut cells: Vec<String> = vec![String::new(); lead as usize];
            for day in 1..=dim {
                cells.push(day.to_string());
            }
            for row in cells.chunks(7) {
                let mut row: Vec<String> = row.to_vec();
                row.resize(7, String::new());
                tex.push_str(&row.join(" & "));
                tex.push_str(" \\\\\n\\hline\n");
            }
            tex.push_str("\\end{tabular}\n\\newpage\n");
        }
        tex.push_str("\\end{document}\n");

        let name = match span {
            'y' => format!("calendar-{}.tex", p.year),
            'd' => format!("calendar-{}-{:02}-{:02}.tex", p.year, p.month, p.day),
            _ => format!("calendar-{}-{:02}.tex", p.year, p.month),
        };
        self.write_calendar_file(&name, &tex, cx);
    }

    /// Write a generated calendar file into the user's home directory (Emacs's
    /// `cal-html-directory` / cal-tex both write a file and tell you where).
    fn write_calendar_file(&mut self, name: &str, body: &str, cx: &mut Context) {
        let dir = zemacs_stdx::path::home_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let path = dir.join(name);
        match std::fs::write(&path, body) {
            Ok(()) => {
                let msg = format!("Wrote {}", path.display());
                self.show(vec![msg.clone()]);
                cx.editor.set_status(msg);
            }
            Err(e) => cx
                .editor
                .set_error(format!("cannot write {}: {e}", path.display())),
        }
    }

    /// Complete a prefix chord: `self.prefix` was armed by `g`/`i`/`p`/`t`/`H`/
    /// `C-x`/`C-c` and `key` is the next key. Unbound keys are dropped, as Emacs
    /// drops an undefined chord.
    fn handle_prefix(&mut self, prefix: Prefix, key: KeyEvent, cx: &mut Context) {
        // The calendars `g <char>` / `p <char>` name, in Emacs's own letters.
        let cal_of = |c: char| -> Option<Cal> {
            Some(match c {
                'c' => Cal::Iso,
                'j' => Cal::Julian,
                'h' => Cal::Hebrew,
                'i' => Cal::Islamic,
                'p' => Cal::Persian,
                'k' => Cal::Coptic,
                'e' => Cal::Ethiopic,
                'f' => Cal::French,
                'b' => Cal::Bahai,
                'a' => Cal::Astro,
                _ => return None,
            })
        };
        match prefix {
            // ---- `g` — goto ----
            Prefix::Goto => match key {
                // g d — calendar-goto-date.
                key!('d') => {
                    self.input = Some((InputMode::Goto, String::new()));
                    cx.editor.set_status("Go to date (Y/M/D): ");
                }
                // g D — calendar-goto-day-of-year.
                key!('D') => self.ask_goto(Cal::DayOfYear, cx),
                // g w — calendar-iso-goto-week.
                key!('w') => self.ask_goto(Cal::IsoWeek, cx),
                // g m — the Mayan sub-map.
                key!('m') => {
                    self.prefix = Some(Prefix::GotoMayan);
                    cx.editor
                        .set_status("g m- (l long count · n next · p previous)");
                }
                // g <char> — the other-calendar gotos.
                KeyEvent {
                    code: KeyCode::Char(c),
                    ..
                } if cal_of(c).is_some() => {
                    if let Some(cal) = cal_of(c) {
                        self.ask_goto(cal, cx);
                    }
                }
                _ => {}
            },
            // ---- `g m` — the Mayan calendars ----
            Prefix::GotoMayan => match key {
                key!('l') => {
                    self.input = Some((InputMode::MayanLongCount, String::new()));
                    cx.editor.set_status("Mayan long count (b.k.t.u.kin): ");
                }
                key!('n') => {
                    self.prefix = Some(Prefix::GotoMayanNext);
                    cx.editor
                        .set_status("g m n- (h haab · t tzolkin · c calendar round)");
                }
                key!('p') => {
                    self.prefix = Some(Prefix::GotoMayanPrev);
                    cx.editor
                        .set_status("g m p- (h haab · t tzolkin · c round)");
                }
                _ => {}
            },
            Prefix::GotoMayanNext | Prefix::GotoMayanPrev => {
                let forward = prefix == Prefix::GotoMayanNext;
                match key {
                    key!('h') => {
                        self.input = Some((InputMode::MayanHaab { forward }, String::new()));
                        cx.editor.set_status("Mayan haab (day month): ");
                    }
                    key!('t') => {
                        self.input = Some((InputMode::MayanTzolkin { forward }, String::new()));
                        cx.editor.set_status("Mayan tzolkin (number name): ");
                    }
                    key!('c') => {
                        self.input = Some((InputMode::MayanRound, String::new()));
                        cx.editor.set_status(
                            "Mayan calendar round (haab-day haab-month tz-num tz-name): ",
                        );
                    }
                    _ => {}
                }
            }
            // ---- `i` — insert a diary entry for the date at point ----
            Prefix::Insert => {
                let kind = match key {
                    key!('d') => DiaryKind::Day,
                    key!('w') => DiaryKind::Weekly,
                    key!('m') => DiaryKind::Monthly,
                    key!('y') => DiaryKind::Yearly,
                    key!('a') => DiaryKind::Anniversary,
                    key!('b') => DiaryKind::Block,
                    key!('c') => DiaryKind::Cyclic,
                    _ => return,
                };
                self.input = Some((InputMode::Diary(kind), String::new()));
                cx.editor.set_status(match kind {
                    DiaryKind::Cyclic => "Cyclic diary entry (N TEXT): ",
                    _ => "Diary entry text: ",
                });
            }
            // ---- `p` — print the date at point on another calendar ----
            Prefix::Print => {
                let cal = match key {
                    key!('d') => Cal::DayOfYear,
                    key!('m') => Cal::Mayan,
                    key!('o') => Cal::Other,
                    KeyEvent {
                        code: KeyCode::Char(c),
                        ..
                    } => match cal_of(c) {
                        Some(cal) => cal,
                        None => return,
                    },
                    _ => return,
                };
                // p o — calendar-print-other-dates: every calendar at once.
                if cal == Cal::Other {
                    let lines: Vec<String> = [
                        Cal::Iso,
                        Cal::Julian,
                        Cal::Hebrew,
                        Cal::Islamic,
                        Cal::Persian,
                        Cal::Coptic,
                        Cal::Ethiopic,
                        Cal::French,
                        Cal::Bahai,
                        Cal::Astro,
                        Cal::Mayan,
                    ]
                    .iter()
                    .map(|c| self.print_on(*c))
                    .collect();
                    cx.editor.set_status(lines.join(" · "));
                    self.show(lines);
                } else {
                    let line = self.print_on(cal);
                    cx.editor.set_status(line.clone());
                    self.show(vec![line]);
                }
            }
            // ---- `t` / `H` — write a LaTeX / HTML calendar ----
            Prefix::Tex => match key {
                key!('d') => self.write_tex('d', cx),
                key!('m') => self.write_tex('m', cx),
                key!('y') => self.write_tex('y', cx),
                _ => {}
            },
            Prefix::Html => match key {
                key!('m') => self.write_html(false, cx),
                key!('y') => self.write_html(true, cx),
                _ => {}
            },
            // ---- `C-x` — year motion and the region ----
            Prefix::CtrlX => match key {
                key!('[') => self.point = add_years(self.point, -1),
                key!(']') => self.point = add_years(self.point, 1),
                key!('<') => self.point = add_months(self.point, -1),
                key!('>') => self.point = add_months(self.point, 1),
                // C-x C-x — calendar-exchange-point-and-mark.
                ctrl!('x') => {
                    if let Some(mark) = self.mark.replace(self.point) {
                        self.point = mark;
                    }
                }
                _ => {}
            },
            // ---- `C-c` — C-c C-l redraws ----
            Prefix::CtrlC => {
                if key == ctrl!('l') {
                    let n = self.redraw();
                    cx.editor
                        .set_status(format!("Calendar redrawn ({n} diary entries)"));
                }
            }
        }
    }

    /// Open the prompt `g <char>` reads its date with.
    fn ask_goto(&mut self, cal: Cal, cx: &mut Context) {
        self.input = Some((InputMode::GotoCal(cal), String::new()));
        cx.editor.set_status(Self::goto_prompt(cal));
    }

    /// Emacs `calendar-mark-holidays` (`x`): mark every holiday of the displayed
    /// month in the grid. Returns how many were marked.
    fn mark_holidays(&mut self) -> usize {
        let p = self.point;
        let dates: Vec<Date> = holidays(p.year, p.month)
            .into_iter()
            .map(|(day, _)| Date::new(p.year, p.month, day))
            .collect();
        self.mark_dates(dates)
    }

    /// `calendar-mayan-goto-long-count-date`: jump to the R.D. of `b.k.t.u.kin`.
    fn mayan_goto_long_count(&mut self, text: &str, cx: &mut Context) {
        let parts: Vec<i64> = text
            .split(['.', ' '])
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if parts.len() != 5 {
            cx.editor
                .set_error("Mayan long count: expected b.k.t.u.kin (5 numbers)");
            return;
        }
        let f = zemacs_core::calendar::fixed_from_mayan_long_count(
            parts[0], parts[1], parts[2], parts[3], parts[4],
        );
        self.point = zemacs_core::calendar::from_rd(f);
        cx.editor.set_status(format!(
            "Mayan {}.{}.{}.{}.{} = {} {}, {}",
            parts[0],
            parts[1],
            parts[2],
            parts[3],
            parts[4],
            MONTH_NAMES[(self.point.month - 1) as usize],
            self.point.day,
            self.point.year
        ));
    }

    /// Parse two whitespace-separated numbers (haab `day month` / tzolkin `num name`).
    fn parse_pair(text: &str) -> Option<(i64, u32)> {
        let mut it = text.split_whitespace();
        let a = it.next()?.parse().ok()?;
        let b = it.next()?.parse().ok()?;
        Some((a, b))
    }

    fn mayan_goto_haab(&mut self, text: &str, forward: bool, cx: &mut Context) {
        let Some(target) = Self::parse_pair(text) else {
            cx.editor.set_error("Mayan haab: expected `day month`");
            return;
        };
        let f = zemacs_core::calendar::mayan_next_haab(
            zemacs_core::calendar::rd(self.point),
            target,
            forward,
        );
        self.point = zemacs_core::calendar::from_rd(f);
        cx.editor.set_status(format!(
            "Mayan haab {} {} → {} {}, {}",
            target.0,
            target.1,
            MONTH_NAMES[(self.point.month - 1) as usize],
            self.point.day,
            self.point.year
        ));
    }

    fn mayan_goto_tzolkin(&mut self, text: &str, forward: bool, cx: &mut Context) {
        let Some(target) = Self::parse_pair(text) else {
            cx.editor.set_error("Mayan tzolkin: expected `number name`");
            return;
        };
        let f = zemacs_core::calendar::mayan_next_tzolkin(
            zemacs_core::calendar::rd(self.point),
            target,
            forward,
        );
        self.point = zemacs_core::calendar::from_rd(f);
        cx.editor.set_status(format!(
            "Mayan tzolkin {} {} → {} {}, {}",
            target.0,
            target.1,
            MONTH_NAMES[(self.point.month - 1) as usize],
            self.point.day,
            self.point.year
        ));
    }

    fn mayan_goto_round(&mut self, text: &str, cx: &mut Context) {
        let nums: Vec<i64> = text
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() != 4 {
            cx.editor.set_error(
                "Mayan calendar round: expected `haab-day haab-month tz-number tz-name`",
            );
            return;
        }
        let f = zemacs_core::calendar::mayan_next_round(
            zemacs_core::calendar::rd(self.point),
            (nums[0], nums[1] as u32),
            (nums[2], nums[3] as u32),
            true,
        );
        self.point = zemacs_core::calendar::from_rd(f);
        cx.editor.set_status(format!(
            "Mayan calendar round → {} {}, {}",
            MONTH_NAMES[(self.point.month - 1) as usize],
            self.point.day,
            self.point.year
        ));
    }
}

impl Default for Calendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Calendar {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        // While a goto-date / diary-insert prompt is open it owns every key.
        if self.input.is_some() {
            self.handle_input_key(event.clone(), cx);
            return EventResult::Consumed(None);
        }
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        // A prefix chord (`g`, `i`, `p`, `t`, `H`, `C-x`, `C-c`) owns the next key.
        if let Some(prefix) = self.prefix.take() {
            self.handle_prefix(prefix, key, cx);
            return EventResult::Consumed(None);
        }

        match key {
            // ---- prefix chords (see `handle_prefix`) ----
            key!('g') => {
                self.prefix = Some(Prefix::Goto);
                cx.editor.set_status("g- (d date · D day-of-year · w ISO week · m Mayan · c/j/h/i/p/k/e/f/b/a other calendars)");
                return EventResult::Consumed(None);
            }
            key!('i') => {
                self.prefix = Some(Prefix::Insert);
                cx.editor
                    .set_status("i- (d day · w weekly · m monthly · y yearly · a anniversary · b block · c cyclic)");
                return EventResult::Consumed(None);
            }
            key!('p') => {
                self.prefix = Some(Prefix::Print);
                cx.editor
                    .set_status("p- (d day-of-year · o all calendars · c/j/h/i/p/k/e/f/b/a/m one)");
                return EventResult::Consumed(None);
            }
            key!('t') => {
                self.prefix = Some(Prefix::Tex);
                cx.editor.set_status("t- (d day · m month · y year LaTeX)");
                return EventResult::Consumed(None);
            }
            key!('H') => {
                self.prefix = Some(Prefix::Html);
                cx.editor.set_status("H- (m month · y year HTML)");
                return EventResult::Consumed(None);
            }
            ctrl!('x') => {
                self.prefix = Some(Prefix::CtrlX);
                return EventResult::Consumed(None);
            }
            ctrl!('c') => {
                self.prefix = Some(Prefix::CtrlC);
                return EventResult::Consumed(None);
            }
            // ---- zemacs aliases on keys Emacs leaves free in the calendar ----
            // `I` inserts a diary entry (Emacs: `i d`).
            key!('I') => {
                self.input = Some((InputMode::Diary(DiaryKind::Day), String::new()));
                cx.editor.set_status("Diary entry text: ");
                return EventResult::Consumed(None);
            }
            // `J` prints the Julian date (Emacs: `p j`).
            key!('J') => {
                let line = self.print_on(Cal::Julian);
                self.show(vec![line.clone()]);
                cx.editor.set_status(line);
                return EventResult::Consumed(None);
            }
            // --- Mayan (cal-mayan) aliases; the Emacs keys are `g m n h` etc. ---
            key!('B') => {
                self.input = Some((InputMode::MayanHaab { forward: false }, String::new()));
                cx.editor.set_status("Previous Mayan haab (day month): ");
                return EventResult::Consumed(None);
            }
            key!('T') => {
                self.input = Some((InputMode::MayanTzolkin { forward: true }, String::new()));
                cx.editor.set_status("Next Mayan tzolkin (number name): ");
                return EventResult::Consumed(None);
            }
            key!('Y') => {
                self.input = Some((InputMode::MayanTzolkin { forward: false }, String::new()));
                cx.editor
                    .set_status("Previous Mayan tzolkin (number name): ");
                return EventResult::Consumed(None);
            }
            key!('R') => {
                self.input = Some((InputMode::MayanRound, String::new()));
                cx.editor
                    .set_status("Mayan calendar round (haab-day haab-month tz-num tz-name): ");
                return EventResult::Consumed(None);
            }
            // ---- the region: C-SPC / C-@ set the mark, M-= counts it ----
            ctrl!(' ') | ctrl!('@') => {
                self.mark = Some(self.point);
                let p = self.point;
                cx.editor.set_status(format!(
                    "Mark set at {} {}, {}",
                    MONTH_NAMES[(p.month - 1) as usize],
                    p.day,
                    p.year
                ));
                return EventResult::Consumed(None);
            }
            alt!('=') => {
                self.count_days_region(cx);
                return EventResult::Consumed(None);
            }
            // ---- SPC / DEL scroll the output pane (Emacs: scroll-other-window) ----
            key!(' ') => {
                self.out_scroll = (self.out_scroll + 1).min(self.output.len().saturating_sub(1));
                return EventResult::Consumed(None);
            }
            key!(Backspace) | shift!(' ') => {
                self.out_scroll = self.out_scroll.saturating_sub(1);
                return EventResult::Consumed(None);
            }
            // a — calendar-list-holidays: every holiday of the month at point.
            key!('a') => {
                let p = self.point;
                let hs = holidays(p.year, p.month);
                let lines: Vec<String> = if hs.is_empty() {
                    vec![format!(
                        "No holidays in {} {}",
                        MONTH_NAMES[(p.month - 1) as usize],
                        p.year
                    )]
                } else {
                    std::iter::once(format!(
                        "Holidays in {} {}:",
                        MONTH_NAMES[(p.month - 1) as usize],
                        p.year
                    ))
                    .chain(hs.iter().map(|&(d, name)| {
                        format!("  {} {d}: {name}", MONTH_NAMES[(p.month - 1) as usize])
                    }))
                    .collect()
                };
                cx.editor.set_status(format!("{} holiday(s)", hs.len()));
                self.show(lines);
                return EventResult::Consumed(None);
            }
            // m — diary-mark-entries: mark every date this month that has one.
            key!('m') => {
                let p = self.point;
                let dim = zemacs_core::calendar::days_in_month(p.year, p.month);
                let dates: Vec<Date> = (1..=dim)
                    .map(|d| Date::new(p.year, p.month, d))
                    .filter(|d| zemacs_core::diary::has_entry(&self.diary, *d))
                    .collect();
                let n = self.mark_dates(dates);
                cx.editor
                    .set_status(format!("Marked {n} date(s) with diary entries"));
                return EventResult::Consumed(None);
            }
            // calendar-other-month (`o`): display another month.
            key!('o') => {
                self.input = Some((InputMode::OtherMonth, String::new()));
                cx.editor.set_status("Other month (MONTH YEAR): ");
                return EventResult::Consumed(None);
            }
            // calendar-lunar-phases (`M`): this month's principal moon phases.
            key!('M') => {
                let phases = lunar_phases_in_month(self.point.year, self.point.month);
                if phases.is_empty() {
                    cx.editor.set_status("No principal moon phase this month");
                } else {
                    let listed = phases
                        .iter()
                        .map(|(d, name)| format!("{name} {}", d.day))
                        .collect::<Vec<_>>()
                        .join(" · ");
                    cx.editor.set_status(format!(
                        "Lunar phases {} {} (approx): {listed}",
                        MONTH_NAMES[(self.point.month - 1) as usize],
                        self.point.year
                    ));
                }
                return EventResult::Consumed(None);
            }
            // calendar-sunrise-sunset (`S`): for the date under the cursor.
            key!('S') => {
                match sunrise_sunset_utc(self.point, DEFAULT_LAT, DEFAULT_LON) {
                    Some((rise, set)) => cx.editor.set_status(format!(
                        "Sunrise {} UTC, sunset {} UTC at {DEFAULT_LAT},{DEFAULT_LON} (approx)",
                        format_hm(rise),
                        format_hm(set),
                    )),
                    None => cx
                        .editor
                        .set_status("No sunrise/sunset on this date (polar day/night)"),
                }
                return EventResult::Consumed(None);
            }
            // diary-show-all-entries (`s`): every entry the diary file holds.
            key!('s') => {
                if self.diary.is_empty() {
                    cx.editor.set_status("Diary: no entries");
                    self.show(vec!["Diary: no entries".to_string()]);
                } else {
                    let lines: Vec<String> =
                        std::iter::once(format!("Diary ({} entries):", self.diary.len()))
                            .chain(
                                self.diary
                                    .iter()
                                    .map(|e| format!("  {}", e.display_text(self.point))),
                            )
                            .collect();
                    cx.editor
                        .set_status(format!("Diary: {} entries", self.diary.len()));
                    self.show(lines);
                }
                return EventResult::Consumed(None);
            }
            // calendar-unmark (`u`) / calendar-mark-holidays (`x`).
            key!('u') => {
                let n = self.unmark();
                cx.editor.set_status(format!("Unmarked {n} date(s)"));
                return EventResult::Consumed(None);
            }
            key!('x') => {
                let n = self.mark_holidays();
                cx.editor.set_status(format!(
                    "Marked {n} holiday(s) in {}",
                    MONTH_NAMES[(self.point.month - 1) as usize]
                ));
                return EventResult::Consumed(None);
            }
            key!('h') => {
                // calendar-cursor-holidays / holidays: list the month's holidays,
                // flagging the one on the point date if any.
                let hs = holidays(self.point.year, self.point.month);
                if hs.is_empty() {
                    cx.editor.set_status(format!(
                        "Holidays: none in {}",
                        MONTH_NAMES[(self.point.month - 1) as usize]
                    ));
                } else {
                    let listed = hs
                        .iter()
                        .map(|&(d, name)| format!("{d} {name}"))
                        .collect::<Vec<_>>()
                        .join(" · ");
                    match holiday_on(self.point) {
                        Some(today) => cx
                            .editor
                            .set_status(format!("Holiday today: {today} — all: {listed}")),
                        None => cx.editor.set_status(format!("Holidays: {listed}")),
                    }
                }
                return EventResult::Consumed(None);
            }
            // `d` (diary-view-entries): the entries for the date at point.
            key!('d') => {
                let hits = zemacs_core::diary::entries_for(&self.diary, self.point);
                let p = self.point;
                if hits.is_empty() {
                    cx.editor.set_status("Diary: no entries for this date");
                    self.show(vec![format!(
                        "No diary entries for {} {}, {}",
                        MONTH_NAMES[(p.month - 1) as usize],
                        p.day,
                        p.year
                    )]);
                } else {
                    let lines: Vec<String> = std::iter::once(format!(
                        "Diary for {} {}, {}:",
                        MONTH_NAMES[(p.month - 1) as usize],
                        p.day,
                        p.year
                    ))
                    .chain(hits.iter().map(|e| format!("  {}", e.display_text(p))))
                    .collect();
                    cx.editor
                        .set_status(format!("Diary: {} entry(ies)", hits.len()));
                    self.show(lines);
                }
                return EventResult::Consumed(None);
            }
            _ => {}
        }
        match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close)),
            ctrl!('f') | key!(Right) | key!('l') => self.point = add_days(self.point, 1),
            ctrl!('b') | key!(Left) => self.point = add_days(self.point, -1),
            ctrl!('n') | key!(Down) | key!('j') => self.point = add_days(self.point, 7),
            ctrl!('p') | key!(Up) | key!('k') => self.point = add_days(self.point, -7),
            ctrl!('a') => self.point = beginning_of_week(self.point),
            ctrl!('e') => self.point = end_of_week(self.point),
            // Beginning / end of month and of year (emacs M-a / M-e / M-< / M->).
            alt!('a') => self.point = beginning_of_month(self.point),
            alt!('e') => self.point = end_of_month(self.point),
            alt!('<') => self.point = beginning_of_year(self.point),
            alt!('>') => self.point = end_of_year(self.point),
            alt!('}') | key!('>') => self.point = add_months(self.point, 1),
            alt!('{') | key!('<') => self.point = add_months(self.point, -1),
            // C-v / PageDown (next) and M-v / PageUp (prior) all scroll the
            // calendar THREE months at a time, as Emacs's scroll-*-three-months do.
            ctrl!('v') | key!(PageDown) => self.point = add_months(self.point, 3),
            alt!('v') | key!(PageUp) => self.point = add_months(self.point, -3),
            key!(']') => self.point = add_years(self.point, 1),
            key!('[') => self.point = add_years(self.point, -1),
            key!('{') => self.point = beginning_of_month(self.point),
            key!('}') => self.point = end_of_month(self.point),
            key!('(') => self.point = beginning_of_year(self.point),
            key!(')') => self.point = end_of_year(self.point),
            key!('.') => self.point = self.today,
            _ => {}
        }
        // Report the day-of-year for the current point (emacs `p d`).
        cx.editor.set_status(format!(
            "{} {}, {} (day {} of {})",
            MONTH_NAMES[(self.point.month - 1) as usize],
            self.point.day,
            self.point.year,
            day_of_year(self.point),
            self.point.year,
        ));
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let today_style = theme.get("diff.plus");
        let diary_style = theme.get("warning");
        let holiday_style = theme.get("function");
        let prompt_style = theme.get("ui.text.focus");
        // Explicitly marked days (calendar-mark-today / calendar-mark-*).
        let mark_style = theme.get("constant");

        surface.clear_with(area, bg);
        if area.width < 22 || area.height < 6 {
            return;
        }

        let p = self.point;
        let title = format!(" {} {}", MONTH_NAMES[(p.month - 1) as usize], p.year);
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "day C-f/b · week C-n/p · month M-{/} · g goto · h holiday · d/I diary · q";
        if title.len() + hint.len() + 3 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }

        // Weekday header row.
        let wy = area.y + 2;
        let mut header = String::new();
        for w in WEEKDAY_ABBR {
            header.push_str(w);
            header.push(' ');
        }
        surface.set_stringn(area.x, wy, &header, area.width as usize, info_style);

        // Day grid.
        let first = Date::new(p.year, p.month, 1);
        let lead = weekday(first) as u32; // 0 = Sunday
        let dim = zemacs_core::calendar::days_in_month(p.year, p.month);
        for d in 1..=dim {
            let cell = lead + d - 1;
            let row = cell / 7;
            let col = cell % 7;
            let x = area.x + (col * 3) as u16;
            let y = wy + 1 + row as u16;
            if y >= area.y + area.height {
                break;
            }
            let cell_date = Date::new(p.year, p.month, d);
            // calendar-star-date replaces the date itself with asterisks.
            let starred = self.starred == Some(cell_date);
            let s = if starred {
                "**".to_string()
            } else {
                format!("{:>2}", d)
            };
            let has_diary = zemacs_core::diary::has_entry(&self.diary, cell_date);
            let has_holiday = holiday_on(cell_date).is_some();
            // Precedence: star > point > explicit mark > today > diary > holiday.
            let style = if starred || d == p.day {
                sel_style
            } else if self.marks.contains(&cell_date) {
                mark_style
            } else if p.year == self.today.year
                && p.month == self.today.month
                && d == self.today.day
            {
                today_style
            } else if has_diary {
                diary_style
            } else if has_holiday {
                holiday_style
            } else {
                text_style
            };
            surface.set_stringn(x, y, &s, 2, style);
        }

        // The output pane (Emacs shows these in another window): the holiday /
        // diary / conversion listings, scrolled by SPC and DEL.
        let grid_bottom = wy + 2 + ((lead + dim - 1) / 7) as u16;
        let last_y = area.y + area.height - 1;
        if !self.output.is_empty() && grid_bottom + 1 < last_y {
            let pane_y = grid_bottom + 1;
            let rows = (last_y - pane_y) as usize;
            let start = self.out_scroll.min(self.output.len().saturating_sub(1));
            for (i, line) in self.output[start..].iter().take(rows).enumerate() {
                let style = if i == 0 && start == 0 {
                    header_style
                } else {
                    text_style
                };
                surface.set_stringn(area.x, pane_y + i as u16, line, area.width as usize, style);
            }
        }

        // Footer: an active goto/diary prompt, else the full point date.
        if let Some((mode, buf)) = &self.input {
            let label = match mode {
                InputMode::Goto => "Go to date (Y/M/D): ",
                InputMode::Diary(DiaryKind::Cyclic) => "Cyclic diary entry (N TEXT): ",
                InputMode::Diary(_) => "Diary entry: ",
                InputMode::GotoCal(cal) => Self::goto_prompt(*cal),
                InputMode::MayanLongCount => "Mayan long count (b.k.t.u.kin): ",
                InputMode::MayanHaab { forward: true } => "Next Mayan haab (day month): ",
                InputMode::MayanHaab { forward: false } => "Prev Mayan haab (day month): ",
                InputMode::MayanTzolkin { forward: true } => "Next Mayan tzolkin (number name): ",
                InputMode::MayanTzolkin { forward: false } => "Prev Mayan tzolkin (number name): ",
                InputMode::MayanRound => "Mayan round (hd hm tn tname): ",
                InputMode::OtherMonth => "Other month (MONTH YEAR): ",
            };
            let line = format!("{label}{buf}_");
            surface.set_stringn(area.x, last_y, &line, area.width as usize, prompt_style);
        } else if area.height >= 8 {
            let footer = format!(
                "{}  {} {}, {}  (day {} of {})",
                WEEKDAY_ABBR[weekday(p) as usize],
                MONTH_NAMES[(p.month - 1) as usize],
                p.day,
                p.year,
                day_of_year(p),
                p.year,
            );
            surface.set_stringn(area.x, last_y, &footer, area.width as usize, info_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `i <char>` writes the diary file's own syntax: the plain date forms for the
    /// one-off / weekly / yearly entries, and the `%%(diary-…)` sexp forms for the
    /// rest. Every line it writes must parse back into the DateSpec it meant —
    /// otherwise the entry is silently dead in the file.
    #[test]
    fn inserted_diary_lines_parse_back_to_the_right_spec() {
        use zemacs_core::diary::{parse_line, DateSpec};
        let mut cal = Calendar::at(Date::new(2026, 7, 13)); // a Monday
        assert_eq!(WEEKDAY_NAMES[weekday(cal.point) as usize], "Monday");

        let round = |line: &str| parse_line(line).expect("a diary line must parse").0;

        // i d — one specific date.
        let line = cal.diary_line(DiaryKind::Day, "Dentist").unwrap();
        assert_eq!(
            round(&line),
            DateSpec::Specific {
                year: 2026,
                month: 7,
                day: 13
            }
        );

        // i w — every Monday.
        let line = cal.diary_line(DiaryKind::Weekly, "Standup").unwrap();
        assert_eq!(round(&line), DateSpec::Weekly { weekday: 1 });

        // i y — every July 13th.
        let line = cal.diary_line(DiaryKind::Yearly, "Birthday").unwrap();
        assert_eq!(round(&line), DateSpec::Yearly { month: 7, day: 13 });

        // i m — the 13th of every month (the wildcard-month sexp form).
        let line = cal.diary_line(DiaryKind::Monthly, "Rent").unwrap();
        assert_eq!(
            round(&line),
            DateSpec::DateWild {
                month: None,
                day: Some(13),
                year: None
            }
        );

        // i a — an anniversary of this date.
        let line = cal.diary_line(DiaryKind::Anniversary, "Wedding").unwrap();
        assert_eq!(
            round(&line),
            DateSpec::Anniversary {
                month: 7,
                day: 13,
                year: Some(2026)
            }
        );

        // i c — every 14 days from this date; the interval is read off the line.
        let line = cal.diary_line(DiaryKind::Cyclic, "14 Payday").unwrap();
        assert_eq!(
            round(&line),
            DateSpec::Cyclic {
                n: 14,
                base: Date::new(2026, 7, 13)
            }
        );
        assert!(
            line.ends_with(" Payday"),
            "the interval is not part of the text: {line}"
        );
        // Without an interval there is nothing to insert.
        assert!(cal.diary_line(DiaryKind::Cyclic, "Payday").is_none());

        // i b — a block needs the region: no mark, no entry.
        assert!(cal.diary_line(DiaryKind::Block, "Vacation").is_none());
        cal.mark = Some(Date::new(2026, 7, 20));
        let line = cal.diary_line(DiaryKind::Block, "Vacation").unwrap();
        assert_eq!(
            round(&line),
            DateSpec::Block {
                start: Date::new(2026, 7, 13),
                end: Date::new(2026, 7, 20)
            },
            "the block runs from the earlier end to the later one, whichever is the mark"
        );
    }

    /// `g <char>` converts FROM the named calendar: the date it lands on must be
    /// the one whose `p <char>` conversion is what was typed. Round-tripping each
    /// calendar catches an inverted or off-by-one conversion.
    #[test]
    fn goto_other_calendar_lands_on_the_date_that_prints_back() {
        use zemacs_core::calendar as c;

        // Julian: the date `p j` prints for July 13 2026, fed back to `g j`, must
        // land on July 13 2026 again.
        let (jy, jm, jd) = c::julian_from_fixed(c::rd(Date::new(2026, 7, 13)));
        assert_eq!(
            c::from_rd(c::fixed_from_julian(jy, jm, jd)),
            Date::new(2026, 7, 13),
            "julian round-trip"
        );

        // Hebrew and Islamic round-trip through their own fixed_from_*.
        let (hy, hm, hd) = c::hebrew_from_fixed(c::rd(Date::new(2026, 7, 13)));
        assert_eq!(
            c::from_rd(c::fixed_from_hebrew(hy, hm, hd)),
            Date::new(2026, 7, 13),
            "hebrew round-trip"
        );
        let (iy, im, id) = c::islamic_from_fixed(c::rd(Date::new(2026, 7, 13))).unwrap();
        assert_eq!(
            c::from_rd(c::fixed_from_islamic(iy, im, id)),
            Date::new(2026, 7, 13),
            "islamic round-trip"
        );

        // The astronomical day number's offset is constant, so `g a` inverts it.
        let d = Date::new(2026, 7, 13);
        let astro = c::astro_day_number(d);
        let offset = c::astro_day_number(c::from_rd(0));
        assert_eq!(c::from_rd(astro - offset), d, "astro day number inverts");
    }

    /// `g D` (day-of-year) and `g w` (ISO week) are the two goto forms that are not
    /// `year month day` — they must land on the date their `p` counterpart names.
    #[test]
    fn goto_day_of_year_and_iso_week() {
        use zemacs_core::calendar as c;
        // Day 200 of 2026.
        let f = c::rd(Date::new(2026, 1, 1)) + 200 - 1;
        let d = c::from_rd(f);
        assert_eq!(day_of_year(d), 200);

        // ISO week 30 of 2026, weekday 1 (Monday): the same date `iso_week` reports.
        let jan4 = Date::new(2026, 1, 4);
        let monday_w1 = c::rd(jan4) - ((weekday_iso(jan4) as i64) - 1);
        let target = c::from_rd(monday_w1 + (30 - 1) * 7);
        let (y, w, dow) = iso_week(target);
        assert_eq!((y, w, dow), (2026, 30, 1));
    }

    /// `M-=` counts the days in the region inclusively (Jul 13 → Jul 20 is 8 days,
    /// not 7), and in either direction — the mark may be before or after point.
    #[test]
    fn region_day_count_is_inclusive_and_symmetric() {
        let a = Date::new(2026, 7, 13);
        let b = Date::new(2026, 7, 20);
        assert_eq!(zemacs_core::calendar::count_days(a, b), 8);
        assert_eq!(zemacs_core::calendar::count_days(b, a), 8);
        // A one-day region is one day, not zero.
        assert_eq!(zemacs_core::calendar::count_days(a, a), 1);
    }
}
