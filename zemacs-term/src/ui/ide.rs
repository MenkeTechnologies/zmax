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
    input::KeyEvent,
    input::{MouseButton, MouseEvent, MouseEventKind},
    keyboard::{KeyCode, KeyModifiers},
    DocumentId,
};

use super::file_tree::{FileTree, TreeAction};

const LEFT_W: u16 = 32;
const BOTTOM_H: u16 = 8;
const STRIPE_W: u16 = 14; // right-pane minimap width

/// Terminal display width of a string — the SAME model `Surface::set_stringn`
/// advances by (unicode-width per grapheme), so toolbar layout, drawing and
/// click hit-regions stay in lockstep. (Zero-width variation selectors like
/// U+FE0E count as 0, so forcing text presentation on the toolbar glyphs doesn't
/// skew the layout.)
fn disp_width(s: &str) -> u16 {
    use zemacs_core::unicode::width::UnicodeWidthStr;
    s.width() as u16
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

/// Find a `NN%` token in `line` (0..=100), e.g. cargo/test/webpack progress.
/// Returns the first plausible percentage so a build LineGauge can reflect it.
fn parse_percent(line: &str) -> Option<u32> {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'%' {
            // walk back over the digits immediately preceding the '%'
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                if let Ok(n) = line[j..i].parse::<u32>() {
                    if n <= 100 {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

/// Count cargo-style test results streaming in the Run console: `(passed,
/// total)` from `test <name> ... ok` / `... FAILED` lines. `None` when no test
/// lines are present yet. Drives the run-console test gauge.
fn parse_test_progress(lines: &[String]) -> Option<(u32, u32)> {
    let mut passed = 0u32;
    let mut failed = 0u32;
    for l in lines {
        let t = l.trim();
        if t.starts_with("test ") && !t.starts_with("test result:") && t.contains(" ... ") {
            if t.ends_with(" ok") {
                passed += 1;
            } else if t.contains("FAILED") {
                failed += 1;
            }
            // "... ignored" lines are excluded from the pass ratio
        }
    }
    let total = passed + failed;
    (total > 0).then_some((passed, total))
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
    Debug,
    Registers,
    Todo,
    Marks,
    Jumplist,
    Recent,
    Harpoon,
    Ci,
}

impl BottomTab {
    /// Stable name for session persistence.
    fn persist_name(self) -> &'static str {
        match self {
            BottomTab::Problems => "problems",
            BottomTab::Run => "run",
            BottomTab::Git => "git",
            BottomTab::Debug => "debug",
            BottomTab::Registers => "registers",
            BottomTab::Todo => "todo",
            BottomTab::Marks => "marks",
            BottomTab::Jumplist => "jumplist",
            BottomTab::Recent => "recent",
            BottomTab::Harpoon => "harpoon",
            BottomTab::Ci => "ci",
        }
    }
    fn from_persist_name(s: &str) -> Option<Self> {
        Some(match s {
            "problems" => BottomTab::Problems,
            "run" => BottomTab::Run,
            "git" => BottomTab::Git,
            "debug" => BottomTab::Debug,
            "registers" => BottomTab::Registers,
            "todo" => BottomTab::Todo,
            "marks" => BottomTab::Marks,
            "jumplist" => BottomTab::Jumplist,
            "recent" => BottomTab::Recent,
            "harpoon" => BottomTab::Harpoon,
            "ci" => BottomTab::Ci,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy)]
enum BottomHit {
    TabProblems,
    TabRun,
    TabGit,
    TabDebug,
    TabRegisters,
    TabTodo,
    TabMarks,
    TabJumplist,
    TabRecent,
    TabHarpoon,
    TabCi,
    Rerun,
    Stop,
    Clear,
}

pub enum IdeAction {
    None,
    OpenFile(PathBuf),
    /// Open a URL in the system browser (CI run links).
    OpenUrl(String),
    /// Open a file and place the cursor on a 1-based line (run-output jump).
    OpenFileAt {
        path: PathBuf,
        line: usize,
    },
    Goto {
        from: usize,
        to: usize,
    },
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
    /// Open a conflicted file and launch the 3-pane merge resolver (the `:merge`
    /// flow), from the Git tab's "Conflicts" section.
    ResolveConflict(PathBuf),
    /// Right-click on a file-tree entry: show a CRUD context menu at (row, col).
    ShowContextMenu {
        path: PathBuf,
        is_dir: bool,
        row: u16,
        col: u16,
    },
    /// Show a pre-built context menu (right-click on a non-tree IDE surface:
    /// the Run console, Structure outline, Problems list, …).
    ShowMenu(crate::ui::context_menu::ContextMenu),
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
    Locate,
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

/// Flattened Debug-tab row: `(kind, text, jump target)` where kind is
/// 0=section header, 1=stack frame, 2=variable, 3=breakpoint.
type DapLine = (u8, String, Option<(PathBuf, usize)>);

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
    ci_state: ratatui::widgets::TableState,
    run: Option<crate::ui::run::Run>,
    registers: Vec<(char, String)>,
    todos: Vec<(usize, String, &'static str)>,
    marks_list: Vec<(usize, String)>,
    /// Jumplist entries: (path if in another doc else None, char pos, label).
    jumplist_rows: Vec<(Option<PathBuf>, usize, String)>,
    /// Recently opened files (newest first).
    recent_rows: Vec<PathBuf>,
    recent_times: Vec<u64>,
    /// Harpoon marks for the current project (pin order).
    harpoon_rows: Vec<PathBuf>,
    bottom_tab: BottomTab, // mirror of the focused column's active tab (keeps existing key/mouse logic working)
    bottom_tabs: [BottomTab; 3], // active tab in each of the three columns
    bottom_focus_col: usize, // which column has keyboard focus (0 | 1 | 2)
    bottom_splits: [u16; 2], // the two divider positions as % of drawer width
    bottom_div_x: [u16; 2], // screen columns of the two dividers (0 = not laid out)
    aux_sels: [usize; 3],  // per-column list selection (mirrored to aux_sel for the focused col)
    resizing_div: Option<usize>, // which divider (0|1) is being dragged
    bottom_body_y: u16,    // top row of the drawer body
    // Fold is emergent from the divider positions: a column with width < 2 is
    // "folded" (a divider dragged to the edge swallowed it). These are its live
    // body x-ranges, (0,0) when folded, for focus/click hit-testing.
    bottom_col_x: [(u16, u16); 3],
    bottom_hits: Vec<(u16, u16, BottomHit)>,
    bottom_header_y: u16,
    bottom_divider_y: u16,
    toolbar_rect: Rect,
    toolbar_y: u16,
    toolbar_hits: Vec<(u16, u16, ToolHit)>,
    /// Row reserved (above the toolbar) for the open-file tabs. The bufferline
    /// itself is drawn by `EditorView` into this rect, so the two top bars stack
    /// as: file tabs, then the run/debug button toolbar.
    bufferline_rect: Rect,
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
    /// Hit region of the PROJECT header's "select opened file" button:
    /// `(row, x0, x1)`. Zeroed when the project panel is hidden/collapsed.
    locate_hit: (u16, u16, u16),
    /// Hit regions of the PROJECT header's Collapse All / Expand All buttons.
    collapse_hit: (u16, u16, u16),
    expand_hit: (u16, u16, u16),
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
    /// Commits ahead/behind the upstream (shown in the GIT tab label).
    git_ahead: usize,
    git_behind: usize,
    /// Per-changed-file diffstat vs HEAD: (repo-relative path, additions, deletions).
    git_diffstat: Vec<(String, u32, u32)>,
    /// Recent per-commit churn (additions + deletions), oldest→newest, for the
    /// git-panel sparkline.
    git_churn: Vec<u64>,
    /// Body rows consumed above the Git file list (the churn sparkline), so
    /// click-to-open can map a clicked row back to a `git_changes` index.
    git_list_offset: u16,
    /// Number of leading entries in `git_changes` that are merge conflicts.
    /// Conflicts are floated to the front of the list and rendered under a
    /// distinct "Conflicts" header; `idx < git_conflict_count` ⇒ conflict.
    git_conflict_count: usize,
    /// Visual-row → `git_changes` index map for the Git tab body (`None` marks a
    /// section header line). Built during render so clicks map back to the right
    /// entry even with the interleaved "Conflicts"/"Changes" headers.
    git_row_layout: Vec<Option<usize>>,
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

    /// Latest LSP `$/progress` snapshot, mirrored from the editor for the
    /// workbench progress gauge.
    lsp_progress: Option<zemacs_view::editor::LspProgress>,
    /// Build/run progress parsed from the Run console as `(fraction, label)`
    /// when the output contains a parseable `NN%`; `None` otherwise.
    build_progress: Option<(f64, String)>,
    /// Rolling samples of Run-console output rate (lines appended per refresh
    /// tick) for the run sparkline; newest at the back.
    run_rate: std::collections::VecDeque<u64>,
    /// Run-console line count at the previous refresh, to compute the rate delta.
    run_last_len: usize,

    /// One-line debug-session status for the Debug tab header.
    dap_status: String,
    /// Flattened Debug-tab rows: `(kind, text, jump target)` where kind is
    /// 0=section header, 1=stack frame, 2=variable, 3=breakpoint.
    dap_lines: Vec<DapLine>,
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
            ci_state: ratatui::widgets::TableState::default(),
            run: None,
            registers: Vec::new(),
            todos: Vec::new(),
            marks_list: Vec::new(),
            jumplist_rows: Vec::new(),
            recent_rows: Vec::new(),
            recent_times: Vec::new(),
            harpoon_rows: Vec::new(),
            bottom_tab: BottomTab::Problems,
            bottom_tabs: [BottomTab::Problems, BottomTab::Marks, BottomTab::Ci],
            bottom_focus_col: 0,
            bottom_splits: [33, 66],
            bottom_div_x: [0, 0],
            aux_sels: [0, 0, 0],
            resizing_div: None,
            bottom_body_y: 0,
            bottom_col_x: [(0, 0); 3],
            bottom_hits: Vec::new(),
            bottom_header_y: 0,
            bottom_divider_y: u16::MAX,
            toolbar_rect: empty_rect(),
            toolbar_y: 0,
            toolbar_hits: Vec::new(),
            bufferline_rect: empty_rect(),
            total_lines: 1,
            view_top_line: 0,
            cursor_char: 0,
            current_doc_path: None,
            auto_reveal: false,
            last_revealed: None,
            harpoon_slot: None,
            harpoon_total: 0,
            breadcrumb_hit: (0, 0),
            locate_hit: (0, 0, 0),
            collapse_hit: (0, 0, 0),
            expand_hit: (0, 0, 0),
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
            git_ahead: 0,
            git_behind: 0,
            git_diffstat: Vec::new(),
            git_churn: Vec::new(),
            git_list_offset: 0,
            git_conflict_count: 0,
            git_row_layout: Vec::new(),
            aux_sel: 0,
            reg_sel: 0,
            run_row_src: Vec::new(),
            run_error_idx: usize::MAX,
            git_last: None,
            run_total_vis: 0,
            lsp_progress: None,
            build_progress: None,
            run_rate: std::collections::VecDeque::new(),
            run_last_len: 0,
            dap_status: String::new(),
            dap_lines: Vec::new(),
        }
    }

    /// Re-read the project file tree from disk. Called by the filesystem watcher
    /// when files change outside the editor.
    pub fn refresh_tree(&mut self) {
        self.project.refresh();
    }

    /// The row reserved above the toolbar for the open-file tabs (empty when the
    /// workbench is hidden or too short). `EditorView` draws the bufferline here.
    pub fn bufferline_rect(&self) -> Rect {
        if self.visible {
            self.bufferline_rect
        } else {
            empty_rect()
        }
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

    /// Attach a running command to the Run tool window and reveal it — without
    /// stealing keyboard focus (it stays on whatever you were editing).
    pub fn set_run(&mut self, run: crate::ui::run::Run) {
        let prev_focus = self.focus;
        self.run = Some(run);
        self.select_tab(BottomTab::Run);
        self.visible = true;
        self.fold_problems = false;
        self.run_error_idx = usize::MAX;
        self.focus = prev_focus;
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
                    let abs = if pb.is_absolute() {
                        pb.to_path_buf()
                    } else {
                        cwd.join(pb)
                    };
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

    /// Whether column `col` (0 = left, 1 = middle, 2 = right) is folded, i.e. a
    /// divider has been dragged to swallow it (zero width).
    fn col_folded(&self, col: usize) -> bool {
        let [a, b] = self.bottom_splits;
        match col {
            0 => a == 0,
            1 => b.saturating_sub(a) <= 2,
            2 => b >= 100,
            _ => false,
        }
    }

    /// Fold/unfold one drawer column by moving the dividers — the keyboard /
    /// context-menu equivalent of dragging a seam to (or back from) the edge.
    /// Returns the column's new fold state.
    pub fn toggle_col_fold(&mut self, col: usize) -> bool {
        let [a, b] = self.bottom_splits;
        self.bottom_splits = match col {
            0 if a == 0 => [25.min(b), b],             // unfold left
            0 => [0, b],                               // fold left
            2 if b >= 100 => [a, a.max(75)],           // unfold right
            2 => [a, 100],                             // fold right
            1 if b.saturating_sub(a) <= 2 => [33, 66], // unfold middle
            1 => {
                let m = (a + b) / 2;
                [m, m] // fold middle
            }
            _ => [a, b],
        };
        // Keep ordered.
        let [na, nb] = self.bottom_splits;
        self.bottom_splits = [na.min(nb), na.max(nb)];
        self.visible = true;
        self.fold_problems = false;
        self.col_folded(col)
    }

    /// Fold/unfold the middle drawer column (kept for the existing binding).
    pub fn toggle_mid_fold(&mut self) -> bool {
        self.toggle_col_fold(1)
    }

    /// Re-run the last command (same cmd/cwd/shell), revealing the Run tab.
    /// Returns false when there's nothing to re-run.
    pub fn rerun_last(&mut self) -> bool {
        let Some(r) = self.run.clone() else {
            return false;
        };
        let prev_focus = self.focus;
        self.run = Some(crate::ui::run::rerun(&r));
        self.select_tab(BottomTab::Run);
        self.visible = true;
        self.fold_problems = false;
        self.run_error_idx = usize::MAX;
        self.focus = prev_focus;
        true
    }

    /// Stop the active run (SIGTERM the process). No-op if nothing is running.
    pub fn stop_run(&mut self) {
        if let Some(r) = &self.run {
            crate::ui::run::stop(r);
        }
    }

    /// Toggle a panel's fold state (context-menu "Fold"): project / structure /
    /// problems (bottom drawer) / minimap.
    pub fn toggle_fold_panel(&mut self, which: &str) {
        match which {
            "project" => self.fold_project = !self.fold_project,
            "structure" => self.fold_structure = !self.fold_structure,
            "problems" => self.fold_problems = !self.fold_problems,
            "minimap" => self.fold_minimap = !self.fold_minimap,
            _ => {}
        }
    }

    /// Wipe the Run console output (keeps the process running; new output still
    /// streams in) and reveal the Run tab so the cleared console is in view.
    /// Returns false when there's no run to clear.
    pub fn clear_run(&mut self) -> bool {
        let Some(run) = &self.run else { return false };
        if let Ok(mut s) = run.lock() {
            s.lines.clear();
            s.line_widths.clear();
            s.scroll = 0;
            s.follow = true;
        }
        self.select_tab(BottomTab::Run);
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
                self.select_tab(BottomTab::Problems);
                self.focus = Focus::Problems;
            }
            "run" => {
                self.fold_problems = false;
                self.select_tab(BottomTab::Run);
                self.focus = Focus::Problems;
            }
            "git" => {
                self.fold_problems = false;
                self.select_tab(BottomTab::Git);
                self.focus = Focus::Problems;
            }
            "ci" => {
                self.fold_problems = false;
                self.select_tab(BottomTab::Ci);
                self.focus = Focus::Problems;
            }
            // Bottom-panel tool windows that already render as tabs but were not
            // directly command-focusable (JetBrains-style Bookmarks/Services/etc.).
            "harpoon" | "bookmarks" => {
                self.fold_problems = false;
                self.select_tab(BottomTab::Harpoon);
                self.focus = Focus::Problems;
            }
            "marks" => {
                self.fold_problems = false;
                self.select_tab(BottomTab::Marks);
                self.focus = Focus::Problems;
            }
            "registers" => {
                self.fold_problems = false;
                self.select_tab(BottomTab::Registers);
                self.focus = Focus::Problems;
            }
            "jumplist" => {
                self.fold_problems = false;
                self.select_tab(BottomTab::Jumplist);
                self.focus = Focus::Problems;
            }
            "recent" => {
                self.fold_problems = false;
                self.select_tab(BottomTab::Recent);
                self.focus = Focus::Problems;
            }
            "todo" => {
                self.fold_problems = false;
                self.select_tab(BottomTab::Todo);
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

    /// JetBrains "Select Opened File": expand the left tree and reveal the file
    /// currently being edited. Falls back to just focusing the tree when the
    /// buffer has no path (e.g. a scratch buffer).
    pub fn reveal_current(&mut self) {
        match self.current_doc_path.clone() {
            Some(path) => self.reveal(&path),
            None => {
                self.visible = true;
                self.fold_project = false;
                self.left_collapsed = false;
                self.focus = Focus::Project;
            }
        }
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
            fold_minimap: self.fold_minimap,
            bottom_height: self.bottom_height,
            bottom_zoom: self.bottom_zoom,
            bottom_splits: self.bottom_splits,
            bottom_mid_folded: self.col_folded(1),
            bottom_left_folded: self.col_folded(0),
            bottom_right_folded: self.col_folded(2),
            auto_reveal: self.auto_reveal,
            bottom_tabs: self
                .bottom_tabs
                .iter()
                .map(|t| t.persist_name().to_string())
                .collect(),
            bottom_focus_col: self.bottom_focus_col,
            expanded_dirs: self
                .project
                .expanded_paths()
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        }
    }

    /// Restore a persisted drawer layout.
    /// Restore the persisted geometry/fold state. Does NOT touch `visible` — the
    /// caller decides whether the workbench is shown (so opening it on demand via
    /// `:ide` still shows it even if it was closed when last saved, while startup
    /// restore gates on the persisted `open` flag).
    pub fn apply_layout(&mut self, l: &crate::appdata::IdeLayout) {
        self.left_width = if l.left_width >= 14 {
            l.left_width
        } else {
            LEFT_W
        };
        self.left_collapsed = l.left_collapsed;
        self.fold_project = l.fold_project;
        self.fold_structure = l.fold_structure;
        self.fold_problems = l.fold_problems;
        self.fold_minimap = l.fold_minimap;
        if l.bottom_height > 0 {
            self.bottom_height = l.bottom_height;
        }
        self.bottom_zoom = l.bottom_zoom;
        // Accept any ordered split pair (edges allowed so a folded column — s0=0,
        // s1=100, or s0==s1 — survives a restore); reject only disordered pairs.
        if l.bottom_splits[0] <= l.bottom_splits[1] && l.bottom_splits[1] <= 100 {
            self.bottom_splits = l.bottom_splits;
        }
        // Migrate the older per-column fold booleans into divider positions.
        if l.bottom_left_folded {
            self.bottom_splits[0] = 0;
        }
        if l.bottom_right_folded {
            self.bottom_splits[1] = 100;
        }
        if l.bottom_mid_folded {
            let m = (self.bottom_splits[0] + self.bottom_splits[1]) / 2;
            self.bottom_splits = [m, m];
        }
        self.auto_reveal = l.auto_reveal;
        if l.bottom_focus_col < 3 {
            self.bottom_focus_col = l.bottom_focus_col;
        }
        for (i, name) in l.bottom_tabs.iter().enumerate().take(3) {
            if let Some(tab) = BottomTab::from_persist_name(name) {
                self.bottom_tabs[i] = tab;
            }
        }
        self.bottom_tab = self.bottom_tabs[self.bottom_focus_col];
        if !l.expanded_dirs.is_empty() {
            self.project
                .set_expanded_paths(l.expanded_dirs.iter().map(PathBuf::from));
        }
    }

    /// F2: toggle the whole workbench (and focus the tree when showing).
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        self.focus = if self.visible {
            Focus::Project
        } else {
            Focus::Editor
        };
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
    /// Which column (0|1|2) a tab lives in. Tabs are distributed 3/3/4.
    fn tab_col(t: BottomTab) -> usize {
        match t {
            BottomTab::Problems | BottomTab::Run | BottomTab::Git | BottomTab::Debug => 0,
            BottomTab::Registers | BottomTab::Todo | BottomTab::Marks | BottomTab::Jumplist => 1,
            BottomTab::Recent | BottomTab::Harpoon | BottomTab::Ci => 2,
        }
    }

    /// Refresh the `bottom_tab` mirror from the focused column.
    fn sync_bottom_tab(&mut self) {
        self.bottom_tab = self.bottom_tabs[self.bottom_focus_col];
    }

    /// True when the CI panel is on screen (a column shows it, drawer open).
    fn ci_visible(&self) -> bool {
        self.visible && !self.fold_problems && self.bottom_tabs.contains(&BottomTab::Ci)
    }

    /// Move keyboard focus to column `c`, swapping the live `aux_sel` so each
    /// column keeps its own list selection.
    fn set_focus_col(&mut self, c: usize) {
        let c = c.min(2);
        if c != self.bottom_focus_col {
            self.aux_sels[self.bottom_focus_col] = self.aux_sel;
            self.bottom_focus_col = c;
            self.aux_sel = self.aux_sels[c];
        }
    }

    /// Show `t` in its column without moving keyboard focus.
    #[allow(dead_code)] // companion to select_tab; kept for the bottom-tab API
    fn show_tab(&mut self, t: BottomTab) {
        self.bottom_tabs[Self::tab_col(t)] = t;
        self.sync_bottom_tab();
    }

    /// Show `t` in its column and give that column keyboard focus.
    fn select_tab(&mut self, t: BottomTab) {
        let c = Self::tab_col(t);
        if c == 1 {
            // Selecting a middle-column tab reveals the middle column if folded.
            if self.bottom_splits[1].saturating_sub(self.bottom_splits[0]) <= 2 {
                self.bottom_splits = [33, 66];
            }
        }
        self.set_focus_col(c);
        self.bottom_tabs[c] = t;
        self.aux_sel = 0;
        self.aux_sels[c] = 0;
        self.focus = Focus::Problems;
        self.sync_bottom_tab();
    }

    fn render_tab_body(
        &mut self,
        tab: BottomTab,
        surface: &mut Surface,
        theme: &zemacs_view::Theme,
        rect: Rect,
    ) {
        match tab {
            BottomTab::Problems => self.render_problems_body(surface, theme, rect),
            BottomTab::Run => self.render_run_body(surface, theme, rect),
            BottomTab::Git => self.render_git_body(surface, theme, rect),
            BottomTab::Debug => self.render_debug_body(surface, theme, rect),
            BottomTab::Registers => self.render_registers_body(surface, theme, rect),
            BottomTab::Todo => self.render_todo_body(surface, theme, rect),
            BottomTab::Marks => self.render_marks_body(surface, theme, rect),
            BottomTab::Jumplist => self.render_jumplist_body(surface, theme, rect),
            BottomTab::Recent => self.render_recent_body(surface, theme, rect),
            BottomTab::Harpoon => self.render_harpoon_body(surface, theme, rect),
            BottomTab::Ci => self.render_ci_body(surface, theme, rect),
        }
    }

    /// Cycle the focused column through its own tab group (`[` / `]`).
    fn cycle_bottom_tab(&mut self, forward: bool) {
        use BottomTab::*;
        const G0: [BottomTab; 4] = [Problems, Run, Git, Debug];
        const G1: [BottomTab; 4] = [Registers, Todo, Marks, Jumplist];
        const G2: [BottomTab; 3] = [Recent, Harpoon, Ci];
        let col = self.bottom_focus_col;
        let order: &[BottomTab] = match col {
            0 => &G0,
            1 => &G1,
            _ => &G2,
        };
        let cur = order
            .iter()
            .position(|t| *t == self.bottom_tabs[col])
            .unwrap_or(0);
        let n = order.len();
        let next = if forward {
            (cur + 1) % n
        } else {
            (cur + n - 1) % n
        };
        self.bottom_tabs[col] = order[next];
        self.aux_sel = 0;
        self.aux_sels[col] = 0;
        self.sync_bottom_tab();
    }

    /// Row count of the currently active simple-list tab (Todo/Marks/Jumps/Recent).
    fn aux_len(&self) -> usize {
        match self.bottom_tab {
            BottomTab::Todo => self.todos.len(),
            BottomTab::Marks => self.marks_list.len(),
            BottomTab::Jumplist => self.jumplist_rows.len(),
            BottomTab::Recent => self.recent_rows.len(),
            BottomTab::Harpoon => self.harpoon_rows.len(),
            BottomTab::Ci => crate::ci::snapshot().len(),
            _ => 0,
        }
    }

    /// Open / jump to the `aux_sel` row of the active simple-list tab (Enter).
    fn activate_aux(&mut self) -> IdeAction {
        match self.bottom_tab {
            BottomTab::Todo => self
                .todos
                .get(self.aux_sel)
                .map(|(pos, _, _)| IdeAction::Goto {
                    from: *pos,
                    to: *pos,
                })
                .unwrap_or(IdeAction::None),
            BottomTab::Marks => self
                .marks_list
                .get(self.aux_sel)
                .map(|(pos, _)| IdeAction::Goto {
                    from: *pos,
                    to: *pos,
                })
                .unwrap_or(IdeAction::None),
            BottomTab::Jumplist => match self.jumplist_rows.get(self.aux_sel) {
                Some((Some(path), _, _)) if path.is_file() => {
                    self.focus = Focus::Editor;
                    IdeAction::OpenFile(path.clone())
                }
                Some((None, pos, _)) => IdeAction::Goto {
                    from: *pos,
                    to: *pos,
                },
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
            BottomTab::Ci => crate::ci::snapshot()
                .get(self.aux_sel)
                .map(|r| IdeAction::OpenUrl(r.url.clone()))
                .unwrap_or(IdeAction::None),
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
        let cur = if s.follow {
            max_top
        } else {
            s.scroll.min(max_top)
        };
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
                            // Conflicts jump straight into the merge resolver.
                            if self.git_sel < self.git_conflict_count {
                                return IdeAction::ResolveConflict(path.clone());
                            }
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
        if !structure
            && self.bottom_tab == BottomTab::Harpoon
            && matches!(key.code, KeyCode::Char('K') | KeyCode::Char('J'))
        {
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
        // Simple list tabs (Todo/Marks/Jumps/Recent/Harpoon/CI): j/k select, Enter
        // activates (CI opens the run in the browser via activate_aux).
        if !structure
            && matches!(
                self.bottom_tab,
                BottomTab::Todo
                    | BottomTab::Marks
                    | BottomTab::Jumplist
                    | BottomTab::Recent
                    | BottomTab::Harpoon
                    | BottomTab::Ci
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
        let len = if structure {
            self.structure.len()
        } else {
            self.problems.len()
        };
        let sel = if structure {
            &mut self.structure_sel
        } else {
            &mut self.problems_sel
        };
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
                .map(|o| IdeAction::Goto {
                    from: o.start,
                    to: o.end,
                })
                .unwrap_or(IdeAction::None)
        } else {
            self.problems
                .get(self.problems_sel)
                .map(|p| IdeAction::Goto {
                    from: p.start,
                    to: p.end,
                })
                .unwrap_or(IdeAction::None)
        }
    }

    pub fn handle_mouse(
        &mut self,
        ev: &MouseEvent,
        line_to_char: impl Fn(usize) -> usize,
    ) -> IdeAction {
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
                        Some(ToolHit::Locate) => {
                            self.reveal_current();
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
                // "Select Opened File" button on the PROJECT header → reveal the
                // current buffer in the tree (expanding/uncollapsing as needed).
                if row == self.locate_hit.0
                    && col >= self.locate_hit.1
                    && col < self.locate_hit.2
                    && self.locate_hit.2 > 0
                {
                    self.reveal_current();
                    return IdeAction::None;
                }
                // Collapse All / Expand All buttons on the PROJECT header.
                if self.collapse_hit.2 > 0
                    && row == self.collapse_hit.0
                    && col >= self.collapse_hit.1
                    && col < self.collapse_hit.2
                {
                    self.project.collapse_all();
                    return IdeAction::None;
                }
                if self.expand_hit.2 > 0
                    && row == self.expand_hit.0
                    && col >= self.expand_hit.1
                    && col < self.expand_hit.2
                {
                    self.project.expand_all();
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
                        Some(BottomHit::TabProblems) => self.select_tab(BottomTab::Problems),
                        Some(BottomHit::TabRun) => self.select_tab(BottomTab::Run),
                        Some(BottomHit::TabGit) => self.select_tab(BottomTab::Git),
                        Some(BottomHit::TabDebug) => self.select_tab(BottomTab::Debug),
                        Some(BottomHit::TabRegisters) => self.select_tab(BottomTab::Registers),
                        Some(BottomHit::TabTodo) => self.select_tab(BottomTab::Todo),
                        Some(BottomHit::TabMarks) => self.select_tab(BottomTab::Marks),
                        Some(BottomHit::TabJumplist) => self.select_tab(BottomTab::Jumplist),
                        Some(BottomHit::TabRecent) => self.select_tab(BottomTab::Recent),
                        Some(BottomHit::TabHarpoon) => self.select_tab(BottomTab::Harpoon),
                        Some(BottomHit::TabCi) => self.select_tab(BottomTab::Ci),
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
                    // Clicking the tree focuses it and keeps focus here (so `/`
                    // speed-search, j/k, etc. route to the tree) — opening a file
                    // does NOT jump focus to the editor. Click the editor body (or
                    // press Enter/Esc) to move focus there.
                    self.focus = Focus::Project;
                    let lr = (row - self.project_rect.y - 1) as usize;
                    return match self.project.click_row(lr) {
                        TreeAction::Open(p) => IdeAction::OpenFile(p),
                        _ => IdeAction::None,
                    };
                }
                if in_rect(&self.structure_rect, col, row) && row > self.structure_rect.y {
                    self.focus = Focus::Structure;
                    let idx =
                        self.structure_state.offset() + (row - self.structure_rect.y - 1) as usize;
                    if idx < self.structure.len() {
                        self.structure_sel = idx;
                        let o = &self.structure[idx];
                        return IdeAction::Goto {
                            from: o.start,
                            to: o.end,
                        };
                    }
                }
                // Three-column drawer: a body click focuses the column under the
                // cursor (so the row handlers below act on that column's tab); a
                // click on a vertical divider starts a resize.
                if row > self.bottom_header_y && in_rect(&self.problems_rect, col, row) {
                    // A click on (or within one cell of) a live divider starts a
                    // resize drag. A folded column's divider sits at the drawer
                    // edge as a grab handle, so the ±1 tolerance makes the edge
                    // handle catchable and dragging it back inward unfolds.
                    let [d0, d1] = self.bottom_div_x;
                    let near0 = d0 != 0 && col.abs_diff(d0) <= 1;
                    let near1 = d1 != 0 && col.abs_diff(d1) <= 1;
                    if near0 || near1 {
                        // Grab whichever divider is closer to the click.
                        let grab = if near0 && (!near1 || col.abs_diff(d0) <= col.abs_diff(d1)) {
                            0
                        } else {
                            1
                        };
                        self.resizing_div = Some(grab);
                        return IdeAction::None;
                    }
                    // Otherwise focus the column whose body range holds the cursor.
                    if let Some(c) = (0..3).find(|&i| {
                        let (x0, x1) = self.bottom_col_x[i];
                        x1 > x0 && col >= x0 && col < x1
                    }) {
                        self.set_focus_col(c);
                    }
                    self.focus = Focus::Problems;
                    self.sync_bottom_tab();
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Todo
                {
                    let idx = (row - self.problems_rect.y - 1) as usize;
                    if let Some((pos, _, _)) = self.todos.get(idx) {
                        self.aux_sel = idx;
                        return IdeAction::Goto {
                            from: *pos,
                            to: *pos,
                        };
                    }
                    return IdeAction::None;
                }
                // Debug tab: click a stack frame / breakpoint row to jump to it.
                // Body row 0 is the status line, so the list starts 2 rows below.
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y + 1
                    && self.bottom_tab == BottomTab::Debug
                {
                    let idx = (row - self.problems_rect.y - 2) as usize;
                    if let Some((_, _, Some((path, line)))) = self.dap_lines.get(idx) {
                        self.focus = Focus::Editor;
                        return IdeAction::OpenFileAt {
                            path: path.clone(),
                            line: *line,
                        };
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Git
                {
                    // account for the churn sparkline above the file list
                    let Some(vis) = (row - self.problems_rect.y - 1)
                        .checked_sub(self.git_list_offset)
                        .map(|v| v as usize)
                    else {
                        return IdeAction::None;
                    };
                    // map the clicked visual row back to a change index, skipping
                    // the "Conflicts"/"Changes" section-header lines.
                    let Some(Some(idx)) = self.git_row_layout.get(vis).copied() else {
                        return IdeAction::None;
                    };
                    if let Some((_, _, path)) = self.git_changes.get(idx) {
                        self.git_sel = idx;
                        if path.is_file() {
                            self.focus = Focus::Editor;
                            if idx < self.git_conflict_count {
                                return IdeAction::ResolveConflict(path.clone());
                            }
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
                        return IdeAction::Goto {
                            from: *pos,
                            to: *pos,
                        };
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
                            None => {
                                return IdeAction::Goto {
                                    from: *pos,
                                    to: *pos,
                                }
                            }
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
                    && self.bottom_tab == BottomTab::Ci
                {
                    let idx = (row - self.problems_rect.y - 1) as usize;
                    let runs = crate::ci::snapshot();
                    if let Some(r) = runs.get(idx) {
                        self.aux_sel = idx;
                        return IdeAction::OpenUrl(r.url.clone());
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
                            let abs = if pb.is_absolute() {
                                pb.to_path_buf()
                            } else {
                                cwd.join(pb)
                            };
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
                    let idx =
                        self.problems_state.offset() + (row - self.problems_rect.y - 1) as usize;
                    if idx < self.problems.len() {
                        self.problems_sel = idx;
                        let p = &self.problems[idx];
                        return IdeAction::Goto {
                            from: p.start,
                            to: p.end,
                        };
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
                    let line = ((frac * self.total_lines as f32) as usize)
                        .min(self.total_lines.saturating_sub(1));
                    let pos = line_to_char(line);
                    return IdeAction::Goto { from: pos, to: pos };
                }
                IdeAction::None
            }
            MouseEventKind::Down(MouseButton::Right) => {
                use crate::ui::context_menu::{ContextMenu, Entry};
                // A "Fold <panel>" entry that toggles the given panel's fold state.
                fn fold_item(label: &'static str, which: &'static str) -> Entry {
                    Entry::item(label, move |compositor, _cx| {
                        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                            view.ide_toggle_fold(which);
                        }
                    })
                }

                // Right-click a file-tree entry → CRUD context menu (+ Fold Panel).
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
                // Right-click the file-tree header / empty area → root New + Fold.
                if in_rect(&self.project_rect, col, row) {
                    let root = self.project.root_path();
                    let entries = vec![
                        Entry::sub(
                            "New",
                            vec![
                                {
                                    let r = root.clone();
                                    Entry::item("File", move |compositor, _cx| {
                                        compositor.push(Box::new(name_prompt(
                                            FileActionKind::NewFile,
                                            r.clone(),
                                            true,
                                        )));
                                    })
                                },
                                {
                                    let r = root.clone();
                                    Entry::item("Directory", move |compositor, _cx| {
                                        compositor.push(Box::new(name_prompt(
                                            FileActionKind::NewFolder,
                                            r.clone(),
                                            true,
                                        )));
                                    })
                                },
                            ],
                        ),
                        Entry::sep(),
                        fold_item("Fold Panel", "project"),
                    ];
                    return IdeAction::ShowMenu(ContextMenu::new(row, col, entries));
                }
                // Right-click the bottom drawer (Run console) → run controls + Fold.
                if in_rect(&self.problems_rect, col, row) {
                    let entries = vec![
                        Entry::item("Rerun", |compositor, cx| {
                            if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                                view.rerun_last_run(cx);
                            }
                        }),
                        Entry::item("Stop", |compositor, _cx| {
                            if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                                view.stop_active_run();
                            }
                        }),
                        Entry::item("Clear", |compositor, cx| {
                            if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                                view.clear_run_output(cx);
                            }
                        }),
                        Entry::sep(),
                        fold_item("Fold Panel", "problems"),
                    ];
                    return IdeAction::ShowMenu(ContextMenu::new(row, col, entries));
                }
                // Right-click the Structure outline → jump to the symbol (+ Fold).
                if in_rect(&self.structure_rect, col, row) {
                    let idx = self.structure_state.offset()
                        + (row.saturating_sub(self.structure_rect.y + 1)) as usize;
                    let mut entries = Vec::new();
                    if row > self.structure_rect.y {
                        if let Some(o) = self.structure.get(idx) {
                            let (from, to) = (o.start, o.end);
                            let name = o.name.clone();
                            entries.push(Entry::item("Go to Symbol", move |_c, cx| {
                                let view_id = cx.editor.tree.focus;
                                let doc_id = cx.editor.tree.get(view_id).doc;
                                if let Some(doc) = cx.editor.documents.get_mut(&doc_id) {
                                    doc.set_selection(view_id, goto_selection(from, to));
                                }
                                cx.editor.ensure_cursor_in_view(view_id);
                            }));
                            entries.push(Entry::item("Copy Name", move |_c, cx| {
                                let _ = cx.editor.registers.push('"', name.clone());
                                cx.editor.set_status(format!("yanked {name}"));
                            }));
                            entries.push(Entry::sep());
                        }
                    }
                    entries.push(fold_item("Fold Panel", "structure"));
                    return IdeAction::ShowMenu(ContextMenu::new(row, col, entries));
                }
                // Right-click the minimap → fold it (or expand when collapsed).
                if in_rect(&self.stripe_rect, col, row) {
                    let label = if self.fold_minimap {
                        "Expand Minimap"
                    } else {
                        "Fold Minimap"
                    };
                    let entries = vec![fold_item(label, "minimap")];
                    return IdeAction::ShowMenu(ContextMenu::new(row, col, entries));
                }
                IdeAction::None
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = matches!(ev.kind, MouseEventKind::ScrollDown);
                if in_rect(&self.project_rect, col, row) {
                    self.project.scroll_sel(down);
                } else if in_rect(&self.structure_rect, col, row) {
                    step(&mut self.structure_sel, self.structure.len(), down);
                } else if in_rect(&self.problems_rect, col, row)
                    && self.bottom_tab == BottomTab::Run
                {
                    // scroll the run console; reaching the bottom re-enables tail-follow
                    if let Some(run) = &self.run {
                        let mut s = run.lock().unwrap();
                        let h = self.problems_rect.height.saturating_sub(1) as usize;
                        let max_top = self.run_total_vis.saturating_sub(h);
                        let cur = if s.follow {
                            max_top
                        } else {
                            s.scroll.min(max_top)
                        };
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
            MouseEventKind::Drag(MouseButton::Left) if self.resizing_div.is_some() => {
                // Dragging a vertical divider sets its position as a % of the
                // drawer. Snapping near an edge folds the pane on that side to
                // zero width; the two dividers may meet to fold the middle.
                if self.problems_rect.width > 2 {
                    let rel = col.saturating_sub(self.problems_rect.x);
                    let mut pct =
                        (rel as u32 * 100 / self.problems_rect.width as u32).min(100) as u16;
                    if pct <= 3 {
                        pct = 0;
                    }
                    if pct >= 97 {
                        pct = 100;
                    }
                    match self.resizing_div {
                        Some(0) => self.bottom_splits[0] = pct.min(self.bottom_splits[1]),
                        Some(1) => self.bottom_splits[1] = pct.max(self.bottom_splits[0]),
                        _ => {}
                    }
                }
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
                self.resizing_div = None;
                self.resizing_stripe = false;
                IdeAction::None
            }
            _ => IdeAction::None,
        }
    }

    /// Render every panel; returns the rect left for the editor.
    pub fn render(
        &mut self,
        area: Rect,
        surface: &mut Surface,
        cx: &mut crate::compositor::Context,
    ) -> Rect {
        if !self.visible {
            return area;
        }
        self.refresh(cx);
        // Keep the project tree's dotfile visibility in sync with the config
        // (`editor.file-explorer.hidden`; false = show dotfiles, the default).
        self.project
            .set_show_hidden(!cx.editor.config().file_explorer.hidden);

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

        // bottom drawer (PROBLEMS/RUN/GIT) — carved FIRST so it spans the FULL
        // width below everything (like JetBrains), with its divider a clean
        // full-width separator. The left column + editor are then carved from the
        // remaining height, so the file tree stops above the drawer instead of
        // running alongside it (which made the divider look like it bled across).
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

        // left column: project (top) + structure (bottom); drag the seam to resize, collapse to a rail
        if self.left_collapsed {
            self.left_rail_rect = Rect::new(rest.x, rest.y, 1, rest.height);
            self.project_rect = empty_rect();
            self.structure_rect = empty_rect();
            self.seam_x = u16::MAX;
            rest = Rect::new(
                rest.x + 1,
                rest.y,
                rest.width.saturating_sub(1),
                rest.height,
            );
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
            rest = Rect::new(
                rest.x + self.left_width,
                rest.y,
                rest.width - self.left_width,
                rest.height,
            );
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
            self.stripe_rect = Rect::new(
                rest.x + rest.width - STRIPE_W,
                rest.y,
                STRIPE_W,
                rest.height,
            );
            rest = Rect::new(rest.x, rest.y, rest.width - STRIPE_W, rest.height);
        } else {
            self.stripe_rect = empty_rect();
        }

        // Two stacked top bars over the editor region: row 1 = open-file tabs
        // (the bufferline, drawn by EditorView into `bufferline_rect`), row 2 =
        // the run/debug button toolbar. Reserve both here so the file names sit
        // above the buttons.
        if rest.height > 4 {
            self.bufferline_rect = Rect::new(rest.x, rest.y, rest.width, 1);
            self.toolbar_rect = Rect::new(rest.x, rest.y + 1, rest.width, 1);
            rest = Rect::new(rest.x, rest.y + 2, rest.width, rest.height - 2);
        } else if rest.height > 3 {
            self.bufferline_rect = empty_rect();
            self.toolbar_rect = Rect::new(rest.x, rest.y, rest.width, 1);
            rest = Rect::new(rest.x, rest.y + 1, rest.width, rest.height - 1);
        } else {
            self.bufferline_rect = empty_rect();
            self.toolbar_rect = empty_rect();
        }

        self.view_lines = rest.height as usize;

        let theme = &cx.editor.theme;
        if self.toolbar_rect.height > 0 {
            self.render_toolbar(surface, theme);
        }
        if self.project_rect.height > 0 {
            surface.clear_with(self.project_rect, theme.get("ui.background"));
            draw_header(
                surface,
                self.project_rect,
                "PROJECT",
                self.fold_project,
                self.focus == Focus::Project,
                theme,
            );
            // JetBrains-style "Select Opened File" button at the header's right
            // edge: ◎ when "always select" is on, ⊙ for a one-shot reveal.
            self.locate_hit = (0, 0, 0);
            if self.project_rect.width > 8 {
                let icon = if self.auto_reveal { "◎" } else { "⊙" };
                let ix = self.project_rect.x + self.project_rect.width - 2;
                let st = if self.auto_reveal {
                    theme.get("function")
                } else {
                    theme.get("comment")
                };
                surface.set_stringn(ix, self.project_rect.y, icon, 1, st);
                self.locate_hit = (self.project_rect.y, ix, ix + 1);
            }
            // JetBrains-style Collapse All (⊟) / Expand All (⊞) buttons, to the
            // left of the "select opened file" button.
            self.collapse_hit = (0, 0, 0);
            self.expand_hit = (0, 0, 0);
            if self.project_rect.width > 12 {
                let cc = theme.get("comment");
                let cx0 = self.project_rect.x + self.project_rect.width - 4;
                let ex0 = self.project_rect.x + self.project_rect.width - 6;
                surface.set_stringn(ex0, self.project_rect.y, "⊞", 1, cc);
                surface.set_stringn(cx0, self.project_rect.y, "⊟", 1, cc);
                self.expand_hit = (self.project_rect.y, ex0, ex0 + 1);
                self.collapse_hit = (self.project_rect.y, cx0, cx0 + 1);
            }
            if !self.fold_project && self.project_rect.height > 1 {
                self.project
                    .render(body_rect(self.project_rect), surface, theme);
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
                        ..line
                            .char_indices()
                            .nth(mid + 3)
                            .map(|(i, _)| i)
                            .unwrap_or(line.len()),
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
                surface.set_stringn(
                    self.problems_rect.x + 1,
                    self.bottom_divider_y,
                    hint,
                    maxw,
                    theme.get("comment"),
                );
            }
        }

        // resize seam / collapse rail down the left edge — only as tall as the
        // left column (stop at the full-width bottom drawer, not through it).
        let seam_h = if self.bottom_divider_y != u16::MAX {
            self.bottom_divider_y.saturating_sub(full.y)
        } else {
            full.height
        };
        if self.left_collapsed && self.left_rail_rect.height > 0 {
            surface.set_string(
                self.left_rail_rect.x,
                full.y,
                "›",
                theme.get("ui.text.focus"),
            );
            for y in 1..seam_h {
                surface.set_string(
                    self.left_rail_rect.x,
                    full.y + y,
                    "▏",
                    theme.get("ui.window"),
                );
            }
        } else if self.seam_x != u16::MAX {
            let style = theme.get("ui.window");
            for y in 0..seam_h {
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
            2 => (
                "INSERT",
                "ui.statusline.insert",
                Color::Rgb(0x00, 0xb3, 0xd7),
            ),
            1 => (
                "VISUAL",
                "ui.statusline.select",
                Color::Rgb(0xff, 0x8c, 0x00),
            ),
            _ => (
                "NORMAL",
                "ui.statusline.normal",
                Color::Rgb(0x9e, 0xd0, 0x10),
            ),
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
        let warn = theme
            .get("warning")
            .fg
            .unwrap_or(Color::Rgb(0x7a, 0xa8, 0x10));
        let fill = theme
            .get("ui.statusline")
            .bg
            .unwrap_or(Color::Rgb(0x1b, 0x1b, 0x20));
        let seg = |bg: Color, fg: Color| Style::default().bg(bg).fg(fg);

        surface.clear_with(area, seg(fill, darkfg));
        let bold = Modifier::BOLD;

        // ── left segments ──────────────────────────────────────────────
        let mut left: Vec<(String, Style)> = Vec::new();
        left.push((
            format!(" {mode_txt} "),
            seg(mode_bg, mode_fg).add_modifier(bold),
        ));
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
            x += disp_width(text).min(right_edge - x);
            if x >= right_edge {
                break;
            }
            let next_bg = left.get(i + 1).and_then(|(_, s)| s.bg).unwrap_or(fill);
            surface.set_stringn(
                x,
                area.y,
                SEP_R,
                1,
                Style::default().fg(style.bg.unwrap_or(fill)).bg(next_bg),
            );
            x += 1;
        }

        // draw right→left, separators point left () with the segment's bg as fg
        let mut rx = right_edge;
        for i in (0..right.len()).rev() {
            let (text, style) = &right[i];
            let w = disp_width(text);
            if rx <= x + w {
                break; // would collide with the left cluster
            }
            rx -= w;
            surface.set_stringn(rx, area.y, text, w as usize, *style);
            if rx <= x {
                break;
            }
            rx -= 1;
            let left_bg = if i == 0 {
                fill
            } else {
                right[i - 1].1.bg.unwrap_or(fill)
            };
            surface.set_stringn(
                rx,
                area.y,
                SEP_L,
                1,
                Style::default().fg(style.bg.unwrap_or(fill)).bg(left_bg),
            );
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

        // CI: the panel can be on screen without ever being explicitly focused
        // (it's a default column), so kick off the first fetch here. `loading`
        // (set inside spawn_fetch) gates against re-spawning every frame.
        if self.ci_visible() && !crate::ci::fetched() && !crate::ci::status().0 {
            crate::ci::spawn_fetch(cx.jobs);
        }

        // ── Debug session snapshot (Debug tool window) ─────────────────────────
        self.dap_lines.clear();
        match cx.editor.debug_adapters.get_active_client() {
            Some(c) => {
                self.dap_status = match c.thread_id {
                    Some(t) => format!("● stopped · thread {t}"),
                    None => "▶ running".to_string(),
                };
                if let Some(t) = c.thread_id {
                    if let Some(frames) = c.stack_frames.get(&t) {
                        self.dap_lines.push((0, "CALL STACK".to_string(), None));
                        for (i, f) in frames.iter().enumerate() {
                            let target = f
                                .source
                                .as_ref()
                                .and_then(|s| s.path.clone())
                                .map(|p| (p, f.line));
                            let marker = if Some(i) == c.active_frame {
                                "▶ "
                            } else {
                                "  "
                            };
                            self.dap_lines
                                .push((1, format!("{marker}{}", f.name), target));
                        }
                    }
                }
                if !cx.editor.dap_variables.is_empty() {
                    self.dap_lines.push((0, "VARIABLES".to_string(), None));
                    for (name, val) in &cx.editor.dap_variables {
                        let text = if val.is_empty() {
                            name.clone()
                        } else {
                            format!("{name} = {val}")
                        };
                        self.dap_lines.push((2, text, None));
                    }
                }
            }
            None => self.dap_status = "no debug session — :dap-launch".to_string(),
        }
        // Breakpoints (shown whether or not a session is live).
        let mut bps: Vec<(std::path::PathBuf, usize)> = Vec::new();
        for (path, list) in &cx.editor.breakpoints {
            for b in list {
                bps.push((path.clone(), b.line));
            }
        }
        if !bps.is_empty() {
            bps.sort();
            self.dap_lines.push((0, "BREAKPOINTS".to_string(), None));
            for (p, line) in bps {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.dap_lines
                    .push((3, format!("{name}:{}", line + 1), Some((p, line + 1))));
            }
        }

        // ── LSP progress + run output-rate / build-progress (workbench gauges) ─
        self.lsp_progress = cx.editor.lsp_progress.clone();
        self.build_progress = None;
        if let Some(run) = self.run.clone() {
            if let Ok(s) = run.lock() {
                // Output rate: lines appended since the previous refresh tick.
                let len = s.lines.len();
                let delta = len.saturating_sub(self.run_last_len) as u64;
                self.run_last_len = len;
                self.run_rate.push_back(delta);
                while self.run_rate.len() > 64 {
                    self.run_rate.pop_front();
                }
                // Build progress: the most recent output line carrying an `NN%`
                // token, while the command is still running. Cargo's `\r` progress
                // bar isn't captured, but many tools print a plain percentage.
                if s.running {
                    for line in s.lines.iter().rev().take(40) {
                        if let Some(pct) = parse_percent(line) {
                            let label = line.trim().chars().take(40).collect::<String>();
                            self.build_progress = Some((pct as f64 / 100.0, label));
                            break;
                        }
                    }
                }
            }
        } else {
            self.run_rate.clear();
            self.run_last_len = 0;
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
                .map(|o| OutlineRow {
                    kind: o.kind,
                    name: o.name,
                    start: o.start,
                    end: o.end,
                })
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

            // TODO tool window: scan for TODO/FIXME/… markers (word-boundary matched).
            self.todos.clear();
            for i in 0..text.len_lines() {
                let line: String = text.line(i).chars().filter(|c| !c.is_control()).collect();
                if let Some(marker) = todo_marker(&line) {
                    self.todos.push((
                        text.line_to_char(i),
                        format!("{}: {}", i + 1, line.trim()),
                        marker,
                    ));
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
                    let lt: String = text
                        .line(line)
                        .chars()
                        .filter(|ch| !ch.is_control())
                        .collect();
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
            self.harpoon_rows
                .iter()
                .position(|m| *m == cp)
                .map(|i| i + 1)
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
            .filter(|r| !r.is_empty())
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
            let stale = self.git_last.is_none_or(|t| t.elapsed().as_millis() > 800);
            if stale {
                if let Some(dir) = self.status_branch_dir.clone() {
                    self.git_changes = git_status(&dir);
                    // Float merge-conflict entries to the front so the Git tab can
                    // render them as a distinct "Conflicts" section; the rest keep
                    // their porcelain order (sort_by_key is stable).
                    self.git_changes
                        .sort_by_key(|(code, _, _)| !git_is_conflict(code));
                    self.git_conflict_count = self
                        .git_changes
                        .iter()
                        .take_while(|(code, _, _)| git_is_conflict(code))
                        .count();
                    (self.git_ahead, self.git_behind) = git_ahead_behind(&dir);
                    self.git_diffstat = git_diffstat(&dir);
                    self.git_churn = git_churn(&dir);
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
                        self.jumplist_rows
                            .push((target, pos, format!("{name}:{line}")));
                    }
                }
            }
        }

        // Recently opened files. Loaded every refresh (the store is a small file and
        // rendering is event-driven, not a constant tick) so the RECENT tab label can
        // show an always-current count.
        let recent = crate::recent_files::load_with_time();
        self.recent_times = recent.iter().map(|(_, t)| *t).collect();
        self.recent_rows = recent.into_iter().map(|(p, _)| p).collect();
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
        let count_style =
            crate::ui::rat::to_rat_style(theme.get("keyword")).add_modifier(RMod::BOLD);
        // The ratatui render blits an offscreen buffer, so empty rows would clobber our clear_with
        // back to a transparent bg — paint the whole block with the panel background to prevent that.
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(crate::ui::rat::to_rat_style(theme.get("ui.window")))
            .style(crate::ui::rat::to_rat_style(theme.get("ui.background")))
            .title(Span::styled(title, title_style))
            .title(
                Line::from(Span::styled(
                    format!(" {} ", self.structure.len()),
                    count_style,
                ))
                .right_aligned(),
            );

        if self.fold_structure {
            crate::ui::rat::render(block, area, surface);
            return;
        }
        if self.structure.is_empty() {
            crate::ui::rat::render(block, area, surface);
            surface.set_stringn(
                area.x + 1,
                area.y + 1,
                "(no symbols)",
                area.width as usize,
                theme.get("comment"),
            );
            return;
        }

        let base = crate::ui::rat::to_rat_style(theme.get("ui.text"));
        let sel_style =
            crate::ui::rat::to_rat_style(theme.get("ui.selection")).add_modifier(RMod::BOLD);

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

        // run/debug + settings/help buttons RIGHT-aligned. ⊙ Locate = JetBrains
        // "Select Opened File" (reveals the current buffer in the tree).
        // U+FE0E (VARIATION SELECTOR-15) forces TEXT presentation so terminals
        // render these symbols in ONE cell — matching unicode-width / the buffer
        // model. Without it, terminals that render them as 2-cell emoji drift the
        // buttons right of their click hit-regions (clicks land on a later button).
        let buttons: [(&str, _, ToolHit); 7] = [
            (" ⊙\u{fe0e} Locate ", theme.get("function"), ToolHit::Locate),
            (" ▶\u{fe0e} Run ", theme.get("diff.plus"), ToolHit::Run),
            (" ◼\u{fe0e} Stop ", theme.get("error"), ToolHit::Stop),
            (" ⟳\u{fe0e} Rerun ", theme.get("function"), ToolHit::Rerun),
            (" 🐞 Debug ", theme.get("keyword"), ToolHit::Debug),
            (
                " ⚙\u{fe0e} Settings ",
                theme.get("comment"),
                ToolHit::Settings,
            ),
            (" ? Help ", theme.get("comment"), ToolHit::Help),
        ];
        let gap = 1u16;
        let total: u16 = buttons.iter().map(|(t, _, _)| disp_width(t)).sum::<u16>()
            + gap * (buttons.len() as u16 - 1);
        let right_edge = area.x + area.width;
        let buttons_start = area.x + area.width.saturating_sub(total + 1);

        // Render the buttons FIRST and clamp their hit-regions to the toolbar's
        // right edge, so they always win over the config selector / breadcrumb
        // when the toolbar is too narrow to fit everything (e.g. after the left
        // drawer is widened). Previously the config selector was pushed first
        // with a range that overran the buttons, so every click opened it.
        let mut x = buttons_start;
        for (text, style, hit) in buttons {
            if x >= right_edge {
                break;
            }
            let avail = right_edge.saturating_sub(x) as usize;
            let (nx, _) = surface.set_stringn(x, area.y, text, avail, style);
            self.toolbar_hits.push((x, nx.min(right_edge), hit));
            x = nx + gap;
        }

        // active run-config selector on the LEFT (click to open the manager),
        // capped to `buttons_start` so its hit-region never overlaps the buttons.
        let cfg = crate::run_config::active()
            .map(|c| if c.name.is_empty() { c.command } else { c.name })
            .or_else(|| self.run.as_ref().map(|r| r.lock().unwrap().cmd.clone()))
            .unwrap_or_else(|| "Edit Configurations…".to_string());
        let label = format!(" ⚙\u{fe0e} {cfg} ▾\u{fe0e} ");
        let cfg_avail = buttons_start.saturating_sub(area.x) as usize;
        let lx = if cfg_avail > 0 {
            let (lx, _) =
                surface.set_stringn(area.x, area.y, &label, cfg_avail, theme.get("function"));
            let lx = lx.min(buttons_start);
            self.toolbar_hits.push((area.x, lx, ToolHit::Configs));
            lx
        } else {
            area.x
        };

        // breadcrumb of the current file in the gap between the selector and buttons
        let bc_start = lx + 2;
        if buttons_start > bc_start + 4 && !self.status_path.is_empty() {
            let avail = (buttons_start - 1 - bc_start) as usize;
            let parts: Vec<&str> = self
                .status_path
                .split('/')
                .filter(|s| !s.is_empty())
                .collect();
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
                let (end_x, _) =
                    surface.set_stringn(bc_start, area.y, &shown, avail, theme.get("comment"));
                self.breadcrumb_hit = (bc_start, end_x);
            }
        }
    }

    /// Bottom tool window: a `Problems | Run` tab header (with run controls) + the active body.
    fn render_bottom(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme) {
        let area = self.problems_rect;
        surface.clear_with(area, theme.get("ui.background"));
        self.bottom_hits.clear();
        self.bottom_header_y = area.y;

        let focused = self.focus == Focus::Problems;
        let on = if focused {
            theme.get("ui.text.focus")
        } else {
            theme.get("ui.text")
        };
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
        let plabel_style = if self.bottom_tab == BottomTab::Problems {
            on
        } else {
            off
        };
        let x0 = x;
        let (mut nx, _) =
            surface.set_stringn(x, area.y, " PROBLEMS ", area.width as usize, plabel_style);
        if self.problems.is_empty() {
            let (e, _) = surface.set_stringn(
                nx,
                area.y,
                "✓ ",
                area.width as usize,
                theme.get("diff.plus"),
            );
            nx = e;
        } else {
            for (count, glyph, scope) in [
                (errs, "⊘", "error"),
                (warns, "⚠", "warning"),
                (infos, "ℹ", "info"),
            ] {
                if count > 0 {
                    let s = format!("{glyph}{count} ");
                    let (e, _) =
                        surface.set_stringn(nx, area.y, &s, area.width as usize, theme.get(scope));
                    nx = e;
                }
            }
        }
        self.bottom_hits.push((x0, nx, BottomHit::TabProblems));
        x = nx + 1;

        // run controls + status (left of the Run tab while a run exists)
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
            let sw = status.chars().count() as u16;
            surface.set_stringn(x, area.y, &status, area.width as usize, off);
            x += sw + 1;
        }

        // Run tab
        let rlabel = " RUN ";
        let rw = rlabel.chars().count() as u16;
        surface.set_stringn(
            x,
            area.y,
            rlabel,
            area.width as usize,
            if self.bottom_tab == BottomTab::Run {
                on
            } else {
                off
            },
        );
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
        let glabel_style = if self.bottom_tab == BottomTab::Git {
            on
        } else {
            off
        };
        let gx0 = x;
        let (mut gx, _) =
            surface.set_stringn(x, area.y, " GIT ", area.width as usize, glabel_style);
        if self.git_changes.is_empty() {
            let (e, _) = surface.set_stringn(
                gx,
                area.y,
                "✓ ",
                area.width as usize,
                theme.get("diff.plus"),
            );
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
                    let (e, _) =
                        surface.set_stringn(gx, area.y, &s, area.width as usize, theme.get(scope));
                    gx = e;
                }
            }
        }
        // ahead/behind the upstream
        if self.git_ahead > 0 {
            let (e, _) = surface.set_stringn(
                gx,
                area.y,
                &format!("↑{} ", self.git_ahead),
                area.width as usize,
                theme.get("diff.plus"),
            );
            gx = e;
        }
        if self.git_behind > 0 {
            let (e, _) = surface.set_stringn(
                gx,
                area.y,
                &format!("↓{} ", self.git_behind),
                area.width as usize,
                theme.get("diff.minus"),
            );
            gx = e;
        }
        self.bottom_hits.push((gx0, gx, BottomHit::TabGit));
        x = gx + 1;

        // Debug tab — ● when a session is live, ⏺N breakpoint count
        let dlabel_style = if self.bottom_tab == BottomTab::Debug {
            on
        } else {
            off
        };
        let dx0 = x;
        let (mut dx, _) =
            surface.set_stringn(x, area.y, " DEBUG ", area.width as usize, dlabel_style);
        if !self.dap_status.starts_with("no ") {
            let (e, _) = surface.set_stringn(
                dx,
                area.y,
                "● ",
                area.width as usize,
                theme.get("diff.plus"),
            );
            dx = e;
        }
        let bpn = self.dap_lines.iter().filter(|(k, _, _)| *k == 3).count();
        if bpn > 0 {
            let (e, _) = surface.set_stringn(
                dx,
                area.y,
                &format!("⏺{bpn} "),
                area.width as usize,
                theme.get("error"),
            );
            dx = e;
        }
        self.bottom_hits.push((dx0, dx, BottomHit::TabDebug));
        x = dx + 1;

        // Registers tab (LOTR)
        let glabel = format!(" REGISTERS {} ", self.registers.len());
        let gw = glabel.chars().count() as u16;
        surface.set_stringn(
            x,
            area.y,
            &glabel,
            area.width as usize,
            if self.bottom_tab == BottomTab::Registers {
                on
            } else {
                off
            },
        );
        self.bottom_hits.push((x, x + gw, BottomHit::TabRegisters));
        x += gw + 1;

        // Todo tab
        let tlabel = format!(" TODO {} ", self.todos.len());
        let tw = tlabel.chars().count() as u16;
        surface.set_stringn(
            x,
            area.y,
            &tlabel,
            area.width as usize,
            if self.bottom_tab == BottomTab::Todo {
                on
            } else {
                off
            },
        );
        self.bottom_hits.push((x, x + tw, BottomHit::TabTodo));
        x += tw + 1;

        // Marks tab
        let mlabel = format!(" MARKS {} ", self.marks_list.len());
        let mw = mlabel.chars().count() as u16;
        surface.set_stringn(
            x,
            area.y,
            &mlabel,
            area.width as usize,
            if self.bottom_tab == BottomTab::Marks {
                on
            } else {
                off
            },
        );
        self.bottom_hits.push((x, x + mw, BottomHit::TabMarks));
        x += mw + 1;

        // Jumplist tab
        let jlabel = format!(" JUMPS {} ", self.jumplist_rows.len());
        let jw = jlabel.chars().count() as u16;
        surface.set_stringn(
            x,
            area.y,
            &jlabel,
            area.width as usize,
            if self.bottom_tab == BottomTab::Jumplist {
                on
            } else {
                off
            },
        );
        self.bottom_hits.push((x, x + jw, BottomHit::TabJumplist));
        x += jw + 1;

        // Recent files tab
        let nlabel = format!(" RECENT {} ", self.recent_rows.len());
        let nw = nlabel.chars().count() as u16;
        surface.set_stringn(
            x,
            area.y,
            &nlabel,
            area.width as usize,
            if self.bottom_tab == BottomTab::Recent {
                on
            } else {
                off
            },
        );
        self.bottom_hits.push((x, x + nw, BottomHit::TabRecent));
        x += nw + 1;

        // Harpoon marks tab
        let hlabel = format!(" ⚓ {} ", self.harpoon_rows.len());
        let hw = hlabel.chars().count() as u16 + 1; // anchor glyph is double-width
        surface.set_stringn(
            x,
            area.y,
            &hlabel,
            area.width as usize,
            if self.bottom_tab == BottomTab::Harpoon {
                on
            } else {
                off
            },
        );
        self.bottom_hits.push((x, x + hw, BottomHit::TabHarpoon));
        x += hw + 2;

        // CI tab (GitHub Actions runs)
        let cilabel = " CI ";
        let ciw = cilabel.chars().count() as u16;
        surface.set_stringn(
            x,
            area.y,
            cilabel,
            area.width as usize,
            if self.bottom_tab == BottomTab::Ci {
                on
            } else {
                off
            },
        );
        self.bottom_hits.push((x, x + ciw, BottomHit::TabCi));

        if self.fold_problems {
            return;
        }
        // Body columns: col0 (Problems/Run/Git), col1 (Registers/Todo/Marks/Jumps),
        // col2 (Recent/Harpoon/CI), separated by two always-draggable dividers
        // whose positions are `bottom_splits` (% of the drawer). A column is
        // "folded" purely when a divider is dragged to swallow it: drag the left
        // divider to the left edge to fold col0, the right divider to the right
        // edge to fold col2, or drag the two together to fold the middle. A
        // folded column has zero width and its divider sits at the edge as a grab
        // handle — drag it back inward to unfold. (Same seam-drag model as the
        // file-tree and minimap.)
        let full = body_rect(area);
        self.bottom_body_y = full.y;
        let end = full.x + full.width;
        let focus_st = theme.get("ui.text.focus");
        self.bottom_div_x = [0, 0];
        self.bottom_col_x = [(0, 0); 3];
        if full.width < 12 {
            // too narrow to split - fall back to the single focused tab
            let t = self.bottom_tab;
            self.render_tab_body(t, surface, theme, full);
            return;
        }
        let tabs = self.bottom_tabs;
        let vline = "\u{2502}";
        // Divider screen columns from the stored %s (s0 <= s1), each kept at least
        // one cell inside the drawer so a fully-folded column still has a grabbable
        // handle rather than falling off the edge.
        let s0 = self.bottom_splits[0].min(100);
        let s1 = self.bottom_splits[1].clamp(s0, 100);
        let last = end.saturating_sub(1);
        let d0 = (full.x + (full.width as u32 * s0 as u32 / 100) as u16).clamp(full.x, last);
        let d1 = (full.x + (full.width as u32 * s1 as u32 / 100) as u16).clamp(d0, last);
        self.bottom_div_x = [d0, d1];

        // Column body rects (a divider cell separates them). Width < 2 = folded.
        let cols = [
            Rect::new(full.x, full.y, d0.saturating_sub(full.x), full.height),
            Rect::new(d0 + 1, full.y, d1.saturating_sub(d0 + 1), full.height),
            Rect::new(
                (d1 + 1).min(end),
                full.y,
                end.saturating_sub(d1 + 1),
                full.height,
            ),
        ];
        for i in 0..3 {
            if cols[i].width >= 2 {
                self.render_tab_body(tabs[i], surface, theme, cols[i]);
                self.bottom_col_x[i] = (cols[i].x, cols[i].x + cols[i].width);
            }
        }
        // Draw the two dividers on top (highlighted while dragging).
        for (dx, active) in [
            (d0, self.resizing_div == Some(0)),
            (d1, self.resizing_div == Some(1)),
        ] {
            let dst = if active { focus_st } else { off };
            for yy in full.y..full.y + full.height {
                surface.set_stringn(dx, yy, vline, 1, dst);
            }
        }
        // A folded column's divider hugs the drawer edge; mark it with an inward
        // chevron so the grab handle is visible and shows which way to drag to
        // unfold. The grab hit-test (mouse down) also widens to ±1 cell.
        let handle = theme.get("function");
        if cols[0].width < 2 {
            surface.set_stringn(d0, full.y, "\u{25B8}", 1, handle); // ▸ drag right to open
        }
        if cols[2].width < 2 {
            surface.set_stringn(d1, full.y, "\u{25C2}", 1, handle); // ◂ drag left to open
        }
    }

    /// CI panel: a ratatui Table of recent GitHub Actions runs (icon · workflow
    /// · branch · sha · age). Data comes from the global `crate::ci` cache,
    /// populated asynchronously by `focus_ci_panel`.
    fn render_ci_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        use ratatui::layout::Constraint;
        use ratatui::style::Modifier as RMod;
        use ratatui::widgets::{Block, Cell, Row, Table};
        if body.height == 0 {
            return;
        }
        let runs = crate::ci::snapshot();
        if runs.is_empty() {
            let (loading, error) = crate::ci::status();
            let msg = if loading {
                "  fetching CI runs…".to_string()
            } else if let Some(e) = error {
                format!("  CI error: {e}")
            } else if crate::ci::fetched() {
                "  no CI runs".to_string()
            } else {
                "  loading…".to_string()
            };
            surface.set_stringn(
                body.x,
                body.y,
                &msg,
                body.width as usize,
                theme.get("comment"),
            );
            return;
        }
        if self.aux_sel >= runs.len() {
            self.aux_sel = runs.len().saturating_sub(1);
        }
        let base = crate::ui::rat::to_rat_style(theme.get("ui.text"));
        let dim = crate::ui::rat::to_rat_style(theme.get("comment"));
        let sel = crate::ui::rat::to_rat_style(theme.get("ui.selection")).add_modifier(RMod::BOLD);
        let rows: Vec<Row> = runs
            .iter()
            .map(|r| {
                let (glyph, scope) = r.icon();
                Row::new(vec![
                    Cell::from(glyph).style(crate::ui::rat::to_rat_style(theme.get(scope))),
                    Cell::from(r.workflow.clone()).style(base),
                    Cell::from(r.branch.clone()).style(dim),
                    Cell::from(r.short_sha()).style(dim),
                    Cell::from(r.age()).style(dim),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Min(12),
                Constraint::Length(16),
                Constraint::Length(8),
                Constraint::Length(9),
            ],
        )
        .column_spacing(1)
        .block(Block::default().style(crate::ui::rat::to_rat_style(theme.get("ui.background"))))
        .row_highlight_style(sel)
        .highlight_symbol("› ");
        self.ci_state.select(Some(self.aux_sel));
        crate::ui::rat::render_stateful(table, body, surface, &mut self.ci_state);
    }

    fn render_jumplist_body(
        &mut self,
        surface: &mut Surface,
        theme: &zemacs_view::Theme,
        body: Rect,
    ) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.jumplist_rows.is_empty() {
            surface.set_stringn(
                body.x,
                body.y,
                "  no jumps",
                body.width as usize,
                theme.get("comment"),
            );
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
                surface.set_style(
                    Rect::new(body.x, y, body.width, 1),
                    theme.get("ui.selection"),
                );
            }
            surface.set_stringn(body.x, y, " ↪", body.width as usize, mark);
            surface.set_stringn(
                body.x + 3,
                y,
                label,
                body.width.saturating_sub(3) as usize,
                base,
            );
        }
    }

    fn render_recent_body(
        &mut self,
        surface: &mut Surface,
        theme: &zemacs_view::Theme,
        body: Rect,
    ) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.recent_rows.is_empty() {
            surface.set_stringn(
                body.x,
                body.y,
                "  no recent files",
                body.width as usize,
                theme.get("comment"),
            );
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
                surface.set_style(
                    Rect::new(body.x, y, body.width, 1),
                    theme.get("ui.selection"),
                );
            }
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let glyph = crate::ui::icons::file_icon(&name);
            let label = format!(" {glyph} {name}");
            let (nx, _) = surface.set_stringn(body.x, y, &label, body.width as usize, base);
            // trailing dimmed relative access time + parent directory
            let age = match self.recent_times.get(i) {
                Some(&t) if t > 0 => format!(
                    "· {} ",
                    crate::recent_files::humanize_age(crate::recent_files::age_since(t))
                ),
                _ => String::new(),
            };
            if let Some(parent) = path.parent().map(|p| p.to_string_lossy().into_owned()) {
                let rem = body.width.saturating_sub(nx - body.x) as usize;
                surface.set_stringn(nx + 1, y, &format!("{age}· {parent}"), rem, dim);
            }
        }
    }

    fn render_harpoon_body(
        &mut self,
        surface: &mut Surface,
        theme: &zemacs_view::Theme,
        body: Rect,
    ) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.harpoon_rows.is_empty() {
            surface.set_stringn(
                body.x,
                body.y,
                "  no marks — pin with SPC H a",
                body.width as usize,
                theme.get("comment"),
            );
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
                surface.set_style(
                    Rect::new(body.x, y, body.width, 1),
                    theme.get("ui.selection"),
                );
            }
            // slot number (1-based) then the file name
            surface.set_stringn(body.x + 1, y, &format!("{}", i + 1), 2, slot_style);
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let glyph = crate::ui::icons::file_icon(&name);
            surface.set_stringn(
                body.x + 3,
                y,
                &format!("{glyph} {name}"),
                body.width.saturating_sub(3) as usize,
                base,
            );
        }
    }

    fn render_todo_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.todos.is_empty() {
            surface.set_stringn(
                body.x,
                body.y,
                "  no TODOs",
                body.width as usize,
                theme.get("comment"),
            );
            return;
        }
        let base = theme.get("ui.text");
        let focused = self.focus == Focus::Problems && self.bottom_tab == BottomTab::Todo;
        for (i, (_, text, marker)) in self.todos.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
            if focused && i == self.aux_sel {
                surface.set_style(
                    Rect::new(body.x, y, body.width, 1),
                    theme.get("ui.selection"),
                );
            }
            let mark = theme.get(todo_marker_scope(marker));
            surface.set_stringn(body.x, y, " •", body.width as usize, mark);
            surface.set_stringn(
                body.x + 3,
                y,
                text,
                body.width.saturating_sub(3) as usize,
                base,
            );
        }
    }

    fn render_registers_body(
        &mut self,
        surface: &mut Surface,
        theme: &zemacs_view::Theme,
        body: Rect,
    ) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.registers.is_empty() {
            surface.set_stringn(
                body.x,
                body.y,
                "  no registers",
                body.width as usize,
                theme.get("comment"),
            );
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
                surface.set_style(
                    Rect::new(body.x, y, body.width, 1),
                    theme.get("ui.selection"),
                );
            }
            let label = format!(" \"{ch}  ");
            let (nx, _) = surface.set_stringn(body.x, y, &label, body.width as usize, name_style);
            surface.set_stringn(
                nx,
                y,
                content,
                body.width.saturating_sub(nx - body.x) as usize,
                base,
            );
        }
    }

    fn render_problems_body(
        &mut self,
        surface: &mut Surface,
        theme: &zemacs_view::Theme,
        body: Rect,
    ) {
        use ratatui::layout::Constraint;
        use ratatui::style::Modifier as RMod;
        use ratatui::widgets::{Block, Cell, Row, Table};
        if body.height == 0 {
            return;
        }
        if self.problems.is_empty() {
            surface.set_stringn(
                body.x,
                body.y,
                "  no problems",
                body.width as usize,
                theme.get("comment"),
            );
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
            [
                Constraint::Length(1),
                Constraint::Length(6),
                Constraint::Min(8),
            ],
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

    /// Debug tool window body: a status line, then Call Stack / Variables /
    /// Breakpoints sections (built in `refresh`). Click a frame or breakpoint row
    /// to jump to its source. Stepping uses the `dap_*` keybindings.
    fn render_debug_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        if body.height == 0 {
            return;
        }
        let base = theme.get("ui.text");
        let dim = theme.get("comment");
        let accent = theme.get("function");
        let kw = theme.get("keyword");

        // status line
        let status_style = if self.dap_status.starts_with("no ") {
            dim
        } else {
            accent
        };
        surface.set_stringn(
            body.x + 1,
            body.y,
            &self.dap_status,
            body.width.saturating_sub(1) as usize,
            status_style,
        );
        if body.height < 2 {
            return;
        }
        let list = Rect::new(body.x, body.y + 1, body.width, body.height - 1);
        for (i, (kind, text, _)) in self.dap_lines.iter().enumerate() {
            if i >= list.height as usize {
                break;
            }
            let y = list.y + i as u16;
            let (style, indent) = match kind {
                0 => (dim, 0u16), // section header
                3 => (kw, 1),     // breakpoint
                _ => (base, 1),   // frame / variable
            };
            surface.set_stringn(
                list.x + indent,
                y,
                text,
                list.width.saturating_sub(indent) as usize,
                style,
            );
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
            surface.set_stringn(
                body.x,
                body.y,
                msg,
                body.width as usize,
                theme.get("comment"),
            );
            return;
        }
        if self.git_sel >= self.git_changes.len() {
            self.git_sel = self.git_changes.len().saturating_sub(1);
        }
        let base = theme.get("ui.text");
        let dim = theme.get("comment");
        let sel_bg = theme.get("ui.selection");
        let plus = theme.get("diff.plus");
        let minus = theme.get("diff.minus");
        let focused = self.focus == Focus::Problems && self.bottom_tab == BottomTab::Git;

        // Top row: per-commit churn sparkline (ratatui Sparkline), when we have
        // history and at least a couple of rows of body to spare.
        let mut list = body;
        self.git_list_offset = 0;
        if self.git_churn.iter().any(|&c| c > 0) && body.height >= 3 && body.width > 12 {
            self.git_list_offset = 1;
            use crate::ui::rat::{render, to_rat_style};
            use ratatui::widgets::Sparkline;
            surface.set_stringn(body.x + 1, body.y, "churn", 5, dim);
            let spark = Sparkline::default()
                .data(&self.git_churn)
                .style(to_rat_style(theme.get("function")));
            render(
                spark,
                Rect::new(body.x + 7, body.y, body.width.saturating_sub(8), 1),
                surface,
            );
            list = Rect::new(body.x, body.y + 1, body.width, body.height - 1);
        }

        // Largest single-file churn, to scale the per-row diffstat bars.
        let max_churn = self
            .git_diffstat
            .iter()
            .map(|(_, a, d)| a + d)
            .max()
            .unwrap_or(1)
            .max(1);
        let list_h = list.height as usize;
        // Reserve a right-hand strip for the "+A −D" counts and the bar.
        let bar_w = 10u16.min(list.width / 5);
        let stat_w = 12u16;
        let name_w = list.width.saturating_sub(4 + stat_w + bar_w + 1) as usize;

        // Build the visual layout: merge conflicts first under a "Conflicts"
        // header, then the normal changes under a "Changes" header (headers only
        // appear when there are conflicts). `None` rows are section headers;
        // `Some(i)` rows map back to `git_changes[i]` for click/keyboard dispatch.
        let nconf = self.git_conflict_count.min(self.git_changes.len());
        let conflict_style = theme
            .try_get("diff.delta.conflict")
            .unwrap_or_else(|| theme.get("warning"));
        let mut layout: Vec<Option<usize>> = Vec::with_capacity(self.git_changes.len() + 2);
        if nconf > 0 {
            layout.push(None); // "Conflicts" header
            layout.extend((0..nconf).map(Some));
            layout.push(None); // "Changes" header
        }
        layout.extend((nconf..self.git_changes.len()).map(Some));
        self.git_row_layout = layout;

        for (k, entry) in self.git_row_layout.iter().enumerate() {
            if k >= list_h {
                break;
            }
            let y = list.y + k as u16;
            let i = match *entry {
                Some(i) => i,
                None => {
                    // Section header — the first `None` is always "Conflicts".
                    let (label, style) = if k == 0 {
                        ("Conflicts", conflict_style)
                    } else {
                        ("Changes", dim)
                    };
                    surface.set_stringn(
                        list.x + 1,
                        y,
                        label,
                        list.width.saturating_sub(1) as usize,
                        style,
                    );
                    continue;
                }
            };
            let (code, disp, _) = &self.git_changes[i];
            let is_conflict = i < nconf;
            // highlight the keyboard-selected row when the panel holds focus
            if focused && i == self.git_sel {
                surface.set_style(Rect::new(list.x, y, list.width, 1), sel_bg);
            }
            // colour by status: conflicts use the conflict style; otherwise
            // added=green, modified=yellow, deleted=red, untracked=dim.
            let st = if is_conflict {
                conflict_style
            } else {
                match code.trim() {
                    "A" | "AM" => plus,
                    "D" => minus,
                    "??" => dim,
                    _ => theme.get("diff.delta"),
                }
            };
            surface.set_stringn(list.x + 1, y, &code.replace(' ', "·"), 3, st);
            // repo-relative path (after the "XY  " porcelain prefix)
            let rel = disp.split_once("  ").map(|x| x.1).unwrap_or("").trim();
            let name_style = if is_conflict { conflict_style } else { base };
            surface.set_stringn(list.x + 4, y, rel, name_w.max(1), name_style);

            // diffstat counts + proportional add/del bar, when numstat has the file
            if let Some((a, d)) = self
                .git_diffstat
                .iter()
                .find(|(p, _, _)| p == rel)
                .map(|(_, a, d)| (*a, *d))
            {
                let sx = list.x + 4 + name_w as u16 + 1;
                if bar_w > 0 && sx + stat_w + bar_w <= list.x + list.width {
                    surface.set_stringn(sx, y, &format!("+{a}"), 6, plus);
                    surface.set_stringn(sx + 6, y, &format!("-{d}"), 6, minus);
                    let total = a + d;
                    let filled = ((total as u64 * bar_w as u64) / max_churn as u64) as u16;
                    let adds_px = if total == 0 {
                        0
                    } else {
                        ((a as u64 * filled as u64) / total as u64) as u16
                    };
                    let bx = sx + stat_w;
                    for k in 0..filled {
                        let style = if k < adds_px { plus } else { minus };
                        surface.set_stringn(bx + k, y, "█", 1, style);
                    }
                }
            }
        }
    }

    fn render_marks_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.marks_list.is_empty() {
            surface.set_stringn(
                body.x,
                body.y,
                "  no marks — set with m{a-z}",
                body.width as usize,
                theme.get("comment"),
            );
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
                surface.set_style(
                    Rect::new(body.x, y, body.width, 1),
                    theme.get("ui.selection"),
                );
            }
            // mark sigil ('x) gets the accent colour, the rest is plain
            let head: String = disp.chars().take(2).collect();
            surface.set_stringn(body.x + 1, y, &head, 2, accent);
            let rest: String = disp.chars().skip(2).collect();
            surface.set_stringn(
                body.x + 3,
                y,
                &rest,
                body.width.saturating_sub(3) as usize,
                base,
            );
        }
    }

    fn render_run_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        let Some(run) = self.run.clone() else {
            surface.set_stringn(
                body.x,
                body.y,
                "  no run — :run [cmd]",
                body.width as usize,
                theme.get("comment"),
            );
            return;
        };
        let s = run.lock().unwrap();
        let full_h = body.height as usize;
        let w = body.width.max(1) as usize;
        if full_h == 0 {
            return;
        }
        let base = theme.get("ui.text");

        // Header row: output-rate sparkline + test-pass gauge (when there's room).
        let header_rows: usize = if full_h >= 4 { 1 } else { 0 };
        if header_rows == 1 {
            self.render_run_header(surface, theme, Rect::new(body.x, body.y, body.width, 1), &s);
        }
        // Output viewport sits below the header.
        let out = Rect::new(
            body.x,
            body.y + header_rows as u16,
            body.width,
            (full_h - header_rows) as u16,
        );
        let height = out.height as usize;

        // Soft-wrap: every output line occupies ceil(width/w) visual rows (≥1).
        // Line widths are precomputed on the streaming task (`RunState::line_widths`),
        // so this is a cheap integer sum rather than a per-frame Unicode scan of the
        // whole console.
        let vis_of = |dw: usize| -> usize {
            if dw == 0 {
                1
            } else {
                dw.div_ceil(w)
            }
        };
        let total_vis: usize = s
            .line_widths
            .iter()
            .map(|&dw| vis_of(dw as usize))
            .sum::<usize>()
            .max(1);
        self.run_total_vis = total_vis;
        let max_top = total_vis.saturating_sub(height);
        // tail-follow unless the user scrolled up
        let top = if s.follow {
            max_top
        } else {
            s.scroll.min(max_top)
        };

        // (re)build the visible-row → source-line map for click-to-jump. Indexed
        // by *body* row (0-based from body.y) so the header offset lines up with
        // the click handler's `row - problems_rect.y - 1`.
        self.run_row_src = vec![usize::MAX; full_h];
        let mut vis = 0usize;
        'lines: for (li, line) in s.lines.iter().enumerate() {
            for chunk in wrap_chunks(line, w) {
                if vis >= top + height {
                    break 'lines;
                }
                if vis >= top {
                    let sr = vis - top;
                    surface.set_stringn(out.x, out.y + sr as u16, chunk, w, base);
                    self.run_row_src[header_rows + sr] = li;
                }
                vis += 1;
            }
            // empty line still consumes one visual row
            if line.is_empty() {
                vis += 1;
            }
        }

        // scrollbar thumb on the right edge when content overflows
        if total_vis > height && out.width > 1 {
            let track_x = out.x + out.width - 1;
            let thumb_h = (height * height / total_vis).max(1);
            let thumb_y = (top * (height - thumb_h)).checked_div(max_top).unwrap_or(0);
            let bar = theme.get("ui.selection");
            for k in 0..thumb_h {
                surface.set_stringn(track_x, out.y + (thumb_y + k) as u16, "▐", 1, bar);
            }
        }
    }

    /// Run-console header: a live output-rate `Sparkline` on the left and a
    /// test-pass `Gauge` on the right (when the output contains cargo-style
    /// `... ok` / `... FAILED` test lines).
    fn render_run_header(
        &self,
        surface: &mut Surface,
        theme: &zemacs_view::Theme,
        area: Rect,
        s: &crate::ui::run::RunState,
    ) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::text::Span;
        use ratatui::widgets::{Gauge, Sparkline};

        let dim = theme.get("comment");
        surface.clear_with(area, theme.get("ui.background"));

        // left: output-rate sparkline (lines/tick)
        let lbl_x = area.x + 1;
        surface.set_stringn(lbl_x, area.y, "out", 3, dim);
        let spark_x = lbl_x + 4;
        let spark_w = (area.width / 2).saturating_sub(6);
        if spark_w > 0 && self.run_rate.iter().any(|&r| r > 0) {
            let data: Vec<u64> = self.run_rate.iter().copied().collect();
            let spark = Sparkline::default()
                .data(&data)
                .style(to_rat_style(theme.get("function")));
            render(spark, Rect::new(spark_x, area.y, spark_w, 1), surface);
        }

        // right: test-pass gauge when test result lines are present
        if let Some((passed, total)) = parse_test_progress(&s.lines) {
            let gw = 20u16.min(area.width / 3);
            if gw >= 8 && area.width > gw + 8 {
                let gx = area.x + area.width - gw - 1;
                let ratio = if total > 0 {
                    passed as f64 / total as f64
                } else {
                    0.0
                };
                let ok = passed == total;
                let bar_scope = if ok { "diff.plus" } else { "diff.delta" };
                let gauge = Gauge::default()
                    .ratio(ratio.clamp(0.0, 1.0))
                    .label(Span::styled(
                        format!("{passed}/{total} tests"),
                        to_rat_style(theme.get("ui.text")),
                    ))
                    .gauge_style(to_rat_style(theme.get(bar_scope)))
                    .use_unicode(true);
                render(gauge, Rect::new(gx, area.y, gw, 1), surface);
            }
        }
    }

    /// Right-pane minimap: a braille `Canvas` "tiny text" overview of the code
    /// shape, with a viewport outline, diagnostic ticks (right edge), and
    /// git-change bars (left edge). Each braille cell packs a 2×4 sub-grid, so
    /// the canvas resolution is `width*2 × height*4` columns/sub-rows.
    fn render_stripe(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme) {
        use crate::ui::rat::{render, to_rat_color};
        use ratatui::symbols::Marker;
        use ratatui::widgets::canvas::{Canvas, Points, Rectangle};
        use zemacs_view::graphics::Color as ZColor;

        let area = self.stripe_rect;
        let w = area.width as usize;
        let h = area.height as usize;
        if w == 0 || h == 0 {
            return;
        }
        let bg_style = theme.get("ui.background");
        surface.clear_with(area, bg_style);

        let total = self.total_lines.max(1);
        let slots = h * 4; // vertical sub-rows
        let cols = (w * 2) as f64; // horizontal sub-cols
        let slot_of = |line: usize| -> usize {
            if total <= slots {
                line.min(slots.saturating_sub(1))
            } else {
                line * slots / total
            }
        };
        // Canvas y grows upward; source lines grow downward → flip.
        let flip = |slot: usize| -> f64 { slots.saturating_sub(1).saturating_sub(slot) as f64 };

        let dot_color = to_rat_color(theme.get("comment").fg.unwrap_or(ZColor::Gray));
        let vp_color = to_rat_color(
            theme
                .get("ui.selection")
                .bg
                .or(theme.get("ui.selection").fg)
                .unwrap_or(ZColor::Blue),
        );

        // code-shape points: sample one source line per vertical sub-row (bounded
        // by the pane, not the file size).
        let mut code_pts: Vec<(f64, f64)> = Vec::new();
        for sub in 0..slots {
            let srcline = if total <= slots {
                sub
            } else {
                sub * total / slots
            };
            let Some(dots) = self.minimap_dots.get(srcline) else {
                continue;
            };
            let cy = flip(sub);
            for (c, on) in dots.iter().enumerate() {
                if *on && (c as f64) < cols {
                    code_pts.push((c as f64, cy));
                }
            }
        }

        // diagnostic ticks on the right edge, coloured by severity.
        let diag_pts: Vec<(f64, f64, ratatui::style::Color)> = self
            .problems
            .iter()
            .map(|p| {
                let color = to_rat_color(sev_mark(p.sev, theme).1.fg.unwrap_or(ZColor::Red));
                (cols - 1.0, flip(slot_of(p.line)), color)
            })
            .collect();

        // git change bars on the left edge, coloured by kind.
        let mut git_pts: Vec<(f64, f64, ratatui::style::Color)> = Vec::new();
        for &(start, end, kind) in &self.git_hunks {
            let scope = match kind {
                0 => "diff.plus",
                2 => "diff.minus",
                _ => "diff.delta",
            };
            let color = to_rat_color(theme.get(scope).fg.unwrap_or(ZColor::Yellow));
            let s0 = slot_of(start as usize);
            let s1 = slot_of((end as usize).saturating_sub(1).max(start as usize));
            for s in s0..=s1.max(s0) {
                git_pts.push((0.0, flip(s), color));
            }
        }

        // viewport outline in (flipped) slot space.
        let vp_top = slot_of(self.view_top_line);
        let vp_bot = slot_of(self.view_top_line + self.view_lines);
        let vp_y = flip(vp_bot);
        let vp_h = vp_bot.saturating_sub(vp_top) as f64;

        let bg_color = bg_style.bg.map(to_rat_color);

        let mut canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([0.0, cols])
            .y_bounds([0.0, slots as f64])
            .paint(move |ctx| {
                ctx.draw(&Points {
                    coords: &code_pts,
                    color: dot_color,
                });
                ctx.draw(&Rectangle {
                    x: 0.0,
                    y: vp_y,
                    width: cols - 1.0,
                    height: vp_h,
                    color: vp_color,
                });
                for (x, y, color) in &diag_pts {
                    ctx.draw(&Points {
                        coords: &[(*x, *y)],
                        color: *color,
                    });
                }
                for (x, y, color) in &git_pts {
                    ctx.draw(&Points {
                        coords: &[(*x, *y)],
                        color: *color,
                    });
                }
            });
        if let Some(bgc) = bg_color {
            canvas = canvas.background_color(bgc);
        }
        render(canvas, area, surface);
    }

    /// Floating LSP/build progress card, anchored to the bottom-right of the
    /// editor `area`. Rendered *after* the document (so it overlays it) by
    /// `EditorView`. Shows an LSP indexing `Gauge` (determinate when the server
    /// reports a percentage) and a build `LineGauge` driven by a parsed `NN%`.
    pub fn render_progress_overlay(
        &self,
        area: Rect,
        surface: &mut Surface,
        theme: &zemacs_view::Theme,
    ) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::symbols;
        use ratatui::text::Span;
        use ratatui::widgets::{Block, Borders, Gauge, LineGauge, Paragraph};

        if !self.visible {
            return;
        }
        let has_lsp = self.lsp_progress.is_some();
        let has_build = self.build_progress.is_some();
        if !has_lsp && !has_build {
            return;
        }

        let inner_h = (has_lsp as u16) * 2 + (has_build as u16) * 2;
        let box_h = inner_h + 2;
        let box_w = 46u16.min(area.width.saturating_sub(4)).max(24);
        // Leave a margin and keep clear of the command/status line (bottom 2 rows).
        if area.width < box_w + 2 || area.height < box_h + 3 {
            return;
        }
        let bx = area.x + area.width - box_w - 1;
        let by = area.y + area.height - box_h - 2;
        let outer = Rect::new(bx, by, box_w, box_h);

        let bg = theme.get("ui.popup");
        let bg_rat = to_rat_style(bg);
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let bar = to_rat_style(theme.get("ui.selection"));

        surface.clear_with(outer, bg);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(dim)
            .title(Span::styled(" Progress ", accent))
            .style(bg_rat);
        render(block, outer, surface);

        let inner = Rect::new(outer.x + 1, outer.y + 1, outer.width - 2, outer.height - 2);
        let mut y = inner.y;

        if let Some(p) = &self.lsp_progress {
            let label = match &p.message {
                Some(msg) if !msg.is_empty() => format!("{} · {} — {}", p.server, p.title, msg),
                _ => format!("{} · {}", p.server, p.title),
            };
            render(
                Paragraph::new(Span::styled(label, text)).style(bg_rat),
                Rect::new(inner.x, y, inner.width, 1),
                surface,
            );
            y += 1;
            let ratio = p.percentage.unwrap_or(0).min(100) as f64 / 100.0;
            let lbl = p
                .percentage
                .map(|n| format!("{n}%"))
                .unwrap_or_else(|| "working…".to_string());
            let gauge = Gauge::default()
                .ratio(ratio)
                .label(Span::styled(lbl, text))
                .gauge_style(bar)
                .style(bg_rat)
                .use_unicode(true);
            render(gauge, Rect::new(inner.x, y, inner.width, 1), surface);
            y += 1;
        }

        if let Some((frac, label)) = &self.build_progress {
            render(
                Paragraph::new(Span::styled(label.clone(), dim)).style(bg_rat),
                Rect::new(inner.x, y, inner.width, 1),
                surface,
            );
            y += 1;
            let lg = LineGauge::default()
                .ratio(frac.clamp(0.0, 1.0))
                .filled_style(bar)
                .unfilled_style(dim)
                .label(Span::styled(format!("{:.0}%", frac * 100.0), text))
                .line_set(symbols::line::THICK);
            render(lg, Rect::new(inner.x, y, inner.width, 1), surface);
        }
    }
}

/// The area below a drawer's 1-row header.
fn body_rect(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    )
}

