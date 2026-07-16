//! End-to-end: `:arduino-compile` on a broken sketch produces avr-gcc
//! diagnostics that the Emacs `*compilation*` parser turns into navigable
//! error entries — so `:next-error` jumps straight to the offending `.ino` line.
//!
//! This drives the real backend: it builds the exact `arduino-cli` argument
//! vector the command handler uses (`embedded::arduino_compile`), runs it against
//! the installed toolchain, and feeds the captured output through the same
//! `zmax_core::compilation::parse_output` the editor uses. It is skipped (not
//! failed) when `arduino-cli` or the `arduino:avr:uno` board isn't installed, so
//! it never breaks a machine without the embedded toolchain.

use zmax_core::compilation;
use zmax_term::embedded::{self, Backend, EmbeddedSettings};

/// Is `arduino-cli` present and does it know the `arduino:avr:uno` board?
fn toolchain_ready() -> bool {
    if !embedded::tool_available(embedded::ARDUINO_CLI) {
        return false;
    }
    std::process::Command::new(embedded::ARDUINO_CLI)
        .args(["board", "details", "-b", "arduino:avr:uno"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn arduino_compile_errors_become_navigable_compilation_entries() {
    if !toolchain_ready() {
        eprintln!("SKIP: arduino-cli / arduino:avr core not installed");
        return;
    }

    // A sketch with a deliberate syntax error (missing comma) in an .ino.
    let dir = tempfile::tempdir().unwrap();
    let sketch = dir.path().join("BlinkBad");
    std::fs::create_dir_all(&sketch).unwrap();
    std::fs::write(
        sketch.join("BlinkBad.ino"),
        "void setup() {\n  pinMode(LED_BUILTIN OUTPUT);\n}\nvoid loop() {}\n",
    )
    .unwrap();

    // Build the argv exactly as the `:arduino-compile` handler does, but point the
    // sketch dir at our temp sketch.
    let settings = EmbeddedSettings {
        backend: Backend::Arduino,
        fqbn: "arduino:avr:uno".into(),
        sketch: String::new(),
        ..Default::default()
    };
    let mut argv = embedded::arduino_compile(&settings).expect("argv builds with an fqbn");
    // Replace the trailing sketch-dir argument with our temp sketch path.
    *argv.last_mut().unwrap() = sketch.to_string_lossy().into_owned();

    let output = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .expect("arduino-cli runs");
    assert!(
        !output.status.success(),
        "the broken sketch must fail to compile"
    );

    // Emacs `compile` scans stdout+stderr interleaved.
    let mut captured = String::from_utf8_lossy(&output.stdout).into_owned();
    captured.push_str(&String::from_utf8_lossy(&output.stderr));

    let entries = compilation::parse_output(&captured);
    assert!(
        !entries.is_empty(),
        "compilation parser found no error locations in:\n{captured}"
    );

    // The real gcc sequence emits a `note:` and an `error:` both at line 2 of the
    // sketch; `:next-error` visits every such location. Prove the actual error
    // location — the one that matters — was captured and points at line 2.
    let sketch_hits: Vec<_> = entries
        .iter()
        .filter(|e| e.file.ends_with("BlinkBad.ino"))
        .collect();
    assert!(
        !sketch_hits.is_empty(),
        "no compilation entry referenced BlinkBad.ino in:\n{captured}"
    );
    assert!(
        sketch_hits
            .iter()
            .any(|e| e.line == 2 && e.kind == compilation::ErrorKind::Error),
        "expected an ERROR entry at BlinkBad.ino:2, got: {sketch_hits:?}"
    );
}
