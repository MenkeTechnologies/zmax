//! Translate a Vim colour scheme into a live zmax [`Theme`].
//!
//! When a sourced `.vimrc`/`init.vim` runs `:colorscheme molokai` (or a bare
//! `:highlight` command), vimlrs sources the scheme's `colors/*.vim` and records
//! every `:highlight` group in its per-thread registry. We read that registry
//! back through [`vimlrs::fusevm_bridge`] and map the Vim highlight groups onto
//! zmax's (helix-derived) theme scopes, producing a `Theme` we apply live.
//!
//! Only groups the scheme actually defined are overlaid; everything else keeps
//! the base theme's value, so partial Vim schemes still yield a coherent editor.

use std::collections::BTreeMap;

use toml::Value;
use zmax_view::theme::{Theme, DEFAULT_THEME_DATA};

/// Which Vim colour channel of a highlight group feeds a given zmax scope.
#[derive(Clone, Copy)]
enum Chan {
    /// Take the group's foreground (+ its display attributes).
    Fg,
    /// Take the group's background.
    Bg,
    /// Take foreground, background, and attributes (status line, menus, …).
    FgBg,
}

/// The zmax scopes a Vim highlight group paints, and from which channel.
/// Group names are matched lowercase. Returns `&[]` for unmapped groups.
fn scope_targets(group: &str) -> &'static [(&'static str, Chan)] {
    use Chan::{Bg, Fg, FgBg};
    match group {
        // ── Core text surface ────────────────────────────────────────────
        "normal" => &[("ui.text", Fg), ("ui.background", Bg)],
        "linenr" | "linenrabove" | "linenrbelow" => {
            &[("ui.linenr", Fg), ("ui.linenr.selected", Fg)]
        }
        "cursorlinenr" => &[("ui.linenr.selected", FgBg)],
        "cursorline" => &[("ui.cursorline.primary", Bg)],
        "cursorcolumn" | "colorcolumn" => &[("ui.virtual.ruler", Bg)],
        "cursor" | "termcursor" => &[("ui.cursor", FgBg), ("ui.cursor.primary", FgBg)],
        "visual" => &[("ui.selection", Bg), ("ui.selection.primary", Bg)],
        "search" | "incsearch" | "curswant" => &[("ui.cursor.match", FgBg)],
        "matchparen" => &[("ui.cursor.match", FgBg)],
        "nontext" | "specialkey" | "whitespace" => &[("ui.virtual.whitespace", Fg)],
        "folded" | "foldcolumn" => &[("ui.virtual", Fg)],
        "signcolumn" => &[("ui.gutter", Bg)],

        // ── Chrome (status line, menus, splits, tabs) ────────────────────
        "statusline" => &[("ui.statusline", FgBg)],
        "statuslinenc" => &[("ui.statusline.inactive", FgBg)],
        "vertsplit" | "winseparator" => &[("ui.window", Fg)],
        "pmenu" => &[("ui.menu", FgBg), ("ui.popup", FgBg)],
        "pmenusel" => &[("ui.menu.selected", FgBg)],
        "pmenusbar" => &[("ui.menu.scroll", Bg)],
        "wildmenu" => &[("ui.menu.selected", FgBg)],
        "tabline" => &[("ui.bufferline.background", FgBg)],
        "tablinesel" => &[("ui.bufferline.active", FgBg)],
        "title" => &[("markup.heading", Fg)],
        "directory" => &[("ui.text.directory", Fg)],
        "question" | "moremsg" | "modemsg" => &[("ui.text.info", Fg)],
        "errormsg" => &[("error", Fg), ("diagnostic.error", Fg)],
        "warningmsg" => &[("warning", Fg), ("diagnostic.warning", Fg)],

        // ── Syntax groups → tree-sitter scopes ───────────────────────────
        "comment" | "specialcomment" => &[("comment", Fg)],
        "todo" => &[("comment.todo", Fg)],
        "constant" => &[("constant", Fg)],
        "string" => &[("string", Fg)],
        "character" => &[("constant.character", Fg)],
        "specialchar" => &[("constant.character.escape", Fg)],
        "number" | "float" => &[("constant.numeric", Fg)],
        "boolean" => &[("constant.builtin.boolean", Fg)],
        "identifier" => &[("variable", Fg)],
        "function" => &[("function", Fg)],
        "statement" | "keyword" => &[("keyword", Fg)],
        "conditional" | "repeat" | "label" | "exception" => &[("keyword.control", Fg)],
        "operator" => &[("operator", Fg)],
        "preproc" | "include" | "define" | "macro" | "precondit" => &[("keyword.directive", Fg)],
        "type" | "storageclass" | "structure" | "typedef" => &[("type", Fg)],
        "special" | "delimiter" | "tag" => &[("punctuation", Fg)],
        "underlined" => &[("markup.link.url", Fg)],
        "error" => &[("error", Fg)],

        // ── Diff / diagnostics ───────────────────────────────────────────
        "diffadd" => &[("diff.plus", FgBg)],
        "diffchange" => &[("diff.delta", FgBg)],
        "diffdelete" => &[("diff.minus", FgBg)],
        "difftext" => &[("diff.delta", FgBg)],
        "spellbad" => &[("diagnostic.error", Fg)],
        "spellcap" | "spellrare" | "spelllocal" => &[("diagnostic.warning", Fg)],
        _ => &[],
    }
}

