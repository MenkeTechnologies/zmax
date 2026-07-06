//! Embedded scripting host.
//!
//! Every embedded interpreter (elisp first; vimscript / perl / awk / zsh to
//! follow) drives the editor through ONE uniform API defined here. The
//! interpreters expose host callbacks as bare `fn` pointers with thread-local
//! state, so the editor can't be captured in a closure — instead the active
//! command [`compositor::Context`] is published through a thread-local raw
//! pointer for the duration of a single, synchronous, on-editor-thread eval
//! (installed by `CxGuard`, cleared on drop). Each language binding marshals
//! its own value type and registers these `api_*` helpers under idiomatic
//! names; the helpers are language-agnostic.
//!
//! Re-entrancy contract: an `api_*` helper must not itself trigger another
//! script eval while it holds the `&mut compositor::Context` from `with_cx`.
//! Nested evals (a future feature) restore the previous pointer via the guard
//! stack, but two live `&mut` borrows of the same context would alias.

use std::cell::{Cell, RefCell};
use std::ptr;

use elisprs::host::ElispHost;
use zemacs_core::{Selection, Tendril, Transaction};

use crate::compositor;
use crate::ui::prompt::PromptEvent;

pub mod awk;
mod capture;
pub mod elisp;
pub mod stryke;
pub mod viml;
mod viml_theme;
pub mod zsh;

thread_local! {
    /// Type-erased pointer to the `compositor::Context` of the in-flight eval.
    static CX_PTR: Cell<*mut ()> = const { Cell::new(ptr::null_mut()) };
}

/// RAII guard publishing the current command context for the duration of an
/// eval. Restores the previous pointer on drop so nested evals are sound.
struct CxGuard {
    prev: *mut (),
}

impl CxGuard {
    fn new(cx: &mut compositor::Context) -> Self {
        let prev = CX_PTR.with(|c| c.get());
        CX_PTR.with(|c| c.set(cx as *mut compositor::Context as *mut ()));
        CxGuard { prev }
    }
}

impl Drop for CxGuard {
    fn drop(&mut self) {
        CX_PTR.with(|c| c.set(self.prev));
    }
}

/// Run `f` with the active editor context. Errors if called outside an eval.
fn with_cx<R>(f: impl FnOnce(&mut compositor::Context) -> R) -> Result<R, String> {
    CX_PTR.with(|c| {
        let p = c.get() as *mut compositor::Context;
        if p.is_null() {
            return Err("editor API called with no active context".to_string());
        }
        // SAFETY: `p` was installed by a `CxGuard` whose scope encloses this
        // call; eval is synchronous on this thread and the pointer is cleared
        // on guard drop. The single-threaded interpreter never aliases it (see
        // the re-entrancy contract above).
        Ok(f(unsafe { &mut *p }))
    })
}

// ── Language-agnostic editor API ──────────────────────────────────────────
//
// These are the primitives every language binds. They return `Result<_,String>`
// so a binding can surface failures as that language's error type.

/// Show a status-line message.
pub(super) fn api_message(text: &str) -> Result<(), String> {
    with_cx(|cx| cx.editor.set_status(text.to_string()))
}

/// Show a status-line error.
pub(super) fn api_error(text: &str) -> Result<(), String> {
    with_cx(|cx| cx.editor.set_error(text.to_string()))
}

/// Run a typable (`:`) command by name with already-split string arguments.
pub(super) fn api_command(name: &str, args: &[String]) -> Result<(), String> {
    let joined = args.join(" ");
    with_cx(|cx| {
        let cmd = crate::commands::typed::TYPABLE_COMMAND_MAP
            .get(name)
            .ok_or_else(|| format!("no such command: '{name}'"))?;
        crate::commands::typed::execute_command(cx, cmd, &joined, PromptEvent::Validate)
            .map_err(|e| e.to_string())
    })?
}

// ── Line-oriented editor API (Vimscript getline/setline/cursor/…) ──────────

/// Buffer line count in Vim terms (ropey counts the char after a trailing
/// newline as an extra empty line; Vim's line count does not include it).
pub(super) fn api_line_count() -> Result<i64, String> {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let t = doc.text();
        let n = t.len_lines();
        if n > 1 && t.line(n - 1).len_chars() == 0 {
            (n - 1) as i64
        } else {
            n as i64
        }
    })
}

