//! Xwidget-WebKit — the zmax port of GNU Emacs's `xwidget-webkit-*` browser.
//!
//! **Partial by construction, and this is the honest accounting.** Emacs embeds a
//! live WebKit widget in a buffer: real layout, JavaScript, forms. A terminal has
//! no surface to embed a browser engine into, so zmax renders the page with the
//! text renderer it already has for `eww` ([`crate::eww::html_to_text`]) and puts
//! the result in a buffer. Everything that follows from *having a rendered page
//! in a buffer* is ported for real — session history, back/forward, reload,
//! browse-history, edit vs. one-key modes, incremental search over the page.
//! Everything that needs a live DOM — JavaScript, form submission, link
//! activation, zoom — is not there, and the commands that drive it are not
//! claimed.
//!
//! The split follows `ui/image.rs` and `ui/docview.rs`: the page is a normal
//! buffer, and this [`Component`] is the *keymap* over it, so a key and the
//! typable of the same name run one code path. Keys follow
//! `xwidget-webkit-mode-map`:
//!
//!   g — `xwidget-webkit-browse-url`   r — `xwidget-webkit-reload`
//!   b — `xwidget-webkit-back`         f — `xwidget-webkit-forward`
//!   w — `xwidget-webkit-current-url`  H — `xwidget-webkit-browse-history`
//!   e — `xwidget-webkit-edit-mode`    C-s / C-r — `xwidget-webkit-isearch-mode`
//!   q / Esc — leave the viewer
//!
//! Scrolling keys are deliberately *not* bound: the page is a real buffer, so
//! unhandled keys fall through to the editor's own motions, which is strictly
//! more than `xwidget-webkit-scroll-*` offers.
//!
//! `xwidget-webkit-edit-mode` is the one command whose Emacs meaning ("send
//! self-inserting characters to the widget instead of running the mode's one-key
//! commands") survives the transposition: while it is on, this overlay stops
//! consuming letters and they reach the buffer underneath. What they cannot do is
//! land in a web form, because there is no live form.

use std::sync::Mutex;

use once_cell::sync::Lazy;
use tui::buffer::Buffer as Surface;
use zmax_core::command_line::Args;
use zmax_core::Selection;
use zmax_view::editor::Editor;
use zmax_view::graphics::Rect;
use zmax_view::input::KeyEvent;
use zmax_view::keyboard::{KeyCode, KeyModifiers};

use crate::ui::PromptEvent;
use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The compositor id the viewer overlay is pushed under.
const ID: &str = "xwidget-webkit";

// ---------------------------------------------------------------------------
// Session history (Emacs's WebKit back-forward list)
// ---------------------------------------------------------------------------

/// The session's page loads, oldest first, and the index of the one on screen.
static HISTORY: Lazy<Mutex<(Vec<String>, usize)>> = Lazy::new(|| Mutex::new((Vec::new(), 0)));

/// Record `url` as the current page. A load made while sitting back in the
/// history truncates the forward entries, exactly as a browser does.
pub fn history_push(url: &str) {
    let mut h = HISTORY.lock().unwrap();
    let (list, idx) = &mut *h;
    if list.get(*idx).is_some_and(|cur| cur == url) {
        return;
    }
    if !list.is_empty() {
        list.truncate(*idx + 1);
    }
    list.push(url.to_string());
    *idx = list.len() - 1;
}

/// The URL on screen, if any page has been loaded this session.
pub fn history_current() -> Option<String> {
    let h = HISTORY.lock().unwrap();
    h.0.get(h.1).cloned()
}

/// Step `delta` entries through the history and return the URL to load, or
/// `None` at either end (Emacs's back/forward are no-ops there).
pub fn history_step(delta: isize) -> Option<String> {
    let mut h = HISTORY.lock().unwrap();
    let (list, idx) = &mut *h;
    let target = *idx as isize + delta;
    if list.is_empty() || target < 0 || target as usize >= list.len() {
        return None;
    }
    *idx = target as usize;
    list.get(*idx).cloned()
}

