//! Utility functions to traverse the unicode graphemes of a `Rope`'s text contents.
//!
//! Based on <https://github.com/cessen/led/blob/c4fa72405f510b7fd16052f90a598c429b3104a6/src/graphemes.rs>
use ropey::{str_utils::byte_to_char_idx, RopeSlice};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};
use unicode_width::UnicodeWidthStr;

use std::borrow::Cow;
use std::fmt::{self, Debug, Display};
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::NonNull;
use std::{slice, str};

use crate::chars::{char_is_whitespace, char_is_word};
use crate::LineEnding;

// ---------------------------------------------------------------------------
// Vim display options that decide how wide a grapheme is and where a soft-wrapped
// line may break: `ambiwidth`, `vartabstop`, `breakat`. Each is off/empty until
// `:set` opts in, so the default rendering is untouched. Thread-local, like the
// other `:set`-driven character tables (`chars::set_extra_keyword_chars`): the
// options are set and read on the editor thread.
// ---------------------------------------------------------------------------

thread_local! {
    /// vim `ambiwidth=double`.
    static AMBIWIDTH_DOUBLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// vim `vartabstop` — the variable tab-stop widths (empty = fixed `tabstop`).
    static VARTABSTOP: std::cell::RefCell<Vec<usize>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// vim `breakat` — the characters a wrapped line may break at (empty = the
    /// default "any non-word character" rule).
    static BREAKAT: std::cell::RefCell<Vec<char>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// vim `ambiwidth`: `double` renders East Asian *ambiguous*-width characters
/// (Greek, Cyrillic, box drawing, …) two cells wide, as CJK terminals do;
/// `single` (the default) renders them one cell wide.
pub fn set_ambiwidth_double(double: bool) {
    AMBIWIDTH_DOUBLE.with(|a| a.set(double));
}

pub fn ambiwidth_double() -> bool {
    AMBIWIDTH_DOUBLE.with(std::cell::Cell::get)
}

/// vim `vartabstop`: a list of tab widths — the first tab stop is `stops[0]`
/// columns in, the next `stops[1]` further, and the last width repeats for every
/// stop past the end of the list. An empty list restores the fixed `tabstop`.
pub fn set_vartabstop(stops: Vec<usize>) {
    let stops: Vec<usize> = stops.into_iter().filter(|&n| n > 0).collect();
    VARTABSTOP.with(|v| *v.borrow_mut() = stops);
}

/// The width of the tab at `visual_x` under the `vartabstop` list: walk the stops
/// until one lies past the cursor, repeating the last width. Pure — unit tested.
pub fn vartab_width_at(visual_x: usize, stops: &[usize]) -> usize {
    let mut pos = 0usize;
    for (i, &w) in stops.iter().enumerate() {
        pos += w;
        if pos > visual_x {
            return pos - visual_x;
        }
        // The final width repeats forever, so once the list runs out keep
        // stepping by it until the stop passes `visual_x`.
        if i + 1 == stops.len() {
            let step = w;
            let past = visual_x - pos;
            let n = past / step + 1;
            return pos + n * step - visual_x;
        }
    }
    1
}

/// vim `breakat`: the characters a wrapped line may be broken at. Empty (the
/// default) keeps zemacs's own rule — break at any non-word character.
pub fn set_breakat(chars: Vec<char>) {
    BREAKAT.with(|b| *b.borrow_mut() = chars);
}

/// Whether soft wrap may break *before* `c`, per `breakat` when it is set;
/// `None` when the option is empty and the default rule applies.
fn breakat_allows(c: char) -> Option<bool> {
    BREAKAT.with(|b| {
        let b = b.borrow();
        (!b.is_empty()).then(|| b.contains(&c))
    })
}

#[inline]
pub fn tab_width_at(visual_x: usize, tab_width: u16) -> usize {
    // vim `vartabstop`: variable-width tab stops replace the fixed `tabstop`.
    if let Some(width) = VARTABSTOP.with(|v| {
        let stops = v.borrow();
        (!stops.is_empty()).then(|| vartab_width_at(visual_x, &stops))
    }) {
        return width;
    }
    tab_width as usize - (visual_x % tab_width as usize)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grapheme<'a> {
    Newline,
    Tab { width: usize },
    Other { g: GraphemeStr<'a> },
}

impl<'a> Grapheme<'a> {
    pub fn new_decoration(g: &'static str) -> Grapheme<'a> {
        assert_ne!(g, "\t");
        Grapheme::new(g.into(), 0, 0)
    }

    pub fn new(g: GraphemeStr<'a>, visual_x: usize, tab_width: u16) -> Grapheme<'a> {
        match g {
            g if g == "\t" => Grapheme::Tab {
                width: tab_width_at(visual_x, tab_width),
            },
            _ if LineEnding::from_str(&g).is_some() => Grapheme::Newline,
            _ => Grapheme::Other { g },
        }
    }

    pub fn change_position(&mut self, visual_x: usize, tab_width: u16) {
        if let Grapheme::Tab { width } = self {
            *width = tab_width_at(visual_x, tab_width)
        }
    }

    /// Returns the a visual width of this grapheme,
    #[inline]
    pub fn width(&self) -> usize {
        match *self {
            // width is not cached because we are dealing with
            // ASCII almost all the time which already has a fastpath
            // it's okay to convert to u16 here because no codepoint has a width larger
            // than 2 and graphemes are usually atmost two visible codepoints wide
            Grapheme::Other { ref g } => grapheme_width(g),
            Grapheme::Tab { width } => width,
            Grapheme::Newline => 1,
        }
    }

    pub fn is_whitespace(&self) -> bool {
        !matches!(&self, Grapheme::Other { g } if !g.chars().next().is_some_and(char_is_whitespace))
    }

    // TODO currently word boundaries are used for softwrapping.
    // This works best for programming languages and well for prose.
    // This could however be improved in the future by considering unicode
    // character classes but
    //
    // vim `breakat` overrides the rule when it is set: a wrapped line may only be
    // broken at one of its characters (`:set breakat=\ ` wraps at spaces only).
    pub fn is_word_boundary(&self) -> bool {
        match self {
            Grapheme::Other { g } => match g.chars().next() {
                Some(c) => match breakat_allows(c) {
                    Some(allowed) => allowed,
                    None => !char_is_word(c),
                },
                None => true,
            },
            // A tab or a newline always ends a word (and `breakat` lists the tab
            // as a break character by default).
            _ => true,
        }
    }
}

impl Display for Grapheme<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Grapheme::Newline => write!(f, " "),
            Grapheme::Tab { width } => {
                for _ in 0..width {
                    write!(f, " ")?;
                }
                Ok(())
            }
            Grapheme::Other { ref g } => {
                write!(f, "{g}")
            }
        }
    }
}

#[must_use]
pub fn grapheme_width(g: &str) -> usize {
    if g.as_bytes()[0] <= 127 {
        // Fast-path ascii.
        // Point 1: theoretically, ascii control characters should have zero
        // width, but in our case we actually want them to have width: if they
        // show up in text, we want to treat them as textual elements that can
        // be edited.  So we can get away with making all ascii single width
        // here.
        // Point 2: we're only examining the first codepoint here, which means
        // we're ignoring graphemes formed with combining characters.  However,
        // if it starts with ascii, it's going to be a single-width grapeheme
        // regardless, so, again, we can get away with that here.
        // Point 3: we're only examining the first _byte_.  But for utf8, when
        // checking for ascii range values only, that works.
        1
    } else if ambiwidth_double() {
        // vim `ambiwidth=double`: East Asian *ambiguous* characters take two
        // cells (the CJK width table), which is what a CJK-configured terminal
        // renders. Unambiguous characters are unaffected.
        UnicodeWidthStr::width_cjk(g).max(1)
    } else {
        // We use max(1) here because all grapeheme clusters--even illformed
        // ones--should have at least some width so they can be edited
        // properly.
        // TODO properly handle unicode width for all codepoints
        // example of where unicode width is currently wrong: 🤦🏼‍♂️ (taken from https://hsivonen.fi/string-length/)
        UnicodeWidthStr::width(g).max(1)
    }
}

// NOTE: for byte indexing versions of these functions see `RopeSliceExt`'s
// `floor_grapheme_boundary` and `ceil_grapheme_boundary` and the rope grapheme iterators.

#[must_use]
pub fn nth_prev_grapheme_boundary(slice: RopeSlice, char_idx: usize, n: usize) -> usize {
    // Bounds check
    debug_assert!(char_idx <= slice.len_chars());

    // We work with bytes for this, so convert.
    let mut byte_idx = slice.char_to_byte(char_idx);

    // Get the chunk with our byte index in it.
    let (mut chunk, mut chunk_byte_idx, mut chunk_char_idx, _) = slice.chunk_at_byte(byte_idx);

    // Set up the grapheme cursor.
    let mut gc = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);

    // Find the previous grapheme cluster boundary.
    for _ in 0..n {
        loop {
            match gc.prev_boundary(chunk, chunk_byte_idx) {
                Ok(None) => return 0,
                Ok(Some(n)) => {
                    byte_idx = n;
                    break;
                }
                Err(GraphemeIncomplete::PrevChunk) => {
                    let (a, b, c, _) = slice.chunk_at_byte(chunk_byte_idx - 1);
                    chunk = a;
                    chunk_byte_idx = b;
                    chunk_char_idx = c;
                }
                Err(GraphemeIncomplete::PreContext(n)) => {
                    let ctx_chunk = slice.chunk_at_byte(n - 1).0;
                    gc.provide_context(ctx_chunk, n - ctx_chunk.len());
                }
                _ => unreachable!(),
            }
        }
    }
    let tmp = byte_to_char_idx(chunk, byte_idx - chunk_byte_idx);
    chunk_char_idx + tmp
}

