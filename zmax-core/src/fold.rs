//! Code folds — the vim `zf`/`za`/`zo`/`zc`/`zR`/`zM`/`zm`/`zr` family, with
//! vim's 'foldlevel' (an outermost fold is level 1, each containing fold adds
//! one; folds deeper than the level are closed — see [`Folds::set_level`]).
//!
//! zmax renders the rope line by line and has
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

use std::sync::atomic::{AtomicBool, Ordering};

/// vim `foldclose=all`: whether a fold closes again as soon as the cursor moves
/// out of it. Set by `:set foldclose=all`, read on every cursor move (see
/// `Document::set_selection`). Empty (the default) leaves folds as the user left
/// them.
static FOLDCLOSE_ALL: AtomicBool = AtomicBool::new(false);

/// vim `foldclose`: `all` closes folds behind the cursor, `""` (the default)
/// does not. (vim's third state — close only folds above 'foldlevel' — is not
/// wired to cursor movement; 'foldlevel' itself is [`Folds::set_level`].)
pub fn set_foldclose_all(on: bool) {
    FOLDCLOSE_ALL.store(on, Ordering::Relaxed);
}

pub fn foldclose_all() -> bool {
    FOLDCLOSE_ALL.load(Ordering::Relaxed)
}

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
    /// vim `foldlevel` for this buffer: folds nested deeper than this level are
    /// closed, the rest are open. `0` (vim's default) closes everything; a level
    /// at or above [`Folds::max_level`] opens everything. Only the level-driven
    /// commands (`zM`, `zR`, `zm`, `zr`, `:set foldlevel`) read and write it —
    /// `za`/`zo`/`zc` change one fold and leave the level alone, exactly as vim
    /// does.
    level: usize,
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

    /// vim `foldclose=all`: close every open fold the cursor is *not* inside, so
    /// a fold snaps shut as soon as the cursor leaves it. Returns whether
    /// anything changed. Pure — unit tested.
    pub fn close_all_except(&mut self, line: usize) -> bool {
        let mut changed = false;
        for fold in &mut self.folds {
            if !fold.closed && !fold.contains(line) {
                fold.closed = true;
                changed = true;
            }
        }
        changed
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

    /// Open the innermost fold at `line` together with every fold nested inside
    /// it (IntelliJ "Expand Recursively"). Returns whether anything changed.
    pub fn open_recursive(&mut self, line: usize) -> bool {
        self.set_recursive(line, false)
    }

    /// Close the innermost fold at `line` together with every fold nested inside
    /// it (IntelliJ "Collapse Recursively"). Returns whether anything changed.
    pub fn close_recursive(&mut self, line: usize) -> bool {
        self.set_recursive(line, true)
    }

    /// Set the closed state of the innermost fold at `line` and all folds whose
    /// range is fully contained within it. Returns whether any fold changed.
    fn set_recursive(&mut self, line: usize, closed: bool) -> bool {
        let Some(i) = self.innermost_idx(line) else {
            return false;
        };
        let (start, end) = (self.folds[i].start, self.folds[i].end);
        let mut changed = false;
        for f in &mut self.folds {
            if f.start >= start && f.end <= end {
                changed |= f.closed != closed;
                f.closed = closed;
            }
        }
        changed
    }

    /// vim fold level of the fold at `idx`: 1 for an outermost fold, +1 for each
    /// fold that fully contains it. (vim numbers fold levels from 1; 'foldlevel'
    /// is the highest level left open.)
    fn fold_level(&self, idx: usize) -> usize {
        let f = self.folds[idx];
        1 + self
            .folds
            .iter()
            .enumerate()
            .filter(|&(i, o)| i != idx && o.start <= f.start && f.end <= o.end)
            .count()
    }

    /// The deepest fold level in the buffer — the 'foldlevel' at which every fold
    /// is open (what `zR` sets). `0` when there are no folds.
    pub fn max_level(&self) -> usize {
        (0..self.folds.len())
            .map(|i| self.fold_level(i))
            .max()
            .unwrap_or(0)
    }

    /// The buffer's current 'foldlevel'.
    pub fn level(&self) -> usize {
        self.level
    }

    /// vim `:set foldlevel=N`: folds *deeper* than level `N` close, the rest open.
    /// `0` closes every fold; [`Folds::max_level`] or higher opens every fold.
    /// Returns whether any fold's state changed.
    pub fn set_level(&mut self, level: usize) -> bool {
        self.level = level;
        let mut changed = false;
        for i in 0..self.folds.len() {
            let closed = self.fold_level(i) > level;
            changed |= self.folds[i].closed != closed;
            self.folds[i].closed = closed;
        }
        changed
    }

    /// vim `zm`: fold more — decrease 'foldlevel' by one, closing the next level
    /// of nested blocks. Stops at `0` (everything closed).
    pub fn fold_more(&mut self) {
        // Starting from a level above the deepest fold would need several `zm`
        // presses before anything moved, so clamp the level into range first.
        let level = self.level.min(self.max_level());
        self.set_level(level.saturating_sub(1));
    }

    /// vim `zr`: fold less — increase 'foldlevel' by one, opening one more level
    /// of nested blocks. Stops at [`Folds::max_level`] (everything open).
    pub fn fold_less(&mut self) {
        let level = (self.level + 1).min(self.max_level());
        self.set_level(level);
    }

    /// Open every fold (vim `zR`: 'foldlevel' goes to the deepest level).
    pub fn open_all(&mut self) {
        self.set_level(self.max_level());
    }

    /// Close every fold (vim `zM`: 'foldlevel' goes to 0).
    pub fn close_all(&mut self) {
        self.set_level(0);
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

    /// Remove all folds (vim `zE`). The buffer goes back to vim's default
    /// 'foldlevel' of 0, so folds made afterwards start out closed.
    pub fn clear(&mut self) {
        self.folds.clear();
        self.level = 0;
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

/// vim `foldignore`: with `foldmethod=indent`, a line whose first non-blank
/// character is one of these (default `#`, so C preprocessor lines) does not get
/// its own fold level from its indent — it inherits the level of the surrounding
/// code, so an unindented `#define` in the middle of a block does not tear the
/// block's fold in two.
///
/// The inherited level is the previous non-ignored line's; at the top of the file
/// (where there is no previous line) it is the next non-ignored line's.
pub fn apply_foldignore(levels: &mut [usize], lines: &[&str], ignore: &str) {
    if ignore.is_empty() || levels.len() != lines.len() {
        return;
    }
    let ignored = |line: &str| {
        line.trim_start()
            .chars()
            .next()
            .is_some_and(|c| ignore.contains(c))
    };
    // Forward pass: inherit from above.
    let mut prev: Option<usize> = None;
    let mut open: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if ignored(line) {
            match prev {
                Some(level) => levels[i] = level,
                // Nothing above yet — remember it for the backward fix-up.
                None => open.push(i),
            }
        } else {
            prev = Some(levels[i]);
        }
    }
    // The leading ignored lines take the first real line's level.
    if let Some(&first) = open.first() {
        let after = lines
            .iter()
            .enumerate()
            .skip(first)
            .find(|(_, l)| !ignored(l))
            .map(|(i, _)| levels[i])
            .unwrap_or(0);
        for i in open {
            levels[i] = after;
        }
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
    fn recursive_open_close_affects_nested_folds() {
        let mut f = Folds::default();
        f.create(1, 10); // outer, closed by default
        f.create(3, 5); // nested, closed by default
                        // From a line inside only the outer fold, open it and every descendant.
        assert!(f.open_recursive(8));
        assert_eq!(
            closed(&f),
            Vec::<(usize, usize)>::new(),
            "outer + nested opened"
        );
        assert!(!f.open_recursive(8), "second call is a no-op");
        // From a line inside the nested fold, only the innermost region closes.
        assert!(f.close_recursive(4));
        assert_eq!(closed(&f), vec![(3, 5)], "only innermost region closed");
        // From the outer header line, both the outer fold and its descendants close.
        assert!(f.close_recursive(1));
        assert_eq!(closed(&f), vec![(1, 10), (3, 5)]);
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

    /// The point of `foldignore`: an unindented `#define` inside an indented
    /// block must not drop the fold level to 0 and split the block's fold.
    #[test]
    fn foldignore_lines_inherit_the_surrounding_level() {
        let lines = ["fn f() {", "    a();", "#define X 1", "    b();", "}"];
        let mut levels = vec![0, 1, 0, 1, 0];
        apply_foldignore(&mut levels, &lines, "#");
        assert_eq!(levels, vec![0, 1, 1, 1, 0], "the #define inherits level 1");

        // A leading ignored line takes the level of the first real line below it.
        let lines = ["#include <a.h>", "    x();"];
        let mut levels = vec![0, 1];
        apply_foldignore(&mut levels, &lines, "#");
        assert_eq!(levels, vec![1, 1]);

        // An empty 'foldignore' changes nothing.
        let lines = ["#define X", "    y();"];
        let mut levels = vec![0, 1];
        apply_foldignore(&mut levels, &lines, "");
        assert_eq!(levels, vec![0, 1]);
    }

    /// vim 'foldlevel': level 1 is an outermost fold, each containing fold adds
    /// one. `zM` (level 0) closes everything, `zR` (level = deepest) opens
    /// everything, and `zm`/`zr` walk one level at a time between them.
    #[test]
    fn fold_level_walks_one_nesting_level_at_a_time() {
        // outer 1..20, inner 3..10, innermost 5..7, plus a second outer 30..40.
        let mut folds = Folds::default();
        assert!(folds.create(1, 20));
        assert!(folds.create(3, 10));
        assert!(folds.create(5, 7));
        assert!(folds.create(30, 40));
        assert_eq!(folds.max_level(), 3, "the deepest fold is at level 3");

        // zR: everything open.
        folds.open_all();
        assert_eq!(folds.level(), 3);
        assert_eq!(closed(&folds), Vec::<(usize, usize)>::new());

        // zm: level 2 — only the level-3 fold closes.
        folds.fold_more();
        assert_eq!(folds.level(), 2);
        assert_eq!(closed(&folds), vec![(5, 7)]);

        // zm again: level 1 — the level-2 fold closes too, outermost still open.
        folds.fold_more();
        assert_eq!(folds.level(), 1);
        assert_eq!(closed(&folds), vec![(3, 10), (5, 7)]);

        // zm again: level 0 — everything closed, and it stops there.
        folds.fold_more();
        assert_eq!(folds.level(), 0);
        assert_eq!(closed(&folds), vec![(1, 20), (3, 10), (5, 7), (30, 40)]);
        folds.fold_more();
        assert_eq!(folds.level(), 0, "zm at level 0 stays at 0");

        // zr walks back out one level at a time.
        folds.fold_less();
        assert_eq!(folds.level(), 1);
        assert_eq!(closed(&folds), vec![(3, 10), (5, 7)]);
        folds.fold_less();
        assert_eq!(closed(&folds), vec![(5, 7)]);
        folds.fold_less();
        assert_eq!(closed(&folds), Vec::<(usize, usize)>::new());
        folds.fold_less();
        assert_eq!(folds.level(), 3, "zr past the deepest level stays there");

        // zM closes everything from any level.
        folds.close_all();
        assert_eq!(folds.level(), 0);
        assert_eq!(closed(&folds), vec![(1, 20), (3, 10), (5, 7), (30, 40)]);
    }

    /// `:set foldlevel=N` closes every fold nested deeper than level N.
    #[test]
    fn set_level_closes_only_folds_deeper_than_the_level() {
        let mut folds = Folds::default();
        assert!(folds.create(0, 20));
        assert!(folds.create(2, 10));

        assert!(folds.set_level(1));
        assert_eq!(
            closed(&folds),
            vec![(2, 10)],
            "level 1 keeps the outer open"
        );
        assert!(
            !folds.set_level(1),
            "re-applying the same level changes nothing"
        );
        assert!(folds.set_level(9));
        assert_eq!(
            closed(&folds),
            Vec::<(usize, usize)>::new(),
            "a level past the deepest fold opens everything"
        );
    }

    /// vim `foldclose=all`: every fold the cursor is not inside snaps shut; the
    /// one it is inside stays open (nested folds around it too).
    #[test]
    fn close_all_except_shuts_folds_the_cursor_left() {
        let mut folds = Folds::default();
        assert!(folds.create(1, 9));
        assert!(folds.create(3, 5));
        assert!(folds.create(20, 30));
        folds.open_all();
        assert_eq!(closed(&folds), Vec::<(usize, usize)>::new());

        // Cursor on line 4: the two folds around it stay open, the far one shuts.
        assert!(folds.close_all_except(4));
        assert_eq!(closed(&folds), vec![(20, 30)]);

        // Moving to line 7 leaves the inner 3..5 fold, which now shuts too.
        assert!(folds.close_all_except(7));
        assert_eq!(closed(&folds), vec![(3, 5), (20, 30)]);

        // Nothing left to close => no change reported (so no needless redraw).
        assert!(!folds.close_all_except(7));

        // Moving out of every fold shuts all of them.
        folds.open_all();
        assert!(folds.close_all_except(100));
        assert_eq!(closed(&folds), vec![(1, 9), (3, 5), (20, 30)]);
    }
}
