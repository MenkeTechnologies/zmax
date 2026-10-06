//! Line bookmarks (JetBrains Bookmarks): `(file, line)` marks kept in named
//! lists, as the IDE's Bookmarks tool window shows them.
//!
//! New bookmarks go into the default list. A line can be in several lists
//! ("Add Bookmark to Another List"); removing it removes it from all of them.
//! Each list keeps the order its bookmarks were added in, which "Move Up" /
//! "Move Down" change; the views show that order unless sorting is on. File
//! bookmarks are the harpoon pins (`crate::harpoon`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The list a fresh store starts with, as the IDE names it.
pub const DEFAULT_LIST: &str = "Bookmarks";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub path: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// The IDE's "description" (Bookmarks view "Edit…").
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookmarkList {
    pub name: String,
    pub marks: Vec<Bookmark>,
}

/// The Bookmarks tool window's options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// "Sort Bookmarks by Type & Name": views list by path and line instead
    /// of the order bookmarks were added and moved in.
    pub sort: bool,
    /// "Ask Before Deleting Lists".
    pub confirm_list_delete: bool,
    /// "Ask Before Rewriting Mnemonic": moving a mnemonic already on another
    /// line asks first.
    pub confirm_mnemonic_move: bool,
    /// "Always Select Opened Element": the tool window follows the editor.
    pub autoscroll_from_source: bool,
    /// "Navigate with Single Click": moving the selection opens the bookmark.
    pub autoscroll_to_source: bool,
    /// "Enable Preview Tab": a bookmark opened from the views replaces the
    /// previous one's buffer instead of adding another.
    pub preview_tab: bool,
}

struct Store {
    lists: Vec<BookmarkList>,
    default: String,
    options: Options,
}

static STORE: Mutex<Store> = Mutex::new(Store {
    lists: Vec::new(),
    default: String::new(),
    options: Options {
        sort: true,
        confirm_list_delete: true,
        confirm_mnemonic_move: false,
        autoscroll_from_source: false,
        autoscroll_to_source: false,
        preview_tab: false,
    },
});

fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut store = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if store.lists.is_empty() {
        store.lists.push(BookmarkList {
            name: DEFAULT_LIST.to_owned(),
            marks: Vec::new(),
        });
    }
    if store.default.is_empty() || !store.lists.iter().any(|l| l.name == store.default) {
        store.default = store.lists[0].name.clone();
    }
    f(&mut store)
}

fn at(path: &Path, line: usize) -> impl Fn(&Bookmark) -> bool + '_ {
    move |b| b.path == path && b.line == line
}

/// Every bookmarked line once, in list order then order within the list —
/// or by path and line when sorting is on.
pub fn all() -> Vec<(PathBuf, usize)> {
    with(|store| {
        let mut out: Vec<(PathBuf, usize)> = Vec::new();
        for mark in store.lists.iter().flat_map(|l| &l.marks) {
            if !out.iter().any(|(p, l)| *p == mark.path && *l == mark.line) {
                out.push((mark.path.clone(), mark.line));
            }
        }
        if store.options.sort {
            out.sort();
        }
        out
    })
}

/// The lists, each in its display order.
pub fn lists() -> Vec<BookmarkList> {
    with(|store| {
        let mut lists = store.lists.clone();
        if store.options.sort {
            for list in &mut lists {
                list.marks
                    .sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
            }
        }
        lists
    })
}

pub fn list_names() -> Vec<String> {
    with(|store| store.lists.iter().map(|l| l.name.clone()).collect())
}

pub fn default_list() -> String {
    with(|store| store.default.clone())
}

pub fn options() -> Options {
    with(|store| store.options)
}

pub fn set_options(f: impl FnOnce(&mut Options)) -> Options {
    with(|store| {
        f(&mut store.options);
        store.options
    })
}

pub fn contains(path: &Path, line: usize) -> bool {
    with(|store| {
        store
            .lists
            .iter()
            .any(|l| l.marks.iter().any(at(path, line)))
    })
}

/// The bookmark on `path:line`, from the first list holding it.
pub fn get(path: &Path, line: usize) -> Option<Bookmark> {
    with(|store| {
        store
            .lists
            .iter()
            .find_map(|l| l.marks.iter().find(|m| at(path, line)(m)).cloned())
    })
}

/// The lists `path:line` is in.
pub fn lists_of(path: &Path, line: usize) -> Vec<String> {
    with(|store| {
        store
            .lists
            .iter()
            .filter(|l| l.marks.iter().any(at(path, line)))
            .map(|l| l.name.clone())
            .collect()
    })
}

/// Bookmark `path:line` in the default list. `false` when it already was
/// bookmarked, in any list.
pub fn add(path: &Path, line: usize) -> bool {
    if contains(path, line) {
        return false;
    }
    let list = default_list();
    add_to(&list, path, line)
}

