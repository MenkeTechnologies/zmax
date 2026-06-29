//! Interactive 3-pane merge viewer (slice 2 of a JetBrains-style diff/merge
//! tool, built on the slice-1 side-by-side alignment).
//!
//! A full-screen overlay [`Component`] that shows the focused buffer's git diff
//! as three vertically-aligned panes: the file's `HEAD` version on the
//! **left**, a live **Result** in the **center**, and the current working-tree
//! buffer on the **right**. Opened with the `:diff` typable command.
//!
//! The alignment is computed once up front from a line-level [`imara_diff`]
//! diff between the two texts (see [`align`]). Each [`DiffRow`] pairs an
//! optional left line with an optional right line; changed regions pair old
//! lines against new lines and pad the shorter side with blank rows so all
//! panes stay in lock-step as you scroll.
//!
//! Contiguous runs of changed rows become [`Block`]s, each with a
//! [`Resolution`] (`Left` = take HEAD, `Right` = keep working tree). The
//! center Result pane is recomputed every frame from the per-block
//! resolutions. `Enter` writes the resolved text back into the document as a
//! single undoable transaction.
//!
//! Keys: `j`/`k`/arrows scroll a row, PageUp/PageDown (`ctrl-d`/`ctrl-u`) a
//! screenful, `g`/`G` jump to top/bottom, `n`/`p` move the selected block,
//! `,`/`[`/`h` take HEAD, `.`/`]`/`l` take working, `L`/`R` resolve all,
//! `Enter`/`a` apply, `q`/`Esc` cancel. Mouse wheel scrolls too.

use std::ops::Range;

use imara_diff::{sources::lines, Algorithm, Diff, InternedInput};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::{Rect, Style};
use zemacs_view::input::MouseEventKind;
use zemacs_view::DocumentId;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
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

/// Which side a change block resolves to in the Result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Resolution {
    /// Take the HEAD (left) side — reverts the hunk.
    Left,
    /// Keep the working-tree (right) side — the default.
    Right,
}

/// A contiguous run of changed rows together with its chosen resolution.
#[derive(Clone, Debug)]
struct Block {
    /// Half-open range of row indices (into `DiffView::rows`) the block covers.
    rows: Range<usize>,
    /// Which side this block contributes to the Result.
    resolution: Resolution,
}

/// Turn the aligned rows into change blocks (contiguous runs of non-unchanged
/// rows), each defaulting to [`Resolution::Right`] so the Result initially
/// equals the working tree. Pure — built on [`change_blocks`].
fn compute_blocks(rows: &[DiffRow]) -> Vec<Block> {
    change_blocks(rows)
        .into_iter()
        .map(|start| {
            let mut end = start;
            while end < rows.len() && rows[end].kind != RowKind::Unchanged {
                end += 1;
            }
            Block {
                rows: start..end,
                resolution: Resolution::Right,
            }
        })
        .collect()
}

