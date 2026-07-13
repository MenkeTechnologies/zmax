use std::{borrow::Cow, cmp::Reverse, iter};

use ropey::iter::Chars;

use crate::{
    char_idx_at_visual_offset,
    chars::{categorize_char, char_is_line_ending, CharCategory},
    doc_formatter::TextFormat,
    graphemes::{
        next_grapheme_boundary, nth_next_grapheme_boundary, nth_prev_grapheme_boundary,
        prev_grapheme_boundary,
    },
    line_ending::rope_is_line_ending,
    position::char_idx_at_visual_block_offset,
    syntax,
    text_annotations::TextAnnotations,
    textobject::TextObject,
    tree_sitter::Node,
    visual_offset_from_block, Range, RopeSlice, Selection, Syntax,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Movement {
    Extend,
    Move,
}

pub fn move_horizontally(
    slice: RopeSlice,
    range: Range,
    dir: Direction,
    count: usize,
    behaviour: Movement,
    _: &TextFormat,
    _: &mut TextAnnotations,
) -> Range {
    let pos = range.cursor(slice);

    // Compute the new position.
    let new_pos = match dir {
        Direction::Forward => nth_next_grapheme_boundary(slice, pos, count),
        Direction::Backward => nth_prev_grapheme_boundary(slice, pos, count),
    };

    // Compute the final new range.
    range.put_cursor(slice, new_pos, behaviour == Movement::Extend)
}

pub fn move_vertically_visual(
    slice: RopeSlice,
    range: Range,
    dir: Direction,
    count: usize,
    behaviour: Movement,
    text_fmt: &TextFormat,
    annotations: &mut TextAnnotations,
) -> Range {
    if !text_fmt.soft_wrap {
        return move_vertically(slice, range, dir, count, behaviour, text_fmt, annotations);
    }
    annotations.clear_line_annotations();
    let pos = range.cursor(slice);

    // Compute the current position's 2d coordinates.
    let (visual_pos, block_off) = visual_offset_from_block(slice, pos, pos, text_fmt, annotations);
    let new_col = range
        .old_visual_position
        .map_or(visual_pos.col as u32, |(_, col)| col);

    // Compute the new position.
    let mut row_off = match dir {
        Direction::Forward => count as isize,
        Direction::Backward => -(count as isize),
    };

    // Compute visual offset relative to block start to avoid trasversing the block twice
    row_off += visual_pos.row as isize;
    let (mut new_pos, virtual_rows) = char_idx_at_visual_offset(
        slice,
        block_off,
        row_off,
        new_col as usize,
        text_fmt,
        annotations,
    );
    if dir == Direction::Forward {
        new_pos += (virtual_rows != 0) as usize;
    }

    // Special-case to avoid moving to the end of the last non-empty line.
    if behaviour == Movement::Extend && slice.line(slice.char_to_line(new_pos)).len_chars() == 0 {
        return range;
    }

    let mut new_range = range.put_cursor(slice, new_pos, behaviour == Movement::Extend);
    new_range.old_visual_position = Some((0, new_col));
    new_range
}

pub fn move_vertically(
    slice: RopeSlice,
    range: Range,
    dir: Direction,
    count: usize,
    behaviour: Movement,
    text_fmt: &TextFormat,
    annotations: &mut TextAnnotations,
) -> Range {
    annotations.clear_line_annotations();
    let pos = range.cursor(slice);
    let line_idx = slice.char_to_line(pos);
    let line_start = slice.line_to_char(line_idx);

    // Compute the current position's 2d coordinates.
    let visual_pos = visual_offset_from_block(slice, line_start, pos, text_fmt, annotations).0;
    let (mut new_row, new_col) = range
        .old_visual_position
        .map_or((visual_pos.row as u32, visual_pos.col as u32), |pos| pos);
    new_row = new_row.max(visual_pos.row as u32);
    let line_idx = slice.char_to_line(pos);

    // Compute the new position.
    let mut new_line_idx = match dir {
        Direction::Forward => line_idx.saturating_add(count),
        Direction::Backward => line_idx.saturating_sub(count),
    };

    let line = if new_line_idx >= slice.len_lines() - 1 {
        // there is no line terminator for the last line
        // so the logic below is not necessary here
        new_line_idx = slice.len_lines() - 1;
        slice
    } else {
        // char_idx_at_visual_block_offset returns a one-past-the-end index
        // in case it reaches the end of the slice
        // to avoid moving to the nextline in that case the line terminator is removed from the line
        let new_line_end = prev_grapheme_boundary(slice, slice.line_to_char(new_line_idx + 1));
        slice.slice(..new_line_end)
    };

    let new_line_start = line.line_to_char(new_line_idx);

    let (new_pos, _) = char_idx_at_visual_block_offset(
        line,
        new_line_start,
        new_row as usize,
        new_col as usize,
        text_fmt,
        annotations,
    );

    // Special-case to avoid moving to the end of the last non-empty line.
    if behaviour == Movement::Extend && slice.line(new_line_idx).len_chars() == 0 {
        return range;
    }

    let mut new_range = range.put_cursor(slice, new_pos, behaviour == Movement::Extend);
    new_range.old_visual_position = Some((new_row, new_col));
    new_range
}

/// Move the cursor to the start (`to_end` false) or end (`to_end` true) of the
/// *visual* (on-screen) line containing it, honoring soft-wrap: under wrapping a
/// single logical line spans several visual rows and this stops at the wrap
/// boundary, not the logical line boundary. With soft-wrap off a visual line is
/// a logical line, so it degenerates to logical line start/end. Implements Emacs
/// `beginning-of-visual-line` / `end-of-visual-line`.
///
/// Column `0` selects the first grapheme of the current visual row; a saturated
/// column clamps to the last grapheme of that row. Reuses the same
/// `char_idx_at_visual_offset` machinery as `move_vertically_visual`.
pub fn goto_visual_line(
    slice: RopeSlice,
    range: Range,
    to_end: bool,
    behaviour: Movement,
    text_fmt: &TextFormat,
    annotations: &mut TextAnnotations,
) -> Range {
    annotations.clear_line_annotations();
    let pos = range.cursor(slice);
    let column = if to_end { usize::MAX } else { 0 };
    let (new_pos, _) = char_idx_at_visual_offset(slice, pos, 0, column, text_fmt, annotations);
    range.put_cursor(slice, new_pos, behaviour == Movement::Extend)
}

pub fn move_next_word_start(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::NextWordStart)
}

/// vim `w`/`W`: move to the start of the next word as a point (vim's caret is a
/// point, not a Helix selection).
///
/// [`move_next_word_start`] first extends the range to cover the grapheme under
/// the cursor (Helix block-cursor semantics) before searching. When that
/// grapheme is the last whitespace immediately before a token, the extended
/// start lands exactly on the token's first char, so Helix treats that boundary
/// as already-consumed and advances an *extra* token — landing on `=` instead of
/// `fetch` for `\tfetch = …`. vim lands on the first token. Helix signals this
/// case by moving the range anchor forward past the origin to the true next word
/// start; take that anchor instead of the overshot head. Done per single step so
/// `count` composes correctly (passing `count` into Helix's internal loop chains
/// anchors and reintroduces the overshoot).
fn next_word_start_vim(slice: RopeSlice, mut pos: usize, count: usize, long: bool) -> usize {
    let step = if long {
        move_next_long_word_start
    } else {
        move_next_word_start
    };
    for _ in 0..count {
        let moved = step(slice, Range::point(pos), 1);
        let next = if moved.anchor > pos {
            moved.anchor
        } else {
            moved.head
        };
        if next == pos {
            break;
        }
        pos = next;
    }
    pos
}

/// vim `w` caret. See [`next_word_start_vim`].
///
/// Start from the visual caret ([`Range::cursor`]), not `range.head`: in normal
/// mode the cursor is a 1-wide block whose head sits one grapheme past the caret,
/// so on the whitespace before a token `range.head` already points *at* the
/// token's first char and the search would skip it. `cursor()` collapses back to
/// the caret (and equals `head` for a point range, so unit tests are unaffected).
pub fn move_next_word_start_vim(slice: RopeSlice, range: Range, count: usize) -> Range {
    Range::point(next_word_start_vim(
        slice,
        range.cursor(slice),
        count,
        false,
    ))
}

/// vim `W` caret. See [`next_word_start_vim`].
pub fn move_next_long_word_start_vim(slice: RopeSlice, range: Range, count: usize) -> Range {
    Range::point(next_word_start_vim(slice, range.cursor(slice), count, true))
}

pub fn move_next_word_end(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::NextWordEnd)
}

pub fn move_prev_word_start(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::PrevWordStart)
}

pub fn move_prev_word_end(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::PrevWordEnd)
}

pub fn move_next_long_word_start(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::NextLongWordStart)
}

pub fn move_next_long_word_end(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::NextLongWordEnd)
}

pub fn move_prev_long_word_start(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::PrevLongWordStart)
}

pub fn move_prev_long_word_end(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::PrevLongWordEnd)
}

pub fn move_next_sub_word_start(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::NextSubWordStart)
}

pub fn move_next_sub_word_end(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::NextSubWordEnd)
}

pub fn move_prev_sub_word_start(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::PrevSubWordStart)
}

pub fn move_prev_sub_word_end(slice: RopeSlice, range: Range, count: usize) -> Range {
    word_move(slice, range, count, WordMotionTarget::PrevSubWordEnd)
}

fn word_move(slice: RopeSlice, range: Range, count: usize, target: WordMotionTarget) -> Range {
    let is_prev = matches!(
        target,
        WordMotionTarget::PrevWordStart
            | WordMotionTarget::PrevLongWordStart
            | WordMotionTarget::PrevSubWordStart
            | WordMotionTarget::PrevWordEnd
            | WordMotionTarget::PrevLongWordEnd
            | WordMotionTarget::PrevSubWordEnd
    );

    // Special-case early-out.
    if (is_prev && range.head == 0) || (!is_prev && range.head == slice.len_chars()) {
        return range;
    }

    // Prepare the range appropriately based on the target movement
    // direction.  This is addressing two things at once:
    //
    //   1. Block-cursor semantics.
    //   2. The anchor position being irrelevant to the output result.
    #[allow(clippy::collapsible_else_if)] // Makes the structure clearer in this case.
    let start_range = if is_prev {
        if range.anchor < range.head {
            Range::new(range.head, prev_grapheme_boundary(slice, range.head))
        } else {
            Range::new(next_grapheme_boundary(slice, range.head), range.head)
        }
    } else {
        if range.anchor < range.head {
            Range::new(prev_grapheme_boundary(slice, range.head), range.head)
        } else {
            Range::new(range.head, next_grapheme_boundary(slice, range.head))
        }
    };

    // Do the main work.
    let mut range = start_range;
    for _ in 0..count {
        let next_range = slice.chars_at(range.head).range_to_target(target, range);
        if range == next_range {
            break;
        }
        range = next_range;
    }
    range
}

