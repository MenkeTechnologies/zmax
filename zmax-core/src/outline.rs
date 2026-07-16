//! Outline — the zmax port of GNU Emacs `outline-mode` heading structure.
//!
//! Outline mode treats a buffer as a tree of headings (lines matching the
//! `outline-regexp`, whose length gives the level) with body text under each.
//! This module is the pure, dependency-free, tested core: it scans text into a
//! list of [`Heading`]s and answers the structural questions the outline
//! commands need — next/previous heading, the parent (up), the next/previous
//! heading at the same level (without leaving the parent), and the extent of a
//! heading's subtree (for folding). No I/O; the command layer maps the returned
//! character offsets / line numbers onto the document and its fold state.
//!
//! Heading syntax: a run of `*` (Org / classic outline) or `#` (Markdown) at the
//! start of a line, followed by whitespace; the run length is the 1-based level.

/// One heading in the outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heading {
    /// 0-based line number of the heading.
    pub line: usize,
    /// 1-based nesting level (number of leading `*`/`#`).
    pub level: u32,
    /// Character offset of the start of the heading line.
    pub char_pos: usize,
}

/// The level of a heading line, or `None` if the line is not a heading. A
/// heading is a run of a single marker char (`*` or `#`) followed by whitespace
/// or end of line.
fn heading_level(line: &str) -> Option<u32> {
    let marker = match line.chars().next()? {
        '*' => '*',
        '#' => '#',
        _ => return None,
    };
    let run = line.chars().take_while(|&c| c == marker).count();
    let after = line.chars().nth(run);
    match after {
        None => Some(run as u32),
        Some(c) if c.is_whitespace() => Some(run as u32),
        _ => None,
    }
}

/// Scan `text` into its headings, in document order.
pub fn headings(text: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut char_pos = 0;
    for (line_no, line) in text.split('\n').enumerate() {
        if let Some(level) = heading_level(line) {
            out.push(Heading {
                line: line_no,
                level,
                char_pos,
            });
        }
        char_pos += line.chars().count() + 1; // +1 for the '\n'
    }
    out
}

/// The index of the heading whose subtree contains `line` (the nearest heading
/// at or before `line`), if any.
fn current_index(hs: &[Heading], line: usize) -> Option<usize> {
    hs.iter().rposition(|h| h.line <= line)
}

/// `outline-next-visible-heading`: the first heading strictly after `line`.
pub fn next_heading(hs: &[Heading], line: usize) -> Option<Heading> {
    hs.iter().find(|h| h.line > line).copied()
}

/// `outline-previous-visible-heading`: the last heading strictly before `line`.
pub fn prev_heading(hs: &[Heading], line: usize) -> Option<Heading> {
    hs.iter().rev().find(|h| h.line < line).copied()
}

/// `outline-up-heading`: the nearest preceding heading with a smaller level
/// (the parent) relative to the heading containing `line`.
pub fn up_heading(hs: &[Heading], line: usize) -> Option<Heading> {
    let i = current_index(hs, line)?;
    let level = hs[i].level;
    hs[..i].iter().rev().find(|h| h.level < level).copied()
}

/// `outline-forward-same-level`: the next heading at the same level as the one
/// containing `line`, without crossing a heading of a smaller level (i.e. not
/// leaving the parent subtree).
pub fn forward_same_level(hs: &[Heading], line: usize) -> Option<Heading> {
    let i = current_index(hs, line)?;
    let level = hs[i].level;
    for h in &hs[i + 1..] {
        if h.level < level {
            return None; // left the parent
        }
        if h.level == level {
            return Some(*h);
        }
    }
    None
}

/// `outline-backward-same-level`: the previous heading at the same level,
/// without crossing up out of the parent subtree.
pub fn backward_same_level(hs: &[Heading], line: usize) -> Option<Heading> {
    let i = current_index(hs, line)?;
    let level = hs[i].level;
    for h in hs[..i].iter().rev() {
        if h.level < level {
            return None;
        }
        if h.level == level {
            return Some(*h);
        }
    }
    None
}

