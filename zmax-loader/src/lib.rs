pub mod config;
pub mod grammar;
pub mod workspace_trust;

use zmax_stdx::{env::current_working_dir, path};

use etcetera::base_strategy::{choose_base_strategy, BaseStrategy};
use std::path::{Path, PathBuf};

pub const VERSION_AND_GIT_HASH: &str = env!("VERSION_AND_GIT_HASH");

static RUNTIME_DIRS: once_cell::sync::Lazy<Vec<PathBuf>> =
    once_cell::sync::Lazy::new(prioritize_runtime_dirs);

static CONFIG_FILE: once_cell::sync::OnceCell<PathBuf> = once_cell::sync::OnceCell::new();

static LOG_FILE: once_cell::sync::OnceCell<PathBuf> = once_cell::sync::OnceCell::new();

pub fn initialize_config_file(specified_file: Option<PathBuf>) {
    let config_file = specified_file.unwrap_or_else(default_config_file);
    ensure_parent_dir(&config_file);
    CONFIG_FILE.set(config_file).ok();
}

pub fn initialize_log_file(specified_file: Option<PathBuf>) {
    let log_file = specified_file.unwrap_or_else(default_log_file);
    ensure_parent_dir(&log_file);
    LOG_FILE.set(log_file).ok();
}

/// A list of runtime directories from highest to lowest priority
///
/// The priority is:
///
/// 1. sibling directory to `CARGO_MANIFEST_DIR` (if environment variable is set)
/// 2. subdirectory of user config directory (always included)
/// 3. `ZMAX_RUNTIME` (if environment variable is set)
/// 4. `ZMAX_DEFAULT_RUNTIME` (if environment variable is set *at build time*)
/// 5. subdirectory of path to zmax executable (always included)
///
/// Postcondition: returns at least two paths (they might not exist).
fn prioritize_runtime_dirs() -> Vec<PathBuf> {
    const RT_DIR: &str = "runtime";
    // Adding higher priority first
    let mut rt_dirs = Vec::new();
    if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
        // this is the directory of the crate being run by cargo, we need the workspace path so we take the parent
        let path = PathBuf::from(dir).parent().unwrap().join(RT_DIR);
        log::debug!("runtime dir: {}", path.to_string_lossy());
        rt_dirs.push(path);
    }

    let conf_rt_dir = config_dir().join(RT_DIR);
    rt_dirs.push(conf_rt_dir);

    if let Ok(dir) = std::env::var("ZMAX_RUNTIME") {
        let dir = path::expand_tilde(Path::new(&dir));
        rt_dirs.push(path::normalize(dir));
    }

    // If this variable is set during build time, it will always be included
    // in the lookup list. This allows downstream packagers to set a fallback
    // directory to a location that is conventional on their distro so that they
    // need not resort to a wrapper script or a global environment variable.
    if let Some(dir) = std::option_env!("ZMAX_DEFAULT_RUNTIME") {
        rt_dirs.push(dir.into());
    }

    // fallback to location of the executable being run
    // canonicalize the path in case the executable is symlinked
    let exe_rt_dir = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok())
        .and_then(|path| path.parent().map(|path| path.to_path_buf().join(RT_DIR)))
        .unwrap();
    rt_dirs.push(exe_rt_dir);
    rt_dirs
}

/// Runtime directories ordered from highest to lowest priority
///
/// All directories should be checked when looking for files.
///
/// Postcondition: returns at least one path (it might not exist).
pub fn runtime_dirs() -> &'static [PathBuf] {
    &RUNTIME_DIRS
}

/// Find file with path relative to runtime directory
///
/// `rel_path` should be the relative path from within the `runtime/` directory.
/// The valid runtime directories are searched in priority order and the first
/// file found to exist is returned, otherwise None.
fn find_runtime_file(rel_path: &Path) -> Option<PathBuf> {
    RUNTIME_DIRS.iter().find_map(|rt_dir| {
        let path = rt_dir.join(rel_path);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    })
}

/// Find file with path relative to runtime directory
///
/// `rel_path` should be the relative path from within the `runtime/` directory.
/// The valid runtime directories are searched in priority order and the first
/// file found to exist is returned, otherwise the path to the final attempt
/// that failed.
pub fn runtime_file(rel_path: impl AsRef<Path>) -> PathBuf {
    find_runtime_file(rel_path.as_ref()).unwrap_or_else(|| {
        RUNTIME_DIRS
            .last()
            .map(|dir| dir.join(rel_path))
            .unwrap_or_default()
    })
}

