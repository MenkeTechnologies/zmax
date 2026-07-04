//! End-to-end: the serial plotter's live pipeline. Spawns a real subprocess
//! that emits Arduino-plotter-format lines on stdout, and proves the background
//! reader thread parses them into the plot model — the exact path a real
//! `arduino-cli monitor` feed takes, minus the physical board.

#![cfg(unix)]

use std::time::{Duration, Instant};
use zemacs_term::ui::serial_plotter::SerialPlotter;

#[test]
fn plotter_ingests_a_live_serial_stream() {
    // A stand-in for `arduino-cli monitor`: emit three two-channel samples.
    let argv = vec![
        "sh".to_string(),
        "-c".to_string(),
        "printf '1 2\\n3 4\\n5 6\\n'".to_string(),
    ];
    let plotter = SerialPlotter::spawn(&argv, "test").expect("spawn stand-in monitor");

    // Poll the live snapshot until the reader thread has consumed all 3 lines.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (samples, channels) = plotter.snapshot();
        if samples >= 3 {
            assert_eq!(channels, 2, "two numeric channels per line");
            return;
        }
        assert!(
            Instant::now() < deadline,
            "reader thread never ingested the stream (got {samples})"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn plotter_ignores_non_numeric_log_lines() {
    let argv = vec![
        "sh".to_string(),
        "-c".to_string(),
        "printf 'Booting...\\n10\\nsensor ready\\n20\\n'".to_string(),
    ];
    let plotter = SerialPlotter::spawn(&argv, "test").expect("spawn");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (samples, channels) = plotter.snapshot();
        // Only the two numeric lines (10, 20) count as samples; log lines are dropped.
        if samples >= 2 {
            assert_eq!(channels, 1);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "expected 2 numeric samples, got {samples}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