// ---- vim `paragraphs` (nroff macros that start a paragraph) ---------------
//
// A paragraph always starts after a blank line. vim's `paragraphs` option adds
// nroff macros: the value is a run of two-character macro names (the default
// `IPLPPPQPP TPHPLIPpLpItpplpipbp`), and a line of the form `.XY` whose `XY` is
// one of them also starts a paragraph — so `{`/`}` stop on `.PP`/`.IP`/… in a
// man page or roff source. Empty (the default here) = blank lines only.

thread_local! {
    static PARAGRAPH_MACROS: std::cell::RefCell<String> =
        const { std::cell::RefCell::new(String::new()) };
}

/// vim `paragraphs`: set the nroff macro names that start a paragraph.
pub fn set_paragraph_macros(spec: &str) {
    PARAGRAPH_MACROS.with(|m| *m.borrow_mut() = spec.to_string());
}

/// Whether `line` is an nroff macro line naming one of the `spec` macros — a
/// leading `.` followed by a macro name, where a one-letter name is padded with
/// a space (vim pairs the option's characters two at a time). Pure — unit tested.
pub fn is_nroff_macro_line(line: &str, spec: &str) -> bool {
    if spec.is_empty() {
        return false;
    }
    let Some(rest) = line.strip_prefix('.') else {
        return false;
    };
    let mut chars = rest.chars();
    let Some(c1) = chars.next() else {
        return false;
    };
    let c2 = match chars.next() {
        Some(c) if !c.is_whitespace() => c,
        _ => ' ',
    };
    spec.chars()
        .collect::<Vec<_>>()
        .chunks(2)
        .any(|pair| pair.len() == 2 && pair[0] == c1 && pair[1] == c2)
}

/// How a line participates in paragraph motion: blank lines *separate*
/// paragraphs, a `paragraphs` macro line *starts* one (the motion lands on it,
/// it is not skipped like a blank), everything else is body text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParaLine {
    Text,
    Blank,
    Macro,
}

fn para_line(line: RopeSlice) -> ParaLine {
    if rope_is_line_ending(line) {
        return ParaLine::Blank;
    }
    let is_macro = PARAGRAPH_MACROS.with(|m| {
        let spec = m.borrow();
        !spec.is_empty() && is_nroff_macro_line(Cow::from(line).trim_end(), &spec)
    });
    if is_macro {
        ParaLine::Macro
    } else {
        ParaLine::Text
    }
}

/// A paragraph boundary: a blank line, or an nroff macro line named by the vim
/// `paragraphs` option.
fn is_paragraph_boundary(line: RopeSlice) -> bool {
    para_line(line) != ParaLine::Text
}

pub fn move_prev_paragraph(
    slice: RopeSlice,
    range: Range,
    count: usize,
    behavior: Movement,
) -> Range {
    let mut line = range.cursor_line(slice);
    let first_char = slice.line_to_char(line) == range.cursor(slice);
    let prev_line_empty = is_paragraph_boundary(slice.line(line.saturating_sub(1)));
    let curr_line_empty = is_paragraph_boundary(slice.line(line));
    let prev_empty_to_line = prev_line_empty && !curr_line_empty;

    // skip character before paragraph boundary
    if prev_empty_to_line && !first_char {
        line += 1;
    }
    let mut lines = slice.lines_at(line);
    lines.reverse();
    let mut lines = lines.map(para_line).peekable();
    let mut last_line = line;
    for _ in 0..count {
        while lines.next_if(|&k| k == ParaLine::Blank).is_some() {
            line -= 1;
        }
        while lines.next_if(|&k| k == ParaLine::Text).is_some() {
            line -= 1;
        }
        // vim `paragraphs`: the nroff macro line above the text block is that
        // paragraph's start, so `{` lands on it rather than below it.
        if lines.next_if(|&k| k == ParaLine::Macro).is_some() {
            line -= 1;
        }
        if line == last_line {
            break;
        }
        last_line = line;
    }

    let head = slice.line_to_char(line);
    let anchor = if behavior == Movement::Move {
        // exclude first character after paragraph boundary
        if prev_empty_to_line && first_char {
            range.cursor(slice)
        } else {
            range.head
        }
    } else {
        range.put_cursor(slice, head, true).anchor
    };
    Range::new(anchor, head)
}

pub fn move_next_paragraph(
    slice: RopeSlice,
    range: Range,
    count: usize,
    behavior: Movement,
) -> Range {
    let mut line = range.cursor_line(slice);
    let last_char =
        prev_grapheme_boundary(slice, slice.line_to_char(line + 1)) == range.cursor(slice);
    let curr_line_empty = is_paragraph_boundary(slice.line(line));
    let next_line_empty =
        is_paragraph_boundary(slice.line(slice.len_lines().saturating_sub(1).min(line + 1)));
    let curr_empty_to_line = curr_line_empty && !next_line_empty;

    // skip character after paragraph boundary
    if curr_empty_to_line && last_char {
        line += 1;
    }
    let mut lines = slice.lines_at(line).map(para_line).peekable();
    let mut last_line = line;
    for _ in 0..count {
        // vim `paragraphs`: a macro line the cursor already sits on starts *this*
        // paragraph — step off it before looking for the next one.
        if lines.next_if(|&k| k == ParaLine::Macro).is_some() {
            line += 1;
        }
        while lines.next_if(|&k| k == ParaLine::Text).is_some() {
            line += 1;
        }
        while lines.next_if(|&k| k == ParaLine::Blank).is_some() {
            line += 1;
        }
        if line == last_line {
            break;
        }
        last_line = line;
    }
    let head = slice.line_to_char(line);
    let anchor = if behavior == Movement::Move {
        if curr_empty_to_line && last_char {
            range.head
        } else {
            range.cursor(slice)
        }
    } else {
        range.put_cursor(slice, head, true).anchor
    };
    Range::new(anchor, head)
}

// ---- sentence motions (vim `(` / `)`) ------------
//
// A sentence is ended by `.`, `!` or `?` followed by the end of a line, or by a
// space or tab. Any number of closing `)`, `]`, `"` and `'` characters may
// appear after the `.`, `!` or `?` before the spaces. A blank line (paragraph
// boundary) is also a sentence boundary. (vim `:help sentence`.)

#[inline]
fn is_sentence_end(c: char) -> bool {
    matches!(c, '.' | '!' | '?')
}

#[inline]
fn is_sentence_closer(c: char) -> bool {
    matches!(c, ')' | ']' | '"' | '\'')
}

#[inline]
fn is_sentence_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// Char index of the start of the next sentence at or after `pos`.
/// Returns `slice.len_chars()` if no further sentence start exists.
pub fn next_sentence_boundary(slice: RopeSlice, pos: usize) -> usize {
    let len = slice.len_chars();
    let mut i = pos;
    while i < len {
        let c = slice.char(i);
        if is_sentence_end(c) {
            let mut j = i + 1;
            while j < len && is_sentence_closer(slice.char(j)) {
                j += 1;
            }
            // The ender must be followed by whitespace or the end of the buffer.
            if j >= len {
                return len;
            }
            if is_sentence_space(slice.char(j)) {
                let mut k = j;
                while k < len && is_sentence_space(slice.char(k)) {
                    k += 1;
                }
                return k;
            }
            i = j;
        } else {
            // A blank line is its own boundary: the start of the line after it.
            if c == '\n' && i + 1 < len && slice.char(i + 1) == '\n' {
                return i + 1;
            }
            i += 1;
        }
    }
    len
}

/// Char index of the first character of the paragraph containing `pos`,
/// used to bound the backward sentence scan to a single paragraph.
pub(crate) fn current_paragraph_start(slice: RopeSlice, pos: usize) -> usize {
    let mut line = slice.char_to_line(pos);
    while line > 0 && !rope_is_line_ending(slice.line(line - 1)) {
        line -= 1;
    }
    slice.line_to_char(line)
}

/// Char index of the start of the sentence before `pos` (or the start of the
/// current sentence when `pos` is mid-sentence), bounded to this paragraph.
pub fn prev_sentence_boundary(slice: RopeSlice, pos: usize) -> usize {
    let para = current_paragraph_start(slice, pos);
    let mut start = para;
    let mut i = para;
    while i < pos {
        let nb = next_sentence_boundary(slice, i);
        if nb <= i || nb >= pos {
            break;
        }
        start = nb;
        i = nb;
    }
    start
}

pub fn move_next_sentence(
    slice: RopeSlice,
    range: Range,
    count: usize,
    behavior: Movement,
) -> Range {
    let len = slice.len_chars();
    let mut pos = range.cursor(slice);
    for _ in 0..count {
        let nb = next_sentence_boundary(slice, pos);
        if nb <= pos {
            break;
        }
        pos = nb;
    }
    let head = pos.min(len.saturating_sub(1));
    let anchor = if behavior == Movement::Move {
        head
    } else {
        range.anchor
    };
    Range::new(anchor, head)
}

pub fn move_prev_sentence(
    slice: RopeSlice,
    range: Range,
    count: usize,
    behavior: Movement,
) -> Range {
    let mut pos = range.cursor(slice);
    for _ in 0..count {
        let pb = prev_sentence_boundary(slice, pos);
        if pb >= pos {
            break;
        }
        pos = pb;
    }
    let anchor = if behavior == Movement::Move {
        pos
    } else {
        range.anchor
    };
    Range::new(anchor, pos)
}

// ---- util ------------

#[inline]
/// Returns first index that doesn't satisfy a given predicate when
/// advancing the character index.
///
/// Returns none if all characters satisfy the predicate.
pub fn skip_while<F>(slice: RopeSlice, pos: usize, fun: F) -> Option<usize>
where
    F: Fn(char) -> bool,
{
    let mut chars = slice.chars_at(pos).enumerate();
    chars.find_map(|(i, c)| if !fun(c) { Some(pos + i) } else { None })
}

#[inline]
/// Returns first index that doesn't satisfy a given predicate when
/// retreating the character index, saturating if all elements satisfy
/// the condition.
pub fn backwards_skip_while<F>(slice: RopeSlice, pos: usize, fun: F) -> Option<usize>
where
    F: Fn(char) -> bool,
{
    let mut chars_starting_from_next = slice.chars_at(pos);
    let mut backwards = iter::from_fn(|| chars_starting_from_next.prev()).enumerate();
    backwards.find_map(|(i, c)| {
        if !fun(c) {
            Some(pos.saturating_sub(i))
        } else {
            None
        }
    })
}

/// Possible targets of a word motion
#[derive(Copy, Clone, Debug)]
pub enum WordMotionTarget {
    NextWordStart,
    NextWordEnd,
    PrevWordStart,
    PrevWordEnd,
    // A "Long word" (also known as a WORD in Vim/Kakoune) is strictly
    // delimited by whitespace, and can consist of punctuation as well
    // as alphanumerics.
    NextLongWordStart,
    NextLongWordEnd,
    PrevLongWordStart,
    PrevLongWordEnd,
    // A sub word is similar to a regular word, except it is also delimited by
    // underscores and transitions from lowercase to uppercase.
    NextSubWordStart,
    NextSubWordEnd,
    PrevSubWordStart,
    PrevSubWordEnd,
}

pub trait CharHelpers {
    fn range_to_target(&mut self, target: WordMotionTarget, origin: Range) -> Range;
}

