//! Org-mode agenda overlay (slice 2).
//!
//! A full-screen [`Component`] that lists the TODO/DONE headings collected from
//! every `.org` source the editor can see — all open `.org` buffers (read from
//! their in-memory text, so unsaved edits are reflected) plus every `*.org` file
//! found by a shallow walk of the editor's working directory (skipping
//! `.git`/`target`/`node_modules`, capped in depth and count). Each source is
//! parsed by the pure, unit-tested [`crate::commands::org::parse_agenda`].
//!
//! Items are grouped and sorted: those carrying a `SCHEDULED:`/`DEADLINE:` date
//! come first under a "Scheduled/Deadline" heading sorted ascending by their
//! earliest date; undated TODOs follow under "TODO"; completed (`DONE`) items
//! are listed last under "Done" (kept rather than omitted, rendered dim). Each
//! row shows the (colored) keyword, the `[#A]` priority, the title, and a
//! right-aligned, dimmed `file:line`.
//!
//! Keys: `j`/`k`/arrows + `g`/`G`/Home/End navigate; `Enter` jumps to the item
//! (opens its file and moves the cursor to the heading line); `t` cycles the
//! selected item's TODO keyword when that file is the focused buffer (other
//! buffers / on-disk files are skipped with a note); `r` re-scans; `q`/`Esc`
//! close. Opened with `:org-agenda` (alias `:agenda`).
//!
//! Deferred to later slices: recurring timestamps, real date math / a "today"
//! grouping, `.org` syntax highlighting, capture, babel and export.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tui::buffer::Buffer as Surface;
use zemacs_view::{editor::Action, graphics::Rect, Editor};

use crate::commands::org::{self, parse_agenda, AgendaItem};
use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Max directory depth and `.org` file count for the working-directory scan, so
/// a huge tree can't stall the agenda.
const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 1000;

/// A rendered line of the agenda: either a section header or an item row
/// (carrying the index into [`OrgAgenda::items`]).
enum Row {
    Header(&'static str),
    Item(usize),
}

/// The full-screen interactive org-agenda overlay.
pub struct OrgAgenda {
    /// Working directory the on-disk `.org` scan walks.
    root: PathBuf,
    /// Items in display order (grouped + sorted).
    items: Vec<AgendaItem>,
    /// Index into `items` of the highlighted row.
    selected: usize,
    /// Top visible rendered row.
    scroll: usize,
    /// Body rows visible in the last render (for scroll clamping).
    viewport: usize,
}

impl OrgAgenda {
    /// Build the agenda by scanning the editor's open buffers and `root`.
    pub fn new(editor: &Editor, root: PathBuf) -> Self {
        let mut agenda = OrgAgenda {
            root,
            items: Vec::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
        };
        agenda.refresh(editor);
        agenda
    }

    /// Re-scan all sources and rebuild the sorted item list, clamping the
    /// selection to the new count.
    fn refresh(&mut self, editor: &Editor) {
        let mut items = collect_items(editor, &self.root);
        items.sort_by(|a, b| {
            group_order(a)
                .cmp(&group_order(b))
                .then_with(|| date_key(a).cmp(&date_key(b)))
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.line.cmp(&b.line))
        });
        self.items = items;
        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
    }

