//! A ratatui-rendered right-click context menu with submenus, separators, and a
//! right-aligned shortcut-hint column — modeled on the JetBrains project-view
//! menu.
//!
//! It is self-contained: the root list plus any open submenu panels are all
//! drawn and event-routed by this one component (no nested compositor layers),
//! and it consumes every mouse event inside itself so choosing an item never
//! leaks to the file tree / editor beneath. Keys: j/k or ↑/↓ move, →/Enter open
//! a submenu or activate an item, ←/Esc back out (or close), click to activate,
//! click outside to dismiss.

use tui::buffer::Buffer as Surface;
use zemacs_core::Position;
use zemacs_view::{
    graphics::Rect,
    input::{MouseButton, MouseEventKind},
    keyboard::KeyCode,
};

use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};

pub const ID: &str = "context-menu";

/// One row of a menu: an action item (with an optional shortcut hint), a submenu,
/// or a separator line.
pub enum Entry {
    Item {
        label: String,
        shortcut: String,
        action: Option<Callback>,
    },
    Sub {
        label: String,
        children: Vec<Entry>,
    },
    Sep,
}

impl Entry {
    pub fn item(label: impl Into<String>, action: impl FnOnce(&mut Compositor, &mut Context) + 'static) -> Self {
        Entry::Item {
            label: label.into(),
            shortcut: String::new(),
            action: Some(Box::new(action)),
        }
    }

    pub fn item_key(
        label: impl Into<String>,
        shortcut: impl Into<String>,
        action: impl FnOnce(&mut Compositor, &mut Context) + 'static,
    ) -> Self {
        Entry::Item {
            label: label.into(),
            shortcut: shortcut.into(),
            action: Some(Box::new(action)),
        }
    }

    pub fn sub(label: impl Into<String>, children: Vec<Entry>) -> Self {
        Entry::Sub {
            label: label.into(),
            children,
        }
    }

    pub fn sep() -> Self {
        Entry::Sep
    }

    fn is_selectable(&self) -> bool {
        !matches!(self, Entry::Sep)
    }
}

/// A rendered panel (root or an open submenu), tracked for mouse hit-testing.
struct Panel {
    rect: Rect,
    depth: usize,
}

pub struct ContextMenu {
    entries: Vec<Entry>,
    anchor: Position,
    /// Indices of the submenus currently open, outermost first.
    open: Vec<usize>,
    /// Selected row within the deepest open list.
    sel: usize,
    panels: Vec<Panel>,
}

impl ContextMenu {
    pub fn new(row: u16, col: u16, entries: Vec<Entry>) -> Self {
        let mut menu = Self {
            entries,
            anchor: Position::new(row as usize, col as usize),
            open: Vec::new(),
            sel: 0,
            panels: Vec::new(),
        };
        menu.sel = menu.first_selectable(&[]);
        menu
    }

    fn close() -> Callback {
        Box::new(|compositor: &mut Compositor, _cx: &mut Context| {
            compositor.remove(ID);
        })
    }

