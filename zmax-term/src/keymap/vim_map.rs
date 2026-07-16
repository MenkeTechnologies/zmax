//! Translate Vim `:map`-family commands into live zmax keymap bindings.
//!
//! EXTENSION — this is how real vimscript plugins' mappings take effect. A
//! `:nnoremap <leader>x :Foo<CR>` reaching us (from the vimlrs `:map` host
//! bridge, or a `:map`-family typable) is parsed into (modes, lhs key sequence,
//! rhs action) and recorded in a process-wide overlay. The overlay is merged on
//! top of `config.keys` and re-applied whenever the keymap is rebuilt (preset
//! swap, config refresh — both reload `config.keys` from the preset+disk and
//! would otherwise drop live mappings).
//!
//! Scope: additive mappings, all three zmax modes, the common Vim key
//! notation (`<C-x>`, `<CR>`, `<Esc>`, `<Space>`, `<Tab>`, `<leader>`, `<M-x>`…)
//! and both rhs shapes — `:Cmd<CR>` → a typable command, and a key sequence →
//! a replay macro. `:unmap`/`:mapclear` remove prior overlay entries (they can't
//! rebind base-preset keys). `<Plug>` rhs targets are resolved through a
//! separate `<Plug>` table.

use std::collections::HashMap;
use std::sync::Mutex;

use indexmap::IndexMap;
use zmax_view::document::Mode;
use zmax_view::input::{parse_macro, KeyEvent};

use crate::commands::MappableCommand;

use super::{merge_keys, KeyTrie, KeyTrieNode};

/// Vim's default `mapleader` (`:help mapleader`) — resolves `<leader>` in an
/// lhs when the sourced script hasn't overridden `g:mapleader`. vimlrs does not
/// yet substitute leaders at `:map`-time, so we resolve to the documented
/// default here.
const DEFAULT_LEADER: char = '\\';

/// One recorded runtime mapping. Kept so it can be re-applied on every keymap
/// rebuild (see module docs).
#[derive(Clone)]
struct UserMapping {
    modes: Vec<Mode>,
    keys: Vec<KeyEvent>,
    value: KeyTrie,
    /// The original `:map`-family command line, kept so `:mkvimrc`/`:mkexrc` can
    /// write the recorded mappings back out verbatim.
    raw: String,
}

/// Process-wide overlay of runtime `:map`s, plus the `<Plug>` resolution table.
struct MapState {
    mappings: Vec<UserMapping>,
    /// `(<Plug>Name, its rhs KeyTrie)`, so `nmap x <Plug>Name` resolves. A Vec
    /// (not a map) keeps the `static` constructor const; the table is small.
    plugs: Vec<(String, KeyTrie)>,
}

static STATE: Mutex<MapState> = Mutex::new(MapState {
    mappings: Vec::new(),
    plugs: Vec::new(),
});

/// Parse and record one `:map`-family command line (the whole line, including
/// the command word, e.g. `nnoremap <silent> <leader>x :Foo<CR>`). Returns a
/// short human description on success, or an error string for an unsupported /
/// malformed line. Does NOT touch the live keymap — the caller applies the
/// overlay via [`apply_user_mappings`] afterwards (usually by sending
/// `ConfigEvent::ApplyUserMappings`).
/// What a `:map`-family line did, so the caller can react. `Applied` mutated the
/// overlay (the caller re-applies it); `List` is a no-rhs query that should
/// display the current bindings for the given modes (vim lists them).
pub enum MapOutcome {
    Applied(String),
    List(Vec<Mode>),
}

