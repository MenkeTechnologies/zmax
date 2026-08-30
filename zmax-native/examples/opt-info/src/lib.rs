//! Example plugin: what is this option actually set to?
//!
//! The options family has one distinction that is easy to miss and impossible
//! to recover once lost: an option set to the empty string and an option never
//! set both read as "no value" through [`Host::option`]. Only
//! [`Host::option_set`] — vim's `exists("&opt")` — tells them apart.
//!
//! That matters because the two mean opposite things. `set backupext=` is a
//! deliberate choice; an unset option means the default applies.
//!
//! The typed readers ([`Host::option_num`], [`Host::option_bool`]) exist so
//! callers do not parse the string themselves and disagree about what `"0"`,
//! `"no"` or `""` mean.
//!
//! ```text
//! :plugin load .../libzmax_native_opt_info.dylib
//! :opt shiftwidth   # → "shiftwidth = 4  (set · number 4 · true)"
//! :opt nosuchopt    # → "nosuchopt is not set"
//! :opt shift        # no exact match → lists completions: shiftround shiftwidth
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// How an option's value reads, given the four things the SDK can say about it.
///
/// A pure function of those four so every branch — including the empty-but-set
/// case that motivated the plugin — is testable without an editor.
fn describe(
    name: &str,
    set: bool,
    value: Option<&str>,
    number: Option<usize>,
    boolean: bool,
) -> String {
    if !set {
        return format!("{name} is not set");
    }
    let shown = match value {
        // Set to empty is a real state and a deliberate one; saying "not set"
        // here would be wrong, and saying nothing at all would be confusing.
        None | Some("") => "(empty)".to_string(),
        Some(v) => v.to_string(),
    };
    let mut facts = vec!["set".to_string()];
    if let Some(n) = number {
        facts.push(format!("number {n}"));
    }
    facts.push(boolean.to_string());
    format!("{name} = {shown}  ({})", facts.join(" · "))
}

/// When a name is not an option, offer what it could have been.
fn suggestions(prefix: &str, matches: &[String]) -> String {
    if matches.is_empty() {
        format!("no option matches {prefix:?}")
    } else {
        format!("did you mean: {}", matches.join(" "))
    }
}

/// `:opt {name}` — describe one option, or list near matches.
fn opt(host: &Host, args: &Args) -> c_int {
    let Some(name) = args.rest().first() else {
        host.error("opt: usage: :opt {option}");
        return 1;
    };

    if host.option_set(name) {
        host.message(&describe(
            name,
            true,
            host.option(name).as_deref(),
            host.option_num(name),
            host.option_bool(name),
        ));
        return 0;
    }

    // Not set — which may mean the name is wrong. Only options that have been
    // set are known to completion, so this lists what is actually available.
    let matches = host.option_completions(name);
    if matches.is_empty() {
        host.message(&describe(name, false, None, None, false));
    } else {
        host.message(&suggestions(name, &matches));
    }
    0
}

declare_plugin! {
    name: "opt-info",
    version: "0.1.0",
    commands: { "opt" => opt },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case the plugin exists for: set-to-empty is NOT unset, and the two
    /// render differently.
    #[test]
    fn set_to_empty_is_not_unset() {
        let empty = describe("backupext", true, Some(""), None, false);
        assert!(empty.contains("(empty)"));
        assert!(!empty.contains("not set"), "it IS set");

        let unset = describe("backupext", false, None, None, false);
        assert_eq!(unset, "backupext is not set");
    }

    /// A missing value on a set option reads the same as an empty one — both
    /// mean "set, with nothing in it".
    #[test]
    fn a_set_option_with_no_value_reads_as_empty() {
        assert!(describe("x", true, None, None, false).contains("(empty)"));
    }

    /// The typed readings are shown alongside the raw string so a caller can
    /// see how the same value is interpreted, rather than parsing it again.
    #[test]
    fn typed_readings_accompany_the_raw_value() {
        let line = describe("shiftwidth", true, Some("4"), Some(4), true);
        assert!(line.contains("shiftwidth = 4"));
        assert!(line.contains("number 4"));
        assert!(line.contains("true"));
    }

    /// An option with no numeric reading simply omits it rather than inventing
    /// a zero.
    #[test]
    fn a_non_numeric_option_omits_the_number() {
        let line = describe("grepprg", true, Some("rg --vimgrep"), None, true);
        assert!(line.contains("rg --vimgrep"));
        assert!(!line.contains("number"), "no invented 0");
    }

    /// Near matches are offered when the name is unknown; an empty match list
    /// says so rather than offering nothing.
    #[test]
    fn unknown_names_get_suggestions_when_there_are_any() {
        let some = suggestions("shift", &["shiftround".into(), "shiftwidth".into()]);
        assert!(some.contains("shiftround shiftwidth"));
        assert!(suggestions("zzz", &[]).contains("no option matches"));
    }
}
