//! Records opened files into the persistent recent-files list that backs the
//! startify start screen (see `crate::recent_files`).

use zmax_event::register_hook;
use zmax_view::{events::DocumentDidOpen, handlers::Handlers};

pub(super) fn register_hooks(_handlers: &Handlers) {
    register_hook!(move |event: &mut DocumentDidOpen<'_>| {
        let doc_id = event.doc;
        if let Some(path) = doc!(event.editor, &doc_id).path() {
            crate::recent_files::record(path);
        }
        Ok(())
    });
}