pub fn register_map_line(line: &str) -> Result<MapOutcome, String> {
    let line = line.trim();
    // Command word = leading ASCII letters plus an optional trailing `!`.
    let alpha_end = line
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(line.len());
    let cmd_end = if line[alpha_end..].starts_with('!') {
        alpha_end + 1
    } else {
        alpha_end
    };
    let cmd = &line[..cmd_end];
    let rest = line[cmd_end..].trim();
    let (modes, unmap, clear, _noremap) =
        mode_from_cmd(cmd).ok_or_else(|| format!("`{cmd}` is not a supported map command"))?;

    if clear {
        let mut st = STATE.lock().unwrap();
        st.mappings.retain(|m| !shares_mode(&m.modes, &modes));
        return Ok(MapOutcome::Applied(format!(
            "mapclear ({})",
            modes_desc(&modes)
        )));
    }

    // Consume the leading `<silent>`/`<buffer>`/`<expr>`/… map arguments.
    let rest = strip_map_args(rest);
    // No arguments (e.g. `:map`, `:nmap`) — list the bindings for these modes,
    // exactly as vim does, rather than doing nothing.
    if rest.is_empty() {
        return Ok(MapOutcome::List(modes));
    }
    let (lhs_raw, rhs_raw) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    let keys = parse_macro(&vim_keys_to_zmax(lhs_raw))
        .map_err(|e| format!("bad lhs `{lhs_raw}`: {e}"))?;
    if keys.is_empty() {
        return Err(format!("lhs `{lhs_raw}` produced no keys"));
    }

    if unmap {
        let mut st = STATE.lock().unwrap();
        st.mappings
            .retain(|m| !(m.keys == keys && shares_mode(&m.modes, &modes)));
        return Ok(MapOutcome::Applied(format!("unmap {lhs_raw}")));
    }

    // `:map {lhs}` with no rhs is a query in vim — list the bindings for the mode.
    if rhs_raw.is_empty() {
        return Ok(MapOutcome::List(modes));
    }

    let value =
        rhs_to_value(rhs_raw, lhs_raw).ok_or_else(|| format!("unsupported rhs `{rhs_raw}`"))?;

    let mut st = STATE.lock().unwrap();
    // A `<Plug>Name` lhs defines a plug target rather than a real key binding.
    if let Some(plug) = plug_name(lhs_raw) {
        st.plugs.retain(|(n, _)| n != &plug);
        st.plugs.push((plug.clone(), value));
        return Ok(MapOutcome::Applied(format!("<Plug>{plug} defined")));
    }
    // Replace any existing overlay entry for the same lhs+modes.
    st.mappings
        .retain(|m| !(m.keys == keys && shares_mode(&m.modes, &modes)));
    st.mappings.push(UserMapping {
        modes: modes.clone(),
        keys,
        value,
        raw: line.to_string(),
    });
    Ok(MapOutcome::Applied(format!(
        "{} {lhs_raw}",
        modes_desc(&modes)
    )))
}

/// Merge every recorded runtime mapping on top of `keys` (the live
/// `config.keys`). Idempotent: re-applying overwrites identical leaves.
pub fn apply_user_mappings(keys: &mut HashMap<Mode, KeyTrie>) {
    let st = STATE.lock().unwrap();
    for m in &st.mappings {
        let leaf = build_nested(&m.keys, m.value.clone());
        let mut delta: HashMap<Mode, KeyTrie> = HashMap::new();
        for &mode in &m.modes {
            delta.insert(mode, leaf.clone());
        }
        merge_keys(keys, delta);
    }
}

/// Whether any runtime mappings are recorded (lets callers skip a rebuild).
pub fn has_user_mappings() -> bool {
    !STATE.lock().unwrap().mappings.is_empty()
}

/// The original command line of every recorded runtime mapping, in definition
/// order — for `:mkvimrc`/`:mkexrc` to write them back out.
pub fn export_map_lines() -> Vec<String> {
    STATE
        .lock()
        .unwrap()
        .mappings
        .iter()
        .map(|m| m.raw.clone())
        .collect()
}

// ── helpers ──────────────────────────────────────────────────────────────

/// Wrap `value` in a `KeyTrie::Node` chain following `keys`, innermost first,
/// so a multi-key lhs becomes nested nodes that `merge_keys` folds into the
/// existing trie without clobbering sibling bindings.
fn build_nested(keys: &[KeyEvent], value: KeyTrie) -> KeyTrie {
    let mut trie = value;
    for key in keys.iter().rev() {
        let mut map: IndexMap<KeyEvent, KeyTrie> = IndexMap::new();
        map.insert(*key, trie);
        trie = KeyTrie::Node(KeyTrieNode::new("", map));
    }
    trie
}

