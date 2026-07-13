//! Occur — the zemacs port of GNU Emacs `occur` / `occur-mode` (`M-s o`).
//!
//! A full-screen [`Component`] over the pure, unit-tested
//! [`zemacs_core::occur`] engine. `occur` searches the source buffer for a
//! regexp and lists every matching line as `N:<line>` (1-based line number),
//! with a movable cursor; visiting an entry jumps to that line in the source
//! buffer. The overlay owns the source buffer's `DocumentId`/`ViewId` so it can
//! set the selection there when the user selects a hit.
//!
//! Keys (parsed into an `occur` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs `occur-mode` counterpart in the port tracker):
//!   n/p, j/k, arrows — move the cursor over the hit list
//!   RET — go to the occurrence, closing the overlay (`occur-mode-goto-occurrence`)
//!   o   — go to the occurrence in the source window, closing the overlay
//!         (`occur-mode-goto-occurrence-other-window`)
//!   C-o — display the occurrence in the source buffer, leaving the list open
//!         (`occur-mode-display-occurrence`)
//!   g   — re-run the search against the current buffer text (`revert-buffer`)
//!   q / Esc — quit the overlay

use tui::buffer::Buffer as Surface;
use zemacs_core::occur::{occur as collect, Match};
use zemacs_view::{graphics::Rect, DocumentId, Editor, ViewId};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The interactive Occur overlay.
pub struct Occur {
    /// The source buffer for single-buffer occur (also the `g` re-run target).
    doc_id: DocumentId,
    /// The source view whose selection is moved when a hit is visited.
    view_id: ViewId,
    /// The regexp source, kept for the title and for `g` (re-run).
    pattern: String,
    /// The matching lines, in buffer order.
    matches: Vec<Match>,
    /// The buffer each match points into, parallel to `matches`. For
    /// single-buffer occur every entry is `doc_id`; for multi-occur they vary.
    doc_ids: Vec<DocumentId>,
    /// The per-match buffer label shown before each hit in multi-occur mode
    /// (empty in single-buffer mode, where no label is shown).
    labels: Vec<String>,
    /// Whether this is a multi-buffer occur (affects labels, jumping and re-run).
    multi: bool,
    /// Cursor position within `matches`.
    cursor: usize,
    /// Scroll offset (first visible row) in list entries.
    scroll: usize,
    /// Rows available for the list, updated each render for page-sizing.
    viewport: usize,
    status: String,
}

impl Occur {
    /// Open the Occur overlay over a pre-collected hit list. `doc_id`/`view_id`
    /// identify the source buffer/view the matches came from and jump back into.
    pub fn new(doc_id: DocumentId, view_id: ViewId, pattern: String, matches: Vec<Match>) -> Self {
        let doc_ids = vec![doc_id; matches.len()];
        Occur {
            doc_id,
            view_id,
            pattern,
            matches,
            doc_ids,
            labels: Vec::new(),
            multi: false,
            cursor: 0,
            scroll: 0,
            viewport: 1,
            status: String::new(),
        }
    }

    /// Open a multi-buffer Occur overlay (Emacs `multi-occur` /
    /// `multi-occur-in-matching-buffers`). Each entry is `(source buffer, buffer
    /// label, matching line)`; visiting a hit switches `view_id` to that buffer.
    pub fn multi(
        view_id: ViewId,
        pattern: String,
        entries: Vec<(DocumentId, String, Match)>,
    ) -> Self {
        let mut doc_ids = Vec::with_capacity(entries.len());
        let mut labels = Vec::with_capacity(entries.len());
        let mut matches = Vec::with_capacity(entries.len());
        for (id, label, m) in entries {
            doc_ids.push(id);
            labels.push(label);
            matches.push(m);
        }
        // A stable fallback `doc_id` for the (unused) single-buffer re-run path.
        let doc_id = doc_ids.first().copied().unwrap_or_default();
        Occur {
            doc_id,
            view_id,
            pattern,
            matches,
            doc_ids,
            labels,
            multi: true,
            cursor: 0,
            scroll: 0,
            viewport: 1,
            status: String::new(),
        }
    }

    /// One `*Occur*` row: `<line>:<text>`, prefixed with the buffer label in
    /// multi-occur mode (`<buffer>:<line>:<text>`).
    fn entry_line(&self, idx: usize, m: &Match) -> String {
        match self.labels.get(idx) {
            Some(label) if self.multi => format!("{label}:{}:{}", m.line_number, m.line_text),
            _ => format!("{:>6}:{}", m.line_number, m.line_text),
        }
    }

    /// Move the cursor by `delta`, clamping to the list bounds.
    fn move_cursor(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        let last = self.matches.len() - 1;
        let next = (self.cursor as isize + delta).clamp(0, last as isize);
        self.cursor = next as usize;
    }