    /// Build the linear list of rendered rows, inserting a section header at each
    /// group boundary.
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut last_group: Option<u8> = None;
        for (i, item) in self.items.iter().enumerate() {
            let g = group_order(item);
            if last_group != Some(g) {
                rows.push(Row::Header(group_label(g)));
                last_group = Some(g);
            }
            rows.push(Row::Item(i));
        }
        rows
    }

    /// Move the selection by `delta`, clamped to the item range.
    fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let max = self.items.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Jump to the selected item: open its file and move the cursor to the
    /// heading line, popping this overlay first (mirrors the magit visit).
    fn jump_callback(&self) -> Option<Callback> {
        let item = self.items.get(self.selected)?;
        let path = item.file.clone();
        let line = item.line;
        Some(Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            compositor.pop();
            if let Err(err) = cx.editor.open(&path, Action::Replace) {
                cx.editor
                    .set_error(format!("failed to open {}: {err}", path.display()));
                return;
            }
            let scrolloff = cx.editor.config().scrolloff;
            let (view, doc) = current!(cx.editor);
            let last = doc.text().len_lines().saturating_sub(1);
            let target = line.min(last);
            let pos = doc.text().line_to_char(target);
            doc.set_selection(view.id, zemacs_core::Selection::point(pos));
            view.ensure_cursor_in_view(doc, scrolloff);
        }))
    }

    /// Cycle the selected item's TODO keyword. Straightforward case only: the
    /// item's file must be the focused buffer (so the change goes through the
    /// same `current!`/`Transaction` path as `:org-todo`). Background buffers and
    /// on-disk-only files are skipped with a note.
    fn cycle_keyword(&mut self, cx: &mut Context) {
        let Some(item) = self.items.get(self.selected).cloned() else {
            return;
        };
        let focused = doc!(cx.editor).path().map(|p| p.to_path_buf());
        let same = focused
            .as_deref()
            .map(|p| paths_equal(p, &item.file))
            .unwrap_or(false);
        if !same {
            cx.editor.set_status(
                "org-agenda: `t` cycles the keyword only in the focused buffer — open the file first",
            );
            return;
        }
        let (view, doc) = current!(cx.editor);
        if item.line >= doc.text().len_lines() {
            return;
        }
        let line_str: String = doc
            .text()
            .line(item.line)
            .chars()
            .collect::<String>()
            .trim_end_matches(['\n', '\r'])
            .to_string();
        let new = org::cycle_todo(&line_str);
        let start = doc.text().line_to_char(item.line);
        let end = start + line_str.chars().count();
        let tx = zemacs_core::Transaction::change(
            doc.text(),
            std::iter::once((start, end, Some(new.into()))),
        );
        doc.apply(&tx, view.id);
        doc.append_changes_to_history(view);
        self.refresh(cx.editor);
    }
}

impl Component for OrgAgenda {
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
            key!('G') | key!(End) => self.selected = self.items.len().saturating_sub(1),
            key!('r') => self.refresh(cx.editor),
            key!('t') => self.cycle_keyword(cx),
            key!(Enter) => {
                if let Some(cb) = self.jump_callback() {
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
        let text_style = theme.get("ui.text");
        let todo_style = theme.get("diff.minus");
        let done_style = theme.get("ui.linenr");
        let prio_style = theme.get("constant.numeric");
        let date_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }

        let title = " Org agenda";
        surface.set_stringn(area.x, area.y, title, area.width as usize, header_style);
        let hint = "j/k move  Enter goto  t todo  r refresh  q quit";
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

        if self.items.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "No agenda items",
                area.width as usize,
                info_style,
            );
            return;
        }

        let rows = self.rows();
        // Keep the selected item row inside the viewport.
        if let Some(sel_row) = rows
            .iter()
            .position(|r| matches!(r, Row::Item(i) if *i == self.selected))
        {
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
                Row::Header(text) => {
                    surface.set_stringn(area.x, y, text, area.width as usize, header_style);
                }
                Row::Item(i) => {
                    let item = &self.items[*i];
                    let selected = *i == self.selected;
                    if selected {
                        surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
                    }
                    let pick = |base| if selected { sel_style } else { base };
                    let right = area.x + area.width;

                    // Left part: [date] KEYWORD [#P] title.
                    let mut x = area.x + 2;
                    let put = |surface: &mut Surface, x: &mut u16, s: &str, style| {
                        if *x >= right {
                            return;
                        }
                        let avail = (right - *x) as usize;
                        surface.set_stringn(*x, y, s, avail, style);
                        *x += s.chars().count() as u16;
                    };

                    if let Some(date) = date_key(item) {
                        put(surface, &mut x, &format!("{date} "), pick(date_style));
                    }
                    let kw_style = if item.keyword == "DONE" {
                        done_style
                    } else {
                        todo_style
                    };
                    put(surface, &mut x, &item.keyword, pick(kw_style));
                    put(surface, &mut x, " ", pick(text_style));
                    if let Some(p) = item.priority {
                        put(surface, &mut x, &format!("[#{p}] "), pick(prio_style));
                    }
                    put(surface, &mut x, &item.title, pick(text_style));

                    // Right-aligned, dim file:line.
                    let loc = format!(
                        "{}:{}",
                        display_path(&item.file, &self.root),
                        item.line + 1
                    );
                    let loc_w = loc.chars().count() as u16;
                    // Only draw it if it fits clear of the left content.
                    if loc_w + 1 < area.width && right.saturating_sub(loc_w + 1) > x {
                        surface.set_stringn(
                            right - loc_w - 1,
                            y,
                            &loc,
                            loc_w as usize,
                            pick(date_style),
                        );
                    }
                }
            }
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("org-agenda")
    }
}

