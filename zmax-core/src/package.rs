//! The package system's pure half — the zmax port of Emacs `package.el`.
//!
//! # What a *package* is in zmax
//!
//! Emacs packages are Emacs Lisp libraries, and zmax runs Emacs Lisp: the
//! embedded `elisprs` interpreter (`M-x eval-buffer`, `:elisp`, `load-library`)
//! is a real evaluator with the editor bound to it as a host. So a zmax package
//! is exactly what an Emacs package is — **a directory of `.el` files** —
//! installed into `~/.zmax/elpa/<name>-<version>/`, put on the elisp load path,
//! and *activated* by evaluating its autoload/main file.
//!
//! The archives are the real ones: an ELPA archive publishes an
//! `archive-contents` file (an Emacs Lisp s-expression) listing every package,
//! its version, summary, dependencies and metadata, plus one
//! `<name>-<version>.tar` (or `.el` for a single-file package) per entry. This
//! module parses that file and models the package menu; the I/O half (HTTP fetch,
//! tar extraction, activation) lives in `zmax-term`.
//!
//! Two honest divergences from Emacs, stated once here so every command that
//! relies on them can point at it:
//!
//! * **No byte compiler.** `elisprs` is an interpreter; there is no `.elc`. What
//!   Emacs gets from byte-compiling at install time (a parse/eval check of every
//!   form) zmax gets by *evaluating* the file, which is what `package-recompile`
//!   does here.
//! * **A package's effect is whatever its elisp does.** A package whose value is a
//!   C module, a GUI widget or a font is inert in zmax — the elisp installs and
//!   loads, but the parts of the Emacs API it calls may not exist. That is a
//!   property of the elisp surface, not of the package system.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How the archive serves a package's contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    /// One `<name>-<version>.el` file.
    Single,
    /// A `<name>-<version>.tar` holding a `<name>-<version>/` directory.
    Tar,
}

impl PackageKind {
    /// The file extension the archive serves this kind under.
    pub fn extension(self) -> &'static str {
        match self {
            PackageKind::Single => "el",
            PackageKind::Tar => "tar",
        }
    }
}

/// One entry of an ELPA `archive-contents` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDesc {
    pub name: String,
    /// Version as the archive's integer vector, e.g. `[1, 3]` or `[20230823, 2214]`.
    pub version: Vec<u64>,
    /// `(name, minimum-version)` for each required package. `emacs` appears here
    /// too — it is a version requirement on the host, not an installable package.
    pub deps: Vec<(String, Vec<u64>)>,
    pub summary: String,
    pub kind: PackageKind,
    pub url: Option<String>,
    pub keywords: Vec<String>,
    pub maintainer: Option<String>,
    /// Which archive served this entry ("gnu", "nongnu", "melpa", …).
    pub archive: String,
}

impl PackageDesc {
    /// The dotted version string the archive names its files with: `[1, 3]` -> `1.3`.
    pub fn version_string(&self) -> String {
        version_string(&self.version)
    }

    /// `<name>-<version>` — the tar's top directory and the install directory name.
    pub fn dir_name(&self) -> String {
        format!("{}-{}", self.name, self.version_string())
    }

    /// The archive file this package is downloaded from, relative to the archive URL.
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.dir_name(), self.kind.extension())
    }
}

/// `[1, 3]` -> `"1.3"`.
pub fn version_string(v: &[u64]) -> String {
    v.iter().map(u64::to_string).collect::<Vec<_>>().join(".")
}

/// `"1.3"` -> `[1, 3]`. Non-numeric components (Emacs's `-alpha`, `snapshot`, …)
/// stop the parse, so `"1.2-git"` reads as `[1, 2]`.
pub fn parse_version(s: &str) -> Vec<u64> {
    s.split('.')
        .map_while(|part| part.parse::<u64>().ok())
        .collect()
}

