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
#[derive(Debug, Clone, PartialEq, Eq)]
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
