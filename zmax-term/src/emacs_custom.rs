//! Emacs Customize state (`cus-edit.el`) that is not the session-customization
//! store: saving the enabled theme (`custom-theme-save`) and the variant lookup
//! `theme-choose-variant` walks.
//!
//! The SET-but-unsaved set itself lives in `commands.rs` (`CUSTOMIZED_UNSAVED`,
//! reached through `custom_note_set` / `custom_unsaved_values` /
//! `custom_mark_saved`), because every live config edit already records itself
//! there; the customization buffer writes to that same store rather than keeping
//! a second one.

/// Emacs `custom-theme-save`: record the enabled theme for future sessions.
/// Emacs saves `custom-enabled-themes` into the custom file; zmax's equivalent is
/// the top-level `theme` key of `config.toml`, and every other key in the file is
/// preserved (the file is parsed, the one key replaced, and written back).
pub fn save_theme_choice(name: &str) -> std::io::Result<()> {
    let path = zmax_loader::config_dir().join("config.toml");
    let mut cfg: toml::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_else(|| toml::Value::Table(Default::default()));
    if !cfg.is_table() {
        cfg = toml::Value::Table(Default::default());
    }
    if let Some(t) = cfg.as_table_mut() {
        t.insert("theme".into(), toml::Value::String(name.to_string()));
    }
    let text = toml::to_string_pretty(&cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

// --- theme variants (`theme-choose-variant`) ---

/// The light/dark tokens a theme name uses to mark its variant.
const VARIANT_TOKENS: [&str; 2] = ["light", "dark"];

/// Split a theme name into its `-`/`_`-separated segments.
fn segments(name: &str) -> Vec<String> {
    name.split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

/// The name's segments with its first `light`/`dark` segment replaced by `%`, and
/// whether it had one. `ayu_dark` → (`["ayu", "%"]`, true); `gruvbox` →
/// (`["gruvbox"]`, false).
fn pattern(name: &str) -> (Vec<String>, bool) {
    let mut segs = segments(name);
    let pos = segs
        .iter()
        .position(|s| VARIANT_TOKENS.contains(&s.as_str()));
    match pos {
        Some(i) => {
            segs[i] = "%".to_string();
            (segs, true)
        }
        None => (segs, false),
    }
}

/// The pattern with its `%` segment dropped — the name of the variant-less base
/// theme (`ayu_%` → `ayu`), so `gruvbox` and `gruvbox_light` land in one family.
fn without_variant(pat: &[String]) -> Vec<String> {
    pat.iter().filter(|s| *s != "%").cloned().collect()
}

/// Every other installed theme that is a variant of `current` — emacs'
/// `theme-choose-variant` reads a theme's declared variants; zmax reads the
/// naming convention every shipped theme follows: the variant is a `light` or
/// `dark` segment of the name, so `ayu_dark`'s variants are `ayu_light`,
/// `github_dark_colorblind`'s is `github_light_colorblind`, and a theme with no
/// such segment (`gruvbox`) is a variant of the ones that add it (`gruvbox_light`).
/// Separator style is ignored, so `adwaita-dark` and `adwaita_light` pair up.
/// The result keeps `all`'s order.
pub fn theme_variants(current: &str, all: &[String]) -> Vec<String> {
    let (cur_pat, cur_has) = pattern(current);
    let cur_base = without_variant(&cur_pat);
    all.iter()
        .filter(|name| !name.eq_ignore_ascii_case(current))
        .filter(|name| {
            let (pat, has) = pattern(name);
            match (cur_has, has) {
                // Two variants of the same family: same pattern.
                (true, true) => pat == cur_pat,
                // A variant of a base theme, in either direction.
                (false, true) => without_variant(&pat) == cur_base,
                (true, false) => pat == cur_base,
                (false, false) => false,
            }
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        [
            "ayu_dark",
            "ayu_light",
            "ayu_mirage",
            "adwaita-dark",
            "adwaita-light",
            "github_dark",
            "github_light",
            "github_dark_colorblind",
            "github_light_colorblind",
            "gruvbox",
            "gruvbox_light",
            "base16_default_dark",
            "base16_default_light",
            "nord",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn variants_pair_up_by_the_light_dark_segment() {
        let all = names();
        assert_eq!(theme_variants("ayu_dark", &all), vec!["ayu_light"]);
        assert_eq!(theme_variants("ayu_light", &all), vec!["ayu_dark"]);
        // A qualified name keeps its qualifier: dark_colorblind ↔ light_colorblind,
        // and never pairs with the plain github_light.
        assert_eq!(
            theme_variants("github_dark_colorblind", &all),
            vec!["github_light_colorblind"]
        );
        assert_eq!(theme_variants("github_dark", &all), vec!["github_light"]);
        assert_eq!(
            theme_variants("base16_default_light", &all),
            vec!["base16_default_dark"]
        );
    }

    #[test]
    fn separator_style_does_not_split_a_family() {
        let all = names();
        assert_eq!(theme_variants("adwaita-dark", &all), vec!["adwaita-light"]);
    }

    #[test]
    fn a_base_theme_and_its_qualified_sibling_are_variants() {
        let all = names();
        assert_eq!(theme_variants("gruvbox", &all), vec!["gruvbox_light"]);
        assert_eq!(theme_variants("gruvbox_light", &all), vec!["gruvbox"]);
    }

    #[test]
    fn a_theme_with_no_sibling_has_no_variants() {
        let all = names();
        assert!(theme_variants("nord", &all).is_empty());
        assert!(
            theme_variants("ayu_mirage", &all).is_empty(),
            "mirage is not a light/dark segment, so it names its own family"
        );
    }
}
