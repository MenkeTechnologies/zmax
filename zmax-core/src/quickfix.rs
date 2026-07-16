//! Cursor-relative quickfix navigation — the zmax port of Vim's
//! `:cabove`/`:cbelow`/`:cafter`/`:cbefore` (and the location-list twins
//! `:labove`/`:lbelow`/`:lafter`/`:lbefore`).
//!
//! The plain `:cnext`/`:cprev`/`:cc` family walks the quickfix list by its own
//! stored index; this module answers a different question: given the *current
//! cursor position inside the current buffer*, which entry is the one just
//! above / below / before / after it? Vim splits this into two motions
//! (`:help :cabove`):
//!
//!   * `:cabove`/`:cbelow` compare by **line only**. Multiple errors on one
//!     line count as a single stop; `:cbelow` lands on the first error of the
//!     target line, `:cabove` on the last error of it.
//!   * `:cbefore`/`:cafter` compare by **line and column**, so every entry is
//!     its own stop.
//!
//! Both take an optional `[count]` selecting the count-th stop in that
//! direction (default 1), and give Vim's "no more items in this direction"
//! error when there are fewer than `count` stops.
//!
//! This is the pure, dependency-free state machine behind those commands: the
//! command layer filters the active list down to the entries in the current
//! buffer (in list order), calls the matching function, and jumps to the
//! returned slice index. Keeping it here means the fiddly grouping and
//! ordering rules are unit-tested without an editor.

/// A quickfix entry's position within the current buffer. 0-based, matching the
/// editor's internal line/column convention (Vim's 1-based output is converted
/// at parse time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loc {
    pub line: usize,
    pub col: usize,
}

impl Loc {
    pub fn new(line: usize, col: usize) -> Self {
        Loc { line, col }
    }
}

/// `:cbelow` / `:lbelow` — the count-th entry on a line *below* the cursor
/// line. Errors sharing a line count as one stop; the result is the first such
/// entry on the target line (Vim lands on the first column). Returns the index
/// into `locs`, or `None` when there are fewer than `count` lines below.
pub fn below(locs: &[Loc], cursor_line: usize, count: usize) -> Option<usize> {
    let count = count.max(1);
    // Candidate indices strictly below the cursor line, nearest line first.
    let mut cands: Vec<usize> = (0..locs.len())
        .filter(|&i| locs[i].line > cursor_line)
        .collect();
    cands.sort_by_key(|&i| (locs[i].line, i));
    nth_line_group(locs, &cands, count, false)
}

/// `:cabove` / `:labove` — the count-th entry on a line *above* the cursor
/// line. Errors sharing a line count as one stop; the result is the last such
/// entry on the target line (Vim lands on the last one). Returns the index into
/// `locs`, or `None` when there are fewer than `count` lines above.
pub fn above(locs: &[Loc], cursor_line: usize, count: usize) -> Option<usize> {
    let count = count.max(1);
    // Candidate indices strictly above the cursor line, nearest line first
    // (i.e. largest line number first).
    let mut cands: Vec<usize> = (0..locs.len())
        .filter(|&i| locs[i].line < cursor_line)
        .collect();
    cands.sort_by_key(|&i| (std::cmp::Reverse(locs[i].line), i));
    nth_line_group(locs, &cands, count, true)
}

/// Walk `cands` (already ordered nearest-line-first) and return the
/// representative of the count-th distinct line. `last_in_group` picks the last
/// entry of that line rather than the first (the `:cabove` rule).
fn nth_line_group(
    locs: &[Loc],
    cands: &[usize],
    count: usize,
    last_in_group: bool,
) -> Option<usize> {
    let mut groups = 0usize;
    let mut i = 0;
    while i < cands.len() {
        let line = locs[cands[i]].line;
        // Extent of this line's run within the sorted candidate list.
        let mut j = i;
        while j + 1 < cands.len() && locs[cands[j + 1]].line == line {
            j += 1;
        }
        groups += 1;
        if groups == count {
            return Some(if last_in_group {
                // Last entry on the line in original list order.
                *cands[i..=j].iter().max_by_key(|&&k| k).unwrap()
            } else {
                // First entry on the line in original list order.
                *cands[i..=j].iter().min_by_key(|&&k| k).unwrap()
            });
        }
        i = j + 1;
    }
    None
}

/// `:cafter` / `:lafter` — the count-th entry strictly *after* the cursor
/// position (comparing line then column). Every entry is its own stop. Returns
/// the index into `locs`, or `None` when fewer than `count` entries follow.
pub fn after(locs: &[Loc], cursor: Loc, count: usize) -> Option<usize> {
    let count = count.max(1);
    let mut cands: Vec<usize> = (0..locs.len())
        .filter(|&i| (locs[i].line, locs[i].col) > (cursor.line, cursor.col))
        .collect();
    // Nearest-after first: ascending position, then list order for exact ties.
    cands.sort_by_key(|&i| (locs[i].line, locs[i].col, i));
    cands.get(count - 1).copied()
}

