//! # `zmax-native` — native plugin SDK for zmax
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
//! use zmax_native::{declare_plugin, Args, Host};
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
//! zmax-native = "0.4"
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
pub const ABI_VERSION: u32 = 2;

/// The one symbol every plugin `cdylib` must export. The host resolves it with
/// `dlsym` after `dlopen`. Signature is [`InitFn`].
pub const INIT_SYMBOL: &[u8] = b"zmax_native_init\0";

/// A plugin-provided command handler.
///
/// * `host`   — the host API table (call back into the editor through it).
/// * `argc`   — number of elements in `argv`.
/// * `argv`   — NUL-terminated C strings; `argv[0]` is the command name,
///   `argv[1..]` the arguments. Valid only for the duration of the call; copy
///   anything you need to keep.
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
/// Where the primary cursor sits, as reported by [`HostApi::cursor`].
///
/// All three are zero-based. `line` and `column` are what a plugin shows a
/// human; `offset` is the char index into the buffer, which is what indexing
/// the text with [`HostApi::buffer_text`] needs. `column` counts characters
/// from the line start, matching how the editor reports a position.
///
/// A [`Self::valid`] of 0 means there was no active editor context and the
/// other fields are meaningless -- a return value cannot be `Option` across the
/// C ABI, so the flag carries that instead.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub column: usize,
    pub offset: usize,
    pub valid: u8,
}