/// The last line of the subtree of the heading containing `line`: everything up
/// to (but not including) the next heading at the same or smaller level.
/// `total_lines` bounds the buffer. Returns `(heading_line, subtree_last_line)`.
pub fn subtree_bounds(hs: &[Heading], line: usize, total_lines: usize) -> Option<(usize, usize)> {
    let i = current_index(hs, line)?;
    let level = hs[i].level;
    let start = hs[i].line;
    let end = hs[i + 1..]
        .iter()
        .find(|h| h.level <= level)
        .map(|h| h.line.saturating_sub(1))
        .unwrap_or(total_lines.saturating_sub(1));
    Some((start, end))
}

/// The body-line range to fold for `outline-hide-subtree` (the lines strictly
/// after the heading, through the end of its subtree). `None` if the subtree has
/// no body.
pub fn subtree_body(hs: &[Heading], line: usize, total_lines: usize) -> Option<(usize, usize)> {
    let (start, end) = subtree_bounds(hs, line, total_lines)?;
    if end > start {
        Some((start + 1, end))
    } else {
        None
    }
}

/// The body-line range of just the current heading's *entry* (the text after
/// the heading, up to its first subheading) for `outline-hide-entry`.
pub fn entry_body(hs: &[Heading], line: usize, total_lines: usize) -> Option<(usize, usize)> {
    let i = current_index(hs, line)?;
    let h = hs[i];
    let end = hs
        .get(i + 1)
        .map(|n| n.line.saturating_sub(1))
        .unwrap_or(total_lines.saturating_sub(1));
    if end > h.line {
        Some((h.line + 1, end))
    } else {
        None
    }
}

/// The body ranges to fold for `outline-hide-body` (hide every heading's body,
/// leaving only heading lines visible). Returns one `(first, last)` line range
/// per heading that has a body.
pub fn all_bodies(hs: &[Heading], total_lines: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (idx, h) in hs.iter().enumerate() {
        // Body runs from the line after the heading up to the line before the
        // next heading (of any level).
        let end = hs
            .get(idx + 1)
            .map(|n| n.line.saturating_sub(1))
            .unwrap_or(total_lines.saturating_sub(1));
        if end > h.line {
            out.push((h.line + 1, end));
        }
    }
    out
}

/// Fold ranges for `outline-hide-sublevels` (show only the top `levels` levels
/// of headings, hiding all bodies and every deeper heading). Each range spans
/// from just after a shallow heading (level <= `levels`) to just before the next
/// shallow heading — so only shallow heading lines stay visible.
pub fn sublevel_folds(hs: &[Heading], levels: u32, total_lines: usize) -> Vec<(usize, usize)> {
    let shallow: Vec<&Heading> = hs.iter().filter(|h| h.level <= levels).collect();
    let mut out = Vec::new();
    for (idx, h) in shallow.iter().enumerate() {
        let end = shallow
            .get(idx + 1)
            .map(|n| n.line.saturating_sub(1))
            .unwrap_or(total_lines.saturating_sub(1));
        if end > h.line {
            out.push((h.line + 1, end));
        }
    }
    out
}

/// Body ranges to fold for `outline-hide-leaves` (in the subtree at `line`, hide
/// every heading's body text while keeping all subheadings visible). One
/// `(first, last)` range per heading in the subtree that has a body.
pub fn subtree_leaf_bodies(hs: &[Heading], line: usize, total_lines: usize) -> Vec<(usize, usize)> {
    let Some((start, end)) = subtree_bounds(hs, line, total_lines) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (idx, h) in hs.iter().enumerate() {
        if h.line < start || h.line > end {
            continue;
        }
        // Body ends before the next heading of any level, clamped to the subtree.
        let body_end = hs
            .get(idx + 1)
            .map(|n| n.line.saturating_sub(1))
            .unwrap_or(total_lines.saturating_sub(1))
            .min(end);
        if body_end > h.line {
            out.push((h.line + 1, body_end));
        }
    }
    out
}

