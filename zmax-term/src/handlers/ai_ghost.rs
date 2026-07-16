//! Real-time AI ghost-text autocomplete (Cursor's "predict next edit" + "partial accept").
//!
//! After the user pauses typing in insert mode, a debounced background request asks the AI provider
//! to continue the code at the cursor (fill-in-the-middle). The reply is rendered as dimmed inline
//! virtual text — the "ghost" — anchored at the cursor (see `Document::set_ghost_text` and
//! `View::text_annotations`). `Tab` accepts the whole suggestion; a word-accept command takes it one
//! word at a time (partial accept). Any other edit or cursor move clears it.
//!
//! Opt-in: gated by [`crate::ai::autocomplete_enabled`] (off by default — it costs an inference call
//! per typing pause). Toggle with `SPC a g` or `ZMAX_AI_AUTOCOMPLETE=1`.

use std::time::Duration;

use tokio::time::Instant;
use zmax_event::{
    cancelable_future, register_hook, send_blocking, AsyncHook, TaskController, TaskHandle,
};
use zmax_view::document::Mode;
use zmax_view::handlers::ai_ghost::GhostEvent;
use zmax_view::{DocumentId, Editor, ViewId};

use crate::events::{OnModeSwitch, PostCommand, PostInsertChar};
use crate::job::{dispatch, dispatch_blocking};
use crate::keymap::MappableCommand;

use super::Handlers;

/// How long the user must pause typing before a suggestion is requested.
const GHOST_DEBOUNCE_MS: u64 = 500;

const GHOST_SYSTEM: &str = "You are an inline code-completion engine inside the zmax editor. \
Continue the code at the <CURSOR> marker. Output ONLY the raw text to insert at the cursor — no \
explanation, no markdown fences, and do not repeat the code that already follows the cursor. Keep \
the completion concise (a few lines at most).";

#[derive(Debug, Clone, Copy)]
struct GhostTrigger {
    pos: usize,
    doc: DocumentId,
    view: ViewId,
}

#[derive(Debug)]
pub struct GhostHandler {
    trigger: Option<GhostTrigger>,
    task_controller: TaskController,
}

impl GhostHandler {
    pub fn new() -> Self {
        Self {
            trigger: None,
            task_controller: TaskController::new(),
        }
    }
}

impl AsyncHook for GhostHandler {
    type Event = GhostEvent;

    fn handle_event(
        &mut self,
        event: Self::Event,
        _old_timeout: Option<Instant>,
    ) -> Option<Instant> {
        match event {
            GhostEvent::Trigger { cursor, doc, view } => {
                // Each keystroke cancels the previous in-flight request and restarts the timer.
                self.task_controller.cancel();
                self.trigger = Some(GhostTrigger {
                    pos: cursor,
                    doc,
                    view,
                });
                Some(Instant::now() + Duration::from_millis(GHOST_DEBOUNCE_MS))
            }
            GhostEvent::Cancel => {
                self.trigger = None;
                self.task_controller.cancel();
                None
            }
        }
    }

    fn finish_debounce(&mut self) {
        let Some(trigger) = self.trigger.take() else {
            return;
        };
        let handle = self.task_controller.restart();
        dispatch_blocking(move |editor, _compositor| request_ghost(trigger, handle, editor));
    }
}

/// Read the cursor context on the main thread, then spawn the (cancelable) inference request.
fn request_ghost(trigger: GhostTrigger, handle: TaskHandle, editor: &mut Editor) {
    if !crate::ai::autocomplete_enabled() || editor.mode != Mode::Insert {
        return;
    }
    let (view, doc) = current_ref!(editor);
    if doc.id() != trigger.doc || view.id != trigger.view {
        return;
    }
    let text = doc.text();
    let pos = doc.selection(view.id).primary().cursor(text.slice(..));
    if pos != trigger.pos {
        return; // the cursor moved while we were debouncing
    }
    let start = pos.saturating_sub(4000);
    let end = (pos + 1500).min(text.len_chars());
    let before = text.slice(start..pos).to_string();
    let after = text.slice(pos..end).to_string();
    let lang = doc.language_name().unwrap_or("").to_string();
    if before.trim().is_empty() {
        return;
    }
    let doc_id = trigger.doc;
    let view_id = trigger.view;

    tokio::spawn(cancelable_future(
        async move {
            let raw = tokio::task::spawn_blocking(move || {
                let provider = crate::ai::provider().ok()?;
                let sys = crate::ai::system_with_rules(GHOST_SYSTEM);
                let user = format!("Language: {lang}\n\n{before}<CURSOR>{after}");
                provider
                    .chat(Some(&sys), &[crate::ai::Message::user(user)])
                    .ok()
            })
            .await
            .ok()
            .flatten();
            let Some(raw) = raw else {
                return;
            };
            let suggestion = clean_suggestion(&raw);
            if suggestion.is_empty() {
                return;
            }
            dispatch(move |editor, _compositor| {
                if editor.mode != Mode::Insert {
                    return;
                }
                let (view, doc) = current!(editor);
                if doc.id() != doc_id || view.id != view_id {
                    return;
                }
                let cur = doc
                    .selection(view.id)
                    .primary()
                    .cursor(doc.text().slice(..));
                if cur != pos {
                    return; // user typed/moved since the request went out
                }
                doc.set_ghost_text(view.id, pos, suggestion);
            })
            .await;
        },
        handle,
    ));
}

/// Trim fences and cap the suggestion to a few short lines so the ghost stays unobtrusive.
fn clean_suggestion(raw: &str) -> String {
    let s = crate::commands::strip_code_fences(raw);
    let mut lines: Vec<&str> = s.lines().take(6).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n").chars().take(400).collect()
}

/// Post a trigger for the current cursor (called after a char is typed).
fn trigger_ghost(editor: &Editor) {
    let (view, doc) = current_ref!(editor);
    let cursor = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    send_blocking(
        &editor.handlers.ai_ghost,
        GhostEvent::Trigger {
            cursor,
            doc: doc.id(),
            view: view.id,
        },
    );
}

/// Drop any visible suggestion and cancel pending work.
fn cancel_ghost(editor: &mut Editor) {
    let view_id = view!(editor).id;
    doc_mut!(editor).clear_ghost_text(view_id);
    send_blocking(&editor.handlers.ai_ghost, GhostEvent::Cancel);
}

pub(super) fn register_hooks(_handlers: &Handlers) {
    register_hook!(move |event: &mut PostInsertChar<'_, '_>| {
        if crate::ai::autocomplete_enabled() {
            // The just-typed char invalidates the old ghost; clear it and re-arm.
            let view_id = view!(event.cx.editor).id;
            doc_mut!(event.cx.editor).clear_ghost_text(view_id);
            trigger_ghost(event.cx.editor);
        }
        Ok(())
    });

    register_hook!(move |event: &mut OnModeSwitch<'_, '_>| {
        if event.old_mode == Mode::Insert {
            cancel_ghost(event.cx.editor);
        }
        Ok(())
    });

    register_hook!(move |event: &mut PostCommand<'_, '_>| {
        if event.cx.editor.mode == Mode::Insert {
            // Keep the suggestion only while it is being accepted; anything else clears it.
            let keep = matches!(
                event.command,
                MappableCommand::Static {
                    name: "ghost_text_accept" | "ghost_text_accept_word",
                    ..
                }
            );
            if !keep {
                cancel_ghost(event.cx.editor);
            }
        }
        Ok(())
    });
}
