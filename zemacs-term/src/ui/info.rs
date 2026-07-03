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

/// Lay `lines` (each `"key : desc"`) into a fixed, width-bounded column grid
/// (column-major, like Emacs' `describe-bindings`) and return the visible slice
/// starting at `scroll` rows down. Returns `(text, body_width, body_height,
/// rows_total, cols)` so the caller can size the box and decide whether a scroll
/// indicator is needed.
fn grid(
    lines: &[&str],
    scroll: usize,
    max_rows: usize,
    max_width: usize,
) -> (String, usize, usize, usize, usize) {
    let n = lines.len();
    if n == 0 {
        return (String::new(), 0, 0, 0, 1);
    }
    let budget = max_width.saturating_sub(6); // borders + margin
    let max_cols = MAX_COLS.min(n).max(1);

    // Natural (untruncated, COL_CAP-bounded) width of each column for a given
    // column count `cols`, laid out column-major.
    let nat_widths = |cols: usize| -> Vec<usize> {
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
    let total_w = |w: &[usize]| w.iter().sum::<usize>() + SEP * w.len().saturating_sub(1);

    // Prefer the MOST columns whose full-width layout fits the popup, so every
    // description is shown in full (Spacemacs' which-key fills the window). Only
    // consider column counts that leave no empty trailing column, and only when
    // even a single column overflows do we truncate it to the budget.
    let (cols, col_w) = (1..=max_cols)
        .rev()
        .filter(|&c| (c - 1) * n.div_ceil(c) < n) // every column is non-empty
        .map(|c| (c, nat_widths(c)))
        .find(|(_, w)| total_w(w) <= budget)
        .unwrap_or_else(|| (1, vec![nat_widths(1)[0].min(budget)]));

    let rows_total = n.div_ceil(cols);
    let visible = rows_total.min(max_rows);
    let scroll = scroll.min(rows_total.saturating_sub(visible));

    let mut out = String::new();
    for r in scroll..scroll + visible {
        let mut line = String::new();
        for (c, &w) in col_w.iter().enumerate() {
            let idx = c * rows_total + r;
            if idx >= n {
                break;
            }
            if c > 0 {
                line.push_str(&" ".repeat(SEP));
            }
            let cell: String = lines[idx].chars().take(w).collect();
            line.push_str(&format!("{cell:w$}"));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    let width = col_w.iter().sum::<usize>() + SEP * cols.saturating_sub(1);
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
        assert!(width <= 30 - 6, "column must be bounded by the budget: {width}");
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

        // Size the box to the grid content, anchored at the bottom-left (above
        // the statusline). A narrow grid — e.g. one wide column when long
        // descriptions can't fit two — must NOT leave a full-width bar of dead
        // space. Stay wide enough for the title, and never wider than the frame.
        let title_w = title.chars().count() + 2; // border corners
        let box_w = (body_w + 4) // borders (2) + horizontal margin (2)
            .max(title_w)
            .min(viewport.width as usize) as u16;
        let height = body_h as u16 + 2; // +2 border
        let area = viewport.intersection(Rect::new(
            viewport.x,
            viewport.y + viewport.height.saturating_sub(height + 1),
            box_w,
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
