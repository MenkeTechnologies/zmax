```
███████╗███╗   ███╗ █████╗ ██╗  ██╗
╚══███╔╝████╗ ████║██╔══██╗╚██╗██╔╝
  ███╔╝ ██╔████╔██║███████║ ╚███╔╝ 
 ███╔╝  ██║╚██╔╝██║██╔══██║ ██╔██╗ 
███████╗██║ ╚═╝ ██║██║  ██║██╔╝ ██╗
╚══════╝╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝
```

[![Build](https://github.com/MenkeTechnologies/zmax/actions/workflows/build.yml/badge.svg)](https://github.com/MenkeTechnologies/zmax/actions/workflows/build.yml)
![Rust](https://img.shields.io/badge/Rust-2021-05d9e8?style=flat-square)
![license](https://img.shields.io/badge/license-MPL--2.0-39ff14?style=flat-square)
[![docs](https://img.shields.io/badge/docs-online-9b5de5?style=flat-square)](https://menketechnologies.github.io/zmax/)
![status](https://img.shields.io/badge/status-stable-39ff14?style=flat-square)

### `[THE MOST POWERFUL CLI IDE  // VIM · EMACS · SPACEMACS SUPERSET]`

# zmax

A modal IDE in Rust

**Design goal: a maximally powerful CLI IDE with zero user configuration.**
Install the binary, open a project, and get the full power of a graphical IDE
in the terminal — LSP, a debugger, tree-sitter, fuzzy file picker, project
tree, a real PTY terminal, magit-style git, run configs, and ten embedded
scripting languages — all in one static binary, working on first launch with no
`init.el`, no plugin manager, and no setup ritual. The reference workflows are
**Spacemacs** and **JetBrains**: the same keys you already press should do the
same thing here. See [`docs/vision.md`](docs/vision.md) for the full design goal
and an honest, source-derived account of how far it's met.

zmax targets **vim/emacs semantics**.  There are four keymap presets — **spacemacs** (default), **vim**,
**helix**, and **emacs** — selectable with `keymap = "..."` in `config.toml` or
`:keymap <name>` at runtime. The default spacemacs keymap is vim keys (the keys
you press are the keys vim binds, including operator-pending edits `dd`, `dw`,
`cw`, `yy` emulated on the Zmax engine) plus the `SPC` leader and the Emacs
`C-x` prefix — both open a which-key popup. The pure `vim` preset drops the
spacemacs layer (no `SPC` leader, no which-key, `C-x` is `decrement`). emacs and
JetBrains functionality is layered on top throughout.

## Port report

Coverage is tracked by a port report measuring zmax against the
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

The numerator is re-derived from zmax source on every run; the only curated
artifact is `port/mapping.json`, and every mapping must point at a real zmax
command — a mapping to non-existent code is flagged as broken, not counted. See
[`port/README.md`](port/README.md) for the methodology and the honesty
contract.

Regenerate:

```sh
python3 scripts/gen_port_report.py
```

## Install

```sh
brew install MenkeTechnologies/menketech/zmax
```

Tagged releases (`git tag v0.1.0 && git push --tags`) build per-target tarballs
(macOS arm64/x86_64, Linux arm64/x86_64) bundling the `zmax` binary with its
tree-sitter runtime, publish them to the GitHub release, and bump the
[homebrew-menketech](https://github.com/MenkeTechnologies/homebrew-menketech)
formula — see `.github/workflows/release.yml`. The tap update needs a
`HOMEBREW_TAP_TOKEN` repo secret (a PAT with write access to the tap).

## Embedded scripting

**A world first: the only IDE to embed 10 scripting languages with zero
external dependencies and no FFI between them** — every interpreter is a
pure-Rust crate compiled into the binary, sharing one host API rather than
bridging through a C ABI.

zmax embeds ten scripting interpreters in the binary, evaluated against the
live buffer: **elisp** (`:elisp`), **vimscript** (`:vim`), **awk** (`:awk`), plus
**zsh** (`:zsh`), **stryke** (`:stryke`), **ruby** (`:ruby`), **php** (`:php`),
**python** (`:python`), **node** (`:node`) and **arb** (`:arb`) on unix.
`SPC a r` (or `:repl`) opens a REPL fronting all of them; `~/.zmax/init.el` and
`init.vim` are sourced at startup. See
[`book/src/scripting.md`](book/src/scripting.md).

## Native plugins

Beyond the embedded interpreters, zmax hosts **native (compiled Rust) plugins**:
an ordinary `cdylib` loaded at runtime with `:zmax-native load <path>` — no editor
recompile, no script glue. A plugin registers typable `:`-commands over a frozen,
versioned C ABI (the [`zmax-native`](zmax-native) SDK crate) and can read/edit the
buffer, run command lines, and post status messages through the host API. Manage
loaded plugins with `:zmax-native load|unload|list`. See
[`zmax-native/README.md`](zmax-native/README.md) and the buildable examples in
[`zmax-native/examples`](zmax-native/examples) (hello, insert-date, buffer-stats,
trim-trailing, banner).

### Package manager

zmax has a built-in **package manager** for those native plugins — Helix, which
zmax forks, ships none. Install a compiled plugin straight from a repo:

```
:zmax-native add owner/repo          # clone, cargo build, install, load
:zmax-native add owner/repo@v1.2.0   # pin a tag/branch/commit
:zmax-native add path:./my-plugin    # a local checkout (no network)
```

Installs land in a content-addressed global store at `~/.zmax/pkg/`, SHA-256
pinned in `~/.zmax/pkg/installed.toml` (the source of truth). Loading is by
**mmap**: `:zmax-native load` / `add` `dlopen`s the store's `cdylib`, so the OS pages
the library in — it is never copied into a buffer. Put one line per plugin in
your config to self-install on first launch and load with zero network after:

```
:zmax-native get owner/repo   # install if absent, else load from the store
:zmax-native sync             # load every installed plugin
```

Full command surface: `add` (`install`, `i`), `get` (`ensure`), `sync`,
`remove` (`rm`, `uninstall`), `registry` (`installed`), `info`, `update`
(`upgrade`, `up`), `gc [--dry-run]`, `clean`. A plugin repo may ship an optional
`zmax-native.toml` (`[plugin]`/`[native]`) to declare its name, version, and
build recipe; without one the kind is auto-detected from the tree. See
[`docs/PACKAGES.md`](docs/PACKAGES.md).

Installable example plugins:
[`zmax-native-wc`](https://github.com/MenkeTechnologies/zmax-native-wc) (`:wc`),
[`zmax-native-uuid`](https://github.com/MenkeTechnologies/zmax-native-uuid) (`:uuid`),
[`zmax-native-toc`](https://github.com/MenkeTechnologies/zmax-native-toc) (`:toc`),
[`zmax-native-lorem`](https://github.com/MenkeTechnologies/zmax-native-lorem) (`:lorem`) —
each a standalone `cdylib` crate built against the `zmax-native` SDK.

## Built-in TUIs

zmax ships a set of interactive terminal panels for tasks that usually mean
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
- **Transform library** — 200+ selection-transform `:` commands: JSON/CSV/TOML
  reshaping, number/stats ops, identifier-case conversion, encoders
  (Base32/Base64/Caesar/Morse/CRC32), extraction (URLs/emails/numbers),
  Markdown/typography, line ops (`:align`/`:reflow`/`:dedup`/`:sort-by-field`),
  and generators (`:uuid`/`:lorem`/`:date`/`:seq`) — each running on the
  selection (or whole buffer). When a transform needs real logic, drop to the
  embedded languages.
- **IDE workbench** (`:ide` / `F2`) — a project file-tree, a tree-sitter
  structure outline, problems/run panels, and an error-stripe minimap; the
  whole layout persists to appdata.
- **Integrated terminal** (`:terminal`) — a PTY shell in a pane, with a `C-\`
  window leader for split/focus and click-to-focus across panes.
- **Comint shell** (`:comint-shell`, emacs `M-x shell`) — a line-oriented
  subprocess buffer: type a command, `Enter` runs it, output streams into the
  scrollback; `M-p`/`M-n` walk the input history, `C-c` interrupts, `F12`
  detaches. The dumb-terminal REPL model behind inferior-lisp / `gud`.
- **Help browser** (`:help`, `SPC h h`) — searchable across every command, key,
  and topic; `SPC h` describe-* routes symbol lookups through LSP hover.
- **Start screen** — a startify-style recent-files page (frecency + MRU) shown
  on launch.
- **Wildfire** — press `<ret>` in normal mode to select the closest text
  object and again to grow to the next enclosing one; `<backspace>` shrinks.

## Embedded development (Arduino / PlatformIO)

zmax ports the Arduino IDE and PlatformIO IDE workflows by driving the same
command-line backends the official IDEs use — `arduino-cli` and `pio` — so no
GUI is needed. Per-project board settings (FQBN, serial port, baud, sketch dir,
PlatformIO environment, monitor filters) persist to
`<project-dir>/embedded.toml`; the leader menu is `SPC a v`.

- **PlatformIO environment** — `:pio-env` selects the `[env:…]` from
  `platformio.ini` (no arg fuzzy-picks; `-` clears). Every project-scoped `pio`
  action (build, upload, clean, test, check, debug, monitor, run targets) then
  targets that one environment via `-e`.
- **Build / flash** — `:arduino-compile` (Verify), `:arduino-upload`
  (compile + flash), `:arduino-compile-export` (Export Compiled Binary),
  `:arduino-burn-bootloader`; `:pio-build`, `:pio-upload`, `:pio-clean`,
  `:pio-cleanall`, `:pio-test`, `:pio-check`, `:pio-size`, `:pio-list-targets`.
  Compiler diagnostics land in the `*compilation*` list so `:next-error` walks
  avr-gcc/arm-gcc errors; uploads run live in a PTY panel.
- **Arduino compile options** — `:arduino-compile-verbose` (`-v`),
  `:arduino-compile-quiet` (`-q`), `:arduino-compile-clean` (`--clean`),
  `:arduino-compile-jobs <n>` (`-j`),
  `:arduino-compiledb` (`--only-compilation-database`, for the C/C++ LSP),
  `:arduino-compile-warnings <none|default|more|all>`,
  `:arduino-compile-profile <name>` (build against a sketch profile),
  `:arduino-compile-debug-opt` (`--optimize-for-debug`),
  `:arduino-compile-board-options <opts>` (custom board menu options),
  `:arduino-compile-build-property <key=value>` (override a build property, e.g.
  `build.extra_flags=-DDEBUG`), `:arduino-compile-output-dir <dir>` (save the
  compiled artifacts to a directory).
  Inspect the build without flashing: `:arduino-compile-properties`
  (`--show-properties`), `:arduino-compile-preprocess` (`--preprocess`),
  `:arduino-compile-dump-profile` (`--dump-profile`). Upload options:
  `:arduino-upload-verbose` (`-v`), `:arduino-upload-verify` (`--verify`),
  `:arduino-upload-programmer <id>`, `:arduino-upload-dir <dir>` /
  `:arduino-upload-file <file>` (flash a pre-built binary without recompiling).
- **PlatformIO build options** — `:pio-build-verbose` (`-v`), `:pio-build-silent` (`-s`),
  `:pio-run-jobs <n>` (parallel jobs), `:pio-build-no-auto-clean`, `:pio-target
  <name>` (any `pio run -t`), and `:pio-upload-to <port>` (flash to a specific
  port). `:pio-exec [args…]` builds and runs the native program (`pio run -t
  exec`), forwarding each argument as a `--program-arg`.
  `:pio-upload-monitor [port]` builds, flashes, then opens the serial monitor in
  one shot (`pio run -t upload -t monitor`) — PlatformIO IDE's "Upload and
  Monitor".
- **Test / analysis** — `:pio-list-tests`, `:pio-test-filter <pattern>` (run one
  suite), `:pio-check-severity <low|medium|high>`. Test options:
  `:pio-test-verbose`, `:pio-test-ignore <pattern>`, `:pio-test-without-building`
  / `-without-uploading` / `-without-testing`, `:pio-test-no-reset`. Analysis
  options: `:pio-check-verbose`, `:pio-check-json`, `:pio-check-flags <flags>`,
  `:pio-check-fail-on <low|medium|high>`, `:pio-check-skip-packages`,
  `:pio-check-src-filters <pattern>`, `:pio-check-silent` (`-s`). `:pio-test-json`
  and `:pio-check-json` dump the test / analysis results as JSON to a scratch
  buffer; `:pio-test-junit <path>` / `:pio-test-json-path <path>` write CI
  reports. `:pio-test-port <port>` runs tests over a specific serial port,
  `:pio-test-upload-port <port>` flashes the test firmware to one, and
  `:pio-test-monitor-dtr <0|1>` / `:pio-test-monitor-rts <0|1>` set the
  post-test monitor line states.
- **PlatformIO build targets** — the full `pio run -t` surface: `:pio-compiledb`
  (generate `compile_commands.json` for the C/C++ LSP), `:pio-buildfs` /
  `:pio-uploadfs` (SPIFFS/LittleFS filesystem image), `:pio-uploadeep`,
  `:pio-bootloader`, `:pio-fuses` (AVR), `:pio-nobuild` (flash without
  rebuilding), `:pio-envdump`.
- **Serial** — `:arduino-monitor` / `:pio-monitor` (live PTY serial monitor) and
  `:arduino-plotter` / `:pio-plotter`, which graph the numbers streaming from the
  board (Arduino IDE Serial Plotter). `:embedded-baud <rate>` sets the rate.
  `:arduino-monitor-raw` (no output transformations) and
  `:arduino-monitor-timestamp` (timestamp each line), `:arduino-monitor-quiet`
  (suppress non-error diagnostics), and `:arduino-monitor-describe` (list the
  port's supported settings) tune the arduino-cli monitor;
  `:pio-monitor-filter <name>` (e.g. `time`, `log2file`, `hexlify`,
  `send_on_enter`), `:pio-monitor-filters-clear`, `:pio-monitor-eol <CR|LF|CRLF>`
  and `:pio-monitor-parity <N|E|O|S|M>` tune the PlatformIO monitor, as do
  `:pio-monitor-rts <0|1>`, `:pio-monitor-dtr <0|1>`, `:pio-monitor-echo`,
  `:pio-monitor-raw`, `:pio-monitor-encoding <enc>`, `:pio-monitor-flow
  <none|rtscts|xonxoff>`, `:pio-monitor-reconnect <on|off>`,
  `:pio-monitor-quiet`, `:pio-monitor-exit-char <n>` and
  `:pio-monitor-menu-char <n>` (all persisted per project and threaded into
  every monitor invocation).
- **Boards & ports** — `:arduino-boards` (pick FQBN), `:arduino-ports` /
  `:pio-devices` (pick serial port), `:arduino-board-info`, `:pio-boards`
  (Board Explorer), `:pio-boards-installed` (installed platforms only),
  `:pio-boards-json` (Board Explorer as JSON). `:arduino-board-details-full`
  dumps the complete board detail for the selected FQBN.
  `:pio-device-logical` lists logical (disk) devices, `:pio-device-mdns`
  lists multicast-DNS / network (OTA) devices, and `:pio-device-serial` lists
  serial ports only. `:arduino-board-list-watch` watches for boards
  connecting/disconnecting, and `:arduino-board-programmers` lists the
  programmers the selected board supports; `:arduino-boards-hidden` lists every
  known board including platform-hidden variants.
- **Boards Manager** — `:arduino-core-search`, `:arduino-board-search`,
  `:arduino-core-install`, `:arduino-core-download` (fetch without installing),
  `:arduino-core-list` (`:arduino-core-list-updatable` for upgradable ones,
  `:arduino-core-list-all` for every installed platform),
  `:arduino-core-uninstall`, `:arduino-core-update-index`,
  `:arduino-core-upgrade`.
- **Library Manager** — `:arduino-lib-search` (search + install),
  `:arduino-lib-search-names <query>` (names-only), or
  `:arduino-lib-install <name>` (install by name),
  `:arduino-lib-list` (`:arduino-lib-list-updatable` for upgradable ones,
  `:arduino-lib-list-all` incl. built-in),
  `:arduino-lib-download`, `:arduino-lib-uninstall`,
  `:arduino-lib-upgrade`, `:arduino-lib-update-index`, `:arduino-lib-examples`,
  `:arduino-lib-deps`, `:arduino-lib-install-git <url>` /
  `:arduino-lib-install-zip <path>` (install from a repo or archive),
  `:arduino-lib-install-no-deps <name>` (skip dependencies);
  PlatformIO packages via `:pio-lib-search`,
  `:pio-lib-install`, `:pio-lib-list`, `:pio-lib-show`, `:pio-lib-uninstall`,
  `:pio-lib-update`, `:pio-lib-outdated`. `:pio-pkg-list-libraries` /
  `:pio-pkg-list-platforms` / `:pio-pkg-list-tools` scope the installed-package
  list to one kind.
- **arduino-cli config & cache** — `:arduino-config` (dump), `:arduino-config-get`
  / `-set` / `-add` / `-remove` / `-delete` / `-init`, `:arduino-cache-clean`,
  `:arduino-completion <shell>`. Build profiles: `:arduino-board-attach`,
  `:arduino-profile-create`, `:arduino-profile-set-default`,
  `:arduino-profile-lib-add <lib>` / `:arduino-profile-lib-remove <lib>`.
  `:arduino-daemon` runs arduino-cli as a gRPC daemon and `:arduino-version`
  reports the CLI version (`--format json`).
- **Debug** — `:arduino-debug` / `:pio-debug` launch the respective debuggers in
  a terminal panel; `:arduino-debug-info` prints the debug config without
  starting a session and `:arduino-debug-programmer <id>` debugs through a
  programmer; `:pio-debug-verbose`, `:pio-debug-interface <name>` and
  `:pio-debug-load-mode <always|modified|manual>` tune the PlatformIO session.
- **Maintenance** — `:arduino-update` / `:arduino-upgrade` / `:arduino-outdated`
  refresh and upgrade cores + libraries together (`:arduino-update-outdated`
  refreshes then reports upgradable items in one step); `:arduino-config` dumps the
  active configuration; `:pio-upgrade` upgrades PlatformIO Core itself
  (`:pio-upgrade-dev` tracks the development branch,
  `:pio-upgrade-deps-only` upgrades only its dependencies);
  `:pio-system-info` (`:pio-system-info-json` for the JSON form),
  `:pio-system-prune` (drop unused caches/packages) with
  scoped variants `:pio-prune-cache` / `:pio-prune-core` / `:pio-prune-platform`
  and `:pio-prune-dry-run` (report without deleting),
  `:pio-system-completion <shell>`, `:pio-settings-get` / `:pio-settings-set` /
  `:pio-settings-reset`, `:pio-ci <src> -b <board>` (standalone CI build);
  `:pio-home` launches the PlatformIO Home GUI (`--port`, `--host`, `--no-open`
  passed through).
- **Platforms & packages** — `:pio-platform-install <spec>` installs a
  development platform globally, `:pio-tool-install <spec>` a tool package
  (compilers, uploaders, debuggers); `:pio-pkg-exec <argv>` runs a tool from an
  installed package (e.g. `esptool.py`, `openocd`), the `-c/--call` form via
  `:pio-pkg-exec-call <argv>`, or a specific package via
  `:pio-pkg-exec-pkg <pkg> <argv>` (`-p`); `:pio-pkg-show-type <pkg> <library|platform|tool>`
  scopes registry details to a package type. Registry authoring via
  `:pio-pkg-pack` (`-o <path>` for the output), `:pio-pkg-publish` (extra args
  forward `--owner` / `--type` / `--private` / `--no-notify`),
  `:pio-pkg-unpublish` (`:pio-pkg-unpublish-undo <pkg>` restores it). Install
  options: `:pio-pkg-install-force <spec>` (`-f`),
  `:pio-pkg-install-global <spec>` (`-g`),
  `:pio-pkg-install-skip-deps <spec>` (`--skip-dependencies`),
  `:pio-lib-install-nosave <name>` (`--no-save`); `:pio-pkg-search-sort <query>
  <relevance|popularity|trending|added|updated>` sorts registry search and
  `:pio-pkg-search-page <query> <n>` pages through it. Global package management:
  `:pio-pkg-list-global` / `:pio-pkg-update-global`.
- **PlatformIO Remote** — drive a remote agent: `:pio-remote-agent-start`
  (forwards `--name` / `--share` / `--working-dir`) / `:pio-remote-agent-list`,
  `:pio-remote-devices`, `:pio-remote-monitor` (forwards `-p`/`-b`/`-f`/`--eol`/
  `--sock` etc.), `:pio-remote-run` /
  `:pio-remote-run-force` (`-r`), `:pio-remote-test`, `:pio-remote-update`
  (`--dry-run`). `:pio-remote-run` and `:pio-remote-test` forward any extra
  flags (`-t <target>`, `--upload-port`, `--test-port`, `-f`/`-i`,
  `--without-building`/`-uploading`).
- **PlatformIO account & org** — `:pio-account-login` / `-logout` / `-show`
  (forwards `--offline` / `--json-output`) /
  `-token` (forwards `--regenerate` / `--json-output`) / `-register` /
  `-password` / `-update` / `-forgot` / `-destroy`;
  organizations `:pio-org-list` / `-create` / `-add` / `-remove` / `-update` /
  `-destroy`; teams `:pio-team-list` / `-create` / `-add` / `-remove` /
  `-update` / `-destroy`; registry access `:pio-access-list` / `-grant` /
  `-revoke` / `-public` / `-private`.
- **Sketches / projects** — `:arduino-new-sketch`, `:arduino-sketch-archive`
  (`:arduino-sketch-archive-full` includes the build output),
  `:pio-init <board>`, `:pio-init-sample <board>` (with example code),
  `:pio-init-no-deps <board>` (skip installing declared dependencies),
  `:pio-init-env-prefix <prefix>` (prefix generated env names),
  `:pio-init-ide <ide>` (generate IDE integration files),
  `:pio-init-option <name=value>` (seed a `platformio.ini` option),
  `:pio-build-conf <path>` / `:pio-test-conf` / `:pio-check-conf` /
  `:pio-debug-conf` run those actions against an alternate `platformio.ini`
  (`-c`, for CI/debug config variants).
  `:pio-project-config` (computed config), `:pio-project-config-lint` (validate
  `platformio.ini`), `:pio-project-metadata` (IDE/LSP
  metadata dump); `:pio-project-config-json` / `:pio-project-metadata-json`
  emit the JSON form, and `:pio-project-metadata-path <path>` writes the metadata
  JSON to a file for external tooling.
- **Raw passthrough** — `:pio <args…>` and `:arduino-cli <args…>` (alias `:acli`)
  run any backend command in a terminal panel, so every subcommand, flag, and
  future capability of both CLIs is reachable even when it has no named command.

## zwire colorscheme sync

With `sync-zwire-theme = true` under `[editor]`, zmax bidirectionally syncs
its colorscheme with the [zwire](https://github.com/MenkeTechnologies) terminal
host, so the editor, the browser, and the HUD share one scheme. A native file
watcher on `~/.zwire/global.toml` re-applies the matching `zgui-<scheme>` /
`zgui-<scheme>-light` theme the instant zwire's scheme changes — no keypress,
no focus event — and committing a `zgui-*` theme in zmax (`:theme`, the
picker, `:theme-toggle`) writes the scheme back into `global.toml` (touching
only `scheme` / `ui.light`), which zwire fans out to the browser and HUD. Only
the eight app-shell schemes round-trip; any other theme you pick stays local.
Defaults to off. See [`book/src/themes.md`](book/src/themes.md).

## Build

```sh
cargo build --bin zmax
./target/debug/zmax
```

The toolchain floats to `stable` (see `rust-toolchain.toml`).

The embedded scripting languages live behind the `scripting` Cargo feature (on by
default). To build a leaner binary without them — dropping every interpreter
crate from the dependency graph — disable default features and keep `git`:

```sh
cargo build --bin zmax --no-default-features --features git
```

## License

Zmax-derived source is licensed under the Mozilla Public License 2.0; see
`LICENSE`. Provenance and licensing details are in `ATTRIBUTION.md`.
