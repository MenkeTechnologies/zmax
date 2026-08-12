//! Filesets — named groups of files you operate on at once, the zmax port of
//! GNU Emacs `filesets.el` (Emacs manual, "Filesets").
//!
//! > If you regularly edit a certain group of files, you can define them as a
//! > "fileset". This lets you perform certain operations, such as visiting,
//! > `query-replace`, and shell commands on all the files at once.
//!
//! A fileset is either an explicit list of files (`:files`, what
//! `filesets-add-buffer` builds) or a regular expression matched against file
//! names under a directory (`:pattern`, the "more complicated" kind the manual
//! points at). [`files`] resolves either into the concrete list the commands act
//! on.
//!
//! `filesets-data` is a `defcustom`, so Emacs persists it with the rest of
//! Customize; here it is `<config-dir>/filesets`, one fileset per line as
//! `name<TAB>kind<TAB>value[<TAB>value…]` (`kind` is `files` or `pattern`).
//! Nothing is read from that file until [`init`] runs — `filesets-init` is what
//! the manual tells you to put in your init file, and it is what arms the
//! subsystem here too.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const FILE_NAME: &str = "filesets";

/// One fileset: a name plus how its file list is produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fileset {
    pub name: String,
    pub kind: Kind,
}

/// `filesets-entry-mode`: the two fileset kinds zmax resolves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `:files` — an explicit list, what `filesets-add-buffer` maintains.
    Files(Vec<PathBuf>),
    /// `:pattern` — every file under `dir` whose name matches `regexp`.
    Pattern { dir: PathBuf, regexp: String },
}

/// `filesets-init` has run: the store is loaded and the commands are live.
static INITIALIZED: AtomicBool = AtomicBool::new(false);
/// `filesets-data` — the fileset list, once loaded.
static DATA: Mutex<Vec<Fileset>> = Mutex::new(Vec::new());

fn store_path() -> PathBuf {
    zmax_loader::config_dir().join(FILE_NAME)
}

/// `filesets-init`: load the saved `filesets-data` and arm the subsystem.
/// Returns how many filesets were loaded. Re-running it re-reads the store,
/// which is what makes it usable after editing the file by hand.
pub fn init() -> usize {
    let loaded = read_store();
    let n = loaded.len();
    if let Ok(mut data) = DATA.lock() {
        *data = loaded;
    }
    INITIALIZED.store(true, Ordering::Relaxed);
    n
}

/// Whether `filesets-init` has run. Every other command needs it — Emacs's do
/// too, since without it `filesets-data` is never read and the menu is absent.
pub fn initialized() -> bool {
    INITIALIZED.load(Ordering::Relaxed)
}

/// Every defined fileset, in definition order.
pub fn all() -> Vec<Fileset> {
    DATA.lock().map(|d| d.clone()).unwrap_or_default()
}

/// The fileset called `name`, if it exists.
pub fn get(name: &str) -> Option<Fileset> {
    all().into_iter().find(|f| f.name == name)
}

/// `filesets-get-filelist`: the concrete files of the fileset called `name`.
/// A `:files` fileset yields its list (dead paths included — Emacs keeps them
/// until you remove them); a `:pattern` fileset is expanded against the
/// directory each time, so new matching files join it automatically.
pub fn files(name: &str) -> Option<Vec<PathBuf>> {
    match get(name)?.kind {
        Kind::Files(files) => Some(files),
        Kind::Pattern { dir, regexp } => expand_pattern(&dir, &regexp),
    }
}

/// Expand a `:pattern` fileset: every regular file directly in `dir` whose name
/// matches `regexp`, sorted. `None` when the regexp does not compile or the
/// directory cannot be read.
fn expand_pattern(dir: &Path, regexp: &str) -> Option<Vec<PathBuf>> {
    let re = regex::Regex::new(regexp).ok()?;
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.file_name()
                .map(|n| re.is_match(&n.to_string_lossy()))
                .unwrap_or(false)
        })
        .collect();
    out.sort();
    Some(out)
}

/// `filesets-add-buffer`: add `file` to the fileset called `name`, creating the
/// fileset when there is none (Emacs asks first; a `:filesets-add-buffer NAME`
/// that names a new set *is* the answer). Returns whether the file was added —
/// `false` when it was already in the set, or the set is a `:pattern` one, which
/// Emacs refuses to add single files to.
pub fn add_buffer(name: &str, file: &Path) -> Result<bool, String> {
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut data = DATA.lock().map_err(|_| "filesets: lock poisoned")?;
    let entry = match data.iter_mut().find(|f| f.name == name) {
        Some(entry) => entry,
        None => {
            data.push(Fileset {
                name: name.to_string(),
                kind: Kind::Files(Vec::new()),
            });
            data.last_mut().expect("just pushed")
        }
    };
    let added = match &mut entry.kind {
        Kind::Files(files) => {
            if files.contains(&file) {
                false
            } else {
                files.push(file);
                true
            }
        }
        Kind::Pattern { .. } => {
            return Err(format!("filesets: `{name}` is a pattern fileset"));
        }
    };
    let snapshot = data.clone();
    drop(data);
    if added {
        write_store(&snapshot);
    }
    Ok(added)
}

