//! Magit-style git status porcelain (slice 1).
//!
//! A full-screen overlay [`Component`] that lists the repo's changes in
//! sections — **Untracked files**, **Unstaged changes**, **Staged changes** and
//! **Merge conflicts** — with a highlighted cursor row and inline actions to
//! stage, unstage, discard, refresh and commit. It is the hub other Magit
//! features will hang off of; opened with the `:magit` typable command (aliases
//! `:git`, `:gst`).
//!
//! The status is read by shelling out to `git status --porcelain` and parsed by
//! the pure, unit-tested [`parse_status`]; mutations (`git add`, `git reset`,
//! `git checkout`, `git commit`) also shell out, after which the buffer
//! re-reads the status so it stays live.
//!
//! Keys: `j`/`k`/arrows move the selection, `g`/`G` jump to top/bottom, `s`
//! stage, `u` unstage, `X` discard (press twice to confirm), `S` stage-all, `U`
//! unstage-all, `c` commit (multi-line message buffer), `a` amend the last
//! commit, `Enter` visit the file (a conflict row opens the `:merge` resolver),
//! `P` push, `F` fetch, `p` pull, `l` open the commit log, `g` refresh,
//! `q`/`Esc` close.
//!
//! Slice 2 adds remote operations, a proper multi-line commit-message editor
//! ([`MagitCommit`], committed via `git commit -F <tempfile>` so multi-line
//! messages and quoting are handled safely), and a scrollable commit log
//! ([`MagitLog`]) with a per-commit diff viewer ([`MagitShow`]). The ahead/behind
//! counts vs the upstream are shown in the header when an upstream is configured.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use tui::buffer::Buffer as Surface;
use zemacs_view::input::KeyEvent;
use zemacs_view::keyboard::{KeyCode, KeyModifiers};
use zemacs_view::{editor::Action, graphics::Rect};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Which section a change belongs to. Ordered as it is rendered.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Section {
    Untracked,
    Unstaged,
    Staged,
    Conflict,
}

impl Section {
    /// Render order (lower = drawn first).
    fn order(self) -> u8 {
        match self {
            Section::Untracked => 0,
            Section::Unstaged => 1,
            Section::Staged => 2,
            Section::Conflict => 3,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Section::Untracked => "Untracked files",
            Section::Unstaged => "Unstaged changes",
            Section::Staged => "Staged changes",
            Section::Conflict => "Merge conflicts",
        }
    }
}

/// One selectable change row: a path (relative to the repo root) classified into
/// exactly one [`Section`], with the two porcelain status chars for display.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StatusEntry {
    pub section: Section,
    pub path: String,
    /// Index (staged) status char.
    pub x: char,
    /// Worktree (unstaged) status char.
    pub y: char,
}

impl StatusEntry {
    /// A short two-char status code shown before the path (e.g. `M `, ` M`,
    /// `??`, `UU`).
    fn code(&self) -> String {
        format!("{}{}", self.x, self.y)
    }
}

/// Classify `git status --porcelain` (v1) output into per-section
/// [`StatusEntry`]s. Pure and unit-tested.
///
/// A single porcelain line can yield two entries: e.g. `MM file` is both a
/// staged and an unstaged change. Untracked (`??`) and unmerged/conflict
/// (`DD`/`AA`/`UU`/`AU`/`UA`/`DU`/`UD`) lines yield exactly one entry. Rename
/// lines (`R  old -> new`) are recorded under their new path.
pub fn parse_status(porcelain: &str) -> Vec<StatusEntry> {
    let mut out = Vec::new();
    for line in porcelain.lines() {
        // Each record is `XY <path>` (path begins at byte 3). Skip anything
        // shorter (blank lines, stray output).
        if line.len() < 4 {
            continue;
        }
        let mut chars = line.chars();
        let x = chars.next().unwrap();
        let y = chars.next().unwrap();
        let rest = &line[3..];
        // For renames/copies porcelain prints `old -> new`; act on the new path.
        let path = match rest.find(" -> ") {
            Some(idx) => rest[idx + 4..].to_string(),
            None => rest.to_string(),
        };

        if x == '?' && y == '?' {
            out.push(StatusEntry {
                section: Section::Untracked,
                path,
                x,
                y,
            });
            continue;
        }

        // Unmerged states (git's definition of an unmerged/conflicted entry).
        let conflict = matches!(
            (x, y),
            ('D', 'D')
                | ('A', 'A')
                | ('U', 'U')
                | ('A', 'U')
                | ('U', 'A')
                | ('D', 'U')
                | ('U', 'D')
        );
        if conflict {
            out.push(StatusEntry {
                section: Section::Conflict,
                path,
                x,
                y,
            });
            continue;
        }

        // Index status (X) ⇒ staged; worktree status (Y) ⇒ unstaged. A file can
        // be both (e.g. `MM`).
        if x != ' ' && x != '?' {
            out.push(StatusEntry {
                section: Section::Staged,
                path: path.clone(),
                x,
                y,
            });
        }
        if y != ' ' && y != '?' {
            out.push(StatusEntry {
                section: Section::Unstaged,
                path,
                x,
                y,
            });
        }
    }
    out
}

/// One hunk of a unified diff: the `@@ -a,b +c,d @@` header line plus the body
/// lines that follow it (context, `+` additions and `-` removals), up to but not
/// including the next hunk header.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hunk {
    /// The `@@ … @@` line (possibly with a trailing section-heading hint).
    pub header: String,
    /// The hunk body lines (verbatim, including their leading ` `/`+`/`-`).
    pub body: Vec<String>,
}

/// A file's parsed diff: the file-level header (`diff --git`, `index`, `---`,
/// `+++`, mode lines …) and the list of [`Hunk`]s. Used both for rendering the
/// expanded view and for reconstructing single-hunk patches to feed `git apply`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FileDiff {
    pub header: Vec<String>,
    pub hunks: Vec<Hunk>,
}

/// Split a unified diff (one file's worth, as produced by `git diff [--cached]
/// -- <path>`) into the file header lines and the list of hunks. Pure and
/// unit-tested.
///
/// Everything before the first `@@` line is the file header; each `@@` line
/// starts a new hunk whose body runs until the next `@@` or end of input. An
/// empty or hunk-less diff yields the header (possibly empty) and no hunks.
pub fn parse_diff_hunks(diff: &str) -> (Vec<String>, Vec<Hunk>) {
    let mut header = Vec::new();
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut seen_hunk = false;
    for line in diff.lines() {
        if line.starts_with("@@") {
            seen_hunk = true;
            hunks.push(Hunk {
                header: line.to_string(),
                body: Vec::new(),
            });
        } else if seen_hunk {
            // Safe: `seen_hunk` is only set after pushing at least one hunk.
            hunks
                .last_mut()
                .expect("hunk exists once seen_hunk is set")
                .body
                .push(line.to_string());
        } else {
            header.push(line.to_string());
        }
    }
    (header, hunks)
}

/// Reassemble a single-hunk patch from a file `header` and one [`Hunk`], in the
/// exact shape `git apply` expects: the header lines, the `@@` line, then the
/// hunk body, each terminated by a newline. Pure and unit-tested.
pub fn hunk_patch(header: &[String], hunk: &Hunk) -> String {
    let mut out = String::new();
    for line in header {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&hunk.header);
    out.push('\n');
    for line in &hunk.body {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A local branch as listed by `git branch`: its name and whether it is the
/// currently checked-out branch (the `*`-marked line).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BranchEntry {
    pub name: String,
    pub current: bool,
}

/// Parse `git branch` (plain, one branch per line) into [`BranchEntry`]s. Pure
/// and unit-tested. The current branch is the `* `-prefixed line; detached-HEAD
/// lines (`* (HEAD detached at …)`) are skipped.
pub fn parse_branches(out: &str) -> Vec<BranchEntry> {
    let mut entries = Vec::new();
    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let current = line.starts_with('*');
        let name = line.trim_start_matches('*').trim();
        if name.is_empty() || name.starts_with('(') {
            continue;
        }
        entries.push(BranchEntry {
            name: name.to_string(),
            current,
        });
    }
    entries
}

