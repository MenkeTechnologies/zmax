//! Directory diff — JetBrains "Compare Directories" viewer.
//!
//! Two directory trees compared file by file: each relative path is equal,
//! different, or only on one side. Every row carries the operation a
//! synchronize would carry out — copy across, delete, or nothing — which
//! starts at the IDE's default (copy a new file to the side that lacks it)
//! and can be set per row, or planned for the selection as a mirror of one
//! side. Synchronize runs the operations of the selected rows, or all.
//!
//! Keys: j/k move · Space mark · Enter diff the pair · > copy to right ·
//! < copy to left · D delete · - do nothing · = default · L mirror to left ·
//! R mirror to right · x exclude · c compare two marked new files · u undo that
//! pairing · s synchronize selected · S synchronize all · W warn on delete ·
//! 1/2/3/4 show equal / different / left-only / right-only · g rescan · q quit.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};
use crate::{ctrl, key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Equal,
    Different,
    OnlyLeft,
    OnlyRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    None,
    CopyToRight,
    CopyToLeft,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to both roots.
    pub rel: PathBuf,
    pub status: Status,
    pub op: Operation,
    /// For a pair the user made of a left-only and a right-only file
    /// ("Compare New Files with Each Other"): the right side's path.
    pub paired_right: Option<PathBuf>,
}

/// The default plan for a row: a file on one side is copied to the other.
pub fn default_op(status: Status) -> Operation {
    match status {
        Status::OnlyLeft => Operation::CopyToRight,
        Status::OnlyRight => Operation::CopyToLeft,
        Status::Equal | Status::Different => Operation::None,
    }
}

/// The plan that makes the right side (`to_right`) — or the left — the same
/// as the other.
pub fn mirror_op(status: Status, to_right: bool) -> Operation {
    match (status, to_right) {
        (Status::Equal, _) => Operation::None,
        (Status::Different, true) => Operation::CopyToRight,
        (Status::Different, false) => Operation::CopyToLeft,
        (Status::OnlyLeft, true) => Operation::CopyToRight,
        (Status::OnlyLeft, false) => Operation::Delete,
        (Status::OnlyRight, true) => Operation::Delete,
        (Status::OnlyRight, false) => Operation::CopyToLeft,
    }
}

/// Every regular file under `root`, relative to it, skipping VCS directories.
fn files_under(root: &Path) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if entry.file_name() != ".git" {
                    stack.push(path);
                }
            } else if kind.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.insert(rel.to_path_buf());
                }
            }
        }
    }
    out
}

/// Compare the trees under `left` and `right`.
pub fn compare(left: &Path, right: &Path) -> Vec<Entry> {
    let (l, r) = (files_under(left), files_under(right));
    l.union(&r)
        .map(|rel| {
            let status = match (l.contains(rel), r.contains(rel)) {
                (true, true) if same_contents(&left.join(rel), &right.join(rel)) => Status::Equal,
                (true, true) => Status::Different,
                (true, false) => Status::OnlyLeft,
                _ => Status::OnlyRight,
            };
            Entry { rel: rel.clone(), status, op: default_op(status), paired_right: None }
        })
        .collect()
}

