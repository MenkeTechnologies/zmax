use std::fmt::Display;

use ropey::RopeSlice;

use crate::chars::{categorize_char, char_is_whitespace, CharCategory};
use crate::graphemes::{next_grapheme_boundary, prev_grapheme_boundary};
use crate::line_ending::rope_is_line_ending;
use crate::movement::{para_line, Direction, ParaLine};
use crate::syntax;
use crate::Range;
use crate::{surround, Syntax};

fn find_word_boundary(slice: RopeSlice, mut pos: usize, direction: Direction, long: bool) -> usize {
    use CharCategory::{Eol, Whitespace};

    let iter = match direction {
        Direction::Forward => slice.chars_at(pos),
        Direction::Backward => {
            let mut iter = slice.chars_at(pos);
            iter.reverse();
            iter
        }
    };

    let mut prev_category = match direction {
        Direction::Forward if pos == 0 => Whitespace,
        Direction::Forward => categorize_char(slice.char(pos - 1)),
        Direction::Backward if pos == slice.len_chars() => Whitespace,
        Direction::Backward => categorize_char(slice.char(pos)),
    };

    for ch in iter {
        match categorize_char(ch) {
            Eol | Whitespace => return pos,
            category => {
                if !long && category != prev_category && pos != 0 && pos != slice.len_chars() {
                    return pos;
                } else {
                    match direction {
                        Direction::Forward => pos += 1,
                        Direction::Backward => pos = pos.saturating_sub(1),
                    }
                    prev_category = category;
                }
            }
        }
    }

    pos
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TextObject {
    Around,
    Inside,
    /// Used for moving between objects.
    Movement,
}

impl Display for TextObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Around => "around",
            Self::Inside => "inside",
            Self::Movement => "movement",
        })
    }
}

// count doesn't do anything yet
/// Chars of whitespace starting at `from`.
fn whitespace_run(slice: RopeSlice, from: usize) -> usize {
    slice
        .chars_at(from)
        .take_while(|c| char_is_whitespace(*c))
        .count()
}

pub fn textobject_word(
    slice: RopeSlice,
    range: Range,
    textobject: TextObject,
    count: usize,
    long: bool,
) -> Range {
    let pos = range.cursor(slice);

    let word_start = find_word_boundary(slice, pos, Direction::Backward, long);
    let word_end = match slice.get_char(pos).map(categorize_char) {
        None | Some(CharCategory::Whitespace | CharCategory::Eol) => pos,
        _ => find_word_boundary(slice, pos + 1, Direction::Forward, long),
    };

    // Special case.
    if word_start == word_end {
        return Range::new(word_start, word_end);
    }

    match textobject {
        TextObject::Inside => {
            // vim counts *chunks* for `iw`: a run of word characters and a run of
            // whitespace each count as one, alternating. So `2iw` is the word plus
            // the space after it and `3iw` reaches the next word (`:h iw`).
            let mut end = word_end;
            for _ in 1..count {
                end = match slice.get_char(end).map(categorize_char) {
                    None => break,
                    Some(CharCategory::Whitespace | CharCategory::Eol) => {
                        end + whitespace_run(slice, end)
                    }
                    _ => find_word_boundary(slice, end + 1, Direction::Forward, long),
                };
            }
            Range::new(word_start, end)
        }
        TextObject::Around => {
            let whitespace_count_right = whitespace_run(slice, word_end);

            let (start, mut end) = if whitespace_count_right > 0 {
                (word_start, word_end + whitespace_count_right)
            } else {
                let whitespace_count_left = {
                    let mut iter = slice.chars_at(word_start);
                    iter.reverse();
                    iter.take_while(|c| char_is_whitespace(*c)).count()
                };
                (word_start - whitespace_count_left, word_end)
            };

            // vim counts whole words for `aw`, each with the whitespace that
            // follows it: `2aw` takes "one two " (`:h aw`).
            for _ in 1..count {
                let word_end = match slice.get_char(end).map(categorize_char) {
                    None => break,
                    Some(CharCategory::Whitespace | CharCategory::Eol) => end,
                    _ => find_word_boundary(slice, end + 1, Direction::Forward, long),
                };
                end = word_end + whitespace_run(slice, word_end);
            }
            Range::new(start, end)
        }
        TextObject::Movement => unreachable!(),
    }
}

