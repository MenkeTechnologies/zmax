//! Side-by-side diff viewer (slice 1 of a JetBrains-style diff/merge tool).
//!
//! A read-only, full-screen overlay [`Component`] that shows the focused
//! buffer's git diff with the file's `HEAD` version on the **left** and the
//! current working-tree buffer on the **right**, vertically aligned line by
//! line. Opened with the `:diff` typable command.
//!
//! The alignment is computed once up front from a line-level [`imara_diff`]
//! diff between the two texts (see [`align`]). Each [`DiffRow`] pairs an
//! optional left line with an optional right line; changed regions pair old
//! lines against new lines and pad the shorter side with blank rows so both
//! panes stay in lock-step as you scroll. This is the foundation for a later
//! 3-pane merge view: the alignment model and scroll-sync already generalise.
//!
//! Keys: `j`/`k`/arrows scroll a row, PageUp/PageDown (`ctrl-d`/`ctrl-u`) a
//! screenful, `g`/`G` jump to top/bottom, `n`/`p` jump between change blocks,
//! `q`/`Esc` close. Mouse wheel scrolls too.

use imara_diff::{sources::lines, Algorithm, Diff, InternedInput};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::{Rect, Style};
use zemacs_view::input::MouseEventKind;

use crate::{
    compositor::{Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// What a single aligned row represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RowKind {
    /// Identical on both sides.
    Unchanged,
    /// Present only on the right (working tree) — an inserted line.
    Added,
    /// Present only on the left (HEAD) — a deleted line.
    Removed,
    /// A modified line: old text on the left, new text on the right.
    Changed,
}

/// One vertically-aligned row of the side-by-side view. `left`/`right` index
/// into [`DiffView::base_lines`] / [`DiffView::doc_lines`]; `None` means that
/// side is a blank filler so the other side's change stays aligned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct DiffRow {
    left: Option<usize>,
    right: Option<usize>,
    kind: RowKind,
}

/// Build the aligned row list for two texts.
///
/// Pure and unit-tested. Lines are tokenised exactly as `imara_diff` sees them
/// (see [`split_lines`]) so the line indices stored in each [`DiffRow`] line up
/// with the displayed line vectors.
fn align(base: &str, doc: &str) -> Vec<DiffRow> {
    let n_base = split_lines(base).len() as u32;
    let n_doc = split_lines(doc).len() as u32;

    let input = InternedInput::new(lines(base), lines(doc));
    let diff = Diff::compute(Algorithm::Histogram, &input);

    let mut rows = Vec::new();
    let mut b = 0u32; // next un-emitted base (HEAD) line
    let mut d = 0u32; // next un-emitted doc (working) line

    for hunk in diff.hunks() {
        // Unchanged region between the previous hunk and this one: paired rows.
        while b < hunk.before.start {
            rows.push(DiffRow {
                left: Some(b as usize),
                right: Some(d as usize),
                kind: RowKind::Unchanged,
            });
            b += 1;
            d += 1;
        }

        // The hunk itself. Pair the overlapping span as `Changed`, then spill
        // the longer side into pure `Removed` / `Added` rows.
        let removed = hunk.before.end - hunk.before.start;
        let added = hunk.after.end - hunk.after.start;
        let common = removed.min(added);
        for _ in 0..common {
            rows.push(DiffRow {
                left: Some(b as usize),
                right: Some(d as usize),
                kind: RowKind::Changed,
            });
            b += 1;
            d += 1;
        }
        while b < hunk.before.end {
            rows.push(DiffRow {
                left: Some(b as usize),
                right: None,
                kind: RowKind::Removed,
            });
            b += 1;
        }
        while d < hunk.after.end {
            rows.push(DiffRow {
                left: None,
                right: Some(d as usize),
                kind: RowKind::Added,
            });
            d += 1;
        }
    }

    // Trailing unchanged tail. Both sides advance together.
    while b < n_base && d < n_doc {
        rows.push(DiffRow {
            left: Some(b as usize),
            right: Some(d as usize),
            kind: RowKind::Unchanged,
        });
        b += 1;
        d += 1;
    }

    rows
}

