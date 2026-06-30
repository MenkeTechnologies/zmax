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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
}

/// The full-screen interactive magit-status overlay.
pub struct MagitStatus {
    /// Absolute path of the repository root (`git rev-parse --show-toplevel`).
    repo_dir: PathBuf,
    /// Current branch (or a short detached-HEAD description).
    head: String,
    /// All change rows, grouped/ordered by section.
    entries: Vec<StatusEntry>,
    /// Index into `entries` of the highlighted row.
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
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
    }

    /// Run a mutating `git -C <repo> …` command, returning the trimmed stderr on
    /// failure.
    fn run_git(&self, args: &[&str]) -> Result<(), String> {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.repo_dir);
        for a in args {
            cmd.arg(a);
        }
        match cmd.output() {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn selected_entry(&self) -> Option<&StatusEntry> {
        self.entries.get(self.selected)
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
            }
        }
        rows
    }

    /// Move the selection by `delta`, clamped to the entry range.
    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as isize - 1;
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
            key!('G') | key!(End) => self.selected = self.entries.len().saturating_sub(1),
            key!(Home) => self.selected = 0,
            key!('s') => self.stage_selected(cx),
            key!('u') => self.unstage_selected(cx),
            key!('S') => self.stage_all(cx),
            key!('U') => self.unstage_all(cx),
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
            "s stage  u unstage  X discard  c commit  a amend  l log  P push  F fetch  p pull  g refresh  q quit";
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

        let rows = self.rows();
        // Keep the selected file row inside the viewport.
        if let Some(sel_row) = rows.iter().position(|r| {
            matches!(r, Row::File(i) if *i == self.selected) && !self.entries.is_empty()
        }) {
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
            match row {
                Row::Blank => {}
                Row::Info(text) => {
                    surface.set_stringn(area.x, y, text, area.width as usize, info_style);
                }
                Row::Header(text) => {
                    surface.set_stringn(area.x, y, text, area.width as usize, header_style);
                }
                Row::File(i) => {
                    let entry = &self.entries[*i];
                    let style = match entry.section {
                        Section::Untracked => plus_style,
                        Section::Unstaged => minus_style,
                        Section::Staged => plus_style,
                        Section::Conflict => conflict_style,
                    };
                    if *i == self.selected {
                        surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
                    }
                    let line = format!("  {} {}", entry.code(), entry.path);
                    let style = if *i == self.selected {
                        sel_style
                    } else {
                        style
                    };
                    surface.set_stringn(area.x, y, &line, area.width as usize, style);
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
}
