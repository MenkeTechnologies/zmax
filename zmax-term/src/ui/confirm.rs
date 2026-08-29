//! Emacs `y-or-n-p`: a one-keystroke question.
//!
//! `y-or-n-p` reads a single key rather than a line — `y`/`Y`/`SPC` answer yes,
//! `n`/`N`/`DEL` answer no, `C-g` quits — so the confirmations emacs asks before
//! redefining an abbrev, overwriting a file or discarding a change commit on the
//! keystroke, with no `RET` after it. A text prompt is `yes-or-no-p`, which is a
//! different (deliberately heavier) question and is spelled out in full.

use crate::compositor::{Component, Context, Event, EventResult};

use zmax_view::graphics::Rect;
use zmax_view::input::{KeyCode, KeyModifiers};

use tui::buffer::Buffer as Surface;

/// What a `Confirm` runs when the answer is yes.
type OnYes = Box<dyn FnOnce(&mut Context)>;

/// The question, and what to run when it is answered yes.
pub struct Confirm {
    question: String,
    on_yes: Option<OnYes>,
    /// Status to report when the answer is no (emacs echoes nothing, but the
    /// caller usually has something to say about what it did *not* do).
    on_no: String,
}

impl Confirm {
    pub fn new(
        question: impl Into<String>,
        on_no: impl Into<String>,
        on_yes: impl FnOnce(&mut Context) + 'static,
    ) -> Self {
        Confirm {
            question: question.into(),
            on_yes: Some(Box::new(on_yes)),
            on_no: on_no.into(),
        }
    }
}

impl Component for Confirm {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        let close = |cx: &mut Context| {
            cx.editor.autoinfo = None;
        };
        let pop: crate::compositor::Callback =
            Box::new(|compositor: &mut crate::compositor::Compositor, _cx| {
                compositor.pop();
            });
        let ctrl_g = key.code == KeyCode::Char('g') && key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char(' ') => {
                close(cx);
                if let Some(on_yes) = self.on_yes.take() {
                    on_yes(cx);
                }
                EventResult::Consumed(Some(pop))
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Backspace | KeyCode::Delete => {
                close(cx);
                cx.editor.set_status(self.on_no.clone());
                EventResult::Consumed(Some(pop))
            }
            KeyCode::Esc => {
                close(cx);
                cx.editor.set_status("Quit");
                EventResult::Consumed(Some(pop))
            }
            _ if ctrl_g => {
                close(cx);
                cx.editor.set_status("Quit");
                EventResult::Consumed(Some(pop))
            }
            // Any other key re-asks, as `y-or-n-p` does.
            _ => EventResult::Consumed(None),
        }
    }

    fn render(&mut self, _area: Rect, _surface: &mut Surface, cx: &mut Context) {
        // The question lives on the status line, which is where emacs's
        // minibuffer prompt appears.
        cx.editor.set_status(self.question.clone());
    }
}
