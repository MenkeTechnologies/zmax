//! Pure formatters for the Emacs GUD/GDB data-buffer views (locals, registers,
//! stack, threads and a memory hexdump).
//!
//! These functions take already-fetched, owned data (produced from DAP
//! `scopes`/`variables`/`stackTrace`/`threads`/`readMemory` responses) and turn
//! it into the plain text zmax renders in a popup. Keeping them free of any
//! I/O or DAP types means they are unit-testable without a live debug adapter,
//! which is where the GNU Emacs `gdb-mode` data buffers get their content.

/// One variable or register row: a name, an optional type, and a value string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarRow {
    /// The variable/register name.
    pub name: String,
    /// The declared type, when the adapter reports one.
    pub ty: Option<String>,
    /// The current value, already rendered by the adapter.
    pub value: String,
}

impl VarRow {
    /// Convenience constructor.
    pub fn new(name: impl Into<String>, ty: Option<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ty,
            value: value.into(),
        }
    }
}

/// A named scope (e.g. `Locals`, `Registers`) with its variable rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRows {
    /// The scope's display name.
    pub name: String,
    /// The variables inside the scope.
    pub vars: Vec<VarRow>,
}

/// One stack-frame row for the stack view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackRow {
    /// The frame level (0 = innermost).
    pub level: usize,
    /// The function/frame name.
    pub name: String,
    /// The source location string (`file:line`), when known.
    pub location: Option<String>,
}

/// One thread row for the threads view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRow {
    /// The DAP thread id.
    pub id: isize,
    /// The thread name.
    pub name: String,
    /// The thread state (e.g. `stopped`, `running`).
    pub state: String,
}

/// Render a single scope's variables, GUD "locals buffer" style.
///
/// Each row is `  name: type = value` (the `: type` is omitted when unknown).
/// An empty scope renders a single `  <no variables>` line.
pub fn format_scope(scope: &ScopeRows) -> String {
    let mut out = format!("[{}]\n", scope.name);
    if scope.vars.is_empty() {
        out.push_str("  <no variables>\n");
        return out;
    }
    for v in &scope.vars {
        match &v.ty {
            Some(ty) => out.push_str(&format!("  {}: {} = {}\n", v.name, ty, v.value)),
            None => out.push_str(&format!("  {} = {}\n", v.name, v.value)),
        }
    }
    out
}

/// Render several scopes back to back (the full locals/registers buffer body).
pub fn format_scopes(scopes: &[ScopeRows]) -> String {
    if scopes.is_empty() {
        return "<no scopes reported by the debug adapter>\n".to_string();
    }
    scopes.iter().map(format_scope).collect()
}

/// Render the stack view for a thread. `active` marks the selected frame with
/// `>`; every other frame is prefixed with a space so columns line up.
pub fn format_stack(rows: &[StackRow], active: Option<usize>) -> String {
    if rows.is_empty() {
        return "<no stack frames>\n".to_string();
    }
    let mut out = String::new();
    for r in rows {
        let marker = if active == Some(r.level) { '>' } else { ' ' };
        match &r.location {
            Some(loc) => out.push_str(&format!("{}#{} {} at {}\n", marker, r.level, r.name, loc)),
            None => out.push_str(&format!("{}#{} {}\n", marker, r.level, r.name)),
        }
    }
    out
}

/// Render the threads view. `current` marks the selected thread with `>`.
pub fn format_threads(rows: &[ThreadRow], current: Option<isize>) -> String {
    if rows.is_empty() {
        return "<no threads>\n".to_string();
    }
    let mut out = String::new();
    for r in rows {
        let marker = if current == Some(r.id) { '>' } else { ' ' };
        out.push_str(&format!("{}{}: {} ({})\n", marker, r.id, r.name, r.state));
    }
    out
}

/// A classic `xxd`-style hexdump: `ADDR: HEXBYTES  ASCII`, 16 bytes per line.
///
/// The address column is the 8-digit hex of `base_addr + row_offset`. The hex
/// column is always padded to a fixed width so the ASCII gutter aligns even on
/// a short final row. Non-printable bytes render as `.` in the ASCII gutter.
pub fn hexdump(base_addr: u64, data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    // 16 bytes * "xx " (three chars each) = 48 columns for the hex field.
    const HEX_WIDTH: usize = 48;
    let mut out = String::new();
    for (row, chunk) in data.chunks(16).enumerate() {
        let addr = base_addr.wrapping_add((row * 16) as u64);
        let mut hex = String::with_capacity(HEX_WIDTH);
        let mut ascii = String::with_capacity(16);
        for &b in chunk {
            hex.push_str(&format!("{:02x} ", b));
            ascii.push(if (0x20..=0x7e).contains(&b) {
                b as char
            } else {
                '.'
            });
        }
        out.push_str(&format!(
            "{:08x}: {:<width$}{}\n",
            addr,
            hex,
            ascii,
            width = HEX_WIDTH
        ));
    }
    out
}