/// 1-based line `lnum` without its trailing newline, or `None` if out of range.
pub(super) fn api_get_line(lnum: i64) -> Result<Option<String>, String> {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let t = doc.text();
        if lnum < 1 {
            return None;
        }
        let i = (lnum - 1) as usize;
        if i >= t.len_lines() {
            return None;
        }
        let mut s = t.line(i).to_string();
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        Some(s)
    })
}

/// Primary cursor as `(line, col)`, both 1-based.
pub(super) fn api_cursor() -> Result<(i64, i64), String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let t = doc.text();
        let c = doc.selection(view.id).primary().cursor(t.slice(..));
        let line = t.char_to_line(c);
        let col = c - t.line_to_char(line);
        ((line + 1) as i64, (col + 1) as i64)
    })
}

/// Move the primary cursor to 1-based `(line, col)`, clamped to the buffer.
pub(super) fn api_set_cursor(line: i64, col: i64) -> Result<(), String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let t = doc.text();
        let li = ((line.max(1) - 1) as usize).min(t.len_lines().saturating_sub(1));
        let base = t.line_to_char(li);
        let raw = t.line(li).to_string();
        let linelen = raw.trim_end_matches(['\n', '\r']).chars().count();
        let off = ((col.max(1) - 1) as usize).min(linelen);
        doc.set_selection(view.id, Selection::point(base + off));
    })
}

/// `setline`/`append` over the live buffer. `append == false` replaces the lines
/// from `lnum`; `append == true` inserts after line `lnum` (`lnum == 0` before
/// line 1). Returns 0 on success, 1 on an out-of-range replace.
pub(super) fn api_set_lines(lnum: i64, lines: Vec<String>, append: bool) -> Result<i64, String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let nlines = doc.text().len_lines();
        if append {
            let li = (lnum.max(0) as usize).min(nlines);
            let pos = doc.text().line_to_char(li);
            let ins: String = lines.iter().map(|l| format!("{l}\n")).collect();
            let tendril: Tendril = ins.into();
            let tx = Transaction::change(doc.text(), std::iter::once((pos, pos, Some(tendril))));
            doc.apply(&tx, view.id);
            0
        } else {
            if lnum < 1 {
                return 1;
            }
            let start_li = ((lnum - 1) as usize).min(nlines);
            let end_li = (start_li + lines.len()).min(nlines);
            let a = doc.text().line_to_char(start_li);
            let b = doc.text().line_to_char(end_li);
            let repl: String = lines.iter().map(|l| format!("{l}\n")).collect();
            let tendril: Tendril = repl.into();
            let tx = Transaction::change(doc.text(), std::iter::once((a, b, Some(tendril))));
            doc.apply(&tx, view.id);
            0
        }
    })
}