/// Emacs `version-list-<`: element-wise, a missing element counting as 0, so
/// `1.2` < `1.2.1` and `1.10` > `1.9`.
pub fn version_cmp(a: &[u64], b: &[u64]) -> Ordering {
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

// ---------------------------------------------------------------------------
// archive-contents: a real Emacs Lisp reader, restricted to what the file uses.
// ---------------------------------------------------------------------------

/// The subset of Emacs Lisp datums an `archive-contents` file is made of.
#[derive(Debug, Clone, PartialEq)]
pub enum Sexp {
    Int(i64),
    Str(String),
    /// A bare symbol, including keywords (`:url`) and `nil`.
    Sym(String),
    /// A proper list, or an improper one — the `cdr` of a dotted pair is stored
    /// in `dot`. `(a . b)` is `List { items: [a], dot: Some(b) }`.
    List {
        items: Vec<Sexp>,
        dot: Option<Box<Sexp>>,
    },
    Vector(Vec<Sexp>),
}

impl Sexp {
    fn as_str(&self) -> Option<&str> {
        match self {
            Sexp::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_sym(&self) -> Option<&str> {
        match self {
            Sexp::Sym(s) => Some(s),
            _ => None,
        }
    }
    fn as_int(&self) -> Option<i64> {
        match self {
            Sexp::Int(i) => Some(*i),
            _ => None,
        }
    }
    /// The elements of a list or vector, ignoring any dotted tail.
    fn items(&self) -> &[Sexp] {
        match self {
            Sexp::List { items, .. } | Sexp::Vector(items) => items,
            _ => &[],
        }
    }
    /// An integer list/vector read as a version: `(1 3)` / `[1 3]` -> `[1, 3]`.
    fn as_version(&self) -> Vec<u64> {
        self.items()
            .iter()
            .filter_map(Sexp::as_int)
            .map(|i| i.max(0) as u64)
            .collect()
    }
}

/// Reader over an Emacs Lisp source string.
struct Reader<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
        }
    }

    fn skip_space(&mut self) {
        while self.pos < self.src.len() {
            match self.src[self.pos] {
                b';' => {
                    while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                c if c.is_ascii_whitespace() => self.pos += 1,
                _ => break,
            }
        }
    }

    fn read(&mut self) -> Result<Sexp, String> {
        self.skip_space();
        let c = *self
            .src
            .get(self.pos)
            .ok_or_else(|| "unexpected end of input".to_string())?;
        match c {
            b'(' => {
                self.pos += 1;
                self.read_list(b')')
            }
            b'[' => {
                self.pos += 1;
                match self.read_list(b']')? {
                    Sexp::List { items, .. } => Ok(Sexp::Vector(items)),
                    other => Ok(other),
                }
            }
            b'"' => self.read_string(),
            b'?' => {
                // A character literal (`?a`, `?\n`) — read it as its code point.
                self.pos += 1;
                let mut ch = *self.src.get(self.pos).unwrap_or(&b' ');
                self.pos += 1;
                if ch == b'\\' {
                    ch = *self.src.get(self.pos).unwrap_or(&b' ');
                    self.pos += 1;
                }
                Ok(Sexp::Int(ch as i64))
            }
            b'\'' | b'`' => {
                // Quote is transparent for our purposes: read what it quotes.
                self.pos += 1;
                self.read()
            }
            _ => self.read_atom(),
        }
    }

    fn read_list(&mut self, close: u8) -> Result<Sexp, String> {
        let mut items = Vec::new();
        let mut dot = None;
        loop {
            self.skip_space();
            match self.src.get(self.pos) {
                None => return Err("unterminated list".into()),
                Some(&c) if c == close => {
                    self.pos += 1;
                    return Ok(Sexp::List { items, dot });
                }
                // A lone `.` between datums makes the list improper.
                Some(b'.')
                    if self
                        .src
                        .get(self.pos + 1)
                        .is_none_or(|c| c.is_ascii_whitespace()) =>
                {
                    self.pos += 1;
                    dot = Some(Box::new(self.read()?));
                }
                _ => items.push(self.read()?),
            }
        }
    }

    fn read_string(&mut self) -> Result<Sexp, String> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let c = *self
                .src
                .get(self.pos)
                .ok_or_else(|| "unterminated string".to_string())?;
            self.pos += 1;
            match c {
                b'"' => return Ok(Sexp::Str(out)),
                b'\\' => {
                    let e = *self
                        .src
                        .get(self.pos)
                        .ok_or_else(|| "unterminated escape".to_string())?;
                    self.pos += 1;
                    out.push(match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        other => other as char,
                    });
                }
                // Strings are UTF-8; copy the bytes of a multi-byte character
                // through unchanged by finding the character boundary.
                _ if c < 0x80 => out.push(c as char),
                _ => {
                    let start = self.pos - 1;
                    while self.pos < self.src.len() && (self.src[self.pos] & 0xC0) == 0x80 {
                        self.pos += 1;
                    }
                    out.push_str(&String::from_utf8_lossy(&self.src[start..self.pos]));
                }
            }
        }
    }

    fn read_atom(&mut self) -> Result<Sexp, String> {
        let start = self.pos;
        while self.pos < self.src.len() {
            let c = self.src[self.pos];
            if c.is_ascii_whitespace() || matches!(c, b'(' | b')' | b'[' | b']' | b'"' | b';') {
                break;
            }
            self.pos += 1;
        }
        if start == self.pos {
            return Err(format!("unreadable character at byte {start}"));
        }
        let text = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
        match text.parse::<i64>() {
            Ok(i) => Ok(Sexp::Int(i)),
            Err(_) => Ok(Sexp::Sym(text)),
        }
    }
}

