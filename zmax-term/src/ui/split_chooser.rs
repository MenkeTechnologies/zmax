//! Split chooser — the zmax port of JetBrains "Open in Split with Chooser"
//! (`SplitChooser`): pick where the current file goes among the window
//! splits, then move or copy it there.
//!
//! `:split-chooser` opens it over the current window. A highlighted drop zone
//! shows the target: a whole window, or the half of one that a new split
//! would take.
//!
//! Keys:
//!   Left/Right/Up/Down, h/l/k/j  use the window on that side, or, when there
//!                                is none, propose a new split there; the
//!                                opposite direction takes a proposed split back
//!   Tab / Shift-Tab              the next / previous window, without a split
//!   c                            the chosen window itself, without a split
//!   Enter                        move the file there: the window it came from
//!                                shows its previous buffer, or closes if it
//!                                has none
//!   d                            open it there and keep it where it was too
//!   Esc / q / C-g                leave without changing anything

use tui::buffer::Buffer as Surface;
use zmax_view::editor::Action;
use zmax_view::graphics::Rect;
use zmax_view::tree::{Direction, Tree};
use zmax_view::{Editor, ViewId};

use crate::{
    compositor::{Component, Context, Event, EventResult},
    ctrl, key, shift,
};

/// The interactive chooser overlay.
#[derive(Clone, Copy)]
pub struct SplitChooser {
    /// The window whose file is being placed.
    source: ViewId,
    /// The window the drop zone is on.
    target: ViewId,
    /// The side of `target` a new split would take; `None` is the window itself.
    side: Option<Direction>,
}

fn opposite(dir: Direction) -> Direction {
    match dir {
        Direction::Left => Direction::Right,
        Direction::Right => Direction::Left,
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
    }
}

/// The part of `area` a split on `side` would take: the whole of it for none.
fn zone(area: Rect, side: Option<Direction>) -> Rect {
    let (half_w, half_h) = (area.width / 2, area.height / 2);
    match side {
        None => area,
        Some(Direction::Left) => Rect::new(area.x, area.y, half_w, area.height),
        Some(Direction::Right) => {
            Rect::new(area.x + area.width - half_w, area.y, half_w, area.height)
        }
        Some(Direction::Up) => Rect::new(area.x, area.y, area.width, half_h),
        Some(Direction::Down) => {
            Rect::new(area.x, area.y + area.height - half_h, area.width, half_h)
        }
    }
}

impl SplitChooser {
    /// A chooser placing the file of `view`, starting on that window.
    pub fn new(view: ViewId) -> Self {
        SplitChooser {
            source: view,
            target: view,
            side: None,
        }
    }

    /// JetBrains "Use Left Split or Switch Left" and its siblings: move onto
    /// the window on `dir`'s side, propose a new split there when there is no
    /// such window, or take back a split proposed on the opposite side.
    fn step(&mut self, tree: &Tree, dir: Direction) {
        self.side = match self.side {
            Some(side) if side == opposite(dir) => None,
            Some(_) => Some(dir),
            None => match tree.find_split_in_direction(self.target, dir) {
                Some(next) => {
                    self.target = next;
                    None
                }
                None => Some(dir),
            },
        };
    }

    /// The next (or previous) window in traversal order, without a split.
    fn cycle(&mut self, tree: &Tree, forward: bool) {
        let ids = tree.leaf_ids();
        let Some(at) = ids.iter().position(|&id| id == self.target) else {
            return;
        };
        let n = ids.len();
        self.target = ids[if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }];
        self.side = None;
    }

    /// Show the source window's file at the chosen place; unless `keep`, take
    /// it out of the source window.
    fn apply(&self, editor: &mut Editor, keep: bool) {
        if self.side.is_none() && self.target == self.source {
            return;
        }
        let (Some(source), true) = (
            editor.tree.try_get(self.source),
            editor.tree.contains(self.target),
        ) else {
            return;
        };
        let doc_id = source.doc;
        let Some(doc) = editor.document(doc_id) else {
            return;
        };
        let selection = doc.selection(self.source).clone();
        let offset = doc.view_offset(self.source);

        editor.focus(self.target);
        match self.side {
            None => editor.switch(doc_id, Action::Replace),
            Some(dir) => {
                let action = match dir {
                    Direction::Left | Direction::Right => Action::VerticalSplit,
                    Direction::Up | Direction::Down => Action::HorizontalSplit,
                };
                editor.switch(doc_id, action);
                // `split-right` / `split-below` decide which side a split
                // lands on; swap it over when that is not the chosen one.
                let (old, new) = (
                    editor.tree.get(self.target).area,
                    editor.tree.get(editor.tree.focus).area,
                );
                let landed_after = match dir {
                    Direction::Left | Direction::Right => new.x > old.x,
                    Direction::Up | Direction::Down => new.y > old.y,
                };
                if landed_after != matches!(dir, Direction::Right | Direction::Down) {
                    editor.tree.swap_split_in_direction(dir);
                }
            }
        }
        let placed = editor.tree.focus;
        if let Some(doc) = editor.document_mut(doc_id) {
            doc.set_selection(placed, selection);
            doc.set_view_offset(placed, offset);
        }

        if keep || placed == self.source || !editor.tree.contains(self.source) {
            return;
        }
        let previous = editor
            .tree
            .get(self.source)
            .docs_access_history
            .iter()
            .rev()
            .copied()
            .find(|&d| d != doc_id && editor.document(d).is_some());
        match previous {
            Some(prev) => {
                editor.focus(self.source);
                editor.switch(prev, Action::Replace);
            }
            None => editor.close(self.source),
        }
        editor.focus(placed);
    }

    /// Where the file would go, for the zone's label.
    fn describe(&self) -> &'static str {
        match self.side {
            None => "this window",
            Some(Direction::Left) => "split left",
            Some(Direction::Right) => "split right",
            Some(Direction::Up) => "split up",
            Some(Direction::Down) => "split down",
        }
    }
}

