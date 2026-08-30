//! Example plugin: jump to a named mark, and say what happened.
//!
//! Two ways to read marks, for two different jobs:
//!
//! - [`Host::mark`] — one mark by name, `None` when never set. What you want
//!   when the user named a specific one.
//! - [`Host::marks`] — every set mark. What you want to list or to offer a
//!   choice.
//!
//! Asking for one and falling back to listing the rest is friendlier than
//! failing, so an unset mark answers with what IS set.
//!
//! Movement goes through [`Host::eval`] and `:goto`, which is **1-based** while
//! the SDK is 0-based. That conversion is the single most likely off-by-one in
//! a plugin that navigates, so it happens once and is named.
//!
//! ```text
//! :plugin load .../libzmax_native_mark_jump.dylib
//! :mj a   # jump to mark a
//! :mj     # list the marks that are set
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// The `:goto` argument for a 0-based line.
///
/// `:goto` parses into a `NonZeroUsize`, so line 0 is not addressable and the
/// conversion is mandatory rather than cosmetic.
fn goto_line_arg(zero_based_line: usize) -> usize {
    zero_based_line + 1
}

/// A mark name must be a single character; anything else is a user error worth
/// naming rather than silently taking the first char.
fn parse_mark_name(arg: &str) -> Result<char, String> {
    let mut chars = arg.chars();
    match (chars.next(), chars.next()) {
        (Some(name), None) => Ok(name),
        (Some(_), Some(_)) => Err(format!(
            "{arg:?} is not a mark name — marks are one character"
        )),
        (None, _) => Err("no mark name given".to_string()),
    }
}

/// What to say when the requested mark is not set: name the ones that are.
fn unset_message(name: char, set: &[char]) -> String {
    if set.is_empty() {
        format!("mark '{name}' is not set (no marks are)")
    } else {
        let names: String = set.iter().collect::<Vec<_>>().iter().copied().collect();
        format!("mark '{name}' is not set — these are: {names}")
    }
}

/// `:mj [name]` — jump to a mark, or list the marks that are set.
fn mark_jump(host: &Host, args: &Args) -> c_int {
    let set: Vec<char> = host.marks().into_iter().map(|(name, _, _)| name).collect();

    let Some(arg) = args.rest().first() else {
        if set.is_empty() {
            host.message("no marks are set");
        } else {
            let names: String = set.iter().copied().collect();
            host.message(&format!("marks set: {names}"));
        }
        return 0;
    };

    let name = match parse_mark_name(arg) {
        Ok(name) => name,
        Err(complaint) => {
            host.error(&complaint);
            return 1;
        }
    };

    let Some(span) = host.mark(name) else {
        host.message(&unset_message(name, &set));
        return 0;
    };

    // `:goto` counts from 1; the SDK counts from 0.
    host.eval(&format!("goto {}", goto_line_arg(span.line)));
    0
}

declare_plugin! {
    name: "mark-jump",
    version: "0.1.0",
    commands: { "mj" => mark_jump },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `:goto` takes a NonZeroUsize, so the first line is 1 and the conversion
    /// is required rather than decorative.
    #[test]
    fn goto_is_one_based() {
        assert_eq!(goto_line_arg(0), 1, "the first line is addressable as 1");
        assert_eq!(goto_line_arg(41), 42);
    }

    /// A mark name is exactly one character; two is a mistake worth naming
    /// rather than truncating to the first.
    #[test]
    fn a_mark_name_is_one_character() {
        assert_eq!(parse_mark_name("a"), Ok('a'));
        assert!(parse_mark_name("ab").unwrap_err().contains("one character"));
        assert!(parse_mark_name("").unwrap_err().contains("no mark name"));
    }

    /// Non-ASCII marks are still single characters and are accepted as such.
    #[test]
    fn a_single_wide_character_is_still_one_name() {
        assert_eq!(parse_mark_name("é"), Ok('é'));
    }

    /// An unset mark answers with what IS set, which is more use than a bare
    /// failure.
    #[test]
    fn an_unset_mark_offers_the_ones_that_are() {
        let msg = unset_message('q', &['a', 'b']);
        assert!(msg.contains("'q' is not set"));
        assert!(msg.contains("ab"), "names the alternatives");
    }

    /// With no marks at all, saying so is clearer than offering an empty list.
    #[test]
    fn no_marks_at_all_is_stated() {
        assert!(unset_message('q', &[]).contains("no marks are"));
    }
}
