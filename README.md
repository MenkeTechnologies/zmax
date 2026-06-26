```
███████╗███████╗███╗   ███╗ █████╗  ██████╗███████╗
╚══███╔╝██╔════╝████╗ ████║██╔══██╗██╔════╝██╔════╝
  ███╔╝ █████╗  ██╔████╔██║███████║██║     ███████╗
 ███╔╝  ██╔══╝  ██║╚██╔╝██║██╔══██║██║     ╚════██║
███████╗███████╗██║ ╚═╝ ██║██║  ██║╚██████╗███████║
╚══════╝╚══════╝╚═╝     ╚═╝╚═╝  ╚═╝ ╚═════╝╚══════╝
```

# zemacs

A modal text editor in Rust, forked from [Helix](https://github.com/helix-editor/helix).

zemacs runs on the Helix engine — tree-sitter syntax, LSP, multiple
selections — but targets **vim/emacs semantics**, not Helix's selection-first
model. The default keymap is vim: the keys you press are the keys vim binds,
including operator-pending edits (`dd`, `dw`, `cw`, `yy`) emulated on the Helix
engine. Build-out continues toward full vim coverage and emacs/Spacemacs-style
functionality on top of that base.

## Status

Early. Vendored Helix base (v25.7.1), binary renamed to `zemacs`, now shipping
a **vim default keymap** (`helix-term/src/keymap/vim.rs`) in place of Helix's
selection-first defaults. Build-out toward the Vim/Neovim + Emacs + Spacemacs
feature set is in progress and tracked by the port report below.

## Port report

The build-out is tracked by a port report measuring zemacs against the
**exhaustive, cited** feature surface of Vim/Neovim, Emacs, and Spacemacs —
inventory items parsed from the Neovim runtime docs, the GNU Emacs manual
indexes, and the Spacemacs documentation.

Live numbers (denominator, ported, partial, per-source breakdown, and item
detail) are in the generated report — never hardcoded here, so they cannot go
stale: [`docs/port_report.md`](docs/port_report.md) (styled HTML:
`docs/port_report.html`).

The numerator is re-derived from zemacs source on every run; the only curated
artifact is `port/mapping.json`, and every mapping must point at a real zemacs
command — a mapping to non-existent code is flagged as broken, not counted. See
[`port/README.md`](port/README.md) for the methodology and the honesty
contract.

Regenerate:

```sh
python3 scripts/gen_port_report.py
```

## Build

```sh
cargo build --bin zemacs
./target/debug/zemacs
```

The toolchain floats to `stable` (see `rust-toolchain.toml`).

## License

Helix-derived source is licensed under the Mozilla Public License 2.0; see
`LICENSE`. Provenance and licensing details are in `ATTRIBUTION.md`.
