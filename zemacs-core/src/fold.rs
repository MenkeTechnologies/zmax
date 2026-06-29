//! Manual code folds — the vim `zf`/`za`/`zo`/`zc`/`zR`/`zM` family.
//!
//! zemacs runs on the Helix engine, which renders the rope line by line and has
//! no native fold concept. A *closed* fold hides the lines after its first line
//! from rendering and from line-wise cursor motion; the first line stays visible
//! (vim shows the fold marker there). Folds are manual (created with `zf` over a
//! motion or visual selection) and stored per view as a sorted list of inclusive
//! line ranges, each with an open/closed flag.
//!
//! This module is the pure fold *model* and its operations. Rendering and motion
//! consult [`Folds::is_line_hidden`] / [`Folds::closed_fold_starting_at`] to skip
//! hidden lines. The model is engine-agnostic and fully unit tested; edit
//! remapping is intentionally conservative (see [`Folds::clamp`]).

/// A single fold over an inclusive range of document lines `[start, end]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fold {
    /// First line of the fold. Stays visible; shows the fold marker when closed.
    pub start: usize,
    /// Last line of the fold (inclusive).
    pub end: usize,
    /// Whether the fold is collapsed (hiding lines `start+1..=end`).
    pub closed: bool,
}

impl Fold {
    /// Number of lines spanned (inclusive).
    pub fn len(&self) -> usize {
        self.end - self.start + 1
    }

    /// A fold always spans at least one line, so it is never empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn contains(&self, line: usize) -> bool {
        self.start <= line && line <= self.end
    }
}

/// All folds for a view, kept sorted by `start`. Folds may nest (one fully
/// inside another) but never partially overlap — creating an overlapping fold
/// is rejected so the model stays a clean tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folds {
    folds: Vec<Fold>,
}

impl Folds {
    pub fn is_empty(&self) -> bool {
        self.folds.is_empty()
    }