/// Draw a drawer header with a fold chevron (▾ open / ▸ folded).
fn draw_header(
    surface: &mut Surface,
    area: Rect,
    title: &str,
    folded: bool,
    focused: bool,
    theme: &zemacs_view::Theme,
) {
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

fn sev_mark(
    sev: Severity,
    theme: &zemacs_view::Theme,
) -> (&'static str, zemacs_view::graphics::Style) {
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
}

/// Rebuild the file tree on the main thread (from a background callback context).
fn refresh_tree_async() {
    crate::job::dispatch_blocking(|_editor, compositor| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.refresh_file_tree();
        }
    });
}

/// File clipboard for the context-menu Cut/Copy/Paste: (source path, is_cut).
static FILE_CLIP: std::sync::Mutex<Option<(PathBuf, bool)>> = std::sync::Mutex::new(None);

/// Spawn a detached command (Open In Finder/Terminal/GitHub, gist), ignoring output.
fn spawn_detached(program: &str, args: &[&str], cwd: Option<&std::path::Path>) {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let _ = cmd.spawn();
}

/// Build the right-click context menu for a file-tree entry at (row, col).
pub fn file_context_menu(
    path: PathBuf,
    is_dir: bool,
    row: u16,
    col: u16,
) -> crate::ui::context_menu::ContextMenu {
    use crate::ui::context_menu::Entry;
    let mut entries = file_menu_entries(path, is_dir);
    entries.push(Entry::sep());
    entries.push(Entry::item("Fold Panel", |compositor, _cx| {
        if let Some(view) = compositor.find::<crate::ui::EditorView>() {
            view.ide_toggle_fold("project");
        }
    }));
    crate::ui::context_menu::ContextMenu::new(row, col, entries)
}

