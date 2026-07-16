//! Emacs timeclock (`timeclock.el`): clock in/out time tracking.
//!
//! A timelog is a sequence of clock-in / clock-out events, each stamped with a
//! whole-second Unix time. This module is the pure, dependency-free model behind
//! the `timeclock-*` commands: parsing/serializing the log, and computing time
//! worked, the remaining workday, and when to leave. All timing is plain integer
//! arithmetic on Unix seconds — "today" is the UTC calendar day of the reference
//! time (`secs / 86400`), which keeps the model deterministic and testable with
//! no clock or timezone dependency. The command layer supplies the current time.

/// Seconds in a day; the Unix epoch starts on a UTC day boundary, so
/// `secs.div_euclid(SECS_PER_DAY)` is the UTC day index.
pub const SECS_PER_DAY: i64 = 86_400;

/// Emacs `timeclock-workday` default — an 8-hour workday, in seconds.
pub const DEFAULT_WORKDAY: i64 = 8 * 3600;

/// One timelog event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// `true` for a clock-in, `false` for a clock-out.
    pub clocked_in: bool,
    /// Event time, whole seconds since the Unix epoch.
    pub secs: i64,
    /// Project name (meaningful only for a clock-in).
    pub project: String,
}

/// A parsed timelog: the ordered list of clock events.
#[derive(Clone, Debug, Default)]
pub struct Timelog {
    entries: Vec<Entry>,
}

