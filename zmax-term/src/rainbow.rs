//! The spacemacs `+themes/colors` layer: `rainbow-mode`, `rainbow-identifiers`
//! and `color-identifiers-mode`.
//!
//! Three independent viewport scanners, each producing overlay highlights that
//! [`crate::ui::editor::EditorView`] pushes above syntax highlighting:
//!
//! * `rainbow-mode` paints every colour *literal* (`#fff`, `#ff8800`,
//!   `rgb(1,2,3)`, `hsl(120,50%,50%)`, `rebeccapurple`) with the colour it
//!   names, exactly like the emacs mode of the same name.
//! * `rainbow-identifiers-mode` gives every identifier a colour derived from its
//!   own text, so the same name is always the same colour. This ports
//!   rainbow-identifiers.el's `cie-l*a*b*` face chooser: the identifier is
//!   hashed, the hash picks an angle on a circle in the CIE L\*a\*b\* plane whose
//!   radius is `saturation` and whose plane is at `lightness`, and that point is
//!   converted to sRGB. Upstream hashes with SHA-1; zmax uses SHA-2 (already a
//!   dependency), which changes *which* colour a given name gets but not the
//!   property that makes the mode useful — a stable, well-spread colour per name.
//! * `color-identifiers-mode` is the same colouring restricted to identifiers the
//!   tree-sitter grammar actually calls identifiers/variables, i.e. "only the
//!   things that are variables" rather than every word in the buffer.
//!
//! Both identifier modes are per-buffer with a global variant, matching the
//! layer's `SPC t C a` / `SPC t C C-a` pair, and share the saturation/lightness
//! knobs that the layer's `SPC C i s` / `SPC C i l` transient states drive.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use zmax_view::DocumentId;

/// Which identifiers a buffer colours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdentMode {
    /// rainbow-identifiers-mode: every identifier-shaped word.
    All,
    /// color-identifiers-mode: only tree-sitter identifier/variable nodes.
    Variables,
}

/// Buffers with `rainbow-mode` on (colour literals painted).
static RAINBOW_DOCS: Mutex<Option<HashSet<DocumentId>>> = Mutex::new(None);
/// Buffers with an identifier-colouring mode on, and which one.
static IDENT_DOCS: Mutex<Option<Vec<(DocumentId, IdentMode)>>> = Mutex::new(None);
/// `rainbow-identifiers` / `color-identifiers` turned on globally (the
/// `SPC t C C-a` / `SPC t C C-v` variants), applying to every buffer.
static IDENT_GLOBAL: Mutex<Option<IdentMode>> = Mutex::new(None);
/// Whether *any* identifier mode is on, so the render path can bail in one load.
static IDENT_ANY: AtomicBool = AtomicBool::new(false);

/// `rainbow-identifiers-cie-l*a*b*-lightness`, upstream default 50.
static LIGHTNESS: AtomicU32 = AtomicU32::new(50);
/// `rainbow-identifiers-cie-l*a*b*-saturation`, upstream default 15.
static SATURATION: AtomicU32 = AtomicU32::new(15);
/// `rainbow-identifiers-cie-l*a*b*-color-count`, upstream default 65536.
const COLOR_COUNT: u64 = 65536;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/* ── rainbow-mode (colour literals) ─────────────────────────────────────── */

/// Toggle `rainbow-mode` for one buffer; returns the new state.
pub fn toggle_rainbow(doc: DocumentId) -> bool {
    let mut guard = lock(&RAINBOW_DOCS);
    let set = guard.get_or_insert_with(HashSet::new);
    if set.remove(&doc) {
        false
    } else {
        set.insert(doc);
        true
    }
}

/// Whether `rainbow-mode` is on for `doc`.
pub fn rainbow_enabled(doc: DocumentId) -> bool {
    lock(&RAINBOW_DOCS)
        .as_ref()
        .is_some_and(|s| s.contains(&doc))
}

/* ── identifier colouring ───────────────────────────────────────────────── */

/// Toggle an identifier-colouring mode for one buffer. Turning one on replaces
/// the other (emacs' two modes both own the same overlays). Returns the state.
pub fn toggle_identifiers(doc: DocumentId, mode: IdentMode) -> bool {
    let mut guard = lock(&IDENT_DOCS);
    let list = guard.get_or_insert_with(Vec::new);
    let on = match list.iter().position(|(d, m)| *d == doc && *m == mode) {
        Some(idx) => {
            list.remove(idx);
            false
        }
        None => {
            list.retain(|(d, _)| *d != doc);
            list.push((doc, mode));
            true
        }
    };
    let any = !list.is_empty() || lock(&IDENT_GLOBAL).is_some();
    IDENT_ANY.store(any, Ordering::Relaxed);
    on
}

