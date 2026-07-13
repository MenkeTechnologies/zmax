//! Calendar — the zemacs port of GNU Emacs `calendar-mode`.
//!
//! A full-screen [`Component`] showing a month grid with a movable "point date".
//! All date arithmetic is the pure, unit-tested [`zemacs_core::calendar`]; this
//! module renders the grid and maps keys to date motion.
//!
//! Keys (parsed into a `calendar` keymap mode by `scripts/gen_port_report.py`,
//! so each maps to its Emacs counterpart):
//!   C-f/Right, C-b/Left — forward/backward one day
//!   C-n/Down, C-p/Up   — forward/backward one week
//!   C-a, C-e           — beginning / end of week
//!   M-}, `>`, PageDown — forward one month; M-{, `<`, PageUp — backward
//!   C-v / M-v          — scroll forward / backward three months
//!   [ / ]              — backward / forward one year
//!   { / }              — beginning / end of month; ( / ) — begin / end of year
//!   `.`                — go to today; `g` — goto-date prompt (Y/M/D)
//!   i / J / p          — print ISO / Julian / day-of-year for point
//!   h                  — list this month's holidays (also marked in the grid)
//!   d                  — show diary entries for point; `I` — insert a diary entry
//!   q/Esc              — exit
//! (j/k/l are accepted too as vim-style aliases, not part of the Emacs map.)

use std::time::{SystemTime, UNIX_EPOCH};

use tui::buffer::Buffer as Surface;
use zemacs_core::calendar::{
    add_days, add_months, add_years, beginning_of_month, beginning_of_week, beginning_of_year,
    day_of_year, end_of_month, end_of_week, end_of_year, from_serial, holiday_on, holidays,
    iso_week, julian_day, parse_ymd, weekday, Date, MONTH_NAMES, WEEKDAY_ABBR,
};
use zemacs_view::graphics::Rect;

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Today's date in local-ish (UTC) terms, from the system clock.
fn today() -> Date {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    from_serial(secs / 86_400)
}

