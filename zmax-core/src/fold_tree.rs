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

/// vim `fline_T` (`fold.c`): the cursor the level getters and
/// [`fold_update_iems_recurse`] thread through the buffer.
///
/// ```c
/// typedef struct {
///   win_T *wp;              // window
///   linenr_T lnum;                // current line number
///   linenr_T off;                 // offset between lnum and real line number
///   linenr_T lnum_save;           // line nr used by foldUpdateIEMSRecurse()
///   int lvl;                      // current level (-1 for undefined)
///   int lvl_next;                 // level used for next line
///   int start;                    // number of folds that are forced to start at
///                                 // this line.
///   int end;                      // level of fold that is forced to end below
///                                 // this line
///   int had_end;                  // level of fold that is forced to end above
///                                 // this line (copy of "end" of prev. line)
/// } fline_T;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FLine {
    /// Current line number, relative to the start of the enclosing fold.
    pub lnum: i64,
    /// Offset between `lnum` and the real buffer line number.
    pub off: i64,
    /// Line number the level was actually read from; differs from `lnum` when
    /// the lines between carry [`UNDEFINED_LEVEL`].
    pub lnum_save: i64,
    /// Level of the current line, [`UNDEFINED_LEVEL`] when undefined.
    pub lvl: i32,
    /// Level to use for the next line.
    pub lvl_next: i32,
    /// Number of folds forced to start at this line (marker method).
    pub start: i32,
    /// Level of the fold forced to end below this line.
    pub end: i32,
    /// Level of the fold forced to end above this line.
    pub had_end: i32,
}

impl Default for FLine {
    fn default() -> Self {
        // The initial values foldUpdateIEMS sets before the walk.
        Self {
            lnum: 0,
            off: 0,
            lnum_save: 0,
            lvl: 0,
            lvl_next: UNDEFINED_LEVEL,
            start: 0,
            end: MAX_LEVEL + 1,
            had_end: MAX_LEVEL + 1,
        }
    }
}

/// vim's `LevelGetter` (`typedef void (*LevelGetter)(fline_T *)`): fills in
/// `flp.lvl`/`flp.lvl_next` for `flp.lnum + flp.off`.
///
/// The C compares the function pointer against `foldlevelMarker` and friends to
/// vary behaviour, so the identity predicates are part of the interface.
pub trait LevelGetter {
    /// Set `flp.lvl` (and `flp.lvl_next` where the method defines it) for the
    /// current line.
    fn get_level(&mut self, flp: &mut FLine);

    /// vim `getlevel == foldlevelMarker`.
    fn is_marker(&self) -> bool {
        false
    }
    /// vim `getlevel == foldlevelExpr`.
    fn is_expr(&self) -> bool {
        false
    }
    /// vim `getlevel == foldlevelSyntax`.
    fn is_syntax(&self) -> bool {
        false
    }

    /// The three methods whose folds can move as a side effect of an edit, so
    /// the walk has to keep going until the fold's real end is found. The C
    /// spells this triple out at each site.
    fn needs_end_search(&self) -> bool {
        self.is_marker() || self.is_expr() || self.is_syntax()
    }

    /// vim's `prev_lnum`/`prev_lnum_lvl` globals, which make the previous
    /// line's level available to `foldlevel()` under `'foldmethod=expr'`.
    fn set_prev(&mut self, _lnum: i64, _lvl: i32) {}
}

