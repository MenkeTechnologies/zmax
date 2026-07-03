//! Embedded / single-board development integration — the zemacs port of the
//! Arduino IDE and PlatformIO IDE workflows.
//!
//! Rather than reimplement toolchains, zemacs drives the same command-line
//! backends the official IDEs use under the hood:
//!
//!   * **`arduino-cli`** — the engine behind the Arduino IDE: board & library
//!     managers, sketch compile/upload, and the serial monitor.
//!   * **`pio`** (PlatformIO Core) — the engine behind the PlatformIO IDE:
//!     `pio run` build/upload, device list, and `pio device monitor`.
//!
//! This module is the pure, dependency-light backend: it detects the tools,
//! persists per-project board settings (FQBN, serial port, baud, sketch dir),
//! and builds the exact argument vectors for each action. The command layer
//! (`commands::typed`) wires those into `:arduino-*` / `:pio-*` typed commands,
//! feeding compile output through the Emacs `*compilation*` error list and
//! hosting the live serial monitor / upload in a PTY [`crate::ui::terminal`]
//! panel.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const ARDUINO_CLI: &str = "arduino-cli";
pub const PIO: &str = "pio";

/// Which backend toolchain drives the board for this project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// Arduino IDE's `arduino-cli`.
    #[default]
    Arduino,
    /// PlatformIO Core's `pio`.
    #[serde(rename = "platformio", alias = "pio")]
    PlatformIO,
}

impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Backend::Arduino => "arduino",
            Backend::PlatformIO => "platformio",
        }
    }

    /// The backend binary name, for `which`-style detection.
    pub fn binary(self) -> &'static str {
        match self {
            Backend::Arduino => ARDUINO_CLI,
            Backend::PlatformIO => PIO,
        }
    }
}

/// Per-project embedded settings, persisted to `<project-dir>/embedded.toml`
/// (alongside the run configs — see [`crate::run_config::project_dir`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddedSettings {
    /// Active backend toolchain.
    pub backend: Backend,
    /// Arduino Fully-Qualified Board Name, e.g. `arduino:avr:uno`.
    pub fqbn: String,
    /// Serial port device, e.g. `/dev/cu.usbmodem1401` or `COM3`.
    pub port: String,
    /// Serial monitor / upload baud rate.
    pub baud: u32,
    /// Sketch / project directory, relative to the workspace root.
    /// Empty = workspace root itself.
    pub sketch: String,
    /// PlatformIO build environment (`[env:<name>]` in `platformio.ini`).
    /// Empty = let `pio` operate on every environment (its default).
    pub env: String,
    /// PlatformIO `device monitor` filters (`-f`), e.g. `time`, `log2file`,
    /// `hexlify`, `send_on_enter`. Applied in order, one `-f` per entry.
    pub filters: Vec<String>,
    /// Serial monitor end-of-line mode: `CR`, `LF`, or `CRLF`. Empty = default.
    pub eol: String,
    /// Serial monitor parity: `N`, `E`, `O`, `S`, or `M`. Empty = default (`N`).
    pub parity: String,
    /// Initial RTS line state (`0` or `1`). Empty = leave unset (`--rts`).
    pub rts: String,
    /// Initial DTR line state (`0` or `1`). Empty = leave unset (`--dtr`).
    pub dtr: String,
    /// Enable local echo in the monitor (`--echo`).
    pub echo: bool,
    /// Disable encodings/transformations of device output (`--raw`).
    pub raw: bool,
    /// Monitor encoding, e.g. `UTF-8`, `Latin-1`, `hexlify`. Empty = default
    /// (`--encoding`).
    pub encoding: String,
    /// Enable RTS/CTS hardware flow control (`--rtscts`).
    pub rtscts: bool,
    /// Enable XON/XOFF software flow control (`--xonxoff`).
    pub xonxoff: bool,
    /// Disable automatic reconnection when the monitor link drops
    /// (`--no-reconnect`).
    pub no_reconnect: bool,
    /// Suppress non-error monitor diagnostics (`--quiet`).
    pub quiet: bool,
    /// ASCII code of the monitor exit character (default `3` = Ctrl+C). Empty =
    /// leave unset (`--exit-char`).
    pub exit_char: String,
    /// ASCII code of the monitor menu character (default `20` = Ctrl+T). Empty =
    /// leave unset (`--menu-char`).
    pub menu_char: String,
}

impl Default for EmbeddedSettings {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            fqbn: String::new(),
            port: String::new(),
            baud: 9600,
            sketch: String::new(),
            env: String::new(),
            filters: Vec::new(),
            eol: String::new(),
            parity: String::new(),
            rts: String::new(),
            dtr: String::new(),
            echo: false,
            raw: false,
            encoding: String::new(),
            rtscts: false,
            xonxoff: false,
            no_reconnect: false,
            quiet: false,
            exit_char: String::new(),
            menu_char: String::new(),
        }
    }
}

fn store_path() -> PathBuf {
    crate::run_config::project_dir().join("embedded.toml")
}

pub fn load() -> EmbeddedSettings {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|c| toml::from_str(&c).ok())
        .unwrap_or_default()
}

pub fn save(data: &EmbeddedSettings) {
    let Ok(contents) = toml::to_string_pretty(data) else {
        return;
    };
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, contents);
}

/// Apply an in-place mutation to the persisted settings and save.
pub fn update(f: impl FnOnce(&mut EmbeddedSettings)) -> EmbeddedSettings {
    let mut data = load();
    f(&mut data);
    save(&data);
    data
}

impl EmbeddedSettings {
    /// Absolute sketch/project directory (workspace root when `sketch` is empty).
    pub fn sketch_dir(&self) -> PathBuf {
        let root = zemacs_loader::find_workspace().0;
        if self.sketch.trim().is_empty() {
            root
        } else {
            root.join(&self.sketch)
        }
    }

    /// `["-e", <env>]` when a build environment is selected, else empty. Appended
    /// to every project-scoped `pio` action (`run`, `test`, `check`, `debug`,
    /// `device monitor`) so it targets a single `[env:<name>]`.
    pub fn pio_env_args(&self) -> Vec<String> {
        if self.env.trim().is_empty() {
            Vec::new()
        } else {
            vec![s("-e"), self.env.trim().to_string()]
        }
    }

    /// The shared `pio device monitor` option tail: port, baud, filters, EOL,
    /// parity, and the selected environment.
    fn pio_monitor_opts(&self) -> Vec<String> {
        let mut v = Vec::new();
        if !self.port.is_empty() {
            v.push(s("-p"));
            v.push(self.port.clone());
        }
        v.push(s("-b"));
        v.push(self.baud.to_string());
        for f in &self.filters {
            v.push(s("-f"));
            v.push(f.clone());
        }
        if !self.eol.trim().is_empty() {
            v.push(s("--eol"));
            v.push(self.eol.trim().to_string());
        }
        if !self.parity.trim().is_empty() {
            v.push(s("--parity"));
            v.push(self.parity.trim().to_string());
        }
        if !self.rts.trim().is_empty() {
            v.push(s("--rts"));
            v.push(self.rts.trim().to_string());
        }
        if !self.dtr.trim().is_empty() {
            v.push(s("--dtr"));
            v.push(self.dtr.trim().to_string());
        }
        if !self.encoding.trim().is_empty() {
            v.push(s("--encoding"));
            v.push(self.encoding.trim().to_string());
        }
        if self.echo {
            v.push(s("--echo"));
        }
        if self.raw {
            v.push(s("--raw"));
        }
        if self.rtscts {
            v.push(s("--rtscts"));
        }
        if self.xonxoff {
            v.push(s("--xonxoff"));
        }
        if self.no_reconnect {
            v.push(s("--no-reconnect"));
        }
        if self.quiet {
            v.push(s("--quiet"));
        }
        if !self.exit_char.trim().is_empty() {
            v.push(s("--exit-char"));
            v.push(self.exit_char.trim().to_string());
        }
        if !self.menu_char.trim().is_empty() {
            v.push(s("--menu-char"));
            v.push(self.menu_char.trim().to_string());
        }
        v.extend(self.pio_env_args());
        v
    }
}

/// Is `binary` on `PATH`?
pub fn tool_available(binary: &str) -> bool {
    zemacs_stdx::env::binary_exists(binary)
}

// ── Argument-vector builders ─────────────────────────────────────────────────
//
// Each returns the full argv (`[program, arg, ...]`). The command layer either
// joins it into a shell string (compile → `*compilation*`) or spawns argv[0]
// with argv[1..] in a PTY panel (upload / monitor).

fn s(v: &str) -> String {
    v.to_string()
}

/// `arduino-cli compile --fqbn <fqbn> <sketch>`
pub fn arduino_compile(settings: &EmbeddedSettings) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    Ok(vec![
        s(ARDUINO_CLI),
        s("compile"),
        s("--fqbn"),
        settings.fqbn.clone(),
        settings.sketch_dir().to_string_lossy().into_owned(),
    ])
}

/// `arduino-cli compile [extra…] --fqbn <fqbn> <sketch>` — compile with extra
/// verified flags spliced before the sketch path (`-v`, `--clean`, `-j <n>`,
/// `--warnings <lvl>`, `--only-compilation-database`, `--optimize-for-debug`,
/// `--profile <name>`). Verified against `arduino-cli compile --help` on 1.5.1.
pub fn arduino_compile_with(
    settings: &EmbeddedSettings,
    extra: &[String],
) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    let mut v = vec![s(ARDUINO_CLI), s("compile"), s("--fqbn"), settings.fqbn.clone()];
    v.extend(extra.iter().cloned());
    v.push(settings.sketch_dir().to_string_lossy().into_owned());
    Ok(v)
}

/// `arduino-cli compile --upload -p <port> [extra…] --fqbn <fqbn> <sketch>` —
/// build + flash with extra verified flags (`--verify`, `--programmer <p>`).
pub fn arduino_upload_with(
    settings: &EmbeddedSettings,
    extra: &[String],
) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    if settings.port.is_empty() {
        return Err("no serial port selected — run :arduino-ports".into());
    }
    let mut v = vec![
        s(ARDUINO_CLI),
        s("compile"),
        s("--upload"),
        s("-p"),
        settings.port.clone(),
        s("--fqbn"),
        settings.fqbn.clone(),
    ];
    v.extend(extra.iter().cloned());
    v.push(settings.sketch_dir().to_string_lossy().into_owned());
    Ok(v)
}

