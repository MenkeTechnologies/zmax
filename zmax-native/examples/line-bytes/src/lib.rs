//! Example plugin: line ↔ byte, the conversion an external tool needs.
//!
//! Anything speaking to a language server, a compiler's error output or a
//! `git blame` is working in BYTES, while the editor works in characters. The
//! SDK provides the bridge at line granularity:
//!
//! - [`Host::line_to_byte`] — vim `line2byte()`, the byte a line starts at.
//! - [`Host::byte_to_line`] — vim `byte2line()`, the line a byte falls in.
//! - [`Host::text_range`] — the text between two CHAR offsets, for when you
//!   have the region and want what is in it.
//!
//! The pair is not symmetric. `line_to_byte` fails past the end (returning
//! `None`), while `byte_to_line` clamps — a byte past the end reports the last
//! line rather than nothing. Round-tripping a byte from an external tool that
//! disagrees with the buffer therefore lands you somewhere plausible instead of
//! failing loudly, which is worth knowing before trusting it.
//!
//! ```text
//! :plugin load .../libzmax_native_line_bytes.dylib
//! :lb          # the cursor's line as a byte range
//! :lb 4096     # which line byte 4096 falls in
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// The byte span of a line, given its start and the next line's start.
///
/// The last line has no following line, so its end comes from the buffer size
/// instead — the caller supplies whichever applies.
fn byte_span(start: usize, next_start: Option<usize>, buffer_bytes: usize) -> (usize, usize) {
    (start, next_start.unwrap_or(buffer_bytes))
}

/// Whether a byte offset is one the buffer can actually answer for.
///
/// `byte_to_line` clamps rather than failing, so a caller that wants to know
/// whether an external tool's offset is stale has to check the bound itself.
fn is_within(byte: usize, buffer_bytes: usize) -> bool {
    byte < buffer_bytes
}

/// The report for a byte offset that came from outside the editor.
fn lookup_report(byte: usize, line: usize, buffer_bytes: usize) -> String {
    if is_within(byte, buffer_bytes) {
        format!("byte {byte} is on line {}", line + 1)
    } else {
        // Clamped: the answer is a real line, but not one derived from this
        // byte, and treating it as such would silently point at the wrong place.
        format!(
            "byte {byte} is past the end ({buffer_bytes} bytes) — clamped to line {}",
            line + 1
        )
    }
}

/// The report for the current line.
fn line_report(line: usize, from: usize, to: usize) -> String {
    format!(
        "line {} spans bytes {from}..{to} ({} bytes)",
        line + 1,
        to.saturating_sub(from)
    )
}

/// `:lb [byte]` — the cursor's line as bytes, or which line a byte falls in.
fn line_bytes(host: &Host, args: &Args) -> c_int {
    // The buffer's byte length: the byte offset just past its last character.
    let buffer_bytes = match host.buffer_text() {
        Some(text) => text.len(),
        None => {
            host.error("lb: no active buffer");
            return 1;
        }
    };

    if let Some(arg) = args.rest().first() {
        let Ok(byte) = arg.parse::<usize>() else {
            host.error(&format!("lb: {arg:?} is not a byte offset"));
            return 1;
        };
        host.message(&lookup_report(byte, host.byte_to_line(byte), buffer_bytes));
        return 0;
    }

    let Some(cursor) = host.cursor() else {
        host.error("lb: no active buffer");
        return 1;
    };
    let Some(start) = host.line_to_byte(cursor.line) else {
        host.error("lb: that line has no byte offset");
        return 1;
    };
    // `line_to_byte` returns None past the end, which is how the last line is
    // detected without a separate line-count comparison.
    let (from, to) = byte_span(start, host.line_to_byte(cursor.line + 1), buffer_bytes);

    host.message(&line_report(cursor.line, from, to));
    0
}

declare_plugin! {
    name: "line-bytes",
    version: "0.1.0",
    commands: { "lb" => line_bytes },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ordinary line ends where the next begins.
    #[test]
    fn a_line_ends_where_the_next_starts() {
        assert_eq!(byte_span(100, Some(140), 5000), (100, 140));
    }

    /// The last line has no next, so it ends at the buffer's end — which is
    /// how `line_to_byte` returning None is put to use rather than treated as
    /// an error.
    #[test]
    fn the_last_line_ends_at_the_buffer() {
        assert_eq!(byte_span(4900, None, 5000), (4900, 5000));
    }

    /// The two conversions are NOT symmetric: `byte_to_line` clamps where
    /// `line_to_byte` fails, so an out-of-range byte still yields a line.
    #[test]
    fn byte_to_line_clamps_where_line_to_byte_fails() {
        assert!(is_within(4999, 5000));
        assert!(!is_within(5000, 5000), "one past the end is out");
        assert!(!is_within(99999, 5000));
    }

    /// A stale offset from an external tool is called out rather than silently
    /// reported as a line — the clamped answer is plausible and wrong.
    #[test]
    fn a_stale_external_offset_is_called_out() {
        let clamped = lookup_report(99999, 411, 5000);
        assert!(clamped.contains("past the end"));
        assert!(clamped.contains("clamped"));
        assert!(clamped.contains("5000 bytes"), "says what the bound was");

        let fine = lookup_report(4096, 300, 5000);
        assert!(!fine.contains("clamped"));
        assert!(fine.contains("line 301"), "1-based for display");
    }

    /// The line report gives both the span and its size, since a caller
    /// usually wants one or the other and computing it twice invites drift.
    #[test]
    fn the_line_report_carries_span_and_size() {
        let line = line_report(41, 1000, 1040);
        assert!(line.contains("line 42"));
        assert!(line.contains("1000..1040"));
        assert!(line.contains("40 bytes"));
    }
}
