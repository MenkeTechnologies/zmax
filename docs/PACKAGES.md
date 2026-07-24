# The zmax plugin package manager

zmax has a built-in package manager for its **native (compiled Rust) plugins** —
the `cdylib`s built against the [`zmax-native`](../zmax-native) SDK. Helix, which
zmax forks, ships no plugin system at all; zmax installs a third-party compiled
plugin from `owner/repo` into a content-addressed global store and loads it at
runtime by **mmap** (`dlopen`), with SHA-256 integrity pinning and zero editor
recompile.

It is **global only**: one store under `~/.zmax/pkg/`, no per-project manifest or
lockfile. The whole workflow is one line per plugin in your config:

```
:zmax-native get owner/repo
```

On first launch that installs the plugin and loads it; every launch after, the
same line loads it from the store with no network. `:zmax-native` needs `git` on
`PATH` for remote sources and `cargo` for plugins shipped as source (no prebuilt
`cdylib`).

Ported from zshrs's [`znative`](https://github.com/MenkeTechnologies/zshrs/blob/main/docs/ZNATIVE.md)
package manager, reduced to zmax's single plugin kind (native) — an editor has no
shell to source script plugins into.

## Commands

All package-manager verbs are subcommands of `:zmax-native` (aliased `:plugin`).

| Command (aliases)            | Arguments       | What it does |
| ---------------------------- | --------------- | ------------ |
| `add` (`install`, `i`)       | `SOURCE…`       | Resolve, build if needed, install into the store, record in the index, and load. Reinstalls (force) if already present. Multiple sources allowed. |
| `get` (`ensure`)             | `NAME_or_SOURCE…` | Load from the store, installing on first use — **zero network** once stored. This is the config startup line. |
| `sync`                       | —               | Load every installed plugin (config startup, no arguments). |
| `remove` (`rm`, `uninstall`) | `NAME…`         | Unload, delete the store copy, drop the index row. |
| `registry` (`installed`)     | —               | List installed plugins: `name version source`. |
| `info` (`show`)              | `NAME`          | Full record: name, version, source, store path, cdylib, integrity. |
| `update` (`upgrade`, `up`)   | `[NAME]`        | Re-resolve and reinstall from the recorded source (one, or all) — pulls latest upstream and rebuilds. |
| `gc`                         | `[--dry-run\|-n]` | Remove `store/<name>@<version>/` directories not pinned by the index (orphans from upgrades) plus the `git/` clone cache. |
| `clean`                      | —               | Clear the scratch directories (`git/`, `cache/`, `bin/`); the store and index are untouched. |

Raw host verbs (not the package manager, kept for direct use):

| Command  | Arguments | What it does |
| -------- | --------- | ------------ |
| `load`   | `PATH…`   | `dlopen` a `cdylib` by file path directly (no store, no index). |
| `unload` | `NAME…`   | Unload a loaded plugin from memory (does not delete it). |
| `list`   | —         | List the currently **loaded** plugins (vs `registry`, which lists installed). |

Errors surface in the editor status line as `plugin: <reason>`.

## Sources

The `add`/`get`/`update` spec is auto-classified:

| Form                                  | Example                                | Resolves to |
| ------------------------------------- | -------------------------------------- | ----------- |
| `owner/repo`                          | `MenkeTechnologies/zmax-hello`         | `git clone https://github.com/owner/repo` |
| `github:owner/repo`                   | `github:owner/repo`                    | GitHub clone (explicit) |
| `git+URL`                             | `git+https://gitlab.com/team/plug.git` | `git clone URL` |
| a URL ending `.git` or with `://`     | `https://example.com/x.git`            | `git clone URL` |
| `path:DIR`                            | `path:./examples/hello-plugin`         | local directory (no network) |
| an absolute / `./` / `../` / `~` path | `~/src/my-plugin`                      | local directory (no network) |

**Install by version** — any remote form may carry an `@ref` suffix (split after
the last `/`) to pin a tag, branch, or commit: `owner/repo@v1.2.0`,
`git+https://host/x.git@main`. The pin is **recorded** in the index
(`source = github:owner/repo@v1.2.0`), so `registry` shows it, `update`
re-fetches that exact ref (not HEAD), and `get owner/repo@v1.2.0` matches only
that pin. Clones are shallow (`git clone --depth 1 [--branch REF]`); an arbitrary
commit sha a shallow `--branch` clone can't reach falls back to a full clone +
`git checkout`.

