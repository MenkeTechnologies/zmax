//! Buffer Menu — the zemacs port of GNU Emacs `buffer-menu` / `list-buffers`
//! (`C-x C-b`, `Buffer-menu-mode`).
//!
//! This is the pure, dependency-free state machine behind the full-screen Buffer
//! Menu Component. It owns an ordered snapshot of the open buffers (one
//! [`BufferRow`] each) plus the per-buffer marks (`>` select, `D` delete, `S`
//! save) and the cursor position. All editor interaction — enumerating
//! documents, switching, saving, killing — lives in the interactive
//! `zemacs-term` layer; everything here is testable without an `Editor`.
//!
//! Marks are keyed by an opaque, stable buffer id (the numeric `DocumentId`) so
//! they survive a refresh that rebuilds the row list, exactly like Emacs's Buffer
//! Menu keeps its `D`/`S`/`>` flags across `revert-buffer` (`g`). The mark column
//! is the single-glyph leftmost column, mirroring Emacs's `tabulated-list` tag
//! area; the `C R M` columns (current `.`, read-only `%`, modified `*`) follow.

use std::collections::BTreeMap;

/// Whether a buffer name is an Emacs "internal" buffer — one that
/// `list-buffers` hides by default and `Buffer-menu-toggle-internal` reveals.
/// Emacs treats a buffer as internal when its name starts with a space, and
/// also groups the `*…*` "special" buffers (`*scratch*`, `*Messages*`) under
/// the same toggle. Pure predicate so the filter is unit-testable.
pub fn is_internal_name(name: &str) -> bool {
    name.starts_with(' ') || (name.len() >= 2 && name.starts_with('*') && name.ends_with('*'))
}

/// A per-row mark in the Buffer Menu's leftmost column. At most one is visible on
/// a line, matching Emacs's single tag column: setting a new mark replaces any
/// previous one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mark {
    /// No mark (a blank tag column).
    #[default]
    None,
    /// `>` — marked for display/selection (`m`, `Buffer-menu-mark`; acted on by
    /// `v`, `Buffer-menu-select`).
    Select,
    /// `D` — flagged for deletion (`d`/`C-d`/`k`, `Buffer-menu-delete`; killed by
    /// `x`, `Buffer-menu-execute`).
    Delete,
    /// `S` — flagged to be saved (`s`, `Buffer-menu-save`; written by `x`).
    Save,
}

impl Mark {
    /// The glyph shown in the mark column.
    pub fn glyph(self) -> char {
        match self {
            Mark::None => ' ',
            Mark::Select => '>',
            Mark::Delete => 'D',
            Mark::Save => 'S',
        }
    }
}

/// One buffer's display data, snapshotted from the editor. The interactive layer
/// maps `key` back to the live `DocumentId` when a key acts on the row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferRow {
    /// Opaque, stable buffer id (the numeric `DocumentId`). Used as the mark key.
    pub key: u64,
    /// Buffer name (Emacs's "Buffer" column) — usually the relative file name.
    pub name: String,
    /// Size in characters (Emacs's "Size" column).
    pub size: usize,
    /// Major-mode / language name (Emacs's "Mode" column).
    pub mode: String,
    /// Full file path, or empty for a non-file buffer (Emacs's "File" column).
    pub file: String,
    /// Whether this is the current buffer (`C` column, shown as `.`).
    pub current: bool,
    /// Whether the buffer is read-only (`R` column, shown as `%`).
    pub readonly: bool,
    /// Whether the buffer has unsaved changes (`M` column, shown as `*`).
    pub modified: bool,
}

/// The Buffer Menu model: the ordered rows, the marks keyed by buffer id, and the
/// cursor (point) row.
#[derive(Clone, Debug, Default)]
pub struct BufferMenu {
    rows: Vec<BufferRow>,
    /// Marks keyed by [`BufferRow::key`], so they survive a refresh.
    marks: BTreeMap<u64, Mark>,
    selected: usize,
}