/// vim `foldUpdateIEMSRecurse` (`fold.c`), the tree builder.
///
/// "Update a fold that starts at `flp->lnum`. At this line there is always a
/// valid foldlevel, and its level >= `level`. […] Remove any folds from
/// `startlnum` up to here at this level. Recursively update nested folds."
///
/// The C explains why it is this involved rather than a rebuild:
///
/// > All this would be a lot simpler if all folds in the range would be deleted
/// > and then created again. But we would lose all information about the folds,
/// > even when making changes that don't affect the folding (e.g. "vj~").
///
/// That preservation is the point — it is what keeps a hand-opened fold open
/// across a recompute, which the flat model threw away on every edit.
///
/// Returns `bot`, "which may have been increased for lines that also need to be
/// updated as a result of a detected change in the fold".
///
/// `buf_lines` is the buffer's line count; the C reads it off the window.
/// `got_int` (interrupt polling) is dropped.
#[allow(clippy::too_many_arguments)]
pub fn fold_update_iems_recurse(
    gap: &mut Vec<Fold>,
    level: i32,
    startlnum: i64,
    flp: &mut FLine,
    getlevel: &mut dyn LevelGetter,
    mut bot: i64,
    topflags: FoldFlag,
    buf_lines: i64,
    fold_manual: &mut bool,
    fold_changed: &mut bool,
) -> i64 {
    let mut fp: Option<usize> = None;

    // If using the marker method, the start line is not the start of a fold
    // at the level we're dealing with and the level is non-zero, we must use
    // the previous fold.  But ignore a fold that starts at or below
    // startlnum, it must be deleted.
    if getlevel.is_marker() && flp.start <= flp.lvl - level && flp.lvl > 0 {
        let (i, _) = fold_find(gap, startlnum - 1);
        fp = Some(i);
        if i >= gap.len() || (gap[i].top as i64) >= startlnum {
            fp = None;
        }
    }

    // C: `int lvl = level;` — set at the top of every iteration before any exit,
    // so the initial value is never read.
    let mut lvl;
    let mut startlnum2 = startlnum;
    let firstlnum = flp.lnum; // first lnum we got
    let mut finish = false;
    let linecount = buf_lines - flp.off;

    flp.lnum_save = flp.lnum;
    loop {
        // Set "lvl" to the level of line "flp->lnum".  When flp->start is set
        // and after the first line of the fold, set the level to zero to
        // force the fold to end.  Do the same when had_end is set: Previous
        // line was marked as end of a fold.
        lvl = flp.lvl.min(MAX_LEVEL);
        if flp.lnum > firstlnum && (level > lvl - flp.start || level >= flp.had_end) {
            lvl = 0;
        }

        if let (true, Some(fpi)) = (flp.lnum > bot && !finish, fp) {
            if !getlevel.needs_end_search() {
                break;
            }
            let mut i = 0;
            if lvl >= level {
                // Compute how deep the folds currently are, if it's deeper
                // than "lvl" then some must be deleted, need to update
                // at least one nested fold.
                let mut ll = flp.lnum - gap[fpi].top as i64;
                let mut cur = &gap[fpi].nested;
                loop {
                    let (j, found) = fold_find(cur, ll);
                    if !found {
                        break;
                    }
                    i += 1;
                    ll -= cur[j].top as i64;
                    cur = &cur[j].nested;
                }
            }
            if lvl < level + i {
                let fp_top = gap[fpi].top as i64;
                let (j, found) = fold_find(&gap[fpi].nested, flp.lnum - fp_top);
                if found {
                    bot =
                        gap[fpi].nested[j].top as i64 + gap[fpi].nested[j].len as i64 - 1 + fp_top;
                }
            } else if (gap[fpi].top as i64 + gap[fpi].len as i64) <= flp.lnum && lvl >= level {
                finish = true;
            } else {
                break;
            }
        }

        // At the start of the first nested fold and at the end of the current
        // fold: check if existing folds at this level, before the current
        // one, need to be deleted or truncated.
        if fp.is_none()
            && (lvl != level
                || flp.lnum_save >= bot
                || flp.start != 0
                || flp.had_end <= MAX_LEVEL
                || flp.lnum == linecount)
        {
            // Remove or update folds that have lines between startlnum and
            // firstlnum.
            loop {
                // set concat to 1 if it's allowed to concatenate this fold
                // with a previous one that touches it.
                let concat: i64 = if flp.start != 0 || flp.had_end <= MAX_LEVEL {
                    0
                } else {
                    1
                };

                // Find an existing fold to re-use.  Preferably one that
                // includes startlnum, otherwise one that ends just before
                // startlnum or starts after it. The C chains these with `||`
                // and each foldFind reassigns `fp`, so the order matters.
                let mut reuse = false;
                let mut idx = 0usize;
                if !gap.is_empty() {
                    let (i, found) = fold_find(gap, startlnum);
                    idx = i;
                    reuse = found;
                    if !reuse && idx < gap.len() && (gap[idx].top as i64) <= firstlnum {
                        reuse = true;
                    }
                    if !reuse {
                        let (i2, found2) = fold_find(gap, firstlnum - concat);
                        idx = i2;
                        reuse = found2;
                    }
                    if !reuse
                        && idx < gap.len()
                        && ((lvl < level && (gap[idx].top as i64) < flp.lnum)
                            || (lvl >= level && (gap[idx].top as i64) <= flp.lnum_save))
                    {
                        reuse = true;
                    }
                }

                if reuse {
                    let fp_top = gap[idx].top as i64;
                    let fp_len = gap[idx].len as i64;
                    if fp_top + fp_len + concat > firstlnum {
                        // Use existing fold for the new fold.
                        let mut idx = idx;
                        if fp_top == firstlnum {
                            // We have found a fold beginning exactly where we want one.
                        } else if fp_top >= startlnum {
                            if fp_top > firstlnum {
                                // We will move the start of this fold up, hence we move all
                                // nested folds (with relative line numbers) down.
                                fold_mark_adjust_recurse(
                                    &mut gap[idx].nested,
                                    0,
                                    MAXLNUM,
                                    fp_top - firstlnum,
                                    0,
                                    false,
                                );
                            } else {
                                // Will move fold down, move nested folds relatively up.
                                fold_mark_adjust_recurse(
                                    &mut gap[idx].nested,
                                    0,
                                    firstlnum - fp_top - 1,
                                    MAXLNUM,
                                    fp_top - firstlnum,
                                    false,
                                );
                            }
                            gap[idx].len = (fp_len + (fp_top - firstlnum)).max(0) as usize;
                            gap[idx].top = firstlnum.max(0) as usize;
                            gap[idx].small = TriState::None;
                            *fold_changed = true;
                        } else if (flp.start != 0 && lvl == level) || firstlnum != startlnum {
                            // There was a fold spanning from above startlnum to below
                            // firstlnum; there is now a break in it, so split.
                            let (breakstart, breakend) = if firstlnum != startlnum {
                                (startlnum, firstlnum)
                            } else {
                                (flp.lnum, flp.lnum)
                            };
                            let top = gap[idx].top as i64;
                            fold_remove(&mut gap[idx].nested, breakstart - top, breakend - top);
                            fold_split(gap, idx, breakstart, breakend - 1);
                            idx += 1;
                            if getlevel.needs_end_search() {
                                finish = true;
                            }
                        }
                        if gap[idx].top as i64 == startlnum && concat == 1 && idx != 0 {
                            let prev = idx - 1;
                            if gap[prev].top as i64 + gap[prev].len as i64 == gap[idx].top as i64 {
                                fold_merge(gap, prev, idx);
                                idx = prev;
                            }
                        }
                        fp = Some(idx);
                        break;
                    }
                    if fp_top >= startlnum {
                        // A fold that starts at or after startlnum and stops
                        // before the new fold must be deleted.
                        gap.remove(idx);
                    } else {
                        // A fold has some lines above startlnum, truncate it
                        // to stop just above startlnum.
                        gap[idx].len = (startlnum - fp_top).max(0) as usize;
                        let new_len = gap[idx].len as i64;
                        fold_mark_adjust_recurse(
                            &mut gap[idx].nested,
                            new_len,
                            MAXLNUM,
                            MAXLNUM,
                            0,
                            false,
                        );
                        *fold_changed = true;
                    }
                } else {
                    // Insert new fold.
                    let i = if gap.is_empty() {
                        0
                    } else {
                        idx.min(gap.len())
                    };
                    fold_insert(gap, i);
                    gap[i].top = firstlnum.max(0) as usize;
                    // The new fold continues until bot, unless we find the
                    // end earlier.
                    gap[i].len = (bot - firstlnum + 1).max(0) as usize;
                    // When the containing fold is open, the new fold is open.
                    // The new fold is closed if the fold above it is closed.
                    // The first fold depends on the containing fold.
                    if topflags == FoldFlag::Open {
                        *fold_manual = true;
                        gap[i].flags = FoldFlag::Open;
                    } else if i == 0 {
                        gap[i].flags = topflags;
                        if topflags != FoldFlag::Level {
                            *fold_manual = true;
                        }
                    } else {
                        gap[i].flags = gap[i - 1].flags;
                    }
                    gap[i].small = TriState::None;
                    if getlevel.needs_end_search() {
                        finish = true;
                    }
                    *fold_changed = true;
                    fp = Some(i);
                    break;
                }
            }
        }

        if lvl < level || flp.lnum > linecount {
            // Found a line with a lower foldlevel, this fold ends just above
            // "flp->lnum".
            break;
        }

        // The fold includes the line "flp->lnum" and "flp->lnum_save".
        if let (true, Some(i)) = (lvl > level, fp) {
            // There is a nested fold, handle it recursively.
            // At least do one line (can happen when finish is true).
            bot = bot.max(flp.lnum);
            let fp_top = gap[i].top as i64;
            let fp_flags = gap[i].flags;

            // Line numbers in the nested fold are relative to the start of
            // this fold.
            flp.lnum = flp.lnum_save - fp_top;
            flp.off += fp_top;
            let mut nested = std::mem::take(&mut gap[i].nested);
            bot = fold_update_iems_recurse(
                &mut nested,
                level + 1,
                startlnum2 - fp_top,
                flp,
                getlevel,
                bot - fp_top,
                fp_flags,
                buf_lines,
                fold_manual,
                fold_changed,
            );
            gap[i].nested = nested;
            let fp_top = gap[i].top as i64;
            flp.lnum += fp_top;
            flp.lnum_save += fp_top;
            flp.off -= fp_top;
            bot += fp_top;
            startlnum2 = flp.lnum;

            // This fold may end at the same line, don't incr. flp->lnum.
        } else {
            // Get the level of the next line, then continue the loop to check
            // if it ends there.
            // Skip over undefined lines, to find the foldlevel after it.
            flp.lnum = flp.lnum_save;
            let ll = flp.lnum + 1;
            loop {
                // Make the previous level available to foldlevel().
                getlevel.set_prev(flp.lnum, flp.lvl);
                flp.lnum += 1;
                if flp.lnum > linecount {
                    break;
                }
                flp.lvl = flp.lvl_next;
                getlevel.get_level(flp);
                if flp.lvl >= 0 || flp.had_end <= MAX_LEVEL {
                    break;
                }
            }
            getlevel.set_prev(0, 0);
            if flp.lnum > linecount {
                break;
            }

            // leave flp->lnum_save to lnum of the line that was used to get
            // the level, flp->lnum to the lnum of the next line.
            flp.lnum_save = flp.lnum;
            flp.lnum = ll;
        }
    }

    let Some(mut fpi) = fp else {
        return bot;
    };

    // Get here when:
    // lvl < level: the folds ends just above "flp->lnum"
    // lvl >= level: fold continues below "bot"

    // Current fold at least extends until lnum.
    let fp_top = gap[fpi].top as i64;
    if (gap[fpi].len as i64) < flp.lnum - fp_top {
        gap[fpi].len = (flp.lnum - fp_top).max(0) as usize;
        gap[fpi].small = TriState::None;
        *fold_changed = true;
    } else if fp_top + gap[fpi].len as i64 > linecount {
        // running into the end of the buffer (deleted last line)
        gap[fpi].len = (linecount - fp_top + 1).max(0) as usize;
    }

    // Delete contained folds from the end of the last one found until where
    // we stopped looking.
    let fp_top = gap[fpi].top as i64;
    fold_remove(
        &mut gap[fpi].nested,
        startlnum2 - fp_top,
        flp.lnum - 1 - fp_top,
    );

    if lvl < level {
        // End of fold found, update the length when it got shorter.
        let fp_top = gap[fpi].top as i64;
        if gap[fpi].len as i64 != flp.lnum - fp_top {
            if fp_top + gap[fpi].len as i64 - 1 > bot {
                // fold continued below bot
                if getlevel.needs_end_search() {
                    // marker method: truncate the fold and make sure the
                    // previously included lines are processed again
                    bot = fp_top + gap[fpi].len as i64 - 1;
                    gap[fpi].len = (flp.lnum - fp_top).max(0) as usize;
                } else {
                    // indent or expr method: split fold to create a new one
                    // below bot
                    fold_split(gap, fpi, flp.lnum, bot);
                }
            } else {
                gap[fpi].len = (flp.lnum - fp_top).max(0) as usize;
            }
            *fold_changed = true;
        }
    }

    // delete following folds that end before the current line
    loop {
        let next = fpi + 1;
        if next >= gap.len() || (gap[next].top as i64) > flp.lnum {
            break;
        }
        if gap[next].top as i64 + gap[next].len as i64 > flp.lnum {
            if (gap[next].top as i64) < flp.lnum {
                // Make fold that includes lnum start at lnum.
                let n_top = gap[next].top as i64;
                fold_mark_adjust_recurse(
                    &mut gap[next].nested,
                    0,
                    flp.lnum - n_top - 1,
                    MAXLNUM,
                    n_top - flp.lnum,
                    false,
                );
                gap[next].len = (gap[next].len as i64 - (flp.lnum - n_top)).max(0) as usize;
                gap[next].top = flp.lnum.max(0) as usize;
                *fold_changed = true;
            }
            if lvl >= level {
                // merge new fold with existing fold that follows
                fold_merge(gap, fpi, next);
            }
            break;
        }
        *fold_changed = true;
        gap.remove(next);
        fpi = fpi.min(gap.len().saturating_sub(1));
    }

    // Need to redraw the lines we inspected, which might be further down than
    // was asked for.
    bot.max(flp.lnum - 1)
}

