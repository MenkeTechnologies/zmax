//! Keyboard-macro naming and serialization, mirroring GNU Emacs
//! `name-last-kbd-macro` / `insert-kbd-macro`.
//!
//! A macro here is stored in zmax's own key-string representation — the same
//! textual form that `record_macro` produces and `zmax_view::input::parse_macro`
//! reads back (e.g. `iHello<esc>`) — NOT Emacs's `format-kbd-macro` output
//! (space-separated key names like `H e l l o`). Only the *structure* of the
//! inserted definition mirrors Emacs; the key spelling is zmax-native so that
//! zmax can actually re-parse and replay it.
//!
//! The named-macro registry is process-global so a named macro survives across
//! buffers, like Emacs's function cell for a named macro.

use std::collections::BTreeMap;
use std::sync::Mutex;

static NAMED_MACROS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Is `name` a usable name for a keyboard macro? Non-empty and free of
/// whitespace (a macro name has to survive being typed as a command/binding).
pub fn valid_macro_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(char::is_whitespace)
}

/// Emacs `name-last-kbd-macro`: bind `name` to the macro key-string `keys`,
/// overwriting any previous macro of the same name. Returns `false` (a no-op)
/// if `name` is not a valid macro name or `keys` is empty.
pub fn name_macro(name: &str, keys: &str) -> bool {
    if !valid_macro_name(name) || keys.is_empty() {
        return false;
    }
    if let Ok(mut m) = NAMED_MACROS.lock() {
        m.insert(name.to_string(), keys.to_string());
        true
    } else {
        false
    }
}

/// The key-string for the named macro `name`, if any.
pub fn macro_named(name: &str) -> Option<String> {
    NAMED_MACROS.lock().ok().and_then(|m| m.get(name).cloned())
}

/// All registered `(name, keys)` pairs, sorted by name.
pub fn named_macros() -> Vec<(String, String)> {
    NAMED_MACROS
        .lock()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

/// Remove a named macro; returns `true` if one was present.
pub fn remove_named_macro(name: &str) -> bool {
    NAMED_MACROS
        .lock()
        .map(|mut m| m.remove(name).is_some())
        .unwrap_or(false)
}

/// Emacs `insert-kbd-macro`: a textual, re-loadable definition of a macro.
///
/// Mirrors the structure Emacs 30 emits for a kmacro object —
/// `(fset 'NAME (kmacro "KEYS"))` — but `KEYS` is zmax's key-string
/// representation, so the inserted form describes exactly the keys zmax would
/// replay. The key-string is emitted with Rust debug quoting so embedded quotes
/// and backslashes are escaped.
pub fn format_kbd_macro_definition(name: &str, keys: &str) -> String {
    format!("(fset '{name}\n   (kmacro {keys:?}))\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_has_emacs_shape() {
        assert_eq!(
            format_kbd_macro_definition("greet", "iHi<esc>"),
            "(fset 'greet\n   (kmacro \"iHi<esc>\"))\n"
        );
    }

    #[test]
    fn definition_escapes_quotes_and_backslashes() {
        // A key-string containing a double quote and a backslash must stay a
        // valid, re-readable string literal.
        assert_eq!(
            format_kbd_macro_definition("q", "a\"b\\c"),
            "(fset 'q\n   (kmacro \"a\\\"b\\\\c\"))\n"
        );
    }

    #[test]
    fn name_validation() {
        assert!(valid_macro_name("my-macro"));
        assert!(valid_macro_name("dup_line"));
        assert!(!valid_macro_name(""));
        assert!(!valid_macro_name("has space"));
        assert!(!valid_macro_name("tab\tname"));
    }

    #[test]
    fn empty_inputs_rejected() {
        assert!(!name_macro("", "abc"));
        assert!(!name_macro("bad name", "abc"));
        assert!(!name_macro("ok", ""));
        assert!(macro_named("no-such-name-xyz").is_none());
    }

    #[test]
    fn store_lookup_overwrite_remove() {
        // Unique names so the shared process-global registry can't cross tests.
        assert!(name_macro("kmt-a", "aaa"));
        assert_eq!(macro_named("kmt-a").as_deref(), Some("aaa"));

        // Overwrite.
        assert!(name_macro("kmt-a", "bbb"));
        assert_eq!(macro_named("kmt-a").as_deref(), Some("bbb"));

        // Appears in the listing.
        assert!(named_macros()
            .iter()
            .any(|(n, k)| n == "kmt-a" && k == "bbb"));

        // Remove is idempotent-reporting.
        assert!(remove_named_macro("kmt-a"));
        assert!(!remove_named_macro("kmt-a"));
        assert!(macro_named("kmt-a").is_none());
    }
}
