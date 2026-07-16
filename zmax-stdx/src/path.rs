//! Functions for working with [Path].

pub use etcetera::home_dir;
use once_cell::sync::Lazy;
use regex_cursor::{engines::meta::Regex, Input};
use ropey::RopeSlice;

use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsString,
    ops::Range,
    path::{Component, Path, PathBuf, MAIN_SEPARATOR_STR},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
};

use crate::env::current_working_dir;

/// Replaces users home directory from `path` with tilde `~` if the directory
/// is available, otherwise returns the path unchanged.
pub fn fold_home_dir<'a, P>(path: P) -> Cow<'a, Path>
where
    P: Into<Cow<'a, Path>>,
{
    let path = path.into();
    if let Ok(home) = home_dir() {
        if let Ok(stripped) = path.strip_prefix(&home) {
            let mut path = OsString::with_capacity(2 + stripped.as_os_str().len());
            path.push("~");
            path.push(MAIN_SEPARATOR_STR);
            path.push(stripped);
            return Cow::Owned(PathBuf::from(path));
        }
    }

    path
}

/// Expands tilde `~` into users home directory if available, otherwise returns the path
/// unchanged.
///
/// The tilde will only be expanded when present as the first component of the path
/// and only slash follows it.
pub fn expand_tilde<'a, P>(path: P) -> Cow<'a, Path>
where
    P: Into<Cow<'a, Path>>,
{
    let path = path.into();
    let mut components = path.components();
    if let Some(Component::Normal(c)) = components.next() {
        if c == "~" {
            if let Ok(mut buf) = home_dir() {
                buf.push(components);
                return Cow::Owned(buf);
            }
        }
    }

    path
}

/// Normalize a path without resolving symlinks.
// Strategy: start from the first component and move up. Canonicalize previous path,
// join component, canonicalize new path, strip prefix and join to the final result.
pub fn normalize(path: impl AsRef<Path>) -> PathBuf {
    let mut components = path.as_ref().components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().copied() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {}
            #[cfg(not(windows))]
            Component::ParentDir => {
                ret.pop();
            }
            #[cfg(windows)]
            Component::ParentDir => {
                if let Some(head) = ret.components().next_back() {
                    match head {
                        Component::Prefix(_) | Component::RootDir => {}
                        Component::CurDir => unreachable!(),
                        // If we left previous component as ".." it means we met a symlink before and we can't pop path.
                        Component::ParentDir => {
                            ret.push("..");
                        }
                        Component::Normal(_) => {
                            if ret.is_symlink() {
                                ret.push("..");
                            } else {
                                ret.pop();
                            }
                        }
                    }
                }
            }
            #[cfg(not(windows))]
            Component::Normal(c) => {
                ret.push(c);
            }
            #[cfg(windows)]
            Component::Normal(c) => 'normal: {
                use std::fs::canonicalize;

                let new_path = ret.join(c);
                if new_path.is_symlink() {
                    ret = new_path;
                    break 'normal;
                }
                let (can_new, can_old) = (canonicalize(&new_path), canonicalize(&ret));
                match (can_new, can_old) {
                    (Ok(can_new), Ok(can_old)) => {
                        let striped = can_new.strip_prefix(can_old);
                        ret.push(striped.unwrap_or_else(|_| c.as_ref()));
                    }
                    _ => ret.push(c),
                }
            }
        }
    }
    dunce::simplified(&ret).to_path_buf()
}

/// Returns the canonical, absolute form of a path with all intermediate components normalized.
///
/// This function is used instead of [`std::fs::canonicalize`] because we don't want to verify
/// here if the path exists, just normalize it's components.
pub fn canonicalize(path: impl AsRef<Path>) -> PathBuf {
    let path = expand_tilde(path.as_ref());
    let path = if path.is_relative() {
        Cow::Owned(current_working_dir().join(path))
    } else {
        path
    };

    normalize(path)
}

/// Convert path into a relative path
pub fn get_relative_path<'a, P>(path: P) -> Cow<'a, Path>
where
    P: Into<Cow<'a, Path>>,
{
    let path = path.into();
    if path.is_absolute() {
        let cwdir = normalize(current_working_dir());
        if let Ok(stripped) = normalize(&path).strip_prefix(cwdir) {
            return Cow::Owned(PathBuf::from(stripped));
        }

        return fold_home_dir(path);
    }

    path
}