/// Finds the previous grapheme boundary before the given char position.
#[must_use]
#[inline(always)]
pub fn prev_grapheme_boundary(slice: RopeSlice, char_idx: usize) -> usize {
    nth_prev_grapheme_boundary(slice, char_idx, 1)
}

#[must_use]
pub fn nth_next_grapheme_boundary(slice: RopeSlice, char_idx: usize, n: usize) -> usize {
    // Bounds check
    debug_assert!(char_idx <= slice.len_chars());

    // We work with bytes for this, so convert.
    let mut byte_idx = slice.char_to_byte(char_idx);

    // Get the chunk with our byte index in it.
    let (mut chunk, mut chunk_byte_idx, mut chunk_char_idx, _) = slice.chunk_at_byte(byte_idx);

    // Set up the grapheme cursor.
    let mut gc = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);

    // Find the nth next grapheme cluster boundary.
    for _ in 0..n {
        loop {
            match gc.next_boundary(chunk, chunk_byte_idx) {
                Ok(None) => return slice.len_chars(),
                Ok(Some(n)) => {
                    byte_idx = n;
                    break;
                }
                Err(GraphemeIncomplete::NextChunk) => {
                    chunk_byte_idx += chunk.len();
                    let (a, _, c, _) = slice.chunk_at_byte(chunk_byte_idx);
                    chunk = a;
                    chunk_char_idx = c;
                }
                Err(GraphemeIncomplete::PreContext(n)) => {
                    let ctx_chunk = slice.chunk_at_byte(n - 1).0;
                    gc.provide_context(ctx_chunk, n - ctx_chunk.len());
                }
                _ => unreachable!(),
            }
        }
    }
    let tmp = byte_to_char_idx(chunk, byte_idx - chunk_byte_idx);
    chunk_char_idx + tmp
}

