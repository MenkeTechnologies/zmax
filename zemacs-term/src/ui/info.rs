use crate::compositor::{Component, Context};
use tui::buffer::Buffer as Surface;
use tui::text::Text;
use tui::widgets::{Paragraph, Widget};
use zemacs_view::graphics::{Margin, Rect};
use zemacs_view::info::Info;

/// Hard cap on which-key popup rows regardless of frame height — keeps a huge
/// prefix map (e.g. the emacs/Spacemacs `C-x` tree) from filling the screen.
/// When a map has more entries than fit (cols × this), the popup becomes
/// vertically scrollable (PgDn/PgUp or the mouse wheel; see `Info::scroll`).
const MAX_ROWS: usize = 16;
/// Width used when DECIDING the column count: a column is treated as at most this
/// wide (like Spacemacs' `which-key-max-description-length`) so one long entry
/// cannot dominate the grid and collapse the column count — the grid stays packed
/// into several width-driven columns. It is only a count-selection cap: once the
/// count is chosen, columns grow past it toward their natural width to fill the
/// bar (see `grid`), so leftover space shows more text rather than truncating.
const COL_CAP: usize = 34;
/// Max columns the which-key grid fills across the width (Spacemacs uses up to 8).
const MAX_COLS: usize = 8;
/// Spaces between columns.
const SEP: usize = 3;