/// The history as `*Xwidget WebKit History*` lists it: oldest first, the page on
/// screen marked.
pub fn history_listing() -> String {
    let h = HISTORY.lock().unwrap();
    let (list, idx) = &*h;
    if list.is_empty() {
        return "Xwidget WebKit History\n\n(no pages loaded this session)\n".to_string();
    }
    let mut out = String::from("Xwidget WebKit History\n\n");
    for (i, url) in list.iter().enumerate() {
        let marker = if i == *idx { '*' } else { ' ' };
        out.push_str(&format!("{marker} {:>3}  {url}\n", i + 1));
    }
    out
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Fetch `url`, render it into a buffer and put the viewer overlay on top.
/// `record` is false for back/forward, which move within the history rather than
/// adding to it.
fn load_url(cx: &mut Context, url: &str, record: bool) {
    let url = crate::eww::normalize_url(url);
    if record {
        history_push(&url);
    }
    let target = url.clone();
    let callback = async move {
        let fetched = tokio::task::spawn_blocking(move || crate::eww::fetch(&target))
            .await
            .unwrap_or_else(|e| Err(format!("join: {e}")));
        let call: crate::job::Callback = crate::job::Callback::EditorCompositor(Box::new(
            move |editor: &mut Editor, compositor: &mut Compositor| match fetched {
                Ok((body, ctype)) => {
                    let text = if ctype.contains("html") || body.trim_start().starts_with('<') {
                        crate::eww::html_to_text(&body, &url)
                    } else {
                        body
                    };
                    let page = format!("xwidget-webkit: {url}\n{}\n\n{text}", "═".repeat(60));
                    crate::commands::show_text_in_scratch(editor, &page);
                    push_viewer(compositor);
                }
                Err(e) => editor.set_error(format!("xwidget-webkit: {url}: {e}")),
            },
        ));
        Ok(call)
    };
    cx.jobs.callback(callback);
}

/// Put the viewer overlay on the compositor unless it is already there.
fn push_viewer(compositor: &mut Compositor) {
    if compositor.find_id::<XwidgetWebkit>(ID).is_none() {
        compositor.push(Box::new(XwidgetWebkit::new()));
    }
}

/// The prompt `xwidget-webkit-browse-url` reads its URL with.
fn url_prompt() -> crate::ui::prompt::Prompt {
    crate::ui::prompt::Prompt::new(
        "xwidget-webkit URL: ".into(),
        None,
        crate::ui::completers::none,
        |cx: &mut Context, input: &str, event: PromptEvent| {
            if event != PromptEvent::Validate {
                return;
            }
            if !input.trim().is_empty() {
                load_url(cx, input.trim(), true);
            }
        },
    )
}

/// `xwidget-webkit-reload`: fetch the page on screen again.
fn reload(cx: &mut Context) {
    match history_current() {
        Some(url) => load_url(cx, &url, false),
        None => cx.editor.set_error("xwidget-webkit: no page loaded"),
    }
}

/// `xwidget-webkit-back` / `-forward`.
fn history_go(cx: &mut Context, delta: isize) {
    match history_step(delta) {
        Some(url) => load_url(cx, &url, false),
        None => cx.editor.set_status(if delta < 0 {
            "xwidget-webkit: no earlier page"
        } else {
            "xwidget-webkit: no later page"
        }),
    }
}

/// `xwidget-webkit-browse-history`: the `*Xwidget WebKit History*` buffer.
fn show_history(cx: &mut Context) {
    let listing = history_listing();
    crate::commands::show_text_in_scratch(cx.editor, &listing);
    cx.editor.set_status("Xwidget WebKit History");
}

// ---------------------------------------------------------------------------
// The viewer overlay
// ---------------------------------------------------------------------------

/// An incremental search over the rendered page — `xwidget-webkit-isearch-mode`.
#[derive(Default)]
struct Isearch {
    query: String,
    /// Which way the last `C-s`/`C-r` moved; the mode starts forwards.
    backward: bool,
    /// Set when the query has no match anywhere, so the prompt can say so the
    /// way `isearch` does.
    failing: bool,
}

/// The viewer overlay. Owns no page content — the page is the buffer beneath —
/// only the mode flags Emacs keeps as buffer-local minor modes.
#[derive(Default)]
pub struct XwidgetWebkit {
    /// `xwidget-webkit-edit-mode`: keystrokes reach the content instead of the
    /// mode's one-key commands.
    edit: bool,
    /// `xwidget-webkit-isearch-mode`, active while searching.
    isearch: Option<Isearch>,
}

impl XwidgetWebkit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Turn `xwidget-webkit-edit-mode` on or off, reporting it the way a minor
    /// mode does.
    pub fn toggle_edit(&mut self, editor: &mut Editor) {
        self.edit = !self.edit;
        editor.set_status(if self.edit {
            "Xwidget-Webkit-Edit mode enabled (Esc leaves it)"
        } else {
            "Xwidget-Webkit-Edit mode disabled"
        });
    }

    /// Start `xwidget-webkit-isearch-mode` in the given direction.
    pub fn start_isearch(&mut self, backward: bool) {
        self.isearch = Some(Isearch {
            backward,
            ..Isearch::default()
        });
    }

    /// Move the cursor to the next/previous occurrence of the query, searching
    /// from the cursor and wrapping — `isearch` semantics over the rendered page.
    fn isearch_move(&mut self, cx: &mut Context, backward: bool) {
        let Some(state) = self.isearch.as_mut() else {
            return;
        };
        state.backward = backward;
        if state.query.is_empty() {
            return;
        }
        let needle = state.query.to_lowercase();
        let query_len = state.query.chars().count();

        let view_id = {
            let (view, doc) = current!(cx.editor);
            let hay = doc.text().to_string().to_lowercase();
            let cursor = doc
                .selection(view.id)
                .primary()
                .cursor(doc.text().slice(..));
            // The selection is in chars; `str::find` answers in bytes.
            let byte_of = |ch: usize| {
                hay.char_indices()
                    .nth(ch)
                    .map(|(b, _)| b)
                    .unwrap_or(hay.len())
            };
            let found = if backward {
                hay[..byte_of(cursor)]
                    .rfind(&needle)
                    .or_else(|| hay.rfind(&needle))
            } else {
                let from = byte_of(cursor + 1);
                hay[from..]
                    .find(&needle)
                    .map(|i| i + from)
                    .or_else(|| hay.find(&needle))
            };
            match found {
                Some(byte) => {
                    let start = hay[..byte].chars().count();
                    state.failing = false;
                    doc.set_selection(view.id, Selection::single(start, start + query_len));
                    Some(view.id)
                }
                None => {
                    state.failing = true;
                    None
                }
            }
        };
        if let Some(id) = view_id {
            cx.editor.ensure_cursor_in_view(id);
        }
    }

    /// The isearch minibuffer's own keys: type, rub out, step, exit.
    fn handle_isearch_key(&mut self, key: KeyEvent, cx: &mut Context) -> EventResult {
        match key {
            key!(Enter) | key!(Esc) => {
                self.isearch = None;
                cx.editor.set_status("Mark saved where search started");
            }
            ctrl!('s') => self.isearch_move(cx, false),
            ctrl!('r') => self.isearch_move(cx, true),
            key!(Backspace) => {
                if let Some(state) = self.isearch.as_mut() {
                    state.query.pop();
                }
                let back = self.isearch.as_ref().is_some_and(|s| s.backward);
                self.isearch_move(cx, back);
            }
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
            } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                if let Some(state) = self.isearch.as_mut() {
                    state.query.push(c);
                }
                let back = self.isearch.as_ref().is_some_and(|s| s.backward);
                self.isearch_move(cx, back);
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }
}

