//! The terminal IDE workbench: the full JetBrains-style panel set rendered inside
//! `EditorView`. Carves the editor area into a left column (PROJECT tree + STRUCTURE
//! outline), a bottom PROBLEMS panel, and a right error-stripe — all keyboard- and
//! mouse-driven, all fed from in-process editor state (no PTY bridge).
//!
//! Keys: F2 toggle · Tab cycle focus · Esc → editor · j/k or wheel move · Enter/click activate.

use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zemacs_core::{diagnostic::Severity, Selection};
use zemacs_view::{
    graphics::Rect,
    input::{MouseButton, MouseEvent, MouseEventKind},
    keyboard::{KeyCode, KeyModifiers},
    input::KeyEvent,
    DocumentId,
};

use super::file_tree::{FileTree, TreeAction};

const LEFT_W: u16 = 32;
const BOTTOM_H: u16 = 8;
const STRIPE_W: u16 = 14; // right-pane minimap width

/// Display width of a string (treats emoji codepoints as 2 cells) for right-aligning the toolbar.
fn disp_width(s: &str) -> u16 {
    s.chars()
        .map(|c| if (c as u32) >= 0x1F000 { 2 } else { 1 })
        .sum()
}

/// Split `line` into display-width-`w` chunks (char-boundary safe) for soft-wrapping run output.
fn wrap_chunks(line: &str, w: usize) -> Vec<&str> {
    if line.is_empty() || w == 0 {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut cw = 0usize;
    for (idx, ch) in line.char_indices() {
        let chw = if (ch as u32) >= 0x1F000 { 2 } else { 1 };
        if cw + chw > w && idx > start {
            chunks.push(&line[start..idx]);
            start = idx;
            cw = 0;
        }
        cw += chw;
    }
    if start < line.len() {
        chunks.push(&line[start..]);
    }
    chunks
}

#[derive(PartialEq, Clone, Copy)]
enum Focus {
    Editor,
    Project,
    Structure,
    Problems,
}

#[derive(PartialEq, Clone, Copy)]
enum BottomTab {
    Problems,
    Run,
    Git,
    Registers,
    Todo,
    Marks,
    Jumplist,
    Recent,
    Harpoon,
}

#[derive(Clone, Copy)]
enum BottomHit {
    TabProblems,
    TabRun,
    TabGit,
    TabRegisters,
    TabTodo,
    TabMarks,
    TabJumplist,
    TabRecent,
    TabHarpoon,
    Rerun,
    Stop,
    Clear,
}

pub enum IdeAction {
    None,
    OpenFile(PathBuf),
    /// Open a file and place the cursor on a 1-based line (run-output jump).
    OpenFileAt { path: PathBuf, line: usize },
    Goto { from: usize, to: usize },
    /// Run/debug toolbar actions that need editor/compositor access.
    RunStart,
    Debug,
    /// Open the Run/Debug Configurations manager.
    RunConfigManager,
    /// Open the Preferences page on a specific tab (0=Settings … 4=Help).
    OpenPrefs(usize),
    /// Paste a register's real contents at the cursor (from the Registers tab).
    PasteRegister(char),
    /// Prompt for a message and `git commit` the staged changes.
    GitCommit,
    /// Copy text to the system clipboard (`+` register).
    CopyText(String),
    /// Show `git diff` for a path in the Run console.
    GitDiff(PathBuf),
    /// Show the repo's `git log` graph in the Run console.
    GitLog,
    /// Remote ops streamed into the Run console (magit P/F/f from the status tab).
    GitPush,
    GitPull,
    GitFetch,
    /// Stash / unstash (synchronous + buffer reload); magit `z` from the status tab.
    GitStash,
    GitStashPop,
    /// Branch checkout picker (magit `b` branch from the status tab).
    GitBranchPicker,
    /// Show `git blame` for a path in the Run console.
    GitBlame(PathBuf),
    /// Right-click on a file-tree entry: show a CRUD context menu at (row, col).
    ShowContextMenu {
        path: PathBuf,
        is_dir: bool,
        row: u16,
        col: u16,
    },
}

#[derive(Clone, Copy)]
enum ToolHit {
    Run,
    Stop,
    Rerun,
    Debug,
    Configs,
    Settings,
    Help,
}

struct OutlineRow {
    kind: String,
    name: String,
    start: usize,
    end: usize,
}

struct ProblemRow {
    line: usize,
    start: usize,
    end: usize,
    sev: Severity,
    msg: String,
}

pub struct Ide {
    project: FileTree,
    focus: Focus,
    visible: bool,
    fold_project: bool,
    fold_structure: bool,
    fold_problems: bool,
    /// Whether the right-hand minimap stripe is collapsed to a thin handle.
    fold_minimap: bool,
    left_width: u16,
    left_collapsed: bool,
    resizing_left: bool,
    seam_x: u16,
    left_rail_rect: Rect,
    bottom_height: u16,
    /// Maximize the bottom panel (read long logs/diffs full-height), restorable.
    bottom_zoom: bool,
    resizing_bottom: bool,
    /// True while the user is dragging the minimap to fold it (like the left drawer seam).
    resizing_stripe: bool,

    structure: Vec<OutlineRow>,
    structure_sel: usize,
    structure_state: ratatui::widgets::ListState,
    structure_key: (Option<DocumentId>, usize),
    /// In-panel incremental symbol search (`/`), moves the selection to matches.
    structure_filter: String,
    structure_searching: bool,

    problems: Vec<ProblemRow>,
    problems_sel: usize,
    problems_state: ratatui::widgets::TableState,
    run: Option<crate::ui::run::Run>,
    registers: Vec<(char, String)>,
    todos: Vec<(usize, String)>,
    marks_list: Vec<(usize, String)>,
    /// Jumplist entries: (path if in another doc else None, char pos, label).
    jumplist_rows: Vec<(Option<PathBuf>, usize, String)>,
    /// Recently opened files (newest first).
    recent_rows: Vec<PathBuf>,
    /// Harpoon marks for the current project (pin order).
    harpoon_rows: Vec<PathBuf>,
    bottom_tab: BottomTab,
    bottom_hits: Vec<(u16, u16, BottomHit)>,
    bottom_header_y: u16,
    bottom_divider_y: u16,
    toolbar_rect: Rect,
    toolbar_y: u16,
    toolbar_hits: Vec<(u16, u16, ToolHit)>,
    total_lines: usize,
    view_top_line: usize,
    /// Primary cursor's char offset (for the symbol breadcrumb).
    cursor_char: usize,
    /// Current document's path (for the clickable breadcrumb → reveal-in-tree).
    current_doc_path: Option<PathBuf>,
    /// "Always select opened file": auto-reveal the current buffer in the tree.
    auto_reveal: bool,
    /// Last path auto-revealed, so we only re-reveal on an actual buffer switch.
    last_revealed: Option<PathBuf>,
    /// Harpoon: current file's 1-based mark slot (if pinned) and total marks.
    harpoon_slot: Option<usize>,
    harpoon_total: usize,
    /// Screen x-range of the toolbar breadcrumb, for click hit-testing.
    breadcrumb_hit: (u16, u16),
    view_lines: usize,
    /// Per source line: which columns hold a non-whitespace glyph (for the braille minimap).
    minimap_dots: Vec<Vec<bool>>,
    minimap_key: (Option<DocumentId>, usize),
    /// Git change hunks for the current doc: (start_line, end_line, kind) where
    /// kind 0=added, 1=modified, 2=removed — drawn as overview ticks on the minimap.
    git_hunks: Vec<(u32, u32, u8)>,

    project_rect: Rect,
    structure_rect: Rect,
    problems_rect: Rect,
    stripe_rect: Rect,

    // vim-airline / JetBrains-style bottom status bar
    status_mode: u8, // 0 Normal, 1 Select/Visual, 2 Insert
    status_path: String,
    status_pct: u16,
    status_lncol: (usize, usize),
    status_sel: usize,
    status_sel_lines: usize,
    status_carets: usize,
    status_lang: String,
    status_lsp: bool,
    status_encoding: String,
    status_indent: String,
    status_modified: bool,
    status_branch: String,
    status_branch_dir: Option<std::path::PathBuf>,

    // VCS / Git changes tab (JetBrains "Commit" tool window)
    git_changes: Vec<(String, String, std::path::PathBuf)>, // (XY code, display, abs path)
    git_sel: usize,
    git_last: Option<std::time::Instant>,
    /// Shared keyboard selection for the simple list tabs (Todo/Marks/Jumps/Recent).
    aux_sel: usize,
    /// Keyboard selection for the Registers tab (Enter pastes the register).
    reg_sel: usize,
    /// Run console: maps each rendered body row → source output-line index, so a
    /// click can resolve the `file:line` under the cursor (error navigation).
    run_row_src: Vec<usize>,
    /// Index into the run output's `file:line` references for keyboard next/prev.
    run_error_idx: usize,

    // Run console: total soft-wrapped visual rows last frame (for scroll clamping)
    run_total_vis: usize,
}

fn empty_rect() -> Rect {
    Rect::new(0, 0, 0, 0)
}

impl Ide {
    pub fn new() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        // Watch the project root so the tree live-updates on external changes.
        crate::file_watcher::spawn(root.clone());
        Self {
            project: FileTree::new(root),
            focus: Focus::Project,
            visible: true,
            fold_project: false,
            fold_structure: false,
            fold_problems: false,
            fold_minimap: false,
            left_width: LEFT_W,
            left_collapsed: false,
            resizing_left: false,
            seam_x: u16::MAX,
            left_rail_rect: empty_rect(),
            bottom_height: BOTTOM_H,
            bottom_zoom: false,
            resizing_bottom: false,
            resizing_stripe: false,
            structure: Vec::new(),
            structure_sel: 0,
            structure_state: ratatui::widgets::ListState::default(),
            structure_key: (None, usize::MAX),
            structure_filter: String::new(),
            structure_searching: false,
            problems: Vec::new(),
            problems_sel: 0,
            problems_state: ratatui::widgets::TableState::default(),
            run: None,
            registers: Vec::new(),
            todos: Vec::new(),
            marks_list: Vec::new(),
            jumplist_rows: Vec::new(),
            recent_rows: Vec::new(),
            harpoon_rows: Vec::new(),
            bottom_tab: BottomTab::Problems,
            bottom_hits: Vec::new(),
            bottom_header_y: 0,
            bottom_divider_y: u16::MAX,
            toolbar_rect: empty_rect(),
            toolbar_y: 0,
            toolbar_hits: Vec::new(),
            total_lines: 1,
            view_top_line: 0,
            cursor_char: 0,
            current_doc_path: None,
            auto_reveal: false,
            last_revealed: None,
            harpoon_slot: None,
            harpoon_total: 0,
            breadcrumb_hit: (0, 0),
            view_lines: 0,
            minimap_dots: Vec::new(),
            minimap_key: (None, usize::MAX),
            git_hunks: Vec::new(),
            project_rect: empty_rect(),
            structure_rect: empty_rect(),
            problems_rect: empty_rect(),
            stripe_rect: empty_rect(),
            status_mode: 0,
            status_path: String::new(),
            status_pct: 0,
            status_lncol: (1, 1),
            status_sel: 0,
            status_sel_lines: 0,
            status_carets: 1,
            status_lang: String::new(),
            status_lsp: false,
            status_encoding: String::new(),
            status_indent: String::new(),
            status_modified: false,
            status_branch: String::new(),
            status_branch_dir: None,
            git_changes: Vec::new(),
            git_sel: 0,
            aux_sel: 0,
            reg_sel: 0,
            run_row_src: Vec::new(),
            run_error_idx: usize::MAX,
            git_last: None,
            run_total_vis: 0,
        }
    }

    /// Re-read the project file tree from disk. Called by the filesystem watcher
    /// when files change outside the editor.
    pub fn refresh_tree(&mut self) {
        self.project.refresh();
    }

    /// True while a panel (not the editor) holds focus — editor cursor hidden, keys routed here.
    pub fn capturing(&self) -> bool {
        self.visible && self.focus != Focus::Editor
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Toggle the whole workbench (Zen / focus mode). When re-showing, hand focus
    /// back to the editor so the cursor stays put instead of jumping into a panel.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.focus = Focus::Editor;
        }
    }

    /// Focus the editor but keep the workbench visible (the `--ide` boot state).
    pub fn focus_editor(&mut self) {
        self.focus = Focus::Editor;
    }

    /// Attach a running command to the Run tool window and reveal it.
    pub fn set_run(&mut self, run: crate::ui::run::Run) {
        self.run = Some(run);
        self.bottom_tab = BottomTab::Run;
        self.visible = true;
        self.fold_problems = false;
        self.run_error_idx = usize::MAX;
    }

    /// Jump to the next / previous `file:line` reference in the run output
    /// (vim `:cnext`/`:cprev`). Returns an `OpenFileAt` action, or `None` when
    /// there's no run or no parseable location.
    pub fn goto_run_error(&mut self, forward: bool) -> IdeAction {
        let Some(run) = self.run.clone() else {
            return IdeAction::None;
        };
        let errors: Vec<(PathBuf, usize)> = {
            let Ok(s) = run.lock() else {
                return IdeAction::None;
            };
            let cwd = s.cwd.clone();
            s.lines
                .iter()
                .filter_map(|l| {
                    let (p, line, _) = parse_file_line(l)?;
                    let pb = std::path::Path::new(&p);
                    let abs = if pb.is_absolute() { pb.to_path_buf() } else { cwd.join(pb) };
                    abs.is_file().then_some((abs, line))
                })
                .collect()
        };
        if errors.is_empty() {
            return IdeAction::None;
        }
        let n = errors.len();
        self.run_error_idx = if self.run_error_idx >= n {
            0
        } else if forward {
            (self.run_error_idx + 1) % n
        } else {
            (self.run_error_idx + n - 1) % n
        };
        let (path, line) = errors[self.run_error_idx].clone();
        IdeAction::OpenFileAt { path, line }
    }

    /// Toggle maximizing the bottom panel (full-height for reading long output).
    /// Returns the new state. Reveals + unfolds the panel when maximizing.
    pub fn toggle_bottom_zoom(&mut self) -> bool {
        self.bottom_zoom = !self.bottom_zoom;
        if self.bottom_zoom {
            self.visible = true;
            self.fold_problems = false;
        }
        self.bottom_zoom
    }

    /// Re-run the last command (same cmd/cwd/shell), revealing the Run tab.
    /// Returns false when there's nothing to re-run.
    pub fn rerun_last(&mut self) -> bool {
        let Some(r) = self.run.clone() else {
            return false;
        };
        self.run = Some(crate::ui::run::rerun(&r));
        self.bottom_tab = BottomTab::Run;
        self.visible = true;
        self.fold_problems = false;
        self.run_error_idx = usize::MAX;
        true
    }

    /// Wipe the Run console output (keeps the process running; new output still
    /// streams in) and reveal the Run tab so the cleared console is in view.
    /// Returns false when there's no run to clear.
    pub fn clear_run(&mut self) -> bool {
        let Some(run) = &self.run else { return false };
        if let Ok(mut s) = run.lock() {
            s.lines.clear();
            s.scroll = 0;
            s.follow = true;
        }
        self.bottom_tab = BottomTab::Run;
        self.visible = true;
        self.fold_problems = false;
        true
    }

    /// Focus a workbench panel by name (JetBrains Alt+1/Alt+7 style): reveal the
    /// workbench, unfold the target panel, and route keys to it. Unknown names
    /// and "editor" simply hand focus back to the editor.
    pub fn focus_panel(&mut self, name: &str) {
        self.visible = true;
        match name {
            "project" => {
                self.fold_project = false;
                self.left_collapsed = false;
                self.focus = Focus::Project;
            }
            "structure" => {
                self.fold_structure = false;
                self.left_collapsed = false;
                self.focus = Focus::Structure;
            }
            "problems" => {
                self.fold_problems = false;
                self.bottom_tab = BottomTab::Problems;
                self.focus = Focus::Problems;
            }
            "run" => {
                self.fold_problems = false;
                self.bottom_tab = BottomTab::Run;
                self.focus = Focus::Problems;
            }
            "git" => {
                self.fold_problems = false;
                self.bottom_tab = BottomTab::Git;
                self.focus = Focus::Problems;
            }
            _ => self.focus = Focus::Editor,
        }
    }

    /// Toggle "always select opened file"; returns the new state. When turning
    /// it on, immediately reveal the current buffer.
    pub fn toggle_auto_reveal(&mut self) -> bool {
        self.auto_reveal = !self.auto_reveal;
        if self.auto_reveal {
            self.visible = true;
            self.last_revealed = None; // force a reveal on the next refresh
        }
        self.auto_reveal
    }

    /// Reveal a file in the project tree: show the workbench, focus + unfold the
    /// project panel, and select the file's row (expanding ancestors).
    pub fn reveal(&mut self, path: &std::path::Path) {
        self.visible = true;
        self.fold_project = false;
        self.left_collapsed = false;
        self.focus = Focus::Project;
        self.project.reveal(path);
    }

    /// Snapshot the drawer layout for session persistence.
    pub fn layout(&self) -> crate::appdata::IdeLayout {
        crate::appdata::IdeLayout {
            open: self.visible,
            left_width: self.left_width,
            left_collapsed: self.left_collapsed,
            fold_project: self.fold_project,
            fold_structure: self.fold_structure,
            fold_problems: self.fold_problems,
        }
    }

    /// Restore a persisted drawer layout.
    pub fn apply_layout(&mut self, l: &crate::appdata::IdeLayout) {
        self.visible = l.open;
        self.left_width = if l.left_width >= 14 { l.left_width } else { LEFT_W };
        self.left_collapsed = l.left_collapsed;
        self.fold_project = l.fold_project;
        self.fold_structure = l.fold_structure;
        self.fold_problems = l.fold_problems;
    }

    /// F2: toggle the whole workbench (and focus the tree when showing).
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        self.focus = if self.visible { Focus::Project } else { Focus::Editor };
    }

    fn toggle_fold(&mut self) {
        match self.focus {
            Focus::Project => self.fold_project = !self.fold_project,
            Focus::Structure => self.fold_structure = !self.fold_structure,
            Focus::Problems => self.fold_problems = !self.fold_problems,
            Focus::Editor => {}
        }
    }

    fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Editor | Focus::Problems => Focus::Project,
            Focus::Project => Focus::Structure,
            Focus::Structure => Focus::Problems,
        };
    }

    /// Whether a screen cell falls inside any panel (so the editor ignores that mouse event).
    pub fn hit(&self, col: u16, row: u16) -> bool {
        if self.resizing_left || self.resizing_bottom {
            return true; // capture the whole drag even when it leaves the panel
        }
        if self.seam_x != u16::MAX && col == self.seam_x {
            return true;
        }
        if self.bottom_divider_y != u16::MAX && row == self.bottom_divider_y {
            return true;
        }
        [
            self.project_rect,
            self.structure_rect,
            self.problems_rect,
            self.stripe_rect,
            self.left_rail_rect,
            self.toolbar_rect,
        ]
        .iter()
        .any(|r| in_rect(r, col, row))
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> IdeAction {
        // While the project tree's speed-search is capturing input, route every
        // key to it — otherwise IDE chrome shortcuts (z, <, >, Esc, Tab) would
        // eat the keystrokes instead of building the filter query.
        if self.focus == Focus::Project && self.project.is_filtering() {
            return match self.project.handle_key(key) {
                TreeAction::Open(p) => {
                    self.focus = Focus::Editor;
                    IdeAction::OpenFile(p)
                }
                TreeAction::Close => IdeAction::None,
                TreeAction::None => IdeAction::None,
            };
        }
        // Same for the structure outline's incremental search.
        if self.focus == Focus::Structure && self.structure_searching {
            return self.list_key(key, true);
        }
        match key.code {
            KeyCode::F(2) => {
                self.toggle();
                IdeAction::None
            }
            KeyCode::Tab => {
                self.cycle_focus();
                IdeAction::None
            }
            KeyCode::Esc => {
                self.focus = Focus::Editor;
                IdeAction::None
            }
            // fold/unfold the focused drawer — but not for the bottom panel, where
            // `z` is the magit stash key (routed to list_key below).
            KeyCode::Char('z') if self.focus != Focus::Problems => {
                self.toggle_fold();
                IdeAction::None
            }
            // collapse / expand the left column horizontally
            KeyCode::Char('<') => {
                self.left_collapsed = true;
                IdeAction::None
            }
            KeyCode::Char('>') => {
                self.left_collapsed = false;
                IdeAction::None
            }
            _ => match self.focus {
                Focus::Project => match self.project.handle_key(key) {
                    TreeAction::Open(p) => {
                        self.focus = Focus::Editor;
                        IdeAction::OpenFile(p)
                    }
                    TreeAction::Close => {
                        self.focus = Focus::Editor;
                        IdeAction::None
                    }
                    TreeAction::None => IdeAction::None,
                },
                Focus::Structure => self.list_key(key, true),
                Focus::Problems => self.list_key(key, false),
                Focus::Editor => IdeAction::None,
            },
        }
    }

    /// Move the structure selection to the match nearest the current row (used
    /// while typing the incremental query). Searches forward, wrapping.
    fn structure_seek(&mut self, from_current: bool) {
        if self.structure_filter.is_empty() {
            return;
        }
        let q = self.structure_filter.to_lowercase();
        let n = self.structure.len();
        if n == 0 {
            return;
        }
        let base = if from_current { 0 } else { 1 };
        for off in base..(n + base) {
            let i = (self.structure_sel + off) % n;
            if self.structure[i].name.to_lowercase().contains(&q) {
                self.structure_sel = i;
                return;
            }
        }
    }

    /// Jump to the next / previous symbol matching the active query (`n` / `N`).
    fn structure_seek_dir(&mut self, forward: bool) {
        if self.structure_filter.is_empty() {
            return;
        }
        let q = self.structure_filter.to_lowercase();
        let n = self.structure.len();
        if n == 0 {
            return;
        }
        for off in 1..=n {
            let i = if forward {
                (self.structure_sel + off) % n
            } else {
                (self.structure_sel + n - off) % n
            };
            if self.structure[i].name.to_lowercase().contains(&q) {
                self.structure_sel = i;
                return;
            }
        }
    }

    /// Cycle the active bottom-panel tab (`]` forward / `[` back).
    fn cycle_bottom_tab(&mut self, forward: bool) {
        use BottomTab::*;
        const ORDER: [BottomTab; 9] =
            [Problems, Run, Git, Registers, Todo, Marks, Jumplist, Recent, Harpoon];
        let cur = ORDER.iter().position(|t| *t == self.bottom_tab).unwrap_or(0);
        let n = ORDER.len();
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        self.bottom_tab = ORDER[next];
        self.aux_sel = 0;
    }

    /// Row count of the currently active simple-list tab (Todo/Marks/Jumps/Recent).
    fn aux_len(&self) -> usize {
        match self.bottom_tab {
            BottomTab::Todo => self.todos.len(),
            BottomTab::Marks => self.marks_list.len(),
            BottomTab::Jumplist => self.jumplist_rows.len(),
            BottomTab::Recent => self.recent_rows.len(),
            BottomTab::Harpoon => self.harpoon_rows.len(),
            _ => 0,
        }
    }

    /// Open / jump to the `aux_sel` row of the active simple-list tab (Enter).
    fn activate_aux(&mut self) -> IdeAction {
        match self.bottom_tab {
            BottomTab::Todo => self
                .todos
                .get(self.aux_sel)
                .map(|(pos, _)| IdeAction::Goto { from: *pos, to: *pos })
                .unwrap_or(IdeAction::None),
            BottomTab::Marks => self
                .marks_list
                .get(self.aux_sel)
                .map(|(pos, _)| IdeAction::Goto { from: *pos, to: *pos })
                .unwrap_or(IdeAction::None),
            BottomTab::Jumplist => match self.jumplist_rows.get(self.aux_sel) {
                Some((Some(path), _, _)) if path.is_file() => {
                    self.focus = Focus::Editor;
                    IdeAction::OpenFile(path.clone())
                }
                Some((None, pos, _)) => IdeAction::Goto { from: *pos, to: *pos },
                _ => IdeAction::None,
            },
            BottomTab::Recent => match self.recent_rows.get(self.aux_sel) {
                Some(path) if path.is_file() => {
                    self.focus = Focus::Editor;
                    IdeAction::OpenFile(path.clone())
                }
                _ => IdeAction::None,
            },
            BottomTab::Harpoon => match self.harpoon_rows.get(self.aux_sel) {
                Some(path) if path.is_file() => {
                    self.focus = Focus::Editor;
                    IdeAction::OpenFile(path.clone())
                }
                _ => IdeAction::None,
            },
            _ => IdeAction::None,
        }
    }

    /// Scroll the Run console by `lines` (positive = toward newest output).
    /// Reaching the bottom re-enables tail-follow; scrolling up pins the view.
    /// `isize::MAX` / `isize::MIN` jump to end / start. Matches the wheel model.
    fn scroll_run(&mut self, lines: isize) {
        let Some(run) = &self.run else { return };
        let mut s = run.lock().unwrap();
        let h = self.problems_rect.height.saturating_sub(1) as usize;
        let max_top = self.run_total_vis.saturating_sub(h);
        let cur = if s.follow { max_top } else { s.scroll.min(max_top) };
        let nt = if lines >= 0 {
            cur.saturating_add(lines as usize)
        } else {
            cur.saturating_sub(lines.unsigned_abs())
        };
        if nt >= max_top {
            s.follow = true;
            s.scroll = max_top;
        } else {
            s.follow = false;
            s.scroll = nt;
        }
    }

    fn list_key(&mut self, key: KeyEvent, structure: bool) -> IdeAction {
        // Structure outline: incremental symbol search captures keystrokes.
        if structure && self.structure_searching {
            match key.code {
                KeyCode::Esc => {
                    self.structure_searching = false;
                    self.structure_filter.clear();
                }
                KeyCode::Enter => {
                    self.structure_searching = false;
                    self.focus = Focus::Editor;
                    return self.activate(true);
                }
                KeyCode::Backspace => {
                    self.structure_filter.pop();
                    self.structure_seek(true);
                }
                // Emacs-style match cycling while typing: C-n/C-j next, C-p/C-k prev.
                KeyCode::Char('n') | KeyCode::Char('j')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.structure_seek_dir(true);
                }
                KeyCode::Char('p') | KeyCode::Char('k')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.structure_seek_dir(false);
                }
                KeyCode::Char(c) => {
                    self.structure_filter.push(c);
                    self.structure_seek(true);
                }
                _ => {}
            }
            return IdeAction::None;
        }
        if structure {
            match key.code {
                KeyCode::Char('/') => {
                    self.structure_searching = true;
                    self.structure_filter.clear();
                    return IdeAction::None;
                }
                KeyCode::Char('n') => {
                    self.structure_seek_dir(true);
                    return IdeAction::None;
                }
                KeyCode::Char('N') => {
                    self.structure_seek_dir(false);
                    return IdeAction::None;
                }
                _ => {}
            }
        }
        // Bottom panel: [ and ] cycle between its tabs (Problems/Run/Git/…).
        if !structure && matches!(key.code, KeyCode::Char('[') | KeyCode::Char(']')) {
            self.cycle_bottom_tab(matches!(key.code, KeyCode::Char(']')));
            return IdeAction::None;
        }
        // Run console: the bottom panel scrolls output instead of moving a cursor.
        if !structure && self.bottom_tab == BottomTab::Run {
            let page = self.problems_rect.height.saturating_sub(1).max(1) as isize;
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => self.scroll_run(1),
                KeyCode::Char('k') | KeyCode::Up => self.scroll_run(-1),
                KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_run(page),
                KeyCode::PageUp => self.scroll_run(-page),
                KeyCode::Char('G') | KeyCode::End => self.scroll_run(isize::MAX),
                KeyCode::Char('g') | KeyCode::Home => self.scroll_run(isize::MIN),
                KeyCode::Char('y') => {
                    if let Some(run) = &self.run {
                        let text = run.lock().map(|s| s.lines.join("\n")).unwrap_or_default();
                        if !text.is_empty() {
                            return IdeAction::CopyText(text);
                        }
                    }
                }
                _ => {}
            }
            return IdeAction::None;
        }
        // Git changes: select a file with j/k/g/G and open it with Enter.
        if !structure && self.bottom_tab == BottomTab::Git {
            let len = self.git_changes.len();
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    if self.git_sel + 1 < len {
                        self.git_sel += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.git_sel = self.git_sel.saturating_sub(1);
                }
                KeyCode::Char('g') | KeyCode::Home => self.git_sel = 0,
                KeyCode::Char('G') | KeyCode::End => self.git_sel = len.saturating_sub(1),
                // s stages, u unstages the selected change, then refresh.
                KeyCode::Char('s') => {
                    if let Some((_, _, path)) = self.git_changes.get(self.git_sel) {
                        git_stage(&path.clone(), true);
                        self.git_last = None;
                    }
                }
                KeyCode::Char('u') => {
                    if let Some((_, _, path)) = self.git_changes.get(self.git_sel) {
                        git_stage(&path.clone(), false);
                        self.git_last = None;
                    }
                }
                KeyCode::Char('S') => {
                    git_stage_all(&std::env::current_dir().unwrap_or_default(), true);
                    self.git_last = None;
                }
                KeyCode::Char('U') => {
                    git_stage_all(&std::env::current_dir().unwrap_or_default(), false);
                    self.git_last = None;
                }
                KeyCode::Char('c') => {
                    self.git_last = None;
                    return IdeAction::GitCommit;
                }
                KeyCode::Char('d') => {
                    if let Some((_, _, path)) = self.git_changes.get(self.git_sel) {
                        return IdeAction::GitDiff(path.clone());
                    }
                }
                KeyCode::Char('l') => return IdeAction::GitLog,
                KeyCode::Char('b') => {
                    if let Some((_, _, path)) = self.git_changes.get(self.git_sel) {
                        return IdeAction::GitBlame(path.clone());
                    }
                }
                // magit status-buffer keys for remote / stash operations
                KeyCode::Char('P') => return IdeAction::GitPush,
                KeyCode::Char('F') => return IdeAction::GitPull,
                KeyCode::Char('f') => return IdeAction::GitFetch,
                KeyCode::Char('z') => return IdeAction::GitStash,
                KeyCode::Char('Z') => return IdeAction::GitStashPop,
                KeyCode::Char('B') => return IdeAction::GitBranchPicker,
                KeyCode::Enter => {
                    if let Some((_, _, path)) = self.git_changes.get(self.git_sel) {
                        if path.is_file() {
                            self.focus = Focus::Editor;
                            return IdeAction::OpenFile(path.clone());
                        }
                    }
                }
                _ => {}
            }
            return IdeAction::None;
        }
        // Registers: j/k select, Enter pastes the chosen register at the cursor.
        if !structure && self.bottom_tab == BottomTab::Registers {
            let len = self.registers.len();
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    if self.reg_sel + 1 < len {
                        self.reg_sel += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => self.reg_sel = self.reg_sel.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => self.reg_sel = 0,
                KeyCode::Char('G') | KeyCode::End => self.reg_sel = len.saturating_sub(1),
                KeyCode::Enter => {
                    if let Some((ch, _)) = self.registers.get(self.reg_sel) {
                        let reg = *ch;
                        self.focus = Focus::Editor;
                        return IdeAction::PasteRegister(reg);
                    }
                }
                _ => {}
            }
            return IdeAction::None;
        }
        // Harpoon: K/J reorder the selected mark up/down (other keys fall through
        // to the shared list nav below).
        if !structure && self.bottom_tab == BottomTab::Harpoon {
            if matches!(key.code, KeyCode::Char('K') | KeyCode::Char('J')) {
                let up = matches!(key.code, KeyCode::Char('K'));
                if let Some(path) = self.harpoon_rows.get(self.aux_sel).cloned() {
                    if crate::harpoon::move_mark(&path, up) {
                        self.harpoon_rows = crate::harpoon::list();
                        self.aux_sel = if up {
                            self.aux_sel.saturating_sub(1)
                        } else {
                            (self.aux_sel + 1).min(self.harpoon_rows.len().saturating_sub(1))
                        };
                    }
                }
                return IdeAction::None;
            }
        }
        // Simple list tabs (Todo/Marks/Jumps/Recent): j/k select, Enter activates.
        if !structure
            && matches!(
                self.bottom_tab,
                BottomTab::Todo
                    | BottomTab::Marks
                    | BottomTab::Jumplist
                    | BottomTab::Recent
                    | BottomTab::Harpoon
            )
        {
            let len = self.aux_len();
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    if self.aux_sel + 1 < len {
                        self.aux_sel += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => self.aux_sel = self.aux_sel.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => self.aux_sel = 0,
                KeyCode::Char('G') | KeyCode::End => self.aux_sel = len.saturating_sub(1),
                KeyCode::Enter => return self.activate_aux(),
                _ => {}
            }
            return IdeAction::None;
        }
        let len = if structure { self.structure.len() } else { self.problems.len() };
        let sel = if structure { &mut self.structure_sel } else { &mut self.problems_sel };
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if *sel + 1 < len {
                    *sel += 1;
                }
                IdeAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *sel = sel.saturating_sub(1);
                IdeAction::None
            }
            KeyCode::Enter => {
                self.focus = Focus::Editor;
                self.activate(structure)
            }
            _ => IdeAction::None,
        }
    }

    fn activate(&self, structure: bool) -> IdeAction {
        if structure {
            self.structure
                .get(self.structure_sel)
                .map(|o| IdeAction::Goto { from: o.start, to: o.end })
                .unwrap_or(IdeAction::None)
        } else {
            self.problems
                .get(self.problems_sel)
                .map(|p| IdeAction::Goto { from: p.start, to: p.end })
                .unwrap_or(IdeAction::None)
        }
    }

    pub fn handle_mouse(&mut self, ev: &MouseEvent, line_to_char: impl Fn(usize) -> usize) -> IdeAction {
        let (col, row) = (ev.column, ev.row);
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // top run/debug toolbar
                if row == self.toolbar_y && self.toolbar_rect.height > 0 {
                    let hit = self
                        .toolbar_hits
                        .iter()
                        .find(|&&(a, b, _)| col >= a && col < b)
                        .map(|&(_, _, h)| h);
                    return match hit {
                        Some(ToolHit::Run) => IdeAction::RunStart,
                        Some(ToolHit::Configs) => IdeAction::RunConfigManager,
                        Some(ToolHit::Settings) => IdeAction::OpenPrefs(0),
                        Some(ToolHit::Help) => IdeAction::OpenPrefs(4),
                        Some(ToolHit::Debug) => IdeAction::Debug,
                        Some(ToolHit::Stop) => {
                            if let Some(r) = &self.run {
                                crate::ui::run::stop(r);
                            }
                            IdeAction::None
                        }
                        Some(ToolHit::Rerun) => {
                            self.rerun_last();
                            IdeAction::None
                        }
                        None => {
                            // Clicking the breadcrumb reveals the current file in the tree.
                            let (bx0, bx1) = self.breadcrumb_hit;
                            if bx1 > bx0 && col >= bx0 && col < bx1 {
                                if let Some(path) = self.current_doc_path.clone() {
                                    self.reveal(&path);
                                }
                            }
                            IdeAction::None
                        }
                    };
                }
                // click the collapse rail to re-expand the left column
                if self.left_collapsed && in_rect(&self.left_rail_rect, col, row) {
                    self.left_collapsed = false;
                    return IdeAction::None;
                }
                // grab the resize seam
                if self.seam_x != u16::MAX && col == self.seam_x {
                    self.resizing_left = true;
                    return IdeAction::None;
                }
                // clicking a drawer's header row folds/unfolds it
                if in_rect(&self.project_rect, col, row) && row == self.project_rect.y {
                    self.focus = Focus::Project;
                    self.fold_project = !self.fold_project;
                    return IdeAction::None;
                }
                if in_rect(&self.structure_rect, col, row) && row == self.structure_rect.y {
                    self.focus = Focus::Structure;
                    self.fold_structure = !self.fold_structure;
                    return IdeAction::None;
                }
                if row == self.bottom_divider_y && self.bottom_divider_y != u16::MAX {
                    // grab the visible divider line to resize the drawer
                    self.resizing_bottom = true;
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row) && row == self.bottom_header_y {
                    self.focus = Focus::Problems;
                    // Tabs/controls act immediately on click; resizing is the divider's job.
                    if col < self.problems_rect.x + 2 {
                        self.fold_problems = !self.fold_problems;
                        return IdeAction::None;
                    }
                    let bhit = self
                        .bottom_hits
                        .iter()
                        .find(|&&(a, b, _)| col >= a && col < b)
                        .map(|&(_, _, h)| h);
                    // Clicking any bottom-panel affordance focuses the panel, so the
                    // keyboard (e.g. j/k to scroll the Run console) routes here.
                    if bhit.is_some() {
                        self.focus = Focus::Problems;
                        self.aux_sel = 0;
                        self.reg_sel = 0;
                    }
                    match bhit {
                        Some(BottomHit::TabProblems) => self.bottom_tab = BottomTab::Problems,
                        Some(BottomHit::TabRun) => self.bottom_tab = BottomTab::Run,
                        Some(BottomHit::TabGit) => self.bottom_tab = BottomTab::Git,
                        Some(BottomHit::TabRegisters) => self.bottom_tab = BottomTab::Registers,
                        Some(BottomHit::TabTodo) => self.bottom_tab = BottomTab::Todo,
                        Some(BottomHit::TabMarks) => self.bottom_tab = BottomTab::Marks,
                        Some(BottomHit::TabJumplist) => self.bottom_tab = BottomTab::Jumplist,
                        Some(BottomHit::TabRecent) => self.bottom_tab = BottomTab::Recent,
                        Some(BottomHit::TabHarpoon) => self.bottom_tab = BottomTab::Harpoon,
                        Some(BottomHit::Stop) => {
                            if let Some(r) = &self.run {
                                crate::ui::run::stop(r);
                            }
                        }
                        Some(BottomHit::Rerun) => {
                            self.rerun_last();
                        }
                        Some(BottomHit::Clear) => {
                            self.clear_run();
                        }
                        None => {}
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.project_rect, col, row) && row > self.project_rect.y {
                    self.focus = Focus::Project;
                    let lr = (row - self.project_rect.y - 1) as usize;
                    return match self.project.click_row(lr) {
                        TreeAction::Open(p) => {
                            self.focus = Focus::Editor;
                            IdeAction::OpenFile(p)
                        }
                        _ => IdeAction::None,
                    };
                }
                if in_rect(&self.structure_rect, col, row) && row > self.structure_rect.y {
                    self.focus = Focus::Structure;
                    let idx = self.structure_state.offset() + (row - self.structure_rect.y - 1) as usize;
                    if idx < self.structure.len() {
                        self.structure_sel = idx;
                        let o = &self.structure[idx];
                        return IdeAction::Goto { from: o.start, to: o.end };
                    }
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Todo
                {
                    let idx = (row - self.problems_rect.y - 1) as usize;
                    if let Some((pos, _)) = self.todos.get(idx) {
                        self.aux_sel = idx;
                        return IdeAction::Goto { from: *pos, to: *pos };
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Git
                {
                    let idx = (row - self.problems_rect.y - 1) as usize;
                    if let Some((_, _, path)) = self.git_changes.get(idx) {
                        self.git_sel = idx;
                        if path.is_file() {
                            self.focus = Focus::Editor;
                            return IdeAction::OpenFile(path.clone());
                        }
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Marks
                {
                    let idx = (row - self.problems_rect.y - 1) as usize;
                    if let Some((pos, _)) = self.marks_list.get(idx) {
                        self.aux_sel = idx;
                        return IdeAction::Goto { from: *pos, to: *pos };
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Jumplist
                {
                    let idx = (row - self.problems_rect.y - 1) as usize;
                    if let Some((target, pos, _)) = self.jumplist_rows.get(idx) {
                        self.aux_sel = idx;
                        match target {
                            // Entry in another document: open it.
                            Some(path) if path.is_file() => {
                                self.focus = Focus::Editor;
                                return IdeAction::OpenFile(path.clone());
                            }
                            // Entry in the focused document: jump to it.
                            None => return IdeAction::Goto { from: *pos, to: *pos },
                            _ => {}
                        }
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Recent
                {
                    let idx = (row - self.problems_rect.y - 1) as usize;
                    if let Some(path) = self.recent_rows.get(idx) {
                        self.aux_sel = idx;
                        if path.is_file() {
                            self.focus = Focus::Editor;
                            return IdeAction::OpenFile(path.clone());
                        }
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Run
                {
                    // Click a build-output line with a `file:line` to jump to it.
                    let br = (row - self.problems_rect.y - 1) as usize;
                    let target = self
                        .run_row_src
                        .get(br)
                        .copied()
                        .filter(|&li| li != usize::MAX)
                        .and_then(|li| {
                            let s = self.run.as_ref()?.lock().ok()?;
                            let text = s.lines.get(li)?.clone();
                            let cwd = s.cwd.clone();
                            drop(s);
                            let (p, line, _col) = parse_file_line(&text)?;
                            let pb = std::path::Path::new(&p);
                            let abs = if pb.is_absolute() { pb.to_path_buf() } else { cwd.join(pb) };
                            abs.is_file().then_some((abs, line))
                        });
                    self.focus = Focus::Problems;
                    if let Some((abs, line)) = target {
                        self.focus = Focus::Editor;
                        return IdeAction::OpenFileAt { path: abs, line };
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Harpoon
                {
                    let idx = (row - self.problems_rect.y - 1) as usize;
                    if let Some(path) = self.harpoon_rows.get(idx) {
                        self.aux_sel = idx;
                        if path.is_file() {
                            self.focus = Focus::Editor;
                            return IdeAction::OpenFile(path.clone());
                        }
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Problems
                {
                    self.focus = Focus::Problems;
                    let idx = self.problems_state.offset() + (row - self.problems_rect.y - 1) as usize;
                    if idx < self.problems.len() {
                        self.problems_sel = idx;
                        let p = &self.problems[idx];
                        return IdeAction::Goto { from: p.start, to: p.end };
                    }
                }
                if in_rect(&self.stripe_rect, col, row) && self.stripe_rect.height > 0 {
                    // Collapsed handle → expand.
                    if self.fold_minimap {
                        self.fold_minimap = false;
                        return IdeAction::None;
                    }
                    // Arm a drag-to-fold: dragging the minimap toward its right edge folds
                    // it, just like dragging the left drawer's seam shut. A plain click
                    // (no drag) still navigates below.
                    self.resizing_stripe = true;
                    // Top-right chevron → collapse.
                    let chevron_x = self.stripe_rect.x + self.stripe_rect.width.saturating_sub(1);
                    if row == self.stripe_rect.y && col == chevron_x {
                        self.fold_minimap = true;
                        self.resizing_stripe = false;
                        return IdeAction::None;
                    }
                    let frac = (row - self.stripe_rect.y) as f32 / self.stripe_rect.height as f32;
                    let line = ((frac * self.total_lines as f32) as usize).min(self.total_lines.saturating_sub(1));
                    let pos = line_to_char(line);
                    return IdeAction::Goto { from: pos, to: pos };
                }
                IdeAction::None
            }
            MouseEventKind::Down(MouseButton::Right) => {
                // Right-click on a file-tree entry → CRUD context menu.
                if in_rect(&self.project_rect, col, row) && row > self.project_rect.y {
                    let lr = (row - self.project_rect.y - 1) as usize;
                    if let Some((path, is_dir)) = self.project.path_at_row(lr) {
                        self.focus = Focus::Project;
                        return IdeAction::ShowContextMenu {
                            path,
                            is_dir,
                            row,
                            col,
                        };
                    }
                }
                IdeAction::None
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = matches!(ev.kind, MouseEventKind::ScrollDown);
                if in_rect(&self.project_rect, col, row) {
                    self.project.scroll_sel(down);
                } else if in_rect(&self.structure_rect, col, row) {
                    step(&mut self.structure_sel, self.structure.len(), down);
                } else if in_rect(&self.problems_rect, col, row) && self.bottom_tab == BottomTab::Run {
                    // scroll the run console; reaching the bottom re-enables tail-follow
                    if let Some(run) = &self.run {
                        let mut s = run.lock().unwrap();
                        let h = self.problems_rect.height.saturating_sub(1) as usize;
                        let max_top = self.run_total_vis.saturating_sub(h);
                        let cur = if s.follow { max_top } else { s.scroll.min(max_top) };
                        if down {
                            let nt = cur + 3;
                            if nt >= max_top {
                                s.follow = true;
                                s.scroll = max_top;
                            } else {
                                s.follow = false;
                                s.scroll = nt;
                            }
                        } else {
                            s.follow = false;
                            s.scroll = cur.saturating_sub(3);
                        }
                    }
                } else if in_rect(&self.problems_rect, col, row) {
                    step(&mut self.problems_sel, self.problems.len(), down);
                }
                IdeAction::None
            }
            MouseEventKind::Drag(MouseButton::Left) if self.resizing_left => {
                let origin = self.project_rect.x;
                if col <= origin + 6 {
                    self.left_collapsed = true;
                    self.resizing_left = false;
                } else {
                    self.left_width = (col - origin + 1).clamp(14, 80);
                }
                IdeAction::None
            }
            MouseEventKind::Drag(MouseButton::Left) if self.resizing_bottom => {
                let panel_bottom = self.problems_rect.y + self.problems_rect.height;
                // the dragged row becomes the divider; the panel sits just below it
                self.bottom_height = panel_bottom.saturating_sub(row + 1).clamp(3, 40);
                IdeAction::None
            }
            MouseEventKind::Drag(MouseButton::Left) if self.resizing_stripe => {
                // Dragging the minimap past its half-way point (toward the right edge)
                // folds it to the thin handle, mirroring the left drawer's collapse.
                let half = self.stripe_rect.x + self.stripe_rect.width / 2;
                if col >= half {
                    self.fold_minimap = true;
                    self.resizing_stripe = false;
                }
                IdeAction::None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.resizing_left = false;
                self.resizing_bottom = false;
                self.resizing_stripe = false;
                IdeAction::None
            }
            _ => IdeAction::None,
        }
    }

    /// Render every panel; returns the rect left for the editor.
    pub fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut crate::compositor::Context) -> Rect {
        if !self.visible {
            return area;
        }
        self.refresh(cx);

        let full = area;
        let mut rest = area;

        // bottom-most: JetBrains status bar spans the full width below everything else
        let statusbar = if rest.height > 6 {
            let sb = Rect::new(rest.x, rest.y + rest.height - 1, rest.width, 1);
            rest = Rect::new(rest.x, rest.y, rest.width, rest.height - 1);
            Some(sb)
        } else {
            None
        };

        // left column: project (top) + structure (bottom); drag the seam to resize, collapse to a rail
        if self.left_collapsed {
            self.left_rail_rect = Rect::new(rest.x, rest.y, 1, rest.height);
            self.project_rect = empty_rect();
            self.structure_rect = empty_rect();
            self.seam_x = u16::MAX;
            rest = Rect::new(rest.x + 1, rest.y, rest.width.saturating_sub(1), rest.height);
        } else if rest.width > self.left_width + 24 {
            self.left_rail_rect = empty_rect();
            let content_w = self.left_width.saturating_sub(1).max(1);
            let col_h = rest.height;
            let (ph, sh) = match (self.fold_project, self.fold_structure) {
                (true, true) => (1, 1),
                (true, false) => (1, col_h.saturating_sub(1)),
                (false, true) => (col_h.saturating_sub(1), 1),
                (false, false) => {
                    let p = (col_h * 6 / 10).max(3);
                    (p, col_h - p)
                }
            };
            self.project_rect = Rect::new(rest.x, rest.y, content_w, ph);
            self.structure_rect = Rect::new(rest.x, rest.y + ph, content_w, sh);
            self.seam_x = rest.x + content_w;
            rest = Rect::new(rest.x + self.left_width, rest.y, rest.width - self.left_width, rest.height);
        } else {
            self.left_rail_rect = empty_rect();
            self.project_rect = empty_rect();
            self.structure_rect = empty_rect();
            self.seam_x = u16::MAX;
        }

        // right minimap pane (collapses to a 1-col handle when folded)
        if self.fold_minimap && rest.width > 4 {
            self.stripe_rect = Rect::new(rest.x + rest.width - 1, rest.y, 1, rest.height);
            rest = Rect::new(rest.x, rest.y, rest.width - 1, rest.height);
        } else if !self.fold_minimap && rest.width > STRIPE_W + 30 {
            self.stripe_rect = Rect::new(rest.x + rest.width - STRIPE_W, rest.y, STRIPE_W, rest.height);
            rest = Rect::new(rest.x, rest.y, rest.width - STRIPE_W, rest.height);
        } else {
            self.stripe_rect = empty_rect();
        }

        // bottom problems — a visible divider line above it is the resize handle (like the left seam)
        let bh = if self.fold_problems {
            1
        } else if self.bottom_zoom {
            // maximize: leave a thin strip (toolbar + a few editor lines) on top
            rest.height.saturating_sub(8).max(self.bottom_height)
        } else {
            self.bottom_height
        };
        if rest.height > bh + 5 {
            self.problems_rect = Rect::new(rest.x, rest.y + rest.height - bh, rest.width, bh);
            self.bottom_divider_y = rest.y + rest.height - bh - 1;
            rest = Rect::new(rest.x, rest.y, rest.width, rest.height - bh - 1);
        } else {
            self.problems_rect = empty_rect();
            self.bottom_divider_y = u16::MAX;
        }

        // top run/debug toolbar — a 1-row strip above the editor (always visible in IDE mode)
        if rest.height > 3 {
            self.toolbar_rect = Rect::new(rest.x, rest.y, rest.width, 1);
            rest = Rect::new(rest.x, rest.y + 1, rest.width, rest.height - 1);
        } else {
            self.toolbar_rect = empty_rect();
        }

        self.view_lines = rest.height as usize;

        let theme = &cx.editor.theme;
        if self.toolbar_rect.height > 0 {
            self.render_toolbar(surface, theme);
        }
        if self.project_rect.height > 0 {
            surface.clear_with(self.project_rect, theme.get("ui.background"));
            draw_header(surface, self.project_rect, "PROJECT", self.fold_project, self.focus == Focus::Project, theme);
            if !self.fold_project && self.project_rect.height > 1 {
                self.project.render(body_rect(self.project_rect), surface, theme);
            }
        }
        if self.structure_rect.height > 0 {
            self.render_structure(surface, theme);
        }
        if self.problems_rect.height > 0 {
            self.render_bottom(surface, theme);
        }
        if self.stripe_rect.height > 0 {
            if self.fold_minimap {
                // Collapsed: a thin clickable handle ("‹" = click to expand).
                let st = theme.get("ui.window");
                for y in self.stripe_rect.y..self.stripe_rect.y + self.stripe_rect.height {
                    surface.set_stringn(self.stripe_rect.x, y, "‹", 1, st);
                }
            } else {
                self.render_stripe(surface, theme);
                // Fold chevron at the top-right corner ("›" = click to collapse).
                let chevron = theme.get("comment");
                let cx_col = self.stripe_rect.x + self.stripe_rect.width.saturating_sub(1);
                surface.set_stringn(cx_col, self.stripe_rect.y, "›", 1, chevron);
            }
        }

        // visible drag handle: a horizontal divider line above the bottom drawer
        if self.bottom_divider_y != u16::MAX && self.problems_rect.width > 0 {
            let grip = if self.resizing_bottom {
                theme.get("ui.text.focus")
            } else {
                theme.get("ui.window")
            };
            let w = self.problems_rect.width as usize;
            // a centred ⣿ "grip" makes the draggable line obvious
            let mut line = "─".repeat(w);
            if w >= 5 {
                let mid = w / 2 - 1;
                line.replace_range(
                    line.char_indices().nth(mid).map(|(i, _)| i).unwrap_or(0)
                        ..line.char_indices().nth(mid + 3).map(|(i, _)| i).unwrap_or(line.len()),
                    "⣿⣿⣿",
                );
            }
            surface.set_stringn(self.problems_rect.x, self.bottom_divider_y, &line, w, grip);
            // Overlay a context hint for the focused tab's list_key shortcuts
            // (these aren't in which-key) on the left of the divider line.
            if self.focus == Focus::Problems {
                let hint = match self.bottom_tab {
                    BottomTab::Git => " s/u stage · c commit · P push · F pull · f fetch · z stash · B branch · d diff · l log · b blame ",
                    BottomTab::Run => " j/k/g/G scroll · y copy · [ ] tabs ",
                    BottomTab::Registers => " ↵ paste register · [ ] tabs ",
                    BottomTab::Problems => " ↵ jump · [ ] tabs ",
                    BottomTab::Harpoon => " ↵ open · K/J reorder · [ ] tabs ",
                    _ => " ↵ open · [ ] tabs ",
                };
                let maxw = (w / 2).saturating_sub(2);
                surface.set_stringn(self.problems_rect.x + 1, self.bottom_divider_y, hint, maxw, theme.get("comment"));
            }
        }

        // resize seam / collapse rail down the left edge
        if self.left_collapsed && self.left_rail_rect.height > 0 {
            surface.set_string(self.left_rail_rect.x, full.y, "›", theme.get("ui.text.focus"));
            for y in 1..full.height {
                surface.set_string(self.left_rail_rect.x, full.y + y, "▏", theme.get("ui.window"));
            }
        } else if self.seam_x != u16::MAX {
            let style = theme.get("ui.window");
            for y in 0..full.height {
                surface.set_string(self.seam_x, full.y + y, "│", style);
            }
        }

        if let Some(sb) = statusbar {
            self.render_statusbar(surface, theme, sb);
        }

        rest
    }

    /// vim-airline powerline status bar: ❮mode❯❮paste❯❮⎇ branch❯❮path❯ … ❮ft❯❮enc❯❮pos❯.
    /// Segments are coloured pills joined by powerline separators ( / ), mode colour by Normal/
    /// Insert/Visual, just like the classic airline theme.
    fn render_statusbar(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, area: Rect) {
        use zemacs_view::graphics::{Color, Modifier, Style};

        const SEP_R: &str = "\u{e0b0}"; //  solid right separator
        const SEP_L: &str = "\u{e0b2}"; //  solid left separator
        const GIT: &str = "\u{e0a0}"; //  branch glyph
        const LN: &str = "\u{e0a1}"; //  line-number glyph

        // Colours come from the active theme's statusline scopes; the RGB values are only
        // fallbacks for themes that leave a given scope unstyled.
        let bgfg = |style: Style, fb_bg: Color, fb_fg: Color| {
            (style.bg.unwrap_or(fb_bg), style.fg.unwrap_or(fb_fg))
        };
        let (mode_txt, mode_scope, fb_mode) = match self.status_mode {
            2 => ("INSERT", "ui.statusline.insert", Color::Rgb(0x00, 0xb3, 0xd7)),
            1 => ("VISUAL", "ui.statusline.select", Color::Rgb(0xff, 0x8c, 0x00)),
            _ => ("NORMAL", "ui.statusline.normal", Color::Rgb(0x9e, 0xd0, 0x10)),
        };
        let blackfg = Color::Rgb(0x10, 0x12, 0x16);
        let (mode_bg, mode_fg) = bgfg(theme.get(mode_scope), fb_mode, blackfg);
        let (gray, grayfg) = bgfg(
            theme.get("ui.statusline"),
            Color::Rgb(0x45, 0x45, 0x4d),
            Color::Rgb(0xd2, 0xd2, 0xd8),
        );
        let (dark, darkfg) = bgfg(
            theme.get("ui.statusline.inactive"),
            Color::Rgb(0x28, 0x28, 0x2f),
            Color::Rgb(0x9c, 0x9c, 0xa6),
        );
        let warn = theme.get("warning").fg.unwrap_or(Color::Rgb(0x7a, 0xa8, 0x10));
        let fill = theme.get("ui.statusline").bg.unwrap_or(Color::Rgb(0x1b, 0x1b, 0x20));
        let seg = |bg: Color, fg: Color| Style::default().bg(bg).fg(fg);

        surface.clear_with(area, seg(fill, darkfg));
        let bold = Modifier::BOLD;

        // ── left segments ──────────────────────────────────────────────
        let mut left: Vec<(String, Style)> = Vec::new();
        left.push((format!(" {mode_txt} "), seg(mode_bg, mode_fg).add_modifier(bold)));
        if self.status_modified {
            // airline's secondary section (where PASTE/spell live) — here: modified flag
            left.push((" + ".to_string(), seg(warn, mode_fg).add_modifier(bold)));
        }
        if !self.status_branch.is_empty() {
            left.push((format!(" {GIT} {} ", self.status_branch), seg(gray, grayfg)));
        }
        if !self.status_path.is_empty() {
            left.push((format!(" {} ", self.status_path), seg(dark, darkfg)));
        }

        // ── right segments (display order left→right) ──────────────────
        let mut right: Vec<(String, Style)> = Vec::new();
        if self.harpoon_total > 0 {
            let label = match self.harpoon_slot {
                Some(n) => format!(" ⚓ {}/{} ", n, self.harpoon_total),
                None => format!(" ⚓ {} ", self.harpoon_total),
            };
            right.push((label, seg(gray, grayfg)));
        }
        // selection / multi-caret stats (only when meaningful)
        if self.status_carets > 1 {
            right.push((
                format!(" {} ⌶ ", self.status_carets),
                seg(warn, mode_fg).add_modifier(bold),
            ));
        } else if self.status_mode == 1 && self.status_sel > 0 {
            let lines = self.status_sel_lines.max(1);
            right.push((
                format!(" {}L {} sel ", lines, self.status_sel),
                seg(warn, mode_fg).add_modifier(bold),
            ));
        }
        if !self.status_lang.is_empty() {
            right.push((format!(" {} ", self.status_lang), seg(dark, darkfg)));
        }
        if !self.status_encoding.is_empty() {
            right.push((format!(" {} ", self.status_encoding), seg(gray, grayfg)));
        }
        let (ln, co) = self.status_lncol;
        right.push((
            format!(" {}%  {LN} {}:{} ", self.status_pct, ln, co),
            seg(mode_bg, mode_fg).add_modifier(bold),
        ));

        let right_edge = area.x + area.width;

        // draw left, separators point right () into the next segment's bg
        let mut x = area.x;
        for i in 0..left.len() {
            let (text, style) = &left[i];
            if x >= right_edge {
                break;
            }
            let avail = (right_edge - x) as usize;
            surface.set_stringn(x, area.y, text, avail, *style);
            x += (disp_width(text) as u16).min(right_edge - x);
            if x >= right_edge {
                break;
            }
            let next_bg = left.get(i + 1).and_then(|(_, s)| s.bg).unwrap_or(fill);
            surface.set_stringn(x, area.y, SEP_R, 1, Style::default().fg(style.bg.unwrap_or(fill)).bg(next_bg));
            x += 1;
        }

        // draw right→left, separators point left () with the segment's bg as fg
        let mut rx = right_edge;
        for i in (0..right.len()).rev() {
            let (text, style) = &right[i];
            let w = disp_width(text) as u16;
            if rx <= x + w {
                break; // would collide with the left cluster
            }
            rx -= w;
            surface.set_stringn(rx, area.y, text, w as usize, *style);
            if rx <= x {
                break;
            }
            rx -= 1;
            let left_bg = if i == 0 { fill } else { right[i - 1].1.bg.unwrap_or(fill) };
            surface.set_stringn(rx, area.y, SEP_L, 1, Style::default().fg(style.bg.unwrap_or(fill)).bg(left_bg));
        }
    }

    fn refresh(&mut self, cx: &mut crate::compositor::Context) {
        // The focused tree node is not guaranteed to be a View — e.g. transiently
        // during startup/session-restore, or after a buffer that failed to open.
        // All the work below dereferences the current view (`doc!`, jumplist, etc.),
        // so bail out rather than panic in `tree.get(focus)` when there isn't one.
        if cx.editor.tree.try_get(cx.editor.tree.focus).is_none() {
            return;
        }

        // LOTR (Lord Of The Registers): live snapshot of every register.
        self.registers = cx
            .editor
            .registers
            .iter_preview()
            .map(|(c, s)| (c, s.replace('\n', "↵").chars().take(120).collect()))
            .collect();

        let doc = doc!(cx.editor);
        let key = (Some(doc.id()), doc.text().len_chars());
        // Recompute on change, and keep retrying while empty — the syntax tree loads
        // asynchronously after a file opens, so the first pass often has no symbols yet.
        if key != self.structure_key || self.structure.is_empty() {
            let loader = cx.editor.syn_loader.load();
            let doc = doc!(cx.editor);
            self.structure = crate::commands::syntax::document_outline(doc, &loader)
                .into_iter()
                .map(|o| OutlineRow { kind: o.kind, name: o.name, start: o.start, end: o.end })
                .collect();
            self.structure_key = key;
            if self.structure_sel >= self.structure.len() {
                self.structure_sel = 0;
            }
        }
        let doc = doc!(cx.editor);
        self.total_lines = doc.text().len_lines().max(1);

        // git change hunks for the minimap overview (reuses the diff the gutter uses)
        self.git_hunks.clear();
        if let Some(handle) = doc.diff_handle() {
            let diff = handle.load();
            for i in 0..diff.len() {
                let h = diff.nth_hunk(i);
                let kind = if h.is_pure_insertion() {
                    0
                } else if h.is_pure_removal() {
                    2
                } else {
                    1
                };
                self.git_hunks.push((h.after.start, h.after.end, kind));
            }
        }

        // minimap density (recomputed only when the buffer changes): per line, per column,
        // whether there's a non-whitespace glyph. 2 columns per minimap cell (braille).
        let mkey = (Some(doc.id()), doc.text().len_chars());
        if mkey != self.minimap_key {
            let text = doc.text();
            let cols = STRIPE_W as usize * 2;
            self.minimap_dots = (0..text.len_lines())
                .map(|i| {
                    text.line(i)
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(cols)
                        .map(|c| !c.is_whitespace())
                        .collect::<Vec<bool>>()
                })
                .collect();

            // TODO tool window: scan for TODO/FIXME/… markers.
            const MARKERS: [&str; 6] = ["TODO", "FIXME", "HACK", "XXX", "BUG", "NOTE"];
            self.todos.clear();
            for i in 0..text.len_lines() {
                let line: String = text.line(i).chars().filter(|c| !c.is_control()).collect();
                if MARKERS.iter().any(|m| line.contains(m)) {
                    self.todos.push((text.line_to_char(i), format!("{}: {}", i + 1, line.trim())));
                }
            }

            self.minimap_key = mkey;
        }
        self.problems = doc
            .diagnostics()
            .iter()
            .map(|d| ProblemRow {
                line: d.line,
                start: d.range.start,
                end: d.range.end,
                sev: d.severity.unwrap_or(Severity::Hint),
                msg: d.message.clone(),
            })
            .collect();
        if self.problems_sel >= self.problems.len() {
            self.problems_sel = 0;
        }

        // marks list (vim :marks) — recomputed each frame; marks change without a text edit.
        {
            let text = doc.text();
            let mut marks: Vec<(char, usize)> = doc.marks_iter().collect();
            marks.sort_by_key(|(c, _)| *c);
            self.marks_list = marks
                .into_iter()
                .map(|(c, pos)| {
                    let p = pos.min(text.len_chars());
                    let line = text.char_to_line(p);
                    let lt: String =
                        text.line(line).chars().filter(|ch| !ch.is_control()).collect();
                    (p, format!("'{c}  {}: {}", line + 1, lt.trim()))
                })
                .collect();
        }

        self.status_mode = match cx.editor.mode() {
            zemacs_view::document::Mode::Normal => 0,
            zemacs_view::document::Mode::Select => 1,
            zemacs_view::document::Mode::Insert => 2,
        };

        let (view, doc) = current_ref!(cx.editor);
        self.view_top_line = doc.text().char_to_line(doc.view_offset(view.id).anchor);
        self.status_path = doc.display_name().to_string();
        self.current_doc_path = doc.path().map(|p| p.to_path_buf());
        // "Always select opened file": reveal the current buffer in the tree when
        // it changes (opt-in, so it doesn't disrupt manual tree browsing).
        if self.auto_reveal && self.current_doc_path != self.last_revealed {
            if let Some(p) = self.current_doc_path.clone() {
                self.project.reveal(&p);
            }
            self.last_revealed = self.current_doc_path.clone();
        }
        // Harpoon marks + indicator: where the current file sits in the project's marks.
        self.harpoon_rows = crate::harpoon::list();
        self.harpoon_total = self.harpoon_rows.len();
        self.harpoon_slot = self.current_doc_path.as_ref().and_then(|p| {
            let cp = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            self.harpoon_rows.iter().position(|m| *m == cp).map(|i| i + 1)
        });

        // status-bar snapshot (JetBrains bottom bar): Ln/Col, selection count, language, LSP, encoding
        let text = doc.text().slice(..);
        let sel = doc.selection(view.id);
        let cursor = sel.primary().cursor(text);
        self.cursor_char = cursor;
        let line = text.char_to_line(cursor);
        let col = cursor - text.line_to_char(line);
        self.status_lncol = (line + 1, col + 1);
        self.status_pct = if self.total_lines <= 1 {
            0
        } else {
            ((line * 100) / (self.total_lines - 1)).min(100) as u16
        };
        self.status_sel = sel.ranges().iter().map(|r| r.len()).sum();
        self.status_sel_lines = sel
            .ranges()
            .iter()
            .filter(|r| r.len() > 0)
            .map(|r| {
                let a = text.char_to_line(r.from());
                let b = text.char_to_line(r.to().saturating_sub(1).max(r.from()));
                b - a + 1
            })
            .sum();
        self.status_carets = sel.len();
        self.status_lang = doc.language_name().unwrap_or("plain text").to_string();
        self.status_lsp = doc.language_servers().next().is_some();
        self.status_encoding = doc.encoding().name().to_string();
        self.status_indent = match doc.indent_style {
            zemacs_core::indent::IndentStyle::Tabs => "Tab".to_string(),
            zemacs_core::indent::IndentStyle::Spaces(n) => format!("{n} sp"),
        };
        self.status_modified = doc.is_modified();
        // git branch — walk up from the file's dir to a .git, read HEAD (cheap; cached by dir)
        let dir = doc
            .path()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        if self.status_branch_dir.as_deref() != Some(dir.as_path()) {
            self.status_branch = git_branch(&dir).unwrap_or_default();
            self.status_branch_dir = Some(dir);
        }

        // VCS changes — only while the Git tab is open, throttled so big repos don't stall the frame
        if self.bottom_tab == BottomTab::Git {
            let stale = self.git_last.map_or(true, |t| t.elapsed().as_millis() > 800);
            if stale {
                if let Some(dir) = self.status_branch_dir.clone() {
                    self.git_changes = git_status(&dir);
                }
                self.git_last = Some(std::time::Instant::now());
            }
        }

        // Jumplist of the focused view. Rebuilt every refresh (not just while the tab is
        // open) so the JUMPS tab label can show an always-current count. `focus` is not
        // guaranteed to be a View (e.g. at startup, or while a non-editor pane is focused),
        // so resolve it fallibly and skip rather than panic.
        if let Some(focused_view) = cx.editor.tree.try_get(cx.editor.tree.focus) {
            self.jumplist_rows.clear();
            let focused_doc = focused_view.doc;
            for (view, focused) in cx.editor.tree.views() {
                if !focused {
                    continue;
                }
                for (doc_id, sel) in view.jumps.iter().rev() {
                    if let Some(doc) = cx.editor.documents.get(doc_id) {
                        let text = doc.text().slice(..);
                        let pos = sel.primary().cursor(text);
                        let line = text.char_to_line(pos) + 1;
                        let name = doc
                            .path()
                            .and_then(|p| p.file_name())
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "[scratch]".to_string());
                        let target = if *doc_id == focused_doc {
                            None
                        } else {
                            doc.path().map(|p| p.to_path_buf())
                        };
                        self.jumplist_rows.push((target, pos, format!("{name}:{line}")));
                    }
                }
            }
        }

        // Recently opened files. Loaded every refresh (the store is a small file and
        // rendering is event-driven, not a constant tick) so the RECENT tab label can
        // show an always-current count.
        self.recent_rows = crate::recent_files::load();
    }

    fn render_structure(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme) {
        use ratatui::style::Modifier as RMod;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, Borders, List, ListItem};
        let area = self.structure_rect;
        surface.clear_with(area, theme.get("ui.background"));

        let focused = self.focus == Focus::Structure;
        let title_style = crate::ui::rat::to_rat_style(if focused {
            theme.get("ui.text.focus")
        } else {
            theme.get("comment")
        });
        let chevron = if self.fold_structure { "▸" } else { "▾" };
        let title = if self.structure_searching || !self.structure_filter.is_empty() {
            format!(" {chevron} STRUCTURE  /{}▏", self.structure_filter)
        } else {
            format!(" {chevron} STRUCTURE ")
        };
        let count_style = crate::ui::rat::to_rat_style(theme.get("keyword")).add_modifier(RMod::BOLD);
        // The ratatui render blits an offscreen buffer, so empty rows would clobber our clear_with
        // back to a transparent bg — paint the whole block with the panel background to prevent that.
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(crate::ui::rat::to_rat_style(theme.get("ui.window")))
            .style(crate::ui::rat::to_rat_style(theme.get("ui.background")))
            .title(Span::styled(title, title_style))
            .title(Line::from(Span::styled(format!(" {} ", self.structure.len()), count_style)).right_aligned());

        if self.fold_structure {
            crate::ui::rat::render(block, area, surface);
            return;
        }
        if self.structure.is_empty() {
            crate::ui::rat::render(block, area, surface);
            surface.set_stringn(area.x + 1, area.y + 1, "(no symbols)", area.width as usize, theme.get("comment"));
            return;
        }

        let base = crate::ui::rat::to_rat_style(theme.get("ui.text"));
        let sel_style = crate::ui::rat::to_rat_style(theme.get("ui.selection")).add_modifier(RMod::BOLD);

        let items: Vec<ListItem> = self
            .structure
            .iter()
            .map(|o| {
                let (glyph, scope) = kind_glyph(&o.kind);
                let icon = crate::ui::rat::to_rat_style(theme.get(scope)).add_modifier(RMod::BOLD);
                // members (methods/fields/…) indent one level under their containers
                let indent = if is_member_kind(&o.kind) { "  " } else { "" };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {indent}{glyph} "), icon),
                    Span::styled(o.name.clone(), base),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(sel_style)
            .highlight_symbol("› ");

        self.structure_state.select(Some(self.structure_sel));
        crate::ui::rat::render_stateful(list, area, surface, &mut self.structure_state);

        // ratatui scrollbar on the right edge when the outline overflows
        let body_h = area.height.saturating_sub(1) as usize;
        if self.structure.len() > body_h && body_h > 0 {
            use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
            let track = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
            let mut sbs = ScrollbarState::new(self.structure.len())
                .position(self.structure_state.offset())
                .viewport_content_length(body_h);
            let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_symbol("▐")
                .thumb_style(crate::ui::rat::to_rat_style(theme.get("ui.selection")))
                .track_symbol(None);
            crate::ui::rat::render_stateful(sb, track, surface, &mut sbs);
        }
    }

    /// Top run/debug toolbar: ▶ Run · ◼ Stop · ⟳ Rerun · 🐞 Debug + the active run config.
    fn render_toolbar(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme) {
        let area = self.toolbar_rect;
        if area.height == 0 {
            return;
        }
        surface.clear_with(area, theme.get("ui.statusline"));
        self.toolbar_hits.clear();
        self.toolbar_y = area.y;
        self.breadcrumb_hit = (0, 0);

        // active run-config selector on the LEFT (click to open the manager)
        let cfg = crate::run_config::active()
            .map(|c| if c.name.is_empty() { c.command } else { c.name })
            .or_else(|| self.run.as_ref().map(|r| r.lock().unwrap().cmd.clone()))
            .unwrap_or_else(|| "Edit Configurations…".to_string());
        let label = format!(" ⚙ {cfg} ▾ ");
        let (lx, _) = surface.set_stringn(area.x, area.y, &label, area.width as usize, theme.get("function"));
        self.toolbar_hits.push((area.x, lx, ToolHit::Configs));

        // run/debug + settings/help buttons RIGHT-aligned
        let buttons: [(&str, _, ToolHit); 6] = [
            (" ▶ Run ", theme.get("diff.plus"), ToolHit::Run),
            (" ◼ Stop ", theme.get("error"), ToolHit::Stop),
            (" ⟳ Rerun ", theme.get("function"), ToolHit::Rerun),
            (" 🐞 Debug ", theme.get("keyword"), ToolHit::Debug),
            (" ⚙ Settings ", theme.get("comment"), ToolHit::Settings),
            (" ? Help ", theme.get("comment"), ToolHit::Help),
        ];
        let gap = 1u16;
        let total: u16 = buttons.iter().map(|(t, _, _)| disp_width(t)).sum::<u16>()
            + gap * (buttons.len() as u16 - 1);
        let buttons_start = area.x + area.width.saturating_sub(total + 1);

        // breadcrumb of the current file in the gap between the selector and buttons
        let bc_start = lx + 2;
        if buttons_start > bc_start + 4 && !self.status_path.is_empty() {
            let avail = (buttons_start - 1 - bc_start) as usize;
            let parts: Vec<&str> = self.status_path.split('/').filter(|s| !s.is_empty()).collect();
            if let Some((file, dirs)) = parts.split_last() {
                let glyph = crate::ui::icons::file_icon(file);
                let mut crumb = String::from(" ");
                crumb.push(glyph);
                crumb.push(' ');
                for d in dirs {
                    crumb.push_str(d);
                    crumb.push_str(" › ");
                }
                crumb.push_str(file);
                // append the innermost outline symbol containing the cursor
                if let Some(sym) = self
                    .structure
                    .iter()
                    .filter(|o| o.start <= self.cursor_char && self.cursor_char <= o.end)
                    .min_by_key(|o| o.end.saturating_sub(o.start))
                {
                    crumb.push_str(" › ");
                    crumb.push_str(&sym.name);
                }
                // left-truncate (keep the filename) when the path is too long
                let shown = if crumb.chars().count() > avail {
                    let tail: String = crumb
                        .chars()
                        .rev()
                        .take(avail.saturating_sub(1))
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    format!("…{tail}")
                } else {
                    crumb
                };
                let (end_x, _) = surface.set_stringn(bc_start, area.y, &shown, avail, theme.get("comment"));
                self.breadcrumb_hit = (bc_start, end_x);
            }
        }

        let mut x = buttons_start;
        for (text, style, hit) in buttons {
            let (nx, _) = surface.set_stringn(x, area.y, text, area.width as usize, style);
            self.toolbar_hits.push((x, nx, hit));
            x = nx + gap;
        }
    }

    /// Bottom tool window: a `Problems | Run` tab header (with run controls) + the active body.
    fn render_bottom(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme) {
        let area = self.problems_rect;
        surface.clear_with(area, theme.get("ui.background"));
        self.bottom_hits.clear();
        self.bottom_header_y = area.y;

        let focused = self.focus == Focus::Problems;
        let on = if focused { theme.get("ui.text.focus") } else { theme.get("ui.text") };
        let off = theme.get("comment");

        // fold chevron
        let chev = if self.fold_problems { "▸ " } else { "▾ " };
        surface.set_stringn(area.x, area.y, chev, 2, off);
        let mut x = area.x + 2;

        // Problems tab with a severity breakdown (E/W/I counts, colour-coded)
        let (mut errs, mut warns, mut infos) = (0usize, 0usize, 0usize);
        for p in &self.problems {
            match p.sev {
                Severity::Error => errs += 1,
                Severity::Warning => warns += 1,
                _ => infos += 1,
            }
        }
        let plabel_style = if self.bottom_tab == BottomTab::Problems { on } else { off };
        let x0 = x;
        let (mut nx, _) = surface.set_stringn(x, area.y, " PROBLEMS ", area.width as usize, plabel_style);
        if self.problems.is_empty() {
            let (e, _) = surface.set_stringn(nx, area.y, "✓ ", area.width as usize, theme.get("diff.plus"));
            nx = e;
        } else {
            for (count, glyph, scope) in [
                (errs, "⊘", "error"),
                (warns, "⚠", "warning"),
                (infos, "ℹ", "info"),
            ] {
                if count > 0 {
                    let s = format!("{glyph}{count} ");
                    let (e, _) = surface.set_stringn(nx, area.y, &s, area.width as usize, theme.get(scope));
                    nx = e;
                }
            }
        }
        self.bottom_hits.push((x0, nx, BottomHit::TabProblems));
        x = nx + 1;

        // Run tab
        let rlabel = " RUN ";
        let rw = rlabel.chars().count() as u16;
        surface.set_stringn(x, area.y, rlabel, area.width as usize, if self.bottom_tab == BottomTab::Run { on } else { off });
        self.bottom_hits.push((x, x + rw, BottomHit::TabRun));
        x += rw + 1;

        // Git / VCS changes tab, with a colour-coded staged/modified/untracked breakdown
        let (mut added, mut modified, mut deleted, mut untracked) = (0usize, 0, 0, 0);
        for (code, _, _) in &self.git_changes {
            match code.trim() {
                "??" => untracked += 1,
                c if c.starts_with('A') => added += 1,
                c if c.contains('D') => deleted += 1,
                _ => modified += 1,
            }
        }
        let glabel_style = if self.bottom_tab == BottomTab::Git { on } else { off };
        let gx0 = x;
        let (mut gx, _) = surface.set_stringn(x, area.y, " GIT ", area.width as usize, glabel_style);
        if self.git_changes.is_empty() {
            let (e, _) = surface.set_stringn(gx, area.y, "✓ ", area.width as usize, theme.get("diff.plus"));
            gx = e;
        } else {
            for (count, sign, scope) in [
                (added, "+", "diff.plus"),
                (modified, "~", "diff.delta"),
                (deleted, "-", "diff.minus"),
                (untracked, "?", "comment"),
            ] {
                if count > 0 {
                    let s = format!("{sign}{count} ");
                    let (e, _) = surface.set_stringn(gx, area.y, &s, area.width as usize, theme.get(scope));
                    gx = e;
                }
            }
        }
        self.bottom_hits.push((gx0, gx, BottomHit::TabGit));
        x = gx + 1;

        // Registers tab (LOTR)
        let glabel = format!(" REGISTERS {} ", self.registers.len());
        let gw = glabel.chars().count() as u16;
        surface.set_stringn(x, area.y, &glabel, area.width as usize, if self.bottom_tab == BottomTab::Registers { on } else { off });
        self.bottom_hits.push((x, x + gw, BottomHit::TabRegisters));
        x += gw + 1;

        // Todo tab
        let tlabel = format!(" TODO {} ", self.todos.len());
        let tw = tlabel.chars().count() as u16;
        surface.set_stringn(x, area.y, &tlabel, area.width as usize, if self.bottom_tab == BottomTab::Todo { on } else { off });
        self.bottom_hits.push((x, x + tw, BottomHit::TabTodo));
        x += tw + 1;

        // Marks tab
        let mlabel = format!(" MARKS {} ", self.marks_list.len());
        let mw = mlabel.chars().count() as u16;
        surface.set_stringn(x, area.y, &mlabel, area.width as usize, if self.bottom_tab == BottomTab::Marks { on } else { off });
        self.bottom_hits.push((x, x + mw, BottomHit::TabMarks));
        x += mw + 1;

        // Jumplist tab
        let jlabel = format!(" JUMPS {} ", self.jumplist_rows.len());
        let jw = jlabel.chars().count() as u16;
        surface.set_stringn(x, area.y, &jlabel, area.width as usize, if self.bottom_tab == BottomTab::Jumplist { on } else { off });
        self.bottom_hits.push((x, x + jw, BottomHit::TabJumplist));
        x += jw + 1;

        // Recent files tab
        let nlabel = format!(" RECENT {} ", self.recent_rows.len());
        let nw = nlabel.chars().count() as u16;
        surface.set_stringn(x, area.y, &nlabel, area.width as usize, if self.bottom_tab == BottomTab::Recent { on } else { off });
        self.bottom_hits.push((x, x + nw, BottomHit::TabRecent));
        x += nw + 1;

        // Harpoon marks tab
        let hlabel = format!(" ⚓ {} ", self.harpoon_rows.len());
        let hw = hlabel.chars().count() as u16 + 1; // anchor glyph is double-width
        surface.set_stringn(x, area.y, &hlabel, area.width as usize, if self.bottom_tab == BottomTab::Harpoon { on } else { off });
        self.bottom_hits.push((x, x + hw, BottomHit::TabHarpoon));
        x += hw + 2;

        // run controls + status
        let run_info = self.run.as_ref().map(|r| {
            let s = r.lock().unwrap();
            (s.running, s.exit_code)
        });
        if let Some((running, code)) = run_info {
            surface.set_stringn(x, area.y, "⟳", 1, theme.get("function"));
            self.bottom_hits.push((x, x + 1, BottomHit::Rerun));
            x += 2;
            surface.set_stringn(x, area.y, "◼", 1, theme.get("error"));
            self.bottom_hits.push((x, x + 1, BottomHit::Stop));
            x += 2;
            surface.set_stringn(x, area.y, "⌦", 1, off);
            self.bottom_hits.push((x, x + 1, BottomHit::Clear));
            x += 2;
            let status = if running {
                "running…".to_string()
            } else {
                format!("exit {}", code.unwrap_or(-1))
            };
            let avail = area.width.saturating_sub(x.saturating_sub(area.x)) as usize;
            surface.set_stringn(x + 1, area.y, &status, avail, off);
        }

        if self.fold_problems {
            return;
        }
        let body = body_rect(area);
        match self.bottom_tab {
            BottomTab::Problems => self.render_problems_body(surface, theme, body),
            BottomTab::Run => self.render_run_body(surface, theme, body),
            BottomTab::Git => self.render_git_body(surface, theme, body),
            BottomTab::Registers => self.render_registers_body(surface, theme, body),
            BottomTab::Todo => self.render_todo_body(surface, theme, body),
            BottomTab::Marks => self.render_marks_body(surface, theme, body),
            BottomTab::Jumplist => self.render_jumplist_body(surface, theme, body),
            BottomTab::Recent => self.render_recent_body(surface, theme, body),
            BottomTab::Harpoon => self.render_harpoon_body(surface, theme, body),
        }
    }

    fn render_jumplist_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.jumplist_rows.is_empty() {
            surface.set_stringn(body.x, body.y, "  no jumps", body.width as usize, theme.get("comment"));
            return;
        }
        let mark = theme.get("function");
        let base = theme.get("ui.text");
        let focused = self.focus == Focus::Problems && self.bottom_tab == BottomTab::Jumplist;
        for (i, (_, _, label)) in self.jumplist_rows.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
            if focused && i == self.aux_sel {
                surface.set_style(Rect::new(body.x, y, body.width, 1), theme.get("ui.selection"));
            }
            surface.set_stringn(body.x, y, " ↪", body.width as usize, mark);
            surface.set_stringn(body.x + 3, y, label, body.width.saturating_sub(3) as usize, base);
        }
    }

    fn render_recent_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.recent_rows.is_empty() {
            surface.set_stringn(body.x, body.y, "  no recent files", body.width as usize, theme.get("comment"));
            return;
        }
        let base = theme.get("ui.text");
        let dim = theme.get("comment");
        let focused = self.focus == Focus::Problems && self.bottom_tab == BottomTab::Recent;
        for (i, path) in self.recent_rows.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
            if focused && i == self.aux_sel {
                surface.set_style(Rect::new(body.x, y, body.width, 1), theme.get("ui.selection"));
            }
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let glyph = crate::ui::icons::file_icon(&name);
            let label = format!(" {glyph} {name}");
            let (nx, _) = surface.set_stringn(body.x, y, &label, body.width as usize, base);
            // trailing dimmed parent directory
            if let Some(parent) = path.parent().map(|p| p.to_string_lossy().into_owned()) {
                let rem = body.width.saturating_sub(nx - body.x) as usize;
                surface.set_stringn(nx + 1, y, &format!("· {parent}"), rem, dim);
            }
        }
    }

    fn render_harpoon_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.harpoon_rows.is_empty() {
            surface.set_stringn(body.x, body.y, "  no marks — pin with SPC H a", body.width as usize, theme.get("comment"));
            return;
        }
        let base = theme.get("ui.text");
        let slot_style = theme.get("keyword");
        let focused = self.focus == Focus::Problems && self.bottom_tab == BottomTab::Harpoon;
        for (i, path) in self.harpoon_rows.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
            if focused && i == self.aux_sel {
                surface.set_style(Rect::new(body.x, y, body.width, 1), theme.get("ui.selection"));
            }
            // slot number (1-based) then the file name
            surface.set_stringn(body.x + 1, y, &format!("{}", i + 1), 2, slot_style);
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let glyph = crate::ui::icons::file_icon(&name);
            surface.set_stringn(body.x + 3, y, &format!("{glyph} {name}"), body.width.saturating_sub(3) as usize, base);
        }
    }

    fn render_todo_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.todos.is_empty() {
            surface.set_stringn(body.x, body.y, "  no TODOs", body.width as usize, theme.get("comment"));
            return;
        }
        let mark = theme.get("warning");
        let base = theme.get("ui.text");
        let focused = self.focus == Focus::Problems && self.bottom_tab == BottomTab::Todo;
        for (i, (_, text)) in self.todos.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
            if focused && i == self.aux_sel {
                surface.set_style(Rect::new(body.x, y, body.width, 1), theme.get("ui.selection"));
            }
            surface.set_stringn(body.x, y, " •", body.width as usize, mark);
            surface.set_stringn(body.x + 3, y, text, body.width.saturating_sub(3) as usize, base);
        }
    }

    fn render_registers_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.registers.is_empty() {
            surface.set_stringn(body.x, body.y, "  no registers", body.width as usize, theme.get("comment"));
            return;
        }
        let name_style = theme.get("keyword");
        let base = theme.get("ui.text");
        let focused = self.focus == Focus::Problems && self.bottom_tab == BottomTab::Registers;
        for (i, (ch, content)) in self.registers.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
            if focused && i == self.reg_sel {
                surface.set_style(Rect::new(body.x, y, body.width, 1), theme.get("ui.selection"));
            }
            let label = format!(" \"{ch}  ");
            let (nx, _) = surface.set_stringn(body.x, y, &label, body.width as usize, name_style);
            surface.set_stringn(nx, y, content, body.width.saturating_sub(nx - body.x) as usize, base);
        }
    }

    fn render_problems_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        use ratatui::layout::Constraint;
        use ratatui::style::Modifier as RMod;
        use ratatui::widgets::{Block, Cell, Row, Table};
        if body.height == 0 {
            return;
        }
        if self.problems.is_empty() {
            surface.set_stringn(body.x, body.y, "  no problems", body.width as usize, theme.get("comment"));
            return;
        }
        let base = crate::ui::rat::to_rat_style(theme.get("ui.text"));
        let dim = crate::ui::rat::to_rat_style(theme.get("comment"));
        let sel = crate::ui::rat::to_rat_style(theme.get("ui.selection")).add_modifier(RMod::BOLD);
        let rows: Vec<Row> = self
            .problems
            .iter()
            .map(|p| {
                let (glyph, st) = sev_mark(p.sev, theme);
                Row::new(vec![
                    Cell::from(glyph).style(crate::ui::rat::to_rat_style(st)),
                    Cell::from(format!("{}", p.line + 1)).style(dim),
                    Cell::from(p.msg.replace('\n', " ")).style(base),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [Constraint::Length(1), Constraint::Length(6), Constraint::Min(8)],
        )
        .column_spacing(1)
        .block(Block::default().style(crate::ui::rat::to_rat_style(theme.get("ui.background"))))
        .row_highlight_style(sel)
        .highlight_symbol("› ");
        self.problems_state.select(Some(self.problems_sel));
        crate::ui::rat::render_stateful(table, body, surface, &mut self.problems_state);

        let body_h = body.height as usize;
        if self.problems.len() > body_h && body_h > 0 {
            use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
            let mut sbs = ScrollbarState::new(self.problems.len())
                .position(self.problems_state.offset())
                .viewport_content_length(body_h);
            let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_symbol("▐")
                .thumb_style(crate::ui::rat::to_rat_style(theme.get("ui.selection")))
                .track_symbol(None);
            crate::ui::rat::render_stateful(sb, body, surface, &mut sbs);
        }
    }

    fn render_git_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.git_changes.is_empty() {
            let msg = if self.status_branch.is_empty() {
                "  not a git repository"
            } else {
                "  working tree clean ✓"
            };
            surface.set_stringn(body.x, body.y, msg, body.width as usize, theme.get("comment"));
            return;
        }
        if self.git_sel >= self.git_changes.len() {
            self.git_sel = self.git_changes.len().saturating_sub(1);
        }
        let base = theme.get("ui.text");
        let sel_bg = theme.get("ui.selection");
        let focused = self.focus == Focus::Problems && self.bottom_tab == BottomTab::Git;
        for (i, (code, disp, _)) in self.git_changes.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
            // highlight the keyboard-selected row when the panel holds focus
            if focused && i == self.git_sel {
                surface.set_style(Rect::new(body.x, y, body.width, 1), sel_bg);
            }
            // colour by status: added=green, modified=yellow, deleted=red, untracked=dim
            let st = match code.trim() {
                "A" | "AM" => theme.get("diff.plus"),
                "D" => theme.get("diff.minus"),
                "??" => theme.get("comment"),
                _ => theme.get("diff.delta"),
            };
            surface.set_stringn(body.x + 1, y, &code.replace(' ', "·"), 3, st);
            let rest: String = disp.chars().skip(2).collect();
            surface.set_stringn(body.x + 4, y, &rest, body.width.saturating_sub(4) as usize, base);
        }
    }

    fn render_marks_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.marks_list.is_empty() {
            surface.set_stringn(body.x, body.y, "  no marks — set with m{a-z}", body.width as usize, theme.get("comment"));
            return;
        }
        let base = theme.get("ui.text");
        let accent = theme.get("keyword");
        let focused = self.focus == Focus::Problems && self.bottom_tab == BottomTab::Marks;
        for (i, (_, disp)) in self.marks_list.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
            if focused && i == self.aux_sel {
                surface.set_style(Rect::new(body.x, y, body.width, 1), theme.get("ui.selection"));
            }
            // mark sigil ('x) gets the accent colour, the rest is plain
            let head: String = disp.chars().take(2).collect();
            surface.set_stringn(body.x + 1, y, &head, 2, accent);
            let rest: String = disp.chars().skip(2).collect();
            surface.set_stringn(body.x + 3, y, &rest, body.width.saturating_sub(3) as usize, base);
        }
    }

    fn render_run_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let Some(run) = self.run.clone() else {
            surface.set_stringn(body.x, body.y, "  no run — :run [cmd]", body.width as usize, theme.get("comment"));
            return;
        };
        let s = run.lock().unwrap();
        let height = body.height as usize;
        let w = body.width.max(1) as usize;
        if height == 0 {
            return;
        }
        let base = theme.get("ui.text");

        // Soft-wrap: every output line occupies ceil(width/w) visual rows (≥1).
        let line_vis = |line: &str| -> usize {
            let dw = disp_width(line) as usize;
            if dw == 0 { 1 } else { dw.div_ceil(w) }
        };
        let total_vis: usize = s.lines.iter().map(|l| line_vis(l)).sum::<usize>().max(1);
        self.run_total_vis = total_vis;
        let max_top = total_vis.saturating_sub(height);
        // tail-follow unless the user scrolled up
        let top = if s.follow { max_top } else { s.scroll.min(max_top) };

        // (re)build the visible-row → source-line map for click-to-jump
        self.run_row_src = vec![usize::MAX; height];
        let mut vis = 0usize;
        'lines: for (li, line) in s.lines.iter().enumerate() {
            for chunk in wrap_chunks(line, w) {
                if vis >= top + height {
                    break 'lines;
                }
                if vis >= top {
                    let sr = vis - top;
                    surface.set_stringn(body.x, body.y + sr as u16, chunk, w, base);
                    self.run_row_src[sr] = li;
                }
                vis += 1;
            }
            // empty line still consumes one visual row
            if line.is_empty() {
                vis += 1;
            }
        }

        // scrollbar thumb on the right edge when content overflows
        if total_vis > height && body.width > 1 {
            let track_x = body.x + body.width - 1;
            let thumb_h = (height * height / total_vis).max(1);
            let thumb_y = if max_top == 0 { 0 } else { top * (height - thumb_h) / max_top };
            let bar = theme.get("ui.selection");
            for k in 0..thumb_h {
                surface.set_stringn(track_x, body.y + (thumb_y + k) as u16, "▐", 1, bar);
            }
        }
    }

    /// Right-pane minimap: a braille-rendered "tiny text" overview (each cell = a 2×4 dot grid, so
    /// 2 source columns × 4 source lines per cell — the VSCode/Sublime code-shape look), with a
    /// viewport box and diagnostic ticks (click to scrub).
    fn render_stripe(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme) {
        // braille dot bit per (subcol, subrow) within a cell (U+2800 base).
        const DOT: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
        let area = self.stripe_rect;
        let w = area.width as usize;
        let h = area.height as usize;
        if w == 0 || h == 0 {
            return;
        }
        surface.clear_with(area, theme.get("ui.background"));
        let total = self.total_lines.max(1);
        let slots = h * 4; // vertical sub-rows across the whole pane
        let last = area.y + area.height.saturating_sub(1);

        // source line -> vertical sub-row slot (1:1 when the file fits the slots, else downscale)
        let slot_of = |line: usize| -> usize {
            if total <= slots {
                line
            } else {
                line * slots / total
            }
        };

        // viewport box (cell rows), drawn first so the dots sit on top
        let vp = theme.get("ui.selection");
        let mut yy = (slot_of(self.view_top_line) / 4) as u16;
        let yend = ((slot_of(self.view_top_line + self.view_lines) / 4) as u16).min(h as u16 - 1);
        while yy <= yend {
            surface.set_style(Rect::new(area.x, area.y + yy, area.width, 1), vp);
            yy += 1;
        }

        // braille density overview
        let dim = theme.get("comment");
        let mut buf = [0u8; 4];
        for cy in 0..h {
            for cx in 0..w {
                let mut bits: u8 = 0;
                for r in 0..4 {
                    let slot = cy * 4 + r;
                    let srcline = if total <= slots { slot } else { slot * total / slots };
                    let Some(dots) = self.minimap_dots.get(srcline) else { continue };
                    for c in 0..2 {
                        if dots.get(cx * 2 + c).copied().unwrap_or(false) {
                            bits |= DOT[c][r];
                        }
                    }
                }
                if bits != 0 {
                    let ch = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
                    surface.set_string(area.x + cx as u16, area.y + cy as u16, ch.encode_utf8(&mut buf), dim);
                }
            }
        }

        // diagnostic ticks on the right edge
        let tick_x = area.x + area.width.saturating_sub(1);
        for p in &self.problems {
            let ty = (area.y + (slot_of(p.line) / 4) as u16).min(last);
            let (_, style) = sev_mark(p.sev, theme);
            surface.set_string(tick_x, ty, "▌", style);
        }

        // git change overview on the left edge: a coloured bar per changed-line range
        for &(start, end, kind) in &self.git_hunks {
            let style = match kind {
                0 => theme.get("diff.plus"),
                2 => theme.get("diff.minus"),
                _ => theme.get("diff.delta"),
            };
            let r0 = (slot_of(start as usize) / 4) as u16;
            let r1 = if end > start {
                ((slot_of((end as usize).saturating_sub(1)) / 4) as u16).max(r0)
            } else {
                r0
            };
            let mut ry = r0;
            while ry <= r1 && area.y + ry <= last {
                surface.set_string(area.x, area.y + ry, "▏", style);
                ry += 1;
            }
        }
    }
}

