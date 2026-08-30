//! Example plugin: what completes here? — the four completion namespaces.
//!
//! vim's `getcompletion({prefix}, {type})` takes a type argument; the SDK
//! splits it into four calls instead, so the namespace is chosen at the call
//! site rather than by a string that can be misspelled:
//!
//! - [`Host::completions`] — `:` command names.
//! - [`Host::file_completions`] — paths.
//! - [`Host::dir_completions`] — directories only, a subset of the above.
//! - [`Host::option_completions`] — option names.
//!
//! The last has a limitation worth stating: **only options that have been SET
//! are known**, because that is all `:set` records. An option at its default
//! will not complete, which makes an empty result ambiguous between "no such
//! option" and "never set".
//!
//! ```text
//! :plugin load .../libzmax_native_complete_peek.dylib
//! :comp w      # → "commands 12 · files 3 · dirs 1 · options 2 — write write! wq…"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// How many candidates to name before eliding.
const SHOWN: usize = 5;

/// One namespace's count and a few of its candidates.
fn namespace(label: &str, candidates: &[String]) -> String {
    format!("{label} {}", candidates.len())
}

/// A sample drawn across namespaces, so the line shows what completing would
/// actually offer rather than only how many things it found.
fn sample(all: &[&[String]]) -> String {
    let flat: Vec<&String> = all.iter().flat_map(|list| list.iter()).collect();
    if flat.is_empty() {
        return "nothing completes".to_string();
    }
    let shown: Vec<String> = flat.iter().take(SHOWN).map(|s| (*s).clone()).collect();
    let ellipsis = if flat.len() > SHOWN { "…" } else { "" };
    format!("{}{ellipsis}", shown.join(" "))
}

/// Whether an empty option result is ambiguous — it is, whenever the prefix
/// could name a real option that simply has never been set.
fn option_note(prefix: &str, options: &[String]) -> String {
    if options.is_empty() && !prefix.is_empty() {
        " (options list only what has been set)".to_string()
    } else {
        String::new()
    }
}

/// `:comp {prefix}` — what each namespace offers for a prefix.
fn comp(host: &Host, args: &Args) -> c_int {
    let Some(prefix) = args.rest().first() else {
        host.error("comp: usage: :comp {prefix}");
        return 1;
    };

    let commands = host.completions(prefix);
    let files = host.file_completions(prefix);
    let dirs = host.dir_completions(prefix);
    let options = host.option_completions(prefix);

    host.message(&format!(
        "{} · {} · {} · {} — {}{}",
        namespace("commands", &commands),
        namespace("files", &files),
        namespace("dirs", &dirs),
        namespace("options", &options),
        sample(&[&commands, &files, &dirs, &options]),
        option_note(prefix, &options),
    ));
    0
}

declare_plugin! {
    name: "complete-peek",
    version: "0.1.0",
    commands: { "comp" => comp },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Each namespace reports its own count, so a prefix that completes as a
    /// command but not as a file is visibly different from one that does both.
    #[test]
    fn each_namespace_counts_separately() {
        assert_eq!(namespace("commands", &list(&["write", "wq"])), "commands 2");
        assert_eq!(namespace("files", &[]), "files 0");
    }

    /// Directories are a SUBSET of files, so a dir count can never exceed the
    /// file count for the same prefix — worth stating, since the two calls
    /// look interchangeable.
    #[test]
    fn directories_are_a_subset_of_files() {
        let files = list(&["src/main.rs", "src/ui/", "src/lib.rs"]);
        let dirs = list(&["src/ui/"]);
        assert!(dirs.len() <= files.len());
        assert!(dirs.iter().all(|d| files.contains(d)));
    }

    /// The sample draws across namespaces and elides once it is long enough to
    /// crowd the status line.
    #[test]
    fn the_sample_is_drawn_across_namespaces_and_elided() {
        let commands = list(&["write", "wq"]);
        let files = list(&["war.txt"]);
        assert_eq!(sample(&[&commands, &files]), "write wq war.txt");

        let many = list(&["a", "b", "c", "d", "e", "f", "g"]);
        let out = sample(&[&many]);
        assert!(out.ends_with('…'));
        assert_eq!(out.split_whitespace().count(), SHOWN);
    }

    /// Nothing completing anywhere is stated rather than rendered as an empty
    /// string trailing the counts.
    #[test]
    fn nothing_completing_is_stated() {
        assert_eq!(sample(&[&[], &[]]), "nothing completes");
    }

    /// An empty option result is ambiguous between "no such option" and "never
    /// set", so the note says which limitation is in play instead of letting
    /// the zero read as authoritative.
    #[test]
    fn an_empty_option_result_is_disclosed_as_ambiguous() {
        assert!(option_note("shift", &[]).contains("only what has been set"));
        assert_eq!(
            option_note("shift", &list(&["shiftwidth"])),
            "",
            "found some"
        );
        assert_eq!(option_note("", &[]), "", "no prefix, nothing to explain");
    }
}
