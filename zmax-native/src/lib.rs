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

    /// vim `getpos("'{mark}")` — where a named mark is. `anchor` and `head` are
    /// both its char offset, since a mark is a point rather than a span, and
    /// `line` is the line it sits on. `valid` is 0 for a mark that has never
    /// been set. Marks are remapped through edits, so this follows the text it
    /// was placed on.
    pub mark: extern "C" fn(host: *const HostApi, name: c_char) -> Span,

    /// vim `getwininfo()[0].width` / `.height` — the current window's text area
    /// in cells, excluding gutters. What a plugin needs to lay anything out.
    pub window_width: extern "C" fn(host: *const HostApi) -> usize,
    pub window_height: extern "C" fn(host: *const HostApi) -> usize,

    /// vim `getcompletion({prefix}, "command")` — the `:`-command names that
    /// start with `prefix`, newline-separated and sorted, covering built-ins and
    /// plugin-registered commands alike. Null when nothing matches. Release with
    /// `free_cstring`.
    pub completions: extern "C" fn(host: *const HostApi, prefix: *const c_char) -> *mut c_char,

    /// vim `getmarklist()` — every set mark as `name:offset:line`, one per
    /// line, sorted by name. Null when none are set. This is the whole list;
    /// `mark` looks one up. Release with `free_cstring`.
    pub marks: extern "C" fn(host: *const HostApi) -> *mut c_char,

    /// vim `getchangelist()` — how many edit positions the changelist holds.
    pub changelist_count: extern "C" fn(host: *const HostApi) -> usize,
    /// The `index`th changelist entry, oldest first. See [`Span`]; `valid` is 0
    /// past the last one.
    pub changelist: extern "C" fn(host: *const HostApi, index: usize) -> Span,
    /// Where `g;`/`g,` would resume in the changelist — vim's changelist index.
    pub changelist_index: extern "C" fn(host: *const HostApi) -> usize,

    /// vim `strdisplaywidth({text})` — how many terminal cells `text` occupies,
    /// which is not its length in characters: a CJK glyph takes two and a
    /// combining mark none. Pairs with `window_width` for anything laying text
    /// out.
    pub display_width: extern "C" fn(host: *const HostApi, text: *const c_char) -> usize,

    /// vim `getbufinfo()[i].name` — the `index`th buffer's absolute path, or
    /// null for a scratch buffer or past the last. `buffer_name` gives the
    /// display name; this is the path on disk. Release with `free_cstring`.
    pub buffer_path_at: extern "C" fn(host: *const HostApi, index: usize) -> *mut c_char,
    /// vim `getbufinfo()[i].changed` — 1 when the `index`th buffer has unsaved
    /// changes. A buffer list that cannot show which entries are dirty is not
    /// much of a buffer list.
    pub buffer_modified: extern "C" fn(host: *const HostApi, index: usize) -> c_int,
    /// vim `winnr()` — which window is focused, as an index into the same order
    /// `window_count` walks.
    pub window_index: extern "C" fn(host: *const HostApi) -> usize,

    /// vim `col("$")` — the length of a line in characters, its line ending
    /// excluded. `usize::MAX` when the line is past the end of the buffer, so
    /// it is distinguishable from an empty line.
    pub line_length: extern "C" fn(host: *const HostApi, line: usize) -> usize,
    /// vim `indent({lnum})` — a line's indentation in columns, counting a tab
    /// as `tabstop` columns rather than as one character.
    pub indent: extern "C" fn(host: *const HostApi, line: usize) -> usize,
    /// vim `wordcount()` — characters, words and lines in the current buffer,
    /// as `chars:words:lines`. One call rather than three, since a plugin
    /// showing a count wants all of them. Release with `free_cstring`.
    pub word_count: extern "C" fn(host: *const HostApi) -> *mut c_char,

    /// vim `&{option}` read as a number, or `usize::MAX` when the option is
    /// unset or is not numeric. `option` returns the raw string; this saves
    /// every caller parsing `shiftwidth` by hand.
    pub option_num: extern "C" fn(host: *const HostApi, name: *const c_char) -> usize,
    /// vim `&{option}` read as a boolean, the way `:set` understands one.
    pub option_bool: extern "C" fn(host: *const HostApi, name: *const c_char) -> c_int,

    /// vim `fnamemodify({fname}, {mods})` — apply path modifiers, left to
    /// right: `:p` absolute, `:h` head (the directory), `:t` tail (the file
    /// name), `:r` root (the name without its extension), `:e` the extension
    /// alone. `:p:h` is the containing directory of an absolute path, which is
    /// the combination most callers actually want. Null on an unknown modifier
    /// rather than silently ignoring it. Release with `free_cstring`.
    pub fname_modify: extern "C" fn(
        host: *const HostApi,
        path: *const c_char,
        mods: *const c_char,
    ) -> *mut c_char,
    /// vim `isdirectory({path})`.
    pub is_directory: extern "C" fn(host: *const HostApi, path: *const c_char) -> c_int,
    /// vim `filereadable({path})` — a readable regular file, so a directory is
    /// 0 even though it can be opened.
    pub file_readable: extern "C" fn(host: *const HostApi, path: *const c_char) -> c_int,
    /// vim `filewritable({path})` — 0 not writable, 1 a writable file, 2 a
    /// writable directory, exactly as vim's three-way answer.
    pub file_writable: extern "C" fn(host: *const HostApi, path: *const c_char) -> c_int,

    /// vim `line2byte({lnum})` — the byte offset a line starts at, or
    /// `usize::MAX` past the end of the buffer.
    pub line_to_byte: extern "C" fn(host: *const HostApi, line: usize) -> usize,
    /// vim `byte2line({byte})` — the line a byte offset falls on, clamped.
    pub byte_to_line: extern "C" fn(host: *const HostApi, byte: usize) -> usize,

    /// vim `getenv({name})` — an environment variable, or null when unset. The
    /// editor's environment, which is what a plugin's own `std::env` would see
    /// too; here for the `get*` family's sake.
    pub env: extern "C" fn(host: *const HostApi, name: *const c_char) -> *mut c_char,

    /// vim `bufnr({name})` — the index of the first buffer whose display name
    /// contains `name`, or `usize::MAX` when none does. Substring matching, as
    /// vim's does, so `bufnr("main")` finds `src/main.rs`.
    pub buffer_index: extern "C" fn(host: *const HostApi, name: *const c_char) -> usize,
    /// vim `winbufnr({win})` — which buffer the `index`th window is showing, as
    /// an index into the `buffer_name` order. `usize::MAX` past the last window.
    pub window_buffer: extern "C" fn(host: *const HostApi, index: usize) -> usize,

    /// vim `foldlevel({lnum})` — how deeply a line is folded, 0 when it is not
    /// inside any fold.
    pub fold_level: extern "C" fn(host: *const HostApi, line: usize) -> usize,
    /// vim `foldclosed({lnum})` — the first line of the closed fold containing
    /// this line, or `usize::MAX` when the line is not inside a closed one.
    pub fold_closed: extern "C" fn(host: *const HostApi, line: usize) -> usize,

    /// vim `searchcount()` — how many times `pattern` matches the current
    /// buffer. The pattern is a Rust regex; an invalid one counts zero rather
    /// than failing the call, since a plugin building a pattern from user input
    /// should not have to pre-validate it.
    pub search_count: extern "C" fn(host: *const HostApi, pattern: *const c_char) -> usize,
    /// Where `pattern` first matches at or after `from`, as a [`Span`] over the
    /// match. `valid` is 0 when it does not match again.
    pub search_next:
        extern "C" fn(host: *const HostApi, pattern: *const c_char, from: usize) -> Span,

    /// vim `getpid()` — the editor's process id. A plugin shares the process,
    /// so this is its own pid too; here so a plugin naming a temp file after
    /// the editor does not have to reach for `std::process`.
    pub pid: extern "C" fn(host: *const HostApi) -> u32,
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

    /// vim `getpos("'{mark}")` — where a named mark is, or `None` when it has
    /// never been set.
    pub fn mark(&self, name: char) -> Option<Span> {
        if !name.is_ascii() {
            return None;
        }
        let span = (self.t().mark)(self.api, name as c_char);
        (span.valid != 0).then_some(span)
    }

    /// The current window's text area in cells, gutters excluded.
    pub fn window_size(&self) -> (usize, usize) {
        (
            (self.t().window_width)(self.api),
            (self.t().window_height)(self.api),
        )
    }

    /// vim `getcompletion({prefix}, "command")` — the `:`-command names
    /// starting with `prefix`, sorted, built-in and plugin alike.
    pub fn completions(&self, prefix: &str) -> Vec<String> {
        let Ok(prefix) = CString::new(prefix) else {
            return Vec::new();
        };
        self.take_string((self.t().completions)(self.api, prefix.as_ptr()))
            .map(|joined| joined.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// vim `getmarklist()` — every set mark, as `(name, offset, line)`, sorted
    /// by name. A malformed row is skipped rather than failing the list.
    pub fn marks(&self) -> Vec<(char, usize, usize)> {
        let Some(joined) = self.take_string((self.t().marks)(self.api)) else {
            return Vec::new();
        };
        joined
            .lines()
            .filter_map(|row| {
                let mut parts = row.splitn(3, ':');
                let name = parts.next()?.chars().next()?;
                let offset = parts.next()?.parse().ok()?;
                let line = parts.next()?.parse().ok()?;
                Some((name, offset, line))
            })
            .collect()
    }

    /// vim `getchangelist()` — every edit position, oldest first.
    pub fn changelist(&self) -> Vec<Span> {
        (0..(self.t().changelist_count)(self.api))
            .filter_map(|i| {
                let span = (self.t().changelist)(self.api, i);
                (span.valid != 0).then_some(span)
            })
            .collect()
    }

    /// Where `g;`/`g,` would resume in the changelist.
    pub fn changelist_index(&self) -> usize {
        (self.t().changelist_index)(self.api)
    }

    /// vim `strdisplaywidth({text})` — terminal cells, not characters. A CJK
    /// glyph is two cells and a combining mark none, so laying anything out
    /// from `text.len()` misaligns it.
    pub fn display_width(&self, text: &str) -> usize {
        match CString::new(text) {
            Ok(text) => (self.t().display_width)(self.api, text.as_ptr()),
            Err(_) => 0,
        }
    }

    /// The `index`th buffer's path on disk, or `None` for a scratch buffer.
    pub fn buffer_path_at(&self, index: usize) -> Option<String> {
        self.take_string((self.t().buffer_path_at)(self.api, index))
    }

    /// vim `getbufinfo()[i].changed` — whether that buffer has unsaved changes.
    pub fn buffer_modified(&self, index: usize) -> bool {
        (self.t().buffer_modified)(self.api, index) != 0
    }

    /// vim `winnr()` — which window is focused.
    pub fn window_index(&self) -> usize {
        (self.t().window_index)(self.api)
    }

    /// vim `col("$")` — a line's length in characters, its line ending
    /// excluded. `None` past the end of the buffer.
    pub fn line_length(&self, line: usize) -> Option<usize> {
        match (self.t().line_length)(self.api, line) {
            usize::MAX => None,
            length => Some(length),
        }
    }

    /// vim `indent({lnum})` — a line's indentation in columns, a tab counting
    /// as `tabstop` columns.
    pub fn indent(&self, line: usize) -> usize {
        (self.t().indent)(self.api, line)
    }

    /// vim `wordcount()` — `(chars, words, lines)` for the current buffer.
    pub fn word_count(&self) -> Option<(usize, usize, usize)> {
        let counts = self.take_string((self.t().word_count)(self.api))?;
        let mut parts = counts.splitn(3, ':');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ))
    }

    /// vim `&{option}` as a number, or `None` when unset or not numeric.
    pub fn option_num(&self, name: &str) -> Option<usize> {
        let name = CString::new(name).ok()?;
        match (self.t().option_num)(self.api, name.as_ptr()) {
            usize::MAX => None,
            value => Some(value),
        }
    }

    /// vim `&{option}` as a boolean, the way `:set` reads one.
    pub fn option_bool(&self, name: &str) -> bool {
        match CString::new(name) {
            Ok(name) => (self.t().option_bool)(self.api, name.as_ptr()) != 0,
            Err(_) => false,
        }
    }

    /// vim `fnamemodify({fname}, {mods})` — `:p`, `:h`, `:t`, `:r`, `:e`,
    /// applied left to right. `None` on an unknown modifier.
    pub fn fname_modify(&self, path: &str, mods: &str) -> Option<String> {
        let path = CString::new(path).ok()?;
        let mods = CString::new(mods).ok()?;
        self.take_string((self.t().fname_modify)(
            self.api,
            path.as_ptr(),
            mods.as_ptr(),
        ))
    }

    /// vim `isdirectory({path})`.
    pub fn is_directory(&self, path: &str) -> bool {
        match CString::new(path) {
            Ok(path) => (self.t().is_directory)(self.api, path.as_ptr()) != 0,
            Err(_) => false,
        }
    }

    /// vim `filereadable({path})` — a readable regular file, not a directory.
    pub fn file_readable(&self, path: &str) -> bool {
        match CString::new(path) {
            Ok(path) => (self.t().file_readable)(self.api, path.as_ptr()) != 0,
            Err(_) => false,
        }
    }

    /// vim `filewritable({path})` — 0 not writable, 1 a file, 2 a directory.
    pub fn file_writable(&self, path: &str) -> i32 {
        match CString::new(path) {
            Ok(path) => (self.t().file_writable)(self.api, path.as_ptr()),
            Err(_) => 0,
        }
    }

    /// vim `line2byte({lnum})` — where a line starts in bytes. `None` past the
    /// end of the buffer.
    pub fn line_to_byte(&self, line: usize) -> Option<usize> {
        match (self.t().line_to_byte)(self.api, line) {
            usize::MAX => None,
            byte => Some(byte),
        }
    }

    /// vim `byte2line({byte})` — the line a byte offset falls on.
    pub fn byte_to_line(&self, byte: usize) -> usize {
        (self.t().byte_to_line)(self.api, byte)
    }

    /// vim `getenv({name})` — the editor's environment.
    pub fn env(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        self.take_string((self.t().env)(self.api, name.as_ptr()))
    }

    /// vim `bufnr({name})` — the first buffer whose display name contains
    /// `name`. `None` when none does.
    pub fn buffer_index(&self, name: &str) -> Option<usize> {
        let name = CString::new(name).ok()?;
        match (self.t().buffer_index)(self.api, name.as_ptr()) {
            usize::MAX => None,
            index => Some(index),
        }
    }

    /// vim `winbufnr({win})` — which buffer a window is showing. `None` past
    /// the last window.
    pub fn window_buffer(&self, index: usize) -> Option<usize> {
        match (self.t().window_buffer)(self.api, index) {
            usize::MAX => None,
            buffer => Some(buffer),
        }
    }

    /// vim `foldlevel({lnum})` — 0 when the line is not inside a fold.
    pub fn fold_level(&self, line: usize) -> usize {
        (self.t().fold_level)(self.api, line)
    }

    /// vim `foldclosed({lnum})` — the first line of the closed fold containing
    /// this line, or `None` when it is not inside a closed one.
    pub fn fold_closed(&self, line: usize) -> Option<usize> {
        match (self.t().fold_closed)(self.api, line) {
            usize::MAX => None,
            line => Some(line),
        }
    }

    /// vim `searchcount()` — how many times a regex matches the buffer. An
    /// invalid pattern counts zero rather than erroring.
    pub fn search_count(&self, pattern: &str) -> usize {
        match CString::new(pattern) {
            Ok(pattern) => (self.t().search_count)(self.api, pattern.as_ptr()),
            Err(_) => 0,
        }
    }

    /// Where a regex first matches at or after `from`. `None` when it does not
    /// match again.
    pub fn search_next(&self, pattern: &str, from: usize) -> Option<Span> {
        let pattern = CString::new(pattern).ok()?;
        let span = (self.t().search_next)(self.api, pattern.as_ptr(), from);
        (span.valid != 0).then_some(span)
    }

    /// vim `getpid()` — the editor's process id, which is the plugin's too.
    pub fn pid(&self) -> u32 {
        (self.t().pid)(self.api)
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
    extern "C" fn fake_mark(_h: *const HostApi, name: c_char) -> Span {
        if name as u8 as char == 'a' {
            Span { anchor: 15, head: 15, line: 4, valid: 1 }
        } else {
            Span { anchor: 0, head: 0, line: 0, valid: 0 }
        }
    }
    extern "C" fn fake_window_width(_h: *const HostApi) -> usize {
        // Deliberately different from the height, so a wrapper reading the
        // wrong slot cannot pass.
        80
    }
    extern "C" fn fake_window_height(_h: *const HostApi) -> usize {
        24
    }
    extern "C" fn fake_buffer_index(_h: *const HostApi, name: *const c_char) -> usize {
        match unsafe { CStr::from_ptr(name) }.to_string_lossy().as_ref() {
            "main" => 1,
            _ => usize::MAX,
        }
    }
    extern "C" fn fake_window_buffer(_h: *const HostApi, index: usize) -> usize {
        // Window 0 shows buffer 1, so an implementation returning the window
        // index cannot pass.
        match index {
            0 => 1,
            1 => 0,
            _ => usize::MAX,
        }
    }
    extern "C" fn fake_fold_level(_h: *const HostApi, line: usize) -> usize {
        match line {
            5 => 2,
            4 => 1,
            _ => 0,
        }
    }
    extern "C" fn fake_fold_closed(_h: *const HostApi, line: usize) -> usize {
        if line == 5 { 3 } else { usize::MAX }
    }
    extern "C" fn fake_search_count(_h: *const HostApi, pattern: *const c_char) -> usize {
        match unsafe { CStr::from_ptr(pattern) }.to_string_lossy().as_ref() {
            "fn " => 7,
            "((" => 0, // an invalid regex counts zero rather than erroring
            _ => 0,
        }
    }
    extern "C" fn fake_search_next(
        _h: *const HostApi,
        pattern: *const c_char,
        from: usize,
    ) -> Span {
        let pattern = unsafe { CStr::from_ptr(pattern) }.to_string_lossy().into_owned();
        record(format!("search:{pattern}:{from}"));
        if pattern == "fn " && from < 100 {
            Span { anchor: 100, head: 103, line: 12, valid: 1 }
        } else {
            Span { anchor: 0, head: 0, line: 0, valid: 0 }
        }
    }
    extern "C" fn fake_pid(_h: *const HostApi) -> u32 {
        4242
    }
    extern "C" fn fake_fname_modify(
        _h: *const HostApi,
        path: *const c_char,
        mods: *const c_char,
    ) -> *mut c_char {
        let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned();
        let mods = unsafe { CStr::from_ptr(mods) }.to_string_lossy().into_owned();
        record(format!("fnamemodify:{path}:{mods}"));
        // An unknown modifier is refused, which the wrapper must surface as
        // None rather than as the untouched path.
        if mods.contains('z') {
            ptr::null_mut()
        } else {
            reply(&format!("{path}{mods}"))
        }
    }
    extern "C" fn fake_is_directory(_h: *const HostApi, path: *const c_char) -> c_int {
        c_int::from(unsafe { CStr::from_ptr(path) }.to_string_lossy() == "/tmp")
    }
    extern "C" fn fake_file_readable(_h: *const HostApi, path: *const c_char) -> c_int {
        c_int::from(unsafe { CStr::from_ptr(path) }.to_string_lossy() == "/tmp/a.rs")
    }
    extern "C" fn fake_file_writable(_h: *const HostApi, path: *const c_char) -> c_int {
        // vim's three-way answer, so a bool wrapper would lose the distinction.
        match unsafe { CStr::from_ptr(path) }.to_string_lossy().as_ref() {
            "/tmp" => 2,
            "/tmp/a.rs" => 1,
            _ => 0,
        }
    }
    extern "C" fn fake_line_to_byte(_h: *const HostApi, line: usize) -> usize {
        if line < 2 { line * 12 } else { usize::MAX }
    }
    extern "C" fn fake_byte_to_line(_h: *const HostApi, byte: usize) -> usize {
        byte / 12
    }
    extern "C" fn fake_env(_h: *const HostApi, name: *const c_char) -> *mut c_char {
        if unsafe { CStr::from_ptr(name) }.to_string_lossy() == "EDITOR" {
            reply("zmax")
        } else {
            ptr::null_mut()
        }
    }
    extern "C" fn fake_buffer_path_at(_h: *const HostApi, index: usize) -> *mut c_char {
        // The second buffer is a scratch one, so the no-path case is reachable.
        if index == 0 { reply("/tmp/a.rs") } else { ptr::null_mut() }
    }
    extern "C" fn fake_buffer_modified(_h: *const HostApi, index: usize) -> c_int {
        c_int::from(index == 1)
    }
    extern "C" fn fake_window_index(_h: *const HostApi) -> usize {
        2
    }
    extern "C" fn fake_line_length(_h: *const HostApi, line: usize) -> usize {
        match line {
            0 => 11,
            1 => 0, // an empty line, which must not read as "past the end"
            _ => usize::MAX,
        }
    }
    extern "C" fn fake_indent(_h: *const HostApi, _line: usize) -> usize {
        8
    }
    extern "C" fn fake_word_count(_h: *const HostApi) -> *mut c_char {
        reply("120:20:5")
    }
    extern "C" fn fake_option_num(_h: *const HostApi, name: *const c_char) -> usize {
        if unsafe { CStr::from_ptr(name) }.to_string_lossy() == "shiftwidth" {
            4
        } else {
            usize::MAX
        }
    }
    extern "C" fn fake_option_bool(_h: *const HostApi, name: *const c_char) -> c_int {
        c_int::from(unsafe { CStr::from_ptr(name) }.to_string_lossy() == "expandtab")
    }
    extern "C" fn fake_marks(_h: *const HostApi) -> *mut c_char {
        // Including a row that cannot be parsed, so the reader has to skip it
        // rather than give up on the whole list.
        reply("a:15:4\nb:99:20\nbroken")
    }
    extern "C" fn fake_changelist_count(_h: *const HostApi) -> usize {
        2
    }
    extern "C" fn fake_changelist(_h: *const HostApi, index: usize) -> Span {
        match index {
            0 => Span { anchor: 3, head: 3, line: 0, valid: 1 },
            1 => Span { anchor: 40, head: 40, line: 9, valid: 1 },
            _ => Span { anchor: 0, head: 0, line: 0, valid: 0 },
        }
    }
    extern "C" fn fake_changelist_index(_h: *const HostApi) -> usize {
        1
    }
    extern "C" fn fake_display_width(_h: *const HostApi, text: *const c_char) -> usize {
        // Two cells per char, so a caller using `len()` instead cannot pass.
        unsafe { CStr::from_ptr(text) }.to_string_lossy().chars().count() * 2
    }
    extern "C" fn fake_completions(_h: *const HostApi, prefix: *const c_char) -> *mut c_char {
        let prefix = unsafe { CStr::from_ptr(prefix) }.to_string_lossy().into_owned();
        record(format!("completions:{prefix}"));
        if prefix == "wr" { reply("write\nwrite-all\nwrite-quit") } else { ptr::null_mut() }
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
            mark: fake_mark,
            window_width: fake_window_width,
            window_height: fake_window_height,
            completions: fake_completions,
            marks: fake_marks,
            changelist_count: fake_changelist_count,
            changelist: fake_changelist,
            changelist_index: fake_changelist_index,
            display_width: fake_display_width,
            buffer_path_at: fake_buffer_path_at,
            buffer_modified: fake_buffer_modified,
            window_index: fake_window_index,
            line_length: fake_line_length,
            indent: fake_indent,
            word_count: fake_word_count,
            option_num: fake_option_num,
            option_bool: fake_option_bool,
            fname_modify: fake_fname_modify,
            is_directory: fake_is_directory,
            file_readable: fake_file_readable,
            file_writable: fake_file_writable,
            line_to_byte: fake_line_to_byte,
            byte_to_line: fake_byte_to_line,
            env: fake_env,
            buffer_index: fake_buffer_index,
            window_buffer: fake_window_buffer,
            fold_level: fake_fold_level,
            fold_closed: fake_fold_closed,
            search_count: fake_search_count,
            search_next: fake_search_next,
            pid: fake_pid,
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

    /// A mark is a point, so both ends of its span are the same offset -- and an
    /// unset mark is `None`, not position zero, which would silently mean the
    /// top of the buffer.
    #[test]
    fn a_mark_is_a_point_and_an_unset_one_is_none() {
        let api = table();
        let host = host(&api);

        let mark = host.mark('a').expect("set");
        assert_eq!((mark.anchor, mark.head, mark.line), (15, 15, 4));
        assert!(host.mark('z').is_none(), "never set");
    }

    /// Width and height come from their own slots. The fake returns 80x24
    /// precisely so a wrapper reading one for the other fails here.
    #[test]
    fn window_width_and_height_are_not_interchangeable() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.window_size(), (80, 24));
    }

    /// Completions arrive newline-separated and come back as a list; nothing
    /// matching is an empty list rather than one empty string.
    #[test]
    fn completions_split_into_a_list() {
        let api = table();
        let host = host(&api);

        assert_eq!(
            host.completions("wr"),
            vec!["write", "write-all", "write-quit"]
        );
        assert!(host.completions("zzz").is_empty(), "nothing matches");
    }

    /// The mark list parses into name/offset/line, and one malformed row is
    /// skipped rather than discarding every other mark with it.
    #[test]
    fn the_mark_list_skips_a_row_it_cannot_read() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.marks(), vec![('a', 15, 4), ('b', 99, 20)]);
    }

    /// The changelist comes back oldest first, and the index says where `g;`
    /// would resume -- which is not the same as its length.
    #[test]
    fn the_changelist_reports_its_entries_and_its_cursor() {
        let api = table();
        let host = host(&api);

        let changes = host.changelist();
        assert_eq!(changes.len(), 2);
        assert_eq!((changes[0].anchor, changes[0].line), (3, 0));
        assert_eq!((changes[1].anchor, changes[1].line), (40, 9));
        assert_eq!(host.changelist_index(), 1);
    }

    /// Display width is cells, not characters. The fake returns two cells per
    /// char precisely so a wrapper handing back `len()` cannot pass.
    #[test]
    fn display_width_is_cells_not_characters() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.display_width("abc"), 6);
        assert_eq!(host.display_width(""), 0);
    }

    /// An empty line has length 0 and a line past the end has none at all --
    /// collapsing the two would make the end of the buffer invisible to a
    /// caller walking lines.
    #[test]
    fn an_empty_line_is_not_the_end_of_the_buffer() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.line_length(0), Some(11));
        assert_eq!(host.line_length(1), Some(0), "empty, but a real line");
        assert_eq!(host.line_length(9), None, "past the end");
    }

    /// A buffer list needs the path and the dirty flag per entry, and a scratch
    /// buffer legitimately has no path.
    #[test]
    fn buffer_entries_carry_a_path_and_a_dirty_flag() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.buffer_path_at(0).as_deref(), Some("/tmp/a.rs"));
        assert!(host.buffer_path_at(1).is_none(), "a scratch buffer");
        assert!(!host.buffer_modified(0));
        assert!(host.buffer_modified(1));
        assert_eq!(host.window_index(), 2);
    }

    /// The typed option readers exist so callers do not parse `&opt` by hand,
    /// and an unset numeric option is `None` rather than 0 -- which is a
    /// perfectly valid `shiftwidth`.
    #[test]
    fn typed_options_report_unset_as_none() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.option_num("shiftwidth"), Some(4));
        assert_eq!(host.option_num("nosuchoption"), None, "not 0");
        assert!(host.option_bool("expandtab"));
        assert!(!host.option_bool("nosuchoption"));
    }

    /// `wordcount()` comes back as one value with all three counts.
    #[test]
    fn the_word_count_carries_chars_words_and_lines() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.word_count(), Some((120, 20, 5)));
        assert_eq!(host.indent(3), 8);
    }

    /// The path and the modifiers both reach the host, and a refused modifier
    /// comes back as `None` rather than as the path unchanged -- returning the
    /// input would look like a successful no-op.
    #[test]
    fn path_modifiers_reach_the_host_and_a_refusal_is_none() {
        let api = table();
        let host = host(&api);
        CALLS.with(|calls| calls.borrow_mut().clear());

        assert_eq!(
            host.fname_modify("/tmp/a.rs", ":p:h").as_deref(),
            Some("/tmp/a.rs:p:h")
        );
        assert!(host.fname_modify("/tmp/a.rs", ":z").is_none(), "refused");
        assert_eq!(
            calls().into_iter().filter(|c| c != "free").collect::<Vec<_>>(),
            vec!["fnamemodify:/tmp/a.rs::p:h", "fnamemodify:/tmp/a.rs::z"]
        );
    }

    /// `filewritable` is vim's three-way answer, so it stays an integer: 2 for a
    /// directory and 1 for a file are different facts, and a bool would lose it.
    #[test]
    fn file_predicates_keep_vims_distinctions() {
        let api = table();
        let host = host(&api);

        assert!(host.is_directory("/tmp"));
        assert!(!host.is_directory("/tmp/a.rs"));
        // A directory opens, but `filereadable` is about regular files.
        assert!(host.file_readable("/tmp/a.rs"));
        assert!(!host.file_readable("/tmp"));

        assert_eq!(host.file_writable("/tmp"), 2, "a directory");
        assert_eq!(host.file_writable("/tmp/a.rs"), 1, "a file");
        assert_eq!(host.file_writable("/nope"), 0);
    }

    /// Line and byte offsets convert both ways, and a line past the end has no
    /// byte offset rather than one of zero.
    #[test]
    fn lines_and_bytes_convert_both_ways() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.line_to_byte(0), Some(0));
        assert_eq!(host.line_to_byte(1), Some(12));
        assert_eq!(host.line_to_byte(7), None, "past the end");
        assert_eq!(host.byte_to_line(12), 1);
        assert_eq!(host.byte_to_line(13), 1, "mid-line");
    }

    /// An unset variable is `None`, not an empty string, which a caller would
    /// otherwise treat as a set-but-empty value.
    #[test]
    fn an_unset_environment_variable_is_none() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.env("EDITOR").as_deref(), Some("zmax"));
        assert!(host.env("NOSUCHVAR").is_none());
    }

    /// A buffer that is not open is `None`, not buffer 0 -- which is a real
    /// buffer a caller would then act on.
    #[test]
    fn a_missing_buffer_is_none_not_buffer_zero() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.buffer_index("main"), Some(1));
        assert_eq!(host.buffer_index("nosuchfile"), None);
        // Window 0 shows buffer 1: the two indices are different things.
        assert_eq!(host.window_buffer(0), Some(1));
        assert_eq!(host.window_buffer(9), None);
    }

    /// A line outside every fold has level 0 and no closed fold, which are
    /// different answers from "folded at depth 0" and "closed at line 0".
    #[test]
    fn fold_queries_distinguish_unfolded_from_folded_at_zero() {
        let api = table();
        let host = host(&api);

        assert_eq!(host.fold_level(5), 2, "nested");
        assert_eq!(host.fold_level(4), 1);
        assert_eq!(host.fold_level(0), 0, "not folded");

        assert_eq!(host.fold_closed(5), Some(3), "the fold starts at 3");
        assert!(host.fold_closed(0).is_none(), "not inside a closed fold");
    }

    /// The pattern and the starting offset both reach the host, and no further
    /// match is `None` rather than a zeroed span at the top of the buffer.
    #[test]
    fn a_search_carries_its_pattern_and_start() {
        let api = table();
        let host = host(&api);
        CALLS.with(|calls| calls.borrow_mut().clear());

        let hit = host.search_next("fn ", 0).expect("matches");
        assert_eq!((hit.anchor, hit.head, hit.line), (100, 103, 12));
        assert!(host.search_next("fn ", 500).is_none(), "nothing after 500");

        assert_eq!(
            calls().into_iter().filter(|c| c != "free").collect::<Vec<_>>(),
            vec!["search:fn :0", "search:fn :500"]
        );

        assert_eq!(host.search_count("fn "), 7);
        assert_eq!(host.search_count("(("), 0, "an invalid pattern counts zero");
        assert_eq!(host.pid(), 4242);
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