pub fn textobject_paragraph(
    slice: RopeSlice,
    range: Range,
    textobject: TextObject,
    count: usize,
) -> Range {
    let mut line = range.cursor_line(slice);
    let prev_line_empty = rope_is_line_ending(slice.line(line.saturating_sub(1)));
    let curr_line_empty = rope_is_line_ending(slice.line(line));
    let next_line_empty =
        line + 1 >= slice.len_lines() || rope_is_line_ending(slice.line(line + 1));
    let last_char =
        prev_grapheme_boundary(slice, slice.line_to_char(line + 1)) == range.cursor(slice);
    let prev_empty_to_line = prev_line_empty && !curr_line_empty;
    let curr_empty_to_line = curr_line_empty && !next_line_empty;

    // Walk backwards from `from` to the first line of the paragraph it sits in:
    // over the blank lines above it, then up through the body. vim `paragraphs`:
    // an nroff macro line (`.PP`, `.IP`, …) *starts* a paragraph, so the walk
    // stops on it — the line is the paragraph's first line, not a separator above
    // it. Without the option set no line is ever `Macro` and this is exactly the
    // blank/non-blank walk it replaces.
    let paragraph_start_above = |from: usize| -> usize {
        let mut at = from;
        let mut lines = slice.lines_at(at);
        lines.reverse();
        let mut lines = lines.map(para_line).peekable();
        while lines.next_if(|&kind| kind == ParaLine::Blank).is_some() {
            at -= 1;
        }
        while lines.next_if(|&kind| kind == ParaLine::Text).is_some() {
            at -= 1;
        }
        if lines.next_if(|&kind| kind == ParaLine::Macro).is_some() {
            at -= 1;
        }
        at
    };

    // skip character before paragraph boundary
    let mut line_back = line; // line but backwards
    if prev_empty_to_line || curr_empty_to_line {
        line_back += 1;
    }
    // do not include current paragraph on paragraph end (include next)
    if !(curr_empty_to_line && last_char) {
        line_back = paragraph_start_above(line_back);
    }

    // skip character after paragraph boundary
    if curr_empty_to_line && last_char {
        line += 1;
    }
    let mut lines = slice.lines_at(line).map(para_line).peekable();
    let mut count_done = 0; // count how many non-whitespace paragraphs done
    for _ in 0..count {
        let mut done = false;
        // A paragraph may open with a `paragraphs` macro line: that line is part
        // of this paragraph, and the *next* macro line is where it ends — so the
        // opener is consumed once here and the body walk below stops at the next
        // one instead of running through it.
        if lines.next_if(|&kind| kind == ParaLine::Macro).is_some() {
            line += 1;
            done = true;
        }
        while lines.next_if(|&kind| kind == ParaLine::Text).is_some() {
            line += 1;
            done = true;
        }
        while lines.next_if(|&kind| kind == ParaLine::Blank).is_some() {
            line += 1;
        }
        count_done += done as usize;
    }

    // search one paragraph backwards for last paragraph
    // makes `map` at the end of the paragraph with trailing newlines useful
    let last_paragraph = count_done != count && lines.peek().is_none();
    if last_paragraph {
        line_back = paragraph_start_above(line_back);
    }

    // handle last whitespaces part separately depending on textobject
    match textobject {
        TextObject::Around => {}
        TextObject::Inside => {
            // remove last whitespace paragraph
            let mut lines = slice.lines_at(line);
            lines.reverse();
            let mut lines = lines.map(rope_is_line_ending).peekable();
            while lines.next_if(|&e| e).is_some() {
                line -= 1;
            }
        }
        TextObject::Movement => unreachable!(),
    }

    let anchor = slice.line_to_char(line_back);
    let head = slice.line_to_char(line);
    Range::new(anchor, head)
}

