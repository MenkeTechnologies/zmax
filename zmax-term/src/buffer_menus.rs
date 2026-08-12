//! The two customizable buffer menus of the Emacs manual's "Customizing Buffer
//! Menus" node: **bs** (`bs-show` / `bs-customize`) and **MSB** (`msb-mode`).
//!
//! `bs-show` is "a buffer list similar to the one normally displayed by `C-x
//! C-b`, but whose display you can customize in a more flexible fashion. For
//! example, you can specify the list of buffer attributes to show, the minimum
//! and maximum width of buffer name column, a regexp for names of buffers that
//! will never be shown and those which will always be shown". So bs is not a
//! second buffer list — it is the Buffer Menu ([`crate::ui::bufmenu`]) driven by
//! the settings below, which is exactly how `:bs-show` opens it.
//!
//! MSB ("mouse select buffer") "provides a different and customizable mouse
//! buffer menu … It replaces the `mouse-buffer-menu` commands". Here that is a
//! flag the `mouse_buffer_menu` command reads: with MSB on, the buffer picker is
//! grouped by major mode instead of listed in most-recently-used order.
//!
//! Both are persisted at `<config-dir>/buffer_menus`, one `key<TAB>value` per
//! line, so a customized menu survives the session that customized it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const FILE_NAME: &str = "buffer_menus";

/// `bs-attributes-list` — which columns a bs listing shows. bs.el stores the
/// column specs themselves; the Buffer Menu owns the rendering here, so what is
/// customizable is which of its columns are drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Attributes {
    /// The `M`/`R`/`.` flag triple (bs.el's "M" and "R" columns).
    pub flags: bool,
    /// The buffer's size in characters (bs.el's "Size").
    pub size: bool,
    /// The major mode / language name (bs.el's "Mode").
    pub mode: bool,
    /// The visited file name (bs.el's "File").
    pub file: bool,
}

impl Default for Attributes {
    fn default() -> Self {
        // bs.el's shipped `bs-attributes-list` shows all of them.
        Self {
            flags: true,
            size: true,
            mode: true,
            file: true,
        }
    }
}

/// The live `bs` customization group.
#[derive(Clone, Debug)]
pub struct Config {
    pub attributes: Attributes,
    /// `bs-minimal-buffer-name-column` (bs.el default 15).
    pub min_name_column: usize,
    /// `bs-maximal-buffer-name-column` (bs.el default 45).
    pub max_name_column: usize,
    /// `bs-dont-show-regexp` — buffers whose name matches are never listed.
    pub dont_show_regexp: String,
    /// `bs-must-always-show-regexp` — buffers whose name matches are listed even
    /// when another filter would have dropped them.
    pub must_always_show_regexp: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            attributes: Attributes::default(),
            min_name_column: 15,
            max_name_column: 45,
            dont_show_regexp: String::new(),
            must_always_show_regexp: String::new(),
        }
    }
}

impl Config {
    /// Whether a buffer named `name` belongs in a bs listing:
    /// `bs-must-always-show-regexp` wins over `bs-dont-show-regexp`, as in
    /// `bs--redisplay`'s filter order.
    pub fn shows(&self, name: &str) -> bool {
        if matches(&self.must_always_show_regexp, name) {
            return true;
        }
        !matches(&self.dont_show_regexp, name)
    }

    /// The buffer-name column width for a listing whose widest name is
    /// `natural`, clamped into `[bs-minimal…, bs-maximal…]`.
    pub fn name_column(&self, natural: usize) -> usize {
        natural.clamp(
            self.min_name_column.min(self.max_name_column),
            self.max_name_column.max(self.min_name_column),
        )
    }
}

/// Whether `name` matches `pattern`; an empty (unset) pattern never matches, and
/// an invalid regexp is treated as unset rather than erroring at display time.
fn matches(pattern: &str, name: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    regex::Regex::new(pattern).is_ok_and(|re| re.is_match(name))
}

/// The live bs group, loaded from disk on first use.
static CONFIG: Mutex<Option<Config>> = Mutex::new(None);
/// `msb-mode` — off until turned on, as in Emacs.
static MSB_MODE: AtomicBool = AtomicBool::new(false);