/// Fold ranges for `outline-show-children` (reveal only the subheadings up to
/// `extra_levels` deeper than the heading at `line`, hiding their bodies and any
/// still-deeper headings). `extra_levels` = 1 shows just the immediate children.
pub fn subtree_child_folds(
    hs: &[Heading],
    line: usize,
    extra_levels: u32,
    total_lines: usize,
) -> Vec<(usize, usize)> {
    let Some(i) = current_index(hs, line) else {
        return Vec::new();
    };
    let Some((start, end)) = subtree_bounds(hs, line, total_lines) else {
        return Vec::new();
    };
    let max_level = hs[i].level + extra_levels;
    // Headings inside the subtree shallow enough to stay visible.
    let shown: Vec<&Heading> = hs
        .iter()
        .filter(|h| h.line >= start && h.line <= end && h.level <= max_level)
        .collect();
    let mut out = Vec::new();
    for (idx, h) in shown.iter().enumerate() {
        let stop = shown
            .get(idx + 1)
            .map(|n| n.line.saturating_sub(1))
            .unwrap_or(end)
            .min(end);
        if stop > h.line {
            out.push((h.line + 1, stop));
        }
    }
    out
}

/// Subtree body ranges to fold for every heading whose index satisfies `pred`
/// (`outline-hide-by-heading-regexp`, where `pred(i)` is true when heading `i`'s
/// line matches the user's regexp). Headings with no body are skipped.
pub fn matching_subtree_bodies(
    hs: &[Heading],
    total_lines: usize,
    pred: impl Fn(usize) -> bool,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (i, h) in hs.iter().enumerate() {
        if pred(i) {
            if let Some(r) = subtree_body(hs, h.line, total_lines) {
                out.push(r);
            }
        }
    }
    out
}

/// The next step of `outline-cycle` (org-style TAB) for the heading at point,
/// chosen from how much of its subtree body is currently hidden: a fully hidden
/// subtree reveals its immediate children, a partially hidden one (children
/// shown) reveals everything, and a fully shown one folds back to hidden.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CycleStep {
    /// FOLDED -> CHILDREN: reveal the immediate subheadings only.
    ShowChildren,
    /// CHILDREN -> SUBTREE: reveal the whole subtree.
    ShowAll,
    /// SUBTREE -> FOLDED: hide the whole subtree body.
    Fold,
}

/// Decide the next `outline-cycle` step from `body_len` (lines in the subtree
/// body) and `hidden` (how many of them are currently hidden).
pub fn outline_cycle_next(body_len: usize, hidden: usize) -> CycleStep {
    if body_len == 0 || hidden == 0 {
        // Nothing hidden (fully shown) -> fold it.
        CycleStep::Fold
    } else if hidden >= body_len {
        // Fully folded -> show children.
        CycleStep::ShowChildren
    } else {
        // Partially shown (children) -> show everything.
        CycleStep::ShowAll
    }
}

/// The next step of `outline-cycle-buffer` (org global TAB): the whole buffer
/// cycles SHOW-ALL -> OVERVIEW (top headings only) -> CONTENTS (all headings, no
/// bodies) -> SHOW-ALL.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BufferCycleStep {
    /// Show only the top-level headings.
    Overview,
    /// Show every heading but no body text.
    Contents,
    /// Reveal everything.
    ShowAll,
}

/// Decide the next `outline-cycle-buffer` step from whether anything is hidden
/// and whether any heading line itself is hidden (which only happens in the
/// overview state, where subheadings are folded away).
pub fn outline_cycle_buffer_next(any_hidden: bool, any_heading_hidden: bool) -> BufferCycleStep {
    if !any_hidden {
        BufferCycleStep::Overview
    } else if any_heading_hidden {
        BufferCycleStep::Contents
    } else {
        BufferCycleStep::ShowAll
    }
}

