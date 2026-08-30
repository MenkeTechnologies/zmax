//! Example plugin: "the thing under the cursor" has three different answers.
//!
//! Put the cursor on `src/main.rs` in a comment and ask each of these:
//!
//! - [`Host::word_at_cursor`] — vim `expand("<cword>")`. Stops at punctuation,
//!   so it returns `src`.
//! - [`Host::long_word_at_cursor`] — vim `expand("<cWORD>")`. Whitespace
//!   delimited, so it returns `src/main.rs` — and would also swallow a trailing
//!   comma or bracket.
//! - [`Host::file_at_cursor`] — vim `expand("<cfile>")`, by `isfname` rules,
//!   the same machinery `gf` uses. Returns `src/main.rs` and keeps the path
//!   intact WITHOUT swallowing adjacent punctuation.
//!
//! Choosing the wrong one is a quiet bug: a lookup plugin using `<cword>` on a
//! qualified name silently searches for the first fragment.
//!
//! ```text
//! :plugin load .../libzmax_native_under_cursor.dylib
//! :under   # → "word 'src' · WORD 'src/main.rs,' · file 'src/main.rs' — all three differ"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// Render one answer, or mark it absent — on whitespace, `word_at_cursor` has
/// nothing to return, and that is a real answer rather than an error.
fn show(label: &str, value: Option<&str>) -> String {
    match value {
        Some(text) => format!("{label} {text:?}"),
        None => format!("{label} —"),
    }
}

/// How many DISTINCT answers the three calls gave.
///
/// The interesting cases are the extremes: all three agreeing means a bare
/// identifier with nothing around it, while three different answers means the
/// choice of call decides what a plugin operates on.
fn agreement(word: Option<&str>, long: Option<&str>, file: Option<&str>) -> String {
    let mut distinct: Vec<&str> = [word, long, file].into_iter().flatten().collect();
    distinct.sort_unstable();
    distinct.dedup();

    match distinct.len() {
        0 => "nothing under the cursor".to_string(),
        1 => "all three agree".to_string(),
        2 => "two of the three differ".to_string(),
        _ => "all three differ".to_string(),
    }
}

/// `:under` — the three readings at the cursor.
fn under(host: &Host, _args: &Args) -> c_int {
    if host.cursor().is_none() {
        host.error("under: no active buffer");
        return 1;
    }
    let word = host.word_at_cursor();
    let long = host.long_word_at_cursor();
    let file = host.file_at_cursor();

    host.message(&format!(
        "{} · {} · {} — {}",
        show("word", word.as_deref()),
        show("WORD", long.as_deref()),
        show("file", file.as_deref()),
        agreement(word.as_deref(), long.as_deref(), file.as_deref()),
    ));
    0
}

declare_plugin! {
    name: "under-cursor",
    version: "0.1.0",
    commands: { "under" => under },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The motivating case: a path in prose gives three different answers, and
    /// only one of them is the path.
    #[test]
    fn a_path_in_prose_splits_the_three() {
        let word = Some("src");
        let long = Some("src/main.rs,");
        let file = Some("src/main.rs");
        assert_eq!(agreement(word, long, file), "all three differ");

        // `<cword>` truncates at the punctuation — the quiet bug.
        assert_ne!(word, file);
        // `<cWORD>` keeps the comma; only `<cfile>` gets the path exactly.
        assert_ne!(long, file);
    }

    /// A bare identifier with nothing around it reads the same three ways,
    /// which is why the bug hides: it works until it does not.
    #[test]
    fn a_bare_identifier_reads_the_same_three_ways() {
        assert_eq!(
            agreement(Some("total"), Some("total"), Some("total")),
            "all three agree"
        );
    }

    /// Two agreeing and one differing is its own case — a trailing bracket
    /// separates WORD from the other two.
    #[test]
    fn a_trailing_bracket_separates_only_the_word() {
        let out = agreement(Some("total"), Some("total)"), Some("total"));
        assert_eq!(out, "two of the three differ");
    }

    /// On whitespace there is nothing to report, and that is an answer rather
    /// than a failure.
    #[test]
    fn whitespace_yields_nothing_and_that_is_fine() {
        assert_eq!(agreement(None, None, None), "nothing under the cursor");
        assert_eq!(show("word", None), "word —");
    }

    /// Values are quoted so leading or trailing spaces in an answer are
    /// visible rather than lost against the separators.
    #[test]
    fn values_are_quoted_so_edges_are_visible() {
        assert_eq!(show("WORD", Some("a b")), r#"WORD "a b""#);
    }
}
