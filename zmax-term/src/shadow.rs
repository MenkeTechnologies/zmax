//! File shadowing — the zmax port of GNU Emacs `shadowfile.el` (Emacs manual,
//! "Shadowing Files").
//!
//! > You can arrange to keep identical "shadow" copies of certain files in more
//! > than one place — possibly on different machines. To do this, first you must
//! > set up a "shadow file group", which is a set of identically-named files
//! > shared between a list of sites. … Once the group is set up, every time you
//! > exit Emacs, it will copy the file you edited to the other files in its
//! > group. You can also do the copying without exiting Emacs, by typing `M-x
//! > shadow-copy-files`.
//!
//! A *site* in Emacs is a Tramp host; zmax's sites are directory prefixes (a
//! mounted remote, a synced directory, a second checkout), which is the same
//! thing minus the transport. A *cluster* names one such prefix so several
//! groups can share it, exactly as `shadow-define-cluster` names a host.
//!
//! Two files back it, as in Emacs: `shadow-info-file` (`<config-dir>/shadows`)
//! holds the group and cluster definitions, and `shadow-todo-file`
//! (`<config-dir>/shadow_todo`) holds the copies that are pending because their
//! source was saved. Saving a shadowed file appends to the todo list
//! (`shadow-add-to-todo`, driven from the save handler in
//! [`crate::application`]); `shadow-copy-files` performs and clears it.
//!
//! Nothing happens until [`initialize`] has run — `shadow-initialize` is what
//! reads the info file and installs the save hook there too.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const INFO_FILE: &str = "shadows";
const TODO_FILE: &str = "shadow_todo";

/// One shadow file group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Group {
    /// `shadow-literal-groups`: these exact files are copies of one another;
    /// saving any of them updates the rest. The names may differ per site.
    Literal(Vec<PathBuf>),
    /// `shadow-regexp-groups`: every file whose name matches `regexp` is shared
    /// between `sites`, under the same name in each.
    Regexp { regexp: String, sites: Vec<PathBuf> },
}

/// `shadow-initialize` has run.
static INITIALIZED: AtomicBool = AtomicBool::new(false);
/// `shadow-literal-groups` + `shadow-regexp-groups`.
static GROUPS: Mutex<Vec<Group>> = Mutex::new(Vec::new());
/// `shadow-clusters`: cluster name -> the directory it stands for.
static CLUSTERS: Mutex<Vec<(String, PathBuf)>> = Mutex::new(Vec::new());
/// `shadow-files-to-copy`: `(from, to)` pairs waiting for `shadow-copy-files`.
static TODO: Mutex<Vec<(PathBuf, PathBuf)>> = Mutex::new(Vec::new());

fn info_path() -> PathBuf {
    zmax_loader::config_dir().join(INFO_FILE)
}

fn todo_path() -> PathBuf {
    zmax_loader::config_dir().join(TODO_FILE)
}

/// `shadow-initialize`: read the info and todo files and arm the save hook.
/// Returns `(groups, clusters, pending)` so the caller can report what was set
/// up — Emacs prints "Shadowfile information files not found" when there is
/// nothing, and an empty result here is that same state.
pub fn initialize() -> (usize, usize, usize) {
    let (groups, clusters) = read_info();
    let todo = read_todo();
    let counts = (groups.len(), clusters.len(), todo.len());
    if let Ok(mut g) = GROUPS.lock() {
        *g = groups;
    }
    if let Ok(mut c) = CLUSTERS.lock() {
        *c = clusters;
    }
    if let Ok(mut t) = TODO.lock() {
        *t = todo;
    }
    INITIALIZED.store(true, Ordering::Relaxed);
    counts
}

/// Whether `shadow-initialize` has run — the save hook is inert until it has.
pub fn initialized() -> bool {
    INITIALIZED.load(Ordering::Relaxed)
}

/// `shadow-define-cluster`: name a site (here, a directory prefix) so groups can
/// refer to it. Redefining a name replaces it.
pub fn define_cluster(name: &str, directory: &Path) {
    let Ok(mut clusters) = CLUSTERS.lock() else {
        return;
    };
    match clusters.iter_mut().find(|(n, _)| n == name) {
        Some(entry) => entry.1 = directory.to_path_buf(),
        None => clusters.push((name.to_string(), directory.to_path_buf())),
    }
    let snapshot = clusters.clone();
    drop(clusters);
    write_info_with_clusters(&snapshot);
}