/// Finds the next grapheme boundary after the given char position.
#[must_use]
#[inline(always)]
pub fn next_grapheme_boundary(slice: RopeSlice, char_idx: usize) -> usize {
    nth_next_grapheme_boundary(slice, char_idx, 1)
}

/// Returns the passed char index if it's already a grapheme boundary,
/// or the next grapheme boundary char index if not.
#[must_use]
#[inline]
pub fn ensure_grapheme_boundary_next(slice: RopeSlice, char_idx: usize) -> usize {
    if char_idx == 0 {
        char_idx
    } else {
        next_grapheme_boundary(slice, char_idx - 1)
    }
}

/// Returns the passed char index if it's already a grapheme boundary,
/// or the prev grapheme boundary char index if not.
#[must_use]
#[inline]
pub fn ensure_grapheme_boundary_prev(slice: RopeSlice, char_idx: usize) -> usize {
    if char_idx == slice.len_chars() {
        char_idx
    } else {
        prev_grapheme_boundary(slice, char_idx + 1)
    }
}

/// A highly compressed Cow<'a, str> that holds
/// atmost u31::MAX bytes and is readonly
pub struct GraphemeStr<'a> {
    ptr: NonNull<u8>,
    len: u32,
    phantom: PhantomData<&'a str>,
}

impl GraphemeStr<'_> {
    const MASK_OWNED: u32 = 1 << 31;

    fn compute_len(&self) -> usize {
        (self.len & !Self::MASK_OWNED) as usize
    }
}

impl Deref for GraphemeStr<'_> {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        unsafe {
            let bytes = slice::from_raw_parts(self.ptr.as_ptr(), self.compute_len());
            str::from_utf8_unchecked(bytes)
        }
    }
}

impl Drop for GraphemeStr<'_> {
    fn drop(&mut self) {
        if self.len & Self::MASK_OWNED != 0 {
            // free allocation
            unsafe {
                drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    self.ptr.as_ptr(),
                    self.compute_len(),
                )));
            }
        }
    }
}

impl<'a> From<&'a str> for GraphemeStr<'a> {
    fn from(g: &'a str) -> Self {
        GraphemeStr {
            ptr: unsafe { NonNull::new_unchecked(g.as_bytes().as_ptr() as *mut u8) },
            len: i32::try_from(g.len()).unwrap() as u32,
            phantom: PhantomData,
        }
    }
}

