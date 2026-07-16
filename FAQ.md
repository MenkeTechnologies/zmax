# zmax FAQ

Frequently asked questions about zmax. Open this any time with `SPC h f`.

## What is zmax?

A modal IDE written in Rust. It ships tree-sitter syntax, LSP, and multiple
selections, and targets **vim/emacs semantics**. The default keymap is vim
(operator-pending edits like `dd`, `dw`, `cw`, `yy` are emulated on the engine),
with emacs- and Spacemacs-style functionality layered on top.

## What makes zmax distinct?

The keys you press are the keys vim binds. On top of vim, zmax adds an emacs
`C-` readline layer and a Spacemacs `SPC` leader tree, plus embedded scripting,
a REPL, an org-mode, a magit-style git UI, a hex editor, an integrated terminal,
true buffer narrowing, and more (see "What's built in?").

## Which keybindings can I use — vim, emacs, or Spacemacs?

All three, simultaneously. Normal/insert/visual vim keys work as in vim; emacs
readline chords (`C-a`, `C-e`, `C-k`, …) work in insert/command lines; and the
Spacemacs `SPC` leader tree is available in normal/visual mode. Coverage against
each source is tracked honestly in `docs/keybinding_report.md`.

## Does zmax have "layers" like Spacemacs?

No — and it doesn't need them. Everything is baked into the binary, so there is
nothing to install or enable. The Spacemacs `SPC h l` ("search layers") binding
has no analogue here by design.

## Where does my config live?

- **Global:** `~/.zmax/config.toml` (open it with `SPC f e i`, the "init" file).
- **Per-project:** `<project>/.zmax/config.toml` (open it with `SPC p e`).
- **Languages:** `~/.zmax/languages.toml` and `<project>/.zmax/languages.toml`.
- Search every live config variable with `SPC h .` (copies the dotted path on
  select, ready to paste into config.toml).

## How do I get help inside the editor?

- `SPC h h` (or `:help`) — searchable Help browser over every command, key, and topic.
- `SPC h d f` / `SPC h d v` — describe the function/variable at point (via LSP hover).
- `SPC h d m` — describe the current modes; `SPC h d p` — describe the language package.
- `SPC h m` — search man pages; `SPC h i` — search GNU info manuals (seeded at point).
- `SPC h n` — browse the release notes (CHANGELOG); `SPC h f` — this FAQ.

## What scripting languages are embedded?

Five, all pure-Rust crates compiled into the binary (no FFI between them):
**elisp** (`:elisp`), **vimscript** (`:vim`), **awk** (`:awk`), and on unix
**zsh** (`:zsh`) and **stryke** (`:stryke`). `SPC a r` (or `:repl`) opens a REPL
fronting all of them. `~/.zmax/init.el` and `init.vim` are sourced at startup.
These live behind the `scripting` Cargo feature (on by default).

## What's built in?

Snippet library (`:snippets`), hex editor (`:hex`), diff/merge (`:diff`/`:merge`),
a magit-style git UI, org-mode, a 200+ command selection-transform library,
an IDE workbench (`:ide` / `F2`), an integrated terminal (`:terminal`), the Help
browser, a startify-style start screen, and Wildfire expand-selection (`<ret>`).

## How does narrowing work?

`SPC n r` narrows the buffer to the selected region (Emacs narrow-to-region):
`gg`/`G`, select-all, and visibility confine to the region, and the bounds track
your edits. `SPC n f`/`SPC n p` narrow to the enclosing function/page. `SPC n F`/
`SPC n P` do the same in an *indirect* split — only that view narrows, the
original stays full. `SPC n w` widens (reveals the whole buffer again).

## How do I change the theme?

`SPC T c` opens a theme picker with live preview (or `:theme <name>`). The
default colorscheme is `zgui-cyberpunk`. Themes and other settings are also
editable in the Preferences window.

## How do I set up a language server / why isn't linting working?

LSP is configured per language in `languages.toml`. To debug a buffer's setup:
`SPC e v` reports the attached language servers, which provide diagnostics, and
the current diagnostic count; `SPC e h` describes each checker's capabilities.
Jump through diagnostics with `SPC e n`/`SPC e p` and list them with `SPC e l`.

## What version am I running?

`SPC h d s` copies the version, OS, and architecture to the clipboard (handy for
bug reports). `zmax --version` prints it on the command line.

## How do I install / build it?

Install: `brew install MenkeTechnologies/menketech/zmax`. Build from source:
`cargo build --bin zmax` (add `--no-default-features --features git` for a
leaner binary without the embedded scripting languages).

## Where do I report bugs or read more?

See the book in `book/src/` (rendered docs), the `README.md`, and the honest
port report in `docs/port_report.md`. Bug reports: the project's GitHub issues.
