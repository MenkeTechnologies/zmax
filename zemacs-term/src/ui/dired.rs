//! Dired — a directory-editor mode, the zemacs port of GNU Emacs Dired.
//!
//! A full-screen [`Component`] listing one directory. Each row is a file or
//! subdirectory with a left-hand mark column (`*` marked, `D` flagged for
//! deletion), a type/size column and the name. Marks are keyed by file **name**
//! so they survive re-sorting and refresh. All pure logic (sorting, human sizes,
//! name transforms, the mark glyph) lives in the filesystem-free, unit-tested
//! [`zemacs_core::dired`]; this module does the directory I/O, rendering and key
//! handling.
//!
//! Keys (parsed into a `dired` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs Dired counterpart in the port tracker):
//!   j/k/n/p/arrows, g/G/Home/End — move point
//!   Enter/f — visit file, or enter subdirectory in place
//!   ^ / - — go up to the parent directory
//!   m — mark; u — unmark (and advance); DEL — unmark previous; U — unmark all;
//!   t — toggle all marks
//!   d — flag for deletion (and advance); ~ flag backups; # flag auto-saves;
//!   & flag garbage (build/tex droppings); x — delete the flagged files;
//!   D — delete the marked files (or the file at point) immediately
//!   w — copy the marked names (or the name at point) to the clipboard
//!   s — cycle sort order (name/time/size/ext); r — reverse; `.` — toggle hidden
//!   ( — toggle hide-details (name-only rows)
//!   M-} / M-{ — next / previous marked file
//!   R / l — refresh (redisplay); q/Esc — quit
//!
//! Ported Emacs Dired commands added in this slice (each bound to a free key,
//! since the single-key match can't express Emacs multi-key `* /` chords):
//!   M-d — mark all subdirectories        (dired-mark-directories)
//!   M-x — mark executables               (dired-mark-executables)
//!   M-s — mark symlinks                  (dired-mark-symlinks)
//!   N   — echo count/size of marked      (dired-number-of-marked-files)
//!   A   — upcase name(s) on disk         (dired-upcase)
//!   Z   — downcase name(s) on disk       (dired-downcase)
//!   K   — kill (hide) lines from listing (dired-do-kill-lines)
//!   > / < — next / previous dirline      (dired-next/prev-dirline)
//!   v   — view file at point read-only   (dired-view-file)
//!   o   — open file in other window      (dired-find-file-other-window)
//!   C-o — display file in a split        (dired-display-file)
//!   T   — touch (mtime = now)            (dired-do-touch)
//! Commands that read a line in the in-mode minibuffer (Enter runs, Esc aborts):
//!   +   — create directory               (dired-create-directory)
//!   E   — create empty file              (dired-create-empty-file)
//!   C   — copy target(s)                 (dired-do-copy)
//!   %   — rename/move target(s)          (dired-do-rename)
//!   S   — symlink to target(s)           (dired-do-symlink)
//!   H   — hardlink to target(s)          (dired-do-hardlink)
//!   M   — chmod target(s) (octal)        (dired-do-chmod)
//!   *   — mark by regexp                 (dired-mark-files-regexp)
//!   /   — flag for deletion by regexp    (dired-flag-files-regexp)
//!   J   — goto file by name              (dired-goto-file)
//!
//! Deferred to a later slice: chown/chgrp (need uid/gid resolution), wdired
//! (editable listing), subdirectory insertion.

use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use regex::Regex;
use tui::buffer::Buffer as Surface;
use zemacs_core::dired::{
    destination_path, human_size, is_executable_mode, is_valid_filename, mark_char, marked_summary,
    next_dir_index, parse_octal_mode, sort_entries, transform_name, DiredEntry, NameTransform,
    SortKey,
};
use zemacs_view::{editor::Action, graphics::Rect};

use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};
use zemacs_view::input::KeyEvent;
use zemacs_view::keyboard::{KeyCode, KeyModifiers};

/// A pending in-mode minibuffer read: which fs/listing action to run when the
/// user hits Enter, carrying the target names captured when the prompt opened.
enum Pending {
    CreateDir,
    CreateFile,
    Copy(Vec<String>),
    Rename(Vec<String>),
    Symlink(Vec<String>),
    Hardlink(Vec<String>),
    Chmod(Vec<String>),
    MarkRegexp,
    FlagRegexp,
    GotoFile,
}

