//! The file-name cache — the zmax port of GNU Emacs `filecache.el` (Emacs
//! manual, "File Name Cache").
//!
//! > You can use the "file name cache" to make it easy to locate a file by name,
//! > without having to remember exactly where it is located. When typing a file
//! > name in the minibuffer, `C-<TAB>` (`file-cache-minibuffer-complete`)
//! > completes it using the file name cache. If you repeat `C-<TAB>`, that
//! > cycles through the possible completions of what you had originally typed.
//!
//! `file-cache-alist` maps a bare file NAME to the list of DIRECTORIES it was
//! found in, and that is the shape here too: completion works on the name alone,
//! and once the name is unique the repeated key cycles the directories it lives
//! in. The cache "does not fill up automatically" — the `file-cache-add-*`
//! commands load it — and "is not persistent: it is kept and maintained only for
//! the duration of the … session", so nothing here touches the disk.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// `file-cache-alist`: bare file name -> the directories holding it, newest
/// directory first (`file-cache-add-file` conses onto the front).
static CACHE: Mutex<Vec<(String, Vec<PathBuf>)>> = Mutex::new(Vec::new());

/// `file-cache-last-completion` — the name the previous
/// `file-cache-minibuffer-complete` settled on, so a repeat of the key is
/// recognised as "cycle" rather than "complete again".
static LAST_COMPLETION: Mutex<Option<String>> = Mutex::new(None);

/// `file-cache-filter-regexps`: names matching these are never cached by the
/// directory-scanning commands. Emacs's shipped list, verbatim — note it is not
/// applied by `file-cache-add-file`, which caches whatever it is handed.
const FILTER_REGEXPS: &[&str] = &[
    "~$",
    r"\.o$",
    r"\.exe$",
    r"\.a$",
    r"\.elc$",
    ",v$",
    r"\.$",
    "#$",
    r"\.class$",
    r"/\.#",
];

/// Whether a scanned path is filtered out by `file-cache-filter-regexps`.
fn filtered(path: &str) -> bool {
    FILTER_REGEXPS
        .iter()
        .any(|re| regex::Regex::new(re).is_ok_and(|re| re.is_match(path)))
}

/// `file-cache-add-file`: cache one file. Returns whether it was new to the
/// cache (a name already cached under another directory counts as new).
pub fn add_file(file: &Path) -> bool {
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let (Some(name), Some(dir)) = (file.file_name(), file.parent()) else {
        return false;
    };
    let name = name.to_string_lossy().into_owned();
    let dir = dir.to_path_buf();
    let Ok(mut cache) = CACHE.lock() else {
        return false;
    };
    match cache.iter_mut().find(|(n, _)| *n == name) {
        Some((_, dirs)) => {
            if dirs.contains(&dir) {
                false
            } else {
                dirs.insert(0, dir);
                true
            }
        }
        None => {
            cache.push((name, vec![dir]));
            true
        }
    }
}

/// `file-cache-add-directory`: cache every file directly in `directory`,
/// optionally only those whose name matches `regexp`. Directories are skipped
/// and `file-cache-filter-regexps` is applied. Returns how many names were
/// added.
pub fn add_directory(directory: &Path, regexp: Option<&str>) -> Result<usize, String> {
    let re = match regexp {
        Some(r) => Some(regex::Regex::new(r).map_err(|e| format!("file-cache: {e}"))?),
        None => None,
    };
    let entries = std::fs::read_dir(directory)
        .map_err(|e| format!("file-cache: {}: {e}", directory.display()))?;
    let mut added = 0;
    for path in entries.flatten().map(|e| e.path()) {
        if !path.is_file() || filtered(&path.to_string_lossy()) {
            continue;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        if let (Some(re), Some(name)) = (&re, &name) {
            if !re.is_match(name) {
                continue;
            }
        }
        if add_file(&path) {
            added += 1;
        }
    }
    Ok(added)
}

/// `file-cache-add-directory-recursively` / `-using-find`: cache every file in
/// `directory` and all of its nested subdirectories. Emacs's `-using-find` shells
/// out to `find(1)`; the walk here is the same set of files without the fork, and
/// applies `file-cache-filter-regexps` the same way.
pub fn add_directory_recursively(directory: &Path, regexp: Option<&str>) -> Result<usize, String> {
    let re = match regexp {
        Some(r) => Some(regex::Regex::new(r).map_err(|e| format!("file-cache: {e}"))?),
        None => None,
    };
    let mut added = 0;
    let mut stack = vec![directory.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                // `find` follows the tree but not symlinked loops; a symlinked
                // directory is left alone for the same reason.
                if !path.is_symlink() {
                    stack.push(path);
                }
                continue;
            }
            if filtered(&path.to_string_lossy()) {
                continue;
            }
            if let (Some(re), Some(name)) = (&re, path.file_name()) {
                if !re.is_match(&name.to_string_lossy()) {
                    continue;
                }
            }
            if add_file(&path) {
                added += 1;
            }
        }
    }
    Ok(added)
}