/// `arduino-cli upload --input-dir|--input-file <path> -p <port> --fqbn <fqbn>` —
/// flash a *pre-built* binary without compiling (the Arduino IDE "Upload Using
/// Programmer" of an exported build). `flag` is `--input-dir` or `--input-file`.
pub fn arduino_upload_input(
    settings: &EmbeddedSettings,
    flag: &str,
    path: &str,
) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    if settings.port.is_empty() {
        return Err("no serial port selected — run :arduino-ports".into());
    }
    Ok(vec![
        s(ARDUINO_CLI),
        s("upload"),
        s(flag),
        s(path),
        s("-p"),
        settings.port.clone(),
        s("--fqbn"),
        settings.fqbn.clone(),
    ])
}

/// `arduino-cli compile --upload -p <port> --fqbn <fqbn> <sketch>`
///
/// The Arduino IDE "Upload" button compiles the sketch *then* flashes it; plain
/// `arduino-cli upload` explicitly does **not** compile first ("This does NOT
/// compile the sketch prior to upload"), so it would flash a stale/absent
/// binary. `compile --upload` (flag `-u`) does both in one step, matching the
/// IDE.
pub fn arduino_upload(settings: &EmbeddedSettings) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    if settings.port.is_empty() {
        return Err("no serial port selected — run :arduino-ports".into());
    }
    Ok(vec![
        s(ARDUINO_CLI),
        s("compile"),
        s("--upload"),
        s("-p"),
        settings.port.clone(),
        s("--fqbn"),
        settings.fqbn.clone(),
        settings.sketch_dir().to_string_lossy().into_owned(),
    ])
}

/// `arduino-cli compile --fqbn <fqbn> -e <sketch>` — compile and export the
/// built binaries to the sketch folder (Arduino IDE "Export Compiled Binary").
pub fn arduino_compile_export(settings: &EmbeddedSettings) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    Ok(vec![
        s(ARDUINO_CLI),
        s("compile"),
        s("--fqbn"),
        settings.fqbn.clone(),
        s("-e"),
        settings.sketch_dir().to_string_lossy().into_owned(),
    ])
}

/// `arduino-cli burn-bootloader -p <port> --fqbn <fqbn>` (Arduino IDE
/// Tools → Burn Bootloader).
pub fn arduino_burn_bootloader(settings: &EmbeddedSettings) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    if settings.port.is_empty() {
        return Err("no serial port selected — run :arduino-ports".into());
    }
    Ok(vec![
        s(ARDUINO_CLI),
        s("burn-bootloader"),
        s("-p"),
        settings.port.clone(),
        s("--fqbn"),
        settings.fqbn.clone(),
    ])
}

/// `arduino-cli board details --fqbn <fqbn>` — board specs / menu options.
pub fn arduino_board_details(fqbn: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("board"), s("details"), s("--fqbn"), s(fqbn)]
}

/// `arduino-cli board details --fqbn <fqbn> --full` — the complete board detail
/// dump (all tools, properties, and identification info).
pub fn arduino_board_details_full(fqbn: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("board"), s("details"), s("--fqbn"), s(fqbn), s("--full")]
}

/// `arduino-cli core search <query>` — Boards Manager search.
pub fn arduino_core_search(query: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("search"), s(query)]
}

/// `arduino-cli core list` — installed platforms (Boards Manager, installed tab).
pub fn arduino_core_list() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("list")]
}

/// `arduino-cli core list --updatable` — only installed platforms with a newer
/// version available.
pub fn arduino_core_list_updatable() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("list"), s("--updatable")]
}

/// `arduino-cli core uninstall <pkg>`
pub fn arduino_core_uninstall(pkg: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("uninstall"), s(pkg)]
}

/// `arduino-cli core upgrade` — upgrade all installed platforms.
pub fn arduino_core_upgrade() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("upgrade")]
}

/// `arduino-cli core update-index` — refresh the Boards Manager index.
pub fn arduino_core_update_index() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("update-index")]
}

/// `arduino-cli lib list` — installed libraries (Library Manager, installed tab).
pub fn arduino_lib_list() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("list")]
}

/// `arduino-cli lib uninstall <name>`
pub fn arduino_lib_uninstall(name: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("uninstall"), s(name)]
}

/// `arduino-cli lib upgrade` — upgrade all installed libraries.
pub fn arduino_lib_upgrade() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("upgrade")]
}

/// `arduino-cli lib examples <name>` — list a library's example sketches.
pub fn arduino_lib_examples(name: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("examples"), s(name)]
}

/// `arduino-cli sketch archive <dir>` — zip the whole sketch (Sketch → Archive).
pub fn arduino_sketch_archive(sketch_dir: &Path) -> Vec<String> {
    vec![
        s(ARDUINO_CLI),
        s("sketch"),
        s("archive"),
        sketch_dir.to_string_lossy().into_owned(),
    ]
}

/// `arduino-cli update` — refresh the core *and* library indexes together.
pub fn arduino_update() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("update")]
}

/// `arduino-cli upgrade` — upgrade all installed cores *and* libraries.
pub fn arduino_upgrade() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("upgrade")]
}

/// `arduino-cli outdated` — list cores and libraries that can be upgraded.
pub fn arduino_outdated() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("outdated")]
}

/// `arduino-cli lib deps <name>` — dependency status for a library.
pub fn arduino_lib_deps(name: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("deps"), s(name)]
}

/// `arduino-cli config dump` — print the active arduino-cli configuration.
pub fn arduino_config_dump() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("config"), s("dump")]
}

/// `arduino-cli debug --fqbn <fqbn> -p <port> <sketch>` — launch the debugger
/// (needs a debug-capable board + programmer).
pub fn arduino_debug(settings: &EmbeddedSettings) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    if settings.port.is_empty() {
        return Err("no serial port selected — run :arduino-ports".into());
    }
    Ok(vec![
        s(ARDUINO_CLI),
        s("debug"),
        s("--fqbn"),
        settings.fqbn.clone(),
        s("-p"),
        settings.port.clone(),
        settings.sketch_dir().to_string_lossy().into_owned(),
    ])
}

/// `arduino-cli debug [extra…] --fqbn <fqbn> -p <port> <sketch>` — debugger with
/// extra verified flags spliced before the sketch (`--info`, `--programmer <p>`,
/// `--interpreter <mode>`). Verified against `arduino-cli debug --help` on 1.5.1.
pub fn arduino_debug_with(
    settings: &EmbeddedSettings,
    extra: &[String],
) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    if settings.port.is_empty() {
        return Err("no serial port selected — run :arduino-ports".into());
    }
    let mut v = vec![
        s(ARDUINO_CLI),
        s("debug"),
        s("--fqbn"),
        settings.fqbn.clone(),
        s("-p"),
        settings.port.clone(),
    ];
    v.extend(extra.iter().cloned());
    v.push(settings.sketch_dir().to_string_lossy().into_owned());
    Ok(v)
}

/// `arduino-cli monitor -p <port> -c baudrate=<baud>`
pub fn arduino_monitor(settings: &EmbeddedSettings) -> Result<Vec<String>, String> {
    if settings.port.is_empty() {
        return Err("no serial port selected — run :arduino-ports".into());
    }
    Ok(vec![
        s(ARDUINO_CLI),
        s("monitor"),
        s("-p"),
        settings.port.clone(),
        s("-c"),
        format!("baudrate={}", settings.baud),
    ])
}

/// `arduino-cli monitor -p <port> [extra…] -c baudrate=<baud>` — serial monitor
/// with extra verified flags (`--raw`, `--timestamp`, `--quiet`, `--describe`).
pub fn arduino_monitor_with(
    settings: &EmbeddedSettings,
    extra: &[String],
) -> Result<Vec<String>, String> {
    if settings.port.is_empty() {
        return Err("no serial port selected — run :arduino-ports".into());
    }
    let mut v = vec![s(ARDUINO_CLI), s("monitor"), s("-p"), settings.port.clone()];
    v.extend(extra.iter().cloned());
    v.push(s("-c"));
    v.push(format!("baudrate={}", settings.baud));
    Ok(v)
}

/// `arduino-cli board details --fqbn <fqbn> --list-programmers` — the programmers
/// a board supports (for `:arduino-upload-programmer`).
pub fn arduino_board_details_programmers(fqbn: &str) -> Vec<String> {
    vec![
        s(ARDUINO_CLI),
        s("board"),
        s("details"),
        s("--fqbn"),
        s(fqbn),
        s("--list-programmers"),
    ]
}

/// `arduino-cli board list --watch` — continuously watch for boards being
/// connected/disconnected (long-running; hosted in a PTY panel).
pub fn arduino_board_list_watch() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("board"), s("list"), s("--watch")]
}

/// `arduino-cli lib list --updatable` — only installed libraries with a newer
/// version available.
pub fn arduino_lib_list_updatable() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("list"), s("--updatable")]
}

/// `arduino-cli lib install --git-url <url>` — install a library straight from a
/// git repository (requires arduino-cli's `library.enable_unsafe_install`).
pub fn arduino_lib_install_git(url: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("install"), s("--git-url"), s(url)]
}

/// `arduino-cli lib install --zip-path <path>` — install a library from a local
/// `.zip` archive (requires arduino-cli's `library.enable_unsafe_install`).
pub fn arduino_lib_install_zip(path: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("install"), s("--zip-path"), s(path)]
}

/// `arduino-cli board listall --format json` (installed platforms' boards).
pub fn arduino_board_listall() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("board"), s("listall"), s("--format"), s("json")]
}

/// `arduino-cli board list --format json` (connected boards / ports).
pub fn arduino_board_list() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("board"), s("list"), s("--format"), s("json")]
}

/// `arduino-cli lib search <query> --format json`
pub fn arduino_lib_search(query: &str) -> Vec<String> {
    vec![
        s(ARDUINO_CLI),
        s("lib"),
        s("search"),
        s(query),
        s("--format"),
        s("json"),
    ]
}

/// `arduino-cli lib install <name>`
pub fn arduino_lib_install(name: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("install"), s(name)]
}

/// `arduino-cli lib install <name> --no-deps` — install a library without pulling
/// its declared dependencies.
pub fn arduino_lib_install_no_deps(name: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("install"), s(name), s("--no-deps")]
}

/// `arduino-cli core install <package>`
pub fn arduino_core_install(pkg: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("install"), s(pkg)]
}

/// `arduino-cli core download <package>` — fetch a core without installing it.
pub fn arduino_core_download(pkg: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("download"), s(pkg)]
}

