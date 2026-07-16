//! Vim signs (`:sign`): user-defined markers placed on buffer lines and drawn in
//! a dedicated gutter column.
//!
//! A sign is a two-part thing in vim: a *definition* (`:sign define {name}
//! text={t} texthl={hl}`) names a glyph + highlight, and a *placement*
//! (`:sign place {id} line={ln} name={name} file={f}`) puts that named sign on a
//! line of a file. Placements are addressed by an id, unique within a *group*
//! (the empty string is vim's global group); `:sign unplace` removes by id
//! and/or group, and `:sign unplace *` clears everything.
//!
//! The store is process-global (mirroring the blame gutter and Hi-Lock), because
//! [`crate::editor::GutterType::width`] decides the sign column's width without an
//! `Editor` in hand — it queries [`has_signs`] here instead. The [`SignStore`]
//! type carries the whole model and is unit-tested directly on local instances;
//! the free functions are thin wrappers over the shared instance used at runtime.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use once_cell::sync::Lazy;

/// vim's default sign priority (`:help sign-priority`).
pub const DEFAULT_PRIORITY: i64 = 10;

/// A sign definition: the glyph shown in the gutter and its highlight group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignDef {
    /// One or two display cells shown in the sign column (vim `text=`).
    pub text: String,
    /// Highlight-group name for the sign text, if any (vim `texthl=`).
    pub texthl: Option<String>,
}

/// A placed sign: a definition put on a specific line of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedSign {
    /// Placement id, unique within `group` for a file.
    pub id: i64,
    /// Sign group (`""` is vim's global group).
    pub group: String,
    /// 0-based line.
    pub line: usize,
    /// The name of the [`SignDef`] this placement shows.
    pub name: String,
    /// Higher priority wins when several signs land on one line (vim).
    pub priority: i64,
}

/// The full sign model: named definitions plus per-file placements.
#[derive(Debug, Default)]
pub struct SignStore {
    defs: HashMap<String, SignDef>,
    placed: HashMap<PathBuf, Vec<PlacedSign>>,
}

impl SignStore {
    /// `:sign define {name} …` — (re)define a sign.
    pub fn define(&mut self, name: &str, def: SignDef) {
        self.defs.insert(name.to_string(), def);
    }

    /// `:sign undefine {name}` — remove a definition. Returns whether one existed.
    pub fn undefine(&mut self, name: &str) -> bool {
        self.defs.remove(name).is_some()
    }

    /// Whether `name` is a defined sign.
    pub fn is_defined(&self, name: &str) -> bool {
        self.defs.contains_key(name)
    }

