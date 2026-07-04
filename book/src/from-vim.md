# Migrating from Vim

Zemacs's default keymap (`spacemacs`) and its dedicated `vim` preset are built
around Vim muscle memory: the **operator → motion** model you already know works
as-is, so most of what you type in Vim does the same thing here. Pick a preset in
`config.toml` (see [Configuration](./configuration.md#keymap-presets)):

```toml
keymap = "vim"   # or the default "spacemacs" (vim base + an emacs C-x prefix)
```

The rest of this page assumes one of those Vim-family presets. If you instead set
`keymap = "helix"` you get the original Kakoune-style *selection → action* model —
see [The `helix` preset](#the-helix-preset) at the bottom.

## What works exactly like Vim

- **Operators + motions / text objects** (verb → noun): `dw`, `de`, `db`, `d$`,
  `diw`, `ci(`, `ca"`, `yy`, `cc`, `dd`, `>>`, `<<`, `gU`/`gu`, `df<char>` …
- **Delete / change:** `x` deletes the character under the cursor (`3x` for a
  count), `X` the one before it, `D`/`C` to the end of the line, `cw` changes to
  the end of the word.
- **Motions:** `w`/`e`/`b`/`ge`, `f`/`t`/`F`/`T` with `;`/`,`, `0`/`^`/`$`,
  `gg`/`G`/`{n}G` (landing on the first non-blank), `{`/`}`, `%`, `H`/`M`/`L`.
- **Lines:** `dd`, `yy`, `p`/`P`, `J` (join with a space), `gJ` (join without),
  `o`/`O`, `>>`/`<<`.
- **Insert entry & misc:** `i`/`a`/`I`/`A`, `s`/`S`, `r`/`R`, `~` (toggle case and
  advance).
- **Search:** `/`, `?`, `n`, `N`, `*`, `#`.
- **Visual:** `v`, `V`, `<C-v>` (blockwise); operators apply to the selection.
- **Jumps & marks:** `<C-o>`/`<C-i>` walk the jumplist; `` `{mark} ``/`'{mark}`
  jump to a mark.

For the authoritative, always-current list of every bound key and every
`:`-command, see the generated [keymap](./keymap.md),
[static commands](./generated/static-cmd.md) and
[typable commands](./generated/typable-cmd.md) references.

## Intentional differences from Vim

- The cursor is a **block** in every mode by default (including insert). Change it
  with:

  ```toml
  [editor.cursor-shape]
  insert = "bar"
  ```

- `f`/`t`/`F`/`T` are **not confined to the current line**.
- Some Ex commands are named differently (e.g. re-read a file, run a shell
  command). Browse [typable commands](./generated/typable-cmd.md) or type `:` and
  use the completion menu.
- Zemacs adds first-class **multiple cursors**, syntax-aware
  [text objects](./textobjects.md), [surround](./surround.md), an integrated LSP,
  and tree-sitter selections — capabilities Vim reaches for plugins to provide.

Zemacs also allows [some limited movement in `insert` mode](./keymap.md#insert-mode)
without switching to `normal` mode.

## The `helix` preset

Setting `keymap = "helix"` switches to the Kakoune-inspired **selection → action**
model: whatever you are going to act on is selected first and the action comes
second, and a cursor is simply a single-width selection. Under that preset the
common Vim keystrokes map differently — a few examples:

| Action | Vim | Zemacs (`helix` preset) |
| --- | --- | --- |
| delete a word | `dw` | `wd` |
| change a word | `cw` | `wc` / `ec` (includes trailing whitespace) |
| delete a character | `x` | `d` (or `;d` to reduce to a single char first) |
| copy a line | `yy` | `Xy` (`X` extends selections to whole lines) |
| delete a line | `dd` | `xd` (`x` selects the whole line) |
| go to last line | `G` | `ge` |
| line start / first non-blank | `0` / `^` | `gh` / `gs` |
| line end | `$` | `gl` |
| matching bracket | `%` | `mm` |
| global replace | `:%s/foo/bar/g` | `%sfoo<ret>cbar<esc>` |

Here `%` selects the whole buffer, `s` opens a regex prompt and reduces the
selection to each match (one cursor per match), and `c` changes them all at once.
The `helix` preset content mirrors the
[Kakoune "Migrating from Vim" wiki](https://github.com/mawww/kakoune/wiki/Migrating-from-Vim),
since Zemacs's selection-first mode descends from the same lineage.