fn store_path() -> PathBuf {
    zmax_loader::config_dir().join(FILE_NAME)
}

/// The bs group's current values.
pub fn config() -> Config {
    let mut guard = CONFIG.lock().expect("bs config mutex");
    guard.get_or_insert_with(load).clone()
}

/// Set one bs variable by its Emacs name. Returns an error naming the variable
/// when it is unknown or the value does not parse; persists on success.
pub fn set(variable: &str, value: &str) -> Result<(), String> {
    let mut guard = CONFIG.lock().expect("bs config mutex");
    let cfg = guard.get_or_insert_with(load);
    let flag = |v: &str| -> Result<bool, String> {
        match v {
            "t" | "on" | "1" | "true" | "yes" => Ok(true),
            "nil" | "off" | "0" | "false" | "no" => Ok(false),
            _ => Err(format!("{variable}: expected t or nil, got `{v}`")),
        }
    };
    let number = |v: &str| -> Result<usize, String> {
        v.parse()
            .map_err(|_| format!("{variable}: expected a number, got `{v}`"))
    };
    let regexp = |v: &str| -> Result<String, String> {
        let v = if v == "nil" { "" } else { v };
        if !v.is_empty() {
            regex::Regex::new(v).map_err(|e| format!("{variable}: {e}"))?;
        }
        Ok(v.to_string())
    };
    match variable {
        "bs-show-flags" => cfg.attributes.flags = flag(value)?,
        "bs-show-size" => cfg.attributes.size = flag(value)?,
        "bs-show-mode" => cfg.attributes.mode = flag(value)?,
        "bs-show-file" => cfg.attributes.file = flag(value)?,
        "bs-minimal-buffer-name-column" => cfg.min_name_column = number(value)?,
        "bs-maximal-buffer-name-column" => cfg.max_name_column = number(value)?,
        "bs-dont-show-regexp" => cfg.dont_show_regexp = regexp(value)?,
        "bs-must-always-show-regexp" => cfg.must_always_show_regexp = regexp(value)?,
        other => return Err(format!("unknown bs variable `{other}`")),
    }
    let cfg = cfg.clone();
    drop(guard);
    save(&cfg);
    Ok(())
}

/// Every bs variable and its current value, in the order `bs-customize` lists
/// them.
pub fn variables() -> Vec<(&'static str, String)> {
    let c = config();
    let flag = |on: bool| if on { "t" } else { "nil" }.to_string();
    let regexp = |s: &str| {
        if s.is_empty() {
            "nil".to_string()
        } else {
            s.to_string()
        }
    };
    vec![
        ("bs-show-flags", flag(c.attributes.flags)),
        ("bs-show-size", flag(c.attributes.size)),
        ("bs-show-mode", flag(c.attributes.mode)),
        ("bs-show-file", flag(c.attributes.file)),
        (
            "bs-minimal-buffer-name-column",
            c.min_name_column.to_string(),
        ),
        (
            "bs-maximal-buffer-name-column",
            c.max_name_column.to_string(),
        ),
        ("bs-dont-show-regexp", regexp(&c.dont_show_regexp)),
        (
            "bs-must-always-show-regexp",
            regexp(&c.must_always_show_regexp),
        ),
    ]
}

fn load() -> Config {
    let mut cfg = Config::default();
    let Ok(contents) = std::fs::read_to_string(store_path()) else {
        return cfg;
    };
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('\t') else {
            continue;
        };
        match key {
            "msb-mode" => MSB_MODE.store(value == "t", Ordering::Relaxed),
            "bs-show-flags" => cfg.attributes.flags = value == "t",
            "bs-show-size" => cfg.attributes.size = value == "t",
            "bs-show-mode" => cfg.attributes.mode = value == "t",
            "bs-show-file" => cfg.attributes.file = value == "t",
            "bs-minimal-buffer-name-column" => {
                cfg.min_name_column = value.parse().unwrap_or(cfg.min_name_column);
            }
            "bs-maximal-buffer-name-column" => {
                cfg.max_name_column = value.parse().unwrap_or(cfg.max_name_column);
            }
            "bs-dont-show-regexp" => cfg.dont_show_regexp = value.to_string(),
            "bs-must-always-show-regexp" => cfg.must_always_show_regexp = value.to_string(),
            _ => {}
        }
    }
    cfg
}