/// The area below a drawer's 1-row header.
fn body_rect(area: Rect) -> Rect {
    Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1))
}

/// Draw a drawer header with a fold chevron (▾ open / ▸ folded).
fn draw_header(surface: &mut Surface, area: Rect, title: &str, folded: bool, focused: bool, theme: &zemacs_view::Theme) {
    let style = if focused {
        theme.get("ui.text.focus")
    } else {
        theme.get("comment")
    };
    let chevron = if folded { "▸" } else { "▾" };
    let text = format!(" {chevron} {title}");
    surface.set_stringn(area.x, area.y, &text, area.width as usize, style);
}

fn in_rect(r: &Rect, col: u16, row: u16) -> bool {
    r.width > 0 && col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

fn step(sel: &mut usize, len: usize, down: bool) {
    if down {
        if *sel + 1 < len {
            *sel += 1;
        }
    } else {
        *sel = sel.saturating_sub(1);
    }
}

/// Outline kind → (icon glyph, theme scope for its colour). JetBrains-style coloured symbol icons.
fn kind_glyph(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "function" | "method" => ("ƒ", "function"),
        "constructor" => ("ƒ", "constructor"),
        "class" | "struct" => ("◇", "type"),
        "interface" | "trait" => ("◈", "type.builtin"),
        "enum" => ("▤", "type.enum"),
        "module" | "namespace" => ("▣", "keyword"),
        "constant" => ("π", "constant"),
        "variable" | "field" | "property" | "member" => ("•", "variable"),
        "macro" => ("#", "function.macro"),
        _ => ("›", "comment"),
    }
}

