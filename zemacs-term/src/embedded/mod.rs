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

/// `arduino-cli upload -p <port> --fqbn <fqbn> <sketch>`
pub fn arduino_upload(settings: &EmbeddedSettings) -> Result<Vec<String>, String> {
    if settings.fqbn.is_empty() {
        return Err("no board selected — run :arduino-boards to pick an FQBN".into());
    }
    if settings.port.is_empty() {
        return Err("no serial port selected — run :arduino-ports".into());
    }
    Ok(vec![
        s(ARDUINO_CLI),
        s("upload"),
        s("-p"),
        settings.port.clone(),
        s("--fqbn"),
        settings.fqbn.clone(),
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

/// `pio pkg install -l <name>`
pub fn pio_lib_install(name: &str) -> Vec<String> {
    vec![s(PIO), s("pkg"), s("install"), s("-l"), s(name)]
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
    fn settings_roundtrip_toml() {
        let st = settings();
        let text = toml::to_string_pretty(&st).unwrap();
        let back: EmbeddedSettings = toml::from_str(&text).unwrap();
        assert_eq!(back.fqbn, st.fqbn);
        assert_eq!(back.baud, st.baud);
        assert_eq!(back.backend, Backend::Arduino);
    }
}
