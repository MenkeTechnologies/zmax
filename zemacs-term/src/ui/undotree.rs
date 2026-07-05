//! Undo-tree browser — the zemacs port of vim's `undotree` plugin
//! (`:UndotreeToggle`).
//!
//! zemacs (like Helix) already stores undo history as a *branching tree*: every
//! [`Revision`](zemacs_core::history) has a parent and may have several
//! children, so redoing after undoing-then-editing does not throw away the old
//! branch — it just creates a new one. That tree is normally invisible; this
//! overlay renders it as a git-log-style graph and lets you travel to any state.
//!
//! A [`Component`] drawn as a **side panel** (like undotree.vim's split): it
//! occupies a narrow column on the right and leaves the buffer visible to its
//! left, so moving through states **live-previews** them in the actual file.
//! Newest revision on top, root (`0  (original)`) at the bottom. The left
//! gutter draws lane bars so branches are visible; the marker is `@` for the
//! current state, `*` for the saved (on-disk) state, `o` otherwise.
//!
//! Keys:
//!   j/k/n/p/arrows, g/G/Home/End — move the selection; the file jumps to that
//!                                  state immediately (live preview)
//!   Enter/Space                  — keep the previewed state and close
//!   q/Esc/C-c                    — restore the original state and close (cancel)
//!
//! All history mutation goes through [`Document::jump_to_revision`], which
//! follows the shortest tree path from the current state to the target, so
//! previewing across branches restores exactly that state and cancelling puts
//! the buffer back where it started.

use std::time::Instant;

use tui::buffer::Buffer as Surface;
use zemacs_view::{graphics::Rect, DocumentId, ViewId};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// One rendered row of the undo-tree graph. Every row is a node row (there are
/// no separate link rows); lane bars in the gutter carry the branch structure.
struct Row {
    /// Left-hand graph gutter: one `marker`/`│`/space per lane, space-separated.
    gutter: String,
    /// The revision index this row represents.
    seq: usize,
    /// Right-hand text: revision number + relative age.
    label: String,
}

pub struct UndoTree {
    /// The buffer whose history we browse (for the title / identity).
    doc_id: DocumentId,
    /// The view showing that buffer, used to apply live-preview jumps.
    view_id: ViewId,
    /// The revision the buffer was on when the panel opened; `q`/`Esc` restores
    /// it (cancel).
    original: usize,
    /// Currently highlighted revision index (also the live-previewed state).
    selected: usize,
    scroll: usize,
    viewport: usize,
}

impl UndoTree {
    /// Open the undo-tree browser for the given document/view, highlighting its
    /// current revision.
    pub fn new(doc_id: DocumentId, view_id: ViewId, current: usize) -> Self {
        UndoTree {
            doc_id,
            view_id,
            original: current,
            selected: current,
            scroll: 0,
            viewport: 1,
        }
    }

    /// Apply revision `to` to the buffer live (preview). Uses the captured
    /// doc/view ids so it always targets the browsed buffer.
    fn preview(&self, cx: &mut Context, to: usize) {
        if cx.editor.document(self.doc_id).is_none() {
            return;
        }
        let view = view_mut!(cx.editor, self.view_id);
        let doc = doc_mut!(cx.editor, &self.doc_id);
        doc.jump_to_revision(view, to);
    }
}

