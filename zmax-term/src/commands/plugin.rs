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
        Some(if meta.file_type().is_symlink() {
            "link"
        } else if meta.is_dir() {
            "dir"
        } else {
            "file"
        }
        .to_string())
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
                (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
                (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
                (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
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

extern "C" fn host_buffer_line(
    _host: *const HostApi,
    buffer: usize,
    line: usize,
) -> *mut c_char {
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
