//! Nested fold tree, ported from neovim `src/nvim/fold.c`.
//!
//! [`crate::fold`] models a buffer's folds as a flat list of `(start, end)` line
//! ranges carrying a single `closed` bool. vim does not: a fold level is
//! computed for every line, the fold *tree* is derived from that level sequence,
//! and each fold records whether it is explicitly open, explicitly closed, or
//! simply follows `'foldlevel'`. Everything that depends on levels — `zM`/`zR`/
//! `zm`/`zr`, `'foldlevel'`, nesting, `'foldnestmax'` clamping, and keeping a
//! hand-opened fold open across a recompute — is unimplementable on the flat
//! model, so this module ports the real one.
//!
//! Line numbers are 1-based, as in vim, so this file reads against the C.

/// vim `fold_T` (`fold.c`):
///
/// ```c
/// typedef struct {
///   linenr_T fd_top;              // first line of fold; for nested fold
///                                 // relative to parent
///   linenr_T fd_len;              // number of lines in the fold
///   garray_T fd_nested;           // array of nested folds
///   char fd_flags;                // see below
///   TriState fd_small;            // kTrue, kFalse, or kNone: fold smaller than
///                                 // 'foldminlines'; kNone applies to nested
///                                 // folds too
/// } fold_T;
/// ```
///
/// `top` being parent-relative is load-bearing: it is what lets
/// `foldMarkAdjust` shift a parent without walking its children.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fold {
    /// First line of the fold; relative to the parent for a nested fold.
    pub top: usize,
    /// Number of lines in the fold.
    pub len: usize,
    /// Nested folds (vim `fd_nested`).
    pub nested: Vec<Fold>,
    /// Open/closed/level (vim `fd_flags`).
    pub flags: FoldFlag,
    /// Whether the fold is smaller than `'foldminlines'` (vim `fd_small`).
    pub small: TriState,
}

/// vim's `fd_flags` values:
///
/// ```c
/// enum {
///   FD_OPEN = 0,    // fold is open (nested ones can be closed)
///   FD_CLOSED = 1,  // fold is closed
///   FD_LEVEL = 2,   // depends on 'foldlevel' (nested folds too)
/// };
/// ```
///
/// [`FoldFlag::Level`] is the state the flat model cannot express: `zM`/`zR`
/// reset every fold to it so `'foldlevel'` regains control, while `zo`/`zc` pin
/// an individual fold to [`FoldFlag::Open`]/[`FoldFlag::Closed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FoldFlag {
    /// Fold is open; nested folds may still be closed.
    Open,
    /// Fold is closed.
    Closed,
    /// Follows `'foldlevel'`, and so do nested folds.
    #[default]
    Level,
}

/// vim `TriState` as `fd_small` uses it: `kNone` means "not computed yet", and
/// applies to nested folds too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriState {
    /// vim `kTrue`.
    True,
    /// vim `kFalse`.
    False,
    /// vim `kNone` — not yet determined.
    #[default]
    None,
}

/// vim `MAX_LEVEL` (`fold.c`): `#define MAX_LEVEL 20 // maximum fold depth`.
pub const MAX_LEVEL: i32 = 20;

/// A line whose fold level is undefined — vim's `flp->lvl == -1`, used for blank
/// lines and `'foldignore'` lines, whose level "depends on surrounding lines".
pub const UNDEFINED_LEVEL: i32 = -1;

