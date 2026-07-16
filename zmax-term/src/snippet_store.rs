//! User-defined snippet library.
//!
//! Each snippet is a trigger word + a scope (a language id, or `*` for all
//! languages) + a human description + an LSP-style snippet body (the syntax the
//! [`zmax_core::snippets::Snippet`] engine parses, e.g. `for ${1:i} { $0 }`).
//! The library persists globally to `<config-dir>/snippets.toml` (alongside
//! `appdata.toml`), tolerant of a missing file (loads as an empty store). The
//! editor TUI (`ui::snippets::SnippetPanel`) does CRUD over the list.
//!
//! Trigger expansion: when the word before the cursor matches a snippet's
//! `trigger` (and its scope applies to the current language), Tab expands the
//! body through the shared snippet engine — activating its tabstops — via
//! [`lookup_trigger`] from the `snippet_expand` command. A process-wide cache
//! keeps that hot path off the disk.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;

/// Process-wide cache of the snippet store, so trigger expansion (which runs on
/// every Tab press) does not hit the disk each time. Populated lazily on the
/// first lookup and refreshed by [`save`] whenever the editor TUI writes a
/// change, so it never goes stale relative to on-disk state we control.
static CACHE: RwLock<Option<SnippetStore>> = RwLock::new(None);

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

impl SnippetStore {
    /// Find the snippet whose trigger exactly matches `word` and whose scope
    /// applies to the current language. A scope of `*` (or empty) matches every
    /// language; otherwise it must equal the document's language *name*.
    pub fn find_trigger(&self, lang: Option<&str>, word: &str) -> Option<&UserSnippet> {
        if word.is_empty() {
            return None;
        }
        self.snippets.iter().find(|s| {
            s.trigger == word
                && (s.scope == "*" || s.scope.is_empty() || Some(s.scope.as_str()) == lang)
        })
    }
}

fn store_path() -> PathBuf {
    zmax_loader::config_dir().join("snippets.toml")
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
    // Keep the expansion cache in step with what we just persisted.
    if let Ok(mut cache) = CACHE.write() {
        *cache = Some(data.clone());
    }
}

/// Look up the body of a user snippet whose trigger matches `word` for the
/// given language, using the process-wide cache (loaded from disk on first
/// use). Returns `None` when nothing matches. This is the hot path called by
/// Tab-driven trigger expansion.
pub fn lookup_trigger(lang: Option<&str>, word: &str) -> Option<String> {
    if word.is_empty() {
        return None;
    }
    if let Ok(cache) = CACHE.read() {
        if let Some(store) = cache.as_ref() {
            return store.find_trigger(lang, word).map(|s| s.body.clone());
        }
    }
    // Cache miss: load from disk, answer, then populate the cache.
    let store = load();
    let result = store.find_trigger(lang, word).map(|s| s.body.clone());
    if let Ok(mut cache) = CACHE.write() {
        *cache = Some(store);
    }
    result
}

/// Validate a snippet body against the engine's LSP-snippet parser. Returns an
/// error message (suitable for an inline status warning) when it fails to parse.
pub fn validate_body(body: &str) -> Result<(), String> {
    zmax_core::snippets::Snippet::parse(body)
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
        assert_eq!(
            parsed.snippets[0].body,
            "for ${1:i} in ${2:iter} {\n    $0\n}"
        );
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

    fn sample() -> SnippetStore {
        SnippetStore {
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
                UserSnippet {
                    trigger: "anyscope".into(),
                    scope: "".into(),
                    description: "empty scope = all langs".into(),
                    body: "X $0".into(),
                },
            ],
        }
    }

    #[test]
    fn find_trigger_matches_scoped_language() {
        let store = sample();
        assert_eq!(
            store
                .find_trigger(Some("rust"), "fori")
                .map(|s| s.body.as_str()),
            Some("for ${1:i} in ${2:iter} {\n    $0\n}")
        );
        // Wrong language: scoped snippet must not match.
        assert!(store.find_trigger(Some("python"), "fori").is_none());
        // No language at all: scoped snippet still must not match.
        assert!(store.find_trigger(None, "fori").is_none());
    }

    #[test]
    fn find_trigger_wildcard_and_empty_scope_match_any_language() {
        let store = sample();
        assert!(store.find_trigger(Some("python"), "todo").is_some());
        assert!(store.find_trigger(None, "todo").is_some());
        assert!(store.find_trigger(Some("go"), "anyscope").is_some());
    }

    #[test]
    fn find_trigger_requires_exact_word_and_rejects_empty() {
        let store = sample();
        // Prefix of a trigger must not match.
        assert!(store.find_trigger(Some("rust"), "for").is_none());
        // Unknown trigger.
        assert!(store.find_trigger(Some("rust"), "nope").is_none());
        // Empty word never matches.
        assert!(store.find_trigger(Some("rust"), "").is_none());
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
