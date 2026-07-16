//! Facemenu — the zmax port of GNU Emacs `facemenu`, `list-faces-display` and
//! `list-colors-display`, rolled into one self-contained face/color browser.
//!
//! A modal [`Component`] with two toggled views (`Tab`):
//!   * **Faces** (`list-faces-display`): every standard face + a summary of its
//!     attributes. `Enter` applies the face to the region (`facemenu-set-face`)
//!     as a face text property.
//!   * **Colors** (`list-colors-display`): every X11/Emacs color name with its
//!     `#rrggbb` hex and a live swatch cell. `f` sets it as the foreground
//!     (`facemenu-set-foreground`), `b` as the background
//!     (`facemenu-set-background`).
//!
//! The face-attribute keys `facemenu-set-bold`/`-italic`/`-underline`/`-default`
//! are `B`/`I`/`U`/`D`. Navigation is Emacs/vim-ish: `n`/`p`, arrows, `j`/`k`,
//! `g`/`G`, `Home`/`End`, `PageUp`/`PageDown`. `q`/`Esc`/`C-c` quit.
//!
//! Everything the menu applies goes through `commands::facemenu_add_face`, which
//! puts the face on the region as a real text property
//! ([`zmax_core::text_props`]) — the same store `enriched-mode` saves and
//! reloads. Opened from `facemenu-set-face` / `-foreground` / `-background`, the
//! menu is a one-shot *picker*: choosing closes it.
//!
//! Keys are parsed into a `facemenu` keymap mode by `scripts/gen_port_report.py`
//! (add `result["facemenu"] = _parse_component_keymap("facemenu.rs", "facemenu")`),
//! so `key:facemenu:<chord>` evidence resolves against the real handler.
//!
//! The face/color tables themselves are the pure, unit-tested
//! [`zmax_core::facemenu`]; this module only renders them and handles keys.

use tui::buffer::Buffer as Surface;
use zmax_core::facemenu::{colors, faces, hex, Face, NamedColor};
use zmax_view::graphics::{Color, Rect, Style};

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

/// The face text property a name from `list-faces-display` stands for.
///
/// The five attribute faces are attribute toggles, so they become exactly what
/// `facemenu-set-bold` and friends produce; every other face keeps its Emacs name
/// and is painted from the theme scope
/// [`zmax_core::facemenu::theme_scope`] resolves it to.
fn face_for_name(name: &str) -> zmax_core::text_props::Face {
    use zmax_core::text_props::Face as PropFace;
    match name {
        "default" => PropFace::default(),
        "bold" => PropFace::bold(),
        "italic" => PropFace::italic(),
        "bold-italic" => PropFace::bold_italic(),
        "underline" => PropFace::underline(),
        other => PropFace::named(other),
    }
}

/// What a `facemenu-set-*` command opened the menu to pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// `facemenu-set-face`: pick a named face.
    Face,
    /// `facemenu-set-foreground`: pick the region's foreground colour.
    Foreground,
    /// `facemenu-set-background`: pick the region's background colour.
    Background,
}

impl Target {
    /// The view the picker opens on.
    fn view(self) -> View {
        match self {
            Target::Face => View::Faces,
            Target::Foreground | Target::Background => View::Colors,
        }
    }
}

/// The interactive facemenu / face-color browser overlay.
pub struct FaceMenu {
    view: View,
    face_sel: usize,
    color_sel: usize,
    scroll: usize,
    viewport: usize,
    /// `Some` when a `facemenu-set-*` command opened the menu to pick one thing:
    /// the choice is applied to the region and the menu closes.
    target: Option<Target>,
}

impl FaceMenu {
    pub fn new() -> Self {
        FaceMenu {
            view: View::Faces,
            face_sel: 0,
            color_sel: 0,
            scroll: 0,
            viewport: 1,
            target: None,
        }
    }

    /// The one-shot picker `facemenu-set-face` / `-foreground` / `-background`
    /// open: it starts on the right view and closes once something is chosen.
    pub fn picker(target: Target) -> Self {
        FaceMenu {
            view: target.view(),
            target: Some(target),
            ..FaceMenu::new()
        }
    }

