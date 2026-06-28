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
const STRIPE_W: u16 = 1;

#[derive(PartialEq, Clone, Copy)]
enum Focus {
    Editor,
    Project,
    Structure,
    Problems,
}

pub enum IdeAction {
    None,
    OpenFile(PathBuf),
    Goto { from: usize, to: usize },
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

    structure: Vec<OutlineRow>,
    structure_sel: usize,
    structure_scroll: usize,
    structure_key: (Option<DocumentId>, usize),

    problems: Vec<ProblemRow>,
    problems_sel: usize,
    problems_scroll: usize,
    total_lines: usize,
    view_top_line: usize,
    view_lines: usize,

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
            structure: Vec::new(),
            structure_sel: 0,
            structure_scroll: 0,
            structure_key: (None, usize::MAX),
            problems: Vec::new(),
            problems_sel: 0,
            problems_scroll: 0,
            total_lines: 1,
            view_top_line: 0,
            view_lines: 0,
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
        [self.project_rect, self.structure_rect, self.problems_rect, self.stripe_rect]
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
                if in_rect(&self.problems_rect, col, row) && row == self.problems_rect.y {
                    self.focus = Focus::Problems;
                    self.fold_problems = !self.fold_problems;
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
                    let idx = self.structure_scroll + (row - self.structure_rect.y - 1) as usize;
                    if idx < self.structure.len() {
                        self.structure_sel = idx;
                        let o = &self.structure[idx];
                        return IdeAction::Goto { from: o.start, to: o.end };
                    }
                }
                if in_rect(&self.problems_rect, col, row) && row > self.problems_rect.y {
                    self.focus = Focus::Problems;
                    let idx = self.problems_scroll + (row - self.problems_rect.y - 1) as usize;
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
            _ => IdeAction::None,
        }
    }

    /// Render every panel; returns the rect left for the editor.
    pub fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut crate::compositor::Context) -> Rect {
        if !self.visible {
            return area;
        }
        self.refresh(cx);

        let mut rest = area;

        // left column: project (top) + structure (bottom), each foldable to its header row
        if rest.width > LEFT_W + 24 {
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
            self.project_rect = Rect::new(rest.x, rest.y, LEFT_W, ph);
            self.structure_rect = Rect::new(rest.x, rest.y + ph, LEFT_W, sh);
            rest = Rect::new(rest.x + LEFT_W, rest.y, rest.width - LEFT_W, rest.height);
        } else {
            self.project_rect = empty_rect();
            self.structure_rect = empty_rect();
        }

        // right error stripe
        if rest.width > 12 {
            self.stripe_rect = Rect::new(rest.x + rest.width - STRIPE_W, rest.y, STRIPE_W, rest.height);
            rest = Rect::new(rest.x, rest.y, rest.width - STRIPE_W, rest.height);
        } else {
            self.stripe_rect = empty_rect();
        }

        // bottom problems (foldable to its header row)
        let bh = if self.fold_problems { 1 } else { BOTTOM_H };
        if rest.height > bh + 4 {
            self.problems_rect = Rect::new(rest.x, rest.y + rest.height - bh, rest.width, bh);
            rest = Rect::new(rest.x, rest.y, rest.width, rest.height - bh);
        } else {
            self.problems_rect = empty_rect();
        }

        self.view_lines = rest.height as usize;

        let theme = &cx.editor.theme;
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
            self.render_problems(surface, theme);
        }
        if self.stripe_rect.height > 0 {
            self.render_stripe(surface, theme);
        }

        rest
    }

    fn refresh(&mut self, cx: &mut crate::compositor::Context) {
        let doc = doc!(cx.editor);
        let key = (Some(doc.id()), doc.text().len_chars());
        if key != self.structure_key {
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

        let (view, doc) = current_ref!(cx.editor);
        self.view_top_line = doc.text().char_to_line(doc.view_offset(view.id).anchor);
    }

    fn render_structure(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme) {
        let area = self.structure_rect;
        surface.clear_with(area, theme.get("ui.background"));
        draw_header(surface, area, "STRUCTURE", self.fold_structure, self.focus == Focus::Structure, theme);
        if self.fold_structure {
            return;
        }
        let body = body_rect(area);
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.structure.is_empty() {
            surface.set_stringn(body.x, body.y, "  (no symbols)", body.width as usize, theme.get("comment"));
            return;
        }
        if self.structure_sel < self.structure_scroll {
            self.structure_scroll = self.structure_sel;
        } else if self.structure_sel >= self.structure_scroll + height {
            self.structure_scroll = self.structure_sel + 1 - height;
        }
        let base = theme.get("ui.text");
        let sel = theme.get("ui.selection");
        for i in 0..height {
            let idx = self.structure_scroll + i;
            let Some(item) = self.structure.get(idx) else { break };
            let y = body.y + i as u16;
            if idx == self.structure_sel {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel);
            }
            let text = format!(" {} {}", short_kind(&item.kind), item.name);
            surface.set_stringn(area.x, y, &text, area.width as usize, base);
        }
    }

    fn render_problems(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme) {
        let area = self.problems_rect;
        surface.clear_with(area, theme.get("ui.background"));
        let title = format!("PROBLEMS  {}", self.problems.len());
        draw_header(surface, area, &title, self.fold_problems, self.focus == Focus::Problems, theme);
        if self.fold_problems {
            return;
        }
        let body = body_rect(area);
        let height = body.height as usize;
        if height == 0 {
            return;
        }
        if self.problems.is_empty() {
            surface.set_stringn(body.x, body.y, "  no problems", body.width as usize, theme.get("comment"));
            return;
        }
        if self.problems_sel < self.problems_scroll {
            self.problems_scroll = self.problems_sel;
        } else if self.problems_sel >= self.problems_scroll + height {
            self.problems_scroll = self.problems_sel + 1 - height;
        }
        let base = theme.get("ui.text");
        let sel = theme.get("ui.selection");
        for i in 0..height {
            let idx = self.problems_scroll + i;
            let Some(p) = self.problems.get(idx) else { break };
            let y = body.y + i as u16;
            if idx == self.problems_sel {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel);
            }
            let (mark, mstyle) = sev_mark(p.sev, theme);
            surface.set_stringn(area.x, y, mark, area.width as usize, mstyle);
            let text = format!("  {}:  {}", p.line + 1, p.msg.replace('\n', " "));
            surface.set_stringn(area.x + 2, y, &text, area.width.saturating_sub(2) as usize, base);
        }
    }

    fn render_stripe(&mut self, surface: &mut Surface, theme: &zemacs_view::Theme) {
        let area = self.stripe_rect;
        surface.clear_with(area, theme.get("ui.virtual.ruler"));
        let total = self.total_lines.max(1);
        let h = area.height as f32;
        let last = area.y + area.height.saturating_sub(1);

        // viewport box — always visible, so the stripe reads as a scrollbar even with no diagnostics
        let y0 = area.y + ((self.view_top_line as f32 / total as f32) * h) as u16;
        let vp_end = (self.view_top_line + self.view_lines).min(total);
        let y1 = (area.y + ((vp_end as f32 / total as f32) * h) as u16).min(last);
        let vp = theme.get("ui.selection");
        let mut y = y0.min(last);
        while y <= y1 {
            surface.set_string(area.x, y, "▐", vp);
            y += 1;
        }

        // diagnostic ticks on top
        for p in &self.problems {
            let ty = (area.y + ((p.line as f32 / total as f32) * h) as u16).min(last);
            let (_, style) = sev_mark(p.sev, theme);
            surface.set_string(area.x, ty, "▌", style);
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