/// Decode standard base64 (the encoding DAP `readMemory` uses for its `data`
/// field) into raw bytes. Whitespace is ignored; any other invalid input
/// returns `None`.
pub fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    // Strip whitespace, keeping padding.
    let cleaned: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !cleaned.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
    for quad in cleaned.chunks(4) {
        let pad = quad.iter().filter(|&&b| b == b'=').count();
        if pad > 2 {
            return None;
        }
        let mut acc: u32 = 0;
        for (i, &b) in quad.iter().enumerate() {
            let six = if b == b'=' {
                // Padding is only allowed at the end of the quad.
                if i < 4 - pad {
                    return None;
                }
                0
            } else {
                val(b)? as u32
            };
            acc = (acc << 6) | six;
        }
        let bytes = [(acc >> 16) as u8, (acc >> 8) as u8, acc as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_with_and_without_types() {
        let scope = ScopeRows {
            name: "Locals".to_string(),
            vars: vec![
                VarRow::new("x", Some("int".to_string()), "5"),
                VarRow::new("msg", None, "\"hi\""),
            ],
        };
        assert_eq!(
            format_scope(&scope),
            "[Locals]\n  x: int = 5\n  msg = \"hi\"\n"
        );
    }

    #[test]
    fn empty_scope_and_no_scopes() {
        let scope = ScopeRows {
            name: "Registers".to_string(),
            vars: vec![],
        };
        assert_eq!(format_scope(&scope), "[Registers]\n  <no variables>\n");
        assert_eq!(
            format_scopes(&[]),
            "<no scopes reported by the debug adapter>\n"
        );
    }

    #[test]
    fn multiple_scopes_concatenate() {
        let scopes = vec![
            ScopeRows {
                name: "Locals".to_string(),
                vars: vec![VarRow::new("a", None, "1")],
            },
            ScopeRows {
                name: "Arguments".to_string(),
                vars: vec![VarRow::new("argc", Some("int".to_string()), "2")],
            },
        ];
        assert_eq!(
            format_scopes(&scopes),
            "[Locals]\n  a = 1\n[Arguments]\n  argc: int = 2\n"
        );
    }

    #[test]
    fn stack_marks_active_frame() {
        let rows = vec![
            StackRow {
                level: 0,
                name: "main".to_string(),
                location: Some("src/main.rs:10".to_string()),
            },
            StackRow {
                level: 1,
                name: "start".to_string(),
                location: None,
            },
        ];
        assert_eq!(
            format_stack(&rows, Some(0)),
            ">#0 main at src/main.rs:10\n #1 start\n"
        );
        assert_eq!(format_stack(&[], None), "<no stack frames>\n");
    }

    #[test]
    fn threads_mark_current() {
        let rows = vec![
            ThreadRow {
                id: 1,
                name: "main".to_string(),
                state: "stopped".to_string(),
            },
            ThreadRow {
                id: 2,
                name: "worker".to_string(),
                state: "running".to_string(),
            },
        ];
        assert_eq!(
            format_threads(&rows, Some(2)),
            " 1: main (stopped)\n>2: worker (running)\n"
        );
    }

    #[test]
    fn hexdump_partial_row_pads_ascii_gutter() {
        let out = hexdump(0, &[0x00, 0x41, 0x42, 0xff]);
        let expected = format!("00000000: 00 41 42 ff {}{}\n", " ".repeat(36), ".AB.");
        assert_eq!(out, expected);
        assert!(hexdump(0, &[]).is_empty());
    }

    #[test]
    fn hexdump_full_and_second_row_addresses() {
        // 0x20..=0x3f is all printable ASCII, so the gutter mirrors the bytes.
        let data: Vec<u8> = (0x20u8..=0x3f).collect();
        let out = hexdump(0x1000, &data);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("00001000: 20 21 22 23 24 25 26 27 28 29 2a 2b 2c 2d 2e 2f "));
        assert!(lines[0].ends_with(" !\"#$%&'()*+,-./"));
        assert!(lines[1].starts_with("00001010: 30 31 32 33"));
        assert!(lines[1].ends_with("0123456789:;<=>?"));
    }

    #[test]
    fn base64_roundtrip_known_vectors() {
        assert_eq!(decode_base64("TWFu").unwrap(), b"Man");
        assert_eq!(decode_base64("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(decode_base64("SGVsbG8h").unwrap(), b"Hello!");
        assert_eq!(decode_base64("Zm9vYmE=").unwrap(), b"fooba");
        assert_eq!(decode_base64("Zm8=").unwrap(), b"fo");
        assert_eq!(decode_base64("").unwrap(), Vec::<u8>::new());
        // Whitespace between groups is ignored.
        assert_eq!(decode_base64("SGVs\nbG8=").unwrap(), b"Hello");
    }

    #[test]
    fn base64_rejects_bad_input() {
        assert!(decode_base64("abc").is_none()); // not a multiple of 4
        assert!(decode_base64("****").is_none()); // invalid alphabet
        assert!(decode_base64("A===").is_none()); // too much padding
    }
}