impl CharHelpers for Chars<'_> {
    /// Note: this only changes the anchor of the range if the head is effectively
    /// starting on a boundary (either directly or after skipping newline characters).
    /// Any other changes to the anchor should be handled by the calling code.
    fn range_to_target(&mut self, target: WordMotionTarget, origin: Range) -> Range {
        let is_prev = matches!(
            target,
            WordMotionTarget::PrevWordStart
                | WordMotionTarget::PrevLongWordStart
                | WordMotionTarget::PrevSubWordStart
                | WordMotionTarget::PrevWordEnd
                | WordMotionTarget::PrevLongWordEnd
                | WordMotionTarget::PrevSubWordEnd
        );

        // Reverse the iterator if needed for the motion direction.
        if is_prev {
            self.reverse();
        }

        // Function to advance index in the appropriate motion direction.
        let advance: &dyn Fn(&mut usize) = if is_prev {
            &|idx| *idx = idx.saturating_sub(1)
        } else {
            &|idx| *idx += 1
        };

        // Initialize state variables.
        let mut anchor = origin.anchor;
        let mut head = origin.head;
        let mut prev_ch = {
            let ch = self.prev();
            if ch.is_some() {
                self.next();
            }
            ch
        };

        // Skip any initial newline characters.
        while let Some(ch) = self.next() {
            if char_is_line_ending(ch) {
                prev_ch = Some(ch);
                advance(&mut head);
            } else {
                self.prev();
                break;
            }
        }
        if prev_ch.map(char_is_line_ending).unwrap_or(false) {
            anchor = head;
        }

        // Find our target position(s).
        let head_start = head;
        #[allow(clippy::while_let_on_iterator)] // Clippy's suggestion to fix doesn't work here.
        while let Some(next_ch) = self.next() {
            if prev_ch.is_none() || reached_target(target, prev_ch.unwrap(), next_ch) {
                if head == head_start {
                    anchor = head;
                } else {
                    break;
                }
            }
            prev_ch = Some(next_ch);
            advance(&mut head);
        }

        // Un-reverse the iterator if needed.
        if is_prev {
            self.reverse();
        }

        Range::new(anchor, head)
    }
}

fn is_word_boundary(a: char, b: char) -> bool {
    categorize_char(a) != categorize_char(b)
}

fn is_long_word_boundary(a: char, b: char) -> bool {
    match (categorize_char(a), categorize_char(b)) {
        (CharCategory::Word, CharCategory::Punctuation)
        | (CharCategory::Punctuation, CharCategory::Word) => false,
        (a, b) if a != b => true,
        _ => false,
    }
}

fn is_sub_word_boundary(a: char, b: char, dir: Direction) -> bool {
    match (categorize_char(a), categorize_char(b)) {
        (CharCategory::Word, CharCategory::Word) => {
            if (a == '_') != (b == '_') {
                return true;
            }

            // Subword boundaries are directional: in 'fooBar', there is a
            // boundary between 'o' and 'B', but not between 'B' and 'a'.
            match dir {
                Direction::Forward => a.is_lowercase() && b.is_uppercase(),
                Direction::Backward => a.is_uppercase() && b.is_lowercase(),
            }
        }
        (a, b) if a != b => true,
        _ => false,
    }
}

fn reached_target(target: WordMotionTarget, prev_ch: char, next_ch: char) -> bool {
    match target {
        WordMotionTarget::NextWordStart | WordMotionTarget::PrevWordEnd => {
            is_word_boundary(prev_ch, next_ch)
                && (char_is_line_ending(next_ch) || !next_ch.is_whitespace())
        }
        WordMotionTarget::NextWordEnd | WordMotionTarget::PrevWordStart => {
            is_word_boundary(prev_ch, next_ch)
                && (!prev_ch.is_whitespace() || char_is_line_ending(next_ch))
        }
        WordMotionTarget::NextLongWordStart | WordMotionTarget::PrevLongWordEnd => {
            is_long_word_boundary(prev_ch, next_ch)
                && (char_is_line_ending(next_ch) || !next_ch.is_whitespace())
        }
        WordMotionTarget::NextLongWordEnd | WordMotionTarget::PrevLongWordStart => {
            is_long_word_boundary(prev_ch, next_ch)
                && (!prev_ch.is_whitespace() || char_is_line_ending(next_ch))
        }
        WordMotionTarget::NextSubWordStart => {
            is_sub_word_boundary(prev_ch, next_ch, Direction::Forward)
                && (char_is_line_ending(next_ch) || !(next_ch.is_whitespace() || next_ch == '_'))
        }
        WordMotionTarget::PrevSubWordEnd => {
            is_sub_word_boundary(prev_ch, next_ch, Direction::Backward)
                && (char_is_line_ending(next_ch) || !(next_ch.is_whitespace() || next_ch == '_'))
        }
        WordMotionTarget::NextSubWordEnd => {
            is_sub_word_boundary(prev_ch, next_ch, Direction::Forward)
                && (!(prev_ch.is_whitespace() || prev_ch == '_') || char_is_line_ending(next_ch))
        }
        WordMotionTarget::PrevSubWordStart => {
            is_sub_word_boundary(prev_ch, next_ch, Direction::Backward)
                && (!(prev_ch.is_whitespace() || prev_ch == '_') || char_is_line_ending(next_ch))
        }
    }
}

/// Finds the range of the next or previous textobject in the syntax tree.
/// Returns the range in the forwards direction.
pub fn goto_treesitter_object(
    slice: RopeSlice,
    range: Range,
    object_name: &str,
    dir: Direction,
    syntax: &Syntax,
    loader: &syntax::Loader,
    count: usize,
) -> Range {
    let get_range = move |range: Range| -> Option<Range> {
        let byte_pos = slice.char_to_byte(range.cursor(slice));

        // Walk the layer at the cursor with that language's own tree and textobject query.
        // Resolved per step so the motion can cross into and out of injected regions.
        let layer = syntax.layer_for_byte_range(byte_pos as u32, byte_pos as u32);
        let slice_tree = syntax
            .tree_for_byte_range(byte_pos as u32, byte_pos as u32)
            .root_node();
        let textobject_query = loader.textobject_query(syntax.layer(layer).language);

        let cap_name = |t: TextObject| format!("{}.{}", object_name, t);
        let nodes = textobject_query?.capture_nodes_any(
            &[
                &cap_name(TextObject::Movement),
                &cap_name(TextObject::Around),
                &cap_name(TextObject::Inside),
            ],
            &slice_tree,
            slice,
        )?;

        let node = match dir {
            Direction::Forward => nodes
                .filter(|n| n.start_byte() > byte_pos)
                .min_by_key(|n| (n.start_byte(), Reverse(n.end_byte())))?,
            Direction::Backward => nodes
                .filter(|n| n.end_byte() < byte_pos)
                .max_by_key(|n| (n.end_byte(), Reverse(n.start_byte())))?,
        };

        let len = slice.len_bytes();
        let start_byte = node.start_byte();
        let end_byte = node.end_byte();
        if start_byte >= len || end_byte >= len {
            return None;
        }

        let start_char = slice.byte_to_char(start_byte);
        let end_char = slice.byte_to_char(end_byte);

        // head of range should be at beginning
        Some(Range::new(start_char, end_char))
    };
    let mut last_range = range;
    for _ in 0..count {
        match get_range(last_range) {
            Some(r) if r != last_range => last_range = r,
            _ => break,
        }
    }
    last_range
}

fn find_parent_start<'tree>(node: &Node<'tree>) -> Option<Node<'tree>> {
    let start = node.start_byte();
    let mut node = Cow::Borrowed(node);

    while node.start_byte() >= start || !node.is_named() {
        node = Cow::Owned(node.parent()?);
    }

    Some(node.into_owned())
}

pub fn move_parent_node_end(
    syntax: &Syntax,
    text: RopeSlice,
    selection: Selection,
    dir: Direction,
    movement: Movement,
) -> Selection {
    selection.transform(|range| {
        let start_from = text.char_to_byte(range.from()) as u32;
        let start_to = text.char_to_byte(range.to()) as u32;

        let mut node = match syntax.named_descendant_for_byte_range(start_from, start_to) {
            Some(node) => node,
            None => {
                log::debug!(
                    "no descendant found for byte range: {} - {}",
                    start_from,
                    start_to
                );
                return range;
            }
        };

        let mut end_head = match dir {
            // moving forward, we always want to move one past the end of the
            // current node, so use the end byte of the current node, which is an exclusive
            // end of the range
            Direction::Forward => text.byte_to_char(node.end_byte() as usize),

            // moving backward, we want the cursor to land on the start char of
            // the current node, or if it is already at the start of a node, to traverse up to
            // the parent
            Direction::Backward => {
                let end_head = text.byte_to_char(node.start_byte() as usize);

                // if we're already on the beginning, look up to the parent
                if end_head == range.cursor(text) {
                    node = find_parent_start(&node).unwrap_or(node);
                    text.byte_to_char(node.start_byte() as usize)
                } else {
                    end_head
                }
            }
        };

        if movement == Movement::Move {
            // preserve direction of original range
            if range.direction() == Direction::Forward {
                Range::new(end_head, end_head + 1)
            } else {
                Range::new(end_head + 1, end_head)
            }
        } else {
            // if we end up with a forward range, then adjust it to be one past
            // where we want
            if end_head >= range.anchor {
                end_head += 1;
            }

            Range::new(range.anchor, end_head)
        }
    })
}

#[cfg(test)]
mod test {
    use ropey::Rope;

    use crate::{coords_at_pos, pos_at_coords};

    use super::*;

    const SINGLE_LINE_SAMPLE: &str = "This is a simple alphabetic line";
    const MULTILINE_SAMPLE: &str = "\
        Multiline\n\
        text sample\n\
        which\n\
        is merely alphabetic\n\
        and whitespaced\n\
    ";

    const MULTIBYTE_CHARACTER_SAMPLE: &str = "\
        パーティーへ行かないか\n\
        The text above is Japanese\n\
    ";

    #[test]
    fn test_vertical_move() {
        let text = Rope::from("abcd\nefg\nwrs");
        let slice = text.slice(..);
        let pos = pos_at_coords(slice, (0, 4).into(), true);

        let range = Range::new(pos, pos);
        assert_eq!(
            coords_at_pos(
                slice,
                move_vertically_visual(
                    slice,
                    range,
                    Direction::Forward,
                    1,
                    Movement::Move,
                    &TextFormat::default(),
                    &mut TextAnnotations::default(),
                )
                .head
            ),
            (1, 3).into()
        );
    }

    #[test]
    fn horizontal_moves_through_single_line_text() {
        let text = Rope::from(SINGLE_LINE_SAMPLE);
        let slice = text.slice(..);
        let position = pos_at_coords(slice, (0, 0).into(), true);

        let mut range = Range::point(position);

        let moves_and_expected_coordinates = [
            ((Direction::Forward, 1usize), (0, 1)), // T|his is a simple alphabetic line
            ((Direction::Forward, 2usize), (0, 3)), // Thi|s is a simple alphabetic line
            ((Direction::Forward, 0usize), (0, 3)), // Thi|s is a simple alphabetic line
            ((Direction::Forward, 999usize), (0, 32)), // This is a simple alphabetic line|
            ((Direction::Forward, 999usize), (0, 32)), // This is a simple alphabetic line|
            ((Direction::Backward, 999usize), (0, 0)), // |This is a simple alphabetic line
        ];

        for ((direction, amount), coordinates) in moves_and_expected_coordinates {
            range = move_horizontally(
                slice,
                range,
                direction,
                amount,
                Movement::Move,
                &TextFormat::default(),
                &mut TextAnnotations::default(),
            );
            assert_eq!(coords_at_pos(slice, range.head), coordinates.into())
        }
    }