    /// Put `face` on the region and report it. The face menu never edits text —
    /// it only adds a text property, so there is nothing to undo through the
    /// history.
    fn apply(&self, cx: &mut Context, face: zmax_core::text_props::Face, label: &str) {
        crate::commands::facemenu_add_face(cx.editor, &face, label);
    }

    /// Apply whatever the current row is, for the `target` the menu was opened
    /// with. Returns false when the row cannot produce a face (an empty table).
    fn apply_target(&self, cx: &mut Context, target: Target) -> bool {
        use zmax_core::text_props::Face as PropFace;
        match target {
            Target::Face => match self.current_face() {
                Some(f) => {
                    self.apply(cx, face_for_name(f.name), "facemenu-set-face");
                    true
                }
                None => false,
            },
            Target::Foreground => match self.current_color() {
                Some(c) => {
                    self.apply(cx, PropFace::foreground(c.rgb), "facemenu-set-foreground");
                    true
                }
                None => false,
            },
            Target::Background => match self.current_color() {
                Some(c) => {
                    self.apply(cx, PropFace::background(c.rgb), "facemenu-set-background");
                    true
                }
                None => false,
            },
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
        use zmax_core::text_props::Face as PropFace;
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!(Tab) => self.toggle_view(cx),
            key!('n') | key!('j') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('p') | key!('k') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!('g') | key!(Home) => self.goto_start(),
            key!('G') | key!(End) => self.goto_end(),
            key!(PageDown) => self.move_selection(page),
            key!(PageUp) => self.move_selection(-page),

            // The setters. Each puts a real face text property on the region
            // (`Document::text_props`), which the editor renders and
            // `enriched-mode` saves. A picker opened by `facemenu-set-face` /
            // `-foreground` / `-background` closes as soon as it has its answer.
            key!(Enter) => {
                let target = self.target.unwrap_or(match self.view {
                    View::Faces => Target::Face,
                    View::Colors => Target::Foreground,
                });
                if self.apply_target(cx, target) && self.target.is_some() {
                    return EventResult::Consumed(Some(close));
                }
            }
            key!('f') if self.view == View::Colors => {
                if self.apply_target(cx, Target::Foreground) && self.target.is_some() {
                    return EventResult::Consumed(Some(close));
                }
            }
            key!('b') if self.view == View::Colors => {
                if self.apply_target(cx, Target::Background) && self.target.is_some() {
                    return EventResult::Consumed(Some(close));
                }
            }
            key!('B') => self.apply(cx, PropFace::bold(), "facemenu-set-bold"),
            key!('I') => self.apply(cx, PropFace::italic(), "facemenu-set-italic"),
            key!('L') => self.apply(cx, PropFace::bold_italic(), "facemenu-set-bold-italic"),
            key!('U') => self.apply(cx, PropFace::underline(), "facemenu-set-underline"),
            key!('D') => self.apply(cx, PropFace::default(), "facemenu-set-default"),
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
                "Tab colors  Enter set-face  B/I/L/U/D attrs  q quit",
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
    fn attribute_faces_become_attribute_toggles_not_theme_scopes() {
        use zmax_core::text_props::Face as PropFace;
        assert_eq!(face_for_name("bold"), PropFace::bold());
        assert_eq!(face_for_name("bold-italic"), PropFace::bold_italic());
        assert_eq!(face_for_name("underline"), PropFace::underline());
        assert!(face_for_name("default").is_default());
        // Everything else keeps its Emacs name so the theme can paint it.
        assert_eq!(
            face_for_name("font-lock-string-face").name.as_deref(),
            Some("font-lock-string-face")
        );
    }

    #[test]
    fn a_picker_opens_on_the_view_its_command_needs() {
        assert_eq!(FaceMenu::picker(Target::Face).view, View::Faces);
        assert_eq!(FaceMenu::picker(Target::Foreground).view, View::Colors);
        assert_eq!(FaceMenu::picker(Target::Background).view, View::Colors);
        assert!(
            FaceMenu::new().target.is_none(),
            "M-x facemenu just browses"
        );
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