fn save(cfg: &Config) {
    let flag = |on: bool| if on { "t" } else { "nil" };
    let body = format!(
        "msb-mode\t{}\nbs-show-flags\t{}\nbs-show-size\t{}\nbs-show-mode\t{}\n\
         bs-show-file\t{}\nbs-minimal-buffer-name-column\t{}\n\
         bs-maximal-buffer-name-column\t{}\nbs-dont-show-regexp\t{}\n\
         bs-must-always-show-regexp\t{}\n",
        flag(msb_mode()),
        flag(cfg.attributes.flags),
        flag(cfg.attributes.size),
        flag(cfg.attributes.mode),
        flag(cfg.attributes.file),
        cfg.min_name_column,
        cfg.max_name_column,
        cfg.dont_show_regexp,
        cfg.must_always_show_regexp,
    );
    let store = store_path();
    if let Some(parent) = store.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(store, body);
}

/// Whether `msb-mode` is on.
pub fn msb_mode() -> bool {
    MSB_MODE.load(Ordering::Relaxed)
}

/// Toggle `msb-mode` (or force it with `on`). Returns the new state.
pub fn set_msb_mode(on: Option<bool>) -> bool {
    let new = on.unwrap_or(!msb_mode());
    MSB_MODE.store(new, Ordering::Relaxed);
    save(&config());
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bs-must-always-show-regexp` overrides `bs-dont-show-regexp` — a buffer
    /// matching both is still listed, which is the point of "always".
    #[test]
    fn always_show_beats_dont_show() {
        let cfg = Config {
            dont_show_regexp: r"^\*".to_string(),
            must_always_show_regexp: r"^\*scratch\*$".to_string(),
            ..Config::default()
        };
        assert!(!cfg.shows("*Messages*"), "a `*…*` buffer is filtered out");
        assert!(cfg.shows("*scratch*"), "…unless it must always show");
        assert!(cfg.shows("main.rs"), "an ordinary buffer is always listed");

        // An unset (empty) filter never drops anything.
        let none = Config::default();
        assert!(none.shows("*Messages*"));
    }

    /// The name column is clamped between `bs-minimal-buffer-name-column` and
    /// `bs-maximal-buffer-name-column`, so a very short or very long buffer name
    /// cannot collapse or blow out the listing.
    #[test]
    fn name_column_is_clamped_to_the_configured_range() {
        let cfg = Config::default();
        assert_eq!(cfg.name_column(3), 15, "widened to the minimum");
        assert_eq!(cfg.name_column(30), 30, "a natural width in range is kept");
        assert_eq!(cfg.name_column(200), 45, "clipped to the maximum");

        // A reversed pair (max < min) still yields a usable width rather than
        // panicking inside `clamp`.
        let reversed = Config {
            min_name_column: 40,
            max_name_column: 10,
            ..Config::default()
        };
        assert_eq!(reversed.name_column(25), 25);
    }

    /// `bs-customize` rejects a bad value instead of storing it: an unparseable
    /// regexp would otherwise be persisted and then silently match nothing, so
    /// the listing would look filtered when the filter is broken. Neither of
    /// these reaches the store, so the test does not touch the config directory.
    #[test]
    fn invalid_values_are_rejected_before_they_are_stored() {
        assert!(set("bs-dont-show-regexp", "(").is_err(), "bad regexp");
        assert!(set("bs-minimal-buffer-name-column", "wide").is_err(), "not a number");
        assert!(set("bs-show-size", "maybe").is_err(), "not a boolean");
        assert!(set("bs-no-such-variable", "t").is_err(), "unknown variable");
        // An unparseable pattern that somehow reached the config still matches
        // nothing rather than panicking at display time.
        assert!(!matches("(", "anything"));
    }
}