pub fn config_dir() -> PathBuf {
    // TODO: allow env var override
    // zmax keeps all of its config under a single dotted home directory
    // (`~/.zmax`) rather than the XDG config location, so config.toml,
    // languages.toml and the `runtime/` overlay all live together.
    let strategy = choose_base_strategy().expect("Unable to find the config directory!");
    let mut path = strategy.home_dir().to_path_buf();
    path.push(".zmax");
    path
}

pub fn cache_dir() -> PathBuf {
    // TODO: allow env var override
    let strategy = choose_base_strategy().expect("Unable to find the cache directory!");
    let mut path = strategy.cache_dir();
    path.push("zmax");
    path
}

pub fn data_dir() -> PathBuf {
    let strategy = choose_base_strategy().expect("Unable to find the data directory!");
    let mut path = strategy.data_dir();
    path.push("zmax");
    path
}

pub fn config_file() -> PathBuf {
    CONFIG_FILE.get().map(|path| path.to_path_buf()).unwrap()
}

pub fn log_file() -> PathBuf {
    LOG_FILE.get().map(|path| path.to_path_buf()).unwrap()
}

pub fn workspace_config_file() -> PathBuf {
    find_workspace().0.join(".zmax").join("config.toml")
}

pub fn workspace_lang_config_file() -> PathBuf {
    find_workspace().0.join(".zmax").join("languages.toml")
}

pub fn lang_config_file() -> PathBuf {
    config_dir().join("languages.toml")
}

pub fn default_log_file() -> PathBuf {
    // The log lives with the rest of zmax's state under `~/.zmax`, not in the
    // XDG cache dir — one dotted home directory holds config, languages and logs.
    config_dir().join("zmax.log")
}

/// Merge two TOML documents, merging values from `right` onto `left`
///
/// `merge_depth` sets the nesting depth up to which values are merged instead
/// of overridden.
///
/// When a table exists in both `left` and `right`, the merged table consists of
/// all keys in `left`'s table unioned with all keys in `right` with the values
/// of `right` being merged recursively onto values of `left`.
///
/// `crate::merge_toml_values(a, b, 3)` combines, for example:
///
/// b:
/// ```toml
/// [[language]]
/// name = "toml"
/// language-server = { command = "taplo", args = ["lsp", "stdio"] }
/// ```
/// a:
/// ```toml
/// [[language]]
/// language-server = { command = "/usr/bin/taplo" }
/// ```
///
/// into:
/// ```toml
/// [[language]]
/// name = "toml"
/// language-server = { command = "/usr/bin/taplo" }
/// ```
///
/// thus it overrides the third depth-level of b with values of a if they exist,
/// but otherwise merges their values
pub fn merge_toml_values(left: toml::Value, right: toml::Value, merge_depth: usize) -> toml::Value {
    use toml::Value;

    fn get_name(v: &Value) -> Option<&str> {
        v.get("name").and_then(Value::as_str)
    }

    match (left, right) {
        (Value::Array(mut left_items), Value::Array(right_items)) => {
            if merge_depth > 0 {
                left_items.reserve(right_items.len());
                for rvalue in right_items {
                    let lvalue = get_name(&rvalue)
                        .and_then(|rname| {
                            left_items.iter().position(|v| get_name(v) == Some(rname))
                        })
                        .map(|lpos| left_items.remove(lpos));
                    let mvalue = match lvalue {
                        Some(lvalue) => merge_toml_values(lvalue, rvalue, merge_depth - 1),
                        None => rvalue,
                    };
                    left_items.push(mvalue);
                }
                Value::Array(left_items)
            } else {
                Value::Array(right_items)
            }
        }
        (Value::Table(mut left_map), Value::Table(right_map)) => {
            if merge_depth > 0 {
                for (rname, rvalue) in right_map {
                    match left_map.remove(&rname) {
                        Some(lvalue) => {
                            let merged_value = merge_toml_values(lvalue, rvalue, merge_depth - 1);
                            left_map.insert(rname, merged_value);
                        }
                        None => {
                            left_map.insert(rname, rvalue);
                        }
                    }
                }
                Value::Table(left_map)
            } else {
                Value::Table(right_map)
            }
        }
        // Catch everything else we didn't handle, and use the right value
        (_, value) => value,
    }
}

/// Finds the current workspace folder.
/// Used as a ceiling dir for LSP root resolution, the filepicker and potentially as a future filewatching root
///
/// This function starts searching the FS upward from the CWD
/// and returns the first directory that contains either `.git`, `.svn`, `.jj` or `.zmax`.
/// If no workspace was found returns (CWD, true).
/// Otherwise (workspace, false) is returned
pub fn find_workspace() -> (PathBuf, bool) {
    let current_dir = current_working_dir();
    find_workspace_in(current_dir)
}