/// The JetBrains project-view context menu, as a tree of `Entry`s. Shared by the
/// file-tree right-click and the editor-pane right-click. Each action maps to a
/// real zemacs operation; unsupported IDE-only items are omitted.
pub(crate) fn file_menu_entries(
    path: PathBuf,
    is_dir: bool,
) -> Vec<crate::ui::context_menu::Entry> {
    use crate::ui::context_menu::Entry;
    use zemacs_view::editor::Action;

    // Directory that New/Paste act in: the entry itself if a dir, else its parent.
    let dir = if is_dir {
        path.clone()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut e = Vec::new();

    // New ›
    {
        let d1 = dir.clone();
        let d2 = dir.clone();
        e.push(Entry::sub(
            "New",
            vec![
                Entry::item("File", move |compositor, _cx| {
                    compositor.push(Box::new(name_prompt(
                        FileActionKind::NewFile,
                        d1.clone(),
                        true,
                    )));
                }),
                Entry::item("Directory", move |compositor, _cx| {
                    compositor.push(Box::new(name_prompt(
                        FileActionKind::NewFolder,
                        d2.clone(),
                        true,
                    )));
                }),
            ],
        ));
    }
    e.push(Entry::sep());

    // Cut / Copy / Copy Path/Reference › / Paste
    {
        let p = path.clone();
        e.push(Entry::item("Cut", move |_c, cx| {
            *FILE_CLIP.lock().unwrap() = Some((p.clone(), true));
            cx.editor.set_status(format!("cut {}", p.display()));
        }));
        let p = path.clone();
        e.push(Entry::item("Copy", move |_c, cx| {
            *FILE_CLIP.lock().unwrap() = Some((p.clone(), false));
            cx.editor.set_status(format!("copied {}", p.display()));
        }));
        let abs = path.clone();
        let nm = name.clone();
        let root = zemacs_loader::find_workspace().0;
        let rel = path
            .strip_prefix(&root)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.clone());
        e.push(Entry::sub(
            "Copy Path/Reference…",
            vec![
                Entry::item("Absolute Path", move |_c, cx| {
                    let s = abs.to_string_lossy().to_string();
                    let _ = cx.editor.registers.push('"', s.clone());
                    cx.editor.set_status(format!("yanked {s}"));
                }),
                Entry::item("Path From Repository Root", move |_c, cx| {
                    let s = rel.to_string_lossy().to_string();
                    let _ = cx.editor.registers.push('"', s.clone());
                    cx.editor.set_status(format!("yanked {s}"));
                }),
                Entry::item("File Name", move |_c, cx| {
                    let _ = cx.editor.registers.push('"', nm.clone());
                    cx.editor.set_status(format!("yanked {nm}"));
                }),
            ],
        ));
        let dst = dir.clone();
        e.push(Entry::item("Paste", move |_c, cx| {
            let clip = FILE_CLIP.lock().unwrap().clone();
            let Some((src, is_cut)) = clip else {
                cx.editor.set_error("nothing to paste");
                return;
            };
            let Some(fname) = src.file_name() else { return };
            let target = dst.join(fname);
            let prog = if is_cut { "mv" } else { "cp" };
            let args: Vec<&str> = if is_cut {
                vec![src.to_str().unwrap_or(""), target.to_str().unwrap_or("")]
            } else {
                vec![
                    "-R",
                    src.to_str().unwrap_or(""),
                    target.to_str().unwrap_or(""),
                ]
            };
            match std::process::Command::new(prog).args(&args).status() {
                Ok(s) if s.success() => {
                    cx.editor
                        .set_status(format!("pasted → {}", target.display()));
                    if is_cut {
                        *FILE_CLIP.lock().unwrap() = None;
                    }
                    refresh_tree_async();
                }
                Ok(_) | Err(_) => cx.editor.set_error("paste failed"),
            }
        }));
    }
    e.push(Entry::sep());

    // Refactor: Rename / Delete
    {
        let p = path.clone();
        e.push(Entry::item("Rename…", move |compositor, _cx| {
            compositor.push(Box::new(name_prompt(
                FileActionKind::Rename,
                p.clone(),
                is_dir,
            )));
        }));
        let p = path.clone();
        e.push(Entry::item_key("Delete…", "⌫", move |_c, cx| {
            let res = if is_dir {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
            match res {
                Ok(()) => cx.editor.set_status(format!("deleted {}", p.display())),
                Err(err) => cx.editor.set_error(format!("delete failed: {err}")),
            }
            refresh_tree_async();
        }));
    }
    e.push(Entry::sep());

    if !is_dir {
        // Run — materialize + activate a JetBrains run configuration, then run it.
        let p = path.clone();
        e.push(Entry::item("Run", move |compositor, cx| {
            if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                view.run_path(cx.editor, &p);
            }
        }));
        // Open in Right Split
        let p = path.clone();
        e.push(Entry::item_key(
            "Open in Right Split",
            "⇧↵",
            move |_c, cx| {
                if let Err(zemacs_view::DocumentOpenError::BinaryFile) =
                    cx.editor.open(&p, Action::VerticalSplit)
                {
                    cx.editor.set_error("binary file — use :hex");
                }
            },
        ));
    }

    // Open In ›
    {
        let p = path.clone();
        let d = dir.clone();
        let root = zemacs_loader::find_workspace().0;
        let rel = path
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().into_owned())
            .ok();
        e.push(Entry::sub(
            "Open In",
            vec![
                Entry::item("Finder", move |_c, _cx| {
                    spawn_detached("open", &["-R", p.to_str().unwrap_or("")], None);
                }),
                Entry::item("Terminal", move |_c, _cx| {
                    spawn_detached("open", &["-a", "Terminal", d.to_str().unwrap_or("")], None);
                }),
                Entry::item("GitHub", move |_c, cx| match &rel {
                    Some(r) => spawn_detached("gh", &["browse", "--", r], None),
                    None => cx.editor.set_error("not in a repo"),
                }),
            ],
        ));
    }
    e.push(Entry::sep());

    if !is_dir {
        // Reload from Disk — re-read the file into its buffer.
        let p = path.clone();
        e.push(Entry::item("Reload from Disk", move |_c, cx| {
            let _ = cx.editor.open(&p, Action::Replace);
            cx.editor.set_status("reloaded from disk");
        }));
        // Compare With HEAD — git diff into the Run console.
        let p = path.clone();
        e.push(Entry::item_key(
            "Compare With HEAD",
            "SPC g =",
            move |compositor, cx| {
                if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                    let cwd = p
                        .parent()
                        .map(|d| d.to_path_buf())
                        .unwrap_or_else(|| PathBuf::from("."));
                    let quoted = p.to_string_lossy().replace('\'', "'\\''");
                    view.start_run(cx, format!("git --no-pager diff -- '{quoted}'"), cwd);
                }
            },
        ));
        // Create Gist
        let p = path.clone();
        e.push(Entry::item("Create Gist…", move |_c, _cx| {
            spawn_detached(
                "gh",
                &["gist", "create", "--web", p.to_str().unwrap_or("")],
                None,
            );
        }));
    }

    e
}