    /// The entry list reached by following `path` submenu indices from the root.
    fn list_at<'a>(&'a self, path: &[usize]) -> &'a Vec<Entry> {
        let mut list = &self.entries;
        for &i in path {
            match list.get(i) {
                Some(Entry::Sub { children, .. }) => list = children,
                _ => break,
            }
        }
        list
    }

    /// Take (remove) the action of the item at `path`/`idx`, if it is an Item.
    /// Returns an owned callback so no `&mut` into the tree escapes (which the
    /// borrow checker rejects for the descend-then-return pattern).
    fn take_action(entries: &mut Vec<Entry>, path: &[usize], idx: usize) -> Option<Callback> {
        match path.split_first() {
            None => match entries.get_mut(idx) {
                Some(Entry::Item { action, .. }) => action.take(),
                _ => None,
            },
            Some((&i, rest)) => match entries.get_mut(i) {
                Some(Entry::Sub { children, .. }) => Self::take_action(children, rest, idx),
                _ => None,
            },
        }
    }

    fn deepest(&self) -> &Vec<Entry> {
        self.list_at(&self.open)
    }

    fn first_selectable(&self, path: &[usize]) -> usize {
        self.list_at(path)
            .iter()
            .position(Entry::is_selectable)
            .unwrap_or(0)
    }

    fn step(&mut self, delta: isize) {
        let n = self.deepest().len();
        if n == 0 {
            return;
        }
        let mut i = self.sel as isize;
        for _ in 0..n {
            i = (i + delta).rem_euclid(n as isize);
            if self.deepest()[i as usize].is_selectable() {
                self.sel = i as usize;
                return;
            }
        }
    }

    /// Enter/→/click on the current selection: open a submenu or run an item.
    fn choose(&mut self, idx: usize) -> EventResult {
        let path = self.open.clone();
        // Classify the target first so no borrow of `self` spans the mutation.
        let is_sub = matches!(self.list_at(&path).get(idx), Some(Entry::Sub { .. }));
        let is_item = matches!(self.list_at(&path).get(idx), Some(Entry::Item { .. }));
        if is_sub {
            self.open.push(idx);
            let np = self.open.clone();
            self.sel = self.first_selectable(&np);
            return EventResult::Consumed(None);
        }
        if is_item {
            let action = Self::take_action(&mut self.entries, &path, idx);
            return match action {
                Some(action) => EventResult::Consumed(Some(Box::new(
                    move |compositor: &mut Compositor, cx: &mut Context| {
                        compositor.remove(ID);
                        action(compositor, cx);
                    },
                ))),
                None => EventResult::Consumed(Some(Self::close())),
            };
        }
        EventResult::Consumed(None)
    }

    /// Back out one level, or close the menu if already at the root.
    fn back(&mut self) -> EventResult {
        if let Some(idx) = self.open.pop() {
            self.sel = idx;
            EventResult::Consumed(None)
        } else {
            EventResult::Consumed(Some(Self::close()))
        }
    }

    /// Panel + content-row index under screen point (x, y), if any.
    fn hit(&self, x: u16, y: u16) -> Option<(usize, usize)> {
        for panel in &self.panels {
            let r = panel.rect;
            if x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height {
                if y > r.y && y + 1 < r.y + r.height {
                    return Some((panel.depth, (y - r.y - 1) as usize));
                }
                return None; // on the panel's border
            }
        }
        None
    }
}

/// Displayed width of a panel's rows (widest "label   shortcut"/"label ›"), + border/padding.
fn panel_width(list: &[Entry]) -> u16 {
    let inner = list
        .iter()
        .map(|e| match e {
            Entry::Item { label, shortcut, .. } => {
                let sc = if shortcut.is_empty() { 0 } else { shortcut.chars().count() + 3 };
                label.chars().count() + sc
            }
            Entry::Sub { label, .. } => label.chars().count() + 2,
            Entry::Sep => 0,
        })
        .max()
        .unwrap_or(6);
    (inner as u16) + 4
}

