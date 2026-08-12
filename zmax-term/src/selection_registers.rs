//! Kakoune's selection registers: `Z` writes the current selections into a
//! register, `z` reads them back, and `<a-z>` / `<a-Z>` combine the two sets.
//!
//! Kakoune stores selections as *text* — one `<anchor_line>.<anchor_col>,<cursor_line>.<cursor_col>`
//! descriptor per selection, 1-based, columns counted in characters, both ends
//! inclusive (`:doc registers`, and the `%val{selections_desc}` expansion the
//! descriptors come from). Keeping that exact wire format is what makes the
//! register a normal register: `"aZ` then `:echo %reg{a}` shows the descriptors,
//! and a descriptor typed by hand restores a selection just as kakoune's does.
//!
//! The conversions here are pure — the commands in [`crate::commands`] own the
//! register and document plumbing.

use zmax_core::{Range, RopeSlice, Selection};

/// The register kakoune's `Z` / `z` use when none is given (`:doc registers`).
pub const DEFAULT: char = '^';

/// How two selection sets are combined (kakoune's `<a-z>` / `<a-Z>` menus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    /// `a` — the two sets side by side.
    Append,
    /// `u` — per pair, the range spanning both (their union).
    Union,
    /// `i` — per pair, the overlap; pairs that do not overlap are dropped.
    Intersection,
    /// `<` — per pair, the one whose cursor sits leftmost.
    Leftmost,
    /// `>` — per pair, the one whose cursor sits rightmost.
    Rightmost,
    /// `+` — per pair, the longer one.
    Longest,
    /// `-` — per pair, the shorter one.
    Shortest,
}

impl Combine {
    /// The menu key kakoune binds the operation to.
    pub fn from_key(key: char) -> Option<Self> {
        Some(match key {
            'a' => Self::Append,
            'u' => Self::Union,
            'i' => Self::Intersection,
            '<' => Self::Leftmost,
            '>' => Self::Rightmost,
            '+' => Self::Longest,
            '-' => Self::Shortest,
            _ => return None,
        })
    }
}

/// One selection as kakoune writes it: `line.col,line.col`, 1-based, inclusive
/// on both ends. A zmax `Range` is half-open at the head, so the descriptor's
/// cursor column is the char *before* an exclusive head.
pub fn encode(text: RopeSlice, selection: &Selection) -> Vec<String> {
    selection
        .iter()
        .map(|range| {
            let (al, ac) = line_col(text, anchor_char(text, range));
            let (hl, hc) = line_col(text, cursor_char(text, range));
            format!("{}.{},{}.{}", al + 1, ac + 1, hl + 1, hc + 1)
        })
        .collect()
}

/// Parse descriptors back into a selection, dropping any that do not name a
/// position inside `text`. `None` when nothing survives — kakoune leaves the
/// selection alone rather than emptying it.
pub fn decode(text: RopeSlice, descs: &[String]) -> Option<Selection> {
    let ranges: Vec<Range> = descs
        .iter()
        .flat_map(|d| d.split_ascii_whitespace())
        .filter_map(|desc| {
            let (anchor, head) = desc.split_once(',')?;
            let anchor = char_at(text, anchor)?;
            let head = char_at(text, head)?;
            // The descriptor's cursor char is inside the selection, so the
            // half-open head sits one char past it (or before it, when the
            // selection points backwards).
            Some(if head >= anchor {
                Range::new(anchor, next_char(text, head))
            } else {
                Range::new(next_char(text, anchor), head)
            })
        })
        .collect();
    (!ranges.is_empty()).then(|| Selection::new(ranges.into(), 0))
}

/// Combine `current` with `stored` the way kakoune's `<a-z>` / `<a-Z>` menus do.
///
/// Every operation but `Append` walks the two sets in lockstep, pairing the nth
/// selection of one with the nth of the other; the longer set keeps its extra
/// selections untouched, which is what kakoune does when the counts differ.
pub fn combine(
    text: RopeSlice,
    current: &Selection,
    stored: &Selection,
    op: Combine,
) -> Option<Selection> {
    if op == Combine::Append {
        let ranges: Vec<Range> = current.iter().chain(stored.iter()).copied().collect();
        return Some(Selection::new(ranges.into(), current.primary_index()));
    }

    let mut ranges: Vec<Range> = Vec::with_capacity(current.len().max(stored.len()));
    for i in 0..current.len().max(stored.len()) {
        match (current.ranges().get(i), stored.ranges().get(i)) {
            (Some(a), Some(b)) => {
                if let Some(r) = pair(text, *a, *b, op) {
                    ranges.push(r);
                }
            }
            (Some(r), None) | (None, Some(r)) => ranges.push(*r),
            (None, None) => break,
        }
    }
    (!ranges.is_empty()).then(|| Selection::new(ranges.into(), 0))
}