/// Split text into lines the same way `imara_diff::sources::lines` tokenises
/// it: one entry per line, trailing newline stripped, no phantom final entry.
fn split_lines(text: &str) -> Vec<String> {
    lines(text)
        .map(|l| l.strip_suffix('\n').unwrap_or(l))
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .map(str::to_string)
        .collect()
}

/// Row indices at which a contiguous run of changed/added/removed rows begins.
fn change_blocks(rows: &[DiffRow]) -> Vec<usize> {
    let mut blocks = Vec::new();
    let mut prev_changed = false;
    for (i, row) in rows.iter().enumerate() {
        let changed = row.kind != RowKind::Unchanged;
        if changed && !prev_changed {
            blocks.push(i);
        }
        prev_changed = changed;
    }
    blocks
}

/// The full-screen side-by-side diff overlay.
pub struct DiffView {
    /// Display name of the file being diffed (shown in the header).
    file_name: String,
    /// HEAD lines (left pane), trailing newline stripped.
    base_lines: Vec<String>,
    /// Working-tree lines (right pane), trailing newline stripped.
    doc_lines: Vec<String>,
    rows: Vec<DiffRow>,
    /// Starting row index of each change block, for `n`/`p` navigation.
    blocks: Vec<usize>,
    /// Index of the top visible row.
    scroll: usize,
    /// Number of body rows visible in the last render (for page scrolling).
    viewport: usize,
}

impl DiffView {
    /// Construct a viewer from the HEAD text and the current buffer text.
    pub fn new(file_name: String, base: &str, doc: &str) -> Self {
        let rows = align(base, doc);
        let blocks = change_blocks(&rows);
        DiffView {
            file_name,
            base_lines: split_lines(base),
            doc_lines: split_lines(doc),
            rows,
            blocks,
            scroll: 0,
            viewport: 1,
        }
    }

    /// True when the two texts are identical (nothing to show).
    pub fn is_unchanged(&self) -> bool {
        self.blocks.is_empty()
    }

    fn max_scroll(&self) -> usize {
        self.rows.len().saturating_sub(self.viewport)
    }

    fn scroll_by(&mut self, delta: isize) {
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, self.max_scroll() as isize) as usize;
    }

    /// Scroll so the next change block below the viewport is at the top.
    fn next_change(&mut self) {
        if let Some(&start) = self.blocks.iter().find(|&&b| b > self.scroll) {
            self.scroll = start.min(self.max_scroll());
        }
    }

    /// Scroll so the previous change block above the viewport is at the top.
    fn prev_change(&mut self) {
        if let Some(&start) = self.blocks.iter().rev().find(|&&b| b < self.scroll) {
            self.scroll = start.min(self.max_scroll());
        }
    }
}

impl Component for DiffView {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let close: crate::compositor::Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        let key = match event {
            Event::Key(key) => *key,
            Event::Mouse(ev) => {
                match ev.kind {
                    MouseEventKind::ScrollDown => self.scroll_by(3),
                    MouseEventKind::ScrollUp => self.scroll_by(-3),
                    _ => {}
                }
                return EventResult::Consumed(None);
            }
            _ => return EventResult::Ignored(None),
        };