/// Current buffer path/name (empty for an unnamed buffer).
pub(super) fn api_buf_name() -> Result<String, String> {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

// ── elisp ⇄ live-buffer sync ────────────────────────────────────────────────
//
// elisprs owns a full set of buffer builtins (point, insert, forward-line,
// search-forward, re-search-forward, looking-at, skip-chars-forward,
// forward-word, replace-match, …) that operate on its own in-memory
// `EditBuffer` (a `Vec<char>` + 1-based point). Rather than re-implement each of
// those against the rope — and leave the un-ported ones silently editing a
// phantom empty buffer — we mirror the live buffer into that `EditBuffer` before
// an eval and flush it back afterwards. Every current and future elisp buffer
// builtin then drives the live buffer for free.

// The Emacs mark lives alongside point but has no home in elisprs's `EditBuffer`
// (which only tracks text + point), so we hold it here, in 0-based char units,
// mirrored from / flushed to the live selection's anchor just like point tracks
// the head. Keeping it here — not reading the live selection on demand — is what
// makes region queries coherent while point moves through the mirror mid-eval.
thread_local! {
    static MARK: Cell<Option<usize>> = const { Cell::new(None) };
    static MARK_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Set the mark to a 0-based char position and activate the region.
pub(super) fn mark_set(pos0: usize) {
    MARK.with(|m| m.set(Some(pos0)));
    MARK_ACTIVE.with(|a| a.set(true));
}

/// The mark's 0-based position, or `None` if it has never been set.
pub(super) fn mark_get() -> Option<usize> {
    MARK.with(|m| m.get())
}

/// Whether the region is active (mark set and not since deactivated).
pub(super) fn mark_is_active() -> bool {
    MARK_ACTIVE.with(|a| a.get()) && MARK.with(|m| m.get().is_some())
}

/// Deactivate the region without forgetting the mark position.
pub(super) fn mark_deactivate() {
    MARK_ACTIVE.with(|a| a.set(false));
}

// The kill ring, front = most recent kill. Emacs semantics: new kills prepend,
// `yank` inserts the front, `current-kill` indexes into it. Held here rather than
// on the elisprs host so it survives host resets and is testable without a
// context; each push is also mirrored into the editor's yank + clipboard
// registers so the editor's own paste yanks elisp-killed text.
thread_local! {
    static KILL_RING: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Cap on retained kills — matches Emacs's default `kill-ring-max`.
const KILL_RING_MAX: usize = 120;

/// Prepend `s` to the kill ring and mirror it to the editor registers.
pub(super) fn kill_push(s: String) {
    KILL_RING.with(|r| {
        let mut v = r.borrow_mut();
        v.insert(0, s.clone());
        v.truncate(KILL_RING_MAX);
    });
    // Best-effort: land the kill in the default yank register (so the editor's
    // paste yanks it) and the system clipboard. A null context or a failing
    // clipboard provider is ignored — the ring is the source of truth.
    let _ = with_cx(|cx| {
        let _ = cx.editor.registers.write('"', vec![s.clone()]);
        let _ = cx.editor.registers.write('+', vec![s]);
    });
}

/// The `n`th kill (0 = most recent), indexing the ring modulo its length as
/// Emacs's `current-kill` does. `None` only if the ring is empty.
pub(super) fn kill_current(n: i64) -> Option<String> {
    KILL_RING.with(|r| {
        let v = r.borrow();
        if v.is_empty() {
            return None;
        }
        let len = v.len() as i64;
        let idx = ((n % len) + len) % len;
        Some(v[idx as usize].clone())
    })
}

/// Copy the live current buffer's text and primary-cursor point (1-based) into
/// the elisp interpreter's current `EditBuffer`, and mirror the selection anchor
/// into the mark. Takes the host by `&mut` (never `with_host`) so it is safe to
/// call from inside a subr, which already holds the host borrow. Best-effort: a
/// null context (no active eval) is a no-op.
pub(super) fn load_buffer_into_host(h: &mut ElispHost) {
    let loaded = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let t = doc.text();
        let range = doc.selection(view.id).primary();
        let cursor = range.cursor(t.slice(..));
        (t.to_string(), cursor + 1, range.anchor, cursor)
    });
    if let Ok((text, point, anchor, head)) = loaded {
        let buf = h.cur_buf();
        buf.text = text.chars().collect();
        buf.point = point.max(1);
        // An anchor distinct from the caret means the live selection is a region
        // → the mark is set and active; a bare cursor leaves the mark alone.
        if anchor != head {
            MARK.with(|m| m.set(Some(anchor)));
            MARK_ACTIVE.with(|a| a.set(true));
        }
    }
}

/// Flush the elisp `EditBuffer` back to the live buffer: if the text changed,
/// replace the whole buffer as one undo step; always move the primary cursor to
/// elisp's point. Whole-buffer replacement (rather than a minimal diff) keeps
/// this simple at the cost of collapsing an eval's edits into a single undo
/// step — acceptable for `M-x eval` / scripted commands.
pub(super) fn flush_host_into_buffer(h: &mut ElispHost) {
    let new_text: String = h.cur_buf().text.iter().collect();
    let point = h.cur_buf().point;
    let _ = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        if doc.text() != &new_text {
            let len = doc.text().len_chars();
            let tendril: Tendril = new_text.as_str().into();
            let tx = Transaction::change(doc.text(), std::iter::once((0, len, Some(tendril))));
            doc.apply(&tx, view.id);
        }
        let max = doc.text().len_chars();
        let head = point.saturating_sub(1).min(max);
        // If the region is active, restore it as a live selection (anchor = mark,
        // head = point) so `(set-mark) (forward-word)` visibly selects, like
        // Emacs; otherwise collapse to a bare cursor at point.
        let sel = match mark_get() {
            Some(mark) if mark_is_active() && mark.min(max) != head => {
                Selection::single(mark.min(max), head)
            }
            _ => Selection::point(head),
        };
        doc.set_selection(view.id, sel);
    });
}

// ── Public entry points ────────────────────────────────────────────────────