    /// All definitions, sorted by name (for `:sign list`).
    pub fn definitions(&self) -> Vec<(String, SignDef)> {
        let mut v: Vec<_> = self
            .defs
            .iter()
            .map(|(n, d)| (n.clone(), d.clone()))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// `:sign place …` — place `sign` in `path`. An existing placement with the
    /// same `(id, group)` in the file is replaced (vim: id is unique per group).
    /// Errors if the sign name is not defined.
    pub fn place(&mut self, path: &Path, sign: PlacedSign) -> Result<(), String> {
        if !self.defs.contains_key(&sign.name) {
            return Err(format!("Unknown sign: {}", sign.name));
        }
        let v = self.placed.entry(path.to_path_buf()).or_default();
        v.retain(|s| !(s.id == sign.id && s.group == sign.group));
        v.push(sign);
        Ok(())
    }

    /// `:sign unplace {id} [group=]` — remove matching placements from `path`.
    /// `id`/`group` = `None` match anything, so `unplace(path, None, None)` clears
    /// the file. Returns the number removed.
    pub fn unplace(&mut self, path: &Path, id: Option<i64>, group: Option<&str>) -> usize {
        let Some(v) = self.placed.get_mut(path) else {
            return 0;
        };
        let before = v.len();
        v.retain(|s| {
            let id_match = id.is_none_or(|i| s.id == i);
            let grp_match = group.is_none_or(|g| s.group == g);
            !(id_match && grp_match)
        });
        before - v.len()
    }

    /// `:sign unplace *` — remove every placed sign in every file. Returns count.
    pub fn unplace_all(&mut self) -> usize {
        let n = self.placed.values().map(Vec::len).sum();
        self.placed.clear();
        n
    }

    /// All placements in `path` (for `:sign place` listing and `:sign jump`).
    pub fn placed_in(&self, path: &Path) -> Vec<PlacedSign> {
        self.placed.get(path).cloned().unwrap_or_default()
    }

    /// Whether `path` has any placed sign (drives the gutter column width).
    pub fn has_signs(&self, path: &Path) -> bool {
        self.placed.get(path).is_some_and(|v| !v.is_empty())
    }

    /// The sign to draw on each line of `path`: the highest-priority placement per
    /// line (ties broken by larger id, like vim), resolved to `(line, text,
    /// texthl)`. Placements whose name is no longer defined are skipped.
    pub fn line_signs(&self, path: &Path) -> Vec<(usize, String, Option<String>)> {
        let Some(v) = self.placed.get(path) else {
            return Vec::new();
        };
        let mut best: HashMap<usize, &PlacedSign> = HashMap::new();
        for s in v {
            // Skip placements whose definition no longer exists so a lower-priority
            // but still-defined sign on the same line can win.
            if !self.defs.contains_key(&s.name) {
                continue;
            }
            match best.get(&s.line) {
                Some(cur) if (cur.priority, cur.id) >= (s.priority, s.id) => {}
                _ => {
                    best.insert(s.line, s);
                }
            }
        }
        best.into_iter()
            .map(|(line, s)| {
                let def = &self.defs[&s.name];
                (line, def.text.clone(), def.texthl.clone())
            })
            .collect()
    }

    /// Find a placement by id (optionally within a group) in `path`, for
    /// `:sign jump`.
    pub fn find(&self, path: &Path, id: i64, group: Option<&str>) -> Option<PlacedSign> {
        self.placed
            .get(path)?
            .iter()
            .find(|s| s.id == id && group.is_none_or(|g| s.group == g))
            .cloned()
    }
}

// --- Process-global instance used at runtime -------------------------------

static SIGNS: Lazy<Mutex<SignStore>> = Lazy::new(|| Mutex::new(SignStore::default()));

/// `:sign define` on the global store.
pub fn define(name: &str, def: SignDef) {
    SIGNS.lock().unwrap().define(name, def);
}

/// `:sign undefine` on the global store.
pub fn undefine(name: &str) -> bool {
    SIGNS.lock().unwrap().undefine(name)
}

/// Whether `name` is defined in the global store.
pub fn is_defined(name: &str) -> bool {
    SIGNS.lock().unwrap().is_defined(name)
}

/// All global definitions, sorted by name.
pub fn definitions() -> Vec<(String, SignDef)> {
    SIGNS.lock().unwrap().definitions()
}

/// `:sign place` on the global store.
pub fn place(path: &Path, sign: PlacedSign) -> Result<(), String> {
    SIGNS.lock().unwrap().place(path, sign)
}

/// `:sign unplace` on the global store.
pub fn unplace(path: &Path, id: Option<i64>, group: Option<&str>) -> usize {
    SIGNS.lock().unwrap().unplace(path, id, group)
}

/// `:sign unplace *` on the global store.
pub fn unplace_all() -> usize {
    SIGNS.lock().unwrap().unplace_all()
}

/// All placements in `path` from the global store.
pub fn placed_in(path: &Path) -> Vec<PlacedSign> {
    SIGNS.lock().unwrap().placed_in(path)
}

/// Whether `path` has any placed sign in the global store (gutter width).
pub fn has_signs(path: &Path) -> bool {
    SIGNS.lock().unwrap().has_signs(path)
}

/// Per-line signs to draw for `path` from the global store.
pub fn line_signs(path: &Path) -> Vec<(usize, String, Option<String>)> {
    SIGNS.lock().unwrap().line_signs(path)
}

/// Find a placement by id/group in `path` from the global store.
pub fn find(path: &Path, id: i64, group: Option<&str>) -> Option<PlacedSign> {
    SIGNS.lock().unwrap().find(path, id, group)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(text: &str) -> SignDef {
        SignDef {
            text: text.to_string(),
            texthl: None,
        }
    }

    fn placed(id: i64, line: usize, name: &str, priority: i64) -> PlacedSign {
        PlacedSign {
            id,
            group: String::new(),
            line,
            name: name.to_string(),
            priority,
        }
    }

    #[test]
    fn place_requires_a_definition() {
        let mut s = SignStore::default();
        let p = Path::new("/f.rs");
        assert!(s.place(p, placed(1, 0, "warn", DEFAULT_PRIORITY)).is_err());
        s.define("warn", def(">>"));
        assert!(s.place(p, placed(1, 0, "warn", DEFAULT_PRIORITY)).is_ok());
        assert!(s.has_signs(p));
    }

    #[test]
    fn same_id_and_group_replaces() {
        let mut s = SignStore::default();
        let p = Path::new("/f.rs");
        s.define("a", def("A"));
        s.define("b", def("B"));
        s.place(p, placed(1, 3, "a", 10)).unwrap();
        s.place(p, placed(1, 7, "b", 10)).unwrap(); // same id → replaces
        let all = s.placed_in(p);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].line, 7);
        assert_eq!(all[0].name, "b");
    }

    #[test]
    fn unplace_by_id_and_group_and_star() {
        let mut s = SignStore::default();
        let p = Path::new("/f.rs");
        s.define("a", def("A"));
        s.place(p, placed(1, 1, "a", 10)).unwrap();
        s.place(p, placed(2, 2, "a", 10)).unwrap();
        let mut g = placed(3, 3, "a", 10);
        g.group = "g".into();
        s.place(p, g).unwrap();
        // Remove only id 1 in the global group.
        assert_eq!(s.unplace(p, Some(1), Some("")), 1);
        assert_eq!(s.placed_in(p).len(), 2);
        // Remove everything in group "g".
        assert_eq!(s.unplace(p, None, Some("g")), 1);
        assert_eq!(s.placed_in(p).len(), 1);
        // Global wipe.
        s.place(Path::new("/other"), placed(9, 0, "a", 10)).unwrap();
        assert_eq!(s.unplace_all(), 2);
        assert!(!s.has_signs(p));
    }

    #[test]
    fn line_signs_pick_highest_priority_then_id() {
        let mut s = SignStore::default();
        let p = Path::new("/f.rs");
        s.define("lo", def("L"));
        s.define("hi", def("H"));
        s.place(p, placed(1, 5, "lo", 10)).unwrap();
        s.place(p, placed(2, 5, "hi", 20)).unwrap(); // higher priority on same line
        let ls = s.line_signs(p);
        assert_eq!(ls, vec![(5, "H".to_string(), None)]);
        // A placement whose def was undefined is skipped.
        s.undefine("hi");
        assert_eq!(s.line_signs(p), vec![(5, "L".to_string(), None)]);
    }

    #[test]
    fn find_by_id_and_group() {
        let mut s = SignStore::default();
        let p = Path::new("/f.rs");
        s.define("a", def("A"));
        s.place(p, placed(4, 9, "a", 10)).unwrap();
        assert_eq!(s.find(p, 4, None).unwrap().line, 9);
        assert_eq!(s.find(p, 4, Some("")).unwrap().line, 9);
        assert!(s.find(p, 4, Some("nope")).is_none());
        assert!(s.find(p, 99, None).is_none());
    }
}