/// Text object for a vim sentence (`is` / `as`). `Inside` selects the sentence
/// text; `Around` also includes the trailing whitespace up to the next
/// sentence, matching vim's `as`.
pub fn textobject_sentence(
    slice: RopeSlice,
    range: Range,
    textobject: TextObject,
    count: usize,
) -> Range {
    let len = slice.len_chars();
    if len == 0 {
        return range;
    }
    let pos = range.cursor(slice);

    // Start of the sentence containing the cursor: the greatest sentence
    // boundary at or before `pos`, bounded to this paragraph.
    let mut start = crate::movement::current_paragraph_start(slice, pos);
    let mut i = start;
    while i < len {
        let nb = crate::movement::next_sentence_boundary(slice, i);
        if nb > i && nb <= pos {
            start = nb;
            i = nb;
        } else {
            break;
        }
    }

    // vim counts chunks for `is` — a sentence and the whitespace after it each
    // count as one — so `2is` is the sentence plus its trailing space and only
    // `3is` reaches the next sentence. `as` counts whole sentences, each with the
    // whitespace that follows (`:h is`, `:h as`). So `as` crosses one sentence
    // boundary per count, `is` one per *two*, ending on the whitespace when the
    // count is even.
    let boundaries = match textobject {
        TextObject::Around => count,
        TextObject::Inside => count.max(1).div_ceil(2),
        TextObject::Movement => return range,
    };
    // `as` runs to the start of the next sentence (trailing whitespace included).
    let mut end_around = start;
    for _ in 0..boundaries.max(1) {
        let next = crate::movement::next_sentence_boundary(slice, end_around).min(len);
        if next <= end_around {
            break;
        }
        end_around = next;
    }
    let trim = matches!(textobject, TextObject::Inside) && count.max(1) % 2 == 1;
    let end = if trim {
        // `is` trims the trailing whitespace.
        let mut e = end_around;
        while e > start && matches!(slice.char(e - 1), ' ' | '\t' | '\n' | '\r') {
            e -= 1;
        }
        e
    } else {
        end_around
    };
    Range::new(start, end)
}

/// A line holding nothing but its line ending — kakoune's `buffer[l] == "\n"`.
fn line_is_empty(slice: RopeSlice, line: usize) -> bool {
    rope_is_line_ending(slice.line(line))
}

/// A line holding nothing but white space — kakoune's `is_only_whitespaces`.
fn line_is_blank(slice: RopeSlice, line: usize) -> bool {
    slice.line(line).chars().all(|c| c.is_whitespace())
}

/// The indent of `line` in columns, tabs advancing to the next `tab_width` stop.
/// Stops at the first character that is neither a space nor a tab.
fn line_indent_width(slice: RopeSlice, line: usize, tab_width: usize) -> usize {
    let mut indent = 0;
    for ch in slice.line(line).chars() {
        match ch {
            ' ' => indent += 1,
            '\t' => indent = (indent / tab_width + 1) * tab_width,
            _ => break,
        }
    }
    indent
}

/// kakoune's `i` object (`:doc keys`, "Objects types"): the current indentation
/// block — the run of lines around the cursor indented at least as far as it is.
///
/// Port of kakoune's `select_indent` (src/selectors.cc). Three details of that
/// function are easy to get wrong and are kept here:
///
/// * The reference indent is read from the nearest *non-empty* line, searching
///   up from the cursor and then down, so a cursor parked on a blank line gets
///   the block it sits in rather than indent 0.
/// * Empty lines do not end the block; they are stepped over.
/// * `Inside` does not drop the block's first line (it is not a header/body
///   object) — it only trims white-space-only lines off either end.
///
/// kakoune's `select_indent` ignores its count, and so does this.
pub fn textobject_indent(
    slice: RopeSlice,
    range: Range,
    textobject: TextObject,
    tab_width: usize,
) -> Range {
    let line_count = slice.len_lines();
    let cursor_line = range.cursor_line(slice);

    let indent = (0..=cursor_line)
        .rev()
        .chain(cursor_line + 1..line_count)
        .find(|&line| !line_is_empty(slice, line))
        .map_or(0, |line| line_indent_width(slice, line, tab_width));

    let part_of_block = |line: usize| {
        line_is_empty(slice, line) || line_indent_width(slice, line, tab_width) >= indent
    };

    let mut begin_line = cursor_line;
    while begin_line > 0 && part_of_block(begin_line - 1) {
        begin_line -= 1;
    }
    let mut end_line = cursor_line;
    while end_line + 1 < line_count && part_of_block(end_line + 1) {
        end_line += 1;
    }

    match textobject {
        TextObject::Around => {}
        TextObject::Inside => {
            while begin_line < end_line && line_is_blank(slice, begin_line) {
                begin_line += 1;
            }
            while begin_line < end_line && line_is_blank(slice, end_line) {
                end_line -= 1;
            }
        }
        TextObject::Movement => unreachable!(),
    }

    Range::new(
        slice.line_to_char(begin_line),
        slice.line_to_char((end_line + 1).min(line_count)),
    )
}

