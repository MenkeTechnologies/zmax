//! Native (Rust) plugin host — editor extension.
//!
//! zmax `dlopen`s third-party `cdylib`s that register **typable commands** over
//! a stable, versioned C ABI (the `zmax-native` crate). A plugin ships a
//! compiled `.dylib`/`.so` and is loaded at runtime with `:plugin load <path>` —
//! no zmax recompile, no script glue. This is the port of zshrs's native plugin
//! host (`zmodload -R`) to the editor.
//!
//! ## Where plugin commands resolve
//!
//! A freshly-loaded plugin command is unknown to the static
//! [`TYPABLE_COMMAND_MAP`](crate::commands::typed::TYPABLE_COMMAND_MAP), so it
//! arrives at [`execute_command_line_inner`](crate::commands::typed)'s
//! fallthrough, which consults [`dispatch`](crate::commands::plugin::dispatch)
//! AFTER built-in typables and BEFORE
//! the user-command / vimscript fallback — the same slot zsh's plugin host
//! occupies (after real builtins, before PATH).
//!
//! ## The editor bridge
//!
//! Host callbacks are bare `extern "C" fn`s that cannot capture `&mut Editor`.
//! The active command [`compositor::Context`] is published through a
//! thread-local raw pointer for the duration of a single, synchronous,
//! on-editor-thread call (installed by `CxGuard`, cleared on drop) — the same
//! pattern the embedded interpreters use, kept independent here so the native
//! plugin ABI works without the `scripting` feature. Every callback that touches
//! the editor goes through `with_cx`; called outside a guarded window it is
//! inert.
//!
//! ## ABI safety
//!
//! Everything crossing the boundary is `#[repr(C)]`. The host verifies the
//! plugin's `abi_version` matches [`zmax_native::ABI_VERSION`] before trusting
//! any pointer it returns; a mismatch is refused (a wrong struct layout would be
//! undefined behaviour). The loaded [`libloading::Library`] is kept alive for
//! the process lifetime — its `Drop` is a `dlclose`, which would invalidate the
//! still-registered function pointers, so unload explicitly purges the registry
//! first.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::{Mutex, OnceLock};

use zmax_core::{Tendril, Transaction};
use zmax_native::{CommandFn, Cursor, HostApi, InitFn, PluginInfo, Span, ABI_VERSION, INIT_SYMBOL};

use crate::compositor;

// ============================================================
// Editor bridge — publishes the active `compositor::Context` for the duration
// of a plugin call so the C-ABI host callbacks can reach the editor.
// ============================================================

thread_local! {
    /// Type-erased pointer to the `compositor::Context` of the in-flight call.
    static CX_PTR: Cell<*mut ()> = const { Cell::new(ptr::null_mut()) };
}

/// RAII guard publishing the current command context. Restores the previous
/// pointer on drop so nested calls (a plugin `eval` that dispatches another
/// plugin command) stay sound.
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

