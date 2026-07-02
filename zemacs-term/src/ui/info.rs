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
/// Widest a single `KEY : description` column is allowed to grow.
const COL_CAP: usize = 48;
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
    let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let budget = max_width.saturating_sub(6); // borders + margin
    let cols_fit = (budget / (widest.clamp(8, COL_CAP) + SEP)).max(1);
    // Use as many columns as fit the full width (Spacemacs' which-key fills the
    // window — up to MAX_COLS), so the grid is wide and short; only overflow
    // scrolls.
    let cols = cols_fit.min(MAX_COLS).min(n).max(1);
    let rows_total = n.div_ceil(cols);
    let visible = rows_total.min(max_rows);
    let scroll = scroll.min(rows_total.saturating_sub(visible));

    // Column-major: column `c` holds items [c*rows_total .. (c+1)*rows_total).
    let col_w: Vec<usize> = (0..cols)
        .map(|c| {
            let s = c * rows_total;
            let e = ((c + 1) * rows_total).min(n);
            lines[s..e]
                .iter()
                .map(|l| l.chars().count())
                .max()
                .unwrap_or(0)
                .min(COL_CAP)
        })
        .collect();

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
        // Spacemacs' which-key bar. (`body_w` still drives column layout inside.)
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