/// `file-cache-clear-cache`: remove every file name from the cache. Returns how
/// many names were dropped.
pub fn clear() -> usize {
    let Ok(mut cache) = CACHE.lock() else {
        return 0;
    };
    let n = cache.len();
    cache.clear();
    if let Ok(mut last) = LAST_COMPLETION.lock() {
        *last = None;
    }
    n
}

/// `file-cache-display`: the cache's contents, as `name` -> the directories it
/// was found in, sorted by name.
pub fn display() -> Vec<(String, Vec<PathBuf>)> {
    let Ok(cache) = CACHE.lock() else {
        return Vec::new();
    };
    let mut rows = cache.clone();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// How many names the cache holds.
pub fn len() -> usize {
    CACHE.lock().map(|c| c.len()).unwrap_or(0)
}

/// What `file-cache-minibuffer-complete` decided to do with the typed text.
#[derive(Debug, PartialEq, Eq)]
pub enum Completion {
    /// The typed name completed (or cycled) to this absolute path.
    Expanded {
        path: String,
        /// `file-cache-multiple-directory-message`: `Some((n, total))` when the
        /// name lives in more than one directory and this is the n-th (1-based).
        directory: Option<(usize, usize)>,
    },
    /// `file-cache-non-unique-message`: several cached names share this prefix,
    /// which has been extended as far as they agree.
    Ambiguous { prefix: String, matches: Vec<String> },
    /// `file-cache-no-match-message`.
    NoMatch,
}

/// `file-cache-minibuffer-complete`: complete `typed` — the whole minibuffer
/// contents — against the cache.
///
/// Emacs completes on the *non-directory* part, so anything typed in front of it
/// is only there to say which directory the cycle currently sits on. Two
/// substitutions happen, in Emacs's order: first the name is completed against
/// the cached names, then, once it is unique, repeating the command cycles
/// through the directories the name was found in.
pub fn minibuffer_complete(typed: &str) -> Completion {
    let (typed_dir, name) = match typed.rfind('/') {
        Some(i) => (&typed[..=i], &typed[i + 1..]),
        None => ("", typed),
    };
    let Ok(cache) = CACHE.lock() else {
        return Completion::NoMatch;
    };

    let exact = cache.iter().find(|(n, _)| n == name);
    let candidates: Vec<&String> = cache
        .iter()
        .map(|(n, _)| n)
        .filter(|n| n.starts_with(name))
        .collect();

    // Not a cached name and nothing starts with it.
    if exact.is_none() && candidates.is_empty() {
        return Completion::NoMatch;
    }

    // The name is not (yet) cached: extend it by the longest prefix every
    // candidate shares, and report the alternatives if that is still ambiguous.
    if exact.is_none() {
        let prefix = common_prefix(&candidates);
        if candidates.len() > 1 {
            return ambiguous(prefix, candidates);
        }
        // A single candidate completes straight to its file.
        let name = candidates[0].clone();
        let dirs = cache
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, d)| d.clone())
            .unwrap_or_default();
        drop(cache);
        return expand(&name, &dirs, "", 0);
    }

    // The name is cached exactly. Emacs still prefers completing to a *longer*
    // cached name the first time (`Makefile` -> `Makefile.in`); only when the
    // command repeats on the same name does it cycle the directories.
    let repeated = LAST_COMPLETION
        .lock()
        .ok()
        .and_then(|l| l.clone())
        .is_some_and(|last| last == name);
    if !repeated && candidates.len() > 1 {
        return ambiguous(common_prefix(&candidates), candidates);
    }

    let (name, dirs) = exact.map(|(n, d)| (n.clone(), d.clone())).expect("exact");
    drop(cache);
    // Cycling: if the typed directory is one of the name's directories, move to
    // the next one, else start at the first (`file-cache-directory-name`).
    let start = dirs
        .iter()
        .position(|d| same_directory(d, typed_dir))
        .map(|i| (i + 1) % dirs.len())
        .unwrap_or(0);
    expand(&name, &dirs, typed_dir, start)
}

