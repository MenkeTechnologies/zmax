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
