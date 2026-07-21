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
//! `P` push, `F` fetch, `p` pull, `R` pick the remote those three target, `!`
//! (Emacs `vc-edit-next-command`) open the next git command for editing before
//! it runs, `l` open the commit log, `g` refresh, `q`/`Esc` close.
//!
//! Slice 2 adds remote operations, a proper multi-line commit-message editor
//! ([`MagitCommit`], committed via `git commit -F <tempfile>` so multi-line
//! messages and quoting are handled safely, with Emacs's Log Edit comment ring on
//! `M-p`/`M-n`/`M-r`/`M-s`), and a scrollable commit log
//! ([`MagitLog`]) with a per-commit diff viewer ([`MagitShow`]). The ahead/behind
//! counts vs the upstream are shown in the header when an upstream is configured.
//!
//! A forge surface ([`MagitForge`], opened with `N`) lists GitHub/GitLab issues
//! and pull requests via the `gh` CLI and ports the Spacemacs `SPC m` topic
//! commands — edit labels, edit a local personal note, copy the topic URL as a
//! kill, toggle a PR's draft state — with a per-topic post list ([`MagitForgeTopic`])
//! where `D` deletes a reply comment.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use tui::buffer::Buffer as Surface;
use zmax_view::input::KeyEvent;
use zmax_view::keyboard::{KeyCode, KeyModifiers};
use zmax_view::{editor::Action, graphics::Rect};

use crate::{
    alt,
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

/// A configured remote as listed by `git remote -v`: its name and its fetch URL.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RemoteEntry {
    pub name: String,
    pub url: String,
}

/// Parse `git remote -v` (`<name>\t<url> (fetch|push)`) into [`RemoteEntry`]s,
/// one per remote. Pure and unit-tested. Only the `(fetch)` lines are kept, so a
/// remote with different fetch and push URLs still appears once.
pub fn parse_remotes(out: &str) -> Vec<RemoteEntry> {
    let mut entries = Vec::new();
    for line in out.lines() {
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        if !rest.ends_with("(fetch)") {
            continue;
        }
        let url = rest.trim_end_matches("(fetch)").trim();
        entries.push(RemoteEntry {
            name: name.trim().to_string(),
            url: url.to_string(),
        });
    }
    entries
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

/// The action applied to a single commit in an interactive-rebase todo list.
///
/// Maps 1:1 onto git's todo verbs (see [`RebaseAction::verb`]). Reword/edit are
/// intentionally not modelled here — they require git to stop mid-rebase for
/// interactive message/commit editing, which this non-interactive driver does
/// not support (it overrides `GIT_EDITOR`/`GIT_SEQUENCE_EDITOR` to never block).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RebaseAction {
    /// Keep the commit as-is.
    Pick,
    /// Meld into the previous commit, combining both messages.
    Squash,
    /// Meld into the previous commit, discarding this commit's message.
    Fixup,
    /// Remove the commit entirely.
    Drop,
}

impl RebaseAction {
    /// The git todo verb for this action (`pick`/`squash`/`fixup`/`drop`).
    pub fn verb(self) -> &'static str {
        match self {
            RebaseAction::Pick => "pick",
            RebaseAction::Squash => "squash",
            RebaseAction::Fixup => "fixup",
            RebaseAction::Drop => "drop",
        }
    }
}

/// One row of an interactive-rebase todo list: the action to apply, the commit's
/// abbreviated SHA and its subject line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RebaseRow {
    pub action: RebaseAction,
    pub sha: String,
    pub subject: String,
}

/// Parse `git log --reverse --format=%h %s <base>..HEAD` output into todo rows,
/// each defaulting to [`RebaseAction::Pick`]. Pure and unit-tested.
///
/// Each non-empty line is `<sha> <subject…>`; the SHA is the first
/// whitespace-delimited token and the subject the remainder (which may be empty
/// for an empty-message commit). Blank lines are skipped. The rows come back in
/// git's todo order (oldest first), matching the `--reverse` flag.
pub fn parse_rebase_todo(out: &str) -> Vec<RebaseRow> {
    let mut rows = Vec::new();
    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let (sha, subject) = match line.split_once(char::is_whitespace) {
            Some((sha, rest)) => (sha.to_string(), rest.trim_start().to_string()),
            None => (line.to_string(), String::new()),
        };
        rows.push(RebaseRow {
            action: RebaseAction::Pick,
            sha,
            subject,
        });
    }
    rows
}

/// Serialize todo rows to a git rebase-todo file body, one `<verb> <sha>
/// <subject>` line per row in display order (including `drop` rows). Pure and
/// unit-tested; the result is fed to `git rebase -i` via `GIT_SEQUENCE_EDITOR`.
pub fn render_todo(rows: &[RebaseRow]) -> String {
    let mut out = String::new();
    for row in rows {
        out.push_str(row.action.verb());
        out.push(' ');
        out.push_str(&row.sha);
        out.push(' ');
        out.push_str(&row.subject);
        out.push('\n');
    }
    out
}