/// Toggle an identifier-colouring mode globally (the `C-a` / `C-v` variants).
pub fn toggle_identifiers_global(mode: IdentMode) -> bool {
    let mut guard = lock(&IDENT_GLOBAL);
    let on = if *guard == Some(mode) {
        *guard = None;
        false
    } else {
        *guard = Some(mode);
        true
    };
    drop(guard);
    let any = on
        || lock(&IDENT_DOCS)
            .as_ref()
            .is_some_and(|list| !list.is_empty());
    IDENT_ANY.store(any, Ordering::Relaxed);
    on
}

/// The identifier mode in force for `doc` (buffer-local beats global).
pub fn ident_mode(doc: DocumentId) -> Option<IdentMode> {
    if !IDENT_ANY.load(Ordering::Relaxed) {
        return None;
    }
    let local = lock(&IDENT_DOCS)
        .as_ref()
        .and_then(|l| l.iter().find(|(d, _)| *d == doc).map(|(_, m)| *m));
    local.or(*lock(&IDENT_GLOBAL))
}

/// Current `(lightness, saturation)`.
pub fn lab_params() -> (f64, f64) {
    (
        LIGHTNESS.load(Ordering::Relaxed) as f64,
        SATURATION.load(Ordering::Relaxed) as f64,
    )
}

/// Adjust saturation the way the layer's transient state does: `+1`, `-1` or a
/// reset to the upstream default. Returns the new value.
pub fn adjust_saturation(delta: i32, reset: bool) -> u32 {
    adjust(&SATURATION, delta, reset, 15)
}

/// Adjust lightness (same contract as [`adjust_saturation`]).
pub fn adjust_lightness(delta: i32, reset: bool) -> u32 {
    adjust(&LIGHTNESS, delta, reset, 50)
}

fn adjust(cell: &AtomicU32, delta: i32, reset: bool, default: u32) -> u32 {
    let next = if reset {
        default
    } else {
        (cell.load(Ordering::Relaxed) as i32 + delta).clamp(0, 100) as u32
    };
    cell.store(next, Ordering::Relaxed);
    next
}

/// rainbow-identifiers' `cie-l*a*b*` face chooser: hash the name, read the hash
/// as an angle on the colour circle at the configured lightness/saturation, and
/// convert that L\*a\*b\* point to sRGB.
pub fn identifier_color(name: &str) -> (u8, u8, u8) {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(name.as_bytes());
    // Upstream reads the hash's trailing bytes; do the same with the last 8.
    let mut hash: u64 = 0;
    for byte in &digest[digest.len() - 8..] {
        hash = (hash << 8) | *byte as u64;
    }
    let (lightness, saturation) = lab_params();
    let angle = 2.0 * std::f64::consts::PI * (hash % COLOR_COUNT) as f64 / COLOR_COUNT as f64;
    lab_to_rgb(lightness, saturation * angle.cos(), saturation * angle.sin())
}

/// CIE L\*a\*b\* (D65) to 8-bit sRGB, clamped — rainbow-identifiers'
/// `rainbow-identifiers--cie-l*a*b*-to-rgb`.
fn lab_to_rgb(l: f64, a: f64, b: f64) -> (u8, u8, u8) {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    // Inverse of the L*a*b* companding function.
    let finv = |t: f64| {
        if t > 6.0 / 29.0 {
            t * t * t
        } else {
            3.0 * (6.0 / 29.0) * (6.0 / 29.0) * (t - 4.0 / 29.0)
        }
    };
    // D65 white point.
    let (x, y, z) = (
        0.950_47 * finv(fx),
        1.0 * finv(fy),
        1.088_83 * finv(fz),
    );
    let linear = [
        3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z,
        -0.969_266_0 * x + 1.876_010_8 * y + 0.041_556_0 * z,
        0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z,
    ];
    let gamma = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    (gamma(linear[0]), gamma(linear[1]), gamma(linear[2]))
}

/* ── colour literals ────────────────────────────────────────────────────── */

/// A colour literal found in a buffer: byte range within the scanned text plus
/// the colour it names.
pub struct ColorLiteral {
    pub start: usize,
    pub end: usize,
    pub rgb: (u8, u8, u8),
}

