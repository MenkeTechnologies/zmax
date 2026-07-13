//! Kmacro menu — the zemacs port of GNU Emacs `kmacro-menu`
//! (`M-x list-keyboard-macros`): a list of the keyboard-macro ring where each
//! macro can be marked, flagged for deletion, copied, transposed, and have its
//! keys, counter, format and ring position edited.
//!
//! The list *is* the ring: every operation rewrites
//! `commands::macro_ring_set`, so a macro deleted or edited here is the macro
//! `C-x e` replays afterwards. The ring head is the "last kbd macro".
//!
//! Keys: `j`/`k` (`n`/`p`) move · `m` mark · `u` unmark · `DEL` unmark backwards ·
//! `U` unmark all · `d` flag for deletion · `x` delete flagged · `D` delete
//! marked · `C` copy · `t` transpose with previous · `e`/`RET` edit the keys ·
//! `c` edit counter · `f` edit format · `P` edit position · `q`/`Esc` close.

use std::collections::BTreeSet;

use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::Rect,
    input::{KeyCode, KeyEvent},
};

use crate::{
    commands::{macro_ring_entries, macro_ring_set, KmacroEntry},
    compositor::{Component, Compositor, Context, Event, EventResult},
};

/// The column an in-place edit is changing (Emacs's `kmacro-menu-edit-*`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Keys,
    Counter,
    Format,
    Position,
}

impl Column {
    fn label(self) -> &'static str {
        match self {
            Column::Keys => "Keys",
            Column::Counter => "Counter",
            Column::Format => "Format",
            Column::Position => "Position",
        }
    }
}

/// The active in-place edit: which column, and the text typed so far.
struct Edit {
    column: Column,
    row: usize,
    buffer: String,
}

pub struct KmacroMenu {
    entries: Vec<KmacroEntry>,
    /// Rows marked with `*` (the macros `kmacro-menu-do-*` act on).
    marked: BTreeSet<usize>,
    /// Rows flagged `D` for deletion by `kmacro-menu-flag-for-deletion`.
    flagged: BTreeSet<usize>,
    selected: usize,
    scroll: usize,
    edit: Option<Edit>,
    /// The last operation's report, shown in the footer.
    message: Option<String>,
}

impl Default for KmacroMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl KmacroMenu {
    pub fn new() -> Self {
        KmacroMenu {
            entries: macro_ring_entries(),
            marked: BTreeSet::new(),
            flagged: BTreeSet::new(),
            selected: 0,
            scroll: 0,
            edit: None,
            message: None,
        }
    }

    /// How many macros the menu is listing.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Write the (possibly reordered / edited) list back to the macro ring — the
    /// menu is a view *of* the ring, so every operation ends here.
    fn commit(&mut self) {
        macro_ring_set(self.entries.clone());
    }

    /// A structural change (delete/copy/transpose) invalidates row-indexed marks,
    /// so they are dropped, exactly as Emacs's tabulated-list revert drops them.
    fn clear_marks(&mut self) {
        self.marked.clear();
        self.flagged.clear();
    }

