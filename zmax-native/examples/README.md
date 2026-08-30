# Example zmax plugins

Buildable native plugins demonstrating the [`zmax-native`](..) C-ABI SDK. Each is
an ordinary `cdylib` — the same shape a third-party plugin author's crate has.

Build them all at once (they share this directory's workspace / `target/`):

```sh
cargo build            # from zmax-native/examples/
```

then, inside zmax, load a `.dylib` (macOS) / `.so` (Linux) from `target/debug/`:

```text
:zmax-native load .../zmax-native/examples/target/debug/libzmax_native_hello.dylib
:zmax-native list
```

| Crate | Commands | Host API exercised |
|---|---|---|
| [`hello-plugin`](hello-plugin) | `:hello`, `:hello-insert`, `:hello-echo` | `message`, `buffer_text`, `insert_text`, `eval` |
| [`insert-date`](insert-date) | `:date`, `:datetime` | `insert_text` (computed content, zero deps) |
| [`buffer-stats`](buffer-stats) | `:bufstats` | `buffer_text` + analysis → `message` |
| [`trim-trailing`](trim-trailing) | `:trim-trailing` | `buffer_text` guard + `eval` (`:%s`) |
| [`banner`](banner) | `:banner <text…>` | `Args` + multi-line `insert_text` |
| [`zwire-lookup`](zwire-lookup) | `:zwire-lookup [site] [term…]` | `word_at_cursor` + `selection_text`, and an IPC socket |
| [`scope-at-cursor`](scope-at-cursor) | `:scope`, `:scope-copy` | `syntax_at` + `cursor` — the theme scope stack under the cursor |
| [`swap-doctor`](swap-doctor) | `:swap-doctor` | `swap_path`/`swap_exists`/`swap_locked_by` + `undo_tree` |
| [`three-lists`](three-lists) | `:lists` | `quickfix` vs `loclist` vs `diagnostics` — three different lists |
| [`block-peek`](block-peek) | `:block-peek` | `region` + `region_pos` + `select_kind`, incl. the blockwise skip |

See [`../README.md`](../README.md) for the SDK reference and how to write your own.