/// Fold ranges for `outline-hide-other`: hide everything except the entry at
/// `line` (its heading and its body), the headings on its ancestor chain, and
/// every top-level heading. Emacs leaves those visible so you keep your bearings
/// in the document while the rest collapses.
///
/// Returns the maximal runs of hidden lines, as `(first, last)` inclusive pairs.
pub fn hide_other_folds(hs: &[Heading], line: usize, total_lines: usize) -> Vec<(usize, usize)> {
    if hs.is_empty() || total_lines == 0 {
        return Vec::new();
    }
    let mut visible = vec![false; total_lines];
    // Top-level headings always stay.
    for h in hs.iter().filter(|h| h.level == 1) {
        if h.line < total_lines {
            visible[h.line] = true;
        }
    }
    // The current heading, its ancestors, and the current entry's body.
    if let Some(cur) = current_index(hs, line) {
        let mut at = Some(hs[cur]);
        while let Some(h) = at {
            if h.line < total_lines {
                visible[h.line] = true;
            }
            at = up_heading(hs, h.line);
        }
        if let Some((first, last)) = entry_body(hs, hs[cur].line, total_lines) {
            for line in visible
                .iter_mut()
                .take(last.min(total_lines - 1) + 1)
                .skip(first)
            {
                *line = true;
            }
        }
    }
    // Coalesce the hidden lines into runs.
    let mut folds = Vec::new();
    let mut run_start: Option<usize> = None;
    for (l, shown) in visible.iter().enumerate() {
        match (shown, run_start) {
            (false, None) => run_start = Some(l),
            (true, Some(start)) => {
                folds.push((start, l - 1));
                run_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = run_start {
        folds.push((start, total_lines - 1));
    }
    folds
}

/// A subtree move: the two inclusive line ranges to exchange, `first` always
/// preceding `second` in the buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiblingSwap {
    /// Inclusive line range of the earlier subtree.
    pub first: (usize, usize),
    /// Inclusive line range of the later subtree.
    pub second: (usize, usize),
    /// Line delta to apply to the cursor's subtree once the two are exchanged.
    /// Negative when moving up, positive when moving down.
    pub cursor_delta: isize,
}

/// `org-metaup` / `org-metadown` (`outline-move-subtree-up`/`-down`): exchange
/// the subtree containing `line` with its previous (`up`) or next sibling
/// subtree at the same level. `None` when there is no sibling on that side (or
/// point is above the first heading), so the caller leaves the buffer alone.
pub fn sibling_swap(
    hs: &[Heading],
    line: usize,
    total_lines: usize,
    up: bool,
) -> Option<SiblingSwap> {
    let cur = subtree_bounds(hs, line, total_lines)?;
    let sib_head = if up {
        backward_same_level(hs, line)?
    } else {
        forward_same_level(hs, line)?
    };
    let sib = subtree_bounds(hs, sib_head.line, total_lines)?;
    let (first, second) = if up { (sib, cur) } else { (cur, sib) };
    // After the exchange the cursor's subtree starts where the other one did,
    // shifted by the difference in the sibling's length.
    let len = |(a, b): (usize, usize)| b + 1 - a;
    let cursor_delta = if up {
        -(len(sib) as isize)
    } else {
        len(sib) as isize
    };
    (first.1 < second.0).then_some(SiblingSwap {
        first,
        second,
        cursor_delta,
    })
}

/// Apply a [`SiblingSwap`] to `lines`, returning the reordered buffer lines. The
/// text between the two subtrees (there is none for true siblings, but a
/// malformed outline can have some) is preserved in place.
pub fn apply_sibling_swap(lines: &[&str], swap: SiblingSwap) -> Vec<String> {
    let (a0, a1) = swap.first;
    let (b0, b1) = swap.second;
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    out.extend(lines[..a0].iter().map(|s| s.to_string()));
    out.extend(lines[b0..=b1].iter().map(|s| s.to_string()));
    out.extend(lines[a1 + 1..b0].iter().map(|s| s.to_string()));
    out.extend(lines[a0..=a1].iter().map(|s| s.to_string()));
    out.extend(lines[b1 + 1..].iter().map(|s| s.to_string()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
* Alpha
alpha body
** Alpha.1
a1 body
** Alpha.2
a2 body
* Beta
beta body
";

    #[test]
    fn detects_headings_and_levels() {
        let hs = headings(DOC);
        assert_eq!(hs.len(), 4);
        assert_eq!((hs[0].line, hs[0].level), (0, 1)); // * Alpha
        assert_eq!((hs[1].line, hs[1].level), (2, 2)); // ** Alpha.1
        assert_eq!((hs[3].line, hs[3].level), (6, 1)); // * Beta
                                                       // char_pos of "** Alpha.1" = len("* Alpha\nalpha body\n")
        assert_eq!(hs[1].char_pos, "* Alpha\nalpha body\n".chars().count());
    }

    #[test]
    fn markdown_hashes_are_headings() {
        let hs = headings("# Title\ntext\n## Sub\nmore\n");
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[0].level, 1);
        assert_eq!(hs[1].level, 2);
        // A `#comment` with no space is not a heading.
        assert!(headings("#nospace\n").is_empty());
    }

    #[test]
    fn navigation() {
        let hs = headings(DOC);
        // from the top heading (line 0)
        assert_eq!(next_heading(&hs, 0).map(|h| h.line), Some(2));
        assert_eq!(prev_heading(&hs, 6).map(|h| h.line), Some(4));
        // up from Alpha.1 (line 2, level 2) -> Alpha (line 0)
        assert_eq!(up_heading(&hs, 2).map(|h| h.line), Some(0));
        // up from a top-level heading -> none
        assert_eq!(up_heading(&hs, 0), None);
    }

    #[test]
    fn same_level_stays_in_parent() {
        let hs = headings(DOC);
        // forward-same-level from Alpha.1 (line 2) -> Alpha.2 (line 4)
        assert_eq!(forward_same_level(&hs, 2).map(|h| h.line), Some(4));
        // forward-same-level from Alpha.2 -> none (next is * Beta, a smaller level)
        assert_eq!(forward_same_level(&hs, 4), None);
        // backward-same-level from Alpha.2 -> Alpha.1
        assert_eq!(backward_same_level(&hs, 4).map(|h| h.line), Some(2));
        // top-level forward: Alpha (0) -> Beta (6)
        assert_eq!(forward_same_level(&hs, 0).map(|h| h.line), Some(6));
    }

    #[test]
    fn subtree_and_bodies() {
        let hs = headings(DOC);
        let total = DOC.split('\n').count();
        // subtree of Alpha (line 0) covers lines 0..=5 (through Alpha.2 body)
        assert_eq!(subtree_bounds(&hs, 0, total), Some((0, 5)));
        // body to hide for Alpha = lines 1..=5
        assert_eq!(subtree_body(&hs, 0, total), Some((1, 5)));
        // subtree of Alpha.1 (line 2) covers only its own body (line 3)
        assert_eq!(subtree_bounds(&hs, 2, total), Some((2, 3)));
        assert_eq!(subtree_body(&hs, 2, total), Some((3, 3)));
        // hide-body: one range per heading with body
        let bodies = all_bodies(&hs, total);
        assert_eq!(bodies, vec![(1, 1), (3, 3), (5, 5), (7, total - 1)]);
        // entry of Alpha (line 0) is just line 1 (stops at the Alpha.1 subheading)
        assert_eq!(entry_body(&hs, 0, total), Some((1, 1)));
    }

    #[test]
    fn hide_sublevels_keeps_only_shallow_headings() {
        let hs = headings(DOC);
        let total = DOC.split('\n').count();
        // levels=1: only * Alpha and * Beta stay visible; everything between
        // each top heading and the next is folded (bodies + ** subheadings).
        assert_eq!(sublevel_folds(&hs, 1, total), vec![(1, 5), (7, total - 1)]);
        // levels=2: all headings visible, only bodies folded (== hide-body).
        assert_eq!(
            sublevel_folds(&hs, 2, total),
            vec![(1, 1), (3, 3), (5, 5), (7, total - 1)]
        );
    }

    #[test]
    fn hide_leaves_folds_bodies_within_the_subtree() {
        let hs = headings(DOC);
        let total = DOC.split('\n').count();
        // Cursor on Alpha (line 0): fold the bodies of Alpha, Alpha.1, Alpha.2,
        // but keep the ** subheadings visible. Beta's subtree is untouched.
        assert_eq!(
            subtree_leaf_bodies(&hs, 0, total),
            vec![(1, 1), (3, 3), (5, 5)]
        );
        // Cursor on Alpha.1 (line 2): only its own body folds.
        assert_eq!(subtree_leaf_bodies(&hs, 2, total), vec![(3, 3)]);
    }

    #[test]
    fn show_children_reveals_immediate_subheadings() {
        let hs = headings(DOC);
        let total = DOC.split('\n').count();
        // Cursor on Alpha (line 0), one level: reveal Alpha.1 and Alpha.2
        // headings, fold Alpha's body and each child's body.
        assert_eq!(
            subtree_child_folds(&hs, 0, 1, total),
            vec![(1, 1), (3, 3), (5, 5)]
        );
        // Alpha.1 (line 2) has no subheadings: its whole body folds.
        assert_eq!(subtree_child_folds(&hs, 2, 1, total), vec![(3, 3)]);
    }

    #[test]
    fn outline_cycle_advances_folded_children_all() {
        // Fully hidden (5 of 5) -> reveal children.
        assert_eq!(outline_cycle_next(5, 5), CycleStep::ShowChildren);
        // Partially hidden (children shown) -> reveal all.
        assert_eq!(outline_cycle_next(5, 2), CycleStep::ShowAll);
        // Nothing hidden (fully shown) -> fold.
        assert_eq!(outline_cycle_next(5, 0), CycleStep::Fold);
        // Empty body -> fold (no-op-ish).
        assert_eq!(outline_cycle_next(0, 0), CycleStep::Fold);
    }

    #[test]
    fn outline_cycle_buffer_advances_overview_contents_showall() {
        // Nothing hidden -> overview.
        assert_eq!(
            outline_cycle_buffer_next(false, false),
            BufferCycleStep::Overview
        );
        // Headings hidden (overview) -> contents.
        assert_eq!(
            outline_cycle_buffer_next(true, true),
            BufferCycleStep::Contents
        );
        // Only bodies hidden (contents) -> show all.
        assert_eq!(
            outline_cycle_buffer_next(true, false),
            BufferCycleStep::ShowAll
        );
    }

    #[test]
    fn matching_subtree_bodies_folds_selected_headings() {
        let hs = headings(DOC);
        let total = DOC.split('\n').count();
        // Fold only the subtrees of the top-level headings (level 1): Alpha's
        // whole subtree body (lines 1..=5) and Beta's body (line 7..=end).
        let folds = matching_subtree_bodies(&hs, total, |i| hs[i].level == 1);
        assert_eq!(folds, vec![(1, 5), (7, total - 1)]);
        // Matching none yields no folds.
        assert!(matching_subtree_bodies(&hs, total, |_| false).is_empty());
    }

    /// `outline-hide-other` keeps three things visible: the current entry, its
    /// ancestors, and every top-level heading. Everything else folds away — the
    /// exact set is the whole behaviour of the command.
    #[test]
    fn hide_other_keeps_the_current_entry_its_ancestors_and_top_levels() {
        //  0 * One
        //  1 body one
        //  2 ** One A
        //  3 body A          <- point here
        //  4 ** One B
        //  5 body B
        //  6 * Two
        //  7 body two
        let text = "* One\nbody one\n** One A\nbody A\n** One B\nbody B\n* Two\nbody two";
        let hs = headings(text);
        let folds = hide_other_folds(&hs, 3, 8);
        // Visible: 0 (top level), 2 (current heading, also its own ancestor
        // chain's leaf), 3 (current body), 6 (top level).
        assert_eq!(folds, vec![(1, 1), (4, 5), (7, 7)]);

        // Point on the top-level heading: its body is visible, its children fold.
        let folds = hide_other_folds(&hs, 0, 8);
        assert_eq!(folds, vec![(2, 5), (7, 7)], "One's body is line 1 only");

        // No headings at all: nothing to hide.
        assert!(hide_other_folds(&[], 0, 4).is_empty());
    }

    /// The swap operates on the buffer's real lines — a trailing newline would
    /// otherwise make an empty last "line" travel with the final subtree.
    fn swap_lines() -> Vec<&'static str> {
        DOC.trim_end_matches('\n').split('\n').collect()
    }

    #[test]
    fn sibling_swap_moves_a_subtree_down_past_its_next_sibling() {
        let lines = swap_lines();
        let hs = headings(DOC);
        // Point inside "** Alpha.1" (line 3), moving down swaps it with "** Alpha.2".
        let swap = sibling_swap(&hs, 3, lines.len(), false).expect("Alpha.1 has a next sibling");
        assert_eq!(swap.first, (2, 3)); // ** Alpha.1 + body
        assert_eq!(swap.second, (4, 5)); // ** Alpha.2 + body
        assert_eq!(swap.cursor_delta, 2);
        let out = apply_sibling_swap(&lines, swap);
        assert_eq!(
            &out[2..6],
            ["** Alpha.2", "a2 body", "** Alpha.1", "a1 body"]
        );
        // Everything outside the swapped span is untouched.
        assert_eq!(&out[0..2], ["* Alpha", "alpha body"]);
        assert_eq!(&out[6..8], ["* Beta", "beta body"]);
        assert_eq!(out.len(), lines.len());
    }

    #[test]
    fn sibling_swap_moves_a_subtree_up_and_carries_its_children() {
        let lines = swap_lines();
        let hs = headings(DOC);
        // "* Beta" (line 6) up: swaps with "* Alpha", whose subtree includes both
        // ** children — the whole 0..=5 block moves below Beta.
        let swap = sibling_swap(&hs, 6, lines.len(), true).expect("Beta has a previous sibling");
        assert_eq!(swap.first, (0, 5));
        assert_eq!(swap.second, (6, 7));
        assert_eq!(swap.cursor_delta, -6);
        let out = apply_sibling_swap(&lines, swap);
        assert_eq!(&out[0..4], ["* Beta", "beta body", "* Alpha", "alpha body"]);
        assert_eq!(&out[4..6], ["** Alpha.1", "a1 body"]);
    }

    #[test]
    fn sibling_swap_declines_at_the_edges() {
        let lines = swap_lines();
        let hs = headings(DOC);
        // "* Alpha" is the first top-level heading: no previous sibling.
        assert!(sibling_swap(&hs, 0, lines.len(), true).is_none());
        // "* Beta" is the last: no next sibling.
        assert!(sibling_swap(&hs, 6, lines.len(), false).is_none());
        // "** Alpha.2" has no next sibling inside its parent (Beta is a level up).
        assert!(sibling_swap(&hs, 4, lines.len(), false).is_none());
        // No headings at all.
        assert!(sibling_swap(&[], 0, 4, true).is_none());
    }
}
