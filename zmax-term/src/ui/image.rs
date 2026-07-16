//! Image — the zmax port of GNU Emacs `image-mode`'s transform keys.
//!
//! Like `doc-view-mode`, the picture is drawn straight to the terminal by
//! `commands::display_images_in_terminal` and the rotate/flip/scale state lives
//! in the `IMAGE_XFORM` the `image-*` typable commands already read and write.
//! This [`Component`] exists to own the *keymap*: without it the commands were
//! reachable only by name (`:image-rotate`), so none of Emacs's image keys
//! worked. Every key dispatches into the same helpers the typables use, so the
//! two paths cannot drift apart.
//!
//! Keys (parsed into an `image` keymap mode by `scripts/gen_port_report.py`).
//! Emacs splits them across two prefixes, and this follows the manual's Image
//! Mode node verbatim:
//!
//!   i + — `image-increase-size`      i - — `image-decrease-size`
//!   i r — `image-rotate` (90° cw)    i h — `image-flip-horizontally`
//!   i v — `image-flip-vertically`
//!   s w — `image-transform-fit-to-window`
//!   s o — `image-transform-reset-to-original`
//!   q / Esc — leave the viewer
//!
//! Deferred, each needing substrate that does not exist yet:
//!   i o (`image-save`), i c (`image-crop`), i x (`image-cut`) — writing a
//!     transformed image back out, and interactive region selection over it.
//!   s 0 (`image-transform-reset-to-initial`) — distinct from `-to-original`:
//!     it restores the *initial display* size (the auto-fit), which needs an
//!     auto-resize model zmax does not have.
//!   s p / s s (`image-transform-set-percent` / `-set-scale`) — both read a
//!     value, so they need a prompt rather than a bare chord; `:image-transform-
//!     set-percent 50` reaches them meanwhile.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;
use zmax_view::input::KeyEvent;

use crate::commands::typed::{image_set_scale, image_transform, image_transform_reset_all};
use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    key,
};

/// The viewer overlay. Holds no transform state of its own — `IMAGE_XFORM` stays
/// the single source of truth, so an `:image-rotate` typed while the overlay is
/// up stays in sync.
#[derive(Default)]
pub struct Image {
    /// `i` or `s` was typed and the next key names the transform.
    pending: Option<char>,
}

impl Image {
    pub fn new() -> Self {
        Self::default()
    }

    /// `i` then a key: Emacs's image-at-point transform map.
    fn dispatch_i_key(&mut self, cx: &mut Context, key: KeyEvent) -> anyhow::Result<()> {
        match key {
            // `image-increase-size` / `-decrease-size`: emacs steps by 20%, zmax
            // by the same 5/4 and 4/5 the typables use, so key and command agree.
            key!('+') => {
                let sc = current_scale(cx);
                image_set_scale(cx, (sc * 5 / 4).max(sc + 1))
            }
            key!('-') => {
                let sc = current_scale(cx);
                image_set_scale(cx, (sc * 4 / 5).max(1))
            }
            key!('r') => image_transform(cx, 90, false, false),
            key!('h') => image_transform(cx, 0, true, false),
            key!('v') => image_transform(cx, 0, false, true),
            _ => Ok(()),
        }
    }

    /// `s` then a key: Emacs's image-mode scaling map.
    fn dispatch_s_key(&mut self, cx: &mut Context, key: KeyEvent) -> anyhow::Result<()> {
        match key {
            key!('w') => image_set_scale(cx, 100),
            key!('o') => image_transform_reset_all(cx),
            _ => Ok(()),
        }
    }
}

/// The current image's scale, or 100% when there is no image (the callers then
/// fail with the same "not an image file" error the typables give).
fn current_scale(cx: &Context) -> u32 {
    crate::commands::typed::current_image_path(cx)
        .map(|p| crate::commands::typed::image_xform_of(&p).3)
        .unwrap_or(100)
}

impl Component for Image {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        if let Some(prefix) = self.pending.take() {
            let done = match prefix {
                'i' => self.dispatch_i_key(cx, key),
                _ => self.dispatch_s_key(cx, key),
            };
            report(cx, done);
            return EventResult::Consumed(None);
        }

        match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close)),
            key!('i') => self.pending = Some('i'),
            key!('s') => self.pending = Some('s'),
            _ => return EventResult::Ignored(None),
        }
        EventResult::Consumed(None)
    }

    /// The picture is painted straight to the terminal, so there is nothing to
    /// draw onto the `Surface` — clearing it would erase the image the terminal
    /// is already holding.
    fn render(&mut self, _area: Rect, _surface: &mut Surface, _ctx: &mut Context) {}

    fn id(&self) -> Option<&'static str> {
        Some("image")
    }
}

/// The helpers fail when the buffer stops being an image; say so on the status
/// line rather than dropping it, which is what the typable path does.
fn report(cx: &mut Context, result: anyhow::Result<()>) {
    if let Err(e) = result {
        cx.editor.set_error(e.to_string());
    }
}
