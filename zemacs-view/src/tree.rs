use crate::{graphics::Rect, DocumentId, View, ViewId};
use slotmap::SlotMap;

/// A structural snapshot of a window layout, used by tabpages to save a tab's
/// splits and rebuild them later. Leaves carry only the `DocumentId`; the
/// editor restores each window's selection separately, matching the
/// left-to-right leaf order of `shape()`/`build_from_shape()`.
#[derive(Debug, Clone)]
pub enum TreeShape {
    Leaf {
        doc: DocumentId,
        focused: bool,
    },
    Split {
        layout: Layout,
        children: Vec<(f32, TreeShape)>,
    },
}

// the dimensions are recomputed on window resize/tree change.
//
#[derive(Debug)]
pub struct Tree {
    root: ViewId,
    // (container, index inside the container)
    pub focus: ViewId,
    // fullscreen: bool,
    area: Rect,

    nodes: SlotMap<ViewId, Node>,

    // used for traversals
    stack: Vec<(ViewId, Rect)>,
}

#[derive(Debug)]
pub struct Node {
    parent: ViewId,
    content: Content,
    /// Relative size weight within the parent container. Siblings split the
    /// container in proportion to their weights. Defaults to 1.0 (equal split);
    /// dragging a pane divider rewrites the two neighbours' weights.
    weight: f32,
}

#[derive(Debug)]
pub enum Content {
    View(Box<View>),
    Container(Box<Container>),
}

impl Node {
    pub fn container(layout: Layout) -> Self {
        Self {
            parent: ViewId::default(),
            content: Content::Container(Box::new(Container::new(layout))),
            weight: 1.0,
        }
    }

    pub fn view(view: View) -> Self {
        Self {
            parent: ViewId::default(),
            content: Content::View(Box::new(view)),
            weight: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Horizontal,
    Vertical,
    // could explore stacked/tabbed
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug)]
pub struct Container {
    layout: Layout,
    children: Vec<ViewId>,
    area: Rect,
}

impl Container {
    pub fn new(layout: Layout) -> Self {
        Self {
            layout,
            children: Vec::new(),
            area: Rect::default(),
        }
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new(Layout::Vertical)
    }
}

impl Tree {
    pub fn new(area: Rect) -> Self {
        let root = Node::container(Layout::Vertical);

        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(root);

        // root is it's own parent
        nodes[root].parent = root;

        Self {
            root,
            focus: root,
            // fullscreen: false,
            area,
            nodes,
            stack: Vec::new(),
        }
    }

    /// Serialize the current layout into a [`TreeShape`] (for tabpages).
    pub fn shape(&self) -> TreeShape {
        self.shape_of(self.root)
    }

    fn shape_of(&self, id: ViewId) -> TreeShape {
        match &self.nodes[id].content {
            Content::View(view) => TreeShape::Leaf {
                doc: view.doc,
                focused: id == self.focus,
            },
            Content::Container(container) => TreeShape::Split {
                layout: container.layout,
                children: container
                    .children
                    .iter()
                    .map(|&ch| (self.nodes[ch].weight, self.shape_of(ch)))
                    .collect(),
            },
        }
    }

    /// Leaf (window) ViewIds in left-to-right order — the same order in which
    /// `shape()` emits leaves, so the two can be zipped.
    pub fn leaf_ids(&self) -> Vec<ViewId> {
        let mut out = Vec::new();
        self.collect_leaves(self.root, &mut out);
        out
    }

    fn collect_leaves(&self, id: ViewId, out: &mut Vec<ViewId>) {
        match &self.nodes[id].content {
            Content::View(_) => out.push(id),
            Content::Container(container) => {
                for &ch in &container.children {
                    self.collect_leaves(ch, out);
                }
            }
        }
    }

    /// Replace the entire layout with one rebuilt from `shape`, minting fresh
    /// views via `make_view`. Returns the new leaf ViewIds in left-to-right
    /// order (matching `shape`'s leaves) so the caller can restore per-window
    /// state. Focus is set to the leaf marked `focused` (else the first leaf).
    /// The tree's `area` is preserved, so sizes recompute to the current frame.
    pub fn build_from_shape(
        &mut self,
        shape: &TreeShape,
        make_view: &mut dyn FnMut(DocumentId) -> View,
    ) -> Vec<ViewId> {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(Node::container(Layout::Vertical));
        nodes[root].parent = root;
        self.nodes = nodes;
        self.root = root;
        self.focus = root;

        let mut leaves = Vec::new();
        let mut focused = None;
        match shape {
            TreeShape::Split { layout, children } => {
                if let Node {
                    content: Content::Container(container),
                    ..
                } = &mut self.nodes[root]
                {
                    container.layout = *layout;
                }
                for (weight, child) in children {
                    let id = self.build_node(root, child, make_view, &mut leaves, &mut focused);
                    self.nodes[id].weight = *weight;
                }
            }
            TreeShape::Leaf { .. } => {
                self.build_node(root, shape, make_view, &mut leaves, &mut focused);
            }
        }
        self.focus = focused.or_else(|| leaves.first().copied()).unwrap_or(root);
        self.recalculate();
        leaves
    }

