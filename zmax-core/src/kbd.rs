//! Emacs `kbd`-syntax key descriptions → zmax key tokens.
//!
//! Emacs's `keymap-set` family takes the key as a `kbd` string: space-separated
//! key descriptions, each a chain of `C-`/`M-`/`S-` modifiers and a base key
//! (`x`, `RET`, `SPC`, `<f5>`, …). zmax spells the same thing differently:
//! `A-` for Meta, lowercase names for the named keys (`ret`, `space`, `tab`,
//! `esc`, `backspace`), and `F5` for the function keys.
//!
//! This module is the pure translation layer between the two. It does not
//! validate that the resulting token names a real key — the caller feeds each
//! token to `zmax_view::input::KeyEvent::from_str`, which is the authority.

/// Translate one Emacs key description (e.g. `C-M-x`, `RET`, `<f5>`) into the
/// zmax token for the same key (`C-A-x`, `ret`, `F5`).
///
/// Modifiers are translated in place and their order preserved; only the base
/// key is renamed. Returns `Err` for an empty description or a modifier with no
/// base key after it.
pub fn emacs_key_to_zmax(desc: &str) -> Result<String, String> {
    if desc.is_empty() {
        return Err("empty key description".to_string());
    }
    let mut mods: Vec<&str> = Vec::new();
    let mut rest = desc;
    loop {
        // A modifier prefix is a single letter followed by `-`, and there has to
        // be something after it (so a bare `-` key still parses).
        let bytes = rest.as_bytes();
        if bytes.len() >= 3 && bytes[1] == b'-' {
            let m = match bytes[0] {
                b'C' => Some("C"),
                b'M' | b'A' => Some("A"), // Emacs Meta / Alt → zmax A-
                b'S' => Some("S"),
                _ => None,
            };
            match m {
                Some(m) => {
                    mods.push(m);
                    rest = &rest[2..];
                    continue;
                }
                None => break,
            }
        }
        break;
    }
    if rest.is_empty() {
        return Err(format!("`{desc}`: modifier with no key"));
    }
    let base = base_key_to_zmax(rest)?;
    if mods.is_empty() {
        Ok(base)
    } else {
        Ok(format!("{}-{}", mods.join("-"), base))
    }
}

/// Translate the base (unmodified) part of an Emacs key description.
fn base_key_to_zmax(base: &str) -> Result<String, String> {
    // `<f5>`, `<return>`, `<tab>` … — Emacs's angle-bracket function-key syntax.
    let inner = base
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(base);
    if inner.is_empty() {
        return Err(format!("`{base}`: empty key name"));
    }
    let lower = inner.to_ascii_lowercase();
    // Function keys: f1..f24 → F1..F24.
    if let Some(n) = lower.strip_prefix('f') {
        if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
            return Ok(format!("F{n}"));
        }
    }
    let named = match lower.as_str() {
        "ret" | "return" | "enter" => Some("ret"),
        "spc" | "space" => Some("space"),
        "tab" => Some("tab"),
        "esc" | "escape" => Some("esc"),
        "del" | "backspace" => Some("backspace"),
        "deletechar" | "delete" => Some("del"),
        "up" => Some("up"),
        "down" => Some("down"),
        "left" => Some("left"),
        "right" => Some("right"),
        "home" => Some("home"),
        "end" => Some("end"),
        "prior" | "pageup" => Some("pageup"),
        "next" | "pagedown" => Some("pagedown"),
        "insert" => Some("ins"),
        "-" => Some("minus"),
        "<" => Some("lt"),
        ">" => Some("gt"),
        _ => None,
    };
    if let Some(n) = named {
        return Ok(n.to_string());
    }
    // A single character is itself (case-significant: `X` is Shift-x in Emacs
    // too, and zmax spells it the same way).
    if inner.chars().count() == 1 {
        return Ok(inner.to_string());
    }
    Err(format!("`{base}`: unknown key name"))
}

/// Translate a whole Emacs `kbd` string (`"C-c C-f"`, `"M-x"`, `"C-x <f5>"`)
/// into the sequence of zmax key tokens it describes.
pub fn emacs_kbd_to_zmax(spec: &str) -> Result<Vec<String>, String> {
    let parts: Vec<&str> = spec.split_whitespace().collect();
    if parts.is_empty() {
        return Err("empty key sequence".to_string());
    }
    parts.iter().map(|p| emacs_key_to_zmax(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_modified_keys() {
        assert_eq!(emacs_key_to_zmax("x").unwrap(), "x");
        assert_eq!(emacs_key_to_zmax("C-x").unwrap(), "C-x");
        assert_eq!(emacs_key_to_zmax("M-x").unwrap(), "A-x");
        assert_eq!(emacs_key_to_zmax("C-M-x").unwrap(), "C-A-x");
        // Shift stays S-, and a capital letter is left alone.
        assert_eq!(emacs_key_to_zmax("S-tab").unwrap(), "S-tab");
        assert_eq!(emacs_key_to_zmax("X").unwrap(), "X");
    }

    #[test]
    fn named_and_function_keys() {
        assert_eq!(emacs_key_to_zmax("RET").unwrap(), "ret");
        assert_eq!(emacs_key_to_zmax("SPC").unwrap(), "space");
        assert_eq!(emacs_key_to_zmax("TAB").unwrap(), "tab");
        assert_eq!(emacs_key_to_zmax("DEL").unwrap(), "backspace");
        assert_eq!(emacs_key_to_zmax("<f5>").unwrap(), "F5");
        assert_eq!(emacs_key_to_zmax("C-<f12>").unwrap(), "C-F12");
        assert_eq!(emacs_key_to_zmax("<return>").unwrap(), "ret");
    }

    #[test]
    fn sequences_split_on_spaces() {
        assert_eq!(
            emacs_kbd_to_zmax("C-c C-f").unwrap(),
            vec!["C-c".to_string(), "C-f".to_string()]
        );
        assert_eq!(
            emacs_kbd_to_zmax("C-x 4 f").unwrap(),
            vec!["C-x".to_string(), "4".to_string(), "f".to_string()]
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(emacs_kbd_to_zmax("").is_err());
        assert!(emacs_key_to_zmax("C-").is_err());
        assert!(emacs_key_to_zmax("frobnicate").is_err());
        // A bare `-` is the minus key, not a dangling modifier.
        assert_eq!(emacs_key_to_zmax("-").unwrap(), "minus");
    }
}
