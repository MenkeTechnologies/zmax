use std::fmt::Display;

use crate::{
    graphemes::next_grapheme_boundary,
    match_brackets::{
        self, find_matching_bracket, find_matching_bracket_fuzzy, get_pair, is_close_bracket,
        is_open_bracket,
    },
    movement::Direction,
    Range, Selection, Syntax,
};
use ropey::RopeSlice;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    PairNotFound,
    CursorOverlap,
    RangeExceedsText,
    CursorOnAmbiguousPair,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match *self {
            Error::PairNotFound => "Surround pair not found around all cursors",
            Error::CursorOverlap => "Cursors overlap for a single surround pair range",
            Error::RangeExceedsText => "Cursor range exceeds text length",
            Error::CursorOnAmbiguousPair => "Cursor on ambiguous surround pair",
        })
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Finds the position of surround pairs of any [`crate::match_brackets::PAIRS`]
/// using tree-sitter when possible.
///
/// # Returns
///
/// Tuple `(anchor, head)`, meaning it is not always ordered.
pub fn find_nth_closest_pairs_pos(
    syntax: Option<&Syntax>,
    text: RopeSlice,
    range: Range,
    skip: usize,
) -> Result<(usize, usize)> {
    match syntax {
        Some(syntax) => find_nth_closest_pairs_ts(syntax, text, range, skip),
        None => find_nth_closest_pairs_plain(text, range, skip),
    }
}

fn find_nth_closest_pairs_ts(
    syntax: &Syntax,
    text: RopeSlice,
    range: Range,
    mut skip: usize,
) -> Result<(usize, usize)> {
    let mut opening = range.from();
    // We want to expand the selection if we are already on the found pair,
    // otherwise we would need to subtract "-1" from "range.to()".
    let mut closing = range.to();

    while skip > 0 {
        closing = find_matching_bracket_fuzzy(syntax, text, closing).ok_or(Error::PairNotFound)?;
        opening = find_matching_bracket(syntax, text, closing).ok_or(Error::PairNotFound)?;
        // If we're already on a closing bracket "find_matching_bracket_fuzzy" will return
        // the position of the opening bracket.
        if closing < opening {
            (opening, closing) = (closing, opening);
        }

        // In case found brackets are partially inside current selection.
        if range.from() < opening || closing < range.to() - 1 {
            closing = next_grapheme_boundary(text, closing);
        } else {
            skip -= 1;
            if skip != 0 {
                closing = next_grapheme_boundary(text, closing);
            }
        }
    }

    // Keep the original direction.
    if let Direction::Forward = range.direction() {
        Ok((opening, closing))
    } else {
        Ok((closing, opening))
    }
}

fn find_nth_closest_pairs_plain(
    text: RopeSlice,
    range: Range,
    mut skip: usize,
) -> Result<(usize, usize)> {
    let mut stack = Vec::with_capacity(2);
    let pos = range.from();
    let mut close_pos = pos.saturating_sub(1);

    for ch in text.chars_at(pos) {
        close_pos += 1;

        if is_open_bracket(ch) {
            // Track open pairs encountered so that we can step over
            // the corresponding close pairs that will come up further
            // down the loop. We want to find a lone close pair whose
            // open pair is before the cursor position.
            stack.push(ch);
            continue;
        }

        if !is_close_bracket(ch) {
            // We don't care if this character isn't a brace pair item,
            // so short circuit here.
            continue;
        }

        let (open, close) = get_pair(ch);

        if stack.last() == Some(&open) {
            // If we are encountering the closing pair for an opener
            // we just found while traversing, then its inside the
            // selection and should be skipped over.
            stack.pop();
            continue;
        }

        match find_nth_open_pair(text, open, close, close_pos, 1) {
            // Before we accept this pair, we want to ensure that the
            // pair encloses the range rather than just the cursor.
            Some(open_pos)
                if open_pos <= pos.saturating_add(1)
                    && close_pos >= range.to().saturating_sub(1) =>
            {
                // Since we have special conditions for when to
                // accept, we can't just pass the skip parameter on
                // through to the find_nth_*_pair methods, so we
                // track skips manually here.
                if skip > 1 {
                    skip -= 1;
                    continue;
                }

                return match range.direction() {
                    Direction::Forward => Ok((open_pos, close_pos)),
                    Direction::Backward => Ok((close_pos, open_pos)),
                };
            }
            _ => continue,
        }
    }

    Err(Error::PairNotFound)
}