/// vim `foldlevelIndent` (`fold.c`), the `'foldmethod=indent'` level getter:
///
/// ```c
/// static void foldlevelIndent(fline_T *flp)
/// {
///   linenr_T lnum = flp->lnum + flp->off;
///   buf_T *buf = flp->wp->w_buffer;
///   char *s = skipwhite(ml_get_buf(buf, lnum));
///
///   // empty line or lines starting with a character in 'foldignore': level
///   // depends on surrounding lines
///   if (*s == NUL || vim_strchr(flp->wp->w_p_fdi, (uint8_t)(*s)) != NULL) {
///     // first and last line can't be undefined, use level 0
///     flp->lvl = (lnum == 1 || lnum == buf->b_ml.ml_line_count) ? 0 : -1;
///   } else {
///     flp->lvl = get_indent_buf(buf, lnum) / get_sw_value(buf);
///   }
///   flp->lvl = MIN(flp->lvl, (int)MAX(0, flp->wp->w_p_fdn));
/// }
/// ```
///
/// Two details the ad-hoc version got wrong. A blank or `'foldignore'` line is
/// [`UNDEFINED_LEVEL`], *not* the previous line's level, so a blank line between
/// two blocks does not drag the preceding fold down over it. And `'foldnestmax'`
/// **clamps** the level, merging deeper blocks into their parent, rather than
/// deleting the folds that sit past it.
///
/// `lnum` and `last_lnum` are 1-based. `line` is the raw line, newline included.
pub fn foldlevel_indent(
    line: &str,
    lnum: usize,
    last_lnum: usize,
    foldignore: &str,
    shiftwidth: usize,
    tab_width: usize,
    foldnestmax: i32,
) -> i32 {
    // `skipwhite` steps over spaces and tabs; the buffer line carries no
    // newline in vim, so a trailing one here is still "empty".
    let s = line.trim_start_matches([' ', '\t']);
    let first = s.chars().next().filter(|c| *c != '\n' && *c != '\r');

    let lvl = match first {
        // empty line or lines starting with a character in 'foldignore': level
        // depends on surrounding lines
        None => undefined_or_edge(lnum, last_lnum),
        Some(c) if foldignore.contains(c) => undefined_or_edge(lnum, last_lnum),
        Some(_) => (indent_columns(line, tab_width) / shiftwidth.max(1)) as i32,
    };
    lvl.min(foldnestmax.max(0))
}

/// vim: "first and last line can't be undefined, use level 0".
fn undefined_or_edge(lnum: usize, last_lnum: usize) -> i32 {
    if lnum == 1 || lnum == last_lnum {
        0
    } else {
        UNDEFINED_LEVEL
    }
}

/// vim `get_indent_buf`: the line's indent in screen columns, tabs counted to
/// the next `tab_width` stop.
fn indent_columns(line: &str, tab_width: usize) -> usize {
    let tw = tab_width.max(1);
    let mut cols = 0;
    for ch in line.chars() {
        match ch {
            ' ' => cols += 1,
            // A tab advances to the next tab stop, which is not the same as
            // adding `tab_width` when the indent is already ragged.
            '\t' => cols += tw - (cols % tw),
            _ => break,
        }
    }
    cols
}

/// vim `MAXLNUM`, used by `foldMarkAdjust` as the "lines were deleted" sentinel
/// for `amount`.
pub const MAXLNUM: i64 = 0x7fff_ffff;

/// vim `foldMarkAdjust` (`fold.c`): adjust the folds for an edit that changed
/// lines `line1..=line2` by `amount`, with everything below shifted by
/// `amount_after`. `amount == MAXLNUM` means those lines were deleted.
///
/// ```c
/// void foldMarkAdjust(win_T *wp, linenr_T line1, linenr_T line2, linenr_T amount,
///                     linenr_T amount_after)
/// {
///   // If deleting marks from line1 to line2, but not deleting all those
///   // lines, set line2 so that only deleted lines have their folds removed.
///   if (amount == MAXLNUM && line2 >= line1 && line2 - line1 >= -amount_after) {
///     line2 = line1 - amount_after - 1;
///   }
///   if (line2 < line1) {
///     line2 = line1;
///   }
///   // If appending a line in Insert mode, it should be included in the fold
///   // just above the line.
///   if ((State & MODE_INSERT) && amount == 1 && line2 == MAXLNUM) {
///     line1--;
///   }
///   foldMarkAdjustRecurse(wp, &wp->w_folds, line1, line2, amount, amount_after);
/// }
/// ```
pub fn fold_mark_adjust(
    folds: &mut Vec<Fold>,
    mut line1: i64,
    mut line2: i64,
    amount: i64,
    amount_after: i64,
    insert_mode: bool,
) {
    if amount == MAXLNUM && line2 >= line1 && line2 - line1 >= -amount_after {
        line2 = line1 - amount_after - 1;
    }
    if line2 < line1 {
        line2 = line1;
    }
    if insert_mode && amount == 1 && line2 == MAXLNUM {
        line1 -= 1;
    }
    fold_mark_adjust_recurse(folds, line1, line2, amount, amount_after, insert_mode);
}