/// Map a Vim `:map`-family command word to `(zmax modes, is_unmap, is_clear,
/// is_noremap)`. Mirrors the mode-prefix table of Vim's mapping commands,
/// collapsed onto zmax's three modes (Normal/Select/Insert). Cmdline/
/// terminal/lang maps have no zmax mode and return `None`.
fn mode_from_cmd(cmd: &str) -> Option<(Vec<Mode>, bool, bool, bool)> {
    let (base, bang) = match cmd.strip_suffix('!') {
        Some(b) => (b, true),
        None => (cmd, false),
    };
    let (prefix, unmap, clear, noremap) = if let Some(p) = base.strip_suffix("mapclear") {
        (p, false, true, false)
    } else if let Some(p) = base.strip_suffix("noremap") {
        (p, false, false, true)
    } else if let Some(p) = base.strip_suffix("unmap") {
        (p, true, false, false)
    } else {
        let p = base.strip_suffix("map")?;
        (p, false, false, false)
    };
    let modes = match prefix {
        // `:map`/`:noremap` = Normal+Visual+Operator-pending; `:map!` = Insert+Cmdline.
        "" => {
            if bang {
                vec![Mode::Insert]
            } else {
                vec![Mode::Normal, Mode::Select]
            }
        }
        "n" => vec![Mode::Normal],
        "i" => vec![Mode::Insert],
        // Visual (`v`/`x`) and Select (`s`) all fold onto zmax's Select mode.
        "v" | "x" | "s" => vec![Mode::Select],
        // Operator-pending has no zmax equivalent; approximate with Normal.
        "o" => vec![Mode::Normal],
        // Cmdline / terminal / lang maps: no zmax mode.
        "c" | "t" | "l" => return None,
        _ => return None,
    };
    Some((modes, unmap, clear, noremap))
}

/// Strip the leading `<silent>`/`<buffer>`/`<nowait>`/`<expr>`/`<unique>`/
/// `<script>` map-argument tokens (Vim's `map_arguments`), returning the rest.
fn strip_map_args(mut rest: &str) -> &str {
    loop {
        let lower = rest.to_ascii_lowercase();
        let matched = [
            "<silent>", "<buffer>", "<nowait>", "<expr>", "<unique>", "<script>",
        ]
        .iter()
        .find_map(|a| lower.strip_prefix(a).map(|r| rest.len() - r.len()));
        match matched {
            Some(n) => rest = rest[n..].trim_start(),
            None => return rest,
        }
    }
}

/// The `<Plug>Name` payload of an lhs, if it is a plug definition.
fn plug_name(lhs: &str) -> Option<String> {
    let lower = lhs.to_ascii_lowercase();
    lower
        .strip_prefix("<plug>")
        .map(|_| lhs["<Plug>".len()..].to_string())
}

/// Translate a Vim rhs into the bound action. `:Cmd<CR>` / `<Cmd>Cmd<CR>` →
/// a typable command; `<Plug>Name` → its recorded target (if known); otherwise
/// a key sequence replayed as a macro.
fn rhs_to_value(rhs: &str, lhs_for_name: &str) -> Option<KeyTrie> {
    let rhs = rhs.trim();
    if rhs.is_empty() {
        return None;
    }
    // `<Plug>Name` rhs — resolve against the plug table (may be defined later;
    // unresolved plugs are dropped rather than mis-bound).
    if let Some(name) = plug_name(rhs) {
        return STATE
            .lock()
            .unwrap()
            .plugs
            .iter()
            .find(|(n, _)| n == &name)
            .map(|(_, v)| v.clone());
    }
    // `:Cmd …<CR>` or nvim `<Cmd>Cmd …<CR>` → a typable command.
    let cmd_body = rhs.strip_prefix(':').or_else(|| {
        let lower = rhs.to_ascii_lowercase();
        lower.strip_prefix("<cmd>").map(|_| &rhs["<Cmd>".len()..])
    });
    if let Some(body) = cmd_body {
        let body = strip_trailing_cr(body).trim();
        if body.is_empty() {
            return None;
        }
        let (word, args) = match body.find(char::is_whitespace) {
            Some(i) => (&body[..i], body[i..].trim()),
            None => (body, ""),
        };
        return Some(KeyTrie::MappableCommand(MappableCommand::Typable {
            name: word.to_string(),
            args: args.to_string(),
            doc: String::new(),
        }));
    }
    // A key sequence rhs → replay it as a macro.
    match parse_macro(&vim_keys_to_zmax(rhs)) {
        Ok(keys) if !keys.is_empty() => Some(KeyTrie::MappableCommand(MappableCommand::Macro {
            name: format!("vim map {lhs_for_name}"),
            keys,
        })),
        _ => None,
    }
}

