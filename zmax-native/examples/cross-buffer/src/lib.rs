//! Example plugin: read another buffer without switching to it.
//!
//! Most of the SDK reads the CURRENT buffer. A handful of calls take a buffer
//! index instead, and together they let a plugin inspect a file the user is not
//! looking at — no switching, no side effects on the jump list, no disturbing
//! the view.
//!
//! - [`Host::buffer_line`] — vim `getbufline()`, a line of any open buffer.
//! - [`Host::buffer_path_at`] / [`Host::buffer_language`] /
//!   [`Host::buffer_modified`] — the same facts `language()` and friends give
//!   for the current one.
//!
//! Note that [`Host::buffer_index`] resolves a name to an index in one call.
//! This plugin does the lookup itself only because it refuses ambiguity — see
//! `resolve` below.
//!
//! Buffer indices are positions in the open-buffer list, not stable ids: closing
//! a buffer renumbers the ones after it. An index is only good for as long as
//! the command holding it runs, which is why this plugin resolves a NAME to an
//! index on every invocation rather than remembering one.
//!
//! ```text
//! :plugin load .../libzmax_native_cross_buffer.dylib
//! :peek           # list the open buffers
//! :peek main 42   # line 42 of the buffer whose name matches "main"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// Resolve a name fragment to a buffer index.
///
/// [`Host::buffer_index`] already does substring matching — `main` finds
/// `src/main.rs` — and for most plugins it is the right call. This deliberately
/// does NOT use it: `buffer_index` returns the FIRST buffer whose name matches,
/// and a command that reads or edits a file must not pick between
/// `src/main.rs` and `tests/main_test.rs` on its own. Ambiguity is refused with
/// both candidates named instead.
///
/// Reach for `buffer_index` when a wrong-but-plausible match is harmless, and
/// for something like this when it is not.
fn resolve(names: &[String], fragment: &str) -> Result<usize, String> {
    let matches: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_index, name)| name.contains(fragment))
        .map(|(index, _name)| index)
        .collect();

    match matches.as_slice() {
        [] => Err(format!("no open buffer matches {fragment:?}")),
        [only] => Ok(*only),
        several => Err(format!(
            "{fragment:?} matches {} buffers: {}",
            several.len(),
            several
                .iter()
                .filter_map(|i| names.get(*i).cloned())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// One row of the buffer listing, in the shape `:ls` uses.
fn listing_row(index: usize, name: &str, language: Option<&str>, modified: bool) -> String {
    format!(
        "{index}{} {name}{}",
        if modified { "+" } else { " " },
        match language {
            Some(lang) => format!(" [{lang}]"),
            None => String::new(),
        }
    )
}

/// `:peek [name] [line]` — list buffers, or show one line of one of them.
fn peek(host: &Host, args: &Args) -> c_int {
    let names = host.buffer_names();

    let Some(fragment) = args.rest().first() else {
        // No argument: list what is open.
        let rows: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                listing_row(
                    index,
                    name,
                    host.buffer_language(index).as_deref(),
                    host.buffer_modified(index),
                )
            })
            .collect();
        host.message(&rows.join("  ·  "));
        return 0;
    };

    let index = match resolve(&names, fragment) {
        Ok(index) => index,
        Err(complaint) => {
            host.error(&complaint);
            return 1;
        }
    };

    // Lines are 1-based on the command line and 0-based in the SDK.
    let line = args
        .rest()
        .get(1)
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(1);
    let zero_based = line.saturating_sub(1);

    match host.buffer_line(index, zero_based) {
        Some(text) => host.message(&format!(
            "{}:{line}: {text}",
            host.buffer_path_at(index)
                .unwrap_or_else(|| "?".to_string())
        )),
        None => host.error(&format!("line {line} is past the end of that buffer")),
    }
    0
}

declare_plugin! {
    name: "cross-buffer",
    version: "0.1.0",
    commands: { "peek" => peek },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "tests/main_test.rs".to_string(),
        ]
    }

    /// A fragment resolves by substring, the way `bufnr()` does.
    #[test]
    fn a_fragment_resolves_by_substring() {
        assert_eq!(resolve(&names(), "lib"), Ok(1));
    }

    /// An ambiguous fragment is refused rather than resolved to the first
    /// match — arbitrarily picking one is how a plugin edits the wrong file.
    #[test]
    fn ambiguity_is_refused_not_guessed() {
        let err = resolve(&names(), "main").unwrap_err();
        assert!(err.contains("matches 2 buffers"));
        assert!(err.contains("src/main.rs"));
        assert!(err.contains("tests/main_test.rs"), "both are named");
    }

    /// A fragment matching nothing says so.
    #[test]
    fn no_match_is_reported() {
        assert!(resolve(&names(), "nothing")
            .unwrap_err()
            .contains("no open buffer"));
    }

    /// Modified buffers are marked, and the language is shown when known —
    /// the same facts the current buffer reports, for one you are not in.
    #[test]
    fn a_listing_row_carries_the_buffers_facts() {
        assert_eq!(
            listing_row(0, "src/main.rs", Some("rust"), true),
            "0+ src/main.rs [rust]"
        );
        assert_eq!(
            listing_row(1, "notes.txt", None, false),
            "1  notes.txt",
            "no language, not modified"
        );
    }
}