/// vis's `al` / `il` (vis.1 "TEXT OBJECTS"): the current line, `Inside` without
/// its leading and trailing white space.
///
/// Port of vis's `text_object_line` / `text_object_line_inner`
/// (text-objects.c:141-151): `al` runs from the start of the line to the start
/// of the next one, so it takes the line ending with it, and `il` then trims
/// with `text_range_inner` (text-objects.c:376-387) — which uses `isspace`, so
/// the trailing newline goes too.
///
/// A line that is nothing but white space leaves `il` empty. vis disposes the
/// selection there; zmax cannot hold an empty one, so the caller gets a
/// zero-width range at the trimmed position and the usual min-width-1 rule
/// applies.
pub fn textobject_line(slice: RopeSlice, range: Range, textobject: TextObject) -> Range {
    let line_count = slice.len_lines();
    let line = range.cursor_line(slice);
    let from = slice.line_to_char(line);
    let to = slice.line_to_char((line + 1).min(line_count));

    match textobject {
        TextObject::Around => Range::new(from, to),
        TextObject::Inside => {
            let mut start = from;
            let mut end = to;
            while end > start && slice.char(end - 1).is_whitespace() {
                end -= 1;
            }
            while start < end && slice.char(start).is_whitespace() {
                start += 1;
            }
            Range::new(start, end)
        }
        TextObject::Movement => unreachable!(),
    }
}

pub fn textobject_pair_surround(
    syntax: Option<&Syntax>,
    slice: RopeSlice,
    range: Range,
    textobject: TextObject,
    ch: char,
    count: usize,
) -> Range {
    textobject_pair_surround_impl(syntax, slice, range, textobject, Some(ch), count)
}

pub fn textobject_pair_surround_closest(
    syntax: Option<&Syntax>,
    slice: RopeSlice,
    range: Range,
    textobject: TextObject,
    count: usize,
) -> Range {
    textobject_pair_surround_impl(syntax, slice, range, textobject, None, count)
}

fn textobject_pair_surround_impl(
    syntax: Option<&Syntax>,
    slice: RopeSlice,
    range: Range,
    textobject: TextObject,
    ch: Option<char>,
    count: usize,
) -> Range {
    let pair_pos = match ch {
        Some(ch) => surround::find_nth_pairs_pos(syntax, slice, ch, range, count),
        None => surround::find_nth_closest_pairs_pos(syntax, slice, range, count),
    };
    pair_pos
        .map(|(anchor, head)| match textobject {
            TextObject::Inside => {
                if anchor < head {
                    Range::new(next_grapheme_boundary(slice, anchor), head)
                } else {
                    Range::new(anchor, next_grapheme_boundary(slice, head))
                }
            }
            TextObject::Around => {
                if anchor < head {
                    Range::new(anchor, next_grapheme_boundary(slice, head))
                } else {
                    Range::new(next_grapheme_boundary(slice, anchor), head)
                }
            }
            TextObject::Movement => unreachable!(),
        })
        .unwrap_or(range)
}