/// Accumulated style for one zmax scope while merging Vim groups.
#[derive(Default)]
struct ScopeStyle {
    fg: Option<String>,
    bg: Option<String>,
    mods: Vec<String>,
}

impl ScopeStyle {
    /// Encode as the TOML a helix theme expects (`{ fg, bg, modifiers }`, or a
    /// bare colour string when only a foreground is set).
    fn into_value(self) -> Option<Value> {
        let has_bg = self.bg.is_some();
        let has_mods = !self.mods.is_empty();
        match (&self.fg, has_bg, has_mods) {
            (None, false, false) => None,
            (Some(fg), false, false) => Some(Value::String(fg.clone())),
            _ => {
                let mut t = toml::map::Map::new();
                if let Some(fg) = self.fg {
                    t.insert("fg".into(), Value::String(fg));
                }
                if let Some(bg) = self.bg {
                    t.insert("bg".into(), Value::String(bg));
                }
                if has_mods {
                    let mut seen = std::collections::HashSet::new();
                    let mods: Vec<Value> = self
                        .mods
                        .into_iter()
                        .filter(|m| seen.insert(m.clone()))
                        .map(Value::String)
                        .collect();
                    t.insert("modifiers".into(), Value::Array(mods));
                }
                Some(Value::Table(t))
            }
        }
    }
}

/// Map a Vim colour token (`#rrggbb`, a cterm number, or a GUI colour name) to a
/// value helix's palette parser accepts (`#rrggbb` or a `0`–`255` index string).
/// `None` when the token is unusable (`NONE`, `fg`, `bg`, empty, unknown name).
fn colour_token(gui: Option<&str>, cterm: Option<&str>) -> Option<String> {
    if let Some(g) = gui {
        let g = g.trim();
        if let Some(hex) = normalize_hex(g) {
            return Some(hex);
        }
        if let Some(hex) = gui_color_name(&g.to_ascii_lowercase()) {
            return Some(hex.to_string());
        }
    }
    if let Some(c) = cterm {
        let c = c.trim();
        // A cterm colour is a 0–255 index (helix → Color::Indexed) or a name.
        if c.parse::<u8>().is_ok() {
            return Some(c.to_string());
        }
        if let Some(hex) = gui_color_name(&c.to_ascii_lowercase()) {
            return Some(hex.to_string());
        }
    }
    None
}

/// Normalise `#rgb`/`#rrggbb` to `#rrggbb`; `None` if not a hex colour.
fn normalize_hex(s: &str) -> Option<String> {
    let hex = s.strip_prefix('#')?;
    let ok = |h: &str| h.chars().all(|c| c.is_ascii_hexdigit());
    match hex.len() {
        6 if ok(hex) => Some(format!("#{}", hex.to_ascii_lowercase())),
        3 if ok(hex) => {
            let mut out = String::from("#");
            for c in hex.chars() {
                out.push(c);
                out.push(c);
            }
            Some(out.to_ascii_lowercase())
        }
        _ => None,
    }
}

