//! Facemenu — the pure face/color substrate for the zmax port of GNU Emacs
//! `facemenu`, `list-faces-display` and `list-colors-display`.
//!
//! Emacs keeps a table of named *colors* (the X11 / `list-colors-display` set)
//! and a table of named *faces* (the `list-faces-display` set that `facemenu`
//! lets you apply to a region). Both are static, data-only tables with no I/O,
//! so they live here — filesystem-free and unit-tested — while the interactive
//! browser overlay (`zmax_term::ui::facemenu`) renders them and handles keys.
//!
//! A face picked here is applied to the buffer as a face text property (see
//! [`crate::text_props`]) and rendered from it. The attribute faces (`bold`,
//! `italic`, `underline`, `bold-italic`, `default`) map straight onto the
//! attribute toggles; the rest name a *theme scope*, which [`theme_scope`]
//! resolves so `font-lock-keyword-face` picks up whatever the active zmax theme
//! paints keywords with.

/// A named color, as in Emacs' `list-colors-display` (X11 `rgb.txt` names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedColor {
    /// The lower-case color name (e.g. `"orange"`).
    pub name: &'static str,
    /// Its 24-bit RGB triple.
    pub rgb: (u8, u8, u8),
}

/// A named face, as listed by Emacs' `list-faces-display`. `attrs` is a short,
/// human-readable summary of what the face does (there is no real font engine to
/// apply here).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    /// The face name (e.g. `"font-lock-keyword-face"`).
    pub name: &'static str,
    /// A one-line description of the face's visual attributes.
    pub attrs: &'static str,
}

/// The standard palette — ~48 X11 / Emacs color names with their RGB values.
/// Names are unique; RGB values may repeat (e.g. `cyan` and `aqua`).
pub fn colors() -> &'static [NamedColor] {
    const fn c(name: &'static str, r: u8, g: u8, b: u8) -> NamedColor {
        NamedColor {
            name,
            rgb: (r, g, b),
        }
    }
    const TABLE: &[NamedColor] = &[
        c("black", 0, 0, 0),
        c("white", 255, 255, 255),
        c("red", 255, 0, 0),
        c("green", 0, 128, 0),
        c("blue", 0, 0, 255),
        c("yellow", 255, 255, 0),
        c("cyan", 0, 255, 255),
        c("magenta", 255, 0, 255),
        c("gray", 128, 128, 128),
        c("orange", 255, 165, 0),
        c("pink", 255, 192, 203),
        c("purple", 128, 0, 128),
        c("brown", 165, 42, 42),
        c("navy", 0, 0, 128),
        c("teal", 0, 128, 128),
        c("olive", 128, 128, 0),
        c("maroon", 128, 0, 0),
        c("lime", 0, 255, 0),
        c("aqua", 0, 255, 255),
        c("silver", 192, 192, 192),
        c("gold", 255, 215, 0),
        c("darkgray", 169, 169, 169),
        c("lightgray", 211, 211, 211),
        c("darkred", 139, 0, 0),
        c("darkgreen", 0, 100, 0),
        c("darkblue", 0, 0, 139),
        c("lightblue", 173, 216, 230),
        c("lightgreen", 144, 238, 144),
        c("salmon", 250, 128, 114),
        c("coral", 255, 127, 80),
        c("tomato", 255, 99, 71),
        c("khaki", 240, 230, 140),
        c("violet", 238, 130, 238),
        c("indigo", 75, 0, 130),
        c("turquoise", 64, 224, 208),
        c("tan", 210, 180, 140),
        c("beige", 245, 245, 220),
        c("ivory", 255, 255, 240),
        c("crimson", 220, 20, 60),
        c("chocolate", 210, 105, 30),
        c("plum", 221, 160, 221),
        c("orchid", 218, 112, 214),
        c("skyblue", 135, 206, 235),
        c("steelblue", 70, 130, 180),
        c("forestgreen", 34, 139, 34),
        c("seagreen", 46, 139, 87),
        c("slategray", 112, 128, 144),
        c("wheat", 245, 222, 179),
    ];
    TABLE
}

/// Look up a color by name, case-insensitively. Returns its RGB triple, or
/// `None` if unknown — the engine behind `facemenu-set-foreground/background`.
pub fn find_color(name: &str) -> Option<(u8, u8, u8)> {
    colors()
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name))
        .map(|c| c.rgb)
}

/// Format an RGB triple as a zero-padded `#rrggbb` hex string.
pub fn hex(rgb: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

/// The standard faces, as listed by `list-faces-display`.
pub fn faces() -> &'static [Face] {
    const fn f(name: &'static str, attrs: &'static str) -> Face {
        Face { name, attrs }
    }
    const TABLE: &[Face] = &[
        f("default", "the default face"),
        f("bold", "bold weight"),
        f("italic", "italic slant"),
        f("underline", "underlined text"),
        f("bold-italic", "bold weight + italic slant"),
        f("highlight", "highlighted (mouse-over) background"),
        f("region", "the active region background"),
        f("secondary-selection", "the secondary selection background"),
        f("shadow", "dimmed / de-emphasised text"),
        f("link", "a clickable hyperlink"),
        f("link-visited", "a followed hyperlink"),
        f("error", "an error message"),
        f("warning", "a warning message"),
        f("success", "a success message"),
        f("font-lock-keyword-face", "language keywords"),
        f("font-lock-string-face", "string literals"),
        f("font-lock-comment-face", "comments"),
        f("font-lock-function-name-face", "function names"),
        f("font-lock-variable-name-face", "variable names"),
        f("font-lock-type-face", "type names"),
        f("font-lock-constant-face", "constants"),
        f("font-lock-builtin-face", "builtins"),
        f("minibuffer-prompt", "the minibuffer prompt"),
        f("mode-line", "the active mode line"),
        f("cursor", "the text cursor"),
        f("fringe", "the window fringe"),
    ];
    TABLE
}