/// In-progress rebase state, surfaced in the [`MagitStatus`] header and gating
/// the continue/abort keys.
#[derive(Clone, PartialEq, Eq, Debug)]
struct RebaseProgress {
    /// Short description of the commit being rebased onto (from the state dir).
    onto: String,
    /// Number of todo steps completed so far.
    done: usize,
    /// Total number of todo steps.
    total: usize,
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
    /// `Some(..)` while an interactive rebase is in progress (detected from the
    /// git state dir); enables the continue/abort keys and the header notice.
    rebase: Option<RebaseProgress>,
    /// Marked files, by repo-relative path (Emacs `vc-dir-mark` and friends).
    /// When this is non-empty, `s` / `u` / `X` act on the whole marked set
    /// instead of the row under the cursor — the VC-directory contract.
    marked: HashSet<String>,
    /// `Some(typed-so-far)` while `%` (`vc-dir-mark-by-regexp`) is reading its
    /// regexp on the title line.
    mark_regexp: Option<String>,
    /// Armed by `!` (Emacs `vc-edit-next-command`, `C-x v !`): the *next* git
    /// command this buffer would run is presented for editing instead of being
    /// run. One-shot — any other command drops it, exactly as the Emacs prefix
    /// removes itself from `post-command-hook`.
    edit_next: bool,
    /// `Some(command-line)` while the armed `!` is reading the edited command on
    /// the title line. Pre-filled with the command git was about to run.
    edit_command: Option<String>,
    /// The remote push/fetch/pull target, chosen with `R`; `None` leaves the
    /// argv bare so git picks its configured default.
    remote: Option<String>,
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
            rebase: None,
            marked: HashSet::new(),
            mark_regexp: None,
            edit_next: false,
            edit_command: None,
            remote: None,
        };
        view.refresh();
        Some(view)
    }

    /// Re-read `git status` + the current branch and rebuild the section list,
    /// clamping the selection to the new entry count.
    fn refresh(&mut self) {
        self.head = git_head(&self.repo_dir);
        self.rebase = detect_rebase(&self.repo_dir);
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
        // A file that no longer has a change cannot stay marked (it has left the
        // buffer); Emacs's vc-dir drops such marks on refresh too.
        let live: HashSet<String> = self.entries.iter().map(|e| e.path.clone()).collect();
        self.marked.retain(|p| live.contains(p));
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

    // --- vc-dir marks (Emacs vc-dir-mark and friends) -----------------------
    //
    // A mark is a repo-relative path. Every file operation below acts on the
    // marked set when it is non-empty, and on the row under the cursor when it
    // is not — exactly the rule `vc-next-action` follows in a `vc-dir` buffer.

    /// The paths the next file operation acts on: the marked files, or the file
    /// under the cursor when nothing is marked.
    fn acted_on(&self) -> Vec<String> {
        if !self.marked.is_empty() {
            // Report them in buffer order, not hash order, so messages are stable.
            return self
                .entries
                .iter()
                .filter(|e| self.marked.contains(&e.path))
                .map(|e| e.path.clone())
                .collect::<Vec<_>>()
                .into_iter()
                .fold(Vec::new(), |mut acc, p| {
                    // One entry per path: a `MM` file appears in two sections.
                    if !acc.contains(&p) {
                        acc.push(p);
                    }
                    acc
                });
        }
        self.selected_entry()
            .map(|e| vec![e.path.clone()])
            .unwrap_or_default()
    }

    /// Is the file at row `i` marked?
    fn is_marked(&self, i: usize) -> bool {
        self.entries
            .get(i)
            .is_some_and(|e| self.marked.contains(&e.path))
    }

    /// Emacs `vc-dir-mark` (`m`): mark the file under the cursor and advance to
    /// the next one, so a run of files can be marked by holding `m`.
    pub(crate) fn mark_file(&mut self, cx: &mut Context) {
        let Some(path) = self.selected_entry().map(|e| e.path.clone()) else {
            cx.editor.set_status("nothing to mark");
            return;
        };
        self.marked.insert(path.clone());
        self.move_selection(1);
        cx.editor
            .set_status(format!("marked {path} ({} marked)", self.marked.len()));
    }

    /// Emacs `vc-dir-unmark` (`DEL`): drop the mark on the file under the cursor.
    fn unmark_file(&mut self, cx: &mut Context) {
        let Some(path) = self.selected_entry().map(|e| e.path.clone()) else {
            return;
        };
        if self.marked.remove(&path) {
            cx.editor
                .set_status(format!("unmarked {path} ({} marked)", self.marked.len()));
        } else if !self.marked.is_empty() {
            // Emacs's `M` on a buffer that has marks clears them; `DEL` on an
            // unmarked file is a no-op, so say what is still marked.
            cx.editor
                .set_status(format!("{} file(s) still marked", self.marked.len()));
        }
    }

    /// Emacs `vc-dir-mark-all-files` (`M`): mark every file that has the same VC
    /// state as the one under the cursor (its section — unstaged, staged,
    /// untracked or conflicted). With nothing under the cursor, mark everything.
    /// Pressing it when marks already exist clears them, as Emacs's `M` does.
    pub(crate) fn mark_all_files(&mut self, cx: &mut Context) {
        if !self.marked.is_empty() {
            self.marked.clear();
            cx.editor.set_status("unmarked all files");
            return;
        }
        let section = self.selected_entry().map(|e| e.section);
        let paths: Vec<String> = self
            .entries
            .iter()
            .filter(|e| section.is_none_or(|s| e.section == s))
            .map(|e| e.path.clone())
            .collect();
        let what = match section {
            Some(s) => s.title(),
            None => "files",
        };
        self.marked.extend(paths);
        cx.editor
            .set_status(format!("marked {} {what}", self.marked.len()));
    }

    /// Emacs `vc-dir-mark-registered-files` (`* r`): mark every *registered*
    /// (git-tracked) file with a change — everything except the untracked ones.
    pub(crate) fn mark_registered_files(&mut self, cx: &mut Context) {
        let paths: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.section != Section::Untracked)
            .map(|e| e.path.clone())
            .collect();
        if paths.is_empty() {
            cx.editor.set_status("no registered files with changes");
            return;
        }
        self.marked.extend(paths);
        cx.editor
            .set_status(format!("marked {} registered file(s)", self.marked.len()));
    }

    /// Open the `%` (`vc-dir-mark-by-regexp`) prompt: the next keys typed are the
    /// regexp, and Enter marks every file matching it.
    pub(crate) fn begin_mark_by_regexp(&mut self, cx: &mut Context) {
        self.mark_regexp = Some(String::new());
        cx.editor
            .set_status("mark files matching regexp (Enter to apply, Esc to cancel)");
    }

    /// Emacs `vc-dir-mark-by-regexp` (`% m`): mark every file whose path matches
    /// `re`. An unparsable regexp is reported, not swallowed.
    fn mark_by_regexp(&mut self, re: &str, cx: &mut Context) {
        let re = match regex::Regex::new(re) {
            Ok(re) => re,
            Err(e) => {
                cx.editor.set_error(format!("bad regexp: {e}"));
                return;
            }
        };
        let paths: Vec<String> = self
            .entries
            .iter()
            .filter(|e| re.is_match(&e.path))
            .map(|e| e.path.clone())
            .collect();
        if paths.is_empty() {
            cx.editor.set_status("no file matches that regexp");
            return;
        }
        let n = paths.len();
        self.marked.extend(paths);
        cx.editor
            .set_status(format!("marked {n} file(s) ({} marked)", self.marked.len()));
    }

    /// Stage the acted-on files (`git add -- <path>…`), then refresh.
    fn stage_selected(&mut self, cx: &mut Context) {
        let paths = self.acted_on();
        if paths.is_empty() {
            return;
        }
        let mut args = vec!["add", "--"];
        args.extend(paths.iter().map(String::as_str));
        match self.run_git(&args) {
            Ok(()) => cx.editor.set_status(format!("staged {}", listing(&paths))),
            Err(e) => cx.editor.set_error(format!("git add: {e}")),
        }
        self.marked.clear();
        self.refresh();
    }

    /// Unstage the acted-on files (`git reset -q HEAD -- <path>…`), then refresh.
    fn unstage_selected(&mut self, cx: &mut Context) {
        let paths = self.acted_on();
        if paths.is_empty() {
            return;
        }
        let mut args = vec!["reset", "-q", "HEAD", "--"];
        args.extend(paths.iter().map(String::as_str));
        match self.run_git(&args) {
            Ok(()) => cx
                .editor
                .set_status(format!("unstaged {}", listing(&paths))),
            Err(e) => cx.editor.set_error(format!("git reset: {e}")),
        }
        self.marked.clear();
        self.refresh();
    }

    /// Discard the acted-on files' worktree changes: `git checkout -- <path>`
    /// for a tracked file, or delete it outright for an untracked one. Caller
    /// gates this behind a confirmation.
    fn discard_selected(&mut self, cx: &mut Context) {
        let paths = self.acted_on();
        if paths.is_empty() {
            return;
        }
        let mut failed = Vec::new();
        for path in &paths {
            let untracked = self
                .entries
                .iter()
                .any(|e| e.path == *path && e.section == Section::Untracked);
            let result = if untracked {
                std::fs::remove_file(self.repo_dir.join(path)).map_err(|e| e.to_string())
            } else {
                self.run_git(&["checkout", "--", path])
            };
            if let Err(e) = result {
                failed.push(format!("{path}: {e}"));
            }
        }
        if failed.is_empty() {
            // Working-tree bytes reverted to HEAD: reload so the buffers and
            // their gutters drop the discarded changes.
            crate::commands::reload_all_open_docs(cx.editor);
            cx.editor
                .set_status(format!("discarded {}", listing(&paths)));
        } else {
            cx.editor
                .set_error(format!("discard failed: {}", failed.join("; ")));
        }
        self.marked.clear();
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
            // A pull can fast-forward HEAD and rewrite tracked files; push/fetch
            // leave the working tree. Reload buffers only when the tree moved.
            if args.first() == Some(&"pull") {
                crate::commands::reload_all_open_docs(cx.editor);
            }
            cx.editor.set_status(format!("{label}: {msg}"));
        } else {
            cx.editor.set_error(format!("{label}: {msg}"));
        }
        self.refresh();
    }

    /// The argv for a remote operation, aimed at the remote picked with `R` when
    /// there is one (`git push origin`) and otherwise left bare so git uses the
    /// branch's configured default (`git push`).
    fn remote_args(&self, op: &str) -> Vec<String> {
        match &self.remote {
            Some(r) => vec![op.to_string(), r.clone()],
            None => vec![op.to_string()],
        }
    }

    /// Run push/fetch/pull against the selected remote.
    fn remote_op_named(&mut self, cx: &mut Context, op: &str) {
        let args = self.remote_args(op);
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        self.remote_op(cx, op, &argv);
    }

    /// Record the remote chosen in the [`MagitRemote`] picker; subsequent
    /// push/fetch/pull name it explicitly.
    pub(crate) fn set_remote(&mut self, name: String) {
        self.remote = Some(name);
    }

    /// Build the remote callback: open the [`MagitRemote`] picker.
    fn remote_callback(&self) -> Callback {
        let repo_dir = self.repo_dir.clone();
        let current = self.remote.clone();
        Box::new(move |compositor: &mut Compositor, _cx: &mut Context| {
            compositor.push(Box::new(MagitRemote::new(
                repo_dir.clone(),
                current.clone(),
            )));
        })
    }

    // --- vc-edit-next-command ----------------------------------------------
    //
    // Emacs's `C-x v !` is a prefix that installs a one-shot filter on the shell
    // command VC is about to run: the next VC command is read back in the
    // minibuffer for editing, then run as edited. Anything else drops the
    // prefix. The same shape here: `!` arms `edit_next`, the next key that maps
    // to a git command fills the title-line prompt instead of running, and any
    // other key disarms.

    /// The git argv the key `key` would run, for the `!` prefix to present for
    /// editing. `None` for keys that run no git command (movement, sub-views,
    /// quit) — those drop the prefix, as Emacs's does when the following command
    /// isn't a VC command.
    fn pending_git_argv(&self, key: KeyEvent) -> Option<Vec<String>> {
        let owned = |args: &[&str]| args.iter().map(|a| a.to_string()).collect::<Vec<String>>();
        // The file operations act on the marked set (or the cursor row); a hunk
        // row stages through a patch on stdin instead and has no editable argv.
        let with_paths = |args: &[&str]| {
            if !matches!(self.selected_target(), Some(Target::File(_))) {
                return None;
            }
            let paths = self.acted_on();
            if paths.is_empty() {
                return None;
            }
            let mut v = owned(args);
            v.extend(paths);
            Some(v)
        };
        match key {
            key!('s') => with_paths(&["add", "--"]),
            key!('u') => with_paths(&["reset", "-q", "HEAD", "--"]),
            key!('X') => with_paths(&["checkout", "--"]),
            key!('S') => Some(owned(&["add", "-A"])),
            key!('U') => Some(owned(&["reset", "-q"])),
            key!('P') => Some(self.remote_args("push")),
            key!('F') => Some(self.remote_args("fetch")),
            key!('p') => Some(self.remote_args("pull")),
            key!('r') if self.rebase.is_some() => Some(owned(&["rebase", "--continue"])),
            key!('A') if self.rebase.is_some() => Some(owned(&["rebase", "--abort"])),
            _ => None,
        }
    }

    /// Run the command line the user edited at the `!` prompt. The line is split
    /// into words like a shell would (Emacs re-splits the edited minibuffer text
    /// with `split-string-and-unquote`) and a leading `git` is dropped, since we
    /// always exec git inside the repo.
    fn run_edited_command(&mut self, line: &str, cx: &mut Context) {
        let mut argv = split_argv(line);
        if argv.first().map(String::as_str) == Some("git") {
            argv.remove(0);
        }
        if argv.is_empty() {
            cx.editor.set_status("cancelled");
            return;
        }
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        let (ok, msg) = self.run_git_noninteractive(&args);
        let msg = if msg.is_empty() {
            if ok {
                "done".to_string()
            } else {
                "failed".to_string()
            }
        } else {
            msg
        };
        let label = argv.join(" ");
        if ok {
            // An arbitrary edited command can rewrite the working tree, so the
            // open buffers have to follow it.
            crate::commands::reload_all_open_docs(cx.editor);
            cx.editor.set_status(format!("git {label}: {msg}"));
        } else {
            cx.editor.set_error(format!("git {label}: {msg}"));
        }
        self.marked.clear();
        self.refresh();
    }

    /// Run a `git -C <repo> …` with `GIT_EDITOR=true` (so any commit-message
    /// prompt auto-accepts and never blocks), returning `(success, message)`
    /// with stdout+stderr condensed into one status-line-friendly string.
    fn run_git_noninteractive(&self, args: &[&str]) -> (bool, String) {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.repo_dir);
        for a in args {
            cmd.arg(a);
        }
        cmd.env("GIT_EDITOR", "true");
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

    /// `r` (when a rebase is in progress): `git rebase --continue`. Surfaces the
    /// outcome and refreshes — if it stops at the next conflict the buffer's
    /// conflict section and in-progress notice reflect that, so the
    /// resolve→continue loop keeps working.
    fn rebase_continue(&mut self, cx: &mut Context) {
        let (ok, msg) = self.run_git_noninteractive(&["rebase", "--continue"]);
        let msg = if msg.is_empty() {
            if ok {
                "continued".to_string()
            } else {
                "stopped".to_string()
            }
        } else {
            msg
        };
        if ok {
            // Rebase advanced HEAD and rewrote the working tree: reload buffers.
            crate::commands::reload_all_open_docs(cx.editor);
            cx.editor.set_status(format!("rebase: {msg}"));
        } else {
            cx.editor.set_error(format!("rebase: {msg}"));
        }
        self.refresh();
    }

    /// `A` (when a rebase is in progress): `git rebase --abort`, restoring the
    /// pre-rebase HEAD, then refresh.
    fn rebase_abort(&mut self, cx: &mut Context) {
        let (ok, msg) = self.run_git_noninteractive(&["rebase", "--abort"]);
        let msg = if msg.is_empty() {
            if ok {
                "aborted".to_string()
            } else {
                "abort failed".to_string()
            }
        } else {
            msg
        };
        if ok {
            // Abort restored the pre-rebase HEAD and working tree: reload buffers.
            crate::commands::reload_all_open_docs(cx.editor);
            cx.editor.set_status(format!("rebase: {msg}"));
        } else {
            cx.editor.set_error(format!("rebase: {msg}"));
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
        // Only shown once a remote has been picked with `R`; without one git
        // resolves push/fetch/pull against its own default.
        if let Some(remote) = &self.remote {
            rows.push(Row::Info(format!("Remote {remote}")));
        }
        if let Some(rb) = &self.rebase {
            rows.push(Row::Header(format!(
                "Rebasing onto {} ({}/{}) — r continue, A abort",
                rb.onto, rb.done, rb.total
            )));
        }
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

    /// Build the forge callback: open the [`MagitForge`] topic list (issues and
    /// pull requests). The Magit forge dispatch is bound to `N`.
    fn forge_callback(&self) -> Callback {
        let repo_dir = self.repo_dir.clone();
        Box::new(move |compositor: &mut Compositor, _cx: &mut Context| {
            compositor.push(Box::new(MagitForge::new(repo_dir.clone())));
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
                    cx.editor
                        .set_status("hunk is already staged (press u to unstage)");
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
                    cx.editor
                        .set_status("hunk is not staged (press s to stage)");
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
        let patch = match self
            .diffs
            .get(&key)
            .and_then(|fd| fd.hunks.get(hunk).map(|h| hunk_patch(&fd.header, h)))
        {
            Some(p) => p,
            None => {
                cx.editor.set_error("no hunk to apply (try g to refresh)");
                return;
            }
        };

        let tmp = std::env::temp_dir().join(format!(
            "zmax-magit-hunk-{}-{}.patch",
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

/// Name the files an operation touched: the path itself for one file, a count
/// plus the first few names for several (what the status line has room for).
fn listing(paths: &[String]) -> String {
    match paths {
        [] => "nothing".to_string(),
        [one] => one.clone(),
        _ => {
            let shown: Vec<&str> = paths.iter().take(3).map(String::as_str).collect();
            let rest = paths.len() - shown.len();
            if rest == 0 {
                format!("{} files ({})", paths.len(), shown.join(", "))
            } else {
                format!("{} files ({}, …)", paths.len(), shown.join(", "))
            }
        }
    }
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

        // `%` (vc-dir-mark-by-regexp) reads its regexp on the title line; while it
        // is open every key belongs to it.
        if self.mark_regexp.is_some() {
            match key {
                key!(Esc) | ctrl!('g') => {
                    self.mark_regexp = None;
                    cx.editor.set_status("cancelled");
                }
                key!(Enter) => {
                    let re = self.mark_regexp.take().unwrap_or_default();
                    if re.is_empty() {
                        cx.editor.set_status("cancelled");
                    } else {
                        self.mark_by_regexp(&re, cx);
                    }
                }
                key!(Backspace) => {
                    if let Some(buf) = &mut self.mark_regexp {
                        buf.pop();
                    }
                }
                KeyEvent {
                    code: KeyCode::Char(c),
                    modifiers,
                } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                    if let Some(buf) = &mut self.mark_regexp {
                        buf.push(c);
                    }
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        // The `!` prompt owns every key while the edited command line is open.
        if self.edit_command.is_some() {
            match key {
                key!(Esc) | ctrl!('g') => {
                    self.edit_command = None;
                    cx.editor.set_status("cancelled");
                }
                key!(Enter) => {
                    let line = self.edit_command.take().unwrap_or_default();
                    self.run_edited_command(&line, cx);
                }
                key!(Backspace) => {
                    if let Some(buf) = &mut self.edit_command {
                        buf.pop();
                    }
                }
                KeyEvent {
                    code: KeyCode::Char(c),
                    modifiers,
                } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                    if let Some(buf) = &mut self.edit_command {
                        buf.push(c);
                    }
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        // `!` (vc-edit-next-command) is armed: the next key that would run git
        // opens the command for editing instead, and any other key disarms.
        if self.edit_next {
            self.edit_next = false;
            if let Some(argv) = self.pending_git_argv(key) {
                self.edit_command = Some(format!("git {}", argv.join(" ")));
                cx.editor
                    .set_status("edit the git command (Enter to run, Esc to cancel)");
                return EventResult::Consumed(None);
            }
        }

        // Any key other than a confirming `X` cancels a pending discard.
        if key != key!('X') && self.pending_discard {
            self.pending_discard = false;
        }

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        match key {
            // ---- vc-dir marks ----
            key!('m') => self.mark_file(cx),
            key!('M') => self.mark_all_files(cx),
            key!(Backspace) => self.unmark_file(cx),
            key!('*') => self.mark_registered_files(cx),
            key!('%') => {
                self.mark_regexp = Some(String::new());
                cx.editor
                    .set_status("mark files matching regexp (Enter to apply, Esc to cancel)");
            }
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
            key!('N') => return EventResult::Consumed(Some(self.forge_callback())),
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
            key!('P') => self.remote_op_named(cx, "push"),
            key!('F') => self.remote_op_named(cx, "fetch"),
            key!('p') => self.remote_op_named(cx, "pull"),
            key!('R') => return EventResult::Consumed(Some(self.remote_callback())),
            key!('!') => {
                self.edit_next = true;
                cx.editor
                    .set_status("! : the next git command will be opened for editing");
            }
            key!('r') if self.rebase.is_some() => self.rebase_continue(cx),
            key!('A') if self.rebase.is_some() => self.rebase_abort(cx),
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
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
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

        // Title + key hint. While `%` is reading a regexp, the title line shows
        // the prompt instead (the mark applies on Enter).
        let title = " Magit status";
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        if let Some(buf) = &self.mark_regexp {
            let line = format!("Mark files matching regexp: {buf}_");
            surface.set_stringn(
                area.x + title.len() as u16 + 2,
                area.y,
                &line,
                area.width as usize,
                info_style,
            );
        } else if let Some(buf) = &self.edit_command {
            // `!` (vc-edit-next-command): the command git is about to run, open
            // for editing on the title line.
            let line = format!("Edit command: {buf}_");
            surface.set_stringn(
                area.x + title.len() as u16 + 2,
                area.y,
                &line,
                area.width as usize,
                info_style,
            );
        } else {
            let hint =
                "Tab expand  s stage  u unstage  X discard  m mark  M mark-all  % regexp  * registered  c commit  a amend  b branch  z stash  N forge  R remote  ! edit-cmd  l log  g refresh  q quit";
            if (title.len() + hint.len() + 3) < area.width as usize {
                surface.set_stringn(
                    area.x + area.width - hint.len() as u16 - 1,
                    area.y,
                    hint,
                    hint.len(),
                    info_style,
                );
            }
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
                    surface.set_stringn(
                        area.x,
                        y,
                        &format!("    {text}"),
                        area.width as usize,
                        info_style,
                    );
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
                    // The vc-dir mark column: `*` on a marked file.
                    let mark = if self.is_marked(*i) { '*' } else { ' ' };
                    let line = format!("{mark}{marker} {} {}", entry.code(), entry.path);
                    let style = if selected_block { sel_style } else { base };
                    surface.set_stringn(area.x, y, &line, area.width as usize, style);
                }
                Row::HunkHeader { text, .. } => {
                    if selected_block {
                        surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
                    }
                    let style = if selected_block {
                        sel_style
                    } else {
                        info_style
                    };
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

/// Split an edited command line into argv the way a shell splits simple input:
/// whitespace separates words, single or double quotes group one, and a
/// backslash escapes the next character. Used by the `!`
/// (`vc-edit-next-command`) prompt, where Emacs re-splits the edited minibuffer
/// text with `split-string-and-unquote`. Pure and unit-tested.
fn split_argv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // A word can be empty and still be a word (`''`), so track "started"
    // separately from the buffer's contents.
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                    started = true;
                }
            }
            '\'' | '"' if quote == Some(c) => quote = None,
            '\'' | '"' if quote.is_none() => {
                quote = Some(c);
                started = true;
            }
            c if c.is_whitespace() && quote.is_none() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// Byte offset of the `char_idx`-th character in `s` (or `s.len()` if past the
/// end), for editing a `String` by character position.
fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// `log-edit-maximum-comment-ring-size` — Emacs's cap on the comment ring.
const COMMENT_RING_SIZE: usize = 32;

/// `log-edit-comment-ring`: the commit messages entered so far, newest first, so
/// index `n` is Emacs's `(ring-ref log-edit-comment-ring n)`. Emacs keeps this for
/// the life of the Emacs session and never writes it to disk; this keeps it for
/// the life of the process, shared by every commit editor opened in it.
static COMMENT_RING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// `log-edit-last-comment-match`: the substring `M-r`/`M-s` fall back to when the
/// prompt is answered with an empty string.
static LAST_COMMENT_MATCH: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Snapshot of [`COMMENT_RING`] (newest first).
fn comment_ring() -> Vec<String> {
    COMMENT_RING.lock().map(|r| r.clone()).unwrap_or_default()
}

/// `log-edit-remember-comment`: push `comment` onto the front of the ring unless
/// it already *is* the newest entry, then drop anything past the ring size.
fn remember_comment(comment: &str) {
    let Ok(mut ring) = COMMENT_RING.lock() else {
        return;
    };
    if ring.first().map(String::as_str) == Some(comment) {
        return;
    }
    ring.insert(0, comment.to_string());
    ring.truncate(COMMENT_RING_SIZE);
}

/// A multi-line commit-message editor overlay.
///
/// Opened from the status buffer with `c` (fresh) or `a` (amend, pre-filled with
/// the last message). The user types a normal multi-line message; `Ctrl-c
/// Ctrl-c` (two presses) confirms and `Esc` cancels. On confirm the message is
/// written to a temp file and committed with `git commit -F <tempfile>` (plus
/// `--amend` when amending), so multi-line text and shell-special characters are
/// handled safely; the buried [`MagitStatus`] is then refreshed.
///
/// Emacs's Log Edit comment ring is here too: `M-p`/`M-n` cycle backward/forward
/// through the messages committed earlier in this session and `M-r`/`M-s` search
/// that ring backward/forward for a substring.
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
    /// The `*vc-diff*` / `*vc-log-files*` pane `C-c C-d` and `C-c C-f` pop up,
    /// shown under the message and scrolled with PageUp/PageDown. Emacs uses a
    /// separate window; this overlay has one, so it shows it inline.
    pane: Option<Pane>,
    /// `log-edit-comment-ring-index`: where the comment ring commands last landed,
    /// or `None` (Emacs's nil) before any of them ran.
    ring_index: Option<usize>,
    /// The open `M-r`/`M-s` prompt: `true` when searching backward (`M-r`), plus
    /// the substring typed so far.
    searching: Option<(bool, String)>,
}

/// The read-only pane `log-edit-show-diff` / `log-edit-show-files` displays.
struct Pane {
    title: String,
    lines: Vec<String>,
    scroll: usize,
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
            pane: None,
            ring_index: None,
            searching: None,
        }
    }

    /// The diff this commit will record: the staged changes, or `HEAD~..` when
    /// amending (the previous commit plus what is staged now) — what
    /// `log-edit-diff-function` shows.
    fn commit_diff(&self) -> String {
        let args: &[&str] = if self.amend {
            &["diff", "--cached", "HEAD~"]
        } else {
            &["diff", "--cached"]
        };
        git_output(&self.repo_dir, args).unwrap_or_default()
    }

    /// The files this commit will record (`log-edit-files`).
    fn commit_files(&self) -> Vec<String> {
        let args: &[&str] = if self.amend {
            &["diff", "--cached", "--name-only", "HEAD~"]
        } else {
            &["diff", "--cached", "--name-only"]
        };
        git_output(&self.repo_dir, args)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Emacs `log-edit-show-diff` (`C-c C-d`): show the diff of what is about to
    /// be committed.
    pub(crate) fn show_diff(&mut self, cx: &mut Context) {
        let diff = self.commit_diff();
        if diff.trim().is_empty() {
            cx.editor.set_status("nothing staged: no diff to show");
            return;
        }
        self.pane = Some(Pane {
            title: "*vc-diff*".to_string(),
            lines: diff.lines().map(str::to_string).collect(),
            scroll: 0,
        });
    }

    /// Emacs `log-edit-show-files` (`C-c C-f`): list the files to be committed.
    pub(crate) fn show_files(&mut self, cx: &mut Context) {
        let files = self.commit_files();
        if files.is_empty() {
            cx.editor.set_status("nothing staged: no files to commit");
            return;
        }
        self.pane = Some(Pane {
            title: format!("*vc-log-files* ({})", files.len()),
            lines: files,
            scroll: 0,
        });
    }

    /// Insert `text` (a block of lines) at the cursor, leaving point after it.
    fn insert_lines(&mut self, text: &[String]) {
        for (i, line) in text.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            for c in line.chars() {
                self.insert_char(c);
            }
        }
    }

    /// Emacs `log-edit-generate-changelog-from-diff` (`C-c C-w`): write the
    /// commit message from the diff itself — a ChangeLog-style line per changed
    /// file, naming the functions its hunks touched.
    pub(crate) fn generate_changelog_from_diff(&mut self, cx: &mut Context) {
        let diff = self.commit_diff();
        let entries = zmax_core::changelog::entries_from_diff(&diff);
        if entries.is_empty() {
            cx.editor
                .set_status("nothing staged: no changes to describe");
            return;
        }
        // The ChangeLog file lines are tab-indented; a commit message is not.
        let lines: Vec<String> = entries
            .iter()
            .map(|l| l.trim_start_matches('\t').trim_end().to_string())
            .collect();
        let n = lines.len();
        self.insert_lines(&lines);
        cx.editor
            .set_status(format!("inserted {n} ChangeLog entr(ies) from the diff"));
    }

    /// Emacs `log-edit-insert-changelog` (`C-c C-a`): write the commit message
    /// from the repository's ChangeLog — the newest entry for each file being
    /// committed. (Write the ChangeLog first, then commit with it.)
    pub(crate) fn insert_changelog(&mut self, cx: &mut Context) {
        let path = self.repo_dir.join("ChangeLog");
        let Ok(text) = std::fs::read_to_string(&path) else {
            cx.editor
                .set_error(format!("no ChangeLog in {}", self.repo_dir.display()));
            return;
        };
        let files = self.commit_files();
        let entries = zmax_core::changelog::entries_for_files(&text, &files);
        if entries.is_empty() {
            cx.editor
                .set_status("ChangeLog has no entry for the files being committed");
            return;
        }
        let n = entries.len();
        self.insert_lines(&entries);
        cx.editor
            .set_status(format!("inserted {n} ChangeLog entr(ies)"));
    }

    /// Replace the whole message with `text`, leaving point at its end — what
    /// `log-edit-previous-comment`'s `delete-region` + `insert` pair does.
    fn set_message(&mut self, text: &str) {
        self.lines = text.split('\n').map(str::to_string).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.row = self.lines.len() - 1;
        self.col = self.lines[self.row].chars().count();
        self.scroll = 0;
    }

    /// `log-edit-new-comment-index`: the ring index `stride` entries from the
    /// current one, wrapped into `0..len`. With no current index a positive
    /// stride counts up from the newest entry and a negative one wraps to the
    /// oldest, so a first `M-p` shows entry 1 and a first `M-n` the last.
    fn new_comment_index(&self, stride: isize, len: usize) -> usize {
        let raw = match self.ring_index {
            Some(i) => i as isize + stride,
            None if stride > 0 => stride - 1,
            None => stride,
        };
        raw.rem_euclid(len as isize) as usize
    }

    /// Emacs `log-edit-previous-comment` (`M-p`): cycle `arg` entries backward
    /// through the comment ring, replacing the message with the entry found.
    pub(crate) fn previous_comment(&mut self, arg: isize, cx: &mut Context) {
        let ring = comment_ring();
        if ring.is_empty() {
            cx.editor.set_error("Empty comment ring");
            return;
        }
        let idx = self.new_comment_index(arg, ring.len());
        self.ring_index = Some(idx);
        self.set_message(&ring[idx]);
        cx.editor.set_status(format!("Comment {}", idx + 1));
    }

    /// Emacs `log-edit-next-comment` (`M-n`): the forward twin of
    /// [`Self::previous_comment`], which it defers to with a negated count.
    pub(crate) fn next_comment(&mut self, arg: isize, cx: &mut Context) {
        self.previous_comment(-arg, cx);
    }

    /// Emacs `log-edit-comment-search-backward` (`M-r`, `stride` 1) and
    /// `log-edit-comment-search-forward` (`M-s`, `stride` -1): step through the
    /// ring `stride` entries at a time until one contains `needle`, then show it.
    /// Emacs `regexp-quote`s the answer, so the match is a plain substring; an
    /// empty answer reuses `log-edit-last-comment-match`. Unlike `M-p`/`M-n` the
    /// search does not wrap — running off either end is "Not found".
    pub(crate) fn comment_search(&mut self, needle: &str, stride: isize, cx: &mut Context) {
        let needle = if needle.is_empty() {
            LAST_COMMENT_MATCH
                .lock()
                .map(|m| m.clone())
                .unwrap_or_default()
        } else {
            if let Ok(mut last) = LAST_COMMENT_MATCH.lock() {
                *last = needle.to_string();
            }
            needle.to_string()
        };
        let ring = comment_ring();
        if ring.is_empty() {
            cx.editor.set_error("Empty comment ring");
            return;
        }
        let len = ring.len() as isize;
        let mut n = self.new_comment_index(stride, ring.len()) as isize;
        while n < len && n >= 0 && !ring[n as usize].contains(&needle) {
            n += stride;
        }
        if n >= len || n < 0 {
            cx.editor.set_error("Not found");
            return;
        }
        self.ring_index = Some(n as usize);
        // Emacs re-enters `log-edit-previous-comment` with a zero count purely to
        // pull the entry it just settled on into the buffer.
        self.previous_comment(0, cx);
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
    pub(crate) fn confirm(&self, cx: &mut Context) -> Option<Callback> {
        let msg = self.message();
        if msg.trim().is_empty() {
            cx.editor.set_status("aborted: empty commit message");
            return None;
        }
        // `log-edit-done` remembers the comment before handing off to the commit,
        // so a message that git then rejects is still reachable with `M-p`.
        remember_comment(&msg);
        let tmp = std::env::temp_dir().join(format!("zmax-COMMIT_EDITMSG-{}", std::process::id()));
        if let Err(e) = std::fs::write(&tmp, &msg) {
            cx.editor
                .set_error(format!("commit: temp write failed: {e}"));
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
                // HEAD moved: re-fetch every open buffer's diff base so gutter
                // hunks reflect the just-committed tree (worktree bytes unchanged,
                // so base-only — never clobbers unsaved edits).
                crate::commands::refresh_all_diff_bases(cx.editor);
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

        // `C-c` opens the `log-edit-mode-map` prefix: `C-c C-c` commits
        // (log-edit-done), `C-c C-d` shows the diff, `C-c C-f` the file list,
        // `C-c C-a` inserts the ChangeLog, `C-c C-w` generates one from the diff,
        // and `C-c C-k` kills the buffer. Any other key drops the chord.
        if self.pending_confirm {
            self.pending_confirm = false;
            match key {
                ctrl!('c') => {
                    if let Some(cb) = self.confirm(cx) {
                        return EventResult::Consumed(Some(cb));
                    }
                }
                ctrl!('d') => self.show_diff(cx),
                ctrl!('f') => self.show_files(cx),
                ctrl!('a') => self.insert_changelog(cx),
                ctrl!('w') => self.generate_changelog_from_diff(cx),
                ctrl!('k') => {
                    return EventResult::Consumed(Some(Box::new(
                        |compositor: &mut Compositor, _cx| {
                            compositor.pop();
                        },
                    )))
                }
                _ => cx.editor.set_status("C-c is not a prefix for that key"),
            }
            return EventResult::Consumed(None);
        }
        if let ctrl!('c') = key {
            self.pending_confirm = true;
            cx.editor.set_status(
                "C-c- (C-c commit · C-d diff · C-f files · C-a ChangeLog · C-w from diff · C-k cancel)",
            );
            return EventResult::Consumed(None);
        }

        // The `M-r` / `M-s` comment-substring prompt owns every key while open;
        // `Esc` / `C-g` abandons it without touching the message.
        if let Some((backward, mut buf)) = self.searching.take() {
            match key {
                key!(Esc) | ctrl!('g') => {}
                key!(Enter) => self.comment_search(&buf, if backward { 1 } else { -1 }, cx),
                key!(Backspace) => {
                    buf.pop();
                    self.searching = Some((backward, buf));
                }
                _ => {
                    if let KeyCode::Char(c) = key.code {
                        if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                            buf.push(c);
                        }
                    }
                    self.searching = Some((backward, buf));
                }
            }
            return EventResult::Consumed(None);
        }

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        // The `*vc-diff*` / `*vc-log-files*` pane scrolls with PageUp/PageDown and
        // closes with Esc; typing keeps editing the message underneath.
        if let Some(pane) = &mut self.pane {
            match key {
                key!(PageDown) => {
                    pane.scroll = (pane.scroll + 5).min(pane.lines.len().saturating_sub(1));
                    return EventResult::Consumed(None);
                }
                key!(PageUp) => {
                    pane.scroll = pane.scroll.saturating_sub(5);
                    return EventResult::Consumed(None);
                }
                key!(Esc) => {
                    self.pane = None;
                    return EventResult::Consumed(None);
                }
                _ => {}
            }
        }

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
            // The Log Edit comment ring. Emacs takes a numeric prefix for how far
            // to step; this overlay has no prefix argument, so each press is one.
            alt!('p') => self.previous_comment(1, cx),
            alt!('n') => self.next_comment(1, cx),
            alt!('r') => self.searching = Some((true, String::new())),
            alt!('s') => self.searching = Some((false, String::new())),
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
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
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
        let hint = "C-c C-c commit  C-c C-d diff  C-c C-f files  C-c C-a ChangeLog  C-c C-w from-diff  Esc cancel";
        if (title.len() + hint.len() + 3) < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }

        // The `M-r` / `M-s` prompt sits on the blank row between title and body.
        if let Some((_, buf)) = &self.searching {
            let line = format!(" Comment substring: {buf}");
            surface.set_stringn(area.x, area.y + 1, &line, area.width as usize, header_style);
            let caret = area.x + line.chars().count() as u16;
            if caret < area.x + area.width {
                surface.set_style(Rect::new(caret, area.y + 1, 1, 1), cursor_style);
            }
        }

        let body_y = area.y + 2;
        let mut body_h = area.height.saturating_sub(2);

        // The diff / file-list pane takes the bottom half when open.
        if let Some(pane) = &self.pane {
            let pane_h = (area.height / 2).max(3).min(body_h.saturating_sub(2));
            if pane_h >= 3 {
                body_h = body_h.saturating_sub(pane_h);
                let pane_y = body_y + body_h;
                surface.set_stringn(
                    area.x,
                    pane_y,
                    &format!(" {} — PageUp/PageDown scroll, Esc close", pane.title),
                    area.width as usize,
                    header_style,
                );
                for (i, line) in pane
                    .lines
                    .iter()
                    .skip(pane.scroll)
                    .take(pane_h.saturating_sub(1) as usize)
                    .enumerate()
                {
                    let style = if line.starts_with('+') {
                        theme.get("diff.plus")
                    } else if line.starts_with('-') {
                        theme.get("diff.minus")
                    } else {
                        info_style
                    };
                    surface.set_stringn(
                        area.x,
                        pane_y + 1 + i as u16,
                        line,
                        area.width as usize,
                        style,
                    );
                }
            }
        }
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
        let out = git_output(&repo_dir, &["log", "--oneline", "--decorate", "-n", "200"])
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

    /// Open the interactive-rebase todo editor for the commits *after* the
    /// selected one (i.e. `<selected_sha>..HEAD`). The selected commit is the
    /// rebase base and stays untouched.
    fn rebase_callback(&self) -> Option<Callback> {
        let base = self.entries.get(self.selected)?.sha.clone();
        let repo_dir = self.repo_dir.clone();
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| match MagitRebase::new(
                repo_dir.clone(),
                &base,
            ) {
                Some(editor) => compositor.push(Box::new(editor)),
                None => cx.editor.set_status("no commits after this one to rebase"),
            },
        ))
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
            key!('r') => {
                if let Some(cb) = self.rebase_callback() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
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
        let hint = "j/k move  Enter/d show diff  r rebase  q back";
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
            surface.set_stringn(
                area.x,
                y,
                &format!("  {}", entry.sha),
                area.width as usize,
                style,
            );
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

/// An interactive-rebase todo editor (magit/lazygit style), opened from
/// [`MagitLog`] with `r` on a commit.
///
/// It rebases the commits *after* the selected one — `git rebase -i <base>`
/// editing `<base>..HEAD` — where `<base>` is the selected commit's SHA. The
/// todo is that commit's descendants up to HEAD in git's order (oldest first).
///
/// Keys: `j`/`k`/arrows move; `g`/`G` top/bottom; `K`/`J` reorder the selected
/// row up/down; `p`/`s`/`f`/`d` set the row's action (pick/squash/fixup/drop);
/// `Enter` or `Ctrl-c Ctrl-c` execute; `q`/`Esc` abort without rebasing.
///
/// Reword and edit are intentionally not offered this slice: they require git to
/// stop mid-rebase for interactive message/commit editing, but the executor
/// overrides `GIT_EDITOR`/`GIT_SEQUENCE_EDITOR` so the rebase never blocks.
///
/// Execution serializes the todo to a temp file and runs git non-interactively:
/// `GIT_SEQUENCE_EDITOR=cp <tmp>` makes git overwrite its generated todo with
/// ours (git appends the todo path, so it runs `cp <tmp> <todo>`), and
/// `GIT_EDITOR=true` auto-accepts any squash/fixup combined message. On a
/// conflict the editor closes and the rebase is left in progress for the user to
/// resolve from the status buffer (resolve → `r` continue / `A` abort).
pub struct MagitRebase {
    repo_dir: PathBuf,
    /// The rebase base (selected commit SHA); `git rebase -i <base_sha>`.
    base_sha: String,
    rows: Vec<RebaseRow>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    /// Set after one `Ctrl-c`; a second `Ctrl-c` runs the rebase.
    pending_exec: bool,
}

impl MagitRebase {
    /// Build the editor for `<base_sha>..HEAD`. Returns `None` (with no overlay)
    /// when there are no commits after the base to rebase.
    fn new(repo_dir: PathBuf, base_sha: &str) -> Option<Self> {
        let range = format!("{base_sha}..HEAD");
        let out = git_output(&repo_dir, &["log", "--reverse", "--format=%h %s", &range])?;
        let rows = parse_rebase_todo(&out);
        if rows.is_empty() {
            return None;
        }
        Some(MagitRebase {
            repo_dir,
            base_sha: base_sha.to_string(),
            rows,
            selected: 0,
            scroll: 0,
            viewport: 1,
            pending_exec: false,
        })
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let max = self.rows.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Move the selected row up (`delta = -1`) or down (`delta = 1`), carrying
    /// the selection with it. No-op at the ends.
    fn move_row(&mut self, delta: isize) {
        let target = self.selected as isize + delta;
        if target < 0 || target >= self.rows.len() as isize {
            return;
        }
        let target = target as usize;
        self.rows.swap(self.selected, target);
        self.selected = target;
    }

    fn set_action(&mut self, action: RebaseAction) {
        if let Some(row) = self.rows.get_mut(self.selected) {
            row.action = action;
        }
    }

    /// Serialize the todo and run the rebase non-interactively. Returns a pop
    /// callback both on success and on a conflict/failure (leaving the rebase in
    /// progress in the latter case); returns `None` only when git can't be
    /// spawned, so the editor stays open.
    fn execute(&self, cx: &mut Context) -> Option<Callback> {
        let todo = render_todo(&self.rows);
        let tmp = std::env::temp_dir().join(format!("zmax-rebase-todo-{}", std::process::id()));
        if let Err(e) = std::fs::write(&tmp, &todo) {
            cx.editor
                .set_error(format!("rebase: temp write failed: {e}"));
            return None;
        }
        let tmp_str = tmp.to_string_lossy().into_owned();
        // git runs `$GIT_SEQUENCE_EDITOR <todopath>`, so `cp <tmp>` expands to
        // `cp <tmp> <todopath>`, overwriting git's generated todo with ours.
        let seq_editor = format!("cp {}", shell_quote(&tmp_str));
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.repo_dir);
        cmd.args(["-c", "rebase.autosquash=false", "rebase", "-i"]);
        cmd.arg(&self.base_sha);
        cmd.env("GIT_SEQUENCE_EDITOR", seq_editor);
        cmd.env("GIT_EDITOR", "true");
        let out = cmd.output();
        let _ = std::fs::remove_file(&tmp);

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match out {
            Ok(o) if o.status.success() => {
                cx.editor
                    .set_status(format!("rebased onto {}", self.base_sha));
                schedule_status_refresh(cx);
                Some(close)
            }
            Ok(o) => {
                // Nonzero exit, typically a merge conflict. Don't hang: close the
                // editor, surface git's message and leave the rebase in progress.
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                let mut parts = Vec::new();
                if !stdout.trim().is_empty() {
                    parts.push(stdout.trim().to_string());
                }
                if !stderr.trim().is_empty() {
                    parts.push(stderr.trim().to_string());
                }
                let msg = condense(&parts.join("\n"));
                let msg = if msg.is_empty() {
                    "resolve conflicts, then continue".to_string()
                } else {
                    msg
                };
                cx.editor.set_error(format!("rebase stopped: {msg}"));
                schedule_status_refresh(cx);
                Some(close)
            }
            Err(e) => {
                cx.editor.set_error(format!("git rebase: {e}"));
                None
            }
        }
    }
}

impl Component for MagitRebase {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // `Ctrl-c Ctrl-c` runs the rebase (two presses); any other key resets it.
        if let ctrl!('c') = key {
            if self.pending_exec {
                self.pending_exec = false;
                if let Some(cb) = self.execute(cx) {
                    return EventResult::Consumed(Some(cb));
                }
            } else {
                self.pending_exec = true;
                cx.editor
                    .set_status("press Ctrl-c again to run the rebase (Esc to cancel)");
            }
            return EventResult::Consumed(None);
        }
        self.pending_exec = false;

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('k') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!('g') | key!(Home) => self.selected = 0,
            key!('G') | key!(End) => self.selected = self.rows.len().saturating_sub(1),
            key!('K') => self.move_row(-1),
            key!('J') => self.move_row(1),
            key!('p') => self.set_action(RebaseAction::Pick),
            key!('s') => self.set_action(RebaseAction::Squash),
            key!('f') => self.set_action(RebaseAction::Fixup),
            key!('d') => self.set_action(RebaseAction::Drop),
            key!(Enter) => {
                if let Some(cb) = self.execute(cx) {
                    return EventResult::Consumed(Some(cb));
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let sha_style = theme.get("constant.numeric");
        let accent_style = to_bold(theme.get("ui.text.focus"));
        let drop_style = theme
            .get("ui.linenr")
            .add_modifier(zmax_view::graphics::Modifier::CROSSED_OUT);
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = " Rebase todo";
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        let hint = "j/k move  K/J reorder  p pick  s squash  f fixup  d drop  Enter run  q abort";
        if (title.len() + hint.len() + 3) < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }
        let base_line = format!(" onto {} ({} commits)", self.base_sha, self.rows.len());
        surface.set_stringn(
            area.x,
            area.y + 1,
            &base_line,
            area.width as usize,
            info_style,
        );

        let body_y = area.y + 3;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h as usize;

        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected - self.viewport + 1;
        }

        for (offset, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let selected = offset == self.selected;
            if selected {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
            }
            // `pick   abc1234 subject` — verb padded so the SHA/subject align.
            let verb = row.action.verb();
            let verb_style = if selected {
                sel_style
            } else {
                match row.action {
                    RebaseAction::Pick => text_style,
                    RebaseAction::Squash | RebaseAction::Fixup => accent_style,
                    RebaseAction::Drop => drop_style,
                }
            };
            surface.set_stringn(
                area.x,
                y,
                &format!("  {verb:<7}"),
                area.width as usize,
                verb_style,
            );
            let sha_x = area.x + 2 + 7;
            if sha_x < area.x + area.width {
                let style = if selected { sel_style } else { sha_style };
                surface.set_stringn(
                    sha_x,
                    y,
                    &row.sha,
                    (area.x + area.width - sha_x) as usize,
                    style,
                );
            }
            let subj_x = sha_x + row.sha.chars().count() as u16 + 1;
            if subj_x < area.x + area.width {
                let style = if selected {
                    sel_style
                } else if row.action == RebaseAction::Drop {
                    drop_style
                } else {
                    text_style
                };
                surface.set_stringn(
                    subj_x,
                    y,
                    &row.subject,
                    (area.x + area.width - subj_x) as usize,
                    style,
                );
            }
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit-rebase")
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
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
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
                // Branch/tag checkout rewrites the working tree and moves HEAD:
                // reload open buffers so their content and gutters follow.
                crate::commands::reload_all_open_docs(cx.editor);
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
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
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
            surface.set_stringn(
                area.x,
                body_y,
                "no branches",
                area.width as usize,
                info_style,
            );
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

/// Record `name` as the buried status buffer's remote, once the compositor is
/// reachable again (the picker itself only holds the repo path).
fn schedule_remote_select(cx: &mut Context, name: String) {
    cx.jobs.callback(async move {
        let call = crate::job::Callback::EditorCompositor(Box::new(
            move |_editor, compositor: &mut Compositor| {
                if let Some(m) = compositor.find::<MagitStatus>() {
                    m.set_remote(name.clone());
                }
            },
        ));
        Ok(call)
    });
}

/// A remote picker sub-view, opened from the status buffer with `R`.
///
/// Lists the configured remotes (`git remote -v`) with their fetch URLs, the
/// selected one marked. `j`/`k`/arrows move, `Enter` makes it the target of the
/// buffer's push/fetch/pull (`git push <remote>` rather than git's default) and
/// `q`/`Esc` go back.
pub struct MagitRemote {
    entries: Vec<RemoteEntry>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    /// The remote already in effect, so the list can mark it.
    current: Option<String>,
}

impl MagitRemote {
    fn new(repo_dir: PathBuf, current: Option<String>) -> Self {
        let out = git_output(&repo_dir, &["remote", "-v"]).unwrap_or_default();
        let entries = parse_remotes(&out);
        // Open on the remote in effect so Enter is a confirm, not a change.
        let selected = current
            .as_ref()
            .and_then(|c| entries.iter().position(|r| &r.name == c))
            .unwrap_or(0);
        MagitRemote {
            entries,
            selected,
            scroll: 0,
            viewport: 1,
            current,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }
}

impl Component for MagitRemote {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
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
            key!(Enter) => {
                if let Some(r) = self.entries.get(self.selected) {
                    let name = r.name.clone();
                    schedule_remote_select(cx, name.clone());
                    cx.editor
                        .set_status(format!("remote operations target {name}"));
                    return EventResult::Consumed(Some(close));
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let cur_style = theme.get("diff.plus");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = " Remotes";
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        let hint = "j/k move  Enter select  q back";
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

        if self.entries.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "no remotes",
                area.width as usize,
                info_style,
            );
            return;
        }

        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected - self.viewport + 1;
        }

        for (offset, r) in self
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
            let current = self.current.as_deref() == Some(r.name.as_str());
            let marker = if current { "* " } else { "  " };
            let style = if offset == self.selected {
                sel_style
            } else if current {
                cur_style
            } else {
                text_style
            };
            surface.set_stringn(
                area.x,
                y,
                &format!("{marker}{}  {}", r.name, r.url),
                area.width as usize,
                style,
            );
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit-remote")
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
            Ok(()) => {
                // push/pop/apply rewrite the working tree; drop doesn't, but
                // reloading is a cheap no-op then (disk == buffer). Reload so
                // buffer content and gutters follow the stash operation.
                crate::commands::reload_all_open_docs(cx.editor);
                cx.editor.set_status(format!("stash: {label}"));
            }
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
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
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
            surface.set_stringn(
                area.x,
                body_y,
                "no stashes",
                area.width as usize,
                info_style,
            );
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
            surface.set_stringn(
                area.x,
                y,
                &format!("  {}", e.reff),
                area.width as usize,
                style,
            );
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
fn to_bold(style: zmax_view::graphics::Style) -> zmax_view::graphics::Style {
    style.add_modifier(zmax_view::graphics::Modifier::BOLD)
}

/// Single-quote a string for safe use inside a shell command (git runs
/// `GIT_SEQUENCE_EDITOR` through the shell). Embedded single quotes are escaped
/// the POSIX way (`'\''`).
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Detect an in-progress rebase by probing the git state directory
/// (`rebase-merge` for interactive/merge rebases, `rebase-apply` for am-style),
/// returning its progress. `None` when no rebase is running.
fn detect_rebase(repo: &Path) -> Option<RebaseProgress> {
    // (state-dir name, current-step file, total-steps file).
    for (name, num_file, end_file) in [
        ("rebase-merge", "msgnum", "end"),
        ("rebase-apply", "next", "last"),
    ] {
        let Some(rel) = git_output(repo, &["rev-parse", "--git-path", name]) else {
            continue;
        };
        let p = PathBuf::from(rel.trim());
        // `--git-path` is relative to the repo (we ran with `-C repo`).
        let dir = if p.is_absolute() { p } else { repo.join(p) };
        if !dir.exists() {
            continue;
        }
        let onto = std::fs::read_to_string(dir.join("onto"))
            .ok()
            .map(|s| s.trim().chars().take(9).collect::<String>())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "?".to_string());
        let read_num = |file: &str| -> usize {
            std::fs::read_to_string(dir.join(file))
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0)
        };
        return Some(RebaseProgress {
            onto,
            done: read_num(num_file),
            total: read_num(end_file),
        });
    }
    None
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

// ---------------------------------------------------------------------------
// Forge: GitHub/GitLab topics (issues and pull requests).
//
// A Magit "forge" surface layered on the git status hub, opened with `N`. The
// topic list is read by shelling out to the `gh` CLI (`gh issue list` / `gh pr
// list --json …`) and parsed by the pure, unit-tested [`parse_forge_topics`];
// per-topic posts come from `gh api …/issues/<n>/comments` and are parsed by
// [`parse_forge_posts`]. It ports the forge topic commands the Spacemacs
// `SPC m` major-mode leader binds: edit labels, edit a personal note, edit the
// topic body, copy the topic URL as a kill, toggle a pull request's draft state,
// and delete a post.
// Emacs's personal note is local (forge keeps it in its own database, never on
// the server), so zmax mirrors that by keeping notes in the repo's git dir.
// ---------------------------------------------------------------------------

/// Whether a forge topic is an issue or a pull request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopicKind {
    Issue,
    Pr,
}

impl TopicKind {
    /// The `gh` subcommand for this kind (`gh issue …` / `gh pr …`).
    fn subcommand(self) -> &'static str {
        match self {
            TopicKind::Issue => "issue",
            TopicKind::Pr => "pr",
        }
    }

    /// Short display tag shown before a topic number.
    fn tag(self) -> &'static str {
        match self {
            TopicKind::Issue => "issue",
            TopicKind::Pr => "pr",
        }
    }
}

/// One forge topic — a GitHub/GitLab issue or pull request — as listed by `gh`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ForgeTopic {
    pub kind: TopicKind,
    pub number: u64,
    pub title: String,
    pub labels: Vec<String>,
    /// Pull requests only: whether the PR is a draft.
    pub is_draft: bool,
    /// The topic's web URL, as reported by `gh` in the `url` field.
    pub url: String,
}

/// One post within a topic: the opening message, then each reply comment.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ForgePost {
    /// GitHub comment id, used to delete the post. `None` for the opening post,
    /// which cannot be deleted (only replies can — matching forge).
    pub id: Option<u64>,
    pub author: String,
    pub body: String,
}

/// Parse the JSON arrays from `gh issue list --json …` and `gh pr list --json …`
/// into [`ForgeTopic`]s (issues first, then PRs). Pure and unit-tested. Rows
/// missing a `number` are skipped; missing string fields default to empty.
pub fn parse_forge_topics(issues_json: &str, prs_json: &str) -> Vec<ForgeTopic> {
    let mut out = Vec::new();
    parse_topic_array(issues_json, TopicKind::Issue, &mut out);
    parse_topic_array(prs_json, TopicKind::Pr, &mut out);
    out
}

fn parse_topic_array(json: &str, kind: TopicKind, out: &mut Vec<ForgeTopic>) {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };
    for item in items {
        let Some(number) = item.get("number").and_then(|n| n.as_u64()) else {
            continue;
        };
        let title = item
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let url = item
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();
        let is_draft = item.get("isDraft").and_then(|d| d.as_bool()).unwrap_or(false);
        // `gh` reports labels as objects with a `name` field.
        let labels = item
            .get("labels")
            .and_then(|l| l.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|lb| lb.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        out.push(ForgeTopic {
            kind,
            number,
            title,
            labels,
            is_draft,
            url,
        });
    }
}

/// Parse the JSON array from `gh api …/issues/<n>/comments` into [`ForgePost`]s,
/// appending to `out` (the opening post is pushed by the caller first). Pure and
/// unit-tested.
pub fn parse_forge_posts(json: &str, out: &mut Vec<ForgePost>) {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };
    for item in items {
        out.push(ForgePost {
            id: item.get("id").and_then(|i| i.as_u64()),
            author: item
                .get("user")
                .and_then(|u| u.get("login"))
                .and_then(|l| l.as_str())
                .unwrap_or("")
                .to_string(),
            body: item
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string(),
        });
    }
}

/// Run a read-only `gh` command in `dir`, returning stdout on success or the
/// trimmed stderr (falling back to a generic message) on failure.
fn gh_output(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("gh");
    cmd.current_dir(dir);
    for a in args {
        cmd.arg(a);
    }
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if stderr.is_empty() {
                "gh command failed".to_string()
            } else {
                stderr
            })
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Run a mutating `gh` command in `dir`, discarding stdout.
fn gh_run(dir: &Path, args: &[&str]) -> Result<(), String> {
    gh_output(dir, args).map(|_| ())
}

/// The local forge-note store for a repo, kept in the git dir. Personal notes
/// are local-only in emacs's forge, so zmax never sends them anywhere.
fn forge_notes_path(repo_dir: &Path) -> PathBuf {
    let git_dir = git_output(repo_dir, &["rev-parse", "--git-dir"])
        .map(|s| {
            let p = PathBuf::from(s.trim());
            if p.is_absolute() {
                p
            } else {
                repo_dir.join(p)
            }
        })
        .unwrap_or_else(|| repo_dir.join(".git"));
    git_dir.join("zmax-forge-notes.json")
}

/// The note map key for a topic (`issue:12` / `pr:34`).
fn note_key(kind: TopicKind, number: u64) -> String {
    format!("{}:{number}", kind.tag())
}

fn load_forge_notes(repo_dir: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(forge_notes_path(repo_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save (or, for empty text, clear) the personal note on a topic.
fn save_forge_note(
    repo_dir: &Path,
    kind: TopicKind,
    number: u64,
    note: &str,
) -> Result<(), String> {
    let mut notes = load_forge_notes(repo_dir);
    let key = note_key(kind, number);
    if note.trim().is_empty() {
        notes.remove(&key);
    } else {
        notes.insert(key, note.to_string());
    }
    let json = serde_json::to_string_pretty(&notes).map_err(|e| e.to_string())?;
    std::fs::write(forge_notes_path(repo_dir), json).map_err(|e| e.to_string())
}

/// Which single-line editor is open over the topic list, if any.
enum ForgeInput {
    /// `SPC m m` (`forge-edit-topic-labels`): comma-separated label set.
    Labels(String),
    /// `SPC m n` (`forge-edit-topic-note`): the local personal note text.
    Note(String),
}

/// The forge topic list, opened from the status buffer with `N`.
///
/// Lists open issues and pull requests (`gh issue list` / `gh pr list`). Keys:
/// `j`/`k`/arrows move, `g`/`G` jump, `m` edit labels (`forge-edit-topic-labels`),
/// `n` edit the personal note (`forge-edit-topic-note`), `u` copy the topic URL to
/// the kill ring (`forge-copy-url-at-point-as-kill`), `d` toggle a PR's draft state
/// (`forge-toggle-draft`), `e` edit the topic body (`forge-edit-post`), `Enter` open
/// the topic's posts (where `D` deletes a post, `forge-delete-comment`), `g` refresh
/// and `q`/`Esc` go back.
pub struct MagitForge {
    repo_dir: PathBuf,
    entries: Vec<ForgeTopic>,
    notes: HashMap<String, String>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    input: Option<ForgeInput>,
    /// Set when `gh` is missing or errors; shown in place of the list.
    error: Option<String>,
}

impl MagitForge {
    fn new(repo_dir: PathBuf) -> Self {
        let mut me = MagitForge {
            repo_dir,
            entries: Vec::new(),
            notes: HashMap::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            input: None,
            error: None,
        };
        me.refresh();
        me
    }

    /// Re-read the topic list and the local notes from disk.
    fn refresh(&mut self) {
        let issues = gh_output(
            &self.repo_dir,
            &[
                "issue", "list", "--state", "open", "--limit", "100", "--json",
                "number,title,labels,url",
            ],
        );
        let prs = gh_output(
            &self.repo_dir,
            &[
                "pr", "list", "--state", "open", "--limit", "100", "--json",
                "number,title,labels,url,isDraft",
            ],
        );
        match (issues, prs) {
            (Ok(i), Ok(p)) => {
                self.entries = parse_forge_topics(&i, &p);
                self.error = None;
            }
            (Err(e), _) | (_, Err(e)) => {
                self.entries.clear();
                self.error = Some(e);
            }
        }
        self.notes = load_forge_notes(&self.repo_dir);
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

    /// `u` (`forge-copy-url-at-point-as-kill`): push the selected topic's web URL
    /// onto the kill ring.
    fn copy_url(&self, cx: &mut Context) {
        if let Some(t) = self.entries.get(self.selected) {
            if t.url.is_empty() {
                cx.editor.set_status("no url for this topic");
            } else {
                crate::emacs_kill::record(t.url.clone());
                cx.editor.set_status(format!("copied {}", t.url));
            }
        }
    }

    /// `d` (`forge-toggle-draft`): flip the selected pull request between draft
    /// and ready via `gh pr ready` / `gh pr ready --undo`.
    fn toggle_draft(&mut self, cx: &mut Context) {
        let Some(t) = self.entries.get(self.selected).cloned() else {
            return;
        };
        if t.kind != TopicKind::Pr {
            cx.editor.set_status("only pull requests can be draft");
            return;
        }
        let num = t.number.to_string();
        let args: Vec<&str> = if t.is_draft {
            vec!["pr", "ready", &num]
        } else {
            vec!["pr", "ready", &num, "--undo"]
        };
        match gh_run(&self.repo_dir, &args) {
            Ok(()) => {
                let state = if t.is_draft { "ready" } else { "draft" };
                cx.editor.set_status(format!("PR #{} marked {state}", t.number));
                self.refresh();
            }
            Err(e) => cx.editor.set_error(format!("gh pr ready: {e}")),
        }
    }

    /// `m` (`forge-edit-topic-labels`): open the label editor prefilled with the
    /// topic's current labels.
    fn begin_labels(&mut self, cx: &mut Context) {
        if let Some(t) = self.entries.get(self.selected) {
            self.input = Some(ForgeInput::Labels(t.labels.join(", ")));
            cx.editor
                .set_status("edit labels (comma-separated; Enter to apply, Esc to cancel)");
        }
    }

    /// Apply the edited label set, diffing against the current labels and calling
    /// `gh <kind> edit --add-label/--remove-label`.
    fn apply_labels(&mut self, text: &str, cx: &mut Context) {
        let Some(t) = self.entries.get(self.selected).cloned() else {
            return;
        };
        let want: Vec<String> = text
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let add: Vec<String> = want.iter().filter(|l| !t.labels.contains(l)).cloned().collect();
        let remove: Vec<String> = t
            .labels
            .iter()
            .filter(|l| !want.contains(l))
            .cloned()
            .collect();
        if add.is_empty() && remove.is_empty() {
            cx.editor.set_status("labels unchanged");
            return;
        }
        let sub = t.kind.subcommand();
        let mut args: Vec<String> = vec![sub.to_string(), "edit".to_string(), t.number.to_string()];
        if !add.is_empty() {
            args.push("--add-label".to_string());
            args.push(add.join(","));
        }
        if !remove.is_empty() {
            args.push("--remove-label".to_string());
            args.push(remove.join(","));
        }
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        match gh_run(&self.repo_dir, &argv) {
            Ok(()) => {
                cx.editor
                    .set_status(format!("updated labels on #{}", t.number));
                self.refresh();
            }
            Err(e) => cx.editor.set_error(format!("gh {sub} edit: {e}")),
        }
    }

    /// `n` (`forge-edit-topic-note`): open the personal-note editor prefilled with
    /// the existing local note.
    fn begin_note(&mut self, cx: &mut Context) {
        if let Some(t) = self.entries.get(self.selected) {
            let existing = self
                .notes
                .get(&note_key(t.kind, t.number))
                .cloned()
                .unwrap_or_default();
            self.input = Some(ForgeInput::Note(existing));
            cx.editor
                .set_status("edit personal note (local; Enter to save, Esc to cancel)");
        }
    }

    /// Save the edited personal note to the local store.
    fn apply_note(&mut self, text: &str, cx: &mut Context) {
        let Some(t) = self.entries.get(self.selected).cloned() else {
            return;
        };
        match save_forge_note(&self.repo_dir, t.kind, t.number, text) {
            Ok(()) => {
                cx.editor.set_status(format!("saved note on #{}", t.number));
                self.notes = load_forge_notes(&self.repo_dir);
            }
            Err(e) => cx.editor.set_error(format!("save note: {e}")),
        }
    }

    /// `e` (`forge-edit-post`, `SPC m e`): open the multi-line body editor over
    /// the selected topic, pre-filled with its current body.
    fn edit_post(&self) -> Option<Callback> {
        let t = self.entries.get(self.selected)?.clone();
        let repo_dir = self.repo_dir.clone();
        Some(Box::new(move |compositor: &mut Compositor, _cx| {
            compositor.push(Box::new(MagitForgePostEdit::new(repo_dir.clone(), t.clone())));
        }))
    }

    /// `Enter`: open the selected topic's posts (the delete-comment surface).
    fn open_topic(&self) -> Option<Callback> {
        let t = self.entries.get(self.selected)?.clone();
        let repo_dir = self.repo_dir.clone();
        Some(Box::new(move |compositor: &mut Compositor, _cx| {
            compositor.push(Box::new(MagitForgeTopic::new(repo_dir.clone(), t.clone())));
        }))
    }
}

impl Component for MagitForge {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // A label/note editor owns every key while it is open.
        if self.input.is_some() {
            match key {
                key!(Esc) | ctrl!('g') => {
                    self.input = None;
                    cx.editor.set_status("cancelled");
                }
                key!(Enter) => {
                    match self.input.take() {
                        Some(ForgeInput::Labels(text)) => self.apply_labels(&text, cx),
                        Some(ForgeInput::Note(text)) => self.apply_note(&text, cx),
                        None => {}
                    }
                }
                key!(Backspace) => {
                    if let Some(ForgeInput::Labels(buf) | ForgeInput::Note(buf)) = &mut self.input {
                        buf.pop();
                    }
                }
                KeyEvent {
                    code: KeyCode::Char(c),
                    modifiers,
                } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                    if let Some(ForgeInput::Labels(buf) | ForgeInput::Note(buf)) = &mut self.input {
                        buf.push(c);
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
            key!('g') | key!(Home) => self.refresh(),
            key!('G') | key!(End) => self.selected = self.entries.len().saturating_sub(1),
            key!('m') => self.begin_labels(cx),
            key!('n') => self.begin_note(cx),
            key!('u') => self.copy_url(cx),
            key!('d') => self.toggle_draft(cx),
            key!('e') => {
                if let Some(cb) = self.edit_post() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            key!(Enter) => {
                if let Some(cb) = self.open_topic() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = " Forge topics";
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        if let Some(ForgeInput::Labels(buf)) = &self.input {
            let line = format!("Labels: {buf}_");
            surface.set_stringn(
                area.x + title.len() as u16 + 2,
                area.y,
                &line,
                area.width as usize,
                info_style,
            );
        } else if let Some(ForgeInput::Note(buf)) = &self.input {
            let line = format!("Note: {buf}_");
            surface.set_stringn(
                area.x + title.len() as u16 + 2,
                area.y,
                &line,
                area.width as usize,
                info_style,
            );
        } else {
            let hint = "j/k move  m labels  n note  e edit-body  u copy-url  d draft  Enter posts  g refresh  q back";
            if (title.len() + hint.len() + 3) < area.width as usize {
                surface.set_stringn(
                    area.x + area.width - hint.len() as u16 - 1,
                    area.y,
                    hint,
                    hint.len(),
                    info_style,
                );
            }
        }

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(2);
        self.viewport = body_h as usize;

        if let Some(err) = &self.error {
            surface.set_stringn(
                area.x,
                body_y,
                &format!("gh: {err}"),
                area.width as usize,
                info_style,
            );
            return;
        }

        if self.entries.is_empty() {
            surface.set_stringn(area.x, body_y, "no open topics", area.width as usize, info_style);
            return;
        }

        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected - self.viewport + 1;
        }

        for (offset, t) in self
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
            let note_mark = if self.notes.contains_key(&note_key(t.kind, t.number)) {
                "*"
            } else {
                " "
            };
            let draft = if t.kind == TopicKind::Pr && t.is_draft {
                " (draft)"
            } else {
                ""
            };
            let labels = if t.labels.is_empty() {
                String::new()
            } else {
                format!("  [{}]", t.labels.join(", "))
            };
            let line = format!(
                "{note_mark}{} #{}{draft}  {}{labels}",
                t.kind.tag(),
                t.number,
                t.title
            );
            let style = if offset == self.selected {
                sel_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, &line, area.width as usize, style);
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit-forge")
    }
}

/// A single topic's posts, opened from the topic list with `Enter`.
///
/// Lists the opening post and each reply comment (`gh api …/issues/<n>/comments`).
/// `D` (press twice to confirm) is `forge-delete-comment`: delete the post the
/// cursor is on via `gh api -X DELETE …/issues/comments/<id>`. The opening post
/// cannot be deleted, only replies — matching forge. `j`/`k` move, `q`/`Esc` back.
pub struct MagitForgeTopic {
    repo_dir: PathBuf,
    topic: ForgeTopic,
    posts: Vec<ForgePost>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    /// Armed by a first `D`; a second `D` confirms the delete.
    pending_delete: bool,
    error: Option<String>,
}

impl MagitForgeTopic {
    fn new(repo_dir: PathBuf, topic: ForgeTopic) -> Self {
        let mut me = MagitForgeTopic {
            repo_dir,
            topic,
            posts: Vec::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            pending_delete: false,
            error: None,
        };
        me.refresh();
        me
    }

    /// Re-read the opening post and the reply comments.
    fn refresh(&mut self) {
        let n = self.topic.number.to_string();
        let head_ep = format!("repos/{{owner}}/{{repo}}/issues/{n}");
        let comments_ep = format!("repos/{{owner}}/{{repo}}/issues/{n}/comments");
        let mut posts = Vec::new();
        match gh_output(&self.repo_dir, &["api", &head_ep]) {
            Ok(json) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
                    posts.push(ForgePost {
                        id: None,
                        author: v
                            .get("user")
                            .and_then(|u| u.get("login"))
                            .and_then(|l| l.as_str())
                            .unwrap_or("")
                            .to_string(),
                        body: v.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string(),
                    });
                }
            }
            Err(e) => {
                self.error = Some(e);
                return;
            }
        }
        match gh_output(&self.repo_dir, &["api", &comments_ep]) {
            Ok(json) => {
                parse_forge_posts(&json, &mut posts);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        self.posts = posts;
        if self.selected >= self.posts.len() {
            self.selected = self.posts.len().saturating_sub(1);
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.posts.is_empty() {
            return;
        }
        let max = self.posts.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// `forge-delete-comment`: delete the post under point. The opening post has
    /// no id and cannot be deleted.
    fn delete_selected(&mut self, cx: &mut Context) {
        let Some(post) = self.posts.get(self.selected) else {
            return;
        };
        let Some(id) = post.id else {
            cx.editor
                .set_status("cannot delete the initial post; only replies");
            return;
        };
        let ep = format!("repos/{{owner}}/{{repo}}/issues/comments/{id}");
        match gh_run(&self.repo_dir, &["api", "-X", "DELETE", &ep]) {
            Ok(()) => {
                cx.editor.set_status("deleted comment");
                self.refresh();
            }
            Err(e) => cx.editor.set_error(format!("gh api delete: {e}")),
        }
    }
}

impl Component for MagitForgeTopic {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // Any key other than a confirming `D` cancels a pending delete.
        if key != key!('D') && self.pending_delete {
            self.pending_delete = false;
        }

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('k') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!('g') | key!(Home) => self.refresh(),
            key!('G') | key!(End) => self.selected = self.posts.len().saturating_sub(1),
            key!('D') => {
                if self.posts.is_empty() {
                    // nothing to delete
                } else if self.pending_delete {
                    self.pending_delete = false;
                    self.delete_selected(cx);
                } else {
                    self.pending_delete = true;
                    cx.editor.set_status("press D again to delete this post");
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = format!(" {} #{}: {}", self.topic.kind.tag(), self.topic.number, self.topic.title);
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "j/k move  D delete post  q back";
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

        if let Some(err) = &self.error {
            surface.set_stringn(
                area.x,
                body_y,
                &format!("gh: {err}"),
                area.width as usize,
                info_style,
            );
            return;
        }

        if self.posts.is_empty() {
            surface.set_stringn(area.x, body_y, "no posts", area.width as usize, info_style);
            return;
        }

        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.viewport {
            self.scroll = self.selected - self.viewport + 1;
        }

        for (offset, p) in self
            .posts
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            if offset == self.selected {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
            }
            // Collapse each post to one row: author plus the first body line.
            let first = p.body.lines().next().unwrap_or("");
            let tag = if p.id.is_none() { "(op)" } else { "    " };
            let line = format!("{tag} @{}: {first}", p.author);
            let style = if offset == self.selected {
                sel_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, &line, area.width as usize, style);
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit-forge-topic")
    }
}

/// Split a fetched topic body into editor lines: normalise CRLF (GitHub bodies use
/// `\r\n`) and drop the single trailing newline `gh --jq .body` appends. Pure and
/// unit-tested. Never returns an empty vector — an empty body is one blank line.
fn body_to_lines(body: &str) -> Vec<String> {
    let normalised = body.replace("\r\n", "\n");
    let trimmed = normalised.strip_suffix('\n').unwrap_or(&normalised);
    let mut lines: Vec<String> = trimmed.split('\n').map(str::to_string).collect();
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// A multi-line editor over a forge topic's body (`forge-edit-post`, `SPC m e`).
///
/// Opened from the topic list with `e`, pre-filled with the topic's current body
/// (`gh <kind> view <n> --json body`). `Ctrl-c Ctrl-c` (two presses) submits the
/// edited body with `gh <kind> edit <n> --body-file <tempfile>` — matching forge's
/// `forge-post-submit` — and `Ctrl-c Ctrl-k` or `Esc` cancels. The editing keys
/// mirror the commit-message editor: arrows / `C-b`/`C-f`/`C-p`/`C-n` move,
/// `C-a`/`C-e` jump to the line ends.
pub struct MagitForgePostEdit {
    repo_dir: PathBuf,
    topic: ForgeTopic,
    /// Body text, one entry per line (never empty: at least `[""]`).
    lines: Vec<String>,
    /// Cursor line index into `lines`.
    row: usize,
    /// Cursor column as a character index within `lines[row]`.
    col: usize,
    /// Top visible body row.
    scroll: usize,
    /// Body rows visible in the last render.
    viewport: usize,
    /// Set after one `Ctrl-c`; a second `Ctrl-c` submits the edited body.
    pending_confirm: bool,
    /// Set when the body could not be fetched; shown in place of the editor and
    /// submission is refused so a failed fetch never clobbers the real body.
    error: Option<String>,
}

impl MagitForgePostEdit {
    fn new(repo_dir: PathBuf, topic: ForgeTopic) -> Self {
        let number = topic.number.to_string();
        let (lines, error) = match gh_output(
            &repo_dir,
            &[
                topic.kind.subcommand(),
                "view",
                &number,
                "--json",
                "body",
                "--jq",
                ".body",
            ],
        ) {
            Ok(body) => (body_to_lines(&body), None),
            Err(e) => (vec![String::new()], Some(e)),
        };
        let row = lines.len() - 1;
        let col = lines[row].chars().count();
        MagitForgePostEdit {
            repo_dir,
            topic,
            lines,
            row,
            col,
            scroll: 0,
            viewport: 1,
            pending_confirm: false,
            error,
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

    /// The assembled body with trailing blank lines trimmed.
    fn body(&self) -> String {
        self.lines.join("\n").trim_end().to_string()
    }

    /// Submit the edited body with `gh <kind> edit <n> --body-file <tempfile>`
    /// (`forge-post-submit`). Returns a close callback on success (so the editor
    /// pops), or `None` to stay open (fetch failed / write / gh error).
    fn confirm(&self, cx: &mut Context) -> Option<Callback> {
        if self.error.is_some() {
            cx.editor
                .set_status("cannot submit: the body was not loaded");
            return None;
        }
        let body = self.body();
        let tmp =
            std::env::temp_dir().join(format!("zmax-forge-post-{}", std::process::id()));
        if let Err(e) = std::fs::write(&tmp, &body) {
            cx.editor
                .set_error(format!("edit post: temp write failed: {e}"));
            return None;
        }
        let sub = self.topic.kind.subcommand();
        let number = self.topic.number.to_string();
        let tmp_arg = tmp.to_string_lossy().into_owned();
        let res = gh_run(&self.repo_dir, &[sub, "edit", &number, "--body-file", &tmp_arg]);
        let _ = std::fs::remove_file(&tmp);
        match res {
            Ok(()) => {
                cx.editor
                    .set_status(format!("updated {sub} #{} body", self.topic.number));
                Some(Box::new(|compositor: &mut Compositor, _cx| {
                    compositor.pop();
                }))
            }
            Err(e) => {
                cx.editor.set_error(format!("gh {sub} edit: {e}"));
                None
            }
        }
    }
}

impl Component for MagitForgePostEdit {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // `C-c` opens the submit prefix: `C-c C-c` submits (`forge-post-submit`),
        // `C-c C-k` cancels (`forge-post-cancel`). Any other key drops the chord.
        if self.pending_confirm {
            self.pending_confirm = false;
            match key {
                ctrl!('c') => {
                    if let Some(cb) = self.confirm(cx) {
                        return EventResult::Consumed(Some(cb));
                    }
                }
                ctrl!('k') => {
                    return EventResult::Consumed(Some(Box::new(
                        |compositor: &mut Compositor, _cx| {
                            compositor.pop();
                        },
                    )))
                }
                _ => cx.editor.set_status("C-c is not a prefix for that key"),
            }
            return EventResult::Consumed(None);
        }
        if let ctrl!('c') = key {
            self.pending_confirm = true;
            cx.editor
                .set_status("C-c- (C-c submit · C-k cancel)");
            return EventResult::Consumed(None);
        }

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
        let mut bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let info_style = theme.get("ui.linenr");
        let header_style = to_bold(theme.get("ui.text.focus"));
        let text_style = theme.get("ui.text");
        let cursor_style = theme.get("ui.cursor");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 4 {
            return;
        }

        let title = format!(
            " Edit {} #{} body",
            self.topic.kind.tag(),
            self.topic.number
        );
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "C-c C-c submit  C-c C-k cancel  Esc cancel";
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

        if let Some(err) = &self.error {
            surface.set_stringn(
                area.x,
                body_y,
                &format!("gh: {err}"),
                area.width as usize,
                info_style,
            );
            return;
        }

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
        Some("magit-forge-post-edit")
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
        assert_eq!(
            hunks[0].body,
            vec![" fn a() {}", "-old line", "+new line", " fn b() {}"]
        );
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

    #[test]
    fn rebase_action_verbs() {
        assert_eq!(RebaseAction::Pick.verb(), "pick");
        assert_eq!(RebaseAction::Squash.verb(), "squash");
        assert_eq!(RebaseAction::Fixup.verb(), "fixup");
        assert_eq!(RebaseAction::Drop.verb(), "drop");
    }

    #[test]
    fn parse_rebase_todo_defaults_to_pick_in_order() {
        // `git log --reverse --format=%h %s <base>..HEAD` — oldest first.
        let out = "aaa1111 first commit\nbbb2222 second commit\nccc3333 third\n";
        let rows = parse_rebase_todo(out);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.action == RebaseAction::Pick));
        assert_eq!(rows[0].sha, "aaa1111");
        assert_eq!(rows[0].subject, "first commit");
        assert_eq!(rows[1].sha, "bbb2222");
        assert_eq!(rows[1].subject, "second commit");
        assert_eq!(rows[2].sha, "ccc3333");
        assert_eq!(rows[2].subject, "third");
    }

    #[test]
    fn parse_rebase_todo_handles_empty_subject_and_blanks() {
        let rows = parse_rebase_todo("\nabc1234\n\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sha, "abc1234");
        assert_eq!(rows[0].subject, "");
        assert!(parse_rebase_todo("").is_empty());
    }

    #[test]
    fn render_todo_emits_verb_sha_subject_in_order() {
        let rows = vec![
            RebaseRow {
                action: RebaseAction::Pick,
                sha: "aaa1111".into(),
                subject: "first".into(),
            },
            RebaseRow {
                action: RebaseAction::Squash,
                sha: "bbb2222".into(),
                subject: "second".into(),
            },
            RebaseRow {
                action: RebaseAction::Fixup,
                sha: "ccc3333".into(),
                subject: "third".into(),
            },
            RebaseRow {
                action: RebaseAction::Drop,
                sha: "ddd4444".into(),
                subject: "fourth".into(),
            },
        ];
        let todo = render_todo(&rows);
        assert_eq!(
            todo,
            "pick aaa1111 first\nsquash bbb2222 second\nfixup ccc3333 third\ndrop ddd4444 fourth\n"
        );
    }

    #[test]
    fn render_todo_keeps_dropped_rows() {
        // A dropped row still emits a `drop …` line (git removes it on apply).
        let rows = vec![RebaseRow {
            action: RebaseAction::Drop,
            sha: "deadbee".into(),
            subject: "obsolete change".into(),
        }];
        assert_eq!(render_todo(&rows), "drop deadbee obsolete change\n");
    }

    #[test]
    fn render_todo_roundtrips_through_parse() {
        // Parsing a `%h %s` log then rendering prepends the default `pick` verb.
        let out = "aaa1111 a\nbbb2222 b\n";
        assert_eq!(
            render_todo(&parse_rebase_todo(out)),
            "pick aaa1111 a\npick bbb2222 b\n"
        );
    }

    #[test]
    fn render_todo_empty() {
        assert_eq!(render_todo(&[]), "");
    }

    #[test]
    fn shell_quote_wraps_and_escapes() {
        assert_eq!(shell_quote("/tmp/todo"), "'/tmp/todo'");
        assert_eq!(shell_quote("/has space/x"), "'/has space/x'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    // --- vc-dir marks -------------------------------------------------------

    /// A status buffer over a fake repo: enough to exercise the mark set without
    /// touching git (the file operations shell out; the *selection* rules do not).
    fn fake_status(porcelain: &str) -> MagitStatus {
        let mut entries = parse_status(porcelain);
        entries.sort_by(|a, b| {
            a.section
                .order()
                .cmp(&b.section.order())
                .then_with(|| a.path.cmp(&b.path))
        });
        MagitStatus {
            repo_dir: PathBuf::from("/nonexistent"),
            head: "main".into(),
            entries,
            selected: 0,
            scroll: 0,
            viewport: 10,
            pending_discard: false,
            upstream: None,
            expanded: HashSet::new(),
            diffs: HashMap::new(),
            rebase: None,
            marked: HashSet::new(),
            mark_regexp: None,
            edit_next: false,
            edit_command: None,
            remote: None,
        }
    }

    /// The whole point of a mark: with none set, an operation acts on the row
    /// under the cursor; with marks set, it acts on the marked files instead.
    #[test]
    fn marks_decide_what_an_operation_acts_on() {
        let mut status = fake_status("?? new.txt\n M src/a.rs\n M src/b.rs\n");
        // Nothing marked: the file under the cursor (the first row) is the target.
        assert_eq!(status.acted_on(), vec!["new.txt".to_string()]);

        status.marked.insert("src/b.rs".into());
        status.marked.insert("src/a.rs".into());
        // Marked files win over the cursor, and come out in buffer order (not the
        // hash set's), so the status message is stable.
        assert_eq!(
            status.acted_on(),
            vec!["src/a.rs".to_string(), "src/b.rs".to_string()]
        );
    }

    /// A file with both a staged and an unstaged change is two rows but one path:
    /// marking it must not stage it twice.
    #[test]
    fn a_path_in_two_sections_is_acted_on_once() {
        let mut status = fake_status("MM src/a.rs\n");
        assert_eq!(
            status.entries.len(),
            2,
            "MM yields a staged and an unstaged row"
        );
        status.marked.insert("src/a.rs".into());
        assert_eq!(status.acted_on(), vec!["src/a.rs".to_string()]);
    }

    /// `vc-dir-mark-registered-files` marks the tracked files only — an untracked
    /// file is not registered with the VCS.
    #[test]
    fn registered_marks_skip_untracked_files() {
        let status = fake_status("?? new.txt\n M src/a.rs\nA  src/b.rs\nUU src/c.rs\n");
        let registered: Vec<String> = status
            .entries
            .iter()
            .filter(|e| e.section != Section::Untracked)
            .map(|e| e.path.clone())
            .collect();
        assert!(registered.contains(&"src/a.rs".to_string()));
        assert!(registered.contains(&"src/b.rs".to_string()));
        assert!(registered.contains(&"src/c.rs".to_string()));
        assert!(
            !registered.contains(&"new.txt".to_string()),
            "an untracked file is not registered"
        );
    }

    /// A refresh drops marks on files that no longer have a change — a stale mark
    /// would otherwise silently include a file in the next operation.
    #[test]
    fn refresh_drops_marks_on_vanished_files() {
        let mut status = fake_status(" M src/a.rs\n M src/b.rs\n");
        status.marked.insert("src/a.rs".into());
        status.marked.insert("src/b.rs".into());
        // Simulate `git status` no longer reporting b.rs (it was committed).
        status.entries.retain(|e| e.path != "src/b.rs");
        let live: HashSet<String> = status.entries.iter().map(|e| e.path.clone()).collect();
        status.marked.retain(|p| live.contains(p));
        assert_eq!(
            status.marked.iter().cloned().collect::<Vec<_>>(),
            vec!["src/a.rs".to_string()]
        );
    }

    // --- vc-edit-next-command / remote selection ----------------------------

    /// The `!` prompt hands the edited line back as argv: quoting has to survive,
    /// or a path with a space would silently become two arguments.
    #[test]
    fn split_argv_honours_quotes_and_escapes() {
        assert_eq!(split_argv("push origin main"), ["push", "origin", "main"]);
        assert_eq!(split_argv("   add   -A  "), ["add", "-A"]);
        assert_eq!(
            split_argv("add -- 'my file.txt'"),
            ["add", "--", "my file.txt"]
        );
        assert_eq!(
            split_argv(r#"commit -m "a b" --amend"#),
            ["commit", "-m", "a b", "--amend"]
        );
        assert_eq!(split_argv(r"add my\ file.txt"), ["add", "my file.txt"]);
        assert!(split_argv("   ").is_empty());
    }

    /// `!` is one-shot and only intercepts keys that run git: it presents the
    /// argv the key would have run, and nothing for a key that runs no command.
    #[test]
    fn edit_next_command_offers_the_argv_a_key_would_run() {
        let mut status = fake_status(" M src/a.rs\n");
        assert_eq!(
            status.pending_git_argv(key!('s')),
            Some(vec![
                "add".to_string(),
                "--".to_string(),
                "src/a.rs".to_string()
            ])
        );
        assert_eq!(
            status.pending_git_argv(key!('P')),
            Some(vec!["push".to_string()])
        );
        // A rebase key is only a command while a rebase is in progress.
        assert_eq!(status.pending_git_argv(key!('r')), None);
        // Movement and sub-views run no git command, so `!` just drops.
        assert_eq!(status.pending_git_argv(key!('j')), None);
        assert_eq!(status.pending_git_argv(key!('l')), None);
        // A picked remote is part of the command that is offered for editing.
        status.set_remote("upstream".into());
        assert_eq!(
            status.pending_git_argv(key!('F')),
            Some(vec!["fetch".to_string(), "upstream".to_string()])
        );
    }

    /// Without a picked remote the argv stays bare, so git resolves the branch's
    /// configured default instead of us guessing `origin`.
    #[test]
    fn remote_args_name_the_remote_only_once_picked() {
        let mut status = fake_status("");
        assert_eq!(status.remote_args("pull"), ["pull".to_string()]);
        status.set_remote("origin".into());
        assert_eq!(
            status.remote_args("pull"),
            ["pull".to_string(), "origin".to_string()]
        );
    }

    /// `git remote -v` prints a fetch *and* a push line per remote; the picker
    /// must list each remote once.
    #[test]
    fn parse_remotes_keeps_one_row_per_remote() {
        let out = "origin\tgit@github.com:o/r.git (fetch)\n\
                   origin\tgit@github.com:o/r.git (push)\n\
                   upstream\thttps://example.invalid/u.git (fetch)\n\
                   upstream\thttps://example.invalid/push.git (push)\n";
        let remotes = parse_remotes(out);
        assert_eq!(
            remotes.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["origin", "upstream"]
        );
        assert_eq!(remotes[1].url, "https://example.invalid/u.git");
    }

    #[test]
    fn listing_names_one_file_and_counts_many() {
        assert_eq!(listing(&["a.rs".to_string()]), "a.rs");
        assert_eq!(listing(&[]), "nothing");
        let many: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(listing(&many), "4 files (a, b, c, …)");
        let three: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(listing(&three), "3 files (a, b, c)");
    }

    #[test]
    fn parse_forge_topics_classifies_issues_and_prs() {
        let issues = r#"[
            {"number": 12, "title": "a bug", "url": "https://github.com/o/r/issues/12",
             "labels": [{"name": "bug"}, {"name": "help wanted"}]}
        ]"#;
        let prs = r#"[
            {"number": 34, "title": "a fix", "url": "https://github.com/o/r/pull/34",
             "isDraft": true, "labels": []}
        ]"#;
        let topics = parse_forge_topics(issues, prs);
        assert_eq!(topics.len(), 2);
        assert_eq!(topics[0].kind, TopicKind::Issue);
        assert_eq!(topics[0].number, 12);
        assert_eq!(topics[0].labels, vec!["bug", "help wanted"]);
        assert!(!topics[0].is_draft);
        assert_eq!(topics[1].kind, TopicKind::Pr);
        assert_eq!(topics[1].number, 34);
        assert!(topics[1].is_draft);
        assert_eq!(topics[1].url, "https://github.com/o/r/pull/34");
    }

    #[test]
    fn parse_forge_topics_skips_rows_without_number_and_tolerates_junk() {
        // A row missing `number` is dropped; malformed JSON yields nothing.
        let issues = r#"[{"title": "no number"}, {"number": 7, "title": "keep"}]"#;
        let topics = parse_forge_topics(issues, "not json");
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].number, 7);
        assert!(topics[0].labels.is_empty());
    }

    #[test]
    fn parse_forge_posts_reads_id_author_body() {
        let json = r#"[
            {"id": 100, "user": {"login": "alice"}, "body": "first reply"},
            {"id": 101, "user": {"login": "bob"}, "body": "second"}
        ]"#;
        // The opening post (no id) is pushed by the caller first.
        let mut posts = vec![ForgePost {
            id: None,
            author: "author".into(),
            body: "op".into(),
        }];
        parse_forge_posts(json, &mut posts);
        assert_eq!(posts.len(), 3);
        assert_eq!(posts[0].id, None);
        assert_eq!(posts[1].id, Some(100));
        assert_eq!(posts[1].author, "alice");
        assert_eq!(posts[2].id, Some(101));
    }

    #[test]
    fn note_key_tags_kind_and_number() {
        assert_eq!(note_key(TopicKind::Issue, 12), "issue:12");
        assert_eq!(note_key(TopicKind::Pr, 34), "pr:34");
    }

    #[test]
    fn body_to_lines_normalises_crlf_and_trailing_newline() {
        // GitHub bodies are CRLF and `--jq .body` appends a trailing newline.
        assert_eq!(body_to_lines("first\r\nsecond\n"), vec!["first", "second"]);
        // A blank interior line is preserved; only the final newline is dropped.
        assert_eq!(body_to_lines("a\n\nb\n"), vec!["a", "", "b"]);
        // An empty body is a single blank line, never an empty vector.
        assert_eq!(body_to_lines(""), vec![""]);
        assert_eq!(body_to_lines("no trailing newline"), vec!["no trailing newline"]);
    }
}