/// The single range two paired selections combine into, or `None` when the
/// operation has no answer for them (a disjoint pair under `Intersection`).
fn pair(text: RopeSlice, a: Range, b: Range, op: Combine) -> Option<Range> {
    match op {
        Combine::Append => unreachable!("handled by the caller"),
        Combine::Union => Some(Range::new(a.from().min(b.from()), a.to().max(b.to()))),
        Combine::Intersection => {
            let from = a.from().max(b.from());
            let to = a.to().min(b.to());
            (from < to).then(|| Range::new(from, to))
        }
        // Kakoune's `<` / `>` compare where the *cursors* sit, not where the
        // selections start.
        Combine::Leftmost => Some(if cursor_char(text, &a) <= cursor_char(text, &b) {
            a
        } else {
            b
        }),
        Combine::Rightmost => Some(if cursor_char(text, &a) <= cursor_char(text, &b) {
            b
        } else {
            a
        }),
        Combine::Longest => Some(if b.len() > a.len() { b } else { a }),
        Combine::Shortest => Some(if b.len() < a.len() { b } else { a }),
    }
}

/// The char the cursor sits *on* — a zmax head is exclusive, kakoune's is not.
fn cursor_char(text: RopeSlice, range: &Range) -> usize {
    if range.head > range.anchor {
        prev_char(text, range.head)
    } else {
        range.head.min(text.len_chars())
    }
}

/// The char the anchor sits *on*. Whichever end of the range is the larger index
/// is the exclusive one, so a backwards selection's anchor needs the same
/// one-char adjustment the cursor gets in a forward one.
fn anchor_char(text: RopeSlice, range: &Range) -> usize {
    if range.anchor > range.head {
        prev_char(text, range.anchor)
    } else {
        range.anchor.min(text.len_chars())
    }
}

fn line_col(text: RopeSlice, char_idx: usize) -> (usize, usize) {
    let char_idx = char_idx.min(text.len_chars());
    let line = text.char_to_line(char_idx);
    (line, char_idx - text.line_to_char(line))
}

fn char_at(text: RopeSlice, desc: &str) -> Option<usize> {
    let (line, col) = desc.split_once('.')?;
    let line = line.trim().parse::<usize>().ok()?.checked_sub(1)?;
    let col = col.trim().parse::<usize>().ok()?.checked_sub(1)?;
    if line >= text.len_lines() {
        return None;
    }
    let start = text.line_to_char(line);
    let line_len = text.line(line).len_chars();
    (col <= line_len).then_some(start + col)
}

fn next_char(text: RopeSlice, char_idx: usize) -> usize {
    (char_idx + 1).min(text.len_chars())
}

fn prev_char(_text: RopeSlice, char_idx: usize) -> usize {
    char_idx.saturating_sub(1)
}

#[cfg(test)]
mod test {
    use super::*;
    use zmax_core::Rope;

    fn rope() -> Rope {
        Rope::from("hello world\nsecond line\nthird\n")
    }

    #[test]
    fn descriptors_round_trip_through_a_register() {
        let rope = rope();
        let text = rope.slice(..);
        // "hello" on line 1, and "line" on line 2.
        let selection = Selection::new(vec![Range::new(0, 5), Range::new(19, 23)].into(), 1);

        // Kakoune writes 1-based, inclusive-on-both-ends descriptors.
        assert_eq!(encode(text, &selection), vec!["1.1,1.5", "2.8,2.11"]);

        let back = decode(text, &encode(text, &selection)).expect("descriptors are valid");
        assert_eq!(back.ranges(), selection.ranges());
    }

    #[test]
    fn a_backwards_selection_keeps_its_direction() {
        let rope = rope();
        let text = rope.slice(..);
        let backwards = Selection::single(5, 0);
        assert_eq!(encode(text, &backwards), vec!["1.5,1.1"]);
        let back = decode(text, &["1.5,1.1".to_string()]).unwrap();
        assert_eq!(back.primary(), backwards.primary());
    }

