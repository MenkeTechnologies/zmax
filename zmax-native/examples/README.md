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
| [`nav-history`](nav-history) | `:nav` | `marks` vs `jumps` vs `changelist` — three histories, incl. closed buffers |
| [`window-map`](window-map) | `:windows` | `window_buffer`/`buffer_window` — not inverses; finds hidden buffers |
| [`width-check`](width-check) | `:width [limit]` | `display_width` vs `line_length` — cells, not characters |
| [`opt-info`](opt-info) | `:opt {option}` | `option_set` vs `option` — set-to-empty is not unset |
| [`file-facts`](file-facts) | `:finfo [path]` | `file_type`/`file_perm`/`file_writable` — a symlink is a `link`, and 2 ≠ writable |
| [`fold-outline`](fold-outline) | `:outline` | `fold_level` + `fold_closed` — structure without a second parse |
| [`search-peek`](search-peek) | `:sc [pattern]` | `search_count`/`search_next` — Rust regexes, and zero is ambiguous |
| [`three-coords`](three-coords) | `:pos` | chars vs bytes vs cells, and the bridges between them |
| [`todo-scan`](todo-scan) | `:todo`, `:todo-next` | `search_next` driven as a LOOP — must advance, and needs a ceiling |
| [`registers`](registers) | `:regs [names]` | `register` — newline-joined, and no register TYPE is recorded |
| [`project-check`](project-check) | `:project` | `executable` + `exepath` — whether, and *which* one won |
| [`writing-stats`](writing-stats) | `:writing` | `word_count` (three counts, one call) + `indent` in columns |

See [`../README.md`](../README.md) for the SDK reference and how to write your own.
