```
███████╗███████╗███╗   ███╗ █████╗  ██████╗███████╗
╚══███╔╝██╔════╝████╗ ████║██╔══██╗██╔════╝██╔════╝
  ███╔╝ █████╗  ██╔████╔██║███████║██║     ███████╗
 ███╔╝  ██╔══╝  ██║╚██╔╝██║██╔══██║██║     ╚════██║
███████╗███████╗██║ ╚═╝ ██║██║  ██║╚██████╗███████║
╚══════╝╚══════╝╚═╝     ╚═╝╚═╝  ╚═╝ ╚═════╝╚══════╝
```

[![Build](https://github.com/MenkeTechnologies/zemacs/actions/workflows/build.yml/badge.svg)](https://github.com/MenkeTechnologies/zemacs/actions/workflows/build.yml)
![Rust](https://img.shields.io/badge/Rust-2021-05d9e8?style=flat-square)
![license](https://img.shields.io/badge/license-MPL--2.0-39ff14?style=flat-square)
[![docs](https://img.shields.io/badge/docs-online-9b5de5?style=flat-square)](https://menketechnologies.github.io/zemacs/)
![status](https://img.shields.io/badge/status-stable-39ff14?style=flat-square)

### `[A MODAL EDITOR ON THE HELIX ENGINE // VIM KEYS · EMACS · SPACEMACS]`

# zemacs

A modal text editor in Rust, forked from [Helix](https://github.com/helix-editor/helix).

zemacs runs on the Zemacs engine — tree-sitter syntax, LSP, multiple
selections — but targets **vim/emacs semantics**, not Zemacs's selection-first
model. The default keymap is vim: the keys you press are the keys vim binds,
including operator-pending edits (`dd`, `dw`, `cw`, `yy`) emulated on the Zemacs
engine, with emacs and Spacemacs-style functionality layered on top.

## Port report

Coverage is tracked by a port report measuring zemacs against the
**exhaustive, cited** feature surface of Vim/Neovim, Emacs, and Spacemacs —
inventory items parsed from the Neovim runtime docs, the GNU Emacs manual
indexes, and the Spacemacs documentation.

Live numbers (denominator, ported, partial, per-source breakdown, and item
detail) are in the generated report — never hardcoded here, so they cannot go
stale: [`docs/port_report.md`](docs/port_report.md) (styled HTML:
`docs/port_report.html`).

For the **keybinding surface specifically** (vim/neovim normal/visual/insert
keys, the Emacs Key Index, and the Spacemacs `SPC` tree — excluding
ex-commands, options, functions and `M-x`), see the focused
[`docs/keybinding_report.md`](docs/keybinding_report.md) (styled HTML:
`docs/keybinding_report.html`).

The numerator is re-derived from zemacs source on every run; the only curated
artifact is `port/mapping.json`, and every mapping must point at a real zemacs
command — a mapping to non-existent code is flagged as broken, not counted. See
[`port/README.md`](port/README.md) for the methodology and the honesty
contract.

Regenerate:

```sh
python3 scripts/gen_port_report.py
```

## Install

```sh
brew install MenkeTechnologies/menketech/zemacs
```

Tagged releases (`git tag v0.1.0 && git push --tags`) build per-target tarballs
(macOS arm64/x86_64, Linux arm64/x86_64) bundling the `zemacs` binary with its
tree-sitter runtime, publish them to the GitHub release, and bump the
[homebrew-menketech](https://github.com/MenkeTechnologies/homebrew-menketech)
formula — see `.github/workflows/release.yml`. The tap update needs a
`HOMEBREW_TAP_TOKEN` repo secret (a PAT with write access to the tap).

## Embedded scripting

**A world first: the only editor to embed 5 scripting languages with zero
external dependencies and no FFI between them** — every interpreter is a
pure-Rust crate compiled into the binary, sharing one host API rather than
bridging through a C ABI.

zemacs embeds several scripting interpreters in the binary, evaluated against the
live buffer: **elisp** (`:elisp`), **vimscript** (`:vim`), **awk** (`:awk`), plus
**zsh** (`:zsh`) and **stryke** (`:stryke`) on unix. `SPC a r` (or `:repl`) opens
a REPL fronting all of them; `~/.zemacs/init.el` and `init.vim` are sourced at
startup. See [`book/src/scripting.md`](book/src/scripting.md).

## Built-in TUIs

zemacs ships a set of interactive terminal panels for tasks that usually mean
leaving the editor:

- **Snippet library** (`:snippets`) — a CRUD editor over reusable snippets
  stored in `snippets.toml`. Type a snippet's trigger word and press `Tab` to
  expand its body with live tab stops (`${1:…}`/`$0`); triggers are scoped per
  language.
- **Hex editor** (`:hex`) — a byte-faithful xxd-style viewer/editor; binary
  files open here automatically instead of being rejected, and `Ctrl-s` writes
  the raw bytes back.
- **Merge & diff** — `:diff` shows the buffer against its git `HEAD`, and
  `:merge` opens a JetBrains-style 3-pane (ours/result/theirs) conflict
  resolver with a diff3 base pane; `]n`/`[n` jump between conflict markers.
- **Magit-style git** — interactive rebase, per-hunk staging, and branch/stash
  menus.
- **Org-mode** — outline folding, `TODO` state cycling, capture, and a
  date-aware agenda.
- **Wildfire** — press `<ret>` in normal mode to select the closest text
  object and again to grow to the next enclosing one; `<backspace>` shrinks.

## Build

```sh
cargo build --bin zemacs
./target/debug/zemacs
```

The toolchain floats to `stable` (see `rust-toolchain.toml`).

The embedded scripting languages live behind the `scripting` Cargo feature (on by
default). To build a leaner binary without them — dropping every interpreter
crate from the dependency graph — disable default features and keep `git`:

```sh
cargo build --bin zemacs --no-default-features --features git
```

## License

Zemacs-derived source is licensed under the Mozilla Public License 2.0; see
`LICENSE`. Provenance and licensing details are in `ATTRIBUTION.md`.