    fn clamp(&mut self) {
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// The rows an operation acts on: the marked ones, or the row at point when
    /// nothing is marked (Emacs's `kmacro-menu` convention).
    fn targets(&self) -> Vec<usize> {
        if self.marked.is_empty() {
            if self.entries.is_empty() {
                Vec::new()
            } else {
                vec![self.selected]
            }
        } else {
            self.marked.iter().copied().collect()
        }
    }

    // ── the kmacro-menu-* operations ────────────────────────────────────────

    /// `kmacro-menu-mark` (`m`): mark the macro at point and move down.
    pub fn mark(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.marked.insert(self.selected);
        self.move_down();
        true
    }

    /// `kmacro-menu-unmark` (`u`): remove the mark/flag at point, move down.
    pub fn unmark(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let had = self.marked.remove(&self.selected) | self.flagged.remove(&self.selected);
        self.move_down();
        had
    }

    /// `kmacro-menu-unmark-backward` (`DEL`): move up, then unmark there.
    pub fn unmark_backward(&mut self) -> bool {
        if self.entries.is_empty() || self.selected == 0 {
            return false;
        }
        self.move_up();
        self.marked.remove(&self.selected) | self.flagged.remove(&self.selected)
    }

    /// `kmacro-menu-unmark-all` (`U`): remove every mark and deletion flag.
    pub fn unmark_all(&mut self) -> usize {
        let n = self.marked.len() + self.flagged.len();
        self.clear_marks();
        n
    }

    /// `kmacro-menu-flag-for-deletion` (`d`): flag the macro at point `D`.
    pub fn flag_for_deletion(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.flagged.insert(self.selected);
        self.move_down();
        true
    }

    /// Remove `rows` from the list and write the ring back. Returns how many.
    fn delete_rows(&mut self, rows: &[usize]) -> usize {
        if rows.is_empty() {
            return 0;
        }
        let doomed: BTreeSet<usize> = rows.iter().copied().collect();
        let mut kept = Vec::with_capacity(self.entries.len());
        for (i, e) in self.entries.iter().enumerate() {
            if !doomed.contains(&i) {
                kept.push(e.clone());
            }
        }
        let removed = self.entries.len() - kept.len();
        self.entries = kept;
        self.clear_marks();
        self.clamp();
        self.commit();
        removed
    }

    /// `kmacro-menu-do-flagged-delete` (`x`): delete the flagged macros.
    pub fn do_flagged_delete(&mut self) -> usize {
        let rows: Vec<usize> = self.flagged.iter().copied().collect();
        self.delete_rows(&rows)
    }

    /// `kmacro-menu-do-delete` (`D`): delete the marked macros (or the one at
    /// point when none are marked).
    pub fn do_delete(&mut self) -> usize {
        let rows = self.targets();
        self.delete_rows(&rows)
    }

    /// `kmacro-menu-do-copy` (`C`): duplicate the marked macros (or the one at
    /// point), each copy inserted right after its original.
    pub fn do_copy(&mut self) -> usize {
        let rows = self.targets();
        if rows.is_empty() {
            return 0;
        }
        let mut out: Vec<KmacroEntry> = Vec::with_capacity(self.entries.len() + rows.len());
        let copy: BTreeSet<usize> = rows.iter().copied().collect();
        for (i, e) in self.entries.iter().enumerate() {
            out.push(e.clone());
            if copy.contains(&i) {
                out.push(e.clone());
            }
        }
        self.entries = out;
        self.clear_marks();
        self.commit();
        copy.len()
    }

    /// `kmacro-menu-transpose` (`t`): transpose the macro at point with the one
    /// before it (and follow it up, as Emacs's transpose commands do).
    pub fn transpose(&mut self) -> bool {
        if self.selected == 0 || self.entries.len() < 2 {
            return false;
        }
        self.entries.swap(self.selected - 1, self.selected);
        self.selected -= 1;
        self.clear_marks();
        self.commit();
        true
    }

    /// Begin editing `column` of the macro at point (`kmacro-menu-edit-keys` /
    /// `-edit-counter` / `-edit-format` / `-edit-position`, and `-edit-column`
    /// for whichever column is under the cursor). `false` when the list is empty.
    pub fn begin_edit(&mut self, column: Column) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let e = &self.entries[self.selected];
        let buffer = match column {
            Column::Keys => e.keys.clone(),
            Column::Counter => e.counter.to_string(),
            Column::Format => e.format.clone(),
            Column::Position => (self.selected + 1).to_string(),
        };
        self.edit = Some(Edit {
            column,
            row: self.selected,
            buffer,
        });
        true
    }

    /// Whether an in-place edit is open (the component is then eating keys).
    pub fn editing(&self) -> Option<Column> {
        self.edit.as_ref().map(|e| e.column)
    }