/// One stash entry as listed by `git stash list`: its ref (`stash@{N}`) and the
/// descriptive remainder.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StashEntry {
    pub reff: String,
    pub summary: String,
}

/// Parse `git stash list` output into [`StashEntry`]s. Pure and unit-tested.
/// Each line is `stash@{N}: <summary>`; the ref is everything up to the first
/// colon, the summary the trimmed remainder.
pub fn parse_stash(out: &str) -> Vec<StashEntry> {
    let mut entries = Vec::new();
    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let (reff, summary) = match line.split_once(':') {
            Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
            None => (line.to_string(), String::new()),
        };
        entries.push(StashEntry { reff, summary });
    }
    entries
}

/// One commit row in the log view: its abbreviated SHA and the rest of the
/// `--oneline` text (summary plus any `--decorate` ref names).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LogEntry {
    pub sha: String,
    pub summary: String,
}

/// Parse `git log --oneline [--decorate]` output into [`LogEntry`]s. Pure and
/// unit-tested.
///
/// Each non-empty line is `\<sha\> \<summary…\>`; the SHA is the first
/// whitespace-delimited token, the summary is the remainder (which may itself
/// begin with `(HEAD -> main, origin/main)` decorations). Blank lines are
/// skipped; a line with only a SHA yields an empty summary.
pub fn parse_log(out: &str) -> Vec<LogEntry> {
    let mut entries = Vec::new();
    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let (sha, summary) = match line.split_once(char::is_whitespace) {
            Some((sha, rest)) => (sha.to_string(), rest.trim_start().to_string()),
            None => (line.to_string(), String::new()),
        };
        entries.push(LogEntry { sha, summary });
    }
    entries
}

/// Parse `git rev-list --left-right --count @{u}...HEAD` output into
/// `(behind, ahead)`: the two whitespace-separated counts are the number of
/// upstream commits missing locally (behind) and the number of local commits
/// missing upstream (ahead). Returns `None` if the two integers can't be read
/// (e.g. no upstream configured). Pure and unit-tested.
pub fn parse_ahead_behind(out: &str) -> Option<(usize, usize)> {
    let mut it = out.split_whitespace();
    let behind = it.next()?.parse().ok()?;
    let ahead = it.next()?.parse().ok()?;
    Some((behind, ahead))
}

/// A single rendered line of the buffer, used for layout, scrolling and mapping
/// the selection cursor to a screen row.
enum Row {
    /// The `On branch …` / summary lines at the top.
    Info(String),
    /// A blank spacer line.
    Blank,
    /// A section header (`Untracked files (3)`).
    Header(String),
    /// A file row; carries the index into [`MagitStatus::entries`].
    File(usize),
    /// A hunk's `@@ … @@` header row (selectable). Identifies the owning entry
    /// and the hunk index within that entry's [`FileDiff`].
    HunkHeader {
        entry: usize,
        hunk: usize,
        text: String,
    },
    /// A hunk body line (not directly selectable, but highlighted when its hunk
    /// is selected). Carries the same `entry`/`hunk` identity for highlighting.
    HunkLine {
        entry: usize,
        hunk: usize,
        text: String,
    },
    /// An indented note shown under an expanded file that has no hunks (e.g. an
    /// untracked file).
    Note(String),
}

/// A selectable item in the status buffer: either a whole file row or a single
/// hunk within an expanded file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    File(usize),
    Hunk { entry: usize, hunk: usize },
}

impl Target {
    /// The index into [`MagitStatus::entries`] this target belongs to.
    fn entry_index(self) -> usize {
        match self {
            Target::File(i) => i,
            Target::Hunk { entry, .. } => entry,
        }
    }
}

/// The full-screen interactive magit-status overlay.
pub struct MagitStatus {
    /// Absolute path of the repository root (`git rev-parse --show-toplevel`).
    repo_dir: PathBuf,
    /// Current branch (or a short detached-HEAD description).
    head: String,
    /// All change rows, grouped/ordered by section.
    entries: Vec<StatusEntry>,
    /// Index into the current [`targets`](MagitStatus::targets) list of the
    /// highlighted item (a file row or a hunk row).
    selected: usize,
    /// Top visible rendered row.
    scroll: usize,
    /// Body rows visible in the last render (for scroll clamping).
    viewport: usize,
    /// Set after one `X` press; a second `X` confirms the destructive discard.
    pending_discard: bool,
    /// `(behind, ahead)` vs the configured upstream, or `None` when there is no
    /// upstream (shown in the header).
    upstream: Option<(usize, usize)>,
    /// Entries whose diff is expanded inline, keyed by `(section, path)` so the
    /// expansion survives a [`refresh`](MagitStatus::refresh).
    expanded: HashSet<(Section, String)>,
    /// Cached parsed diffs for the currently expanded entries, keyed the same
    /// way; rebuilt by [`refresh`](MagitStatus::refresh).
    diffs: HashMap<(Section, String), FileDiff>,
}

impl MagitStatus {
    /// Build a status buffer for the repository containing `start`, reading the
    /// initial status immediately. Returns `None` when `start` isn't inside a
    /// git work tree.
    pub fn new(start: &Path) -> Option<Self> {
        let repo_dir = git_repo_root(start)?;
        let mut view = MagitStatus {
            repo_dir,
            head: String::new(),
            entries: Vec::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            pending_discard: false,
            upstream: None,
            expanded: HashSet::new(),
            diffs: HashMap::new(),
        };
        view.refresh();
        Some(view)
    }

    /// Re-read `git status` + the current branch and rebuild the section list,
    /// clamping the selection to the new entry count.
    fn refresh(&mut self) {
        self.head = git_head(&self.repo_dir);
        self.upstream = git_output(
            &self.repo_dir,
            &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
        )
        .and_then(|s| parse_ahead_behind(&s));
        let porcelain = git_output(&self.repo_dir, &["status", "--porcelain"]).unwrap_or_default();
        let mut entries = parse_status(&porcelain);
        entries.sort_by(|a, b| {
            a.section
                .order()
                .cmp(&b.section.order())
                .then_with(|| a.path.cmp(&b.path))
        });
        self.entries = entries;
        self.rebuild_diffs();
        let target_count = self.targets().len();
        if self.selected >= target_count {
            self.selected = target_count.saturating_sub(1);
        }
    }

    /// Recompute the cached [`FileDiff`]s for every currently expanded entry by
    /// shelling out to `git diff` (worktree) or `git diff --cached` (index).
    /// Untracked/conflict entries have no plain diff, so they get no cache entry
    /// (the expanded view shows a note instead).
    fn rebuild_diffs(&mut self) {
        let keys: Vec<(Section, String)> = self
            .entries
            .iter()
            .map(|e| (e.section, e.path.clone()))
            .filter(|k| self.expanded.contains(k))
            .collect();
        let mut diffs = HashMap::new();
        for (section, path) in keys {
            let args: Vec<&str> = match section {
                Section::Unstaged => vec!["diff", "--", &path],
                Section::Staged => vec!["diff", "--cached", "--", &path],
                // Untracked has no tracked diff; conflicts show a combined diff
                // that isn't separately stageable, so we skip the cache.
                Section::Untracked | Section::Conflict => continue,
            };
            if let Some(out) = git_output(&self.repo_dir, &args) {
                let (header, hunks) = parse_diff_hunks(&out);
                diffs.insert((section, path), FileDiff { header, hunks });
            }
        }
        self.diffs = diffs;
    }

    /// Run a mutating `git -C <repo> …` command, returning the trimmed stderr on
    /// failure.
    fn run_git(&self, args: &[&str]) -> Result<(), String> {
        git_run(&self.repo_dir, args)
    }

    /// The currently selected target (file row or hunk row), if any.
    fn selected_target(&self) -> Option<Target> {
        self.targets().get(self.selected).copied()
    }