/// `arduino-cli lib download <name>` — fetch a library without installing it.
pub fn arduino_lib_download(name: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("download"), s(name)]
}

/// `arduino-cli lib update-index` — refresh the library index.
pub fn arduino_lib_update_index() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("lib"), s("update-index")]
}

/// `arduino-cli board search [query]` — search the Boards Manager for a board.
pub fn arduino_board_search(query: &str) -> Vec<String> {
    let mut v = vec![s(ARDUINO_CLI), s("board"), s("search")];
    if !query.trim().is_empty() {
        v.push(s(query.trim()));
    }
    v
}

/// `arduino-cli cache clean` — delete the Boards/Library Manager download cache.
pub fn arduino_cache_clean() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("cache"), s("clean")]
}

/// `arduino-cli completion <shell>` — emit a shell completion script.
pub fn arduino_completion(shell: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("completion"), s(shell)]
}

/// `arduino-cli config dump` is already exposed via [`arduino_config_dump`]; this
/// is the generic `arduino-cli config <action> [args…]` for `get`, `set`,
/// `add`, `remove`, `delete`, `init` — whose value arguments the user supplies.
pub fn arduino_config(action: &str, args: &[String]) -> Vec<String> {
    let mut v = vec![s(ARDUINO_CLI), s("config"), s(action)];
    v.extend(args.iter().cloned());
    v
}

/// Generic `arduino-cli <group> <sub> [args…]` for the leaves whose arguments
/// the user supplies (`board attach`, `profile create`/`set-default`, …). Keeps
/// one builder rather than inventing a signature per rarely-used leaf.
pub fn arduino_sub(group: &str, sub: &str, args: &[String]) -> Vec<String> {
    let mut v = vec![s(ARDUINO_CLI), s(group), s(sub)];
    v.extend(args.iter().cloned());
    v
}

/// `arduino-cli <args…>` — raw passthrough so any arduino-cli command, flag, or
/// future subcommand is reachable from inside zemacs.
pub fn arduino_passthrough(args: &[String]) -> Vec<String> {
    let mut v = vec![s(ARDUINO_CLI)];
    v.extend(args.iter().cloned());
    v
}

/// `arduino-cli daemon [args…]` — run arduino-cli as a long-running gRPC daemon
/// (IDE/tooling backend). Extra args tune the server (`--port <n>`,
/// `--daemonize`, `--debug`).
pub fn arduino_daemon(args: &[String]) -> Vec<String> {
    let mut v = vec![s(ARDUINO_CLI), s("daemon")];
    v.extend(args.iter().cloned());
    v
}

/// `arduino-cli version [args…]` — the arduino-cli version (add `--format json`
/// for machine-readable output).
pub fn arduino_version(args: &[String]) -> Vec<String> {
    let mut v = vec![s(ARDUINO_CLI), s("version")];
    v.extend(args.iter().cloned());
    v
}

/// `pio <args…>` — raw passthrough so any `pio` command, flag, or future
/// subcommand (including `home`) is reachable from inside zemacs.
pub fn pio_passthrough(args: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO)];
    v.extend(args.iter().cloned());
    v
}

/// `pio home [args…]` — launch the PlatformIO Home GUI server (opens a browser
/// unless `--no-open`). Extra args tune the server: `--port <n>` (default 8008),
/// `--host <addr>`, `--no-open`, `--shutdown-timeout <secs>`, `--session-id`.
/// Verified against `pio home --help` on PlatformIO 6.1.19.
pub fn pio_home(args: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("home")];
    v.extend(args.iter().cloned());
    v
}

/// `pio run [-e env]` — build the PlatformIO project.
pub fn pio_build(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("run")];
    v.extend(settings.pio_env_args());
    v
}

/// `pio run -t upload [-e env]`
pub fn pio_upload(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("run"), s("-t"), s("upload")];
    v.extend(settings.pio_env_args());
    v
}

/// `pio run -t exec [-a <arg>]… [-e env]` — build and execute the program on the
/// native platform, passing each `program_args` entry as a separate `-a`
/// (`--program-arg`). Verified end-to-end against `pio run -t exec -a … -a …` on
/// PlatformIO 6.1.19 with the `native` platform.
pub fn pio_run_exec(settings: &EmbeddedSettings, program_args: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("run"), s("-t"), s("exec")];
    for a in program_args {
        v.push(s("-a"));
        v.push(a.clone());
    }
    v.extend(settings.pio_env_args());
    v
}

/// `pio device monitor [-p port] -b baud [-f filter…] [--eol …] [--parity …] [-e env]`
pub fn pio_monitor(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("device"), s("monitor")];
    v.extend(settings.pio_monitor_opts());
    v
}

/// `pio remote device monitor` — the serial monitor over a Remote agent. Emitted
/// bare (the agent selects the attached device); extra options are supplied via
/// [`pio_remote_device_monitor_with`].
pub fn pio_remote_device_monitor() -> Vec<String> {
    vec![s(PIO), s("remote"), s("device"), s("monitor")]
}

/// `pio remote device monitor [extra…]` — the Remote-agent serial monitor with
/// extra options forwarded. Verified against `pio remote device monitor --help`
/// (contrib-pioremote 1.0.2): shares the local monitor's flags (`-p`, `-b`,
/// `-f`, `--eol`, `--parity`, `--raw`, `--quiet`, …) and adds `--sock <dir>` for
/// a Unix-socket transport. Note: it has no `--no-reconnect`, so the persisted
/// monitor settings are not auto-threaded here — the caller passes explicit flags.
pub fn pio_remote_device_monitor_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("remote"), s("device"), s("monitor")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio device list --json-output`
pub fn pio_device_list() -> Vec<String> {
    vec![s(PIO), s("device"), s("list"), s("--json-output")]
}

/// `pio device list --logical` — logical (disk) devices rather than serial ports.
pub fn pio_device_list_logical() -> Vec<String> {
    vec![s(PIO), s("device"), s("list"), s("--logical")]
}

/// `pio device list --mdns` — multicast-DNS services (network/OTA devices).
pub fn pio_device_list_mdns() -> Vec<String> {
    vec![s(PIO), s("device"), s("list"), s("--mdns")]
}

/// `pio device list --serial` — serial ports only (explicit serial filter,
/// sibling of `--logical`/`--mdns`).
pub fn pio_device_list_serial() -> Vec<String> {
    vec![s(PIO), s("device"), s("list"), s("--serial")]
}

/// `pio project init --board <id>`
pub fn pio_init(board: &str) -> Vec<String> {
    vec![s(PIO), s("project"), s("init"), s("--board"), s(board)]
}

/// `pio pkg install -l <name>` — add a library to the project.
pub fn pio_lib_install(name: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("install"), s("-l"), s(name)]
}

/// `pio run -t clean [-e env]` — remove build artifacts (PlatformIO IDE "Clean").
pub fn pio_clean(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("run"), s("-t"), s("clean")];
    v.extend(settings.pio_env_args());
    v
}

/// `pio test [-e env]` — run the project's unit tests (PlatformIO IDE "Test").
pub fn pio_test(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("test")];
    v.extend(settings.pio_env_args());
    v
}

/// `pio test -f <pattern> [-e env]` — run only the tests matching `pattern`.
pub fn pio_test_filter(settings: &EmbeddedSettings, pattern: &str) -> Vec<String> {
    let mut v = vec![s(PIO), s("test"), s("-f"), s(pattern)];
    v.extend(settings.pio_env_args());
    v
}

/// `pio test --list-tests [-e env]` — list the project's test suites.
pub fn pio_list_tests(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("test"), s("--list-tests")];
    v.extend(settings.pio_env_args());
    v
}

/// `pio check [-e env]` — static code analysis (PlatformIO IDE "Check").
pub fn pio_check(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("check")];
    v.extend(settings.pio_env_args());
    v
}

/// `pio check --severity <low|medium|high> [-e env]` — analysis filtered by the
/// minimum defect severity.
pub fn pio_check_severity(settings: &EmbeddedSettings, severity: &str) -> Vec<String> {
    let mut v = vec![s(PIO), s("check"), s("--severity"), s(severity)];
    v.extend(settings.pio_env_args());
    v
}

/// `pio boards [query]` — the PlatformIO Board Explorer. Empty query lists all.
pub fn pio_boards(query: &str) -> Vec<String> {
    let mut v = vec![s(PIO), s("boards")];
    if !query.trim().is_empty() {
        v.push(s(query));
    }
    v
}

/// `pio boards --json-output [query]` — the Board Explorer as JSON.
pub fn pio_boards_json(query: &str) -> Vec<String> {
    let mut v = vec![s(PIO), s("boards"), s("--json-output")];
    if !query.trim().is_empty() {
        v.push(s(query));
    }
    v
}

/// `pio boards --installed [query]` — only boards from installed platforms.
pub fn pio_boards_installed(query: &str) -> Vec<String> {
    let mut v = vec![s(PIO), s("boards"), s("--installed")];
    if !query.trim().is_empty() {
        v.push(s(query));
    }
    v
}

/// `pio pkg list` — installed packages/libraries for the project.
pub fn pio_pkg_list() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("list")]
}

/// `pio pkg list --only-<scope>` — installed packages of one kind only, where
/// `scope` is `libraries`, `platforms`, or `tools`. Verified against
/// `pio pkg list --help` on PlatformIO 6.1.19.
pub fn pio_pkg_list_scope(scope: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("list"), format!("--only-{}", scope)]
}

/// `pio pkg list -g` — globally installed packages (rather than the project's).
pub fn pio_pkg_list_global() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("list"), s("-g")]
}

/// `pio pkg uninstall -l <name>` — remove a project library.
pub fn pio_pkg_uninstall(name: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("uninstall"), s("-l"), s(name)]
}

/// `pio pkg update` — update the project's installed packages.
pub fn pio_pkg_update() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("update")]
}

/// `pio pkg update -g` — update globally installed packages.
pub fn pio_pkg_update_global() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("update"), s("-g")]
}

/// `pio pkg search <query>` — search the PlatformIO registry (Library Manager).
pub fn pio_pkg_search(query: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("search"), s(query)]
}

/// `pio pkg search <query> --page <n>` — a specific page of registry results
/// (results are paginated). Verified against `pio pkg search --help` on 6.1.19.
pub fn pio_pkg_search_page(query: &str, page: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("search"), s(query), s("--page"), s(page)]
}

/// `pio pkg outdated` — list installed packages with newer versions available.
pub fn pio_pkg_outdated() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("outdated")]
}

/// `pio pkg show <pkg>` — registry details for a package.
pub fn pio_pkg_show(pkg: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("show"), s(pkg)]
}

/// `pio debug [-e env]` — the PlatformIO Unified Debugger for the project.
pub fn pio_debug(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("debug")];
    v.extend(settings.pio_env_args());
    v
}

/// `pio upgrade` — upgrade PlatformIO Core itself.
pub fn pio_upgrade() -> Vec<String> {
    vec![s(PIO), s("upgrade")]
}

// ── Flag-carrying builders ───────────────────────────────────────────────────
//
// Each appends a verified flag vector (checked against `<cmd> --help` on
// PlatformIO 6.1.19) to the base verb, then threads the selected build
// environment. The typed-command layer supplies the specific flags so every
// documented option is reachable first-class, not only via the raw `:pio`
// passthrough.

/// `pio run [extra…] [-e env]` — build with extra options (`-v`, `-s`, `-j <n>`,
/// `-t <target>`, `--upload-port <p>`, `--disable-auto-clean`).
pub fn pio_run_with(settings: &EmbeddedSettings, extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("run")];
    v.extend(extra.iter().cloned());
    v.extend(settings.pio_env_args());
    v
}

/// `pio test [extra…] [-e env]` — unit tests with extra options (`-v`, `-i
/// <pattern>`, `--without-building`, `--without-uploading`, `--without-testing`,
/// `--no-reset`).
pub fn pio_test_with(settings: &EmbeddedSettings, extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("test")];
    v.extend(extra.iter().cloned());
    v.extend(settings.pio_env_args());
    v
}