/// Read one Emacs Lisp datum from `src`.
pub fn read_sexp(src: &str) -> Result<Sexp, String> {
    Reader::new(src).read()
}

/// Parse an ELPA `archive-contents` file.
///
/// The real wire format, verified against `https://elpa.gnu.org/packages/archive-contents`:
///
/// ```text
/// (1
///  (a68-mode . [(1 3) ((emacs (24 3))) "Major mode for editing Algol 68 code" tar
///                ((:url . "https://git.sr.ht/~jemarch/a68-mode")
///                 (:keywords "languages")
///                 (:maintainer "Jose E. Marchesi" . "jemarch@gnu.org"))])
///  …)
/// ```
///
/// The leading `1` is the archive format version. Each following element is a
/// dotted pair of the package's symbol and a five-slot vector:
/// version, dependencies, summary, kind, extra properties.
pub fn parse_archive_contents(src: &str, archive: &str) -> Result<Vec<PackageDesc>, String> {
    let top = read_sexp(src)?;
    let mut out = Vec::new();
    // Skip the leading format-version integer; every other element is an entry.
    for entry in top.items().iter().skip(1) {
        let Sexp::List {
            items,
            dot: Some(vec),
        } = entry
        else {
            continue;
        };
        let (Some(name), Sexp::Vector(slots)) = (items.first().and_then(Sexp::as_sym), &**vec)
        else {
            continue;
        };
        if slots.len() < 4 {
            continue;
        }
        let deps = slots[1]
            .items()
            .iter()
            .filter_map(|d| {
                let parts = d.items();
                let dname = parts.first().and_then(Sexp::as_sym)?;
                let dver = parts.get(1).map(Sexp::as_version).unwrap_or_default();
                Some((dname.to_string(), dver))
            })
            .collect();
        let kind = match slots[3].as_sym() {
            Some("single") => PackageKind::Single,
            _ => PackageKind::Tar,
        };
        let mut url = None;
        let mut keywords = Vec::new();
        let mut maintainer = None;
        // The extras alist: `(:url . "…")`, `(:keywords "a" "b")`,
        // `(:maintainer "Name" . "mail")`.
        for prop in slots.get(4).map(Sexp::items).unwrap_or(&[]) {
            let Sexp::List { items, dot } = prop else {
                continue;
            };
            match items.first().and_then(Sexp::as_sym) {
                Some(":url") => {
                    url = dot
                        .as_deref()
                        .and_then(Sexp::as_str)
                        .or_else(|| items.get(1).and_then(Sexp::as_str))
                        .map(str::to_string);
                }
                Some(":keywords") => {
                    keywords = items[1..]
                        .iter()
                        .filter_map(Sexp::as_str)
                        .map(str::to_string)
                        .collect();
                }
                Some(":maintainer") => {
                    maintainer = items.get(1).and_then(Sexp::as_str).map(str::to_string);
                }
                _ => {}
            }
        }
        out.push(PackageDesc {
            name: name.to_string(),
            version: slots[0].as_version(),
            deps,
            summary: slots[2].as_str().unwrap_or_default().to_string(),
            kind,
            url,
            keywords,
            maintainer,
            archive: archive.to_string(),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Installed packages.
// ---------------------------------------------------------------------------

/// A package directory found under the elpa dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub name: String,
    pub version: Vec<u64>,
    pub dir: PathBuf,
}

/// Split an install directory name into `(name, version)`. The version is the
/// part after the *last* hyphen that parses as a version, so `a68-mode-1.3`
/// reads as `("a68-mode", [1, 3])`.
pub fn split_dir_name(dir: &str) -> Option<(String, Vec<u64>)> {
    let (name, ver) = dir.rsplit_once('-')?;
    let version = parse_version(ver);
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some((name.to_string(), version))
}

/// Every package installed under `elpa_dir`, newest version first within a name.
pub fn installed_packages(elpa_dir: &Path) -> Vec<Installed> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(elpa_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let file_name = entry.file_name();
        let Some((name, version)) = file_name.to_str().and_then(split_dir_name) else {
            continue;
        };
        out.push(Installed {
            name,
            version,
            dir: entry.path(),
        });
    }
    out.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(version_cmp(&b.version, &a.version))
    });
    out
}