fn is_member_kind(kind: &str) -> bool {
    matches!(
        kind,
        "method" | "field" | "property" | "constant" | "variable" | "member" | "constructor"
    )
}

fn sev_mark(sev: Severity, theme: &zemacs_view::Theme) -> (&'static str, zemacs_view::graphics::Style) {
    match sev {
        Severity::Error => ("E", theme.get("error")),
        Severity::Warning => ("W", theme.get("warning")),
        Severity::Info => ("I", theme.get("info")),
        Severity::Hint => ("H", theme.get("hint")),
    }
}

/// Build via `Selection::single` so callers can apply a goto in one place.
pub fn goto_selection(from: usize, to: usize) -> Selection {
    Selection::single(from, to)
}

// ---- right-click file-tree context menu (CRUD) ----

#[derive(Clone, Copy, PartialEq)]
enum FileActionKind {
    NewFile,
    NewFolder,
    Rename,
    Delete,
    CopyPath,
}

pub struct ContextAction {
    label: &'static str,
    kind: FileActionKind,
}

impl crate::ui::menu::Item for ContextAction {
    type Data = ();
    fn format(&self, _: &()) -> crate::ui::menu::Row<'_> {
        crate::ui::menu::Row::new(vec![crate::ui::menu::Cell::from(self.label)])
    }
}

