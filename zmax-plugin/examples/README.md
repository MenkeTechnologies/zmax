# Example zmax plugins

Buildable native plugins demonstrating the [`zmax-plugin`](..) C-ABI SDK. Each is
an ordinary `cdylib` — the same shape a third-party plugin author's crate has.

Build them all at once (they share this directory's workspace / `target/`):

```sh
cargo build            # from zmax-plugin/examples/
```

then, inside zmax, load a `.dylib` (macOS) / `.so` (Linux) from `target/debug/`:

```text
:plugin load .../zmax-plugin/examples/target/debug/libzmax_plugin_hello.dylib
:plugin list
```

| Crate | Commands | Host API exercised |
|---|---|---|
| [`hello-plugin`](hello-plugin) | `:hello`, `:hello-insert`, `:hello-echo` | `message`, `buffer_text`, `insert_text`, `eval` |
| [`insert-date`](insert-date) | `:date`, `:datetime` | `insert_text` (computed content, zero deps) |
| [`buffer-stats`](buffer-stats) | `:bufstats` | `buffer_text` + analysis → `message` |
| [`trim-trailing`](trim-trailing) | `:trim-trailing` | `buffer_text` guard + `eval` (`:%s`) |
| [`banner`](banner) | `:banner <text…>` | `Args` + multi-line `insert_text` |

See [`../README.md`](../README.md) for the SDK reference and how to write your own.