/// Transform the given range to select text objects based on tree-sitter.
/// `object_name` is a query capture base name like "function", "class", etc.
/// `slice_tree` is the tree-sitter node corresponding to given text slice.
pub fn textobject_treesitter(
    slice: RopeSlice,
    range: Range,
    textobject: TextObject,
    object_name: &str,
    syntax: &Syntax,
    loader: &syntax::Loader,
    count: usize,
) -> Range {
    textobject_treesitter_opt(slice, range, textobject, object_name, syntax, loader, count)
        .unwrap_or(range)
}

/// Like [`textobject_treesitter`], but says so when there is no such object at the
/// cursor instead of falling back to `range`.
///
/// A caller that has to abort — vim's operators do nothing when the object is
/// absent — cannot use the falling-back form: the range it hands back is the
/// cursor's own, so `dif` outside any function deletes the character under the
/// cursor. Nor can such a caller infer failure by checking whether the range
/// changed, since an object may legitimately be exactly the cursor's range.
pub fn textobject_treesitter_opt(
    slice: RopeSlice,
    range: Range,
    textobject: TextObject,
    object_name: &str,
    syntax: &Syntax,
    loader: &syntax::Loader,
    _count: usize,
) -> Option<Range> {
    let byte_pos = slice.char_to_byte(range.cursor(slice));
    let layer = syntax.layer_for_byte_range(byte_pos as u32, byte_pos as u32);
    let root = syntax
        .tree_for_byte_range(byte_pos as u32, byte_pos as u32)
        .root_node();
    let textobject_query = loader.textobject_query(syntax.layer(layer).language);
    let get_range = move || -> Option<Range> {
        let capture_name = format!("{}.{}", object_name, textobject); // eg. function.inner
        let node = textobject_query?
            .capture_nodes(&capture_name, &root, slice)?
            .filter(|node| node.byte_range().contains(&byte_pos))
            .min_by_key(|node| node.byte_range().len())?;

        let len = slice.len_bytes();
        let start_byte = node.start_byte();
        let end_byte = node.end_byte();
        if start_byte >= len || end_byte >= len {
            return None;
        }

        let start_char = slice.byte_to_char(start_byte);
        let end_char = slice.byte_to_char(end_byte);

        Some(Range::new(start_char, end_char))
    };
    get_range()
}

#[cfg(test)]
mod test {
    use super::TextObject::*;
    use super::*;

    use crate::Range;
    use ropey::Rope;

    #[test]
    fn test_textobject_word() {
        // (text, [(char position, textobject, final range), ...])
        let tests = &[
            (
                "cursor at beginning of doc",
                vec![(0, Inside, (0, 6)), (0, Around, (0, 7))],
            ),
            (
                "cursor at middle of word",
                vec![
                    (13, Inside, (10, 16)),
                    (10, Inside, (10, 16)),
                    (15, Inside, (10, 16)),
                    (13, Around, (10, 17)),
                    (10, Around, (10, 17)),
                    (15, Around, (10, 17)),
                ],
            ),
            (
                "cursor between word whitespace",
                vec![(6, Inside, (6, 6)), (6, Around, (6, 6))],
            ),
            (
                "cursor on word before newline\n",
                vec![
                    (22, Inside, (22, 29)),
                    (28, Inside, (22, 29)),
                    (25, Inside, (22, 29)),
                    (22, Around, (21, 29)),
                    (28, Around, (21, 29)),
                    (25, Around, (21, 29)),
                ],
            ),
            (
                "cursor on newline\nnext line",
                vec![(17, Inside, (17, 17)), (17, Around, (17, 17))],
            ),
            (
                "cursor on word after newline\nnext line",
                vec![
                    (29, Inside, (29, 33)),
                    (30, Inside, (29, 33)),
                    (32, Inside, (29, 33)),
                    (29, Around, (29, 34)),
                    (30, Around, (29, 34)),
                    (32, Around, (29, 34)),
                ],
            ),
            (
                "cursor on #$%:;* punctuation",
                vec![
                    (13, Inside, (10, 16)),
                    (10, Inside, (10, 16)),
                    (15, Inside, (10, 16)),
                    (13, Around, (10, 17)),
                    (10, Around, (10, 17)),
                    (15, Around, (10, 17)),
                ],
            ),
            (
                "cursor on punc%^#$:;.tuation",
                vec![
                    (14, Inside, (14, 21)),
                    (20, Inside, (14, 21)),
                    (17, Inside, (14, 21)),
                    (14, Around, (14, 21)),
                    (20, Around, (14, 21)),
                    (17, Around, (14, 21)),
                ],
            ),
            (
                "cursor in   extra whitespace",
                vec![
                    (9, Inside, (9, 9)),
                    (10, Inside, (10, 10)),
                    (11, Inside, (11, 11)),
                    (9, Around, (9, 9)),
                    (10, Around, (10, 10)),
                    (11, Around, (11, 11)),
                ],
            ),
            (
                "cursor on word   with extra whitespace",
                vec![(11, Inside, (10, 14)), (11, Around, (10, 17))],
            ),
            (
                "cursor at end with extra   whitespace",
                vec![(28, Inside, (27, 37)), (28, Around, (24, 37))],
            ),
            (
                "cursor at end of doc",
                vec![(19, Inside, (17, 20)), (19, Around, (16, 20))],
            ),
        ];

        for (sample, scenario) in tests {
            let doc = Rope::from(*sample);
            let slice = doc.slice(..);
            for &case in scenario {
                let (pos, objtype, expected_range) = case;
                // cursor is a single width selection
                let range = Range::new(pos, pos + 1);
                let result = textobject_word(slice, range, objtype, 1, false);
                assert_eq!(
                    result,
                    expected_range.into(),
                    "\nCase failed: {:?} - {:?}",
                    sample,
                    case
                );
            }
        }
    }