/// Evaluate an elisp source string against the live editor. Returns the printed
/// result on success. Runs synchronously on the editor thread. The live buffer
/// is mirrored into elisprs's `EditBuffer` before the eval and flushed back
/// after, so elisp's native buffer builtins act on the live buffer. On an eval
/// error the buffer is left untouched (no partial flush).
pub fn eval_elisp(cx: &mut compositor::Context, src: &str) -> Result<String, String> {
    let _guard = CxGuard::new(cx);
    elisp::ensure_builtins();
    elisp::ensure_editor_lisp();
    elisprs::with_host(load_buffer_into_host);
    let value = elisprs::eval_str(src)?;
    elisprs::with_host(flush_host_into_buffer);
    Ok(elisprs::print(&value, true))
}

// Tracks whether the vimlrs -> editor host hooks have been installed on this
// thread (see install_viml_hooks). thread_local because vimlrs state is
// thread-local and the hooks bridge into it.
thread_local! {
    static VIML_HOOKS_INSTALLED: Cell<bool> = const { Cell::new(false) };
}

/// Install vimlrs → editor host hooks once per thread. Currently bridges the
/// `:set` ex-command: whenever vimlrs runs `:set` (from `:vim`, `init.vim`, or a
/// sourced plugin) it mirrors the option onto the live editor by running
/// zemacs's own `:set` command through [`with_cx`]. This is the first editor
/// ex-command wired through; `:map`/`:command`/`:autocmd` follow the same shape.
fn install_viml_host_hooks() {
    if VIML_HOOKS_INSTALLED.with(|c| c.replace(true)) {
        return;
    }
    // Editor builtins (getline/setline/append/getbufline, line()/col()/getpos()/
    // setpos()/cursor(), bufname()/bufnr()) → the live buffer/cursor. Installed
    // once; each callback resolves the current context via `with_cx` at call time.
    vimlrs::fusevm_bridge::install_editor_host(vimlrs::fusevm_bridge::EditorHost {
        line_count: Box::new(|| api_line_count().unwrap_or(1)),
        get_line: Box::new(|n| api_get_line(n).ok().flatten()),
        set_lines: Box::new(|lnum, lines, append| api_set_lines(lnum, lines, append).unwrap_or(1)),
        cursor: Box::new(|| api_cursor().unwrap_or((1, 1))),
        set_cursor: Box::new(|l, c| {
            let _ = api_set_cursor(l, c);
        }),
        buf_name: Box::new(|| api_buf_name().unwrap_or_default()),
        // Vimscript's current-buffer number; zemacs presents a single current
        // buffer to scripts, so 1 (matches `bufnr('')` on a normal buffer).
        buf_nr: Box::new(|| 1),
    });
    vimlrs::fusevm_bridge::install_set_hook(Box::new(|args: &str| {
        let _ = with_cx(|cx| {
            crate::commands::typed::run_command_line(cx, &format!("set {args}"));
        });
    }));
    // `:map`/`:nmap`/`:nnoremap`/… → the live zemacs keymap. vimlrs fires the
    // raw command line; we record it in the runtime overlay and ask the
    // application to merge the overlay onto `config.keys`.
    vimlrs::fusevm_bridge::install_map_hook(Box::new(|line: &str| {
        let _ = with_cx(|cx| {
            match crate::keymap::vim_map::register_map_line(line) {
                Ok(crate::keymap::vim_map::MapOutcome::Applied(_)) => {
                    cx.editor
                        .config_events
                        .0
                        .send(zemacs_view::editor::ConfigEvent::ApplyUserMappings)
                        .ok();
                }
                // A bare `:map`/`:nmap` query while sourcing a plugin: don't pop a
                // listing buffer during startup.
                Ok(crate::keymap::vim_map::MapOutcome::List(_)) => {}
                Err(e) => log::debug!("vim map `{line}` not applied: {e}"),
            }
        });
    }));
    // Colours. vimlrs records every `:highlight` group (and, for `:colorscheme`,
    // sources the scheme's `colors/*.vim` first) in its highlight registry. Each
    // `:highlight` just marks the theme dirty; `:colorscheme` (fired once the
    // scheme file is fully sourced) rebuilds + applies the zemacs theme from the
    // registry. Trailing standalone `:highlight` overrides in the vimrc are
    // flushed after sourcing (see `flush_viml_theme`).
    vimlrs::fusevm_bridge::add_colorscheme_dir(zemacs_loader::config_dir());
    vimlrs::fusevm_bridge::install_highlight_hook(Box::new(|_args: &str| {
        VIML_THEME_DIRTY.with(|d| d.set(true));
    }));
    vimlrs::fusevm_bridge::install_colorscheme_hook(Box::new(|name: &str| {
        VIML_SCHEME_NAME.with(|n| *n.borrow_mut() = name.to_string());
        VIML_THEME_DIRTY.with(|d| d.set(false));
        let _ = with_cx(|cx| apply_viml_theme(cx, name));
    }));
}