/// Which single-line prompt (if any) is active at the foot of the overlay.
#[derive(Clone, Copy)]
enum InputMode {
    /// `calendar-goto-date`: parse a typed `Y/M/D` and jump point there.
    Goto,
    /// `diary-insert-entry`: capture entry text for the date at point.
    Diary,
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
}

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
        }
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
                    InputMode::Diary => {
                        let p = self.point;
                        let text = text.trim().to_string();
                        if text.is_empty() {
                            cx.editor.set_error("Diary: empty entry, nothing added");
                        } else {
                            self.diary.push(zemacs_core::diary::Entry {
                                spec: zemacs_core::diary::DateSpec::Specific {
                                    year: p.year,
                                    month: p.month,
                                    day: p.day,
                                },
                                text: text.clone(),
                            });
                            cx.editor.set_status(format!(
                                "Diary: added \"{text}\" for {} {}, {}",
                                MONTH_NAMES[(p.month - 1) as usize],
                                p.day,
                                p.year
                            ));
                        }
                    }
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
        // Open the goto-date prompt (calendar-goto-date, `g`) or the
        // diary-insert prompt (diary-insert-entry, `I`).
        match key {
            key!('g') => {
                self.input = Some((InputMode::Goto, String::new()));
                cx.editor.set_status("Go to date (Y/M/D): ");
                return EventResult::Consumed(None);
            }
            key!('I') => {
                self.input = Some((InputMode::Diary, String::new()));
                cx.editor.set_status("Diary entry text: ");
                return EventResult::Consumed(None);
            }
            // --- Mayan calendar (cal-mayan): jump by long count / haab / tzolkin ---
            key!('m') => {
                self.input = Some((InputMode::MayanLongCount, String::new()));
                cx.editor.set_status("Mayan long count (b.k.t.u.kin): ");
                return EventResult::Consumed(None);
            }
            key!('H') => {
                self.input = Some((InputMode::MayanHaab { forward: true }, String::new()));
                cx.editor.set_status("Next Mayan haab (day month): ");
                return EventResult::Consumed(None);
            }
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
            _ => {}
        }
        // `d` shows the diary entries for the date at point (emacs
        // diary-view-entries, `d` in calendar-mode).
        if let key!('d') = key {
            let hits = zemacs_core::diary::entries_for(&self.diary, self.point);
            if hits.is_empty() {
                cx.editor.set_status("Diary: no entries for this date");
            } else {
                let joined = hits
                    .iter()
                    .map(|e| e.display_text(self.point))
                    .collect::<Vec<_>>()
                    .join(" · ");
                cx.editor.set_status(format!("Diary: {joined}"));
            }
            return EventResult::Consumed(None);
        }
        // Print commands: report a conversion of the point date and stop (so the
        // day-of-year status below does not overwrite it).
        let p = self.point;
        match key {
            key!('i') => {
                let (y, w, dow) = iso_week(p);
                cx.editor.set_status(format!("ISO date: {y}-W{w:02}-{dow}"));
                return EventResult::Consumed(None);
            }
            key!('J') => {
                cx.editor
                    .set_status(format!("Julian day number: {}", julian_day(p)));
                return EventResult::Consumed(None);
            }
            key!('p') => {
                cx.editor
                    .set_status(format!("Day {} of {}", day_of_year(p), p.year));
                return EventResult::Consumed(None);
            }
            key!('a') => {
                // calendar-print-other-dates: point's date in every other calendar.
                use zemacs_core::calendar as c;
                let islamic = c::islamic_string(p).unwrap_or_else(|| "pre-Islamic".into());
                let french = c::french_string(p).unwrap_or_else(|| "pre-Revolution".into());
                cx.editor.set_status(format!(
                    "Julian {} · Hebrew {} · Islamic {} · Persian {} · Coptic {} · Ethiopic {} · French {} · Baha'i {} · Astro {} · Mayan {}",
                    c::julian_string(p),
                    c::hebrew_string(p),
                    islamic,
                    c::persian_string(p),
                    c::coptic_string(p),
                    c::ethiopic_string(p),
                    french,
                    c::bahai_string(p),
                    c::astro_day_number(p),
                    c::mayan_string(p),
                ));
                return EventResult::Consumed(None);
            }
            _ => {}
        }
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            ctrl!('f') | key!(Right) | key!('l') => self.point = add_days(self.point, 1),
            ctrl!('b') | key!(Left) => self.point = add_days(self.point, -1),
            ctrl!('n') | key!(Down) | key!('j') => self.point = add_days(self.point, 7),
            ctrl!('p') | key!(Up) | key!('k') => self.point = add_days(self.point, -7),
            ctrl!('a') => self.point = beginning_of_week(self.point),
            ctrl!('e') => self.point = end_of_week(self.point),
            alt!('}') | key!('>') | key!(PageDown) => self.point = add_months(self.point, 1),
            alt!('{') | key!('<') | key!(PageUp) => self.point = add_months(self.point, -1),
            // Emacs C-v / M-v scroll the calendar three months at a time.
            ctrl!('v') => self.point = add_months(self.point, 3),
            alt!('v') => self.point = add_months(self.point, -3),
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

        // Footer: an active goto/diary prompt, else the full point date.
        let last_y = area.y + area.height - 1;
        if let Some((mode, buf)) = &self.input {
            let label = match mode {
                InputMode::Goto => "Go to date (Y/M/D): ",
                InputMode::Diary => "Diary entry: ",
                InputMode::MayanLongCount => "Mayan long count (b.k.t.u.kin): ",
                InputMode::MayanHaab { forward: true } => "Next Mayan haab (day month): ",
                InputMode::MayanHaab { forward: false } => "Prev Mayan haab (day month): ",
                InputMode::MayanTzolkin { forward: true } => "Next Mayan tzolkin (number name): ",
                InputMode::MayanTzolkin { forward: false } => "Prev Mayan tzolkin (number name): ",
                InputMode::MayanRound => "Mayan round (hd hm tn tname): ",
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
