# Configuration

Zemacs keeps all of its configuration under a single dotted home directory:

- Linux and Mac: `~/.zemacs/config.toml`
- Windows: `%USERPROFILE%\.zemacs\config.toml`

On first run, if this file does not exist, Zemacs writes a default starter
`config.toml` there for you to edit. Override global configuration parameters by
editing it.

> 💡 You can easily open the config file by typing `:config-open` within Zemacs normal mode.

Example config:

```toml
theme = "onedark"

[editor]
line-number = "relative"
mouse = false

[editor.cursor-shape]
insert = "bar"
normal = "block"
select = "underline"

[editor.file-picker]
hidden = false

# The project file tree (sidebar) shows dotfiles by default; set hidden = true
# to hide names starting with a dot.
[editor.file-explorer]
hidden = false
```

You can use a custom configuration file by specifying it with the `-c` or
`--config` command line argument, for example `hx -c path/to/custom-config.toml`.
You can reload the config file by issuing the `:config-reload` command. Alternatively, on Unix operating systems, you can reload it by sending the USR1
signal to the Zemacs process, such as by using the command `pkill -USR1 hx`.

Finally, you can have a `config.toml` and a `languages.toml` local to a project by putting it under a `.zemacs` directory in your repository.
Its settings will be merged with the configuration directory and the built-in configuration.

## Keymap presets

Zemacs ships **four** keybinding presets. Select one with the top-level `keymap`
key (or switch at runtime with `:keymap <name>`, or in Preferences ▸ Keymap):

```toml
keymap = "spacemacs"   # "spacemacs" (default) | "vim" | "helix" | "emacs"
```

| Preset | Starts in | Leader / prefixes |
| --- | --- | --- |
| `spacemacs` *(default)* | Normal | vim/evil keys + the `SPC` leader **and** the Emacs `C-x` prefix; both open a which-key popup. |
| `vim` | Normal | pure vim — no `SPC` leader and no which-key popup; `C-x` is `decrement`. |
| `helix` | Normal | the original selection-first keymap with its `SPC` leader. |
| `emacs` | Insert | modeless Emacs bindings (`C-x`, `C-c`, `M-x`, …). |

`decrement` per line is on `g C-x` in every preset. Any `[keys.*]` overrides you
add are merged on top of the selected preset.

