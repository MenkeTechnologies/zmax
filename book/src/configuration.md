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
```

You can use a custom configuration file by specifying it with the `-c` or
`--config` command line argument, for example `hx -c path/to/custom-config.toml`.
You can reload the config file by issuing the `:config-reload` command. Alternatively, on Unix operating systems, you can reload it by sending the USR1
signal to the Zemacs process, such as by using the command `pkill -USR1 hx`.

Finally, you can have a `config.toml` and a `languages.toml` local to a project by putting it under a `.zemacs` directory in your repository.
Its settings will be merged with the configuration directory and the built-in configuration.

