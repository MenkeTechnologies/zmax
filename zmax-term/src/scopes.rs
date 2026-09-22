//! Named search scopes — JetBrains' scopes (`ScopeView.EditScopes`,
//! `AddToScopeAction`).
//!
//! A scope is a name bound to a file glob: `tests` for `tests/**`, `rust` for
//! `*.rs`. The IDE uses them to narrow searches, inspections and replaces to a
//! part of the project without retyping the pattern each time; here they narrow
//! project search, which is where the mask already applies.
//!
//! Scopes live in `<config-dir>/scopes.toml` — a plain `name = "glob"` table,
//! so they survive restarts and can be edited by hand. A scope that names a
//! glob the matcher rejects is kept in the file and reported when used, rather
//! than dropped on load: silently losing a scope someone wrote is worse than
//! telling them it does not compile.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// name -> glob.
pub type Scopes = BTreeMap<String, String>;

fn store_path() -> PathBuf {
    zmax_loader::config_dir().join("scopes.toml")
}

/// Parse the store. Lines that are not `name = "glob"` are skipped, so a
/// hand-edited file with a comment or a stray blank line still loads. Pure —
/// unit tested.
pub fn parse(text: &str) -> Scopes {
    let mut out = Scopes::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, glob)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"').to_string();
        let glob = glob.trim().trim_matches('"').to_string();
        if !name.is_empty() && !glob.is_empty() {
            out.insert(name, glob);
        }
    }
    out
}

/// Render the store back. Sorted by name (a `BTreeMap` already is), one entry
/// per line, quoted so a glob containing `#` or spaces round-trips. Pure —
/// unit tested.
pub fn render(scopes: &Scopes) -> String {
    let mut out = String::new();
    for (name, glob) in scopes {
        out.push_str(&format!("{name} = \"{glob}\"\n"));
    }
    out
}

/// Load the saved scopes (empty when the file is missing).
pub fn load() -> Scopes {
    parse(&std::fs::read_to_string(store_path()).unwrap_or_default())
}

/// Write the scopes back.
pub fn save(scopes: &Scopes) -> std::io::Result<()> {
    std::fs::write(store_path(), render(scopes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_render_round_trip() {
        let text = "rust = \"*.rs\"\ntests = \"tests/**\"\n";
        let scopes = parse(text);
        assert_eq!(scopes["rust"], "*.rs");
        assert_eq!(scopes["tests"], "tests/**");
        assert_eq!(render(&scopes), text);
    }

    #[test]
    fn parse_skips_what_it_cannot_read_rather_than_failing() {
        let scopes = parse("# comment\n\nnonsense\nrust = \"*.rs\"\n= \"no name\"\nempty =\n");
        assert_eq!(scopes.len(), 1, "only the real entry survives: {scopes:?}");
        assert_eq!(scopes["rust"], "*.rs");
    }

    #[test]
    fn a_glob_with_spaces_survives_the_round_trip() {
        let mut scopes = Scopes::new();
        scopes.insert("docs".into(), "docs/**/*.md".into());
        scopes.insert("odd".into(), "a b/*.txt".into());
        assert_eq!(parse(&render(&scopes)), scopes);
    }
}