/// `filesets-remove-buffer`: drop `file` from the fileset called `name`.
/// Returns whether it was in the set.
pub fn remove_buffer(name: &str, file: &Path) -> Result<bool, String> {
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut data = DATA.lock().map_err(|_| "filesets: lock poisoned")?;
    let entry = data
        .iter_mut()
        .find(|f| f.name == name)
        .ok_or_else(|| format!("filesets: unknown fileset `{name}`"))?;
    let removed = match &mut entry.kind {
        Kind::Files(files) => {
            let before = files.len();
            files.retain(|f| f != &file);
            before != files.len()
        }
        Kind::Pattern { .. } => {
            return Err(format!("filesets: `{name}` is a pattern fileset"));
        }
    };
    let snapshot = data.clone();
    drop(data);
    if removed {
        write_store(&snapshot);
    }
    Ok(removed)
}

/// Define (or redefine) a `:pattern` fileset — the manual's "fileset as a
/// regular expression matching file names".
pub fn define_pattern(name: &str, dir: &Path, regexp: &str) -> Result<(), String> {
    regex::Regex::new(regexp).map_err(|e| format!("filesets: {e}"))?;
    let kind = Kind::Pattern {
        dir: dir.to_path_buf(),
        regexp: regexp.to_string(),
    };
    let mut data = DATA.lock().map_err(|_| "filesets: lock poisoned")?;
    match data.iter_mut().find(|f| f.name == name) {
        Some(entry) => entry.kind = kind,
        None => data.push(Fileset {
            name: name.to_string(),
            kind,
        }),
    }
    let snapshot = data.clone();
    drop(data);
    write_store(&snapshot);
    Ok(())
}

/// Delete the whole fileset called `name`. Returns whether it existed.
pub fn delete(name: &str) -> bool {
    let Ok(mut data) = DATA.lock() else {
        return false;
    };
    let before = data.len();
    data.retain(|f| f.name != name);
    let removed = before != data.len();
    let snapshot = data.clone();
    drop(data);
    if removed {
        write_store(&snapshot);
    }
    removed
}

/// Parse the store. A line is `name<TAB>files<TAB>path…` or
/// `name<TAB>pattern<TAB>dir<TAB>regexp`.
fn read_store() -> Vec<Fileset> {
    let Ok(contents) = std::fs::read_to_string(store_path()) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?.to_string();
            let kind = match parts.next()? {
                "pattern" => Kind::Pattern {
                    dir: PathBuf::from(parts.next()?),
                    regexp: parts.next()?.to_string(),
                },
                "files" => Kind::Files(parts.map(PathBuf::from).collect()),
                _ => return None,
            };
            (!name.is_empty()).then_some(Fileset { name, kind })
        })
        .collect()
}

fn write_store(data: &[Fileset]) {
    let body = data
        .iter()
        .map(|f| match &f.kind {
            Kind::Files(files) => {
                let paths: Vec<String> = files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                format!("{}\tfiles\t{}", f.name, paths.join("\t"))
            }
            Kind::Pattern { dir, regexp } => {
                format!("{}\tpattern\t{}\t{regexp}", f.name, dir.to_string_lossy())
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let store = store_path();
    if let Some(parent) = store.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(store, body);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `:files` fileset round-trips through the store format, and a
    /// `:pattern` one keeps its directory and regexp — the two shapes share one
    /// line format, so a parser that confused them would lose a set silently.
    #[test]
    fn store_format_round_trips_both_fileset_kinds() {
        let data = vec![
            Fileset {
                name: "docs".into(),
                kind: Kind::Files(vec![PathBuf::from("/tmp/a.md"), PathBuf::from("/tmp/b.md")]),
            },
            Fileset {
                name: "rust".into(),
                kind: Kind::Pattern {
                    dir: PathBuf::from("/tmp/src"),
                    regexp: r"\.rs$".into(),
                },
            },
        ];
        // Re-parse what `write_store` would have written.
        let body = data
            .iter()
            .map(|f| match &f.kind {
                Kind::Files(files) => format!(
                    "{}\tfiles\t{}",
                    f.name,
                    files
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("\t")
                ),
                Kind::Pattern { dir, regexp } => {
                    format!("{}\tpattern\t{}\t{regexp}", f.name, dir.to_string_lossy())
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let parsed: Vec<Fileset> = body
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let name = parts.next()?.to_string();
                let kind = match parts.next()? {
                    "pattern" => Kind::Pattern {
                        dir: PathBuf::from(parts.next()?),
                        regexp: parts.next()?.to_string(),
                    },
                    "files" => Kind::Files(parts.map(PathBuf::from).collect()),
                    _ => return None,
                };
                Some(Fileset { name, kind })
            })
            .collect();
        assert_eq!(parsed, data);
    }

    /// A pattern fileset expands against the directory every time, so files
    /// created after the set was defined join it — the reason to define one.
    /// Directories never enter the list even when their name matches.
    #[test]
    fn pattern_filesets_expand_lazily() {
        let dir = std::env::temp_dir().join(format!("zmax-fileset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("one.rs"), "").expect("write");
        std::fs::write(dir.join("notes.txt"), "").expect("write");
        std::fs::create_dir_all(dir.join("nested.rs")).expect("a directory named like a match");

        let first = expand_pattern(&dir, r"\.rs$").expect("expands");
        assert_eq!(
            first,
            vec![dir.join("one.rs")],
            "only the matching regular file"
        );

        std::fs::write(dir.join("two.rs"), "").expect("write");
        let second = expand_pattern(&dir, r"\.rs$").expect("expands");
        assert_eq!(
            second,
            vec![dir.join("one.rs"), dir.join("two.rs")],
            "a new matching file joins the set"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
