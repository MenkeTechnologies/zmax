## Registers

- [User-defined registers](#user-defined-registers)
- [Default registers](#default-registers)
- [Special registers](#special-registers)
- [tmux paste buffers](#tmux-paste-buffers)

In Zmax, registers are storage locations for text and other data, such as the
result of a search. Registers can be used to cut, copy, and paste text, similar
to the clipboard in other text editors. In the modal presets
(`spacemacs`/`vim`/`helix`) usage is similar to Vim, with `"` being used to
select a register; the `emacs` preset reaches register commands through Emacs
chords (see the [static-command reference](./generated/static-cmd.md)). The
register mechanics and tables below are the same across all presets.

### User-defined registers

Zmax allows you to create your own named registers for storing text, for
example:

- `"ay` - Yank the current selection to register `a`.
- `"op` - Paste the text in register `o` after the selection.

If a register is selected before invoking a change or delete command, the selection will be stored in the register and the action will be carried out:

- `"hc` - Store the selection in register `h` and then change it (delete and enter insert mode).
- `"md` - Store the selection in register `m` and delete it.

### Default registers

Commands that use registers, like yank (`y`), use a default register if none is specified.
These registers are used as defaults:

| Register character | Contains              |
| ---                | ---                   |
| `/`                | Last search           |
| `:`                | Last executed command |
| `"`                | Last yanked text      |
| `@`                | Last recorded macro   |

### Special registers

Some registers have special behavior when read from and written to.

| Register character | When read              | When written             |
| ---                | ---                    | ---                      |
| `_`                | No values are returned | All values are discarded |
| `#`                | Selection indices (first selection is `1`, second is `2`, etc.) | This register is not writable |
| `.`                | Contents of the current selections | This register is not writable |
| `%`                | Name of the current file | This register is not writable |
| `+`                | Reads from the system clipboard | Joins and yanks to the system clipboard |
| `*`                | Reads from the primary clipboard | Joins and yanks to the primary clipboard |

When yanking multiple selections to the clipboard registers, the selections
are joined with newlines. Pasting from these registers will paste multiple
selections if the clipboard was last yanked to by the Zmax session. Otherwise
the clipboard contents are pasted as one selection.


### tmux paste buffers

Inside a tmux session the paste buffers are a third store, next to the registers
and the system clipboard. They are shared with every pane of the session (`prefix
]` pastes them) and are untouched by what other applications copy, so they are
the place to park text that should survive a stray Cmd-C elsewhere.

| Command | Action |
| --- | --- |
| `:tmux-buffer-yank` | Yank the selections into a NEW tmux buffer. The system clipboard is left alone. |
| `:tmux-buffer-paste-after [name]` | Paste the newest tmux buffer, or the named one, after the selections. |
| `:tmux-buffer-paste-before [name]` | The same, before the selections. |
| `:tmux-buffers` | List the session's buffers (name and sample) in the status bar. |

The same actions are bindable as the static commands `yank_to_tmux_buffer`,
`paste_tmux_buffer_after`, `paste_tmux_buffer_before`, and
`tmux_buffer_picker` (a fuzzy picker over the buffers, newest first), and appear
under **tmux Buffers** in the editor's right-click menu when a tmux session is
detected. Outside tmux the commands report an error and the menu entry is absent.