/// Every defined cluster.
pub fn clusters() -> Vec<(String, PathBuf)> {
    CLUSTERS.lock().map(|c| c.clone()).unwrap_or_default()
}

/// Resolve a site as written by the user: a cluster name stands for its
/// directory, anything else is taken as a path.
fn resolve_site(site: &str) -> PathBuf {
    clusters()
        .into_iter()
        .find(|(n, _)| n == site)
        .map(|(_, dir)| dir)
        .unwrap_or_else(|| PathBuf::from(site))
}

/// `shadow-define-literal-group`: declare that `files` are copies of one
/// another. Each entry is a path or a cluster name joined with the first file's
/// name, so `shadow-define-literal-group notes.txt backup` shadows into the
/// `backup` cluster under the same name.
pub fn define_literal_group(files: &[String]) -> Result<usize, String> {
    if files.len() < 2 {
        return Err("shadow-define-literal-group: needs at least two locations".into());
    }
    let first = PathBuf::from(&files[0]);
    let name = first
        .file_name()
        .ok_or_else(|| format!("shadow: {} has no file name", first.display()))?
        .to_owned();
    let mut group = vec![first.clone()];
    for site in &files[1..] {
        let resolved = resolve_site(site);
        // A directory (or a cluster) shadows the file under the same name; a
        // full path is the file's name at that site, which is the whole point of
        // a *literal* group ("It may have different filenames on each site").
        group.push(if resolved.is_dir() {
            resolved.join(&name)
        } else {
            resolved
        });
    }
    let n = group.len();
    push_group(Group::Literal(group));
    Ok(n)
}

/// `shadow-define-regexp-group`: share every file matching `regexp` between
/// `sites` (directories or cluster names), under the same name in each.
pub fn define_regexp_group(regexp: &str, sites: &[String]) -> Result<usize, String> {
    regex::Regex::new(regexp).map_err(|e| format!("shadow: {e}"))?;
    if sites.len() < 2 {
        return Err("shadow-define-regexp-group: needs at least two sites".into());
    }
    let sites: Vec<PathBuf> = sites.iter().map(|s| resolve_site(s)).collect();
    let n = sites.len();
    push_group(Group::Regexp {
        regexp: regexp.to_string(),
        sites,
    });
    Ok(n)
}

fn push_group(group: Group) {
    let Ok(mut groups) = GROUPS.lock() else {
        return;
    };
    groups.push(group);
    let snapshot = groups.clone();
    drop(groups);
    write_info_with_groups(&snapshot);
}

/// Every defined group.
pub fn groups() -> Vec<Group> {
    GROUPS.lock().map(|g| g.clone()).unwrap_or_default()
}

/// `shadow-shadows-of`: where `file` has to be copied to keep its group in step
/// — every other member of every group it belongs to.
pub fn shadows_of(file: &Path) -> Vec<PathBuf> {
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut out: Vec<PathBuf> = Vec::new();
    for group in groups() {
        match group {
            Group::Literal(members) => {
                if members.iter().any(|m| same_file(m, &file)) {
                    for member in members {
                        if !same_file(&member, &file) && !out.contains(&member) {
                            out.push(member);
                        }
                    }
                }
            }
            Group::Regexp { regexp, sites } => {
                let Ok(re) = regex::Regex::new(&regexp) else {
                    continue;
                };
                let Some(name) = file.file_name() else {
                    continue;
                };
                if !re.is_match(&file.to_string_lossy()) {
                    continue;
                }
                // Only a file that lives at one of the sites is in the group; the
                // shadows are the same name at every other site.
                if !sites.iter().any(|s| file.starts_with(s)) {
                    continue;
                }
                for site in sites {
                    let target = site.join(name);
                    if !same_file(&target, &file) && !out.contains(&target) {
                        out.push(target);
                    }
                }
            }
        }
    }
    out
}

