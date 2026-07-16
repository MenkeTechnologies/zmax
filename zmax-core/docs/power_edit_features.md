# `zmax-core::power_edit` — round-3 editor-engine batch

Pure-Rust, editor-type-free algorithms in `zmax-core/src/power_edit.rs`, each
unit-tested in isolation (21 tests). The command layer extracts the live
selection's region / line span, calls one of these, and applies the result as a
single undoable transaction. Built on top of the round-1
`zmax-core::region_ops` and round-2 `zmax-core::text_engine` batches, and
deliberately disjoint from them and from the tree-sitter-driven modules
(`object`, `fold`, `indent`, `match_brackets`, `comment`, `surround`): everything
here is language-agnostic and syntax-free.

Column arithmetic treats each `char` as one display column (monospace ASCII
assumption); the grapheme / tree-sitter modules handle true display width.

Honesty: every row below is a real in-engine algorithm + test. No
LSP-over-socket, GPU, native-terminal, or renderer work is claimed here — those
boundaries stay with the respective server / view layers.

| Function / type | Capability | Prior art |
|---|---|---|
| `soft_wrap_offsets` | Word-wrap a line into visual-row start offsets (hard-break over-long words) | Emacs `visual-line-mode`, VS Code Word Wrap, Sublime Word Wrap |
| `visual_row_col` / `visual_move_down` / `visual_move_up` | Visual-line cursor motion with goal-column preservation | Vim `gj`/`gk`, Emacs visual-line `next-line`/`previous-line` |
| `expand_region` | Grow selection word → bracket/quote pair → line → paragraph → buffer | `expand-region.el`, JetBrains Extend Selection (Ctrl-W), VS Code Expand Selection |
| `MultiCursor::add_next_match` | Add the next occurrence of the selection as a new cursor (wraps) | VS Code Add Next Occurrence (Ctrl-D), Sublime Ctrl-D |
| `MultiCursor::add_all_matches` | Select every occurrence at once | VS Code Ctrl-Shift-L, Sublime Alt-F3 |
| `join_with_separator` | Join lines with a caller-chosen separator (optional trim) | JetBrains / VS Code Join Lines, Emacs `join-line` |
| `sequence_increment` | Turn a column of numbers into an incrementing sequence (cumulative step) | Vim visual-block `g CTRL-A` |
| `uniq_all` | Remove all duplicate lines, keep first, preserve order | Emacs `delete-duplicate-lines` |
| `uniq_count` | Collapse adjacent dup runs with a count prefix | coreutils `uniq -c` |
| `format_markdown_table` | Render rows as an aligned GFM table with header rule | Org-mode / markdown table re-align |
| `comment_box` | Draw a comment-char box/banner around text | Emacs `comment-box` |
| `indent_guide_columns` | Compute indent-guide columns, blank lines inherit neighbours | VS Code / Sublime / JetBrains indent guides |
| `normalize_whitespace` | Collapse internal whitespace runs to one space + trim | Emacs `just-one-space` / `cycle-spacing` |
| `squeeze_blank_lines` | Collapse consecutive blank lines to one | Emacs `delete-blank-lines`, `cat -s` |
| `wrap_in_tag` | Wrap text in an HTML tag from a minimal Emmet abbr (`tag#id.class`) | Emmet Wrap with Abbreviation (VS Code / Sublime / JetBrains) |
| `KmacroCounter` | Keyboard-macro counter with step + `%d`/`%0Nd` format | Emacs `kmacro-insert-counter` / `kmacro-set-format` |
| `query_replace_matches` / `query_replace` | Match list + selective (per-match y/n) replace | Emacs `query-replace` / `query-replace-regexp` (register-sourced replacement) |
| ⭐ `transpose_grid` | Transpose a delimited grid (rows ↔ columns), pad ragged rows | **zmax original** — beyond Emacs `transpose-lines` and all 1-D transposes in Vim / VS Code / Sublime |
| ⭐ `align_table_auto` | Auto-detect the dominant delimiter and align all columns | **zmax original** — beyond Emacs `align-regexp` / Vim easy-align, which need an explicit pattern |

## Test coverage

`cargo test -p zmax-core --lib power_edit` — 21 tests, all green. Full crate:
225 tests (204 baseline + 21 new), clippy clean.