    /// The [`StatusEntry`] the selection belongs to (the file itself for a file
    /// row, or the owning file for a hunk row).
    fn selected_entry(&self) -> Option<&StatusEntry> {
        self.selected_target()
            .and_then(|t| self.entries.get(t.entry_index()))
    }

    /// The list of selectable targets in render order, derived from the rendered
    /// rows so it always matches what's on screen.
    fn targets(&self) -> Vec<Target> {
        self.rows()
            .iter()
            .filter_map(|r| match r {
                Row::File(i) => Some(Target::File(*i)),
                Row::HunkHeader { entry, hunk, .. } => Some(Target::Hunk {
                    entry: *entry,
                    hunk: *hunk,
                }),
                _ => None,
            })
            .collect()
    }

    /// Stage the selected file (`git add -- <path>`), then refresh.
    fn stage_selected(&mut self, cx: &mut Context) {
        let Some(path) = self.selected_entry().map(|e| e.path.clone()) else {
            return;
        };
        match self.run_git(&["add", "--", &path]) {
            Ok(()) => cx.editor.set_status(format!("staged {path}")),
            Err(e) => cx.editor.set_error(format!("git add: {e}")),
        }
        self.refresh();
    }

    /// Unstage the selected file (`git reset -q HEAD -- <path>`), then refresh.
    fn unstage_selected(&mut self, cx: &mut Context) {
        let Some(path) = self.selected_entry().map(|e| e.path.clone()) else {
            return;
        };
        match self.run_git(&["reset", "-q", "HEAD", "--", &path]) {
            Ok(()) => cx.editor.set_status(format!("unstaged {path}")),
            Err(e) => cx.editor.set_error(format!("git reset: {e}")),
        }
        self.refresh();
    }

    /// Discard the selected file's worktree changes: `git checkout -- <path>`
    /// for a tracked file, or delete it outright for an untracked one. Caller
    /// gates this behind a confirmation.
    fn discard_selected(&mut self, cx: &mut Context) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        let result = if entry.section == Section::Untracked {
            std::fs::remove_file(self.repo_dir.join(&entry.path)).map_err(|e| e.to_string())
        } else {
            self.run_git(&["checkout", "--", &entry.path])
        };
        match result {
            Ok(()) => cx.editor.set_status(format!("discarded {}", entry.path)),
            Err(e) => cx.editor.set_error(format!("discard failed: {e}")),
        }
        self.refresh();
    }

    fn stage_all(&mut self, cx: &mut Context) {
        match self.run_git(&["add", "-A"]) {
            Ok(()) => cx.editor.set_status("staged all changes"),
            Err(e) => cx.editor.set_error(format!("git add -A: {e}")),
        }
        self.refresh();
    }

    fn unstage_all(&mut self, cx: &mut Context) {
        match self.run_git(&["reset", "-q"]) {
            Ok(()) => cx.editor.set_status("unstaged all changes"),
            Err(e) => cx.editor.set_error(format!("git reset: {e}")),
        }
        self.refresh();
    }

    /// Run a `git -C <repo> …` and return `(success, message)` where `message`
    /// is the trimmed stdout + stderr joined into one line (git's remote
    /// commands write their progress/result to stderr).
    fn run_git_message(&self, args: &[&str]) -> (bool, String) {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.repo_dir);
        for a in args {
            cmd.arg(a);
        }
        match cmd.output() {
            Ok(out) => {
                let mut parts = Vec::new();
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stdout.trim().is_empty() {
                    parts.push(stdout.trim().to_string());
                }
                if !stderr.trim().is_empty() {
                    parts.push(stderr.trim().to_string());
                }
                (out.status.success(), condense(&parts.join("\n")))
            }
            Err(e) => (false, e.to_string()),
        }
    }

    /// Run a remote operation (push/fetch/pull), surface its output in the
    /// status line and refresh the buffer.
    fn remote_op(&mut self, cx: &mut Context, label: &str, args: &[&str]) {
        cx.editor.set_status(format!("{label}…"));
        let (ok, msg) = self.run_git_message(args);
        let msg = if msg.is_empty() {
            if ok {
                "done".to_string()
            } else {
                "failed".to_string()
            }
        } else {
            msg
        };
        if ok {
            cx.editor.set_status(format!("{label}: {msg}"));
        } else {
            cx.editor.set_error(format!("{label}: {msg}"));
        }
        self.refresh();
    }

    /// Build the linear list of rendered rows from the current entries.
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut head_line = format!("On branch {}", self.head);
        if let Some((behind, ahead)) = self.upstream {
            if ahead > 0 || behind > 0 {
                head_line.push_str(&format!(" (ahead {ahead}, behind {behind})"));
            } else {
                head_line.push_str(" (up to date)");
            }
        }
        rows.push(Row::Info(head_line));
        if self.entries.is_empty() {
            rows.push(Row::Blank);
            rows.push(Row::Info("nothing to commit, working tree clean".into()));
            return rows;
        }
        for section in [
            Section::Untracked,
            Section::Unstaged,
            Section::Staged,
            Section::Conflict,
        ] {
            let idxs: Vec<usize> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.section == section)
                .map(|(i, _)| i)
                .collect();
            if idxs.is_empty() {
                continue;
            }
            rows.push(Row::Blank);
            rows.push(Row::Header(format!("{} ({})", section.title(), idxs.len())));
            for i in idxs {
                rows.push(Row::File(i));
                let entry = &self.entries[i];
                let key = (entry.section, entry.path.clone());
                if !self.expanded.contains(&key) {
                    continue;
                }
                match self.diffs.get(&key) {
                    Some(fd) if !fd.hunks.is_empty() => {
                        for (h, hunk) in fd.hunks.iter().enumerate() {
                            rows.push(Row::HunkHeader {
                                entry: i,
                                hunk: h,
                                text: hunk.header.clone(),
                            });
                            for line in &hunk.body {
                                rows.push(Row::HunkLine {
                                    entry: i,
                                    hunk: h,
                                    text: line.clone(),
                                });
                            }
                        }
                    }
                    _ => {
                        let note = match entry.section {
                            Section::Untracked => "(untracked — s stages the whole file)",
                            Section::Conflict => "(conflict — resolve via Enter)",
                            _ => "(no changes to show)",
                        };
                        rows.push(Row::Note(note.to_string()));
                    }
                }
            }
        }
        rows
    }

    /// Move the selection by `delta`, clamped to the target range.
    fn move_selection(&mut self, delta: isize) {
        let count = self.targets().len();
        if count == 0 {
            return;
        }
        let max = count as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Visit the selected file: open it in the editor and close this overlay.
    /// A conflict row additionally launches the `:merge` resolver.
    fn visit_callback(&self) -> Option<Callback> {
        let entry = self.selected_entry()?.clone();
        let abs = self.repo_dir.join(&entry.path);
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                if let Err(err) = cx.editor.open(&abs, Action::Replace) {
                    cx.editor
                        .set_error(format!("failed to open {}: {err}", abs.display()));
                    return;
                }
                if entry.section == Section::Conflict {
                    crate::commands::typed::open_merge(cx.editor, cx.jobs);
                }
            },
        ))
    }

    /// Build the commit callback: open the multi-line [`MagitCommit`] message
    /// editor. A plain commit refuses when nothing is staged; an amend opens the
    /// editor pre-filled with the last commit message (`git log -1 --format=%B`)
    /// and is allowed even with nothing staged (a reword).
    fn commit_callback(&self, amend: bool) -> Callback {
        let has_staged = self.entries.iter().any(|e| e.section == Section::Staged);
        let repo_dir = self.repo_dir.clone();
        Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            if !amend && !has_staged {
                cx.editor.set_status("nothing staged to commit");
                return;
            }
            let initial = if amend {
                git_output(&repo_dir, &["log", "-1", "--format=%B"]).unwrap_or_default()
            } else {
                String::new()
            };
            let editor = MagitCommit::new(repo_dir.clone(), amend, initial.trim_end());
            compositor.push(Box::new(editor));
        })
    }

    /// Build the log callback: open the [`MagitLog`] commit-log sub-view.
    fn log_callback(&self) -> Callback {
        let repo_dir = self.repo_dir.clone();
        Box::new(move |compositor: &mut Compositor, _cx: &mut Context| {
            compositor.push(Box::new(MagitLog::new(repo_dir.clone())));
        })
    }

    /// Build the branch callback: open the [`MagitBranch`] menu.
    fn branch_callback(&self) -> Callback {
        let repo_dir = self.repo_dir.clone();
        Box::new(move |compositor: &mut Compositor, _cx: &mut Context| {
            compositor.push(Box::new(MagitBranch::new(repo_dir.clone())));
        })
    }

    /// Build the stash callback: open the [`MagitStash`] menu.
    fn stash_callback(&self) -> Callback {
        let repo_dir = self.repo_dir.clone();
        Box::new(move |compositor: &mut Compositor, _cx: &mut Context| {
            compositor.push(Box::new(MagitStash::new(repo_dir.clone())));
        })
    }

    /// `s`: stage the selection. On a file row this stages the whole file
    /// (slice-1 behaviour); on a hunk row it stages just that hunk via
    /// `git apply --cached`.
    fn stage(&mut self, cx: &mut Context) {
        match self.selected_target() {
            Some(Target::File(_)) => self.stage_selected(cx),
            Some(Target::Hunk { entry, hunk }) => {
                if self.entries[entry].section == Section::Unstaged {
                    self.apply_hunk(cx, entry, hunk, false);
                } else {
                    cx.editor.set_status("hunk is already staged (press u to unstage)");
                }
            }
            None => {}
        }
    }

    /// `u`: unstage the selection. On a file row this unstages the whole file;
    /// on a hunk row it unstages just that hunk via `git apply --cached
    /// --reverse`.
    fn unstage(&mut self, cx: &mut Context) {
        match self.selected_target() {
            Some(Target::File(_)) => self.unstage_selected(cx),
            Some(Target::Hunk { entry, hunk }) => {
                if self.entries[entry].section == Section::Staged {
                    self.apply_hunk(cx, entry, hunk, true);
                } else {
                    cx.editor.set_status("hunk is not staged (press s to stage)");
                }
            }
            None => {}
        }
    }

    /// Apply (stage) or reverse-apply (unstage) a single hunk by building a
    /// minimal one-hunk patch from the cached [`FileDiff`] and feeding it to
    /// `git apply --cached [--reverse]` via a temp file. Surfaces any
    /// `git apply` error in the status line and never panics.
    fn apply_hunk(&mut self, cx: &mut Context, entry: usize, hunk: usize, reverse: bool) {
        let Some(e) = self.entries.get(entry).cloned() else {
            return;
        };
        let key = (e.section, e.path.clone());
        // Clone the patch pieces so we drop the borrow on `self.diffs` before
        // shelling out / refreshing.
        let patch = match self.diffs.get(&key).and_then(|fd| {
            fd.hunks
                .get(hunk)
                .map(|h| hunk_patch(&fd.header, h))
        }) {
            Some(p) => p,
            None => {
                cx.editor.set_error("no hunk to apply (try g to refresh)");
                return;
            }
        };

        let tmp = std::env::temp_dir().join(format!(
            "zemacs-magit-hunk-{}-{}.patch",
            std::process::id(),
            hunk
        ));
        if let Err(err) = std::fs::write(&tmp, &patch) {
            cx.editor
                .set_error(format!("hunk apply: temp write failed: {err}"));
            return;
        }
        let tmp_str = tmp.to_string_lossy().into_owned();
        let mut args = vec!["apply", "--cached"];
        if reverse {
            args.push("--reverse");
        }
        args.push(&tmp_str);
        let result = self.run_git(&args);
        let _ = std::fs::remove_file(&tmp);
        match result {
            Ok(()) => {
                let verb = if reverse { "unstaged" } else { "staged" };
                cx.editor.set_status(format!("{verb} hunk in {}", e.path));
            }
            Err(err) => cx.editor.set_error(format!("git apply: {err}")),
        }
        self.refresh();
    }

    /// `Tab`: toggle inline expansion of the selection's file. Untracked and
    /// conflict files have no separable hunks, so the expanded view just shows a
    /// note.
    fn toggle_expand(&mut self, cx: &mut Context) {
        let Some(e) = self.selected_entry().cloned() else {
            return;
        };
        let key = (e.section, e.path.clone());
        // `remove` returns false when it wasn't expanded → expand it now.
        if !self.expanded.remove(&key) {
            if e.section == Section::Untracked {
                cx.editor
                    .set_status("untracked file — press s to stage the whole file");
            }
            self.expanded.insert(key);
        }
        self.refresh();
    }
}

