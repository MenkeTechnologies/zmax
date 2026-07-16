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
    /// vim `emoji` (default on): emoji are full width. Off makes them ambiguous
    /// width — one cell, or two under `ambiwidth=double`.
    static EMOJI: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    /// vim `isprint` — which characters below U+0100 are shown as themselves.
    /// `None` (until `:set isprint=…`) leaves every character printable, i.e.
    /// zmax's own rendering.
    static ISPRINT: std::cell::RefCell<Option<[bool; 256]>> =
        const { std::cell::RefCell::new(None) };
    /// vim `display` contains `uhex`: unprintable characters render as `<xx>`
    /// rather than `^C` / `~C`.
    static DISPLAY_UHEX: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// vim `emoji`: when on (the default) emoji are full width, which is what the
/// Unicode width table already says. `:set noemoji` makes them *ambiguous* width
/// instead — a single cell, or two under `ambiwidth=double`.
pub fn set_emoji(on: bool) {
    EMOJI.with(|e| e.set(on));
}

fn emoji_enabled() -> bool {
    EMOJI.with(std::cell::Cell::get)
}

/// The Unicode `Emoji_Presentation=Yes` ranges — the characters vim's `emoji`
/// option governs. Text-presentation pictographs (`✓`, `⚠`, `☺`) are excluded,
/// exactly as the option's documentation says ("This excludes 'text emoji'
/// characters, which are normally displayed as single width").
#[rustfmt::skip]
const EMOJI_PRESENTATION: &[(u32, u32)] = &[
    (0x231A, 0x231B), (0x23E9, 0x23EC), (0x23F0, 0x23F0), (0x23F3, 0x23F3),
    (0x25FD, 0x25FE), (0x2614, 0x2615), (0x2648, 0x2653), (0x267F, 0x267F),
    (0x2693, 0x2693), (0x26A1, 0x26A1), (0x26AA, 0x26AB), (0x26BD, 0x26BE),
    (0x26C4, 0x26C5), (0x26CE, 0x26CE), (0x26D4, 0x26D4), (0x26EA, 0x26EA),
    (0x26F2, 0x26F3), (0x26F5, 0x26F5), (0x26FA, 0x26FA), (0x26FD, 0x26FD),
    (0x2705, 0x2705), (0x270A, 0x270B), (0x2728, 0x2728), (0x274C, 0x274C),
    (0x274E, 0x274E), (0x2753, 0x2755), (0x2757, 0x2757), (0x2795, 0x2797),
    (0x27B0, 0x27B0), (0x27BF, 0x27BF), (0x2B1B, 0x2B1C), (0x2B50, 0x2B50),
    (0x2B55, 0x2B55), (0x1F004, 0x1F004), (0x1F0CF, 0x1F0CF), (0x1F18E, 0x1F18E),
    (0x1F191, 0x1F19A), (0x1F1E6, 0x1F1FF), (0x1F201, 0x1F201), (0x1F21A, 0x1F21A),
    (0x1F22F, 0x1F22F), (0x1F232, 0x1F236), (0x1F238, 0x1F23A), (0x1F250, 0x1F251),
    (0x1F300, 0x1F320), (0x1F32D, 0x1F335), (0x1F337, 0x1F37C), (0x1F37E, 0x1F393),
    (0x1F3A0, 0x1F3CA), (0x1F3CF, 0x1F3D3), (0x1F3E0, 0x1F3F0), (0x1F3F4, 0x1F3F4),
    (0x1F3F8, 0x1F43E), (0x1F440, 0x1F440), (0x1F442, 0x1F4FC), (0x1F4FF, 0x1F53D),
    (0x1F54B, 0x1F54E), (0x1F550, 0x1F567), (0x1F57A, 0x1F57A), (0x1F595, 0x1F596),
    (0x1F5A4, 0x1F5A4), (0x1F5FB, 0x1F64F), (0x1F680, 0x1F6C5), (0x1F6CC, 0x1F6CC),
    (0x1F6D0, 0x1F6D2), (0x1F6D5, 0x1F6D7), (0x1F6DC, 0x1F6DF), (0x1F6EB, 0x1F6EC),
    (0x1F6F4, 0x1F6FC), (0x1F7E0, 0x1F7EB), (0x1F7F0, 0x1F7F0), (0x1F90C, 0x1F93A),
    (0x1F93C, 0x1F945), (0x1F947, 0x1F9FF), (0x1FA70, 0x1FA7C), (0x1FA80, 0x1FA88),
    (0x1FA90, 0x1FABD), (0x1FABF, 0x1FAC5), (0x1FACE, 0x1FADB), (0x1FAE0, 0x1FAE8),
    (0x1FAF0, 0x1FAF8),
];

