//! DocView — the zmax port of GNU Emacs `doc-view-mode`, the PDF/PS/DVI/EPUB
//! viewer.
//!
//! The page itself is drawn straight to the terminal by
//! `commands::display_doc_page_in_terminal` (terminal graphics, not a
//! `Surface`), and the current page/resolution live in the `DOCVIEW` state that
//! the `doc-view-*` typable commands already read and write. This [`Component`]
//! exists to own the *keymap*: without it those commands were reachable only by
//! name (`:doc-view-next-page`), so none of Emacs's DocView keys worked. Every
//! key here dispatches into the same `docview_step` / `docview_zoom` helpers the
//! typables use, so the two paths cannot drift apart.
//!
//! Keys (parsed into a `docview` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs DocView counterpart in the port tracker). The bindings
//! follow the Emacs manual's DocView Navigation node verbatim:
//!   n / PageDown / next / C-x ] — next page (`doc-view-next-page`)
//!   p / PageUp / prior / C-x [  — previous page (`doc-view-previous-page`)
//!   SPC — scroll or advance (`doc-view-scroll-up-or-next-page`)
//!   DEL — scroll or retreat (`doc-view-scroll-down-or-previous-page`)
//!   M-< — first page (`doc-view-first-page`)
//!   M-> — last page (`doc-view-last-page`)
//!   `+` — enlarge (`doc-view-enlarge`)
//!   `-` — shrink (`doc-view-shrink`)
//!   q / Esc — leave the viewer
//!
//! `SPC`/`DEL` advance a whole page rather than scrolling within one: the page is
//! rendered as a single terminal image, so there is nothing to scroll inside it.
//! They therefore share the next/previous handlers.
//!
//! Deferred: `doc-view-set-slice-using-mouse` and `doc-view-show-tooltip`, which
//! need pointer hit-testing against the rendered image.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;
use zmax_view::input::KeyEvent;

use crate::commands::typed::{docview_step, docview_zoom, DocPage};
use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// One `+`/`-` press, matching `doc-view-enlarge` / `doc-view-shrink`.
const ZOOM_STEP: i32 = 25;

/// The viewer overlay. Holds no page state of its own — `DOCVIEW` is the single
/// source of truth, so a `:doc-view-goto-page` typed while the overlay is up
/// stays in sync.
#[derive(Default)]
pub struct DocView {
    /// `C-x` was typed and the next key decides whether it is `C-x [` or `C-x ]`.
    pending_ctrl_x: bool,
}

impl DocView {
    pub fn new() -> Self {
        Self::default()
    }

    /// `C-x` then a key: Emacs's `C-x [` / `C-x ]` page pair. Kept in its own fn
    /// so the chords read as the two-key sequences they are.
    fn dispatch_ctrl_x_key(&mut self, cx: &mut Context, key: KeyEvent) -> anyhow::Result<()> {
        match key {
            key!('[') => docview_step(cx, DocPage::Prev),
            key!(']') => docview_step(cx, DocPage::Next),
            _ => Ok(()),
        }
    }
}

impl Component for DocView {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        // `C-x [` / `C-x ]` are two-key chords; a Component has no keymap trie, so
        // the prefix is tracked by hand as the prompt does it.
        if std::mem::take(&mut self.pending_ctrl_x) {
            let stepped = self.dispatch_ctrl_x_key(cx, key);
            report(cx, stepped);
            return EventResult::Consumed(None);
        }

        let done = match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close)),
            ctrl!('x') => {
                self.pending_ctrl_x = true;
                Ok(())
            }
            // Next page. SPC is `doc-view-scroll-up-or-next-page`: the page is one
            // image, so there is nothing to scroll within and it advances.
            key!('n') | key!(PageDown) | key!(' ') => docview_step(cx, DocPage::Next),
            // Previous page; DEL is `doc-view-scroll-down-or-previous-page`.
            key!('p') | key!(PageUp) | key!(Backspace) => docview_step(cx, DocPage::Prev),
            alt!('<') => docview_step(cx, DocPage::First),
            alt!('>') => docview_step(cx, DocPage::Last),
            key!('+') => docview_zoom(cx, ZOOM_STEP),
            key!('-') => docview_zoom(cx, -ZOOM_STEP),
            _ => return EventResult::Ignored(None),
        };
        report(cx, done);
        EventResult::Consumed(None)
    }

    /// The page is painted straight to the terminal by the step/zoom commands, so
    /// there is nothing to draw onto the `Surface` — and clearing it would erase
    /// the image the terminal is already holding.
    fn render(&mut self, _area: Rect, _surface: &mut Surface, _ctx: &mut Context) {}

    fn id(&self) -> Option<&'static str> {
        Some("docview")
    }
}

/// The helpers fail when the buffer stops being a document; say so on the status
/// line rather than dropping it, which is what the typable path does.
fn report(cx: &mut Context, result: anyhow::Result<()>) {
    if let Err(e) = result {
        cx.editor.set_error(e.to_string());
    }
}
