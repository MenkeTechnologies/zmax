# Embedded scripting

zemacs embeds several scripting interpreters directly in the editor binary, so
you can evaluate scripts against the live buffer with no external process. Each
language drives the editor through one uniform host API.

| Language        | Command(s)                     | Interpreter   | Platforms |
| --------------- | ------------------------------ | ------------- | --------- |
| Emacs Lisp      | `:elisp` (`:eval-expression`, `:el`) | `elisprs`     | all       |
| Vimscript (VimL)| `:vim` (`:viml`, `:vimscript`) | `vimlrs`      | all       |
| AWK             | `:awk` (`:awk-filter`)         | `awkrs`       | all       |
| zsh             | `:zsh` (`:zshell`)             | `zshrs`       | unix only |
| stryke          | `:stryke` (`:st`)             | `strykelang`  | unix only |

> 💡 These are gated behind the `scripting` Cargo feature, which is **on by
> default**. A build made with `--no-default-features` (see
> [Building from source](./building-from-source.md#cargo-features)) omits all of
> them — the commands below then report that scripting was not compiled in.

## Commands

- **`:elisp <code>`** — evaluate an Emacs Lisp expression against the editor;
  the result is shown on the status line. A subset of the editor is exposed as
  elisp builtins (point/region, buffer access, `message`, running typable
  commands, etc.).
- **`:vim <code>`** — evaluate Vimscript; captured `:echo` output and the
  trailing expression value are shown. Globals and functions persist across
  calls.
- **`:awk <program>`** — filter the current selection (or the whole buffer when
  there is no selection) through an AWK program, replacing it with the program's
  output as a single undo step.
- **`:zsh <command>`** — run a command line in the embedded shell; its captured
  output is shown in a popup. Shell state (variables, functions, `cwd`) persists
  across calls. _Note: `cd`/`export` mutate the real editor process._
- **`:stryke <code>`** — evaluate stryke (strykelang) source; state persists
  across calls.

## REPL

`SPC a r` (or `:repl [lang]`) opens a full-screen REPL panel fronting all of the
embedded languages behind one read-eval-print loop:

- **Enter** evaluates, **Alt-Enter** inserts a newline.
- **Tab** / **Shift-Tab** cycle the active language.
- **↑/↓** or **C-p/C-n** browse per-language history.
- **C-l** clears the transcript, **PgUp/PgDn** scroll, **Esc** closes.

`:repl awk` (etc.) opens directly on a given language. Per-language input history
is persisted to `~/.zemacs/repl-history.toml`.

## Startup scripts

At startup zemacs loads these files from the config directory (`~/.zemacs/`) if
they exist, best-effort (errors surface on the status line):

- `init.el` — evaluated as Emacs Lisp.
- `init.vim` — evaluated as Vimscript.