/// Schedule a refresh of the (possibly buried) [`MagitStatus`] overlay once the
/// current job settles — used after a commit pops its editor.
fn schedule_status_refresh(cx: &mut Context) {
    cx.jobs.callback(async move {
        let call = crate::job::Callback::EditorCompositor(Box::new(
            move |_editor, compositor: &mut Compositor| {
                if let Some(m) = compositor.find::<MagitStatus>() {
                    m.refresh();
                }
            },
        ));
        Ok(call)
    });
}

/// Collapse a multi-line git message into a single status-line-friendly string:
/// non-empty lines joined with `" · "`, truncated so the status bar stays sane.
fn condense(msg: &str) -> String {
    let joined = msg
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    if joined.chars().count() > 160 {
        let truncated: String = joined.chars().take(157).collect();
        format!("{truncated}…")
    } else {
        joined
    }
}

impl Component for MagitStatus {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // Any key other than a confirming `X` cancels a pending discard.
        if key != key!('X') && self.pending_discard {
            self.pending_discard = false;
        }

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('k') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!('g') => self.refresh(),
            key!('G') | key!(End) => self.selected = self.targets().len().saturating_sub(1),
            key!(Home) => self.selected = 0,
            key!(Tab) => self.toggle_expand(cx),
            key!('s') => self.stage(cx),
            key!('u') => self.unstage(cx),
            key!('S') => self.stage_all(cx),
            key!('U') => self.unstage_all(cx),
            key!('b') => return EventResult::Consumed(Some(self.branch_callback())),
            key!('z') => return EventResult::Consumed(Some(self.stash_callback())),
            key!('X') => {
                if self.entries.is_empty() {
                    // nothing to discard
                } else if self.pending_discard {
                    self.pending_discard = false;
                    self.discard_selected(cx);
                } else {
                    self.pending_discard = true;
                    let name = self
                        .selected_entry()
                        .map(|e| e.path.as_str())
                        .unwrap_or("file");
                    cx.editor
                        .set_status(format!("press X again to discard {name}"));
                }
            }
            key!('c') => return EventResult::Consumed(Some(self.commit_callback(false))),
            key!('a') => return EventResult::Consumed(Some(self.commit_callback(true))),
            key!('l') => return EventResult::Consumed(Some(self.log_callback())),
            key!('P') => self.remote_op(cx, "push", &["push"]),
            key!('F') => self.remote_op(cx, "fetch", &["fetch"]),
            key!('p') => self.remote_op(cx, "pull", &["pull"]),
            key!(Enter) => {
                if let Some(cb) = self.visit_callback() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            _ => {}
        }
        // Stay modal: never leak keys to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let plus_style = theme.get("diff.plus");
        let minus_style = theme.get("diff.minus");
        let conflict_style = theme.get("diff.delta.conflict");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        // Title + key hint.
        let title = " Magit status";
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        let hint =
            "Tab expand  s stage  u unstage  X discard  c commit  a amend  b branch  z stash  l log  P push  F fetch  p pull  g refresh  q quit";
        if (title.len() + hint.len() + 3) < area.width as usize {
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

        let text_style = theme.get("ui.text");
        let sel_target = self.selected_target();
        let rows = self.rows();
        // Keep the selected target's row inside the viewport.
        let is_selected_row = |row: &Row| -> bool {
            match (row, sel_target) {
                (Row::File(i), Some(Target::File(j))) => *i == j,
                (
                    Row::HunkHeader { entry, hunk, .. },
                    Some(Target::Hunk {
                        entry: se,
                        hunk: sh,
                    }),
                ) => *entry == se && *hunk == sh,
                _ => false,
            }
        };
        // A row belongs to the selected hunk (header or body) — highlighted as a
        // block when that hunk is the selection.
        let in_selected_hunk = |row: &Row| -> bool {
            match (row, sel_target) {
                (
                    Row::HunkHeader { entry, hunk, .. } | Row::HunkLine { entry, hunk, .. },
                    Some(Target::Hunk {
                        entry: se,
                        hunk: sh,
                    }),
                ) => *entry == se && *hunk == sh,
                _ => false,
            }
        };
        if let Some(sel_row) = rows.iter().position(is_selected_row) {
            if sel_row < self.scroll {
                self.scroll = sel_row;
            } else if sel_row >= self.scroll + self.viewport {
                self.scroll = sel_row - self.viewport + 1;
            }
        }

        for (offset, row) in rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let selected_block = is_selected_row(row) || in_selected_hunk(row);
            match row {
                Row::Blank => {}
                Row::Info(text) => {
                    surface.set_stringn(area.x, y, text, area.width as usize, info_style);
                }
                Row::Header(text) => {
                    surface.set_stringn(area.x, y, text, area.width as usize, header_style);
                }
                Row::Note(text) => {
                    surface.set_stringn(area.x, y, &format!("    {text}"), area.width as usize, info_style);
                }
                Row::File(i) => {
                    let entry = &self.entries[*i];
                    let base = match entry.section {
                        Section::Untracked => plus_style,
                        Section::Unstaged => minus_style,
                        Section::Staged => plus_style,
                        Section::Conflict => conflict_style,
                    };
                    if selected_block {
                        surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
                    }
                    let marker = if self.expanded.contains(&(entry.section, entry.path.clone())) {
                        '▾'
                    } else {
                        '▸'
                    };
                    let line = format!("{marker} {} {}", entry.code(), entry.path);
                    let style = if selected_block { sel_style } else { base };
                    surface.set_stringn(area.x, y, &line, area.width as usize, style);
                }
                Row::HunkHeader { text, .. } => {
                    if selected_block {
                        surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
                    }
                    let style = if selected_block { sel_style } else { info_style };
                    surface.set_stringn(
                        area.x,
                        y,
                        &format!("    {text}"),
                        area.width as usize,
                        style,
                    );
                }
                Row::HunkLine { text, .. } => {
                    if selected_block {
                        surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
                    }
                    let base = if text.starts_with('+') {
                        plus_style
                    } else if text.starts_with('-') {
                        minus_style
                    } else {
                        text_style
                    };
                    let style = if selected_block { sel_style } else { base };
                    surface.set_stringn(
                        area.x,
                        y,
                        &format!("    {text}"),
                        area.width as usize,
                        style,
                    );
                }
            }
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit")
    }
}

