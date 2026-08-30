//! Example plugin: what is loaded, and is my command name already taken?
//!
//! A plugin author's own concern, and one the SDK can answer before the
//! collision bites. [`Host::command_exists`] covers built-ins AND plugin
//! commands, so it is how a plugin checks whether the name it wants is free.
//!
//! Command resolution matters here: a plugin command is unknown to the static
//! table, so it resolves in the `:`-dispatcher's fallthrough — AFTER built-in
//! typable commands. A plugin registering `write` therefore does not shadow
//! `:write`; it becomes unreachable instead. Checking first is the only way to
//! find that out without wondering why your command never runs.
//!
//! ```text
//! :plugin load .../libzmax_native_plugin_doctor.dylib
//! :plugins            # what is loaded, and the editor's pid
//! :plugins-free write # → "'write' is TAKEN — a plugin command would be unreachable"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// The verdict on a candidate command name.
///
/// Worded around the consequence rather than the fact: "taken" understates it,
/// because the plugin command is not rejected, it simply never runs.
fn name_verdict(name: &str, taken: bool) -> String {
    if taken {
        format!("{name:?} is TAKEN — a plugin command by that name would be unreachable")
    } else {
        format!("{name:?} is free")
    }
}

/// The loaded-plugin listing.
fn listing(plugins: &[String], pid: u32) -> String {
    if plugins.is_empty() {
        return format!("no native plugins loaded (editor pid {pid})");
    }
    format!(
        "{} loaded: {} (editor pid {pid})",
        plugins.len(),
        plugins.join(", ")
    )
}

/// `:plugins` — what native plugins are loaded.
fn plugins(host: &Host, _args: &Args) -> c_int {
    host.message(&listing(&host.plugin_names(), host.pid()));
    0
}

/// `:plugins-free {name}` — is a command name available?
fn plugins_free(host: &Host, args: &Args) -> c_int {
    let Some(name) = args.rest().first() else {
        host.error("plugins-free: usage: :plugins-free {command}");
        return 1;
    };
    host.message(&name_verdict(name, host.command_exists(name)));
    0
}

declare_plugin! {
    name: "plugin-doctor",
    version: "0.1.0",
    commands: {
        "plugins" => plugins,
        "plugins-free" => plugins_free,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A taken name does not merely conflict — the plugin command becomes
    /// unreachable, because plugin commands resolve AFTER built-ins. The
    /// wording says so, since "taken" would understate the consequence.
    #[test]
    fn a_taken_name_means_unreachable_not_rejected() {
        let taken = name_verdict("write", true);
        assert!(taken.contains("TAKEN"));
        assert!(taken.contains("unreachable"), "the actual consequence");
        assert!(
            !taken.contains("rejected"),
            "it is not refused, it is shadowed"
        );
    }

    /// A free name is stated plainly.
    #[test]
    fn a_free_name_is_plain() {
        assert_eq!(
            name_verdict("zwire-lookup", false),
            "\"zwire-lookup\" is free"
        );
    }

    /// The listing names what is loaded and the editor's pid, which is what
    /// you need to attach a debugger to the right process.
    #[test]
    fn the_listing_carries_the_pid() {
        let out = listing(&["hello".to_string(), "banner".to_string()], 4242);
        assert!(out.contains("2 loaded"));
        assert!(out.contains("hello, banner"));
        assert!(out.contains("pid 4242"));
    }

    /// An empty list still reports the pid — the plugin asking is itself
    /// loaded, so an empty answer is a signal worth seeing in full.
    #[test]
    fn an_empty_listing_still_reports_the_pid() {
        let out = listing(&[], 99);
        assert!(out.contains("no native plugins loaded"));
        assert!(out.contains("pid 99"));
    }
}