    #[test]
    fn horizontal_moves_through_multiline_text() {
        let text = Rope::from(MULTILINE_SAMPLE);
        let slice = text.slice(..);
        let position = pos_at_coords(slice, (0, 0).into(), true);

        let mut range = Range::point(position);

        let moves_and_expected_coordinates = [
            ((Direction::Forward, 11usize), (1, 1)), // Multiline\nt|ext sample\n...
            ((Direction::Backward, 1usize), (1, 0)), // Multiline\n|text sample\n...
            ((Direction::Backward, 5usize), (0, 5)), // Multi|line\ntext sample\n...
            ((Direction::Backward, 999usize), (0, 0)), // |Multiline\ntext sample\n...
            ((Direction::Forward, 3usize), (0, 3)),  // Mul|tiline\ntext sample\n...
            ((Direction::Forward, 0usize), (0, 3)),  // Mul|tiline\ntext sample\n...
            ((Direction::Backward, 0usize), (0, 3)), // Mul|tiline\ntext sample\n...
            ((Direction::Forward, 999usize), (5, 0)), // ...and whitespaced\n|
            ((Direction::Forward, 999usize), (5, 0)), // ...and whitespaced\n|
        ];

        for ((direction, amount), coordinates) in moves_and_expected_coordinates {
            range = move_horizontally(
                slice,
                range,
                direction,
                amount,
                Movement::Move,
                &TextFormat::default(),
                &mut TextAnnotations::default(),
            );
            assert_eq!(coords_at_pos(slice, range.head), coordinates.into());
            assert_eq!(range.head, range.anchor);
        }
    }

    #[test]
    fn selection_extending_moves_in_single_line_text() {
        let text = Rope::from(SINGLE_LINE_SAMPLE);
        let slice = text.slice(..);
        let position = pos_at_coords(slice, (0, 0).into(), true);

        let mut range = Range::point(position);
        let original_anchor = range.anchor;

        let moves = [
            (Direction::Forward, 1usize),
            (Direction::Forward, 5usize),
            (Direction::Backward, 3usize),
        ];

        for (direction, amount) in moves {
            range = move_horizontally(
                slice,
                range,
                direction,
                amount,
                Movement::Extend,
                &TextFormat::default(),
                &mut TextAnnotations::default(),
            );
            assert_eq!(range.anchor, original_anchor);
        }
    }

    #[test]
    fn vertical_moves_in_single_column() {
        let text = Rope::from(MULTILINE_SAMPLE);
        let slice = text.slice(..);
        let position = pos_at_coords(slice, (0, 0).into(), true);
        let mut range = Range::point(position);
        let moves_and_expected_coordinates = [
            ((Direction::Forward, 1usize), (1, 0)),
            ((Direction::Forward, 2usize), (3, 0)),
            ((Direction::Forward, 1usize), (4, 0)),
            ((Direction::Backward, 999usize), (0, 0)),
            ((Direction::Forward, 4usize), (4, 0)),
            ((Direction::Forward, 0usize), (4, 0)),
            ((Direction::Backward, 0usize), (4, 0)),
            ((Direction::Forward, 5), (5, 0)),
            ((Direction::Forward, 999usize), (5, 0)),
        ];

        for ((direction, amount), coordinates) in moves_and_expected_coordinates {
            range = move_vertically_visual(
                slice,
                range,
                direction,
                amount,
                Movement::Move,
                &TextFormat::default(),
                &mut TextAnnotations::default(),
            );
            assert_eq!(coords_at_pos(slice, range.head), coordinates.into());
            assert_eq!(range.head, range.anchor);
        }
    }

    #[test]
    fn vertical_moves_jumping_column() {
        let text = Rope::from(MULTILINE_SAMPLE);
        let slice = text.slice(..);
        let position = pos_at_coords(slice, (0, 0).into(), true);
        let mut range = Range::point(position);

        enum Axis {
            H,
            V,
        }
        let moves_and_expected_coordinates = [
            // Places cursor at the end of line
            ((Axis::H, Direction::Forward, 8usize), (0, 8)),
            // First descent preserves column as the target line is wider
            ((Axis::V, Direction::Forward, 1usize), (1, 8)),
            // Second descent clamps column as the target line is shorter
            ((Axis::V, Direction::Forward, 1usize), (2, 5)),
            // Third descent restores the original column
            ((Axis::V, Direction::Forward, 1usize), (3, 8)),
            // Behaviour is preserved even through long jumps
            ((Axis::V, Direction::Backward, 999usize), (0, 8)),
            ((Axis::V, Direction::Forward, 4usize), (4, 8)),
            ((Axis::V, Direction::Forward, 999usize), (5, 0)),
        ];

        for ((axis, direction, amount), coordinates) in moves_and_expected_coordinates {
            range = match axis {
                Axis::H => move_horizontally(
                    slice,
                    range,
                    direction,
                    amount,
                    Movement::Move,
                    &TextFormat::default(),
                    &mut TextAnnotations::default(),
                ),
                Axis::V => move_vertically_visual(
                    slice,
                    range,
                    direction,
                    amount,
                    Movement::Move,
                    &TextFormat::default(),
                    &mut TextAnnotations::default(),
                ),
            };
            assert_eq!(coords_at_pos(slice, range.head), coordinates.into());
            assert_eq!(range.head, range.anchor);
        }
    }

    #[test]
    fn multibyte_character_wide_column_jumps() {
        let text = Rope::from(MULTIBYTE_CHARACTER_SAMPLE);
        let slice = text.slice(..);
        let position = pos_at_coords(slice, (0, 0).into(), true);
        let mut range = Range::point(position);

        // FIXME: The behaviour captured in this test diverges from both Kakoune and Vim. These
        // will attempt to preserve the horizontal position of the cursor, rather than
        // placing it at the same character index.
        enum Axis {
            H,
            V,
        }
        let moves_and_expected_coordinates = [
            // Places cursor at the fourth kana.
            ((Axis::H, Direction::Forward, 4), (0, 4)),
            // Descent places cursor at the 8th character.
            ((Axis::V, Direction::Forward, 1usize), (1, 8)),
            // Moving back 2 characters.
            ((Axis::H, Direction::Backward, 2usize), (1, 6)),
            // Jumping back up 1 line.
            ((Axis::V, Direction::Backward, 1usize), (0, 3)),
        ];

        for ((axis, direction, amount), coordinates) in moves_and_expected_coordinates {
            range = match axis {
                Axis::H => move_horizontally(
                    slice,
                    range,
                    direction,
                    amount,
                    Movement::Move,
                    &TextFormat::default(),
                    &mut TextAnnotations::default(),
                ),
                Axis::V => move_vertically_visual(
                    slice,
                    range,
                    direction,
                    amount,
                    Movement::Move,
                    &TextFormat::default(),
                    &mut TextAnnotations::default(),
                ),
            };
            assert_eq!(coords_at_pos(slice, range.head), coordinates.into());
            assert_eq!(range.head, range.anchor);
        }
    }

    #[test]
    #[should_panic]
    fn nonsensical_ranges_panic_on_forward_movement_attempt_in_debug_mode() {
        move_next_word_start(Rope::from("Sample").slice(..), Range::point(99999999), 1);
    }

    #[test]
    #[should_panic]
    fn nonsensical_ranges_panic_on_forward_to_end_movement_attempt_in_debug_mode() {
        move_next_word_end(Rope::from("Sample").slice(..), Range::point(99999999), 1);
    }

    #[test]
    #[should_panic]
    fn nonsensical_ranges_panic_on_backwards_movement_attempt_in_debug_mode() {
        move_prev_word_start(Rope::from("Sample").slice(..), Range::point(99999999), 1);
    }