pub fn find_workspace_in(dir: impl AsRef<Path>) -> (PathBuf, bool) {
    let dir = dir.as_ref();
    for ancestor in dir.ancestors() {
        if ancestor.join(".git").exists()
            || ancestor.join(".svn").exists()
            || ancestor.join(".jj").exists()
            || ancestor.join(".zmax").exists()
        {
            return (ancestor.to_owned(), false);
        }
    }

    (dir.to_owned(), true)
}

fn default_config_file() -> PathBuf {
    config_dir().join("config.toml")
}

fn ensure_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).ok();
        }
    }
}

#[cfg(test)]
mod merge_toml_tests {
    use std::str;

    use super::merge_toml_values;
    use toml::Value;

    #[test]
    fn language_toml_map_merges() {
        const USER: &str = r#"
        [[language]]
        name = "nix"
        test = "bbb"
        indent = { tab-width = 4, unit = "    ", test = "aaa" }
        "#;

        let base = include_bytes!("../../languages.toml");
        let base = str::from_utf8(base).expect("Couldn't parse built-in languages config");
        let base: Value = toml::from_str(base).expect("Couldn't parse built-in languages config");
        let user: Value = toml::from_str(USER).unwrap();

        let merged = merge_toml_values(base, user, 3);
        let languages = merged.get("language").unwrap().as_array().unwrap();
        let nix = languages
            .iter()
            .find(|v| v.get("name").unwrap().as_str().unwrap() == "nix")
            .unwrap();
        let nix_indent = nix.get("indent").unwrap();

        // We changed tab-width and unit in indent so check them if they are the new values
        assert_eq!(
            nix_indent.get("tab-width").unwrap().as_integer().unwrap(),
            4
        );
        assert_eq!(nix_indent.get("unit").unwrap().as_str().unwrap(), "    ");
        // We added a new keys, so check them
        assert_eq!(nix.get("test").unwrap().as_str().unwrap(), "bbb");
        assert_eq!(nix_indent.get("test").unwrap().as_str().unwrap(), "aaa");
        // We didn't change comment-token so it should be same
        assert_eq!(nix.get("comment-token").unwrap().as_str().unwrap(), "#");
    }

    #[test]
    fn language_toml_nested_array_merges() {
        const USER: &str = r#"
        [[language]]
        name = "typescript"
        language-server = { command = "deno", args = ["lsp"] }
        "#;

        let base = include_bytes!("../../languages.toml");
        let base = str::from_utf8(base).expect("Couldn't parse built-in languages config");
        let base: Value = toml::from_str(base).expect("Couldn't parse built-in languages config");
        let user: Value = toml::from_str(USER).unwrap();

        let merged = merge_toml_values(base, user, 3);
        let languages = merged.get("language").unwrap().as_array().unwrap();
        let ts = languages
            .iter()
            .find(|v| v.get("name").unwrap().as_str().unwrap() == "typescript")
            .unwrap();
        assert_eq!(
            ts.get("language-server")
                .unwrap()
                .get("args")
                .unwrap()
                .as_array()
                .unwrap(),
            &vec![Value::String("lsp".into())]
        )
    }
}

#[cfg(test)]
mod merge_depth_tests {
    use super::merge_toml_values;
    use toml::Value;

    fn merge(base: &str, user: &str, depth: usize) -> Value {
        merge_toml_values(
            toml::from_str(base).unwrap(),
            toml::from_str(user).unwrap(),
            depth,
        )
    }

    /// Arrays of tables merge by the `name` key, not by position: a user entry
    /// for an existing language updates that language, and one for a new
    /// language is appended. Merging positionally would rewrite whichever
    /// language happened to sit at that index.
    #[test]
    fn language_arrays_merge_by_name_not_position() {
        let merged = merge(
            r##"
            [[language]]
            name = "rust"
            comment-token = "//"
            [[language]]
            name = "toml"
            comment-token = "#"
            "##,
            r##"
            [[language]]
            name = "toml"
            comment-token = ";"
            [[language]]
            name = "stryke"
            comment-token = "#"
            "##,
            3,
        );

        let languages = merged.get("language").unwrap().as_array().unwrap();
        let token = |name: &str| -> Option<String> {
            languages
                .iter()
                .find(|value| value.get("name").unwrap().as_str() == Some(name))?
                .get("comment-token")?
                .as_str()
                .map(str::to_string)
        };

        assert_eq!(languages.len(), 3, "the new language is appended");
        assert_eq!(token("toml").as_deref(), Some(";"), "user value wins");
        assert_eq!(token("rust").as_deref(), Some("//"), "untouched entry kept");
        assert_eq!(token("stryke").as_deref(), Some("#"));
    }