/// Byte offset of the `char_idx`-th character in `s` (or `s.len()` if past the
/// end), for editing a `String` by character position.
fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// A multi-line commit-message editor overlay.
///
/// Opened from the status buffer with `c` (fresh) or `a` (amend, pre-filled with
/// the last message). The user types a normal multi-line message; `Ctrl-c
/// Ctrl-c` (two presses) confirms and `Esc` cancels. On confirm the message is
/// written to a temp file and committed with `git commit -F <tempfile>` (plus
/// `--amend` when amending), so multi-line text and shell-special characters are
/// handled safely; the buried [`MagitStatus`] is then refreshed.
pub struct MagitCommit {
    repo_dir: PathBuf,
    /// True when amending the previous commit (`git commit --amend`).
    amend: bool,
    /// Message body, one entry per line (never empty: at least `[""]`).
    lines: Vec<String>,
    /// Cursor line index into `lines`.
    row: usize,
    /// Cursor column as a character index within `lines[row]`.
    col: usize,
    /// Top visible body row.
    scroll: usize,
    /// Body rows visible in the last render.
    viewport: usize,
    /// Set after one `Ctrl-c`; a second `Ctrl-c` confirms the commit.
    pending_confirm: bool,
}

impl MagitCommit {
    fn new(repo_dir: PathBuf, amend: bool, initial: &str) -> Self {
        let mut lines: Vec<String> = initial.split('\n').map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let row = lines.len() - 1;
        let col = lines[row].chars().count();
        MagitCommit {
            repo_dir,
            amend,
            lines,
            row,
            col,
            scroll: 0,
            viewport: 1,
            pending_confirm: false,
        }
    }

    /// Character length of the current line.
    fn cur_len(&self) -> usize {
        self.lines[self.row].chars().count()
    }

    fn insert_char(&mut self, c: char) {
        let b = char_to_byte(&self.lines[self.row], self.col);
        self.lines[self.row].insert(b, c);
        self.col += 1;
    }

    fn newline(&mut self) {
        let b = char_to_byte(&self.lines[self.row], self.col);
        let tail = self.lines[self.row].split_off(b);
        self.lines.insert(self.row + 1, tail);
        self.row += 1;
        self.col = 0;
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            let start = char_to_byte(&self.lines[self.row], self.col - 1);
            let end = char_to_byte(&self.lines[self.row], self.col);
            self.lines[self.row].replace_range(start..end, "");
            self.col -= 1;
        } else if self.row > 0 {
            let cur = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.cur_len();
            self.lines[self.row].push_str(&cur);
        }
    }

    fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.cur_len();
        }
    }

    fn move_right(&mut self) {
        if self.col < self.cur_len() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn move_up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(self.cur_len());
        }
    }

    fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.cur_len());
        }
    }

    /// The assembled message with trailing blank lines trimmed.
    fn message(&self) -> String {
        self.lines.join("\n").trim_end().to_string()
    }

    /// Run the commit. Returns a close callback on success (so the editor pops),
    /// or `None` to stay open (empty message / write error).
    fn confirm(&self, cx: &mut Context) -> Option<Callback> {
        let msg = self.message();
        if msg.trim().is_empty() {
            cx.editor.set_status("aborted: empty commit message");
            return None;
        }
        let tmp = std::env::temp_dir().join(format!("zemacs-COMMIT_EDITMSG-{}", std::process::id()));
        if let Err(e) = std::fs::write(&tmp, &msg) {
            cx.editor.set_error(format!("commit: temp write failed: {e}"));
            return None;
        }
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.repo_dir).arg("commit");
        if self.amend {
            cmd.arg("--amend");
        }
        cmd.arg("-F").arg(&tmp);
        let out = cmd.output();
        let _ = std::fs::remove_file(&tmp);
        match out {
            Ok(o) if o.status.success() => {
                let summary = String::from_utf8_lossy(&o.stdout);
                let first = summary.lines().next().unwrap_or("committed");
                cx.editor.set_status(format!("commit: {}", first.trim()));
            }
            Ok(o) => {
                cx.editor.set_error(format!(
                    "git commit: {}",
                    condense(&String::from_utf8_lossy(&o.stderr))
                ));
                return None;
            }
            Err(e) => {
                cx.editor.set_error(format!("git commit: {e}"));
                return None;
            }
        }
        schedule_status_refresh(cx);
        Some(Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        }))
    }
}

impl Component for MagitCommit {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // `Ctrl-c Ctrl-c` confirms (two presses); any other key resets the chord.
        if let ctrl!('c') = key {
            if self.pending_confirm {
                self.pending_confirm = false;
                if let Some(cb) = self.confirm(cx) {
                    return EventResult::Consumed(Some(cb));
                }
            } else {
                self.pending_confirm = true;
                cx.editor
                    .set_status("press Ctrl-c again to commit (Esc to cancel)");
            }
            return EventResult::Consumed(None);
        }
        self.pending_confirm = false;

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        match key {
            key!(Esc) => return EventResult::Consumed(Some(close)),
            key!(Enter) => self.newline(),
            key!(Backspace) => self.backspace(),
            key!(Left) | ctrl!('b') => self.move_left(),
            key!(Right) | ctrl!('f') => self.move_right(),
            key!(Up) | ctrl!('p') => self.move_up(),
            key!(Down) | ctrl!('n') => self.move_down(),
            key!(Home) | ctrl!('a') => self.col = 0,
            key!(End) | ctrl!('e') => self.col = self.cur_len(),
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
            } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                self.insert_char(c)
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let cursor_style = theme.get("ui.cursor");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 4 {
            return;
        }

