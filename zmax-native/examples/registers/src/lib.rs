//! Example plugin: what is in the registers? — vim's `:registers`.
//!
//! Demonstrates [`Host::register`], which is `getreg({regname})`. Two things
//! about it shape this plugin:
//!
//! - A register holding several lines comes back with them JOINED by newlines,
//!   the way vim renders a list register. There is no separate line count, so
//!   the newlines are the only signal that a register is linewise-ish.
//! - The SDK does not report a register's TYPE. vim's `getregtype()` would say
//!   charwise / linewise / blockwise; zmax does not record it, so this plugin
//!   describes shape (one line vs several) rather than claiming a type it
//!   cannot know.
//!
//! ```text
//! :plugin load .../libzmax_native_registers.dylib
//! :regs        # the named registers that hold something
//! :regs abc    # only those
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// How much of a register's contents to show before eliding.
const PREVIEW_CELLS: usize = 28;

/// A register's contents as one previewable line.
///
/// Newlines become a visible marker rather than breaking the status line, and
/// the result is truncated on CHARACTER count with an ellipsis — a preview is
/// allowed to be approximate, but it must not smear across the display.
fn preview(contents: &str) -> String {
    let flattened = contents.replace('\n', "⏎");
    let mut out: String = flattened.chars().take(PREVIEW_CELLS).collect();
    if flattened.chars().count() > PREVIEW_CELLS {
        out.push('…');
    }
    out
}

/// Describe a register's shape without claiming a type the editor does not
/// record.
///
/// vim would say charwise/linewise/blockwise here; zmax keeps no register type,
/// so this reports what can actually be seen: how many lines it holds.
fn shape(contents: &str) -> String {
    let lines = contents.split('\n').count();
    if lines > 1 {
        format!("{lines} lines")
    } else {
        format!("{} chars", contents.chars().count())
    }
}

/// One rendered row, or `None` for an empty register — an empty register is
/// indistinguishable from an unset one through this API, so neither is listed.
fn row(name: char, contents: Option<&str>) -> Option<String> {
    let contents = contents?;
    if contents.is_empty() {
        return None;
    }
    Some(format!(
        "\"{name} [{}] {}",
        shape(contents),
        preview(contents)
    ))
}

/// Which registers to inspect: an explicit set, else the named ones plus the
/// numbered yank/delete ring and the special ones vim exposes.
fn register_set(arg: Option<&str>) -> Vec<char> {
    match arg {
        Some(explicit) => explicit.chars().collect(),
        None => ('a'..='z')
            .chain('0'..='9')
            // vim's unnamed, yank, and last-search registers.
            .chain(['"', '/', ':'])
            .collect(),
    }
}

/// `:regs [names]` — list the registers that hold something.
fn regs(host: &Host, args: &Args) -> c_int {
    let wanted = register_set(args.rest().first().map(String::as_str));

    let rows: Vec<String> = wanted
        .into_iter()
        .filter_map(|name| {
            let contents = host.register(name);
            row(name, contents.as_deref())
        })
        .collect();

    if rows.is_empty() {
        host.message("no registers hold anything");
    } else {
        host.message(&rows.join("  ·  "));
    }
    0
}

declare_plugin! {
    name: "registers",
    version: "0.1.0",
    commands: { "regs" => regs },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A multi-line register arrives newline-joined, and that is the only
    /// signal it holds more than one line — so the shape is read from it.
    #[test]
    fn newlines_are_the_only_signal_of_several_lines() {
        assert_eq!(shape("one\ntwo\nthree"), "3 lines");
        assert_eq!(shape("just one"), "8 chars");
    }

    /// Newlines are made visible rather than allowed to break the status line.
    #[test]
    fn newlines_are_shown_not_emitted() {
        let out = preview("a\nb");
        assert!(!out.contains('\n'), "never a real newline");
        assert!(out.contains('⏎'));
    }

    /// Long contents are elided so one register cannot fill the line.
    #[test]
    fn long_contents_are_elided() {
        let long = "x".repeat(100);
        let out = preview(&long);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), PREVIEW_CELLS + 1);
    }

    /// Empty and unset are indistinguishable through this API, so neither is
    /// listed — claiming a register is "set but empty" would be inventing a
    /// distinction the SDK cannot make for registers.
    #[test]
    fn empty_and_unset_are_both_omitted() {
        assert_eq!(row('a', None), None, "unset");
        assert_eq!(row('a', Some("")), None, "empty");
        assert!(row('a', Some("text")).is_some());
    }

    /// A row names the register the way vim does, with a leading quote.
    #[test]
    fn a_row_is_labelled_like_vim() {
        let out = row('q', Some("hello")).unwrap();
        assert!(out.starts_with("\"q "), "vim's register notation");
        assert!(out.contains("5 chars"));
        assert!(out.contains("hello"));
    }

    /// An explicit argument selects exactly those registers; the default set
    /// covers the named, numbered and special ones.
    #[test]
    fn the_register_set_is_selectable() {
        assert_eq!(register_set(Some("abq")), vec!['a', 'b', 'q']);
        let all = register_set(None);
        assert!(all.contains(&'a') && all.contains(&'z'));
        assert!(all.contains(&'0') && all.contains(&'9'));
        assert!(all.contains(&'"'), "the unnamed register");
        assert!(all.contains(&'/'), "the last search");
    }
}