    #[test]
    fn test_behaviour_when_moving_to_start_of_next_words() {
        let tests = [
            ("Basic forward motion stops at the first space",
                vec![(1, Range::new(0, 0), Range::new(0, 6))]),
            (" Starting from a boundary advances the anchor",
                vec![(1, Range::new(0, 0), Range::new(1, 10))]),
            ("Long       whitespace gap is bridged by the head",
                vec![(1, Range::new(0, 0), Range::new(0, 11))]),
            ("Previous anchor is irrelevant for forward motions",
                vec![(1, Range::new(12, 0), Range::new(0, 9))]),
            ("    Starting from whitespace moves to last space in sequence",
                vec![(1, Range::new(0, 0), Range::new(0, 4))]),
            ("Starting from mid-word leaves anchor at start position and moves head",
                vec![(1, Range::new(3, 3), Range::new(3, 9))]),
            ("Identifiers_with_underscores are considered a single word",
                vec![(1, Range::new(0, 0), Range::new(0, 29))]),
            ("Jumping\n    into starting whitespace selects the spaces before 'into'",
                vec![(1, Range::new(0, 7), Range::new(8, 12))]),
            ("alphanumeric.!,and.?=punctuation are considered 'words' for the purposes of word motion",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 12)),
                    (1, Range::new(0, 12), Range::new(12, 15)),
                    (1, Range::new(12, 15), Range::new(15, 18))
                ]),
            ("...   ... punctuation and spaces behave as expected",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 6)),
                    (1, Range::new(0, 6), Range::new(6, 10)),
                ]),
            (".._.._ punctuation is not joined by underscores into a single block",
                vec![(1, Range::new(0, 0), Range::new(0, 2))]),
            ("Newlines\n\nare bridged seamlessly.",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 8)),
                    (1, Range::new(0, 8), Range::new(10, 14)),
                ]),
            ("Jumping\n\n\n\n\n\n   from newlines to whitespace selects whitespace.",
                vec![
                    (1, Range::new(0, 9), Range::new(13, 16)),
                ]),
            ("A failed motion does not modify the range",
                vec![
                    (3, Range::new(37, 41), Range::new(37, 41)),
                ]),
            ("oh oh oh two character words!",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 3)),
                    (1, Range::new(0, 3), Range::new(3, 6)),
                    (1, Range::new(0, 2), Range::new(1, 3)),
                ]),
            ("Multiple motions at once resolve correctly",
                vec![
                    (3, Range::new(0, 0), Range::new(17, 20)),
                ]),
            ("Excessive motions are performed partially",
                vec![
                    (999, Range::new(0, 0), Range::new(32, 41)),
                ]),
            ("", // Edge case of moving forward in empty string
                vec![
                    (1, Range::new(0, 0), Range::new(0, 0)),
                ]),
            ("\n\n\n\n\n", // Edge case of moving forward in all newlines
                vec![
                    (1, Range::new(0, 0), Range::new(5, 5)),
                ]),
            ("\n   \n   \n Jumping through alternated space blocks and newlines selects the space blocks",
                vec![
                    (1, Range::new(0, 0), Range::new(1, 4)),
                    (1, Range::new(1, 4), Range::new(5, 8)),
                ]),
            ("ヒーリクス multibyte characters behave as normal characters",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 6)),
                ]),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_next_word_start(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_vim_next_word_start_caret() {
        // (sample, cursor, count, expected caret char index) — expected values are
        // what stock vim `w`/`W` produce. The regression this guards: `w` on the
        // whitespace immediately before a token must land on that token, not skip
        // it. `\tfetch = +x`: \t0 f1 e2 t3 c4 h5 ' '6 =7 ' '8 +9 x10
        let w = [
            ("\tfetch = +x", 0, 1, 1), // tab indent: land on `fetch`, not `=`
            ("\tfetch = +x", 0, 2, 7), // 2w: fetch -> `=`
            ("\tfetch = +x", 0, 3, 9), // 3w: -> `+`
            ("\tfetch = +x", 1, 1, 7), // from `f`: -> `=`
            ("\tfetch = +x", 1, 2, 9), // from `f`, 2w: -> `+` (count composes)
            ("foo bar baz", 3, 1, 4),  // single space before token: -> `bar`
            ("foo bar baz", 0, 1, 4),  // from word: -> `bar`
            ("foo  bar", 0, 1, 5),     // two spaces: -> `bar` (unchanged)
            ("foo  bar", 3, 1, 5),     // on first of two spaces: -> `bar`
            ("a\nb", 0, 1, 2),         // across newline: -> `b`
        ];
        for (sample, cursor, count, expected) in w {
            let range =
                move_next_word_start_vim(Rope::from(sample).slice(..), Range::point(cursor), count);
            assert_eq!(
                range.head, expected,
                "w case failed: [{:?}] cursor={} count={}",
                sample, cursor, count
            );
        }

        // `W` (long word): punctuation joins the surrounding token.
        // `.foo bar`: .0 f1 o2 o3 ' '4 b5 a6 r7 — `.foo` is one WORD.
        let big = [
            ("\t.foo bar", 0, 1, 1), // tab indent: land on `.foo`
            (".foo bar", 0, 1, 5),   // from `.foo`: -> `bar`
        ];
        for (sample, cursor, count, expected) in big {
            let range = move_next_long_word_start_vim(
                Rope::from(sample).slice(..),
                Range::point(cursor),
                count,
            );
            assert_eq!(
                range.head, expected,
                "W case failed: [{:?}] cursor={} count={}",
                sample, cursor, count
            );
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_start_of_next_sub_words() {
        let tests = [
            (
                "NextSubwordStart",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 11)),
                ],
            ),
            (
                "next_subword_start",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 5)),
                    (1, Range::new(4, 4), Range::new(5, 13)),
                ],
            ),
            (
                "Next_Subword_Start",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 5)),
                    (1, Range::new(4, 4), Range::new(5, 13)),
                ],
            ),
            (
                "NEXT_SUBWORD_START",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 5)),
                    (1, Range::new(4, 4), Range::new(5, 13)),
                ],
            ),
            (
                "next subword start",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 5)),
                    (1, Range::new(4, 4), Range::new(5, 13)),
                ],
            ),
            (
                "Next Subword Start",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 5)),
                    (1, Range::new(4, 4), Range::new(5, 13)),
                ],
            ),
            (
                "NEXT SUBWORD START",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 5)),
                    (1, Range::new(4, 4), Range::new(5, 13)),
                ],
            ),
            (
                "next__subword__start",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 6)),
                    (1, Range::new(4, 4), Range::new(4, 6)),
                    (1, Range::new(5, 5), Range::new(6, 15)),
                ],
            ),
            (
                "Next__Subword__Start",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 6)),
                    (1, Range::new(4, 4), Range::new(4, 6)),
                    (1, Range::new(5, 5), Range::new(6, 15)),
                ],
            ),
            (
                "NEXT__SUBWORD__START",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 6)),
                    (1, Range::new(4, 4), Range::new(4, 6)),
                    (1, Range::new(5, 5), Range::new(6, 15)),
                ],
            ),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_next_sub_word_start(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_end_of_next_sub_words() {
        let tests = [
            (
                "NextSubwordEnd",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 11)),
                ],
            ),
            (
                "next subword end",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 12)),
                ],
            ),
            (
                "Next Subword End",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 12)),
                ],
            ),
            (
                "NEXT SUBWORD END",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 12)),
                ],
            ),
            (
                "next_subword_end",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 12)),
                ],
            ),
            (
                "Next_Subword_End",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 12)),
                ],
            ),
            (
                "NEXT_SUBWORD_END",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 12)),
                ],
            ),
            (
                "next__subword__end",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 13)),
                    (1, Range::new(5, 5), Range::new(5, 13)),
                ],
            ),
            (
                "Next__Subword__End",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 13)),
                    (1, Range::new(5, 5), Range::new(5, 13)),
                ],
            ),
            (
                "NEXT__SUBWORD__END",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 4)),
                    (1, Range::new(4, 4), Range::new(4, 13)),
                    (1, Range::new(5, 5), Range::new(5, 13)),
                ],
            ),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_next_sub_word_end(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_start_of_next_long_words() {
        let tests = [
            ("Basic forward motion stops at the first space",
                vec![(1, Range::new(0, 0), Range::new(0, 6))]),
            (" Starting from a boundary advances the anchor",
                vec![(1, Range::new(0, 0), Range::new(1, 10))]),
            ("Long       whitespace gap is bridged by the head",
                vec![(1, Range::new(0, 0), Range::new(0, 11))]),
            ("Previous anchor is irrelevant for forward motions",
                vec![(1, Range::new(12, 0), Range::new(0, 9))]),
            ("    Starting from whitespace moves to last space in sequence",
                vec![(1, Range::new(0, 0), Range::new(0, 4))]),
            ("Starting from mid-word leaves anchor at start position and moves head",
                vec![(1, Range::new(3, 3), Range::new(3, 9))]),
            ("Identifiers_with_underscores are considered a single word",
                vec![(1, Range::new(0, 0), Range::new(0, 29))]),
            ("Jumping\n    into starting whitespace selects the spaces before 'into'",
                vec![(1, Range::new(0, 7), Range::new(8, 12))]),
            ("alphanumeric.!,and.?=punctuation are not treated any differently than alphanumerics",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 33)),
                ]),
            ("...   ... punctuation and spaces behave as expected",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 6)),
                    (1, Range::new(0, 6), Range::new(6, 10)),
                ]),
            (".._.._ punctuation is joined by underscores into a single word, as it behaves like alphanumerics",
                vec![(1, Range::new(0, 0), Range::new(0, 7))]),
            ("Newlines\n\nare bridged seamlessly.",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 8)),
                    (1, Range::new(0, 8), Range::new(10, 14)),
                ]),
            ("Jumping\n\n\n\n\n\n   from newlines to whitespace selects whitespace.",
                vec![
                    (1, Range::new(0, 9), Range::new(13, 16)),
                ]),
            ("A failed motion does not modify the range",
                vec![
                    (3, Range::new(37, 41), Range::new(37, 41)),
                ]),
            ("oh oh oh two character words!",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 3)),
                    (1, Range::new(0, 3), Range::new(3, 6)),
                    (1, Range::new(0, 1), Range::new(0, 3)),
                ]),
            ("Multiple motions at once resolve correctly",
                vec![
                    (3, Range::new(0, 0), Range::new(17, 20)),
                ]),
            ("Excessive motions are performed partially",
                vec![
                    (999, Range::new(0, 0), Range::new(32, 41)),
                ]),
            ("", // Edge case of moving forward in empty string
                vec![
                    (1, Range::new(0, 0), Range::new(0, 0)),
                ]),
            ("\n\n\n\n\n", // Edge case of moving forward in all newlines
                vec![
                    (1, Range::new(0, 0), Range::new(5, 5)),
                ]),
            ("\n   \n   \n Jumping through alternated space blocks and newlines selects the space blocks",
                vec![
                    (1, Range::new(0, 0), Range::new(1, 4)),
                    (1, Range::new(1, 4), Range::new(5, 8)),
                ]),
            ("ヒー..リクス multibyte characters behave as normal characters, including their interaction with punctuation",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 8)),
                ]),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_next_long_word_start(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_start_of_previous_words() {
        let tests = [
            ("Basic backward motion from the middle of a word",
                vec![(1, Range::new(3, 3), Range::new(4, 0))]),

            // // Why do we want this behavior?  The current behavior fails this
            // // test, but seems better and more consistent.
            // ("Starting from after boundary retreats the anchor",
            //     vec![(1, Range::new(0, 9), Range::new(8, 0))]),

            ("    Jump to start of a word preceded by whitespace",
                vec![(1, Range::new(5, 5), Range::new(6, 4))]),
            ("    Jump to start of line from start of word preceded by whitespace",
                vec![(1, Range::new(4, 4), Range::new(4, 0))]),
            ("Previous anchor is irrelevant for backward motions",
                vec![(1, Range::new(12, 5), Range::new(6, 0))]),
            ("    Starting from whitespace moves to first space in sequence",
                vec![(1, Range::new(0, 4), Range::new(4, 0))]),
            ("Identifiers_with_underscores are considered a single word",
                vec![(1, Range::new(0, 20), Range::new(20, 0))]),
            ("Jumping\n    \nback through a newline selects whitespace",
                vec![(1, Range::new(0, 13), Range::new(12, 8))]),
            ("Jumping to start of word from the end selects the word",
                vec![(1, Range::new(6, 7), Range::new(7, 0))]),
            ("alphanumeric.!,and.?=punctuation are considered 'words' for the purposes of word motion",
                vec![
                    (1, Range::new(29, 30), Range::new(30, 21)),
                    (1, Range::new(30, 21), Range::new(21, 18)),
                    (1, Range::new(21, 18), Range::new(18, 15))
                ]),
            ("...   ... punctuation and spaces behave as expected",
                vec![
                    (1, Range::new(0, 10), Range::new(10, 6)),
                    (1, Range::new(10, 6), Range::new(6, 0)),
                ]),
            (".._.._ punctuation is not joined by underscores into a single block",
                vec![(1, Range::new(0, 6), Range::new(5, 3))]),
            ("Newlines\n\nare bridged seamlessly.",
                vec![
                    (1, Range::new(0, 10), Range::new(8, 0)),
                ]),
            ("Jumping    \n\n\n\n\nback from within a newline group selects previous block",
                vec![
                    (1, Range::new(0, 13), Range::new(11, 0)),
                ]),
            ("Failed motions do not modify the range",
                vec![
                    (0, Range::new(3, 0), Range::new(3, 0)),
                ]),
            ("Multiple motions at once resolve correctly",
                vec![
                    (3, Range::new(18, 18), Range::new(9, 0)),
                ]),
            ("Excessive motions are performed partially",
                vec![
                    (999, Range::new(40, 40), Range::new(10, 0)),
                ]),
            ("", // Edge case of moving backwards in empty string
                vec![
                    (1, Range::new(0, 0), Range::new(0, 0)),
                ]),
            ("\n\n\n\n\n", // Edge case of moving backwards in all newlines
                vec![
                    (1, Range::new(5, 5), Range::new(0, 0)),
                ]),
            ("   \n   \nJumping back through alternated space blocks and newlines selects the space blocks",
                vec![
                    (1, Range::new(0, 8), Range::new(7, 4)),
                    (1, Range::new(7, 4), Range::new(3, 0)),
                ]),
            ("ヒーリクス multibyte characters behave as normal characters",
                vec![
                    (1, Range::new(0, 6), Range::new(6, 0)),
                ]),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_prev_word_start(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_start_of_previous_sub_words() {
        let tests = [
            (
                "PrevSubwordEnd",
                vec![
                    (1, Range::new(13, 13), Range::new(14, 11)),
                    (1, Range::new(11, 11), Range::new(11, 4)),
                ],
            ),
            (
                "prev subword end",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 13)),
                    (1, Range::new(12, 12), Range::new(13, 5)),
                ],
            ),
            (
                "Prev Subword End",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 13)),
                    (1, Range::new(12, 12), Range::new(13, 5)),
                ],
            ),
            (
                "PREV SUBWORD END",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 13)),
                    (1, Range::new(12, 12), Range::new(13, 5)),
                ],
            ),
            (
                "prev_subword_end",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 13)),
                    (1, Range::new(12, 12), Range::new(13, 5)),
                ],
            ),
            (
                "Prev_Subword_End",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 13)),
                    (1, Range::new(12, 12), Range::new(13, 5)),
                ],
            ),
            (
                "PREV_SUBWORD_END",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 13)),
                    (1, Range::new(12, 12), Range::new(13, 5)),
                ],
            ),
            (
                "prev__subword__end",
                vec![
                    (1, Range::new(17, 17), Range::new(18, 15)),
                    (1, Range::new(13, 13), Range::new(14, 6)),
                    (1, Range::new(14, 14), Range::new(15, 6)),
                ],
            ),
            (
                "Prev__Subword__End",
                vec![
                    (1, Range::new(17, 17), Range::new(18, 15)),
                    (1, Range::new(13, 13), Range::new(14, 6)),
                    (1, Range::new(14, 14), Range::new(15, 6)),
                ],
            ),
            (
                "PREV__SUBWORD__END",
                vec![
                    (1, Range::new(17, 17), Range::new(18, 15)),
                    (1, Range::new(13, 13), Range::new(14, 6)),
                    (1, Range::new(14, 14), Range::new(15, 6)),
                ],
            ),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_prev_sub_word_start(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_start_of_previous_long_words() {
        let tests = [
            (
                "Basic backward motion from the middle of a word",
                vec![(1, Range::new(3, 3), Range::new(4, 0))],
            ),

            // // Why do we want this behavior?  The current behavior fails this
            // // test, but seems better and more consistent.
            // ("Starting from after boundary retreats the anchor",
            //     vec![(1, Range::new(0, 9), Range::new(8, 0))]),

            (
                "    Jump to start of a word preceded by whitespace",
                vec![(1, Range::new(5, 5), Range::new(6, 4))],
            ),
            (
                "    Jump to start of line from start of word preceded by whitespace",
                vec![(1, Range::new(3, 4), Range::new(4, 0))],
            ),
            ("Previous anchor is irrelevant for backward motions",
                vec![(1, Range::new(12, 5), Range::new(6, 0))]),
            (
                "    Starting from whitespace moves to first space in sequence",
                vec![(1, Range::new(0, 4), Range::new(4, 0))],
            ),
            ("Identifiers_with_underscores are considered a single word",
                vec![(1, Range::new(0, 20), Range::new(20, 0))]),
            (
                "Jumping\n    \nback through a newline selects whitespace",
                vec![(1, Range::new(0, 13), Range::new(12, 8))],
            ),
            (
                "Jumping to start of word from the end selects the word",
                vec![(1, Range::new(6, 7), Range::new(7, 0))],
            ),
            (
                "alphanumeric.!,and.?=punctuation are treated exactly the same",
                vec![(1, Range::new(29, 30), Range::new(30, 0))],
            ),
            (
                "...   ... punctuation and spaces behave as expected",
                vec![
                    (1, Range::new(0, 10), Range::new(10, 6)),
                    (1, Range::new(10, 6), Range::new(6, 0)),
                ],
            ),
            (".._.._ punctuation is joined by underscores into a single block",
                vec![(1, Range::new(0, 6), Range::new(6, 0))]),
            (
                "Newlines\n\nare bridged seamlessly.",
                vec![(1, Range::new(0, 10), Range::new(8, 0))],
            ),
            (
                "Jumping    \n\n\n\n\nback from within a newline group selects previous block",
                vec![(1, Range::new(0, 13), Range::new(11, 0))],
            ),
            (
                "Failed motions do not modify the range",
                vec![(0, Range::new(3, 0), Range::new(3, 0))],
            ),
            (
                "Multiple motions at once resolve correctly",
                vec![(3, Range::new(19, 19), Range::new(9, 0))],
            ),
            (
                "Excessive motions are performed partially",
                vec![(999, Range::new(40, 40), Range::new(10, 0))],
            ),
            (
                "", // Edge case of moving backwards in empty string
                vec![(1, Range::new(0, 0), Range::new(0, 0))],
            ),
            (
                "\n\n\n\n\n", // Edge case of moving backwards in all newlines
                vec![(1, Range::new(5, 5), Range::new(0, 0))],
            ),
            ("   \n   \nJumping back through alternated space blocks and newlines selects the space blocks",
                vec![
                    (1, Range::new(0, 8), Range::new(7, 4)),
                    (1, Range::new(7, 4), Range::new(3, 0)),
                ]),
            ("ヒーリ..クス multibyte characters behave as normal characters, including when interacting with punctuation",
                vec![
                    (1, Range::new(0, 8), Range::new(8, 0)),
                ]),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_prev_long_word_start(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_end_of_next_words() {
        let tests = [
            ("Basic forward motion from the start of a word to the end of it",
                vec![(1, Range::new(0, 0), Range::new(0, 5))]),
            ("Basic forward motion from the end of a word to the end of the next",
                vec![(1, Range::new(0, 5), Range::new(5, 13))]),
            ("Basic forward motion from the middle of a word to the end of it",
                vec![(1, Range::new(2, 2), Range::new(2, 5))]),
            ("    Jumping to end of a word preceded by whitespace",
                vec![(1, Range::new(0, 0), Range::new(0, 11))]),

            // // Why do we want this behavior?  The current behavior fails this
            // // test, but seems better and more consistent.
            // (" Starting from a boundary advances the anchor",
            //     vec![(1, Range::new(0, 0), Range::new(1, 9))]),

            ("Previous anchor is irrelevant for end of word motion",
                vec![(1, Range::new(12, 2), Range::new(2, 8))]),
            ("Identifiers_with_underscores are considered a single word",
                vec![(1, Range::new(0, 0), Range::new(0, 28))]),
            ("Jumping\n    into starting whitespace selects up to the end of next word",
                vec![(1, Range::new(0, 7), Range::new(8, 16))]),
            ("alphanumeric.!,and.?=punctuation are considered 'words' for the purposes of word motion",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 12)),
                    (1, Range::new(0, 12), Range::new(12, 15)),
                    (1, Range::new(12, 15), Range::new(15, 18))
                ]),
            ("...   ... punctuation and spaces behave as expected",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 3)),
                    (1, Range::new(0, 3), Range::new(3, 9)),
                ]),
            (".._.._ punctuation is not joined by underscores into a single block",
                vec![(1, Range::new(0, 0), Range::new(0, 2))]),
            ("Newlines\n\nare bridged seamlessly.",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 8)),
                    (1, Range::new(0, 8), Range::new(10, 13)),
                ]),
            ("Jumping\n\n\n\n\n\n   from newlines to whitespace selects to end of next word.",
                vec![
                    (1, Range::new(0, 8), Range::new(13, 20)),
                ]),
            ("A failed motion does not modify the range",
                vec![
                    (3, Range::new(37, 41), Range::new(37, 41)),
                ]),
            ("Multiple motions at once resolve correctly",
                vec![
                    (3, Range::new(0, 0), Range::new(16, 19)),
                ]),
            ("Excessive motions are performed partially",
                vec![
                    (999, Range::new(0, 0), Range::new(31, 41)),
                ]),
            ("", // Edge case of moving forward in empty string
                vec![
                    (1, Range::new(0, 0), Range::new(0, 0)),
                ]),
            ("\n\n\n\n\n", // Edge case of moving forward in all newlines
                vec![
                    (1, Range::new(0, 0), Range::new(5, 5)),
                ]),
            ("\n   \n   \n Jumping through alternated space blocks and newlines selects the space blocks",
                vec![
                    (1, Range::new(0, 0), Range::new(1, 4)),
                    (1, Range::new(1, 4), Range::new(5, 8)),
                ]),
            ("ヒーリクス multibyte characters behave as normal characters",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 5)),
                ]),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_next_word_end(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_end_of_previous_words() {
        let tests = [
            ("Basic backward motion from the middle of a word",
                vec![(1, Range::new(9, 9), Range::new(10, 5))]),
            ("Starting from after boundary retreats the anchor",
                vec![(1, Range::new(0, 14), Range::new(13, 8))]),
            ("Jump     to end of a word succeeded by whitespace",
                vec![(1, Range::new(11, 11), Range::new(11, 4))]),
            ("    Jump to start of line from end of word preceded by whitespace",
                vec![(1, Range::new(8, 8), Range::new(8, 0))]),
            ("Previous anchor is irrelevant for backward motions",
                vec![(1, Range::new(26, 12), Range::new(13, 8))]),
            ("    Starting from whitespace moves to first space in sequence",
                vec![(1, Range::new(0, 4), Range::new(4, 0))]),
            ("Test identifiers_with_underscores are considered a single word",
                vec![(1, Range::new(0, 25), Range::new(25, 4))]),
            ("Jumping\n    \nback through a newline selects whitespace",
                vec![(1, Range::new(0, 13), Range::new(12, 8))]),
            ("Jumping to start of word from the end selects the whole word",
                vec![(1, Range::new(16, 16), Range::new(16, 10))]),
            ("alphanumeric.!,and.?=punctuation are considered 'words' for the purposes of word motion",
                vec![
                    (1, Range::new(30, 30), Range::new(31, 21)),
                    (1, Range::new(31, 21), Range::new(21, 18)),
                    (1, Range::new(21, 18), Range::new(18, 15))
                ]),

            ("...   ... punctuation and spaces behave as expected",
                vec![
                    (1, Range::new(0, 10), Range::new(9, 3)),
                    (1, Range::new(9, 3), Range::new(3, 0)),
                ]),
            (".._.._ punctuation is not joined by underscores into a single block",
                vec![(1, Range::new(0, 5), Range::new(5, 3))]),
            ("Newlines\n\nare bridged seamlessly.",
                vec![
                    (1, Range::new(0, 10), Range::new(8, 0)),
                ]),
            ("Jumping    \n\n\n\n\nback from within a newline group selects previous block",
                vec![
                    (1, Range::new(0, 13), Range::new(11, 7)),
                ]),
            ("Failed motions do not modify the range",
                vec![
                    (0, Range::new(3, 0), Range::new(3, 0)),
                ]),
            ("Multiple motions at once resolve correctly",
                vec![
                    (3, Range::new(24, 24), Range::new(16, 8)),
                ]),
            ("Excessive motions are performed partially",
                vec![
                    (999, Range::new(40, 40), Range::new(9, 0)),
                ]),
            ("", // Edge case of moving backwards in empty string
                vec![
                    (1, Range::new(0, 0), Range::new(0, 0)),
                ]),
            ("\n\n\n\n\n", // Edge case of moving backwards in all newlines
                vec![
                    (1, Range::new(5, 5), Range::new(0, 0)),
                ]),
            ("   \n   \nJumping back through alternated space blocks and newlines selects the space blocks",
                vec![
                    (1, Range::new(0, 8), Range::new(7, 4)),
                    (1, Range::new(7, 4), Range::new(3, 0)),
                ]),
            ("Test ヒーリクス multibyte characters behave as normal characters",
                vec![
                    (1, Range::new(0, 10), Range::new(10, 4)),
                ]),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_prev_word_end(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_end_of_previous_sub_words() {
        let tests = [
            (
                "PrevSubwordEnd",
                vec![
                    (1, Range::new(13, 13), Range::new(14, 11)),
                    (1, Range::new(11, 11), Range::new(11, 4)),
                ],
            ),
            (
                "prev subword end",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 12)),
                    (1, Range::new(12, 12), Range::new(12, 4)),
                ],
            ),
            (
                "Prev Subword End",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 12)),
                    (1, Range::new(12, 12), Range::new(12, 4)),
                ],
            ),
            (
                "PREV SUBWORD END",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 12)),
                    (1, Range::new(12, 12), Range::new(12, 4)),
                ],
            ),
            (
                "prev_subword_end",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 12)),
                    (1, Range::new(12, 12), Range::new(12, 4)),
                ],
            ),
            (
                "Prev_Subword_End",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 12)),
                    (1, Range::new(12, 12), Range::new(12, 4)),
                ],
            ),
            (
                "PREV_SUBWORD_END",
                vec![
                    (1, Range::new(15, 15), Range::new(16, 12)),
                    (1, Range::new(12, 12), Range::new(12, 4)),
                ],
            ),
            (
                "prev__subword__end",
                vec![
                    (1, Range::new(17, 17), Range::new(18, 13)),
                    (1, Range::new(13, 13), Range::new(13, 4)),
                    (1, Range::new(14, 14), Range::new(15, 13)),
                ],
            ),
            (
                "Prev__Subword__End",
                vec![
                    (1, Range::new(17, 17), Range::new(18, 13)),
                    (1, Range::new(13, 13), Range::new(13, 4)),
                    (1, Range::new(14, 14), Range::new(15, 13)),
                ],
            ),
            (
                "PREV__SUBWORD__END",
                vec![
                    (1, Range::new(17, 17), Range::new(18, 13)),
                    (1, Range::new(13, 13), Range::new(13, 4)),
                    (1, Range::new(14, 14), Range::new(15, 13)),
                ],
            ),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_prev_sub_word_end(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_end_of_next_long_words() {
        let tests = [
            ("Basic forward motion from the start of a word to the end of it",
                vec![(1, Range::new(0, 0), Range::new(0, 5))]),
            ("Basic forward motion from the end of a word to the end of the next",
                vec![(1, Range::new(0, 5), Range::new(5, 13))]),
            ("Basic forward motion from the middle of a word to the end of it",
                vec![(1, Range::new(2, 2), Range::new(2, 5))]),
            ("    Jumping to end of a word preceded by whitespace",
                vec![(1, Range::new(0, 0), Range::new(0, 11))]),

            // // Why do we want this behavior?  The current behavior fails this
            // // test, but seems better and more consistent.
            // (" Starting from a boundary advances the anchor",
            //     vec![(1, Range::new(0, 0), Range::new(1, 9))]),

            ("Previous anchor is irrelevant for end of word motion",
                vec![(1, Range::new(12, 2), Range::new(2, 8))]),
            ("Identifiers_with_underscores are considered a single word",
                vec![(1, Range::new(0, 0), Range::new(0, 28))]),
            ("Jumping\n    into starting whitespace selects up to the end of next word",
                vec![(1, Range::new(0, 7), Range::new(8, 16))]),
            ("alphanumeric.!,and.?=punctuation are treated the same way",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 32)),
                ]),
            ("...   ... punctuation and spaces behave as expected",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 3)),
                    (1, Range::new(0, 3), Range::new(3, 9)),
                ]),
            (".._.._ punctuation is joined by underscores into a single block",
                vec![(1, Range::new(0, 0), Range::new(0, 6))]),
            ("Newlines\n\nare bridged seamlessly.",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 8)),
                    (1, Range::new(0, 8), Range::new(10, 13)),
                ]),
            ("Jumping\n\n\n\n\n\n   from newlines to whitespace selects to end of next word.",
                vec![
                    (1, Range::new(0, 9), Range::new(13, 20)),
                ]),
            ("A failed motion does not modify the range",
                vec![
                    (3, Range::new(37, 41), Range::new(37, 41)),
                ]),
            ("Multiple motions at once resolve correctly",
                vec![
                    (3, Range::new(0, 0), Range::new(16, 19)),
                ]),
            ("Excessive motions are performed partially",
                vec![
                    (999, Range::new(0, 0), Range::new(31, 41)),
                ]),
            ("", // Edge case of moving forward in empty string
                vec![
                    (1, Range::new(0, 0), Range::new(0, 0)),
                ]),
            ("\n\n\n\n\n", // Edge case of moving forward in all newlines
                vec![
                    (1, Range::new(0, 0), Range::new(5, 5)),
                ]),
            ("\n   \n   \n Jumping through alternated space blocks and newlines selects the space blocks",
                vec![
                    (1, Range::new(0, 0), Range::new(1, 4)),
                    (1, Range::new(1, 4), Range::new(5, 8)),
                ]),
            ("ヒーリ..クス multibyte characters behave as normal characters, including  when they interact with punctuation",
                vec![
                    (1, Range::new(0, 0), Range::new(0, 7)),
                ]),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_next_long_word_end(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_end_of_prev_long_words() {
        let tests = [
            (
                "Basic backward motion from the middle of a word",
                vec![(1, Range::new(3, 3), Range::new(4, 0))],
            ),
            ("Starting from after boundary retreats the anchor",
                vec![(1, Range::new(0, 9), Range::new(8, 0))],
            ),
            (
                "Jump    to end of a word succeeded by whitespace",
                vec![(1, Range::new(10, 10), Range::new(10, 4))],
            ),
            (
                "    Jump to start of line from end of word preceded by whitespace",
                vec![(1, Range::new(3, 4), Range::new(4, 0))],
            ),
            ("Previous anchor is irrelevant for backward motions",
                vec![(1, Range::new(12, 5), Range::new(6, 0))]),
            (
                "    Starting from whitespace moves to first space in sequence",
                vec![(1, Range::new(0, 4), Range::new(4, 0))],
            ),
            ("Identifiers_with_underscores are considered a single word",
                vec![(1, Range::new(0, 20), Range::new(20, 0))]),
            (
                "Jumping\n    \nback through a newline selects whitespace",
                vec![(1, Range::new(0, 13), Range::new(12, 8))],
            ),
            (
                "Jumping to start of word from the end selects the word",
                vec![(1, Range::new(6, 7), Range::new(7, 0))],
            ),
            (
                "alphanumeric.!,and.?=punctuation are treated exactly the same",
                vec![(1, Range::new(29, 30), Range::new(30, 0))],
            ),
            (
                "...   ... punctuation and spaces behave as expected",
                vec![
                    (1, Range::new(0, 10), Range::new(9, 3)),
                    (1, Range::new(10, 6), Range::new(7, 3)),
                ],
            ),
            (".._.._ punctuation is joined by underscores into a single block",
                vec![(1, Range::new(0, 6), Range::new(6, 0))]),
            (
                "Newlines\n\nare bridged seamlessly.",
                vec![(1, Range::new(0, 10), Range::new(8, 0))],
            ),
            (
                "Jumping    \n\n\n\n\nback from within a newline group selects previous block",
                vec![(1, Range::new(0, 13), Range::new(11, 7))],
            ),
            (
                "Failed motions do not modify the range",
                vec![(0, Range::new(3, 0), Range::new(3, 0))],
            ),
            (
                "Multiple motions at once resolve correctly",
                vec![(3, Range::new(19, 19), Range::new(8, 0))],
            ),
            (
                "Excessive motions are performed partially",
                vec![(999, Range::new(40, 40), Range::new(9, 0))],
            ),
            (
                "", // Edge case of moving backwards in empty string
                vec![(1, Range::new(0, 0), Range::new(0, 0))],
            ),
            (
                "\n\n\n\n\n", // Edge case of moving backwards in all newlines
                vec![(1, Range::new(5, 5), Range::new(0, 0))],
            ),
            ("   \n   \nJumping back through alternated space blocks and newlines selects the space blocks",
                vec![
                    (1, Range::new(0, 8), Range::new(7, 4)),
                    (1, Range::new(7, 4), Range::new(3, 0)),
                ]),
            ("ヒーリ..クス multibyte characters behave as normal characters, including when interacting with punctuation",
                vec![
                    (1, Range::new(0, 8), Range::new(7, 0)),
                ]),
        ];

        for (sample, scenario) in tests {
            for (count, begin, expected_end) in scenario.into_iter() {
                let range = move_prev_long_word_end(Rope::from(sample).slice(..), begin, count);
                assert_eq!(range, expected_end, "Case failed: [{}]", sample);
            }
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_prev_paragraph_single() {
        let tests = [
            ("#[|]#", "#[|]#"),
            ("#[s|]#tart at\nfirst char\n", "#[|s]#tart at\nfirst char\n"),
            ("start at\nlast char#[\n|]#", "#[|start at\nlast char\n]#"),
            (
                "goto\nfirst\n\n#[p|]#aragraph",
                "#[|goto\nfirst\n\n]#paragraph",
            ),
            (
                "goto\nfirst\n#[\n|]#paragraph",
                "#[|goto\nfirst\n\n]#paragraph",
            ),
            (
                "goto\nsecond\n\np#[a|]#ragraph",
                "goto\nsecond\n\n#[|pa]#ragraph",
            ),
            (
                "here\n\nhave\nmultiple\nparagraph\n\n\n\n\n#[|]#",
                "here\n\n#[|have\nmultiple\nparagraph\n\n\n\n\n]#",
            ),
        ];

        for (before, expected) in tests {
            let (s, selection) = crate::test::print(before);
            let text = Rope::from(s.as_str());
            let selection =
                selection.transform(|r| move_prev_paragraph(text.slice(..), r, 1, Movement::Move));
            let actual = crate::test::plain(s.as_ref(), &selection);
            assert_eq!(actual, expected, "\nbefore: `{:?}`", before);
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_prev_paragraph_double() {
        let tests = [
            (
                "on#[e|]#\n\ntwo\n\nthree\n\n",
                "#[|one]#\n\ntwo\n\nthree\n\n",
            ),
            (
                "one\n\ntwo\n\nth#[r|]#ee\n\n",
                "one\n\n#[|two\n\nthr]#ee\n\n",
            ),
        ];

        for (before, expected) in tests {
            let (s, selection) = crate::test::print(before);
            let text = Rope::from(s.as_str());
            let selection =
                selection.transform(|r| move_prev_paragraph(text.slice(..), r, 2, Movement::Move));
            let actual = crate::test::plain(s.as_ref(), &selection);
            assert_eq!(actual, expected, "\nbefore: `{:?}`", before);
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_prev_paragraph_extend() {
        let tests = [
            (
                "one\n\n#[|two\n\n]#three\n\n",
                "#[|one\n\ntwo\n\n]#three\n\n",
            ),
            (
                "#[|one\n\ntwo\n\n]#three\n\n",
                "#[|one\n\ntwo\n\n]#three\n\n",
            ),
        ];

        for (before, expected) in tests {
            let (s, selection) = crate::test::print(before);
            let text = Rope::from(s.as_str());
            let selection = selection
                .transform(|r| move_prev_paragraph(text.slice(..), r, 1, Movement::Extend));
            let actual = crate::test::plain(s.as_ref(), &selection);
            assert_eq!(actual, expected, "\nbefore: `{:?}`", before);
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_next_paragraph_single() {
        let tests = [
            ("#[|]#", "#[|]#"),
            ("#[s|]#tart at\nfirst char\n", "#[start at\nfirst char\n|]#"),
            ("start at\nlast char#[\n|]#", "start at\nlast char#[\n|]#"),
            (
                "a\nb\n\n#[g|]#oto\nthird\n\nparagraph",
                "a\nb\n\n#[goto\nthird\n\n|]#paragraph",
            ),
            (
                "a\nb\n#[\n|]#goto\nthird\n\nparagraph",
                "a\nb\n\n#[goto\nthird\n\n|]#paragraph",
            ),
            (
                "a\nb#[\n|]#\n\ngoto\nsecond\n\nparagraph",
                "a\nb#[\n\n|]#goto\nsecond\n\nparagraph",
            ),
            (
                "here\n\nhave\n#[m|]#ultiple\nparagraph\n\n\n\n\n",
                "here\n\nhave\n#[multiple\nparagraph\n\n\n\n\n|]#",
            ),
            (
                "#[t|]#ext\n\n\nafter two blank lines\n\nmore text\n",
                "#[text\n\n\n|]#after two blank lines\n\nmore text\n",
            ),
            (
                "#[text\n\n\n|]#after two blank lines\n\nmore text\n",
                "text\n\n\n#[after two blank lines\n\n|]#more text\n",
            ),
        ];

        for (before, expected) in tests {
            let (s, selection) = crate::test::print(before);
            let text = Rope::from(s.as_str());
            let selection =
                selection.transform(|r| move_next_paragraph(text.slice(..), r, 1, Movement::Move));
            let actual = crate::test::plain(s.as_ref(), &selection);
            assert_eq!(actual, expected, "\nbefore: `{:?}`", before);
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_next_paragraph_double() {
        let tests = [
            (
                "one\n\ntwo\n\nth#[r|]#ee\n\n",
                "one\n\ntwo\n\nth#[ree\n\n|]#",
            ),
            (
                "on#[e|]#\n\ntwo\n\nthree\n\n",
                "on#[e\n\ntwo\n\n|]#three\n\n",
            ),
        ];

        for (before, expected) in tests {
            let (s, selection) = crate::test::print(before);
            let text = Rope::from(s.as_str());
            let selection =
                selection.transform(|r| move_next_paragraph(text.slice(..), r, 2, Movement::Move));
            let actual = crate::test::plain(s.as_ref(), &selection);
            assert_eq!(actual, expected, "\nbefore: `{:?}`", before);
        }
    }

    #[test]
    fn test_behaviour_when_moving_to_next_paragraph_extend() {
        let tests = [
            (
                "one\n\n#[two\n\n|]#three\n\n",
                "one\n\n#[two\n\nthree\n\n|]#",
            ),
            (
                "one\n\n#[two\n\nthree\n\n|]#",
                "one\n\n#[two\n\nthree\n\n|]#",
            ),
        ];

        for (before, expected) in tests {
            let (s, selection) = crate::test::print(before);
            let text = Rope::from(s.as_str());
            let selection = selection
                .transform(|r| move_next_paragraph(text.slice(..), r, 1, Movement::Extend));
            let actual = crate::test::plain(s.as_ref(), &selection);
            assert_eq!(actual, expected, "\nbefore: `{:?}`", before);
        }
    }

    #[test]
    fn test_next_sentence_boundary() {
        // "One. Two. Three." — starts at 0, 5, 10.
        let text = Rope::from("One. Two. Three.");
        let s = text.slice(..);
        assert_eq!(next_sentence_boundary(s, 0), 5);
        assert_eq!(next_sentence_boundary(s, 5), 10);
        // No further sentence start past the last ender -> len.
        assert_eq!(next_sentence_boundary(s, 10), s.len_chars());

        // Newlines count as the whitespace after an ender.
        let nl = Rope::from("Hi.\nBye.");
        assert_eq!(next_sentence_boundary(nl.slice(..), 0), 4);

        // Closing punctuation may follow the ender before the space.
        let close = Rope::from("(Yes.)  No.");
        // ender '.' at 4, closer ')' at 5, spaces 6-7, "No" at 8.
        assert_eq!(next_sentence_boundary(close.slice(..), 0), 8);

        // A blank line is its own boundary.
        let para = Rope::from("a\n\nb");
        assert_eq!(next_sentence_boundary(para.slice(..), 0), 2);
    }

    #[test]
    fn test_prev_sentence_boundary() {
        let text = Rope::from("One. Two. Three.");
        let s = text.slice(..);
        // From inside "Three" -> start of "Three" (10).
        assert_eq!(prev_sentence_boundary(s, 12), 10);
        // From exactly a sentence start -> the previous start.
        assert_eq!(prev_sentence_boundary(s, 10), 5);
        assert_eq!(prev_sentence_boundary(s, 5), 0);
        // Within the first sentence -> paragraph start (0).
        assert_eq!(prev_sentence_boundary(s, 2), 0);
    }

    #[test]
    fn test_goto_visual_line() {
        // Soft-wrap off: a visual line is a logical line, so this matches
        // logical line start/end.
        let text = Rope::from("hello\nworld");
        let s = text.slice(..);
        let tf = TextFormat::default();
        let mut ann = TextAnnotations::default();
        // From column 3 of line 0: start -> char 0; end -> char 5 (just past the
        // last visible grapheme "hello", i.e. before the newline).
        assert_eq!(
            goto_visual_line(s, Range::point(3), false, Movement::Move, &tf, &mut ann).cursor(s),
            0
        );
        assert_eq!(
            goto_visual_line(s, Range::point(3), true, Movement::Move, &tf, &mut ann).cursor(s),
            5
        );
        // Extend moves only the head to the row start; the anchor stays behind
        // (put_cursor widens it to 4 to keep the original grapheme selected).
        let ext = goto_visual_line(s, Range::point(3), false, Movement::Extend, &tf, &mut ann);
        assert_eq!((ext.anchor, ext.head), (4, 0));

        // Soft-wrap on (default viewport_width 17): "aaaa bbbb cccc " fills the
        // first visual row and "dddd" word-wraps to the second (char 15).
        let long = Rope::from("aaaa bbbb cccc dddd eeee");
        let ls = long.slice(..);
        let tf2 = TextFormat {
            soft_wrap: true,
            ..Default::default()
        };
        // A cursor inside "cccc" (char 12) snaps to the visual row's start (0) or
        // end (14, the space before the wrap) — NOT the logical line boundaries.
        assert_eq!(
            goto_visual_line(ls, Range::point(12), false, Movement::Move, &tf2, &mut ann)
                .cursor(ls),
            0
        );
        assert_eq!(
            goto_visual_line(ls, Range::point(12), true, Movement::Move, &tf2, &mut ann).cursor(ls),
            14
        );
        // A cursor on the second visual row (char 16, inside "dddd") snaps to
        // that row's start (char 15), proving the stop is per visual row.
        assert_eq!(
            goto_visual_line(ls, Range::point(16), false, Movement::Move, &tf2, &mut ann)
                .cursor(ls),
            15
        );
    }

    #[test]
    fn test_move_sentence_cursor() {
        let text = Rope::from("One. Two. Three.");
        let s = text.slice(..);
        let r = Range::point(0);
        let fwd = move_next_sentence(s, r, 1, Movement::Move);
        assert_eq!(fwd.cursor(s), 5);
        let back = move_prev_sentence(s, Range::point(10), 1, Movement::Move);
        assert_eq!(back.cursor(s), 5);
        // count repeats.
        let two = move_next_sentence(s, Range::point(0), 2, Movement::Move);
        assert_eq!(two.cursor(s), 10);
    }

    /// vim `paragraphs`: with the option unset a `.PP` line is ordinary text, and
    /// with `:set paragraphs=PP` it starts a paragraph — `}` stops on it and `{`
    /// comes back to it. Pure macro-name matching is checked separately.
    #[test]
    fn paragraphs_option_makes_nroff_macro_lines_paragraph_starts() {
        let text = Rope::from("first para line\n.PP\nsecond para line\nmore\n");
        let s = text.slice(..);

        set_paragraph_macros("");
        let default = move_next_paragraph(s, Range::point(0), 1, Movement::Move);
        assert!(
            s.char_to_line(default.head) > 3,
            "without `paragraphs`, `.PP` is plain text and `}}` runs to the end"
        );

        set_paragraph_macros("PP");
        let next = move_next_paragraph(s, Range::point(0), 1, Movement::Move);
        assert_eq!(s.char_to_line(next.head), 1, "`}}` stops on the `.PP` line");

        let from = Range::point(text.line_to_char(3));
        let prev = move_prev_paragraph(s, from, 1, Movement::Move);
        assert_eq!(
            s.char_to_line(prev.head),
            1,
            "`{{` returns to the `.PP` line"
        );

        set_paragraph_macros("");
    }

    /// vim pairs the `paragraphs` value two characters at a time; a one-letter
    /// macro is padded with a space (`.P` matches the pair `P `).
    #[test]
    fn nroff_macro_line_matches_option_pairs() {
        let spec = "IPLPPPQPP TPHPLIPpLpItpplpipbp";
        assert!(is_nroff_macro_line(".IP", spec));
        assert!(is_nroff_macro_line(".PP", spec));
        assert!(
            is_nroff_macro_line(".P", spec),
            "`P ` pair matches a bare .P"
        );
        assert!(is_nroff_macro_line(".TP", spec));
        assert!(!is_nroff_macro_line(".XY", spec));
        assert!(!is_nroff_macro_line("PP", spec), "no leading dot");
        assert!(
            !is_nroff_macro_line(".PP", ""),
            "unset option matches nothing"
        );
    }
}