impl From<String> for GraphemeStr<'_> {
    fn from(g: String) -> Self {
        let len = g.len();
        let ptr = Box::into_raw(g.into_bytes().into_boxed_slice()) as *mut u8;
        GraphemeStr {
            ptr: unsafe { NonNull::new_unchecked(ptr) },
            len: (i32::try_from(len).unwrap() as u32) | Self::MASK_OWNED,
            phantom: PhantomData,
        }
    }
}

impl<'a> From<Cow<'a, str>> for GraphemeStr<'a> {
    fn from(g: Cow<'a, str>) -> Self {
        match g {
            Cow::Borrowed(g) => g.into(),
            Cow::Owned(g) => g.into(),
        }
    }
}

impl<T: Deref<Target = str>> PartialEq<T> for GraphemeStr<'_> {
    fn eq(&self, other: &T) -> bool {
        self.deref() == other.deref()
    }
}
impl PartialEq<str> for GraphemeStr<'_> {
    fn eq(&self, other: &str) -> bool {
        self.deref() == other
    }
}
impl Eq for GraphemeStr<'_> {}
impl Debug for GraphemeStr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(self.deref(), f)
    }
}
impl Display for GraphemeStr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(self.deref(), f)
    }
}
impl Clone for GraphemeStr<'_> {
    fn clone(&self) -> Self {
        self.deref().to_owned().into()
    }
}

#[cfg(test)]
mod vim_display_option_tests {
    use super::*;

    /// vim `ambiwidth=double`: an East Asian *ambiguous* character (`α`, `─`)
    /// takes two cells; unambiguous ones (`a`, `字`) are unchanged.
    #[test]
    fn ambiwidth_double_widens_ambiguous_chars() {
        set_ambiwidth_double(false);
        assert_eq!(grapheme_width("α"), 1);
        assert_eq!(grapheme_width("─"), 1);

        set_ambiwidth_double(true);
        assert_eq!(grapheme_width("α"), 2);
        assert_eq!(grapheme_width("─"), 2);
        // ASCII and genuinely wide characters are not affected either way.
        assert_eq!(grapheme_width("a"), 1);
        assert_eq!(grapheme_width("字"), 2);

        set_ambiwidth_double(false);
    }

    /// vim `vartabstop=4,8,12`: stops at columns 4, 12 and 24, and the last width
    /// (12) repeats past the end of the list.
    #[test]
    fn vartabstop_stops_then_repeats_last_width() {
        let stops = [4, 8, 12];
        // First stop: 4 columns in.
        assert_eq!(vartab_width_at(0, &stops), 4);
        assert_eq!(vartab_width_at(3, &stops), 1);
        // Second: 8 further (column 12).
        assert_eq!(vartab_width_at(4, &stops), 8);
        assert_eq!(vartab_width_at(11, &stops), 1);
        // Third: 12 further (column 24), then 12 forever (36, 48, …).
        assert_eq!(vartab_width_at(12, &stops), 12);
        assert_eq!(vartab_width_at(24, &stops), 12);
        assert_eq!(vartab_width_at(30, &stops), 6);
        assert_eq!(vartab_width_at(36, &stops), 12);
    }

    /// `:set vartabstop` replaces the fixed `tabstop` used by `tab_width_at`, and
    /// clearing it restores the fixed stops.
    #[test]
    fn set_vartabstop_overrides_fixed_tab_width() {
        assert_eq!(tab_width_at(0, 8), 8);

        set_vartabstop(vec![4, 8]);
        assert_eq!(tab_width_at(0, 8), 4);
        assert_eq!(tab_width_at(4, 8), 8);

        set_vartabstop(Vec::new());
        assert_eq!(tab_width_at(0, 8), 8);
    }

    /// vim `breakat`: only the listed characters end a word for soft wrap, so
    /// `breakat= ` (space only) stops zemacs breaking a wrapped line at `-` or `.`.
    #[test]
    fn breakat_restricts_soft_wrap_break_points() {
        let boundary = |c: &str| Grapheme::Other { g: c.into() }.is_word_boundary();

        // Default: any non-word character is a break point.
        set_breakat(Vec::new());
        assert!(boundary("-"));
        assert!(boundary(" "));
        assert!(!boundary("a"));

        // `:set breakat=\ ` — break at spaces only.
        set_breakat(vec![' ']);
        assert!(boundary(" "));
        assert!(!boundary("-"));
        assert!(!boundary("."));
        assert!(!boundary("a"));

        // `:set breakat=\ -` — spaces and hyphens.
        set_breakat(vec![' ', '-']);
        assert!(boundary("-"));
        assert!(!boundary("."));

        set_breakat(Vec::new());
    }
}
