//! Find-in-Files: a full-screen project-wide search & replace tool window.
//!
//! A modal [`Component`] (same overlay pattern as [`crate::ui::repl`]) with a
//! query + replace field, regex/case toggles, and a grouped results list
//! (file → matching lines, match substrings highlighted). Navigate with the
//! arrows / `C-n`/`C-p`, `Enter` on a match opens the file at that line, and
//! `Alt-Enter` performs a (two-step, confirmed) project-wide replace.
//!
//! Open: `:search [query]` · the search runs synchronously over the working
//! directory, respecting `.gitignore` (via the `ignore` crate), capped so a huge
//! tree can't hang the UI.

use std::path::PathBuf;

use regex::{NoExpand, RegexBuilder};
use tui::buffer::Buffer as Surface;
use zemacs_core::Selection;
use zemacs_view::{
    document::Mode,
    editor::Action,
    graphics::{CursorKind, Rect},
    input::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind},
};

use crate::compositor::{Component, Compositor, Context, Event, EventResult};

/// Cap total matches / per-file size so the synchronous search stays snappy.
const MAX_MATCHES: usize = 5000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// One matching line within a file.
struct LineMatch {
    line0: usize,                // 0-based line number
    text: String,                // the (truncated) line text
    ranges: Vec<(usize, usize)>, // char ranges of matches within `text`
}

/// All matches in a single file.
struct FileHit {
    path: PathBuf,
    rel: String,
    matches: Vec<LineMatch>,
}

/// A flattened, navigable row: a file header or one matching line.
#[derive(Clone, Copy)]
enum Row {
    Header(usize),       // index into `hits`
    Match(usize, usize), // (file index, match index)
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Query,
    Replace,
}

pub struct SearchPanel {
    query: Vec<char>,
    replace: Vec<char>,
    cursor: usize,
    field: Field,
    regex: bool,
    case: bool,
    hits: Vec<FileHit>,
    rows: Vec<Row>,
    sel: usize,
    scroll: u16,
    status: String,
    /// Armed when a replace-all is pending confirmation (press Alt-Enter again).
    confirm_replace: bool,
    caret: Option<zemacs_core::Position>,
    /// Click hit regions for the toggles: `(x0, x1, row, is_regex)`.
    toggle_hits: Vec<(u16, u16, u16, bool)>,
}

impl SearchPanel {
    pub fn new(initial: &str) -> Self {
        let mut p = Self {
            query: initial.chars().collect(),
            replace: Vec::new(),
            cursor: initial.chars().count(),
            field: Field::Query,
            regex: false,
            case: false,
            hits: Vec::new(),
            rows: Vec::new(),
            sel: 0,
            scroll: 0,
            status: "type a query · Enter to search".to_string(),
            confirm_replace: false,
            caret: None,
            toggle_hits: Vec::new(),
        };
        if !p.query.is_empty() {
            p.run_search();
        }
        p
    }

    fn buf(&self) -> &Vec<char> {
        match self.field {
            Field::Query => &self.query,
            Field::Replace => &self.replace,
        }
    }
    fn buf_mut(&mut self) -> &mut Vec<char> {
        match self.field {
            Field::Query => &mut self.query,
            Field::Replace => &mut self.replace,
        }
    }

    /// Compile the active query into a `Regex` (escaping it in literal mode).
    fn build_regex(&self) -> Result<regex::Regex, String> {
        let pat: String = self.query.iter().collect();
        let pat = if self.regex { pat } else { regex::escape(&pat) };
        RegexBuilder::new(&pat)
            .case_insensitive(!self.case)
            .build()
            .map_err(|e| format!("bad regex: {e}"))
    }