/// Format a duration (in whole seconds) as a compact relative age.
fn fmt_ago(secs: u64) -> String {
    match secs {
        0 => "just now".to_string(),
        s if s < 60 => format!("{s}s ago"),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// Build the graph rows (newest revision first) from a history snapshot.
///
/// `nodes[i]` is `(parent, timestamp)` for revision `i`; `nodes[0]` is the root
/// (parent 0). `now` is the instant relative to which ages are computed. The
/// returned rows are top-to-bottom (highest revision index first).
///
/// The gutter uses an incremental lane assignment (git-graph style): walking
/// revisions newest→oldest, each lane holds the revision it is waiting to draw
/// (a parent referenced by an already-drawn child). A branch point shows up as
/// two lanes collapsing into one at the parent's row.
fn build_rows(nodes: &[(usize, Instant)], current: usize, saved: usize, now: Instant) -> Vec<Row> {
    let count = nodes.len();
    let mut rows = Vec::with_capacity(count);
    // lanes[c] = the revision index this lane will draw next, if any.
    let mut lanes: Vec<Option<usize>> = Vec::new();

    for seq in (0..count).rev() {
        // Lanes already waiting to draw this revision (its children converge here).
        let waiting: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter_map(|(i, l)| (*l == Some(seq)).then_some(i))
            .collect();

        let col = if let Some(&c) = waiting.first() {
            c
        } else {
            // A tip nobody references yet: take the first free lane, else append.
            match lanes.iter().position(|l| l.is_none()) {
                Some(i) => {
                    lanes[i] = Some(seq);
                    i
                }
                None => {
                    lanes.push(Some(seq));
                    lanes.len() - 1
                }
            }
        };

        let marker = if seq == current {
            '@'
        } else if seq == saved {
            '*'
        } else {
            'o'
        };

        let width = lanes.len();
        let mut gutter = String::with_capacity(width * 2);
        for (c, lane) in lanes.iter().enumerate().take(width) {
            if c == col {
                gutter.push(marker);
            } else if lane.is_some() {
                gutter.push('│');
            } else {
                gutter.push(' ');
            }
            gutter.push(' ');
        }

        let secs = now.saturating_duration_since(nodes[seq].1).as_secs();
        let label = if seq == 0 {
            format!("{seq}  (original)")
        } else {
            format!("{seq}  {}", fmt_ago(secs))
        };
        rows.push(Row { gutter, seq, label });

        // Free the extra converging children, then point this lane at the parent.
        for &w in waiting.iter().skip(1) {
            lanes[w] = None;
        }
        lanes[col] = if seq == 0 { None } else { Some(nodes[seq].0) };
    }

    rows
}

impl Component for UndoTree {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // The buffer went away — nothing to browse.
        let Some(doc) = cx.editor.document(self.doc_id) else {
            let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
                compositor.pop();
            });
            return EventResult::Consumed(Some(close));
        };
        // Ordered revision list (top→bottom), so j/k match what's drawn.
        let seqs: Vec<usize> = build_rows(&doc.undo_tree_snapshot().nodes, 0, 0, Instant::now())
            .into_iter()
            .map(|r| r.seq)
            .collect();
        let pos = seqs.iter().position(|&s| s == self.selected).unwrap_or(0);

        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        // Move the selection and live-preview the new state in the buffer.
        let mut go = |this: &mut Self, idx: usize| {
            if let Some(&s) = seqs.get(idx) {
                if s != this.selected {
                    this.selected = s;
                    this.preview(cx, s);
                }
            }
        };

        match key {
            // Cancel: restore the original state, then close.
            key!('q') | key!(Esc) | ctrl!('c') => {
                self.preview(cx, self.original);
                return EventResult::Consumed(Some(close));
            }
            // Accept: keep the previewed state (already applied), then close.
            key!(Enter) | key!(' ') => {
                let target = self.selected;
                cx.editor
                    .set_status(format!("Undo tree: kept state {target}"));
                return EventResult::Consumed(Some(close));
            }
            key!('j') | key!(Down) | ctrl!('n') => go(self, pos.saturating_add(1)),
            key!('k') | key!(Up) | ctrl!('p') => go(self, pos.saturating_sub(1)),
            key!('g') | key!(Home) => go(self, 0),
            key!('G') | key!(End) => go(self, seqs.len().saturating_sub(1)),
            _ => {}
        }
        // Modal for key input, but the buffer underneath stays visible.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        // Draw only a right-hand panel; the editor view underneath shows through
        // on the left, so state changes are previewed live in the real file.
        let panel_w = (area.width / 3).clamp(28, area.width).min(area.width);
        if area.width < 20 || area.height < 3 {
            return;
        }
        let panel = Rect::new(area.x + area.width - panel_w, area.y, panel_w, area.height);

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.menu");
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let border_style = theme.get("ui.window");
        let sel_style = theme.get("ui.selection");
        let cur_style = theme.get("diff.plus");
        let saved_style = theme.get("warning");
        let help_style = theme.get("ui.linenr");

        surface.clear_with(panel, bg);
        // Left border so the panel reads as a separate split.
        for y in panel.y..panel.y + panel.height {
            surface.set_string(panel.x, y, "│", border_style);
        }
        let inner_x = panel.x + 2;
        let inner_w = panel.width.saturating_sub(2) as usize;

        let Some(doc) = ctx.editor.document(self.doc_id) else {
            surface.set_stringn(inner_x, panel.y, "(buffer closed)", inner_w, help_style);
            return;
        };
        let snap = doc.undo_tree_snapshot();
        let rows = build_rows(&snap.nodes, snap.current, snap.saved, Instant::now());

        let name = doc
            .path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "[scratch]".to_string());
        surface.set_stringn(
            inner_x,
            panel.y,
            &format!("Undo Tree — {name} ({})", rows.len()),
            inner_w,
            header_style,
        );
        surface.set_stringn(
            inner_x,
            panel.y + 1,
            "j/k preview · ⏎ keep · q cancel",
            inner_w,
            help_style,
        );

        let body_y = panel.y + 2;
        let list_h = panel.height.saturating_sub(2) as usize;
        self.viewport = list_h.max(1);

        // Keep the selection in view.
        let sel_row = rows
            .iter()
            .position(|r| r.seq == self.selected)
            .unwrap_or(0);
        if sel_row < self.scroll {
            self.scroll = sel_row;
        } else if sel_row >= self.scroll + self.viewport {
            self.scroll = sel_row + 1 - self.viewport;
        }

        // Widest gutter, so the labels line up in a single column.
        let gutter_w = rows
            .iter()
            .map(|r| r.gutter.chars().count())
            .max()
            .unwrap_or(0);

        for (offset, row) in rows.iter().enumerate().skip(self.scroll).take(list_h) {
            let y = body_y + (offset - self.scroll) as u16;
            let selected = row.seq == self.selected;
            let base = if selected {
                sel_style
            } else if row.seq == snap.current {
                cur_style
            } else if row.seq == snap.saved {
                saved_style
            } else {
                text_style
            };
            if selected {
                surface.clear_with(
                    Rect::new(inner_x, y, panel.width.saturating_sub(2), 1),
                    sel_style,
                );
            }
            let line = format!("{:<gw$} {}", row.gutter, row.label, gw = gutter_w);
            surface.set_stringn(inner_x, y, &line, inner_w, base);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> Instant {
        Instant::now()
    }

    #[test]
    fn linear_history_markers_and_order() {
        // 0(root) -> 1 -> 2 -> 3, current at 2, saved at 3.
        let now = ts();
        let nodes = vec![(0, now), (0, now), (1, now), (2, now)];
        let rows = build_rows(&nodes, 2, 3, now);
        // Newest first.
        let seqs: Vec<usize> = rows.iter().map(|r| r.seq).collect();
        assert_eq!(seqs, vec![3, 2, 1, 0]);
        // Markers: 3 saved '*', 2 current '@', root labeled original.
        assert!(rows[0].gutter.starts_with('*'));
        assert!(rows[1].gutter.starts_with('@'));
        assert!(rows[3].label.contains("(original)"));
        // Linear history uses a single lane.
        assert!(rows
            .iter()
            .all(|r| r.gutter.trim_end().chars().count() == 1));
    }

    #[test]
    fn branch_shows_second_lane() {
        // 0 -> 1, then two children of 1: 2 and 3 (a branch).
        //   parents: [0,0,1,1]
        let now = ts();
        let nodes = vec![(0, now), (0, now), (1, now), (1, now)];
        let rows = build_rows(&nodes, 3, 3, now);
        let seqs: Vec<usize> = rows.iter().map(|r| r.seq).collect();
        assert_eq!(seqs, vec![3, 2, 1, 0]);
        // At least one row must use a second lane (a '│' beside the marker),
        // proving the branch is visualized rather than flattened.
        assert!(
            rows.iter().any(|r| r.gutter.trim_end().chars().count() > 1),
            "branch should occupy a second lane: {:?}",
            rows.iter().map(|r| r.gutter.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn single_root_is_one_row() {
        let now = ts();
        let nodes = vec![(0, now)];
        let rows = build_rows(&nodes, 0, 0, now);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].seq, 0);
        assert!(rows[0].gutter.starts_with('@'));
    }
}