/// vim `foldlevelMarker` (`fold.c`), the `'foldmethod=marker'` level getter.
///
/// "Requires that `flp->lvl` is set to the fold level of the previous line!
/// Careful: This means you can't call this function twice on the same line."
/// The level is a running count of markers, not a property of the line, so the
/// caller must thread the previous level in — [`fold_update`] does.
///
/// ```c
/// if (*s == cstart && strncmp(s + 1, startmarker, foldstartmarkerlen - 1) == 0) {
///   s += foldstartmarkerlen;
///   if (ascii_isdigit(*s)) {
///     int n = atoi(s);
///     if (n > 0) { flp->lvl = n; flp->lvl_next = n; flp->start = MAX(n - start_lvl, 1); }
///   } else { flp->lvl++; flp->lvl_next++; flp->start++; }
/// } else if (*s == cend && strncmp(s + 1, foldendmarker + 1, foldendmarkerlen - 1) == 0) {
///   s += foldendmarkerlen;
///   if (ascii_isdigit(*s)) {
///     int n = atoi(s);
///     if (n > 0) { flp->lvl = n; flp->lvl_next = n - 1;
///                  flp->lvl_next = MIN(flp->lvl_next, start_lvl); }
///   } else { flp->lvl_next--; }
/// }
/// ```
///
/// A marker may carry an explicit level (`{{{2`), and one line may hold several
/// markers — both are why this scans the whole line rather than testing for a
/// prefix. `lvl_next` is floored at 0: "The level can't go negative, must be
/// missing a start marker."
pub fn foldlevel_marker(line: &str, start_marker: &str, end_marker: &str, flp: &mut FLine) {
    let start_lvl = flp.lvl;

    // Default: no start found, next level is same as current level
    flp.start = 0;
    flp.lvl_next = flp.lvl;

    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        if !start_marker.is_empty() && rest.starts_with(start_marker) {
            i += start_marker.len();
            match leading_level(&line[i..]) {
                // found startmarker: set flp->lvl
                Some(n) => {
                    flp.lvl = n;
                    flp.lvl_next = n;
                    flp.start = (n - start_lvl).max(1);
                }
                None => {
                    flp.lvl += 1;
                    flp.lvl_next += 1;
                    flp.start += 1;
                }
            }
        } else if !end_marker.is_empty() && rest.starts_with(end_marker) {
            i += end_marker.len();
            match leading_level(&line[i..]) {
                // found endmarker: set flp->lvl_next
                Some(n) => {
                    flp.lvl = n;
                    // never start a fold with an end marker
                    flp.lvl_next = (n - 1).min(start_lvl);
                }
                None => flp.lvl_next -= 1,
            }
        } else {
            // vim `MB_PTR_ADV`: step one character, not one byte.
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }

    // The level can't go negative, must be missing a start marker.
    flp.lvl_next = flp.lvl_next.max(0);
}