/// Whether two paths name the same file, comparing canonically when both exist
/// and textually otherwise (a shadow target usually does not exist yet).
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// `shadow-add-to-todo`: a shadowed file was saved — queue its copies. Returns
/// how many were queued. Inert until `shadow-initialize` has run, as in Emacs,
/// where the hook is not installed before then.
pub fn add_to_todo(file: &Path) -> usize {
    if !initialized() {
        return 0;
    }
    let shadows = shadows_of(file);
    if shadows.is_empty() {
        return 0;
    }
    let source = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let Ok(mut todo) = TODO.lock() else {
        return 0;
    };
    let mut added = 0;
    for shadow in shadows {
        let pair = (source.clone(), shadow);
        if !todo.contains(&pair) {
            todo.push(pair);
            added += 1;
        }
    }
    let snapshot = todo.clone();
    drop(todo);
    if added > 0 {
        write_todo(&snapshot);
    }
    added
}

/// The pending copies (`shadow-files-to-copy`).
pub fn pending() -> Vec<(PathBuf, PathBuf)> {
    TODO.lock().map(|t| t.clone()).unwrap_or_default()
}

/// `shadow-copy-files`: perform every pending copy and clear the todo list.
/// Returns `(copied, errors)`; a copy that fails stays pending so the next call
/// retries it, which is what Emacs's todo file is for.
pub fn copy_files() -> (usize, Vec<String>) {
    let mut copied = 0;
    let mut errors = Vec::new();
    let mut left = Vec::new();
    for (from, to) in pending() {
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::copy(&from, &to) {
            Ok(_) => copied += 1,
            Err(e) => {
                errors.push(format!("{} -> {}: {e}", from.display(), to.display()));
                left.push((from, to));
            }
        }
    }
    if let Ok(mut todo) = TODO.lock() {
        *todo = left.clone();
    }
    write_todo(&left);
    (copied, errors)
}

/// `shadow-cancel`: forget the pending copies without performing them. Returns
/// how many were dropped.
pub fn cancel() -> usize {
    let Ok(mut todo) = TODO.lock() else {
        return 0;
    };
    let n = todo.len();
    todo.clear();
    drop(todo);
    write_todo(&[]);
    n
}

// ── the two info files ──────────────────────────────────────────────────────
// `shadows`: one definition per line —
//   cluster<TAB>name<TAB>dir
//   literal<TAB>path<TAB>path…
//   regexp<TAB>regexp<TAB>site…
// `shadow_todo`: one pending copy per line — `from<TAB>to`.