impl BufferMenu {
    /// Build a menu over `rows`, cursor on the first row.
    pub fn new(rows: Vec<BufferRow>) -> Self {
        BufferMenu {
            rows,
            marks: BTreeMap::new(),
            selected: 0,
        }
    }

    /// Refresh (`g`, `revert-buffer`): replace the rows with a fresh snapshot,
    /// dropping marks whose buffer is gone and clamping the cursor into range.
    pub fn set_rows(&mut self, rows: Vec<BufferRow>) {
        self.rows = rows;
        let present: std::collections::BTreeSet<u64> = self.rows.iter().map(|r| r.key).collect();
        self.marks.retain(|k, _| present.contains(k));
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    pub fn rows(&self) -> &[BufferRow] {
        &self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The cursor row index (0-based).
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The row under point, if any.
    pub fn current_row(&self) -> Option<&BufferRow> {
        self.rows.get(self.selected)
    }

    /// The buffer id under point, if any.
    pub fn current_key(&self) -> Option<u64> {
        self.current_row().map(|r| r.key)
    }

    /// Move point by `delta` rows, clamped to the list bounds.
    pub fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let max = self.rows.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Point to the first / last row.
    pub fn goto_first(&mut self) {
        self.selected = 0;
    }

    pub fn goto_last(&mut self) {
        self.selected = self.rows.len().saturating_sub(1);
    }

    /// The mark on a given buffer id (`Mark::None` when unmarked).
    pub fn mark_of(&self, key: u64) -> Mark {
        self.marks.get(&key).copied().unwrap_or(Mark::None)
    }

    /// The mark on the row under point.
    pub fn current_mark(&self) -> Mark {
        self.current_key()
            .map(|k| self.mark_of(k))
            .unwrap_or(Mark::None)
    }

    /// Set (or clear, for `Mark::None`) the mark on a buffer id.
    pub fn set_mark(&mut self, key: u64, mark: Mark) {
        if mark == Mark::None {
            self.marks.remove(&key);
        } else {
            self.marks.insert(key, mark);
        }
    }

    /// Mark the row under point and advance one row (`m`/`d`/`s` all advance in
    /// Emacs's Buffer Menu).
    pub fn mark_current(&mut self, mark: Mark) {
        if let Some(key) = self.current_key() {
            self.set_mark(key, mark);
            self.move_selection(1);
        }
    }

    /// Unmark the row under point and advance (`u`, `Buffer-menu-unmark`).
    pub fn unmark_current(&mut self) {
        if let Some(key) = self.current_key() {
            self.marks.remove(&key);
            self.move_selection(1);
        }
    }

    /// Move up one row and unmark it (`DEL`, `Buffer-menu-backup-unmark`).
    pub fn backup_unmark(&mut self) {
        self.move_selection(-1);
        if let Some(key) = self.current_key() {
            self.marks.remove(&key);
        }
    }

    /// Remove every mark (`U`/`M-DEL`, `Buffer-menu-unmark-all-buffers`).
    pub fn unmark_all(&mut self) {
        self.marks.clear();
    }

    /// Bury the buffer under point (`b`, `Buffer-menu-bury`): move its row to the
    /// bottom of the list, leaving point on the same index so it now shows the
    /// following buffer (matching Emacs, which sinks the current line and advances
    /// to the next). Marks are keyed by buffer id, so the buried row keeps its
    /// mark. Returns the buried buffer id, if any.
    pub fn bury_current(&mut self) -> Option<u64> {
        if self.selected >= self.rows.len() {
            return None;
        }
        let row = self.rows.remove(self.selected);
        let key = row.key;
        self.rows.push(row);
        // Point stays at the same index (now the next buffer), clamped.
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
        Some(key)
    }

    /// Whether any row carries a mark.
    pub fn has_marks(&self) -> bool {
        !self.marks.is_empty()
    }

    /// The buffer ids flagged with `mark`, in row (display) order.
    pub fn flagged(&self, mark: Mark) -> Vec<u64> {
        self.rows
            .iter()
            .filter(|r| self.mark_of(r.key) == mark)
            .map(|r| r.key)
            .collect()
    }

    /// Width for the Buffer-name column: the widest name, floored at the header
    /// label width so `Buffer` always fits.
    pub fn name_width(&self) -> usize {
        self.rows
            .iter()
            .map(|r| r.name.chars().count())
            .max()
            .unwrap_or(0)
            .max("Buffer".len())
    }

    /// The `C R M` status triple for a row (current `.`, read-only `%`,
    /// modified `*`; a space where the flag is off).
    pub fn crm(&self, row: &BufferRow) -> String {
        let c = if row.current { '.' } else { ' ' };
        let r = if row.readonly { '%' } else { ' ' };
        let m = if row.modified { '*' } else { ' ' };
        format!("{c}{r}{m}")
    }

    /// Render one row: `<mark><C><R><M> <name>  <size>  <mode>  <file>`, with the
    /// name padded to `name_w`. Matches the Emacs Buffer Menu column order.
    pub fn format_line(&self, row: &BufferRow, name_w: usize) -> String {
        let mark = self.mark_of(row.key).glyph();
        format!(
            "{}{} {:<name_w$}  {:>8}  {:<12}  {}",
            mark,
            self.crm(row),
            row.name,
            row.size,
            row.mode,
            row.file,
            name_w = name_w,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: u64, name: &str) -> BufferRow {
        BufferRow {
            key,
            name: name.to_string(),
            size: 10 * key as usize,
            mode: "Text".to_string(),
            file: format!("/tmp/{name}"),
            current: false,
            readonly: false,
            modified: false,
        }
    }

    fn menu(names: &[(u64, &str)]) -> BufferMenu {
        BufferMenu::new(names.iter().map(|(k, n)| row(*k, n)).collect())
    }

    #[test]
    fn glyphs_match_emacs_columns() {
        assert_eq!(Mark::None.glyph(), ' ');
        assert_eq!(Mark::Select.glyph(), '>');
        assert_eq!(Mark::Delete.glyph(), 'D');
        assert_eq!(Mark::Save.glyph(), 'S');
    }

    #[test]
    fn mark_current_advances_and_replaces() {
        let mut m = menu(&[(1, "a"), (2, "b"), (3, "c")]);
        m.mark_current(Mark::Select); // marks a, point -> b
        assert_eq!(m.selected(), 1);
        assert_eq!(m.mark_of(1), Mark::Select);
        m.mark_current(Mark::Delete); // marks b, point -> c
        assert_eq!(m.mark_of(2), Mark::Delete);
        // A second mark on a replaces the first (single tag column).
        m.set_mark(1, Mark::Save);
        assert_eq!(m.mark_of(1), Mark::Save);
    }

    #[test]
    fn unmark_and_backup_unmark() {
        let mut m = menu(&[(1, "a"), (2, "b"), (3, "c")]);
        m.set_mark(1, Mark::Delete);
        m.set_mark(2, Mark::Delete);
        // point on a: unmark advances to b
        m.unmark_current();
        assert_eq!(m.mark_of(1), Mark::None);
        assert_eq!(m.selected(), 1);
        // point on b: backup_unmark moves to a and clears a (already clear)
        m.backup_unmark();
        assert_eq!(m.selected(), 0);
        // b still flagged; move down and back up to clear it
        m.move_selection(1);
        m.backup_unmark();
        assert_eq!(m.selected(), 0);
        assert_eq!(m.mark_of(2), Mark::Delete); // b untouched: we cleared a
    }

    #[test]
    fn flagged_is_in_row_order() {
        let mut m = menu(&[(3, "c"), (1, "a"), (2, "b")]);
        m.set_mark(2, Mark::Delete);
        m.set_mark(3, Mark::Delete);
        m.set_mark(1, Mark::Save);
        assert_eq!(m.flagged(Mark::Delete), vec![3, 2]); // display order, not key
        assert_eq!(m.flagged(Mark::Save), vec![1]);
        assert!(m.has_marks());
        m.unmark_all();
        assert!(!m.has_marks());
        assert_eq!(m.flagged(Mark::Delete), Vec::<u64>::new());
    }

    #[test]
    fn refresh_drops_gone_marks_and_clamps_point() {
        let mut m = menu(&[(1, "a"), (2, "b"), (3, "c")]);
        m.set_mark(1, Mark::Delete);
        m.set_mark(3, Mark::Save);
        m.goto_last(); // point on c (index 2)
        assert_eq!(m.selected(), 2);
        // Buffer 3 was killed; refresh with only a and b.
        m.set_rows(vec![row(1, "a"), row(2, "b")]);
        assert_eq!(m.len(), 2);
        assert_eq!(m.selected(), 1); // clamped from 2 -> 1
        assert_eq!(m.mark_of(1), Mark::Delete); // survived
        assert_eq!(m.mark_of(3), Mark::None); // dropped: gone
    }

    #[test]
    fn bury_sinks_row_and_keeps_point_index() {
        let mut m = menu(&[(1, "a"), (2, "b"), (3, "c")]);
        m.set_mark(1, Mark::Save);
        // Point on a: bury sinks a to the bottom, point now shows b.
        assert_eq!(m.bury_current(), Some(1));
        let order: Vec<u64> = m.rows().iter().map(|r| r.key).collect();
        assert_eq!(order, vec![2, 3, 1]);
        assert_eq!(m.selected(), 0);
        assert_eq!(m.current_key(), Some(2));
        // Buried row kept its mark (marks are keyed by id).
        assert_eq!(m.mark_of(1), Mark::Save);
        // Burying the last row leaves it last and clamps point onto it.
        m.goto_last();
        assert_eq!(m.current_key(), Some(1));
        assert_eq!(m.bury_current(), Some(1));
        assert_eq!(m.rows().last().map(|r| r.key), Some(1));
        assert_eq!(m.selected(), 2);
        // Empty menu: bury is a no-op.
        let mut empty = BufferMenu::default();
        assert_eq!(empty.bury_current(), None);
    }

    #[test]
    fn internal_name_matches_emacs_convention() {
        // Space-prefixed hidden buffers and *…* special buffers are internal.
        assert!(is_internal_name(" *code-conversion-work*"));
        assert!(is_internal_name("*scratch*"));
        assert!(is_internal_name("*Messages*"));
        // Ordinary file/visible buffers are not.
        assert!(!is_internal_name("main.rs"));
        assert!(!is_internal_name("[scratch]"));
        assert!(!is_internal_name("*not-closed"));
        assert!(!is_internal_name(""));
        assert!(!is_internal_name("*"));
    }

    #[test]
    fn move_selection_clamps_to_bounds() {
        let mut m = menu(&[(1, "a"), (2, "b")]);
        m.move_selection(-5);
        assert_eq!(m.selected(), 0);
        m.move_selection(10);
        assert_eq!(m.selected(), 1);
        let mut empty = BufferMenu::default();
        empty.move_selection(1); // no panic on empty
        assert!(empty.current_row().is_none());
    }

    #[test]
    fn name_width_and_crm_and_line() {
        let mut r = row(1, "main.rs");
        r.current = true;
        r.modified = true;
        let m = BufferMenu::new(vec![r.clone(), row(2, "a-really-long-buffer-name")]);
        assert_eq!(m.name_width(), "a-really-long-buffer-name".len());
        // current, not read-only, modified -> C=. R=space M=*
        assert_eq!(m.crm(&r), ". *");
        let line = m.format_line(&r, m.name_width());
        assert!(line.starts_with(" . *")); // no mark, current, not-ro, modified
        assert!(line.contains("main.rs"));
        assert!(line.contains("Text"));
    }
}