    #[test]
    fn test_textobject_paragraph_inside_single() {
        let tests = [
            ("#[|]#", "#[|]#"),
            ("firs#[t|]#\n\nparagraph\n\n", "#[first\n|]#\nparagraph\n\n"),
            (
                "second\n\npa#[r|]#agraph\n\n",
                "second\n\n#[paragraph\n|]#\n",
            ),
            ("#[f|]#irst char\n\n", "#[first char\n|]#\n"),
            ("last char\n#[\n|]#", "#[last char\n|]#\n"),
            (
                "empty to line\n#[\n|]#paragraph boundary\n\n",
                "empty to line\n\n#[paragraph boundary\n|]#\n",
            ),
            (
                "line to empty\n\n#[p|]#aragraph boundary\n\n",
                "line to empty\n\n#[paragraph boundary\n|]#\n",
            ),
        ];

        for (before, expected) in tests {
            let (s, selection) = crate::test::print(before);
            let text = Rope::from(s.as_str());
            let selection = selection
                .transform(|r| textobject_paragraph(text.slice(..), r, TextObject::Inside, 1));
            let actual = crate::test::plain(s.as_ref(), &selection);
            assert_eq!(actual, expected, "\nbefore: `{:?}`", before);
        }
    }

    #[test]
    fn test_textobject_sentence() {
        // Inside: select just the sentence text (no trailing space).
        let text = Rope::from("One. Two. Three.");
        let s = text.slice(..);
        // cursor in "Two" (index 6)
        let inside = textobject_sentence(s, Range::point(6), TextObject::Inside, 1);
        assert_eq!(s.slice(inside.from()..inside.to()), "Two.");
        // Around: include the trailing whitespace up to the next sentence.
        let around = textobject_sentence(s, Range::point(6), TextObject::Around, 1);
        assert_eq!(s.slice(around.from()..around.to()), "Two. ");
        // First sentence, inside.
        let first = textobject_sentence(s, Range::point(1), TextObject::Inside, 1);
        assert_eq!(s.slice(first.from()..first.to()), "One.");
    }

    #[test]
    fn test_textobject_paragraph_inside_double() {
        let tests = [
            (
                "last two\n\n#[p|]#aragraph\n\nwithout whitespaces\n\n",
                "last two\n\n#[paragraph\n\nwithout whitespaces\n|]#\n",
            ),
            (
                "last two\n#[\n|]#paragraph\n\nwithout whitespaces\n\n",
                "last two\n\n#[paragraph\n\nwithout whitespaces\n|]#\n",
            ),
        ];

        for (before, expected) in tests {
            let (s, selection) = crate::test::print(before);
            let text = Rope::from(s.as_str());
            let selection = selection
                .transform(|r| textobject_paragraph(text.slice(..), r, TextObject::Inside, 2));
            let actual = crate::test::plain(s.as_ref(), &selection);
            assert_eq!(actual, expected, "\nbefore: `{:?}`", before);
        }
    }