thread_local! {
    /// Set whenever a `:highlight` runs; a post-source flush rebuilds the theme
    /// so trailing `:highlight` overrides (outside a `:colorscheme`) still apply.
    static VIML_THEME_DIRTY: Cell<bool> = const { Cell::new(false) };
    /// The most recent `:colorscheme` name, for naming a theme built from
    /// standalone `:highlight` commands (no `:colorscheme`).
    static VIML_SCHEME_NAME: std::cell::RefCell<String> =
        const { std::cell::RefCell::new(String::new()) };
}

/// Build a theme from vimlrs's current highlight registry and apply it live.
fn apply_viml_theme(cx: &mut compositor::Context, name: &str) {
    if let Some(theme) = viml_theme::build_theme(name) {
        if let Err(e) = cx.editor.set_theme(theme) {
            log::debug!("vim colorscheme `{name}` not applied: {e}");
        }
    }
}

/// After sourcing a vimrc, apply any pending `:highlight` changes that were not
/// already applied by a `:colorscheme` (e.g. a vimrc that only sets highlights,
/// or adds overrides after its `:colorscheme` line).
fn flush_viml_theme(cx: &mut compositor::Context) {
    if VIML_THEME_DIRTY.with(|d| d.replace(false)) {
        let name = VIML_SCHEME_NAME.with(|n| n.borrow().clone());
        apply_viml_theme(cx, &name);
    }
}

pub fn eval_viml(cx: &mut compositor::Context, src: &str) -> Result<String, String> {
    // Publish the context so host hooks (e.g. `:set`) can reach the live editor.
    let _guard = CxGuard::new(cx);
    install_viml_host_hooks();
    let out = viml::eval(src);
    // Apply any `:highlight`s not already applied by a `:colorscheme` (e.g. a
    // bare `:hi Comment …` typed at the `:vim` prompt).
    with_cx(flush_viml_theme).ok();
    out
}

/// Source a Vimscript file through the embedded vimlrs interpreter (the vim
/// `:source` ex-command). Uses vimlrs's file evaluator so the script runs with
/// script context (`s:` scope, `<SID>`, line continuations) rather than
/// string-eval, then flushes any theme a sourced `:colorscheme`/`:highlight`
/// defined. Same host-hook wiring as `load_init_scripts`' vimrc pass.
pub fn source_viml_file(
    cx: &mut compositor::Context,
    path: &std::path::Path,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        let _guard = CxGuard::new(cx);
        install_viml_host_hooks();
        let res = vimlrs::fusevm_bridge::eval_file(path).map_err(|e| e.0);
        let _ = with_cx(flush_viml_theme);
        res
    }
    #[cfg(not(unix))]
    {
        let _ = (cx, path);
        Err("vimlrs (`:source`) is only available on unix".to_string())
    }
}

/// Filter the primary selection (or the whole buffer, if the selection is
/// empty) through an awk `program`, replacing it with the program's output as
/// one undo step. Returns a short status message.
pub fn run_awk_filter(cx: &mut compositor::Context, program: &str) -> Result<String, String> {
    let _guard = CxGuard::new(cx);

    // Read the target range and its text.
    let (from, to, input) = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text();
        let sel = doc.selection(view.id).primary();
        let (f, t) = (sel.from(), sel.to());
        if f == t {
            (0usize, text.len_chars(), text.to_string())
        } else {
            (f, t, text.slice(f..t).to_string())
        }
    })?;

    // Run awk outside any editor borrow (it must not re-enter the context).
    let output = awk::run(program, &input)?;

    // Replace the range with the output.
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let tendril: Tendril = output.as_str().into();
        let tx = Transaction::change(doc.text(), std::iter::once((from, to, Some(tendril))));
        doc.apply(&tx, view.id);
    })?;

    Ok(format!("awk: filtered {} chars", to.saturating_sub(from)))
}

/// Evaluate stryke source via the embedded strykelang interpreter. Returns
/// captured `print` output or the last expression value. State persists across
/// calls. Does not touch the editor (no host-fn bridge yet), so no context guard.
pub fn eval_stryke(_cx: &mut compositor::Context, code: &str) -> Result<String, String> {
    stryke::eval(code)
}

