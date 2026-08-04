# Embedded scripting

zmax embeds several scripting interpreters directly in the IDE binary, so
you can evaluate scripts against the live buffer with no external process. Each
language drives the editor through one uniform host API.

| Language        | Command(s)                     | Interpreter   | Platforms |
| --------------- | ------------------------------ | ------------- | --------- |
| Emacs Lisp      | `:elisp` (`:eval-expression`, `:el`) | `elisprs`     | all       |
| Vimscript (VimL)| `:vim` (`:viml`, `:vimscript`) | `vimlrs`      | all       |
| AWK             | `:awk` (`:awk-filter`)         | `awkrs`       | all       |
| zsh             | `:zsh` (`:zshell`)             | `zshrs`       | unix only |
| stryke          | `:stryke` (`:st`)             | `strykelang`  | unix only |
| Ruby            | `:ruby` (`:rb`)               | `rubylang`    | unix only |
| PHP             | `:php`                        | `phplang`     | unix only |
| Python          | `:python` (`:py`)            | `pythonrs`    | unix only |
| JavaScript      | `:node` (`:js`, `:javascript`) | `node-js`   | unix only |
| arb             | `:arb` (`:arb-filter`)        | `arblang`     | unix only |
| Tcl             | `:tcl` (`:tclsh`)             | `tclrs`       | unix only |
| R               | `:rlang` (`:rscript`)         | `rlang`       | unix only |

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
- **`:ruby <code>`** — evaluate Ruby source; captured `puts`/`print` output or
  the value's `inspect` is shown.
- **`:php <code>`** — evaluate PHP source (the `<?php` open tag is optional);
  captured `echo`/`print` output is shown.
- **`:python <code>`** — evaluate Python source; captured `print` output or the
  value's `repr` is shown.
- **`:node <code>`** — evaluate JavaScript source; captured `console.log` output
  or the value's `inspect` is shown.
- **`:arb <program>`** — filter the current selection (or the whole buffer when
  there is no selection) through an arb spec's `out { }` pipeline, replacing it
  with the pipeline's output as a single undo step.
- **`:tcl <script>`** — evaluate Tcl source; what the script printed is shown,
  or the value of its last command when it printed nothing. State (`set`,
  `proc`) persists across calls. The interpreter runs on its own thread with the
  large stack tclrs's nesting limit is sized against, so a deep `proc` recursion
  cannot overflow the editor's stack. Expressions must be braced — `expr {$a +
  1}`, not `expr $a + 1` — which is what tclrs's compiler accepts today (and the
  idiomatic Tcl spelling anyway).
- **`:rlang <code>`** — evaluate R source; R's own transcript (autoprint,
  `print`, `cat`) is shown. Named `:rlang` because `:r` is vim's `:read`.

## Polyglot pipelines (`:xpipe`)

`:xpipe` filters each selection through a **chain** of the embedded languages,
in this process. Stages are separated by a whitespace-delimited `|>`:

```
:xpipe awk '{print $2}' |> php 'echo strtoupper($stdin);' |> ruby 'stdin.reverse'
```

Nothing forks. `:pipe` spawns a shell per selection and moves the text through
pipe file descriptors; every `:xpipe` stage is a call into an interpreter that
is already linked into the binary, so an N-stage chain costs N function calls
rather than N `fork`+`execve` pairs. The whole chain lands as one undo step, and
it runs over every selection.

Each stage receives the previous stage's output bound to a variable named
`stdin`, spelled in that language's own syntax:

| Stage language | Binding |
| -------------- | ------- |
| `awk`, `arb` | the record stream — these are line filters and take input natively |
| `ruby`, `python`, `node`, `rlang`, `tcl`, `elisp` | `stdin` |
| `php`, `zsh`, `stryke` | `$stdin` |
| `vim` | `g:stdin` |

A stage's output is what that language's own `:` command would have shown: what
the program printed, or its last value when it printed nothing.

- **`:xpipe <chain>`** (`:xp`, `:|>`) — replace each selection with the chain's
  output.
- **`:xpipe-to <chain>`** — run the chain and discard the output.
- **`:xpipe-insert <chain>`** / **`:xpipe-append <chain>`** — run the chain with
  no input and insert/append its output at each selection.

Notes:

- A bare `|` is live syntax in most of these languages (awk's `print | "cmd"`,
  ruby/JS block parameters, zsh pipelines), which is why the separator is `|>`.
  Write `\|>` for a literal one inside a stage.
- A stage may be wrapped in single quotes out of shell habit — they are
  stripped. Double quotes are program text and are left alone.
- `elisp` stages are pure text filters: unlike `:elisp`, they do not mirror the
  live buffer, because the pipeline writes the result itself.
- Failures name the stage: `xpipe: stage 2/3 (ruby): …`.

## REPL

`SPC a r` (or `:repl [lang]`) opens a full-screen REPL panel fronting all of the
embedded languages behind one read-eval-print loop:

- **Enter** evaluates, **Alt-Enter** inserts a newline.
- **Tab** / **Shift-Tab** cycle the active language.
- **↑/↓** or **C-p/C-n** browse per-language history.
- **C-l** clears the transcript, **PgUp/PgDn** scroll, **Esc** closes.

`:repl awk` (etc.) opens directly on a given language. Per-language input history
is persisted to `~/.zmax/repl-history.toml`.

## Startup scripts

At startup zmax loads these files from the config directory (`~/.zmax/`) if
they exist, best-effort (errors surface on the status line):

- `init.el` — evaluated as Emacs Lisp.
- `init.vim` — evaluated as Vimscript.