/// Returns a truncated filepath where the basepart of the path is reduced to the first
/// char of the folder and the whole filename appended.
///
/// Also strip the current working directory from the beginning of the path.
/// Note that this function does not check if the truncated path is unambiguous.
///
/// ```
///    use zmax_stdx::path::get_truncated_path;
///    use std::path::Path;
///
///    assert_eq!(
///         get_truncated_path("/home/cnorris/documents/jokes.txt").as_path(),
///         Path::new("/h/c/d/jokes.txt")
///     );
///     assert_eq!(
///         get_truncated_path("jokes.txt").as_path(),
///         Path::new("jokes.txt")
///     );
///     assert_eq!(
///         get_truncated_path("/jokes.txt").as_path(),
///         Path::new("/jokes.txt")
///     );
///     assert_eq!(
///         get_truncated_path("/h/c/d/jokes.txt").as_path(),
///         Path::new("/h/c/d/jokes.txt")
///     );
///     assert_eq!(get_truncated_path("").as_path(), Path::new(""));
/// ```
///
pub fn get_truncated_path(path: impl AsRef<Path>) -> PathBuf {
    let cwd = current_working_dir();
    let path = path.as_ref();
    let path = path.strip_prefix(cwd).unwrap_or(path);
    let file = path.file_name().unwrap_or_default();
    let base = path.parent().unwrap_or_else(|| Path::new(""));
    let mut ret = PathBuf::with_capacity(file.len());
    // A char can't be directly pushed to a PathBuf
    let mut first_char_buffer = String::new();
    for d in base {
        let Some(first_char) = d.to_string_lossy().chars().next() else {
            break;
        };
        first_char_buffer.push(first_char);
        ret.push(&first_char_buffer);
        first_char_buffer.clear();
    }
    ret.push(file);
    ret
}

// --- vim 'isfname' ----------------------------------------------------------
//
// 'isfname' names the characters that may appear in a file name. It is what the
// path-under-cursor scan (`gf`, `find_paths`, `get_path_suffix`) uses to decide
// where a path ends, so `:set isfname` has to reach the regex those build. The
// value is parsed once into a regex character-class body; each change bumps a
// generation counter and the path regexes are rebuilt on their next use.

/// The character-class body derived from the current 'isfname' (`None` = the
/// option was never `:set`, so the built-in class applies).
static ISFNAME_CLASS: RwLock<Option<String>> = RwLock::new(None);
/// Bumped by every [`set_isfname`]; keys the compiled-regex cache.
static ISFNAME_GEN: AtomicU64 = AtomicU64::new(0);

/// Parse a vim 'isfname' value into the characters it allows. Items are comma
/// separated: a single character, an `a-b` character range, a decimal character
/// code, a `48-57` code range, `@` (every alphabetic character — always allowed,
/// so it adds nothing to `\w`) and `^x` (an exclusion, removed from the set). A
/// literal comma is written `,,`, which leaves an empty item on either side of
/// it. Pure — unit tested.
pub fn parse_isfname(value: &str) -> Vec<char> {
    let code = |s: &str| -> Option<char> {
        match s.parse::<u32>() {
            Ok(n) => char::from_u32(n),
            Err(_) => {
                let mut it = s.chars();
                match (it.next(), it.next()) {
                    (Some(c), None) => Some(c),
                    _ => None,
                }
            }
        }
    };
    let mut include = Vec::new();
    let mut exclude = Vec::new();
    for raw in value.split(',') {
        let (item, out) = match raw.strip_prefix('^') {
            // `^x`: x is *not* a file-name character.
            Some(rest) if !rest.is_empty() => (rest, &mut exclude),
            _ => (raw, &mut include),
        };
        // An empty item is one half of the `,,` that writes a literal comma.
        if item.is_empty() {
            out.push(',');
            continue;
        }
        // `@` is "all alphabetic characters"; `\w` already covers them.
        if item == "@" {
            continue;
        }
        // A range `a-b` / `48-57` — but a lone `-` is a literal hyphen.
        if let Some((lo, hi)) = item.split_once('-') {
            if let (Some(lo), Some(hi)) = (code(lo), code(hi)) {
                for n in (lo as u32)..=(hi as u32) {
                    if let Some(c) = char::from_u32(n) {
                        out.push(c);
                    }
                }
                continue;
            }
        }
        if let Some(c) = code(item) {
            out.push(c);
        }
    }
    include.retain(|c| !exclude.contains(c));
    include.sort_unstable();
    include.dedup();
    include
}