        let title = if self.amend {
            " Amend commit"
        } else {
            " Commit message"
        };
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        let hint = "Ctrl-c Ctrl-c commit   Esc cancel";
        if (title.len() + hint.len() + 3) < area.width as usize {
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

        // Keep the cursor row inside the viewport.
        if self.row < self.scroll {
            self.scroll = self.row;
        } else if self.row >= self.scroll + self.viewport {
            self.scroll = self.row - self.viewport + 1;
        }

        for (offset, line) in self
            .lines
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            surface.set_stringn(area.x, y, line, area.width as usize, text_style);
            if offset == self.row {
                // Draw a block cursor over the character at the cursor column.
                let cx_col = area.x + self.col as u16;
                if cx_col < area.x + area.width {
                    surface.set_style(Rect::new(cx_col, y, 1, 1), cursor_style);
                }
            }
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit-commit")
    }
}

/// A scrollable commit-log sub-view (`git log --oneline --decorate`).
///
/// Opened from the status buffer with `l`. `j`/`k`/arrows move the selection,
/// `g`/`G` jump to top/bottom, `Enter`/`d` open the selected commit's diff
/// ([`MagitShow`]), `q`/`Esc` return to the status buffer.
pub struct MagitLog {
    repo_dir: PathBuf,
    entries: Vec<LogEntry>,
    selected: usize,
    scroll: usize,
    viewport: usize,
}

impl MagitLog {
    fn new(repo_dir: PathBuf) -> Self {
        let out = git_output(
            &repo_dir,
            &["log", "--oneline", "--decorate", "-n", "200"],
        )
        .unwrap_or_default();
        MagitLog {
            repo_dir,
            entries: parse_log(&out),
            selected: 0,
            scroll: 0,
            viewport: 1,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Open the selected commit's diff in a [`MagitShow`] viewer.
    fn show_callback(&self) -> Option<Callback> {
        let sha = self.entries.get(self.selected)?.sha.clone();
        let repo_dir = self.repo_dir.clone();
        Some(Box::new(move |compositor: &mut Compositor, _cx| {
            compositor.push(Box::new(MagitShow::new(repo_dir.clone(), &sha)));
        }))
    }
}

impl Component for MagitLog {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('k') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!('g') | key!(Home) => self.selected = 0,
            key!('G') | key!(End) => self.selected = self.entries.len().saturating_sub(1),
            key!(Enter) | key!('d') => {
                if let Some(cb) = self.show_callback() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let sha_style = theme.get("constant.numeric");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = " Magit log";
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        let hint = "j/k move  Enter/d show diff  q back";
        if (title.len() + hint.len() + 3) < area.width as usize {
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

        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected - self.viewport + 1;
        }

        if self.entries.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "no commits",
                area.width as usize,
                info_style,
            );
            return;
        }

        for (offset, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            if offset == self.selected {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
            }
            let style = if offset == self.selected {
                sel_style
            } else {
                sha_style
            };
            surface.set_stringn(area.x, y, &format!("  {}", entry.sha), area.width as usize, style);
            let body_x = area.x + 2 + entry.sha.chars().count() as u16 + 1;
            if body_x < area.x + area.width {
                let style = if offset == self.selected {
                    sel_style
                } else {
                    text_style
                };
                surface.set_stringn(
                    body_x,
                    y,
                    &entry.summary,
                    (area.x + area.width - body_x) as usize,
                    style,
                );
            }
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit-log")
    }
}

/// A scrollable viewer for a single commit's diff (`git show <sha>`).
///
/// Pushed on top of [`MagitLog`]. `j`/`k`/arrows scroll a line, PageUp/PageDown
/// (`Ctrl-u`/`Ctrl-d`) a screenful, `g`/`G` jump to top/bottom, `q`/`Esc`
/// return to the log.
pub struct MagitShow {
    title: String,
    lines: Vec<String>,
    scroll: usize,
    viewport: usize,
}

impl MagitShow {
    fn new(repo_dir: PathBuf, sha: &str) -> Self {
        let out = git_output(&repo_dir, &["show", "--stat", "-p", sha]).unwrap_or_default();
        let lines: Vec<String> = out.lines().map(str::to_string).collect();
        MagitShow {
            title: format!(" {sha}"),
            lines,
            scroll: 0,
            viewport: 1,
        }
    }

    fn max_scroll(&self) -> usize {
        self.lines.len().saturating_sub(self.viewport)
    }

    fn scroll_by(&mut self, delta: isize) {
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, self.max_scroll() as isize) as usize;
    }
}

impl Component for MagitShow {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        let page = self.viewport.max(1) as isize;
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.scroll_by(1),
            key!('k') | key!(Up) | ctrl!('p') => self.scroll_by(-1),
            key!(PageDown) | ctrl!('d') | ctrl!('f') => self.scroll_by(page),
            key!(PageUp) | ctrl!('u') | ctrl!('b') => self.scroll_by(-page),
            key!('g') | key!(Home) => self.scroll = 0,
            key!('G') | key!(End) => self.scroll = self.max_scroll(),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let plus_style = theme.get("diff.plus");
        let minus_style = theme.get("diff.minus");
        let meta_style = theme.get("ui.text.focus");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        surface.set_stringn(
            area.x,
            area.y,
            &self.title,
            area.width as usize,
            header_style,
        );
        let hint = "j/k scroll  q back";
        if (self.title.len() + hint.len() + 3) < area.width as usize {
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
        self.scroll = self.scroll.min(self.max_scroll());

        for (offset, line) in self
            .lines
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let style = if line.starts_with("+++") || line.starts_with("---") {
                meta_style
            } else if line.starts_with('+') {
                plus_style
            } else if line.starts_with('-') {
                minus_style
            } else if line.starts_with("commit ")
                || line.starts_with("diff ")
                || line.starts_with("@@")
            {
                meta_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, line, area.width as usize, style);
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit-show")
    }
}

/// A branch menu sub-view, opened from the status buffer with `b`.
///
/// Lists local branches (`git branch`), the current one marked. `j`/`k`/arrows
/// move, `Enter` checks out the selected branch (`git checkout <b>`), `n` starts
/// creating a new branch — type a name then `Enter` runs `git checkout -b
/// <name>` — and `q`/`Esc` go back. After a successful checkout/create the menu
/// pops and the buried [`MagitStatus`] is refreshed.
pub struct MagitBranch {
    repo_dir: PathBuf,
    entries: Vec<BranchEntry>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    /// `Some(name)` while typing a new branch name; `None` in list mode.
    creating: Option<String>,
}