/// Find every colour literal in `text` (rainbow-mode's keyword set: hex triples
/// of 3/6/12 digits, functional `rgb()`/`rgba()`/`hsl()`/`hsla()` notation, and
/// CSS/X11 colour names). Offsets are character indices into `text`.
pub fn color_literals(text: &str) -> Vec<ColorLiteral> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '#' {
            if let Some((len, rgb)) = hex_literal(&chars[i + 1..]) {
                out.push(ColorLiteral {
                    start: i,
                    end: i + 1 + len,
                    rgb,
                });
                i += 1 + len;
                continue;
            }
        }
        if c.is_ascii_alphabetic() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let lower = word.to_ascii_lowercase();
            if matches!(lower.as_str(), "rgb" | "rgba" | "hsl" | "hsla") {
                if let Some((len, rgb)) = functional_literal(&lower, &chars[j..]) {
                    out.push(ColorLiteral {
                        start: i,
                        end: j + len,
                        rgb,
                    });
                    i = j + len;
                    continue;
                }
            }
            // A bare word is a colour only when the whole word is a colour name.
            if let Some(rgb) = named_color(&lower) {
                out.push(ColorLiteral {
                    start: i,
                    end: j,
                    rgb,
                });
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// `#rgb`, `#rrggbb`, `#rrrgggbbb`, `#rrrrggggbbbb` — the four widths emacs'
/// rainbow-mode accepts. Returns the digit count consumed and the colour.
fn hex_literal(rest: &[char]) -> Option<(usize, (u8, u8, u8))> {
    let digits = rest
        .iter()
        .take_while(|c| c.is_ascii_hexdigit())
        .count()
        .min(12);
    // A longer run of hex digits than any accepted width is not a colour.
    let follows_more = rest.get(digits).is_some_and(|c| c.is_ascii_hexdigit());
    let width = match digits {
        12 => 4,
        9 => 3,
        6 => 2,
        3 => 1,
        _ => return None,
    };
    if follows_more {
        return None;
    }
    let val = |k: usize| -> Option<u8> {
        let s: String = rest[k * width..(k + 1) * width].iter().collect();
        let v = u32::from_str_radix(&s, 16).ok()?;
        // Scale the component down to 8 bits (emacs keeps 16-bit components).
        let max = (1u32 << (4 * width as u32)) - 1;
        Some(((v * 255 + max / 2) / max) as u8)
    };
    Some((digits, (val(0)?, val(1)?, val(2)?)))
}

/// `rgb(1,2,3)` / `rgba(1,2,3,.5)` / `hsl(120,50%,50%)` / `hsla(…)` starting at
/// the `(`. Returns the characters consumed from the `(` and the colour.
fn functional_literal(kind: &str, rest: &[char]) -> Option<(usize, (u8, u8, u8))> {
    if rest.first() != Some(&'(') {
        return None;
    }
    let close = rest.iter().position(|c| *c == ')')?;
    let inner: String = rest[1..close].iter().collect();
    let parts: Vec<&str> = inner
        .split([',', '/'])
        .flat_map(|p| p.split_whitespace())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let num = |s: &str| -> Option<f64> { s.trim().trim_end_matches(['%', 'd', 'e', 'g']).parse().ok() };
    let rgb = if kind.starts_with("rgb") {
        let comp = |s: &str| -> Option<u8> {
            let v = num(s)?;
            Some(if s.trim().ends_with('%') {
                (v / 100.0 * 255.0).clamp(0.0, 255.0) as u8
            } else {
                v.clamp(0.0, 255.0) as u8
            })
        };
        (comp(parts[0])?, comp(parts[1])?, comp(parts[2])?)
    } else {
        hsl_to_rgb(
            num(parts[0])?,
            num(parts[1])? / 100.0,
            num(parts[2])? / 100.0,
        )
    };
    Some((close + 1, rgb))
}

/// CSS `hsl()` to sRGB.
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let h = ((h % 360.0) + 360.0) % 360.0 / 60.0;
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to8 = |v: f64| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    (to8(r), to8(g), to8(b))
}

/// A readable foreground for text painted on `rgb` — rainbow-mode picks black or
/// white by the background's luminance so the literal stays legible.
pub fn contrast_fg(rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let (r, g, b) = (rgb.0 as f64, rgb.1 as f64, rgb.2 as f64);
    let luma = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
    if luma > 0.55 {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    }
}