/// Drop a trailing `<CR>`/`<Enter>`/`<Return>` (any case) from a command rhs.
fn strip_trailing_cr(s: &str) -> &str {
    let t = s.trim_end();
    for suffix in ["<cr>", "<enter>", "<return>"] {
        if t.to_ascii_lowercase().ends_with(suffix) {
            return t[..t.len() - suffix.len()].trim_end();
        }
    }
    t
}

/// Rewrite a Vim key string into the zmax macro notation understood by
/// [`parse_macro`]: each `<…>` token is normalized (`<CR>`→`<ret>`, `<M-x>`→
/// `<A-x>`, `<leader>`→the leader char, …) and plain characters pass through.
fn vim_keys_to_zmax(vim: &str) -> String {
    let bytes = vim.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < vim.len() {
        if bytes[i] == b'<' {
            if let Some(rel) = vim[i..].find('>') {
                let inner = &vim[i + 1..i + rel];
                if let Some(tok) = rewrite_special(inner) {
                    // A single bare char (e.g. leader `\`, `<Bar>` `|`) needs no
                    // angle brackets; multi-char / modified tokens (`ret`, `A-x`)
                    // must stay wrapped so `parse_macro` reads them as one key.
                    if tok.chars().count() == 1 {
                        out.push_str(&tok);
                    } else {
                        out.push('<');
                        out.push_str(&tok);
                        out.push('>');
                    }
                }
                i += rel + 1;
                continue;
            }
        }
        let ch = vim[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Rewrite the inside of one Vim `<…>` token to a zmax `KeyEvent`-parseable
/// token. Splits the leading modifier run (`C-`/`S-`/`A-`/`M-`/`D-`), mapping
/// Vim's `M`/`D` (Meta/Cmd) onto zmax's `A` (Alt), then rewrites the base key
/// name. Returns `None` for `<Nop>` (produces no key).
fn rewrite_special(inner: &str) -> Option<String> {
    let mut mods = String::new();
    let mut rest = inner;
    while rest.len() >= 2 && rest.as_bytes()[1] == b'-' {
        let m = rest.as_bytes()[0].to_ascii_uppercase();
        let z = match m {
            b'C' | b'S' | b'A' => m as char,
            b'M' | b'D' => 'A', // Meta / Cmd → Alt
            b'T' => 'A',        // best effort
            _ => break,
        };
        mods.push(z);
        mods.push('-');
        rest = &rest[2..];
    }
    let base = match rest.to_ascii_lowercase().as_str() {
        "cr" | "return" | "enter" => "ret".to_string(),
        "esc" => "esc".to_string(),
        "space" => "space".to_string(),
        "tab" => "tab".to_string(),
        "bs" | "backspace" => "backspace".to_string(),
        "del" | "delete" => "del".to_string(),
        "ins" | "insert" => "ins".to_string(),
        "up" | "down" | "left" | "right" | "home" | "end" => rest.to_ascii_lowercase(),
        "pageup" => "pageup".to_string(),
        "pagedown" => "pagedown".to_string(),
        "lt" => "lt".to_string(),
        "bar" => "|".to_string(),
        "bslash" => "\\".to_string(),
        "leader" | "localleader" => DEFAULT_LEADER.to_string(),
        "nop" | "" => return None,
        // Function keys `F1`..`F12` and single characters pass through with
        // their original case (`KeyEvent::from_str` wants `F5`, `x`, `X`, …).
        _ => rest.to_string(),
    };
    Some(format!("{mods}{base}"))
}

fn shares_mode(a: &[Mode], b: &[Mode]) -> bool {
    a.iter().any(|m| b.contains(m))
}

fn modes_desc(modes: &[Mode]) -> String {
    modes
        .iter()
        .map(Mode::to_string)
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyseq(s: &str) -> Vec<KeyEvent> {
        parse_macro(&vim_keys_to_zmax(s)).unwrap()
    }

    #[test]
    fn normalizes_common_vim_notation() {
        // <CR>/<Esc>/<Space>/<Tab> map to zmax names; plain chars pass through.
        assert_eq!(vim_keys_to_zmax("<CR>"), "<ret>");
        assert_eq!(vim_keys_to_zmax("<Esc>"), "<esc>");
        assert_eq!(vim_keys_to_zmax("<C-w>v"), "<C-w>v");
        assert_eq!(vim_keys_to_zmax("<M-x>"), "<A-x>");
        assert_eq!(vim_keys_to_zmax("<leader>gs"), "\\gs");
        assert_eq!(vim_keys_to_zmax("<Nop>"), "");
        // all of these must parse into key sequences
        for s in ["<C-w>v", "<leader>x", "gcc", "<C-CR>", "<S-Tab>"] {
            assert!(!keyseq(s).is_empty(), "failed: {s}");
        }
    }

    #[test]
    fn cmd_rhs_becomes_typable() {
        assert!(matches!(
            rhs_to_value(":Files<CR>", "x"),
            Some(KeyTrie::MappableCommand(MappableCommand::Typable { .. }))
        ));
    }

    #[test]
    fn key_rhs_becomes_macro() {
        assert!(matches!(
            rhs_to_value("<C-w>h", "x"),
            Some(KeyTrie::MappableCommand(MappableCommand::Macro { .. }))
        ));
    }

    #[test]
    fn mode_prefixes() {
        assert_eq!(mode_from_cmd("nnoremap").unwrap().0, vec![Mode::Normal]);
        assert_eq!(mode_from_cmd("inoremap").unwrap().0, vec![Mode::Insert]);
        assert_eq!(mode_from_cmd("vmap").unwrap().0, vec![Mode::Select]);
        assert_eq!(mode_from_cmd("xnoremap").unwrap().0, vec![Mode::Select]);
        assert_eq!(
            mode_from_cmd("map").unwrap().0,
            vec![Mode::Normal, Mode::Select]
        );
        assert!(mode_from_cmd("cnoremap").is_none()); // cmdline: no zmax mode
        assert!(mode_from_cmd("nnoremap").unwrap().3); // noremap flag
    }

    // Serializes tests that mutate the process-global STATE.mappings (they run
    // in parallel otherwise and would clobber each other).
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn no_rhs_lists_bindings() {
        let _g = TEST_GUARD.lock().unwrap();
        assert!(
            matches!(register_map_line("nmap").unwrap(), MapOutcome::List(ref m) if *m == vec![Mode::Normal])
        );
        assert!(
            matches!(register_map_line("vmap").unwrap(), MapOutcome::List(ref m) if *m == vec![Mode::Select])
        );
        assert!(matches!(
            register_map_line("map").unwrap(),
            MapOutcome::List(_)
        ));
        // a bound lhs (no rhs) also lists; a full mapping applies.
        assert!(matches!(
            register_map_line("nmap gx").unwrap(),
            MapOutcome::List(_)
        ));
        assert!(matches!(
            register_map_line("nnoremap gx :Foo<CR>").unwrap(),
            MapOutcome::Applied(_)
        ));
        STATE.lock().unwrap().mappings.clear();
    }

    #[test]
    fn register_and_apply_roundtrip() {
        let _g = TEST_GUARD.lock().unwrap();
        // clean slate for this test
        STATE.lock().unwrap().mappings.clear();
        register_map_line("nnoremap <silent> <leader>w :write<CR>").unwrap();
        let mut keys: HashMap<Mode, KeyTrie> = HashMap::new();
        keys.insert(Mode::Normal, KeyTrie::Node(KeyTrieNode::default()));
        apply_user_mappings(&mut keys);
        let normal = &keys[&Mode::Normal];
        let seq = keyseq("<leader>w");
        assert!(normal.search(&seq).is_some(), "mapping not installed");
        STATE.lock().unwrap().mappings.clear();
    }
}