    fn build_node(
        &mut self,
        parent: ViewId,
        shape: &TreeShape,
        make_view: &mut dyn FnMut(DocumentId) -> View,
        leaves: &mut Vec<ViewId>,
        focused: &mut Option<ViewId>,
    ) -> ViewId {
        match shape {
            TreeShape::Leaf {
                doc,
                focused: is_focus,
            } => {
                let view = make_view(*doc);
                let id = self.nodes.insert(Node::view(view));
                self.nodes[id].parent = parent;
                if let Node {
                    content: Content::View(v),
                    ..
                } = &mut self.nodes[id]
                {
                    v.id = id;
                }
                self.push_child(parent, id);
                leaves.push(id);
                if *is_focus {
                    *focused = Some(id);
                }
                id
            }
            TreeShape::Split { layout, children } => {
                let cid = self.nodes.insert(Node::container(*layout));
                self.nodes[cid].parent = parent;
                self.push_child(parent, cid);
                for (weight, child) in children {
                    let id = self.build_node(cid, child, make_view, leaves, focused);
                    self.nodes[id].weight = *weight;
                }
                cid
            }
        }
    }

    fn push_child(&mut self, parent: ViewId, child: ViewId) {
        if let Node {
            content: Content::Container(container),
            ..
        } = &mut self.nodes[parent]
        {
            container.children.push(child);
        }
    }

    pub fn insert(&mut self, view: View) -> ViewId {
        let focus = self.focus;
        let parent = self.nodes[focus].parent;
        let mut node = Node::view(view);
        node.parent = parent;
        let node = self.nodes.insert(node);
        self.get_mut(node).id = node;

        let container = match &mut self.nodes[parent] {
            Node {
                content: Content::Container(container),
                ..
            } => container,
            _ => unreachable!(),
        };

        // insert node after the current item if there is children already
        let pos = if container.children.is_empty() {
            0
        } else {
            let pos = container
                .children
                .iter()
                .position(|&child| child == focus)
                .unwrap();
            pos + 1
        };

        container.children.insert(pos, node);
        // focus the new node
        self.focus = node;

        // recalculate all the sizes
        self.recalculate();

        node
    }

    pub fn split(&mut self, view: View, layout: Layout) -> ViewId {
        self.split_with(view, layout, false)
    }

    /// Split, placing the new view before (left/above) or after (right/below) the
    /// focused one. `before` backs vim `nosplitright`/`nosplitbelow`; the default
    /// (`false`, after) preserves the historical behavior.
    pub fn split_with(&mut self, view: View, layout: Layout, before: bool) -> ViewId {
        let focus = self.focus;
        let parent = self.nodes[focus].parent;

        let node = Node::view(view);
        let node = self.nodes.insert(node);
        self.get_mut(node).id = node;

        let container = match &mut self.nodes[parent] {
            Node {
                content: Content::Container(container),
                ..
            } => container,
            _ => unreachable!(),
        };
        if container.layout == layout {
            // insert node before/after the current item, if there are children.
            let pos = if container.children.is_empty() {
                0
            } else {
                let pos = container
                    .children
                    .iter()
                    .position(|&child| child == focus)
                    .unwrap();
                if before {
                    pos
                } else {
                    pos + 1
                }
            };
            container.children.insert(pos, node);
            self.nodes[node].parent = parent;
        } else {
            let mut split = Node::container(layout);
            split.parent = parent;
            let split = self.nodes.insert(split);

            let container = match &mut self.nodes[split] {
                Node {
                    content: Content::Container(container),
                    ..
                } => container,
                _ => unreachable!(),
            };
            if before {
                container.children.push(node);
                container.children.push(focus);
            } else {
                container.children.push(focus);
                container.children.push(node);
            }
            self.nodes[focus].parent = split;
            self.nodes[node].parent = split;

            let container = match &mut self.nodes[parent] {
                Node {
                    content: Content::Container(container),
                    ..
                } => container,
                _ => unreachable!(),
            };

            let pos = container
                .children
                .iter()
                .position(|&child| child == focus)
                .unwrap();

            // replace focus on parent with split
            container.children[pos] = split;
        }

        // focus the new node
        self.focus = node;

        // recalculate all the sizes
        self.recalculate();

        node
    }

    /// Get a mutable reference to a [Container] by index.
    /// # Panics
    /// Panics if `index` is not in self.nodes, or if the node's content is not a [Content::Container].
    fn container_mut(&mut self, index: ViewId) -> &mut Container {
        match &mut self.nodes[index] {
            Node {
                content: Content::Container(container),
                ..
            } => container,
            _ => unreachable!(),
        }
    }

    fn remove_or_replace(&mut self, child: ViewId, replacement: Option<ViewId>) {
        let parent = self.nodes[child].parent;

        self.nodes.remove(child);

        let container = self.container_mut(parent);
        let pos = container
            .children
            .iter()
            .position(|&item| item == child)
            .unwrap();

        if let Some(new) = replacement {
            container.children[pos] = new;
            self.nodes[new].parent = parent;
        } else {
            container.children.remove(pos);
        }
    }

    pub fn remove(&mut self, index: ViewId) {
        if self.focus == index {
            // focus on something else
            self.focus = self.prev();
        }

        let parent = self.nodes[index].parent;
        let parent_is_root = parent == self.root;

        self.remove_or_replace(index, None);

        let parent_container = self.container_mut(parent);
        if parent_container.children.len() == 1 && !parent_is_root {
            // Lets merge the only child back to its grandparent so that Views
            // are equally spaced.
            let sibling = parent_container.children.pop().unwrap();
            self.remove_or_replace(parent, Some(sibling));
        }

        self.recalculate()
    }

    pub fn views(&self) -> impl Iterator<Item = (&View, bool)> {
        let focus = self.focus;
        self.nodes.iter().filter_map(move |(key, node)| match node {
            Node {
                content: Content::View(view),
                ..
            } => Some((view.as_ref(), focus == key)),
            _ => None,
        })
    }

