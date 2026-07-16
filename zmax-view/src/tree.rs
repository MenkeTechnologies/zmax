use crate::{graphics::Rect, DocumentId, View, ViewId};
use slotmap::SlotMap;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

// vim `winminwidth` / `winminheight`: the floor a window can be resized to. The
// `:set` option store lives in zmax-term, so the values are pushed down here.
// The defaults are the sizes the tree always enforced.
static WIN_MIN_WIDTH: AtomicU16 = AtomicU16::new(3);
static WIN_MIN_HEIGHT: AtomicU16 = AtomicU16::new(2);

pub fn set_win_min_width(cols: u16) {
    WIN_MIN_WIDTH.store(cols.max(1), Ordering::Relaxed);
}

pub fn set_win_min_height(rows: u16) {
    WIN_MIN_HEIGHT.store(rows.max(1), Ordering::Relaxed);
}

// vim `winheight` / `winwidth`: the size the *current* window is grown to when
// it is smaller than this — vim's "give the window you are working in room".
// `0` means the policy is off, which is where zmax starts: vim's own defaults
// (`winheight=1`, `winwidth=20`) would resize windows the user never asked to
// resize, so the policy only runs once `:set winheight`/`winwidth` asks for it.
static WIN_HEIGHT: AtomicU16 = AtomicU16::new(0);
static WIN_WIDTH: AtomicU16 = AtomicU16::new(0);

pub fn set_win_height(rows: u16) {
    WIN_HEIGHT.store(rows, Ordering::Relaxed);
}

pub fn set_win_width(cols: u16) {
    WIN_WIDTH.store(cols, Ordering::Relaxed);
}

fn win_height() -> u16 {
    WIN_HEIGHT.load(Ordering::Relaxed)
}

fn win_width() -> u16 {
    WIN_WIDTH.load(Ordering::Relaxed)
}

/// vim `eadirection` (`ead`, default `both`): the directions `equalalways`
/// (and CTRL-W =) levels windows in — `ver` heights only, `hor` widths only.
static EA_VER: AtomicBool = AtomicBool::new(true);
static EA_HOR: AtomicBool = AtomicBool::new(true);

pub fn set_eadirection(spec: &str) {
    let spec = spec.trim();
    let (ver, hor) = match spec {
        "ver" => (true, false),
        "hor" => (false, true),
        // Anything else (`both`, or an empty value) levels in both directions.
        _ => (true, true),
    };
    EA_VER.store(ver, Ordering::Relaxed);
    EA_HOR.store(hor, Ordering::Relaxed);
}

fn eadirection_ver() -> bool {
    EA_VER.load(Ordering::Relaxed)
}

fn eadirection_hor() -> bool {
    EA_HOR.load(Ordering::Relaxed)
}

fn win_min_width() -> u16 {
    WIN_MIN_WIDTH.load(Ordering::Relaxed)
}

fn win_min_height() -> u16 {
    WIN_MIN_HEIGHT.load(Ordering::Relaxed)
}

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

/// Lay `total` cells out over sibling windows: a window vim's `winfixheight` /
/// `winfixwidth` pinned (`fixed[i]`) keeps its size and the rest share what is
/// left, in proportion to their weights, with the last flexible one absorbing
/// the rounding remainder. The pins are dropped whenever honouring them cannot
/// work — every sibling pinned, or the pinned sizes leaving nothing for the
/// others — because a window must always fit somewhere. Pure — unit tested.
fn split_sizes(total: u16, weights: &[f32], weight_total: f32, fixed: &[Option<u16>]) -> Vec<u16> {
    let len = weights.len();
    let plain = |i: usize| -> u16 { (total as f32 * (weights[i] / weight_total)) as u16 };
    let flexible: Vec<usize> = (0..len).filter(|&i| fixed[i].is_none()).collect();
    let pinned: u16 = fixed.iter().flatten().sum();
    if flexible.is_empty() || pinned >= total {
        return (0..len).map(plain).collect();
    }
    let flex_weight: f32 = flexible.iter().map(|&i| weights[i]).sum();
    let flex_weight = if flex_weight > f32::EPSILON {
        flex_weight
    } else {
        flexible.len() as f32
    };
    let room = total - pinned;
    let mut sizes = vec![0u16; len];
    let mut used = 0u16;
    for (n, &i) in flexible.iter().enumerate() {
        sizes[i] = if n + 1 == flexible.len() {
            room.saturating_sub(used)
        } else {
            (room as f32 * (weights[i] / flex_weight)) as u16
        };
        used = used.saturating_add(sizes[i]);
    }
    for (i, size) in fixed.iter().enumerate() {
        if let Some(size) = size {
            sizes[i] = *size;
        }
    }
    sizes
}