    /// Run the search over the working directory, filling `hits`/`rows`.
    fn run_search(&mut self) {
        self.hits.clear();
        self.rows.clear();
        self.sel = 0;
        self.scroll = 0;
        self.confirm_replace = false;
        if self.query.is_empty() {
            self.status = "type a query · Enter to search".to_string();
            return;
        }
        let re = match self.build_regex() {
            Ok(re) => re,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let root = zemacs_stdx::env::current_working_dir();
        let mut total = 0usize;
        'walk: for entry in ignore::WalkBuilder::new(&root).build().flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
                continue;
            }
            let path = entry.path();
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            let mut matches = Vec::new();
            for (i, line) in content.lines().enumerate() {
                let mut ranges = Vec::new();
                for m in re.find_iter(line) {
                    // byte offsets → char indices within the line
                    let start = line[..m.start()].chars().count();
                    let end = line[..m.end()].chars().count();
                    ranges.push((start, end));
                }
                if !ranges.is_empty() {
                    let text: String = line.chars().take(400).collect();
                    matches.push(LineMatch {
                        line0: i,
                        text,
                        ranges,
                    });
                    total += 1;
                    if total >= MAX_MATCHES {
                        let rel = rel_path(&root, path);
                        self.hits.push(FileHit {
                            path: path.to_path_buf(),
                            rel,
                            matches,
                        });
                        break 'walk;
                    }
                }
            }
            if !matches.is_empty() {
                let rel = rel_path(&root, path);
                self.hits.push(FileHit {
                    path: path.to_path_buf(),
                    rel,
                    matches,
                });
            }
        }
        self.rebuild_rows();
        let capped = if total >= MAX_MATCHES {
            " (capped)"
        } else {
            ""
        };
        self.status = format!("{total} matches in {} files{capped}", self.hits.len());
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        for (fi, hit) in self.hits.iter().enumerate() {
            self.rows.push(Row::Header(fi));
            for mi in 0..hit.matches.len() {
                self.rows.push(Row::Match(fi, mi));
            }
        }
    }

    /// Perform the project-wide replace across every file with matches.
    fn replace_all(&mut self) {
        let replacement: String = self.replace.iter().collect();
        let re = match self.build_regex() {
            Ok(re) => re,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let mut files = 0usize;
        let mut count = 0usize;
        for hit in &self.hits {
            let Ok(content) = std::fs::read_to_string(&hit.path) else {
                continue;
            };
            let n: usize = re.find_iter(&content).count();
            let new = if self.regex {
                re.replace_all(&content, replacement.as_str())
            } else {
                re.replace_all(&content, NoExpand(&replacement))
            };
            if new != content && std::fs::write(&hit.path, new.as_ref()).is_ok() {
                files += 1;
                count += n;
            }
        }
        self.status = format!("replaced {count} occurrences in {files} files — re-running search");
        self.run_search();
    }

    fn move_sel(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as isize;
        let mut i = self.sel as isize + delta;
        i = i.clamp(0, n - 1);
        self.sel = i as usize;
    }

    /// Resolve the selected row to `(path, 1-based line)` for opening.
    fn selected_target(&self) -> Option<(PathBuf, usize)> {
        match self.rows.get(self.sel)? {
            Row::Header(fi) => {
                let h = self.hits.get(*fi)?;
                Some((
                    h.path.clone(),
                    h.matches.first().map(|m| m.line0 + 1).unwrap_or(1),
                ))
            }
            Row::Match(fi, mi) => {
                let h = self.hits.get(*fi)?;
                let m = h.matches.get(*mi)?;
                Some((h.path.clone(), m.line0 + 1))
            }
        }
    }

    fn close() -> EventResult {
        EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
            c.pop();
        })))
    }

    /// Open `path` at 1-based `line`, popping this panel.
    fn open(path: PathBuf, line: usize) -> EventResult {
        EventResult::Consumed(Some(Box::new(
            move |c: &mut Compositor, cx: &mut Context| {
                c.pop();
                let scrolloff = cx.editor.config().scrolloff;
                match cx.editor.open(&path, Action::Replace) {
                    Ok(_) => {
                        let (view, doc) = current!(cx.editor);
                        let text = doc.text();
                        let last = text.len_lines().saturating_sub(1);
                        let pos = text.line_to_char(line.saturating_sub(1).min(last));
                        doc.set_selection(view.id, Selection::point(pos));
                        view.ensure_cursor_in_view(doc, scrolloff);
                    }
                    Err(e) => cx.editor.set_error(format!("open failed: {e}")),
                }
            },
        )))
    }
}

