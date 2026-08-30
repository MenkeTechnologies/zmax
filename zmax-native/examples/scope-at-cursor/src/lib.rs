//! Example plugin: what syntax scope is under the cursor?
//!
//! The question a theme author asks constantly — "this token is the wrong
//! colour, which scope do I style?" — answered from the editor's own
//! highlighter rather than by guessing from the grammar.
//!
//! Demonstrates [`Host::syntax_at`], which returns the whole scope STACK rather
//! than a single name. The outer entry answers "is this a comment"; the inner
//! one answers "what kind of comment". A theme rule can attach at either level,
//! so both are shown.
//!
//! ```text
//! :plugin load .../libzmax_native_scope_at_cursor.dylib
//! :scope        # → "comment › comment.line.documentation   (line 12, col 4)"
//! :scope-copy   # same, but yanked into the buffer as a comment
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// Render a scope stack outermost-first with a separator that reads as nesting.
///
/// The innermost scope is the most specific rule a theme can write, so it is
/// the one a caller usually wants; it is deliberately last, matching the order
/// the host returns and the order a theme file reads.
fn format_scopes(scopes: &[String]) -> String {
    if scopes.is_empty() {
        // Not an error: plain text, or a buffer with no grammar loaded, simply
        // has no scope at that offset.
        return "no syntax scope here".to_string();
    }
    scopes.join(" › ")
}

/// The most specific scope — what a theme rule should usually target.
fn innermost(scopes: &[String]) -> Option<&str> {
    scopes.last().map(String::as_str)
}

/// `:scope` — report the scope stack under the cursor on the status line.
fn scope(host: &Host, _args: &Args) -> c_int {
    let Some(cursor) = host.cursor() else {
        host.error("scope: no active buffer");
        return 1;
    };
    let scopes = host.syntax_at(cursor.offset);
    host.message(&format!(
        "{}   (line {}, col {})",
        format_scopes(&scopes),
        cursor.line + 1,
        cursor.column + 1,
    ));
    0
}

/// `:scope-copy` — insert the innermost scope at the cursor, for pasting
/// straight into a theme file.
fn scope_copy(host: &Host, _args: &Args) -> c_int {
    let Some(cursor) = host.cursor() else {
        host.error("scope-copy: no active buffer");
        return 1;
    };
    let scopes = host.syntax_at(cursor.offset);
    let Some(name) = innermost(&scopes) else {
        host.error("scope-copy: no syntax scope under the cursor");
        return 1;
    };
    host.insert_text(name);
    0
}

declare_plugin! {
    name: "scope-at-cursor",
    version: "0.1.0",
    commands: {
        "scope" => scope,
        "scope-copy" => scope_copy,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stack reads outermost to innermost, so a reader can see both the
    /// broad category and the specific rule.
    #[test]
    fn a_stack_renders_as_nesting() {
        let scopes = vec![
            "comment".to_string(),
            "comment.line".to_string(),
            "comment.line.documentation".to_string(),
        ];
        assert_eq!(
            format_scopes(&scopes),
            "comment › comment.line › comment.line.documentation"
        );
        assert_eq!(innermost(&scopes), Some("comment.line.documentation"));
    }

    /// A single scope needs no separator.
    #[test]
    fn one_scope_stands_alone() {
        let scopes = vec!["keyword".to_string()];
        assert_eq!(format_scopes(&scopes), "keyword");
        assert_eq!(innermost(&scopes), Some("keyword"));
    }

    /// No scope is an ordinary answer, not a failure: plain text and buffers
    /// with no grammar both land here.
    #[test]
    fn no_scope_is_not_an_error() {
        assert_eq!(format_scopes(&[]), "no syntax scope here");
        assert_eq!(innermost(&[]), None);
    }
}