// ---------------------------------------------------------------------------
// The package menu model (Emacs `package-menu-mode`).
// ---------------------------------------------------------------------------

/// A package's state in the menu, as Emacs labels the `Status` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Installed, and the archive has nothing newer.
    Installed,
    /// Installed, but the archive offers a newer version.
    Obsolete,
    /// In an archive, not installed.
    Available,
    /// Installed but not present in any archive (installed from a file or VC).
    External,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Installed => "installed",
            Status::Obsolete => "obsolete",
            Status::Available => "available",
            Status::External => "external",
        }
    }
}

/// What `package-menu-execute` will do to a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Install,
    Delete,
}

impl Mark {
    pub fn char(self) -> char {
        match self {
            Mark::Install => 'I',
            Mark::Delete => 'D',
        }
    }
}

/// One row of the package menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub desc: PackageDesc,
    pub status: Status,
    /// The version installed on disk, if any (may differ from `desc.version`).
    pub installed: Option<Vec<u64>>,
    pub mark: Option<Mark>,
    /// Hidden by `package-menu-hide-package`; shown again by `toggle-hiding`.
    pub hidden: bool,
}

/// A `/`-filter, as the Emacs package menu's filter commands express it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    Name(String),
    Description(String),
    NameOrDescription(String),
    Keyword(String),
    Status(String),
    Archive(String),
    Version(String),
    Marked,
    Upgradable,
}

impl Filter {
    /// How the filter names itself in the menu header.
    pub fn label(&self) -> String {
        match self {
            Filter::Name(s) => format!("name:{s}"),
            Filter::Description(s) => format!("description:{s}"),
            Filter::NameOrDescription(s) => format!("name-or-description:{s}"),
            Filter::Keyword(s) => format!("keyword:{s}"),
            Filter::Status(s) => format!("status:{s}"),
            Filter::Archive(s) => format!("archive:{s}"),
            Filter::Version(s) => format!("version:{s}"),
            Filter::Marked => "marked".into(),
            Filter::Upgradable => "upgradable".into(),
        }
    }

    fn matches(&self, row: &Row) -> bool {
        let ci = |hay: &str, needle: &str| hay.to_lowercase().contains(&needle.to_lowercase());
        match self {
            Filter::Name(s) => ci(&row.desc.name, s),
            Filter::Description(s) => ci(&row.desc.summary, s),
            Filter::NameOrDescription(s) => ci(&row.desc.name, s) || ci(&row.desc.summary, s),
            Filter::Keyword(s) => row.desc.keywords.iter().any(|k| k.eq_ignore_ascii_case(s)),
            Filter::Status(s) => row.status.label().eq_ignore_ascii_case(s),
            Filter::Archive(s) => row.desc.archive.eq_ignore_ascii_case(s),
            Filter::Version(s) => row.desc.version_string().starts_with(s.trim()),
            Filter::Marked => row.mark.is_some(),
            Filter::Upgradable => row.status == Status::Obsolete,
        }
    }
}

/// The model behind the package menu Component and the `package-menu-*` commands.
#[derive(Debug, Default, Clone)]
pub struct PackageMenu {
    rows: Vec<Row>,
    /// Active filters, most recently added last. Emacs composes them (each `/`
    /// filter narrows what the previous ones left).
    pub filters: Vec<Filter>,
    /// `package-menu-toggle-hiding`: while false, rows hidden by
    /// `package-menu-hide-package` are listed anyway (greyed out).
    pub hiding: bool,
    pub selected: usize,
}