/// `pio check [extra…] [-e env]` — static analysis with extra options (`-v`,
/// `--json-output`, `--flags <f>`, `--fail-on-defect <sev>`, `--skip-packages`,
/// `--src-filters <pat>`).
pub fn pio_check_with(settings: &EmbeddedSettings, extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("check")];
    v.extend(extra.iter().cloned());
    v.extend(settings.pio_env_args());
    v
}

/// `pio debug [extra…] [-e env]` — debugger with extra options (`-v`,
/// `--interface <name>`, `--load-mode <always|modified|manual>`).
pub fn pio_debug_with(settings: &EmbeddedSettings, extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("debug")];
    v.extend(extra.iter().cloned());
    v.extend(settings.pio_env_args());
    v
}

/// `pio project init [extra…]` — scaffold/update with extra options (`--ide
/// <name>`, `--sample-code`, `-O <name=value>`, `--board <id>`).
pub fn pio_project_init_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("project"), s("init")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio pkg install [extra…]` — install with extra options (`-f`, `-g`,
/// `--skip-dependencies`, `--no-save`, `-l/-p/-t <spec>`).
pub fn pio_pkg_install_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("pkg"), s("install")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio upgrade [extra…]` — upgrade Core with extra options (`--dev`,
/// `--only-dependencies`).
pub fn pio_upgrade_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("upgrade")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio remote run [extra…] [-e env]` — remote build with extra options
/// (`-r/--force-remote`, `-v`, `--disable-auto-clean`, `-t <target>`,
/// `--upload-port <p>`, `-s`).
pub fn pio_remote_run_with(settings: &EmbeddedSettings, extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("remote"), s("run")];
    v.extend(extra.iter().cloned());
    v.extend(settings.pio_env_args());
    v
}

/// `pio remote test [extra…] [-e env]` — remote unit tests with extra options
/// (`-r/--force-remote`, `-v`, `-f <filter>`, `-i <ignore>`,
/// `--without-building`, `--without-uploading`, `--test-port <p>`,
/// `--upload-port <p>`).
pub fn pio_remote_test_with(settings: &EmbeddedSettings, extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("remote"), s("test")];
    v.extend(extra.iter().cloned());
    v.extend(settings.pio_env_args());
    v
}

/// `pio remote agent start [extra…]` — start an agent with extra options
/// (`--name <n>`, `--share <email>`, `--working-dir <dir>`).
pub fn pio_remote_agent_start_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("remote"), s("agent"), s("start")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio pkg search <query> --sort <relevance|popularity|trending|added|updated>`
/// — registry search with an explicit sort order.
pub fn pio_pkg_search_sort(query: &str, sort: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("search"), s(query), s("--sort"), s(sort)]
}

/// `pio pkg pack [extra…]` — tarball the current package with extra options
/// (`-o <path>` to choose the destination).
pub fn pio_pkg_pack_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("pkg"), s("pack")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio pkg show <pkg> --type <library|platform|tool>` — registry details scoped
/// to a package type (disambiguates same-named packages across types).
pub fn pio_pkg_show_type(pkg: &str, pkg_type: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("show"), s("--type"), s(pkg_type), s(pkg)]
}

/// `pio pkg exec -c <argv…>` — run a package tool through the `-c/--call` form
/// (a single call string), as opposed to the `exec -- <argv>` form.
pub fn pio_pkg_exec_call(args: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("pkg"), s("exec"), s("-c")];
    v.extend(args.iter().cloned());
    v
}

/// `pio pkg exec -p <pkg> -- <argv…>` — run a tool from a *specific* installed
/// package (scoped via `-p/--package`), disambiguating tools shared by name.
pub fn pio_pkg_exec_package(pkg: &str, args: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("pkg"), s("exec"), s("-p"), s(pkg), s("--")];
    v.extend(args.iter().cloned());
    v
}

/// `pio account token [extra…]` — print/regenerate the auth token with extra
/// options (`--regenerate`, `--json-output`, `-p <password>`).
pub fn pio_account_token_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("account"), s("token")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio remote update [extra…]` — update remote platforms/packages/libraries
/// with extra options (`--dry-run`).
pub fn pio_remote_update_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("remote"), s("update")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio run -t <target> [-e env]` — a built-in PlatformIO build target. Covers
/// the report targets (`size`, `envdump`, `compiledb`, `buildfs`, `cleanall`)
/// and the flashing targets (`uploadfs`, `uploadeep`, `bootloader`, `fuses`,
/// `nobuild`). Verified against `pio run --list-targets` on PlatformIO 6.1.19.
pub fn pio_run_target(settings: &EmbeddedSettings, target: &str) -> Vec<String> {
    let mut v = vec![s(PIO), s("run"), s("-t"), s(target)];
    v.extend(settings.pio_env_args());
    v
}

/// `pio run --list-targets [-e env]` — enumerate the project's build targets.
pub fn pio_list_targets(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("run"), s("--list-targets")];
    v.extend(settings.pio_env_args());
    v
}

/// `pio project config` — the project's computed configuration (all envs).
pub fn pio_project_config() -> Vec<String> {
    vec![s(PIO), s("project"), s("config")]
}

/// `pio project config --json-output` — the computed configuration as JSON.
pub fn pio_project_config_json() -> Vec<String> {
    vec![s(PIO), s("project"), s("config"), s("--json-output")]
}

/// `pio project config --lint` — validate `platformio.ini`, reporting unknown
/// options or malformed values without building.
pub fn pio_project_config_lint() -> Vec<String> {
    vec![s(PIO), s("project"), s("config"), s("--lint")]
}

/// `pio project metadata` — dump the IDE/LSP metadata (include paths, defines,
/// compiler flags) PlatformIO exposes to editor extensions.
pub fn pio_project_metadata() -> Vec<String> {
    vec![s(PIO), s("project"), s("metadata")]
}

/// `pio project metadata --json-output` — the IDE/LSP metadata as JSON (the form
/// editor extensions consume).
pub fn pio_project_metadata_json() -> Vec<String> {
    vec![s(PIO), s("project"), s("metadata"), s("--json-output")]
}

/// `pio project metadata --json-output --json-output-path <path>` — write the
/// IDE/LSP metadata JSON to a file (for external tooling to read).
pub fn pio_project_metadata_path(path: &str) -> Vec<String> {
    vec![
        s(PIO),
        s("project"),
        s("metadata"),
        s("--json-output"),
        s("--json-output-path"),
        s(path),
    ]
}

/// `pio pkg exec -- <argv…>` — run an executable from an installed package/tool
/// (e.g. `esptool.py`, `openocd`) inside the project's tool environment.
pub fn pio_pkg_exec(args: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("pkg"), s("exec"), s("--")];
    v.extend(args.iter().cloned());
    v
}

/// `pio pkg install -g -p <spec>` — install a development platform globally
/// (the PlatformIO equivalent of `arduino-cli core install`).
pub fn pio_platform_install(spec: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("install"), s("-g"), s("-p"), s(spec)]
}

/// `pio pkg install -g -t <spec>` — install a tool package globally
/// (compilers, uploaders, debuggers).
pub fn pio_tool_install(spec: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("install"), s("-g"), s("-t"), s(spec)]
}

/// `pio pkg pack` — build a tarball of the current package (registry authoring).
pub fn pio_pkg_pack() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("pack")]
}

/// `pio pkg publish` — publish the current package to the PlatformIO registry.
pub fn pio_pkg_publish() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("publish")]
}

/// `pio pkg publish [extra…]` — publish with extra options (`--owner <o>`,
/// `--type <library|platform|tool>`, `--private`, `--notify`/`--no-notify`,
/// `--released-at <date>`, `--no-interactive`, or a `<pkg>` path/tarball).
/// Verified against `pio pkg publish --help` on PlatformIO 6.1.19.
pub fn pio_pkg_publish_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("pkg"), s("publish")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio pkg unpublish <pkg>` — remove a previously published package.
pub fn pio_pkg_unpublish(pkg: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("unpublish"), s(pkg)]
}

/// `pio pkg unpublish <pkg> --undo` — restore a package that was unpublished
/// (undo a prior removal). Verified against `pio pkg unpublish --help` on 6.1.19.
pub fn pio_pkg_unpublish_undo(pkg: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("unpublish"), s(pkg), s("--undo")]
}

/// `pio system info` — system-wide PlatformIO information (core version, paths,
/// Python, platforms).
pub fn pio_system_info() -> Vec<String> {
    vec![s(PIO), s("system"), s("info")]
}

/// `pio system info --json-output` — system-wide information as JSON.
pub fn pio_system_info_json() -> Vec<String> {
    vec![s(PIO), s("system"), s("info"), s("--json-output")]
}

