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
        // Data-loss guard: autosave fires on every change, so a transient buffer
        // glitch (a stray edit, a botched filter/script, or a buffer that opened
        // with the wrong content) would be written to disk instantly and silently
        // truncate the file. Refuse to autosave when the buffer has collapsed to a
        // tiny fraction of — or emptied out — a file that has real content on disk.
        // The change isn't lost (it stays in the buffer and undo history); the user
        // can still `:w!` to force it. Explicit `:w` is unaffected.
        if let Some((path, buf_len, disk_len)) = editor.documents.get(&doc_id).and_then(|doc| {
            let path = doc.path()?;
            let buf_len = doc.text().len_bytes() as u64;
            let disk_len = std::fs::metadata(path).ok()?.len();
            is_catastrophic_truncation(buf_len, disk_len)
                .then(|| (path.to_path_buf(), buf_len, disk_len))
        }) {
            editor.set_error(format!(
                "autosave skipped: buffer ({buf_len} B) is far smaller than {} ({disk_len} B) on disk — use :w! to overwrite",
                path.display()
            ));
            continue;
        }

        if let Err(err) = editor.save::<PathBuf>(doc_id, None, false) {
            editor.set_error(format!("autosave failed: {err}"));
        }
    }
}

/// Would writing a `buf_len`-byte buffer over a file that is currently `disk_len`
/// bytes catastrophically truncate it? True when the buffer has emptied out, or
/// collapsed to under a tenth of a file that has real content on disk — the
/// signature of a glitch rather than an intentional edit. Used to stop autosave
/// (which fires on every keystroke) from silently destroying a file; never blocks
/// a brand-new file (`disk_len == 0`) or normal shrinking edits.
fn is_catastrophic_truncation(buf_len: u64, disk_len: u64) -> bool {
    (buf_len == 0 && disk_len > 16) || (disk_len > 256 && buf_len * 10 < disk_len)
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

#[cfg(test)]
mod tests {
    use super::is_catastrophic_truncation;

    #[test]
    fn blocks_collapse_to_garbage() {
        // The reported bug: a 521-line README (~7 KB) overwritten with `{a}` (4 B)
        // or `(something)` (11 B).
        assert!(is_catastrophic_truncation(4, 7000));
        assert!(is_catastrophic_truncation(11, 7000));
        // Emptied buffer over a file with real content.
        assert!(is_catastrophic_truncation(0, 100));
    }

    #[test]
    fn allows_normal_and_new_files() {
        // Brand-new file (nothing on disk yet) always saves.
        assert!(!is_catastrophic_truncation(0, 0));
        assert!(!is_catastrophic_truncation(50, 0));
        // Emptying a trivially small file is not treated as catastrophic.
        assert!(!is_catastrophic_truncation(0, 10));
        // Ordinary edits: deleting up to ~half a file, or growing it.
        assert!(!is_catastrophic_truncation(600, 1000));
        assert!(!is_catastrophic_truncation(5000, 4000));
        // A small file shrinking is fine (under the substantial-file threshold).
        assert!(!is_catastrophic_truncation(10, 200));
    }
}