impl Component for SplitChooser {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        let tree = &cx.editor.tree;
        let done = |apply: Option<(SplitChooser, bool)>| {
            EventResult::Consumed(Some(Box::new(move |compositor, cx| {
                compositor.pop();
                if let Some((chooser, keep)) = apply {
                    chooser.apply(cx.editor, keep);
                }
            })))
        };
        match *key {
            key!(Left) | key!('h') => self.step(tree, Direction::Left),
            key!(Right) | key!('l') => self.step(tree, Direction::Right),
            key!(Up) | key!('k') => self.step(tree, Direction::Up),
            key!(Down) | key!('j') => self.step(tree, Direction::Down),
            key!(Tab) => self.cycle(tree, true),
            shift!(Tab) => self.cycle(tree, false),
            key!('c') => self.side = None,
            key!(Enter) => return done(Some((*self, false))),
            key!('d') => return done(Some((*self, true))),
            key!(Esc) | key!('q') | ctrl!('g') => return done(None),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, _area: Rect, surface: &mut Surface, cx: &mut Context) {
        let Some(view) = cx.editor.tree.try_get(self.target) else {
            return;
        };
        let theme = &cx.editor.theme;
        let zone = zone(view.area, self.side);
        surface.set_style(zone, theme.get("ui.selection"));
        let label = format!(
            " {} · arrows choose · Enter move · d duplicate · Esc ",
            self.describe()
        );
        surface.set_stringn(
            zone.x,
            zone.y + zone.height / 2,
            &label,
            zone.width as usize,
            theme.get("ui.text.focus"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zmax_view::editor::GutterConfig;
    use zmax_view::tree::Layout;
    use zmax_view::{DocumentId, View};

    /// Two windows side by side; returns the tree and the (left, right) ids.
    fn side_by_side() -> (Tree, ViewId, ViewId) {
        let mut tree = Tree::new(Rect::new(0, 0, 80, 20));
        let left = tree.insert(View::new(DocumentId::default(), GutterConfig::default()));
        let right = tree.split_with(
            View::new(DocumentId::default(), GutterConfig::default()),
            Layout::Vertical,
            false,
        );
        tree.focus = left;
        (tree, left, right)
    }

    #[test]
    fn arrows_switch_to_a_neighbour_or_propose_a_split() {
        let (tree, left, right) = side_by_side();
        let mut ch = SplitChooser::new(left);

        // A window on the right: switch onto it.
        ch.step(&tree, Direction::Right);
        assert_eq!((ch.target, ch.side), (right, None));
        // Nothing further right: propose a new split there.
        ch.step(&tree, Direction::Right);
        assert_eq!((ch.target, ch.side), (right, Some(Direction::Right)));
        // The opposite arrow takes the proposal back.
        ch.step(&tree, Direction::Left);
        assert_eq!((ch.target, ch.side), (right, None));
        // Nothing above: a split above; then down turns it back.
        ch.step(&tree, Direction::Up);
        assert_eq!(ch.side, Some(Direction::Up));
        ch.step(&tree, Direction::Down);
        assert_eq!(ch.side, None);

        ch.side = Some(Direction::Down);
        ch.cycle(&tree, true);
        assert_eq!((ch.target, ch.side), (left, None), "Tab wraps, unsplit");
        ch.cycle(&tree, false);
        assert_eq!(ch.target, right);
    }

    #[test]
    fn the_drop_zone_is_the_half_a_split_would_take() {
        let a = Rect::new(10, 2, 41, 21);
        assert_eq!(zone(a, None), a);
        assert_eq!(zone(a, Some(Direction::Left)), Rect::new(10, 2, 20, 21));
        assert_eq!(zone(a, Some(Direction::Right)), Rect::new(31, 2, 20, 21));
        assert_eq!(zone(a, Some(Direction::Up)), Rect::new(10, 2, 41, 10));
        assert_eq!(zone(a, Some(Direction::Down)), Rect::new(10, 13, 41, 10));
    }
}
