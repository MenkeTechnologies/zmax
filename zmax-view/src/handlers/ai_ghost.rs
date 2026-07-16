//! Event type for the AI ghost-text (inline completion) handler. The handler implementation lives
//! in `zmax-term` (it needs the AI provider); this crate only owns the event the `Handlers` struct
//! carries a sender for.

use crate::{DocumentId, ViewId};

#[derive(Debug)]
pub enum GhostEvent {
    /// The user typed; (re)arm the debounced suggestion request at `cursor`.
    Trigger {
        cursor: usize,
        doc: DocumentId,
        view: ViewId,
    },
    /// Cancel any pending or in-flight suggestion (cursor moved, left insert mode, etc.).
    Cancel,
}
