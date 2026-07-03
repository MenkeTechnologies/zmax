use crate::compositor::{Component, Context};
use tui::buffer::Buffer as Surface;
use tui::text::Text;
use tui::widgets::{Block, Paragraph, Widget};
use zemacs_view::graphics::{Margin, Rect};
use zemacs_view::info::Info;

/// Hard cap on which-key popup rows regardless of frame height — keeps a huge
/// prefix map (e.g. the emacs/Spacemacs `C-x` tree) from filling the screen.
/// When a map has more entries than fit (cols × this), the popup becomes
/// vertically scrollable (PgDn/PgUp or the mouse wheel; see `Info::scroll`).
const MAX_ROWS: usize = 16;
/// Widest a single `KEY : description` column is allowed to grow — a sanity
/// ceiling so one pathologically long entry cannot dominate the grid. Normal
/// which-key descriptions are well under this, so they are shown in full.
const COL_CAP: usize = 80;
/// Max columns the which-key grid fills across the width (Spacemacs uses up to 8).
const MAX_COLS: usize = 8;
/// Spaces between columns.
const SEP: usize = 3;

/// Lay `lines` (each `"key : desc"`) into a full-width, column-major grid (like
/// Emacs' `describe-bindings`) and return the visible slice starting at `scroll`
/// rows down. The columns are **distributed across the whole width** so the grid
/// always fills the bar with no right-edge dead space; the column count (1..=8)
/// is driven by the screen width — as many columns as fit the widest entry
/// without truncation. Returns `(text, body_width, body_height, rows_total,
/// cols)`; `body_width` is the full inner width the grid spans.
fn grid(
    lines: &[&str],
    scroll: usize,
    max_rows: usize,
    max_width: usize,
) -> (String, usize, usize, usize, usize) {
    let n = lines.len();
    let budget = max_width.saturating_sub(6).max(1); // borders + margin
    if n == 0 {
        return (String::new(), budget, 0, 0, 1);
    }
    let max_cols = MAX_COLS.min(n).max(1);

    // Each column is sized to ITS OWN content (variable widths), column-major —
    // so short entries let many columns fit (like Spacemacs' which-key), not the
    // few that the single widest entry would allow.
    let col_widths = |cols: usize| -> Vec<usize> {
        let rows = n.div_ceil(cols);
        (0..cols)
            .map(|c| {
                let s = (c * rows).min(n);
                let e = ((c + 1) * rows).min(n);
                lines[s..e]
                    .iter()
                    .map(|l| l.chars().count())
                    .max()
                    .unwrap_or(0)
                    .min(COL_CAP)
            })
            .collect()
    };
    // The MOST columns whose natural (untruncated) widths fit the bar, leaving no
    // empty trailing column. Falls back to a single budget-bounded column.
    let (cols, cw) = (1..=max_cols)
        .rev()
        .filter(|&c| (c - 1) * n.div_ceil(c) < n)
        .map(|c| (c, col_widths(c)))
        .find(|(_, w)| w.iter().sum::<usize>() + SEP * w.len().saturating_sub(1) <= budget)
        .unwrap_or_else(|| (1, vec![col_widths(1)[0].min(budget)]));

    let rows_total = n.div_ceil(cols);
    let visible = rows_total.min(max_rows);
    let scroll = scroll.min(rows_total.saturating_sub(visible));

    // Spread the leftover width evenly into the inter-column gaps so the grid
    // fills the whole bar and the last column reaches the right edge — no dead
    // space. (With one column there is no gap; short content then trails.)
    let content: usize = cw.iter().sum();
    let gaps = cols.saturating_sub(1);
    let leftover = budget.saturating_sub(content + SEP * gaps);
    let gap_base = if gaps > 0 { SEP + leftover / gaps } else { 0 };
    let gap_extra = if gaps > 0 { leftover % gaps } else { 0 };

    let mut out = String::new();
    for r in scroll..scroll + visible {
        let mut line = String::new();
        for c in 0..cols {
            let w = cw[c];
            let idx = c * rows_total + r;
            let cell: String = if idx < n {
                lines[idx].chars().take(w).collect()
            } else {
                String::new()
            };
            line.push_str(&format!("{cell:<w$}"));
            if c + 1 < cols {
                line.push_str(&" ".repeat(gap_base + usize::from(c < gap_extra)));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    // The grid spans the full inner width.
    let width = budget;
    (out, width, visible, rows_total, cols)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_descriptions_are_not_truncated_when_they_fit() {
        // Descriptions ~54 chars — longer than the old 48-wide column cap. In a
        // wide popup they must be shown in FULL (the which-key cutoff bug).
        let a = "i : Ask the AI provider about the selection/buffer text";
        let b = "k : Generate a shell command from natural language help";
        let lines = vec![a, b];
        let (text, width, _h, _rows, _cols) = grid(&lines, 0, 16, 220);
        assert!(text.contains("selection/buffer text"), "clipped: {text:?}");
        assert!(text.contains("natural language help"), "clipped: {text:?}");
        assert!(width <= 220 - 6, "grid must fit the popup budget: {width}");
    }

    #[test]
    fn many_short_entries_fill_multiple_columns() {
        let lines: Vec<String> = (0..12).map(|i| format!("{i} : short entry")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (_text, _w, _h, rows, cols) = grid(&refs, 0, 16, 220);
        // Short entries + a wide popup → a wide, short grid (more than one column).
        assert!(cols > 1, "expected multiple columns, got {cols}");
        assert_eq!(rows, 12usize.div_ceil(cols));
    }

    #[test]
    fn narrow_popup_truncates_to_the_budget() {
        // One entry far wider than a narrow popup: falls back to a single column
        // bounded by the budget (truncation only when it genuinely cannot fit).
        let long = "x : an extremely long which-key description that will not fit a narrow popup";
        let (_text, width, _h, _rows, cols) = grid(&[long], 0, 16, 30);
        assert_eq!(cols, 1);
        assert_eq!(width, 30 - 6, "the bar spans the full inner width");
    }

    #[test]
    fn variable_widths_fit_many_short_columns_like_spacemacs() {
        // A Spacemacs-style menu: mostly short labels with a couple of longer
        // ones. Per-column widths (not the single global widest) must let several
        // columns fit and fill the bar — the whole point of the reference layout.
        let mut lines: Vec<String> = (0..40).map(|i| format!("{i} : +grp")).collect();
        lines[3] = "', : select window by number".into();
        lines[20] = "l : layouts-transient-state".into();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (_t, width, _h, _r, cols) = grid(&refs, 0, 16, 110);
        // The single-widest formula (~28 wide) would allow only ~3 columns; with
        // per-column widths the short columns pack in many more.
        assert!(cols >= 4, "expected a wide multi-column grid, got {cols}");
        assert_eq!(width, 110 - 6);
    }

    #[test]
    fn grid_always_spans_the_full_width_with_no_dead_space() {
        // Whatever the entry lengths or column count, the grid fills the whole
        // inner width (body_width == budget) so the full-width bar has no dead
        // space, and the column count stays within 1..=8.
        for &(w, count, desc_len) in &[(200usize, 20usize, 24usize), (90, 16, 78), (300, 40, 12)] {
            let lines: Vec<String> = (0..count)
                .map(|i| format!("{i} : {}", "x".repeat(desc_len)))
                .collect();
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            let (_t, width, _h, _r, cols) = grid(&refs, 0, 16, w);
            assert_eq!(width, w - 6, "grid must span the full inner width for w={w}");
            assert!((1..=8).contains(&cols), "cols out of 1..=8: {cols}");
        }
    }
}

impl Component for Info {
    fn render(&mut self, viewport: Rect, surface: &mut Surface, cx: &mut Context) {
        let text_style = cx.editor.theme.get("ui.text.info");
        let popup_style = cx.editor.theme.get("ui.popup.info");

        // Cap body height at ~the frame minus chrome, and never taller than
        // MAX_ROWS (Spacemacs-style short grid); overflow scrolls.
        let avail = (viewport.height as usize).saturating_sub(6);
        let cap = avail.clamp(1, MAX_ROWS);

        let lines: Vec<&str> = self.text.lines().collect();
        let (text, body_w, body_h, rows_total, _cols) =
            grid(&lines, self.scroll as usize, cap, viewport.width as usize);

        // Clamp the stored scroll so PgDn past the end / a shrunk map is corrected.
        let scrollable = rows_total > body_h;
        let max_scroll = rows_total.saturating_sub(body_h);
        self.scroll = (self.scroll as usize).min(max_scroll) as u16;

        // Title carries a scroll indicator when there's more below/above.
        let title = if scrollable {
            let pct = (self.scroll as usize * 100)
                .checked_div(max_scroll)
                .unwrap_or(0);
            format!("{}  [{pct}%  PgDn/PgUp]", self.title)
        } else {
            self.title.to_string()
        };

        // Full editor width, anchored at the bottom (above the statusline) —
        // Spacemacs' which-key bar. The grid itself (`body_w`) is distributed to
        // fill this width, so there is no dead space inside the bar.
        let _ = body_w;
        let height = body_h as u16 + 2; // +2 border
        let area = viewport.intersection(Rect::new(
            viewport.x,
            viewport.y + viewport.height.saturating_sub(height + 1),
            viewport.width,
            height,
        ));
        surface.clear_with(area, popup_style);

        let block = Block::bordered().title(title).border_style(popup_style);

        let margin = Margin::horizontal(1);
        let inner = block.inner(area).inner(margin);
        block.render(area, surface);

        Paragraph::new(&Text::from(text.as_str()))
            .style(text_style)
            .render(inner, surface);
    }
}
