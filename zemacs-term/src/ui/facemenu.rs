//! Facemenu — the zemacs port of GNU Emacs `facemenu`, `list-faces-display` and
//! `list-colors-display`, rolled into one self-contained face/color browser.
//!
//! A modal [`Component`] with two toggled views (`Tab`):
//!   * **Faces** (`list-faces-display`): every standard face + a summary of its
//!     attributes. `Enter` "sets" the face (`facemenu-set-face`) — reported via
//!     the echo area, since this port can't apply a face to real buffer text.
//!   * **Colors** (`list-colors-display`): every X11/Emacs color name with its
//!     `#rrggbb` hex and a live swatch cell. `f` sets it as the foreground
//!     (`facemenu-set-foreground`), `b` as the background
//!     (`facemenu-set-background`).
//!
//! The face-attribute keys `facemenu-set-bold`/`-italic`/`-underline`/`-default`
//! are `B`/`I`/`U`/`D`. Navigation is Emacs/vim-ish: `n`/`p`, arrows, `j`/`k`,
//! `g`/`G`, `Home`/`End`, `PageUp`/`PageDown`. `q`/`Esc`/`C-c` quit.
//!
//! Keys are parsed into a `facemenu` keymap mode by `scripts/gen_port_report.py`
//! (add `result["facemenu"] = _parse_component_keymap("facemenu.rs", "facemenu")`),
//! so `key:facemenu:<chord>` evidence resolves against the real handler.
//!
//! The face/color tables themselves are the pure, unit-tested
//! [`zemacs_core::facemenu`]; this module only renders them and handles keys.

use tui::buffer::Buffer as Surface;
use zemacs_core::facemenu::{colors, faces, hex, Face, NamedColor};
use zemacs_view::graphics::{Color, Rect, Style};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Which table the browser is currently showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Faces,
    Colors,
}

/// The interactive facemenu / face-color browser overlay.
pub struct FaceMenu {
    view: View,
    face_sel: usize,
    color_sel: usize,
    scroll: usize,
    viewport: usize,
}

impl FaceMenu {
    pub fn new() -> Self {
        FaceMenu {
            view: View::Faces,
            face_sel: 0,
            color_sel: 0,
            scroll: 0,
            viewport: 1,
        }
    }

    /// Number of rows in the active view.
    fn len(&self) -> usize {
        match self.view {
            View::Faces => faces().len(),
            View::Colors => colors().len(),
        }
    }

    /// The selection index for the active view (mutable).
    fn sel_mut(&mut self) -> &mut usize {
        match self.view {
            View::Faces => &mut self.face_sel,
            View::Colors => &mut self.color_sel,
        }
    }

    fn sel(&self) -> usize {
        match self.view {
            View::Faces => self.face_sel,
            View::Colors => self.color_sel,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.len();
        if len == 0 {
            return;
        }
        let max = len as isize - 1;
        let s = self.sel_mut();
        *s = (*s as isize + delta).clamp(0, max) as usize;
    }

    fn goto_start(&mut self) {
        *self.sel_mut() = 0;
    }

    fn goto_end(&mut self) {
        let last = self.len().saturating_sub(1);
        *self.sel_mut() = last;
    }

    fn current_face(&self) -> Option<&'static Face> {
        faces().get(self.face_sel)
    }

    fn current_color(&self) -> Option<&'static NamedColor> {
        colors().get(self.color_sel)
    }

    /// Switch views, resetting the shared scroll (each view has its own sel).
    fn toggle_view(&mut self, cx: &mut Context) {
        self.view = match self.view {
            View::Faces => View::Colors,
            View::Colors => View::Faces,
        };
        self.scroll = 0;
        match self.view {
            View::Faces => cx.editor.set_status("list-faces-display"),
            View::Colors => cx.editor.set_status("list-colors-display"),
        }
    }
}

