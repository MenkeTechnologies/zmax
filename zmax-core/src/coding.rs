//! Coding systems — the zmax port of the GNU Emacs `recode-region` /
//! `recode-file-name` re-decoding commands.
//!
//! When a file is read with the wrong coding system its bytes are decoded into
//! the wrong characters (the classic mojibake). Emacs fixes that without
//! re-reading the file: it encodes the text back with the coding system it was
//! *mistakenly* decoded as — reconstructing the original bytes — and decodes
//! those bytes again with the right one. That round trip is what this module is;
//! it is pure, and `encoding_rs` (already a core dependency) provides the tables.

/// Text that was decoded with the wrong coding system, re-decoded with the right
/// one. `interpreted_as` is the coding system the bytes were mistakenly read
/// with; `really_in` is what they actually were.
///
/// Unknown coding-system names, text the wrong coding system cannot represent
/// (so the original bytes cannot be reconstructed), and bytes that are not valid
/// in the target are all errors — Emacs signals in those cases rather than
/// silently mangling the buffer.
pub fn recode(text: &str, interpreted_as: &str, really_in: &str) -> Result<String, String> {
    let wrong = lookup(interpreted_as)?;
    let right = lookup(really_in)?;
    let (bytes, _, had_errors) = wrong.encode(text);
    if had_errors {
        return Err(format!(
            "the text cannot be represented in {}",
            wrong.name()
        ));
    }
    let (decoded, _, malformed) = right.decode(&bytes);
    if malformed {
        return Err(format!("the bytes are not valid {}", right.name()));
    }
    Ok(decoded.into_owned())
}

/// Resolve a coding-system name to an encoding. Accepts every `encoding_rs`
/// label plus the Emacs spellings that are not IANA labels (`latin-1`,
/// `mule-utf-8`, the `-unix`/`-dos`/`-mac` EOL suffixes Emacs appends).
pub fn lookup(name: &str) -> Result<&'static Encoding, String> {
    let name = name.trim();
    // Emacs writes the end-of-line convention as a suffix on the coding system
    // (`utf-8-unix`, `latin-1-dos`). The EOL half is the document's line ending,
    // not its encoding, so strip it before asking encoding_rs.
    let base = ["-unix", "-dos", "-mac"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    let base = match base.to_ascii_lowercase().as_str() {
        "latin-1" | "latin1" => "iso-8859-1",
        "mule-utf-8" | "prefer-utf-8" | "undecided" => "utf-8",
        "chinese-gbk" => "gbk",
        "japanese-shift-jis" | "sjis" => "shift_jis",
        _ => base,
    };
    Encoding::for_label(base.as_bytes()).ok_or_else(|| format!("unknown coding system: {name}"))
}

// ---------------------------------------------------------------------------
// The coding-system registry: the settings Emacs's `C-x RET` map writes.
//
// Each of these governs a byte<->character conversion that really happens
// somewhere in zmax, and the code that performs that conversion reads the
// value from here. They live in `zmax-core` because the conversions are spread
// across crates: the clipboard is in `zmax-view`, the subprocess pipes and the
// hosted terminal are in `zmax-term`.
// ---------------------------------------------------------------------------

use crate::encoding::Encoding;
use std::sync::RwLock;

/// One settable coding system. `None` = the default (UTF-8, plus lossy decoding
/// where the conversion cannot fail).
type Slot = RwLock<Option<&'static Encoding>>;

/// `set-terminal-coding-system`: how the bytes of the terminal zmax *hosts*
/// (the `M-x term` PTY) are decoded into characters.
static TERMINAL: Slot = RwLock::new(None);
/// `set-keyboard-coding-system`: how typed characters are encoded into the bytes
/// sent to that terminal's PTY.
static KEYBOARD: Slot = RwLock::new(None);
/// `set-selection-coding-system`: the bytes exchanged with the window system's
/// clipboard.
static SELECTION: Slot = RwLock::new(None);
/// `set-next-selection-coding-system`: same, but consumed by the next clipboard
/// transfer only.
static NEXT_SELECTION: Slot = RwLock::new(None);
/// `set-buffer-process-coding-system`: the bytes of a subprocess's pipes
/// (`M-x shell`, the `:sh` family). Emacs makes this a pair (decode, encode).
static PROCESS_DECODE: Slot = RwLock::new(None);
static PROCESS_ENCODE: Slot = RwLock::new(None);
/// `set-file-name-coding-system`: how a file *name* is encoded into the bytes the
/// filesystem stores.
static FILE_NAME: Slot = RwLock::new(None);

fn get(slot: &Slot) -> Option<&'static Encoding> {
    slot.read().ok().and_then(|g| *g)
}

fn put(slot: &Slot, encoding: Option<&'static Encoding>) {
    if let Ok(mut g) = slot.write() {
        *g = encoding;
    }
}

pub fn terminal_coding() -> Option<&'static Encoding> {
    get(&TERMINAL)
}
pub fn set_terminal_coding(encoding: Option<&'static Encoding>) {
    put(&TERMINAL, encoding);
}

