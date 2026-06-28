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
    keyboard::KeyCode,
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
    Registers,
    Todo,
    Marks,
}

#[derive(Clone, Copy)]
enum BottomHit {
    TabProblems,
    TabRun,
    TabRegisters,
    TabTodo,
    TabMarks,
    Rerun,
    Stop,
}

pub enum IdeAction {
    None,
    OpenFile(PathBuf),
    Goto { from: usize, to: usize },
    /// Run/debug toolbar actions that need editor/compositor access.
    RunStart,
    Debug,
}

#[derive(Clone, Copy)]
enum ToolHit {
    Run,
    Stop,
    Rerun,
    Debug,
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
    left_width: u16,
    left_collapsed: bool,
    resizing_left: bool,
    seam_x: u16,
    left_rail_rect: Rect,
    bottom_height: u16,
    resizing_bottom: bool,

    structure: Vec<OutlineRow>,
    structure_sel: usize,
    structure_state: ratatui::widgets::ListState,
    structure_key: (Option<DocumentId>, usize),

    problems: Vec<ProblemRow>,
    problems_sel: usize,
    problems_state: ratatui::widgets::TableState,
    run: Option<crate::ui::run::Run>,
    registers: Vec<(char, String)>,
    todos: Vec<(usize, String)>,
    marks_list: Vec<(usize, String)>,
    bottom_tab: BottomTab,
    bottom_hits: Vec<(u16, u16, BottomHit)>,
    bottom_header_y: u16,
    bottom_divider_y: u16,
    toolbar_rect: Rect,
    toolbar_y: u16,
    toolbar_hits: Vec<(u16, u16, ToolHit)>,
    total_lines: usize,
    view_top_line: usize,
    view_lines: usize,
    /// Per source line: which columns hold a non-whitespace glyph (for the braille minimap).
    minimap_dots: Vec<Vec<bool>>,
    minimap_key: (Option<DocumentId>, usize),

    project_rect: Rect,
    structure_rect: Rect,
    problems_rect: Rect,
    stripe_rect: Rect,
}

fn empty_rect() -> Rect {
    Rect::new(0, 0, 0, 0)
}