    #[test]
    fn descriptors_outside_the_buffer_are_dropped_not_clamped() {
        let rope = rope();
        let text = rope.slice(..);
        // Line 99 does not exist; "2.3,2.5" does. Only the real one survives.
        let sel = decode(text, &["99.1,99.4".into(), "2.3,2.5".into()]).unwrap();
        assert_eq!(sel.len(), 1);
        assert_eq!(sel.primary(), Range::new(14, 17));
        // Nothing valid at all leaves the caller to keep the current selection.
        assert!(decode(text, &["99.1,99.4".into()]).is_none());
        assert!(decode(text, &["not-a-descriptor".into()]).is_none());
    }

    #[test]
    fn combine_pairs_the_two_sets_in_lockstep() {
        let rope = rope();
        let text = rope.slice(..);
        let current = Selection::new(vec![Range::new(0, 5), Range::new(12, 18)].into(), 0);
        let stored = Selection::new(vec![Range::new(3, 9), Range::new(15, 23)].into(), 0);

        let union = combine(text, &current, &stored, Combine::Union).unwrap();
        assert_eq!(union.ranges(), &[Range::new(0, 9), Range::new(12, 23)]);

        let inter = combine(text, &current, &stored, Combine::Intersection).unwrap();
        assert_eq!(inter.ranges(), &[Range::new(3, 5), Range::new(15, 18)]);

        // Append hands both sets to `Selection::new`, which merges overlapping
        // ranges: zmax cannot hold the overlapping selections kakoune allows, so
        // these four collapse to two. (Mapped as `partial`, not `ported`.)
        let append = combine(text, &current, &stored, Combine::Append).unwrap();
        assert_eq!(append.ranges(), &[Range::new(0, 9), Range::new(12, 23)]);
        // Disjoint sets append with nothing to merge, which is the common case.
        let disjoint = Selection::new(vec![Range::new(24, 26)].into(), 0);
        let appended = combine(text, &current, &disjoint, Combine::Append).unwrap();
        assert_eq!(appended.len(), 3);

        // Longest / shortest compare the pair, not the whole set.
        let longest = combine(text, &current, &stored, Combine::Longest).unwrap();
        assert_eq!(longest.ranges(), &[Range::new(3, 9), Range::new(15, 23)]);
        let shortest = combine(text, &current, &stored, Combine::Shortest).unwrap();
        assert_eq!(shortest.ranges(), &[Range::new(0, 5), Range::new(12, 18)]);

        // Leftmost / rightmost go by cursor position, so a backwards selection
        // whose cursor sits early wins `<` even though it ends late.
        let backwards = Selection::single(9, 1);
        let forwards = Selection::single(4, 7);
        let left = combine(text, &forwards, &backwards, Combine::Leftmost).unwrap();
        assert_eq!(left.primary(), Range::new(9, 1));
        let right = combine(text, &forwards, &backwards, Combine::Rightmost).unwrap();
        assert_eq!(right.primary(), Range::new(4, 7));
    }

    #[test]
    fn an_unequal_pair_count_keeps_the_extra_selections() {
        let rope = rope();
        let text = rope.slice(..);
        let current = Selection::new(vec![Range::new(0, 5), Range::new(12, 18)].into(), 0);
        let stored = Selection::single(3, 9);
        let union = combine(text, &current, &stored, Combine::Union).unwrap();
        // Pair 1 unions; the unpaired second selection survives as-is.
        assert_eq!(union.ranges(), &[Range::new(0, 9), Range::new(12, 18)]);
        // Even a disjoint intersection cannot empty the set: the extra stays.
        let disjoint = Selection::single(30, 31);
        let inter = combine(text, &current, &disjoint, Combine::Intersection).unwrap();
        assert_eq!(inter.ranges(), &[Range::new(12, 18)]);
    }

    #[test]
    fn menu_keys_match_kakoune() {
        assert_eq!(Combine::from_key('a'), Some(Combine::Append));
        assert_eq!(Combine::from_key('u'), Some(Combine::Union));
        assert_eq!(Combine::from_key('i'), Some(Combine::Intersection));
        assert_eq!(Combine::from_key('<'), Some(Combine::Leftmost));
        assert_eq!(Combine::from_key('>'), Some(Combine::Rightmost));
        assert_eq!(Combine::from_key('+'), Some(Combine::Longest));
        assert_eq!(Combine::from_key('-'), Some(Combine::Shortest));
        assert_eq!(Combine::from_key('q'), None);
    }
}