    #[test]
    fn test_textobject_paragraph_around_single() {
        let tests = [
            ("#[|]#", "#[|]#"),
            ("firs#[t|]#\n\nparagraph\n\n", "#[first\n\n|]#paragraph\n\n"),
            (
                "second\n\npa#[r|]#agraph\n\n",
                "second\n\n#[paragraph\n\n|]#",
            ),
            ("#[f|]#irst char\n\n", "#[first char\n\n|]#"),
            ("last char\n#[\n|]#", "#[last char\n\n|]#"),
            (
                "empty to line\n#[\n|]#paragraph boundary\n\n",
                "empty to line\n\n#[paragraph boundary\n\n|]#",
            ),
            (
                "line to empty\n\n#[p|]#aragraph boundary\n\n",
                "line to empty\n\n#[paragraph boundary\n\n|]#",
            ),
        ];

        for (before, expected) in tests {
            let (s, selection) = crate::test::print(before);
            let text = Rope::from(s.as_str());
            let selection = selection
                .transform(|r| textobject_paragraph(text.slice(..), r, TextObject::Around, 1));
            let actual = crate::test::plain(s.as_ref(), &selection);
            assert_eq!(actual, expected, "\nbefore: `{:?}`", before);
        }
    }

    #[test]
    fn test_textobject_surround() {
        // (text, [(cursor position, textobject, final range, surround char, count), ...])
        let tests = &[
            (
                "simple (single) surround pairs",
                vec![
                    (3, Inside, (3, 3), '(', 1),
                    (7, Inside, (8, 14), ')', 1),
                    (10, Inside, (8, 14), '(', 1),
                    (14, Inside, (8, 14), ')', 1),
                    (3, Around, (3, 3), '(', 1),
                    (7, Around, (7, 15), ')', 1),
                    (10, Around, (7, 15), '(', 1),
                    (14, Around, (7, 15), ')', 1),
                ],
            ),
            (
                "samexx 'single' surround pairs",
                vec![
                    // Cursor before the quotes: Vim's `i'`/`a'` jump forward to
                    // the next quoted string on the line (was point-unchanged in
                    // upstream Helix; changed by the vim dot-repeat/quote work).
                    (3, Inside, (8, 14), '\'', 1),
                    (7, Inside, (7, 7), '\'', 1),
                    (10, Inside, (8, 14), '\'', 1),
                    (14, Inside, (14, 14), '\'', 1),
                    (3, Around, (7, 15), '\'', 1),
                    (7, Around, (7, 7), '\'', 1),
                    (10, Around, (7, 15), '\'', 1),
                    (14, Around, (14, 14), '\'', 1),
                ],
            ),
            (
                "(nested (surround (pairs)) 3 levels)",
                vec![
                    (0, Inside, (1, 35), '(', 1),
                    (6, Inside, (1, 35), ')', 1),
                    (8, Inside, (9, 25), '(', 1),
                    (8, Inside, (9, 35), ')', 2),
                    (20, Inside, (9, 25), '(', 2),
                    (20, Inside, (1, 35), ')', 3),
                    (0, Around, (0, 36), '(', 1),
                    (6, Around, (0, 36), ')', 1),
                    (8, Around, (8, 26), '(', 1),
                    (8, Around, (8, 36), ')', 2),
                    (20, Around, (8, 26), '(', 2),
                    (20, Around, (0, 36), ')', 3),
                ],
            ),
            (
                "(mixed {surround [pair] same} line)",
                vec![
                    (2, Inside, (1, 34), '(', 1),
                    (9, Inside, (8, 28), '{', 1),
                    (18, Inside, (18, 22), '[', 1),
                    (2, Around, (0, 35), '(', 1),
                    (9, Around, (7, 29), '{', 1),
                    (18, Around, (17, 23), '[', 1),
                ],
            ),
            (
                "(stepped (surround) pairs (should) skip)",
                vec![(22, Inside, (1, 39), '(', 1), (22, Around, (0, 40), '(', 1)],
            ),
            (
                "[surround pairs{\non different]\nlines}",
                vec![
                    (7, Inside, (1, 29), '[', 1),
                    (15, Inside, (16, 36), '{', 1),
                    (7, Around, (0, 30), '[', 1),
                    (15, Around, (15, 37), '{', 1),
                ],
            ),
        ];

        for (sample, scenario) in tests {
            let doc = Rope::from(*sample);
            let slice = doc.slice(..);
            for &case in scenario {
                let (pos, objtype, expected_range, ch, count) = case;
                let result =
                    textobject_pair_surround(None, slice, Range::point(pos), objtype, ch, count);
                assert_eq!(
                    result,
                    expected_range.into(),
                    "\nCase failed: {:?} - {:?}",
                    sample,
                    case
                );
            }
        }
    }

