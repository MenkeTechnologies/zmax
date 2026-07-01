use crate::compositor::{Component, Context};
use tui::buffer::Buffer as Surface;
use tui::text::Text;
use tui::widgets::{Block, Paragraph, Widget};
use zemacs_view::graphics::{Margin, Rect};
use zemacs_view::info::Info;

/// Hard cap on which-key popup rows regardless of frame height — keeps a huge
/// prefix map (e.g. the emacs `C-x` tree) from filling the screen, matching
/// Spacemacs' `which-key` side window (which docks a short, multi-column grid).
const MAX_ROWS: usize = 16;

/// Reflow `lines` (each `"key  desc"`) into a column-major grid at most `rows`
/// tall, so a long list becomes a short, wide grid instead of a full-screen
/// column. Columns and per-column width are bounded so the grid fits `max_width`
/// (descriptions are truncated to their column budget). Returns the grid text
/// plus its width and height in cells.
fn reflow_columns(lines: &[&str], rows: usize, max_width: usize) -> (String, usize, usize) {
    let n = lines.len();
    let rows = rows.max(1);
    let cols = n.div_ceil(rows);
    const SEP: usize = 3; // spaces between columns
    let budget = max_width.saturating_sub(6); // borders + margin
    // Split the width budget evenly across columns, then clamp each column to
    // the width its own longest cell actually needs.
    let col_cap = (budget / cols.max(1)).saturating_sub(SEP).max(8);
    let col_w: Vec<usize> = (0..cols)
        .map(|c| {
            let s = c * rows;
            let e = ((c + 1) * rows).min(n);
            lines[s..e]
                .iter()
                .map(|l| l.chars().count())
                .max()
                .unwrap_or(0)
                .min(col_cap)
        })
        .collect();
    let real_rows = rows.min(n);
    let mut out = String::new();
    for r in 0..real_rows {
        let mut line = String::new();
        for (c, &w) in col_w.iter().enumerate() {
            let idx = c * rows + r;
            if idx >= n {
                break;
            }
            if c > 0 {
                line.push_str("   ");
            }
            let cell: String = lines[idx].chars().take(w).collect();
            line.push_str(&format!("{cell:w$}"));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    let width = col_w.iter().sum::<usize>() + SEP * cols.saturating_sub(1);
    (out, width, real_rows)
}

impl Component for Info {
    fn render(&mut self, viewport: Rect, surface: &mut Surface, cx: &mut Context) {
        let text_style = cx.editor.theme.get("ui.text.info");
        let popup_style = cx.editor.theme.get("ui.popup.info");

        // Cap the popup at ~40% of the frame height (Spacemacs-style); anything
        // taller reflows into extra columns rather than a full-screen list.
        let avail = (viewport.height as usize).saturating_sub(6);
        let cap = avail.min(MAX_ROWS).max(1);

        let lines: Vec<&str> = self.text.lines().collect();
        let (text, body_w, body_h) = if lines.len() > cap {
            reflow_columns(&lines, cap, viewport.width as usize)
        } else {
            (self.text.clone(), self.width as usize, self.height as usize)
        };

        // Calculate the area of the terminal to modify. Because we want to
        // render at the bottom right, we use the viewport's width and height
        // which evaluate to the most bottom right coordinate.
        let width = body_w as u16 + 2 + 2; // +2 for border, +2 for margin
        let height = body_h as u16 + 2; // +2 for border
        let area = viewport.intersection(Rect::new(
            viewport.width.saturating_sub(width),
            viewport.height.saturating_sub(height + 2), // +2 for statusline
            width,
            height,
        ));
        surface.clear_with(area, popup_style);

        let block = Block::bordered()
            .title(self.title.as_ref())
            .border_style(popup_style);

        let margin = Margin::horizontal(1);
        let inner = block.inner(area).inner(margin);
        block.render(area, surface);

        Paragraph::new(&Text::from(text.as_str()))
            .style(text_style)
            .render(inner, surface);
    }
}