/// vim's `ascii_isdigit(*s)` + `atoi(s)` after a marker: the explicit level in
/// `{{{2`. `None` when no digits follow, and when they parse to 0 — the C only
/// takes the branch `if (n > 0)`.
fn leading_level(s: &str) -> Option<i32> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<i32>().ok().filter(|n| *n > 0)
}

/// The `'foldmethod=marker'` [`LevelGetter`], over the buffer's lines.
pub struct MarkerLevelGetter<'a> {
    /// Buffer lines, 0-indexed; vim line `n` is `lines[n - 1]`.
    pub lines: &'a [&'a str],
    /// The open half of `'foldmarker'` (default `{{{`).
    pub start_marker: &'a str,
    /// The close half of `'foldmarker'` (default `}}}`).
    pub end_marker: &'a str,
}

impl LevelGetter for MarkerLevelGetter<'_> {
    fn is_marker(&self) -> bool {
        true
    }

    fn get_level(&mut self, flp: &mut FLine) {
        let lnum = flp.lnum + flp.off;
        if lnum < 1 || lnum > self.lines.len() as i64 {
            flp.start = 0;
            flp.lvl_next = flp.lvl;
            return;
        }
        let line = self.lines[(lnum - 1) as usize];
        foldlevel_marker(line, self.start_marker, self.end_marker, flp);
    }
}

/// vim `foldlevelSyntax` (`fold.c`), the `'foldmethod=syntax'` level getter:
///
/// ```c
/// static void foldlevelSyntax(fline_T *flp)
/// {
///   linenr_T lnum = flp->lnum + flp->off;
///
///   // Use the maximum fold level at the start of this line and the next.
///   flp->lvl = syn_get_foldlevel(flp->wp, lnum);
///   flp->start = 0;
///   if (lnum < flp->wp->w_buffer->b_ml.ml_line_count) {
///     int n = syn_get_foldlevel(flp->wp, lnum + 1);
///     if (n > flp->lvl) {
///       flp->start = n - flp->lvl;        // fold(s) start here
///       flp->lvl = n;
///     }
///   }
/// }
/// ```
///
/// vim reads the level from its own syntax engine's fold regions. zmax has no
/// such regions, so `levels` is supplied by the caller — the nesting depth of
/// the tree-sitter `function.around`/`class.around` captures covering each line.
/// The shape above is what matters and is ported exactly: a line takes the
/// maximum of its own level and the next line's, so a fold that opens on the
/// following line is recorded as starting here.
///
/// `levels` is 0-indexed; `levels[n - 1]` is vim's line `n`.
pub struct SyntaxLevelGetter<'a> {
    /// Per-line nesting depth of the syntax fold regions.
    pub levels: &'a [i32],
}

