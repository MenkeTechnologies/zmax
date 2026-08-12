//! The parts of emacs's mouse surface that are not "click in the text": the mode
//! line and the split dividers, the menus the Control clicks pop up, and the
//! `ffap` bindings (mouse and keyboard).
//!
//! Every emacs mouse command reads its click out of the event it was invoked with
//! (`(interactive "e")`). zmax records that event on the editor — the buffer
//! position in [`Editor::last_mouse_pos`], the window a mode-line/divider click
//! named in [`Editor::last_mouse_view`], and the screen cell a menu should open at
//! in [`Editor::last_mouse_screen`] — and the mouse handler in `ui::editor`
//! *dispatches through the commands below*, so these are the real handlers for
//! those clicks and not a second implementation beside them. Each also works from
//! a key or `M-x`, acting on the last click (or on point, for the menus).

use zmax_core::Selection;
use zmax_view::{editor::Action, Editor, ViewId};

use crate::commands::{click_pos, ffap_open, ffap_resolve, Context, MappableCommand};
use crate::compositor::Compositor;
use crate::ui::{
    context_menu::{ContextMenu, Entry},
    PromptEvent,
};

/// The window the last click named — the one whose mode line or divider was hit —
/// falling back to the selected window when the mouse has not been used (or when
/// that window has since been closed).
fn clicked_view(editor: &Editor) -> ViewId {
    editor
        .last_mouse_view
        .filter(|id| editor.tree.try_get(*id).is_some())
        .unwrap_or(editor.tree.focus)
}

/// emacs `mouse-select-window` (`mouse-1` on a mode line, a header line or a
/// window divider): the window the click belongs to becomes the selected one.
pub fn mouse_select_window(cx: &mut Context) {
    let view = clicked_view(cx.editor);
    cx.editor.focus(view);
}

/// emacs `mouse-split-window-horizontally` (`C-mouse-2` on a mode line): the
/// clicked window becomes two side-by-side windows, with the boundary running
/// through the click.
pub fn mouse_split_window_horizontally(cx: &mut Context) {
    split_at_click(cx);
}

/// Split the clicked window side by side and move the new boundary onto the
/// click. Emacs splits at the click position rather than in half, so the drag
/// that usually follows starts from where the pointer already is.
/// (`mouse-split-window-vertically`, the scroll bar's stacked split, lives with
/// the scroll-bar handler in `commands`.)
fn split_at_click(cx: &mut Context) {
    let view = clicked_view(cx.editor);
    cx.editor.focus(view);
    let click = cx.editor.last_mouse_screen.take();
    MappableCommand::vsplit.execute(cx);
    // The split halves the window; this moves the boundary onto the click. The
    // resize clamps at the minimum pane size, as emacs's does.
    let (Some((_, column)), Some(area)) = (click, cx.editor.tree.try_get(view).map(|v| v.area))
    else {
        return;
    };
    let delta = column as i16 - area.right() as i16;
    if delta != 0 {
        cx.editor.tree.resize_horizontal(view, delta);
    }
}

/// emacs `context-menu-open` (`S-<f10>`, and the `down-mouse-3` that
/// `context-menu-mode` binds): pop up the buffer's context menu — at the click
/// when the mouse opened it, at point when a key did.
pub fn context_menu_open(cx: &mut Context) {
    let anchor = cx
        .editor
        .last_mouse_screen
        .take()
        .unwrap_or_else(|| point_screen_pos(cx.editor));
    let path = doc!(cx.editor).path().map(|p| p.to_path_buf());
    cx.callback.push(Box::new(
        move |compositor: &mut Compositor, _cx: &mut crate::compositor::Context| {
            let mut entries = crate::ui::editor::editor_menu_entries(path.clone());
            // Reveal in Tree at the end when the buffer has a path.
            if let Some(path) = path {
                entries.push(Entry::sep());
                entries.push(Entry::item("Reveal in Tree", move |compositor, _cx| {
                    if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                        view.reveal_in_tree(&path);
                    }
                }));
            }
            compositor.push(Box::new(ContextMenu::new(anchor.0, anchor.1, entries)));
        },
    ));
}

/// The screen cell point sits on, for a menu opened from the keyboard. Falls back
/// to the window's top-left corner when point is scrolled out of view.
fn point_screen_pos(editor: &Editor) -> (u16, u16) {
    let (view, doc) = current_ref!(editor);
    let text = doc.text().slice(..);
    let pos = doc.selection(view.id).primary().cursor(text);
    match view.screen_coords_at_pos(doc, text, pos) {
        Some(coords) => {
            let inner = view.inner_area(doc);
            (inner.y + coords.row as u16, inner.x + coords.col as u16)
        }
        None => (view.area.y, view.area.x),
    }
}

/// emacs `ffap-at-mouse` (`S-mouse-3` once `ffap-bindings` is installed): point
/// moves to the click and the file or URL guessed from the text there is visited.
/// With nothing to guess ffap says so and visits nothing.
pub fn ffap_at_mouse(cx: &mut Context) {
    let pos = click_pos(cx.editor);
    {
        let (view, doc) = current!(cx.editor);
        doc.set_selection(view.id, Selection::point(pos));
    }
    if crate::ui::editor::ffap_guess_at(doc!(cx.editor), pos).is_none() {
        cx.editor
            .set_error("ffap-at-mouse: no file or URL at the mouse click");
        return;
    }
    MappableCommand::goto_file.execute(cx);
}

