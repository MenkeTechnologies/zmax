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
//! unstage-all, `c` commit (single-line prompt), `Enter` visit the file (a
//! conflict row opens the `:merge` resolver), `g` refresh, `q`/`Esc` close.

use std::path::{Path, PathBuf};
use std::process::Command;

use tui::buffer::Buffer as Surface;
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
        };
        view.refresh();
        Some(view)
    }

    /// Re-read `git status` + the current branch and rebuild the section list,
    /// clamping the selection to the new entry count.
    fn refresh(&mut self) {
        self.head = git_head(&self.repo_dir);
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

    /// Build the linear list of rendered rows from the current entries.
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        rows.push(Row::Info(format!("On branch {}", self.head)));
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
        Some(Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            compositor.pop();
            if let Err(err) = cx.editor.open(&abs, Action::Replace) {
                cx.editor
                    .set_error(format!("failed to open {}: {err}", abs.display()));
                return;
            }
            if entry.section == Section::Conflict {
                crate::commands::typed::open_merge(cx.editor, cx.jobs);
            }
        }))
    }

    /// Build the commit prompt callback: if nothing is staged, status-message
    /// and do nothing; otherwise push a single-line prompt that runs
    /// `git commit -m <msg>` and refreshes this buffer.
    fn commit_callback(&self) -> Callback {
        let has_staged = self.entries.iter().any(|e| e.section == Section::Staged);
        let repo_dir = self.repo_dir.clone();
        Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            if !has_staged {
                cx.editor.set_status("nothing staged to commit");
                return;
            }
            let prompt = crate::ui::Prompt::new(
                "commit message: ".into(),
                None,
                |_editor, _input| Vec::new(),
                move |cx: &mut Context, input: &str, event| {
                    if event != crate::ui::PromptEvent::Validate {
                        return;
                    }
                    let msg = input.trim();
                    if msg.is_empty() {
                        cx.editor.set_status("aborted: empty commit message");
                        return;
                    }
                    let out = Command::new("git")
                        .arg("-C")
                        .arg(&repo_dir)
                        .args(["commit", "-m", msg])
                        .output();
                    match out {
                        Ok(o) if o.status.success() => {
                            let summary = String::from_utf8_lossy(&o.stdout);
                            let first = summary.lines().next().unwrap_or("committed");
                            cx.editor.set_status(format!("commit: {}", first.trim()));
                        }
                        Ok(o) => cx.editor.set_error(format!(
                            "git commit: {}",
                            String::from_utf8_lossy(&o.stderr).trim()
                        )),
                        Err(e) => cx.editor.set_error(format!("git commit: {e}")),
                    }
                    // Refresh the magit buffer once the commit settles.
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
                },
            );
            compositor.push(Box::new(prompt));
        })
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
            key!('c') => return EventResult::Consumed(Some(self.commit_callback())),
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
        let hint = "s stage  u unstage  X discard  S/U all  c commit  Enter visit  g refresh  q quit";
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
        if let Some(sel_row) = rows.iter().position(
            |r| matches!(r, Row::File(i) if *i == self.selected) && !self.entries.is_empty(),
        ) {
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
                    let style = if *i == self.selected { sel_style } else { style };
                    surface.set_stringn(area.x, y, &line, area.width as usize, style);
                }
            }
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("magit")
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
}