/// Prompt for a name, then create/rename the target and refresh the tree.
fn name_prompt(kind: FileActionKind, target: PathBuf, _is_dir: bool) -> crate::ui::Prompt {
    use crate::ui::PromptEvent;
    let label: std::borrow::Cow<'static, str> = match kind {
        FileActionKind::NewFile => "New file: ".into(),
        FileActionKind::NewFolder => "New folder: ".into(),
        FileActionKind::Rename => "Rename to: ".into(),
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
        let col = parts
            .get(2)
            .and_then(|c| c.parse::<usize>().ok())
            .unwrap_or(1);
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

/// Commits (ahead, behind) the upstream via one `git rev-list` call. (0, 0) when
/// there's no upstream configured.
fn git_ahead_behind(dir: &std::path::Path) -> (usize, usize) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-list", "--left-right", "--count", "@{u}...HEAD"])
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            let mut it = s.split_whitespace();
            let behind = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let ahead = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            return (ahead, behind);
        }
    }
    (0, 0)
}

/// The TODO-style markers scanned for in the Todo tab, ordered by precedence.
const TODO_MARKERS: [&str; 6] = ["FIXME", "BUG", "XXX", "HACK", "TODO", "NOTE"];

/// Find a TODO-style marker in `line`, requiring a word boundary on both sides so
/// identifiers like `update_todos`, `DENOTE`, or `AUTODOC` don't produce false hits.
/// Returns the canonical marker name (the longest/highest-precedence match).
fn todo_marker(line: &str) -> Option<&'static str> {
    let bytes = line.as_bytes();
    let boundary = |c: u8| !(c.is_ascii_alphanumeric() || c == b'_');
    // Scan positions left-to-right; return the marker matching at the earliest spot.
    for i in 0..bytes.len() {
        if i > 0 && !boundary(bytes[i - 1]) {
            continue;
        }
        for marker in TODO_MARKERS {
            let mb = marker.as_bytes();
            let end = i + mb.len();
            if end <= bytes.len()
                && &bytes[i..end] == mb
                && (end >= bytes.len() || boundary(bytes[end]))
            {
                return Some(marker);
            }
        }
    }
    None
}