/// Grow sibling `idx` to `want` cells (vim `winheight` / `winwidth`), taking the
/// difference from the other siblings in proportion to the room each has above
/// the `min` floor. Returns the new sizes, or `None` when nothing moves — the
/// window is already big enough, or its siblings have no room to give. Pure —
/// unit tested.
fn grow_to(sizes: &[u16], idx: usize, want: u16, min: u16) -> Option<Vec<u16>> {
    let total: u16 = sizes.iter().copied().sum();
    // Never take a sibling below the floor: that caps what `want` can be.
    let others = sizes.len().checked_sub(1)? as u16;
    let want = want.min(total.saturating_sub(min.saturating_mul(others)));
    let need = want.checked_sub(sizes[idx]).filter(|n| *n > 0)?;

    let slack: Vec<u16> = sizes
        .iter()
        .enumerate()
        .map(|(i, &s)| if i == idx { 0 } else { s.saturating_sub(min) })
        .collect();
    let total_slack: u16 = slack.iter().sum();
    if total_slack == 0 {
        return None;
    }
    let take = need.min(total_slack);

    let mut out = sizes.to_vec();
    let mut taken = 0u16;
    let last = slack.iter().rposition(|&s| s > 0)?;
    for (i, &s) in slack.iter().enumerate() {
        if s == 0 {
            continue;
        }
        // The last donor absorbs the rounding remainder, so the sizes still add
        // up to `total` exactly.
        let give = if i == last {
            take - taken
        } else {
            (take as u32 * s as u32 / total_slack as u32) as u16
        };
        out[i] = s + min - give;
        taken += give;
    }
    out[idx] = sizes[idx] + taken;
    Some(out)
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
        // vim `winheight`/`winwidth`: the window just focused gets its room.
        self.apply_win_size_policy();

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
        // vim `winheight`/`winwidth`: the window just focused gets its room.
        self.apply_win_size_policy();

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

        self.recalculate();
        // vim `winheight`/`winwidth`: closing a window re-focuses another one,
        // which must get its room too.
        self.apply_win_size_policy();
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

    /// The size vim's `winfixheight` / `winfixwidth` pins this node to, if any.
    /// Only leaf windows can be pinned (a container follows its children).
    fn fixed_size(&self, id: ViewId, vertical: bool) -> Option<u16> {
        match &self.nodes[id].content {
            Content::View(view) if vertical && view.winfixheight => Some(view.area.height),
            Content::View(view) if !vertical && view.winfixwidth => Some(view.area.width),
            _ => None,
        }
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

            // vim `winfixheight` / `winfixwidth`: a pinned window keeps the size it
            // has now while its siblings absorb the change.
            let vertical_axis = matches!(layout, Layout::Horizontal);
            let fixed: Vec<Option<u16>> = children
                .iter()
                .map(|child| self.fixed_size(*child, vertical_axis))
                .collect();
            let any_fixed = fixed.iter().any(Option::is_some);

            match layout {
                Layout::Horizontal => {
                    let sizes = split_sizes(area.height, &weights, total, &fixed);
                    let mut child_y = area.y;
                    for (i, child) in children.iter().enumerate() {
                        // The last child absorbs any rounding remainder — unless it
                        // is pinned, in which case the remainder already went to the
                        // last flexible sibling.
                        let height = if i == len - 1 && !(any_fixed && fixed[i].is_some()) {
                            (area.y + area.height).saturating_sub(child_y)
                        } else {
                            sizes[i]
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
                    let sizes = split_sizes(used_area, &weights, total, &fixed);

                    let mut child_x = area.x;
                    for (i, child) in children.iter().enumerate() {
                        // The last child absorbs rounding + gap remainder (unless pinned).
                        let width = if i == len - 1 && !(any_fixed && fixed[i].is_some()) {
                            (area.x + area.width).saturating_sub(child_x)
                        } else {
                            sizes[i]
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

        // vim `winminwidth`: a window is never resized below this many columns.
        let min = win_min_width() as f32;
        let mut widths: Vec<f32> = children
            .iter()
            .map(|c| self.node_width(*c) as f32)
            .collect();

        // vim refuses a resize that would take either window below the floor
        // rather than silently applying a smaller one.
        let new_left = widths[idx] + delta as f32;
        let new_right = widths[idx + 1] - delta as f32;
        if new_left < min || new_right < min {
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

        // vim `winminheight`: a window is never resized below this many rows.
        let min = win_min_height() as f32;
        let mut heights: Vec<f32> = children
            .iter()
            .map(|c| self.node_height(*c) as f32)
            .collect();

        let new_top = heights[idx] + delta as f32;
        let new_bottom = heights[idx + 1] - delta as f32;
        if new_top < min || new_bottom < min {
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

    /// vim `winheight` / `winwidth`: give the focused window at least that many
    /// rows / columns, taking the space from its siblings (never below
    /// `winminheight` / `winminwidth`). Runs on every focus change, split and
    /// close, and when the options themselves are set. Does nothing while both
    /// options are `0` (the default — see [`set_win_height`]). Returns true if
    /// the layout changed.
    pub fn apply_win_size_policy(&mut self) -> bool {
        self.apply_size_policy(win_height(), win_width())
    }

    /// [`Tree::apply_win_size_policy`] with the two sizes given explicitly, so the
    /// policy can be exercised without touching the process-wide option statics.
    fn apply_size_policy(&mut self, height: u16, width: u16) -> bool {
        if (height == 0 && width == 0)
            || self.is_empty()
            || !matches!(self.nodes[self.focus].content, Content::View(_))
        {
            return false;
        }
        // Heights first: growing the focused window vertically can only change
        // widths through a re-layout, which the final `recalculate` does anyway.
        let grew_height = self.grow_focus(true, height, win_min_height());
        let grew_width = self.grow_focus(false, width, win_min_width());
        if grew_height || grew_width {
            self.recalculate();
        }
        grew_height || grew_width
    }

    /// One axis of [`Tree::apply_win_size_policy`]: walk from the focused window
    /// up to the root and, in every container that splits *this* axis, grow the
    /// ancestor the focus sits in. Growing the ancestors too is what makes the
    /// policy work through nested splits — a window three levels deep cannot get
    /// 20 columns if its parent container only has 10.
    fn grow_focus(&mut self, vertical: bool, want: u16, min: u16) -> bool {
        if want == 0 {
            return false;
        }
        let mut node = self.focus;
        let mut changed = false;
        loop {
            let parent = self.nodes[node].parent;
            if parent == node {
                return changed;
            }
            let (layout, children) = match &self.nodes[parent].content {
                Content::Container(c) => (c.layout, c.children.clone()),
                Content::View(_) => return changed,
            };
            // A `Horizontal` container stacks its children, so it is the one that
            // splits the vertical (height) axis; `Vertical` splits the width.
            let splits_axis = (layout == Layout::Horizontal) == vertical;
            if splits_axis && children.len() > 1 {
                changed |= self.grow_child(&children, node, vertical, want, min);
            }
            node = parent;
        }
    }

    /// Rewrite the size weights of one container's children so `child` reaches
    /// `want` cells on the given axis. The weights are cell counts, the same
    /// convention [`Tree::resize_horizontal`] uses when it drags a divider.
    fn grow_child(
        &mut self,
        children: &[ViewId],
        child: ViewId,
        vertical: bool,
        want: u16,
        min: u16,
    ) -> bool {
        let Some(idx) = children.iter().position(|&c| c == child) else {
            return false;
        };
        let sizes: Vec<u16> = children
            .iter()
            .map(|&c| {
                if vertical {
                    self.node_height(c)
                } else {
                    self.node_width(c)
                }
            })
            .collect();
        let Some(sizes) = grow_to(&sizes, idx, want, min) else {
            return false;
        };
        for (&c, size) in children.iter().zip(sizes) {
            self.nodes[c].weight = size as f32;
        }
        true
    }

    /// Reset every view's size weight to equal (vim CTRL-W =), in the directions
    /// vim `eadirection` allows: `ver` only levels the heights (the children of a
    /// horizontal split), `hor` only the widths, `both` (the default) does both.
    pub fn equalize(&mut self) {
        let ids: Vec<ViewId> = self.nodes.iter().map(|(id, _)| id).collect();
        for id in ids {
            let parent = self.nodes[id].parent;
            let level = match &self.nodes[parent].content {
                Content::Container(container) => match container.layout {
                    Layout::Horizontal => eadirection_ver(),
                    Layout::Vertical => eadirection_hor(),
                },
                Content::View(_) => true,
            };
            if level {
                self.nodes[id].weight = 1.0;
            }
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

    // vim `winminwidth`: a resize that would push a window below the floor is
    // refused outright, exactly as vim refuses to shrink past `winminwidth`.
    #[test]
    fn winminwidth_is_the_resize_floor() {
        let mut tree = Tree::new(Rect::new(0, 0, 40, 20));
        let left = tree.insert(View::new(DocumentId::new(10), GutterConfig::default()));
        tree.split_with(
            View::new(DocumentId::new(20), GutterConfig::default()),
            Layout::Vertical,
            false,
        );

        // With the default floor (3), the left window can shrink to a sliver.
        assert!(tree.resize_horizontal(left, -16));
        assert!(tree.node_width(left) <= 5);

        // Raise the floor: the same window can no longer be shrunk further.
        set_win_min_width(10);
        let before = tree.node_width(left);
        assert!(!tree.resize_horizontal(left, -1));
        assert_eq!(
            tree.node_width(left),
            before,
            "refused resize changes nothing"
        );
        set_win_min_width(3);
    }

    // vim `winwidth`: the focused window is grown to the requested width, and the
    // space comes out of its sibling — which never drops below `winminwidth`.
    // (The policy is driven through `apply_size_policy` so the test never writes
    // the process-wide option statics that the other tests in this file read.)
    #[test]
    fn winwidth_grows_the_focused_window_and_winheight_the_stack() {
        let mut tree = Tree::new(Rect::new(0, 0, 100, 40));
        let left = tree.insert(View::new(DocumentId::new(10), GutterConfig::default()));
        let right = tree.split_with(
            View::new(DocumentId::new(20), GutterConfig::default()),
            Layout::Vertical,
            false,
        );
        // Off (both `0`): an even split stays an even split.
        assert!(!tree.apply_size_policy(0, 0));
        assert!(tree.node_width(left).abs_diff(tree.node_width(right)) <= 1);

        // `:set winwidth=80` — the focused (right) window takes what it needs.
        tree.focus = right;
        assert!(tree.apply_size_policy(0, 80));
        assert!(
            tree.node_width(right) >= 80,
            "focused window grew to winwidth, got {}",
            tree.node_width(right)
        );
        assert!(
            tree.node_width(left) >= win_min_width(),
            "the donor stays above winminwidth"
        );

        // Focus the other one: the policy follows the focus.
        tree.focus = left;
        assert!(tree.apply_size_policy(0, 80));
        assert!(tree.node_width(left) >= 80);

        // A request bigger than the container is capped by `winminwidth`, not by
        // squeezing the sibling out of existence.
        tree.apply_size_policy(0, 500);
        assert!(tree.node_width(right) >= win_min_width());

        // Same policy on the vertical axis (`winheight`) in a stacked split.
        let mut tree = Tree::new(Rect::new(0, 0, 100, 40));
        let top = tree.insert(View::new(DocumentId::new(10), GutterConfig::default()));
        let bottom = tree.split_with(
            View::new(DocumentId::new(20), GutterConfig::default()),
            Layout::Horizontal,
            false,
        );
        tree.focus = top;
        assert!(tree.apply_size_policy(30, 0));
        assert!(tree.node_height(top) >= 30);
        assert!(tree.node_height(bottom) >= win_min_height());
    }

    // The `:set` entry points feed the same policy: they store what the option
    // loop pushes down, and the default (`0`) leaves the layout alone.
    #[test]
    fn win_height_and_width_default_to_off() {
        assert_eq!(win_height(), 0);
        assert_eq!(win_width(), 0);
    }

    // The pure sizing rule behind `winheight`/`winwidth`.
    #[test]
    fn grow_to_takes_from_the_siblings_slack_only() {
        // 10/10/10, grow the middle to 20: the 10 needed comes evenly off the two
        // siblings, and the total is preserved.
        let out = grow_to(&[10, 10, 10], 1, 20, 2).unwrap();
        assert_eq!(out.iter().sum::<u16>(), 30);
        assert_eq!(out[1], 20);
        assert_eq!(out, vec![5, 20, 5]);

        // Already big enough: nothing to do.
        assert_eq!(grow_to(&[10, 30], 1, 20, 2), None);

        // The floor caps the request: with min=8 the sibling cannot go below 8, so
        // the grown window stops at 22 rather than taking all 30.
        let out = grow_to(&[10, 20], 0, 30, 8).unwrap();
        assert_eq!(out, vec![22, 8]);

        // No slack at all (every sibling is already at the floor): refuse.
        assert_eq!(grow_to(&[5, 3], 0, 8, 3), None);
    }

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

    /// vim `winfixheight` / `winfixwidth`: the pinned window keeps its size and
    /// its siblings absorb the whole change. When the pins cannot be honoured
    /// (everything pinned, or no room left) they are dropped — a window always
    /// has to fit somewhere.
    #[test]
    fn split_sizes_keeps_pinned_windows_and_shares_the_rest() {
        let w = [1.0f32, 1.0, 1.0];

        // No pins: the plain weighted split (the last flexible one takes the
        // rounding remainder).
        assert_eq!(
            split_sizes(30, &w, 3.0, &[None, None, None]),
            vec![10, 10, 10]
        );

        // Middle window pinned at 5: the other two share the remaining 25.
        assert_eq!(
            split_sizes(30, &w, 3.0, &[None, Some(5), None]),
            vec![12, 5, 13]
        );

        // Growing the container leaves the pinned window alone.
        assert_eq!(
            split_sizes(60, &w, 3.0, &[None, Some(5), None]),
            vec![27, 5, 28]
        );

        // Weights still divide what is left.
        assert_eq!(
            split_sizes(30, &[3.0, 1.0, 1.0], 5.0, &[None, Some(6), None]),
            vec![18, 6, 6]
        );

        // Every window pinned => the pins are ignored, plain split.
        assert_eq!(
            split_sizes(30, &w, 3.0, &[Some(9), Some(9), Some(9)]),
            vec![10, 10, 10]
        );

        // Pins bigger than the container => ignored rather than starving the rest.
        assert_eq!(
            split_sizes(10, &w, 3.0, &[Some(40), None, None]),
            vec![3, 3, 3]
        );
    }

    /// vim `eadirection`: `equalalways` / CTRL-W = only levels the windows in the
    /// named direction — `ver` evens the heights of a horizontal split and leaves
    /// the widths of a vertical one alone, `hor` the other way round.
    #[test]
    fn eadirection_limits_which_axis_equalize_levels() {
        // Two windows side by side (vertical split), then one split below the
        // right-hand one: widths live in the vertical container, heights in the
        // horizontal one.
        let mut tree = Tree::new(Rect::new(0, 0, 180, 80));
        let mut view = View::new(DocumentId::default(), GutterConfig::default());
        view.area = Rect::new(0, 0, 180, 80);
        let left = tree.insert(view);
        let right = tree.split(
            View::new(DocumentId::default(), GutterConfig::default()),
            Layout::Vertical,
        );
        let below = tree.split(
            View::new(DocumentId::default(), GutterConfig::default()),
            Layout::Horizontal,
        );

        // Skew both axes: the left window twice as wide, the bottom one taller.
        let skew = |tree: &mut Tree| {
            tree.nodes[left].weight = 2.0;
            tree.nodes[below].weight = 3.0;
            tree.recalculate();
        };

        skew(&mut tree);
        let (skewed_w, skewed_h) = (tree.node_width(left), tree.node_height(below));

        // `ver`: heights are levelled, the skewed widths are left alone.
        set_eadirection("ver");
        tree.equalize();
        assert_eq!(
            tree.node_width(left),
            skewed_w,
            "widths must not be touched"
        );
        assert_eq!(tree.node_height(below), tree.node_height(right));

        // `hor`: widths are levelled, the skewed heights are left alone.
        skew(&mut tree);
        set_eadirection("hor");
        tree.equalize();
        assert_eq!(
            tree.node_height(below),
            skewed_h,
            "heights must not be touched"
        );
        assert_eq!(
            tree.node_width(left),
            tree.node_width(right) + 1,
            "the vsplit gap goes to the right pane"
        );

        // `both` (the default) levels everything.
        skew(&mut tree);
        set_eadirection("both");
        tree.equalize();
        assert_eq!(tree.node_height(below), tree.node_height(right));
        assert_ne!(tree.node_width(left), skewed_w);
    }
}