impl MagitBranch {
    fn new(repo_dir: PathBuf) -> Self {
        let out = git_output(&repo_dir, &["branch", "--no-color"]).unwrap_or_default();
        let entries = parse_branches(&out);
        let selected = entries.iter().position(|b| b.current).unwrap_or(0);
        MagitBranch {
            repo_dir,
            entries,
            selected,
            scroll: 0,
            viewport: 1,
            creating: None,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Run `git checkout …`; on success refresh the buried status and return a
    /// pop callback, otherwise surface the error and stay.
    fn run_checkout(&self, cx: &mut Context, args: &[&str], label: String) -> Option<Callback> {
        match git_run(&self.repo_dir, args) {
            Ok(()) => {
                cx.editor.set_status(label);
                schedule_status_refresh(cx);
                Some(Box::new(|compositor: &mut Compositor, _cx| {
                    compositor.pop();
                }))
            }
            Err(e) => {
                cx.editor.set_error(format!("git checkout: {e}"));
                None
            }
        }
    }
}

impl Component for MagitBranch {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // Branch-name input mode.
        if let Some(name) = self.creating.as_mut() {
            match key {
                key!(Esc) => self.creating = None,
                key!(Enter) => {
                    let name = name.trim().to_string();
                    if name.is_empty() {
                        cx.editor.set_status("branch name is empty");
                    } else if let Some(cb) =
                        self.run_checkout(cx, &["checkout", "-b", &name], format!("created {name}"))
                    {
                        return EventResult::Consumed(Some(cb));
                    } else {
                        self.creating = None;
                    }
                }
                key!(Backspace) => {
                    name.pop();
                }
                KeyEvent {
                    code: KeyCode::Char(c),
                    modifiers,
                } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                    name.push(c);
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
            key!('n') | key!('c') => {
                self.creating = Some(String::new());
                cx.editor
                    .set_status("new branch name (Enter to create, Esc to cancel)");
            }
            key!(Enter) => {
                if let Some(b) = self.entries.get(self.selected) {
                    if b.current {
                        cx.editor.set_status(format!("already on {}", b.name));
                    } else {
                        let name = b.name.clone();
                        if let Some(cb) = self.run_checkout(
                            cx,
                            &["checkout", &name],
                            format!("checked out {name}"),
                        ) {
                            return EventResult::Consumed(Some(cb));
                        }
                    }
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let cur_style = theme.get("diff.plus");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = " Branches";
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        let hint = "j/k move  Enter checkout  n new  q back";
        if (title.len() + hint.len() + 3) < area.width as usize {
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

        if let Some(name) = &self.creating {
            surface.set_stringn(
                area.x,
                body_y,
                &format!("new branch: {name}_"),
                area.width as usize,
                text_style,
            );
            return;
        }

        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected - self.viewport + 1;
        }

        if self.entries.is_empty() {
            surface.set_stringn(area.x, body_y, "no branches", area.width as usize, info_style);
            return;
        }

        for (offset, b) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            if offset == self.selected {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
            }
            let marker = if b.current { "* " } else { "  " };
            let style = if offset == self.selected {
                sel_style
            } else if b.current {
                cur_style
            } else {
                text_style
            };
            surface.set_stringn(
                area.x,
                y,
                &format!("{marker}{}", b.name),
                area.width as usize,
                style,
            );
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit-branch")
    }
}

/// A stash menu sub-view, opened from the status buffer with `z`.
///
/// Lists stash entries (`git stash list`). `s` pushes a new stash (type an
/// optional message then `Enter`), `p` pops the latest, `a` applies the selected
/// entry, `D` drops it; `j`/`k`/arrows move and `q`/`Esc` go back. After every
/// mutation the list reloads in place and the buried [`MagitStatus`] is
/// refreshed.
pub struct MagitStash {
    repo_dir: PathBuf,
    entries: Vec<StashEntry>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    /// `Some(msg)` while typing a stash-push message; `None` in list mode.
    pushing: Option<String>,
}

impl MagitStash {
    fn new(repo_dir: PathBuf) -> Self {
        let mut view = MagitStash {
            repo_dir,
            entries: Vec::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            pushing: None,
        };
        view.reload();
        view
    }

    fn reload(&mut self) {
        let out = git_output(&self.repo_dir, &["stash", "list"]).unwrap_or_default();
        self.entries = parse_stash(&out);
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Run a stash mutation, reload the list, refresh the buried status and
    /// report the outcome. Stays open.
    fn run_stash(&mut self, cx: &mut Context, args: &[&str], label: &str) {
        match git_run(&self.repo_dir, args) {
            Ok(()) => cx.editor.set_status(format!("stash: {label}")),
            Err(e) => cx.editor.set_error(format!("git stash: {e}")),
        }
        self.reload();
        schedule_status_refresh(cx);
    }
}

impl Component for MagitStash {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // Stash-message input mode.
        if let Some(msg) = self.pushing.as_mut() {
            match key {
                key!(Esc) => self.pushing = None,
                key!(Enter) => {
                    let msg = msg.trim().to_string();
                    self.pushing = None;
                    if msg.is_empty() {
                        self.run_stash(cx, &["stash", "push"], "pushed");
                    } else {
                        self.run_stash(cx, &["stash", "push", "-m", &msg], "pushed");
                    }
                }
                key!(Backspace) => {
                    msg.pop();
                }
                KeyEvent {
                    code: KeyCode::Char(c),
                    modifiers,
                } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                    msg.push(c);
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
            key!('s') => {
                self.pushing = Some(String::new());
                cx.editor
                    .set_status("stash message (Enter to push, empty for none, Esc cancel)");
            }
            key!('p') => self.run_stash(cx, &["stash", "pop"], "popped"),
            key!('a') | key!(Enter) => {
                if let Some(e) = self.entries.get(self.selected) {
                    let reff = e.reff.clone();
                    self.run_stash(cx, &["stash", "apply", &reff], "applied");
                }
            }
            key!('D') => {
                if let Some(e) = self.entries.get(self.selected) {
                    let reff = e.reff.clone();
                    self.run_stash(cx, &["stash", "drop", &reff], "dropped");
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let ref_style = theme.get("constant.numeric");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = " Stashes";
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        let hint = "s push  p pop  a apply  D drop  q back";
        if (title.len() + hint.len() + 3) < area.width as usize {
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

        if let Some(msg) = &self.pushing {
            surface.set_stringn(
                area.x,
                body_y,
                &format!("stash message: {msg}_"),
                area.width as usize,
                text_style,
            );
            return;
        }

        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected - self.viewport + 1;
        }

        if self.entries.is_empty() {
            surface.set_stringn(area.x, body_y, "no stashes", area.width as usize, info_style);
            return;
        }

        for (offset, e) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            if offset == self.selected {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
            }
            let style = if offset == self.selected {
                sel_style
            } else {
                ref_style
            };
            surface.set_stringn(area.x, y, &format!("  {}", e.reff), area.width as usize, style);
            let body_x = area.x + 2 + e.reff.chars().count() as u16 + 1;
            if body_x < area.x + area.width {
                let style = if offset == self.selected {
                    sel_style
                } else {
                    text_style
                };
                surface.set_stringn(
                    body_x,
                    y,
                    &e.summary,
                    (area.x + area.width - body_x) as usize,
                    style,
                );
            }
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit-stash")
    }
}

/// Add BOLD to a style.
fn to_bold(style: zemacs_view::graphics::Style) -> zemacs_view::graphics::Style {
    style.add_modifier(zemacs_view::graphics::Modifier::BOLD)
}

/// Resolve the git work-tree root containing `start`.
fn git_repo_root(start: &Path) -> Option<PathBuf> {
    let dir = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };
    let out = git_output(&dir, &["rev-parse", "--show-toplevel"])?;
    let root = out.trim();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

/// The current branch, or a short detached-HEAD description.
fn git_head(repo: &Path) -> String {
    if let Some(branch) = git_output(repo, &["symbolic-ref", "--short", "HEAD"]) {
        let branch = branch.trim();
        if !branch.is_empty() {
            return branch.to_string();
        }
    }
    match git_output(repo, &["rev-parse", "--short", "HEAD"]) {
        Some(sha) if !sha.trim().is_empty() => format!("HEAD detached at {}", sha.trim()),
        _ => "(no commits yet)".to_string(),
    }
}

/// Run a mutating `git -C <dir> …`, returning `Ok(())` on success or the trimmed
/// stderr (falling back to stdout, then a generic message) on failure.
fn git_run(dir: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir);
    for a in args {
        cmd.arg(a);
    }
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if !stderr.is_empty() {
                Err(stderr)
            } else {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                Err(if stdout.is_empty() {
                    "command failed".to_string()
                } else {
                    stdout
                })
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Run a read-only `git -C <dir> …`, returning stdout on success.
fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(entries: &'a [StatusEntry], section: Section, path: &str) -> &'a StatusEntry {
        entries
            .iter()
            .find(|e| e.section == section && e.path == path)
            .unwrap_or_else(|| panic!("no {section:?} entry for {path}"))
    }

    #[test]
    fn classifies_untracked() {
        let entries = parse_status("?? new.txt\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].section, Section::Untracked);
        assert_eq!(entries[0].path, "new.txt");
    }

    #[test]
    fn staged_only() {
        let entries = parse_status("M  staged.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].section, Section::Staged);
        assert_eq!(entries[0].path, "staged.rs");
        assert_eq!(entries[0].x, 'M');
        assert_eq!(entries[0].y, ' ');
    }

    #[test]
    fn unstaged_only() {
        let entries = parse_status(" M work.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].section, Section::Unstaged);
        assert_eq!(entries[0].path, "work.rs");
    }

    #[test]
    fn both_staged_and_unstaged() {
        // `MM` ⇒ a staged modification plus a further unstaged modification.
        let entries = parse_status("MM both.rs\n");
        assert_eq!(entries.len(), 2);
        entry(&entries, Section::Staged, "both.rs");
        entry(&entries, Section::Unstaged, "both.rs");
    }

    #[test]
    fn added_then_modified() {
        let entries = parse_status("AM added.rs\n");
        assert_eq!(entries.len(), 2);
        let staged = entry(&entries, Section::Staged, "added.rs");
        assert_eq!(staged.x, 'A');
        entry(&entries, Section::Unstaged, "added.rs");
    }

    #[test]
    fn conflict_states() {
        for code in ["UU", "AA", "DD", "AU", "UA", "DU", "UD"] {
            let entries = parse_status(&format!("{code} conflict.rs\n"));
            assert_eq!(entries.len(), 1, "{code} should be a single conflict entry");
            assert_eq!(entries[0].section, Section::Conflict, "{code}");
            assert_eq!(entries[0].path, "conflict.rs");
        }
    }

    #[test]
    fn rename_uses_new_path() {
        let entries = parse_status("R  old.rs -> new.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].section, Section::Staged);
        assert_eq!(entries[0].path, "new.rs");
    }

    #[test]
    fn mixed_output_all_sections() {
        let porcelain = "\
?? untracked.txt
 M unstaged.rs
M  staged.rs
UU conflict.rs
MM both.rs
";
        let entries = parse_status(porcelain);
        // untracked + unstaged + staged + conflict + (staged & unstaged for MM)
        assert_eq!(entries.len(), 6);
        entry(&entries, Section::Untracked, "untracked.txt");
        entry(&entries, Section::Unstaged, "unstaged.rs");
        entry(&entries, Section::Staged, "staged.rs");
        entry(&entries, Section::Conflict, "conflict.rs");
        entry(&entries, Section::Staged, "both.rs");
        entry(&entries, Section::Unstaged, "both.rs");
    }

    #[test]
    fn ignores_blank_and_short_lines() {
        let entries = parse_status("\n\nM  ok.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "ok.rs");
    }

    #[test]
    fn parse_log_splits_sha_and_summary() {
        let out = "abc1234 feat: do a thing\ndef5678 fix: another\n";
        let log = parse_log(out);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].sha, "abc1234");
        assert_eq!(log[0].summary, "feat: do a thing");
        assert_eq!(log[1].sha, "def5678");
        assert_eq!(log[1].summary, "fix: another");
    }

    #[test]
    fn parse_log_keeps_decorations_in_summary() {
        let out = "deadbee (HEAD -> main, origin/main) release: v1\n";
        let log = parse_log(out);
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].sha, "deadbee");
        assert_eq!(log[0].summary, "(HEAD -> main, origin/main) release: v1");
    }

    #[test]
    fn parse_log_handles_sha_only_and_blanks() {
        let log = parse_log("\nabc1234\n\n");
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].sha, "abc1234");
        assert_eq!(log[0].summary, "");
    }

    #[test]
    fn parse_log_empty() {
        assert!(parse_log("").is_empty());
    }

    #[test]
    fn ahead_behind_tab_separated() {
        // `--left-right --count @{u}...HEAD` prints "<behind>\t<ahead>".
        assert_eq!(parse_ahead_behind("3\t5\n"), Some((3, 5)));
        assert_eq!(parse_ahead_behind("0 0"), Some((0, 0)));
    }

    #[test]
    fn ahead_behind_rejects_garbage() {
        assert_eq!(parse_ahead_behind(""), None);
        assert_eq!(parse_ahead_behind("nope"), None);
        assert_eq!(parse_ahead_behind("1"), None);
    }

    const TWO_HUNK_DIFF: &str = "\
diff --git a/foo.rs b/foo.rs
index 1111111..2222222 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,3 @@
 fn a() {}
-old line
+new line
 fn b() {}
@@ -10,2 +10,3 @@
 tail
+added
 end
";

    #[test]
    fn parse_diff_hunks_splits_header_and_hunks() {
        let (header, hunks) = parse_diff_hunks(TWO_HUNK_DIFF);
        // Four header lines precede the first @@.
        assert_eq!(header.len(), 4);
        assert_eq!(header[0], "diff --git a/foo.rs b/foo.rs");
        assert_eq!(header[3], "+++ b/foo.rs");
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].header, "@@ -1,3 +1,3 @@");
        // body: context, -, +, context.
        assert_eq!(hunks[0].body, vec![" fn a() {}", "-old line", "+new line", " fn b() {}"]);
        assert_eq!(hunks[1].header, "@@ -10,2 +10,3 @@");
        assert_eq!(hunks[1].body, vec![" tail", "+added", " end"]);
    }

    #[test]
    fn parse_diff_hunks_empty_and_no_hunks() {
        let (header, hunks) = parse_diff_hunks("");
        assert!(header.is_empty());
        assert!(hunks.is_empty());

        // A header with no @@ (e.g. a pure mode/rename change) yields no hunks.
        let only_header = "diff --git a/x b/x\nold mode 100644\nnew mode 100755\n";
        let (header, hunks) = parse_diff_hunks(only_header);
        assert_eq!(header.len(), 3);
        assert!(hunks.is_empty());
    }

    #[test]
    fn hunk_patch_reassembles_appliable_shape() {
        let (header, hunks) = parse_diff_hunks(TWO_HUNK_DIFF);
        let patch = hunk_patch(&header, &hunks[0]);
        // The patch is the header + just the first hunk, newline-terminated.
        let expected = "\
diff --git a/foo.rs b/foo.rs
index 1111111..2222222 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,3 @@
 fn a() {}
-old line
+new line
 fn b() {}
";
        assert_eq!(patch, expected);
        assert!(patch.ends_with('\n'));
        // Round-trips: re-parsing the single-hunk patch gives one hunk.
        let (h2, hunks2) = parse_diff_hunks(&patch);
        assert_eq!(h2, header);
        assert_eq!(hunks2.len(), 1);
        assert_eq!(hunks2[0], hunks[0]);
    }

    #[test]
    fn parse_branches_marks_current_and_splits() {
        let out = "* main\n  feature/x\n  release\n";
        let branches = parse_branches(out);
        assert_eq!(branches.len(), 3);
        assert_eq!(branches[0].name, "main");
        assert!(branches[0].current);
        assert_eq!(branches[1].name, "feature/x");
        assert!(!branches[1].current);
        assert_eq!(branches[2].name, "release");
    }

    #[test]
    fn parse_branches_skips_detached_and_blanks() {
        let out = "* (HEAD detached at abc1234)\n  main\n\n";
        let branches = parse_branches(out);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "main");
        assert!(!branches[0].current);
    }

    #[test]
    fn parse_stash_splits_ref_and_summary() {
        let out = "\
stash@{0}: WIP on main: 1234567 fix things
stash@{1}: On feature: experiment
";
        let stashes = parse_stash(out);
        assert_eq!(stashes.len(), 2);
        assert_eq!(stashes[0].reff, "stash@{0}");
        assert_eq!(stashes[0].summary, "WIP on main: 1234567 fix things");
        assert_eq!(stashes[1].reff, "stash@{1}");
        assert_eq!(stashes[1].summary, "On feature: experiment");
    }

    #[test]
    fn parse_stash_empty() {
        assert!(parse_stash("").is_empty());
        assert!(parse_stash("\n\n").is_empty());
    }
}