/// Run a zsh command line through the embedded shell, capturing stdout+stderr.
/// Shell state (vars/functions/cwd) persists across calls. Returns (exit
/// status, captured output). Does not touch the editor, so no context guard is
/// needed.
pub fn run_zsh(cmd: &str) -> Result<(i32, String), String> {
    zsh::run(cmd)
}

/// Run an awk `program` against the current buffer's text and RETURN its output
/// without modifying the buffer — the REPL counterpart to [`run_awk_filter`],
/// which replaces the selection in place. Used by the embedded-language REPL.
pub fn repl_awk(cx: &mut compositor::Context, program: &str) -> Result<String, String> {
    let _guard = CxGuard::new(cx);
    let input = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let _ = view;
        doc.text().to_string()
    })?;
    awk::run(program, &input)
}

/// Load embedded-scripting init files if present (best-effort; errors go to the
/// status line). Called once at startup after the editor is constructed.
pub fn load_init_scripts(cx: &mut compositor::Context) {
    let dir = zemacs_loader::config_dir();

    // Emacs Lisp init. zemacs's own config-dir `init.el` is always sourced; the
    // user's PERSONAL Emacs config (`~/.emacs.d/init.el`, `~/.config/emacs/init.el`,
    // `~/.emacs`) is sourced only when the `source-emacs-config` setting is enabled
    // (off by default) — symmetric with `source-vimrc`. zemacs is not Emacs and
    // must not silently run a personal init.el. Personal files are sourced first,
    // then zemacs's own `init.el` last so a zemacs-specific override wins.
    let mut el_candidates: Vec<std::path::PathBuf> = Vec::new();
    if cx.editor.config().source_emacs_config {
        if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
            el_candidates.push(home.join(".emacs.d/init.el"));
            el_candidates.push(home.join(".config/emacs/init.el"));
            el_candidates.push(home.join(".emacs"));
        }
    }
    el_candidates.push(dir.join("init.el"));
    // An arbitrary user-specified Emacs Lisp file, sourced last so it has final say.
    if let Some(file) = cx.editor.config().source_elisp_file.clone() {
        if !file.trim().is_empty() {
            el_candidates.push(
                zemacs_stdx::path::expand_tilde(std::path::Path::new(file.trim())).into_owned(),
            );
        }
    }
    let el_present: Vec<std::path::PathBuf> =
        el_candidates.into_iter().filter(|p| p.exists()).collect();

    if !el_present.is_empty() {
        let _guard = CxGuard::new(cx);
        elisp::ensure_builtins();
        elisp::ensure_editor_lisp();
        // Load via eval_str, not eval_file: eval_file resets the host (for its
        // bytecode cache), which would wipe the editor subrs / editor-lisp that
        // init.el needs. Correctness over the cache for a small config file.
        for init_el in el_present {
            match std::fs::read_to_string(&init_el) {
                Ok(src) => {
                    elisprs::with_host(load_buffer_into_host);
                    if let Err(e) = elisprs::eval_str(&src) {
                        cx.editor.set_error(format!("{}: {e}", init_el.display()));
                    } else {
                        elisprs::with_host(flush_host_into_buffer);
                    }
                }
                Err(e) => cx.editor.set_error(format!("{}: {e}", init_el.display())),
            }
        }
    }

    #[cfg(unix)]
    {
        // Source Vim configuration so zemacs honours `:set`/`:map`/`:colorscheme`.
        // Files are sourced in increasing priority — the user's personal config
        // first, then zemacs's own `init.vim`, so a zemacs-specific override wins.
        // Each is best-effort and independent; one failing does not stop the
        // others.
        //
        // The user's *personal* Vim files (`~/.vimrc` etc.) are read ONLY when the
        // `source-vimrc` setting is enabled (off by default): zemacs is not Vim
        // and must not silently inherit a personal `.vimrc`. zemacs's own
        // `init.vim` in the config dir is always sourced — it is an explicit
        // zemacs config, not the user's Vim setup.
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if cx.editor.config().source_vimrc {
            if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
                candidates.push(home.join(".vimrc"));
                candidates.push(home.join(".vim/vimrc"));
                candidates.push(home.join(".config/nvim/init.vim"));
            }
        }
        candidates.push(dir.join("init.vim"));
        // An arbitrary user-specified Vimscript file, sourced last so it wins.
        if let Some(file) = cx.editor.config().source_viml_file.clone() {
            if !file.trim().is_empty() {
                candidates.push(
                    zemacs_stdx::path::expand_tilde(std::path::Path::new(file.trim())).into_owned(),
                );
            }
        }

        let mut any = false;
        for path in candidates {
            if path.exists() {
                let _guard = CxGuard::new(cx);
                install_viml_host_hooks();
                if let Err(e) = vimlrs::fusevm_bridge::eval_file(&path) {
                    cx.editor.set_error(format!("{}: {}", path.display(), e.0));
                }
                any = true;
            }
        }
        // Apply any highlights a sourced vimrc defined without a `:colorscheme`.
        if any {
            let _guard = CxGuard::new(cx);
            let _ = with_cx(flush_viml_theme);
        }
    }
}

