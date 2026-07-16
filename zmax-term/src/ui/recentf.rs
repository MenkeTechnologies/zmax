//! `recentf-edit-list` — the editable view of the recent-files list.
//!
//! Emacs pops up a dialog of the recent files with a checkbox per entry; you
//! tick the ones to forget and hit `Ok`, and they leave `recentf-list`. This is
//! the same thing as a terminal overlay over [`crate::recent_files`]:
//!
//!   n/p, j/k, arrows   move
//!   d / u / t          mark for deletion / unmark / toggle the mark
//!   x                  delete the marked entries from the store (and persist)
//!   RET                open the file at point
//!   q / Esc            leave

use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Component, Compositor, Context, Event, EventResult},
    key,
};

pub struct RecentfEdit {
    /// The store's entries, newest first, with their age at open time.
    entries: Vec<(PathBuf, u64)>,
    cursor: usize,
    /// Indices of `entries` marked for deletion.
    marked: Vec<usize>,
    status: String,
}

impl Default for RecentfEdit {
    fn default() -> Self {
        Self::new()
    }
}

impl RecentfEdit {
    pub fn new() -> Self {
        Self {
            entries: crate::recent_files::load_with_time(),
            cursor: 0,
            marked: Vec::new(),
            status: String::new(),
        }
    }

    fn toggle_mark(&mut self, on: Option<bool>) {
        if self.cursor >= self.entries.len() {
            return;
        }
        let at = self.marked.iter().position(|&i| i == self.cursor);
        let want = on.unwrap_or(at.is_none());
        match (want, at) {
            (true, None) => self.marked.push(self.cursor),
            (false, Some(i)) => {
                self.marked.remove(i);
            }
            _ => {}
        }
    }

    /// `x`: forget every marked file — remove it from the store on disk and from
    /// this view.
    fn execute(&mut self) {
        if self.marked.is_empty() {
            self.status = "no entries marked (d marks, x deletes)".to_string();
            return;
        }
        let doomed: Vec<PathBuf> = self
            .marked
            .iter()
            .filter_map(|&i| self.entries.get(i).map(|(p, _)| p.clone()))
            .collect();
        match crate::recent_files::remove(&doomed) {
            Ok(n) => {
                self.entries.retain(|(p, _)| !doomed.contains(p));
                self.marked.clear();
                self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
                self.status = format!("{n} entr{} removed", if n == 1 { "y" } else { "ies" });
            }
            Err(e) => self.status = format!("cannot write the recent-files store: {e}"),
        }
    }
}

impl Component for RecentfEdit {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        self.status.clear();
        let last = self.entries.len().saturating_sub(1);
        match key {
            key!('q') | key!(Esc) => {
                return EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, _| {
                    compositor.pop();
                })))
            }
            key!('n') | key!('j') | key!(Down) => self.cursor = (self.cursor + 1).min(last),
            key!('p') | key!('k') | key!(Up) => self.cursor = self.cursor.saturating_sub(1),
            key!('<') | key!(Home) => self.cursor = 0,
            key!('>') | key!(End) => self.cursor = last,
            key!('d') => {
                self.toggle_mark(Some(true));
                self.cursor = (self.cursor + 1).min(last);
            }
            key!('u') => {
                self.toggle_mark(Some(false));
                self.cursor = (self.cursor + 1).min(last);
            }
            key!('t') => self.toggle_mark(None),
            key!('x') => self.execute(),
            key!(Enter) => {
                if let Some((path, _)) = self.entries.get(self.cursor).cloned() {
                    return EventResult::Consumed(Some(Box::new(
                        move |compositor: &mut Compositor, cx: &mut Context| {
                            compositor.pop();
                            if let Err(e) =
                                cx.editor.open(&path, zmax_view::editor::Action::Replace)
                            {
                                cx.editor.set_error(format!("{}: {e}", path.display()));
                            }
                        },
                    )));
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let header = theme.get("ui.text.focus");
        let text = theme.get("ui.text");
        let info = theme.get("ui.linenr");
        let selected = theme.get("ui.selection");
        let marked = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 16 || area.height < 4 {
            return;
        }

        let title = format!(
            " RECENTF-EDIT-LIST  {} files  {} marked",
            self.entries.len(),
            self.marked.len()
        );
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header);
        let hint = "d mark  u unmark  x delete  RET open  q quit";
        if title.len() + hint.len() + 3 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info,
            );
        }

        let rows = area.height.saturating_sub(3) as usize;
        let top = self
            .cursor
            .saturating_sub(rows / 2)
            .min(self.entries.len().saturating_sub(rows));
        for (row, i) in (top..self.entries.len()).take(rows).enumerate() {
            let (path, time) = &self.entries[i];
            let is_marked = self.marked.contains(&i);
            let age = crate::recent_files::humanize_age(crate::recent_files::age_since(*time));
            let line = format!(
                "{} {:>4}  {}",
                if is_marked { 'D' } else { ' ' },
                age,
                path.display()
            );
            let style = if i == self.cursor {
                selected
            } else if is_marked {
                marked
            } else {
                text
            };
            surface.set_stringn(
                area.x,
                area.y + 2 + row as u16,
                &line,
                area.width as usize,
                style,
            );
        }

        if self.entries.is_empty() {
            surface.set_stringn(
                area.x,
                area.y + 2,
                "[the recent-files list is empty]",
                area.width as usize,
                info,
            );
        }
        if !self.status.is_empty() {
            surface.set_stringn(
                area.x,
                area.y + area.height - 1,
                &self.status,
                area.width as usize,
                info,
            );
        }
    }
}