impl Component for SearchPanel {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key: KeyEvent = match event {
            Event::Key(k) => *k,
            Event::Mouse(ev) => return self.handle_mouse(ev.column, ev.row, ev.kind),
            Event::Paste(s) => {
                let cur = self.cursor;
                let b = self.buf_mut();
                for (i, c) in s.chars().enumerate() {
                    b.insert(cur + i, c);
                }
                self.cursor += s.chars().count();
                return EventResult::Consumed(None);
            }
            _ => return EventResult::Ignored(None),
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // Alt-Enter: two-step confirmed replace-all.
        if key.code == KeyCode::Enter && alt {
            if self.query.is_empty() || self.hits.is_empty() {
                return EventResult::Consumed(None);
            }
            if self.confirm_replace {
                self.confirm_replace = false;
                self.replace_all();
            } else {
                self.confirm_replace = true;
                let r: String = self.replace.iter().collect();
                self.status =
                    format!("replace all → \"{r}\"? Alt-Enter again to confirm, Esc to cancel");
            }
            return EventResult::Consumed(None);
        }
        // any other key cancels a pending replace confirmation
        self.confirm_replace = false;

        match key.code {
            KeyCode::Esc | KeyCode::Char('c') if matches!(key.code, KeyCode::Esc) || ctrl => {
                return Self::close()
            }
            KeyCode::Enter => {
                // In the results, open the selection; in the query field, search.
                if let Some((p, l)) = self.selected_target() {
                    if !self.rows.is_empty() {
                        return Self::open(p, l);
                    }
                }
                self.run_search();
            }
            KeyCode::Tab => {
                self.field = if self.field == Field::Query {
                    Field::Replace
                } else {
                    Field::Query
                };
                self.cursor = self.buf().len();
            }
            KeyCode::Char('r') if alt => {
                self.regex = !self.regex;
                self.run_search();
            }
            KeyCode::Char('i') if alt => {
                self.case = !self.case;
                self.run_search();
            }
            KeyCode::Down | KeyCode::Char('n') if matches!(key.code, KeyCode::Down) || ctrl => {
                self.move_sel(1)
            }
            KeyCode::Up | KeyCode::Char('p') if matches!(key.code, KeyCode::Up) || ctrl => {
                self.move_sel(-1)
            }
            KeyCode::PageDown => self.move_sel(10),
            KeyCode::PageUp => self.move_sel(-10),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.buf().len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.buf().len(),
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    let i = self.cursor;
                    self.buf_mut().remove(i);
                    if self.field == Field::Query {
                        self.run_search();
                    }
                }
            }
            KeyCode::Char('u') if ctrl => {
                let c = self.cursor;
                self.buf_mut().drain(0..c);
                self.cursor = 0;
                if self.field == Field::Query {
                    self.run_search();
                }
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                let i = self.cursor;
                self.buf_mut().insert(i, c);
                self.cursor += 1;
                if self.field == Field::Query {
                    self.run_search();
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let base = theme.get("ui.text");
        let dim = theme.get("comment");
        let accent = theme.get("function");
        let kw = theme.get("keyword");
        let sel_bg = theme.get("ui.selection");
        let hl = theme.get("ui.cursor.match");
        surface.clear_with(area, theme.get("ui.background"));
        self.toggle_hits.clear();
        if area.height < 6 || area.width < 20 {
            self.caret = None;
            return;
        }

        // title bar
        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        surface.set_stringn(area.x + 1, area.y, " Find in Files ", 15, accent);
        let right = format!(" {} ", self.status);
        let rw = right.chars().count().min(area.width as usize / 2) as u16;
        surface.set_stringn(area.x + area.width - rw, area.y, &right, rw as usize, dim);

        // query row
        let qy = area.y + 1;
        surface.set_stringn(area.x + 1, qy, "Find:   ", 8, dim);
        let qx = area.x + 9;
        let qw = area.width.saturating_sub(24);
        let qstr: String = self.query.iter().collect();
        surface.set_stringn(qx, qy, &qstr, qw as usize, base);
        // toggles, right-aligned on the query row
        let rt = if self.regex { accent } else { dim };
        let ct = if self.case { accent } else { dim };
        let tx = area.x + area.width - 14;
        surface.set_stringn(tx, qy, ".* regex", 8, rt);
        self.toggle_hits.push((tx, tx + 8, qy, true));
        surface.set_stringn(tx + 9, qy, "Aa", 2, ct);
        self.toggle_hits.push((tx + 9, tx + 11, qy, false));

        // replace row
        let ry = area.y + 2;
        surface.set_stringn(area.x + 1, ry, "Replace:", 8, dim);
        let rstr: String = self.replace.iter().collect();
        surface.set_stringn(qx, ry, &rstr, qw as usize, base);

        // separator
        surface.set_stringn(
            area.x,
            area.y + 3,
            &"─".repeat(area.width as usize),
            area.width as usize,
            dim,
        );

        // results
        let body_y = area.y + 4;
        let footer_y = area.y + area.height - 1;
        let body_h = footer_y.saturating_sub(body_y);
        if self.sel as u16 >= self.scroll + body_h {
            self.scroll = self.sel as u16 - body_h + 1;
        } else if (self.sel as u16) < self.scroll {
            self.scroll = self.sel as u16;
        }
        for vis in 0..body_h {
            let ri = (self.scroll + vis) as usize;
            let Some(row) = self.rows.get(ri) else { break };
            let y = body_y + vis;
            if ri == self.sel {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel_bg);
            }
            match row {
                Row::Header(fi) => {
                    let h = &self.hits[*fi];
                    surface.set_stringn(area.x + 1, y, &h.rel, (area.width - 8) as usize, kw);
                    let cnt = format!(" {} ", h.matches.len());
                    surface.set_stringn(
                        area.x + area.width - cnt.len() as u16 - 1,
                        y,
                        &cnt,
                        cnt.len(),
                        dim,
                    );
                }
                Row::Match(fi, mi) => {
                    let m = &self.hits[*fi].matches[*mi];
                    let num = format!("{:>5} ", m.line0 + 1);
                    surface.set_stringn(area.x + 2, y, &num, 6, dim);
                    let tx0 = area.x + 8;
                    let maxw = area.width.saturating_sub(8) as usize;
                    // render with match ranges highlighted
                    for (ci, ch) in m.text.chars().take(maxw).enumerate() {
                        let in_match = m.ranges.iter().any(|&(s, e)| ci >= s && ci < e);
                        let st = if in_match { hl } else { base };
                        let mut b = [0u8; 4];
                        surface.set_stringn(tx0 + ci as u16, y, ch.encode_utf8(&mut b), 1, st);
                    }
                }
            }
        }
        if self.rows.is_empty() {
            surface.set_stringn(area.x + 2, body_y, "  no results", 20, dim);
        }

        // footer
        let hint = " Enter open · Tab field · Alt-r regex · Alt-i case · Alt-Enter replace-all · Esc close ";
        surface.set_stringn(
            area.x + 1,
            footer_y,
            hint,
            area.width.saturating_sub(2) as usize,
            dim,
        );

        // caret in the focused input (both fields share the same left column)
        let cx = qx;
        let cy = if self.field == Field::Query { qy } else { ry };
        self.caret = Some(zemacs_core::Position::new(
            cy as usize,
            (cx + self.cursor as u16) as usize,
        ));
    }

    fn cursor(
        &self,
        _area: Rect,
        editor: &zemacs_view::editor::Editor,
    ) -> (Option<zemacs_core::Position>, CursorKind) {
        (
            self.caret,
            editor.config().cursor_shape.from_mode(Mode::Insert),
        )
    }

    fn id(&self) -> Option<&'static str> {
        Some("search")
    }
}

impl SearchPanel {
    fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) -> EventResult {
        match kind {
            MouseEventKind::ScrollDown => self.move_sel(2),
            MouseEventKind::ScrollUp => self.move_sel(-2),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(&(_, _, _, is_regex)) = self
                    .toggle_hits
                    .iter()
                    .find(|&&(x0, x1, r, _)| row == r && col >= x0 && col < x1)
                {
                    if is_regex {
                        self.regex = !self.regex;
                    } else {
                        self.case = !self.case;
                    }
                    self.run_search();
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }
}

/// Display path relative to `root` when possible.
fn rel_path(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