#[cfg(test)]
mod tests {
    /// The embedded elisprs interpreter links and runs inside zemacs-term.
    #[test]
    fn pure_eval_runs() {
        let v = elisprs::eval_str("(+ 1 2 3)").expect("eval");
        assert_eq!(elisprs::print(&v, true), "6");
    }

    /// Editor API helpers fail cleanly when invoked with no active context
    /// (i.e. outside an eval guard), rather than dereferencing a null pointer.
    #[test]
    fn api_without_context_errors() {
        assert!(super::api_message("hi").is_err());
        assert!(super::api_error("boom").is_err());
    }

    /// The editor subrs and the editor-lisp command layer install onto the host.
    /// Definitions need no active editor context (they don't call the subrs), so
    /// this exercises the install path without a full `compositor::Context`.
    #[test]
    fn editor_layer_installs() {
        super::elisp::ensure_builtins();
        super::elisp::ensure_editor_lisp();
        let bound = |expr: &str| elisprs::print(&elisprs::eval_str(expr).unwrap(), true);
        // buffer-identity subrs and the command wrappers are all defined.
        assert_eq!(bound("(fboundp 'editor-command)"), "t");
        assert_eq!(bound("(fboundp 'buffer-name)"), "t");
        assert_eq!(bound("(fboundp 'message)"), "t");
        assert_eq!(bound("(fboundp 'find-file)"), "t");
        assert_eq!(bound("(fboundp 'switch-to-buffer)"), "t");
        assert_eq!(bound("zemacs--editor-lisp-loaded"), "t");
    }

    /// Mark & region work against the mirrored buffer + mark state, so they are
    /// exercisable without a live editor context. Region positions track the
    /// mirror's point and the separately-held mark.
    #[test]
    fn mark_and_region() {
        super::elisp::ensure_builtins();
        let e = |s: &str| elisprs::print(&elisprs::eval_str(s).unwrap(), true);
        // point 3, mark 8 → region [3,8), active.
        assert_eq!(
            e(
                "(progn (erase-buffer) (insert \"hello world\") (goto-char 3) \
               (set-mark 8) (list (region-beginning) (region-end) (region-active-p)))"
            ),
            "(3 8 t)"
        );
        // deactivate keeps the mark position but reports an inactive region.
        assert_eq!(e("(progn (deactivate-mark) (region-active-p))"), "nil");
        // whole-buffer marks point-min..point-max.
        assert_eq!(
            e("(progn (mark-whole-buffer) (list (region-beginning) (region-end)))"),
            "(1 12)"
        );
    }

    /// Kill ring & yank cut and paste on the mirror; the ring is the source of
    /// truth (register bridging silently no-ops without a live context).
    #[test]
    fn kill_ring_and_yank() {
        super::elisp::ensure_builtins();
        let e = |s: &str| elisprs::print(&elisprs::eval_str(s).unwrap(), true);
        // kill-region removes the text and pushes it; current-kill sees it.
        assert_eq!(
            e("(progn (erase-buffer) (insert \"hello world\") (kill-region 1 6) (buffer-string))"),
            "\" world\""
        );
        assert_eq!(e("(current-kill 0)"), "\"hello\"");
        // yank reinserts the most recent kill at point.
        assert_eq!(
            e("(progn (goto-char 1) (yank) (buffer-string))"),
            "\"hello world\""
        );
    }

    /// The embedded vimlrs interpreter links, evaluates, and captures `:echo`.
    #[cfg(unix)]
    #[test]
    fn viml_eval_and_echo() {
        assert_eq!(super::viml::eval("3 + 4").unwrap(), "7");
        assert_eq!(super::viml::eval("echo 'hi'").unwrap(), "hi");
    }

