//! User-defined snippet library.
//!
//! Each snippet is a trigger word + a scope (a language id, or `*` for all
//! languages) + a human description + an LSP-style snippet body (the syntax the
//! [`zemacs_core::snippets::Snippet`] engine parses, e.g. `for ${1:i} { $0 }`).
//! The library persists globally to `<config-dir>/snippets.toml` (alongside
//! `appdata.toml`), tolerant of a missing file (loads as an empty store). The
//! editor TUI (`ui::snippets::SnippetPanel`) does CRUD over the list.
//!
//! Slice 1 is the store + editor only; expansion-on-trigger is a later slice.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSnippet {
    /// The trigger word that (eventually) expands to the body.
    pub trigger: String,
    /// Language id this snippet applies to, or `*` for all languages.
    pub scope: String,
    /// Human-readable description shown in the editor / (future) completion menu.
    pub description: String,
    /// The snippet body in LSP snippet syntax (`${1:foo}`, `$0`, …).
    pub body: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SnippetStore {
    #[serde(rename = "snippet", default)]
    pub snippets: Vec<UserSnippet>,
}

fn store_path() -> PathBuf {
    zemacs_loader::config_dir().join("snippets.toml")
}

pub fn load() -> SnippetStore {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|c| toml::from_str(&c).ok())
        .unwrap_or_default()
}

pub fn save(data: &SnippetStore) {
    let Ok(contents) = toml::to_string_pretty(data) else {
        return;
    };
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, contents);
}

/// Validate a snippet body against the engine's LSP-snippet parser. Returns an
/// error message (suitable for an inline status warning) when it fails to parse.
pub fn validate_body(body: &str) -> Result<(), String> {
    zemacs_core::snippets::Snippet::parse(body)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_round_trips_through_toml() {
        let data = SnippetStore {
            snippets: vec![
                UserSnippet {
                    trigger: "fori".into(),
                    scope: "rust".into(),
                    description: "for loop".into(),
                    body: "for ${1:i} in ${2:iter} {\n    $0\n}".into(),
                },
                UserSnippet {
                    trigger: "todo".into(),
                    scope: "*".into(),
                    description: "todo marker".into(),
                    body: "TODO: ${1:what}".into(),
                },
            ],
        };
        let serialized = toml::to_string_pretty(&data).unwrap();
        let parsed: SnippetStore = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.snippets.len(), 2);
        assert_eq!(parsed.snippets[0].trigger, "fori");
        assert_eq!(parsed.snippets[0].scope, "rust");
        assert_eq!(parsed.snippets[0].body, "for ${1:i} in ${2:iter} {\n    $0\n}");
        assert_eq!(parsed.snippets[1].scope, "*");
    }

    #[test]
    fn missing_or_empty_file_loads_as_empty() {
        // A blank document parses to a default (empty) store rather than erroring.
        let parsed: SnippetStore = toml::from_str("").unwrap();
        assert!(parsed.snippets.is_empty());
    }

    #[test]
    fn valid_body_parses() {
        assert!(validate_body("${1:foo} bar $0").is_ok());
    }

    #[test]
    fn malformed_body_is_reported() {
        // The LSP-snippet engine is deliberately tolerant — almost any string is
        // accepted as literal text — so an empty body is the one input it rejects.
        // That is also why `SnippetPanel::add` seeds a fresh snippet with a `$0`
        // body rather than an empty one.
        assert!(validate_body("").is_err());
    }
}