pub fn keyboard_coding() -> Option<&'static Encoding> {
    get(&KEYBOARD)
}
pub fn set_keyboard_coding(encoding: Option<&'static Encoding>) {
    put(&KEYBOARD, encoding);
}

/// The coding system the next clipboard transfer uses: the one-shot
/// `set-next-selection-coding-system` if one is armed, else the persistent
/// `set-selection-coding-system`. **Taking** it is what makes the one-shot
/// one-shot, so this is called once per transfer.
pub fn take_selection_coding() -> Option<&'static Encoding> {
    let next = NEXT_SELECTION.write().ok().and_then(|mut g| g.take());
    next.or_else(|| get(&SELECTION))
}
pub fn selection_coding() -> Option<&'static Encoding> {
    get(&SELECTION)
}
pub fn set_selection_coding(encoding: Option<&'static Encoding>) {
    put(&SELECTION, encoding);
}
pub fn set_next_selection_coding(encoding: Option<&'static Encoding>) {
    put(&NEXT_SELECTION, encoding);
}

/// `(decode, encode)` for subprocess pipes.
pub fn process_coding() -> (Option<&'static Encoding>, Option<&'static Encoding>) {
    (get(&PROCESS_DECODE), get(&PROCESS_ENCODE))
}
pub fn set_process_coding(decode: Option<&'static Encoding>, encode: Option<&'static Encoding>) {
    put(&PROCESS_DECODE, decode);
    put(&PROCESS_ENCODE, encode);
}

pub fn file_name_coding() -> Option<&'static Encoding> {
    get(&FILE_NAME)
}
pub fn set_file_name_coding(encoding: Option<&'static Encoding>) {
    put(&FILE_NAME, encoding);
}