/// Rebuild the file tree on the main thread (from a background callback context).
fn refresh_tree_async() {
    crate::job::dispatch_blocking(|_editor, compositor| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.refresh_file_tree();
        }
    });
}

/// Build the right-click CRUD context menu for a file-tree entry at (row, col).
pub fn file_context_menu(
    path: PathBuf,
    is_dir: bool,
    row: u16,
    col: u16,
) -> crate::ui::popup::Popup<crate::ui::menu::Menu<ContextAction>> {
    use crate::ui::menu::Menu;
    use crate::ui::PromptEvent;

    let mut items = Vec::new();
    if is_dir {
        items.push(ContextAction { label: "New File", kind: FileActionKind::NewFile });
        items.push(ContextAction { label: "New Folder", kind: FileActionKind::NewFolder });
    }
    items.push(ContextAction { label: "Rename", kind: FileActionKind::Rename });
    items.push(ContextAction { label: "Delete", kind: FileActionKind::Delete });
    items.push(ContextAction { label: "Copy Path", kind: FileActionKind::CopyPath });

    let menu = Menu::new(items, (), move |editor, item, event| {
        if !matches!(event, PromptEvent::Validate) {
            return;
        }
        let Some(item) = item else { return };
        let path = path.clone();
        match item.kind {
            FileActionKind::CopyPath => {
                let s = path.to_string_lossy().to_string();
                let _ = editor.registers.push('"', s.clone());
                editor.set_status(format!("yanked path: {s}"));
            }
            FileActionKind::Delete => {
                let res = if is_dir {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                match res {
                    Ok(()) => editor.set_status(format!("deleted {}", path.display())),
                    Err(e) => editor.set_error(format!("delete failed: {e}")),
                }
                refresh_tree_async();
            }
            kind => {
                // New File / New Folder / Rename need a name prompt, which requires
                // compositor access — hop onto the main loop to push it.
                crate::job::dispatch_blocking(move |_editor, compositor| {
                    compositor.push(Box::new(name_prompt(kind, path.clone(), is_dir)));
                });
            }
        }
    });

    crate::ui::popup::Popup::new("file-context-menu", menu)
        .position(Some(zemacs_core::Position::new(row as usize, col as usize)))
        .auto_close(true)
}

/// Prompt for a name, then create/rename the target and refresh the tree.
fn name_prompt(kind: FileActionKind, target: PathBuf, _is_dir: bool) -> crate::ui::Prompt {
    use crate::ui::PromptEvent;
    let label: std::borrow::Cow<'static, str> = match kind {
        FileActionKind::NewFile => "New file: ".into(),
        FileActionKind::NewFolder => "New folder: ".into(),
        FileActionKind::Rename => "Rename to: ".into(),
        _ => "".into(),
    };
    crate::ui::Prompt::new(
        label,
        None,
        |_editor, _input| Vec::new(),
        move |cx, input, event| {
            if !matches!(event, PromptEvent::Validate) || input.trim().is_empty() {
                return;
            }
            let input = input.trim();
            let result = match kind {
                FileActionKind::NewFile => {
                    let p = target.join(input);
                    std::fs::File::create(&p).map(|_| p)
                }
                FileActionKind::NewFolder => {
                    let p = target.join(input);
                    std::fs::create_dir_all(&p).map(|_| p)
                }
                FileActionKind::Rename => {
                    let parent = target
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let np = parent.join(input);
                    std::fs::rename(&target, &np).map(|_| np)
                }
                _ => return,
            };
            match result {
                Ok(p) => cx.editor.set_status(format!("created {}", p.display())),
                Err(e) => cx.editor.set_error(format!("failed: {e}")),
            }
            refresh_tree_async();
        },
    )
}

