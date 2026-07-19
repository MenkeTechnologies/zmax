//! # `zmax-plugin` — native plugin SDK for zmax
//!
//! zmax hosts third-party plugins written in a **native compiled language**
//! (Rust) and loaded at runtime — no recompile of the editor, no script glue. A
//! plugin is an ordinary `cdylib` that zmax `dlopen`s via `:plugin load <path>`.
//! Each plugin registers **typable commands** (the editor's `:`-commands) that
//! resolve just like the built-in ones.
//!
//! The boundary between host and plugin is a hand-rolled, versioned **C ABI**
//! (`#[repr(C)]` structs + `extern "C"` fn pointers). Both sides depend on THIS
//! crate so they agree on the exact layout. Nothing about Rust's unstable
//! `repr(Rust)` layout, allocator, or panic ABI crosses the boundary — only
//! C-representable data.
//!
//! ## Writing a plugin
//!
//! ```ignore
//! use zmax_plugin::{declare_plugin, Args, Host};
//! use std::os::raw::c_int;
//!
//! fn hello(host: &Host, args: &Args) -> c_int {
//!     host.message(&format!("hello from rust, argv={:?}", args.to_vec()));
//!     // insert some text into the current buffer
//!     host.insert_text("greetings\n");
//!     0
//! }
//!
//! declare_plugin! {
//!     name: "hello",
//!     version: "0.1.0",
//!     commands: { "hello" => hello },
//! }
//! ```
//!
//! `Cargo.toml`:
//! ```toml
//! [lib]
//! crate-type = ["cdylib"]
//! [dependencies]
//! zmax-plugin = "0.4"
//! ```
//!
//! `cargo build` produces `libhello.dylib` / `libhello.so`; then inside zmax:
//! `:plugin load ~/plugins/libhello.dylib` and `:hello` is a live command.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

/// ABI version. Bumped on ANY change to [`HostApi`], [`PluginInfo`],
/// [`CommandFn`], or [`InitFn`] layout/semantics. The host refuses to load a
/// plugin whose `abi_version` does not match its own — a mismatched struct
/// layout is undefined behaviour, so this is a hard gate, not a warning.
pub const ABI_VERSION: u32 = 1;

/// The one symbol every plugin `cdylib` must export. The host resolves it with
/// `dlsym` after `dlopen`. Signature is [`InitFn`].
pub const INIT_SYMBOL: &[u8] = b"zmax_plugin_init\0";

/// A plugin-provided command handler.
///
/// * `host`   — the host API table (call back into the editor through it).
/// * `argc`   — number of elements in `argv`.
/// * `argv`   — NUL-terminated C strings; `argv[0]` is the command name,
///              `argv[1..]` the arguments. Valid only for the duration of the
///              call; copy anything you need to keep.
///
/// Returns the command's exit status (0 = success).
pub type CommandFn =
    extern "C" fn(host: *const HostApi, argc: usize, argv: *const *const c_char) -> c_int;

/// Signature of [`INIT_SYMBOL`]. Called exactly once, right after the dylib is
/// loaded. The plugin registers its commands through `host.register_command` and
/// returns a pointer to a `'static` [`PluginInfo`] describing itself (or null on
/// failure).
pub type InitFn = extern "C" fn(host: *const HostApi) -> *const PluginInfo;

