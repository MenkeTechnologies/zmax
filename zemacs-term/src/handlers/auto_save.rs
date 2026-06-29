use std::{path::PathBuf, time::Duration};

use anyhow::Ok;
use arc_swap::access::Access;

use tokio::time::Instant;
use zemacs_event::{register_hook, send_blocking};
use zemacs_view::{
    document::Mode,
    events::DocumentDidChange,
    handlers::{AutoSaveEvent, Handlers},
    DocumentId, Editor,
};

use crate::{events::OnModeSwitch, job};

#[derive(Debug, Default)]
pub(super) struct AutoSaveHandler;

impl AutoSaveHandler {
    pub fn new() -> AutoSaveHandler {
        AutoSaveHandler
    }
}

impl zemacs_event::AsyncHook for AutoSaveHandler {
    type Event = AutoSaveEvent;

    fn handle_event(
        &mut self,
        event: Self::Event,
        existing_debounce: Option<tokio::time::Instant>,
    ) -> Option<Instant> {
        match event {
            // JetBrains-style: save as soon as the editor goes idle for an
            // instant. Coalesces bursts (e.g. a paste or held key) into a
            // single save instead of one write per keystroke, but with no
            // user-perceptible delay.
            Self::Event::SaveNow => Some(Instant::now()),
            Self::Event::DocumentChanged { save_after } => {
                Some(Instant::now() + Duration::from_millis(save_after))
            }
            Self::Event::LeftInsertMode => {
                // If a change is already pending, let its debounce run down;
                // otherwise flush any outstanding changes immediately.
                if existing_debounce.is_some() {
                    existing_debounce
                } else {
                    Some(Instant::now())
                }
            }
        }
    }

    fn finish_debounce(&mut self) {
        job::dispatch_blocking(move |editor, _| save_changed_docs(editor));
    }
}

/// Save every modified, on-disk document immediately.
///
/// Unlike the `:write-all` command path, this does **not** commit the
/// in-flight insert-mode changeset to history, so autosaving while typing
/// never fragments the undo history (the original reason Helix deferred
/// autosave until leaving insert mode). It also skips formatting / code
/// actions so the cursor and buffer are never disturbed mid-edit.
fn save_changed_docs(editor: &mut Editor) {
    let to_save: Vec<DocumentId> = editor
        .documents
        .values()
        .filter(|doc| doc.path().is_some() && doc.is_modified())
        .map(|doc| doc.id())
        .collect();

    for doc_id in to_save {
        if let Err(err) = editor.save::<PathBuf>(doc_id, None, false) {
            editor.set_error(format!("autosave failed: {err}"));
        }
    }
}

pub(super) fn register_hooks(handlers: &Handlers) {
    let tx = handlers.auto_save.clone();
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        // Ignore programmatic/ghost edits (e.g. inline completion previews).
        if event.ghost_transaction {
            return Ok(());
        }
        let config = event.doc.config.load();
        if config.auto_save.on_change {
            send_blocking(&tx, AutoSaveEvent::SaveNow);
        } else if config.auto_save.after_delay.enable {
            send_blocking(
                &tx,
                AutoSaveEvent::DocumentChanged {
                    save_after: config.auto_save.after_delay.timeout,
                },
            );
        }
        Ok(())
    });

    let tx = handlers.auto_save.clone();
    register_hook!(move |event: &mut OnModeSwitch<'_, '_>| {
        if event.old_mode == Mode::Insert {
            let config = event.cx.editor.config();
            if config.auto_save.on_change || config.auto_save.after_delay.enable {
                send_blocking(&tx, AutoSaveEvent::LeftInsertMode);
            }
        }
        Ok(())
    });
}
