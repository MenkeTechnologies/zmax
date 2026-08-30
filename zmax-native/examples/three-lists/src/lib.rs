//! Example plugin: the quickfix list, the location list, and the diagnostics
//! are three DIFFERENT lists.
//!
//! This trips people up, so the plugin exists mostly to make the distinction
//! visible:
//!
//! - [`Host::quickfix`] — vim's quickfix list, filled by `:grep`/`:make`, and
//!   the one `:cnext` walks. Global: one per editor.
//! - [`Host::loclist`] — the same idea scoped to a WINDOW, driven by `:lopen`
//!   and friends. Switch windows and it changes.
//! - [`Host::diagnostics`] — what the language server reported. vim has no
//!   equivalent, and `:cnext` does not walk these.
//!
//! Reading one when you meant another is silent, which is exactly why it is
//! worth showing all three side by side.
//!
//! ```text
//! :plugin load .../libzmax_native_three_lists.dylib
//! :lists   # → "quickfix 12 · loclist 0 · diagnostics 3 — first qf: src/main.rs:42"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host, QfItem};

/// `path:line` for the first entry of a list, or a dash when it is empty.
///
/// Quickfix line/column are 1-based already — they come from `:grep` output,
/// which counts from 1 — so they are printed as-is rather than adjusted like
/// the char offsets elsewhere in the SDK.
fn first_entry(list: &[QfItem]) -> String {
    match list.first() {
        Some(item) => format!("{}:{}", item.path, item.line),
        None => "—".to_string(),
    }
}

/// One line summarising all three lists.
fn summary(quickfix: &[QfItem], loclist: &[QfItem], diagnostics: usize) -> String {
    format!(
        "quickfix {} · loclist {} · diagnostics {} — first qf: {}, first loc: {}",
        quickfix.len(),
        loclist.len(),
        diagnostics,
        first_entry(quickfix),
        first_entry(loclist),
    )
}

/// `:lists` — show the size and head of each of the three lists.
fn lists(host: &Host, _args: &Args) -> c_int {
    let quickfix = host.quickfix();
    let loclist = host.loclist();
    let diagnostics = host.diagnostics().len();
    host.message(&summary(&quickfix, &loclist, diagnostics));
    0
}

declare_plugin! {
    name: "three-lists",
    version: "0.1.0",
    commands: { "lists" => lists },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, line: usize) -> QfItem {
        QfItem {
            path: path.to_string(),
            line,
            col: 1,
            text: "message".to_string(),
        }
    }

    /// The counts are reported independently, so a reader can see at a glance
    /// that the lists are unrelated rather than three views of one thing.
    #[test]
    fn the_three_counts_are_independent() {
        let qf = vec![item("src/main.rs", 42), item("src/lib.rs", 7)];
        let loc = vec![item("README.md", 3)];
        let line = summary(&qf, &loc, 5);

        assert!(line.contains("quickfix 2"));
        assert!(line.contains("loclist 1"));
        assert!(line.contains("diagnostics 5"));
        assert!(line.contains("src/main.rs:42"), "quickfix head");
        assert!(
            line.contains("README.md:3"),
            "loclist head, a different file"
        );
    }

    /// An empty list reads as a dash rather than as a missing field, so an
    /// empty quickfix cannot be mistaken for a failure to read it.
    #[test]
    fn an_empty_list_is_a_dash() {
        assert_eq!(first_entry(&[]), "—");
        let line = summary(&[], &[], 0);
        assert!(line.contains("quickfix 0"));
        assert!(line.contains("first qf: —"));
    }

    /// Quickfix positions stay 1-based, matching the `:grep` output they came
    /// from — they are not char offsets and are not adjusted.
    #[test]
    fn quickfix_lines_are_printed_as_given() {
        assert_eq!(first_entry(&[item("a.rs", 1)]), "a.rs:1");
        assert_eq!(first_entry(&[item("a.rs", 999)]), "a.rs:999");
    }
}
