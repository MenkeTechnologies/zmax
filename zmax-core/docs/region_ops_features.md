# `zmax-core::region_ops` — region / structural editing batch

Pure-Rust, editor-type-free algorithms in `zmax-core/src/region_ops.rs`, each
unit-tested in isolation (16 tests). The command layer extracts the live
selection's line span / region, calls one of these, and applies the result as a
single undoable transaction. Distinct from the `zmax-term::text_ops` batch (no
overlap in functions).

Honesty: every row below is a real in-engine algorithm + test. No LSP/GPU/native
work is claimed here.

| Function / type | Capability | Prior art |
|---|---|---|
| `join_lines` | Join a line block into one (space-collapsed or raw) | VS Code Join Lines (Ctrl+J), Vim `J`, JetBrains |
| `sort_lines` + `SortOptions` | Sort lines: reverse / ignore-case / numeric / unique | Sublime Sort Lines, VS Code, Vim `:sort`, Emacs `sort-lines`, GNU `sort` |
| `reverse_lines` | Reverse the order of a line block | Emacs `reverse-region`, Vim `:g/^/m0` |
| `uniq_adjacent` | Collapse only *runs* of identical lines | coreutils `uniq`, Emacs `delete-duplicate-lines` (adjacent) |
| `number_lines` | Prefix lines with padded sequential numbers | Emacs `rectangle-number-lines`, JetBrains |
| `trim_trailing_whitespace` | Strip trailing spaces/tabs per line | Emacs `delete-trailing-whitespace`, VS Code `trimTrailingWhitespace` |
| `occur` | List matching lines with 1-based numbers | Emacs `occur`, Vim `:g/pat/p` |
| `transpose_lines` | Swap a line with the following line | Emacs `C-x C-t`, VS Code Move Line Down |
| `rot13` | ROT13 cipher over ASCII letters | Emacs `rot13-region`, Vim `g?` |
| `invert_case` | Swap case of every cased char (Unicode-aware) | Vim `g~` |
| `transpose_chars` | Swap the two chars straddling the cursor | Emacs `C-t` |
| `eval_arithmetic` | Evaluate a selected arithmetic expression (`+ - * / % ^`, parens, unary, right-assoc `^`) | Emacs `calc-eval`, VS Code "Calculate" |
| `splice_sexp` | paredit: remove the enclosing bracket pair | Emacs paredit `splice-sexp` (`M-s`) |
| `slurp_forward` | paredit: pull the next sibling form inside the list | Emacs paredit `slurp-forward` (`C-)`) |
| `KillRing` | Bounded most-recent-first kill ring with rotating yank pointer | GNU Emacs `kill-ring` / `M-y` |
| `Registers` | Named registers; uppercase name appends (Vim), case-insensitive lookup | Vim / Emacs registers |
| ⭐ `rotate_lines` | Cyclic wrap-around rotate of a line block by ±n | zmax original — beyond Emacs, VS Code, Vim, Sublime, JetBrains, Zed, Helix |

Build/test: `cargo test -p zmax-core` (green; region_ops adds 16 tests).