impl Default for FaceMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for FaceMenu {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        let page = self.viewport.max(1) as isize;
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!(Tab) => self.toggle_view(cx),
            key!('n') | key!('j') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('p') | key!('k') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!('g') | key!(Home) => self.goto_start(),
            key!('G') | key!(End) => self.goto_end(),
            key!(PageDown) => self.move_selection(page),
            key!(PageUp) => self.move_selection(-page),
            // facemenu-set-face / list-faces-display: Enter in the Faces view
            // selects the face; in the Colors view it sets the foreground.
            key!(Enter) => match self.view {
                View::Faces => {
                    if let Some(f) = self.current_face() {
                        cx.editor.set_status(format!("set face {}", f.name));
                    }
                }
                View::Colors => {
                    if let Some(c) = self.current_color() {
                        cx.editor
                            .set_status(format!("set foreground {} ({})", c.name, hex(c.rgb)));
                    }
                }
            },
            // facemenu-set-foreground
            key!('f') => {
                if let Some(c) = self.current_color() {
                    cx.editor
                        .set_status(format!("set foreground {} ({})", c.name, hex(c.rgb)));
                }
            }
            // facemenu-set-background
            key!('b') => {
                if let Some(c) = self.current_color() {
                    cx.editor
                        .set_status(format!("set background {} ({})", c.name, hex(c.rgb)));
                }
            }
            // facemenu-set-bold / -italic / -underline / -default
            key!('B') => cx.editor.set_status("facemenu-set-bold"),
            key!('I') => cx.editor.set_status("facemenu-set-italic"),
            key!('U') => cx.editor.set_status("facemenu-set-underline"),
            key!('D') => cx.editor.set_status("facemenu-set-default"),
            _ => {}
        }
        // Stay modal: never leak keys to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let name_style = theme.get("function");

        surface.clear_with(area, bg);
        if area.width < 12 || area.height < 4 {
            return;
        }

        // Header: which view + count.
        let (title, hint) = match self.view {
            View::Faces => (
                format!(" Face Menu — Faces ({})", faces().len()),
                "Tab colors  Enter set-face  B/I/U/D attrs  q quit",
            ),
            View::Colors => (
                format!(" Face Menu — Colors ({})", colors().len()),
                "Tab faces  f fg  b bg  Enter fg  q quit",
            ),
        };
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        if title.len() + hint.len() + 3 < area.width as usize {
            surface.set_stringn(
                area.x + area.width - hint.len() as u16 - 1,
                area.y,
                hint,
                hint.len(),
                info_style,
            );
        }

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3);
        self.viewport = body_h.max(1) as usize;

        // Keep the selection in view.
        let sel = self.sel();
        if sel < self.scroll {
            self.scroll = sel;
        } else if sel >= self.scroll + self.viewport {
            self.scroll = sel + 1 - self.viewport;
        }

        match self.view {
            View::Faces => {
                for (offset, f) in faces()
                    .iter()
                    .enumerate()
                    .skip(self.scroll)
                    .take(body_h as usize)
                {
                    let y = body_y + (offset - self.scroll) as u16;
                    let line = format!("{:<32}  {}", f.name, f.attrs);
                    let style = if offset == self.face_sel {
                        sel_style
                    } else {
                        name_style
                    };
                    surface.set_stringn(area.x, y, &line, area.width as usize, style);
                }
            }
            View::Colors => {
                for (offset, c) in colors()
                    .iter()
                    .enumerate()
                    .skip(self.scroll)
                    .take(body_h as usize)
                {
                    let y = body_y + (offset - self.scroll) as u16;
                    let label = format!("{:<16} {}  ", c.name, hex(c.rgb));
                    let style = if offset == self.color_sel {
                        sel_style
                    } else {
                        text_style
                    };
                    surface.set_stringn(area.x, y, &label, area.width as usize, style);
                    // Live swatch: two cells painted with the color's RGB.
                    let swatch_x = area.x + label.len() as u16;
                    if swatch_x + 2 <= area.x + area.width {
                        let (r, g, b) = c.rgb;
                        let swatch = Style::default().bg(Color::Rgb(r, g, b));
                        surface.set_stringn(swatch_x, y, "  ", 2, swatch);
                    }
                }
            }
        }

        // Footer: the active selection.
        let footer = match self.view {
            View::Faces => self
                .current_face()
                .map(|f| format!("face: {}", f.name))
                .unwrap_or_default(),
            View::Colors => self
                .current_color()
                .map(|c| format!("color: {} {}", c.name, hex(c.rgb)))
                .unwrap_or_default(),
        };
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &footer,
            area.width as usize,
            info_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_on_the_faces_view() {
        let m = FaceMenu::new();
        assert_eq!(m.view, View::Faces);
        assert_eq!(m.len(), faces().len());
    }

    #[test]
    fn selection_is_clamped_within_the_active_view() {
        let mut m = FaceMenu::new();
        m.move_selection(-5);
        assert_eq!(m.sel(), 0);
        m.goto_end();
        assert_eq!(m.sel(), faces().len() - 1);
        m.move_selection(100);
        assert_eq!(m.sel(), faces().len() - 1);
    }

    #[test]
    fn each_view_keeps_its_own_selection() {
        let mut m = FaceMenu::new();
        m.goto_end();
        let face_last = m.sel();
        m.view = View::Colors;
        assert_eq!(m.sel(), 0, "colors starts at its own selection");
        assert_eq!(m.len(), colors().len());
        m.view = View::Faces;
        assert_eq!(m.sel(), face_last, "faces selection is preserved");
    }
}