/// vim `foldMarkAdjustRecurse` (`fold.c`). The six cases in the C, in order:
///
/// ```text
///    1  2  3
///    1  2  3
/// line1     2      3  4  5
///       2  3  4  5
///       2  3  4  5
/// line2     2      3  4  5
///          3     5  6
///          3     5  6
/// ```
///
/// 1 is entirely above the edit, 6 entirely below, 4 entirely inside; 2, 3 and 5
/// straddle an edge and have to grow, shrink or move, recursing into their
/// nested folds. That truncate/split behaviour is what a flat list of ranges
/// cannot do — it can only shift a fold or drop it.
///
/// `foldFind` is skipped: the C uses it to start the scan at the first fold that
/// could be affected, and case 1 `continue`s over exactly those folds anyway.
pub fn fold_mark_adjust_recurse(
    gap: &mut Vec<Fold>,
    line1: i64,
    line2: i64,
    amount: i64,
    amount_after: i64,
    insert_mode: bool,
) {
    if gap.is_empty() {
        return;
    }
    // In Insert mode an inserted line at the top of a fold is considered part
    // of the fold, otherwise it isn't.
    let top = if insert_mode && amount == 1 && line2 == MAXLNUM {
        line1 + 1
    } else {
        line1
    };

    let mut i = 0;
    while i < gap.len() {
        let fd_top = gap[i].top as i64;
        let fd_len = gap[i].len as i64;
        let last = fd_top + fd_len - 1; // last line of fold

        // 1. fold completely above line1: nothing to do
        if last < line1 {
            i += 1;
            continue;
        }

        if fd_top > line2 {
            // 6. fold below line2: only adjust for amount_after
            if amount_after == 0 {
                break;
            }
            gap[i].top = (fd_top + amount_after).max(0) as usize;
        } else if fd_top >= top && last <= line2 {
            // 4. fold completely contained in range
            if amount == MAXLNUM {
                // Deleting lines: delete the fold completely
                gap.remove(i);
                continue; // C does `i--; fp--;` so the index is revisited
            }
            gap[i].top = (fd_top + amount).max(0) as usize;
        } else if fd_top < top {
            // 2 or 3: need to correct nested folds too
            fold_mark_adjust_recurse(
                &mut gap[i].nested,
                line1 - fd_top,
                line2 - fd_top,
                amount,
                amount_after,
                insert_mode,
            );
            if last <= line2 {
                // 2. fold contains line1, line2 is below fold
                gap[i].len = if amount == MAXLNUM {
                    (line1 - fd_top).max(0) as usize
                } else {
                    (fd_len + amount).max(0) as usize
                };
            } else {
                // 3. fold contains line1 and line2
                gap[i].len = (fd_len + amount_after).max(0) as usize;
            }
        } else {
            // 5. fold is below line1 and contains line2; need to
            // correct nested folds too
            if amount == MAXLNUM {
                fold_mark_adjust_recurse(
                    &mut gap[i].nested,
                    0,
                    line2 - fd_top,
                    amount,
                    amount_after + (fd_top - top),
                    insert_mode,
                );
                gap[i].len = (fd_len - (line2 - fd_top + 1)).max(0) as usize;
                gap[i].top = line1.max(0) as usize;
            } else {
                fold_mark_adjust_recurse(
                    &mut gap[i].nested,
                    0,
                    line2 - fd_top,
                    amount,
                    amount_after - amount,
                    insert_mode,
                );
                gap[i].len = (fd_len + amount_after - amount).max(0) as usize;
                gap[i].top = (fd_top + amount).max(0) as usize;
            }
        }
        i += 1;
    }
}

