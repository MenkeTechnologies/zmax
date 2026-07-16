//! Records closed files onto the session "recently closed" stack so they can be
//! reopened with `reopen_last_closed` (see `crate::closed_files`).

use zmax_event::register_hook;
use zmax_view::{events::DocumentDidClose, handlers::Handlers};

pub(super) fn register_hooks(_handlers: &Handlers) {
    register_hook!(move |event: &mut DocumentDidClose<'_>| {
        if let Some(path) = event.doc.path() {
            crate::closed_files::push(path);
        }
        Ok(())
    });
}