impl Timelog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a timelog: one event per line, `i|o <unix-secs> [project…]`.
    /// Lines that don't start with a valid code or lack a numeric time are
    /// skipped (mirroring emacs's tolerance of a partially written log).
    pub fn parse(text: &str) -> Self {
        let mut entries = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.splitn(3, char::is_whitespace);
            let clocked_in = match parts.next().unwrap_or("") {
                "i" | "I" => true,
                "o" | "O" => false,
                _ => continue,
            };
            let Some(secs) = parts.next().and_then(|s| s.parse::<i64>().ok()) else {
                continue;
            };
            let project = parts.next().unwrap_or("").trim().to_string();
            entries.push(Entry {
                clocked_in,
                secs,
                project,
            });
        }
        Self { entries }
    }

    /// Serialize back to the `i|o <secs> [project]` line format.
    pub fn serialize(&self) -> String {
        self.entries
            .iter()
            .map(|e| {
                let code = if e.clocked_in { 'i' } else { 'o' };
                if e.clocked_in && !e.project.is_empty() {
                    format!("{code} {} {}", e.secs, e.project)
                } else {
                    format!("{code} {}", e.secs)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether the last event is a clock-in (i.e. currently on the clock).
    pub fn is_clocked_in(&self) -> bool {
        self.entries.last().is_some_and(|e| e.clocked_in)
    }

    /// The project of the open clock-in, if currently clocked in.
    pub fn current_project(&self) -> Option<&str> {
        match self.entries.last() {
            Some(e) if e.clocked_in => Some(e.project.as_str()),
            _ => None,
        }
    }

    /// The time of the last event, if any.
    pub fn last_time(&self) -> Option<i64> {
        self.entries.last().map(|e| e.secs)
    }

    /// Record a clock-in for `project` at `secs`.
    pub fn clock_in(&mut self, secs: i64, project: &str) {
        self.entries.push(Entry {
            clocked_in: true,
            secs,
            project: project.to_string(),
        });
    }

    /// Record a clock-out at `secs`.
    pub fn clock_out(&mut self, secs: i64) {
        self.entries.push(Entry {
            clocked_in: false,
            secs,
            project: String::new(),
        });
    }

    /// Seconds worked whose intervals fall within the UTC day of `now_secs`. An
    /// open final clock-in is counted up to `now_secs`; each in→out interval is
    /// clipped to the day window so work spanning midnight counts only its
    /// in-day portion.
    pub fn elapsed_on_day(&self, now_secs: i64) -> i64 {
        let day_start = now_secs.div_euclid(SECS_PER_DAY) * SECS_PER_DAY;
        let day_end = day_start + SECS_PER_DAY;
        let mut total = 0i64;
        let mut i = 0;
        while i < self.entries.len() {
            if !self.entries[i].clocked_in {
                i += 1; // stray clock-out; skip
                continue;
            }
            let start = self.entries[i].secs;
            // The interval closes at the next clock-out, else it is still open.
            let (end, step) = match self.entries.get(i + 1) {
                Some(next) if !next.clocked_in => (next.secs, 2),
                _ => (now_secs.max(start), 1),
            };
            let s = start.max(day_start);
            let e = end.min(day_end);
            if e > s {
                total += e - s;
            }
            i += step;
        }
        total
    }

    /// Seconds left in the workday (`workday` minus today's elapsed, floored at 0).
    pub fn workday_remaining(&self, now_secs: i64, workday: i64) -> i64 {
        (workday - self.elapsed_on_day(now_secs)).max(0)
    }

    /// The Unix time at which the workday would be complete: now + remaining.
    pub fn when_to_leave(&self, now_secs: i64, workday: i64) -> i64 {
        now_secs + self.workday_remaining(now_secs, workday)
    }
}

/// Format a non-negative duration as `H:MM:SS` (emacs timeclock's display form).
pub fn format_duration(secs: i64) -> String {
    let secs = secs.max(0);
    format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_serialize_round_trip() {
        let text = "i 3600 zmax\no 7200\ni 10000 stryke";
        let log = Timelog::parse(text);
        assert_eq!(log.entries.len(), 3);
        assert!(log.is_clocked_in());
        assert_eq!(log.current_project(), Some("stryke"));
        assert_eq!(log.serialize(), text);
    }

    #[test]
    fn parse_skips_malformed_lines() {
        let log = Timelog::parse("i 100 a\ngarbage\nx 5\no notanumber\no 200");
        assert_eq!(log.entries.len(), 2); // only the two valid lines
        assert!(!log.is_clocked_in());
    }

    #[test]
    fn elapsed_sums_closed_and_open_intervals() {
        let mut log = Timelog::new();
        log.clock_in(3600, "p"); // 01:00
        log.clock_out(7200); //     02:00  -> 3600s worked
        assert_eq!(log.elapsed_on_day(7200), 3600);
        // Re-clock-in and leave it open; elapsed counts up to `now`.
        log.clock_in(9000, "p"); // 02:30
        assert_eq!(log.elapsed_on_day(10800), 3600 + 1800); // now 03:00
    }

    #[test]
    fn elapsed_clips_to_the_utc_day() {
        let mut log = Timelog::new();
        // In near end of day 0, out early in day 1.
        log.clock_in(SECS_PER_DAY - 400, "p");
        log.clock_out(SECS_PER_DAY + 3600);
        // Viewed from day 0: only the last 400s of day 0 count.
        assert_eq!(log.elapsed_on_day(SECS_PER_DAY - 1), 400);
        // Viewed from day 1: only the first 3600s of day 1 count.
        assert_eq!(log.elapsed_on_day(SECS_PER_DAY + 10), 3600);
    }

    #[test]
    fn workday_remaining_and_when_to_leave() {
        let mut log = Timelog::new();
        log.clock_in(0, "p");
        log.clock_out(2 * 3600); // worked 2h on day 0
        let now = 2 * 3600;
        assert_eq!(log.workday_remaining(now, DEFAULT_WORKDAY), 6 * 3600);
        assert_eq!(log.when_to_leave(now, DEFAULT_WORKDAY), now + 6 * 3600);
        // A full workday leaves nothing (floored at 0, not negative).
        log.clock_in(now, "p");
        log.clock_out(now + 10 * 3600);
        assert_eq!(log.workday_remaining(now + 10 * 3600, DEFAULT_WORKDAY), 0);
    }

    #[test]
    fn format_duration_is_h_mm_ss() {
        assert_eq!(format_duration(0), "0:00:00");
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_duration(-5), "0:00:00");
    }
}