/// The regex character-class body (to be spliced inside `[…]`) for the extra
/// characters `isfname` allows beyond `\w`. `/` and `\` are dropped: they are
/// the path *separator*, which the path regex builds on its own, and a component
/// that could swallow them would no longer split into components. Pure — unit
/// tested.
fn isfname_class_body(chars: &[char]) -> String {
    let mut out = String::new();
    for &c in chars {
        if c == '/' || c == '\\' || c.is_alphanumeric() || c == '_' {
            continue;
        }
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// vim `:set isfname=…` — the characters a file name may contain. Every path
/// scan compiled after this call uses the new set. An empty value restores the
/// built-in class.
pub fn set_isfname(value: &str) {
    let class = (!value.trim().is_empty()).then(|| isfname_class_body(&parse_isfname(value)));
    *ISFNAME_CLASS.write().unwrap() = class;
    ISFNAME_GEN.fetch_add(1, Ordering::Relaxed);
}

fn path_component_regex(windows: bool) -> String {
    // TODO: support backslash path escape on windows (when using git bash for example)
    let space_escape = if windows { r"[\^`]\s" } else { r"[\\]\s" };
    // vim 'isfname', when set, *is* the set of file-name characters — it replaces
    // the built-in class below rather than adding to it.
    if let Some(class) = ISFNAME_CLASS.read().unwrap().as_deref() {
        return format!("[\\w{class}]|{space_escape}");
    }
    // partially baesd on what's allowed in an url but with some care to avoid
    // false positives (like any kind of brackets or quotes)
    r"[\w@.\-+#$%?!,;~&]|".to_owned() + space_escape
}

/// The path regex for one `(match_single_file, anchored)` shape, rebuilt
/// whenever `:set isfname` changed the file-name character class. (It used to be
/// four `Lazy` statics; an option that must reach them rules that out.)
fn path_regex(match_single_file: bool, anchored: bool) -> Arc<Regex> {
    /// `(match_single_file, anchored)` → the 'isfname' generation it was built
    /// for, and the regex.
    type RegexCache = HashMap<(bool, bool), (u64, Arc<Regex>)>;
    static CACHE: Lazy<Mutex<RegexCache>> = Lazy::new(|| Mutex::new(HashMap::new()));
    let generation = ISFNAME_GEN.load(Ordering::Relaxed);
    let key = (match_single_file, anchored);
    let mut cache = CACHE.lock().unwrap();
    if let Some((cached, regex)) = cache.get(&key) {
        if *cached == generation {
            return regex.clone();
        }
    }
    let regex = Arc::new(compile_path_regex(
        "",
        if anchored { "$" } else { "" },
        match_single_file,
        cfg!(windows),
    ));
    cache.insert(key, (generation, regex.clone()));
    regex
}

/// Regex for delimited environment captures like `${HOME}`.
fn braced_env_regex(windows: bool) -> String {
    r"\$\{(?:".to_owned() + &path_component_regex(windows) + r"|[/:=])+\}"
}

fn compile_path_regex(
    prefix: &str,
    postfix: &str,
    match_single_file: bool,
    windows: bool,
) -> Regex {
    let first_component = format!(
        "(?:{}|(?:{}))",
        braced_env_regex(windows),
        path_component_regex(windows)
    );
    // For all components except the first we allow an equals so that `foo=/
    // bar/baz` does not include foo. This is primarily intended for url queries
    // (where an equals is never in the first component)
    let component = format!("(?:{first_component}|=)");
    let sep = if windows { r"[/\\]" } else { "/" };
    let url_prefix = r"[\w+\-.]+://??";
    let path_prefix = if windows {
        // single slash handles most windows prefixes (like\\server\...) but `\
        // \?\C:\..` (and C:\) needs special handling, since we don't allow : in path
        // components (so that colon separated paths and <path>:<line> work)
        r"\\\\\?\\\w:|\w:|\\|"
    } else {
        ""
    };
    let path_start = format!("(?:{first_component}+|~|{path_prefix}{url_prefix})");
    let optional = if match_single_file {
        format!("|{path_start}")
    } else {
        String::new()
    };
    let path_regex = format!(
        "{prefix}(?:{path_start}?(?:(?:{sep}{component}+)+{sep}?|{sep}){optional}){postfix}"
    );
    Regex::new(&path_regex).unwrap()
}

/// If `src` ends with a path then this function returns the part of the slice.
pub fn get_path_suffix(src: RopeSlice<'_>, match_single_file: bool) -> Option<RopeSlice<'_>> {
    let regex = path_regex(match_single_file, true);
    regex
        .find(Input::new(src))
        .map(|mat| src.byte_slice(mat.range()))
}

/// Returns an iterator of the **byte** ranges in src that contain a path.
pub fn find_paths(
    src: RopeSlice<'_>,
    match_single_file: bool,
) -> impl Iterator<Item = Range<usize>> + '_ {
    // The regex is rebuilt on `:set isfname`, so it is owned (an `Arc`) rather
    // than a `&'static` — the matches are collected so the iterator borrows only
    // `src`, as its signature promises.
    let regex = path_regex(match_single_file, false);
    regex
        .find_iter(Input::new(src))
        .map(|mat| mat.range())
        .collect::<Vec<_>>()
        .into_iter()
}

/// Performs substitution of `~` and environment variables, see [`env::expand`](crate::env::expand) and [`expand_tilde`]
pub fn expand<T: AsRef<Path> + ?Sized>(path: &T) -> Cow<'_, Path> {
    let path = path.as_ref();
    let path = expand_tilde(path);
    match crate::env::expand(&*path) {
        Cow::Borrowed(_) => path,
        Cow::Owned(path) => PathBuf::from(path).into(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsStr,
        path::{Component, Path},
    };

    use std::sync::{Mutex, MutexGuard};

    use regex_cursor::Input;
    use ropey::RopeSlice;

    use crate::path::{self, compile_path_regex};

    /// `compile_path_regex`/`get_path_suffix`/`find_paths` all read the process-
    /// global `ISFNAME_CLASS`, and `set_isfname` mutates it. Tests run in parallel
    /// within a binary, so a test that reads the default class must not overlap
    /// with one that has temporarily changed it (`set_isfname_changes_the_path_scan`
    /// drops `$`, which would make `$FOO` stop matching). Every test touching that
    /// global takes this lock so they run one at a time. Poison is recovered from:
    /// a failing test still fails on its own, but must not cascade into the others.
    static ISFNAME_LOCK: Mutex<()> = Mutex::new(());

    fn isfname_guard() -> MutexGuard<'static, ()> {
        ISFNAME_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn expand_tilde() {
        for path in ["~", "~/foo"] {
            let expanded = path::expand_tilde(Path::new(path));

            let tilde = Component::Normal(OsStr::new("~"));

            let mut component_count = 0;
            for component in expanded.components() {
                // No tilde left.
                assert_ne!(component, tilde);
                component_count += 1;
            }

            // The path was at least expanded to something.
            assert_ne!(component_count, 0);
        }
    }

    macro_rules! assert_match {
        ($regex: expr, $haystack: expr) => {
            let haystack = Input::new(RopeSlice::from($haystack));
            assert!(
                $regex.is_match(haystack),
                "regex should match {}",
                $haystack
            );
        };
    }
    macro_rules! assert_no_match {
        ($regex: expr, $haystack: expr) => {
            let haystack = Input::new(RopeSlice::from($haystack));
            assert!(
                !$regex.is_match(haystack),
                "regex should not match {}",
                $haystack
            );
        };
    }

    macro_rules! assert_matches {
        ($regex: expr, $haystack: expr, [$($matches: expr),*]) => {
            let src = $haystack;
            let matches: Vec<_> = $regex
                .find_iter(Input::new(RopeSlice::from(src)))
                .map(|it| &src[it.range()])
                .collect();
            assert_eq!(matches, vec![$($matches),*]);
        };
    }

    /// Linux-only path
    #[test]
    fn path_regex_unix() {
        let _guard = isfname_guard();
        // due to ambiguity with the `\` path separator we can't support space escapes `\ ` on windows
        let regex = compile_path_regex("^", "$", false, false);
        assert_match!(regex, "${FOO}/hello\\ world");
        assert_match!(regex, "${FOO}/\\ ");
    }

    /// Windows-only paths
    #[test]
    fn path_regex_windows() {
        let _guard = isfname_guard();
        let regex = compile_path_regex("^", "$", false, true);
        assert_match!(regex, "${FOO}/hello^ world");
        assert_match!(regex, "${FOO}/hello` world");
        assert_match!(regex, "${FOO}/^ ");
        assert_match!(regex, "${FOO}/` ");
        assert_match!(regex, r"foo\bar");
        assert_match!(regex, r"foo\bar");
        assert_match!(regex, r"..\bar");
        assert_match!(regex, r"..\");
        assert_match!(regex, r"C:\");
        assert_match!(regex, r"\\?\C:\foo");
        assert_match!(regex, r"\\server\foo");
    }

    /// Paths that should work on all platforms
    #[test]
    fn path_regex() {
        let _guard = isfname_guard();
        for windows in [false, true] {
            let regex = compile_path_regex("^", "$", false, windows);
            assert_no_match!(regex, "foo");
            assert_no_match!(regex, "");
            assert_match!(regex, "https://github.com/notifications/query=foo");
            assert_match!(regex, "file:///foo/bar");
            assert_match!(regex, "foo/bar");
            assert_match!(regex, "$HOME/foo");
            assert_match!(regex, "${FOO:-bar}/baz");
            assert_match!(regex, "foo/bar_");
            assert_match!(regex, "/home/bar");
            assert_match!(regex, "foo/");
            assert_match!(regex, "./");
            assert_match!(regex, "../");
            assert_match!(regex, "../..");
            assert_match!(regex, "./foo");
            assert_match!(regex, "./foo.rs");
            assert_match!(regex, "/");
            assert_match!(regex, "~/");
            assert_match!(regex, "~/foo");
            assert_match!(regex, "~/foo");
            assert_match!(regex, "~/foo/../baz");
            assert_match!(regex, "${HOME}/foo");
            assert_match!(regex, "$HOME/foo");
            assert_match!(regex, "/$FOO");
            assert_match!(regex, "/${FOO}");
            assert_match!(regex, "/${FOO}/${BAR}");
            assert_match!(regex, "/${FOO}/${BAR}/foo");
            assert_match!(regex, "/${FOO}/${BAR}");
            assert_match!(regex, "${FOO}/hello_$WORLD");
            assert_match!(regex, "${FOO}/hello_${WORLD}");
            let regex = compile_path_regex("", "", false, windows);
            assert_no_match!(regex, "");
            assert_matches!(
                regex,
                r#"${FOO}/hello_${WORLD}  ${FOO}/hello_${WORLD} foo("./bar", "/home/foo")""#,
                [
                    "${FOO}/hello_${WORLD}",
                    "${FOO}/hello_${WORLD}",
                    "./bar",
                    "/home/foo"
                ]
            );
            assert_matches!(
                regex,
                r#"--> zmax-stdx/src/path.rs:427:13"#,
                ["zmax-stdx/src/path.rs"]
            );
            assert_matches!(
                regex,
                r#"PATH=/foo/bar:/bar/baz:${foo:-/foo}/bar:${PATH}"#,
                ["/foo/bar", "/bar/baz", "${foo:-/foo}/bar"]
            );
            let regex = compile_path_regex("^", "$", true, windows);
            assert_no_match!(regex, "");
            assert_match!(regex, "foo");
            assert_match!(regex, "foo/");
            assert_match!(regex, "$FOO");
            assert_match!(regex, "${BAR}");
        }
    }

    /// vim 'isfname': single chars, `a-b` and `48-57` ranges, `@` (already in
    /// `\w`), the `,,` literal comma, and `^x` exclusions.
    #[test]
    fn isfname_is_parsed_like_vim() {
        // vim's own unix default.
        let unix = path::parse_isfname(r"@,48-57,/,\,.,-,_,+,,,#,$,%,~,=");
        for c in [
            '0', '9', '/', '\\', '.', '-', '_', '+', ',', '#', '$', '%', '~', '=',
        ] {
            assert!(unix.contains(&c), "isfname default should allow {c:?}");
        }
        // `@` contributes nothing of its own (it names the alphabetic chars).
        assert!(!unix.contains(&'@'));
        // A range of characters, and an exclusion removing one of them again.
        assert_eq!(path::parse_isfname("a-e,^c"), vec!['a', 'b', 'd', 'e']);
        // `@-@` is the literal `@` (not the alphabetic class).
        assert!(path::parse_isfname("@-@").contains(&'@'));
    }

    /// `:set isfname` must reach the compiled path scan: a colon is not a
    /// file-name character by default (so `path:12` stops at the colon), and
    /// adding it makes the same text scan as one path.
    #[test]
    fn set_isfname_changes_the_path_scan() {
        let _guard = isfname_guard();
        let suffix =
            |src: &str| path::get_path_suffix(RopeSlice::from(src), false).map(|s| s.to_string());
        assert_eq!(suffix("src/main.rs"), Some("src/main.rs".to_string()));

        path::set_isfname(r"@,48-57,/,.,-,_,:");
        assert_eq!(suffix("src/main.rs:12"), Some("src/main.rs:12".to_string()));
        // A character left out of 'isfname' now ends the path.
        assert_eq!(suffix("src/main.rs#x"), None);

        path::set_isfname("");
        assert_eq!(suffix("src/main.rs"), Some("src/main.rs".to_string()));
    }
}