/// The "complete, but not unique" answer. Emacs records the string here too
/// (`file-cache-last-completion`) — without that, pressing the key again would
/// give the same message forever instead of moving on to cycling the
/// directories.
fn ambiguous(prefix: String, candidates: Vec<&String>) -> Completion {
    if let Ok(mut last) = LAST_COMPLETION.lock() {
        *last = Some(prefix.clone());
    }
    Completion::Ambiguous {
        prefix,
        matches: candidates.into_iter().cloned().collect(),
    }
}

/// Build the completion for `name` in `dirs[index]`, recording it as the last
/// completion so a repeat of the key cycles instead of re-completing.
fn expand(name: &str, dirs: &[PathBuf], _typed_dir: &str, index: usize) -> Completion {
    if let Ok(mut last) = LAST_COMPLETION.lock() {
        *last = Some(name.to_string());
    }
    let Some(dir) = dirs.get(index) else {
        return Completion::NoMatch;
    };
    Completion::Expanded {
        path: dir.join(name).to_string_lossy().into_owned(),
        directory: (dirs.len() > 1).then_some((index + 1, dirs.len())),
    }
}

/// Whether a cached directory and the directory part typed into the minibuffer
/// name the same place (`file-cache-canonical-directory`: a cached directory has
/// no trailing slash, a typed one does).
fn same_directory(cached: &Path, typed: &str) -> bool {
    !typed.is_empty() && cached.to_string_lossy().trim_end_matches('/') == typed.trim_end_matches('/')
}