/// vim `foldFind` (`fold.c`): binary-search `gap` for the fold containing
/// `lnum`.
///
/// Returns the index and whether it actually contains `lnum`. When it does not,
/// the index is the first fold *below* `lnum` and may be `gap.len()` — the C
/// documents this as "careful: it can be beyond the end of the array!", and the
/// callers lean on it to find an insertion point.
pub fn fold_find(gap: &[Fold], lnum: i64) -> (usize, bool) {
    if gap.is_empty() {
        return (0, false);
    }
    let mut low: i64 = 0;
    let mut high: i64 = gap.len() as i64 - 1;
    while low <= high {
        let i = ((low + high) / 2) as usize;
        let top = gap[i].top as i64;
        if top > lnum {
            // fold below lnum, adjust high
            high = i as i64 - 1;
        } else if top + gap[i].len as i64 <= lnum {
            // fold above lnum, adjust low
            low = i as i64 + 1;
        } else {
            // lnum is inside this fold
            return (i, true);
        }
    }
    (low as usize, false)
}

/// vim `foldInsert` (`fold.c`): insert a new, empty fold at position `i`.
pub fn fold_insert(gap: &mut Vec<Fold>, i: usize) {
    gap.insert(i, Fold::default());
}

/// vim `foldSplit` (`fold.c`): split the `i`th fold, which starts before `top`
/// and ends below `bot`, into one part ending above `top` and another starting
/// below `bot`.
///
/// "The caller must first have taken care of any nested folds from `top` to
/// `bot`!" — nested folds below `bot` move to the new second fold, rebased onto
/// its `top`, because nested `top` is parent-relative.
pub fn fold_split(gap: &mut Vec<Fold>, i: usize, top: i64, bot: i64) {
    fold_insert(gap, i + 1);
    let fp_top = gap[i].top as i64;
    let fp_len = gap[i].len as i64;

    let new_top = bot + 1;
    gap[i + 1].top = new_top.max(0) as usize;
    gap[i + 1].len = (fp_len - (new_top - fp_top)).max(0) as usize;
    gap[i + 1].flags = gap[i].flags;
    gap[i + 1].small = TriState::None;
    gap[i].small = TriState::None;

    // Move nested folds below bot to the new fold. There can't be any between
    // top and bot, they have been removed by the caller.
    let (idx, _) = fold_find(&gap[i].nested, new_top - fp_top);
    if idx < gap[i].nested.len() {
        let delta = new_top - fp_top;
        let moved: Vec<Fold> = gap[i]
            .nested
            .split_off(idx)
            .into_iter()
            .map(|mut f| {
                f.top = (f.top as i64 - delta).max(0) as usize;
                f
            })
            .collect();
        gap[i + 1].nested = moved;
    }
    gap[i].len = (top - fp_top).max(0) as usize;
}

/// vim `foldRemove` (`fold.c`): remove folds within `top..=bot`.
///
/// ```text
///      1  2  3
///      1  2  3
/// top     2  3  4  5
///     2  3  4  5
/// bot     2  3  4  5
///        3     5  6
///        3     5  6
///
/// 1: not changed
/// 2: truncate to stop above "top"
/// 3: split in two parts, one stops above "top", other starts below "bot".
/// 4: deleted
/// 5: made to start below "bot".
/// 6: not changed
/// ```
pub fn fold_remove(gap: &mut Vec<Fold>, top: i64, bot: i64) {
    if bot < top {
        return; // nothing to do
    }
    while !gap.is_empty() {
        // Find fold that includes top or a following one.
        let (i, found) = fold_find(gap, top);
        if found && (gap[i].top as i64) < top {
            // 2: or 3: need to delete nested folds
            let fp_top = gap[i].top as i64;
            fold_remove(&mut gap[i].nested, top - fp_top, bot - fp_top);
            if fp_top + gap[i].len as i64 - 1 > bot {
                // 3: need to split it.
                fold_split(gap, i, top, bot);
            } else {
                // 2: truncate fold at "top".
                gap[i].len = (top - fp_top).max(0) as usize;
            }
            continue;
        }
        if i >= gap.len() || (gap[i].top as i64) > bot {
            // 6: Found a fold below bot, can stop looking.
            break;
        }
        if (gap[i].top as i64) >= top {
            let fp_top = gap[i].top as i64;
            if fp_top + gap[i].len as i64 - 1 > bot {
                // 5: Make fold that includes bot start below bot.
                fold_mark_adjust_recurse(
                    &mut gap[i].nested,
                    0,
                    bot - fp_top,
                    MAXLNUM,
                    fp_top - bot - 1,
                    false,
                );
                gap[i].len = (gap[i].len as i64 - (bot - fp_top + 1)).max(0) as usize;
                gap[i].top = (bot + 1).max(0) as usize;
                break;
            }
            // 4: Delete completely contained fold.
            gap.remove(i);
        } else {
            break;
        }
    }
}

