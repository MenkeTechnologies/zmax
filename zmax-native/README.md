# zmax-native

Stable C-ABI SDK for writing **native (compiled Rust) plugins** for the zmax
editor. A plugin is an ordinary `cdylib` that zmax `dlopen`s at runtime via
`:zmax-native load <path>` — no editor recompile, no script glue. Each plugin
registers **typable commands** (the editor's `:`-commands) that resolve like the
built-in ones.

The host↔plugin boundary is a hand-rolled, versioned C ABI (`#[repr(C)]` structs
+ `extern "C"` fn pointers). Both the editor and the plugin depend on this crate
so they agree on the exact layout; nothing about Rust's unstable `repr(Rust)`
layout, allocator, or panic ABI crosses the boundary — only C-representable data.
The host refuses to load a plugin whose `ABI_VERSION` does not match its own.

## Writing a plugin

`Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
zmax-native = "0.4"
```

`src/lib.rs`:

```rust
use std::os::raw::c_int;
use zmax_native::{declare_plugin, Args, Host};

fn hello(host: &Host, args: &Args) -> c_int {
    host.message(&format!("hello, {}", args.rest().join(" ")));
    host.insert_text("greetings\n"); // undoable buffer edit
    0
}

declare_plugin! {
    name: "hello",
    version: "0.1.0",
    commands: { "hello" => hello },
}
```

`cargo build` produces `libhello.dylib` / `libhello.so`. Then inside zmax:

```text
:zmax-native load ~/plugins/libhello.dylib
:hello world
:zmax-native list
:zmax-native unload hello
```

Complete, buildable examples live in [`examples/`](examples) — `hello-plugin`,
`insert-date`, `buffer-stats`, `trim-trailing`, and `banner`, each exercising a
different part of the host API. See [`examples/README.md`](examples/README.md).

## The host API

Each command handler receives a [`Host`] (the editor callback table) and [`Args`]
(the argument vector, `argv[0]` = command name). `Host` exposes:

| method | effect |
|---|---|
| `register_command(name, handler)` | register a `:`-command (usually via the macro) |
| `message(text)` | show `text` on the status line |
| `error(text)` | show `text` on the status line, styled as an error |
| `eval(line)` | run a `:` command line, returns its exit status |
| `buffer_text()` | read the current buffer's full text |
| `insert_text(text)` | insert at the primary cursor (one undoable transaction) |

Where the cursor is and what is around it:

| method | effect |
|---|---|
| `cursor()` | `Cursor { line, column, offset }`, zero-based; `None` with no editor context |
| `word_at_cursor()` | the word under the cursor, as `miw` selects it; `None` on whitespace |
| `selection_text()` | the primary selection's text |
| `selection_count()` | how many selections there are — zmax is multi-selection |

Named after the vim `get*` functions they correspond to, so the mapping is
obvious. Note that line numbers here are **zero-based**, unlike vim's, to agree
with `Cursor::line`:

| method | vim | effect |
|---|---|---|
| `line(n)` | `getline({lnum})` | one line, without its line ending |
| `lines(start, end)` | `getline({start}, {end})` | a range, end-exclusive, clamped |
| `line_count()` | `line("$")` | lines in the buffer |
| `mode()` | `mode()` | `normal`, `insert` or `select` |
| `cwd()` | `getcwd()` | the editor's working directory |
| `buffer_path()` | `expand("%:p")` | `None` for an unwritten scratch buffer |
| `language()` | `&filetype` | the `languages.toml` language name |
| `is_modified()` | `&modified` | unsaved changes |
| `register(c)` | `getreg({regname})` | values joined with newlines, as vim renders a list register |

What is in the buffer, and what the language servers said about it:

| method | vim | effect |
|---|---|---|
| `selection(i)` / `selections()` | `getpos("'<")`, `getpos("'>")` | one selection as a `Span { anchor, head, line }` |
| `text_range(from, to)` | — | the text a `Span` addresses; clamped and ordered |
| `buffer_count()` / `buffer_name(i)` / `buffer_names()` | `getbufinfo()` | the open buffers |
| `diagnostic_count()` / `diagnostic(i)` / `diagnostics()` | `getqflist()` | position, message and severity |
| `option(name)` | `&{option}` | by long or short name, as on `:set` |
| `search_pattern()` | `getreg("/")` | the last search |
| `window_count()` | `getwininfo()` | open splits |
| `window_view()` | `winline()` | the first and last line the window shows |
| `file_size(path)` | `getfsize()` | any path, not just the open buffer |
| `file_type(path)` | `getftype()` | `file`, `dir` or `link` |
| `file_time(path)` | `getftime()` | seconds since the epoch |
| `file_perm(path)` | `getfperm()` | the nine `rwxrwxrwx` characters |
| `buffer_line(buf, n)` | `getbufline()` | a line of any open buffer |
| `command_exists(name)` | `exists(":cmd")` | built-ins and plugin commands alike |
| `plugin_count()` / `plugin_name(i)` / `plugin_names()` | `getscriptinfo()` | the loaded native plugins |
| `mark(name)` | `getpos("'a")` | a named mark, `None` when never set |
| `window_size()` | `getwininfo()` width/height | the text area in cells, gutters excluded |
| `completions(prefix)` | `getcompletion(p, "command")` | matching `:`-command names |
| `marks()` | `getmarklist()` | every set mark, sorted by name |
| `changelist()` / `changelist_index()` | `getchangelist()` | edit positions, oldest first, and where `g;` resumes |
| `display_width(text)` | `strdisplaywidth()` | terminal cells, not characters |
| `buffer_path_at(i)` / `buffer_modified(i)` | `getbufinfo()` | the path on disk, and whether it is dirty |
| `window_index()` | `winnr()` | which window is focused |
| `line_length(n)` | `col("$")` | characters, line ending excluded; `None` past the end |
| `indent(n)` | `indent({lnum})` | columns, a tab counting as `tabstop` |
| `word_count()` | `wordcount()` | chars, words and lines in one call |
| `option_num(name)` / `option_bool(name)` | `&opt` | typed, so callers do not parse the string |

Positions here are **char** offsets. A language server counts bytes, which is
the same split vim has between `col()` and `charcol()`, so there is a bridge:

| method | effect |
|---|---|
| `byte_offset(char)` | the byte offset of a char offset |
| `char_offset(byte)` | the inverse, rounded down to a char boundary |

Two places where this deliberately differs from vim, both because copying vim
would lose information zmax has:

- **`Span` carries `anchor`/`head`, not start/end.** vim's `'<`/`'>` are always
  in document order; a zmax selection has a direction, so `head < anchor` for a
  backwards one, and that is which end the user is extending from.
- **`file_type` reports a symlink as `link`**, like `getftype`, because it stats
  the link rather than its target.

Every one of these reports `None`/`0` when there is no active editor context
rather than inventing a value, and every string is released through the host's
own allocator before it reaches you.

Editor-touching callbacks are valid only **while a command is executing** — the
host publishes the active editor context for the duration of that call. They are
inert if invoked from a background thread the plugin spawned.

## Command resolution

A plugin command is unknown to the editor's static command table, so it resolves
in the `:`-dispatcher's fallthrough: **after** built-in typable commands and
**before** the user-command / vimscript fallback.

## Safety notes

- `ABI_VERSION` is bumped on any layout/semantics change to `HostApi`,
  `PluginInfo`, `CommandFn`, or `InitFn`. Mismatched plugins are refused.
- The loaded library is kept alive for the process lifetime; `:zmax-native unload`
  purges the plugin's command registrations **before** `dlclose`, so no live
  function pointer survives the unload.
- Loading two plugins with the same `name` is refused — unload the first.
