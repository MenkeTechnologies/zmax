use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use tokio::task::JoinSet;
use zmax_core::chars::char_is_word;
use zmax_core::completion::CompletionProvider;
use zmax_core::syntax::config::LanguageServerFeature;
use zmax_event::{register_hook, TaskHandle};
use zmax_lsp::lsp;
use zmax_stdx::rope::RopeSliceExt;
use zmax_view::document::Mode;
use zmax_view::handlers::completion::{CompletionEvent, ResponseContext};
use zmax_view::{DocumentId, Editor};

use crate::commands;
use crate::compositor::Compositor;
use crate::events::{OnModeSwitch, PostCommand, PostInsertChar};
use crate::handlers::completion::request::{request_incomplete_completion_list, Trigger};
use crate::job::dispatch;
use crate::keymap::MappableCommand;
use crate::ui::lsp::signature_help::SignatureHelp;
use crate::ui::{self, Popup};

use super::Handlers;

pub use item::{CompletionItem, CompletionItems, CompletionResponse, LspCompletionItem};
pub use request::CompletionHandler;
pub use resolve::ResolveHandler;

mod item;
mod path;
mod request;
mod resolve;
mod sql;
mod word;

async fn handle_response(
    requests: &mut JoinSet<CompletionResponse>,
    is_incomplete: bool,
) -> Option<CompletionResponse> {
    loop {
        let response = requests.join_next().await?.unwrap();
        if !is_incomplete && !response.context.is_incomplete && response.items.is_empty() {
            continue;
        }
        return Some(response);
    }
}

async fn replace_completions(
    handle: TaskHandle,
    mut requests: JoinSet<CompletionResponse>,
    is_incomplete: bool,
) {
    while let Some(mut response) = handle_response(&mut requests, is_incomplete).await {
        let handle = handle.clone();
        dispatch(move |editor, compositor| {
            let editor_view = compositor.find::<ui::EditorView>().unwrap();
            let Some(completion) = &mut editor_view.completion else {
                return;
            };
            if handle.is_canceled() {
                log::info!("dropping outdated completion response");
                return;
            }

            completion.replace_provider_completions(&mut response, is_incomplete);
            if completion.is_empty() {
                editor_view.clear_completion(editor);
                // clearing completions might mean we want to immediately re-request them (usually
                // this occurs if typing a trigger char)
                trigger_auto_completion(editor, false);
            } else {
                editor
                    .handlers
                    .completions
                    .active_completions
                    .insert(response.provider, response.context);
            }
        })
        .await;
    }
}

fn show_completion(
    editor: &mut Editor,
    compositor: &mut Compositor,
    mut items: Vec<CompletionItem>,
    context: HashMap<CompletionProvider, ResponseContext>,
    trigger: Trigger,
) {
    let (view, doc) = current_ref!(editor);
    // check if the completion request is stale.
    //
    // Completions are completed asynchronously and therefore the user could
    //switch document/view or leave insert mode. In all of thoise cases the
    // completion should be discarded
    if editor.mode != Mode::Insert || view.id != trigger.view || doc.id() != trigger.doc {
        return;
    }

    // With completion-preview-mode on, emacs shows the top candidate in-line after point instead of
    // opening a completion menu.
    if completion_preview_mode_enabled(doc.id()) {
        show_completion_preview(editor, &items);
        return;
    }

    let size = compositor.size();
    let ui = compositor.find::<ui::EditorView>().unwrap();
    if ui.completion.is_some() {
        return;
    }
    word::retain_valid_completions(trigger, doc, view.id, &mut items);
    editor.handlers.completions.active_completions = context;

    let completion_area = ui.set_completion(editor, items, trigger.pos, size);
    let signature_help_area = compositor
        .find_id::<Popup<SignatureHelp>>(SignatureHelp::ID)
        .map(|signature_help| signature_help.area(size, editor));
    // Delete the signature help popup if they intersect.
    if matches!((completion_area, signature_help_area),(Some(a), Some(b)) if a.intersects(b)) {
        compositor.remove(SignatureHelp::ID);
    }
}

pub fn trigger_auto_completion(editor: &Editor, trigger_char_only: bool) {
    let config = editor.config.load();
    if !config.auto_completion {
        return;
    }
    let (view, doc): (&zmax_view::View, &zmax_view::Document) = current_ref!(editor);
    let mut text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(text);
    text = doc.text().slice(..cursor);

    let is_trigger_char = doc
        .language_servers_with_feature(LanguageServerFeature::Completion)
        .any(|ls| {
            matches!(&ls.capabilities().completion_provider, Some(lsp::CompletionOptions {
                        trigger_characters: Some(triggers),
                        ..
                    }) if triggers.iter().any(|trigger| text.ends_with(trigger)))
        });

    let cursor_char = text
        .get_bytes_at(text.len_bytes())
        .and_then(|t| t.reversed().next());

    #[cfg(windows)]
    let is_path_completion_trigger = matches!(cursor_char, Some(b'/' | b'\\'));
    #[cfg(not(windows))]
    let is_path_completion_trigger = matches!(cursor_char, Some(b'/'));

    let handler = &editor.handlers.completions;
    if is_trigger_char || (is_path_completion_trigger && doc.path_completion_enabled()) {
        handler.event(CompletionEvent::TriggerChar {
            cursor,
            doc: doc.id(),
            view: view.id,
        });
        return;
    }

    let is_auto_trigger = !trigger_char_only
        && doc
            .text()
            .chars_at(cursor)
            .reversed()
            .take(config.completion_trigger_len as usize)
            .all(char_is_word);

    if is_auto_trigger {
        handler.event(CompletionEvent::AutoTrigger {
            cursor,
            doc: doc.id(),
            view: view.id,
        });
    }
}