/// Theme scope for a marker's severity coloring.
fn todo_marker_scope(marker: &str) -> &'static str {
    match marker {
        "FIXME" | "BUG" | "XXX" => "error",
        "TODO" | "HACK" => "warning",
        _ => "comment",
    }
}

/// True when a porcelain `XY` status code denotes an unmerged (merge-conflict)
/// entry: one side `U`, or both-added/both-deleted. See `git status --porcelain`.
fn git_is_conflict(code: &str) -> bool {
    matches!(
        code.trim_end(),
        "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU"
    )
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

/// Per-file diffstat of the working tree vs HEAD: `(repo-relative path,
/// additions, deletions)`. Binary files (numstat `-`) report `(0, 0)`.
fn git_diffstat(dir: &std::path::Path) -> Vec<(String, u32, u32)> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["diff", "--numstat", "HEAD"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let adds = it.next()?;
            let dels = it.next()?;
            let path = it.next()?.trim().to_string();
            Some((path, adds.parse().unwrap_or(0), dels.parse().unwrap_or(0)))
        })
        .collect()
}

/// Per-commit churn (additions + deletions) for the last 30 commits, oldest →
/// newest, powering the git-panel sparkline. A `0x01` record separator is
/// emitted per commit via `--format`, followed by that commit's `--numstat`.
fn git_churn(dir: &std::path::Path) -> Vec<u64> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["log", "-30", "--numstat", "--format=%x01"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut churn: Vec<u64> = Vec::new();
    let mut cur: u64 = 0;
    let mut started = false;
    for line in text.lines() {
        if line.starts_with('\u{1}') {
            if started {
                churn.push(cur);
            }
            cur = 0;
            started = true;
        } else if !line.trim().is_empty() {
            let mut it = line.split('\t');
            let a = it.next().and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
            let d = it.next().and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
            cur += a + d;
        }
    }
    if started {
        churn.push(cur);
    }
    churn.reverse(); // git log is newest→oldest; the sparkline wants oldest→newest
    churn
}