    pub fn views_mut(&mut self) -> impl Iterator<Item = (&mut View, bool)> {
        let focus = self.focus;
        self.nodes
            .iter_mut()
            .filter_map(move |(key, node)| match node {
                Node {
                    content: Content::View(view),
                    ..
                } => Some((view.as_mut(), focus == key)),
                _ => None,
            })
    }

    /// Get reference to a [View] by index.
    /// # Panics
    ///
    /// Panics if `index` is not in self.nodes, or if the node's content is not [Content::View]. This can be checked with [Self::contains].
    pub fn get(&self, index: ViewId) -> &View {
        self.try_get(index).unwrap()
    }

    /// Try to get reference to a [View] by index. Returns `None` if node content is not a [`Content::View`].
    ///
    /// Does not panic if the view does not exists anymore.
    pub fn try_get(&self, index: ViewId) -> Option<&View> {
        match self.nodes.get(index) {
            Some(Node {
                content: Content::View(view),
                ..
            }) => Some(view),
            _ => None,
        }
    }

    /// Get a mutable reference to a [View] by index.
    /// # Panics
    ///
    /// Panics if `index` is not in self.nodes, or if the node's content is not [Content::View]. This can be checked with [Self::contains].
    pub fn get_mut(&mut self, index: ViewId) -> &mut View {
        match &mut self.nodes[index] {
            Node {
                content: Content::View(view),
                ..
            } => view,
            _ => unreachable!(),
        }
    }

    /// Check if tree contains a [Node] with a given index.
    pub fn contains(&self, index: ViewId) -> bool {
        self.nodes.contains_key(index)
    }

    pub fn is_empty(&self) -> bool {
        match &self.nodes[self.root] {
            Node {
                content: Content::Container(container),
                ..
            } => container.children.is_empty(),
            _ => unreachable!(),
        }
    }

    pub fn resize(&mut self, area: Rect) -> bool {
        if self.area != area {
            self.area = area;
            self.recalculate();
            return true;
        }
        false
    }

    pub fn recalculate(&mut self) {
        if self.is_empty() {
            // There are no more views, so the tree should focus itself again.
            self.focus = self.root;

            return;
        }

        self.stack.push((self.root, self.area));

        // take the area
        // fetch the node
        // a) node is view, give it whole area
        // b) node is container, calculate areas for each child and push them on the stack

        while let Some((key, area)) = self.stack.pop() {
            // First record this node's own area, then (for containers) gather the
            // layout + children so sibling weights can be read without holding a
            // mutable borrow of `self.nodes`.
            let layout_children = match &mut self.nodes[key].content {
                Content::View(view) => {
                    view.area = area;
                    None
                }
                Content::Container(container) => {
                    container.area = area;
                    Some((container.layout, container.children.clone()))
                }
            };

            let Some((layout, children)) = layout_children else {
                continue;
            };

            let len = children.len();
            if len == 0 {
                continue;
            }

            // Per-child size weights (default 1.0 → equal split). Total is clamped
            // away from zero so a degenerate all-zero set still divides evenly.
            let weights: Vec<f32> = children
                .iter()
                .map(|child| self.nodes[*child].weight.max(0.0))
                .collect();
            let total: f32 = {
                let sum: f32 = weights.iter().sum();
                if sum > f32::EPSILON {
                    sum
                } else {
                    len as f32
                }
            };

            match layout {
                Layout::Horizontal => {
                    let mut child_y = area.y;
                    for (i, child) in children.iter().enumerate() {
                        // The last child absorbs any rounding remainder.
                        let height = if i == len - 1 {
                            (area.y + area.height).saturating_sub(child_y)
                        } else {
                            // floor (truncate) so the last child absorbs the remainder,
                            // matching the original equal-split behaviour.
                            (area.height as f32 * (weights[i] / total)) as u16
                        };
                        let child_area = Rect::new(area.x, child_y, area.width, height);
                        child_y = child_y.saturating_add(height);
                        self.stack.push((*child, child_area));
                    }
                }
                Layout::Vertical => {
                    let len_u16 = len as u16;
                    let inner_gap = 1u16;
                    let total_gap = inner_gap * len_u16.saturating_sub(2);
                    let used_area = area.width.saturating_sub(total_gap);

                    let mut child_x = area.x;
                    for (i, child) in children.iter().enumerate() {
                        // The last child absorbs rounding + gap remainder.
                        let width = if i == len - 1 {
                            (area.x + area.width).saturating_sub(child_x)
                        } else {
                            // floor (truncate) so the last child absorbs the remainder,
                            // matching the original equal-split behaviour.
                            (used_area as f32 * (weights[i] / total)) as u16
                        };
                        let child_area = Rect::new(child_x, area.y, width, area.height);
                        child_x = child_x.saturating_add(width);
                        if i != len - 1 {
                            child_x = child_x.saturating_add(inner_gap);
                        }
                        self.stack.push((*child, child_area));
                    }
                }
            }
        }
    }

    /// Width of a node's current laid-out area (view or container).
    pub fn node_width(&self, id: ViewId) -> u16 {
        match &self.nodes[id].content {
            Content::View(view) => view.area.width,
            Content::Container(container) => container.area.width,
        }
    }

    /// Height of a node's current laid-out area (view or container).
    pub fn node_height(&self, id: ViewId) -> u16 {
        match &self.nodes[id].content {
            Content::View(view) => view.area.height,
            Content::Container(container) => container.area.height,
        }
    }

