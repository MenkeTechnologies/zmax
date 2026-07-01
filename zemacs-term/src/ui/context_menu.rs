//! A ratatui-rendered right-click context menu.
//!
//! Unlike the generic `Popup<Menu>` (which tracks the editor cursor and, with
//! auto-close, lets the activating click fall through to the layer beneath), this
//! component is anchored to the click point and consumes every mouse event inside
//! itself — so choosing an item activates it instead of leaking to the file tree.
//! Keys: j/k or ↑/↓ move, Enter activates, Esc/q closes; click an item to run it,
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

/// One row in the menu: a label and the action run when it's chosen.
pub struct ContextItem {
    pub label: String,
    pub action: Callback,
}

impl ContextItem {
    pub fn new(
        label: impl Into<String>,
        action: impl FnOnce(&mut Compositor, &mut Context) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            action: Box::new(action),
        }
    }
}

pub struct ContextMenu {
    items: Vec<ContextItem>,
    selected: usize,
    anchor: Position,
    /// Rect the menu last rendered into, for mouse hit-testing.
    area: Rect,
}

impl ContextMenu {
    pub fn new(row: u16, col: u16, items: Vec<ContextItem>) -> Self {
        Self {
            items,
            selected: 0,
            anchor: Position::new(row as usize, col as usize),
            area: Rect::default(),
        }
    }

    fn close() -> Callback {
        Box::new(|compositor: &mut Compositor, _cx: &mut Context| {
            compositor.remove(ID);
        })
    }

    /// Close the menu and run item `idx`'s action.
    fn activate(&mut self, idx: usize) -> EventResult {
        if idx >= self.items.len() {
            return EventResult::Consumed(Some(Self::close()));
        }
        let action = self.items.remove(idx).action;
        EventResult::Consumed(Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.remove(ID);
                action(compositor, cx);
            },
        )))
    }

    /// Content-row index under screen row `y`, if it lands on an item (inside the
    /// border). Rows: `area.y` = top border, items follow, last = bottom border.
    fn item_at(&self, y: u16) -> Option<usize> {
        if y > self.area.y && y + 1 < self.area.y + self.area.height {
            let idx = (y - self.area.y - 1) as usize;
            (idx < self.items.len()).then_some(idx)
        } else {
            None
        }
    }
}

impl Component for ContextMenu {
    fn handle_event(&mut self, event: &Event, _ctx: &mut Context) -> EventResult {
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.selected + 1 < self.items.len() {
                        self.selected += 1;
                    }
                    EventResult::Consumed(None)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected = self.selected.saturating_sub(1);
                    EventResult::Consumed(None)
                }
                KeyCode::Enter => {
                    let idx = self.selected;
                    self.activate(idx)
                }
                KeyCode::Esc | KeyCode::Char('q') => EventResult::Consumed(Some(Self::close())),
                // Stay modal: swallow other keys rather than leaking to the editor.
                _ => EventResult::Consumed(None),
            },
            Event::Mouse(ev) => {
                let inside = ev.column >= self.area.x
                    && ev.column < self.area.x + self.area.width
                    && ev.row >= self.area.y
                    && ev.row < self.area.y + self.area.height;
                match ev.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if !inside {
                            // Dismiss on an outside click and consume it so the click
                            // doesn't also hit the file tree / editor beneath.
                            return EventResult::Consumed(Some(Self::close()));
                        }
                        match self.item_at(ev.row) {
                            Some(idx) => self.activate(idx),
                            None => EventResult::Consumed(None),
                        }
                    }
                    MouseEventKind::Moved => {
                        if let Some(idx) = self.item_at(ev.row) {
                            self.selected = idx;
                        }
                        EventResult::Consumed(None)
                    }
                    MouseEventKind::ScrollDown => {
                        if self.selected + 1 < self.items.len() {
                            self.selected += 1;
                        }
                        EventResult::Consumed(None)
                    }
                    MouseEventKind::ScrollUp => {
                        self.selected = self.selected.saturating_sub(1);
                        EventResult::Consumed(None)
                    }
                    _ => EventResult::Consumed(None),
                }
            }
            _ => EventResult::Ignored(None),
        }
    }

    fn render(&mut self, viewport: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::to_rat_style;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, BorderType, Borders, List, ListItem};

        let theme = &ctx.editor.theme;
        let menu_style = theme.get("ui.menu");
        let sel_style = theme.get("ui.menu.selected");

        // Size to the widest label (+ side padding) and clamp to the viewport.
        let inner_w = self
            .items
            .iter()
            .map(|i| i.label.chars().count())
            .max()
            .unwrap_or(4) as u16
            + 2;
        let width = (inner_w + 2).min(viewport.width);
        let height = (self.items.len() as u16 + 2).min(viewport.height);

        let mut x = self.anchor.col as u16;
        let mut y = self.anchor.row as u16;
        if x + width > viewport.x + viewport.width {
            x = (viewport.x + viewport.width).saturating_sub(width);
        }
        if y + height > viewport.y + viewport.height {
            y = (viewport.y + viewport.height).saturating_sub(height);
        }
        let area = Rect::new(x, y, width, height);
        self.area = area;

        surface.clear_with(area, menu_style);

        let items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let style = if i == self.selected { sel_style } else { menu_style };
                let label = format!(" {} ", it.label);
                ListItem::new(Line::from(Span::styled(label, to_rat_style(style))))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(to_rat_style(theme.get("ui.window")))
            .style(to_rat_style(menu_style));
        let list = List::new(items).block(block);
        crate::ui::rat::render(list, area, surface);
    }

    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
}