/// vim `foldMerge` (`fold.c`): merge the adjacent folds at `i1` and `i2`, which
/// only works when `i1` ends just above `i2`. The result is `i1`; `i2` is
/// deleted and its nested folds move across, rebased by `i1`'s length.
pub fn fold_merge(gap: &mut Vec<Fold>, i1: usize, i2: usize) {
    let mut fp2 = gap.remove(i2);
    let fp1 = &mut gap[i1];
    let fp1_len = fp1.len as i64;

    // If the last nested fold in fp1 touches the first nested fold in fp2,
    // merge them recursively.
    let (i3, found3) = fold_find(&fp1.nested, fp1_len - 1);
    let (i4, found4) = fold_find(&fp2.nested, 0);
    if found3 && found4 {
        let child = fp2.nested.remove(i4);
        let mut pair = vec![std::mem::take(&mut fp1.nested[i3]), child];
        fold_merge(&mut pair, 0, 1);
        fp1.nested[i3] = pair.remove(0);
    }

    // Move nested folds in fp2 to the end of fp1.
    for mut f in fp2.nested.drain(..) {
        f.top = (f.top as i64 + fp1_len).max(0) as usize;
        fp1.nested.push(f);
    }
    fp1.len += fp2.len;
}

/// vim `newFoldLevelWin` (`fold.c`): `'foldlevel'` changed, so every top-level
/// fold goes back to [`FoldFlag::Level`] and hands control back to it.
///
/// ```c
/// if (wp->w_fold_manual) {
///   // Set all flags for the first level of folds to FD_LEVEL.  Following
///   // manual open/close will then change the flags to FD_OPEN or
///   // FD_CLOSED for those folds that don't use 'foldlevel'.
///   fold_T *fp = (fold_T *)wp->w_folds.ga_data;
///   for (int i = 0; i < wp->w_folds.ga_len; i++) {
///     fp[i].fd_flags = FD_LEVEL;
///   }
///   wp->w_fold_manual = false;
/// }
/// ```
///
/// This is what `zM`/`zR` actually do, and why they are not "close/open every
/// fold": they move `'foldlevel'` and let the level decide. Only the first level
/// is reset because [`FoldFlag::Level`] already covers nested folds.
pub fn new_fold_level(folds: &mut [Fold], fold_manual: &mut bool) {
    if *fold_manual {
        for f in folds.iter_mut() {
            f.flags = FoldFlag::Level;
        }
        *fold_manual = false;
    }
}

