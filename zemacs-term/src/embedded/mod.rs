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
}

impl Default for EmbeddedSettings {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            fqbn: String::new(),
            port: String::new(),
            baud: 9600,
            sketch: String::new(),
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

/// `arduino-cli core search <query>` — Boards Manager search.
pub fn arduino_core_search(query: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("search"), s(query)]
}

/// `arduino-cli core list` — installed platforms (Boards Manager, installed tab).
pub fn arduino_core_list() -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("list")]
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

/// `arduino-cli core install <package>`
pub fn arduino_core_install(pkg: &str) -> Vec<String> {
    vec![s(ARDUINO_CLI), s("core"), s("install"), s(pkg)]
}

/// `pio run` — build the PlatformIO project.
pub fn pio_build() -> Vec<String> {
    vec![s(PIO), s("run")]
}

/// `pio run -t upload`
pub fn pio_upload() -> Vec<String> {
    vec![s(PIO), s("run"), s("-t"), s("upload")]
}

/// `pio device monitor [-p port] [-b baud]`
pub fn pio_monitor(settings: &EmbeddedSettings) -> Vec<String> {
    let mut v = vec![s(PIO), s("device"), s("monitor")];
    if !settings.port.is_empty() {
        v.push(s("-p"));
        v.push(settings.port.clone());
    }
    v.push(s("-b"));
    v.push(settings.baud.to_string());
    v
}

/// `pio device list --json-output`
pub fn pio_device_list() -> Vec<String> {
    vec![s(PIO), s("device"), s("list"), s("--json-output")]
}

/// `pio project init --board <id>`
pub fn pio_init(board: &str) -> Vec<String> {
    vec![s(PIO), s("project"), s("init"), s("--board"), s(board)]
}

/// `pio pkg install -l <name>` — add a library to the project.
pub fn pio_lib_install(name: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("install"), s("-l"), s(name)]
}

/// `pio run -t clean` — remove build artifacts (PlatformIO IDE "Clean").
pub fn pio_clean() -> Vec<String> {
    vec![s(PIO), s("run"), s("-t"), s("clean")]
}

/// `pio test` — run the project's unit tests (PlatformIO IDE "Test").
pub fn pio_test() -> Vec<String> {
    vec![s(PIO), s("test")]
}

/// `pio check` — static code analysis (PlatformIO IDE "Check").
pub fn pio_check() -> Vec<String> {
    vec![s(PIO), s("check")]
}

/// `pio boards [query]` — the PlatformIO Board Explorer. Empty query lists all.
pub fn pio_boards(query: &str) -> Vec<String> {
    let mut v = vec![s(PIO), s("boards")];
    if !query.trim().is_empty() {
        v.push(s(query));
    }
    v
}

/// `pio pkg list` — installed packages/libraries for the project.
pub fn pio_pkg_list() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("list")]
}

/// `pio pkg uninstall -l <name>` — remove a project library.
pub fn pio_pkg_uninstall(name: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("uninstall"), s("-l"), s(name)]
}

/// `pio pkg update` — update the project's installed packages.
pub fn pio_pkg_update() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("update")]
}

/// `pio pkg search <query>` — search the PlatformIO registry (Library Manager).
pub fn pio_pkg_search(query: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("search"), s(query)]
}

/// `pio pkg outdated` — list installed packages with newer versions available.
pub fn pio_pkg_outdated() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("outdated")]
}

/// `pio pkg show <pkg>` — registry details for a package.
pub fn pio_pkg_show(pkg: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("show"), s(pkg)]
}

/// `pio debug` — the PlatformIO Unified Debugger for the project.
pub fn pio_debug() -> Vec<String> {
    vec![s(PIO), s("debug")]
}

/// `pio upgrade` — upgrade PlatformIO Core itself.
pub fn pio_upgrade() -> Vec<String> {
    vec![s(PIO), s("upgrade")]
}

/// `pio run -t <target>` — a built-in PlatformIO build target. Covers the
/// report targets (`size`, `envdump`, `compiledb`, `buildfs`, `cleanall`) and
/// the flashing targets (`uploadfs`, `uploadeep`, `bootloader`, `fuses`,
/// `nobuild`). Verified against `pio run --list-targets` on PlatformIO 6.1.19.
pub fn pio_run_target(target: &str) -> Vec<String> {
    vec![s(PIO), s("run"), s("-t"), s(target)]
}

/// `pio project config` — the project's computed configuration (all envs).
pub fn pio_project_config() -> Vec<String> {
    vec![s(PIO), s("project"), s("config")]
}

/// `pio project metadata` — dump the IDE/LSP metadata (include paths, defines,
/// compiler flags) PlatformIO exposes to editor extensions.
pub fn pio_project_metadata() -> Vec<String> {
    vec![s(PIO), s("project"), s("metadata")]
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

/// `pio pkg pack` — build a tarball of the current package (registry authoring).
pub fn pio_pkg_pack() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("pack")]
}

/// `pio pkg publish` — publish the current package to the PlatformIO registry.
pub fn pio_pkg_publish() -> Vec<String> {
    vec![s(PIO), s("pkg"), s("publish")]
}

/// `pio pkg unpublish <pkg>` — remove a previously published package.
pub fn pio_pkg_unpublish(pkg: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("unpublish"), s(pkg)]
}

/// `pio system info` — system-wide PlatformIO information (core version, paths,
/// Python, platforms).
pub fn pio_system_info() -> Vec<String> {
    vec![s(PIO), s("system"), s("info")]
}

/// `pio system prune -f` — remove unused caches/packages without prompting
/// (per project convention: no confirmation on maintenance ops).
pub fn pio_system_prune() -> Vec<String> {
    vec![s(PIO), s("system"), s("prune"), s("-f")]
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

/// `pio account token` — print (or regenerate) the account auth token.
pub fn pio_account_token() -> Vec<String> {
    vec![s(PIO), s("account"), s("token")]
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
        assert_eq!(pio_clean(), ["pio", "run", "-t", "clean"]);
        assert_eq!(pio_test(), ["pio", "test"]);
        assert_eq!(pio_check(), ["pio", "check"]);
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
        assert_eq!(pio_debug(), ["pio", "debug"]);
        assert_eq!(pio_upgrade(), ["pio", "upgrade"]);
    }

    #[test]
    fn pio_run_targets_build_argv() {
        assert_eq!(pio_run_target("size"), ["pio", "run", "-t", "size"]);
        assert_eq!(pio_run_target("compiledb"), ["pio", "run", "-t", "compiledb"]);
        assert_eq!(pio_run_target("uploadfs"), ["pio", "run", "-t", "uploadfs"]);
        assert_eq!(pio_run_target("nobuild"), ["pio", "run", "-t", "nobuild"]);
    }

    #[test]
    fn pio_project_and_system_subcommands() {
        assert_eq!(pio_project_config(), ["pio", "project", "config"]);
        assert_eq!(pio_project_metadata(), ["pio", "project", "metadata"]);
        assert_eq!(pio_system_info(), ["pio", "system", "info"]);
        assert_eq!(pio_system_prune(), ["pio", "system", "prune", "-f"]);
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