#[cfg(test)]
mod parse_tests {
    use super::{
        git_is_conflict, parse_file_line, parse_percent, parse_test_progress, todo_marker,
        todo_marker_scope,
    };

    #[test]
    fn git_conflict_codes() {
        // The seven porcelain unmerged states are conflicts.
        for code in ["DD", "AU", "UD", "UA", "DU", "AA", "UU"] {
            assert!(git_is_conflict(code), "{code} should be a conflict");
        }
        // Ordinary staged/working-tree changes and untracked files are not.
        for code in ["M ", " M", "MM", "A ", "AM", "D ", "R ", "??", "!!"] {
            assert!(!git_is_conflict(code), "{code} should not be a conflict");
        }
    }

    #[test]
    fn conflicts_float_to_front() {
        // Mirror the Git-tab refresh: stable sort with conflicts (key=false)
        // first, then count the leading conflict run.
        let mut changes = [
            (" M".to_string(), "a".to_string()),
            ("UU".to_string(), "b".to_string()),
            ("??".to_string(), "c".to_string()),
            ("AA".to_string(), "d".to_string()),
        ];
        changes.sort_by_key(|(code, _)| !git_is_conflict(code));
        let nconf = changes
            .iter()
            .take_while(|(code, _)| git_is_conflict(code))
            .count();
        assert_eq!(nconf, 2);
        // conflicts keep their relative order, as do the non-conflicts
        let order: Vec<&str> = changes.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(order, ["b", "d", "a", "c"]);
    }