/// The four two-path filesystem operations Dired offers on a set of targets.
#[derive(Clone, Copy)]
enum LinkKind {
    Copy,
    Rename,
    Symlink,
    Hardlink,
}

impl LinkKind {
    /// Present-tense verb for an error message.
    fn verb(self) -> &'static str {
        match self {
            LinkKind::Copy => "copy",
            LinkKind::Rename => "rename",
            LinkKind::Symlink => "symlink",
            LinkKind::Hardlink => "hardlink",
        }
    }
    /// Past-tense verb for the success status line.
    fn past(self) -> &'static str {
        match self {
            LinkKind::Copy => "copied",
            LinkKind::Rename => "renamed",
            LinkKind::Symlink => "symlinked",
            LinkKind::Hardlink => "hardlinked",
        }
    }
}

/// The in-mode minibuffer state (Emacs Dired reads copy/rename/regexp arguments
/// in the echo area). Kept inside the component so the action can mutate the
/// listing and refresh it in place — a pushed `Prompt` layer could not reach
/// back into this component's marks.
struct Input {
    prompt: &'static str,
    buffer: String,
    action: Pending,
}

/// Set a file's access + modification times to now (POSIX `utimes(path, NULL)`),
/// the `touch` behind Emacs `dired-do-touch`. Uses the already-present `libc`.
fn set_mtime_now(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    match std::ffi::CString::new(path.as_os_str().as_bytes()) {
        Ok(c) => unsafe { libc::utimes(c.as_ptr(), std::ptr::null()) == 0 },
        Err(_) => false,
    }
}

/// The interactive Dired overlay for a single directory.
pub struct Dired {
    dir: PathBuf,
    entries: Vec<DiredEntry>,
    /// Marked / deletion-flagged entries, keyed by file name (survive re-sort).
    marked: HashSet<String>,
    flagged: HashSet<String>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    sort: SortKey,
    reverse: bool,
    show_hidden: bool,
    /// When false (Emacs `dired-hide-details-mode`), rows show only the mark and
    /// file name, hiding the type/size columns.
    show_details: bool,
    error: Option<String>,
    /// Active in-mode minibuffer read, if any (see [`Input`]).
    input: Option<Input>,
}