/// Current git branch for `start`: walk up to a `.git`, read `HEAD`. Returns the short branch name
/// (or a 7-char hash for a detached HEAD). Cheap enough to call when the active directory changes.
fn git_branch(start: &std::path::Path) -> Option<String> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        let head = dir.join(".git").join("HEAD");
        if let Ok(content) = std::fs::read_to_string(&head) {
            let t = content.trim();
            return Some(match t.strip_prefix("ref: refs/heads/") {
                Some(branch) => branch.to_string(),
                None => t.chars().take(7).collect(),
            });
        }
        cur = dir.parent();
    }
    None
}

/// `git status --porcelain` for the repo containing `dir`, as (XY code, "XY  path", abs path) rows.
/// Parse a `path:line[:col]` reference out of a build/compiler output line
/// (e.g. `src/main.rs:42:10: error: …`). Returns the path, 1-based line, col.
fn parse_file_line(line: &str) -> Option<(String, usize, usize)> {
    for raw in line.split(char::is_whitespace) {
        let tok = raw.trim_matches(|c| matches!(c, ':' | ',' | '(' | ')' | '[' | ']' | '"' | '\''));
        let parts: Vec<&str> = tok.split(':').collect();
        if parts.len() < 2 {
            continue;
        }
        let path = parts[0];
        // must look like a path (avoids matching timestamps like 12:34)
        if path.is_empty() || !(path.contains('/') || path.contains('.')) {
            continue;
        }
        let Ok(lineno) = parts[1].parse::<usize>() else {
            continue;
        };
        if lineno == 0 {
            continue;
        }
        let col = parts.get(2).and_then(|c| c.parse::<usize>().ok()).unwrap_or(1);
        return Some((path.to_string(), lineno, col.max(1)));
    }
    None
}