/// Like [`crate::search::find_nth_char`] but confined to the cursor's own line:
/// the scan stops at a line boundary (`\n`/`\r`) instead of crossing into
/// adjacent lines, returning `None` if the nth match isn't reached first.
///
/// Used for quote text objects (`i"`, `a'`, …), which in Vim only match a pair
/// on the current line. The unbounded scan would otherwise let dot-repeating
/// `ci"` on a quote-less line reach back to a previous line's quotes. The
/// nth-match semantics are preserved, so nested quotes (`n = 2` selecting the
/// outer pair) still work — only the line boundary is added.
fn find_nth_char_on_line(
    mut n: usize,
    text: RopeSlice,
    ch: char,
    mut pos: usize,
    direction: Direction,
) -> Option<usize> {
    if n == 0 {
        return None;
    }

    let mut chars = text.get_chars_at(pos)?;

    match direction {
        Direction::Forward => loop {
            let c = chars.next()?;
            if c == '\n' || c == '\r' {
                return None;
            }
            if c == ch {
                n -= 1;
                if n == 0 {
                    return Some(pos);
                }
            }
            pos += 1;
        },
        Direction::Backward => loop {
            let c = chars.prev()?;
            if c == '\n' || c == '\r' {
                return None;
            }
            pos -= 1;
            if c == ch {
                n -= 1;
                if n == 0 {
                    return Some(pos);
                }
            }
        },
    }
}

thread_local! {
    /// vim `quoteescape` — characters that escape a quote inside a quoted string
    /// so it is not treated as a string boundary. `None` = never `:set` this
    /// session (use vim's default `\`); `Some(vec)` = explicitly set (an empty
    /// vec means no escaping, i.e. `:set quoteescape=`).
    static QUOTE_ESCAPE: std::cell::RefCell<Option<Vec<char>>> =
        const { std::cell::RefCell::new(None) };
}

/// Set the vim `quoteescape` characters (an empty vec disables escaping).
pub fn set_quote_escape_chars(chars: Vec<char>) {
    QUOTE_ESCAPE.with(|c| *c.borrow_mut() = Some(chars));
}

/// Whether the quote at char index `i` is escaped: preceded by an odd run of
/// `quoteescape` characters (so `\"` is escaped but `\\"` is not).
fn quote_is_escaped(text: RopeSlice, i: usize, line_start: usize) -> bool {
    QUOTE_ESCAPE.with(|esc| {
        let esc = esc.borrow();
        let is_esc = |c: char| match &*esc {
            None => c == '\\',         // never set -> vim default `\`
            Some(v) => v.contains(&c), // explicitly set (empty vec = no escaping)
        };
        let mut j = i;
        let mut count = 0;
        while j > line_start && is_esc(text.char(j - 1)) {
            count += 1;
            j -= 1;
        }
        count % 2 == 1
    })
}

/// Vim `i"`/`a'`/… pairing for the quote character `ch`, restricted to the
/// cursor's own line. Quotes on the line pair up left-to-right — `(q0,q1)`,
/// `(q2,q3)`, … — and this returns the `(open, close)` char positions of the
/// first pair whose closing quote is at or after `pos`, i.e. the pair the cursor
/// is inside, or the next quoted string to the right when the cursor is outside
/// one. A trailing unmatched quote is ignored. Returns `None` when the line has
/// no usable pair. The caller handles the "cursor directly on a quote" case
/// separately, so `pos` here is never on `ch`.
fn find_quote_pair_on_line(text: RopeSlice, ch: char, pos: usize) -> Option<(usize, usize)> {
    let len = text.len_chars();
    let line = text.char_to_line(pos.min(len.saturating_sub(1)));
    let line_start = text.line_to_char(line);

    // Positions of `ch` on this line, in order, stopping at the line boundary.
    // Skip `quoteescape`-escaped quotes so `di"` on `"a \"b\" c"` spans the whole
    // string rather than stopping at the first escaped quote.
    let mut quotes = Vec::new();
    let mut i = line_start;
    while i < len {
        let c = text.char(i);
        if c == '\n' || c == '\r' {
            break;
        }
        if c == ch && !quote_is_escaped(text, i, line_start) {
            quotes.push(i);
        }
        i += 1;
    }

    // Pair left-to-right; take the first pair not entirely before the cursor.
    let mut k = 0;
    while k + 1 < quotes.len() {
        let (open, close) = (quotes[k], quotes[k + 1]);
        if close >= pos {
            return Some((open, close));
        }
        k += 2;
    }
    None
}