fn same_contents(a: &Path, b: &Path) -> bool {
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// Which statuses are shown (JetBrains "Show Equal / Different / New on Left /
/// New on Right").
#[derive(Clone, Copy, Debug)]
pub struct Filter {
    pub equal: bool,
    pub different: bool,
    pub left: bool,
    pub right: bool,
}

impl Filter {
    fn shows(&self, status: Status) -> bool {
        match status {
            Status::Equal => self.equal,
            Status::Different => self.different,
            Status::OnlyLeft => self.left,
            Status::OnlyRight => self.right,
        }
    }
}

pub struct DirDiff {
    left: PathBuf,
    right: PathBuf,
    entries: Vec<Entry>,
    filter: Filter,
    /// Indices into `entries` of the rows shown.
    shown: Vec<usize>,
    /// Index into `shown`.
    selected: usize,
    marked: BTreeSet<usize>,
    scroll: usize,
    /// "Warn When Delete": a synchronize that deletes asks first.
    warn_on_delete: bool,
    status: String,
}

impl DirDiff {
    pub fn new(left: PathBuf, right: PathBuf) -> Self {
        let entries = compare(&left, &right);
        let mut view = DirDiff {
            left,
            right,
            entries,
            filter: Filter { equal: false, different: true, left: true, right: true },
            shown: Vec::new(),
            selected: 0,
            marked: BTreeSet::new(),
            scroll: 0,
            warn_on_delete: true,
            status: String::new(),
        };
        view.refilter();
        view
    }

    fn refilter(&mut self) {
        self.shown = (0..self.entries.len()).filter(|&i| self.filter.shows(self.entries[i].status)).collect();
        self.selected = self.selected.min(self.shown.len().saturating_sub(1));
        self.marked.retain(|i| self.shown.contains(i));
    }

    /// The rows an operation applies to: the marked ones, or the selected one.
    fn targets(&self) -> Vec<usize> {
        if self.marked.is_empty() {
            self.shown.get(self.selected).copied().into_iter().collect()
        } else {
            self.marked.iter().copied().collect()
        }
    }

    fn set_op(&mut self, f: impl Fn(&Entry) -> Operation) {
        for i in self.targets() {
            let op = f(&self.entries[i]);
            self.entries[i].op = op;
        }
    }

    /// JetBrains "Exclude from Results": the rows leave the comparison.
    fn exclude(&mut self) {
        let mut targets = self.targets();
        targets.sort_unstable_by(|a, b| b.cmp(a));
        for i in targets {
            self.entries.remove(i);
        }
        self.marked.clear();
        self.refilter();
    }

    /// JetBrains "Compare New Files with Each Other": a marked left-only and a
    /// marked right-only file become one row that compares the two.
    fn pair_new_files(&mut self) -> Result<(), &'static str> {
        let marked: Vec<usize> = self.marked.iter().copied().collect();
        let [a, b] = marked[..] else {
            return Err("mark one new file on each side");
        };
        let (l, r) = match (self.entries[a].status, self.entries[b].status) {
            (Status::OnlyLeft, Status::OnlyRight) => (a, b),
            (Status::OnlyRight, Status::OnlyLeft) => (b, a),
            _ => return Err("mark one new file on each side"),
        };
        let right_rel = self.entries[r].rel.clone();
        let equal = same_contents(&self.left.join(&self.entries[l].rel), &self.right.join(&right_rel));
        let entry = &mut self.entries[l];
        entry.status = if equal { Status::Equal } else { Status::Different };
        entry.op = Operation::None;
        entry.paired_right = Some(right_rel);
        self.entries.remove(r);
        self.marked.clear();
        self.refilter();
        Ok(())
    }

    /// JetBrains "Cancel Comparing New Files with Each Other": split the
    /// pair back into its two new files.
    fn unpair(&mut self) {
        for i in self.targets() {
            if let Some(right) = self.entries[i].paired_right.take() {
                self.entries[i].status = Status::OnlyLeft;
                self.entries[i].op = default_op(Status::OnlyLeft);
                self.entries.push(Entry {
                    rel: right,
                    status: Status::OnlyRight,
                    op: default_op(Status::OnlyRight),
                    paired_right: None,
                });
            }
        }
        self.entries.sort_by(|a, b| a.rel.cmp(&b.rel));
        self.refilter();
    }

    fn right_path(&self, entry: &Entry) -> PathBuf {
        self.right.join(entry.paired_right.as_ref().unwrap_or(&entry.rel))
    }

    /// Carry out the operations of `rows`, then compare again. Returns what
    /// was done.
    fn synchronize(&mut self, rows: Vec<usize>) -> String {
        let (mut done, mut failed) = (0, Vec::new());
        for i in rows {
            let entry = self.entries[i].clone();
            let (left, right) = (self.left.join(&entry.rel), self.right_path(&entry));
            let result = match entry.op {
                Operation::None => continue,
                Operation::CopyToRight => copy(&left, &right),
                Operation::CopyToLeft => copy(&right, &left),
                Operation::Delete => match entry.status {
                    Status::OnlyLeft => std::fs::remove_file(&left),
                    Status::OnlyRight => std::fs::remove_file(&right),
                    _ => std::fs::remove_file(&left).and_then(|_| std::fs::remove_file(&right)),
                },
            };
            match result {
                Ok(()) => done += 1,
                Err(e) => failed.push(format!("{}: {e}", entry.rel.display())),
            }
        }
        self.entries = compare(&self.left, &self.right);
        self.marked.clear();
        self.refilter();
        match failed.as_slice() {
            [] => format!("synchronized {done} file(s)"),
            _ => format!("synchronized {done}, failed: {}", failed.join("; ")),
        }
    }

    /// Synchronize `rows`, asking first when one deletes and the option is on.
    fn synchronize_asking(&mut self, rows: Vec<usize>) -> Option<Callback> {
        let deletes = rows.iter().filter(|&&i| self.entries[i].op == Operation::Delete).count();
        if deletes == 0 || !self.warn_on_delete {
            self.status = self.synchronize(rows);
            return None;
        }
        let question = format!("Synchronize, deleting {deletes} file(s)? (y/n)");
        Some(Box::new(move |compositor: &mut Compositor, _| {
            let confirm = crate::ui::confirm::Confirm::new(question, "nothing synchronized", move |_cx| {
                crate::compositor::defer([Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
                    if let Some(view) = compositor.find::<DirDiff>() {
                        let status = view.synchronize(rows);
                        cx.editor.set_status(status);
                    }
                }) as Callback]);
            });
            compositor.push(Box::new(confirm));
        }))
    }

    /// Open the selected pair in the diff viewer, comparison only.
    fn diff_selected(&self, cx: &mut Context) -> Option<Callback> {
        let entry = self.entries.get(*self.shown.get(self.selected)?)?.clone();
        let (left, right) = (self.left.join(&entry.rel), self.right_path(&entry));
        let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
        let (a, b) = (read(&left), read(&right));
        let doc_id = zmax_view::current_ref!(cx.editor).1.id();
        let view = crate::ui::merge::DiffView::new(entry.rel.display().to_string(), doc_id, &a, &b)
            .read_only()
            .with_labels(format!(" {}", self.left.display()), format!(" {}", self.right.display()));
        Some(Box::new(move |compositor: &mut Compositor, _| compositor.push(Box::new(view))))
    }
}