fn read_info() -> (Vec<Group>, Vec<(String, PathBuf)>) {
    let Ok(contents) = std::fs::read_to_string(info_path()) else {
        return (Vec::new(), Vec::new());
    };
    let mut groups = Vec::new();
    let mut clusters = Vec::new();
    for line in contents.lines() {
        let mut parts = line.split('\t');
        match parts.next() {
            Some("cluster") => {
                if let (Some(name), Some(dir)) = (parts.next(), parts.next()) {
                    clusters.push((name.to_string(), PathBuf::from(dir)));
                }
            }
            Some("literal") => {
                let files: Vec<PathBuf> = parts.map(PathBuf::from).collect();
                if files.len() > 1 {
                    groups.push(Group::Literal(files));
                }
            }
            Some("regexp") => {
                if let Some(regexp) = parts.next() {
                    let sites: Vec<PathBuf> = parts.map(PathBuf::from).collect();
                    if !sites.is_empty() {
                        groups.push(Group::Regexp {
                            regexp: regexp.to_string(),
                            sites,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    (groups, clusters)
}

/// `shadow-write-info-file`, rendering both halves of the file.
fn write_info(groups: &[Group], clusters: &[(String, PathBuf)]) {
    let mut lines: Vec<String> = clusters
        .iter()
        .map(|(name, dir)| format!("cluster\t{name}\t{}", dir.to_string_lossy()))
        .collect();
    for group in groups {
        lines.push(match group {
            Group::Literal(files) => format!(
                "literal\t{}",
                files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("\t")
            ),
            Group::Regexp { regexp, sites } => format!(
                "regexp\t{regexp}\t{}",
                sites
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("\t")
            ),
        });
    }
    write_lines(&info_path(), &lines.join("\n"));
}

fn write_info_with_groups(groups: &[Group]) {
    write_info(groups, &clusters());
}

fn write_info_with_clusters(clusters: &[(String, PathBuf)]) {
    write_info(&groups(), clusters);
}

fn read_todo() -> Vec<(PathBuf, PathBuf)> {
    let Ok(contents) = std::fs::read_to_string(todo_path()) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| {
            let (from, to) = line.split_once('\t')?;
            Some((PathBuf::from(from), PathBuf::from(to)))
        })
        .collect()
}

fn write_todo(todo: &[(PathBuf, PathBuf)]) {
    let body = todo
        .iter()
        .map(|(from, to)| format!("{}\t{}", from.to_string_lossy(), to.to_string_lossy()))
        .collect::<Vec<_>>()
        .join("\n");
    write_lines(&todo_path(), &body);
}

fn write_lines(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, body);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A literal group's shadows are its other members — the file that was saved
    /// is never queued as a copy of itself, which would truncate it.
    #[test]
    fn a_literal_group_shadows_every_other_member() {
        let group = Group::Literal(vec![
            PathBuf::from("/a/notes.txt"),
            PathBuf::from("/b/notes.txt"),
            PathBuf::from("/c/other-name.txt"),
        ]);
        let Group::Literal(members) = &group else {
            unreachable!()
        };
        let saved = PathBuf::from("/b/notes.txt");
        let shadows: Vec<&PathBuf> = members.iter().filter(|m| **m != saved).collect();
        assert_eq!(shadows.len(), 2);
        assert!(!shadows.contains(&&saved), "never a copy of itself");
    }

    /// A regexp group only claims files that actually live at one of its sites:
    /// a matching name somewhere else is not part of the group, or saving any
    /// `*.txt` anywhere would start overwriting the sites.
    #[test]
    fn a_regexp_group_only_claims_files_under_its_sites() {
        let sites = [PathBuf::from("/site-a"), PathBuf::from("/site-b")];
        let re = regex::Regex::new(r"\.txt$").expect("regexp");

        let inside = PathBuf::from("/site-a/notes.txt");
        assert!(re.is_match(&inside.to_string_lossy()));
        assert!(sites.iter().any(|s| inside.starts_with(s)));

        let outside = PathBuf::from("/elsewhere/notes.txt");
        assert!(re.is_match(&outside.to_string_lossy()));
        assert!(
            !sites.iter().any(|s| outside.starts_with(s)),
            "a matching name outside every site is not in the group"
        );
    }

    /// The info file round-trips clusters and both group kinds through one
    /// line-oriented format.
    #[test]
    fn info_file_format_round_trips() {
        let groups = vec![
            Group::Literal(vec![PathBuf::from("/a/x"), PathBuf::from("/b/x")]),
            Group::Regexp {
                regexp: r"\.org$".into(),
                sites: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            },
        ];
        let clusters = vec![("backup".to_string(), PathBuf::from("/mnt/backup"))];

        let mut lines: Vec<String> = clusters
            .iter()
            .map(|(n, d)| format!("cluster\t{n}\t{}", d.to_string_lossy()))
            .collect();
        for g in &groups {
            lines.push(match g {
                Group::Literal(files) => format!(
                    "literal\t{}",
                    files
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("\t")
                ),
                Group::Regexp { regexp, sites } => format!(
                    "regexp\t{regexp}\t{}",
                    sites
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("\t")
                ),
            });
        }

        // Re-parse with the same reader `read_info` uses.
        let mut parsed_groups = Vec::new();
        let mut parsed_clusters = Vec::new();
        for line in lines.join("\n").lines() {
            let mut parts = line.split('\t');
            match parts.next() {
                Some("cluster") => {
                    let (name, dir) = (parts.next().unwrap(), parts.next().unwrap());
                    parsed_clusters.push((name.to_string(), PathBuf::from(dir)));
                }
                Some("literal") => {
                    parsed_groups.push(Group::Literal(parts.map(PathBuf::from).collect()))
                }
                Some("regexp") => {
                    let regexp = parts.next().unwrap().to_string();
                    parsed_groups.push(Group::Regexp {
                        regexp,
                        sites: parts.map(PathBuf::from).collect(),
                    });
                }
                _ => {}
            }
        }
        assert_eq!(parsed_groups, groups);
        assert_eq!(parsed_clusters, clusters);
    }
}