    pub fn len(&self) -> usize {
        self.folds.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Fold> {
        self.folds.iter()
    }

    /// Create a closed fold over `[start, end]` (vim `zf`). A single-line range
    /// or a range that partially overlaps an existing fold is rejected and
    /// returns `false`. An identical range is reused (re-closed) rather than
    /// duplicated. Returns `true` if a fold now covers the range.
    pub fn create(&mut self, start: usize, end: usize) -> bool {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        if start == end {
            return false; // a fold must span at least two lines
        }
        // Reuse an exact match (just close it).
        if let Some(f) = self
            .folds
            .iter_mut()
            .find(|f| f.start == start && f.end == end)
        {
            f.closed = true;
            return true;
        }
        // Reject partial overlap (allowed: full nesting either direction).
        for f in &self.folds {
            let nested = (f.start <= start && end <= f.end) || (start <= f.start && f.end <= end);
            let disjoint = end < f.start || f.end < start;
            if !nested && !disjoint {
                return false;
            }
        }
        self.folds.push(Fold {
            start,
            end,
            closed: true,
        });
        self.folds
            .sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        true
    }

    /// Index of the innermost (smallest) fold containing `line`, if any.
    fn innermost_idx(&self, line: usize) -> Option<usize> {
        self.folds
            .iter()
            .enumerate()
            .filter(|(_, f)| f.contains(line))
            .min_by_key(|(_, f)| f.len())
            .map(|(i, _)| i)
    }

    /// The innermost fold containing `line`.
    pub fn innermost_at(&self, line: usize) -> Option<Fold> {
        self.innermost_idx(line).map(|i| self.folds[i])
    }

    /// `true` if `line` is hidden by some closed fold — inside a closed fold's
    /// range but not on its first (still-visible) line.
    pub fn is_line_hidden(&self, line: usize) -> bool {
        self.folds
            .iter()
            .any(|f| f.closed && f.start < line && line <= f.end)
    }

    /// The closed fold whose first line is exactly `line` (the visible header of
    /// a collapsed region), preferring the outermost such fold.
    pub fn closed_fold_starting_at(&self, line: usize) -> Option<Fold> {
        self.folds
            .iter()
            .filter(|f| f.closed && f.start == line)
            .max_by_key(|f| f.len())
            .copied()
    }

    /// If `line` is hidden, the line of the visible header of the outermost
    /// closed fold hiding it (where the cursor should snap to). Else `line`.
    pub fn visible_anchor(&self, line: usize) -> usize {
        self.folds
            .iter()
            .filter(|f| f.closed && f.start < line && line <= f.end)
            .min_by_key(|f| f.start)
            .map(|f| f.start)
            .unwrap_or(line)
    }

    /// Open the innermost fold at `line` (vim `zo`). Returns whether anything changed.
    pub fn open(&mut self, line: usize) -> bool {
        match self.innermost_idx(line) {
            Some(i) if self.folds[i].closed => {
                self.folds[i].closed = false;
                true
            }
            _ => false,
        }
    }

    /// Close the innermost fold at `line` (vim `zc`). Returns whether anything changed.
    pub fn close(&mut self, line: usize) -> bool {
        match self.innermost_idx(line) {
            Some(i) if !self.folds[i].closed => {
                self.folds[i].closed = true;
                true
            }
            _ => false,
        }
    }

    /// Toggle the innermost fold at `line` (vim `za`). Returns whether a fold existed.
    pub fn toggle(&mut self, line: usize) -> bool {
        match self.innermost_idx(line) {
            Some(i) => {
                self.folds[i].closed = !self.folds[i].closed;
                true
            }
            None => false,
        }
    }

    /// Open every fold (vim `zR`).
    pub fn open_all(&mut self) {
        for f in &mut self.folds {
            f.closed = false;
        }
    }

    /// Close every fold (vim `zM`).
    pub fn close_all(&mut self) {
        for f in &mut self.folds {
            f.closed = true;
        }
    }

    /// Delete the innermost fold at `line` (vim `zd`). Returns whether one was removed.
    pub fn delete(&mut self, line: usize) -> bool {
        match self.innermost_idx(line) {
            Some(i) => {
                self.folds.remove(i);
                true
            }
            None => false,
        }
    }

    /// Remove all folds (vim `zE`).
    pub fn clear(&mut self) {
        self.folds.clear();
    }

    /// First fold start strictly after `line`, for `zj` (move to next fold).
    pub fn next_fold_start(&self, line: usize) -> Option<usize> {
        self.folds
            .iter()
            .map(|f| f.start)
            .filter(|&s| s > line)
            .min()
    }

    /// Last fold end strictly before `line`, for `zk` (move to prev fold).
    pub fn prev_fold_end(&self, line: usize) -> Option<usize> {
        self.folds.iter().map(|f| f.end).filter(|&e| e < line).max()
    }

    /// Closed folds as inclusive `(start, end)` line ranges — the form the
    /// document formatter consumes to hide folded lines (see `TextFormat::folded`).
    pub fn closed_ranges(&self) -> Vec<(usize, usize)> {
        self.folds
            .iter()
            .filter(|f| f.closed)
            .map(|f| (f.start, f.end))
            .collect()
    }

    /// Conservatively reconcile folds after an edit changed the line count:
    /// drop folds whose range no longer fits in `[0, last_line]`. This keeps the
    /// model safe (no out-of-range hides) at the cost of not making folds follow
    /// inserted/removed lines — a deliberate first-pass limitation.
    pub fn clamp(&mut self, last_line: usize) {
        self.folds.retain(|f| f.end <= last_line && f.start < f.end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(folds: &Folds) -> Vec<(usize, usize)> {
        folds
            .iter()
            .filter(|f| f.closed)
            .map(|f| (f.start, f.end))
            .collect()
    }

    #[test]
    fn create_rejects_single_line_and_dedups() {
        let mut f = Folds::default();
        assert!(!f.create(3, 3), "single-line fold rejected");
        assert!(f.create(2, 6));
        assert!(f.create(2, 6), "identical range reused, not duplicated");
        assert_eq!(f.len(), 1);
        // order-independent
        assert!(f.create(10, 8));
        assert_eq!(closed(&f), vec![(2, 6), (8, 10)]);
    }

    #[test]
    fn hidden_lines_respect_open_close() {
        let mut f = Folds::default();
        f.create(2, 5); // closed by default
        assert!(!f.is_line_hidden(2), "fold header stays visible");
        assert!(f.is_line_hidden(3));
        assert!(f.is_line_hidden(5));
        assert!(!f.is_line_hidden(6));
        f.open(3);
        assert!(!f.is_line_hidden(4), "open fold hides nothing");
        f.toggle(2);
        assert!(f.is_line_hidden(4), "toggle re-closes");
    }

    #[test]
    fn nested_folds_and_innermost() {
        let mut f = Folds::default();
        assert!(f.create(1, 10));
        assert!(f.create(3, 5), "nested fold allowed");
        assert!(!f.create(4, 12), "partial overlap rejected");
        assert_eq!(
            f.innermost_at(4),
            Some(Fold {
                start: 3,
                end: 5,
                closed: true
            })
        );
        assert_eq!(
            f.innermost_at(8),
            Some(Fold {
                start: 1,
                end: 10,
                closed: true
            })
        );
    }

    #[test]
    fn visible_anchor_snaps_to_outer_header() {
        let mut f = Folds::default();
        f.create(1, 10);
        f.create(3, 5);
        // line 4 hidden by both; snaps to the outermost header (line 1)
        assert_eq!(f.visible_anchor(4), 1);
        f.open(4); // opens innermost (3,5) — still hidden by outer (1,10)
        assert_eq!(f.visible_anchor(4), 1);
    }

    #[test]
    fn open_close_all_and_delete() {
        let mut f = Folds::default();
        f.create(2, 4);
        f.create(6, 9);
        f.open_all();
        assert!(closed(&f).is_empty());
        f.close_all();
        assert_eq!(closed(&f).len(), 2);
        assert!(f.delete(3));
        assert_eq!(f.len(), 1);
        f.clear();
        assert!(f.is_empty());
    }

    #[test]
    fn navigation_next_prev() {
        let mut f = Folds::default();
        f.create(2, 4);
        f.create(8, 10);
        assert_eq!(f.next_fold_start(0), Some(2));
        assert_eq!(f.next_fold_start(2), Some(8));
        assert_eq!(f.next_fold_start(8), None);
        assert_eq!(f.prev_fold_end(20), Some(10));
        assert_eq!(f.prev_fold_end(9), Some(4));
        assert_eq!(f.prev_fold_end(4), None);
    }

    #[test]
    fn clamp_drops_out_of_range_folds() {
        let mut f = Folds::default();
        f.create(2, 4);
        f.create(8, 20);
        f.clamp(10); // doc shrank to 10 lines
        assert_eq!(closed(&f), vec![(2, 4)]);
    }
}