/// Decode subprocess/terminal bytes with the coding system `slot` names, falling
/// back to lossy UTF-8 — which is what every existing decode site in zmax does,
/// so an unset coding system changes nothing.
pub fn decode_with(encoding: Option<&'static Encoding>, bytes: &[u8]) -> String {
    match encoding {
        Some(e) => e.decode(bytes).0.into_owned(),
        None => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Encode text for a subprocess/terminal, falling back to UTF-8.
pub fn encode_with(encoding: Option<&'static Encoding>, text: &str) -> Vec<u8> {
    match encoding {
        Some(e) => e.encode(text).0.into_owned(),
        None => text.as_bytes().to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Language environments (Emacs `set-language-environment`).
// ---------------------------------------------------------------------------

/// The language environments zmax knows, and the coding system each one makes
/// the default. Emacs's table also carries input methods, character sets and
/// sample text; the coding system is the part that has an effect here.
pub const LANGUAGE_ENVIRONMENTS: &[(&str, &str)] = &[
    ("UTF-8", "utf-8"),
    ("English", "utf-8"),
    ("ASCII", "utf-8"),
    ("Latin-1", "iso-8859-1"),
    ("Latin-2", "iso-8859-2"),
    ("Latin-3", "iso-8859-3"),
    ("Latin-4", "iso-8859-4"),
    ("Latin-5", "iso-8859-9"),
    ("Latin-6", "iso-8859-10"),
    ("Latin-7", "iso-8859-13"),
    ("Latin-8", "iso-8859-14"),
    ("Latin-9", "iso-8859-15"),
    ("Greek", "iso-8859-7"),
    ("Hebrew", "iso-8859-8"),
    ("Cyrillic-ISO", "iso-8859-5"),
    ("Cyrillic-KOI8", "koi8-r"),
    ("Cyrillic-ALT", "windows-1251"),
    ("Ukrainian", "koi8-u"),
    ("Japanese", "euc-jp"),
    ("Chinese-GB", "gbk"),
    ("Chinese-GB18030", "gb18030"),
    ("Chinese-BIG5", "big5"),
    ("Korean", "euc-kr"),
    ("Thai", "windows-874"),
    ("Turkish", "iso-8859-9"),
    ("Vietnamese", "windows-1258"),
];

/// The coding system a language environment makes the default, matched
/// case-insensitively.
pub fn language_environment_coding(name: &str) -> Option<&'static Encoding> {
    LANGUAGE_ENVIRONMENTS
        .iter()
        .find(|(env, _)| env.eq_ignore_ascii_case(name.trim()))
        .and_then(|(_, coding)| Encoding::for_label(coding.as_bytes()))
}

/// The coding system a POSIX locale names: the codeset after the `.`, with the
/// `@modifier` stripped. `en_US.UTF-8` -> UTF-8, `ru_RU.KOI8-R` -> KOI8-R,
/// `C`/`POSIX` -> US-ASCII (which `encoding_rs` answers with windows-1252, its
/// ASCII-compatible superset). A locale with no codeset yields `None`, which is
/// what Emacs treats as "leave the default alone".
pub fn locale_coding(locale: &str) -> Option<&'static Encoding> {
    let codeset = locale.split('@').next()?.split('.').nth(1)?;
    // POSIX spells several codesets in ways the WHATWG label table does not know
    // (glibc's `eucJP`, `ISO8859-1`), so normalise before looking up.
    let lower = codeset.to_ascii_lowercase();
    let normalised = match lower.as_str() {
        "eucjp" => "euc-jp".to_string(),
        "euckr" => "euc-kr".to_string(),
        "euccn" => "gbk".to_string(),
        "sjis" => "shift_jis".to_string(),
        other => match other.strip_prefix("iso8859-") {
            Some(part) => format!("iso-8859-{part}"),
            None => other.to_string(),
        },
    };
    Encoding::for_label(normalised.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTF-8 text read as Latin-1 comes out as mojibake; recoding it back the
    /// other way restores the original — the whole point of `recode-region`.
    #[test]
    fn recode_undoes_a_wrong_decoding() {
        // "héllo" in UTF-8, decoded as windows-1252, is "hÃ©llo".
        let mojibake = "hÃ©llo";
        assert_eq!(recode(mojibake, "windows-1252", "utf-8").unwrap(), "héllo");
        // …and the inverse round-trips.
        assert_eq!(recode("héllo", "utf-8", "windows-1252").unwrap(), mojibake);
    }

    /// Names that no coding system answers to, and text the source coding cannot
    /// hold, are reported rather than silently corrupting the region.
    #[test]
    fn recode_rejects_bad_input() {
        assert!(recode("x", "no-such-coding", "utf-8").is_err());
        assert!(recode("x", "utf-8", "no-such-coding").is_err());
        // A CJK character cannot be encoded in Latin-1, so the original bytes
        // cannot be reconstructed.
        assert!(recode("漢", "iso-8859-1", "utf-8").is_err());
    }

    /// Emacs names coding systems in ways IANA does not: `latin-1`, and an
    /// end-of-line suffix glued onto the encoding. Both must resolve, because
    /// they are what a user types at the `C-x RET` prompts.
    #[test]
    fn lookup_accepts_the_emacs_spellings() {
        assert_eq!(lookup("latin-1").unwrap(), lookup("iso-8859-1").unwrap());
        assert_eq!(lookup("utf-8-unix").unwrap().name(), "UTF-8");
        assert_eq!(
            lookup("latin-1-dos").unwrap(),
            lookup("iso-8859-1").unwrap()
        );
        assert_eq!(lookup("  UTF-8  ").unwrap().name(), "UTF-8");
        assert!(lookup("no-such-coding").is_err());
    }

    /// The decode/encode helpers must be transparent when no coding system is
    /// set — an unset coding system may not change what any existing call site
    /// does (lossy UTF-8 decode, UTF-8 encode).
    #[test]
    fn unset_coding_systems_are_transparent() {
        assert_eq!(decode_with(None, b"hello"), "hello");
        // Invalid UTF-8 is replaced, not an error — the pre-existing behaviour.
        assert_eq!(decode_with(None, &[0xff, b'a']), "\u{fffd}a");
        assert_eq!(encode_with(None, "héllo"), "héllo".as_bytes());
        // …and a set coding system really transcodes.
        let latin1 = lookup("iso-8859-1").unwrap();
        assert_eq!(encode_with(Some(latin1), "é"), vec![0xe9]);
        assert_eq!(decode_with(Some(latin1), &[0xe9]), "é");
    }

    /// The one-shot `set-next-selection-coding-system` outranks the persistent
    /// one exactly once, then the persistent one takes over again.
    #[test]
    fn next_selection_coding_is_consumed_once() {
        let latin1 = lookup("iso-8859-1").unwrap();
        let koi8 = lookup("koi8-r").unwrap();
        set_selection_coding(Some(latin1));
        set_next_selection_coding(Some(koi8));
        assert_eq!(take_selection_coding(), Some(koi8));
        assert_eq!(take_selection_coding(), Some(latin1));
        set_selection_coding(None);
        assert_eq!(take_selection_coding(), None);
    }

    #[test]
    fn language_environments_and_locales_name_real_codings() {
        assert_eq!(
            language_environment_coding("Japanese").unwrap().name(),
            "EUC-JP"
        );
        assert_eq!(
            language_environment_coding("latin-1").unwrap().name(),
            "windows-1252"
        );
        assert_eq!(
            language_environment_coding("Cyrillic-KOI8").unwrap().name(),
            "KOI8-R"
        );
        assert!(language_environment_coding("Klingon").is_none());

        assert_eq!(locale_coding("en_US.UTF-8").unwrap().name(), "UTF-8");
        assert_eq!(locale_coding("ru_RU.KOI8-R").unwrap().name(), "KOI8-R");
        assert_eq!(locale_coding("ja_JP.eucJP@euro").unwrap().name(), "EUC-JP");
        assert_eq!(
            locale_coding("en_US.ISO8859-1").unwrap().name(),
            "windows-1252"
        );
        // No codeset in the locale: nothing to set.
        assert!(locale_coding("C").is_none());
        assert!(locale_coding("en_US").is_none());
    }
}