    /// VimL globals persist across separate eval calls (thread-local state).
    #[cfg(unix)]
    #[test]
    fn viml_state_persists() {
        super::viml::eval("let g:zz = 41").unwrap();
        assert_eq!(super::viml::eval("g:zz + 1").unwrap(), "42");
    }

    /// A vimrc's `:highlight` commands are translated into a live zemacs theme:
    /// `Normal` paints the text/background surface, syntax groups map to
    /// tree-sitter scopes, and Vim display attributes become modifiers.
    #[cfg(unix)]
    #[test]
    fn viml_highlights_build_theme() {
        use zemacs_view::theme::{Color, Modifier};
        super::viml::eval("highlight Normal guifg=#abcdef guibg=#111111").unwrap();
        super::viml::eval("hi Comment guifg=#00ff00 gui=italic").unwrap();
        super::viml::eval("hi String ctermfg=203").unwrap();

        let theme = super::viml_theme::build_theme("acme").expect("theme built");
        assert_eq!(theme.name(), "vim:acme");
        // Normal → ui.text (fg) + ui.background (bg).
        assert_eq!(theme.get("ui.text").fg, Some(Color::Rgb(0xab, 0xcd, 0xef)));
        assert_eq!(
            theme.get("ui.background").bg,
            Some(Color::Rgb(0x11, 0x11, 0x11))
        );
        // Syntax groups → tree-sitter scopes, with attributes → modifiers.
        let comment = theme.get("comment");
        assert_eq!(comment.fg, Some(Color::Rgb(0x00, 0xff, 0x00)));
        assert!(comment.add_modifier.contains(Modifier::ITALIC));
        // A cterm-only colour becomes an indexed colour.
        assert_eq!(theme.get("string").fg, Some(Color::Indexed(203)));
    }

    /// `:colorscheme {name}` sources `colors/{name}.vim` from a registered dir,
    /// running its `:highlight` commands, so the whole scheme file feeds the
    /// zemacs theme — the real `.vimrc` path, end to end.
    #[cfg(unix)]
    #[test]
    fn viml_colorscheme_file_builds_theme() {
        use std::io::Write;
        use zemacs_view::theme::Color;

        let dir = std::env::temp_dir().join(format!("zemacs-colo-{}", std::process::id()));
        let colors = dir.join("colors");
        std::fs::create_dir_all(&colors).unwrap();
        let mut f = std::fs::File::create(colors.join("zztest.vim")).unwrap();
        writeln!(f, "highlight Normal guifg=#fedcba guibg=#020202").unwrap();
        writeln!(f, "hi Keyword guifg=#ff8800").unwrap();
        drop(f);

        vimlrs::fusevm_bridge::add_colorscheme_dir(dir.clone());
        super::viml::eval("colorscheme zztest").unwrap();
        assert_eq!(super::viml::eval("g:colors_name").unwrap(), "zztest");

        let theme = super::viml_theme::build_theme("zztest").expect("theme built");
        assert_eq!(theme.get("ui.text").fg, Some(Color::Rgb(0xfe, 0xdc, 0xba)));
        assert_eq!(theme.get("keyword").fg, Some(Color::Rgb(0xff, 0x88, 0x00)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The embedded awkrs interpreter filters string input → string output.
    #[cfg(unix)]
    #[test]
    fn awk_filter_runs() {
        assert_eq!(
            super::awk::run("{print $1}", "a b\nc d\n").unwrap(),
            "a\nc\n"
        );
        assert_eq!(super::awk::run("BEGIN{print 1+2}", "").unwrap(), "3\n");
    }

    /// The embedded zshrs shell runs a command and its output is captured (not
    /// leaked to the terminal); shell state persists across calls.
    #[cfg(unix)]
    #[test]
    fn zsh_runs_and_persists() {
        let (status, out) = super::zsh::run("echo hello").unwrap();
        assert_eq!(status, 0);
        assert!(out.contains("hello"), "captured output: {out:?}");
        super::zsh::run("ZV=42").unwrap();
        assert!(super::zsh::run("echo $ZV").unwrap().1.contains("42"));
    }

    /// The embedded strykelang interpreter evaluates expressions (value-based
    /// display) and persists state across calls.
    #[cfg(unix)]
    #[test]
    fn stryke_eval_and_persist() {
        assert_eq!(super::stryke::eval("2 + 3 * 4").unwrap(), "14");
        super::stryke::eval("$pv = 41").unwrap();
        assert_eq!(super::stryke::eval("$pv + 1").unwrap(), "42");
    }
}