/// Stage everything (`git add -A`) or unstage everything (`git reset`). Best-effort.
fn git_stage_all(dir: &std::path::Path, stage: bool) {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(dir);
    if stage {
        cmd.args(["add", "-A"]);
    } else {
        cmd.args(["reset", "-q"]);
    }
    let _ = cmd.output();
}

/// Stage (`git add`) or unstage (`git reset HEAD`) a single path. Best-effort.
fn git_stage(path: &std::path::Path, stage: bool) {
    let dir = path.parent().unwrap_or(path);
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(dir);
    if stage {
        cmd.args(["add", "--"]);
    } else {
        cmd.args(["reset", "-q", "HEAD", "--"]);
    }
    let _ = cmd.arg(path).output();
}

fn git_status(dir: &std::path::Path) -> Vec<(String, String, std::path::PathBuf)> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain", "--no-renames"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    // resolve paths against the repo root, not `dir`
    let root = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| std::path::PathBuf::from(s.trim().to_string()))
        .unwrap_or_else(|| dir.to_path_buf());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| {
            let code = l[..2].to_string();
            let path = l[3..].trim();
            let disp = format!("{}  {}", code, path);
            (code, disp, root.join(path))
        })
        .collect()
}

#[cfg(test)]
mod parse_tests {
    use super::parse_file_line;

    #[test]
    fn parses_file_line_col() {
        assert_eq!(
            parse_file_line("  src/main.rs:42:10: error: boom"),
            Some(("src/main.rs".into(), 42, 10))
        );
        assert_eq!(
            parse_file_line("error[E0382]: at ./lib/foo.rs:7"),
            Some(("./lib/foo.rs".into(), 7, 1))
        );
        // a bare timestamp must NOT match (no path-like token)
        assert_eq!(parse_file_line("12:34:56 building"), None);
        assert_eq!(parse_file_line("no location here"), None);
    }
}