// ---------------------------------------------------------------------------
// completion-preview-mode / global-completion-preview-mode (emacs 30)
// ---------------------------------------------------------------------------
//
// Emacs shows the first completion candidate for the symbol at point as an in-line preview right
// after point instead of popping up a menu; `TAB` inserts it. Here the same completion responses
// that would open the popup are rendered through the existing ghost-text substrate
// (`Document::set_ghost_text`) when the mode is on for the current buffer, and the popup is
// suppressed — `ghost_text_accept` (insert-mode `tab` in the vim preset) inserts the preview.

/// `completion-preview-minimum-symbol-length`: how many symbol characters must precede point
/// before a preview is shown (emacs default 3).
const PREVIEW_MINIMUM_SYMBOL_LENGTH: usize = 3;

/// `completion-preview-exact-match-only`: when true, only preview if a single candidate matches
/// (emacs default nil).
const PREVIEW_EXACT_MATCH_ONLY: bool = false;

/// `global-completion-preview-mode`, tri-state so the `ZMAX_COMPLETION_PREVIEW` env default can be
/// overridden at runtime: 0 = unset, 1 = on, 2 = off.
static PREVIEW_GLOBAL: AtomicU8 = AtomicU8::new(0);

/// Buffers whose local `completion-preview-mode` was toggled away from the global setting.
static PREVIEW_BUFFERS: OnceLock<Mutex<HashMap<DocumentId, bool>>> = OnceLock::new();

fn preview_buffers() -> &'static Mutex<HashMap<DocumentId, bool>> {
    PREVIEW_BUFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Whether `global-completion-preview-mode` is on.
pub fn global_completion_preview_mode_enabled() -> bool {
    match PREVIEW_GLOBAL.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => std::env::var("ZMAX_COMPLETION_PREVIEW")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false),
    }
}

/// `M-x global-completion-preview-mode`: turn the preview on or off in every buffer. Buffer-local
/// toggles made afterwards still win for their own buffer, as in emacs.
#[allow(dead_code)] // entry point for the mode command; until then the env var is the switch
pub fn toggle_global_completion_preview_mode() -> bool {
    let new = !global_completion_preview_mode_enabled();
    PREVIEW_GLOBAL.store(if new { 1 } else { 2 }, Ordering::Relaxed);
    new
}

/// Whether `completion-preview-mode` is on for `doc` — its buffer-local setting if it has one,
/// otherwise the global mode.
pub fn completion_preview_mode_enabled(doc: DocumentId) -> bool {
    preview_buffers()
        .lock()
        .unwrap()
        .get(&doc)
        .copied()
        .unwrap_or_else(global_completion_preview_mode_enabled)
}

/// `M-x completion-preview-mode`: toggle the preview in a single buffer.
#[allow(dead_code)] // entry point for the mode command; until then the env var is the switch
pub fn toggle_completion_preview_mode(doc: DocumentId) -> bool {
    let new = !completion_preview_mode_enabled(doc);
    preview_buffers().lock().unwrap().insert(doc, new);
    new
}

/// The text a candidate would insert, or `None` for candidates that cannot be shown as a plain
/// preview (snippets expand placeholders, so emacs's capf previews them as literal text at most).
fn preview_text(item: &CompletionItem) -> Option<&str> {
    if item.is_snippet() {
        return None;
    }
    match item {
        CompletionItem::Lsp(item) => Some(
            item.item
                .insert_text
                .as_deref()
                .unwrap_or(item.item.label.as_str()),
        ),
        CompletionItem::Other(item) => Some(&item.label),
    }
}

/// The part of the first candidate that extends `symbol`, i.e. what emacs renders after point.
///
/// Candidates are ordered by `completion-preview-sort-function`, whose default
/// (`minibuffer--sort-by-length-alpha`) is shortest first with alphabetical ties.
fn preview_suffix(symbol: &str, items: &[CompletionItem]) -> Option<String> {
    let mut candidates: Vec<&str> = items
        .iter()
        .filter_map(preview_text)
        .filter(|candidate| candidate.len() > symbol.len() && candidate.starts_with(symbol))
        .collect();
    candidates.sort_unstable_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    candidates.dedup();
    if PREVIEW_EXACT_MATCH_ONLY && candidates.len() > 1 {
        return None;
    }
    candidates
        .first()
        .map(|candidate| candidate[symbol.len()..].to_string())
}