impl LevelGetter for SyntaxLevelGetter<'_> {
    fn is_syntax(&self) -> bool {
        true
    }

    fn get_level(&mut self, flp: &mut FLine) {
        let lnum = flp.lnum + flp.off;
        let at = |n: i64| -> i32 {
            if n < 1 || n > self.levels.len() as i64 {
                0
            } else {
                self.levels[(n - 1) as usize]
            }
        };
        // Use the maximum fold level at the start of this line and the next.
        flp.lvl = at(lnum);
        flp.start = 0;
        if lnum < self.levels.len() as i64 {
            let n = at(lnum + 1);
            if n > flp.lvl {
                flp.start = n - flp.lvl; // fold(s) start here
                flp.lvl = n;
            }
        }
        flp.lvl_next = at(lnum + 1);
    }
}

/// Per-line nesting depth for [`SyntaxLevelGetter`], from inclusive 0-based
/// `(start, end)` line ranges: a line's level is how many ranges cover it.
///
/// The ranges are the tree-sitter captures zmax uses in place of vim's syntax
/// fold regions; nested captures (a method inside a class) give the inner lines
/// a higher level, which is what lets the builder nest the folds.
pub fn syntax_levels(ranges: &[(usize, usize)], line_count: usize) -> Vec<i32> {
    let mut levels = vec![0; line_count];
    for &(s, e) in ranges {
        // The range's *first* line keeps the enclosing level. That is vim's
        // convention for a syntax fold region and it is load-bearing twice over:
        // `SyntaxLevelGetter` takes the maximum of a line's level and the next
        // one's, which is what pulls the header into the fold it opens; and it is
        // the only thing separating two siblings that touch. Counting the header
        // too left `fn foo() {…}` immediately followed by `fn bar() {…}` at level
        // 1 on every line, with no dip between them, so the tree builder saw one
        // run and produced a single fold over both.
        for level in levels
            .iter_mut()
            .take(e.min(line_count.saturating_sub(1)) + 1)
            .skip(s + 1)
        {
            *level += 1;
        }
    }
    levels
}

/// The `'foldmethod=indent'` [`LevelGetter`], over the buffer's lines.
pub struct IndentLevelGetter<'a> {
    /// Buffer lines, 0-indexed; vim line `n` is `lines[n - 1]`.
    pub lines: &'a [&'a str],
    /// vim `'foldignore'`.
    pub foldignore: &'a str,
    /// vim `'shiftwidth'`.
    pub shiftwidth: usize,
    /// vim `'tabstop'`.
    pub tab_width: usize,
    /// vim `'foldnestmax'`.
    pub foldnestmax: i32,
}

impl LevelGetter for IndentLevelGetter<'_> {
    fn get_level(&mut self, flp: &mut FLine) {
        let lnum = flp.lnum + flp.off;
        if lnum < 1 || lnum > self.lines.len() as i64 {
            flp.lvl = 0;
            return;
        }
        flp.lvl = foldlevel_indent(
            self.lines[(lnum - 1) as usize],
            lnum as usize,
            self.lines.len(),
            self.foldignore,
            self.shiftwidth,
            self.tab_width,
            self.foldnestmax,
        );
    }
}