/// Sort/group bucket for an item: dated-active (0), undated TODO (1), done (2).
fn group_order(item: &AgendaItem) -> u8 {
    if item.keyword == "DONE" {
        2
    } else if date_key(item).is_some() {
        0
    } else {
        1
    }
}

/// Human label for a group bucket.
fn group_label(group: u8) -> &'static str {
    match group {
        0 => "Scheduled/Deadline",
        1 => "TODO",
        _ => "Done",
    }
}

/// The earliest of an item's scheduled/deadline dates (lexicographic on the
/// `YYYY-MM-DD` strings, which is chronological), or `None` when undated.
fn date_key(item: &AgendaItem) -> Option<&str> {
    match (item.scheduled.as_deref(), item.deadline.as_deref()) {
        (Some(s), Some(d)) => Some(s.min(d)),
        (Some(s), None) => Some(s),
        (None, Some(d)) => Some(d),
        (None, None) => None,
    }
}

/// Collect agenda items from open `.org` buffers (in-memory text) plus the
/// `.org` files under `root`, de-duplicating files that are already open.
fn collect_items(editor: &Editor, root: &Path) -> Vec<AgendaItem> {
    let mut items = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for doc in editor.documents() {
        let Some(path) = doc.path() else {
            continue;
        };
        if path.extension().and_then(|e| e.to_str()) != Some("org") {
            continue;
        }
        seen.insert(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        let text = doc.text().to_string();
        items.extend(parse_agenda(path, &text));
    }

    for path in walk_org_files(root) {
        let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.contains(&canon) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            items.extend(parse_agenda(&path, &text));
        }
    }
    items
}

/// Shallow recursive walk for `*.org` files under `root`, skipping the ignored
/// dirs the file watcher skips and bounded by [`MAX_DEPTH`]/[`MAX_FILES`].
fn walk_org_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if out.len() >= MAX_FILES {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if ft.is_dir() {
                if depth + 1 > MAX_DEPTH {
                    continue;
                }
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if matches!(name.as_ref(), ".git" | "target" | "node_modules") {
                    continue;
                }
                stack.push((path, depth + 1));
            } else if ft.is_file()
                && path.extension().and_then(|e| e.to_str()) == Some("org")
            {
                out.push(path);
                if out.len() >= MAX_FILES {
                    break;
                }
            }
        }
    }
    out
}

/// Display form of `path` for the `file:line` suffix: relative to `root` when
/// possible, else the file name, else the full path.
fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Whether two paths refer to the same file, comparing canonicalised forms and
/// falling back to a direct comparison when canonicalisation fails.
fn paths_equal(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Add BOLD to a style.
fn to_bold(style: zemacs_view::graphics::Style) -> zemacs_view::graphics::Style {
    style.add_modifier(zemacs_view::graphics::Modifier::BOLD)
}