/// emacs `hs-toggle-hiding` (`S-mouse-2` under hs-minor-mode): hide the block the
/// click landed in, or show it again when it is already hidden. From a key it
/// acts at point.
pub fn hs_toggle_hiding(cx: &mut Context) {
    let pos = click_pos(cx.editor);
    {
        let (view, doc) = current!(cx.editor);
        doc.set_selection(view.id, Selection::point(pos));
    }
    MappableCommand::fold_toggle.execute(cx);
}

// ── ffap-bindings (the `[remap …]` half of `M-x ffap-bindings`) ──────────────
//
// `ffap-bindings` does not bind keys of its own for the file-finding commands: it
// *remaps the commands*, so every key bound to `find-file`, `dired`,
// `find-file-other-window`, … starts reading its default from the text at point.
// The same shape is used here — the ordinary command checks the flag and hands
// over — so one toggle changes `C-x C-f`, `C-x d`, `SPC f f` and the rest at once,
// exactly as the remap does in emacs.

/// emacs `ffap-bindings`: install the remaps (or take them back).
pub fn ffap_bindings(cx: &mut Context) {
    let on = !cx.editor.ffap_bindings;
    cx.editor.ffap_bindings = on;
    cx.editor.set_status(if on {
        "ffap bindings installed: the file commands read the name at point"
    } else {
        "ffap bindings removed"
    });
}

/// Run `to` instead of the caller when the ffap bindings are installed and the
/// text at point really names something to visit. Returns true when it did, so
/// the caller returns without doing its own work.
///
/// With nothing at point the caller runs unchanged: emacs's ffap commands prompt
/// for the file when the guess comes up empty, and prompting is what the ordinary
/// command already does.
pub fn remapped(cx: &mut Context, to: MappableCommand) -> bool {
    if !cx.editor.ffap_bindings || !guess_at_point(cx) {
        return false;
    }
    to.execute(cx);
    true
}

/// Whether ffap has anything to work with at point (or at the last click).
fn guess_at_point(cx: &Context) -> bool {
    let pos = click_pos(cx.editor);
    crate::ui::editor::ffap_guess_at(doc!(cx.editor), pos).is_some()
}

/// emacs `find-file-read-only` (`C-x C-r`): visit a file you name, in a buffer
/// that arrives read-only. `ffap-bindings` remaps it to `ffap-read-only`, which
/// takes the name from the text at point instead of asking.
pub fn find_file_read_only(cx: &mut Context) {
    if remapped(cx, MappableCommand::goto_file_readonly) {
        return;
    }
    let prompt = crate::ui::prompt::Prompt::new(
        "Find file read-only: ".into(),
        None,
        crate::ui::completers::filename,
        move |cx: &mut crate::compositor::Context, input: &str, ev: PromptEvent| {
            if ev != PromptEvent::Validate {
                return;
            }
            let path = zmax_stdx::path::expand_tilde(std::path::Path::new(input.trim()));
            match cx.editor.open(&path, Action::Replace) {
                Ok(_) => {
                    doc_mut!(cx.editor).readonly = true;
                    cx.editor
                        .set_status(format!("{} (read-only)", path.display()));
                }
                Err(e) => cx.editor.set_error(format!("{}: {e}", path.display())),
            }
        },
    );
    cx.push_layer(Box::new(prompt));
}

/// emacs `ffap-alternate-file` (`C-x C-v` with the ffap bindings): visit the file
/// at point *in place of* this buffer — the buffer it replaces is killed, which is
/// what makes `find-alternate-file` different from `find-file`.
pub fn ffap_alternate_file(cx: &mut Context) {
    let old = doc!(cx.editor).id();
    MappableCommand::goto_file.execute(cx);
    if doc!(cx.editor).id() == old {
        return;
    }
    if cx.editor.close_document(old, false).is_err() {
        cx.editor
            .set_status("the buffer it replaced has unsaved changes and was kept");
    }
}

/// emacs `ffap-other-window` (`C-x 4 f` with the ffap bindings): the file at point
/// is visited in another window. An existing other window is reused; this being
/// the only window is the one case that splits, as `find-file-other-window` does.
pub fn ffap_other_window(cx: &mut Context) {
    let other = cx
        .editor
        .tree
        .views()
        .map(|(view, _)| view.id)
        .find(|id| *id != cx.editor.tree.focus);
    let Some(other) = other else {
        MappableCommand::goto_file_vsplit.execute(cx);
        return;
    };
    // The guess is read here, in the window point is in — the other window holds
    // another buffer, and ffap must not scan that one.
    let Some((path, line)) = ffap_target(cx) else {
        cx.editor
            .set_error("ffap-other-window: no file at point names a file that exists");
        return;
    };
    cx.editor.focus(other);
    ffap_open(cx, &path, line);
}

/// The file the ffap guess at point (or at the last click) names, resolved
/// against the file system, with the `:LINE` suffix it carried.
fn ffap_target(cx: &Context) -> Option<(std::path::PathBuf, Option<usize>)> {
    let pos = click_pos(cx.editor);
    let text = doc!(cx.editor).text().to_string();
    let found = zmax_core::ffap::file_refs(&text)
        .into_iter()
        .find(|r| r.start <= pos && pos <= r.end)?;
    let path = ffap_resolve(cx.editor, &found.path)?;
    Some((path, found.line))
}
