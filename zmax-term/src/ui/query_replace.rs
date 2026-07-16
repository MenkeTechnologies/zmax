//! `query-replace`, `query-replace-regexp` and `tags-query-replace` — the
//! per-match y/n/!/q loop.
//!
//! Emacs stops at every match, shows it, and asks. This is that loop as a
//! compositor layer: it owns the keyboard while it runs, so each answer is read
//! synchronously, and the editor underneath keeps rendering (the current match is
//! selected in the document, so it is highlighted and scrolled into view).
//!
//! Keys (`query-replace-map`):
//!
//!   SPC, y   replace this match and move to the next
//!   DEL, n   leave this one alone and move to the next
//!   ,        replace this one but stay on it
//!   .        replace this one and exit
//!   !        replace this and every remaining match in this file, no questions
//!   Y        …and in every remaining file too (tags-query-replace)
//!   ^        back up to the previous match
//!   u        undo the last replacement;  U  undo all of them
//!   RET, q, Esc   exit
//!   ?        show this list
//!
//! Answers are recorded, not applied: the match list is computed once, in
//! document coordinates, and the accepted matches are committed in a single
//! transaction when the file is done. Nothing shifts under the loop, `^`/`u`
//! are exact, and the whole run is one undo step (as in Emacs, where the
//! replacements of one query-replace undo together).
//!
//! `tags-query-replace` supplies a queue of files: when the matches in one file
//! run out, its replacements are committed and the next file in the queue is
//! opened and searched.

use std::collections::VecDeque;
use std::path::PathBuf;

use regex::Regex;
use tui::buffer::Buffer as Surface;
use zmax_core::{Selection, Tendril, Transaction};
use zmax_view::{
    editor::Action,
    graphics::{CursorKind, Rect},
    DocumentId, ViewId,
};

use crate::{
    compositor::{Component, Compositor, Context, Event, EventResult},
    key,
};

/// The matches of one document: `(start_char, end_char, replacement)`, in
/// document coordinates and document order.
pub type Matches = Vec<(usize, usize, Tendril)>;

/// What to replace, and with what — the pattern half of a query-replace run,
/// which stays fixed as the loop moves from file to file.
pub struct Search {
    /// The pattern as typed, for the prompt line.
    pub from: String,
    /// The replacement, in Emacs syntax (`\1`, `\&`), for the prompt line.
    pub to: String,
    pub re: Regex,
    /// Regexp mode (`query-replace-regexp`) expands back-references; literal mode
    /// (`query-replace`) inserts the replacement verbatim.
    pub regexp: bool,
}

impl Search {
    /// The matches of this search in `text`, restricted to `range` (Emacs
    /// searches from point to the end of the buffer, or over the active region).
    pub fn scan(&self, text: &zmax_core::Rope, range: std::ops::Range<usize>) -> Matches {
        let slice: String = text.slice(range.start..range.end).chars().collect();
        zmax_core::query_replace::matches(&slice, &self.re, &self.to, self.regexp)
            .into_iter()
            .map(|(s, e, rep)| (range.start + s, range.start + e, Tendril::from(rep)))
            .collect()
    }
}

/// Open `path` and scan the whole file. `None` when the file cannot be opened or
/// holds no match — the caller moves on to the next file, exactly as Emacs's tags
/// loop skips files with nothing in them.
pub fn open_and_scan(
    cx: &mut Context,
    path: &std::path::Path,
    search: &Search,
) -> Option<(DocumentId, ViewId, Matches)> {
    if let Err(e) = cx.editor.open(path, Action::Replace) {
        cx.editor.set_error(format!("{}: {e}", path.display()));
        return None;
    }
    let (view, doc) = current!(cx.editor);
    let end = doc.text().len_chars();
    let matches = search.scan(doc.text(), 0..end);
    (!matches.is_empty()).then(|| (doc.id(), view.id, matches))
}

pub struct QueryReplace {
    search: Search,

    doc_id: DocumentId,
    view_id: ViewId,
    /// Matches in the current document, in document order and coordinates.
    matches: Matches,
    /// The match being asked about.
    idx: usize,
    /// Indices of `matches` the user said yes to.
    accepted: Vec<usize>,

    /// Files still to visit (`tags-query-replace`); empty for a single buffer.
    files: VecDeque<PathBuf>,
    /// `Y`: no more questions, in this file or any later one.
    all_files: bool,
    /// Replacements committed in the files already finished.
    done_before: usize,
    /// `?` toggles the key list into the prompt area.
    help: bool,
}

impl QueryReplace {
    /// Start the loop over `matches` in the current document. `files` is the
    /// (possibly empty) queue of further files to visit when these run out.
    pub fn new(
        search: Search,
        doc_id: DocumentId,
        view_id: ViewId,
        matches: Matches,
        files: VecDeque<PathBuf>,
    ) -> Self {
        Self {
            search,
            doc_id,
            view_id,
            matches,
            idx: 0,
            accepted: Vec::new(),
            files,
            all_files: false,
            done_before: 0,
            help: false,
        }
    }