/// vim `foldUpdateIEMS` (`fold.c`) for a whole-buffer update: derive the fold
/// tree from the level sequence.
///
/// This is the driver loop the C runs after picking a `LevelGetter`. Only the
/// full-buffer case is ported (`top = 1`, `bot = line count`), which is what
/// `w_foldinvalid` forces anyway; the incremental range case, the `diff`
/// context padding and the `syntax` `bot` extension are not here yet.
///
/// ```c
/// // Backup to a line for which the fold level is defined.  Since it's
/// // always defined for line one, we will stop there.
/// fline.lvl = -1;
/// for (; !got_int; fline.lnum--) {
///   fline.lvl_next = -1;
///   getlevel(&fline);
///   if (fline.lvl >= 0) break;
/// }
/// ```
pub fn fold_update(
    gap: &mut Vec<Fold>,
    getlevel: &mut dyn LevelGetter,
    buf_lines: i64,
    fold_manual: &mut bool,
    fold_changed: &mut bool,
) {
    if buf_lines <= 0 {
        gap.clear();
        return;
    }
    let mut flp = FLine {
        lnum: 1,
        ..FLine::default()
    };

    if getlevel.is_marker() {
        // The C does not scan backwards for `marker`: the level is a running
        // marker count, so it primes at `top` with the level already in `flp`
        // (0 for a whole-buffer update) and reads forward.
        flp.lnum = 1;
        flp.lvl = 0;
        getlevel.get_level(&mut flp);
    } else {
        // Backup to a line for which the fold level is defined; line one always is.
        flp.lvl = UNDEFINED_LEVEL;
        while flp.lnum >= 1 {
            flp.lvl_next = UNDEFINED_LEVEL;
            getlevel.get_level(&mut flp);
            if flp.lvl >= 0 {
                break;
            }
            flp.lnum -= 1;
        }
    }

    let mut start = flp.lnum;
    let mut end = buf_lines;
    if start > end {
        end = start;
    }

    loop {
        if flp.lnum > buf_lines || flp.lnum > end {
            break;
        }
        // A level 1 fold starts at a line with foldlevel > 0.
        if flp.lvl > 0 {
            end = fold_update_iems_recurse(
                gap,
                1,
                start,
                &mut flp,
                getlevel,
                end,
                FoldFlag::Level,
                buf_lines,
                fold_manual,
                fold_changed,
            );
            start = flp.lnum;
        } else {
            if flp.lnum == buf_lines {
                break;
            }
            flp.lnum += 1;
            flp.lvl = flp.lvl_next;
            getlevel.get_level(&mut flp);
        }
    }

    // There can't be any folds from start until end now.
    fold_remove(gap, start, end);
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
/// `fd_small` is applied last, per the C:
///
/// ```c
/// // Small fold isn't closed anyway.
/// if (closed) { checkSmall(wp, fp, lnum_off); if (fp->fd_small == kTrue) closed = false; }
/// ```
///
/// A fold shorter than `'foldminlines'` is not *deleted*, it merely declines to
/// display closed — the distinction the old `filter_folds` lost by dropping the
/// range outright.
pub fn check_closed(
    fold: &Fold,
    use_level: &mut bool,
    level: i32,
    foldlevel: i32,
    foldminlines: usize,
) -> bool {
    let closed = if *use_level || fold.flags == FoldFlag::Level {
        *use_level = true;
        level >= foldlevel
    } else {
        fold.flags == FoldFlag::Closed
    };
    closed && !is_small(fold, foldminlines)
}

/// vim `checkSmall` (`fold.c`): is the fold shorter than `'foldminlines'`?
///
/// ```c
/// if (fp->fd_len > wp->w_p_fml) {
///   fp->fd_small = kFalse;
/// } else {
///   int count = 0;
///   for (int n = 0; n < fp->fd_len; n++) {
///     count += plines_win_nofold(wp, fp->fd_top + lnum_off + n);
///     if (count > wp->w_p_fml) { fp->fd_small = kFalse; return; }
///   }
///   fp->fd_small = kTrue;
/// }
/// ```
///
/// vim counts *screen* lines, so a wrapped line counts more than once. This
/// counts buffer lines, which is the same answer whenever nothing in the fold
/// wraps; a fold of long wrapped lines can still be treated as small here where
/// vim would not.
fn is_small(fold: &Fold, foldminlines: usize) -> bool {
    fold.len <= foldminlines
}

/// The closed folds as absolute inclusive `(start, end)` line ranges, resolving
/// each fold's flag against `foldlevel` exactly as [`check_closed`] does.
///
/// A closed fold hides its whole extent, so its nested folds are not descended
/// into — matching vim, where a closed outer fold makes the inner ones moot.
/// `use_level` is threaded down each branch independently, so one subtree being
/// level-driven does not affect its siblings.
///
/// This is the bridge the renderer needs: it consumes line ranges, and the tree
/// is the thing that knows which folds are actually closed.
pub fn closed_ranges(gap: &[Fold], foldlevel: i32, foldminlines: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    // vim's caller (`hasFoldingWin`) starts `level` at 0 for the top level and
    // increments on descent, so a top-level fold closes when `0 >= 'foldlevel'`
    // — which is what makes `'foldlevel'` "the highest level left open".
    collect_closed(gap, foldlevel, foldminlines, 0, 0, false, &mut out);
    out
}

fn collect_closed(
    gap: &[Fold],
    foldlevel: i32,
    foldminlines: usize,
    level: i32,
    off: usize,
    use_level: bool,
    out: &mut Vec<(usize, usize)>,
) {
    for f in gap {
        let abs_top = off + f.top;
        // Each sibling gets its own copy: `use_levelp` in the C is per-descent.
        let mut ul = use_level;
        if check_closed(f, &mut ul, level, foldlevel, foldminlines) {
            out.push((abs_top, abs_top + f.len.saturating_sub(1)));
        } else {
            collect_closed(
                &f.nested,
                foldlevel,
                foldminlines,
                level + 1,
                abs_top,
                ul,
                out,
            );
        }
    }
}

/// Every fold in the tree as an absolute inclusive `(start, end)` line range,
/// outermost first, regardless of open/closed state.
///
/// Unlike [`closed_ranges`] this keeps descending into open folds, because the
/// caller wants the buffer's whole fold structure rather than what is currently
/// hidden. Ranges are 1-based like the rest of this module; callers holding
/// 0-based line indices must subtract one.
pub fn all_ranges(gap: &[Fold]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    collect_all(gap, 0, &mut out);
    out
}

fn collect_all(gap: &[Fold], off: usize, out: &mut Vec<(usize, usize)>) {
    for f in gap {
        let abs_top = off + f.top;
        out.push((abs_top, abs_top + f.len.saturating_sub(1)));
        collect_all(&f.nested, abs_top, out);
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

    fn build(lines: &[&str]) -> Vec<Fold> {
        let mut gap = Vec::new();
        let mut getter = IndentLevelGetter {
            lines,
            foldignore: "#",
            shiftwidth: 4,
            tab_width: 8,
            foldnestmax: MAX_LEVEL,
        };
        let (mut manual, mut changed) = (false, false);
        fold_update(
            &mut gap,
            &mut getter,
            lines.len() as i64,
            &mut manual,
            &mut changed,
        );
        gap
    }

    fn build_marker(lines: &[&str]) -> Vec<Fold> {
        let mut gap = Vec::new();
        let mut getter = MarkerLevelGetter {
            lines,
            start_marker: "{{{",
            end_marker: "}}}",
        };
        let (mut manual, mut changed) = (false, false);
        fold_update(
            &mut gap,
            &mut getter,
            lines.len() as i64,
            &mut manual,
            &mut changed,
        );
        gap
    }

    #[test]
    fn marker_level_counts_markers_across_lines() {
        // The level is a running count, so the previous line's level is the
        // input. A start marker raises it, an end marker lowers the *next*
        // line's level — the line holding `}}}` is still inside the fold.
        let mut flp = FLine {
            lvl: 0,
            ..FLine::default()
        };
        foldlevel_marker("# section {{{", "{{{", "}}}", &mut flp);
        assert_eq!((flp.lvl, flp.lvl_next, flp.start), (1, 1, 1));

        flp.lvl = flp.lvl_next;
        foldlevel_marker("body", "{{{", "}}}", &mut flp);
        assert_eq!((flp.lvl, flp.lvl_next, flp.start), (1, 1, 0));

        flp.lvl = flp.lvl_next;
        foldlevel_marker("# }}}", "{{{", "}}}", &mut flp);
        assert_eq!(
            (flp.lvl, flp.lvl_next),
            (1, 0),
            "the end-marker line is still level 1; the next line drops to 0"
        );
    }

    #[test]
    fn marker_level_honours_an_explicit_level_and_never_goes_negative() {
        // `{{{2` sets the level outright rather than incrementing.
        let mut flp = FLine {
            lvl: 0,
            ..FLine::default()
        };
        foldlevel_marker("x {{{2", "{{{", "}}}", &mut flp);
        assert_eq!((flp.lvl, flp.lvl_next, flp.start), (2, 2, 2));

        // `}}}1` sets lvl and clamps lvl_next to the level we came in at, so an
        // end marker never *starts* a fold.
        flp.lvl = 3;
        foldlevel_marker("x }}}1", "{{{", "}}}", &mut flp);
        assert_eq!((flp.lvl, flp.lvl_next), (1, 0));

        // A stray end marker cannot drive the level below zero.
        flp.lvl = 0;
        foldlevel_marker("}}}", "{{{", "}}}", &mut flp);
        assert_eq!(flp.lvl_next, 0, "missing start marker must not go negative");

        // Two markers on one line net out.
        flp.lvl = 0;
        foldlevel_marker("a {{{ b }}}", "{{{", "}}}", &mut flp);
        assert_eq!((flp.lvl, flp.lvl_next), (1, 0));
    }

    #[test]
    fn syntax_levels_count_covering_ranges_and_nest() {
        // A class spanning 0..=9 with a method at 2..=4: the method's body sits
        // at depth 2, the rest of the class at 1. Each range's first line — the
        // `class`/`def` header — keeps the enclosing level, which is what makes
        // the level rise *into* the fold and what keeps two touching regions
        // apart.
        let levels = syntax_levels(&[(0, 9), (2, 4)], 12);
        assert_eq!(levels, vec![0, 1, 1, 2, 2, 1, 1, 1, 1, 1, 0, 0]);

        // Two functions back to back: the level dips on each header, so the
        // builder sees two runs rather than one block from the first line of the
        // first to the last line of the second.
        assert_eq!(
            syntax_levels(&[(0, 3), (4, 6)], 7),
            vec![0, 1, 1, 1, 0, 1, 1]
        );

        // A range running past the buffer is clamped, not a panic.
        assert_eq!(syntax_levels(&[(0, 99)], 3), vec![0, 1, 1]);
        assert!(syntax_levels(&[], 0).is_empty());
    }

    #[test]
    fn fold_update_nests_syntax_folds_from_capture_depth() {
        // vim takes the max of this line's level and the next, so a fold that
        // opens on the following line is recorded as starting here.
        let levels = syntax_levels(&[(0, 5), (2, 4)], 8);
        let mut gap = Vec::new();
        let mut getter = SyntaxLevelGetter { levels: &levels };
        let (mut manual, mut changed) = (false, false);
        fold_update(&mut gap, &mut getter, 8, &mut manual, &mut changed);

        assert_eq!(spans(&gap), vec![(1, 6)], "outer capture becomes one fold");
        // Both folds come out as exactly the captures they were built from:
        // 0-based 0..=5 and 2..=4, which are vim lines 1..=6 and 3..=5. Taking
        // the maximum of a line's level and the next one's is what puts each
        // header inside the fold it opens rather than above it.
        assert_eq!(
            all_ranges(&gap),
            vec![(1, 6), (3, 5)],
            "each capture folds as itself, header included"
        );
        assert_eq!(
            spans(&gap[0].nested),
            vec![(2, 3)],
            "the nested fold is stored relative to its parent"
        );
    }

    #[test]
    fn fold_update_builds_a_fold_per_marker_pair() {
        // The shape of the user's ~/.zshrc: flat `#{{{` / `#}}}` sections.
        let lines = [
            "#{{{ one\n", // 1
            "a\n",        // 2
            "#}}}\n",     // 3
            "between\n",  // 4
            "#{{{ two\n", // 5
            "b\n",        // 6
            "c\n",        // 7
            "#}}}\n",     // 8
        ];
        let folds = build_marker(&lines);
        assert_eq!(
            spans(&folds),
            vec![(1, 3), (5, 4)],
            "one fold per marker pair, each spanning marker line to marker line"
        );
    }

    #[test]
    fn fold_update_derives_a_fold_per_indented_block() {
        // Two sibling blocks, the shape of a shell rc file. This is the case
        // that folded 1 of 20: the level sequence has two runs above level 0,
        // so two folds must come out of it.
        let lines = [
            "if true; then\n",  // 1  lvl 0
            "    a\n",          // 2  lvl 1
            "    b\n",          // 3  lvl 1
            "fi\n",             // 4  lvl 0
            "if false; then\n", // 5  lvl 0
            "    c\n",          // 6  lvl 1
            "    d\n",          // 7  lvl 1
            "fi\n",             // 8  lvl 0
        ];
        let folds = build(&lines);
        // The header lines are level 0, so they are *not* in the fold: vim's
        // indent method folds the indented run only, leaving `if`/`fi` visible.
        // The old indent_fold_ranges pushed (header, end) instead.
        assert_eq!(
            spans(&folds),
            vec![(2, 2), (6, 2)],
            "one fold per indented run, header line left outside"
        );
        assert!(folds.iter().all(|f| f.nested.is_empty()));
    }

    #[test]
    fn fold_update_nests_deeper_blocks_inside_their_parent() {
        let lines = [
            "outer\n",          // 1  lvl 0
            "    mid\n",        // 2  lvl 1
            "        inner\n",  // 3  lvl 2
            "        inner2\n", // 4  lvl 2
            "    mid2\n",       // 5  lvl 1
            "done\n",           // 6  lvl 0
        ];
        let folds = build(&lines);
        // Level-1 run is lines 2..=5; "outer"/"done" are level 0 and stay out.
        assert_eq!(spans(&folds), vec![(2, 4)], "top-level fold covers 2..=5");
        assert_eq!(
            spans(&folds[0].nested),
            vec![(1, 2)],
            "level-2 run nests inside, parent-relative: parent top 2 + child top 1 = absolute 3..=4"
        );
    }

    #[test]
    fn fold_update_does_not_drag_a_fold_over_a_blank_gap() {
        // The blank line is UNDEFINED_LEVEL, so it takes the level of the
        // surrounding lines rather than extending the first block. Under the
        // old "blank inherits the previous level" rule the two blocks fused.
        let lines = [
            "a\n",     // 1 lvl 0
            "    x\n", // 2 lvl 1
            "\n",      // 3 undefined
            "b\n",     // 4 lvl 0
            "    y\n", // 5 lvl 1
            "end\n",   // 6 lvl 0
        ];
        let folds = build(&lines);
        assert_eq!(
            spans(&folds),
            vec![(2, 1), (5, 1)],
            "two separate folds on the indented lines, not one spanning the gap"
        );
    }

    #[test]
    fn fold_update_leaves_nothing_when_the_buffer_is_flat() {
        let lines = ["a\n", "b\n", "c\n"];
        assert!(build(&lines).is_empty(), "no indent, no folds");
        assert!(build(&[]).is_empty());
    }

    #[test]
    fn closed_ranges_resolves_flags_against_foldlevel() {
        // Outer 10..=29 with a nested fold at absolute 15..=18.
        let mut outer = fold(10, 20);
        outer.nested.push(fold(5, 4));
        let gap = vec![outer];

        // zM: 'foldlevel' 0 closes the outer fold, which hides everything —
        // the nested fold is not descended into.
        assert_eq!(closed_ranges(&gap, 0, 1), vec![(10, 29)]);

        // 'foldlevel' 1 leaves level-1 open and closes level 2.
        assert_eq!(closed_ranges(&gap, 1, 1), vec![(15, 18)]);

        // zR: 'foldlevel' at the deepest level opens everything.
        assert!(closed_ranges(&gap, 9, 1).is_empty());
    }

    #[test]
    fn foldminlines_stops_a_short_fold_closing_without_deleting_it() {
        // vim's 'foldminlines' is a display rule: a fold at or under the limit
        // declines to show closed but still exists. filter_folds dropped the
        // range entirely, so the fold could never be closed at any setting.
        let gap = vec![fold(5, 2), fold(20, 9)];

        // Default 'foldminlines' 1: the 2-line fold is not small, both close.
        assert_eq!(closed_ranges(&gap, 0, 1), vec![(5, 6), (20, 28)]);

        // 'foldminlines' 3: the 2-line fold is small and stays open; the
        // 9-line one still closes.
        assert_eq!(closed_ranges(&gap, 0, 3), vec![(20, 28)]);

        // The fold is still there — it just isn't closed.
        assert_eq!(gap.len(), 2);

        // 'foldminlines' 0 closes even a single-line fold.
        assert_eq!(closed_ranges(&[fold(7, 1)], 0, 0), vec![(7, 7)]);
        assert!(closed_ranges(&[fold(7, 1)], 0, 1).is_empty());
    }

    #[test]
    fn closed_ranges_honours_an_explicitly_opened_fold() {
        // This is what the flat model could not do: a fold the user opened by
        // hand stays open at 'foldlevel' 0, while its level-driven sibling shuts.
        let mut opened = fold(1, 5);
        opened.flags = FoldFlag::Open;
        opened.nested.push(fold(1, 2)); // absolute 2..=3, still FD_LEVEL
        let sibling = fold(20, 4);
        let gap = vec![opened, sibling];

        assert_eq!(
            closed_ranges(&gap, 0, 1),
            vec![(2, 3), (20, 23)],
            "hand-opened fold stays open; its FD_LEVEL child and the sibling close"
        );
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
        assert!(
            check_closed(&lvl, &mut use_level, 1, 0, 1),
            "level 1 >= fdl 0"
        );
        assert!(use_level, "and marks the subtree as level-driven");

        let mut use_level = false;
        assert!(
            !check_closed(&lvl, &mut use_level, 1, 2, 1),
            "level 1 < fdl 2"
        );

        // ...an explicitly closed fold is closed regardless of 'foldlevel'.
        let mut closed = fold(1, 5);
        closed.flags = FoldFlag::Closed;
        let mut use_level = false;
        assert!(check_closed(&closed, &mut use_level, 1, 9, 1));
        assert!(
            !use_level,
            "an explicit flag does not make the subtree level-driven"
        );

        // ...but once an ancestor was FD_LEVEL, a child's own flag is ignored.
        let mut open = fold(1, 5);
        open.flags = FoldFlag::Open;
        let mut use_level = true;
        assert!(
            check_closed(&open, &mut use_level, 3, 1, 1),
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