impl Component for XwidgetWebkit {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        if self.isearch.is_some() {
            return self.handle_isearch_key(key, cx);
        }

        // Edit mode: characters belong to the content, so only the key that
        // leaves the mode is still ours.
        if self.edit {
            if key == key!(Esc) {
                self.toggle_edit(cx.editor);
                return EventResult::Consumed(None);
            }
            return EventResult::Ignored(None);
        }

        match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close)),
            key!('g') => {
                let open: Callback = Box::new(|compositor: &mut Compositor, _cx| {
                    compositor.push(Box::new(url_prompt()));
                });
                return EventResult::Consumed(Some(open));
            }
            key!('r') => reload(cx),
            key!('b') => history_go(cx, -1),
            key!('f') => history_go(cx, 1),
            key!('w') => match history_current() {
                Some(url) => cx.editor.set_status(url),
                None => cx.editor.set_status("xwidget-webkit: no page loaded"),
            },
            key!('H') => show_history(cx),
            key!('e') => self.toggle_edit(cx.editor),
            ctrl!('s') => {
                self.start_isearch(false);
                self.isearch_move(cx, false);
            }
            ctrl!('r') => {
                self.start_isearch(true);
                self.isearch_move(cx, true);
            }
            _ => return EventResult::Ignored(None),
        }
        EventResult::Consumed(None)
    }

    /// Only the isearch prompt is drawn; the page is the buffer underneath and
    /// paints itself.
    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let Some(state) = &self.isearch else { return };
        if area.height == 0 {
            return;
        }
        let style = ctx.editor.theme.get("ui.text");
        let dir = if state.backward { "reverse " } else { "" };
        let fail = if state.failing { "failing " } else { "" };
        let line = format!("{fail}Webkit {dir}I-search: {}", state.query);
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &line,
            area.width as usize,
            style,
        );
    }

    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
}

// ---------------------------------------------------------------------------
// Typable commands
// ---------------------------------------------------------------------------

