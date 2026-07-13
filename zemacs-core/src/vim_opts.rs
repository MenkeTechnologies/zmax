//! The vim `:set` option store, as seen from `zemacs-core`.
//!
//! `:set` is parsed in the command layer (`zemacs-term`), which keeps every
//! option it accepts in its own store so the whole option surface round-trips
//! (`:set opt?`). Core subsystems need to *read* those values too — the C/lisp
//! indenters want `cinoptions`, `cinscopedecls`, `preserveindent` — but core is
//! a dependency of the command layer, not the other way round, so it cannot
//! reach into that store. The command layer therefore mirrors every store/reset
//! into this module ([`set`] / [`reset`]) and core reads the value here.
//!
//! Values are keyed exactly as the user typed them, because that is how `:set`
//! records them: `:set cino=>8` stores `cino`, `:set cinoptions=>8` stores
//! `cinoptions`. Lookups therefore pass every spelling of the option, most
//! specific first: `get(&["cinoptions", "cino"])`.
//!
//! The store is thread-local, like the command layer's: the editor's `:set` and
//! the edit commands that read it run on the same thread.

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static STORE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Record a `:set name=value` (`value` is `on`/`off` for boolean options).
pub fn set(name: &str, value: &str) {
    STORE.with(|s| {
        s.borrow_mut().insert(name.to_string(), value.to_string());
    });
}

/// Forget an option (`:set name&` — back to its default); `all` clears the store
/// (`:set all&`), matching the command layer's own reset.
pub fn reset(name: &str) {
    if name == "all" {
        clear();
    } else {
        STORE.with(|s| {
            s.borrow_mut().remove(name);
        });
    }
}

/// Drop every stored option (`:set all&`, and test isolation).
pub fn clear() {
    STORE.with(|s| s.borrow_mut().clear());
}

/// The value of the first of `names` (the option's spellings: full name, then
/// abbreviations) that was `:set`, or `None` when the user never set any of
/// them — in which case the caller keeps its own default.
pub fn get(names: &[&str]) -> Option<String> {
    STORE.with(|s| {
        let store = s.borrow();
        names
            .iter()
            .find_map(|n| store.get(*n))
            .filter(|v| !v.is_empty())
            .cloned()
    })
}

/// [`get`], parsed as a number (`None` when unset or not a number).
pub fn get_num(names: &[&str]) -> Option<usize> {
    get(names).and_then(|v| v.parse().ok())
}

/// Whether a boolean option is on. `:set pi` stores `on`, `:set nopi` stores
/// `off`; an option that was never set is off (its vim default).
pub fn get_bool(names: &[&str]) -> bool {
    matches!(
        get(names).as_deref(),
        Some("on" | "1" | "true" | "yes" | "y")
    )
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn set_get_reset() {
        clear();
        assert_eq!(get(&["cinoptions", "cino"]), None);
        // The abbreviation the user typed is the key; a lookup passes both.
        set("cino", ">8");
        assert_eq!(get(&["cinoptions", "cino"]).as_deref(), Some(">8"));
        // The full name wins over the abbreviation when both were set.
        set("cinoptions", ">4");
        assert_eq!(get(&["cinoptions", "cino"]).as_deref(), Some(">4"));
        reset("cinoptions");
        assert_eq!(get(&["cinoptions", "cino"]).as_deref(), Some(">8"));
        reset("cino");
        assert_eq!(get(&["cinoptions", "cino"]), None);
    }

    #[test]
    fn bools_and_numbers() {
        clear();
        assert!(!get_bool(&["preserveindent", "pi"]));
        set("pi", "on");
        assert!(get_bool(&["preserveindent", "pi"]));
        set("pi", "off");
        assert!(!get_bool(&["preserveindent", "pi"]));
        set("chistory", "20");
        assert_eq!(get_num(&["chistory"]), Some(20));
        set("chistory", "many");
        assert_eq!(get_num(&["chistory"]), None);
        // `:set all&` clears everything.
        reset("all");
        assert_eq!(get(&["pi"]), None);
        assert_eq!(get(&["chistory"]), None);
    }
}