/// Put `path:line` in `list`, making the list when there is none of that name.
/// `false` when it was in that list already.
pub fn add_to(list: &str, path: &Path, line: usize) -> bool {
    with(|store| {
        let description = store
            .lists
            .iter()
            .find_map(|l| l.marks.iter().find(|m| at(path, line)(m)))
            .and_then(|m| m.description.clone());
        let index = match store.lists.iter().position(|l| l.name == list) {
            Some(index) => index,
            None => {
                store.lists.push(BookmarkList {
                    name: list.to_owned(),
                    marks: Vec::new(),
                });
                store.lists.len() - 1
            }
        };
        let marks = &mut store.lists[index].marks;
        if marks.iter().any(at(path, line)) {
            return false;
        }
        marks.push(Bookmark {
            path: path.to_path_buf(),
            line,
            description,
        });
        true
    })
}

/// Remove `path:line` from every list. `false` when it was not bookmarked.
pub fn remove(path: &Path, line: usize) -> bool {
    with(|store| {
        let mut removed = false;
        for list in &mut store.lists {
            let before = list.marks.len();
            list.marks.retain(|m| !at(path, line)(m));
            removed |= list.marks.len() < before;
        }
        removed
    })
}

/// Add a list with no bookmarks. `false` when the name is taken.
pub fn create_list(name: &str) -> bool {
    with(|store| {
        if store.lists.iter().any(|l| l.name == name) {
            return false;
        }
        store.lists.push(BookmarkList {
            name: name.to_owned(),
            marks: Vec::new(),
        });
        true
    })
}

/// Delete a list and its bookmarks. The last list is emptied rather than
/// deleted, since new bookmarks need a list to go into.
pub fn delete_list(name: &str) -> bool {
    with(|store| {
        let Some(index) = store.lists.iter().position(|l| l.name == name) else {
            return false;
        };
        if store.lists.len() == 1 {
            store.lists[0].marks.clear();
        } else {
            store.lists.remove(index);
        }
        true
    })
}

/// Make `name` the list new bookmarks go into.
pub fn set_default_list(name: &str) -> bool {
    with(|store| {
        let found = store.lists.iter().any(|l| l.name == name);
        if found {
            store.default = name.to_owned();
        }
        found
    })
}

/// Set (`Some`) or clear the description of `path:line` in every list.
pub fn set_description(path: &Path, line: usize, description: Option<String>) -> bool {
    with(|store| {
        let mut found = false;
        for mark in store.lists.iter_mut().flat_map(|l| &mut l.marks) {
            if at(path, line)(mark) {
                mark.description = description.clone();
                found = true;
            }
        }
        found
    })
}

/// Move `path:line` one place up (`up`) or down within each list holding it.
/// `false` when it is not bookmarked or already at that end of every list.
pub fn move_mark(path: &Path, line: usize, up: bool) -> bool {
    with(|store| {
        let mut moved = false;
        for list in &mut store.lists {
            let Some(i) = list.marks.iter().position(at(path, line)) else {
                continue;
            };
            let j = if up { i.checked_sub(1) } else { Some(i + 1) };
            if let Some(j) = j.filter(|&j| j < list.marks.len()) {
                list.marks.swap(i, j);
                moved = true;
            }
        }
        moved
    })
}

/// Drop every bookmark and list; for tests, which share the process-global
/// store.
#[cfg(test)]
pub fn clear() {
    with(|store| {
        store.lists.clear();
        store.default.clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test, since the store is process-global and tests run in parallel.
    #[test]
    fn lists_default_order_and_descriptions() {
        clear();
        set_options(|o| o.sort = false);
        let (a, b) = (Path::new("/p/a.rs"), Path::new("/p/b.rs"));

        assert!(add(b, 3));
        assert!(add(a, 1));
        assert!(!add(a, 1), "a line is bookmarked once");
        assert_eq!(vec![(b.to_path_buf(), 3), (a.to_path_buf(), 1)], all());

        assert!(move_mark(a, 1, true));
        assert_eq!(vec![(a.to_path_buf(), 1), (b.to_path_buf(), 3)], all());
        assert!(!move_mark(a, 1, true), "already first");

        assert!(create_list("todo"));
        assert!(set_default_list("todo"));
        assert!(add(a, 9));
        assert_eq!(vec!["todo".to_owned()], lists_of(a, 9));

        // In a second list; the description follows it there.
        assert!(set_description(b, 3, Some("entry".into())));
        assert!(add_to("todo", b, 3));
        assert_eq!(
            vec![DEFAULT_LIST.to_owned(), "todo".to_owned()],
            lists_of(b, 3)
        );
        assert_eq!(
            Some("entry".to_owned()),
            get(b, 3).and_then(|m| m.description)
        );

        assert!(remove(b, 3));
        assert!(!contains(b, 3), "removed from every list");

        assert!(delete_list("todo"));
        assert!(!contains(a, 9), "a list's bookmarks go with it");
        assert_eq!(DEFAULT_LIST, default_list(), "the default falls back");

        set_options(|o| o.sort = true);
        clear();
    }
}
