## Using pickers

Zmax has a variety of pickers, which are interactive windows used to select various kinds of items. These include a file picker, global search picker, and more. In the modal presets (`spacemacs`/`vim`/`helix`) most pickers are accessed via keybindings in [space mode](./keymap.md#space-mode); the modeless `emacs` preset reaches them through Emacs chords (see the [static-command reference](./generated/static-cmd.md)). Once open, every picker uses the same [keymap](./keymap.md#picker) for navigation regardless of preset.

### Filtering Picker Results

Most pickers perform fuzzy matching using [fzf syntax](https://github.com/junegunn/fzf?tab=readme-ov-file#search-syntax). Two exceptions are the global search picker, which uses regex, and the workspace symbol picker, which passes search terms to the language server. Note that OR operations (`|`) are not currently supported.

If a picker shows multiple columns, you may apply the filter to a specific column by prefixing the column name with `%`. Column names can be shortened to any prefix, so `%p`, `%pa` or `%pat` all mean the same as `%path`. For example, a query of `zmax %p .toml !lang` in the global search picker searches for the term "zmax" within files with paths ending in ".toml" but not including "lang".

You can insert the contents of a [register](./registers.md) using `Ctrl-r` followed by a register name. For example, one could insert the currently selected text using `Ctrl-r`-`.`, or the directory of the current file using `Ctrl-r`-`%` followed by `Ctrl-w` to remove the last path section. The global search picker will use the contents of the [search register](./registers.md#default-registers) if you press `Enter` without typing a filter. For example, pressing `*`-`Space-/`-`Enter` will start a global search for the currently selected text.

### fzf.vim commands

Alongside the native pickers, zmax ships the [fzf.vim](https://github.com/junegunn/fzf.vim) command surface — `:Files`, `:GFiles`, `:Buffers`, `:Rg`, `:BLines`, `:Commands`, `:Maps`, `:Colors`, `:Locate`, `:Todo` and the rest — reachable from `SPC F` in the modal presets.

They do **not** need the `fzf` binary installed. The picker is [arb](./scripting.md), one of the twelve interpreters compiled into zmax, running its `--fzf` mode in this process: no `fork`, no `exec`, and no external dependency. It is a drop-in for fzf, so it paints fzf's own palette and honours your existing configuration:

| Variable | Effect |
|--|--|
| `FZF_DEFAULT_OPTS_FILE` | Read first, as fzf does |
| `FZF_DEFAULT_OPTS` | Read next, so your prompt, layout, border, colors and `--bind` table carry over |
| `FZF_DEFAULT_COMMAND` | Source command when a command supplies no list of its own |
| `FZF_CTRL_T_COMMAND` / `FZF_CTRL_T_OPTS` | Used for the file-listing commands, so `:Files` matches your shell's `Ctrl-t` finder |

Precedence is fzf's: the options file, then the environment, then `[editor.fzf]`, then the flags a particular command passes.

Preview commands receive the `FZF_*` variables fzf exports to its children — `FZF_QUERY`, `FZF_CURRENT_ITEM`, `FZF_PREVIEW_LINES`, `FZF_PREVIEW_COLUMNS`, `FZF_TOTAL_COUNT`, `FZF_MATCH_COUNT`, `FZF_POS` and the rest — so a preview written for fzf works unchanged. The four that describe the last keystroke (`FZF_KEY`, `FZF_ACTION`, `FZF_IDLE_TIME`, `FZF_IDLE_TIME_MS`) are not set, and neither are the `--listen` variables, since that mode is not run.

When a command supplies no candidates and no source, and none of the environment commands above is set, the list comes from the same file walk the native file picker uses — so `:Files` and `Space-f` show the same files under the same [`[editor.file-picker]`](./editor.md#editorfile-picker-section) settings.

`--preview-window` is the one fzf option with no effect here: the preview pane is laid out by arb, not by that spec.

### File explorer

`Space-e` opens an interactive file explorer for browsing and opening files, rooted at the workspace; `Space-.` opens one rooted at the current buffer's directory. Unlike the file picker, the explorer does not ignore most files by default; its ignore behaviour is configured separately in the [`[editor.file-explorer]`](./editor.md#editorfile-explorer-section) section.