/// vim `check_closed` (`fold.c`): is this fold closed, given the enclosing
/// state?
///
/// ```c
/// // Check if this fold is closed.  If the flag is FD_LEVEL this
/// // fold and all folds it contains depend on 'foldlevel'.
/// if (*use_levelp || fp->fd_flags == FD_LEVEL) {
///   *use_levelp = true;
///   if (level >= wp->w_p_fdl) {
///     closed = true;
///   }
/// } else if (fp->fd_flags == FD_CLOSED) {
///   closed = true;
/// }
/// ```
///
/// `use_level` is threaded down the tree: once an ancestor was
/// [`FoldFlag::Level`], every fold beneath it follows `'foldlevel'` too and its
/// own flag is ignored. The flat model has no way to express that.
///
/// The `fd_small` half of the C is not ported yet, so a fold shorter than
/// `'foldminlines'` still reports closed here.
pub fn check_closed(fold: &Fold, use_level: &mut bool, level: i32, foldlevel: i32) -> bool {
    if *use_level || fold.flags == FoldFlag::Level {
        *use_level = true;
        level >= foldlevel
    } else {
        fold.flags == FoldFlag::Closed
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const NEST: i32 = MAX_LEVEL;

    fn lvl(line: &str, lnum: usize, last: usize) -> i32 {
        foldlevel_indent(line, lnum, last, "#", 4, 8, NEST)
    }

    #[test]
    fn indent_level_is_indent_over_shiftwidth() {
        assert_eq!(lvl("code\n", 2, 10), 0);
        assert_eq!(lvl("    code\n", 2, 10), 1);
        assert_eq!(lvl("        code\n", 2, 10), 2);
        // Ragged indent truncates toward zero, as integer division does in C.
        assert_eq!(lvl("      code\n", 2, 10), 1);
    }

    #[test]
    fn blank_and_foldignore_lines_are_undefined_not_inherited() {
        // The flat model gave these the previous line's level, which dragged a
        // fold across the gap between two blocks.
        assert_eq!(lvl("\n", 5, 10), UNDEFINED_LEVEL);
        assert_eq!(lvl("   \t \n", 5, 10), UNDEFINED_LEVEL);
        // 'foldignore' default is "#": an unindented preprocessor/comment line
        // takes the surrounding level rather than tearing the fold in two.
        assert_eq!(lvl("#define X 1\n", 5, 10), UNDEFINED_LEVEL);
        assert_eq!(lvl("    # indented comment\n", 5, 10), UNDEFINED_LEVEL);

        // "first and last line can't be undefined, use level 0".
        assert_eq!(lvl("\n", 1, 10), 0);
        assert_eq!(lvl("\n", 10, 10), 0);
    }

    #[test]
    fn foldnestmax_clamps_the_level_it_does_not_drop_the_fold() {
        // 5 levels of indent with 'foldnestmax' 2 merges into level 2 rather
        // than losing the fold, which is what filter_folds did.
        let deep = "                    code\n"; // 20 spaces, sw=4 -> level 5
        assert_eq!(foldlevel_indent(deep, 2, 10, "#", 4, 8, NEST), 5);
        assert_eq!(foldlevel_indent(deep, 2, 10, "#", 4, 8, 2), 2);
        // A negative 'foldnestmax' clamps to 0, per MAX(0, w_p_fdn).
        assert_eq!(foldlevel_indent(deep, 2, 10, "#", 4, 8, -3), 0);
    }

    #[test]
    fn tabs_advance_to_the_next_tab_stop() {
        // A tab from column 0 with tab_width 8 lands on 8, so sw=4 gives 2.
        assert_eq!(foldlevel_indent("\tcode\n", 2, 10, "#", 4, 8, NEST), 2);
        // Two spaces then a tab still lands on 8, not 10.
        assert_eq!(foldlevel_indent("  \tcode\n", 2, 10, "#", 4, 8, NEST), 2);
    }

    fn fold(top: usize, len: usize) -> Fold {
        Fold {
            top,
            len,
            nested: Vec::new(),
            flags: FoldFlag::Level,
            small: TriState::None,
        }
    }

    fn spans(folds: &[Fold]) -> Vec<(usize, usize)> {
        folds.iter().map(|f| (f.top, f.len)).collect()
    }

    #[test]
    fn fold_find_locates_or_points_just_below() {
        let folds = vec![fold(5, 3), fold(10, 4), fold(20, 2)]; // 5-7, 10-13, 20-21
        assert_eq!(fold_find(&folds, 6), (0, true));
        assert_eq!(fold_find(&folds, 13), (1, true));
        // Not inside any fold: index of the first fold below it.
        assert_eq!(fold_find(&folds, 8), (1, false));
        assert_eq!(fold_find(&folds, 1), (0, false));
        // "careful: it can be beyond the end of the array!"
        assert_eq!(fold_find(&folds, 99), (3, false));
        assert_eq!(fold_find(&[], 4), (0, false));
    }

    #[test]
    fn fold_split_moves_nested_folds_to_the_right_half() {
        // Outer 10..=29; nested at absolute 12..=13 and 25..=26.
        let mut outer = fold(10, 20);
        outer.nested.push(fold(2, 2));
        outer.nested.push(fold(15, 2));
        let mut gap = vec![outer];

        // Split so the first part ends above 15 and the second starts below 20.
        fold_split(&mut gap, 0, 15, 20);

        assert_eq!(gap.len(), 2);
        assert_eq!(
            (gap[0].top, gap[0].len),
            (10, 5),
            "first part ends above top"
        );
        assert_eq!((gap[1].top, gap[1].len), (21, 9), "second starts below bot");
        assert_eq!(spans(&gap[0].nested), vec![(2, 2)], "nested above stays");
        assert_eq!(
            spans(&gap[1].nested),
            vec![(4, 2)],
            "nested below moved and was rebased onto the new parent top"
        );
    }

    #[test]
    fn fold_remove_truncates_splits_and_deletes() {
        // 2: fold starts above top, ends inside -> truncated to stop above top.
        let mut gap = vec![fold(5, 10)]; // 5..=14
        fold_remove(&mut gap, 10, 20);
        assert_eq!(spans(&gap), vec![(5, 5)], "truncated to 5..=9");

        // 3: fold spans the whole removed range -> split in two.
        let mut gap = vec![fold(5, 30)]; // 5..=34
        fold_remove(&mut gap, 10, 20);
        assert_eq!(spans(&gap), vec![(5, 5), (21, 14)], "split around 10..=20");

        // 4: fold entirely inside -> deleted. 5: fold containing bot -> starts below bot.
        let mut gap = vec![fold(12, 3), fold(18, 8)]; // 12..=14, 18..=25
        fold_remove(&mut gap, 10, 20);
        assert_eq!(
            spans(&gap),
            vec![(21, 5)],
            "inner deleted, last starts below bot"
        );

        // 1 and 6: folds outside the range are untouched.
        let mut gap = vec![fold(1, 3), fold(40, 2)];
        fold_remove(&mut gap, 10, 20);
        assert_eq!(spans(&gap), vec![(1, 3), (40, 2)]);
    }

    #[test]
    fn fold_merge_joins_adjacent_folds_and_rebases_children() {
        // 10..=14 followed immediately by 15..=19, each with one child.
        let mut a = fold(10, 5);
        a.nested.push(fold(1, 2)); // absolute 11..=12
        let mut b = fold(15, 5);
        b.nested.push(fold(2, 2)); // absolute 17..=18
        let mut gap = vec![a, b];

        fold_merge(&mut gap, 0, 1);

        assert_eq!(gap.len(), 1, "fp2 deleted");
        assert_eq!((gap[0].top, gap[0].len), (10, 10), "lengths summed");
        assert_eq!(
            spans(&gap[0].nested),
            vec![(1, 2), (7, 2)],
            "fp2's child rebased by fp1's length, so it still lands on 17"
        );
    }

    #[test]
    fn mark_adjust_shifts_folds_below_the_edit() {
        // Case 6: fold entirely below the edit only takes amount_after.
        let mut folds = vec![fold(10, 5), fold(30, 4)];
        fold_mark_adjust(&mut folds, 2, 3, 0, 2, false);
        assert_eq!(spans(&folds), vec![(12, 5), (32, 4)]);

        // Case 1: fold entirely above is untouched.
        let mut folds = vec![fold(2, 3), fold(40, 2)];
        fold_mark_adjust(&mut folds, 20, 21, 0, 5, false);
        assert_eq!(spans(&folds), vec![(2, 3), (45, 2)]);
    }

    #[test]
    fn mark_adjust_truncates_a_fold_the_edit_starts_inside() {
        // Case 2: the fold contains line1 and line2 is below it, so the fold
        // grows by `amount` rather than being dropped. A flat range list can
        // only shift or drop, which is why folds rotted across edits.
        let mut folds = vec![fold(5, 10)]; // lines 5..=14
        fold_mark_adjust(&mut folds, 8, 14, 3, 3, false);
        assert_eq!(
            spans(&folds),
            vec![(5, 13)],
            "fold grew by the inserted lines"
        );
    }

    #[test]
    fn mark_adjust_deletes_a_fold_whose_lines_all_went_away() {
        // Case 4 with amount == MAXLNUM: the fold sits entirely in the deleted
        // range and is removed, nested folds with it.
        let mut inner = fold(20, 3);
        inner.nested.push(fold(1, 2));
        let mut folds = vec![fold(5, 2), inner, fold(40, 2)];
        fold_mark_adjust(&mut folds, 19, 25, MAXLNUM, -7, false);
        assert_eq!(
            spans(&folds),
            vec![(5, 2), (33, 2)],
            "the contained fold is gone, the one below shifted up"
        );
    }

    #[test]
    fn mark_adjust_recurses_into_nested_folds() {
        // Case 3: the fold contains both line1 and line2, so it absorbs
        // amount_after and its children are adjusted in parent-relative space.
        let mut outer = fold(10, 20); // lines 10..=29
        outer.nested.push(fold(5, 4)); // absolute 14..=17
        let mut folds = vec![outer];
        fold_mark_adjust(&mut folds, 12, 13, 0, 4, false);
        assert_eq!(folds[0].len, 24, "outer fold absorbed the inserted lines");
        assert_eq!(
            spans(&folds[0].nested),
            vec![(9, 4)],
            "nested fold shifted within its parent, staying parent-relative"
        );
    }

    #[test]
    fn check_closed_threads_use_level_down_the_tree() {
        // An FD_LEVEL fold defers to 'foldlevel'...
        let lvl = fold(1, 5);
        let mut use_level = false;
        assert!(check_closed(&lvl, &mut use_level, 1, 0), "level 1 >= fdl 0");
        assert!(use_level, "and marks the subtree as level-driven");

        let mut use_level = false;
        assert!(!check_closed(&lvl, &mut use_level, 1, 2), "level 1 < fdl 2");

        // ...an explicitly closed fold is closed regardless of 'foldlevel'.
        let mut closed = fold(1, 5);
        closed.flags = FoldFlag::Closed;
        let mut use_level = false;
        assert!(check_closed(&closed, &mut use_level, 1, 9));
        assert!(
            !use_level,
            "an explicit flag does not make the subtree level-driven"
        );

        // ...but once an ancestor was FD_LEVEL, a child's own flag is ignored.
        let mut open = fold(1, 5);
        open.flags = FoldFlag::Open;
        let mut use_level = true;
        assert!(
            check_closed(&open, &mut use_level, 3, 1),
            "inherited use_level overrides the child's FD_OPEN"
        );
    }

    #[test]
    fn new_fold_level_hands_control_back_to_foldlevel() {
        // zM/zR do not stamp every fold: they reset the first level to FD_LEVEL
        // and move 'foldlevel'. Nested folds follow because FD_LEVEL covers them.
        let mut folds = vec![fold(1, 5), fold(10, 5)];
        folds[0].flags = FoldFlag::Open;
        folds[1].flags = FoldFlag::Closed;
        folds[0].nested.push(fold(2, 2));
        folds[0].nested[0].flags = FoldFlag::Open;

        let mut manual = true;
        new_fold_level(&mut folds, &mut manual);
        assert_eq!(folds[0].flags, FoldFlag::Level);
        assert_eq!(folds[1].flags, FoldFlag::Level);
        assert!(!manual, "w_fold_manual cleared");
        assert_eq!(
            folds[0].nested[0].flags,
            FoldFlag::Open,
            "only the first level is reset; FD_LEVEL already covers nested folds"
        );

        // Not manual: nothing to reset.
        let mut folds = vec![fold(1, 5)];
        folds[0].flags = FoldFlag::Closed;
        let mut manual = false;
        new_fold_level(&mut folds, &mut manual);
        assert_eq!(folds[0].flags, FoldFlag::Closed);
    }

    #[test]
    fn fold_defaults_match_the_c_struct() {
        // fd_flags defaults to FD_LEVEL and fd_small to kNone, so a freshly
        // built fold follows 'foldlevel' until something pins it.
        assert_eq!(FoldFlag::default(), FoldFlag::Level);
        assert_eq!(TriState::default(), TriState::None);
        let f = Fold {
            top: 3,
            len: 5,
            nested: Vec::new(),
            flags: FoldFlag::default(),
            small: TriState::default(),
        };
        assert_eq!(f.top + f.len - 1, 7, "top is inclusive, len counts lines");
    }
}