## How a plugin is built

A native plugin is loaded from a `lib*.{dylib,so}`. When the resolved source has
no prebuilt library at its root, `:zmax-native add` runs `cargo build --release` in
the staged tree and stages the produced `cdylib`. The crate must declare:

```toml
[lib]
crate-type = ["cdylib"]
```

The kind is determined from an optional `zmax-native.toml` first, then by layout:

1. an explicit `[native]` table in `zmax-native.toml`, **or**
2. a prebuilt `lib*.{dylib,so}` at the tree root, **or**
3. a `Cargo.toml` whose `[lib] crate-type` includes `cdylib`.

Anything else is refused — it is not a zmax plugin.

## The store

Everything lives under `~/.zmax/pkg/` (override the root with `ZMAX_PKG_DIR`):

```
~/.zmax/pkg/
  store/<name>@<version>/   # the installed plugin (content-addressed)
  installed.toml            # the global index — the source of truth
  git/                      # scratch: remote clones land here, then copy to store/
  cache/  bin/              # internal scratch
```

The copy into `store/` excludes `.git/` and `target/`, so the store holds only
loadable content. Each install is SHA-256 pinned as `sha256-<hex>` in
`installed.toml`. A record:

```toml
[[package]]
name = "hello"
version = "0.1.0"
source = "github:owner/repo"
kind = "native"
integrity = "sha256-…"
lib = "libhello.dylib"   # the cdylib that :zmax-native load dlopens (mmap)
```

## `zmax-native.toml` (optional manifest)

A plugin repo may ship a `zmax-native.toml` at its root to declare metadata and
the build recipe explicitly (it overrides auto-detection):

```toml
[plugin]
name = "hello"
version = "0.1.0"
description = "example zmax plugin"

[native]
lib = "hello"      # produces lib<lib>.{dylib,so}
# build = true     # run `cargo build`; defaults to true when a Cargo.toml is
                   # present and no prebuilt cdylib sits at the tree root
```

An ordinary cdylib crate needs no `zmax-native.toml` at all.

## In your config

List the plugins you want with `:zmax-native get owner/repo`, one per line. First
launch installs each; later launches load from the store with no network. A bare
`:zmax-native sync` loads everything already in the store — handy if you prefer to
`:zmax-native add` interactively and keep just one line at startup.

## Examples

```
:zmax-native add path:./zmax-native/examples/hello-plugin   # local checkout
:zmax-native add owner/repo                                 # install (force) + load
:zmax-native add owner/repo@v0.2.1                          # pinned ref
:zmax-native get owner/repo                                 # install-on-first-use, else load
:zmax-native registry                                       # what's installed
:zmax-native info hello                                     # details for one
:zmax-native update                                         # reinstall everything from source
:zmax-native gc --dry-run                                   # what gc would reclaim
:zmax-native remove hello                                   # unload + delete
```

## Published example plugins

Installable standalone plugin repos, each an ordinary `cdylib` crate depending on
the [`zmax-native`](../zmax-native) SDK — the shape a third-party plugin takes:

| Repo | Command(s) | What it does |
| ---- | ---------- | ------------ |
| [`MenkeTechnologies/zmax-native-wc`](https://github.com/MenkeTechnologies/zmax-native-wc)     | `:wc`                    | line/word/char/byte counts of the buffer on the status line |
| [`MenkeTechnologies/zmax-native-uuid`](https://github.com/MenkeTechnologies/zmax-native-uuid) | `:uuid`, `:uuid-upper`   | insert a random UUIDv4 at the cursor |
| [`MenkeTechnologies/zmax-native-toc`](https://github.com/MenkeTechnologies/zmax-native-toc)   | `:toc`                   | insert a Markdown table of contents from the buffer's headings |

```
:zmax-native add MenkeTechnologies/zmax-native-wc
:zmax-native add MenkeTechnologies/zmax-native-uuid
:zmax-native add MenkeTechnologies/zmax-native-toc
```

The buildable, in-tree SDK examples live under
[`zmax-native/examples/`](../zmax-native/examples) (hello, insert-date,
buffer-stats, trim-trailing, banner).
