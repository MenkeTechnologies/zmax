# zmax port tracking

This directory holds the **port report** infrastructure: the instrument that
measures how much of the Vim/Neovim + Emacs + Spacemacs + fzf.vim + JetBrains
(IntelliJ IDEA) feature surface zmax implements. zmax starts from a
modal core and is being built out toward the union of those editors; this report
tracks that build-out.

## Layout

| Path | Role |
|---|---|
| `data/*.json` | **Denominator.** Cited feature inventories, one JSON array per source/category. Every item was parsed from an upstream primary source — not hand-written. |
| `mapping.json` | **The only curated artifact.** Maps a spec item id to the zmax command/keybinding that implements it. |
| `../scripts/gen_port_report.py` | Generator. Re-derives the numerator from zmax source on every run, applies the mapping, writes the report. |
| `../docs/port_report.html` | Standalone styled report (browser preview). |
| `../docs/port_report.md` | Markdown report. |
| `../docs/keybinding_report.{html,md}` | Focused report: keybinding coverage only (vim/emacs/spacemacs key-press surface). |
| `../book/src/generated/port-report.md` | Same markdown, wired into the mdBook so it publishes to gh-pages. |

## Denominator sources (cited, parsed — not invented)

| File(s) | Source |
|---|---|
| `vim_normal/insert/visual/cmdline/excmds.json` | Neovim `runtime/doc/index.txt` |
| `vim_options.json` | Neovim `runtime/doc/options.txt` |
| `vim_functions.json` | Neovim `runtime/doc/vimfn.txt` (builtin function list) |
| `emacs_commands.json` | GNU Emacs Manual — Command Index |
| `emacs_keys.json` | GNU Emacs Manual — Key Index |
| `spacemacs_bindings.json` | Spacemacs `doc/DOCUMENTATION.org` |
| `spacemacs_layers.json` | Spacemacs `layers/` git tree |
| `fzf_vim.json` | junegunn/fzf.vim — Commands reference |
| `jetbrains_keymap.json` | JetBrains IntelliJ IDEA — macOS Default Keymap reference (`jetbrains.com/help/idea/reference-keymap-mac-default.html`) |
| `functionality.json` | **Primary measure.** A curated taxonomy of distinct editor *capabilities*, deduplicated across the sources above — one row per feature, not per source. The report leads with this; the per-source rows are secondary muscle-memory-compatibility views. Includes capabilities zmax lacks (absent) so the denominator stays fair. |

Each item carries a `doc_ref` back to its source line/anchor.

**A note on the emacs denominator.** `emacs_commands.json` + `emacs_keys.json`
are the *entire* GNU Emacs manual indexes — 3008 items — including games
(`5x5`), two-column mode, Dired, TeX-mode, Gnus, Calc, and Buffer-Menu keys
(445 of the 1124 keys are major-mode-specific). No editor "ports" that surface,
so emacs coverage is reported as a low single-digit percentage by construction:
it measures *fraction of all of Emacs*, not editing-command coverage. The
mapped set targets the global editing/movement/search/kill-yank/window/buffer
commands a code editor actually has; the long mode-specific tail stays absent on
purpose. Read the emacs row with that scope in mind — it is not comparable to
the focused Vim/Neovim core-command denominator.

## Honesty contract

This mirrors the zshrs `gen_port_report.py` precedent. The report is the number
the maintainer reads to know reality without auditing every symbol, so the
generator is built to make faking the number structurally impossible:

1. **The numerator is re-parsed from source every run.** The set of zmax
   static commands, typable `:` commands, and default keybindings is extracted
   from `zmax-term/src/commands.rs`, `commands/typed.rs`, and the active
   default keymap `keymap/vim.rs` at generation time. The command-line editing
   surface is read from the `:` prompt's hardcoded key handler in
   `ui/prompt.rs` (exposed as the `command` mode). There is no cached count
   to edit.
2. **Every mapping evidence token must resolve to real zmax code.** Evidence
   is `static:<cmd>`, `typable:<name>`, or `key:<mode>:<chord>` (mode is
   `normal`/`select`/`insert`/`command`). A token that
   does not resolve to a parsed command/binding is a **broken mapping**: it is
   counted as *absent* and listed loudly at the top of the report. The number
   can only go up by adding real code and pointing the mapping at it.
3. **`ported` and `partial` are separate.** Headline coverage counts `ported`
   only. `partial` is for genuine capability-present-but-different-model cases
   (e.g. Vim `d{motion}` vs zmax select-then-`d`).
4. **No whitelisting, no detector-bypass annotations.** Do not edit this script
   or the inventories to make the number move. Move the number by shipping
   commands. See `~/.claude/CLAUDE.md` "Audit-Tool Tampering".

## Regenerate

```sh
python3 scripts/gen_port_report.py
```

Output ends with `broken=N`. **`broken` must be 0** before committing — a
non-zero count means a mapping points at code that no longer exists.

## Adding coverage

When you implement a feature in zmax:

1. Add an entry to `mapping.json`:
   ```json
   {"spec_id": "neovim.ex-command.substitute", "status": "ported",
    "evidence": ["typable:substitute"], "note": "..."}
   ```
2. Re-run the generator; confirm `broken=0` and the count moved.
3. Commit the code and the mapping together.