    /// Apply the open edit. `Err` describes why it was rejected (an unparsable
    /// macro, a non-numeric counter, an out-of-range position).
    fn commit_edit(&mut self) -> Result<String, String> {
        let Some(edit) = self.edit.take() else {
            return Err("no edit in progress".to_string());
        };
        let row = edit.row.min(self.entries.len().saturating_sub(1));
        if self.entries.is_empty() {
            return Err("no macros".to_string());
        }
        let text = edit.buffer.trim().to_string();
        match edit.column {
            Column::Keys => {
                if text.is_empty() {
                    return Err("a macro cannot be empty".to_string());
                }
                // The keys have to stay replayable, or the ring holds a macro that
                // can never run — reject the edit instead.
                zemacs_view::input::parse_macro(&text)
                    .map_err(|e| format!("invalid macro: {e}"))?;
                self.entries[row].keys = text.clone();
                self.commit();
                Ok(format!("keys → {text}"))
            }
            Column::Counter => {
                let n: i64 = text
                    .parse()
                    .map_err(|_| format!("`{text}` is not a number"))?;
                self.entries[row].counter = n;
                self.commit();
                Ok(format!("counter → {n}"))
            }
            Column::Format => {
                let fmt = if text.is_empty() {
                    "%d".to_string()
                } else {
                    text
                };
                self.entries[row].format = fmt.clone();
                self.commit();
                Ok(format!("format → {fmt}"))
            }
            Column::Position => {
                let pos: usize = text
                    .parse()
                    .map_err(|_| format!("`{text}` is not a number"))?;
                if pos == 0 || pos > self.entries.len() {
                    return Err(format!("position must be 1..{}", self.entries.len()));
                }
                let entry = self.entries.remove(row);
                self.entries.insert(pos - 1, entry);
                self.selected = pos - 1;
                self.clear_marks();
                self.commit();
                Ok(format!("moved to position {pos}"))
            }
        }
    }

    /// Feed a key to the open edit. Returns `true` while the edit is consuming
    /// keys.
    fn handle_edit_key(&mut self, key: KeyEvent) -> bool {
        if self.edit.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.edit = None;
                self.message = Some("edit cancelled".to_string());
            }
            KeyCode::Enter => match self.commit_edit() {
                Ok(msg) => self.message = Some(msg),
                Err(err) => self.message = Some(err),
            },
            KeyCode::Backspace => {
                if let Some(e) = self.edit.as_mut() {
                    e.buffer.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(e) = self.edit.as_mut() {
                    e.buffer.push(c);
                }
            }
            _ => {}
        }
        true
    }

    /// The column a `kmacro-menu-edit-column` should edit: the one the cursor's
    /// x position falls in. The menu has no horizontal cursor, so the keys column
    /// (the one that defines the macro) is the column at point.
    pub fn column_at_point(&self) -> Column {
        Column::Keys
    }

    /// Set the footer message (so a command can report what it did in-place).
    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
    }

    /// Refresh from the ring — a macro recorded while the menu is open shows up.
    pub fn refresh(&mut self) {
        self.entries = macro_ring_entries();
        self.clear_marks();
        self.clamp();
    }
}