    /// Apply this file's accepted replacements as one transaction.
    fn commit(&mut self, cx: &mut Context) -> usize {
        if self.accepted.is_empty() {
            return 0;
        }
        self.accepted.sort_unstable();
        let changes: Vec<_> = self
            .accepted
            .iter()
            .map(|&i| {
                let (s, e, ref rep) = self.matches[i];
                (s, e, Some(rep.clone()))
            })
            .collect();
        let n = changes.len();
        let doc = doc_mut!(cx.editor, &self.doc_id);
        let transaction = Transaction::change(doc.text(), changes.into_iter());
        doc.apply(&transaction, self.view_id);
        let view = view_mut!(cx.editor, self.view_id);
        let doc = doc_mut!(cx.editor, &self.doc_id);
        doc.append_changes_to_history(view);
        self.accepted.clear();
        n
    }

    /// This file is done: commit it, then move to the next file that has a match.
    /// Returns `Consumed`, either continuing in the next file or closing the loop.
    fn next_file(&mut self, cx: &mut Context) -> EventResult {
        self.done_before += self.commit(cx);

        while let Some(path) = self.files.pop_front() {
            let Some((doc_id, view_id, matches)) = open_and_scan(cx, &path, &self.search) else {
                continue;
            };
            self.doc_id = doc_id;
            self.view_id = view_id;
            self.matches = matches;
            self.idx = 0;
            if self.all_files {
                // `Y`: take every match in this file too, then keep going.
                self.accepted.extend(0..self.matches.len());
                self.done_before += self.commit(cx);
                continue;
            }
            return EventResult::Consumed(None);
        }

        self.finish(cx)
    }

    /// Close the loop, reporting how many replacements were made.
    fn finish(&mut self, cx: &mut Context) -> EventResult {
        let n = self.done_before + self.commit(cx);
        cx.editor.set_status(format!(
            "Replaced {n} occurrence{}",
            if n == 1 { "" } else { "s" }
        ));
        EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, _| {
            compositor.pop();
        })))
    }

    /// Answer `y` on the current match without moving.
    fn accept_current(&mut self) {
        if !self.accepted.contains(&self.idx) {
            self.accepted.push(self.idx);
        }
    }

    /// The `?` key list, shown in place of the prompt.
    const HELP: &'static str =
        "SPC/y replace  DEL/n skip  , replace-stay  . replace-quit  ! rest-of-file  \
         Y all-files  ^ back  u undo  U undo-all  RET/q quit";
}

impl Component for QueryReplace {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(k) => *k,
            _ => return EventResult::Ignored(None),
        };
        if self.help {
            // Any key dismisses the key list; it is not an answer.
            self.help = false;
            return EventResult::Consumed(None);
        }
        match key {
            key!('?') => {
                self.help = true;
                return EventResult::Consumed(None);
            }
            // Replace and advance.
            key!(' ') | key!('y') => {
                self.accept_current();
                self.idx += 1;
            }
            // Skip and advance.
            key!(Backspace) | key!(Delete) | key!('n') => self.idx += 1,
            // Replace but stay on this match (Emacs `,`), so you can see the result
            // before deciding to move on.
            key!(',') => self.accept_current(),
            // Replace this one and stop.
            key!('.') => {
                self.accept_current();
                return self.finish(cx);
            }
            // Every remaining match in this file, no questions.
            key!('!') => {
                self.accepted.extend(self.idx..self.matches.len());
                self.idx = self.matches.len();
                return self.next_file(cx);
            }
            // …and in every file still queued (tags-query-replace).
            key!('Y') => {
                self.all_files = true;
                self.accepted.extend(self.idx..self.matches.len());
                self.idx = self.matches.len();
                return self.next_file(cx);
            }
            // Back up to the previous match.
            key!('^') => self.idx = self.idx.saturating_sub(1),
            // Undo the last replacement: un-accept it and go back to it.
            key!('u') => {
                if let Some(last) = self.accepted.pop() {
                    self.idx = last;
                }
            }
            // Undo every replacement made in this file and start over.
            key!('U') => {
                self.accepted.clear();
                self.idx = 0;
            }
            key!(Enter) | key!('q') | key!(Esc) => return self.finish(cx),
            // Anything else is not an answer: swallow it rather than let it reach
            // the buffer underneath, where it would edit the text being replaced.
            _ => return EventResult::Consumed(None),
        }

        if self.idx >= self.matches.len() {
            return self.next_file(cx);
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        let Some(&(start, end, _)) = self.matches.get(self.idx) else {
            return;
        };

        // Select the match in the document so it is highlighted and scrolled in.
        let scrolloff = cx.editor.config().scrolloff;
        let doc = doc_mut!(cx.editor, &self.doc_id);
        doc.set_selection(self.view_id, Selection::single(start, end));
        let view = view_mut!(cx.editor, self.view_id);
        let doc = doc_mut!(cx.editor, &self.doc_id);
        view.ensure_cursor_in_view(doc, scrolloff);

        let line = if self.help {
            Self::HELP.to_string()
        } else {
            let left = self.matches.len() - self.idx;
            let files = if self.files.is_empty() {
                String::new()
            } else {
                format!(", {} file(s) left", self.files.len())
            };
            format!(
                "Query replacing {} with {} ({left} left{files}): (? for help) ",
                self.search.from, self.search.to
            )
        };
        let style = cx.editor.theme.get("ui.statusline");
        let row = area.y + area.height.saturating_sub(1);
        surface.set_string(area.x, row, &line, style);
    }

    fn cursor(
        &self,
        _area: Rect,
        _editor: &zmax_view::Editor,
    ) -> (Option<zmax_core::Position>, CursorKind) {
        (None, CursorKind::Hidden)
    }
}