/// Whether `c` is an emoji-presentation character (the set vim's `emoji` option
/// widens / narrows). Pure — unit tested.
pub fn is_emoji_presentation(c: char) -> bool {
    let c = c as u32;
    EMOJI_PRESENTATION
        .binary_search_by(|&(lo, hi)| {
            if c < lo {
                std::cmp::Ordering::Greater
            } else if c > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// vim `isprint`: the characters (below U+0100) that are displayed as
/// themselves. The value is a comma list of characters, decimal codes, ranges
/// (`a-b`, `128-140`), `@` (every alphabetic character) and `^`-prefixed
/// exclusions — the same syntax as `isfname`. ASCII 32..126 are always
/// printable, whatever the option says. An empty value restores zmax's own
/// rendering (every character printable). Pure — unit tested.
pub fn parse_isprint(spec: &str) -> [bool; 256] {
    let mut table = [false; 256];
    let code = |s: &str| -> Option<u32> {
        if let Ok(n) = s.parse::<u32>() {
            (n < 256).then_some(n)
        } else {
            let mut it = s.chars();
            match (it.next(), it.next()) {
                (Some(c), None) if (c as u32) < 256 => Some(c as u32),
                _ => None,
            }
        }
    };
    for raw in spec.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        let (item, printable) = match item.strip_prefix('^') {
            Some(rest) if !rest.is_empty() => (rest, false),
            _ => (item, true),
        };
        if item == "@" {
            for (n, slot) in table.iter_mut().enumerate() {
                if char::from_u32(n as u32).is_some_and(char::is_alphabetic) {
                    *slot = printable;
                }
            }
            continue;
        }
        // A range `a-b` / `128-140` (a lone `-` is the literal hyphen).
        if let Some((a, b)) = item.split_once('-') {
            if !a.is_empty() && !b.is_empty() {
                if let (Some(lo), Some(hi)) = (code(a), code(b)) {
                    for slot in table.iter_mut().take(hi as usize + 1).skip(lo as usize) {
                        *slot = printable;
                    }
                    continue;
                }
            }
        }
        if let Some(c) = code(item) {
            table[c as usize] = printable;
        }
    }
    // "The characters from space (ASCII 32) to '~' (ASCII 126) are always
    // displayed directly, even when they are not included in 'isprint'."
    for slot in table.iter_mut().take(127).skip(32) {
        *slot = true;
    }
    table
}

/// vim `isprint`: an empty value restores the untouched (all-printable)
/// rendering.
pub fn set_isprint(spec: &str) {
    let table = (!spec.trim().is_empty()).then(|| parse_isprint(spec));
    ISPRINT.with(|p| *p.borrow_mut() = table);
}

/// vim `display` contains `uhex`: show unprintable characters as `<xx>`.
pub fn set_display_uhex(on: bool) {
    DISPLAY_UHEX.with(|u| u.set(on));
}

fn display_uhex() -> bool {
    DISPLAY_UHEX.with(std::cell::Cell::get)
}

/// vim's default `isprint` (`@,161-255`) — the table `display=uhex` falls back on
/// when `isprint` itself was never `:set`.
fn default_isprint() -> [bool; 256] {
    parse_isprint("@,161-255")
}

/// How an unprintable character is displayed, per vim's table:
///
/// ```text
///   0 -  31   "^@" - "^_"        128 - 159   "~@" - "~_"
///      127    "^?"               160 - 254   "| " - "|~"
///                                    255     "~?"
/// ```
///
/// With `display=uhex` every unprintable character is `<xx>` instead. `None`
/// when the character is printable (which is every character until `:set
/// isprint` / `:set display=uhex` says otherwise, and always for U+0100 and
/// above — vim only classifies the first 256 codepoints). Pure — unit tested.
pub fn unprintable_repr(c: char) -> Option<String> {
    let code = c as u32;
    if code >= 256 || (32..127).contains(&code) {
        return None;
    }
    let uhex = display_uhex();
    let table = ISPRINT.with(|p| *p.borrow());
    let table = match (table, uhex) {
        (Some(t), _) => t,
        (None, true) => default_isprint(),
        (None, false) => return None,
    };
    if table[code as usize] {
        return None;
    }
    if uhex {
        return Some(format!("<{code:02x}>"));
    }
    let repr = match code {
        0..=31 => format!("^{}", char::from(code as u8 + 64)),
        127 => "^?".to_string(),
        128..=159 => format!("~{}", char::from((code - 64) as u8)),
        255 => "~?".to_string(),
        _ => format!("|{}", char::from((code - 128) as u8)),
    };
    Some(repr)
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
/// default) keeps zmax's own rule — break at any non-word character.
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
            // vim `isprint` / `display=uhex`: a character the option calls
            // unprintable is drawn as `^C` / `~C` / `|c` (or `<xx>` under
            // `uhex`) instead of itself.
            _ => match single_char(&g).and_then(unprintable_repr) {
                Some(repr) => Grapheme::Other { g: repr.into() },
                None => Grapheme::Other { g },
            },
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

/// The single `char` a grapheme cluster consists of, or `None` when it is a
/// multi-codepoint cluster (which vim's `isprint` never covers).
fn single_char(g: &str) -> Option<char> {
    let mut it = g.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
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
        //
        // Exception: the printable-ASCII decorations `isprint` / `display=uhex`
        // substitute for an unprintable character (`^A`, `~@`, `<a0>`) are more
        // than one character wide. A *real* cluster that starts with ASCII is
        // always one cell (whatever follows it is a zero-width combining mark,
        // which is never ASCII), so only an all-printable-ASCII run of length > 1
        // can be one of those decorations.
        let bytes = g.as_bytes();
        if bytes.len() > 1 && bytes.iter().all(|b| (32..127).contains(b)) {
            bytes.len()
        } else {
            1
        }
    } else if !emoji_enabled() && g.chars().next().is_some_and(is_emoji_presentation) {
        // vim `noemoji`: emoji stop being full width and become *ambiguous*
        // width — one cell, or two under `ambiwidth=double`.
        if ambiwidth_double() {
            2
        } else {
            1
        }
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
    /// `breakat= ` (space only) stops zmax breaking a wrapped line at `-` or `.`.
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

    /// vim `emoji` (on by default): emoji keep their full width. `:set noemoji`
    /// makes them ambiguous width — one cell, two under `ambiwidth=double`. Text
    /// pictographs (`⚠`, which is not `Emoji_Presentation`) are never touched.
    #[test]
    fn noemoji_narrows_emoji_to_ambiguous_width() {
        set_ambiwidth_double(false);
        set_emoji(true);
        assert_eq!(grapheme_width("🚀"), 2);
        assert_eq!(grapheme_width("⌚"), 2, "U+231A is Emoji_Presentation");

        set_emoji(false);
        assert_eq!(grapheme_width("🚀"), 1, ":set noemoji => single cell");
        assert_eq!(grapheme_width("⌚"), 1);
        assert_eq!(grapheme_width("字"), 2, "CJK is unaffected by `emoji`");

        // Ambiguous width means `ambiwidth=double` widens them again.
        set_ambiwidth_double(true);
        assert_eq!(grapheme_width("🚀"), 2);

        set_ambiwidth_double(false);
        set_emoji(true);
        assert!(!is_emoji_presentation('⚠'), "text pictograph, not emoji");
        assert!(is_emoji_presentation('🚀'));
        assert!(!is_emoji_presentation('a'));
    }

    /// vim `isprint`: characters outside the option render as `^C` / `~C` / `|c`,
    /// and `display=uhex` renders every unprintable one as `<xx>`. Until either is
    /// `:set`, nothing is substituted (zmax's own rendering).
    #[test]
    fn isprint_and_uhex_substitute_unprintable_characters() {
        let g = |c: char| Grapheme::new(c.to_string().into(), 0, 4);
        let other = |c: char| match g(c) {
            Grapheme::Other { g } => g.to_string(),
            grapheme => panic!("expected Other, got {grapheme:?}"),
        };

        // Untouched by default: no `isprint`, no `uhex`.
        set_isprint("");
        set_display_uhex(false);
        assert_eq!(other('\u{1}'), "\u{1}");
        assert_eq!(unprintable_repr('\u{1}'), None);

        // vim's own default value.
        set_isprint("@,161-255");
        assert_eq!(other('\u{1}'), "^A", "control chars show as ^X");
        assert_eq!(other('\u{7f}'), "^?");
        assert_eq!(other('\u{80}'), "~@");
        assert_eq!(other('\u{a0}'), "| ", "NBSP is outside 161-255");
        assert_eq!(other('é'), "é", "161-255 is printable");
        assert_eq!(other('a'), "a", "32..126 is always printable");
        assert_eq!(other('🚀'), "🚀", "U+0100 and above always print");
        assert_eq!(g('\u{1}').width(), 2, "`^A` takes two cells");

        // `^`-exclusions and explicit ranges.
        set_isprint("@,161-255,^é");
        assert_eq!(other('é'), "|i", "excluded => unprintable again");

        // `:set display=uhex` overrides the representation.
        set_isprint("@,161-255");
        set_display_uhex(true);
        assert_eq!(other('\u{1}'), "<01>");
        assert_eq!(other('\u{a0}'), "<a0>");
        assert_eq!(g('\u{1}').width(), 4);

        // uhex alone falls back on vim's default isprint table.
        set_isprint("");
        assert_eq!(other('\u{1}'), "<01>");
        assert_eq!(other('é'), "é");

        set_display_uhex(false);
        assert_eq!(other('\u{1}'), "\u{1}", "both unset => untouched again");
    }

    /// The `isprint` value language: characters, decimal codes, ranges, `@` for
    /// the alphabetic characters and `^` exclusions.
    #[test]
    fn parse_isprint_reads_vims_value_language() {
        let t = parse_isprint("@,161-255");
        assert!(t['a' as usize], "@ = alphabetic");
        assert!(t[200], "161-255");
        assert!(!t[160], "just outside the range");
        assert!(!t[1], "control chars are not printable");
        assert!(t[32] && t[126], "ASCII 32..126 always printable");

        let t = parse_isprint("1-8,@,^A");
        assert!(t[1] && t[8], "numeric range");
        assert!(!t[9], "outside the numeric range");
        assert!(
            t['A' as usize],
            "^A is stripped to a literal `A`, still printable via 32..126"
        );

        // A lone `-` is the literal hyphen, not a range.
        let t = parse_isprint("-");
        assert!(t['-' as usize]);
    }
}