impl Dired {
    /// Open Dired on `dir`, reading its contents. Errors if the directory can't
    /// be read.
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        let mut d = Dired {
            dir,
            entries: Vec::new(),
            marked: HashSet::new(),
            flagged: HashSet::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            sort: SortKey::Name,
            reverse: false,
            show_hidden: false,
            show_details: true,
            error: None,
            input: None,
        };
        d.read_dir()?;
        Ok(d)
    }

    /// Read `self.dir` into `self.entries` (respecting `show_hidden`) and sort.
    /// Marks/flags naming files no longer present are dropped.
    fn read_dir(&mut self) -> std::io::Result<()> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if !self.show_hidden && name.starts_with('.') {
                continue;
            }
            let ft = entry.file_type().ok();
            let meta = entry.metadata().ok();
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            entries.push(DiredEntry {
                name,
                is_dir: ft.map(|f| f.is_dir()).unwrap_or(false),
                is_symlink: ft.map(|f| f.is_symlink()).unwrap_or(false),
                size: meta.map(|m| m.len()).unwrap_or(0),
                mtime,
            });
        }
        sort_entries(&mut entries, self.sort, self.reverse);
        let present: HashSet<&String> = entries.iter().map(|e| &e.name).collect();
        self.marked.retain(|n| present.contains(n));
        self.flagged.retain(|n| present.contains(n));
        self.entries = entries;
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        self.error = None;
        Ok(())
    }

    fn resort(&mut self) {
        sort_entries(&mut self.entries, self.sort, self.reverse);
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    fn current_name(&self) -> Option<String> {
        self.entries.get(self.selected).map(|e| e.name.clone())
    }

    /// Names to act on: the marked set if non-empty, else the entry at point.
    fn targets(&self) -> Vec<String> {
        if !self.marked.is_empty() {
            self.entries
                .iter()
                .filter(|e| self.marked.contains(&e.name))
                .map(|e| e.name.clone())
                .collect()
        } else {
            self.current_name().into_iter().collect()
        }
    }

    fn toggle_all_marks(&mut self) {
        let mut next = HashSet::new();
        for e in &self.entries {
            if !self.marked.contains(&e.name) {
                next.insert(e.name.clone());
            }
        }
        self.marked = next;
    }

    /// Move point to the next (`dir = 1`) or previous (`dir = -1`) marked file,
    /// wrapping around — Emacs `dired-next-marked-file` / `dired-prev-marked-file`.
    fn next_marked(&mut self, dir: isize) {
        let n = self.entries.len();
        if n == 0 || self.marked.is_empty() {
            return;
        }
        for step in 1..=n as isize {
            let idx = (self.selected as isize + dir * step).rem_euclid(n as isize) as usize;
            if self.marked.contains(&self.entries[idx].name) {
                self.selected = idx;
                return;
            }
        }
    }

    /// Flag every entry whose name satisfies `pred` for deletion, returning the
    /// number newly flagged — the shared engine behind the Emacs `~`/`#`/`&`
    /// dired flag-by-pattern commands.
    fn flag_matching(&mut self, pred: impl Fn(&str) -> bool) -> usize {
        let mut n = 0;
        for e in &self.entries {
            if pred(&e.name) && self.flagged.insert(e.name.clone()) {
                n += 1;
            }
        }
        n
    }

    /// Delete a set of names from disk (files or directory trees). Returns the
    /// count deleted; records the first error.
    fn delete_names(&mut self, names: &[String]) -> usize {
        let mut n = 0;
        for name in names {
            let path = self.dir.join(name);
            let res = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match res {
                Ok(()) => {
                    self.marked.remove(name);
                    self.flagged.remove(name);
                    n += 1;
                }
                Err(e) => {
                    self.error = Some(format!("delete {name}: {e}"));
                    break;
                }
            }
        }
        n
    }

    /// Mark every entry satisfying `pred`, returning the number newly marked —
    /// the engine behind `dired-mark-directories` / `dired-mark-symlinks`.
    fn mark_where(&mut self, pred: impl Fn(&DiredEntry) -> bool) -> usize {
        let mut n = 0;
        for e in &self.entries {
            if pred(e) && self.marked.insert(e.name.clone()) {
                n += 1;
            }
        }
        n
    }

    /// `dired-mark-executables`: mark every regular file with an execute bit set.
    fn mark_executables(&mut self) -> usize {
        let names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.name.clone())
            .collect();
        let mut n = 0;
        for name in names {
            if let Ok(meta) = std::fs::metadata(self.dir.join(&name)) {
                if is_executable_mode(meta.permissions().mode()) && self.marked.insert(name) {
                    n += 1;
                }
            }
        }
        n
    }

    /// `dired-upcase` / `dired-downcase`: rename the target(s) on disk applying
    /// `t`, then re-read the directory. Returns the number renamed.
    fn rename_transform(&mut self, t: NameTransform) -> usize {
        let targets = self.targets();
        let mut n = 0;
        for name in &targets {
            let new = transform_name(name, t);
            if new == *name {
                continue;
            }
            let from = self.dir.join(name);
            let to = self.dir.join(&new);
            match std::fs::rename(&from, &to) {
                Ok(()) => {
                    self.marked.remove(name);
                    self.flagged.remove(name);
                    n += 1;
                }
                Err(e) => {
                    self.error = Some(format!("rename {name}: {e}"));
                    break;
                }
            }
        }
        let _ = self.read_dir();
        n
    }

    /// `dired-do-kill-lines`: drop the target entries from the *listing* only
    /// (they stay on disk). Returns how many rows were removed.
    fn kill_lines(&mut self) -> usize {
        let targets: HashSet<String> = self.targets().into_iter().collect();
        if targets.is_empty() {
            return 0;
        }
        let before = self.entries.len();
        self.entries.retain(|e| !targets.contains(&e.name));
        for name in &targets {
            self.marked.remove(name);
            self.flagged.remove(name);
        }
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        before - self.entries.len()
    }

    /// `dired-do-touch`: set the target(s)' mtime to now, then refresh.
    fn touch_targets(&mut self) -> usize {
        let targets = self.targets();
        let mut n = 0;
        for name in &targets {
            if set_mtime_now(&self.dir.join(name)) {
                n += 1;
            } else {
                self.error = Some(format!("touch {name}: failed"));
                break;
            }
        }
        let _ = self.read_dir();
        n
    }

    /// Build a callback that opens the file at point in another editor window
    /// via `action` (a split), popping this overlay. Directories are entered in
    /// place instead. `read_only` marks the opened document read-only.
    fn open_file(&mut self, action: Action, read_only: bool) -> Option<Callback> {
        if self.entries.get(self.selected)?.is_dir {
            self.visit();
            return None;
        }
        let name = self.current_name()?;
        let path = self.dir.join(&name);
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                match cx.editor.open(&path, action) {
                    Ok(id) => {
                        if read_only {
                            if let Some(doc) = cx.editor.document_mut(id) {
                                doc.readonly = true;
                            }
                            cx.editor
                                .set_status(format!("dired: viewing {} (read-only)", path.display()));
                        }
                    }
                    Err(err) => cx
                        .editor
                        .set_error(format!("failed to open {}: {err}", path.display())),
                }
            },
        ))
    }

    /// Open the in-mode minibuffer for `action`, showing `prompt`.
    fn begin_input(&mut self, prompt: &'static str, action: Pending) {
        self.input = Some(Input {
            prompt,
            buffer: String::new(),
            action,
        });
    }

    /// Execute a completed minibuffer read (`text` is the typed line), refresh
    /// the listing if the filesystem changed, and report via `set_status`.
    fn run_pending(&mut self, action: Pending, text: &str, cx: &mut Context) {
        let text = text.trim();
        match action {
            Pending::CreateDir => {
                if text.is_empty() {
                    return;
                }
                let path = self.dir.join(text);
                match std::fs::create_dir_all(&path) {
                    Ok(()) => {
                        let _ = self.read_dir();
                        cx.editor
                            .set_status(format!("dired: created directory {text}"));
                    }
                    Err(e) => cx.editor.set_error(format!("mkdir {text}: {e}")),
                }
            }
            Pending::CreateFile => {
                if !is_valid_filename(text) {
                    cx.editor.set_error("dired: invalid file name");
                    return;
                }
                match std::fs::File::create(self.dir.join(text)) {
                    Ok(_) => {
                        let _ = self.read_dir();
                        cx.editor.set_status(format!("dired: created {text}"));
                    }
                    Err(e) => cx.editor.set_error(format!("create {text}: {e}")),
                }
            }
            Pending::Copy(targets) => {
                self.link_or_copy(&targets, text, LinkKind::Copy, cx);
            }
            Pending::Rename(targets) => {
                self.link_or_copy(&targets, text, LinkKind::Rename, cx);
            }
            Pending::Symlink(targets) => {
                self.link_or_copy(&targets, text, LinkKind::Symlink, cx);
            }
            Pending::Hardlink(targets) => {
                self.link_or_copy(&targets, text, LinkKind::Hardlink, cx);
            }
            Pending::Chmod(targets) => {
                let mode = match parse_octal_mode(text) {
                    Some(m) => m,
                    None => {
                        cx.editor.set_error("dired: invalid octal mode");
                        return;
                    }
                };
                let mut n = 0;
                for name in &targets {
                    let perm = std::fs::Permissions::from_mode(mode);
                    match std::fs::set_permissions(self.dir.join(name), perm) {
                        Ok(()) => n += 1,
                        Err(e) => {
                            cx.editor.set_error(format!("chmod {name}: {e}"));
                            break;
                        }
                    }
                }
                let _ = self.read_dir();
                cx.editor
                    .set_status(format!("dired: chmod {text} on {n} file(s)"));
            }
            Pending::MarkRegexp | Pending::FlagRegexp => {
                if text.is_empty() {
                    return;
                }
                let re = match Regex::new(text) {
                    Ok(re) => re,
                    Err(e) => {
                        cx.editor.set_error(format!("dired: bad regexp: {e}"));
                        return;
                    }
                };
                let flag = matches!(action, Pending::FlagRegexp);
                let names: Vec<String> = self
                    .entries
                    .iter()
                    .filter(|e| re.is_match(&e.name))
                    .map(|e| e.name.clone())
                    .collect();
                let mut n = 0;
                for name in names {
                    let inserted = if flag {
                        self.flagged.insert(name)
                    } else {
                        self.marked.insert(name)
                    };
                    if inserted {
                        n += 1;
                    }
                }
                let what = if flag { "flagged" } else { "marked" };
                cx.editor
                    .set_status(format!("dired: {what} {n} file(s) matching /{text}/"));
            }
            Pending::GotoFile => {
                if let Some(i) = self
                    .entries
                    .iter()
                    .position(|e| e.name == text)
                    .or_else(|| self.entries.iter().position(|e| e.name.starts_with(text)))
                {
                    self.selected = i;
                } else {
                    cx.editor.set_status(format!("dired: no file named {text}"));
                }
            }
        }
    }

    /// Shared body for copy/rename/symlink/hardlink over a set of targets to a
    /// user-typed destination, refreshing and reporting the result.
    fn link_or_copy(&mut self, targets: &[String], dest: &str, kind: LinkKind, cx: &mut Context) {
        if dest.is_empty() || targets.is_empty() {
            return;
        }
        let dest_path = self.dir.join(dest);
        let dest_is_dir = dest_path.is_dir();
        let mut n = 0;
        for name in targets {
            let src = self.dir.join(name);
            let to = destination_path(&dest_path, dest_is_dir, name);
            let res = match kind {
                LinkKind::Copy => std::fs::copy(&src, &to).map(|_| ()),
                LinkKind::Rename => std::fs::rename(&src, &to),
                LinkKind::Symlink => std::os::unix::fs::symlink(&src, &to),
                LinkKind::Hardlink => std::fs::hard_link(&src, &to),
            };
            match res {
                Ok(()) => n += 1,
                Err(e) => {
                    cx.editor.set_error(format!("{} {name}: {e}", kind.verb()));
                    break;
                }
            }
        }
        let _ = self.read_dir();
        cx.editor
            .set_status(format!("dired: {} {n} file(s)", kind.past()));
    }

    /// Visit the entry at point: enter a subdirectory in place, or open a file
    /// (popping this overlay).
    fn visit(&mut self) -> Option<Callback> {
        let e = self.entries.get(self.selected)?;
        let path = self.dir.join(&e.name);
        if e.is_dir {
            self.dir = std::fs::canonicalize(&path).unwrap_or(path);
            self.selected = 0;
            self.scroll = 0;
            self.marked.clear();
            self.flagged.clear();
            if let Err(err) = self.read_dir() {
                self.error = Some(format!("{err}"));
            }
            None
        } else {
            Some(Box::new(
                move |compositor: &mut Compositor, cx: &mut Context| {
                    compositor.pop();
                    if let Err(err) = cx.editor.open(&path, Action::Replace) {
                        cx.editor
                            .set_error(format!("failed to open {}: {err}", path.display()));
                    }
                },
            ))
        }
    }

    /// Go up to the parent directory, selecting the directory we came from.
    fn up_dir(&mut self) {
        if let Some(parent) = self.dir.parent().map(|p| p.to_path_buf()) {
            let from = self
                .dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            self.dir = parent;
            self.marked.clear();
            self.flagged.clear();
            self.selected = 0;
            self.scroll = 0;
            if let Err(err) = self.read_dir() {
                self.error = Some(format!("{err}"));
            }
            if let Some(from) = from {
                if let Some(i) = self.entries.iter().position(|e| e.name == from) {
                    self.selected = i;
                }
            }
        }
    }
}