    /// If `(col, row)` falls on a split divider, return the view whose edge forms
    /// it together with the resize axis: `true` for a **vertical** divider (the
    /// border between left/right panes, on a view's right edge — drag it with
    /// [`Self::resize_horizontal`]) or `false` for a **horizontal** divider
    /// (between top/bottom panes, on a view's bottom edge — drag with
    /// [`Self::resize_vertical`]).
    pub fn split_divider_at(&self, col: u16, row: u16) -> Option<(ViewId, bool)> {
        self.views().map(|(view, _)| view).find_map(|view| {
            let a = view.area;
            // Vertical divider between side-by-side panes. Accept the right-edge
            // column and the one just left of it, so the 1-cell border is easier
            // to grab (the thin target was the "sometimes can't drag" symptom).
            if (col + 1 == a.right() || col == a.right())
                && row >= a.y
                && row < a.y + a.height
                && a.right() < self.area.x + self.area.width
            {
                return Some((view.id, true));
            }
            // Horizontal divider between stacked panes. The *visible* separator
            // is the top pane's statusline (its last row, `bottom - 1`), so accept
            // both that row and the bottom edge itself — otherwise the user has to
            // click one row below the line they can actually see.
            if (row + 1 == a.y + a.height || row == a.y + a.height)
                && col >= a.x
                && col < a.x + a.width
                && a.y + a.height < self.area.y + self.area.height
            {
                return Some((view.id, false));
            }
            None
        })
    }

    /// Drag the vertical divider on the right edge of `view` by `delta` columns
    /// (positive grows `view`, shrinking its right neighbour). Pins every sibling
    /// to its current width first so only the dragged divider moves. Returns true
    /// if the layout changed.
    pub fn resize_horizontal(&mut self, view: ViewId, delta: i16) -> bool {
        if delta == 0 {
            return false;
        }
        let parent = self.nodes[view].parent;
        let (layout, children) = match &self.nodes[parent].content {
            Content::Container(c) => (c.layout, c.children.clone()),
            Content::View(_) => return false,
        };
        if layout != Layout::Vertical {
            return false;
        }
        let Some(idx) = children.iter().position(|c| *c == view) else {
            return false;
        };
        if idx + 1 >= children.len() {
            return false;
        }

        const MIN: f32 = 3.0;
        let mut widths: Vec<f32> = children
            .iter()
            .map(|c| self.node_width(*c) as f32)
            .collect();

        let new_left = (widths[idx] + delta as f32).max(MIN);
        let applied = new_left - widths[idx];
        let new_right = widths[idx + 1] - applied;
        if new_right < MIN {
            return false;
        }
        widths[idx] = new_left;
        widths[idx + 1] = new_right;

        for (child, width) in children.iter().zip(widths) {
            self.nodes[*child].weight = width;
        }
        self.recalculate();
        true
    }

    /// Resize the given view's height by `delta` rows, borrowing from the next
    /// sibling in a horizontally-laid-out (stacked) container. Mirror of
    /// `resize_horizontal` for the vertical axis (vim CTRL-W + / CTRL-W -).
    pub fn resize_vertical(&mut self, view: ViewId, delta: i16) -> bool {
        if delta == 0 {
            return false;
        }
        let parent = self.nodes[view].parent;
        let (layout, children) = match &self.nodes[parent].content {
            Content::Container(c) => (c.layout, c.children.clone()),
            Content::View(_) => return false,
        };
        if layout != Layout::Horizontal {
            return false;
        }
        let Some(idx) = children.iter().position(|c| *c == view) else {
            return false;
        };
        if idx + 1 >= children.len() {
            return false;
        }

        const MIN: f32 = 2.0;
        let mut heights: Vec<f32> = children
            .iter()
            .map(|c| self.node_height(*c) as f32)
            .collect();

        let new_top = (heights[idx] + delta as f32).max(MIN);
        let applied = new_top - heights[idx];
        let new_bottom = heights[idx + 1] - applied;
        if new_bottom < MIN {
            return false;
        }
        heights[idx] = new_top;
        heights[idx + 1] = new_bottom;

        for (child, height) in children.iter().zip(heights) {
            self.nodes[*child].weight = height;
        }
        self.recalculate();
        true
    }

    /// Reset every view's size weight to equal (vim CTRL-W =).
    pub fn equalize(&mut self) {
        let ids: Vec<ViewId> = self.nodes.iter().map(|(id, _)| id).collect();
        for id in ids {
            self.nodes[id].weight = 1.0;
        }
        self.recalculate();
    }

    pub fn traverse(&self) -> Traverse<'_> {
        Traverse::new(self)
    }