    /// At depth 0 the user value replaces the base value outright rather than
    /// merging into it. This is the bottom of the recursion, and it is what makes
    /// a user's `language-server = { command = ... }` override the whole table
    /// instead of inheriting keys from the shipped one.
    #[test]
    fn depth_zero_replaces_instead_of_merging() {
        let merged = merge(
            r##"
            [[language]]
            name = "toml"
            comment-token = "#"
            "##,
            r##"
            [[language]]
            name = "stryke"
            "##,
            0,
        );

        let languages = merged.get("language").unwrap().as_array().unwrap();
        assert_eq!(
            languages.len(),
            1,
            "the base array is discarded: {languages:?}"
        );
        assert_eq!(languages[0].get("name").unwrap().as_str(), Some("stryke"));
    }

    /// A scalar on either side is not merged -- the user's value wins whole.
    #[test]
    fn scalars_take_the_user_value() {
        let merged = merge(r##"theme = "base16""##, r##"theme = "onedark""##, 3);

        assert_eq!(merged.get("theme").unwrap().as_str(), Some("onedark"));
    }
}

#[cfg(test)]
mod runtime_and_workspace_tests {
    use super::*;

    /// The runtime directory list is what `-g fetch` writes into and what
    /// `find_runtime_file` searches. `prioritize_runtime_dirs` documents a
    /// post-condition of at least two paths, and grammar fetch/build takes
    /// `.first()` -- a fetch that lands in the wrong directory leaves `-g build`
    /// reading an absent one, which is exactly how a stale `~/.zmax/runtime`
    /// symlink failed every grammar with no reason printed.
    #[test]
    fn runtime_dirs_are_named_runtime_and_meet_the_documented_minimum() {
        let dirs = prioritize_runtime_dirs();

        assert!(dirs.len() >= 2, "post-condition: {dirs:?}");
        for dir in &dirs {
            assert_eq!(
                dir.file_name().and_then(|name| name.to_str()),
                Some("runtime"),
                "every entry is a `runtime` directory: {dir:?}"
            );
        }
        assert!(!runtime_dirs().is_empty(), "post-condition: at least one");
    }

    /// Under cargo the workspace's own `runtime/` outranks `~/.zmax/runtime`, so
    /// `cargo run -- -g fetch` populates the checkout rather than the user's
    /// config directory. The config directory is always in the list behind it.
    #[test]
    fn the_cargo_workspace_runtime_outranks_the_config_dir() {
        let dirs = prioritize_runtime_dirs();
        let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo test");
        let workspace_runtime = Path::new(&manifest).parent().unwrap().join("runtime");

        assert_eq!(dirs.first(), Some(&workspace_runtime));
        assert!(
            dirs.contains(&config_dir().join("runtime")),
            "the config dir runtime is always included: {dirs:?}"
        );
    }

    /// A missing runtime file still yields a path to report, never an empty one --
    /// callers join and display it in error messages.
    #[test]
    fn runtime_file_falls_back_to_a_reportable_path() {
        let path = runtime_file("grammars/sources/definitely-absent");

        assert!(
            path.ends_with("grammars/sources/definitely-absent"),
            "{path:?}"
        );
    }

    /// `.git`, `.svn`, `.jj` and `.zmax` each mark a workspace root, and the
    /// search walks up from a nested directory. The workspace root is where
    /// `.zmax/languages.toml` is read from, which is the file grammar
    /// fetch/build resolves its sources through.
    #[test]
    fn workspace_root_is_the_nearest_marked_ancestor() {
        for marker in [".git", ".svn", ".jj", ".zmax"] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join(marker)).unwrap();
            let nested = root.path().join("src").join("deep");
            std::fs::create_dir_all(&nested).unwrap();

            assert_eq!(
                find_workspace_in(&nested),
                (root.path().to_path_buf(), false),
                "{marker} must mark the root"
            );
        }
    }

    /// Nested markers resolve to the closest one, not the outermost -- a repo
    /// checked out inside another repo keeps its own `.zmax` config.
    #[test]
    fn the_nearest_marker_wins_over_an_outer_one() {
        let outer = tempfile::tempdir().unwrap();
        std::fs::create_dir(outer.path().join(".git")).unwrap();
        let inner = outer.path().join("vendor").join("zshrs");
        std::fs::create_dir_all(inner.join(".git")).unwrap();

        assert_eq!(find_workspace_in(&inner), (inner, false));
    }

    /// With no marker anywhere the directory itself is the workspace, flagged so
    /// callers can tell a real root from this fallback.
    #[test]
    fn an_unmarked_directory_is_its_own_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();

        let (found, is_fallback) = find_workspace_in(&nested);

        // An ancestor of a temp dir could itself be marked on some machines; the
        // flag and the returned path must agree either way.
        if is_fallback {
            assert_eq!(found, nested);
        } else {
            assert!(nested.starts_with(&found), "{found:?} vs {nested:?}");
        }
    }
}