        let page = self.viewport.max(1) as isize;
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!(Down) => self.scroll_by(1),
            key!('k') | key!(Up) => self.scroll_by(-1),
            key!(PageDown) | ctrl!('d') | ctrl!('f') => self.scroll_by(page),
            key!(PageUp) | ctrl!('u') | ctrl!('b') => self.scroll_by(-page),
            key!('g') | key!(Home) => self.scroll = 0,
            key!('G') | key!(End) => self.scroll = self.max_scroll(),
            key!('n') => self.next_change(),
            key!('p') => self.prev_change(),
            _ => {}
        }
        // Stay modal: never let keys leak to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use ratatui::text::Line;
        use ratatui::widgets::Paragraph;

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        let linenr_style = theme.get("ui.linenr");
        let sep_style = theme.get("ui.background.separator");
        let plus_style = theme.get("diff.plus");
        let minus_style = theme.get("diff.minus");
        let delta_style = theme.get("diff.delta");

        surface.clear_with(area, bg);

        if area.width < 8 || area.height < 4 {
            return;
        }

        // ── Layout ──────────────────────────────────────────────────────────
        // Two header rows, then the body. A 1-column separator splits the panes.
        let header_h = 2u16;
        let body_y = area.y + header_h;
        let body_h = area.height.saturating_sub(header_h);
        self.viewport = body_h as usize;

        let pane_w = area.width.saturating_sub(1) / 2;
        let sep_x = area.x + pane_w;
        let right_x = sep_x + 1;
        let right_w = area.width.saturating_sub(pane_w + 1);

        // Gutter width: enough digits for the largest line number, plus a space.
        let max_no = self.base_lines.len().max(self.doc_lines.len()).max(1);
        let digits = ((max_no as f64).log10().floor() as usize) + 1;
        let gutter = (digits + 1) as u16;

        // ── Header ──────────────────────────────────────────────────────────
        let changes = self.blocks.len();
        let header = format!(
            " {}  —  {} change{}",
            self.file_name,
            changes,
            if changes == 1 { "" } else { "s" }
        );
        let title_style = theme.get("ui.text.focus");
        surface.set_stringn(
            area.x,
            area.y,
            &header,
            area.width as usize,
            to_zstyle_bold(title_style),
        );
        // Column labels on the second header row.
        surface.set_stringn(area.x, area.y + 1, " HEAD", pane_w as usize, linenr_style);
        surface.set_stringn(
            right_x,
            area.y + 1,
            " Working tree",
            right_w as usize,
            linenr_style,
        );
        // Separator down the full height.
        for y in area.y..area.y + area.height {
            surface.set_string(sep_x, y, "\u{2502}", sep_style);
        }

        if body_h == 0 {
            return;
        }

        // ── Body: build a ratatui Paragraph per pane ─────────────────────────
        let style = PaneStyle {
            text: text_style,
            linenr: linenr_style,
            filler: sep_style,
            plus: plus_style,
            minus: minus_style,
            delta: delta_style,
        };
        let left_inner = pane_w.saturating_sub(gutter) as usize;
        let right_inner = right_w.saturating_sub(gutter) as usize;

        let mut left_lines = Vec::with_capacity(body_h as usize);
        let mut right_lines = Vec::with_capacity(body_h as usize);
        for row in self.rows.iter().skip(self.scroll).take(body_h as usize) {
            left_lines.push(pane_line(
                row.left,
                &self.base_lines,
                row.kind,
                Side::Left,
                gutter as usize,
                left_inner,
                &style,
            ));
            right_lines.push(pane_line(
                row.right,
                &self.doc_lines,
                row.kind,
                Side::Right,
                gutter as usize,
                right_inner,
                &style,
            ));
        }
        // Pad the tail so the background fills the whole body.
        while left_lines.len() < body_h as usize {
            left_lines.push(Line::default());
            right_lines.push(Line::default());
        }

        let left_rect = Rect::new(area.x, body_y, pane_w, body_h);
        let right_rect = Rect::new(right_x, body_y, right_w, body_h);
        crate::ui::rat::render(Paragraph::new(left_lines), left_rect, surface);
        crate::ui::rat::render(Paragraph::new(right_lines), right_rect, surface);
    }

    fn id(&self) -> Option<&'static str> {
        Some("diff")
    }
}

/// Which pane a line belongs to (selects deleted/added emphasis).
#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

/// Resolved theme styles for the panes.
struct PaneStyle {
    text: Style,
    linenr: Style,
    filler: Style,
    plus: Style,
    minus: Style,
    delta: Style,
}

/// Build one ratatui `Line` for a pane row: a right-aligned line-number gutter
/// followed by the line content, padded to `inner` so the row background fills
/// the pane width.
fn pane_line<'a>(
    idx: Option<usize>,
    src: &[String],
    kind: RowKind,
    side: Side,
    gutter: usize,
    inner: usize,
    style: &PaneStyle,
) -> ratatui::text::Line<'a> {
    use crate::ui::rat::to_rat_style;
    use ratatui::text::{Line, Span};

    let zstyle = match (kind, side) {
        (RowKind::Unchanged, _) => style.text,
        (RowKind::Changed, _) => style.delta,
        (RowKind::Removed, _) => style.minus,
        (RowKind::Added, _) => style.plus,
    };

    match idx {
        Some(i) => {
            let num = format!("{:>width$} ", i + 1, width = gutter.saturating_sub(1));
            let mut content: String = src.get(i).map(|s| s.replace('\t', "    ")).unwrap_or_default();
            // Truncate/pad to the inner width so the styled background spans the pane.
            truncate_pad(&mut content, inner);
            Line::from(vec![
                Span::styled(num, to_rat_style(style.linenr)),
                Span::styled(content, to_rat_style(zstyle)),
            ])
        }
        None => {
            // Blank filler on the side that has no counterpart line.
            let _ = side;
            let mut filler = String::new();
            truncate_pad(&mut filler, gutter + inner);
            Line::from(Span::styled(filler, to_rat_style(style.filler)))
        }
    }
}

