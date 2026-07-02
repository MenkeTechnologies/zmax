# `zemacs-core::text_engine` — round-2 editor-engine batch

Pure-Rust, editor-type-free algorithms in `zemacs-core/src/text_engine.rs`, each
unit-tested in isolation (17 tests). The command layer extracts the live
selection's region / line span, calls one of these, and applies the result as a
single undoable transaction. Deliberately disjoint from the round-1
`zemacs-core::region_ops` batch and from the tree-sitter-driven modules
(`object`, `fold`, `indent`, `match_brackets`, `comment`, `surround`): everything
here is language-agnostic and syntax-free.

Honesty: every row below is a real in-engine algorithm + test. No
LSP-over-socket, GPU, or native work is claimed here — those boundaries are left
to the respective server/renderer layers.

| Function / type | Capability | Prior art |
|---|---|---|
| `align_on_separator` | Align a line block on the first separator into a column | Emacs `align-regexp`, Vim `vim-easy-align` / `Tabular`, Sublime Alignment |
| `fill_paragraph` | Hard word-wrap text to a column with a prefix (collapses whitespace) | Emacs `fill-paragraph` (M-q), VS Code Rewrap |
| `untabify` | Expand tabs to spaces honoring column stops | Emacs `untabify`, VS Code "Convert Indentation to Spaces", Vim `:retab` |
| `tabify_indent` | Convert leading spaces to tabs (+ remainder) | Emacs `tabify`, VS Code "Convert Indentation to Tabs" |
| `transpose_words` | Swap the word before the cursor with the following word | Emacs `transpose-words` (M-t) |
| `sort_by_field` | Stable-sort lines by a `sep`-delimited column key | Emacs `sort-fields`, Vim `:sort /re/`, `sort -k -t` |
| `extract_rectangle` / `kill_rectangle` / `string_rectangle` / `open_rectangle` | Column-rectangle read / cut / replace / open | Emacs `rectangle-mark-mode` (`C-x r k/t/o`), Sublime/VS Code column edits |
| `RectClip` / `RectRegisters` | Rectangular clipboard payload + named rect registers | Emacs `C-x r r` rectangle registers |
| `merge_ranges` | Normalize a selection set: sort + merge overlapping/touching | VS Code & Sublime multi-cursor, Helix selections |
| `subtract_range` | Remove a hole from a selection set, splitting interiors | VS Code / Sublime multi-cursor edits |
| `compute_indent_folds` | Compute fold ranges purely from indentation | VS Code default folding, Vim `foldmethod=indent` |
| `match_tag` | Match an HTML/XML open/close tag pair from either side | Emacs `sgml-mode`, VS Code / JetBrains matching-tag |
| `subword_boundaries` / `next_subword_start` | camelCase / snake / acronym subword segmentation + motion | Emacs `subword-mode`, VS Code camelCase motion, JetBrains CamelHumps |
| `search_all` / `IncrementalSearch` | All match offsets + wrap-around match cycling | Emacs `isearch` (C-s/C-r), every editor's find bar |
| `UndoTree` | Branching undo history (undo/redo across branches) | `undo-tree.el`, Vim persistent branching undo |
| `strip_common_indent` | Remove the shared leading-whitespace prefix (dedent) | Python `textwrap.dedent` (no editor single-command equivalent) |
| `first_unbalanced` | Index of the first unmatched/mismatched bracket | Emacs `check-parens` |
| ⭐ `cycle_identifier_case` | Cycle snake → kebab → camel → Pascal → SCREAMING → … | zemacs original — beyond Emacs, VS Code, Vim, Sublime, JetBrains, Zed, Helix (they offer discrete conversions, not a cycle) |
| ⭐ `sum_column` | Sum a numeric column across a line block (returns total + count) | zemacs original — spreadsheet-style total with no built-in equivalent in the listed editors |

Build/test: `cargo test -p zemacs-core` (green; `text_engine` adds 17 tests, taking
the lib suite from 187 → 204). `cargo clippy -p zemacs-core --all-targets` is warning-free.