    /// Build the callback that pops the overlay and moves point to the hit under
    /// the cursor in the source view (`RET`, `o`). In multi-occur mode the source
    /// view is first switched to the buffer the hit lives in.
    fn goto_current(&self) -> Option<Callback> {
        let m = self.matches.get(self.cursor)?;
        let (line, col) = (m.line_number, m.match_col);
        let doc_id = self
            .doc_ids
            .get(self.cursor)
            .copied()
            .unwrap_or(self.doc_id);
        let (view_id, multi) = (self.view_id, self.multi);
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                if multi {
                    cx.editor
                        .switch(doc_id, zemacs_view::editor::Action::Replace);
                }
                jump(cx.editor, doc_id, view_id, line, col);
            },
        ))
    }

    /// `g` (`revert-buffer`): re-run the regexp against the current buffer text,
    /// refreshing the hit list and clamping the cursor.
    fn rerun(&mut self, editor: &Editor) {
        let Some(text) = editor
            .documents()
            .find(|d| d.id() == self.doc_id)
            .map(|d| d.text().to_string())
        else {
            self.status = "occur: source buffer is gone".to_string();
            return;
        };
        let re = match regex::Regex::new(&self.pattern) {
            Ok(re) => re,
            Err(e) => {
                self.status = format!("occur: invalid regexp: {e}");
                return;
            }
        };
        self.matches = collect(&text, |line| {
            re.find(line).map(|hit| line[..hit.start()].chars().count())
        });
        self.cursor = self.cursor.min(self.matches.len().saturating_sub(1));
        self.status = format!("{} matches for {}", self.matches.len(), self.pattern);
    }
}

/// Set the source view's selection to the start of the match: the char at
/// (`line`, `col`) — `line` 1-based, `col` a 0-based character column — clamped
/// to the buffer.
fn jump(editor: &mut Editor, doc_id: DocumentId, view_id: ViewId, line: usize, col: usize) {
    let Some(doc) = editor.document_mut(doc_id) else {
        return;
    };
    let text = doc.text();
    let line_idx = line
        .saturating_sub(1)
        .min(text.len_lines().saturating_sub(1));
    let pos = (text.line_to_char(line_idx) + col).min(text.len_chars());
    doc.set_selection(view_id, zemacs_core::Selection::point(pos));
}

impl Component for Occur {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        self.status.clear();
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),

            // Motion (n/p and vim-style j/k, arrows).
            key!('n') | key!('j') | key!(Down) | ctrl!('n') => self.move_cursor(1),
            key!('p') | key!('k') | key!(Up) | ctrl!('p') => self.move_cursor(-1),
            key!('<') | key!(Home) => self.cursor = 0,
            key!('>') | key!(End) => self.cursor = self.matches.len().saturating_sub(1),

            // Visit the occurrence, closing the overlay (RET / o).
            key!(Enter) | key!('o') => {
                if let Some(cb) = self.goto_current() {
                    return EventResult::Consumed(Some(cb));
                }
            }
            // Display the occurrence in the source buffer, keeping the list open.
            ctrl!('o') => {
                if let Some(m) = self.matches.get(self.cursor) {
                    let (line, col) = (m.line_number, m.match_col);
                    let doc_id = self
                        .doc_ids
                        .get(self.cursor)
                        .copied()
                        .unwrap_or(self.doc_id);
                    if self.multi {
                        cx.editor
                            .switch(doc_id, zemacs_view::editor::Action::Replace);
                    }
                    jump(cx.editor, doc_id, self.view_id, line, col);
                }
            }

            // Re-run the search (single-buffer occur only).
            key!('g') => {
                if self.multi {
                    self.status = "multi-occur: re-run not available".to_string();
                } else {
                    self.rerun(cx.editor);
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
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 12 || area.height < 3 {
            return;
        }

        let total = self.matches.len();
        let title = format!(" *Occur*  {total} matches for \"{}\"", self.pattern);
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = "n/p move  RET goto  C-o show  g rerun  q quit";
        if title.len() + hint.len() + 3 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h as usize;

        if self.matches.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "(no matches)",
                area.width as usize,
                info_style,
            );
            return;
        }

        // Keep the cursor in view.
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.viewport > 0 && self.cursor >= self.scroll + self.viewport {
            self.scroll = self.cursor + 1 - self.viewport;
        }

        for (offset, m) in self
            .matches
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let style = if offset == self.cursor {
                sel_style
            } else {
                text_style
            };
            surface.set_stringn(
                area.x,
                y,
                &self.entry_line(offset, m),
                area.width as usize,
                style,
            );
        }

        // Footer: position, or the last status message.
        let footer = if self.status.is_empty() {
            format!("{}/{}", self.cursor + 1, total)
        } else {
            self.status.clone()
        };
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &footer,
            area.width as usize,
            info_style,
        );
    }
}