/// A half-open span of the buffer in char offsets, as [`HostApi::selection`]
/// and [`HostApi::diagnostic`] report one.
///
/// `anchor` is the end that stays put when a selection is extended and `head`
/// is the end that moves, so `head < anchor` for a backwards selection --
/// unlike vim's `'<`/`'>`, which are always in document order. `valid` is 0
/// when the index was out of range.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub anchor: usize,
    pub head: usize,
    pub line: usize,
    pub valid: u8,
}

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
    /// Where the primary cursor is. See [`Cursor`]; `valid` is 0 when there is
    /// no active editor context.
    pub cursor: extern "C" fn(host: *const HostApi) -> Cursor,
    /// The word under the primary cursor, as `miw` would select it. Returns a
    /// freshly allocated C string the caller MUST release with `free_cstring`,
    /// or null when there is no editor context or the cursor is not on a word.
    pub word_at_cursor: extern "C" fn(host: *const HostApi) -> *mut c_char,
    /// The text of the primary selection, or null when there is no editor
    /// context. An empty (cursor-width) selection yields the single character
    /// under the cursor, matching what the editor would yank. Release with
    /// `free_cstring`.
    pub selection_text: extern "C" fn(host: *const HostApi) -> *mut c_char,

    // --- vim-style getters -------------------------------------------------
    // Named after the `get*` family a vimscript author reaches for, so the
    // mapping is obvious: `line` is `getline()`, `line_count` is `line("$")`,
    // `cwd` is `getcwd()`, `register` is `getreg()`, and so on. All strings are
    // released with `free_cstring`; all return null / 0 when there is no
    // active editor context rather than inventing a value.
    /// vim `getline({lnum})`. One line of the current buffer WITHOUT its line
    /// ending, or null if `line` is past the end. Zero-based, unlike vim's
    /// one-based `lnum`, to match [`Cursor::line`].
    pub line: extern "C" fn(host: *const HostApi, line: usize) -> *mut c_char,
    /// vim `line("$")` — the number of lines in the current buffer.
    pub line_count: extern "C" fn(host: *const HostApi) -> usize,
    /// vim `mode()` — `normal`, `insert` or `select`.
    pub mode: extern "C" fn(host: *const HostApi) -> *mut c_char,
    /// vim `getcwd()` — the editor's working directory.
    pub cwd: extern "C" fn(host: *const HostApi) -> *mut c_char,
    /// vim `expand("%:p")` — the current buffer's absolute path, or null for a
    /// scratch buffer that has never been written.
    pub buffer_path: extern "C" fn(host: *const HostApi) -> *mut c_char,
    /// vim `&filetype` — the language name from `languages.toml`, or null when
    /// the buffer has no language configured.
    pub language: extern "C" fn(host: *const HostApi) -> *mut c_char,
    /// vim `&modified` — 1 when the buffer has unsaved changes, 0 otherwise (or
    /// when there is no editor context).
    pub is_modified: extern "C" fn(host: *const HostApi) -> c_int,
    /// vim `getreg({regname})` — a register's contents. Multiple values are
    /// joined with newlines, as vim does for a list register. Null when the
    /// register is empty or unset.
    pub register: extern "C" fn(host: *const HostApi, name: c_char) -> *mut c_char,
    /// The number of selections (cursors). zmax is multi-selection, so a plugin
    /// that assumes one would silently act on part of the user's intent; this
    /// is what tells it there are more.
    pub selection_count: extern "C" fn(host: *const HostApi) -> usize,

    /// vim `getpos("'<")`/`getpos("'>")` for one selection, by index. See
    /// [`Span`]; `valid` is 0 past the last selection.
    pub selection: extern "C" fn(host: *const HostApi, index: usize) -> Span,
    /// The text between two char offsets, clamped to the buffer. vim
    /// `getbufline` reads whole lines; this reads a span, which is what a
    /// [`Span`] from `selection` or `diagnostic` addresses. Release with
    /// `free_cstring`.
    pub text_range: extern "C" fn(host: *const HostApi, from: usize, to: usize) -> *mut c_char,

    /// vim `getbufinfo()` — how many buffers are open.
    pub buffer_count: extern "C" fn(host: *const HostApi) -> usize,
    /// The display name of the `index`th open buffer, in the editor's own
    /// order. Null past the last one. Release with `free_cstring`.
    pub buffer_name: extern "C" fn(host: *const HostApi, index: usize) -> *mut c_char,

    /// vim `getqflist()` — how many diagnostics the current buffer has.
    pub diagnostic_count: extern "C" fn(host: *const HostApi) -> usize,
    /// Where the `index`th diagnostic is. `valid` is 0 past the last one.
    pub diagnostic: extern "C" fn(host: *const HostApi, index: usize) -> Span,
    /// The `index`th diagnostic's message, or null past the last one. Release
    /// with `free_cstring`.
    pub diagnostic_message: extern "C" fn(host: *const HostApi, index: usize) -> *mut c_char,
    /// The `index`th diagnostic's severity as `hint`, `info`, `warning` or
    /// `error`. A diagnostic whose server left the severity unset reads as
    /// `warning`, matching how the editor treats it. Null past the last one.
    pub diagnostic_severity: extern "C" fn(host: *const HostApi, index: usize) -> *mut c_char,

    /// vim `&{option}` / `getbufvar(buf, "&opt")` — a vim option's value as a
    /// string, or null when it is not set. Both the long and short spellings
    /// work (`shiftwidth` and `sw`), as they do on `:set`.
    pub option: extern "C" fn(host: *const HostApi, name: *const c_char) -> *mut c_char,
    /// vim `getreg("/")` — the last search pattern, or null when nothing has
    /// been searched for yet.
    pub search_pattern: extern "C" fn(host: *const HostApi) -> *mut c_char,

    /// vim `getwininfo()` — how many windows (splits) are open.
    pub window_count: extern "C" fn(host: *const HostApi) -> usize,
    /// vim `winline()`/`wincol()` as a pair: the first visible line and column
    /// of the current window, which is what a plugin needs to know what the
    /// user can actually see. `line`/`anchor` are that offset; `head` is the
    /// last visible line. `valid` is 0 with no editor context.
    pub window_view: extern "C" fn(host: *const HostApi) -> Span,

    /// vim `getfsize({fname})` — a file's size in bytes, or -1 when it cannot
    /// be read. Takes a path so a plugin can ask about any file, not just the
    /// open one.
    pub file_size: extern "C" fn(host: *const HostApi, path: *const c_char) -> i64,
    /// vim `getftype({fname})` — `file`, `dir`, `link`, or null when the path
    /// does not exist. Release with `free_cstring`.
    pub file_type: extern "C" fn(host: *const HostApi, path: *const c_char) -> *mut c_char,
    /// vim `getftime({fname})` — last modification time in seconds since the
    /// epoch, or -1 when the file cannot be read.
    pub file_time: extern "C" fn(host: *const HostApi, path: *const c_char) -> i64,
    /// vim `getfperm({fname})` — permissions as the nine characters
    /// `rwxrwxrwx`, with `-` where a bit is clear. Null when the path cannot be
    /// read. Release with `free_cstring`.
    pub file_perm: extern "C" fn(host: *const HostApi, path: *const c_char) -> *mut c_char,

    /// vim `getbufline({buf}, {lnum})` — one line of ANY open buffer, not just
    /// the current one, by the same index `buffer_name` uses. Zero-based, and
    /// null past either end. Release with `free_cstring`.
    pub buffer_line:
        extern "C" fn(host: *const HostApi, buffer: usize, line: usize) -> *mut c_char,

    /// The byte offset of a char offset in the current buffer. vim's `col()` is
    /// byte-based while `charcol()` is char-based; everything else in this API
    /// is char-based, and a language server wants bytes, so this is the bridge.
    /// Clamped to the buffer.
    pub byte_offset: extern "C" fn(host: *const HostApi, char_offset: usize) -> usize,
    /// The char offset of a byte offset -- the inverse of `byte_offset`, for
    /// turning a language server's position back into one this API accepts.
    /// Clamped, and rounded down to a char boundary.
    pub char_offset: extern "C" fn(host: *const HostApi, byte_offset: usize) -> usize,

    /// vim `exists(":{name}")` — whether a `:`-command of that name resolves,
    /// built-in or plugin-registered. 1 when it does.
    pub command_exists: extern "C" fn(host: *const HostApi, name: *const c_char) -> c_int,
    /// vim `getscriptinfo()` — how many native plugins are loaded.
    pub plugin_count: extern "C" fn(host: *const HostApi) -> usize,
    /// The `index`th loaded plugin as `name version`, or null past the last.
    /// Release with `free_cstring`.
    pub plugin_name: extern "C" fn(host: *const HostApi, index: usize) -> *mut c_char,
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
    /// (in `zmax_native_init` or a [`CommandFn`] call) and must remain valid for
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
            Ok(c) => (self.t().eval)(self.api, c.as_ptr()),
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

    /// Where the primary cursor sits, or `None` if there is no active editor
    /// context.
    pub fn cursor(&self) -> Option<Cursor> {
        let cursor = (self.t().cursor)(self.api);
        (cursor.valid != 0).then_some(cursor)
    }

    /// The word under the primary cursor, selected the way `miw` selects one.
    /// `None` when there is no editor context, or when the cursor is on
    /// whitespace rather than a word.
    pub fn word_at_cursor(&self) -> Option<String> {
        self.take_string((self.t().word_at_cursor)(self.api))
    }

    /// The text of the primary selection, or `None` if there is no active
    /// editor context.
    pub fn selection_text(&self) -> Option<String> {
        self.take_string((self.t().selection_text)(self.api))
    }

    /// vim `getline({lnum})`, zero-based: one line without its line ending, or
    /// `None` past the end of the buffer.
    pub fn line(&self, line: usize) -> Option<String> {
        self.take_string((self.t().line)(self.api, line))
    }

    /// vim `line("$")` — how many lines the current buffer has.
    pub fn line_count(&self) -> usize {
        (self.t().line_count)(self.api)
    }

    /// Every line from `start` up to but not including `end`, clamped to the
    /// buffer. vim `getline({start}, {end})`, zero-based and end-exclusive.
    pub fn lines(&self, start: usize, end: usize) -> Vec<String> {
        (start..end.min(self.line_count()))
            .filter_map(|n| self.line(n))
            .collect()
    }

    /// vim `mode()` — `normal`, `insert` or `select`.
    pub fn mode(&self) -> Option<String> {
        self.take_string((self.t().mode)(self.api))
    }

    /// vim `getcwd()`.
    pub fn cwd(&self) -> Option<String> {
        self.take_string((self.t().cwd)(self.api))
    }

    /// vim `expand("%:p")` — `None` for a scratch buffer with no path.
    pub fn buffer_path(&self) -> Option<String> {
        self.take_string((self.t().buffer_path)(self.api))
    }

    /// vim `&filetype` — `None` when no language is configured.
    pub fn language(&self) -> Option<String> {
        self.take_string((self.t().language)(self.api))
    }

    /// vim `&modified`.
    pub fn is_modified(&self) -> bool {
        (self.t().is_modified)(self.api) != 0
    }

    /// vim `getreg({regname})` — `None` when the register is empty.
    pub fn register(&self, name: char) -> Option<String> {
        // A register name is a single ASCII character; anything else cannot
        // name one, so there is nothing to read.
        if !name.is_ascii() {
            return None;
        }
        self.take_string((self.t().register)(self.api, name as c_char))
    }

    /// How many selections (cursors) there are. zmax is multi-selection, so a
    /// plugin that assumes one would act on only part of what the user meant.
    pub fn selection_count(&self) -> usize {
        (self.t().selection_count)(self.api)
    }

    /// One selection by index, vim's `getpos("'<")`/`getpos("'>")` for it.
    /// `None` past the last selection.
    pub fn selection(&self, index: usize) -> Option<Span> {
        let span = (self.t().selection)(self.api, index);
        (span.valid != 0).then_some(span)
    }

    /// Every selection, in the editor's order.
    pub fn selections(&self) -> Vec<Span> {
        (0..self.selection_count())
            .filter_map(|i| self.selection(i))
            .collect()
    }

    /// The text between two char offsets, clamped to the buffer -- the span a
    /// [`Span`] addresses.
    pub fn text_range(&self, from: usize, to: usize) -> Option<String> {
        self.take_string((self.t().text_range)(self.api, from, to))
    }

    /// vim `getbufinfo()` — how many buffers are open.
    pub fn buffer_count(&self) -> usize {
        (self.t().buffer_count)(self.api)
    }

    /// The display name of one open buffer, or `None` past the last.
    pub fn buffer_name(&self, index: usize) -> Option<String> {
        self.take_string((self.t().buffer_name)(self.api, index))
    }

    /// Every open buffer's display name, in the editor's order.
    pub fn buffer_names(&self) -> Vec<String> {
        (0..self.buffer_count())
            .filter_map(|i| self.buffer_name(i))
            .collect()
    }

    /// vim `getqflist()` — how many diagnostics the current buffer has.
    pub fn diagnostic_count(&self) -> usize {
        (self.t().diagnostic_count)(self.api)
    }

    /// One diagnostic: where it is, what it says, and how bad it is. `None`
    /// past the last one.
    pub fn diagnostic(&self, index: usize) -> Option<DiagnosticInfo> {
        let span = (self.t().diagnostic)(self.api, index);
        if span.valid == 0 {
            return None;
        }
        Some(DiagnosticInfo {
            span,
            message: self
                .take_string((self.t().diagnostic_message)(self.api, index))
                .unwrap_or_default(),
            severity: self
                .take_string((self.t().diagnostic_severity)(self.api, index))
                .unwrap_or_default(),
        })
    }

    /// Every diagnostic on the current buffer, vim's `getqflist()`.
    pub fn diagnostics(&self) -> Vec<DiagnosticInfo> {
        (0..self.diagnostic_count())
            .filter_map(|i| self.diagnostic(i))
            .collect()
    }

    /// vim `&{option}` — an option's value, by long or short name. `None` when
    /// it is not set.
    pub fn option(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        self.take_string((self.t().option)(self.api, name.as_ptr()))
    }

    /// vim `getreg("/")` — the last search pattern.
    pub fn search_pattern(&self) -> Option<String> {
        self.take_string((self.t().search_pattern)(self.api))
    }

    /// vim `getwininfo()` — how many windows (splits) are open.
    pub fn window_count(&self) -> usize {
        (self.t().window_count)(self.api)
    }

    /// The first and last line the current window is showing, so a plugin can
    /// tell what the user can see. `None` with no editor context.
    pub fn window_view(&self) -> Option<Span> {
        let span = (self.t().window_view)(self.api);
        (span.valid != 0).then_some(span)
    }

    /// vim `getfsize({fname})` — a file's size, or `None` if it cannot be read.
    pub fn file_size(&self, path: &str) -> Option<u64> {
        let path = CString::new(path).ok()?;
        match (self.t().file_size)(self.api, path.as_ptr()) {
            size if size < 0 => None,
            size => Some(size as u64),
        }
    }

    /// vim `getftype({fname})` — `file`, `dir` or `link`; `None` when the path
    /// does not exist.
    pub fn file_type(&self, path: &str) -> Option<String> {
        let path = CString::new(path).ok()?;
        self.take_string((self.t().file_type)(self.api, path.as_ptr()))
    }

    /// vim `getftime({fname})` — modification time in seconds since the epoch,
    /// or `None` when the file cannot be read.
    pub fn file_time(&self, path: &str) -> Option<i64> {
        let path = CString::new(path).ok()?;
        match (self.t().file_time)(self.api, path.as_ptr()) {
            time if time < 0 => None,
            time => Some(time),
        }
    }

    /// vim `getfperm({fname})` — permissions as `rwxrwxrwx`.
    pub fn file_perm(&self, path: &str) -> Option<String> {
        let path = CString::new(path).ok()?;
        self.take_string((self.t().file_perm)(self.api, path.as_ptr()))
    }

    /// vim `getbufline({buf}, {lnum})` — one line of any open buffer, by the
    /// index `buffer_name` uses. `None` past either end.
    pub fn buffer_line(&self, buffer: usize, line: usize) -> Option<String> {
        self.take_string((self.t().buffer_line)(self.api, buffer, line))
    }

    /// The byte offset of a char offset -- what a language server means by a
    /// column. Everything else here is char-based.
    pub fn byte_offset(&self, char_offset: usize) -> usize {
        (self.t().byte_offset)(self.api, char_offset)
    }

    /// The char offset of a byte offset, for turning a language server's
    /// position back into one this API accepts.
    pub fn char_offset(&self, byte_offset: usize) -> usize {
        (self.t().char_offset)(self.api, byte_offset)
    }

    /// vim `exists(":{name}")` — whether a `:`-command of that name resolves.
    pub fn command_exists(&self, name: &str) -> bool {
        match CString::new(name) {
            Ok(name) => (self.t().command_exists)(self.api, name.as_ptr()) != 0,
            Err(_) => false,
        }
    }

    /// vim `getscriptinfo()` — how many native plugins are loaded.
    pub fn plugin_count(&self) -> usize {
        (self.t().plugin_count)(self.api)
    }

    /// One loaded plugin as `name version`, or `None` past the last.
    pub fn plugin_name(&self, index: usize) -> Option<String> {
        self.take_string((self.t().plugin_name)(self.api, index))
    }

    /// Every loaded plugin, as `name version`.
    pub fn plugin_names(&self) -> Vec<String> {
        (0..self.plugin_count())
            .filter_map(|i| self.plugin_name(i))
            .collect()
    }

    /// Adopt a host-allocated C string and release it through the host's own
    /// allocator, which is the only correct way to free one across the ABI.
    fn take_string(&self, raw: *mut c_char) -> Option<String> {
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
}