impl Component for ContextMenu {
    fn handle_event(&mut self, event: &Event, _ctx: &mut Context) -> EventResult {
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    self.step(1);
                    EventResult::Consumed(None)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.step(-1);
                    EventResult::Consumed(None)
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    // Only open submenus with →; don't activate items.
                    let idx = self.sel;
                    if matches!(self.deepest().get(idx), Some(Entry::Sub { .. })) {
                        return self.choose(idx);
                    }
                    EventResult::Consumed(None)
                }
                KeyCode::Left | KeyCode::Char('h') => self.back(),
                KeyCode::Enter => {
                    let idx = self.sel;
                    self.choose(idx)
                }
                KeyCode::Esc | KeyCode::Char('q') => self.back(),
                _ => EventResult::Consumed(None),
            },
            Event::Mouse(ev) => match ev.kind {
                MouseEventKind::Down(MouseButton::Left) => match self.hit(ev.column, ev.row) {
                    Some((depth, idx)) => {
                        self.open.truncate(depth);
                        if self.list_at(&self.open).get(idx).is_some_and(Entry::is_selectable) {
                            self.sel = idx;
                            self.choose(idx)
                        } else {
                            EventResult::Consumed(None)
                        }
                    }
                    None => EventResult::Consumed(Some(Self::close())),
                },
                MouseEventKind::Moved => {
                    if let Some((depth, idx)) = self.hit(ev.column, ev.row) {
                        self.open.truncate(depth);
                        if self.list_at(&self.open).get(idx).is_some_and(Entry::is_selectable) {
                            self.sel = idx;
                            // Auto-open a hovered submenu (JetBrains behavior).
                            if matches!(self.deepest().get(idx), Some(Entry::Sub { .. })) {
                                self.open.push(idx);
                                self.sel = self.first_selectable(&self.open.clone());
                            }
                        }
                    }
                    EventResult::Consumed(None)
                }
                MouseEventKind::ScrollDown => {
                    self.step(1);
                    EventResult::Consumed(None)
                }
                MouseEventKind::ScrollUp => {
                    self.step(-1);
                    EventResult::Consumed(None)
                }
                _ => EventResult::Consumed(None),
            },
            _ => EventResult::Ignored(None),
        }
    }

    fn render(&mut self, viewport: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::to_rat_style;
        use ratatui::widgets::{Block, BorderType, Borders};

        let theme = &ctx.editor.theme;
        let menu_style = theme.get("ui.menu");
        let sel_style = theme.get("ui.menu.selected");
        let dim_style = theme.get("ui.text.inactive");
        let border_style = theme.get("ui.window");

        self.panels.clear();

        // Walk the open chain: panel 0 = root, each subsequent panel is the open
        // submenu, placed to the right of its parent at the parent's selected row.
        let mut depth = 0usize;
        let mut panel_x = self.anchor.col as u16;
        let mut panel_y = self.anchor.row as u16;
        loop {
            let path: Vec<usize> = self.open[..depth].to_vec();
            let list = self.list_at(&path);
            let width = panel_width(list).min(viewport.width);
            let height = (list.len() as u16 + 2).min(viewport.height);

            let mut x = panel_x;
            let mut y = panel_y;
            if x + width > viewport.x + viewport.width {
                x = (viewport.x + viewport.width).saturating_sub(width);
            }
            if y + height > viewport.y + viewport.height {
                y = (viewport.y + viewport.height).saturating_sub(height);
            }
            let area = Rect::new(x, y, width, height);
            surface.clear_with(area, menu_style);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(to_rat_style(border_style))
                .style(to_rat_style(menu_style));
            crate::ui::rat::render(block, area, surface);

            // Selection highlight applies to the row selected in the DEEPEST panel.
            let inner_w = area.width.saturating_sub(2) as usize;
            for (i, entry) in list.iter().enumerate() {
                let ry = area.y + 1 + i as u16;
                if ry + 1 >= area.y + area.height {
                    break;
                }
                let is_sel = depth == self.open.len() && i == self.sel;
                match entry {
                    Entry::Sep => {
                        let line = "─".repeat(inner_w);
                        surface.set_stringn(area.x + 1, ry, &line, inner_w, border_style);
                    }
                    Entry::Item { label, shortcut, .. } => {
                        let style = if is_sel { sel_style } else { menu_style };
                        if is_sel {
                            surface.set_style(Rect::new(area.x + 1, ry, area.width - 2, 1), style);
                        }
                        surface.set_stringn(area.x + 1, ry, &format!(" {label}"), inner_w, style);
                        if !shortcut.is_empty() {
                            let sc = format!("{shortcut} ");
                            let scw = sc.chars().count() as u16;
                            let sx = area.x + area.width - 1 - scw;
                            let sc_style = if is_sel { style } else { dim_style };
                            surface.set_stringn(sx, ry, &sc, scw as usize, sc_style);
                        }
                    }
                    Entry::Sub { label, .. } => {
                        let style = if is_sel { sel_style } else { menu_style };
                        if is_sel {
                            surface.set_style(Rect::new(area.x + 1, ry, area.width - 2, 1), style);
                        }
                        surface.set_stringn(area.x + 1, ry, &format!(" {label}"), inner_w, style);
                        surface.set_stringn(area.x + area.width - 2, ry, "›", 1, style);
                    }
                }
            }

            self.panels.push(Panel { rect: area, depth });

            if depth < self.open.len() {
                // Position the next panel to the right, aligned to the open row.
                panel_x = area.x + area.width;
                panel_y = area.y + 1 + self.open[depth] as u16;
                depth += 1;
            } else {
                break;
            }
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
}