    // Finds the split in the given direction if it exists
    pub fn find_split_in_direction(&self, id: ViewId, direction: Direction) -> Option<ViewId> {
        let parent = self.nodes[id].parent;
        // Base case, we found the root of the tree
        if parent == id {
            return None;
        }
        // Parent must always be a container
        let parent_container = match &self.nodes[parent].content {
            Content::Container(container) => container,
            Content::View(_) => unreachable!(),
        };

        match (direction, parent_container.layout) {
            (Direction::Up, Layout::Vertical)
            | (Direction::Left, Layout::Horizontal)
            | (Direction::Right, Layout::Horizontal)
            | (Direction::Down, Layout::Vertical) => {
                // The desired direction of movement is not possible within
                // the parent container so the search must continue closer to
                // the root of the split tree.
                self.find_split_in_direction(parent, direction)
            }
            (Direction::Up, Layout::Horizontal)
            | (Direction::Down, Layout::Horizontal)
            | (Direction::Left, Layout::Vertical)
            | (Direction::Right, Layout::Vertical) => {
                // It's possible to move in the desired direction within
                // the parent container so an attempt is made to find the
                // correct child.
                match self.find_child(id, &parent_container.children, direction) {
                    // Child is found, search is ended
                    Some(id) => Some(id),
                    // A child is not found. This could be because of either two scenarios
                    // 1. Its not possible to move in the desired direction, and search should end
                    // 2. A layout like the following with focus at X and desired direction Right
                    // | _ | x |   |
                    // | _ _ _ |   |
                    // | _ _ _ |   |
                    // The container containing X ends at X so no rightward movement is possible
                    // however there still exists another view/container to the right that hasn't
                    // been explored. Thus another search is done here in the parent container
                    // before concluding it's not possible to move in the desired direction.
                    None => self.find_split_in_direction(parent, direction),
                }
            }
        }
    }

    fn find_child(&self, id: ViewId, children: &[ViewId], direction: Direction) -> Option<ViewId> {
        let mut child_id = match direction {
            // index wise in the child list the Up and Left represents a -1
            // thus reversed iterator.
            Direction::Up | Direction::Left => children
                .iter()
                .rev()
                .skip_while(|i| **i != id)
                .copied()
                .nth(1)?,
            // Down and Right => +1 index wise in the child list
            Direction::Down | Direction::Right => {
                children.iter().skip_while(|i| **i != id).copied().nth(1)?
            }
        };
        let (current_x, current_y) = match &self.nodes[self.focus].content {
            Content::View(current_view) => (current_view.area.left(), current_view.area.top()),
            Content::Container(_) => unreachable!(),
        };

        // If the child is a container the search finds the closest container child
        // visually based on screen location.
        while let Content::Container(container) = &self.nodes[child_id].content {
            match (direction, container.layout) {
                (_, Layout::Vertical) => {
                    // find closest split based on x because y is irrelevant
                    // in a vertical container (and already correct based on previous search)
                    child_id = *container.children.iter().min_by_key(|id| {
                        let x = match &self.nodes[**id].content {
                            Content::View(view) => view.area.left(),
                            Content::Container(container) => container.area.left(),
                        };
                        (current_x as i16 - x as i16).abs()
                    })?;
                }
                (_, Layout::Horizontal) => {
                    // find closest split based on y because x is irrelevant
                    // in a horizontal container (and already correct based on previous search)
                    child_id = *container.children.iter().min_by_key(|id| {
                        let y = match &self.nodes[**id].content {
                            Content::View(view) => view.area.top(),
                            Content::Container(container) => container.area.top(),
                        };
                        (current_y as i16 - y as i16).abs()
                    })?;
                }
            }
        }
        Some(child_id)
    }

    pub fn prev(&self) -> ViewId {
        // This function is very dumb, but that's because we don't store any parent links.
        // (we'd be able to go parent.prev_sibling() recursively until we find something)
        // For now that's okay though, since it's unlikely you'll be able to open a large enough
        // number of splits to notice.

        let mut views = self
            .traverse()
            .rev()
            .skip_while(|&(id, _view)| id != self.focus)
            .skip(1); // Skip focused value
        if let Some((id, _)) = views.next() {
            id
        } else {
            // extremely crude, take the last item
            let (key, _) = self.traverse().next_back().unwrap();
            key
        }
    }

    pub fn next(&self) -> ViewId {
        // This function is very dumb, but that's because we don't store any parent links.
        // (we'd be able to go parent.next_sibling() recursively until we find something)
        // For now that's okay though, since it's unlikely you'll be able to open a large enough
        // number of splits to notice.

        let mut views = self
            .traverse()
            .skip_while(|&(id, _view)| id != self.focus)
            .skip(1); // Skip focused value
        if let Some((id, _)) = views.next() {
            id
        } else {
            // extremely crude, take the first item again
            let (key, _) = self.traverse().next().unwrap();
            key
        }
    }

    pub fn transpose(&mut self) {
        let focus = self.focus;
        let parent = self.nodes[focus].parent;
        if let Content::Container(container) = &mut self.nodes[parent].content {
            container.layout = match container.layout {
                Layout::Vertical => Layout::Horizontal,
                Layout::Horizontal => Layout::Vertical,
            };
            self.recalculate();
        }
    }

    pub fn swap_split_in_direction(&mut self, direction: Direction) -> Option<()> {
        let focus = self.focus;
        let target = self.find_split_in_direction(focus, direction)?;
        let focus_parent = self.nodes[focus].parent;
        let target_parent = self.nodes[target].parent;

        if focus_parent == target_parent {
            let parent = focus_parent;
            let [parent, focus, target] = self.nodes.get_disjoint_mut([parent, focus, target])?;
            match (&mut parent.content, &mut focus.content, &mut target.content) {
                (
                    Content::Container(parent),
                    Content::View(focus_view),
                    Content::View(target_view),
                ) => {
                    let focus_pos = parent.children.iter().position(|id| focus_view.id == *id)?;
                    let target_pos = parent
                        .children
                        .iter()
                        .position(|id| target_view.id == *id)?;
                    // swap node positions so that traversal order is kept
                    parent.children[focus_pos] = target_view.id;
                    parent.children[target_pos] = focus_view.id;
                    // swap area so that views rendered at the correct location
                    std::mem::swap(&mut focus_view.area, &mut target_view.area);

                    Some(())
                }
                _ => unreachable!(),
            }
        } else {
            let [focus_parent, target_parent, focus, target] =
                self.nodes
                    .get_disjoint_mut([focus_parent, target_parent, focus, target])?;
            match (
                &mut focus_parent.content,
                &mut target_parent.content,
                &mut focus.content,
                &mut target.content,
            ) {
                (
                    Content::Container(focus_parent),
                    Content::Container(target_parent),
                    Content::View(focus_view),
                    Content::View(target_view),
                ) => {
                    let focus_pos = focus_parent
                        .children
                        .iter()
                        .position(|id| focus_view.id == *id)?;
                    let target_pos = target_parent
                        .children
                        .iter()
                        .position(|id| target_view.id == *id)?;
                    // re-parent target and focus nodes
                    std::mem::swap(
                        &mut focus_parent.children[focus_pos],
                        &mut target_parent.children[target_pos],
                    );
                    std::mem::swap(&mut focus.parent, &mut target.parent);
                    // swap area so that views rendered at the correct location
                    std::mem::swap(&mut focus_view.area, &mut target_view.area);

                    Some(())
                }
                _ => unreachable!(),
            }
        }
    }