impl PackageMenu {
    /// Build the menu from the archive listing and what is on disk.
    pub fn new(available: Vec<PackageDesc>, installed: &[Installed]) -> Self {
        // Newest installed version per package name.
        let mut best: HashMap<&str, &Vec<u64>> = HashMap::new();
        for inst in installed {
            best.entry(inst.name.as_str())
                .and_modify(|v| {
                    if version_cmp(&inst.version, v) == Ordering::Greater {
                        *v = &inst.version;
                    }
                })
                .or_insert(&inst.version);
        }
        let mut rows: Vec<Row> = available
            .into_iter()
            .map(|desc| {
                let inst = best.get(desc.name.as_str()).map(|v| (*v).clone());
                let status = match &inst {
                    None => Status::Available,
                    Some(v) if version_cmp(&desc.version, v) == Ordering::Greater => {
                        Status::Obsolete
                    }
                    Some(_) => Status::Installed,
                };
                Row {
                    status,
                    installed: inst,
                    desc,
                    mark: None,
                    hidden: false,
                }
            })
            .collect();
        // Installed packages no archive lists (from a file, or from VC) still get
        // a row, so they can be described and deleted.
        let listed: Vec<String> = rows.iter().map(|r| r.desc.name.clone()).collect();
        for inst in installed {
            if listed.contains(&inst.name) {
                continue;
            }
            rows.push(Row {
                desc: PackageDesc {
                    name: inst.name.clone(),
                    version: inst.version.clone(),
                    deps: Vec::new(),
                    summary: String::new(),
                    kind: PackageKind::Tar,
                    url: None,
                    keywords: Vec::new(),
                    maintainer: None,
                    archive: String::new(),
                },
                status: Status::External,
                installed: Some(inst.version.clone()),
                mark: None,
                hidden: false,
            });
        }
        rows.sort_by(|a, b| a.desc.name.cmp(&b.desc.name));
        Self {
            rows,
            filters: Vec::new(),
            hiding: true,
            selected: 0,
        }
    }

    /// Every row, filtered and hidden state applied — what the menu displays.
    pub fn visible(&self) -> Vec<&Row> {
        self.rows
            .iter()
            .filter(|r| !(self.hiding && r.hidden))
            .filter(|r| self.filters.iter().all(|f| f.matches(r)))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.visible().is_empty()
    }

    /// Index into `rows` of the currently selected visible row.
    fn selected_index(&self) -> Option<usize> {
        let name = self.visible().get(self.selected)?.desc.name.clone();
        self.rows.iter().position(|r| r.desc.name == name)
    }