/// Lay `lines` (each `"key : desc"`) into a column-major grid (like Emacs'
/// `describe-bindings`) and return the visible slice starting at `scroll` rows
/// down. The column count (1..=8) is driven by the screen width — as many columns
/// as fit at each column's `COL_CAP`-capped content width, so one long entry can't
/// collapse the count. Leftover width is filled in two passes: first the COLUMN
/// WIDTHS grow toward their natural, untruncated width (so long menus show more
/// text and their gaps stay tight — no mid-bar chasm), then any residual is spread
/// EVENLY across the gaps (so short-label menus fan out edge-to-edge instead of
/// leaving dead space on the right). A `>=2`-column grid therefore always spans
/// the full bar. Returns `(text, body_width, body_height, rows_total, cols)`;
/// `body_width` is the full inner width the grid spans.
fn grid(
    lines: &[&str],
    scroll: usize,
    max_rows: usize,
    max_width: usize,
) -> (String, usize, usize, usize, usize) {
    let n = lines.len();
    let budget = max_width.saturating_sub(2).max(1); // borderless: 1-col margin each side
    if n == 0 {
        return (String::new(), budget, 0, 0, 1);
    }
    let max_cols = MAX_COLS.min(n).max(1);

    // Width of column `c` under a given column count, either capped at `COL_CAP`
    // (for count selection: a long entry can't widen its column and collapse the
    // count) or at its natural, untruncated content width (for display).
    let col_width = |cols: usize, c: usize, cap: usize| -> usize {
        let rows = n.div_ceil(cols);
        let s = (c * rows).min(n);
        let e = ((c + 1) * rows).min(n);
        lines[s..e]
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
            .min(cap)
    };
    // The MOST columns whose COL_CAP-capped widths fit the bar, leaving no empty
    // trailing column. Falls back to a single budget-bounded column.
    let capped =
        |cols: usize| -> Vec<usize> { (0..cols).map(|c| col_width(cols, c, COL_CAP)).collect() };
    let (cols, mut cw) = (1..=max_cols)
        .rev()
        .filter(|&c| (c - 1) * n.div_ceil(c) < n)
        .map(|c| (c, capped(c)))
        .find(|(_, w)| w.iter().sum::<usize>() + SEP * w.len().saturating_sub(1) <= budget)
        .unwrap_or_else(|| (1, vec![capped(1)[0].min(budget)]));

    let rows_total = n.div_ceil(cols);
    let visible = rows_total.min(max_rows);
    let scroll = scroll.min(rows_total.saturating_sub(visible));

    // Fill the whole bar with NO dead space, in two passes:
    //   1. Grow each column from its capped width toward its NATURAL (untruncated)
    //      width. Long-description menus (e.g. the file menu) soak up the leftover
    //      here, so more text is shown and the gaps below stay at `SEP` — this is
    //      what stops the mid-bar chasm a 2-column menu used to open by dumping all
    //      leftover into its single gap.
    //   2. Distribute whatever leftover remains (short-label menus can't grow past
    //      their content) EVENLY across the inter-column gaps, so many short columns
    //      spread edge-to-edge instead of clustering on the left with dead space on
    //      the right. With growth done first, the residual is small, so the gaps
    //      never blow up into a chasm.
    let gaps = cols.saturating_sub(1);
    let natural: Vec<usize> = (0..cols).map(|c| col_width(cols, c, usize::MAX)).collect();
    let mut leftover = budget.saturating_sub(cw.iter().sum::<usize>() + SEP * gaps);
    let mut growing = true;
    while leftover > 0 && growing {
        growing = false;
        for c in 0..cols {
            if leftover == 0 {
                break;
            }
            if cw[c] < natural[c] {
                cw[c] += 1;
                leftover -= 1;
                growing = true;
            }
        }
    }
    // Pass 2: residual leftover spread evenly over the gaps (base + a 1-col extra
    // for the first `gap_extra` gaps). One column has no gaps, so its short content
    // trails into uniform background — the only case the width can't be filled.
    let gap_base = leftover.checked_div(gaps).map_or(0, |q| SEP + q);
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

        // Borderless, full editor width, anchored at the bottom (above the
        // statusline) — Spacemacs' which-key bar has no box, the content sits
        // flush against the modeline. `clear_with` paints the whole bar in the
        // popup background, so width past the grid is uniform bg, not a gap; the
        // grid grows its columns (not its gaps) so it never leaves a mid-bar chasm.
        let _ = body_w;
        let height = body_h as u16;
        let area = viewport.intersection(Rect::new(
            viewport.x,
            viewport.y + viewport.height.saturating_sub(height + 1),
            viewport.width,
            height,
        ));
        surface.clear_with(area, popup_style);

        // One column of horizontal padding so content isn't jammed on the edge.
        let inner = area.inner(Margin::horizontal(1));
        Paragraph::new(&Text::from(text.as_str()))
            .style(text_style)
            .render(inner, surface);

        // With no title bar to host it, surface a compact scroll indicator at the
        // top-right only when the map overflows (PgDn/PgUp / wheel still scroll).
        if scrollable && area.height > 0 {
            let pct = (self.scroll as usize * 100)
                .checked_div(max_scroll)
                .unwrap_or(0);
            let ind = format!(" {pct}%  PgDn/PgUp ");
            let w = ind.chars().count() as u16;
            if area.width > w {
                surface.set_string(area.x + area.width - w, area.y, &ind, popup_style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_full_width_1_to_8_cols_by_screen_width() {
        // The which-key bar must never leave dead space and must use 1..=8 columns
        // driven by screen width. Sweep realistic menus across widths and assert:
        //   * cols always in 1..=8,
        //   * cols never shrinks as the bar widens (responsive to width),
        //   * no row overruns the bar,
        //   * with >=2 columns the grid spans the FULL width (its widest row equals
        //     the budget) — the last column always holds its widest entry at full
        //     column width, and the gap distribution makes that row sum to budget,
        //     so there is zero right-edge dead space and no mid-bar chasm.
        // One column is exempt (no gap to spread into, so short content trails into
        // uniform background).
        let long: Vec<String> = (0..25)
            .map(|i| match i % 5 {
                0 => format!("{i:>2} : +grp"), // some short entries, like a real menu
                _ => format!("{i:>2} : a reasonably long which-key description here"),
            })
            .collect();
        let short: Vec<String> = (0..40).map(|i| format!("{i:>2} : +grp")).collect();
        for menu in [&long, &short] {
            let refs: Vec<&str> = menu.iter().map(String::as_str).collect();
            let mut prev_cols = 0;
            for w in [40usize, 60, 90, 120, 160, 220, 300] {
                let (text, budget, _h, _r, cols) = grid(&refs, 0, 16, w);
                assert!((1..=8).contains(&cols), "w={w}: cols {cols} out of 1..=8");
                assert!(cols >= prev_cols, "w={w}: cols shrank ({prev_cols} -> {cols})");
                prev_cols = cols;
                for l in text.lines() {
                    assert!(l.chars().count() <= budget, "w={w}: row overruns bar: {l:?}");
                }
                if cols >= 2 {
                    let widest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
                    assert_eq!(widest, budget, "w={w} cols={cols}: dead space (widest {widest} != budget {budget})");
                }
            }
        }
    }

    #[test]
    fn col_cap_governs_count_not_display_width() {
        // COL_CAP caps the width used to CHOOSE the column count so a long entry
        // can't collapse the grid to one column — but once the count is chosen the
        // columns grow toward their natural width, so with room to spare the long
        // description is shown in full (no gratuitous truncation) and every row
        // still fits the bar.
        let a = "i : Ask the AI provider about the selection/buffer text"; // > COL_CAP
        let b = "k : Generate a shell command from natural language help";
        let lines = vec![a, b];
        let (text, width, _h, _rows, cols) = grid(&lines, 0, 16, 220);
        // A wide bar keeps two columns (the long entry didn't collapse the count)...
        assert_eq!(cols, 2, "long entry collapsed the column count: {text:?}");
        // ...and there is room, so the full description is shown, not cut off.
        assert!(text.contains("selection/buffer text"), "cut off: {text:?}");
        // Every rendered row still fits the bar.
        for line in text.lines() {
            assert!(line.chars().count() <= width, "row overruns bar: {line:?}");
        }
    }

    #[test]
    fn leftover_grows_columns_instead_of_a_mid_bar_gap() {
        // The regression this fixes: a 2-column menu of long descriptions on a
        // narrower bar used to dump all leftover width into its single inter-column
        // gap, opening a chasm down the middle. The leftover must go into the
        // column widths (kept at SEP gaps), so no row contains a run of spaces
        // wider than a normal alignment pad.
        let lines: Vec<String> = (0..24)
            .map(|i| format!("{i:>2} : open the thing and do a fairly long action"))
            .collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (text, _w, _h, _r, cols) = grid(&refs, 0, 16, 92);
        assert_eq!(cols, 2, "expected the 2-column layout that used to chasm");
        // No inter-column run of spaces wider than SEP + a short label pad. The old
        // code produced ~20-space gaps here; the fix keeps them at SEP.
        for line in text.lines() {
            let max_run = line.split(|c| c != ' ').map(str::len).max().unwrap_or(0);
            assert!(
                max_run <= SEP + 2,
                "mid-bar gap of {max_run} spaces: {line:?}"
            );
        }
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
        assert_eq!(width, 30 - 2, "the bar spans the full inner width");
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
        assert_eq!(width, 110 - 2);
    }

    #[test]
    fn grid_reports_the_full_inner_width_and_bounded_cols() {
        // Whatever the entry lengths or column count, the reported body width is
        // the full inner span (body_width == budget) — the bar is cleared to that
        // width in the popup bg, so unused right-edge space is uniform, not a gap —
        // and the column count stays within 1..=8.
        for &(w, count, desc_len) in &[(200usize, 20usize, 24usize), (90, 16, 78), (300, 40, 12)] {
            let lines: Vec<String> = (0..count)
                .map(|i| format!("{i} : {}", "x".repeat(desc_len)))
                .collect();
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            let (_t, width, _h, _r, cols) = grid(&refs, 0, 16, w);
            assert_eq!(
                width,
                w - 2,
                "grid must span the full inner width for w={w}"
            );
            assert!((1..=8).contains(&cols), "cols out of 1..=8: {cols}");
        }
    }
}
