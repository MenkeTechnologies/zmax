//! Native (Rust) plugin host — editor extension.
//!
//! zmax `dlopen`s third-party `cdylib`s that register **typable commands** over
//! a stable, versioned C ABI (the `zmax-plugin` crate). A plugin ships a
//! compiled `.dylib`/`.so` and is loaded at runtime with `:plugin load <path>` —
//! no zmax recompile, no script glue. This is the port of zshrs's native plugin
//! host (`zmodload -R`) to the editor.
//!
//! ## Where plugin commands resolve
//!
//! A freshly-loaded plugin command is unknown to the static
//! [`TYPABLE_COMMAND_MAP`](crate::commands::typed::TYPABLE_COMMAND_MAP), so it
//! arrives at [`execute_command_line_inner`](crate::commands::typed)'s
//! fallthrough, which consults [`dispatch`] AFTER built-in typables and BEFORE
//! the user-command / vimscript fallback — the same slot zsh's plugin host
//! occupies (after real builtins, before PATH).
//!
//! ## The editor bridge
//!
//! Host callbacks are bare `extern "C" fn`s that cannot capture `&mut Editor`.
//! The active command [`compositor::Context`] is published through a
//! thread-local raw pointer for the duration of a single, synchronous,
//! on-editor-thread call (installed by [`CxGuard`], cleared on drop) — the same
//! pattern the embedded interpreters use, kept independent here so the native
//! plugin ABI works without the `scripting` feature. Every callback that touches
//! the editor goes through [`with_cx`]; called outside a guarded window it is
//! inert.
//!
//! ## ABI safety
//!
//! Everything crossing the boundary is `#[repr(C)]`. The host verifies the
//! plugin's `abi_version` matches [`zmax_plugin::ABI_VERSION`] before trusting
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
use zmax_plugin::{CommandFn, HostApi, InitFn, PluginInfo, ABI_VERSION, INIT_SYMBOL};

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
    Some(rc as i32)
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
