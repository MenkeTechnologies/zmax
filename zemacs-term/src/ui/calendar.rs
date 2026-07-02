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
//!   `.`                — go to today
//!   q/Esc              — exit
//! (h/j/k/l are accepted too as vim-style aliases, not part of the Emacs map.)

use std::time::{SystemTime, UNIX_EPOCH};

use tui::buffer::Buffer as Surface;
use zemacs_core::calendar::{
    add_days, add_months, add_years, beginning_of_month, beginning_of_week, beginning_of_year,
    day_of_year, end_of_month, end_of_week, end_of_year, from_serial, iso_week, julian_day, weekday,
    Date, MONTH_NAMES, WEEKDAY_ABBR,
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

/// The interactive Calendar overlay.
pub struct Calendar {
    point: Date,
    today: Date,
    /// Diary entries loaded from `~/diary`, used to mark dates and show entries.
    diary: Vec<zemacs_core::diary::Entry>,
}

impl Calendar {
    pub fn new() -> Self {
        let today = today();
        Calendar {
            point: today,
            today,
            diary: crate::commands::diary_entries(),
        }
    }
}

impl Default for Calendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Calendar {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        // `d` shows the diary entries for the date at point (emacs
        // diary-view-entries, `d` in calendar-mode).
        if let key!('d') = key {
            let hits = zemacs_core::diary::entries_for(&self.diary, self.point);
            if hits.is_empty() {
                cx.editor.set_status("Diary: no entries for this date");
            } else {
                let joined = hits.iter().map(|e| e.text.as_str()).collect::<Vec<_>>().join(" · ");
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
                cx.editor.set_status(format!("Julian day number: {}", julian_day(p)));
                return EventResult::Consumed(None);
            }
            key!('p') => {
                cx.editor.set_status(format!(
                    "Day {} of {}",
                    day_of_year(p),
                    p.year
                ));
                return EventResult::Consumed(None);
            }
            _ => {}
        }
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            ctrl!('f') | key!(Right) | key!('l') => self.point = add_days(self.point, 1),
            ctrl!('b') | key!(Left) | key!('h') => self.point = add_days(self.point, -1),
            ctrl!('n') | key!(Down) | key!('j') => self.point = add_days(self.point, 7),
            ctrl!('p') | key!(Up) | key!('k') => self.point = add_days(self.point, -7),
            ctrl!('a') => self.point = beginning_of_week(self.point),
            ctrl!('e') => self.point = end_of_week(self.point),
            alt!('}') | key!('>') | key!(PageDown) => self.point = add_months(self.point, 1),
            alt!('{') | key!('<') | key!(PageUp) => self.point = add_months(self.point, -1),
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

        surface.clear_with(area, bg);
        if area.width < 22 || area.height < 6 {
            return;
        }

        let p = self.point;
        let title = format!(" {} {}", MONTH_NAMES[(p.month - 1) as usize], p.year);
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "day C-f/b · week C-n/p · month M-{/} · year [/] · i/J/p print · d diary · q";
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
            let s = format!("{:>2}", d);
            let has_diary =
                zemacs_core::diary::has_entry(&self.diary, Date::new(p.year, p.month, d));
            let style = if d == p.day {
                sel_style
            } else if p.year == self.today.year
                && p.month == self.today.month
                && d == self.today.day
            {
                today_style
            } else if has_diary {
                diary_style
            } else {
                text_style
            };
            surface.set_stringn(x, y, &s, 2, style);
        }

        // Footer: full point date.
        if area.height >= 8 {
            let footer = format!(
                "{}  {} {}, {}  (day {} of {})",
                WEEKDAY_ABBR[weekday(p) as usize],
                MONTH_NAMES[(p.month - 1) as usize],
                p.day,
                p.year,
                day_of_year(p),
                p.year,
            );
            surface.set_stringn(
                area.x,
                area.y + area.height - 1,
                &footer,
                area.width as usize,
                info_style,
            );
        }
    }
}