/// `:xwidget-webkit-browse-url [url]` — Emacs `xwidget-webkit-browse-url`: browse
/// a URL in the WebKit buffer, prompting for it when none is given. A bare host
/// gets `https://` prepended, as Emacs does.
pub fn ex_browse_url(cx: &mut Context, args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let url = args.join(" ");
    if url.trim().is_empty() {
        let call: crate::job::Callback = crate::job::Callback::EditorCompositor(Box::new(
            |_editor: &mut Editor, compositor: &mut Compositor| {
                compositor.push(Box::new(url_prompt()));
            },
        ));
        cx.jobs.callback(async move { Ok(call) });
        return Ok(());
    }
    load_url(cx, url.trim(), true);
    Ok(())
}

/// `:xwidget-webkit-mode` — Emacs `xwidget-webkit-mode`, the major mode of a
/// WebKit buffer, which is what supplies its one-key commands. Running it puts
/// the viewer's keymap over the current buffer; running it again takes it off,
/// the way `image-mode` doubles as its own toggle.
pub fn ex_mode(cx: &mut Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: crate::job::Callback = crate::job::Callback::EditorCompositor(Box::new(
        |editor: &mut Editor, compositor: &mut Compositor| {
            if compositor.find_id::<XwidgetWebkit>(ID).is_some() {
                compositor.pop();
                editor.set_status("Xwidget-WebKit mode disabled");
            } else {
                compositor.push(Box::new(XwidgetWebkit::new()));
                editor.set_status("Xwidget-WebKit mode enabled (g browse, b/f history, q quit)");
            }
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

/// `:xwidget-webkit-browse-history` — Emacs `xwidget-webkit-browse-history`:
/// display a buffer containing the history of page loads.
pub fn ex_browse_history(cx: &mut Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    show_history(cx);
    Ok(())
}

/// `:xwidget-webkit-edit-mode` — Emacs `xwidget-webkit-edit-mode`: send
/// self-inserting characters to the page instead of running the mode's one-key
/// commands.
pub fn ex_edit_mode(cx: &mut Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: crate::job::Callback = crate::job::Callback::EditorCompositor(Box::new(
        |editor: &mut Editor, compositor: &mut Compositor| match compositor
            .find_id::<XwidgetWebkit>(ID)
        {
            Some(view) => view.toggle_edit(editor),
            None => editor.set_error("xwidget-webkit-edit-mode: no WebKit buffer"),
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

/// `:xwidget-webkit-isearch-mode` — Emacs `xwidget-webkit-isearch-mode`: search
/// incrementally inside the WebKit buffer, `C-s`/`C-r` stepping between results.
pub fn ex_isearch_mode(cx: &mut Context, _args: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let call: crate::job::Callback = crate::job::Callback::EditorCompositor(Box::new(
        |editor: &mut Editor, compositor: &mut Compositor| match compositor
            .find_id::<XwidgetWebkit>(ID)
        {
            Some(view) => {
                view.start_isearch(false);
                editor.set_status("Webkit I-search: (C-s next, C-r previous, RET exits)");
            }
            None => editor.set_error("xwidget-webkit-isearch-mode: no WebKit buffer"),
        },
    ));
    cx.jobs.callback(async move { Ok(call) });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `HISTORY` is process-global, and cargo runs these tests on parallel
    /// threads in one process — so without a lock of their own, one test's
    /// `reset()` lands in the middle of the other's sequence and the walk sees
    /// an empty history. Serialise them.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Reset the shared history so the assertions below are order-independent.
    fn reset() {
        let mut h = HISTORY.lock().unwrap();
        h.0.clear();
        h.1 = 0;
    }

    #[test]
    fn history_walks_back_and_forward_and_truncates() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        history_push("https://a");
        history_push("https://b");
        history_push("https://c");
        assert_eq!(history_current().as_deref(), Some("https://c"));
        assert_eq!(history_step(-1).as_deref(), Some("https://b"));
        assert_eq!(history_step(-1).as_deref(), Some("https://a"));
        // Already at the oldest entry: no move, like Emacs's back at the end.
        assert_eq!(history_step(-1), None);
        assert_eq!(history_step(1).as_deref(), Some("https://b"));
        // A fresh load from the middle drops what was ahead.
        history_push("https://d");
        assert_eq!(history_step(1), None);
        assert!(history_listing().contains("https://d"));
        assert!(!history_listing().contains("https://c"));
        reset();
    }

    #[test]
    fn history_ignores_a_reload_of_the_same_url() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        history_push("https://a");
        history_push("https://a");
        assert_eq!(history_step(-1), None);
        assert_eq!(history_current().as_deref(), Some("https://a"));
        reset();
    }
}