    pub fn area(&self) -> Rect {
        self.area
    }
}

#[derive(Debug)]
pub struct Traverse<'a> {
    tree: &'a Tree,
    stack: Vec<ViewId>, // TODO: reuse the one we use on update
}

impl<'a> Traverse<'a> {
    fn new(tree: &'a Tree) -> Self {
        Self {
            tree,
            stack: vec![tree.root],
        }
    }
}

impl<'a> Iterator for Traverse<'a> {
    type Item = (ViewId, &'a View);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let key = self.stack.pop()?;

            let node = &self.tree.nodes[key];

            match &node.content {
                Content::View(view) => return Some((key, view)),
                Content::Container(container) => {
                    self.stack.extend(container.children.iter().rev());
                }
            }
        }
    }
}

impl DoubleEndedIterator for Traverse<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            let key = self.stack.pop()?;

            let node = &self.tree.nodes[key];

            match &node.content {
                Content::View(view) => return Some((key, view)),
                Content::Container(container) => {
                    self.stack.extend(container.children.iter());
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::editor::GutterConfig;
    use crate::DocumentId;

    // vim `splitright`/`splitbelow` substrate: `split_with(before)` places the new
    // view to the left/above (before) or right/below (after, the default) of the
    // focused window.
    #[test]
    fn split_with_places_new_view_before_or_after_focus() {
        let area = Rect::new(0, 0, 180, 80);
        let docs = |tree: &Tree| -> Vec<DocumentId> {
            tree.leaf_ids().iter().map(|&id| tree.get(id).doc).collect()
        };

        // Default (after): new view goes to the right.
        let mut tree = Tree::new(area);
        tree.insert(View::new(DocumentId::new(10), GutterConfig::default()));
        tree.split_with(
            View::new(DocumentId::new(20), GutterConfig::default()),
            Layout::Vertical,
            false,
        );
        assert_eq!(
            docs(&tree),
            vec![DocumentId::new(10), DocumentId::new(20)],
            "before=false places the new view after (to the right)"
        );

        // `nosplitright` (before): new view goes to the left.
        let mut tree = Tree::new(area);
        tree.insert(View::new(DocumentId::new(10), GutterConfig::default()));
        tree.split_with(
            View::new(DocumentId::new(20), GutterConfig::default()),
            Layout::Vertical,
            true,
        );
        assert_eq!(
            docs(&tree),
            vec![DocumentId::new(20), DocumentId::new(10)],
            "before=true places the new view before (to the left)"
        );
    }

    // Collect (doc, focused) for each leaf of a shape, left-to-right.
    fn shape_leaves(shape: &TreeShape) -> Vec<(DocumentId, bool)> {
        match shape {
            TreeShape::Leaf { doc, focused } => vec![(*doc, *focused)],
            TreeShape::Split { children, .. } => {
                children.iter().flat_map(|(_, c)| shape_leaves(c)).collect()
            }
        }
    }

    #[test]
    fn tabpage_shape_roundtrips_layout_and_focus() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 180,
            height: 80,
        };
        let mut tree = Tree::new(area);

        // Three windows on distinct documents in a vertical split.
        let mut v0 = View::new(DocumentId::new(10), GutterConfig::default());
        v0.area = Rect::new(0, 0, 180, 80);
        tree.insert(v0);
        tree.split(
            View::new(DocumentId::new(20), GutterConfig::default()),
            Layout::Vertical,
        );
        tree.split(
            View::new(DocumentId::new(30), GutterConfig::default()),
            Layout::Vertical,
        );
        // Focus the middle window.
        let middle = tree.leaf_ids()[1];
        tree.focus = middle;

        let shape = tree.shape();
        let before = shape_leaves(&shape);
        assert_eq!(
            before,
            vec![
                (DocumentId::new(10), false),
                (DocumentId::new(20), true),
                (DocumentId::new(30), false),
            ],
            "snapshot must preserve left-to-right docs and mark the focused leaf"
        );

        // Rebuild into a fresh tree; new ViewIds, same structure.
        let mut rebuilt = Tree::new(area);
        let new_ids =
            rebuilt.build_from_shape(&shape, &mut |doc| View::new(doc, GutterConfig::default()));

        assert_eq!(new_ids.len(), 3, "three leaves rebuilt");
        let after_docs: Vec<DocumentId> = rebuilt
            .leaf_ids()
            .iter()
            .map(|&id| rebuilt.get(id).doc)
            .collect();
        assert_eq!(
            after_docs,
            vec![
                DocumentId::new(10),
                DocumentId::new(20),
                DocumentId::new(30)
            ],
            "rebuilt windows keep document order"
        );
        // Focus lands on the leaf that was marked focused (the middle one).
        assert_eq!(rebuilt.get(rebuilt.focus).doc, DocumentId::new(20));
        // The returned ids are in the same order as the rebuilt leaves.
        assert_eq!(new_ids, rebuilt.leaf_ids());
    }

    #[test]
    fn horizontal_divider_hitbox_includes_statusline_row() {
        // Two stacked panes (a `:split`): top [y 0..40], bottom [y 40..80].
        let area = Rect {
            x: 0,
            y: 0,
            width: 180,
            height: 80,
        };
        let mut tree = Tree::new(area);
        let mut v0 = View::new(DocumentId::new(1), GutterConfig::default());
        v0.area = area;
        tree.insert(v0);
        tree.split(
            View::new(DocumentId::new(2), GutterConfig::default()),
            Layout::Horizontal,
        );

        // Identify the top pane (the one anchored at y == 0).
        let top = tree
            .views()
            .map(|(v, _)| v)
            .find(|v| v.area.y == 0)
            .map(|v| v.id)
            .unwrap();
        let split_row = tree.get(top).area.y + tree.get(top).area.height; // 40

        // The visible divider is the top pane's statusline (split_row - 1) — it
        // must be grabbable, and so must the boundary row itself.
        assert_eq!(
            tree.split_divider_at(90, split_row - 1),
            Some((top, false)),
            "statusline row must be a grabbable horizontal divider"
        );
        assert_eq!(tree.split_divider_at(90, split_row), Some((top, false)));
        // Interior content rows are not dividers.
        assert_eq!(tree.split_divider_at(90, split_row - 2), None);
        assert_eq!(tree.split_divider_at(90, 10), None);
        // The bottom pane's own last row has nothing below it: not a divider.
        assert_eq!(tree.split_divider_at(90, area.height - 1), None);
    }

    #[test]
    fn find_split_in_direction() {
        let mut tree = Tree::new(Rect {
            x: 0,
            y: 0,
            width: 180,
            height: 80,
        });
        let mut view = View::new(DocumentId::default(), GutterConfig::default());
        view.area = Rect::new(0, 0, 180, 80);
        tree.insert(view);

        let l0 = tree.focus;
        let view = View::new(DocumentId::default(), GutterConfig::default());
        tree.split(view, Layout::Vertical);
        let r0 = tree.focus;

        tree.focus = l0;
        let view = View::new(DocumentId::default(), GutterConfig::default());
        tree.split(view, Layout::Horizontal);
        let l1 = tree.focus;

        tree.focus = l0;
        let view = View::new(DocumentId::default(), GutterConfig::default());
        tree.split(view, Layout::Vertical);

        // Tree in test
        // | L0  | L2 |    |
        // |    L1    | R0 |
        let l2 = tree.focus;
        assert_eq!(Some(l0), tree.find_split_in_direction(l2, Direction::Left));
        assert_eq!(Some(l1), tree.find_split_in_direction(l2, Direction::Down));
        assert_eq!(Some(r0), tree.find_split_in_direction(l2, Direction::Right));
        assert_eq!(None, tree.find_split_in_direction(l2, Direction::Up));

        tree.focus = l1;
        assert_eq!(None, tree.find_split_in_direction(l1, Direction::Left));
        assert_eq!(None, tree.find_split_in_direction(l1, Direction::Down));
        assert_eq!(Some(r0), tree.find_split_in_direction(l1, Direction::Right));
        assert_eq!(Some(l0), tree.find_split_in_direction(l1, Direction::Up));

        tree.focus = l0;
        assert_eq!(None, tree.find_split_in_direction(l0, Direction::Left));
        assert_eq!(Some(l1), tree.find_split_in_direction(l0, Direction::Down));
        assert_eq!(Some(l2), tree.find_split_in_direction(l0, Direction::Right));
        assert_eq!(None, tree.find_split_in_direction(l0, Direction::Up));

        tree.focus = r0;
        assert_eq!(Some(l2), tree.find_split_in_direction(r0, Direction::Left));
        assert_eq!(None, tree.find_split_in_direction(r0, Direction::Down));
        assert_eq!(None, tree.find_split_in_direction(r0, Direction::Right));
        assert_eq!(None, tree.find_split_in_direction(r0, Direction::Up));
    }

    #[test]
    fn swap_split_in_direction() {
        let mut tree = Tree::new(Rect {
            x: 0,
            y: 0,
            width: 180,
            height: 80,
        });

        let doc_l0 = DocumentId::default();
        let mut view = View::new(doc_l0, GutterConfig::default());
        view.area = Rect::new(0, 0, 180, 80);
        tree.insert(view);

        let l0 = tree.focus;

        let doc_r0 = DocumentId::default();
        let view = View::new(doc_r0, GutterConfig::default());
        tree.split(view, Layout::Vertical);
        let r0 = tree.focus;

        tree.focus = l0;

        let doc_l1 = DocumentId::default();
        let view = View::new(doc_l1, GutterConfig::default());
        tree.split(view, Layout::Horizontal);
        let l1 = tree.focus;

        tree.focus = l0;

        let doc_l2 = DocumentId::default();
        let view = View::new(doc_l2, GutterConfig::default());
        tree.split(view, Layout::Vertical);
        let l2 = tree.focus;

        // Views in test
        // | L0  | L2 |    |
        // |    L1    | R0 |

        // Document IDs in test
        // | l0  | l2 |    |
        // |    l1    | r0 |

        fn doc_id(tree: &Tree, view_id: ViewId) -> Option<DocumentId> {
            if let Content::View(view) = &tree.nodes[view_id].content {
                Some(view.doc)
            } else {
                None
            }
        }

        tree.focus = l0;
        // `*` marks the view in focus from view table (here L0)
        // | l0*  | l2 |    |
        // |    l1     | r0 |
        tree.swap_split_in_direction(Direction::Down);
        // | l1   | l2 |    |
        // |    l0*    | r0 |
        assert_eq!(tree.focus, l0);
        assert_eq!(doc_id(&tree, l0), Some(doc_l1));
        assert_eq!(doc_id(&tree, l1), Some(doc_l0));
        assert_eq!(doc_id(&tree, l2), Some(doc_l2));
        assert_eq!(doc_id(&tree, r0), Some(doc_r0));

        tree.swap_split_in_direction(Direction::Right);

        // | l1  | l2 |     |
        // |    r0    | l0* |
        assert_eq!(tree.focus, l0);
        assert_eq!(doc_id(&tree, l0), Some(doc_l1));
        assert_eq!(doc_id(&tree, l1), Some(doc_r0));
        assert_eq!(doc_id(&tree, l2), Some(doc_l2));
        assert_eq!(doc_id(&tree, r0), Some(doc_l0));

        // cannot swap, nothing changes
        tree.swap_split_in_direction(Direction::Up);
        // | l1  | l2 |     |
        // |    r0    | l0* |
        assert_eq!(tree.focus, l0);
        assert_eq!(doc_id(&tree, l0), Some(doc_l1));
        assert_eq!(doc_id(&tree, l1), Some(doc_r0));
        assert_eq!(doc_id(&tree, l2), Some(doc_l2));
        assert_eq!(doc_id(&tree, r0), Some(doc_l0));

        // cannot swap, nothing changes
        tree.swap_split_in_direction(Direction::Down);
        // | l1  | l2 |     |
        // |    r0    | l0* |
        assert_eq!(tree.focus, l0);
        assert_eq!(doc_id(&tree, l0), Some(doc_l1));
        assert_eq!(doc_id(&tree, l1), Some(doc_r0));
        assert_eq!(doc_id(&tree, l2), Some(doc_l2));
        assert_eq!(doc_id(&tree, r0), Some(doc_l0));

        tree.focus = l2;
        // | l1  | l2* |    |
        // |    r0     | l0 |

        tree.swap_split_in_direction(Direction::Down);
        // | l1  | r0  |    |
        // |    l2*    | l0 |
        assert_eq!(tree.focus, l2);
        assert_eq!(doc_id(&tree, l0), Some(doc_l1));
        assert_eq!(doc_id(&tree, l1), Some(doc_l2));
        assert_eq!(doc_id(&tree, l2), Some(doc_r0));
        assert_eq!(doc_id(&tree, r0), Some(doc_l0));

        tree.swap_split_in_direction(Direction::Up);
        // | l2* | r0 |    |
        // |    l1    | l0 |
        assert_eq!(tree.focus, l2);
        assert_eq!(doc_id(&tree, l0), Some(doc_l2));
        assert_eq!(doc_id(&tree, l1), Some(doc_l1));
        assert_eq!(doc_id(&tree, l2), Some(doc_r0));
        assert_eq!(doc_id(&tree, r0), Some(doc_l0));
    }

    #[test]
    fn all_vertical_views_have_same_width() {
        let tree_area_width = 180;
        let mut tree = Tree::new(Rect {
            x: 0,
            y: 0,
            width: tree_area_width,
            height: 80,
        });
        let mut view = View::new(DocumentId::default(), GutterConfig::default());
        view.area = Rect::new(0, 0, 180, 80);
        tree.insert(view);

        let view = View::new(DocumentId::default(), GutterConfig::default());
        tree.split(view, Layout::Vertical);

        let view = View::new(DocumentId::default(), GutterConfig::default());
        tree.split(view, Layout::Horizontal);

        tree.remove(tree.focus);

        let view = View::new(DocumentId::default(), GutterConfig::default());
        tree.split(view, Layout::Vertical);

        // Make sure that we only have one level in the tree.
        assert_eq!(3, tree.views().count());
        assert_eq!(
            vec![
                tree_area_width / 3 - 1, // gap here
                tree_area_width / 3 - 1, // gap here
                tree_area_width / 3
            ],
            tree.views()
                .map(|(view, _)| view.area.width)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn vsplit_gap_rounding() {
        let (tree_area_width, tree_area_height) = (80, 24);
        let mut tree = Tree::new(Rect {
            x: 0,
            y: 0,
            width: tree_area_width,
            height: tree_area_height,
        });
        let mut view = View::new(DocumentId::default(), GutterConfig::default());
        view.area = Rect::new(0, 0, tree_area_width, tree_area_height);
        tree.insert(view);

        for _ in 0..9 {
            let view = View::new(DocumentId::default(), GutterConfig::default());
            tree.split(view, Layout::Vertical);
        }

        assert_eq!(10, tree.views().count());
        assert_eq!(
            std::iter::repeat_n(7, 9)
                .chain(Some(8)) // Rounding in `recalculate`.
                .collect::<Vec<_>>(),
            tree.views()
                .map(|(view, _)| view.area.width)
                .collect::<Vec<_>>()
        );
    }
}