/// The longest prefix every candidate shares (`completion-try-completion`).
fn common_prefix(candidates: &[&String]) -> String {
    let Some(first) = candidates.first() else {
        return String::new();
    };
    let mut end = first.len();
    for c in &candidates[1..] {
        end = end.min(
            first
                .char_indices()
                .zip(c.char_indices())
                .take_while(|((_, a), (_, b))| a == b)
                .last()
                .map(|((i, ch), _)| i + ch.len_utf8())
                .unwrap_or(0),
        );
    }
    first[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `file-cache-filter-regexps` keeps build droppings out of the cache — the
    /// scanning commands apply it, which is what makes a cached `src/` usable.
    #[test]
    fn filter_regexps_drop_build_output() {
        assert!(filtered("/tmp/main.o"));
        assert!(filtered("/tmp/lib.a"));
        assert!(filtered("/tmp/notes.txt~"));
        assert!(filtered("/tmp/Foo.class"));
        assert!(filtered("/tmp/.#lockfile"));
        assert!(!filtered("/tmp/main.rs"));
        assert!(!filtered("/tmp/Makefile"));
    }

    /// The completion extends the typed name only as far as every cached
    /// candidate agrees — `completion-try-completion`'s longest common prefix.
    #[test]
    fn ambiguous_names_extend_to_the_common_prefix() {
        let a = "Makefile".to_string();
        let b = "Makefile.in".to_string();
        let c = "Makefile.am".to_string();
        assert_eq!(common_prefix(&[&a, &b, &c]), "Makefile");
        assert_eq!(common_prefix(&[&b, &c]), "Makefile.");
        let x = "alpha".to_string();
        let y = "beta".to_string();
        assert_eq!(common_prefix(&[&x, &y]), "", "no shared prefix at all");
        assert_eq!(common_prefix(&[&x]), "alpha", "a lone candidate is itself");
    }

    /// The two substitutions Emacs documents, in order: `C-TAB` first completes
    /// the *name* (reporting "not unique" while several cached names share the
    /// prefix), and only once the name is settled does repeating it cycle
    /// through the *directories* that name lives in — wrapping at the end.
    ///
    /// The regression this guards: without recording the ambiguous answer as the
    /// last completion, the second `C-TAB` repeats "not unique" forever and the
    /// directory cycle is unreachable.
    #[test]
    fn completion_then_directory_cycling() {
        let root = std::env::temp_dir().join(format!("zmax-filecache-{}", std::process::id()));
        let (one, two) = (root.join("one"), root.join("two"));
        std::fs::create_dir_all(&one).expect("temp dir");
        std::fs::create_dir_all(&two).expect("temp dir");
        std::fs::write(one.join("Makefile"), "").expect("write");
        std::fs::write(two.join("Makefile"), "").expect("write");
        std::fs::write(one.join("Makefile.in"), "").expect("write");
        let one = std::fs::canonicalize(&one).expect("canonical");
        let two = std::fs::canonicalize(&two).expect("canonical");

        clear();
        assert_eq!(add_directory(&one, None).expect("scan"), 2);
        assert_eq!(add_directory(&two, None).expect("scan"), 1);
        assert_eq!(len(), 2, "two distinct names, three files");

        // `Makefile` and `Makefile.in` share the prefix: not unique yet.
        match minibuffer_complete("Makefile") {
            Completion::Ambiguous { prefix, matches } => {
                assert_eq!(prefix, "Makefile");
                assert_eq!(matches.len(), 2);
            }
            other => panic!("expected an ambiguous completion, got {other:?}"),
        }

        // Repeating settles on the name and picks its first directory.
        let first = match minibuffer_complete("Makefile") {
            Completion::Expanded { path, directory } => {
                assert_eq!(directory, Some((1, 2)), "two directories hold this name");
                path
            }
            other => panic!("expected the name to expand, got {other:?}"),
        };

        // …and repeating again moves to the other directory, then wraps back.
        let second = match minibuffer_complete(&first) {
            Completion::Expanded { path, directory } => {
                assert_eq!(directory, Some((2, 2)));
                path
            }
            other => panic!("expected the directory to cycle, got {other:?}"),
        };
        assert_ne!(first, second, "the cycle really moved");
        let mut seen = [first.clone(), second.clone()];
        seen.sort();
        assert_eq!(
            seen,
            [
                one.join("Makefile").to_string_lossy().into_owned(),
                two.join("Makefile").to_string_lossy().into_owned(),
            ]
        );
        match minibuffer_complete(&second) {
            Completion::Expanded { path, .. } => assert_eq!(path, first, "wraps to the first"),
            other => panic!("expected the cycle to wrap, got {other:?}"),
        }

        // A name nothing starts with is `file-cache-no-match-message`.
        assert_eq!(minibuffer_complete("zzz"), Completion::NoMatch);

        clear();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A name cached in several directories cycles: the directory currently in
    /// the minibuffer selects the *next* one, and the last wraps to the first.
    #[test]
    fn repeating_cycles_through_the_directories() {
        let dirs = [PathBuf::from("/one"), PathBuf::from("/two")];
        assert!(same_directory(&dirs[0], "/one/"));
        assert!(same_directory(&dirs[0], "/one"));
        assert!(!same_directory(&dirs[0], "/two/"));
        assert!(
            !same_directory(&dirs[0], ""),
            "nothing typed means no current directory, so the cycle starts over"
        );

        let next = |typed: &str| {
            dirs.iter()
                .position(|d| same_directory(d, typed))
                .map(|i| (i + 1) % dirs.len())
                .unwrap_or(0)
        };
        assert_eq!(next(""), 0);
        assert_eq!(next("/one/"), 1);
        assert_eq!(next("/two/"), 0, "the last directory wraps to the first");
    }
}