/// Run `f` with the active editor context, or `None` if called with no context
/// installed (e.g. from a background thread the plugin spawned).
fn with_cx<R>(f: impl FnOnce(&mut compositor::Context) -> R) -> Option<R> {
    CX_PTR.with(|c| {
        let p = c.get() as *mut compositor::Context;
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` was installed by a `CxGuard` whose scope encloses this
        // call; plugin calls are synchronous on this thread and the pointer is
        // cleared on guard drop. The single-threaded call never aliases it.
        Some(f(unsafe { &mut *p }))
    })
}

// ============================================================
// Registries.
// ============================================================

/// One loaded plugin. Dropping `_lib` runs `dlclose`, so this is only ever
/// removed by [`unload`] AFTER its commands are purged from [`registry`].
struct LoadedPlugin {
    name: String,
    version: String,
    path: String,
    /// Kept alive for the process lifetime; drop = `dlclose`.
    _lib: libloading::Library,
}

fn plugins() -> &'static Mutex<Vec<LoadedPlugin>> {
    static P: OnceLock<Mutex<Vec<LoadedPlugin>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(Vec::new()))
}

/// command-name → handler. Consulted by [`dispatch`].
fn registry() -> &'static Mutex<HashMap<String, CommandFn>> {
    static R: OnceLock<Mutex<HashMap<String, CommandFn>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Staging area for commands registered during a single `init` call. `init`
/// runs before it returns the plugin name, so registrations are buffered here
/// and tagged with the owning plugin afterwards. Serialised by [`load_lock`].
fn staging() -> &'static Mutex<Vec<(String, CommandFn)>> {
    static S: OnceLock<Mutex<Vec<(String, CommandFn)>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

/// Serialises `load`/`unload` so the [`staging`] buffer is single-writer.
fn load_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

/// Which plugin owns each registered command name — parallel to [`registry`],
/// used only for `unload` bookkeeping.
fn ownership() -> &'static Mutex<HashMap<String, String>> {
    static O: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    O.get_or_init(|| Mutex::new(HashMap::new()))
}

// ============================================================
// Host API callbacks — the `extern "C"` functions plugins call back through.
// One shared, leaked `HostApi` table for the whole process.
// ============================================================

extern "C" fn host_register_command(
    _host: *const HostApi,
    name: *const c_char,
    handler: CommandFn,
) -> c_int {
    if name.is_null() {
        return 1;
    }
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    staging().lock().unwrap().push((name, handler));
    0
}

extern "C" fn host_message(_host: *const HostApi, text: *const c_char) {
    if text.is_null() {
        return;
    }
    let s = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    with_cx(|cx| cx.editor.set_status(s));
}

extern "C" fn host_error(_host: *const HostApi, text: *const c_char) {
    if text.is_null() {
        return;
    }
    let s = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    with_cx(|cx| cx.editor.set_error(s));
}

extern "C" fn host_eval(_host: *const HostApi, line: *const c_char) -> c_int {
    if line.is_null() {
        return 1;
    }
    let line = unsafe { CStr::from_ptr(line) }
        .to_string_lossy()
        .into_owned();
    // A plugin command runs inside a `CxGuard`, so a context is in scope.
    // Re-entrant `with_cx` is safe: the borrow is released before this returns.
    match with_cx(|cx| crate::commands::typed::eval_command_line(cx, &line)) {
        Some(true) => 0,
        _ => 1,
    }
}

extern "C" fn host_buffer_text(_host: *const HostApi) -> *mut c_char {
    let text = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.text().to_string()
    });
    match text.and_then(|s| CString::new(s).ok()) {
        Some(c) => c.into_raw(),
        None => ptr::null_mut(),
    }
}

extern "C" fn host_insert_text(_host: *const HostApi, text: *const c_char) -> c_int {
    if text.is_null() {
        return 1;
    }
    let s = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    let ok = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let sel = doc.selection(view.id).clone();
        let tendril: Tendril = s.into();
        let tx = Transaction::change_by_selection(doc.text(), &sel, |range| {
            (range.head, range.head, Some(tendril.clone()))
        });
        doc.apply(&tx, view.id);
    });
    if ok.is_some() {
        0
    } else {
        1
    }
}

/// Hand a `String` back to the plugin as an owned C string, or null when there
/// was nothing to give. Released by `host_free_cstring`.
fn into_raw_cstring(text: Option<String>) -> *mut c_char {
    match text.and_then(|s| CString::new(s).ok()) {
        Some(c) => c.into_raw(),
        None => ptr::null_mut(),
    }
}

extern "C" fn host_cursor(_host: *const HostApi) -> Cursor {
    let found = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let offset = doc.selection(view.id).primary().cursor(text);
        let line = text.char_to_line(offset);
        Cursor {
            line,
            column: offset - text.line_to_char(line),
            offset,
            valid: 1,
        }
    });
    found.unwrap_or(Cursor {
        line: 0,
        column: 0,
        offset: 0,
        valid: 0,
    })
}

extern "C" fn host_word_at_cursor(_host: *const HostApi) -> *mut c_char {
    let word = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let range = doc.selection(view.id).primary();
        // The same object `miw` selects, so a plugin agrees with the editor
        // about where a word starts and ends.
        let word = zmax_core::textobject::textobject_word(
            text,
            range,
            zmax_core::textobject::TextObject::Inside,
            1,
            false,
        );
        let selected = text.slice(word.from()..word.to()).to_string();
        // On whitespace the object collapses to nothing useful; report no word
        // rather than a blank one.
        (!selected.trim().is_empty()).then_some(selected)
    });
    into_raw_cstring(word.flatten())
}

extern "C" fn host_selection_text(_host: *const HostApi) -> *mut c_char {
    let selected = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        doc.selection(view.id).primary().fragment(text).to_string()
    });
    into_raw_cstring(selected)
}

/* ---- vim-style getters ---- */

extern "C" fn host_line(_host: *const HostApi, line: usize) -> *mut c_char {
    let text = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text();
        // `len_lines` counts a trailing empty line, so a request for it is past
        // the end as far as a caller asking for content is concerned.
        (line < text.len_lines()).then(|| {
            // Without its line ending: `getline()` never includes one.
            let slice = text.line(line);
            let mut s = slice.to_string();
            while s.ends_with('\n') || s.ends_with('\r') {
                s.pop();
            }
            s
        })
    });
    into_raw_cstring(text.flatten())
}

extern "C" fn host_line_count(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.text().len_lines()
    })
    .unwrap_or(0)
}

extern "C" fn host_mode(_host: *const HostApi) -> *mut c_char {
    into_raw_cstring(with_cx(|cx| cx.editor.mode().to_string()))
}

extern "C" fn host_cwd(_host: *const HostApi) -> *mut c_char {
    into_raw_cstring(Some(
        zmax_stdx::env::current_working_dir()
            .to_string_lossy()
            .into_owned(),
    ))
}

extern "C" fn host_buffer_path(_host: *const HostApi) -> *mut c_char {
    let path = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.path().map(|p| p.to_string_lossy().into_owned())
    });
    into_raw_cstring(path.flatten())
}

extern "C" fn host_language(_host: *const HostApi) -> *mut c_char {
    let language = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.language_name().map(str::to_string)
    });
    into_raw_cstring(language.flatten())
}

extern "C" fn host_is_modified(_host: *const HostApi) -> c_int {
    let modified = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.is_modified()
    });
    c_int::from(modified.unwrap_or(false))
}

extern "C" fn host_register(_host: *const HostApi, name: c_char) -> *mut c_char {
    let Some(name) = char::from_u32(name as u8 as u32) else {
        return ptr::null_mut();
    };
    let values = with_cx(|cx| {
        // Joined with newlines, the way vim renders a list register.
        cx.editor
            .registers
            .read(name, cx.editor)
            .map(|values| values.map(|v| v.to_string()).collect::<Vec<_>>().join("\n"))
    });
    into_raw_cstring(values.flatten().filter(|s| !s.is_empty()))
}

extern "C" fn host_selection_count(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        doc.selection(view.id).len()
    })
    .unwrap_or(0)
}

/// The span nothing valid maps to.
const NO_SPAN: Span = Span {
    anchor: 0,
    head: 0,
    line: 0,
    valid: 0,
};

extern "C" fn host_selection(_host: *const HostApi, index: usize) -> Span {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let ranges = doc.selection(view.id);
        match ranges.ranges().get(index) {
            Some(range) => Span {
                anchor: range.anchor,
                head: range.head,
                line: text.char_to_line(range.cursor(text)),
                valid: 1,
            },
            None => NO_SPAN,
        }
    })
    .unwrap_or(NO_SPAN)
}

extern "C" fn host_text_range(_host: *const HostApi, from: usize, to: usize) -> *mut c_char {
    let text = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let slice = doc.text().slice(..);
        // Clamped, and ordered, so a caller passing a backwards selection's
        // head/anchor still gets the text between them.
        let len = slice.len_chars();
        let (from, to) = (from.min(to).min(len), to.max(from).min(len));
        slice.slice(from..to).to_string()
    });
    into_raw_cstring(text)
}

extern "C" fn host_buffer_count(_host: *const HostApi) -> usize {
    with_cx(|cx| cx.editor.documents().count()).unwrap_or(0)
}

extern "C" fn host_buffer_name(_host: *const HostApi, index: usize) -> *mut c_char {
    let name = with_cx(|cx| {
        cx.editor
            .documents()
            .nth(index)
            .map(|doc| doc.display_name().into_owned())
    });
    into_raw_cstring(name.flatten())
}

extern "C" fn host_diagnostic_count(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.diagnostics().len()
    })
    .unwrap_or(0)
}

extern "C" fn host_diagnostic(_host: *const HostApi, index: usize) -> Span {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        match doc.diagnostics().get(index) {
            Some(diagnostic) => Span {
                anchor: diagnostic.range.start,
                head: diagnostic.range.end,
                line: diagnostic.line,
                valid: 1,
            },
            None => NO_SPAN,
        }
    })
    .unwrap_or(NO_SPAN)
}

extern "C" fn host_diagnostic_message(_host: *const HostApi, index: usize) -> *mut c_char {
    let message = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.diagnostics().get(index).map(|d| d.message.clone())
    });
    into_raw_cstring(message.flatten())
}

extern "C" fn host_diagnostic_severity(_host: *const HostApi, index: usize) -> *mut c_char {
    let severity = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.diagnostics().get(index).map(|d| {
            // `Diagnostic::severity` applies the editor's own default for a
            // server that left it unset, so a plugin sees what the gutter does.
            match d.severity() {
                zmax_core::diagnostic::Severity::Hint => "hint",
                zmax_core::diagnostic::Severity::Info => "info",
                zmax_core::diagnostic::Severity::Warning => "warning",
                zmax_core::diagnostic::Severity::Error => "error",
            }
            .to_string()
        })
    });
    into_raw_cstring(severity.flatten())
}

/// Read a `*const c_char` argument as a `String`, or `None` when it is null.
fn arg_string(raw: *const c_char) -> Option<String> {
    (!raw.is_null()).then(|| {
        unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned()
    })
}

extern "C" fn host_option(_host: *const HostApi, name: *const c_char) -> *mut c_char {
    let Some(name) = arg_string(name) else {
        return ptr::null_mut();
    };
    // `vim_opts::get` takes the spellings a name can have; passing the one the
    // caller used covers both the long and short form it knows.
    into_raw_cstring(zmax_core::vim_opts::get(&[name.as_str()]))
}

extern "C" fn host_search_pattern(_host: *const HostApi) -> *mut c_char {
    let pattern = with_cx(|cx| {
        cx.editor
            .registers
            .read('/', cx.editor)
            .and_then(|mut values| values.next().map(|v| v.to_string()))
    });
    into_raw_cstring(pattern.flatten().filter(|s| !s.is_empty()))
}

extern "C" fn host_window_count(_host: *const HostApi) -> usize {
    with_cx(|cx| cx.editor.tree.views().count()).unwrap_or(0)
}

extern "C" fn host_window_view(_host: *const HostApi) -> Span {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let first = doc.view_offset(view.id).anchor;
        let first_line = text.char_to_line(first.min(text.len_chars()));
        // The last line the viewport can show, clamped to the document: a short
        // buffer does not fill the window.
        let last_line = (first_line + view.inner_height()).min(text.len_lines().saturating_sub(1));
        Span {
            anchor: first,
            head: text.line_to_char(last_line),
            line: first_line,
            valid: 1,
        }
    })
    .unwrap_or(NO_SPAN)
}

extern "C" fn host_file_size(_host: *const HostApi, path: *const c_char) -> i64 {
    arg_string(path)
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|meta| i64::try_from(meta.len()).ok())
        .unwrap_or(-1)
}

extern "C" fn host_file_type(_host: *const HostApi, path: *const c_char) -> *mut c_char {
    let kind = arg_string(path).and_then(|p| {
        // `symlink_metadata` so a link reports as one rather than as its
        // target, which is what vim's `getftype` does.
        let meta = std::fs::symlink_metadata(&p).ok()?;
        Some(
            if meta.file_type().is_symlink() {
                "link"
            } else if meta.is_dir() {
                "dir"
            } else {
                "file"
            }
            .to_string(),
        )
    });
    into_raw_cstring(kind)
}

extern "C" fn host_file_time(_host: *const HostApi, path: *const c_char) -> i64 {
    arg_string(path)
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_secs()).ok())
        .unwrap_or(-1)
}

extern "C" fn host_file_perm(_host: *const HostApi, path: *const c_char) -> *mut c_char {
    let perm = arg_string(path).and_then(|p| {
        let meta = std::fs::metadata(p).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode();
            // `rwxrwxrwx`, high bit first, exactly as getfperm renders it.
            let bits = [
                (0o400, 'r'),
                (0o200, 'w'),
                (0o100, 'x'),
                (0o040, 'r'),
                (0o020, 'w'),
                (0o010, 'x'),
                (0o004, 'r'),
                (0o002, 'w'),
                (0o001, 'x'),
            ];
            Some(
                bits.iter()
                    .map(|(bit, ch)| if mode & bit != 0 { *ch } else { '-' })
                    .collect::<String>(),
            )
        }
        #[cfg(not(unix))]
        {
            // Windows has no mode bits; report what it does know.
            Some(if meta.permissions().readonly() {
                "r--r--r--".to_string()
            } else {
                "rw-rw-rw-".to_string()
            })
        }
    });
    into_raw_cstring(perm)
}

extern "C" fn host_buffer_line(_host: *const HostApi, buffer: usize, line: usize) -> *mut c_char {
    let text = with_cx(|cx| {
        let doc = cx.editor.documents().nth(buffer)?;
        let rope = doc.text();
        if line >= rope.len_lines() {
            return None;
        }
        let mut s = rope.line(line).to_string();
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        Some(s)
    });
    into_raw_cstring(text.flatten())
}

extern "C" fn host_byte_offset(_host: *const HostApi, char_offset: usize) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text();
        text.char_to_byte(char_offset.min(text.len_chars()))
    })
    .unwrap_or(0)
}

extern "C" fn host_char_offset(_host: *const HostApi, byte_offset: usize) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text();
        // `byte_to_char` rounds down to the char containing the byte, so a
        // position landing mid-codepoint resolves to that character.
        text.byte_to_char(byte_offset.min(text.len_bytes()))
    })
    .unwrap_or(0)
}

extern "C" fn host_command_exists(_host: *const HostApi, name: *const c_char) -> c_int {
    let Some(name) = arg_string(name) else {
        return 0;
    };
    let name = name.trim_start_matches(':');
    let builtin = crate::commands::typed::TYPABLE_COMMAND_MAP.contains_key(name);
    // Plugin-registered commands resolve too, so `exists` agrees with what
    // typing the name would actually do.
    let from_plugin = registry()
        .lock()
        .map(|reg| reg.contains_key(name))
        .unwrap_or(false);
    c_int::from(builtin || from_plugin)
}

extern "C" fn host_plugin_count(_host: *const HostApi) -> usize {
    plugins().lock().map(|p| p.len()).unwrap_or(0)
}

extern "C" fn host_plugin_name(_host: *const HostApi, index: usize) -> *mut c_char {
    let name = plugins()
        .lock()
        .ok()
        .and_then(|p| p.get(index).map(|p| format!("{} {}", p.name, p.version)));
    into_raw_cstring(name)
}

extern "C" fn host_mark(_host: *const HostApi, name: c_char) -> Span {
    let Some(name) = char::from_u32(name as u8 as u32) else {
        return NO_SPAN;
    };
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        match doc.mark(name) {
            Some(pos) => {
                let text = doc.text().slice(..);
                let pos = pos.min(text.len_chars());
                Span {
                    // A mark is a point, so both ends are the same offset.
                    anchor: pos,
                    head: pos,
                    line: text.char_to_line(pos),
                    valid: 1,
                }
            }
            None => NO_SPAN,
        }
    })
    .unwrap_or(NO_SPAN)
}

extern "C" fn host_window_width(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        // The text area, so a plugin laying something out is not told about
        // columns the gutters have already taken.
        view.inner_area(doc).width as usize
    })
    .unwrap_or(0)
}

extern "C" fn host_window_height(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        view.inner_area(doc).height as usize
    })
    .unwrap_or(0)
}

extern "C" fn host_completions(_host: *const HostApi, prefix: *const c_char) -> *mut c_char {
    let Some(prefix) = arg_string(prefix) else {
        return ptr::null_mut();
    };
    let prefix = prefix.trim_start_matches(':');

    let mut names: Vec<String> = crate::commands::typed::TYPABLE_COMMAND_LIST
        .iter()
        .map(|cmd| cmd.name.to_string())
        .filter(|name| name.starts_with(prefix))
        .collect();
    // Plugin commands complete too, so the list matches what the `:` prompt
    // would actually accept.
    if let Ok(reg) = registry().lock() {
        names.extend(
            reg.keys()
                .filter(|name| name.starts_with(prefix))
                .map(|name| name.to_string()),
        );
    }
    names.sort_unstable();
    names.dedup();

    into_raw_cstring((!names.is_empty()).then(|| names.join("\n")))
}

extern "C" fn host_marks(_host: *const HostApi) -> *mut c_char {
    let rows = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let mut marks: Vec<(char, usize)> = doc.marks_iter().collect();
        // Sorted by name so the list is stable between calls; a HashMap's order
        // is not.
        marks.sort_unstable();
        marks
            .into_iter()
            .map(|(name, pos)| {
                let pos = pos.min(text.len_chars());
                format!("{name}:{pos}:{}", text.char_to_line(pos))
            })
            .collect::<Vec<_>>()
    });
    into_raw_cstring(
        rows.filter(|rows| !rows.is_empty())
            .map(|rows| rows.join("\n")),
    )
}

extern "C" fn host_changelist_count(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.changelist().0.len()
    })
    .unwrap_or(0)
}

extern "C" fn host_changelist(_host: *const HostApi, index: usize) -> Span {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        match doc.changelist().0.get(index) {
            Some(&pos) => {
                let pos = pos.min(text.len_chars());
                Span {
                    // A change is recorded as a point, like a mark.
                    anchor: pos,
                    head: pos,
                    line: text.char_to_line(pos),
                    valid: 1,
                }
            }
            None => NO_SPAN,
        }
    })
    .unwrap_or(NO_SPAN)
}

extern "C" fn host_changelist_index(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.changelist().1
    })
    .unwrap_or(0)
}

extern "C" fn host_display_width(_host: *const HostApi, text: *const c_char) -> usize {
    let Some(text) = arg_string(text) else {
        return 0;
    };
    // Per grapheme cluster, the way the renderer measures it -- summing char
    // widths would double-count a combining mark's base.
    use zmax_core::unicode::segmentation::UnicodeSegmentation;
    text.graphemes(true)
        .map(zmax_core::graphemes::grapheme_width)
        .sum()
}

extern "C" fn host_buffer_path_at(_host: *const HostApi, index: usize) -> *mut c_char {
    let path = with_cx(|cx| {
        cx.editor
            .documents()
            .nth(index)
            .and_then(|doc| doc.path().map(|p| p.to_string_lossy().into_owned()))
    });
    into_raw_cstring(path.flatten())
}

extern "C" fn host_buffer_modified(_host: *const HostApi, index: usize) -> c_int {
    let modified = with_cx(|cx| {
        cx.editor
            .documents()
            .nth(index)
            .map(|doc| doc.is_modified())
            .unwrap_or(false)
    });
    c_int::from(modified.unwrap_or(false))
}

extern "C" fn host_window_index(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        cx.editor
            .tree
            .views()
            .position(|(_, focused)| focused)
            .unwrap_or(0)
    })
    .unwrap_or(0)
}

extern "C" fn host_line_length(_host: *const HostApi, line: usize) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text();
        if line >= text.len_lines() {
            // Distinguishable from an empty line, which is length 0.
            return usize::MAX;
        }
        let slice = text.line(line);
        // Without the line ending, as `col("$")` counts it.
        let mut len = slice.len_chars();
        let mut chars = slice.chars_at(len);
        while len > 0 {
            match chars.prev() {
                Some('\n') | Some('\r') => len -= 1,
                _ => break,
            }
        }
        len
    })
    .unwrap_or(usize::MAX)
}

extern "C" fn host_indent(_host: *const HostApi, line: usize) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text();
        if line >= text.len_lines() {
            return 0;
        }
        // Columns, not characters: a tab is worth `tabstop` of them.
        zmax_core::indent::indent_level_for_line(
            text.line(line),
            doc.tab_width(),
            doc.indent_width(),
        ) * doc.indent_width()
    })
    .unwrap_or(0)
}

extern "C" fn host_word_count(_host: *const HostApi) -> *mut c_char {
    let counts = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text();
        let chars = text.len_chars();
        let words = text.slice(..).to_string().split_whitespace().count();
        format!("{chars}:{words}:{}", text.len_lines())
    });
    into_raw_cstring(counts)
}

extern "C" fn host_option_num(_host: *const HostApi, name: *const c_char) -> usize {
    arg_string(name)
        .and_then(|name| zmax_core::vim_opts::get_num(&[name.as_str()]))
        .unwrap_or(usize::MAX)
}

extern "C" fn host_option_bool(_host: *const HostApi, name: *const c_char) -> c_int {
    let set = arg_string(name)
        .map(|name| zmax_core::vim_opts::get_bool(&[name.as_str()]))
        .unwrap_or(false);
    c_int::from(set)
}

extern "C" fn host_fname_modify(
    _host: *const HostApi,
    path: *const c_char,
    mods: *const c_char,
) -> *mut c_char {
    let (Some(path), Some(mods)) = (arg_string(path), arg_string(mods)) else {
        return ptr::null_mut();
    };

    let mut current = path;
    // Modifiers apply left to right, so `:p:h` is the directory of the absolute
    // path rather than the absolute form of the directory.
    for modifier in mods.split(':').filter(|m| !m.is_empty()) {
        let path = std::path::Path::new(&current);
        current = match modifier {
            "p" => zmax_stdx::path::canonicalize(path)
                .to_string_lossy()
                .into_owned(),
            "h" => path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                // vim gives "." for a bare name, not an empty string.
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| ".".to_string()),
            "t" => path
                .file_name()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "r" => match (path.parent(), path.file_stem()) {
                (Some(parent), Some(stem)) if !parent.as_os_str().is_empty() => {
                    parent.join(stem).to_string_lossy().into_owned()
                }
                (_, Some(stem)) => stem.to_string_lossy().into_owned(),
                _ => current.clone(),
            },
            "e" => path
                .extension()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            // An unknown modifier is an error rather than a silent no-op: the
            // caller asked for something this does not do.
            _ => return ptr::null_mut(),
        };
    }
    into_raw_cstring(Some(current))
}

extern "C" fn host_is_directory(_host: *const HostApi, path: *const c_char) -> c_int {
    let is_dir = arg_string(path)
        .map(|p| std::path::Path::new(&p).is_dir())
        .unwrap_or(false);
    c_int::from(is_dir)
}

extern "C" fn host_file_readable(_host: *const HostApi, path: *const c_char) -> c_int {
    // A regular file that opens: vim's `filereadable` is 0 for a directory even
    // though a directory can be opened.
    let readable = arg_string(path)
        .map(|p| {
            let path = std::path::Path::new(&p);
            path.is_file() && std::fs::File::open(path).is_ok()
        })
        .unwrap_or(false);
    c_int::from(readable)
}

extern "C" fn host_file_writable(_host: *const HostApi, path: *const c_char) -> c_int {
    let Some(path) = arg_string(path) else {
        return 0;
    };
    let path = std::path::Path::new(&path);
    let Ok(meta) = std::fs::metadata(path) else {
        return 0;
    };
    if meta.permissions().readonly() {
        return 0;
    }
    // vim answers 2 for a writable directory and 1 for a writable file.
    if meta.is_dir() {
        2
    } else {
        1
    }
}

extern "C" fn host_line_to_byte(_host: *const HostApi, line: usize) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text();
        if line >= text.len_lines() {
            return usize::MAX;
        }
        text.char_to_byte(text.line_to_char(line))
    })
    .unwrap_or(usize::MAX)
}

extern "C" fn host_byte_to_line(_host: *const HostApi, byte: usize) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text();
        text.byte_to_line(byte.min(text.len_bytes()))
    })
    .unwrap_or(0)
}

extern "C" fn host_env(_host: *const HostApi, name: *const c_char) -> *mut c_char {
    into_raw_cstring(arg_string(name).and_then(|name| std::env::var(name).ok()))
}

extern "C" fn host_buffer_index(_host: *const HostApi, name: *const c_char) -> usize {
    let Some(name) = arg_string(name) else {
        return usize::MAX;
    };
    with_cx(|cx| {
        cx.editor
            .documents()
            // Substring, like vim's `bufnr`, so a leaf name finds a path.
            .position(|doc| doc.display_name().contains(&name))
            .unwrap_or(usize::MAX)
    })
    .unwrap_or(usize::MAX)
}

extern "C" fn host_window_buffer(_host: *const HostApi, index: usize) -> usize {
    with_cx(|cx| {
        let Some((view, _)) = cx.editor.tree.views().nth(index) else {
            return usize::MAX;
        };
        let doc_id = view.doc;
        cx.editor
            .documents()
            .position(|doc| doc.id() == doc_id)
            .unwrap_or(usize::MAX)
    })
    .unwrap_or(usize::MAX)
}

extern "C" fn host_fold_level(_host: *const HostApi, line: usize) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        // Nested folds each add a level, so the count of folds covering the
        // line is its depth.
        doc.folds()
            .iter()
            .filter(|fold| line >= fold.start && line <= fold.end)
            .count()
    })
    .unwrap_or(0)
}

extern "C" fn host_fold_closed(_host: *const HostApi, line: usize) -> usize {
    with_cx(|cx| doc_folds_closed_start(cx, line)).unwrap_or(usize::MAX)
}

/// The first line of the innermost closed fold covering `line`.
fn doc_folds_closed_start(cx: &mut compositor::Context, line: usize) -> usize {
    let (_view, doc) = current!(cx.editor);
    doc.folds()
        .iter()
        .filter(|fold| fold.closed && line >= fold.start && line <= fold.end)
        // Innermost: the latest start still covering the line.
        .map(|fold| fold.start)
        .max()
        .unwrap_or(usize::MAX)
}

extern "C" fn host_search_count(_host: *const HostApi, pattern: *const c_char) -> usize {
    let Some(pattern) = arg_string(pattern) else {
        return 0;
    };
    // An invalid pattern counts zero: a plugin building one from user input
    // should not have to pre-validate it to avoid an error here.
    let Ok(regex) = regex::Regex::new(&pattern) else {
        return 0;
    };
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        regex.find_iter(&doc.text().slice(..).to_string()).count()
    })
    .unwrap_or(0)
}

extern "C" fn host_search_next(_host: *const HostApi, pattern: *const c_char, from: usize) -> Span {
    let Some(pattern) = arg_string(pattern) else {
        return NO_SPAN;
    };
    let Ok(regex) = regex::Regex::new(&pattern) else {
        return NO_SPAN;
    };
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text();
        let haystack = text.slice(..).to_string();
        // `from` is a char offset, but the regex works in bytes.
        let start = text.char_to_byte(from.min(text.len_chars()));
        match regex.find_at(&haystack, start) {
            Some(m) => {
                let anchor = text.byte_to_char(m.start());
                Span {
                    anchor,
                    head: text.byte_to_char(m.end()),
                    line: text.char_to_line(anchor),
                    valid: 1,
                }
            }
            None => NO_SPAN,
        }
    })
    .unwrap_or(NO_SPAN)
}

extern "C" fn host_pid(_host: *const HostApi) -> u32 {
    std::process::id()
}

extern "C" fn host_virtual_column(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let line = text.char_to_line(cursor);
        let start = text.line_to_char(line);
        // Screen cells, not characters: a tab draws to the next stop and a wide
        // glyph takes two, which is what has to line up with `window_width`.
        let tab_width = doc.tab_width() as u16;
        let mut column = 0usize;
        use zmax_stdx::rope::RopeSliceExt;
        for grapheme in text.slice(start..cursor).graphemes() {
            let grapheme = std::borrow::Cow::from(grapheme);
            column += if grapheme == "\t" {
                zmax_core::graphemes::tab_width_at(column, tab_width)
            } else {
                zmax_core::graphemes::grapheme_width(&grapheme)
            };
        }
        column
    })
    .unwrap_or(0)
}

extern "C" fn host_file_at_cursor(_host: *const HostApi) -> *mut c_char {
    let found = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let line = text.char_to_line(cursor);
        let line_text = text.line(line);
        let column = cursor - text.line_to_char(line);

        // `isfname` rules, the same ones `gf` uses, so a path keeps its dots
        // and slashes where a word object would stop at the first one.
        zmax_stdx::path::find_paths(line_text, true)
            .find(|range| {
                let (start, end) = (
                    line_text.byte_to_char(range.start),
                    line_text.byte_to_char(range.end),
                );
                column >= start && column <= end
            })
            .map(|range| line_text.byte_slice(range).to_string())
    });
    into_raw_cstring(found.flatten().filter(|found| !found.is_empty()))
}

extern "C" fn host_change_number(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        doc.get_current_revision()
    })
    .unwrap_or(0)
}

extern "C" fn host_buffer_window(_host: *const HostApi, buffer: usize) -> usize {
    with_cx(|cx| {
        let Some(doc_id) = cx.editor.documents().nth(buffer).map(|doc| doc.id()) else {
            return usize::MAX;
        };
        cx.editor
            .tree
            .views()
            .position(|(view, _)| view.doc == doc_id)
            .unwrap_or(usize::MAX)
    })
    .unwrap_or(usize::MAX)
}

extern "C" fn host_long_word_at_cursor(_host: *const HostApi) -> *mut c_char {
    let word = with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let range = doc.selection(view.id).primary();
        // `long` is vim's WORD: whitespace-delimited, so punctuation stays.
        let word = zmax_core::textobject::textobject_word(
            text,
            range,
            zmax_core::textobject::TextObject::Inside,
            1,
            true,
        );
        let selected = text.slice(word.from()..word.to()).to_string();
        (!selected.trim().is_empty()).then_some(selected)
    });
    into_raw_cstring(word.flatten())
}

extern "C" fn host_screen_position(_host: *const HostApi) -> Span {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let cursor = doc.selection(view.id).primary().cursor(text);
        let cursor_line = text.char_to_line(cursor);

        let first = doc.view_offset(view.id).anchor.min(text.len_chars());
        let first_line = text.char_to_line(first);
        // Above the viewport there is no screen row, so the position is not a
        // valid one rather than a negative row clamped to zero.
        if cursor_line < first_line {
            return NO_SPAN;
        }
        Span {
            anchor: host_virtual_column(_host),
            head: 0,
            line: cursor_line - first_line,
            valid: 1,
        }
    })
    .unwrap_or(NO_SPAN)
}

extern "C" fn host_window_width_at(_host: *const HostApi, index: usize) -> usize {
    with_cx(|cx| {
        let Some((view, _)) = cx.editor.tree.views().nth(index) else {
            return usize::MAX;
        };
        let doc_id = view.doc;
        let area = cx
            .editor
            .documents()
            .find(|doc| doc.id() == doc_id)
            .map(|doc| view.inner_area(doc));
        area.map(|area| area.width as usize).unwrap_or(usize::MAX)
    })
    .unwrap_or(usize::MAX)
}

extern "C" fn host_window_height_at(_host: *const HostApi, index: usize) -> usize {
    with_cx(|cx| {
        let Some((view, _)) = cx.editor.tree.views().nth(index) else {
            return usize::MAX;
        };
        let doc_id = view.doc;
        let area = cx
            .editor
            .documents()
            .find(|doc| doc.id() == doc_id)
            .map(|doc| view.inner_area(doc));
        area.map(|area| area.height as usize).unwrap_or(usize::MAX)
    })
    .unwrap_or(usize::MAX)
}

extern "C" fn host_file_completions(_host: *const HostApi, prefix: *const c_char) -> *mut c_char {
    let Some(prefix) = arg_string(prefix) else {
        return ptr::null_mut();
    };
    let expanded = zmax_stdx::path::expand_tilde(std::path::Path::new(&prefix));
    // Split into the directory to read and the fragment to match, so a prefix
    // ending in a separator lists that directory whole.
    let (dir, fragment) = if prefix.ends_with(std::path::MAIN_SEPARATOR) {
        (expanded.to_path_buf(), String::new())
    } else {
        (
            expanded
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from(".")),
            expanded
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    };

    let Ok(entries) = std::fs::read_dir(if dir.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        dir.as_path()
    }) else {
        return ptr::null_mut();
    };

    let mut matches: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&fragment) {
                return None;
            }
            let mut path = dir.join(&name).to_string_lossy().into_owned();
            // vim marks a directory with a trailing separator so a caller can
            // tell it apart without stat'ing it again.
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                path.push(std::path::MAIN_SEPARATOR);
            }
            Some(path)
        })
        .collect();
    matches.sort_unstable();

    into_raw_cstring((!matches.is_empty()).then(|| matches.join("\n")))
}

extern "C" fn host_dir_completions(_host: *const HostApi, prefix: *const c_char) -> *mut c_char {
    // The file list already marks directories with a trailing separator, so
    // filtering on that keeps the two in step rather than re-deriving it.
    let raw = host_file_completions(_host, prefix);
    if raw.is_null() {
        return ptr::null_mut();
    }
    let all = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { drop(CString::from_raw(raw)) };

    let dirs: Vec<&str> = all
        .lines()
        .filter(|path| path.ends_with(std::path::MAIN_SEPARATOR))
        .collect();
    into_raw_cstring((!dirs.is_empty()).then(|| dirs.join("\n")))
}

extern "C" fn host_option_set(_host: *const HostApi, name: *const c_char) -> c_int {
    let set = arg_string(name)
        .map(|name| zmax_core::vim_opts::get(&[name.as_str()]).is_some())
        .unwrap_or(false);
    c_int::from(set)
}

extern "C" fn host_buffer_language(_host: *const HostApi, index: usize) -> *mut c_char {
    let language = with_cx(|cx| {
        cx.editor
            .documents()
            .nth(index)
            .and_then(|doc| doc.language_name().map(str::to_string))
    });
    into_raw_cstring(language.flatten())
}

extern "C" fn host_cursor_wanted_column(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let (view, doc) = current!(cx.editor);
        doc.selection(view.id)
            .primary()
            .old_visual_position
            // The remembered column a vertical motion aims for; the row half of
            // the pair is not part of `curswant`.
            .map(|(_row, column)| column as usize)
            .unwrap_or(usize::MAX)
    })
    .unwrap_or(usize::MAX)
}

extern "C" fn host_jump_count(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let view = cx.editor.tree.get(cx.editor.tree.focus);
        view.jumps.len()
    })
    .unwrap_or(0)
}

extern "C" fn host_jump(_host: *const HostApi, index: usize) -> Span {
    with_cx(|cx| {
        let view = cx.editor.tree.get(cx.editor.tree.focus);
        let Some((doc_id, selection)) = view.jumps.get(index) else {
            return NO_SPAN;
        };
        let range = selection.primary();
        // The jump's own document, which is not necessarily the current one --
        // measuring the line against the wrong buffer would be nonsense.
        let Some(doc) = cx.editor.documents().find(|doc| doc.id() == doc_id) else {
            return NO_SPAN;
        };
        let text = doc.text().slice(..);
        let cursor = range.cursor(text).min(text.len_chars());
        Span {
            anchor: cursor,
            head: cursor,
            line: text.char_to_line(cursor),
            valid: 1,
        }
    })
    .unwrap_or(NO_SPAN)
}

extern "C" fn host_jump_buffer(_host: *const HostApi, index: usize) -> usize {
    with_cx(|cx| {
        let view = cx.editor.tree.get(cx.editor.tree.focus);
        let Some((doc_id, _)) = view.jumps.get(index) else {
            return usize::MAX;
        };
        // A jump outlives the buffer it points into, so a closed one has no
        // index rather than a stale one.
        cx.editor
            .documents()
            .position(|doc| doc.id() == doc_id)
            .unwrap_or(usize::MAX)
    })
    .unwrap_or(usize::MAX)
}

extern "C" fn host_jump_index(_host: *const HostApi) -> usize {
    with_cx(|cx| {
        let view = cx.editor.tree.get(cx.editor.tree.focus);
        view.jumps.current_index()
    })
    .unwrap_or(0)
}

extern "C" fn host_option_completions(_host: *const HostApi, prefix: *const c_char) -> *mut c_char {
    let Some(prefix) = arg_string(prefix) else {
        return ptr::null_mut();
    };
    // Only options that have been set are known: `:set` is the only thing that
    // records one, so there is no table of every possible name to draw on.
    let matches: Vec<String> = zmax_core::vim_opts::names()
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .collect();
    into_raw_cstring((!matches.is_empty()).then(|| matches.join("\n")))
}

extern "C" fn host_syntax_at(_host: *const HostApi, offset: usize) -> *mut c_char {
    let scopes = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let syntax = doc.syntax()?;
        let text = doc.text().slice(..);
        let target = text.char_to_byte(offset.min(text.len_chars())) as u32;

        let loader = cx.editor.syn_loader.load();
        let mut highlighter = syntax.highlighter(text, &loader, ..);

        // Walk events up to the offset, keeping the active stack. `Refresh`
        // means the highlighter rebuilt the stack rather than pushing onto it,
        // so the old contents no longer apply -- the same contract the renderer
        // follows in ui::markdown.
        let mut stack: Vec<zmax_core::syntax::Highlight> = Vec::new();
        while highlighter.next_event_offset() <= target {
            let (event, new_highlights) = highlighter.advance();
            if event == zmax_core::syntax::HighlightEvent::Refresh {
                stack.clear();
            }
            stack.extend(new_highlights);
            // `advance` past the end stops moving, so bail rather than spin.
            if highlighter.next_event_offset() == u32::MAX {
                break;
            }
        }

        let theme = &cx.editor.theme;
        let names: Vec<String> = stack
            .into_iter()
            .map(|highlight| theme.scope(highlight).to_string())
            .collect();
        (!names.is_empty()).then(|| names.join("\n"))
    });
    into_raw_cstring(scopes.flatten())
}

/// vim `getregion()`. Charwise and linewise are offset arithmetic; blockwise
/// is the rectangle between the two SCREEN columns, and follows the same rule
/// `block_reproject` applies to `CTRL-V`: a row whose text does not reach the
/// block's left column is skipped, not emitted as an empty row.
#[allow(deprecated)] // visual_coords_at_pos/pos_at_visual_coords: no softwrap in a block
extern "C" fn host_region(_host: *const HostApi, from: usize, to: usize, mode: u8) -> *mut c_char {
    use zmax_core::{pos_at_visual_coords, visual_coords_at_pos, Position};

    let rows = with_cx(|cx| {
        let (_view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let len = text.len_chars();
        let (from, to) = (from.min(len), to.min(len));
        let (from, to) = (from.min(to), from.max(to));

        match mode {
            // Charwise: exactly the offsets, as `text_range` reads them.
            0 => Some(text.slice(from..to).to_string()),
            // Linewise: widened to whole lines, the last without its ending.
            1 => {
                let (first, last) = (text.char_to_line(from), text.char_to_line(to));
                let start = text.line_to_char(first);
                let end = if last + 1 < text.len_lines() {
                    text.line_to_char(last + 1)
                } else {
                    len
                };
                Some(
                    text.slice(start..end)
                        .to_string()
                        .trim_end_matches('\n')
                        .to_string(),
                )
            }
            // Blockwise: the rectangle between the two screen columns.
            2 => {
                let tab_width = doc.tab_width();
                let a = visual_coords_at_pos(text, from, tab_width);
                let b = visual_coords_at_pos(text, to, tab_width);
                let (r0, r1) = (a.row.min(b.row), a.row.max(b.row));
                let (cmin, cmax) = (a.col.min(b.col), a.col.max(b.col));

                let mut out: Vec<String> = Vec::new();
                for row in r0..=r1.min(text.len_lines().saturating_sub(1)) {
                    let left = pos_at_visual_coords(text, Position::new(row, cmin), tab_width);
                    // The row's text stops short of the block's left edge.
                    if visual_coords_at_pos(text, left, tab_width).col != cmin {
                        continue;
                    }
                    let right = pos_at_visual_coords(text, Position::new(row, cmax + 1), tab_width);
                    out.push(text.slice(left..right.max(left)).to_string());
                }
                (!out.is_empty()).then(|| out.join("\n"))
            }
            _ => None,
        }
    });
    into_raw_cstring(rows.flatten())
}

extern "C" fn host_tab_count(_host: *const HostApi) -> usize {
    with_cx(|cx| cx.editor.tab_count()).unwrap_or(1)
}

extern "C" fn host_tab_index(_host: *const HostApi) -> usize {
    with_cx(|cx| cx.editor.current_tab()).unwrap_or(0)
}

/// vim `getbgcolor()`. A theme that leaves `ui.background` unset lets the
/// terminal's own background show through, which is reported as null rather
/// than as an invented colour.
extern "C" fn host_bg_color(_host: *const HostApi) -> *mut c_char {
    use zmax_view::graphics::Color;

    let color = with_cx(|cx| {
        let bg = cx.editor.theme.get("ui.background").bg?;
        Some(match bg {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Color::Indexed(n) => n.to_string(),
            named => format!("{named:?}").to_lowercase(),
        })
    });
    into_raw_cstring(color.flatten())
}

extern "C" fn host_free_cstring(_host: *const HostApi, s: *mut c_char) {
    if !s.is_null() {
        // Reclaim ownership of a string we handed out via `into_raw`.
        unsafe { drop(CString::from_raw(s)) };
    }
}

/// The single process-wide host table. Leaked so its address is `'static` —
/// plugins may retain the `*const HostApi` and call through it from any command.
fn host_api() -> *const HostApi {
    static API: OnceLock<usize> = OnceLock::new();
    let addr = API.get_or_init(|| {
        let boxed = Box::new(HostApi {
            abi_version: ABI_VERSION,
            ctx: ptr::null_mut(),
            register_command: host_register_command,
            message: host_message,
            error: host_error,
            eval: host_eval,
            buffer_text: host_buffer_text,
            insert_text: host_insert_text,
            free_cstring: host_free_cstring,
            cursor: host_cursor,
            word_at_cursor: host_word_at_cursor,
            selection_text: host_selection_text,
            line: host_line,
            line_count: host_line_count,
            mode: host_mode,
            cwd: host_cwd,
            buffer_path: host_buffer_path,
            language: host_language,
            is_modified: host_is_modified,
            register: host_register,
            selection_count: host_selection_count,
            selection: host_selection,
            text_range: host_text_range,
            buffer_count: host_buffer_count,
            buffer_name: host_buffer_name,
            diagnostic_count: host_diagnostic_count,
            diagnostic: host_diagnostic,
            diagnostic_message: host_diagnostic_message,
            diagnostic_severity: host_diagnostic_severity,
            option: host_option,
            search_pattern: host_search_pattern,
            window_count: host_window_count,
            window_view: host_window_view,
            file_size: host_file_size,
            file_type: host_file_type,
            file_time: host_file_time,
            file_perm: host_file_perm,
            buffer_line: host_buffer_line,
            byte_offset: host_byte_offset,
            char_offset: host_char_offset,
            command_exists: host_command_exists,
            plugin_count: host_plugin_count,
            plugin_name: host_plugin_name,
            mark: host_mark,
            window_width: host_window_width,
            window_height: host_window_height,
            completions: host_completions,
            marks: host_marks,
            changelist_count: host_changelist_count,
            changelist: host_changelist,
            changelist_index: host_changelist_index,
            display_width: host_display_width,
            buffer_path_at: host_buffer_path_at,
            buffer_modified: host_buffer_modified,
            window_index: host_window_index,
            line_length: host_line_length,
            indent: host_indent,
            word_count: host_word_count,
            option_num: host_option_num,
            option_bool: host_option_bool,
            fname_modify: host_fname_modify,
            is_directory: host_is_directory,
            file_readable: host_file_readable,
            file_writable: host_file_writable,
            line_to_byte: host_line_to_byte,
            byte_to_line: host_byte_to_line,
            env: host_env,
            buffer_index: host_buffer_index,
            window_buffer: host_window_buffer,
            fold_level: host_fold_level,
            fold_closed: host_fold_closed,
            search_count: host_search_count,
            search_next: host_search_next,
            pid: host_pid,
            virtual_column: host_virtual_column,
            file_at_cursor: host_file_at_cursor,
            change_number: host_change_number,
            buffer_window: host_buffer_window,
            long_word_at_cursor: host_long_word_at_cursor,
            screen_position: host_screen_position,
            window_width_at: host_window_width_at,
            window_height_at: host_window_height_at,
            file_completions: host_file_completions,
            dir_completions: host_dir_completions,
            option_set: host_option_set,
            buffer_language: host_buffer_language,
            cursor_wanted_column: host_cursor_wanted_column,
            jump_count: host_jump_count,
            jump: host_jump,
            jump_buffer: host_jump_buffer,
            jump_index: host_jump_index,
            option_completions: host_option_completions,
            syntax_at: host_syntax_at,
            region: host_region,
            tab_count: host_tab_count,
            tab_index: host_tab_index,
            bg_color: host_bg_color,
        });
        Box::into_raw(boxed) as usize
    });
    *addr as *const HostApi
}

// ============================================================
// Public API — driven by `:plugin load/unload/list`.
// ============================================================

/// Load a plugin `cdylib` from `path`. Returns the plugin's name on success.
/// Loading a plugin whose name is already present is refused (unload first).
pub fn load(path: &str) -> Result<String, String> {
    let _guard = load_lock().lock().unwrap();

    // `dlopen`. libloading resolves relative paths against the loader's search
    // rules; expand `~` for convenience since callers hand raw tokens here.
    let expanded = expand_tilde(path);
    let lib = unsafe { libloading::Library::new(&expanded) }
        .map_err(|e| format!("cannot load `{}`: {}", path, e))?;

    // Resolve the mandatory init symbol.
    let init: libloading::Symbol<InitFn> = unsafe {
        lib.get(INIT_SYMBOL).map_err(|_| {
            format!(
                "`{}`: not a zmax plugin (no {})",
                path,
                String::from_utf8_lossy(&INIT_SYMBOL[..INIT_SYMBOL.len() - 1])
            )
        })?
    };

    // Clear staging, call init, collect what it registered.
    staging().lock().unwrap().clear();
    let info_ptr: *const PluginInfo = init(host_api());
    if info_ptr.is_null() {
        staging().lock().unwrap().clear();
        return Err(format!(
            "`{}`: plugin init failed (ABI mismatch or error)",
            path
        ));
    }
    let info = unsafe { &*info_ptr };
    if info.abi_version != ABI_VERSION {
        staging().lock().unwrap().clear();
        return Err(format!(
            "`{}`: ABI version {} != host {}",
            path, info.abi_version, ABI_VERSION
        ));
    }
    let name = cstr_or(info.name, "unknown");
    let version = cstr_or(info.version, "?");

    // Refuse a duplicate name — the second load's commands would shadow the
    // first with no clean unload story.
    if plugins().lock().unwrap().iter().any(|p| p.name == name) {
        staging().lock().unwrap().clear();
        return Err(format!("plugin `{}` already loaded", name));
    }

    // Commit staged commands into the live registry, tagged with owner.
    let staged: Vec<(String, CommandFn)> = std::mem::take(&mut *staging().lock().unwrap());
    {
        let mut reg = registry().lock().unwrap();
        let mut own = ownership().lock().unwrap();
        for (cmd, func) in staged {
            reg.insert(cmd.clone(), func);
            own.insert(cmd, name.clone());
        }
    }

    plugins().lock().unwrap().push(LoadedPlugin {
        name: name.clone(),
        version: version.clone(),
        path: expanded,
        _lib: lib,
    });

    log::info!("loaded native plugin `{}` v{} ({})", name, version, path);
    Ok(name)
}

/// Unload a plugin by name: purge its command registrations FIRST (so no live
/// function pointer survives), then drop the `Library` (`dlclose`).
pub fn unload(name: &str) -> Result<(), String> {
    let _guard = load_lock().lock().unwrap();

    let present = plugins().lock().unwrap().iter().any(|p| p.name == name);
    if !present {
        return Err(format!("plugin `{}` not loaded", name));
    }

    // Purge registry entries owned by this plugin.
    {
        let mut own = ownership().lock().unwrap();
        let mut reg = registry().lock().unwrap();
        let owned: Vec<String> = own
            .iter()
            .filter(|(_, o)| o.as_str() == name)
            .map(|(c, _)| c.clone())
            .collect();
        for cmd in owned {
            reg.remove(&cmd);
            own.remove(&cmd);
        }
    }

    // Now it is safe to dlclose.
    let mut ps = plugins().lock().unwrap();
    if let Some(pos) = ps.iter().position(|p| p.name == name) {
        let p = ps.remove(pos);
        log::info!("unloaded native plugin `{}`", name);
        drop(p); // explicit: dlclose here, after registry purge.
    }
    Ok(())
}

/// Command-resolution hook. Called from the `:`-command dispatcher for names
/// unknown to the static registry. Installs the editor bridge, runs the plugin
/// handler, and returns `Some(exit_status)` if a plugin owns `cmd`, else `None`.
pub fn dispatch(cx: &mut compositor::Context, cmd: &str, args: &[String]) -> Option<i32> {
    // Copy the handler out under the lock, then release it before calling — the
    // handler may itself `load`/`eval`, which would re-take these locks.
    let func = { registry().lock().unwrap().get(cmd).copied() }?;

    // Build argv = [cmd, args...] as NUL-terminated C strings.
    let mut owned: Vec<CString> = Vec::with_capacity(args.len() + 1);
    owned.push(CString::new(cmd).ok()?);
    for a in args {
        owned.push(
            CString::new(a.as_str())
                .unwrap_or_else(|_| CString::new(a.replace('\0', "")).unwrap_or_default()),
        );
    }
    let ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();

    let _bridge = CxGuard::new(cx);
    let rc = func(host_api(), ptrs.len(), ptrs.as_ptr());
    // `owned`/`ptrs` outlive the call. Done.
    Some(rc)
}

/// `(name, version, path)` for each loaded plugin, sorted by name.
pub fn list() -> Vec<(String, String, String)> {
    let mut v: Vec<(String, String, String)> = plugins()
        .lock()
        .unwrap()
        .iter()
        .map(|p| (p.name.clone(), p.version.clone(), p.path.clone()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// True if `name` is a live plugin command. Consulted by the `:`-command
/// dispatcher before falling through to user commands / vimscript.
pub fn is_plugin_command(name: &str) -> bool {
    registry().lock().unwrap().contains_key(name)
}

fn cstr_or(p: *const c_char, dflt: &str) -> String {
    if p.is_null() {
        dflt.to_string()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/{}", home.trim_end_matches('/'), rest);
        }
    }
    path.to_string()
}
