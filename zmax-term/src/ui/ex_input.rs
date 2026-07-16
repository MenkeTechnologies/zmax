use tui::buffer::Buffer as Surface;
use zmax_core::{Selection, Tendril, Transaction};
use zmax_view::{
    graphics::{CursorKind, Rect},
    input::{KeyCode, KeyEvent},
    DocumentId, ViewId,
};

use crate::compositor::{Component, Compositor, Context, Event, EventResult};

/// vim `:append` / `:insert` / `:change` — the Ex line-input mode.
///
/// After the command the user types lines; a line containing only `.` ends input
/// and inserts everything as a single undo step. `:append` puts the lines after
/// the current line, `:insert` before it, and `:change` replaces it. `Esc` aborts
/// (nothing is inserted, and for `:change` nothing is deleted, since the delete is
/// part of the commit transaction). `Backspace` edits the current line.
pub struct ExInput {
    doc_id: DocumentId,
    view_id: ViewId,
    /// The span replaced on commit. Empty (`from == to`) for append/insert; the
    /// line being replaced for change.
    from: usize,
    to: usize,
    /// Prepend a newline to the inserted block — needed when appending after a
    /// final line that has no trailing newline, so the new lines start fresh.
    lead_newline: bool,
    label: &'static str,
    /// Confirmed lines (the terminating `.` is not included).
    lines: Vec<String>,
    /// The line currently being typed.
    current: String,
}

impl ExInput {
    pub fn new(
        doc_id: DocumentId,
        view_id: ViewId,
        from: usize,
        to: usize,
        lead_newline: bool,
        label: &'static str,
    ) -> Self {
        Self {
            doc_id,
            view_id,
            from,
            to,
            lead_newline,
            label,
            lines: Vec::new(),
            current: String::new(),
        }
    }

    fn pop() -> EventResult {
        EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, _| {
            compositor.pop();
        })))
    }

    /// Replace `[from, to)` with the collected lines as one transaction, then pop.
    fn commit(&mut self, cx: &mut Context) -> EventResult {
        // Nothing to do only when there are no lines AND no range to delete.
        if !self.lines.is_empty() || self.to > self.from {
            let mut text: String = self.lines.iter().map(|l| format!("{l}\n")).collect();
            if self.lead_newline && !text.is_empty() {
                text.insert(0, '\n');
            }
            let doc = doc_mut!(cx.editor, &self.doc_id);
            let tendril = (!text.is_empty()).then(|| Tendril::from(text.as_str()));
            let tx =
                Transaction::change(doc.text(), std::iter::once((self.from, self.to, tendril)));
            doc.apply(&tx, self.view_id);
            // Leave the cursor at the start of the last inserted line.
            let end = self.from + text.len();
            let cursor = end.saturating_sub(1).min(doc.text().len_chars());
            doc.set_selection(self.view_id, Selection::point(cursor));
            let view = view_mut!(cx.editor, self.view_id);
            let doc = doc_mut!(cx.editor, &self.doc_id);
            doc.append_changes_to_history(view);
        }
        Self::pop()
    }
}

impl Component for ExInput {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let KeyEvent { code, .. } = match event {
            Event::Key(k) => *k,
            _ => return EventResult::Ignored(None),
        };
        match code {
            KeyCode::Char(c) => self.current.push(c),
            KeyCode::Backspace => {
                self.current.pop();
            }
            KeyCode::Enter => {
                if self.current == "." {
                    return self.commit(cx);
                }
                self.lines.push(std::mem::take(&mut self.current));
            }
            // Abort: discard everything (and, for :change, leave the line intact).
            KeyCode::Esc => return Self::pop(),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        let style = cx.editor.theme.get("ui.statusline");
        let row = area.y + area.height.saturating_sub(1);
        let status = format!(
            "-- :{} (type lines, end with '.') [{} line(s)] -- {}",
            self.label,
            self.lines.len(),
            self.current
        );
        surface.set_string(area.x, row, &status, style);
    }

    fn cursor(
        &self,
        _area: Rect,
        _editor: &zmax_view::Editor,
    ) -> (Option<zmax_core::Position>, CursorKind) {
        (None, CursorKind::Hidden)
    }
}