/// `pio system prune -f [--<scope>]` — remove unused data without prompting (per
/// project convention: no confirmation on maintenance ops). `scope` narrows the
/// sweep to one subset (`cache`, `core-packages`, `platform-packages`); an empty
/// scope prunes everything. Verified against `pio system prune --help` on
/// PlatformIO 6.1.19.
pub fn pio_system_prune_scoped(scope: &str) -> Vec<String> {
    let mut v = vec![s(PIO), s("system"), s("prune"), s("-f")];
    if !scope.trim().is_empty() {
        v.push(format!("--{}", scope.trim()));
    }
    v
}

/// `pio system prune -f` — remove all unused caches/packages without prompting.
pub fn pio_system_prune() -> Vec<String> {
    pio_system_prune_scoped("")
}

/// `pio system prune --dry-run` — report what prune would remove, deleting
/// nothing (read-only; no `-f` needed).
pub fn pio_system_prune_dry_run() -> Vec<String> {
    vec![s(PIO), s("system"), s("prune"), s("--dry-run")]
}

/// `pio settings get [name]` — print PlatformIO Core settings (all, or one key).
pub fn pio_settings_get(name: &str) -> Vec<String> {
    let mut v = vec![s(PIO), s("settings"), s("get")];
    if !name.trim().is_empty() {
        v.push(s(name.trim()));
    }
    v
}

/// `pio settings set <name> <value>` — change a PlatformIO Core setting.
pub fn pio_settings_set(name: &str, value: &str) -> Vec<String> {
    vec![s(PIO), s("settings"), s("set"), s(name), s(value)]
}

/// `pio remote agent list` — active PlatformIO Remote agents.
pub fn pio_remote_agent_list() -> Vec<String> {
    vec![s(PIO), s("remote"), s("agent"), s("list")]
}

/// `pio remote agent start` — start a Remote agent on this machine.
pub fn pio_remote_agent_start() -> Vec<String> {
    vec![s(PIO), s("remote"), s("agent"), s("start")]
}

/// `pio remote device list` — serial devices attached to remote agents.
pub fn pio_remote_device_list() -> Vec<String> {
    vec![s(PIO), s("remote"), s("device"), s("list")]
}

/// `pio remote run` — build/upload the project via a remote agent.
pub fn pio_remote_run() -> Vec<String> {
    vec![s(PIO), s("remote"), s("run")]
}

/// `pio remote test` — run unit tests via a remote agent.
pub fn pio_remote_test() -> Vec<String> {
    vec![s(PIO), s("remote"), s("test")]
}

/// `pio remote update` — update platforms/packages/libraries on remote agents.
pub fn pio_remote_update() -> Vec<String> {
    vec![s(PIO), s("remote"), s("update")]
}

/// `pio account login` — sign in to a PlatformIO account (interactive).
pub fn pio_account_login() -> Vec<String> {
    vec![s(PIO), s("account"), s("login")]
}

/// `pio account logout` — sign out of the PlatformIO account.
pub fn pio_account_logout() -> Vec<String> {
    vec![s(PIO), s("account"), s("logout")]
}

/// `pio account show` — the current PlatformIO account information.
pub fn pio_account_show() -> Vec<String> {
    vec![s(PIO), s("account"), s("show")]
}

/// `pio account show [extra…]` — account information with extra options
/// (`--offline` to use cached data without a network round-trip, `--json-output`
/// for the machine-readable form). Verified against `pio account show --help`
/// on PlatformIO 6.1.19.
pub fn pio_account_show_with(extra: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("account"), s("show")];
    v.extend(extra.iter().cloned());
    v
}

/// `pio account token` — print (or regenerate) the account auth token.
pub fn pio_account_token() -> Vec<String> {
    vec![s(PIO), s("account"), s("token")]
}

/// `pio settings reset` — restore PlatformIO Core settings to their defaults.
pub fn pio_settings_reset() -> Vec<String> {
    vec![s(PIO), s("settings"), s("reset")]
}

/// `pio system completion <shell>` — emit a shell completion script
/// (`bash`/`zsh`/`fish`/`powershell`).
pub fn pio_system_completion(shell: &str) -> Vec<String> {
    vec![s(PIO), s("system"), s("completion"), s(shell)]
}

/// `pio ci <argv…>` — build a standalone source tree in an isolated project
/// (needs at least `-b <board>` and a source path in `args`).
pub fn pio_ci(args: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s("ci")];
    v.extend(args.iter().cloned());
    v
}

/// Generic `pio <group> <sub> [args…]` builder for the cloud-account command
/// families (`org`, `team`, `access`, and the `account` lifecycle) whose leaves
/// take user-supplied positional arguments (org/team names, member e-mails,
/// resource specs). Keeping one parameterised builder avoids inventing flag
/// shapes for commands whose `--help` needs the PlatformIO cloud to resolve.
pub fn pio_sub(group: &str, sub: &str, args: &[String]) -> Vec<String> {
    let mut v = vec![s(PIO), s(group), s(sub)];
    v.extend(args.iter().cloned());
    v
}

/// POSIX-quote an argv into a single shell command line, so paths with spaces
/// survive the `sh -c` round-trip the compilation buffer uses.
pub fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b'=' | b':' | b','))
    {
        arg.to_string()
    } else {
        // Single-quote, escaping embedded single quotes as '\''.
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// A single Arduino board from `board listall` JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct BoardEntry {
    pub name: String,
    pub fqbn: Option<String>,
}

/// Parse `arduino-cli board listall --format json` output into `(name, fqbn)`
/// pairs. The JSON shape is `{ "boards": [ { "name", "fqbn" }, ... ] }`; older
/// versions omit the wrapper, so accept a bare array too.
pub fn parse_board_listall(json: &str) -> Vec<BoardEntry> {
    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(default)]
        boards: Vec<BoardEntry>,
    }
    if let Ok(w) = serde_json::from_str::<Wrapper>(json) {
        if !w.boards.is_empty() {
            return w.boards;
        }
    }
    serde_json::from_str::<Vec<BoardEntry>>(json).unwrap_or_default()
}

/// A connected port from `board list` JSON.
#[derive(Debug, Clone)]
pub struct PortEntry {
    pub address: String,
    pub label: String,
}

/// Parse `arduino-cli board list --format json` into connected serial ports.
/// Shape (v1.x): `{ "detected_ports": [ { "port": { "address", "protocol_label" },
/// "matching_boards": [ { "name" } ] } ] }`.
pub fn parse_port_list(json: &str) -> Vec<PortEntry> {
    let root: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let ports = root
        .get("detected_ports")
        .or_else(|| root.as_array().map(|_| &root))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let arr = match ports.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|entry| {
            let port = entry.get("port").unwrap_or(entry);
            let address = port
                .get("address")
                .and_then(|a| a.as_str())?
                .to_string();
            let board = entry
                .get("matching_boards")
                .and_then(|b| b.as_array())
                .and_then(|b| b.first())
                .and_then(|b| b.get("name"))
                .and_then(|n| n.as_str());
            let proto = port
                .get("protocol_label")
                .and_then(|p| p.as_str())
                .unwrap_or("serial");
            let label = match board {
                Some(name) => format!("{address} — {name}"),
                None => format!("{address} ({proto})"),
            };
            Some(PortEntry { address, label })
        })
        .collect()
}