    #[test]
    fn percent_token_parsing() {
        assert_eq!(parse_percent("Building 72% done"), Some(72));
        assert_eq!(parse_percent("[ 5%] compiling"), Some(5));
        assert_eq!(parse_percent("done 100%"), Some(100));
        // out-of-range and non-percent tokens are ignored
        assert_eq!(parse_percent("temperature 250% off"), None);
        assert_eq!(parse_percent("no percentage here"), None);
        assert_eq!(parse_percent("just a % sign"), None);
    }

    // Regression: the IDE layout must survive a save (`layout`) → restore
    // (`apply_layout`) round-trip, so reopening the workbench (e.g. via `:ide`)
    // brings back the user's drawer widths, folds, and collapse/hide state.
    #[test]
    fn ide_layout_round_trips() {
        let mut ide = super::Ide::new();
        ide.left_width = 42;
        ide.left_collapsed = true;
        ide.fold_minimap = true;
        ide.bottom_height = 17;
        // A folded left column is encoded as the left divider sitting at 0%.
        ide.bottom_splits = [0, 70];
        ide.bottom_zoom = true;
        let saved = ide.layout();
        assert!(
            saved.bottom_left_folded,
            "s0=0 should persist as left-folded"
        );

        let mut restored = super::Ide::new();
        restored.apply_layout(&saved);
        assert_eq!(restored.left_width, 42);
        assert!(restored.left_collapsed);
        assert!(restored.fold_minimap);
        assert_eq!(restored.bottom_height, 17);
        assert_eq!(restored.bottom_splits, [0, 70]);
        assert!(
            restored.col_folded(0),
            "left column stays folded after restore"
        );
        assert!(restored.bottom_zoom);
    }

