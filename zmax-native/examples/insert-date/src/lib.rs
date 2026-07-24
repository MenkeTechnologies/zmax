//! Example plugin: insert the current UTC date/time at the cursor.
//!
//! Demonstrates computing content in the plugin and writing it into the buffer
//! with `insert_text` — with zero dependencies (the calendar math is done here
//! rather than pulling `chrono`/`time`).
//!
//! ```text
//! :plugin load .../libzmax_native_insert_date.dylib
//! :date        # inserts e.g. 2026-07-17
//! :datetime    # inserts e.g. 2026-07-17T15:04:22Z
//! ```

use std::os::raw::c_int;
use std::time::{SystemTime, UNIX_EPOCH};

use zmax_native::{declare_plugin, Args, Host};

/// Civil (Y, M, D) from a count of days since the Unix epoch, using Howard
/// Hinnant's `civil_from_days` algorithm (valid for the proleptic Gregorian
/// calendar over the entire supported range).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

/// Now, as (year, month, day, hour, minute, second) in UTC.
fn now_utc() -> (i64, u32, u32, u32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (
        y,
        m,
        d,
        (sod / 3600) as u32,
        ((sod % 3600) / 60) as u32,
        (sod % 60) as u32,
    )
}

/// `:date` — insert `YYYY-MM-DD`.
fn insert_date(host: &Host, _args: &Args) -> c_int {
    let (y, m, d, ..) = now_utc();
    if host.insert_text(&format!("{y:04}-{m:02}-{d:02}")) {
        0
    } else {
        host.error("date: no active buffer");
        1
    }
}

/// `:datetime` — insert an ISO-8601 UTC timestamp `YYYY-MM-DDThh:mm:ssZ`.
fn insert_datetime(host: &Host, _args: &Args) -> c_int {
    let (y, m, d, hh, mm, ss) = now_utc();
    if host.insert_text(&format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")) {
        0
    } else {
        host.error("datetime: no active buffer");
        1
    }
}

declare_plugin! {
    name: "insert-date",
    version: "0.1.0",
    commands: {
        "date" => insert_date,
        "datetime" => insert_datetime,
    },
}