/// Truncate `s` to `width` display columns (best-effort, char-based) or pad it
/// with spaces to exactly `width` columns.
fn truncate_pad(s: &mut String, width: usize) {
    let count = s.chars().count();
    if count > width {
        *s = s.chars().take(width).collect();
    } else {
        s.extend(std::iter::repeat_n(' ', width - count));
    }
}

/// Add BOLD to a zemacs style.
fn to_zstyle_bold(style: Style) -> Style {
    style.add_modifier(zemacs_view::graphics::Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(rows: &[DiffRow]) -> Vec<RowKind> {
        rows.iter().map(|r| r.kind).collect()
    }

    #[test]
    fn identical_texts_pair_every_line() {
        let rows = align("a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.kind == RowKind::Unchanged));
        for (i, r) in rows.iter().enumerate() {
            assert_eq!(r.left, Some(i));
            assert_eq!(r.right, Some(i));
        }
        assert!(change_blocks(&rows).is_empty());
    }

    #[test]
    fn pure_insertion_pads_left_side() {
        // "b" inserted between a and c.
        let rows = align("a\nc\n", "a\nb\nc\n");
        assert_eq!(
            kinds(&rows),
            vec![RowKind::Unchanged, RowKind::Added, RowKind::Unchanged]
        );
        let added = &rows[1];
        assert_eq!(added.left, None, "inserted line has no HEAD counterpart");
        assert_eq!(added.right, Some(1));
        assert_eq!(change_blocks(&rows), vec![1]);
    }

    #[test]
    fn pure_deletion_pads_right_side() {
        // "b" removed.
        let rows = align("a\nb\nc\n", "a\nc\n");
        assert_eq!(
            kinds(&rows),
            vec![RowKind::Unchanged, RowKind::Removed, RowKind::Unchanged]
        );
        let removed = &rows[1];
        assert_eq!(removed.left, Some(1));
        assert_eq!(removed.right, None, "deleted line has no working counterpart");
    }

    #[test]
    fn modification_pairs_old_against_new() {
        // Single changed line: old "b" on the left, new "B" on the right.
        let rows = align("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(
            kinds(&rows),
            vec![RowKind::Unchanged, RowKind::Changed, RowKind::Unchanged]
        );
        let changed = &rows[1];
        assert_eq!(changed.left, Some(1));
        assert_eq!(changed.right, Some(1));
    }

    #[test]
    fn lopsided_change_pairs_then_pads() {
        // 1 old line replaced by 3 new lines: 1 Changed + 2 Added, panes aligned.
        let rows = align("a\nx\nc\n", "a\np\nq\nr\nc\n");
        assert_eq!(
            kinds(&rows),
            vec![
                RowKind::Unchanged,
                RowKind::Changed,
                RowKind::Added,
                RowKind::Added,
                RowKind::Unchanged,
            ]
        );
        // The two pure-Added rows have blank HEAD sides so the panes stay aligned.
        assert_eq!(rows[2].left, None);
        assert_eq!(rows[3].left, None);
        // One contiguous change block starting at row 1.
        assert_eq!(change_blocks(&rows), vec![1]);
    }

    #[test]
    fn split_lines_matches_diff_tokenisation() {
        assert_eq!(split_lines("a\nb\n"), vec!["a", "b"]);
        assert_eq!(split_lines("a\nb"), vec!["a", "b"]);
        assert_eq!(split_lines(""), Vec::<String>::new());
        assert_eq!(split_lines("a\r\nb\r\n"), vec!["a", "b"]);
    }
}