/// Render (or drop) the in-line preview for a fresh set of candidates.
fn show_completion_preview(editor: &mut Editor, items: &[CompletionItem]) {
    let (view, doc) = current_ref!(editor);
    let view_id = view.id;
    let text = doc.text();
    let cursor = doc.selection(view_id).primary().cursor(text.slice(..));
    let symbol: String = {
        let mut chars: Vec<char> = text
            .chars_at(cursor)
            .reversed()
            .take_while(|&c| char_is_word(c))
            .collect();
        chars.reverse();
        chars.into_iter().collect()
    };
    let suffix = if symbol.chars().count() >= PREVIEW_MINIMUM_SYMBOL_LENGTH {
        preview_suffix(&symbol, items)
    } else {
        None
    };

    let doc = doc_mut!(editor);
    match suffix {
        Some(suffix) => doc.set_ghost_text(view_id, cursor, suffix),
        None => {
            doc.clear_ghost_text(view_id);
        }
    }
}

/// Drop a visible preview (on a new keystroke or when leaving insert mode). Only touches the ghost
/// text when the mode is on, so it never steals the AI autocomplete's suggestion.
fn clear_completion_preview(editor: &mut Editor) {
    let (view, doc) = current_ref!(editor);
    if !completion_preview_mode_enabled(doc.id()) {
        return;
    }
    let view_id = view.id;
    doc_mut!(editor).clear_ghost_text(view_id);
}

fn update_completion_filter(cx: &mut commands::Context, c: Option<char>) {
    cx.callback.push(Box::new(move |compositor, cx| {
        let editor_view = compositor.find::<ui::EditorView>().unwrap();
        if let Some(completion) = &mut editor_view.completion {
            completion.update_filter(c);
            if completion.is_empty() || c.is_some_and(|c| !char_is_word(c)) {
                editor_view.clear_completion(cx.editor);
                // clearing completions might mean we want to immediately rerequest them (usually
                // this occurs if typing a trigger char)
                if c.is_some() {
                    trigger_auto_completion(cx.editor, false);
                }
            } else {
                let handle = cx.editor.handlers.completions.request_controller.restart();
                request_incomplete_completion_list(cx.editor, handle)
            }
        }
    }))
}

fn clear_completions(cx: &mut commands::Context) {
    cx.callback.push(Box::new(|compositor, cx| {
        let editor_view = compositor.find::<ui::EditorView>().unwrap();
        editor_view.clear_completion(cx.editor);
    }))
}

fn completion_post_command_hook(
    PostCommand { command, cx }: &mut PostCommand<'_, '_>,
) -> anyhow::Result<()> {
    if cx.editor.mode == Mode::Insert {
        if cx.editor.last_completion.is_some() {
            match command {
                MappableCommand::Static {
                    name: "delete_word_forward" | "delete_char_forward" | "completion",
                    ..
                } => (),
                MappableCommand::Static {
                    name: "delete_char_backward",
                    ..
                } => update_completion_filter(cx, None),
                _ => clear_completions(cx),
            }
        } else {
            let event = match command {
                MappableCommand::Static {
                    name: "delete_char_backward" | "delete_word_forward" | "delete_char_forward",
                    ..
                } => {
                    let (view, doc) = current!(cx.editor);
                    let primary_cursor = doc
                        .selection(view.id)
                        .primary()
                        .cursor(doc.text().slice(..));
                    CompletionEvent::DeleteText {
                        cursor: primary_cursor,
                    }
                }
                // hacks: some commands are handeled elsewhere and we don't want to
                // cancel in that case
                MappableCommand::Static {
                    name: "completion" | "insert_mode" | "append_mode",
                    ..
                } => return Ok(()),
                _ => CompletionEvent::Cancel,
            };
            cx.editor.handlers.completions.event(event);
        }
    }
    Ok(())
}

pub(super) fn register_hooks(_handlers: &Handlers) {
    register_hook!(move |event: &mut PostCommand<'_, '_>| completion_post_command_hook(event));

    register_hook!(move |event: &mut OnModeSwitch<'_, '_>| {
        if event.old_mode == Mode::Insert {
            clear_completion_preview(event.cx.editor);
            event
                .cx
                .editor
                .handlers
                .completions
                .event(CompletionEvent::Cancel);
            clear_completions(event.cx);
        } else if event.new_mode == Mode::Insert {
            trigger_auto_completion(event.cx.editor, false)
        }
        Ok(())
    });

    register_hook!(move |event: &mut PostInsertChar<'_, '_>| {
        // The typed char invalidates any visible preview; the next response repaints it.
        clear_completion_preview(event.cx.editor);
        if event.cx.editor.last_completion.is_some() {
            update_completion_filter(event.cx, Some(event.c))
        } else {
            trigger_auto_completion(event.cx.editor, false);
        }
        Ok(())
    });
}
