//! Example plugin: where have I been in this buffer?
//!
//! Three separate histories answer three different questions, and the editor
//! keeps them apart on purpose:
//!
//! - [`Host::marks`] — places you NAMED, with `ma` and friends.
//! - [`Host::jumps`] — places you JUMPED FROM, unwound by `CTRL-O`.
//! - [`Host::changelist`] — places you EDITED, revisited with `g;`.
//!
//! The jump list has a wrinkle worth showing: a jump outlives the buffer it
//! points into, so its buffer is `None` once that buffer is closed. Reporting a
//! stale index into whatever now sits in that slot would be worse than
//! admitting the buffer is gone, so the SDK admits it and this plugin prints it
//! as `(closed)`.
//!
//! ```text
//! :plugin load .../libzmax_native_nav_history.dylib
//! :nav   # → "marks a,b,q · jumps 4 (at 2, 1 closed) · changes 7 (at 7, the end)"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host, Span};

/// Mark names, comma-separated. Marks are already sorted by name.
fn mark_names(marks: &[(char, usize, usize)]) -> String {
    if marks.is_empty() {
        return "none".to_string();
    }
    marks
        .iter()
        .map(|(name, _offset, _line)| name.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Summarise the jump list, calling out entries whose buffer has been closed.
///
/// Those are the interesting ones: `CTRL-O` can still walk to them, but nothing
/// can resolve which buffer they meant.
fn jump_summary(jumps: &[(Span, Option<usize>)], index: usize) -> String {
    if jumps.is_empty() {
        return "jumps none".to_string();
    }
    let closed = jumps.iter().filter(|(_span, buf)| buf.is_none()).count();
    let mut out = format!("jumps {} (at {index}", jumps.len());
    if closed > 0 {
        out.push_str(&format!(", {closed} closed"));
    }
    out.push(')');
    out
}

/// Summarise the change list, noting when the cursor sits past the last entry —
/// the state you are in before pressing `g;` for the first time.
fn change_summary(changes: &[Span], index: usize) -> String {
    if changes.is_empty() {
        return "changes none".to_string();
    }
    if index >= changes.len() {
        format!("changes {} (at {index}, the end)", changes.len())
    } else {
        format!("changes {} (at {index})", changes.len())
    }
}

/// `:nav` — one line covering all three histories.
fn nav(host: &Host, _args: &Args) -> c_int {
    let marks = host.marks();
    let jumps = host.jumps();
    let changes = host.changelist();
    host.message(&format!(
        "marks {} · {} · {}",
        mark_names(&marks),
        jump_summary(&jumps, host.jump_index()),
        change_summary(&changes, host.changelist_index()),
    ));
    0
}

declare_plugin! {
    name: "nav-history",
    version: "0.1.0",
    commands: { "nav" => nav },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(anchor: usize) -> Span {
        Span {
            anchor,
            head: anchor,
            line: 0,
            valid: 1,
        }
    }

    /// Marks come back sorted by name, so the rendering keeps that order rather
    /// than imposing its own.
    #[test]
    fn marks_render_in_the_order_given() {
        let marks = vec![('a', 10, 1), ('b', 20, 2), ('q', 30, 3)];
        assert_eq!(mark_names(&marks), "a,b,q");
        assert_eq!(mark_names(&[]), "none");
    }

    /// A jump into a closed buffer is counted and called out — it is still a
    /// jump, but nothing can say which buffer it meant.
    #[test]
    fn closed_buffers_are_called_out() {
        let jumps = vec![(span(5), None), (span(90), Some(0)), (span(120), Some(1))];
        let line = jump_summary(&jumps, 2);
        assert!(line.contains("jumps 3"));
        assert!(line.contains("at 2"));
        assert!(line.contains("1 closed"));
    }

    /// With every buffer still open there is nothing to call out, so the
    /// summary stays quiet rather than printing "0 closed".
    #[test]
    fn no_closed_buffers_means_no_note() {
        let jumps = vec![(span(5), Some(0)), (span(9), Some(0))];
        let line = jump_summary(&jumps, 1);
        assert!(line.contains("jumps 2"));
        assert!(!line.contains("closed"));
        assert_eq!(jump_summary(&[], 0), "jumps none");
    }

    /// Sitting past the last change is the normal state before `g;` — worth
    /// distinguishing from sitting on an entry.
    #[test]
    fn the_change_cursor_can_sit_past_the_end() {
        let changes = vec![span(1), span(2), span(3)];
        assert!(change_summary(&changes, 3).contains("the end"));
        assert!(!change_summary(&changes, 1).contains("the end"));
        assert_eq!(change_summary(&[], 0), "changes none");
    }
}