    #[test]
    fn test_textobject_indent() {
        // A python-shaped block: the `def` header is indent 0, its body 4.
        let doc = Rope::from("def f():\n    a = 1\n\n    b = 2\nc = 3\n");
        let slice = doc.slice(..);
        let body = slice.line_to_char(1);

        // From inside the body the block is lines 1..=3 — the blank line 2 does
        // not end it, and line 4 (indent 0) does.
        assert_eq!(
            textobject_indent(slice, Range::point(body), Around, 4),
            Range::new(slice.line_to_char(1), slice.line_to_char(4))
        );
        // kakoune's `Inner` trims blank lines off the ends; it does not drop the
        // block's first line.
        assert_eq!(
            textobject_indent(slice, Range::point(body), Inside, 4),
            Range::new(slice.line_to_char(1), slice.line_to_char(4))
        );

        // From the blank line the indent is read from the nearest non-empty line
        // above, so the same block comes back rather than the whole buffer.
        let blank = slice.line_to_char(2);
        assert_eq!(
            textobject_indent(slice, Range::point(blank), Around, 4),
            Range::new(slice.line_to_char(1), slice.line_to_char(4))
        );

        // From the indent-0 header every line qualifies, so the block is the
        // whole buffer.
        assert_eq!(
            textobject_indent(slice, Range::point(0), Around, 4),
            Range::new(0, slice.len_chars())
        );
    }

    #[test]
    fn test_textobject_indent_trailing_blank_lines() {
        // The trailing blank line is part of the `Around` block (kakoune steps
        // over empties) but is trimmed by `Inside`.
        let doc = Rope::from("a\n    x\n    \ny\n");
        let slice = doc.slice(..);
        let x = slice.line_to_char(1);
        assert_eq!(
            textobject_indent(slice, Range::point(x), Around, 4),
            Range::new(slice.line_to_char(1), slice.line_to_char(3))
        );
        assert_eq!(
            textobject_indent(slice, Range::point(x), Inside, 4),
            Range::new(slice.line_to_char(1), slice.line_to_char(2))
        );
    }

    #[test]
    fn test_textobject_line() {
        let doc = Rope::from("first\n   padded   \nlast");
        let slice = doc.slice(..);

        // `al` takes the line ending with it.
        assert_eq!(
            textobject_line(slice, Range::point(0), Around),
            Range::new(0, 6)
        );
        // `il` drops the leading spaces, the trailing spaces and the newline.
        let padded = slice.line_to_char(1);
        assert_eq!(
            textobject_line(slice, Range::point(padded), Inside),
            Range::new(padded + 3, padded + 9)
        );
        // The last line has no line ending to take.
        let last = slice.line_to_char(2);
        assert_eq!(
            textobject_line(slice, Range::point(last), Around),
            Range::new(last, slice.len_chars())
        );

        // A white-space-only line leaves `il` empty.
        let doc = Rope::from("  \n");
        let slice = doc.slice(..);
        assert_eq!(
            textobject_line(slice, Range::point(0), Inside),
            Range::new(0, 0)
        );
    }
}