impl Component for KmacroMenu {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        let key = *key;
        if self.handle_edit_key(key) {
            return EventResult::Consumed(None);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                return EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                    c.pop();
                })))
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('n') => self.move_down(),
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('p') => self.move_up(),
            KeyCode::Char('m') => {
                self.mark();
            }
            KeyCode::Char('u') => {
                self.unmark();
            }
            KeyCode::Backspace => {
                self.unmark_backward();
            }
            KeyCode::Char('U') => {
                let n = self.unmark_all();
                self.message = Some(format!("{n} mark(s) removed"));
            }
            KeyCode::Char('d') => {
                self.flag_for_deletion();
            }
            KeyCode::Char('x') => {
                let n = self.do_flagged_delete();
                self.message = Some(format!("{n} macro(s) deleted"));
            }
            KeyCode::Char('D') => {
                let n = self.do_delete();
                self.message = Some(format!("{n} macro(s) deleted"));
            }
            KeyCode::Char('C') => {
                let n = self.do_copy();
                self.message = Some(format!("{n} macro(s) copied"));
            }
            KeyCode::Char('t') => {
                if !self.transpose() {
                    self.message = Some("nothing to transpose with".to_string());
                }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                let col = self.column_at_point();
                self.begin_edit(col);
            }
            KeyCode::Char('c') => {
                self.begin_edit(Column::Counter);
            }
            KeyCode::Char('f') => {
                self.begin_edit(Column::Format);
            }
            KeyCode::Char('P') => {
                self.begin_edit(Column::Position);
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let header = theme.get("ui.text.focus");
        let text = theme.get("ui.text");
        let dim = theme.get("ui.linenr");
        let sel = theme.get("ui.selection");
        let mark_style = theme.get("warning");

        surface.clear_with(area, bg);
        if area.width < 30 || area.height < 5 {
            return;
        }
        surface.set_stringn(
            area.x,
            area.y,
            " Keyboard macros (ring)",
            area.width as usize,
            header,
        );
        surface.set_stringn(
            area.x,
            area.y + 1,
            "   # Keys                                     Counter  Format",
            area.width as usize,
            dim,
        );

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(4) as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if body_h > 0 && self.selected >= self.scroll + body_h {
            self.scroll = self.selected + 1 - body_h;
        }
        for (row, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h)
        {
            let y = body_y + (row - self.scroll) as u16;
            let flag = if self.flagged.contains(&row) {
                'D'
            } else if self.marked.contains(&row) {
                '*'
            } else {
                ' '
            };
            let keys: String = if entry.keys.chars().count() > 40 {
                format!("{}…", entry.keys.chars().take(39).collect::<String>())
            } else {
                entry.keys.clone()
            };
            let line = format!(
                "{flag} {:>2} {:<41}{:>7}  {}",
                row + 1,
                keys,
                entry.counter,
                entry.format
            );
            let style = if row == self.selected {
                sel
            } else if flag != ' ' {
                mark_style
            } else {
                text
            };
            surface.set_stringn(area.x, y, &line, area.width as usize, style);
        }

        let last = area.y + area.height - 1;
        if let Some(edit) = &self.edit {
            let line = format!("{}: {}_", edit.column.label(), edit.buffer);
            surface.set_stringn(area.x, last, &line, area.width as usize, header);
        } else if let Some(msg) = &self.message {
            surface.set_stringn(area.x, last, msg, area.width as usize, header);
        } else {
            surface.set_stringn(
                area.x,
                last,
                " m mark · u unmark · U unmark all · d flag · x delete flagged · D delete · C copy · t transpose · e/c/f/P edit · q close",
                area.width as usize,
                dim,
            );
        }
        if self.entries.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "  (no keyboard macros recorded yet)",
                area.width as usize,
                dim,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(keys: &str, counter: i64) -> KmacroEntry {
        KmacroEntry {
            keys: keys.to_string(),
            counter,
            format: "%d".to_string(),
        }
    }

    /// A menu over a fixed list, without touching the process-global ring (the
    /// operations under test are pure list edits; `commit` is what writes back).
    fn menu(entries: Vec<KmacroEntry>) -> KmacroMenu {
        let mut m = KmacroMenu::new();
        m.entries = entries;
        m.selected = 0;
        m.marked.clear();
        m.flagged.clear();
        m
    }

    #[test]
    fn marking_moves_down_and_unmark_backward_returns() {
        let mut m = menu(vec![entry("a", 0), entry("b", 0), entry("c", 0)]);
        assert!(m.mark()); // marks row 0, point → 1
        assert_eq!(m.selected, 1);
        assert!(m.mark()); // marks row 1, point → 2
        assert_eq!(m.marked.iter().copied().collect::<Vec<_>>(), vec![0, 1]);

        // DEL steps back onto row 1 and clears its mark.
        assert!(m.unmark_backward());
        assert_eq!(m.selected, 1);
        assert_eq!(m.marked.iter().copied().collect::<Vec<_>>(), vec![0]);

        assert_eq!(m.unmark_all(), 1);
        assert!(m.marked.is_empty());
    }

    #[test]
    fn flagged_delete_removes_only_the_flagged_rows() {
        let mut m = menu(vec![entry("a", 0), entry("b", 0), entry("c", 0)]);
        m.selected = 1;
        m.flag_for_deletion(); // flags row 1 ("b")
        assert_eq!(m.do_flagged_delete(), 1);
        let keys: Vec<&str> = m.entries.iter().map(|e| e.keys.as_str()).collect();
        assert_eq!(keys, vec!["a", "c"]);
        // Marks never survive a structural change.
        assert!(m.flagged.is_empty());
    }

    #[test]
    fn delete_and_copy_act_on_point_when_nothing_is_marked() {
        let mut m = menu(vec![entry("a", 0), entry("b", 0)]);
        m.selected = 1;
        assert_eq!(m.do_copy(), 1);
        let keys: Vec<&str> = m.entries.iter().map(|e| e.keys.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "b"], "the copy follows its original");

        m.selected = 0;
        assert_eq!(m.do_delete(), 1);
        let keys: Vec<&str> = m.entries.iter().map(|e| e.keys.as_str()).collect();
        assert_eq!(keys, vec!["b", "b"]);
    }

    #[test]
    fn transpose_swaps_with_the_previous_macro() {
        let mut m = menu(vec![entry("a", 0), entry("b", 0)]);
        assert!(!m.transpose(), "nothing before the first row");
        m.selected = 1;
        assert!(m.transpose());
        let keys: Vec<&str> = m.entries.iter().map(|e| e.keys.as_str()).collect();
        assert_eq!(keys, vec!["b", "a"]);
        assert_eq!(m.selected, 0, "point follows the macro it moved");
    }

    #[test]
    fn edits_are_validated_before_they_reach_the_ring() {
        let mut m = menu(vec![entry("ihi<esc>", 3)]);

        // A macro that cannot be parsed back is rejected, and the old keys stay.
        m.begin_edit(Column::Keys);
        m.edit.as_mut().unwrap().buffer = "<not-a-key>".to_string();
        assert!(m.commit_edit().is_err());
        assert_eq!(m.entries[0].keys, "ihi<esc>");

        // A counter has to be a number.
        m.begin_edit(Column::Counter);
        m.edit.as_mut().unwrap().buffer = "nine".to_string();
        assert!(m.commit_edit().is_err());
        assert_eq!(m.entries[0].counter, 3);

        // A good counter lands.
        m.begin_edit(Column::Counter);
        m.edit.as_mut().unwrap().buffer = "42".to_string();
        assert!(m.commit_edit().is_ok());
        assert_eq!(m.entries[0].counter, 42);

        // An empty format falls back to Emacs's default.
        m.begin_edit(Column::Format);
        m.edit.as_mut().unwrap().buffer = String::new();
        assert!(m.commit_edit().is_ok());
        assert_eq!(m.entries[0].format, "%d");
    }

    #[test]
    fn edit_position_moves_the_macro_in_the_ring() {
        let mut m = menu(vec![entry("a", 0), entry("b", 0), entry("c", 0)]);
        m.selected = 2; // "c"
        m.begin_edit(Column::Position);
        m.edit.as_mut().unwrap().buffer = "1".to_string();
        assert!(m.commit_edit().is_ok());
        let keys: Vec<&str> = m.entries.iter().map(|e| e.keys.as_str()).collect();
        assert_eq!(keys, vec!["c", "a", "b"]);
        assert_eq!(m.selected, 0);

        // Out of range is rejected.
        m.begin_edit(Column::Position);
        m.edit.as_mut().unwrap().buffer = "9".to_string();
        assert!(m.commit_edit().is_err());
    }
}
