//! Outline — the zemacs port of GNU Emacs `outline-mode` heading structure.
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
}
