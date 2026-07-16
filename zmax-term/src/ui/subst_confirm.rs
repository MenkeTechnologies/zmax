use tui::buffer::Buffer as Surface;
use zmax_core::{Selection, Tendril, Transaction};
use zmax_view::{
    graphics::{CursorKind, Rect},
    input::{KeyCode, KeyEvent},
    DocumentId, ViewId,
};

use crate::compositor::{Component, Compositor, Context, Event, EventResult};

/// Interactive `:s/pat/rep/c` — vim's per-match confirmation prompt.
///
/// The full match list is precomputed in original document coordinates and no
/// edit is applied until the user finishes, so match spans never shift while
/// prompting. On finish, all accepted matches are committed in a single
/// transaction (one undo step, matching vim). Keys: `y` replace, `n` skip,
/// `a` replace this + all remaining, `l` replace this then stop, `q`/`Esc` stop.
pub struct SubstituteConfirm {
    doc_id: DocumentId,
    view_id: ViewId,
    /// `(start, end, replacement)` in original document coordinates, doc order.
    matches: Vec<(usize, usize, Tendril)>,
    idx: usize,
    accepted: Vec<usize>,
}

impl SubstituteConfirm {
    pub fn new(doc_id: DocumentId, view_id: ViewId, matches: Vec<(usize, usize, Tendril)>) -> Self {
        Self {
            doc_id,
            view_id,
            matches,
            idx: 0,
            accepted: Vec::new(),
        }
    }

    /// Commit the accepted substitutions as one transaction and pop this layer.
    fn finish(&mut self, cx: &mut Context) -> EventResult {
        if !self.accepted.is_empty() {
            let changes: Vec<_> = self
                .accepted
                .iter()
                .map(|&i| {
                    let (s, e, ref rep) = self.matches[i];
                    (s, e, Some(rep.clone()))
                })
                .collect();
            let doc = doc_mut!(cx.editor, &self.doc_id);
            let transaction = Transaction::change(doc.text(), changes.into_iter());
            doc.apply(&transaction, self.view_id);
            let view = view_mut!(cx.editor, self.view_id);
            let doc = doc_mut!(cx.editor, &self.doc_id);
            doc.append_changes_to_history(view);
        }
        EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, _| {
            compositor.pop();
        })))
    }
}

impl Component for SubstituteConfirm {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(k) => *k,
            _ => return EventResult::Ignored(None),
        };
        match key {
            KeyEvent {
                code: KeyCode::Char('y'),
                ..
            } => {
                self.accepted.push(self.idx);
                self.idx += 1;
            }
            KeyEvent {
                code: KeyCode::Char('n'),
                ..
            } => {
                self.idx += 1;
            }
            KeyEvent {
                code: KeyCode::Char('a'),
                ..
            } => {
                self.accepted.extend(self.idx..self.matches.len());
                return self.finish(cx);
            }
            KeyEvent {
                code: KeyCode::Char('l'),
                ..
            } => {
                self.accepted.push(self.idx);
                return self.finish(cx);
            }
            KeyEvent {
                code: KeyCode::Char('q'),
                ..
            }
            | KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                return self.finish(cx);
            }
            // Swallow any other key so it doesn't fall through to the editor.
            _ => return EventResult::Consumed(None),
        }
        if self.idx >= self.matches.len() {
            return self.finish(cx);
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        // Highlight the current match so the user sees what they are confirming.
        let (start, end, rep) = &self.matches[self.idx];
        let (start, end) = (*start, *end);
        let prompt = format!("replace with {}? (y)es (n)o (a)ll (l)ast (q)uit", rep);

        let scrolloff = cx.editor.config().scrolloff;
        let doc = doc_mut!(cx.editor, &self.doc_id);
        doc.set_selection(self.view_id, Selection::single(start, end));
        let view = view_mut!(cx.editor, self.view_id);
        let doc = doc_mut!(cx.editor, &self.doc_id);
        view.ensure_cursor_in_view(doc, scrolloff);

        let style = cx.editor.theme.get("ui.statusline");
        let row = area.y + area.height.saturating_sub(1);
        surface.set_string(area.x, row, &prompt, style);
    }

    fn cursor(
        &self,
        _area: Rect,
        _editor: &zmax_view::Editor,
    ) -> (Option<zmax_core::Position>, CursorKind) {
        (None, CursorKind::Hidden)
    }
}