/// Copy `from` over `to`, making `to`'s directory if needed.
fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to).map(|_| ())
}

fn status_mark(status: Status) -> &'static str {
    match status {
        Status::Equal => "=",
        Status::Different => "≠",
        Status::OnlyLeft => "◧",
        Status::OnlyRight => "◨",
    }
}

fn op_mark(op: Operation) -> &'static str {
    match op {
        Operation::None => "  ",
        Operation::CopyToRight => "→ ",
        Operation::CopyToLeft => "← ",
        Operation::Delete => "✗ ",
    }
}

impl Component for DirDiff {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        self.status.clear();
        match *key {
            key!('q') | key!(Esc) | ctrl!('c') => {
                return EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, _| {
                    compositor.pop();
                })))
            }
            key!('j') | key!(Down) => self.selected = (self.selected + 1).min(self.shown.len().saturating_sub(1)),
            key!('k') | key!(Up) => self.selected = self.selected.saturating_sub(1),
            key!(' ') => {
                if let Some(&i) = self.shown.get(self.selected) {
                    if !self.marked.remove(&i) {
                        self.marked.insert(i);
                    }
                    self.selected = (self.selected + 1).min(self.shown.len().saturating_sub(1));
                }
            }
            key!(Enter) => return EventResult::Consumed(self.diff_selected(cx)),
            key!('>') => self.set_op(|_| Operation::CopyToRight),
            key!('<') => self.set_op(|_| Operation::CopyToLeft),
            key!('D') => self.set_op(|_| Operation::Delete),
            key!('-') => self.set_op(|_| Operation::None),
            key!('=') => self.set_op(|e| default_op(e.status)),
            key!('L') => self.set_op(|e| mirror_op(e.status, false)),
            key!('R') => self.set_op(|e| mirror_op(e.status, true)),
            key!('x') => self.exclude(),
            key!('c') => {
                if let Err(e) = self.pair_new_files() {
                    self.status = e.to_string();
                }
            }
            key!('u') => self.unpair(),
            key!('s') => return EventResult::Consumed(self.synchronize_asking(self.targets())),
            key!('S') => return EventResult::Consumed(self.synchronize_asking(self.shown.clone())),
            key!('W') => {
                self.warn_on_delete = !self.warn_on_delete;
                self.status = format!("warn when deleting: {}", if self.warn_on_delete { "on" } else { "off" });
            }
            key!('1') => self.filter.equal = !self.filter.equal,
            key!('2') => self.filter.different = !self.filter.different,
            key!('3') => self.filter.left = !self.filter.left,
            key!('4') => self.filter.right = !self.filter.right,
            key!('g') => self.entries = compare(&self.left, &self.right),
            _ => {}
        }
        self.refilter();
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        let theme = &cx.editor.theme;
        surface.clear_with(area, theme.get("ui.background"));
        if area.height < 4 {
            return;
        }
        let on = |b: bool| if b { "on" } else { "off" };
        let title = format!(
            " {}  ⇔  {}   = {}  ≠ {}  ◧ {}  ◨ {}",
            self.left.display(),
            self.right.display(),
            on(self.filter.equal),
            on(self.filter.different),
            on(self.filter.left),
            on(self.filter.right)
        );
        surface.set_stringn(area.x, area.y, &title, area.width as usize, theme.get("ui.text.focus"));
        let body_h = area.height.saturating_sub(2) as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + body_h {
            self.scroll = self.selected + 1 - body_h;
        }
        for (row, &i) in self.shown.iter().enumerate().skip(self.scroll).take(body_h) {
            let entry = &self.entries[i];
            let y = area.y + 1 + (row - self.scroll) as u16;
            let mark = if self.marked.contains(&i) { "●" } else { " " };
            let name = match &entry.paired_right {
                Some(r) => format!("{} ⇔ {}", entry.rel.display(), r.display()),
                None => entry.rel.display().to_string(),
            };
            let line = format!("{mark}{} {} {name}", status_mark(entry.status), op_mark(entry.op));
            let style = if row == self.selected {
                theme.get("ui.selection")
            } else {
                match entry.status {
                    Status::Different => theme.get("diff.delta"),
                    Status::OnlyLeft => theme.get("diff.minus"),
                    Status::OnlyRight => theme.get("diff.plus"),
                    Status::Equal => theme.get("ui.text"),
                }
            };
            surface.set_stringn(area.x, y, &line, area.width as usize, style);
        }
        let footer = if self.status.is_empty() {
            "⏎ diff  > < D - = ops  L R mirror  x exclude  c/u pair  s/S sync  W warn  1-4 filters  q quit"
        } else {
            self.status.as_str()
        };
        surface.set_stringn(area.x, area.y + area.height - 1, footer, area.width as usize, theme.get("ui.linenr"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_mirror_plans() {
        assert_eq!(Operation::CopyToRight, default_op(Status::OnlyLeft));
        assert_eq!(Operation::None, default_op(Status::Different));
        assert_eq!(Operation::Delete, mirror_op(Status::OnlyLeft, false), "mirroring to the left drops what the right lacks");
        assert_eq!(Operation::CopyToLeft, mirror_op(Status::Different, false));
        assert_eq!(Operation::Delete, mirror_op(Status::OnlyRight, true));
    }

    #[test]
    fn compare_and_synchronize_two_trees() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::write(a.path().join("same"), "x").unwrap();
        std::fs::write(b.path().join("same"), "x").unwrap();
        std::fs::write(a.path().join("diff"), "1").unwrap();
        std::fs::write(b.path().join("diff"), "2").unwrap();
        std::fs::create_dir(a.path().join("sub")).unwrap();
        std::fs::write(a.path().join("sub/new"), "n").unwrap();
        let mut view = DirDiff::new(a.path().to_path_buf(), b.path().to_path_buf());
        let status = |v: &DirDiff, name: &str| v.entries.iter().find(|e| e.rel == Path::new(name)).map(|e| e.status);
        assert_eq!(Some(Status::Equal), status(&view, "same"));
        assert_eq!(Some(Status::Different), status(&view, "diff"));
        assert_eq!(Some(Status::OnlyLeft), status(&view, "sub/new"));
        assert_eq!(2, view.shown.len(), "equal files are hidden by default");

        let all = view.shown.clone();
        view.synchronize(all);
        assert_eq!("n", std::fs::read_to_string(b.path().join("sub/new")).unwrap(), "the new file was copied over");
        assert_eq!("2", std::fs::read_to_string(b.path().join("diff")).unwrap(), "a differing file is left alone by default");
    }
}