/// The CSS Color Module Level 4 named colours (the same list emacs' rainbow-mode
/// reads out of `color-name-rgb-alist` / its html-colors keyword).
#[rustfmt::skip]
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff), ("antiquewhite", 0xfaebd7), ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4), ("azure", 0xf0ffff), ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4), ("black", 0x000000), ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff), ("blueviolet", 0x8a2be2), ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887), ("cadetblue", 0x5f9ea0), ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e), ("coral", 0xff7f50), ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc), ("crimson", 0xdc143c), ("cyan", 0x00ffff),
    ("darkblue", 0x00008b), ("darkcyan", 0x008b8b), ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9), ("darkgreen", 0x006400), ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b), ("darkmagenta", 0x8b008b), ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00), ("darkorchid", 0x9932cc), ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a), ("darkseagreen", 0x8fbc8f), ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f), ("darkslategrey", 0x2f4f4f), ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3), ("deeppink", 0xff1493), ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969), ("dimgrey", 0x696969), ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222), ("floralwhite", 0xfffaf0), ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff), ("gainsboro", 0xdcdcdc), ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700), ("goldenrod", 0xdaa520), ("gray", 0x808080),
    ("green", 0x008000), ("greenyellow", 0xadff2f), ("grey", 0x808080),
    ("honeydew", 0xf0fff0), ("hotpink", 0xff69b4), ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082), ("ivory", 0xfffff0), ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa), ("lavenderblush", 0xfff0f5), ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd), ("lightblue", 0xadd8e6), ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff), ("lightgoldenrodyellow", 0xfafad2), ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90), ("lightgrey", 0xd3d3d3), ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a), ("lightseagreen", 0x20b2aa), ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899), ("lightslategrey", 0x778899), ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0), ("lime", 0x00ff00), ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6), ("magenta", 0xff00ff), ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa), ("mediumblue", 0x0000cd), ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db), ("mediumseagreen", 0x3cb371), ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a), ("mediumturquoise", 0x48d1cc), ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970), ("mintcream", 0xf5fffa), ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5), ("navajowhite", 0xffdead), ("navy", 0x000080),
    ("oldlace", 0xfdf5e6), ("olive", 0x808000), ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500), ("orangered", 0xff4500), ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa), ("palegreen", 0x98fb98), ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093), ("papayawhip", 0xffefd5), ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f), ("pink", 0xffc0cb), ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6), ("purple", 0x800080), ("rebeccapurple", 0x663399),
    ("red", 0xff0000), ("rosybrown", 0xbc8f8f), ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513), ("salmon", 0xfa8072), ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57), ("seashell", 0xfff5ee), ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0), ("skyblue", 0x87ceeb), ("slateblue", 0x6a5acd),
    ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f), ("steelblue", 0x4682b4), ("tan", 0xd2b48c),
    ("teal", 0x008080), ("thistle", 0xd8bfd8), ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0), ("violet", 0xee82ee), ("wheat", 0xf5deb3),
    ("white", 0xffffff), ("whitesmoke", 0xf5f5f5), ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

/// Look up a CSS/X11 colour name (case-insensitive; `lower` must already be
/// lowercased).
fn named_color(lower: &str) -> Option<(u8, u8, u8)> {
    NAMED_COLORS
        .iter()
        .find(|(n, _)| *n == lower)
        .map(|(_, v)| (((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8))
}

/// Identifier-shaped words in `text`, as `(start, end, text)` character ranges.
/// A word starts with a letter or `_` and continues with letters, digits and
/// `_`; a leading digit makes it a number, not an identifier.
pub fn identifiers(text: &str) -> Vec<(usize, usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push((start, i, chars[start..i].iter().collect()));
        } else {
            // Skip a whole number so `0xdeadbeef` is not read as an identifier.
            while i < chars.len() && chars[i].is_alphanumeric() {
                i += 1;
            }
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_literals_of_every_accepted_width() {
        let lits = color_literals("#fff #ff8800 #ffff88880000");
        assert_eq!(lits.len(), 3);
        assert_eq!(lits[0].rgb, (255, 255, 255));
        assert_eq!(lits[1].rgb, (255, 136, 0));
        assert_eq!(lits[2].rgb, (255, 136, 0));
    }

    #[test]
    fn a_seven_digit_hex_run_is_not_a_colour() {
        assert!(color_literals("#1234567").is_empty());
    }

    #[test]
    fn functional_and_named_notation() {
        let lits = color_literals("rgb(255, 0, 0) hsl(120, 100%, 50%) rebeccapurple");
        assert_eq!(lits.len(), 3);
        assert_eq!(lits[0].rgb, (255, 0, 0));
        assert_eq!(lits[1].rgb, (0, 255, 0));
        assert_eq!(lits[2].rgb, (0x66, 0x33, 0x99));
    }

    #[test]
    fn identifier_colour_is_stable_and_name_dependent() {
        assert_eq!(identifier_color("counter"), identifier_color("counter"));
        assert_ne!(identifier_color("counter"), identifier_color("total"));
    }

    #[test]
    fn saturation_and_lightness_clamp_and_reset() {
        assert_eq!(adjust_saturation(0, true), 15);
        assert_eq!(adjust_saturation(5, false), 20);
        assert_eq!(adjust_saturation(-500, false), 0);
        assert_eq!(adjust_saturation(0, true), 15);
        assert_eq!(adjust_lightness(0, true), 50);
    }

    #[test]
    fn identifiers_skip_numbers_and_hex_escapes() {
        let ids = identifiers("let x1 = 0xdead + total;");
        let names: Vec<&str> = ids.iter().map(|(_, _, s)| s.as_str()).collect();
        assert_eq!(names, vec!["let", "x1", "total"]);
    }
}