/// Compute the resolved Result text from the alignment + per-block
/// resolutions. Pure (no editor state) so it can be unit-tested.
///
/// Walks `rows` in order: unchanged rows emit their (identical) line; rows
/// inside a block emit the chosen side's *actual* line and skip padded blanks
/// (`None`). Each emitted line is newline-terminated.
fn result_text(
    rows: &[DiffRow],
    blocks: &[Block],
    base_lines: &[String],
    doc_lines: &[String],
) -> String {
    // Per-row resolution, `None` for unchanged rows outside any block.
    let mut row_res: Vec<Option<Resolution>> = vec![None; rows.len()];
    for block in blocks {
        for i in block.rows.clone() {
            row_res[i] = Some(block.resolution);
        }
    }

    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        match row_res[i] {
            // Unchanged: both sides hold the same line; use the working tree.
            None => {
                if let Some(r) = row.right.and_then(|r| doc_lines.get(r)) {
                    out.push_str(r);
                    out.push('\n');
                } else if let Some(l) = row.left.and_then(|l| base_lines.get(l)) {
                    out.push_str(l);
                    out.push('\n');
                }
            }
            Some(Resolution::Left) => {
                if let Some(l) = row.left.and_then(|l| base_lines.get(l)) {
                    out.push_str(l);
                    out.push('\n');
                }
            }
            Some(Resolution::Right) => {
                if let Some(r) = row.right.and_then(|r| doc_lines.get(r)) {
                    out.push_str(r);
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// The full-screen interactive 3-pane merge overlay.
pub struct DiffView {
    /// Display name of the file being diffed (shown in the header).
    file_name: String,
    /// Document the resolved Result is written back into on Apply.
    doc_id: DocumentId,
    /// HEAD lines (left pane), trailing newline stripped.
    base_lines: Vec<String>,
    /// Working-tree lines (right pane), trailing newline stripped.
    doc_lines: Vec<String>,
    rows: Vec<DiffRow>,
    /// Change blocks with their (mutable) per-block resolution.
    blocks: Vec<Block>,
    /// Index into `blocks` of the currently-focused block.
    selected: usize,
    /// Index of the top visible row.
    scroll: usize,
    /// Number of body rows visible in the last render (for page scrolling).
    viewport: usize,
}

impl DiffView {
    /// Construct a viewer from the HEAD text and the current buffer text.
    /// `doc_id` is the document the resolved Result is applied to.
    pub fn new(file_name: String, doc_id: DocumentId, base: &str, doc: &str) -> Self {
        let rows = align(base, doc);
        let blocks = compute_blocks(&rows);
        DiffView {
            file_name,
            doc_id,
            base_lines: split_lines(base),
            doc_lines: split_lines(doc),
            rows,
            blocks,
            selected: 0,
            scroll: 0,
            viewport: 1,
        }
    }

    /// True when the two texts are identical (nothing to show).
    pub fn is_unchanged(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Number of blocks resolved away from the default (`Right`), for the header.
    fn resolved_count(&self) -> usize {
        self.blocks
            .iter()
            .filter(|b| b.resolution == Resolution::Left)
            .count()
    }

    fn max_scroll(&self) -> usize {
        self.rows.len().saturating_sub(self.viewport)
    }

    fn scroll_by(&mut self, delta: isize) {
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, self.max_scroll() as isize) as usize;
    }

    /// Scroll so the selected block is within the viewport.
    fn scroll_to_selected(&mut self) {
        if let Some(block) = self.blocks.get(self.selected) {
            let start = block.rows.start;
            if start < self.scroll {
                self.scroll = start;
            } else if start >= self.scroll + self.viewport {
                self.scroll = start.saturating_sub(self.viewport.saturating_sub(1));
            }
            self.scroll = self.scroll.min(self.max_scroll());
        }
    }

    /// Focus the next change block and scroll it into view.
    fn next_change(&mut self) {
        if self.selected + 1 < self.blocks.len() {
            self.selected += 1;
        }
        self.scroll_to_selected();
    }

    /// Focus the previous change block and scroll it into view.
    fn prev_change(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.scroll_to_selected();
    }

    /// Set the selected block's resolution.
    fn resolve_selected(&mut self, resolution: Resolution) {
        if let Some(block) = self.blocks.get_mut(self.selected) {
            block.resolution = resolution;
        }
    }

    /// Set every block's resolution.
    fn resolve_all(&mut self, resolution: Resolution) {
        for block in &mut self.blocks {
            block.resolution = resolution;
        }
    }

    /// The block index owning row `i`, if any (for render highlighting).
    fn block_at(&self, i: usize) -> Option<usize> {
        self.blocks.iter().position(|b| b.rows.contains(&i))
    }

    /// Build the resolved Result text from the current resolutions.
    fn result_text(&self) -> String {
        result_text(&self.rows, &self.blocks, &self.base_lines, &self.doc_lines)
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
            // Apply: write the resolved Result back into the document, then close.
            key!(Enter) | key!('a') => {
                let result = self.result_text();
                let doc_id = self.doc_id;
                let apply: Callback = Box::new(move |compositor: &mut Compositor, cx| {
                    let (view, doc) = current!(cx.editor);
                    if doc.id() == doc_id {
                        let new_text = zemacs_core::Rope::from(result.as_str());
                        let transaction =
                            zemacs_core::diff::compare_ropes(&doc.text().clone(), &new_text);
                        doc.apply(&transaction, view.id);
                        doc.append_changes_to_history(view);
                    }
                    compositor.pop();
                });
                return EventResult::Consumed(Some(apply));
            }
            key!('j') | key!(Down) => self.scroll_by(1),
            key!('k') | key!(Up) => self.scroll_by(-1),
            key!(PageDown) | ctrl!('d') | ctrl!('f') => self.scroll_by(page),
            key!(PageUp) | ctrl!('u') | ctrl!('b') => self.scroll_by(-page),
            key!('g') | key!(Home) => self.scroll = 0,
            key!('G') | key!(End) => self.scroll = self.max_scroll(),
            key!('n') => self.next_change(),
            key!('p') => self.prev_change(),
            // Resolve the selected block.
            key!(',') | key!('[') | key!('h') => self.resolve_selected(Resolution::Left),
            key!('.') | key!(']') | key!('l') => self.resolve_selected(Resolution::Right),
            // Resolve all blocks one way.
            key!('L') => self.resolve_all(Resolution::Left),
            key!('R') => self.resolve_all(Resolution::Right),
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
        // Two header rows, then the body. Three panes split into thirds with two
        // 1-column separators between them.
        let header_h = 2u16;
        let body_y = area.y + header_h;
        let body_h = area.height.saturating_sub(header_h);
        self.viewport = body_h as usize;

        // (width - 2 separators) / 3, with any remainder going to the panes
        // earlier so the total exactly fills the area.
        let avail = area.width.saturating_sub(2);
        let third = avail / 3;
        let left_w = third + (avail % 3).min(1);
        let center_w = third + (avail % 3).saturating_sub(1).min(1);
        let right_w = avail - left_w - center_w;

        let left_x = area.x;
        let sep1_x = left_x + left_w;
        let center_x = sep1_x + 1;
        let sep2_x = center_x + center_w;
        let right_x = sep2_x + 1;

        // Gutter width: enough digits for the largest line number, plus a space.
        let max_no = self.base_lines.len().max(self.doc_lines.len()).max(1);
        let digits = ((max_no as f64).log10().floor() as usize) + 1;
        let gutter = (digits + 1) as u16;
        // Center gutter is two wider: a select marker + a direction arrow.
        let center_gutter = gutter + 2;

        // ── Header ──────────────────────────────────────────────────────────
        let changes = self.blocks.len();
        let resolved = self.resolved_count();
        let header = format!(
            " {}  —  {} change{} · {} resolved",
            self.file_name,
            changes,
            if changes == 1 { "" } else { "s" },
            resolved,
        );
        let title_style = theme.get("ui.text.focus");
        surface.set_stringn(
            area.x,
            area.y,
            &header,
            area.width as usize,
            to_zstyle_bold(title_style),
        );
        // Key hint + column labels on the second header row.
        let hint = ", take HEAD   . take working   n/p nav   Enter apply   q cancel";
        surface.set_stringn(left_x, area.y + 1, " HEAD", left_w as usize, linenr_style);
        surface.set_stringn(center_x, area.y + 1, " Result", center_w as usize, linenr_style);
        surface.set_stringn(
            right_x,
            area.y + 1,
            " Working tree",
            right_w as usize,
            linenr_style,
        );
        // Separators down the full height.
        for y in area.y..area.y + area.height {
            surface.set_string(sep1_x, y, "\u{2502}", sep_style);
            surface.set_string(sep2_x, y, "\u{2502}", sep_style);
        }
        // Overlay the key hint dimly on the right of the title row if it fits.
        if (header.len() + hint.len() + 3) < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                linenr_style,
            );
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
        let selected_style = theme.get("ui.selection");
        let left_inner = left_w.saturating_sub(gutter) as usize;
        let center_inner = center_w.saturating_sub(center_gutter) as usize;
        let right_inner = right_w.saturating_sub(gutter) as usize;

        let mut left_lines = Vec::with_capacity(body_h as usize);
        let mut center_lines = Vec::with_capacity(body_h as usize);
        let mut right_lines = Vec::with_capacity(body_h as usize);
        for (offset, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body_h as usize)
        {
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
            // Center Result line, recomputed live from the resolutions.
            let block = self.block_at(offset);
            let resolution = block.map(|b| self.blocks[b].resolution);
            let selected = block == Some(self.selected);
            center_lines.push(result_line(
                row,
                resolution,
                selected,
                &self.base_lines,
                &self.doc_lines,
                gutter as usize,
                center_inner,
                &style,
                selected_style,
            ));
        }
        // Pad the tail so the background fills the whole body.
        while left_lines.len() < body_h as usize {
            left_lines.push(Line::default());
            center_lines.push(Line::default());
            right_lines.push(Line::default());
        }

        let left_rect = Rect::new(left_x, body_y, left_w, body_h);
        let center_rect = Rect::new(center_x, body_y, center_w, body_h);
        let right_rect = Rect::new(right_x, body_y, right_w, body_h);
        crate::ui::rat::render(Paragraph::new(left_lines), left_rect, surface);
        crate::ui::rat::render(Paragraph::new(center_lines), center_rect, surface);
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

/// Build the center **Result** pane line for one aligned row, live from its
/// block resolution. A two-column prefix (`▌` select marker + `◀`/`▶` direction
/// arrow) precedes the same number gutter + content layout as [`pane_line`].
/// Unchanged rows (`resolution == None`) show no marker/arrow.
#[allow(clippy::too_many_arguments)]
fn result_line<'a>(
    row: &DiffRow,
    resolution: Option<Resolution>,
    selected: bool,
    base_lines: &[String],
    doc_lines: &[String],
    gutter: usize,
    inner: usize,
    style: &PaneStyle,
    selected_style: Style,
) -> ratatui::text::Line<'a> {
    use crate::ui::rat::to_rat_style;
    use ratatui::text::{Line, Span};

    let marker = if selected { "\u{258C}" } else { " " }; // ▌
    let arrow = match resolution {
        None => " ",
        Some(Resolution::Left) => "\u{25C0}",  // ◀
        Some(Resolution::Right) => "\u{25B6}", // ▶
    };
    // Which source line this row resolves to.
    let (idx, src): (Option<usize>, &[String]) = match resolution {
        None if row.right.is_some() => (row.right, doc_lines),
        None => (row.left, base_lines),
        Some(Resolution::Left) => (row.left, base_lines),
        Some(Resolution::Right) => (row.right, doc_lines),
    };

    let content_style = if selected { selected_style } else { style.text };
    let mut prefix = vec![
        Span::styled(marker.to_string(), to_rat_style(style.linenr)),
        Span::styled(arrow.to_string(), to_rat_style(style.delta)),
    ];

    match idx {
        Some(i) => {
            let num = format!("{:>width$} ", i + 1, width = gutter.saturating_sub(1));
            let mut content: String =
                src.get(i).map(|s| s.replace('\t', "    ")).unwrap_or_default();
            truncate_pad(&mut content, inner);
            prefix.push(Span::styled(num, to_rat_style(style.linenr)));
            prefix.push(Span::styled(content, to_rat_style(content_style)));
        }
        None => {
            // Resolved side contributes no line here (a blank filler row).
            let mut filler = String::new();
            truncate_pad(&mut filler, gutter + inner);
            prefix.push(Span::styled(filler, to_rat_style(style.filler)));
        }
    }
    Line::from(prefix)
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

    // ── Result computation (slice 2) ─────────────────────────────────────────

    /// Build the full Result-text inputs from two texts, override each block's
    /// resolution with `resolutions[i]`, and return the merged text.
    fn merged(base: &str, doc: &str, resolutions: &[Resolution]) -> String {
        let rows = align(base, doc);
        let mut blocks = compute_blocks(&rows);
        assert_eq!(
            blocks.len(),
            resolutions.len(),
            "test gave the wrong number of resolutions"
        );
        for (b, &r) in blocks.iter_mut().zip(resolutions) {
            b.resolution = r;
        }
        let base_lines = split_lines(base);
        let doc_lines = split_lines(doc);
        result_text(&rows, &blocks, &base_lines, &doc_lines)
    }

    #[test]
    fn compute_blocks_default_to_right() {
        let rows = align("a\nb\nc\n", "a\nB\nc\n");
        let blocks = compute_blocks(&rows);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].rows, 1..2);
        assert_eq!(blocks[0].resolution, Resolution::Right);
    }

    #[test]
    fn unchanged_text_results_unchanged() {
        // No blocks: Result is just the text.
        let rows = align("a\nb\n", "a\nb\n");
        let blocks = compute_blocks(&rows);
        assert!(blocks.is_empty());
        let lines = split_lines("a\nb\n");
        assert_eq!(result_text(&rows, &blocks, &lines, &lines), "a\nb\n");
    }

    #[test]
    fn modification_left_is_head_right_is_working() {
        // "b" -> "B". Left keeps HEAD ("b"), Right keeps working ("B").
        assert_eq!(merged("a\nb\nc\n", "a\nB\nc\n", &[Resolution::Left]), "a\nb\nc\n");
        assert_eq!(
            merged("a\nb\nc\n", "a\nB\nc\n", &[Resolution::Right]),
            "a\nB\nc\n"
        );
    }

    #[test]
    fn deletion_left_keeps_line_right_drops_it() {
        // "b" deleted in working. Left reverts (keeps "b"), Right drops it.
        assert_eq!(merged("a\nb\nc\n", "a\nc\n", &[Resolution::Left]), "a\nb\nc\n");
        assert_eq!(merged("a\nb\nc\n", "a\nc\n", &[Resolution::Right]), "a\nc\n");
    }

    #[test]
    fn insertion_left_drops_line_right_keeps_it() {
        // "b" inserted in working. Left drops it, Right keeps it.
        assert_eq!(merged("a\nc\n", "a\nb\nc\n", &[Resolution::Left]), "a\nc\n");
        assert_eq!(
            merged("a\nc\n", "a\nb\nc\n", &[Resolution::Right]),
            "a\nb\nc\n"
        );
    }

    #[test]
    fn lopsided_change_emits_actual_lines_not_blanks() {
        // 1 line -> 3 lines. Right emits all three working lines (no padding);
        // Left emits the single HEAD line.
        let base = "a\nx\nc\n";
        let doc = "a\np\nq\nr\nc\n";
        assert_eq!(merged(base, doc, &[Resolution::Right]), "a\np\nq\nr\nc\n");
        assert_eq!(merged(base, doc, &[Resolution::Left]), "a\nx\nc\n");
    }

    #[test]
    fn multiple_blocks_resolve_independently() {
        // Two separate changes: take HEAD for the first, working for the second.
        let base = "a\nb\nc\nd\ne\n";
        let doc = "a\nB\nc\nD\ne\n";
        assert_eq!(
            merged(base, doc, &[Resolution::Left, Resolution::Right]),
            "a\nb\nc\nD\ne\n"
        );
    }
}