/// The zmax theme scope that paints an Emacs face name, for the faces whose
/// look comes from the theme rather than from a plain attribute toggle.
///
/// `facemenu-set-face font-lock-string-face` has to produce the colour the
/// *current* theme uses for strings, so the face text property stores the Emacs
/// name and the renderer resolves it through this table against the live theme.
/// The five attribute faces (`bold`, `italic`, `underline`, `bold-italic`,
/// `default`) are absent on purpose: they are attribute toggles, not scopes.
pub fn theme_scope(face: &str) -> Option<&'static str> {
    Some(match face {
        "highlight" => "ui.highlight",
        "region" => "ui.selection",
        "secondary-selection" => "ui.selection",
        "shadow" => "comment",
        "link" => "markup.link.url",
        "link-visited" => "markup.link.text",
        "error" => "error",
        "warning" => "warning",
        "success" => "diagnostic.hint",
        "font-lock-keyword-face" => "keyword",
        "font-lock-string-face" => "string",
        "font-lock-comment-face" => "comment",
        "font-lock-function-name-face" => "function",
        "font-lock-variable-name-face" => "variable",
        "font-lock-type-face" => "type",
        "font-lock-constant-face" => "constant",
        "font-lock-builtin-face" => "function.builtin",
        "minibuffer-prompt" => "ui.text.focus",
        "mode-line" => "ui.statusline",
        "cursor" => "ui.cursor",
        "fringe" => "ui.gutter",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_non_attribute_face_resolves_to_a_theme_scope() {
        // The five attribute faces are toggles, not scopes; every other face in
        // `list-faces-display` must be paintable by the theme or the face menu
        // would silently do nothing when it is chosen.
        const ATTRS: [&str; 5] = ["default", "bold", "italic", "underline", "bold-italic"];
        for face in faces() {
            if ATTRS.contains(&face.name) {
                assert!(theme_scope(face.name).is_none(), "{}", face.name);
            } else {
                assert!(
                    theme_scope(face.name).is_some(),
                    "{} has no theme scope",
                    face.name
                );
            }
        }
    }

    #[test]
    fn theme_scope_rejects_unknown_faces() {
        assert_eq!(theme_scope("no-such-face"), None);
    }

    #[test]
    fn colors_non_empty_and_names_unique() {
        let cs = colors();
        assert!(
            cs.len() >= 40,
            "expected the full X11 palette, got {}",
            cs.len()
        );
        let names: HashSet<&str> = cs.iter().map(|c| c.name).collect();
        assert_eq!(names.len(), cs.len(), "color names must be unique");
    }

    #[test]
    fn find_color_is_case_insensitive_hit() {
        assert_eq!(find_color("red"), Some((255, 0, 0)));
        assert_eq!(find_color("RED"), Some((255, 0, 0)));
        assert_eq!(find_color("ReD"), Some((255, 0, 0)));
    }

    #[test]
    fn find_color_miss_is_none() {
        assert_eq!(find_color("chartreusey-nonsense"), None);
        assert_eq!(find_color(""), None);
    }

    #[test]
    fn hex_formats_a_triple() {
        assert_eq!(hex((255, 0, 0)), "#ff0000");
        assert_eq!(hex((0, 255, 0)), "#00ff00");
        assert_eq!(hex((0, 0, 255)), "#0000ff");
    }

    #[test]
    fn hex_zero_pads_each_channel() {
        assert_eq!(hex((0, 0, 0)), "#000000");
        assert_eq!(hex((1, 2, 3)), "#010203");
        assert_eq!(hex((255, 255, 255)), "#ffffff");
    }

    #[test]
    fn faces_contains_default_and_bold() {
        let names: HashSet<&str> = faces().iter().map(|f| f.name).collect();
        assert!(names.contains("default"));
        assert!(names.contains("bold"));
        assert!(names.contains("font-lock-keyword-face"));
    }

    #[test]
    fn faces_names_unique_and_have_attrs() {
        let fs = faces();
        let names: HashSet<&str> = fs.iter().map(|f| f.name).collect();
        assert_eq!(names.len(), fs.len(), "face names must be unique");
        assert!(fs.iter().all(|f| !f.attrs.is_empty()));
    }

    #[test]
    fn known_color_rgb_values() {
        assert_eq!(find_color("black"), Some((0, 0, 0)));
        assert_eq!(find_color("white"), Some((255, 255, 255)));
        assert_eq!(find_color("orange"), Some((255, 165, 0)));
    }

    #[test]
    fn find_color_round_trips_through_hex() {
        let rgb = find_color("gold").unwrap();
        assert_eq!(hex(rgb), "#ffd700");
    }
}