impl Ide {
    pub fn new() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            project: FileTree::new(root),
            focus: Focus::Project,
            visible: true,
            fold_project: false,
            fold_structure: false,
            fold_problems: false,
            left_width: LEFT_W,
            left_collapsed: false,
            resizing_left: false,
            seam_x: u16::MAX,
            left_rail_rect: empty_rect(),
            bottom_height: BOTTOM_H,
            resizing_bottom: false,
            structure: Vec::new(),
            structure_sel: 0,
            structure_state: ratatui::widgets::ListState::default(),
            structure_key: (None, usize::MAX),
            problems: Vec::new(),
            problems_sel: 0,
            problems_state: ratatui::widgets::TableState::default(),
            run: None,
            registers: Vec::new(),
            todos: Vec::new(),
            marks_list: Vec::new(),
            bottom_tab: BottomTab::Problems,
            bottom_hits: Vec::new(),
            bottom_header_y: 0,
            bottom_divider_y: u16::MAX,
            toolbar_rect: empty_rect(),
            toolbar_y: 0,
            toolbar_hits: Vec::new(),
            total_lines: 1,
            view_top_line: 0,
            view_lines: 0,
            minimap_dots: Vec::new(),
            minimap_key: (None, usize::MAX),
            project_rect: empty_rect(),
            structure_rect: empty_rect(),
            problems_rect: empty_rect(),
            stripe_rect: empty_rect(),
        }
    }

    /// True while a panel (not the editor) holds focus — editor cursor hidden, keys routed here.
    pub fn capturing(&self) -> bool {
        self.visible && self.focus != Focus::Editor
    }

    pub fn visible(&self) -> bool {
        self.visible
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
            // fold/unfold the focused drawer
            KeyCode::Char('z') => {
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

    fn list_key(&mut self, key: KeyEvent, structure: bool) -> IdeAction {
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
                        Some(ToolHit::Debug) => IdeAction::Debug,
                        Some(ToolHit::Stop) => {
                            if let Some(r) = &self.run {
                                crate::ui::run::stop(r);
                            }
                            IdeAction::None
                        }
                        Some(ToolHit::Rerun) => {
                            if let Some(r) = self.run.clone() {
                                self.run = Some(crate::ui::run::rerun(&r));
                                self.bottom_tab = BottomTab::Run;
                            }
                            IdeAction::None
                        }
                        None => IdeAction::None,
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
                    match self
                        .bottom_hits
                        .iter()
                        .find(|&&(a, b, _)| col >= a && col < b)
                        .map(|&(_, _, h)| h)
                    {
                        Some(BottomHit::TabProblems) => self.bottom_tab = BottomTab::Problems,
                        Some(BottomHit::TabRun) => self.bottom_tab = BottomTab::Run,
                        Some(BottomHit::TabRegisters) => self.bottom_tab = BottomTab::Registers,
                        Some(BottomHit::TabTodo) => self.bottom_tab = BottomTab::Todo,
                        Some(BottomHit::TabMarks) => self.bottom_tab = BottomTab::Marks,
                        Some(BottomHit::Stop) => {
                            if let Some(r) = &self.run {
                                crate::ui::run::stop(r);
                            }
                        }
                        Some(BottomHit::Rerun) => {
                            if let Some(r) = self.run.clone() {
                                self.run = Some(crate::ui::run::rerun(&r));
                                self.bottom_tab = BottomTab::Run;
                            }
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
                        return IdeAction::Goto { from: *pos, to: *pos };
                    }
                    return IdeAction::None;
                }
                if in_rect(&self.problems_rect, col, row)
                    && row > self.problems_rect.y
                    && self.bottom_tab == BottomTab::Marks
                {
                    let idx = (row - self.problems_rect.y - 1) as usize;
                    if let Some((pos, _)) = self.marks_list.get(idx) {
                        return IdeAction::Goto { from: *pos, to: *pos };
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
                    let frac = (row - self.stripe_rect.y) as f32 / self.stripe_rect.height as f32;
                    let line = ((frac * self.total_lines as f32) as usize).min(self.total_lines.saturating_sub(1));
                    let pos = line_to_char(line);
                    return IdeAction::Goto { from: pos, to: pos };
                }
                IdeAction::None
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = matches!(ev.kind, MouseEventKind::ScrollDown);
                if in_rect(&self.project_rect, col, row) {
                    self.project.scroll_sel(down);
                } else if in_rect(&self.structure_rect, col, row) {
                    step(&mut self.structure_sel, self.structure.len(), down);
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
            MouseEventKind::Up(MouseButton::Left) => {
                self.resizing_left = false;
                self.resizing_bottom = false;
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

        // right minimap pane
        if rest.width > STRIPE_W + 30 {
            self.stripe_rect = Rect::new(rest.x + rest.width - STRIPE_W, rest.y, STRIPE_W, rest.height);
            rest = Rect::new(rest.x, rest.y, rest.width - STRIPE_W, rest.height);
        } else {
            self.stripe_rect = empty_rect();
        }

        // bottom problems — a visible divider line above it is the resize handle (like the left seam)
        let bh = if self.fold_problems { 1 } else { self.bottom_height };
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
            self.render_stripe(surface, theme);
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

        rest
    }

    fn refresh(&mut self, cx: &mut crate::compositor::Context) {
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

        let (view, doc) = current_ref!(cx.editor);
        self.view_top_line = doc.text().char_to_line(doc.view_offset(view.id).anchor);
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
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(crate::ui::rat::to_rat_style(theme.get("ui.window")))
            .title(Span::styled(format!(" {chevron} STRUCTURE "), title_style));

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
        let kind_style = crate::ui::rat::to_rat_style(theme.get("function"));
        let sel_style = crate::ui::rat::to_rat_style(theme.get("ui.selection")).add_modifier(RMod::BOLD);

        let items: Vec<ListItem> = self
            .structure
            .iter()
            .map(|o| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {} ", short_kind(&o.kind)), kind_style),
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

        // run config label on the LEFT
        let cfg = self
            .run
            .as_ref()
            .map(|r| r.lock().unwrap().cmd.clone())
            .unwrap_or_else(|| "cargo run".to_string());
        surface.set_stringn(area.x + 1, area.y, &format!("[{cfg}]"), area.width as usize, theme.get("comment"));

        // run/debug buttons RIGHT-aligned
        let buttons: [(&str, _, ToolHit); 4] = [
            (" ▶ Run ", theme.get("diff.plus"), ToolHit::Run),
            (" ◼ Stop ", theme.get("error"), ToolHit::Stop),
            (" ⟳ Rerun ", theme.get("function"), ToolHit::Rerun),
            (" 🐞 Debug ", theme.get("keyword"), ToolHit::Debug),
        ];
        let gap = 1u16;
        let total: u16 = buttons.iter().map(|(t, _, _)| disp_width(t)).sum::<u16>()
            + gap * (buttons.len() as u16 - 1);
        let mut x = area.x + area.width.saturating_sub(total + 1);
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

        // Problems tab
        let plabel = format!(" PROBLEMS {} ", self.problems.len());
        let pw = plabel.chars().count() as u16;
        surface.set_stringn(x, area.y, &plabel, area.width as usize, if self.bottom_tab == BottomTab::Problems { on } else { off });
        self.bottom_hits.push((x, x + pw, BottomHit::TabProblems));
        x += pw + 1;

        // Run tab
        let rlabel = " RUN ";
        let rw = rlabel.chars().count() as u16;
        surface.set_stringn(x, area.y, rlabel, area.width as usize, if self.bottom_tab == BottomTab::Run { on } else { off });
        self.bottom_hits.push((x, x + rw, BottomHit::TabRun));
        x += rw + 1;

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
        x += mw + 2;

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
            BottomTab::Registers => self.render_registers_body(surface, theme, body),
            BottomTab::Todo => self.render_todo_body(surface, theme, body),
            BottomTab::Marks => self.render_marks_body(surface, theme, body),
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
        for (i, (_, text)) in self.todos.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
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
        for (i, (ch, content)) in self.registers.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
            let label = format!(" \"{ch}  ");
            let (nx, _) = surface.set_stringn(body.x, y, &label, body.width as usize, name_style);
            surface.set_stringn(nx, y, content, body.width.saturating_sub(nx - body.x) as usize, base);
        }
    }

    fn render_problems_body(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme, body: Rect) {
        use ratatui::layout::Constraint;
        use ratatui::style::Modifier as RMod;
        use ratatui::widgets::{Cell, Row, Table};
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
        .row_highlight_style(sel)
        .highlight_symbol("› ");
        self.problems_state.select(Some(self.problems_sel));
        crate::ui::rat::render_stateful(table, body, surface, &mut self.problems_state);
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
        for (i, (_, disp)) in self.marks_list.iter().enumerate() {
            if i >= height {
                break;
            }
            let y = body.y + i as u16;
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
        if height == 0 {
            return;
        }
        let base = theme.get("ui.text");
        // tail-follow: show the last `height` lines
        let start = s.lines.len().saturating_sub(height);
        for (i, line) in s.lines[start..].iter().enumerate() {
            if i >= height {
                break;
            }
            surface.set_stringn(body.x, body.y + i as u16, line, body.width as usize, base);
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

fn short_kind(kind: &str) -> &str {
    match kind {
        "function" | "method" => "ƒ",
        "class" | "struct" | "interface" | "enum" => "◇",
        "module" | "namespace" => "▣",
        "constant" | "variable" | "field" => "•",
        _ => "·",
    }
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