/// Find the position of surround pairs of `ch` which can be either a closing
/// or opening pair. `n` will skip n - 1 pairs (eg. n=2 will discard (only)
/// the first pair found and keep looking)
pub fn find_nth_pairs_pos(
    syntax: Option<&Syntax>,
    text: RopeSlice,
    ch: char,
    range: Range,
    n: usize,
) -> Result<(usize, usize)> {
    if text.len_chars() < 2 {
        return Err(Error::PairNotFound);
    }
    if range.to() >= text.len_chars() {
        return Err(Error::RangeExceedsText);
    }

    let (open, close) = get_pair(ch);
    let pos = range.cursor(text);

    let (open, close) = if open == close {
        if Some(open) == text.get_char(pos) {
            // Cursor is directly on match character for which the opening and closing pairs are the same. For instance: ", ', `
            //
            // This is potentially ambiguous, because there's no way to know which side of the char we should be searching on.
            syntax
                .map_or_else(
                    || match_brackets::find_matching_bracket_plaintext(text.slice(..), pos),
                    |syntax| {
                        match_brackets::find_matching_bracket_fuzzy(syntax, text.slice(..), pos)
                    },
                )
                .map(|matching_pair_pos| {
                    if matching_pair_pos > pos {
                        (Some(pos), Some(matching_pair_pos))
                    } else {
                        (Some(matching_pair_pos), Some(pos))
                    }
                })
                .ok_or(Error::CursorOnAmbiguousPair)?
        } else if n == 1 {
            // Same open/close char (a quote: `"`, `'`, `` ` ``) and the cursor is
            // not sitting on one. Match Vim's `i"`/`a'`/…: quotes pair up
            // left-to-right *within the cursor's line*, and the target is the pair
            // the cursor is inside or — if it's outside any pair — the next quoted
            // string to the right on that line. This both stops the scan from
            // crossing newlines (dot-repeating `ci"` on a quote-less line used to
            // walk back to a previous line's quotes) and picks the forward pair
            // when the cursor sits before the quotes, as Vim does.
            match find_quote_pair_on_line(text, open, pos) {
                Some((o, c)) => (Some(o), Some(c)),
                None => (None, None),
            }
        } else {
            // `{count}i"` (a Helix extension: expand outward by `count` nesting
            // levels). Keep the nth-outward scan, but still bounded to the line so
            // it can't cross into another line's quotes.
            (
                find_nth_char_on_line(n, text, open, pos, Direction::Backward),
                find_nth_char_on_line(n, text, close, pos, Direction::Forward),
            )
        }
    } else {
        (
            find_nth_open_pair(text, open, close, pos, n),
            find_nth_close_pair(text, open, close, pos, n),
        )
    };

    // preserve original direction
    match range.direction() {
        Direction::Forward => Option::zip(open, close).ok_or(Error::PairNotFound),
        Direction::Backward => Option::zip(close, open).ok_or(Error::PairNotFound),
    }
}

fn find_nth_open_pair(
    text: RopeSlice,
    open: char,
    close: char,
    mut pos: usize,
    n: usize,
) -> Option<usize> {
    if pos >= text.len_chars() {
        return None;
    }

    let mut chars = text.chars_at(pos + 1);

    // Adjusts pos for the first iteration, and handles the case of the
    // cursor being *on* the close character which will get falsely stepped over
    // if not skipped here
    if chars.prev()? == open {
        return Some(pos);
    }

    for _ in 0..n {
        let mut step_over: usize = 0;

        loop {
            let c = chars.prev()?;
            pos = pos.saturating_sub(1);

            // ignore other surround pairs that are enclosed *within* our search scope
            if c == close {
                step_over += 1;
            } else if c == open {
                if step_over == 0 {
                    break;
                }

                step_over = step_over.saturating_sub(1);
            }
        }
    }

    Some(pos)
}

fn find_nth_close_pair(
    text: RopeSlice,
    open: char,
    close: char,
    mut pos: usize,
    n: usize,
) -> Option<usize> {
    if pos >= text.len_chars() {
        return None;
    }

    let mut chars = text.chars_at(pos);

    if chars.next()? == close {
        return Some(pos);
    }

    for _ in 0..n {
        let mut step_over: usize = 0;

        loop {
            let c = chars.next()?;
            pos += 1;

            if c == open {
                step_over += 1;
            } else if c == close {
                if step_over == 0 {
                    break;
                }

                step_over = step_over.saturating_sub(1);
            }
        }
    }

    Some(pos)
}