    pub fn current(&self) -> Option<&Row> {
        let visible = self.visible();
        visible.get(self.selected).copied()
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.visible().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, len as isize - 1) as usize;
    }

    pub fn goto_first(&mut self) {
        self.selected = 0;
    }

    pub fn goto_last(&mut self) {
        self.selected = self.visible().len().saturating_sub(1);
    }

    /// Keep the selection inside the visible rows after a filter changes them.
    fn clamp(&mut self) {
        let len = self.visible().len();
        if self.selected >= len {
            self.selected = len.saturating_sub(1);
        }
    }

    /// `package-menu-mark-install` / `-mark-delete`: mark the current row and
    /// step down, as Emacs's marking commands do.
    pub fn mark_current(&mut self, mark: Mark) -> Option<String> {
        let idx = self.selected_index()?;
        self.rows[idx].mark = Some(mark);
        let name = self.rows[idx].desc.name.clone();
        self.move_selection(1);
        Some(name)
    }

    /// `package-menu-mark-unmark` (`u` / `DEL`).
    pub fn unmark_current(&mut self, step: isize) -> Option<String> {
        let idx = self.selected_index()?;
        self.rows[idx].mark = None;
        let name = self.rows[idx].desc.name.clone();
        self.move_selection(step);
        Some(name)
    }

    /// `package-menu-mark-upgrades` (`U`): mark every obsolete row for install
    /// (installing a newer version *is* the upgrade). Returns how many.
    pub fn mark_upgrades(&mut self) -> usize {
        let mut n = 0;
        for row in &mut self.rows {
            if row.status == Status::Obsolete {
                row.mark = Some(Mark::Install);
                n += 1;
            }
        }
        n
    }

    /// `package-menu-mark-obsolete-for-deletion` (`~`): mark for deletion every
    /// installed version that a newer installed version supersedes.
    pub fn mark_obsolete_for_deletion(&mut self, installed: &[Installed]) -> usize {
        let mut newest: HashMap<&str, &Vec<u64>> = HashMap::new();
        for inst in installed {
            newest
                .entry(inst.name.as_str())
                .and_modify(|v| {
                    if version_cmp(&inst.version, v) == Ordering::Greater {
                        *v = &inst.version;
                    }
                })
                .or_insert(&inst.version);
        }
        let mut n = 0;
        for row in &mut self.rows {
            let Some(have) = &row.installed else { continue };
            let Some(best) = newest.get(row.desc.name.as_str()) else {
                continue;
            };
            if version_cmp(have, best) == Ordering::Less {
                row.mark = Some(Mark::Delete);
                n += 1;
            }
        }
        n
    }

    /// Every marked row's `(name, mark)` — what `package-menu-execute` runs.
    pub fn marked(&self) -> Vec<(String, Mark, PackageDesc)> {
        self.rows
            .iter()
            .filter_map(|r| r.mark.map(|m| (r.desc.name.clone(), m, r.desc.clone())))
            .collect()
    }

    pub fn clear_marks(&mut self) {
        for row in &mut self.rows {
            row.mark = None;
        }
    }

    /// `package-menu-hide-package` (`H`).
    pub fn hide_current(&mut self) -> Option<String> {
        let idx = self.selected_index()?;
        self.rows[idx].hidden = true;
        let name = self.rows[idx].desc.name.clone();
        self.clamp();
        Some(name)
    }

    /// `package-menu-toggle-hiding` (`(`).
    pub fn toggle_hiding(&mut self) -> bool {
        self.hiding = !self.hiding;
        self.clamp();
        self.hiding
    }

    pub fn add_filter(&mut self, filter: Filter) {
        self.filters.push(filter);
        self.selected = 0;
    }

    /// `package-menu-filter-clear` (`/ /`).
    pub fn clear_filters(&mut self) {
        self.filters.clear();
        self.selected = 0;
    }

    /// Every keyword any listed package declares, sorted — the completion table
    /// `package-menu-filter-by-keyword` and `finder-by-keyword` read.
    pub fn keywords(&self) -> Vec<String> {
        let mut all: Vec<String> = self
            .rows
            .iter()
            .flat_map(|r| r.desc.keywords.iter().cloned())
            .collect();
        all.sort();
        all.dedup();
        all
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The archive entry for a package name, if any archive lists it.
    pub fn find(&self, name: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.desc.name == name)
    }
}