    // Bad/uninitialized persisted geometry must fall back to defaults rather than
    // produce an unusable workbench (e.g. a zero-width drawer or inverted splits).
    #[test]
    fn apply_layout_rejects_bad_geometry() {
        let mut ide = super::Ide::new();
        let (def_left, def_splits, def_height) =
            (ide.left_width, ide.bottom_splits, ide.bottom_height);
        let bad = crate::appdata::IdeLayout {
            open: true,
            left_width: 5,           // below the 14-col minimum
            bottom_splits: [80, 20], // disordered (first >= second)
            bottom_height: 0,        // unset
            ..Default::default()
        };
        ide.apply_layout(&bad);
        assert_eq!(
            ide.left_width, def_left,
            "tiny left_width should be rejected"
        );
        assert_eq!(
            ide.bottom_splits, def_splits,
            "disordered splits should be rejected"
        );
        assert_eq!(
            ide.bottom_height, def_height,
            "zero height should be rejected"
        );
    }

    #[test]
    fn test_progress_counts_results() {
        let lines: Vec<String> = [
            "running 3 tests",
            "test foo::a ... ok",
            "test foo::b ... FAILED",
            "test foo::c ... ok",
            "test result: FAILED. 2 passed; 1 failed", // summary line must not be counted
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(parse_test_progress(&lines), Some((2, 3)));
        // ignored tests don't count toward the pass ratio
        let ig = vec!["test x ... ignored".to_string()];
        assert_eq!(parse_test_progress(&ig), None);
        let empty: Vec<String> = Vec::new();
        assert_eq!(parse_test_progress(&empty), None);
    }

    #[test]
    fn todo_marker_word_boundary() {
        // real markers in comment context
        assert_eq!(todo_marker("// TODO: fix this"), Some("TODO"));
        assert_eq!(todo_marker("    # FIXME later"), Some("FIXME"));
        assert_eq!(todo_marker("/* HACK */"), Some("HACK"));
        assert_eq!(todo_marker("NOTE at start"), Some("NOTE"));
        assert_eq!(todo_marker("trailing BUG"), Some("BUG"));
        // false positives that must NOT match (no word boundary)
        assert_eq!(todo_marker("fn update_todos() {}"), None);
        assert_eq!(todo_marker("let DENOTED = 1;"), None);
        assert_eq!(todo_marker("AUTODOC generator"), None);
        assert_eq!(todo_marker("the BUGGY code"), None);
        assert_eq!(todo_marker("no markers here"), None);
        // earliest marker wins when several appear
        assert_eq!(todo_marker("FIXME and TODO"), Some("FIXME"));
        assert_eq!(todo_marker("TODO before FIXME"), Some("TODO"));
    }

    #[test]
    fn todo_marker_severity_scope() {
        assert_eq!(todo_marker_scope("FIXME"), "error");
        assert_eq!(todo_marker_scope("BUG"), "error");
        assert_eq!(todo_marker_scope("TODO"), "warning");
        assert_eq!(todo_marker_scope("HACK"), "warning");
        assert_eq!(todo_marker_scope("NOTE"), "comment");
    }

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
