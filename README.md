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

zemacs starts from the Helix/vim-style modal core — selection-first editing,
tree-sitter syntax, LSP, multiple selections — and is being built out toward
full Spacemacs-style functionality (layered keymaps, an extension layer, and
editor-as-environment workflows) on top of that base.

## Status

Early. This is the vendored Helix base (v25.7.1) with the binary renamed to
`zemacs`. Build-out toward the Vim/Neovim + Emacs + Spacemacs feature set is in
progress.

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