/// Resolve `desc`'s dependency closure against the archive listing, deepest
/// dependency first, so installing in order never leaves a package loaded before
/// something it requires. `emacs` (a host-version requirement, not a package) and
/// anything already installed at a new enough version are dropped.
pub fn install_order(
    desc: &PackageDesc,
    available: &[PackageDesc],
    installed: &[Installed],
) -> Vec<PackageDesc> {
    fn have(name: &str, want: &[u64], installed: &[Installed]) -> bool {
        installed
            .iter()
            .any(|i| i.name == name && version_cmp(&i.version, want) != Ordering::Less)
    }
    fn walk(
        desc: &PackageDesc,
        available: &[PackageDesc],
        installed: &[Installed],
        seen: &mut Vec<String>,
        out: &mut Vec<PackageDesc>,
    ) {
        if seen.contains(&desc.name) {
            return;
        }
        seen.push(desc.name.clone());
        for (dep, want) in &desc.deps {
            if dep == "emacs" || have(dep, want, installed) {
                continue;
            }
            if let Some(d) = available.iter().find(|a| &a.name == dep) {
                walk(d, available, installed, seen, out);
            }
        }
        out.push(desc.clone());
    }
    let mut out = Vec::new();
    walk(desc, available, installed, &mut Vec::new(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact bytes GNU ELPA serves (the head of the real
    /// `https://elpa.gnu.org/packages/archive-contents`), so the parser is pinned
    /// to the wire format rather than to a guess about it.
    const GNU: &str = r#"(1
 (a68-mode
  . [(1 3) ((emacs (24 3))) "Major mode for editing Algol 68 code" tar
     ((:url . "https://git.sr.ht/~jemarch/a68-mode")
      (:keywords "languages")
      (:maintainer "Jose E. Marchesi" . "jemarch@gnu.org")
      (:authors ("Omar Polo" . "op@omarpolo.com"))
      (:commit . "a35b2fec07dcf9c3550cebc7f75e13f240088db2"))])
 (ace-window
  . [(0 10 0) ((avy (0 5 0))) "Quickly switch windows." tar
     ((:url . "https://github.com/abo-abo/ace-window")
      (:keywords "window" "location"))])
 (tiny . [(1 1) nil "Quickly generate linear ranges in Emacs" single
     ((:url . "https://github.com/abo-abo/tiny"))]))"#;

    #[test]
    fn parses_the_real_gnu_elpa_wire_format() {
        let pkgs = parse_archive_contents(GNU, "gnu").unwrap();
        assert_eq!(pkgs.len(), 3);

        let a68 = &pkgs[0];
        assert_eq!(a68.name, "a68-mode");
        assert_eq!(a68.version, vec![1, 3]);
        assert_eq!(a68.version_string(), "1.3");
        assert_eq!(a68.summary, "Major mode for editing Algol 68 code");
        assert_eq!(a68.kind, PackageKind::Tar);
        assert_eq!(a68.file_name(), "a68-mode-1.3.tar");
        assert_eq!(a68.deps, vec![("emacs".to_string(), vec![24, 3])]);
        assert_eq!(
            a68.url.as_deref(),
            Some("https://git.sr.ht/~jemarch/a68-mode")
        );
        assert_eq!(a68.keywords, vec!["languages"]);
        assert_eq!(a68.maintainer.as_deref(), Some("Jose E. Marchesi"));
        assert_eq!(a68.archive, "gnu");

        // Two keywords, and a dependency on a real package (not just `emacs`).
        assert_eq!(pkgs[1].keywords, vec!["window", "location"]);
        assert_eq!(pkgs[1].deps, vec![("avy".to_string(), vec![0, 5, 0])]);

        // `nil` dependencies and the `single` kind (one .el file, no tar).
        assert!(pkgs[2].deps.is_empty());
        assert_eq!(pkgs[2].kind, PackageKind::Single);
        assert_eq!(pkgs[2].file_name(), "tiny-1.1.el");
    }

    /// MELPA's entries are on one line and carry different extras; the same
    /// reader must handle them.
    #[test]
    fn parses_the_melpa_wire_format() {
        let src = r#"(1
 (0blayout . [(20190703 527) nil "Layout grouping with ease" tar ((:url . "https://github.com/etu/0blayout") (:commit . "fd9a8f") (:keywords "convenience" "window-management"))]))"#;
        let pkgs = parse_archive_contents(src, "melpa").unwrap();
        assert_eq!(pkgs[0].name, "0blayout");
        assert_eq!(pkgs[0].version_string(), "20190703.527");
        assert_eq!(pkgs[0].keywords, vec!["convenience", "window-management"]);
        assert_eq!(pkgs[0].archive, "melpa");
    }

    #[test]
    fn version_comparison_is_element_wise() {
        assert_eq!(version_cmp(&[1, 2], &[1, 2, 1]), Ordering::Less);
        assert_eq!(version_cmp(&[1, 10], &[1, 9]), Ordering::Greater);
        assert_eq!(version_cmp(&[1, 2, 0], &[1, 2]), Ordering::Equal);
        assert_eq!(parse_version("20230823.2214"), vec![20230823, 2214]);
        // A non-numeric tail stops the parse rather than poisoning the version.
        assert_eq!(parse_version("1.2-git"), vec![1]);
    }

    #[test]
    fn install_dir_names_round_trip_through_hyphenated_package_names() {
        assert_eq!(
            split_dir_name("a68-mode-1.3"),
            Some(("a68-mode".to_string(), vec![1, 3]))
        );
        assert_eq!(
            split_dir_name("magit-20230101.1"),
            Some(("magit".to_string(), vec![20230101, 1]))
        );
        // Not a package dir: no version part.
        assert_eq!(split_dir_name("archives"), None);
    }

    fn desc(name: &str, ver: &[u64], deps: &[(&str, &[u64])]) -> PackageDesc {
        PackageDesc {
            name: name.into(),
            version: ver.to_vec(),
            deps: deps
                .iter()
                .map(|(n, v)| ((*n).to_string(), v.to_vec()))
                .collect(),
            summary: String::new(),
            kind: PackageKind::Tar,
            url: None,
            keywords: Vec::new(),
            maintainer: None,
            archive: "gnu".into(),
        }
    }

    /// Dependencies install before their dependants, `emacs` is not a package,
    /// and an already-installed dependency is not reinstalled.
    #[test]
    fn install_order_is_depth_first_and_skips_what_is_present() {
        let avail = vec![
            desc("a", &[1], &[("b", &[1]), ("emacs", &[27, 1])]),
            desc("b", &[2], &[("c", &[1])]),
            desc("c", &[1], &[]),
            desc("d", &[1], &[]),
        ];
        let order = install_order(&avail[0], &avail, &[]);
        let names: Vec<&str> = order.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["c", "b", "a"]);

        let have = vec![Installed {
            name: "c".into(),
            version: vec![1],
            dir: PathBuf::from("/tmp/c-1"),
        }];
        let order = install_order(&avail[0], &avail, &have);
        let names: Vec<&str> = order.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["b", "a"]);
    }

    /// A dependency cycle must terminate rather than recurse forever.
    #[test]
    fn install_order_survives_a_dependency_cycle() {
        let avail = vec![
            desc("a", &[1], &[("b", &[1])]),
            desc("b", &[1], &[("a", &[1])]),
        ];
        let order = install_order(&avail[0], &avail, &[]);
        assert_eq!(order.len(), 2);
    }

    fn menu() -> PackageMenu {
        let avail = vec![
            desc("alpha", &[2], &[]),
            desc("beta", &[1], &[]),
            desc("gamma", &[1], &[]),
        ];
        let installed = vec![
            Installed {
                name: "alpha".into(),
                version: vec![1],
                dir: PathBuf::from("/e/alpha-1"),
            },
            Installed {
                name: "beta".into(),
                version: vec![1],
                dir: PathBuf::from("/e/beta-1"),
            },
            Installed {
                name: "local".into(),
                version: vec![1],
                dir: PathBuf::from("/e/local-1"),
            },
        ];
        PackageMenu::new(avail, &installed)
    }

    /// The Status column: newer in the archive = obsolete (upgradable), same =
    /// installed, absent from disk = available, absent from the archive = external.
    #[test]
    fn statuses_reflect_disk_versus_archive() {
        let m = menu();
        assert_eq!(m.find("alpha").unwrap().status, Status::Obsolete);
        assert_eq!(m.find("beta").unwrap().status, Status::Installed);
        assert_eq!(m.find("gamma").unwrap().status, Status::Available);
        assert_eq!(m.find("local").unwrap().status, Status::External);
    }

    #[test]
    fn mark_upgrades_marks_every_obsolete_package() {
        let mut m = menu();
        assert_eq!(m.mark_upgrades(), 1);
        let marked = m.marked();
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].0, "alpha");
        assert_eq!(marked[0].1, Mark::Install);
        m.clear_marks();
        assert!(m.marked().is_empty());
    }

    /// Filters compose (Emacs stacks them) and `filter-clear` drops them all.
    #[test]
    fn filters_compose_and_clear() {
        let mut m = menu();
        assert_eq!(m.visible().len(), 4);
        m.add_filter(Filter::Status("installed".into()));
        assert_eq!(m.visible().len(), 1);
        assert_eq!(m.visible()[0].desc.name, "beta");
        m.add_filter(Filter::Name("alpha".into()));
        assert!(m.visible().is_empty());
        m.clear_filters();
        assert_eq!(m.visible().len(), 4);

        m.add_filter(Filter::Upgradable);
        assert_eq!(m.visible().len(), 1);
        assert_eq!(m.visible()[0].desc.name, "alpha");
    }

    /// Hiding removes a row from the listing; toggle-hiding brings it back.
    #[test]
    fn hide_and_toggle_hiding() {
        let mut m = menu();
        m.selected = 0; // alpha
        assert_eq!(m.hide_current().as_deref(), Some("alpha"));
        assert_eq!(m.visible().len(), 3);
        assert!(!m.toggle_hiding());
        assert_eq!(m.visible().len(), 4);
    }

    /// Marking steps down the list, and the mark lands on the row it was made on
    /// even when a filter is narrowing the view.
    #[test]
    fn marking_through_a_filter_marks_the_right_row() {
        let mut m = menu();
        m.add_filter(Filter::Name("a".into())); // alpha, beta, gamma
        m.selected = 1;
        assert_eq!(m.mark_current(Mark::Delete).as_deref(), Some("beta"));
        assert_eq!(m.selected, 2);
        assert_eq!(m.find("beta").unwrap().mark, Some(Mark::Delete));
        assert_eq!(m.find("alpha").unwrap().mark, None);
    }
}