/// Find position of surround characters around every cursor. Returns None
/// if any positions overlap. Note that the positions are in a flat Vec.
/// Use get_surround_pos().chunks(2) to get matching pairs of surround positions.
/// `ch` can be either closing or opening pair. If `ch` is None, surround pairs
/// are automatically detected around each cursor (note that this may result
/// in them selecting different surround characters for each selection).
pub fn get_surround_pos(
    syntax: Option<&Syntax>,
    text: RopeSlice,
    selection: &Selection,
    ch: Option<char>,
    skip: usize,
) -> Result<Vec<usize>> {
    let mut change_pos = Vec::new();

    for &range in selection {
        let (open_pos, close_pos) = {
            let range_raw = match ch {
                Some(ch) => find_nth_pairs_pos(syntax, text, ch, range, skip)?,
                None => find_nth_closest_pairs_pos(syntax, text, range, skip)?,
            };
            let range = Range::new(range_raw.0, range_raw.1);
            (range.from(), range.to())
        };
        if change_pos.contains(&open_pos) || change_pos.contains(&close_pos) {
            return Err(Error::CursorOverlap);
        }
        // ensure the positions are always paired in the forward direction
        change_pos.extend_from_slice(&[open_pos.min(close_pos), close_pos.max(open_pos)]);
    }
    Ok(change_pos)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Range;

    use ropey::Rope;
    use smallvec::SmallVec;

    #[test]
    fn test_get_surround_pos() {
        #[rustfmt::skip]
        let (doc, selection, expectations) =
            rope_with_selections_and_expectations(
                "(some) (chars)\n(newline)",
                "_ ^  _ _ ^   _\n_    ^  _"
            );

        assert_eq!(
            get_surround_pos(None, doc.slice(..), &selection, Some('('), 1).unwrap(),
            expectations
        );
    }

    #[test]
    fn test_get_surround_pos_bail_different_surround_chars() {
        #[rustfmt::skip]
        let (doc, selection, _) =
            rope_with_selections_and_expectations(
                "[some]\n(chars)xx\n(newline)",
                "  ^   \n  ^      \n         "
            );

        assert_eq!(
            get_surround_pos(None, doc.slice(..), &selection, Some('('), 1),
            Err(Error::PairNotFound)
        );
    }

    #[test]
    fn test_get_surround_pos_bail_overlapping_surround_chars() {
        #[rustfmt::skip]
        let (doc, selection, _) =
            rope_with_selections_and_expectations(
                "[some]\n(chars)xx\n(newline)",
                "      \n       ^ \n      ^  "
            );

        assert_eq!(
            get_surround_pos(None, doc.slice(..), &selection, Some('('), 1),
            Err(Error::PairNotFound) // overlapping surround chars
        );
    }

    #[test]
    fn test_get_surround_pos_bail_cursor_overlap() {
        #[rustfmt::skip]
        let (doc, selection, _) =
            rope_with_selections_and_expectations(
                "[some]\n(chars)xx\n(newline)",
                "  ^^  \n         \n         "
            );

        assert_eq!(
            get_surround_pos(None, doc.slice(..), &selection, Some('['), 1),
            Err(Error::CursorOverlap)
        );
    }

    #[test]
    fn quoteescape_skips_backslash_escaped_quotes() {
        // default `quoteescape` (`\`) skips the inner `\"` so the pair spans the
        // whole string: `"a \"b\" c"` (indices 0..=10) pairs the outer quotes.
        let doc = ropey::Rope::from(r#""a \"b\" c""#);
        let slice = doc.slice(..);
        assert_eq!(
            find_nth_pairs_pos(None, slice, '"', crate::Selection::point(5).primary(), 1)
                .expect("pair found"),
            (0, 10)
        );
    }

    #[test]
    fn test_find_nth_pairs_pos_quote_success() {
        #[rustfmt::skip]
        let (doc, selection, expectations) =
            rope_with_selections_and_expectations(
                "some 'quoted text' on this 'line'\n'and this one'",
                "     _        ^  _               \n              "
            );

        assert_eq!(2, expectations.len());
        assert_eq!(
            find_nth_pairs_pos(None, doc.slice(..), '\'', selection.primary(), 1)
                .expect("find should succeed"),
            (expectations[0], expectations[1])
        )
    }

    #[test]
    fn test_find_nth_pairs_pos_nested_quote_success() {
        #[rustfmt::skip]
        let (doc, selection, expectations) =
            rope_with_selections_and_expectations(
                "some 'nested 'quoted' text' on this 'line'\n'and this one'",
                "     _           ^        _               \n              "
            );

        assert_eq!(2, expectations.len());
        assert_eq!(
            find_nth_pairs_pos(None, doc.slice(..), '\'', selection.primary(), 2)
                .expect("find should succeed"),
            (expectations[0], expectations[1])
        )
    }

    #[test]
    fn test_find_nth_pairs_pos_inside_quote_ambiguous() {
        #[rustfmt::skip]
        let (doc, selection, _) =
            rope_with_selections_and_expectations(
                "some 'nested 'quoted' text' on this 'line'\n'and this one'",
                "                    ^                     \n              "
            );

        assert_eq!(
            find_nth_pairs_pos(None, doc.slice(..), '\'', selection.primary(), 1),
            Err(Error::CursorOnAmbiguousPair)
        )
    }

    #[test]
    fn test_find_nth_pairs_pos_quote_does_not_cross_line() {
        // A previous line has a quote pair; the cursor sits on a later,
        // quote-less line. Vim's `i"` only matches on the cursor's own line, so
        // this must not reach back to the earlier line (the dot-repeat `ci"`
        // "jumps to the original line" bug).
        #[rustfmt::skip]
        let (doc, selection, _) =
            rope_with_selections_and_expectations(
                "say 'hi' here\nplain line no quotes",
                "             \n      ^             "
            );

        assert_eq!(
            find_nth_pairs_pos(None, doc.slice(..), '\'', selection.primary(), 1),
            Err(Error::PairNotFound)
        )
    }

    #[test]
    fn test_find_nth_pairs_pos_quote_same_line_still_found() {
        // The line-restriction must not break the normal case: a quote pair on
        // the cursor's own line is still found, even when an earlier line also
        // has quotes.
        #[rustfmt::skip]
        let (doc, selection, expectations) =
            rope_with_selections_and_expectations(
                "first 'line'\nsecond 'here' ok",
                "            \n       _^   _   "
            );

        assert_eq!(2, expectations.len());
        assert_eq!(
            find_nth_pairs_pos(None, doc.slice(..), '\'', selection.primary(), 1)
                .expect("find should succeed"),
            (expectations[0], expectations[1])
        )
    }

    #[test]
    fn test_find_nth_pairs_pos_quote_forward_when_cursor_before() {
        // Cursor is before the quotes on its line (not inside any pair). Vim's
        // `i"` grabs the next quoted string to the right on the line, so this
        // must find the forward pair rather than failing.
        #[rustfmt::skip]
        let (doc, selection, expectations) =
            rope_with_selections_and_expectations(
                "foo 'bar' baz",
                "^   _   _    "
            );

        assert_eq!(2, expectations.len());
        assert_eq!(
            find_nth_pairs_pos(None, doc.slice(..), '\'', selection.primary(), 1)
                .expect("find should succeed"),
            (expectations[0], expectations[1])
        )
    }

    #[test]
    fn test_find_nth_pairs_pos_quote_second_string_on_line() {
        // Cursor sits in the gap between two quoted strings; Vim selects the
        // second (forward) string, not the first.
        #[rustfmt::skip]
        let (doc, selection, expectations) =
            rope_with_selections_and_expectations(
                "'a' X 'bb' Y",
                "    ^ _  _  "
            );

        assert_eq!(2, expectations.len());
        assert_eq!(
            find_nth_pairs_pos(None, doc.slice(..), '\'', selection.primary(), 1)
                .expect("find should succeed"),
            (expectations[0], expectations[1])
        )
    }

    #[test]
    fn test_find_nth_closest_pairs_pos_index_range_panic() {
        #[rustfmt::skip]
        let (doc, selection, _) =
            rope_with_selections_and_expectations(
                "(a)c)",
                "^^^^^"
            );

        assert_eq!(
            find_nth_closest_pairs_pos(None, doc.slice(..), selection.primary(), 1),
            Err(Error::PairNotFound)
        )
    }

    // Create a Rope and a matching Selection using a specification language.
    // ^ is a single-point selection.
    // _ is an expected index. These are returned as a Vec<usize> for use in assertions.
    fn rope_with_selections_and_expectations(
        text: &str,
        spec: &str,
    ) -> (Rope, Selection, Vec<usize>) {
        if text.len() != spec.len() {
            panic!("specification must match text length -- are newlines aligned?");
        }

        let rope = Rope::from(text);

        let selections: SmallVec<[Range; 1]> = spec
            .match_indices('^')
            .map(|(i, _)| Range::point(i))
            .collect();

        let expectations: Vec<usize> = spec.match_indices('_').map(|(i, _)| i).collect();

        (rope, Selection::new(selections, 0), expectations)
    }
}