/// The host API table handed to the plugin. Every field is a C-ABI function
/// pointer into zmax. Layout is frozen by [`ABI_VERSION`].
///
/// A single instance lives for the whole process; plugins may store the
/// `*const HostApi` they are given and call through it from any command.
///
/// Callbacks that touch the editor (`message`, `error`, `eval`, `buffer_text`,
/// `insert_text`) are only valid **while a plugin command is executing** — the
/// host publishes the active editor context for the duration of that call. They
/// are inert (return empty/failure) if invoked outside that window, e.g. from a
/// background thread the plugin spawned.
#[repr(C)]
pub struct HostApi {
    /// Must equal [`ABI_VERSION`]. Checked by the plugin's own `declare_plugin!`
    /// glue before it trusts the rest of the table.
    pub abi_version: u32,
    /// Reserved for the host; opaque to plugins. Currently null.
    pub ctx: *mut c_void,
    /// Register a command name → handler. Returns 0 on success. Names registered
    /// here resolve as `:`-commands in the editor (after built-in commands,
    /// before the user-command / vimscript fallthrough). `name` is copied.
    pub register_command:
        extern "C" fn(host: *const HostApi, name: *const c_char, handler: CommandFn) -> c_int,
    /// Show text on the editor status line (no trailing newline needed). This is
    /// the TUI-safe replacement for a shell's stdout: a plugin must never write
    /// to the real terminal fds while the editor owns them.
    pub message: extern "C" fn(host: *const HostApi, text: *const c_char),
    /// Show text on the editor status line styled as an error.
    pub error: extern "C" fn(host: *const HostApi, text: *const c_char),
    /// Run a `:` command line in the editor and return 0 on success, non-zero on
    /// failure. `line` is UTF-8, NUL-terminated, without the leading `:`.
    pub eval: extern "C" fn(host: *const HostApi, line: *const c_char) -> c_int,
    /// Read the current buffer's full text. Returns a freshly allocated C string
    /// the caller MUST release with `free_cstring`, or null if there is no active
    /// editor context.
    pub buffer_text: extern "C" fn(host: *const HostApi) -> *mut c_char,
    /// Insert `text` at the primary cursor of the current buffer (undoable as one
    /// transaction). Returns 0 on success.
    pub insert_text: extern "C" fn(host: *const HostApi, text: *const c_char) -> c_int,
    /// Release a string previously returned by `buffer_text`.
    pub free_cstring: extern "C" fn(host: *const HostApi, s: *mut c_char),
}

/// What a plugin returns from its [`InitFn`]. The strings must have `'static`
/// lifetime (typically string literals via the `declare_plugin!` macro).
#[repr(C)]
pub struct PluginInfo {
    /// Must equal [`ABI_VERSION`]. Redundant with the host-side check, but lets
    /// the host reject a plugin that lied about its ABI.
    pub abi_version: u32,
    /// Plugin name, NUL-terminated. Used for `:plugin list` and
    /// `:plugin unload <name>`.
    pub name: *const c_char,
    /// Plugin version, NUL-terminated. Informational.
    pub version: *const c_char,
}

// PluginInfo is only ever pointed at `'static` data; it carries no interior
// mutability. Marking it Sync lets the macro place it in a `static`.
unsafe impl Sync for PluginInfo {}

// ============================================================
// Ergonomic wrappers for plugin authors. None of this crosses the ABI; it is
// convenience over the raw pointers above.
// ============================================================

/// Safe wrapper over `*const HostApi` for use inside a command handler. Cheap to
/// construct; borrows the host table.
pub struct Host {
    api: *const HostApi,
}

impl Host {
    /// Wrap a raw host pointer.
    ///
    /// # Safety
    /// `api` must be the non-null `*const HostApi` the host handed to the plugin
    /// (in `zmax_plugin_init` or a [`CommandFn`] call) and must remain valid for
    /// the lifetime of this `Host`.
    pub unsafe fn from_raw(api: *const HostApi) -> Self {
        Host { api }
    }

    #[inline]
    fn t(&self) -> &HostApi {
        // Safe: constructed only from a valid host pointer.
        unsafe { &*self.api }
    }

    /// Register a command handler by name. Usually done for you by
    /// `declare_plugin!`; exposed for dynamic registration.
    pub fn register_command(&self, name: &str, handler: CommandFn) -> bool {
        let Ok(cname) = CString::new(name) else {
            return false;
        };
        ((self.t().register_command)(self.api, cname.as_ptr(), handler)) == 0
    }

    /// Show `text` on the editor status line.
    pub fn message(&self, text: &str) {
        if let Ok(c) = CString::new(text) {
            (self.t().message)(self.api, c.as_ptr());
        }
    }

    /// Show `text` on the editor status line as an error.
    pub fn error(&self, text: &str) {
        if let Ok(c) = CString::new(text) {
            (self.t().error)(self.api, c.as_ptr());
        }
    }

    /// Run a `:` command `line` (without the leading `:`); returns its exit status.
    pub fn eval(&self, line: &str) -> i32 {
        match CString::new(line) {
            Ok(c) => (self.t().eval)(self.api, c.as_ptr()) as i32,
            Err(_) => 1,
        }
    }

    /// Read the current buffer's full text, or `None` if there is no active
    /// editor context.
    pub fn buffer_text(&self) -> Option<String> {
        let raw = (self.t().buffer_text)(self.api);
        if raw.is_null() {
            return None;
        }
        // Safe: host contract says this is a valid C string owned by us.
        let s = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        (self.t().free_cstring)(self.api, raw);
        Some(s)
    }