/// Parse the `[env:<name>]` section headers from a `platformio.ini`, in file
/// order — the build environments the user can select with `:pio-env`.
pub fn parse_pio_envs(ini: &str) -> Vec<String> {
    ini.lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("[env:")
                .and_then(|rest| rest.strip_suffix(']'))
                .map(|name| name.trim().to_string())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// Heuristic: does `dir` (or an ancestor) look like a PlatformIO project?
pub fn is_platformio_project(dir: &Path) -> bool {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.join("platformio.ini").is_file() {
            return true;
        }
        cur = d.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> EmbeddedSettings {
        EmbeddedSettings {
            backend: Backend::Arduino,
            fqbn: "arduino:avr:uno".into(),
            port: "/dev/cu.usbmodem1401".into(),
            baud: 115200,
            sketch: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn compile_argv_has_fqbn_and_sketch() {
        let argv = arduino_compile(&settings()).unwrap();
        assert_eq!(argv[0], "arduino-cli");
        assert_eq!(argv[1], "compile");
        assert!(argv.contains(&"--fqbn".to_string()));
        assert!(argv.contains(&"arduino:avr:uno".to_string()));
    }

    #[test]
    fn compile_needs_a_board() {
        let mut st = settings();
        st.fqbn.clear();
        assert!(arduino_compile(&st).is_err());
    }

    #[test]
    fn upload_needs_a_port() {
        let mut st = settings();
        st.port.clear();
        assert!(arduino_upload(&st).is_err());
    }

    #[test]
    fn monitor_encodes_baud() {
        let argv = arduino_monitor(&settings()).unwrap();
        assert!(argv.contains(&"baudrate=115200".to_string()));
    }

    #[test]
    fn pio_monitor_carries_port_and_baud() {
        let argv = pio_monitor(&settings());
        assert!(argv.windows(2).any(|w| w == ["-p", "/dev/cu.usbmodem1401"]));
        assert!(argv.windows(2).any(|w| w == ["-b", "115200"]));
    }

    #[test]
    fn pio_device_list_serial_uses_serial_flag() {
        assert_eq!(
            pio_device_list_serial(),
            vec!["pio", "device", "list", "--serial"]
        );
    }

    #[test]
    fn pio_pkg_list_scope_builds_only_flag() {
        assert_eq!(
            pio_pkg_list_scope("libraries"),
            vec!["pio", "pkg", "list", "--only-libraries"]
        );
        assert_eq!(
            pio_pkg_list_scope("platforms"),
            vec!["pio", "pkg", "list", "--only-platforms"]
        );
        assert_eq!(
            pio_pkg_list_scope("tools"),
            vec!["pio", "pkg", "list", "--only-tools"]
        );
    }

    #[test]
    fn pio_json_report_builders_append_json_output() {
        assert!(pio_project_config_json().ends_with(&["--json-output".to_string()]));
        assert!(pio_project_metadata_json().ends_with(&["--json-output".to_string()]));
        assert!(pio_system_info_json().ends_with(&["--json-output".to_string()]));
    }

    #[test]
    fn pio_pkg_search_page_carries_query_and_page() {
        let argv = pio_pkg_search_page("wifi", "2");
        assert!(argv.windows(2).any(|w| w == ["--page", "2"]));
        assert!(argv.contains(&"wifi".to_string()));
    }

    #[test]
    fn pio_pkg_unpublish_undo_appends_undo() {
        let argv = pio_pkg_unpublish_undo("owner/pkg@1.0.0");
        assert_eq!(argv[..4], ["pio", "pkg", "unpublish", "owner/pkg@1.0.0"]);
        assert!(argv.contains(&"--undo".to_string()));
    }

    #[test]
    fn arduino_daemon_and_version_forward_args() {
        assert_eq!(
            arduino_daemon(&["--port".into(), "50051".into()]),
            ["arduino-cli", "daemon", "--port", "50051"]
        );
        assert_eq!(
            arduino_version(&["--format".into(), "json".into()]),
            ["arduino-cli", "version", "--format", "json"]
        );
        assert_eq!(arduino_daemon(&[]), ["arduino-cli", "daemon"]);
    }

    #[test]
    fn arduino_compile_with_splices_flags_before_sketch() {
        let argv = arduino_compile_with(&settings(), &["-v".into(), "--clean".into()]).unwrap();
        assert_eq!(argv[0], "arduino-cli");
        assert_eq!(argv[1], "compile");
        // sketch path is the final positional; flags precede it.
        let sketch = argv.last().unwrap();
        assert!(argv.contains(&"-v".to_string()));
        assert!(argv.contains(&"--clean".to_string()));
        let clean_idx = argv.iter().position(|a| a == "--clean").unwrap();
        assert!(clean_idx < argv.len() - 1, "flags must precede the sketch path {sketch}");
    }

    #[test]
    fn arduino_compile_with_needs_a_board() {
        let mut st = settings();
        st.fqbn.clear();
        assert!(arduino_compile_with(&st, &["-v".into()]).is_err());
    }

    #[test]
    fn arduino_upload_with_carries_port_and_verify() {
        let argv = arduino_upload_with(&settings(), &["--verify".into()]).unwrap();
        assert_eq!(argv[1], "compile");
        assert!(argv.contains(&"--upload".to_string()));
        assert!(argv.contains(&"--verify".to_string()));
        assert!(argv.windows(2).any(|w| w == ["-p", "/dev/cu.usbmodem1401"]));
    }

    #[test]
    fn arduino_upload_input_flashes_prebuilt() {
        let argv = arduino_upload_input(&settings(), "--input-dir", "build").unwrap();
        assert_eq!(argv[1], "upload");
        assert!(argv.windows(2).any(|w| w == ["--input-dir", "build"]));
        assert!(argv.windows(2).any(|w| w == ["--fqbn", "arduino:avr:uno"]));
    }

    #[test]
    fn arduino_monitor_with_carries_raw_and_baud() {
        let argv = arduino_monitor_with(&settings(), &["--raw".into()]).unwrap();
        assert!(argv.contains(&"--raw".to_string()));
        assert!(argv.contains(&"baudrate=115200".to_string()));
    }

    #[test]
    fn arduino_debug_with_splices_flags_before_sketch() {
        let argv = arduino_debug_with(&settings(), &["--info".into()]).unwrap();
        assert_eq!(argv[1], "debug");
        assert!(argv.contains(&"--info".to_string()));
        // sketch path stays the trailing positional.
        let info_idx = argv.iter().position(|a| a == "--info").unwrap();
        assert!(info_idx < argv.len() - 1);
        assert!(argv.windows(2).any(|w| w == ["-p", "/dev/cu.usbmodem1401"]));
    }

    #[test]
    fn arduino_debug_with_needs_board_and_port() {
        let mut st = settings();
        st.port.clear();
        assert!(arduino_debug_with(&st, &["--info".into()]).is_err());
    }

    #[test]
    fn arduino_third_pass_flag_builders() {
        assert!(arduino_core_list_updatable().contains(&"--updatable".to_string()));
        assert_eq!(
            arduino_lib_install_no_deps("Servo"),
            ["arduino-cli", "lib", "install", "Servo", "--no-deps"]
        );
        assert!(arduino_board_details_full("arduino:avr:uno").contains(&"--full".to_string()));
    }

    #[test]
    fn arduino_board_and_lib_flag_builders() {
        assert!(arduino_board_details_programmers("arduino:avr:uno")
            .contains(&"--list-programmers".to_string()));
        assert_eq!(
            arduino_board_list_watch(),
            ["arduino-cli", "board", "list", "--watch"]
        );
        assert!(arduino_lib_list_updatable().contains(&"--updatable".to_string()));
        assert!(arduino_lib_install_git("https://example.com/lib.git")
            .contains(&"--git-url".to_string()));
        assert!(arduino_lib_install_zip("/tmp/lib.zip").contains(&"--zip-path".to_string()));
    }

    #[test]
    fn arduino_profile_lib_add_remove_via_sub() {
        assert_eq!(
            arduino_sub("profile", "lib", &["add".into(), "Servo".into()]),
            ["arduino-cli", "profile", "lib", "add", "Servo"]
        );
        assert_eq!(
            arduino_sub("profile", "lib", &["remove".into(), "Servo".into()]),
            ["arduino-cli", "profile", "lib", "remove", "Servo"]
        );
    }

    #[test]
    fn pio_third_pass_flag_builders() {
        assert_eq!(
            pio_pkg_list_global(),
            ["pio", "pkg", "list", "-g"]
        );
        assert_eq!(
            pio_pkg_update_global(),
            ["pio", "pkg", "update", "-g"]
        );
        assert!(pio_project_config_lint().contains(&"--lint".to_string()));
        let pubv = pio_pkg_publish_with(&["--private".into(), "--type".into(), "library".into()]);
        assert_eq!(pubv[..3], ["pio", "pkg", "publish"]);
        assert!(pubv.contains(&"--private".to_string()));
        assert!(pubv.windows(2).any(|w| w == ["--type", "library"]));
    }

    #[test]
    fn pio_run_exec_threads_program_args_and_env() {
        let mut st = settings();
        st.env = "native".into();
        let argv = pio_run_exec(&st, &["hello".into(), "world".into()]);
        assert_eq!(argv[..4], ["pio", "run", "-t", "exec"]);
        // one -a per program argument.
        assert_eq!(argv.iter().filter(|a| *a == "-a").count(), 2);
        assert!(argv.windows(2).any(|w| w == ["-a", "hello"]));
        assert!(argv.windows(2).any(|w| w == ["-a", "world"]));
        assert!(argv.windows(2).any(|w| w == ["-e", "native"]));
        // no program args → bare exec target.
        assert_eq!(pio_run_exec(&settings(), &[]), ["pio", "run", "-t", "exec"]);
    }

    #[test]
    fn pio_account_show_and_remote_monitor_forward_args() {
        let av = pio_account_show_with(&["--offline".into()]);
        assert_eq!(av[..3], ["pio", "account", "show"]);
        assert!(av.contains(&"--offline".to_string()));
        let rv = pio_remote_device_monitor_with(&["--sock".into(), "/tmp/sock".into()]);
        assert_eq!(rv[..4], ["pio", "remote", "device", "monitor"]);
        assert!(rv.windows(2).any(|w| w == ["--sock", "/tmp/sock"]));
        // bare builder still emits no flags.
        assert_eq!(pio_remote_device_monitor().len(), 4);
    }

    #[test]
    fn pio_project_metadata_path_writes_to_file() {
        let argv = pio_project_metadata_path("meta.json");
        assert!(argv.windows(2).any(|w| w == ["--json-output-path", "meta.json"]));
        assert!(argv.contains(&"--json-output".to_string()));
    }

    #[test]
    fn pio_test_with_carries_ci_report_flags() {
        let mut st = settings();
        st.env = "uno".into();
        let argv = pio_test_with(&st, &["--junit-output-path".into(), "out.xml".into()]);
        assert!(argv.windows(2).any(|w| w == ["--junit-output-path", "out.xml"]));
        assert!(argv.windows(2).any(|w| w == ["-e", "uno"]));
    }

    #[test]
    fn pio_boards_json_appends_json_and_optional_query() {
        assert_eq!(pio_boards_json(""), ["pio", "boards", "--json-output"]);
        assert_eq!(
            pio_boards_json("esp32"),
            ["pio", "boards", "--json-output", "esp32"]
        );
    }

    #[test]
    fn pio_pkg_exec_package_scopes_and_terminates_flags() {
        let argv = pio_pkg_exec_package("tool-esptoolpy", &["esptool.py".into(), "--help".into()]);
        assert!(argv.windows(2).any(|w| w == ["-p", "tool-esptoolpy"]));
        // The `--` guard must precede the tool argv so its flags are not parsed by pio.
        let dd = argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(argv[dd + 1], "esptool.py");
    }

    #[test]
    fn pio_remote_run_and_test_with_thread_extra_and_env() {
        let mut st = settings();
        st.env = "esp32dev".into();
        let run = pio_remote_run_with(&st, &["-t".into(), "upload".into()]);
        assert_eq!(run[..3], ["pio", "remote", "run"]);
        assert!(run.windows(2).any(|w| w == ["-t", "upload"]));
        assert!(run.windows(2).any(|w| w == ["-e", "esp32dev"]));
        let test = pio_remote_test_with(&st, &["--without-building".into()]);
        assert_eq!(test[..3], ["pio", "remote", "test"]);
        assert!(test.contains(&"--without-building".to_string()));
        assert!(test.windows(2).any(|w| w == ["-e", "esp32dev"]));
    }

    #[test]
    fn shell_join_quotes_spaces() {
        let argv = vec![
            "arduino-cli".to_string(),
            "compile".to_string(),
            "/tmp/my sketch".to_string(),
        ];
        let joined = shell_join(&argv);
        assert_eq!(joined, "arduino-cli compile '/tmp/my sketch'");
    }

    #[test]
    fn shell_join_leaves_plain_flags_bare() {
        let argv = vec!["-c".to_string(), "baudrate=9600".to_string()];
        assert_eq!(shell_join(&argv), "-c baudrate=9600");
    }

    #[test]
    fn parse_board_listall_wrapped() {
        let json = r#"{"boards":[{"name":"Arduino Uno","fqbn":"arduino:avr:uno"},
                                   {"name":"Arduino Nano","fqbn":"arduino:avr:nano"}]}"#;
        let boards = parse_board_listall(json);
        assert_eq!(boards.len(), 2);
        assert_eq!(boards[0].fqbn.as_deref(), Some("arduino:avr:uno"));
    }

    #[test]
    fn parse_port_list_extracts_address_and_board() {
        let json = r#"{"detected_ports":[
            {"port":{"address":"/dev/cu.usbmodem1401","protocol_label":"Serial Port (USB)"},
             "matching_boards":[{"name":"Arduino Uno","fqbn":"arduino:avr:uno"}]}]}"#;
        let ports = parse_port_list(json);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].address, "/dev/cu.usbmodem1401");
        assert!(ports[0].label.contains("Arduino Uno"));
    }

    #[test]
    fn upload_compiles_then_flashes() {
        // Arduino IDE "Upload" = compile + flash; plain `arduino-cli upload`
        // does not compile first, so we drive `compile --upload`.
        let argv = arduino_upload(&settings()).unwrap();
        assert_eq!(argv[1], "compile");
        assert!(argv.contains(&"--upload".to_string()));
        assert!(argv.windows(2).any(|w| w == ["-p", "/dev/cu.usbmodem1401"]));
        assert!(argv.contains(&"arduino:avr:uno".to_string()));
    }

    #[test]
    fn compile_export_sets_export_flag() {
        let argv = arduino_compile_export(&settings()).unwrap();
        assert_eq!(argv[1], "compile");
        assert!(argv.contains(&"-e".to_string()));
        assert!(argv.contains(&"--fqbn".to_string()));
    }

    #[test]
    fn compile_export_needs_a_board() {
        let mut st = settings();
        st.fqbn.clear();
        assert!(arduino_compile_export(&st).is_err());
    }

    #[test]
    fn burn_bootloader_needs_board_and_port() {
        let argv = arduino_burn_bootloader(&settings()).unwrap();
        assert_eq!(argv[1], "burn-bootloader");
        assert!(argv.windows(2).any(|w| w == ["-p", "/dev/cu.usbmodem1401"]));
        let mut st = settings();
        st.fqbn.clear();
        assert!(arduino_burn_bootloader(&st).is_err());
        let mut st = settings();
        st.port.clear();
        assert!(arduino_burn_bootloader(&st).is_err());
    }

    #[test]
    fn board_details_carries_fqbn() {
        let argv = arduino_board_details("arduino:avr:uno");
        assert_eq!(argv, ["arduino-cli", "board", "details", "--fqbn", "arduino:avr:uno"]);
    }

    #[test]
    fn core_and_lib_subcommands() {
        assert_eq!(arduino_core_search("esp32"), ["arduino-cli", "core", "search", "esp32"]);
        assert_eq!(arduino_core_list(), ["arduino-cli", "core", "list"]);
        assert_eq!(arduino_core_uninstall("arduino:avr"), ["arduino-cli", "core", "uninstall", "arduino:avr"]);
        assert_eq!(arduino_core_upgrade(), ["arduino-cli", "core", "upgrade"]);
        assert_eq!(arduino_core_update_index(), ["arduino-cli", "core", "update-index"]);
        assert_eq!(arduino_lib_list(), ["arduino-cli", "lib", "list"]);
        assert_eq!(arduino_lib_uninstall("Servo"), ["arduino-cli", "lib", "uninstall", "Servo"]);
        assert_eq!(arduino_lib_upgrade(), ["arduino-cli", "lib", "upgrade"]);
        assert_eq!(arduino_lib_examples("Servo"), ["arduino-cli", "lib", "examples", "Servo"]);
    }

    #[test]
    fn sketch_archive_takes_dir() {
        let argv = arduino_sketch_archive(Path::new("/tmp/blink"));
        assert_eq!(argv, ["arduino-cli", "sketch", "archive", "/tmp/blink"]);
    }

    #[test]
    fn pio_run_targets_and_pkg() {
        let st = settings();
        assert_eq!(pio_clean(&st), ["pio", "run", "-t", "clean"]);
        assert_eq!(pio_test(&st), ["pio", "test"]);
        assert_eq!(pio_check(&st), ["pio", "check"]);
        assert_eq!(pio_pkg_list(), ["pio", "pkg", "list"]);
        assert_eq!(pio_pkg_uninstall("Adafruit GFX"), ["pio", "pkg", "uninstall", "-l", "Adafruit GFX"]);
        assert_eq!(pio_pkg_update(), ["pio", "pkg", "update"]);
        assert_eq!(pio_lib_install("Adafruit GFX"), ["pio", "pkg", "install", "-l", "Adafruit GFX"]);
    }

    #[test]
    fn pio_boards_omits_empty_query() {
        assert_eq!(pio_boards(""), ["pio", "boards"]);
        assert_eq!(pio_boards("uno"), ["pio", "boards", "uno"]);
    }

    #[test]
    fn combined_index_and_upgrade_subcommands() {
        assert_eq!(arduino_update(), ["arduino-cli", "update"]);
        assert_eq!(arduino_upgrade(), ["arduino-cli", "upgrade"]);
        assert_eq!(arduino_outdated(), ["arduino-cli", "outdated"]);
        assert_eq!(arduino_lib_deps("Servo"), ["arduino-cli", "lib", "deps", "Servo"]);
        assert_eq!(arduino_config_dump(), ["arduino-cli", "config", "dump"]);
    }

    #[test]
    fn debug_needs_board_and_port() {
        let argv = arduino_debug(&settings()).unwrap();
        assert_eq!(argv[1], "debug");
        assert!(argv.windows(2).any(|w| w == ["--fqbn", "arduino:avr:uno"]));
        assert!(argv.windows(2).any(|w| w == ["-p", "/dev/cu.usbmodem1401"]));
        let mut st = settings();
        st.port.clear();
        assert!(arduino_debug(&st).is_err());
    }

    #[test]
    fn pio_pkg_and_maintenance_subcommands() {
        assert_eq!(pio_pkg_search("Adafruit"), ["pio", "pkg", "search", "Adafruit"]);
        assert_eq!(pio_pkg_outdated(), ["pio", "pkg", "outdated"]);
        assert_eq!(pio_pkg_show("Servo"), ["pio", "pkg", "show", "Servo"]);
        assert_eq!(pio_debug(&settings()), ["pio", "debug"]);
        assert_eq!(pio_upgrade(), ["pio", "upgrade"]);
    }

    #[test]
    fn pio_run_targets_build_argv() {
        let st = settings();
        assert_eq!(pio_run_target(&st, "size"), ["pio", "run", "-t", "size"]);
        assert_eq!(pio_run_target(&st, "compiledb"), ["pio", "run", "-t", "compiledb"]);
        assert_eq!(pio_run_target(&st, "uploadfs"), ["pio", "run", "-t", "uploadfs"]);
        assert_eq!(pio_run_target(&st, "nobuild"), ["pio", "run", "-t", "nobuild"]);
    }

    #[test]
    fn pio_env_threads_into_project_actions() {
        let mut st = settings();
        st.env = "esp32dev".into();
        assert_eq!(pio_build(&st), ["pio", "run", "-e", "esp32dev"]);
        assert_eq!(pio_upload(&st), ["pio", "run", "-t", "upload", "-e", "esp32dev"]);
        assert_eq!(pio_clean(&st), ["pio", "run", "-t", "clean", "-e", "esp32dev"]);
        assert_eq!(pio_test(&st), ["pio", "test", "-e", "esp32dev"]);
        assert_eq!(pio_check(&st), ["pio", "check", "-e", "esp32dev"]);
        assert_eq!(pio_debug(&st), ["pio", "debug", "-e", "esp32dev"]);
        assert_eq!(pio_run_target(&st, "size"), ["pio", "run", "-t", "size", "-e", "esp32dev"]);
        // Empty env adds no flag.
        let st = settings();
        assert_eq!(pio_build(&st), ["pio", "run"]);
        assert!(st.pio_env_args().is_empty());
    }

    #[test]
    fn pio_monitor_carries_filters_eol_parity_env() {
        let mut st = settings();
        st.filters = vec!["time".into(), "log2file".into()];
        st.eol = "LF".into();
        st.parity = "E".into();
        st.env = "uno".into();
        let argv = pio_monitor(&st);
        assert!(argv.windows(2).any(|w| w == ["-f", "time"]));
        assert!(argv.windows(2).any(|w| w == ["-f", "log2file"]));
        assert!(argv.windows(2).any(|w| w == ["--eol", "LF"]));
        assert!(argv.windows(2).any(|w| w == ["--parity", "E"]));
        assert!(argv.windows(2).any(|w| w == ["-e", "uno"]));
    }

    #[test]
    fn pio_extra_build_and_maintenance_subcommands() {
        let st = settings();
        assert_eq!(pio_list_targets(&st), ["pio", "run", "--list-targets"]);
        assert_eq!(pio_list_tests(&st), ["pio", "test", "--list-tests"]);
        assert_eq!(pio_test_filter(&st, "sensor*"), ["pio", "test", "-f", "sensor*"]);
        assert_eq!(pio_check_severity(&st, "high"), ["pio", "check", "--severity", "high"]);
        assert_eq!(pio_boards_installed(""), ["pio", "boards", "--installed"]);
        assert_eq!(pio_boards_installed("uno"), ["pio", "boards", "--installed", "uno"]);
        assert_eq!(pio_tool_install("tool-openocd"), ["pio", "pkg", "install", "-g", "-t", "tool-openocd"]);
        assert_eq!(pio_settings_reset(), ["pio", "settings", "reset"]);
        assert_eq!(pio_system_completion("zsh"), ["pio", "system", "completion", "zsh"]);
        assert_eq!(pio_remote_device_monitor(), ["pio", "remote", "device", "monitor"]);
    }

    #[test]
    fn arduino_extra_leaves_and_config() {
        assert_eq!(arduino_core_download("arduino:avr"), ["arduino-cli", "core", "download", "arduino:avr"]);
        assert_eq!(arduino_lib_download("Servo"), ["arduino-cli", "lib", "download", "Servo"]);
        assert_eq!(arduino_lib_update_index(), ["arduino-cli", "lib", "update-index"]);
        assert_eq!(arduino_board_search(""), ["arduino-cli", "board", "search"]);
        assert_eq!(arduino_board_search("uno"), ["arduino-cli", "board", "search", "uno"]);
        assert_eq!(arduino_cache_clean(), ["arduino-cli", "cache", "clean"]);
        assert_eq!(arduino_completion("zsh"), ["arduino-cli", "completion", "zsh"]);
        assert_eq!(
            arduino_config("set", &["board_manager.additional_urls".to_string(), "http://x".to_string()]),
            ["arduino-cli", "config", "set", "board_manager.additional_urls", "http://x"]
        );
        assert_eq!(arduino_config("init", &[]), ["arduino-cli", "config", "init"]);
        assert_eq!(
            arduino_sub("profile", "set-default", &["nano".to_string()]),
            ["arduino-cli", "profile", "set-default", "nano"]
        );
    }

    #[test]
    fn raw_passthroughs_prepend_binary() {
        assert_eq!(
            pio_passthrough(&["home".to_string(), "--port".to_string(), "8080".to_string()]),
            ["pio", "home", "--port", "8080"]
        );
        assert_eq!(
            arduino_passthrough(&["version".to_string()]),
            ["arduino-cli", "version"]
        );
        assert_eq!(pio_passthrough(&[]), ["pio"]);
    }

    #[test]
    fn parse_pio_envs_extracts_section_names() {
        let ini = "[platformio]\ndefault_envs = uno\n\n[env:uno]\nboard = uno\n\n[env:esp32dev]\nboard = esp32dev\n\n[common]\nx = 1\n";
        assert_eq!(parse_pio_envs(ini), ["uno", "esp32dev"]);
        assert!(parse_pio_envs("[platformio]\n").is_empty());
    }

    #[test]
    fn pio_ci_and_sub_passthrough() {
        let ci = pio_ci(&["-b".to_string(), "uno".to_string(), "src/main.cpp".to_string()]);
        assert_eq!(ci, ["pio", "ci", "-b", "uno", "src/main.cpp"]);
        assert_eq!(
            pio_sub("org", "create", &["MyOrg".to_string()]),
            ["pio", "org", "create", "MyOrg"]
        );
        assert_eq!(pio_sub("access", "list", &[]), ["pio", "access", "list"]);
        assert_eq!(
            pio_sub("team", "add", &["MyOrg:devs".to_string(), "user@x.com".to_string()]),
            ["pio", "team", "add", "MyOrg:devs", "user@x.com"]
        );
    }

    #[test]
    fn pio_project_and_system_subcommands() {
        assert_eq!(pio_project_config(), ["pio", "project", "config"]);
        assert_eq!(pio_project_metadata(), ["pio", "project", "metadata"]);
        assert_eq!(pio_system_info(), ["pio", "system", "info"]);
        assert_eq!(pio_system_prune(), ["pio", "system", "prune", "-f"]);
    }

    #[test]
    fn pio_prune_scopes_and_dry_run() {
        assert_eq!(pio_system_prune_scoped(""), ["pio", "system", "prune", "-f"]);
        assert_eq!(pio_system_prune_scoped("cache"), ["pio", "system", "prune", "-f", "--cache"]);
        assert_eq!(
            pio_system_prune_scoped("core-packages"),
            ["pio", "system", "prune", "-f", "--core-packages"]
        );
        assert_eq!(
            pio_system_prune_scoped("platform-packages"),
            ["pio", "system", "prune", "-f", "--platform-packages"]
        );
        assert_eq!(pio_system_prune_dry_run(), ["pio", "system", "prune", "--dry-run"]);
    }

    #[test]
    fn pio_monitor_carries_line_discipline() {
        let mut st = settings();
        st.rts = "0".into();
        st.dtr = "1".into();
        st.echo = true;
        st.raw = true;
        st.encoding = "hexlify".into();
        st.rtscts = true;
        st.xonxoff = true;
        st.no_reconnect = true;
        let argv = pio_monitor(&st);
        assert!(argv.windows(2).any(|w| w == ["--rts", "0"]));
        assert!(argv.windows(2).any(|w| w == ["--dtr", "1"]));
        assert!(argv.windows(2).any(|w| w == ["--encoding", "hexlify"]));
        assert!(argv.contains(&"--echo".to_string()));
        assert!(argv.contains(&"--raw".to_string()));
        assert!(argv.contains(&"--rtscts".to_string()));
        assert!(argv.contains(&"--xonxoff".to_string()));
        assert!(argv.contains(&"--no-reconnect".to_string()));
        // Defaults add none of them.
        let plain = pio_monitor(&settings());
        assert!(!plain.contains(&"--echo".to_string()));
        assert!(!plain.iter().any(|a| a == "--rts"));
    }

    #[test]
    fn pio_monitor_carries_quiet_and_control_chars() {
        let mut st = settings();
        st.quiet = true;
        st.exit_char = "4".into();
        st.menu_char = "1".into();
        let argv = pio_monitor(&st);
        assert!(argv.contains(&"--quiet".to_string()));
        assert!(argv.windows(2).any(|w| w == ["--exit-char", "4"]));
        assert!(argv.windows(2).any(|w| w == ["--menu-char", "1"]));
        let plain = pio_monitor(&settings());
        assert!(!plain.contains(&"--quiet".to_string()));
        assert!(!plain.iter().any(|a| a == "--exit-char"));
    }

    #[test]
    fn pio_remaining_leaf_builders() {
        assert_eq!(
            pio_pkg_pack_with(&["-o".into(), "dist/".into()]),
            ["pio", "pkg", "pack", "-o", "dist/"]
        );
        assert_eq!(pio_pkg_pack_with(&[]), ["pio", "pkg", "pack"]);
        assert_eq!(
            pio_pkg_show_type("Servo", "library"),
            ["pio", "pkg", "show", "--type", "library", "Servo"]
        );
        assert_eq!(
            pio_pkg_exec_call(&["esptool.py --version".into()]),
            ["pio", "pkg", "exec", "-c", "esptool.py --version"]
        );
        assert_eq!(
            pio_account_token_with(&["--regenerate".into()]),
            ["pio", "account", "token", "--regenerate"]
        );
        assert_eq!(
            pio_remote_update_with(&["--dry-run".into()]),
            ["pio", "remote", "update", "--dry-run"]
        );
    }

    #[test]
    fn pio_flag_carrying_builders_thread_env() {
        let mut st = settings();
        st.env = "uno".into();
        assert_eq!(pio_run_with(&st, &["-v".into()]), ["pio", "run", "-v", "-e", "uno"]);
        assert_eq!(
            pio_run_with(&st, &["-j".into(), "4".into()]),
            ["pio", "run", "-j", "4", "-e", "uno"]
        );
        assert_eq!(
            pio_test_with(&st, &["--without-building".into()]),
            ["pio", "test", "--without-building", "-e", "uno"]
        );
        assert_eq!(
            pio_check_with(&st, &["--json-output".into()]),
            ["pio", "check", "--json-output", "-e", "uno"]
        );
        assert_eq!(
            pio_debug_with(&st, &["--interface".into(), "gdb".into()]),
            ["pio", "debug", "--interface", "gdb", "-e", "uno"]
        );
        assert_eq!(
            pio_remote_run_with(&st, &["-r".into()]),
            ["pio", "remote", "run", "-r", "-e", "uno"]
        );
        // Project-global builders take no env.
        assert_eq!(
            pio_project_init_with(&["--ide".into(), "vscode".into()]),
            ["pio", "project", "init", "--ide", "vscode"]
        );
        assert_eq!(
            pio_pkg_install_with(&["-f".into(), "-l".into(), "Servo".into()]),
            ["pio", "pkg", "install", "-f", "-l", "Servo"]
        );
        assert_eq!(pio_upgrade_with(&["--dev".into()]), ["pio", "upgrade", "--dev"]);
        assert_eq!(
            pio_remote_agent_start_with(&["--name".into(), "lab".into()]),
            ["pio", "remote", "agent", "start", "--name", "lab"]
        );
        assert_eq!(
            pio_pkg_search_sort("Adafruit", "popularity"),
            ["pio", "pkg", "search", "Adafruit", "--sort", "popularity"]
        );
    }

    #[test]
    fn pio_home_and_device_list_variants() {
        assert_eq!(pio_home(&[]), ["pio", "home"]);
        assert_eq!(
            pio_home(&["--port".to_string(), "8010".to_string(), "--no-open".to_string()]),
            ["pio", "home", "--port", "8010", "--no-open"]
        );
        assert_eq!(pio_device_list_logical(), ["pio", "device", "list", "--logical"]);
        assert_eq!(pio_device_list_mdns(), ["pio", "device", "list", "--mdns"]);
    }

    #[test]
    fn pio_pkg_authoring_subcommands() {
        let exec = pio_pkg_exec(&["esptool.py".to_string(), "--help".to_string()]);
        assert_eq!(exec, ["pio", "pkg", "exec", "--", "esptool.py", "--help"]);
        assert_eq!(pio_pkg_pack(), ["pio", "pkg", "pack"]);
        assert_eq!(pio_pkg_publish(), ["pio", "pkg", "publish"]);
        assert_eq!(pio_pkg_unpublish("Foo@1.0.0"), ["pio", "pkg", "unpublish", "Foo@1.0.0"]);
        assert_eq!(pio_platform_install("atmelavr"), ["pio", "pkg", "install", "-g", "-p", "atmelavr"]);
    }

    #[test]
    fn pio_settings_get_omits_empty_name() {
        assert_eq!(pio_settings_get(""), ["pio", "settings", "get"]);
        assert_eq!(pio_settings_get("enable_telemetry"), ["pio", "settings", "get", "enable_telemetry"]);
        assert_eq!(
            pio_settings_set("enable_telemetry", "false"),
            ["pio", "settings", "set", "enable_telemetry", "false"]
        );
    }

    #[test]
    fn pio_remote_and_account_subcommands() {
        assert_eq!(pio_remote_agent_list(), ["pio", "remote", "agent", "list"]);
        assert_eq!(pio_remote_agent_start(), ["pio", "remote", "agent", "start"]);
        assert_eq!(pio_remote_device_list(), ["pio", "remote", "device", "list"]);
        assert_eq!(pio_remote_run(), ["pio", "remote", "run"]);
        assert_eq!(pio_remote_test(), ["pio", "remote", "test"]);
        assert_eq!(pio_remote_update(), ["pio", "remote", "update"]);
        assert_eq!(pio_account_login(), ["pio", "account", "login"]);
        assert_eq!(pio_account_logout(), ["pio", "account", "logout"]);
        assert_eq!(pio_account_show(), ["pio", "account", "show"]);
        assert_eq!(pio_account_token(), ["pio", "account", "token"]);
    }

    #[test]
    fn settings_roundtrip_toml() {
        let st = settings();
        let text = toml::to_string_pretty(&st).unwrap();
        let back: EmbeddedSettings = toml::from_str(&text).unwrap();
        assert_eq!(back.fqbn, st.fqbn);
        assert_eq!(back.baud, st.baud);
        assert_eq!(back.backend, Backend::Arduino);
    }
}