impl Component for Dired {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // In-mode minibuffer: route keys to line editing while a prompt is open.
        if self.input.is_some() {
            match key {
                key!(Esc) | ctrl!('c') | ctrl!('g') => self.input = None,
                key!(Enter) | ctrl!('j') => {
                    if let Some(inp) = self.input.take() {
                        self.run_pending(inp.action, &inp.buffer, cx);
                    }
                }
                key!(Backspace) | ctrl!('h') => {
                    if let Some(inp) = self.input.as_mut() {
                        inp.buffer.pop();
                    }
                }
                ctrl!('u') => {
                    if let Some(inp) = self.input.as_mut() {
                        inp.buffer.clear();
                    }
                }
                KeyEvent {
                    code: KeyCode::Char(c),
                    modifiers,
                } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                    if let Some(inp) = self.input.as_mut() {
                        inp.buffer.push(c);
                    }
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('k') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!('g') | key!(Home) => self.selected = 0,
            key!('G') | key!(End) => self.selected = self.entries.len().saturating_sub(1),
            key!('R') | key!('l') => {
                if let Err(err) = self.read_dir() {
                    self.error = Some(format!("{err}"));
                }
            }
            alt!('}') => self.next_marked(1),
            alt!('{') => self.next_marked(-1),
            key!('(') => self.show_details = !self.show_details,
            key!(Enter) | key!('f') => {
                if let Some(cb) = self.visit() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('^') | key!('-') => self.up_dir(),
            key!('m') => {
                if let Some(n) = self.current_name() {
                    self.marked.insert(n);
                    self.move_selection(1);
                }
            }
            key!('u') => {
                if let Some(n) = self.current_name() {
                    self.marked.remove(&n);
                    self.flagged.remove(&n);
                    self.move_selection(1);
                }
            }
            key!(Backspace) => {
                self.move_selection(-1);
                if let Some(n) = self.current_name() {
                    self.marked.remove(&n);
                    self.flagged.remove(&n);
                }
            }
            key!('U') => {
                self.marked.clear();
                self.flagged.clear();
            }
            key!('t') => self.toggle_all_marks(),
            key!('d') => {
                if let Some(n) = self.current_name() {
                    self.flagged.insert(n);
                    self.move_selection(1);
                }
            }
            key!('~') => {
                let n = self.flag_matching(zemacs_core::dired::is_backup_file);
                cx.editor
                    .set_status(format!("dired: flagged {n} backup file(s)"));
            }
            key!('#') => {
                let n = self.flag_matching(zemacs_core::dired::is_auto_save_file);
                cx.editor
                    .set_status(format!("dired: flagged {n} auto-save file(s)"));
            }
            key!('&') => {
                let n = self.flag_matching(zemacs_core::dired::is_garbage_file);
                cx.editor
                    .set_status(format!("dired: flagged {n} garbage file(s)"));
            }
            key!('x') => {
                let names: Vec<String> = self
                    .entries
                    .iter()
                    .filter(|e| self.flagged.contains(&e.name))
                    .map(|e| e.name.clone())
                    .collect();
                if names.is_empty() {
                    cx.editor.set_status("dired: no files flagged for deletion");
                } else {
                    let n = self.delete_names(&names);
                    let _ = self.read_dir();
                    cx.editor.set_status(format!("dired: deleted {n} file(s)"));
                }
            }
            key!('D') => {
                let names = self.targets();
                if !names.is_empty() {
                    let n = self.delete_names(&names);
                    let _ = self.read_dir();
                    cx.editor.set_status(format!("dired: deleted {n} file(s)"));
                }
            }
            key!('w') => {
                let names = self.targets();
                if !names.is_empty() {
                    let joined = names.join(" ");
                    let _ = cx.editor.registers.write('+', vec![joined.clone()]);
                    cx.editor.set_status(format!("dired: copied {joined}"));
                }
            }
            key!('s') => {
                self.sort = self.sort.next();
                self.resort();
                cx.editor
                    .set_status(format!("dired: sorted by {}", self.sort.label()));
            }
            key!('r') => {
                self.reverse = !self.reverse;
                self.resort();
            }
            key!('.') => {
                self.show_hidden = !self.show_hidden;
                if let Err(err) = self.read_dir() {
                    self.error = Some(format!("{err}"));
                }
            }
            // ---- ported no-prompt commands ----
            alt!('d') => {
                let n = self.mark_where(|e| e.is_dir);
                cx.editor
                    .set_status(format!("dired: marked {n} directory(ies)"));
            }
            alt!('x') => {
                let n = self.mark_executables();
                cx.editor
                    .set_status(format!("dired: marked {n} executable(s)"));
            }
            alt!('s') => {
                let n = self.mark_where(|e| e.is_symlink);
                cx.editor.set_status(format!("dired: marked {n} symlink(s)"));
            }
            key!('N') => {
                let (count, bytes) = marked_summary(&self.entries, |n| self.marked.contains(n));
                cx.editor.set_status(format!(
                    "dired: {count} marked file(s), {} total",
                    human_size(bytes)
                ));
            }
            key!('A') => {
                let n = self.rename_transform(NameTransform::Upcase);
                cx.editor.set_status(format!("dired: upcased {n} name(s)"));
            }
            key!('Z') => {
                let n = self.rename_transform(NameTransform::Downcase);
                cx.editor.set_status(format!("dired: downcased {n} name(s)"));
            }
            key!('K') => {
                let n = self.kill_lines();
                cx.editor.set_status(format!("dired: killed {n} line(s)"));
            }
            key!('>') => {
                if let Some(i) = next_dir_index(&self.entries, self.selected, true) {
                    self.selected = i;
                }
            }
            key!('<') => {
                if let Some(i) = next_dir_index(&self.entries, self.selected, false) {
                    self.selected = i;
                }
            }
            key!('T') => {
                let n = self.touch_targets();
                cx.editor.set_status(format!("dired: touched {n} file(s)"));
            }
            key!('v') => {
                if let Some(cb) = self.open_file(Action::Replace, true) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!('o') => {
                if let Some(cb) = self.open_file(Action::VerticalSplit, false) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            ctrl!('o') => {
                if let Some(cb) = self.open_file(Action::HorizontalSplit, false) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            // ---- ported minibuffer-prompt commands ----
            key!('+') => self.begin_input("Create directory: ", Pending::CreateDir),
            key!('E') => self.begin_input("Create empty file: ", Pending::CreateFile),
            key!('C') => {
                let t = self.targets();
                self.begin_input("Copy to: ", Pending::Copy(t));
            }
            key!('%') => {
                let t = self.targets();
                self.begin_input("Rename to: ", Pending::Rename(t));
            }
            key!('S') => {
                let t = self.targets();
                self.begin_input("Symlink to: ", Pending::Symlink(t));
            }
            key!('H') => {
                let t = self.targets();
                self.begin_input("Hardlink to: ", Pending::Hardlink(t));
            }
            key!('M') => {
                let t = self.targets();
                self.begin_input("Chmod (octal): ", Pending::Chmod(t));
            }
            key!('*') => self.begin_input("Mark (regexp): ", Pending::MarkRegexp),
            key!('/') => self.begin_input("Flag for deletion (regexp): ", Pending::FlagRegexp),
            key!('J') => self.begin_input("Goto file: ", Pending::GotoFile),
            _ => {}
        }
        // Stay modal: never leak keys to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let header_style = theme.get("ui.text.focus");
        let dir_style = theme.get("ui.text.directory");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let mark_style = theme.get("diff.plus");
        let flag_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = format!(" Dired: {}", self.dir.display());
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "Enter open  m mark  d flag  x del  s sort  q quit";
        if title.len() + hint.len() + 3 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(2);
        self.viewport = body_h as usize;

        if let Some(err) = &self.error {
            surface.set_stringn(area.x, area.y + 1, err, area.width as usize, flag_style);
        }

        if self.entries.is_empty() {
            surface.set_stringn(area.x, body_y, "(empty)", area.width as usize, info_style);
            return;
        }

        // Keep the selection in view.
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected + 1 - self.viewport;
        }

        for (offset, e) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let m = mark_char(
                self.marked.contains(&e.name),
                self.flagged.contains(&e.name),
            );
            let kind = if e.is_symlink {
                "l"
            } else if e.is_dir {
                "d"
            } else {
                "-"
            };
            let size = if e.is_dir {
                String::new()
            } else {
                human_size(e.size)
            };
            let name = if e.is_dir {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            };
            let line = if self.show_details {
                format!("{} {} {:>7}  {}", m, kind, size, name)
            } else {
                // dired-hide-details-mode: mark + name only.
                format!("{} {}", m, name)
            };
            let base = if offset == self.selected {
                sel_style
            } else if e.is_dir {
                dir_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, &line, area.width as usize, base);
            // Accent the mark column.
            if m != ' ' {
                let ms = if m == 'D' { flag_style } else { mark_style };
                surface.set_stringn(area.x, y, &m.to_string(), 1, ms);
            }
        }

        // Footer: the active minibuffer read, else the listing counts.
        let footer = if let Some(inp) = &self.input {
            format!("{}{}", inp.prompt, inp.buffer)
        } else {
            format!(
                "{} items  {} marked  {} flagged  sort:{}{}",
                self.entries.len(),
                self.marked.len(),
                self.flagged.len(),
                self.sort.label(),
                if self.reverse { " (rev)" } else { "" }
            )
        };
        if body_h > 0 {
            let style = if self.input.is_some() {
                header_style
            } else {
                info_style
            };
            surface.set_stringn(
                area.x,
                area.y + area.height - 1,
                &footer,
                area.width as usize,
                style,
            );
        }
    }
}