/// One diagnostic, as [`Host::diagnostic`] hands it over: where it is, what it
/// says, and its severity as `hint`, `info`, `warning` or `error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticInfo {
    pub span: Span,
    pub message: String,
    pub severity: String,
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
/// `#[no_mangle] extern "C" fn zmax_native_init` the host looks for, plus the
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
        pub extern "C" fn zmax_native_init(
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
// hard-codes so users need only `use zmax_native::*` or the two names above.
#[doc(hidden)]
pub const ABIVERSION_FOR_MACRO: u32 = ABI_VERSION;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ptr;

    // What the fake host was asked for, and what it hands back. A thread-local
    // rather than a field on the table, because the callbacks are plain
    // `extern "C"` fns with no closure environment -- the same constraint a real
    // host works under.
    thread_local! {
        static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn record(call: impl Into<String>) {
        CALLS.with(|calls| calls.borrow_mut().push(call.into()));
    }

    fn calls() -> Vec<String> {
        CALLS.with(|calls| calls.borrow().clone())
    }

    /// Hand back an owned C string the way the host does, so `free_cstring` has
    /// something real to release.
    fn reply(text: &str) -> *mut c_char {
        CString::new(text).unwrap().into_raw()
    }

    extern "C" fn fake_register_command(
        _h: *const HostApi,
        _name: *const c_char,
        _f: CommandFn,
    ) -> c_int {
        0
    }
    extern "C" fn fake_message(_h: *const HostApi, text: *const c_char) {
        record(format!("message:{}", unsafe { CStr::from_ptr(text) }.to_string_lossy()));
    }
    extern "C" fn fake_error(_h: *const HostApi, text: *const c_char) {
        record(format!("error:{}", unsafe { CStr::from_ptr(text) }.to_string_lossy()));
    }
    extern "C" fn fake_eval(_h: *const HostApi, line: *const c_char) -> c_int {
        record(format!("eval:{}", unsafe { CStr::from_ptr(line) }.to_string_lossy()));
        0
    }
    extern "C" fn fake_buffer_text(_h: *const HostApi) -> *mut c_char {
        reply("buffer")
    }
    extern "C" fn fake_insert_text(_h: *const HostApi, text: *const c_char) -> c_int {
        record(format!("insert:{}", unsafe { CStr::from_ptr(text) }.to_string_lossy()));
        0
    }
    extern "C" fn fake_free_cstring(_h: *const HostApi, s: *mut c_char) {
        record("free");
        if !s.is_null() {
            drop(unsafe { CString::from_raw(s) });
        }
    }
    extern "C" fn fake_cursor(_h: *const HostApi) -> Cursor {
        Cursor { line: 3, column: 7, offset: 42, valid: 1 }
    }
    extern "C" fn fake_word_at_cursor(_h: *const HostApi) -> *mut c_char {
        reply("UnixStream")
    }
    extern "C" fn fake_selection_text(_h: *const HostApi) -> *mut c_char {
        reply("selected")
    }
    extern "C" fn fake_line(_h: *const HostApi, line: usize) -> *mut c_char {
        // Two lines only, so the past-the-end case is reachable.
        if line < 2 { reply(&format!("line {line}")) } else { ptr::null_mut() }
    }
    extern "C" fn fake_line_count(_h: *const HostApi) -> usize {
        2
    }
    extern "C" fn fake_mode(_h: *const HostApi) -> *mut c_char {
        reply("normal")
    }
    extern "C" fn fake_cwd(_h: *const HostApi) -> *mut c_char {
        reply("/tmp/project")
    }
    extern "C" fn fake_buffer_path(_h: *const HostApi) -> *mut c_char {
        reply("/tmp/project/main.rs")
    }
    extern "C" fn fake_language(_h: *const HostApi) -> *mut c_char {
        reply("rust")
    }
    extern "C" fn fake_is_modified(_h: *const HostApi) -> c_int {
        1
    }
    extern "C" fn fake_register(_h: *const HostApi, name: c_char) -> *mut c_char {
        record(format!("register:{}", name as u8 as char));
        reply("yanked")
    }
    extern "C" fn fake_selection_count(_h: *const HostApi) -> usize {
        2
    }
    extern "C" fn fake_selection(_h: *const HostApi, index: usize) -> Span {
        match index {
            // A backwards selection: head before anchor.
            0 => Span { anchor: 10, head: 4, line: 1, valid: 1 },
            1 => Span { anchor: 20, head: 25, line: 2, valid: 1 },
            _ => Span { anchor: 0, head: 0, line: 0, valid: 0 },
        }
    }
    extern "C" fn fake_text_range(_h: *const HostApi, from: usize, to: usize) -> *mut c_char {
        reply(&format!("text[{from}..{to}]"))
    }
    extern "C" fn fake_buffer_count(_h: *const HostApi) -> usize {
        2
    }
    extern "C" fn fake_buffer_name(_h: *const HostApi, index: usize) -> *mut c_char {
        if index < 2 { reply(&format!("buf{index}")) } else { ptr::null_mut() }
    }
    extern "C" fn fake_diagnostic_count(_h: *const HostApi) -> usize {
        1
    }
    extern "C" fn fake_diagnostic(_h: *const HostApi, index: usize) -> Span {
        if index == 0 {
            Span { anchor: 5, head: 9, line: 2, valid: 1 }
        } else {
            Span { anchor: 0, head: 0, line: 0, valid: 0 }
        }
    }
    extern "C" fn fake_diagnostic_message(_h: *const HostApi, _index: usize) -> *mut c_char {
        reply("unused variable")
    }
    extern "C" fn fake_diagnostic_severity(_h: *const HostApi, _index: usize) -> *mut c_char {
        reply("warning")
    }
    extern "C" fn fake_option(_h: *const HostApi, name: *const c_char) -> *mut c_char {
        record(format!("option:{}", unsafe { CStr::from_ptr(name) }.to_string_lossy()));
        reply("4")
    }
    extern "C" fn fake_search_pattern(_h: *const HostApi) -> *mut c_char {
        reply("needle")
    }
    extern "C" fn fake_window_count(_h: *const HostApi) -> usize {
        3
    }
    extern "C" fn fake_window_view(_h: *const HostApi) -> Span {
        Span { anchor: 100, head: 400, line: 12, valid: 1 }
    }
    extern "C" fn fake_file_size(_h: *const HostApi, path: *const c_char) -> i64 {
        // A missing file is -1, which the wrapper must turn into `None` rather
        // than a colossal u64.
        if unsafe { CStr::from_ptr(path) }.to_string_lossy() == "/absent" {
            -1
        } else {
            1234
        }
    }
    extern "C" fn fake_file_type(_h: *const HostApi, _path: *const c_char) -> *mut c_char {
        reply("file")
    }
    extern "C" fn fake_file_time(_h: *const HostApi, path: *const c_char) -> i64 {
        if unsafe { CStr::from_ptr(path) }.to_string_lossy() == "/absent" {
            -1
        } else {
            1_700_000_000
        }
    }
    extern "C" fn fake_file_perm(_h: *const HostApi, _path: *const c_char) -> *mut c_char {
        reply("rw-r--r--")
    }
    extern "C" fn fake_buffer_line(
        _h: *const HostApi,
        buffer: usize,
        line: usize,
    ) -> *mut c_char {
        if buffer < 2 && line < 2 {
            reply(&format!("buf{buffer} line{line}"))
        } else {
            ptr::null_mut()
        }
    }
    extern "C" fn fake_byte_offset(_h: *const HostApi, char_offset: usize) -> usize {
        // A buffer of two-byte characters, so the two offsets cannot be
        // confused for one another.
        char_offset * 2
    }
    extern "C" fn fake_char_offset(_h: *const HostApi, byte_offset: usize) -> usize {
        byte_offset / 2
    }
    extern "C" fn fake_command_exists(_h: *const HostApi, name: *const c_char) -> c_int {
        let name = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
        record(format!("exists:{name}"));
        c_int::from(name == "write")
    }
    extern "C" fn fake_plugin_count(_h: *const HostApi) -> usize {
        1
    }
    extern "C" fn fake_plugin_name(_h: *const HostApi, index: usize) -> *mut c_char {
        if index == 0 { reply("hello 0.1.0") } else { ptr::null_mut() }
    }

    fn table() -> HostApi {
        HostApi {
            abi_version: ABI_VERSION,
            ctx: ptr::null_mut(),
            register_command: fake_register_command,
            message: fake_message,
            error: fake_error,
            eval: fake_eval,
            buffer_text: fake_buffer_text,
            insert_text: fake_insert_text,
            free_cstring: fake_free_cstring,
            cursor: fake_cursor,
            word_at_cursor: fake_word_at_cursor,
            selection_text: fake_selection_text,
            line: fake_line,
            line_count: fake_line_count,
            mode: fake_mode,
            cwd: fake_cwd,
            buffer_path: fake_buffer_path,
            language: fake_language,
            is_modified: fake_is_modified,
            register: fake_register,
            selection_count: fake_selection_count,
            selection: fake_selection,
            text_range: fake_text_range,
            buffer_count: fake_buffer_count,
            buffer_name: fake_buffer_name,
            diagnostic_count: fake_diagnostic_count,
            diagnostic: fake_diagnostic,
            diagnostic_message: fake_diagnostic_message,
            diagnostic_severity: fake_diagnostic_severity,
            option: fake_option,
            search_pattern: fake_search_pattern,
            window_count: fake_window_count,
            window_view: fake_window_view,
            file_size: fake_file_size,
            file_type: fake_file_type,
            file_time: fake_file_time,
            file_perm: fake_file_perm,
            buffer_line: fake_buffer_line,
            byte_offset: fake_byte_offset,
            char_offset: fake_char_offset,
            command_exists: fake_command_exists,
            plugin_count: fake_plugin_count,
            plugin_name: fake_plugin_name,
        }
    }

    fn host(api: &HostApi) -> Host {
        // Safe: `api` outlives the `Host` for the body of each test.
        unsafe { Host::from_raw(api as *const HostApi) }
    }

    /// Every accessor must read its OWN slot. The table is hand-wired, so a
    /// field pointed at the neighbouring function compiles perfectly and calls
    /// the wrong thing -- each fake returns a distinct value precisely so a
    /// swap shows up here rather than in someone's editor.
    #[test]
    fn every_wrapper_reads_its_own_slot() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.buffer_text().as_deref(), Some("buffer"));
        assert_eq!(host.word_at_cursor().as_deref(), Some("UnixStream"));
        assert_eq!(host.selection_text().as_deref(), Some("selected"));
        assert_eq!(host.mode().as_deref(), Some("normal"));
        assert_eq!(host.cwd().as_deref(), Some("/tmp/project"));
        assert_eq!(host.buffer_path().as_deref(), Some("/tmp/project/main.rs"));
        assert_eq!(host.language().as_deref(), Some("rust"));
        assert_eq!(host.search_pattern().as_deref(), Some("needle"));
        assert_eq!(host.file_type("/x").as_deref(), Some("file"));
        assert_eq!(host.file_perm("/x").as_deref(), Some("rw-r--r--"));
        assert_eq!(host.buffer_line(1, 0).as_deref(), Some("buf1 line0"));
        assert_eq!(host.plugin_name(0).as_deref(), Some("hello 0.1.0"));
        assert_eq!(host.plugin_count(), 1);

        assert_eq!(host.line_count(), 2);
        assert_eq!(host.selection_count(), 2);
        assert_eq!(host.buffer_count(), 2);
        assert_eq!(host.diagnostic_count(), 1);
        assert_eq!(host.window_count(), 3);
        assert!(host.is_modified());
    }

    /// `Cursor` and `Span` cross the ABI by value, so their fields have to
    /// arrive in the order they were sent -- a reordered struct would silently
    /// swap line for column.
    #[test]
    fn positions_survive_the_boundary_field_for_field() {
        let api = table();
        let host = host(&api);

        let cursor = host.cursor().expect("valid");
        assert_eq!((cursor.line, cursor.column, cursor.offset), (3, 7, 42));

        let window = host.window_view().expect("valid");
        assert_eq!((window.anchor, window.head, window.line), (100, 400, 12));
    }

    /// A backwards selection keeps its direction: `head` before `anchor` is how
    /// the editor says which end the user is extending from, and flattening it
    /// into document order would throw that away.
    #[test]
    fn a_backwards_selection_keeps_its_direction() {
        let api = table();
        let host = host(&api);

        let selections = host.selections();
        assert_eq!(selections.len(), 2);
        assert!(selections[0].head < selections[0].anchor, "backwards");
        assert!(selections[1].head > selections[1].anchor, "forwards");
    }

    /// `valid == 0` is how "no editor context" crosses an ABI that cannot carry
    /// `Option`, so it must become `None` rather than a zeroed position that
    /// reads as the top of the buffer.
    #[test]
    fn an_invalid_span_becomes_none() {
        let api = table();
        let host = host(&api);

        assert!(host.selection(2).is_none(), "past the last selection");
        assert!(host.diagnostic(1).is_none(), "past the last diagnostic");
        assert!(host.line(5).is_none(), "past the end of the buffer");
        assert!(host.buffer_name(9).is_none());
    }

    /// The wrappers own what the host allocates: every string read must be
    /// handed back through `free_cstring`, or a plugin leaks on every call.
    #[test]
    fn every_string_read_is_freed() {
        let api = table();
        let host = host(&api);
        CALLS.with(|calls| calls.borrow_mut().clear());

        host.buffer_text();
        host.word_at_cursor();
        host.mode();
        host.line(0);

        assert_eq!(
            calls().iter().filter(|c| *c == "free").count(),
            4,
            "one free per string read: {:?}",
            calls()
        );
    }

    /// A null return means "nothing", and nothing must not be freed.
    #[test]
    fn a_null_reply_is_none_and_is_not_freed() {
        let api = table();
        let host = host(&api);
        CALLS.with(|calls| calls.borrow_mut().clear());

        assert!(host.line(99).is_none());

        assert!(!calls().contains(&"free".to_string()), "{:?}", calls());
    }

    /// The arguments a wrapper takes reach the host unchanged.
    #[test]
    fn arguments_reach_the_host_intact() {
        let api = table();
        let host = host(&api);
        CALLS.with(|calls| calls.borrow_mut().clear());

        host.message("hello");
        host.error("bad");
        host.eval("write");
        host.insert_text("text");
        host.option("shiftwidth");
        host.register('a');

        assert_eq!(
            calls()
                .into_iter()
                .filter(|c| c != "free")
                .collect::<Vec<_>>(),
            vec![
                "message:hello",
                "error:bad",
                "eval:write",
                "insert:text",
                "option:shiftwidth",
                "register:a",
            ]
        );
    }

    /// `file_size` reports a missing file as -1 across the ABI, which must not
    /// come back as a `u64` near its maximum.
    #[test]
    fn a_missing_files_size_is_none_not_a_huge_number() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.file_size("/present"), Some(1234));
        assert_eq!(host.file_size("/absent"), None);
    }

    /// The byte and char offsets are inverses, and are not the same number --
    /// the whole reason both exist is that a language server counts bytes while
    /// this API counts characters.
    #[test]
    fn byte_and_char_offsets_convert_both_ways() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.byte_offset(10), 20);
        assert_eq!(host.char_offset(20), 10);
        assert_eq!(host.char_offset(host.byte_offset(7)), 7, "round trip");
    }

    /// `exists(":cmd")` answers for the name as typed, and the leading colon is
    /// the caller's to include or not.
    #[test]
    fn command_existence_is_reported_for_the_name_given() {
        let api = table();
        let host = host(&api);
        CALLS.with(|calls| calls.borrow_mut().clear());

        assert!(host.command_exists("write"));
        assert!(!host.command_exists("nosuchcommand"));
        assert_eq!(
            calls(),
            vec!["exists:write", "exists:nosuchcommand"],
            "the name reaches the host unchanged"
        );
    }

    /// A missing file has no time, and -1 must not come back as a date in the
    /// distant future.
    #[test]
    fn a_missing_files_time_is_none() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.file_time("/present"), Some(1_700_000_000));
        assert_eq!(host.file_time("/absent"), None);
    }

    /// A diagnostic arrives as one value: where, what, and how bad.
    #[test]
    fn a_diagnostic_arrives_whole() {
        let api = table();
        let host = host(&api);

        let diagnostics = host.diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "unused variable");
        assert_eq!(diagnostics[0].severity, "warning");
        assert_eq!((diagnostics[0].span.anchor, diagnostics[0].span.head), (5, 9));
    }

    /// `lines` stops at the buffer's end rather than running to the requested
    /// bound, so asking for more than exists is not an error.
    #[test]
    fn lines_are_clamped_to_the_buffer() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.lines(0, 99), vec!["line 0", "line 1"]);
        assert_eq!(host.lines(1, 2), vec!["line 1"]);
        assert!(host.lines(5, 9).is_empty());
    }
}