/// `:cbefore` / `:lbefore` — the count-th entry strictly *before* the cursor
/// position (comparing line then column). Every entry is its own stop. Returns
/// the index into `locs`, or `None` when fewer than `count` entries precede.
pub fn before(locs: &[Loc], cursor: Loc, count: usize) -> Option<usize> {
    let count = count.max(1);
    let mut cands: Vec<usize> = (0..locs.len())
        .filter(|&i| (locs[i].line, locs[i].col) < (cursor.line, cursor.col))
        .collect();
    // Nearest-before first: descending position, then reverse list order.
    cands.sort_by_key(|&i| std::cmp::Reverse((locs[i].line, locs[i].col, i)));
    cands.get(count - 1).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locs(pairs: &[(usize, usize)]) -> Vec<Loc> {
        pairs.iter().map(|&(l, c)| Loc::new(l, c)).collect()
    }

    #[test]
    fn below_picks_nearest_line_first_entry() {
        // Entries on lines 2, 5, 5, 9. Cursor on line 3.
        let l = locs(&[(2, 0), (5, 1), (5, 4), (9, 0)]);
        // nearest below line 3 is line 5; first entry of that line is index 1.
        assert_eq!(below(&l, 3, 1), Some(1));
        // second line-stop below is line 9 -> index 3.
        assert_eq!(below(&l, 3, 2), Some(3));
        // no third line below.
        assert_eq!(below(&l, 3, 3), None);
    }

    #[test]
    fn above_picks_nearest_line_last_entry() {
        let l = locs(&[(2, 0), (5, 1), (5, 4), (9, 0)]);
        // cursor on line 7: nearest above is line 5; last entry of it is index 2.
        assert_eq!(above(&l, 7, 1), Some(2));
        // second stop up is line 2 -> index 0.
        assert_eq!(above(&l, 7, 2), Some(0));
        assert_eq!(above(&l, 7, 3), None);
    }

    #[test]
    fn below_above_boundaries() {
        let l = locs(&[(4, 0)]);
        // an entry exactly on the cursor line is neither above nor below.
        assert_eq!(below(&l, 4, 1), None);
        assert_eq!(above(&l, 4, 1), None);
        // empty list.
        assert_eq!(below(&[], 0, 1), None);
        assert_eq!(above(&[], 0, 1), None);
    }

    #[test]
    fn after_uses_line_and_column() {
        // Two entries on the same line, different columns.
        let l = locs(&[(3, 2), (3, 8), (6, 0)]);
        // cursor at (3,2): the entry at (3,2) is not "after"; next is (3,8).
        assert_eq!(after(&l, Loc::new(3, 2), 1), Some(1));
        assert_eq!(after(&l, Loc::new(3, 2), 2), Some(2));
        assert_eq!(after(&l, Loc::new(3, 2), 3), None);
        // cursor before everything on the line.
        assert_eq!(after(&l, Loc::new(3, 0), 1), Some(0));
    }

    #[test]
    fn before_uses_line_and_column() {
        let l = locs(&[(3, 2), (3, 8), (6, 0)]);
        // cursor at (6,0): nearest before is (3,8) index 1, then (3,2) index 0.
        assert_eq!(before(&l, Loc::new(6, 0), 1), Some(1));
        assert_eq!(before(&l, Loc::new(6, 0), 2), Some(0));
        assert_eq!(before(&l, Loc::new(6, 0), 3), None);
        // cursor at (3,8): only (3,2) precedes.
        assert_eq!(before(&l, Loc::new(3, 8), 1), Some(0));
        assert_eq!(before(&l, Loc::new(3, 8), 2), None);
    }

    #[test]
    fn count_zero_is_treated_as_one() {
        let l = locs(&[(1, 0), (5, 0)]);
        assert_eq!(below(&l, 2, 0), below(&l, 2, 1));
        assert_eq!(above(&l, 2, 0), above(&l, 2, 1));
        assert_eq!(after(&l, Loc::new(2, 0), 0), after(&l, Loc::new(2, 0), 1));
        assert_eq!(before(&l, Loc::new(2, 0), 0), before(&l, Loc::new(2, 0), 1));
    }

    #[test]
    fn unsorted_input_is_ordered_by_position() {
        // List not in line order (Vim lists need not be sorted).
        let l = locs(&[(9, 0), (2, 0), (5, 0)]);
        // below line 3 -> nearest is line 5 at index 2.
        assert_eq!(below(&l, 3, 1), Some(2));
        // above line 8 -> nearest is line 5 at index 2, then line 2 at index 1.
        assert_eq!(above(&l, 8, 1), Some(2));
        assert_eq!(above(&l, 8, 2), Some(1));
    }
}