/// The Vim GUI colour names that appear in colorschemes, as `#rrggbb`. Covers the
/// standard 16 plus the common `dark*`/`light*` variants Vim recognises.
fn gui_color_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "black" => "#000000",
        "white" => "#ffffff",
        "red" => "#ff0000",
        "darkred" => "#8b0000",
        "green" => "#00ff00",
        "darkgreen" => "#006400",
        "blue" => "#0000ff",
        "darkblue" => "#00008b",
        "cyan" => "#00ffff",
        "darkcyan" => "#008b8b",
        "magenta" => "#ff00ff",
        "darkmagenta" => "#8b008b",
        "yellow" => "#ffff00",
        "darkyellow" | "brown" => "#a52a2a",
        "gray" | "grey" | "lightgrey" | "lightgray" => "#bebebe",
        "darkgray" | "darkgrey" => "#a9a9a9",
        "seagreen" => "#2e8b57",
        "orange" => "#ffa500",
        "purple" => "#a020f0",
        "violet" => "#ee82ee",
        "slateblue" => "#6a5acd",
        "gold" => "#ffd700",
        "pink" => "#ffc0cb",
        _ => return None,
    })
}

/// Translate Vim display attributes (`bold`, `italic`, `undercurl`, …) to helix
/// modifier strings (`bold`, `italic`, `underlined`, `reversed`, `crossed_out`).
fn map_attrs(attrs: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for a in attrs {
        let m = match a.as_str() {
            "bold" => "bold",
            "italic" => "italic",
            "underline" | "undercurl" | "underdouble" | "underdotted" | "underdashed" => {
                "underlined"
            }
            "reverse" | "inverse" | "standout" => "reversed",
            "strikethrough" => "crossed_out",
            _ => continue,
        };
        out.push(m.to_string());
    }
    out
}

/// Build a live [`Theme`] from the highlight groups vimlrs currently has defined
/// (populated by a sourced colorscheme/vimrc). `name` is the active scheme name
/// (for the theme's display name). Returns `None` when no groups are defined or
/// none map to a usable colour — the caller then leaves the theme unchanged.
pub fn build_theme(name: &str) -> Option<Theme> {
    let groups = vimlrs::fusevm_bridge::hl_names();
    if groups.is_empty() {
        return None;
    }

    let mut acc: BTreeMap<&'static str, ScopeStyle> = BTreeMap::new();
    for g in &groups {
        let Some(hl) = vimlrs::fusevm_bridge::hl_resolved(g) else {
            continue;
        };
        if hl.cleared {
            continue;
        }
        let fg = colour_token(hl.guifg.as_deref(), hl.ctermfg.as_deref());
        let bg = colour_token(hl.guibg.as_deref(), hl.ctermbg.as_deref());
        let mods = map_attrs(&hl.attrs);
        if fg.is_none() && bg.is_none() && mods.is_empty() {
            continue;
        }
        for (scope, chan) in scope_targets(&g.to_ascii_lowercase()) {
            let e = acc.entry(scope).or_default();
            match chan {
                Chan::Fg => {
                    if let Some(c) = &fg {
                        e.fg = Some(c.clone());
                    }
                    e.mods.extend(mods.iter().cloned());
                }
                Chan::Bg => {
                    if let Some(c) = &bg {
                        e.bg = Some(c.clone());
                    }
                }
                Chan::FgBg => {
                    if let Some(c) = &fg {
                        e.fg = Some(c.clone());
                    }
                    if let Some(c) = &bg {
                        e.bg = Some(c.clone());
                    }
                    e.mods.extend(mods.iter().cloned());
                }
            }
        }
    }
    if acc.is_empty() {
        return None;
    }

    // Overlay onto the built-in default theme so every UI scope the Vim scheme
    // did not touch keeps a sensible value.
    let mut base = DEFAULT_THEME_DATA.clone();
    let table = base.as_table_mut()?;
    for (scope, style) in acc {
        if let Some(v) = style.into_value() {
            table.insert(scope.to_string(), v);
        }
    }
    let mut theme = Theme::from(base);
    theme.set_name(if name.is_empty() {
        "vim".to_string()
    } else {
        format!("vim:{name}")
    });
    Some(theme)
}
