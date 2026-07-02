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

use std::cell::Cell;
use std::ptr;

use zemacs_core::{Selection, Tendril, Transaction};

use crate::compositor;
use crate::ui::prompt::PromptEvent;

pub mod awk;
mod capture;
pub mod elisp;
pub mod stryke;
pub mod viml;
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

/// Insert text at each cursor (primary + secondaries), as one undo step.
pub(super) fn api_insert(text: &str) -> Result<(), String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let sel = doc.selection(view.id).clone();
        let tendril: Tendril = text.into();
        let tx = Transaction::change_by_selection(doc.text(), &sel, |range| {
            (range.from(), range.from(), Some(tendril.clone()))
        });
        doc.apply(&tx, view.id);
    })
}

/// Whole-buffer text.
pub(super) fn api_buffer_string() -> Result<String, String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let _ = view;
        doc.text().to_string()
    })
}

/// Emacs-style point (1-based) of the primary cursor.
pub(super) fn api_point() -> Result<i64, String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let cursor = doc
            .selection(view.id)
            .primary()
            .cursor(doc.text().slice(..));
        cursor as i64 + 1
    })
}

/// Smallest valid point (always 1).
pub(super) fn api_point_min() -> Result<i64, String> {
    Ok(1)
}

/// One past the last character (Emacs `point-max`).
pub(super) fn api_point_max() -> Result<i64, String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let _ = view;
        doc.text().len_chars() as i64 + 1
    })
}

/// Move the primary cursor to a 1-based position.
pub(super) fn api_goto_char(pos: i64) -> Result<(), String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let max = doc.text().len_chars();
        let idx = (pos.max(1) as usize - 1).min(max);
        doc.set_selection(view.id, Selection::point(idx));
    })
}

/// Text between two 1-based positions `[start, end)`.
pub(super) fn api_buffer_substring(start: i64, end: i64) -> Result<String, String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let _ = view;
        let max = doc.text().len_chars();
        let a = (start.max(1) as usize - 1).min(max);
        let b = (end.max(1) as usize - 1).min(max);
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        doc.text().slice(a..b).to_string()
    })
}

/// Delete the region between two 1-based positions `[start, end)`.
pub(super) fn api_delete_region(start: i64, end: i64) -> Result<(), String> {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let max = doc.text().len_chars();
        let a = (start.max(1) as usize - 1).min(max);
        let b = (end.max(1) as usize - 1).min(max);
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let tx = Transaction::change(doc.text(), std::iter::once((a, b, None)));
        doc.apply(&tx, view.id);
    })
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

// ── Public entry points ────────────────────────────────────────────────────

/// Evaluate an elisp source string against the live editor. Returns the printed
/// result on success. Runs synchronously on the editor thread.
pub fn eval_elisp(cx: &mut compositor::Context, src: &str) -> Result<String, String> {
    let _guard = CxGuard::new(cx);
    elisp::ensure_builtins();
    let value = elisprs::eval_str(src)?;
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
}

pub fn eval_viml(cx: &mut compositor::Context, src: &str) -> Result<String, String> {
    // Publish the context so host hooks (e.g. `:set`) can reach the live editor.
    let _guard = CxGuard::new(cx);
    install_viml_host_hooks();
    viml::eval(src)
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

    let init_el = dir.join("init.el");
    if init_el.exists() {
        let _guard = CxGuard::new(cx);
        elisp::ensure_builtins();
        if let Err(e) = elisprs::eval_file(&init_el.to_string_lossy()) {
            cx.editor.set_error(format!("init.el: {e}"));
        }
    }

    #[cfg(unix)]
    {
        let init_vim = dir.join("init.vim");
        if init_vim.exists() {
            let _guard = CxGuard::new(cx);
            install_viml_host_hooks();
            if let Err(e) = vimlrs::fusevm_bridge::eval_file(&init_vim) {
                cx.editor.set_error(format!("init.vim: {}", e.0));
            }
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
        assert!(super::api_point().is_err());
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
