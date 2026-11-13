//! A Markdown buffer beside a live rendered preview of it — JetBrains
//! Markdown "Editor and Preview" (`TextEditorWithPreview.Layout.EditorAndPreview`),
//! laid out side by side or one above the other.
//!
//! The preview is a scratch buffer that refuses edits and carries a
//! [`PreviewOf`] link to its source. After each command and each typed
//! character, a preview whose source changed is rendered again, and with
//! "Sync Scroll" on, scrolled so the source's top line is the preview's.

use std::sync::atomic::{AtomicBool, Ordering};

use zmax_core::Transaction;
use zmax_view::document::PreviewOf;
use zmax_view::editor::Action;
use zmax_view::view::ViewPosition;
use zmax_view::{DocumentId, Editor, ViewId};

/// JetBrains "Sync Scroll" (`AutoScrollAction`), on as the IDE ships it.
static SYNC_SCROLL: AtomicBool = AtomicBool::new(true);

/// Flip Sync Scroll and bring the previews in line; returns the new state.
pub fn toggle_sync_scroll(editor: &mut Editor) -> bool {
    let on = !SYNC_SCROLL.fetch_xor(true, Ordering::Relaxed);
    sync(editor);
    on
}

/// The views showing `doc`, the focused one first.
fn views_of(editor: &Editor, doc: DocumentId) -> Vec<ViewId> {
    let focus = editor.tree.focus;
    let mut views: Vec<ViewId> = editor
        .tree
        .views()
        .filter(|(view, _)| view.doc == doc)
        .map(|(view, _)| view.id)
        .collect();
    views.sort_by_key(|id| *id != focus);
    views
}

/// Show a preview of the focused Markdown buffer in a `split` of its view
/// (`Action::VerticalSplit` beside it, `Action::HorizontalSplit` below it),
/// keeping the source focused, or in the view itself (`Action::Replace`,
/// JetBrains "Preview Only"). A preview it already has moves to the new
/// layout rather than being opened twice.
pub fn open(editor: &mut Editor, split: Action) -> Result<(), &'static str> {
    let source_view = editor.tree.focus;
    let source = editor.tree.get(source_view).doc;
    let doc = editor.document(source).ok_or("no buffer")?;
    if doc.preview_of.is_some() {
        return Err("this is a preview; focus its Markdown buffer");
    }
    if doc.language_name() != Some("markdown") {
        return Err("not a Markdown buffer");
    }
    let existing = editor
        .documents()
        .find(|d| d.preview_of.as_ref().is_some_and(|p| p.source == source))
        .map(|d| d.id());
    let preview = match existing {
        Some(preview) => {
            for view in views_of(editor, preview) {
                editor.close(view);
            }
            editor.focus(source_view);
            editor.switch(preview, split);
            preview
        }
        None => {
            let preview = editor.new_file(split);
            let doc = editor.document_mut(preview).expect("just opened");
            doc.modifiable = false;
            doc.preview_of = Some(PreviewOf {
                source,
                version: i32::MIN,
                line_starts: Vec::new(),
                top: None,
            });
            preview
        }
    };
    let view = editor.tree.focus;
    if let Some(doc) = editor.document_mut(preview) {
        doc.ensure_view_init(view);
        if let Some(link) = doc.preview_of.as_mut() {
            link.top = None;
        }
    }
    editor.focus(source_view);
    sync(editor);
    Ok(())
}

/// JetBrains "Editor Only" (`TextEditorWithPreview.Layout.EditorOnly`) from a
/// preview: its view shows the Markdown buffer again.
pub fn show_source(editor: &mut Editor) -> Result<(), &'static str> {
    let view = editor.tree.focus;
    let doc = editor.tree.get(view).doc;
    let source = editor
        .document(doc)
        .and_then(|d| d.preview_of.as_ref())
        .map(|link| link.source)
        .ok_or("not a Markdown preview")?;
    editor.switch(source, Action::Replace);
    Ok(())
}

/// Render every preview whose source changed since, and with Sync Scroll on,
/// scroll each shown preview to its source's top line.
pub fn sync(editor: &mut Editor) {
    let previews: Vec<(DocumentId, PreviewOf)> = editor
        .documents()
        .filter_map(|d| d.preview_of.clone().map(|link| (d.id(), link)))
        .collect();
    for (preview, mut link) in previews {
        // A command scrolls its view once it has run; read the source's top
        // line where the screen will show it.
        let focus = editor.tree.focus;
        if editor
            .tree
            .try_get(focus)
            .is_some_and(|v| v.doc == link.source)
        {
            editor.ensure_cursor_in_view(focus);
        }
        let Some(source) = editor.document(link.source) else {
            // The source closed: what is left is a plain read-only scratch.
            if let Some(doc) = editor.document_mut(preview) {
                doc.preview_of = None;
            }
            continue;
        };
        let version = source.version();
        let (rendered, starts) = (version != link.version)
            .then(|| crate::commands::render_markdown_mapped(&source.text().to_string()))
            .unzip();
        let top_line = SYNC_SCROLL
            .load(Ordering::Relaxed)
            .then(|| views_of(editor, link.source).first().copied())
            .flatten()
            .map(|view| {
                let text = source.text();
                text.char_to_line(source.view_offset(view).anchor.min(text.len_chars()))
            });
        let preview_views = views_of(editor, preview);
        let Some(doc) = editor.document_mut(preview) else {
            continue;
        };
        if let (Some(rendered), Some(starts)) = (rendered, starts) {
            if let Some(&view) = preview_views.first() {
                let edit = Transaction::change(
                    doc.text(),
                    [(0, doc.text().len_chars(), Some(rendered.into()))].into_iter(),
                );
                doc.modifiable = true;
                doc.apply(&edit, view);
                doc.modifiable = false;
                link.version = version;
                link.line_starts = starts;
                link.top = None;
            }
        }
        if let Some(line) = top_line {
            let last = doc.text().len_lines().saturating_sub(1);
            let target = link
                .line_starts
                .get(line)
                .copied()
                .unwrap_or(last)
                .min(last);
            if link.top != Some(target) {
                let anchor = doc.text().line_to_char(target);
                for &view in &preview_views {
                    doc.set_view_offset(
                        view,
                        ViewPosition {
                            anchor,
                            horizontal_offset: 0,
                            vertical_offset: 0,
                        },
                    );
                }
                link.top = Some(target);
            }
        }
        doc.preview_of = Some(link);
    }
}

pub fn register_hooks() {
    use crate::events::{PostCommand, PostInsertChar};
    use zmax_event::register_hook;
    register_hook!(move |event: &mut PostCommand<'_, '_>| {
        sync(event.cx.editor);
        Ok(())
    });
    register_hook!(move |event: &mut PostInsertChar<'_, '_>| {
        sync(event.cx.editor);
        Ok(())
    });
}