    /// Insert `text` at the primary cursor. Returns true on success.
    pub fn insert_text(&self, text: &str) -> bool {
        match CString::new(text) {
            Ok(c) => (self.t().insert_text)(self.api, c.as_ptr()) == 0,
            Err(_) => false,
        }
    }
}

/// Safe view over a command's `(argc, argv)`. `argv[0]` is the command name.
pub struct Args {
    items: Vec<String>,
}

impl Args {
    /// Decode a raw `(argc, argv)` pair into owned `String`s.
    ///
    /// # Safety
    /// `argv` must point to `argc` valid, NUL-terminated C strings, as
    /// guaranteed by the host when it invokes a [`CommandFn`].
    pub unsafe fn from_raw(argc: usize, argv: *const *const c_char) -> Self {
        let mut items = Vec::with_capacity(argc);
        if !argv.is_null() {
            for i in 0..argc {
                let p = *argv.add(i);
                if p.is_null() {
                    break;
                }
                items.push(CStr::from_ptr(p).to_string_lossy().into_owned());
            }
        }
        Args { items }
    }

    /// The command name (`argv[0]`), or `""` if somehow empty.
    pub fn name(&self) -> &str {
        self.items.first().map(String::as_str).unwrap_or("")
    }

    /// The positional arguments (everything after `argv[0]`).
    pub fn rest(&self) -> &[String] {
        if self.items.is_empty() {
            &[]
        } else {
            &self.items[1..]
        }
    }

    /// All of `argv`, name included.
    pub fn to_vec(&self) -> &[String] {
        &self.items
    }
}

/// Declare a plugin: its identity and the commands it registers. Expands to the
/// `#[no_mangle] extern "C" fn zmax_plugin_init` the host looks for, plus the
/// `'static` [`PluginInfo`]. Each handler is `fn(&Host, &Args) -> c_int`.
///
/// ```ignore
/// declare_plugin! {
///     name: "hello",
///     version: "0.1.0",
///     commands: {
///         "hello" => hello_handler,
///         "bye"   => bye_handler,
///     },
/// }
/// ```
#[macro_export]
macro_rules! declare_plugin {
    (
        name: $name:literal,
        version: $version:literal,
        commands: { $($cmd:literal => $handler:path),+ $(,)? } $(,)?
    ) => {
        static __ZMAX_PLUGIN_INFO: $crate::PluginInfo = $crate::PluginInfo {
            abi_version: $crate::ABIVERSION_FOR_MACRO,
            name: concat!($name, "\0").as_ptr() as *const ::std::os::raw::c_char,
            version: concat!($version, "\0").as_ptr() as *const ::std::os::raw::c_char,
        };

        #[no_mangle]
        pub extern "C" fn zmax_plugin_init(
            host: *const $crate::HostApi,
        ) -> *const $crate::PluginInfo {
            if host.is_null() {
                return ::std::ptr::null();
            }
            // Verify the host speaks our ABI before touching the table.
            let ver = unsafe { (*host).abi_version };
            if ver != $crate::ABI_VERSION {
                return ::std::ptr::null();
            }
            let h = unsafe { $crate::Host::from_raw(host) };
            $(
                {
                    // One trampoline per registered handler: adapts the C-ABI
                    // CommandFn to the ergonomic fn(&Host, &Args).
                    extern "C" fn __trampoline(
                        host: *const $crate::HostApi,
                        argc: usize,
                        argv: *const *const ::std::os::raw::c_char,
                    ) -> ::std::os::raw::c_int {
                        let h = unsafe { $crate::Host::from_raw(host) };
                        let a = unsafe { $crate::Args::from_raw(argc, argv) };
                        $handler(&h, &a)
                    }
                    h.register_command($cmd, __trampoline);
                }
            )+
            &__ZMAX_PLUGIN_INFO as *const $crate::PluginInfo
        }
    };
}

// The macro can't name `ABI_VERSION` inside a `const` initializer of a
// downstream crate without importing it; re-export under a stable path the macro
// hard-codes so users need only `use zmax_plugin::*` or the two names above.
#[doc(hidden)]
pub const ABIVERSION_FOR_MACRO: u32 = ABI_VERSION;
