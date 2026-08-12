//! The **kakoune** keymap.
//!
//! zmax's `helix` preset already carries kakoune's *model* — selection first,
//! action second, multiple selections as a core primitive — because helix took
//! it from kakoune. What it does not carry is kakoune's *key surface*: helix
//! moved several bindings (the view menu onto `z`, text objects behind `m`,
//! `,` for keeping the primary selection) and left kakoune's selection
//! registers unbound.
//!
//! This preset is the helix base with kakoune's own keys merged over it, taken
//! from `doc/pages/keys.asciidoc` (the same file `port/data/kakoune.json` is
//! parsed from). Where kakoune has a feature zmax does not model the same way,
//! the key is left as helix binds it rather than bound to something that only
//! looks similar — those items are the `partial` rows of the port report.

use std::collections::HashMap;

use super::macros::keymap;
use super::{default as helix, merge_keys, KeyTrie, Mode};
use zmax_core::hashmap;

pub fn default() -> HashMap<Mode, KeyTrie> {
    let mut keys = helix::default();
    merge_keys(&mut keys, overrides());
    keys
}

/// The keys kakoune places differently from helix. Everything else is inherited.
fn overrides() -> HashMap<Mode, KeyTrie> {
    // `v` is kakoune's view command (`V` locks it until <esc>), so the view menu
    // moves off helix's `z` — which kakoune needs for its selection registers.
    let view = keymap!({ "View"
        "v" | "c" => align_view_center,
        "m" => align_view_middle,
        "t" => align_view_top,
        "b" => align_view_bottom,
        "j" | "down" => scroll_down,
        "k" | "up" => scroll_up,
        "C-b" | "pageup" => page_up,
        "C-f" | "pagedown" => page_down,
        "C-u" | "backspace" => page_cursor_half_up,
        "C-d" | "space" => page_cursor_half_down,
    });
    let view_sticky = keymap!({ "View" sticky=true
        "v" | "c" => align_view_center,
        "m" => align_view_middle,
        "t" => align_view_top,
        "b" => align_view_bottom,
        "j" | "down" => scroll_down,
        "k" | "up" => scroll_up,
        "C-b" | "pageup" => page_up,
        "C-f" | "pagedown" => page_down,
        "C-u" | "backspace" => page_cursor_half_up,
        "C-d" | "space" => page_cursor_half_down,
    });

    let normal = keymap!({ "Normal mode"
        // View commands live on v/V, not z/Z.
        "v" => { "View"
            "v" | "c" => align_view_center,
            "m" => align_view_middle,
            "t" => align_view_top,
            "b" => align_view_bottom,
            "j" | "down" => scroll_down,
            "k" | "up" => scroll_up,
            "C-b" | "pageup" => page_up,
            "C-f" | "pagedown" => page_down,
            "C-u" | "backspace" => page_cursor_half_up,
            "C-d" | "space" => page_cursor_half_down,
        },
        "V" => { "View" sticky=true
            "v" | "c" => align_view_center,
            "m" => align_view_middle,
            "t" => align_view_top,
            "b" => align_view_bottom,
            "j" | "down" => scroll_down,
            "k" | "up" => scroll_up,
            "C-b" | "pageup" => page_up,
            "C-f" | "pagedown" => page_down,
            "C-u" | "backspace" => page_cursor_half_up,
            "C-d" | "space" => page_cursor_half_down,
        },

        // Selection registers (`:doc registers`): Z saves, z restores, A-z
        // combines the register's selections with the current ones.
        "Z" => save_selections_to_register,
        "z" => restore_selections_from_register,
        "A-z" => combine_selections_from_register,
        "A-Z" => save_selections_to_register,

        // Text objects are `<a-i>` / `<a-a>` in kakoune; helix puts them behind `m`.
        "A-i" => select_textobject_inner,
        "A-a" => select_textobject_around,

        // `m` selects to the next matching pair (`M` extends it) rather than
        // opening a menu.
        "m" => match_brackets,
        "M" => match_brackets,

        // Keeping / clearing selections by regex is `<a-k>` / `<a-K>`.
        "A-k" => keep_selections,
        "A-K" => remove_selections,

        // `$` pipes each selection to a shell command and keeps the ones the
        // command exits zero on.
        "$" => shell_keep_pipe,

        // The primary selection: <space> keeps only it, <a-space> drops it.
        "space" => keep_primary_selection,
        "A-space" => remove_primary_selection,

        // Change history (kakoune walks it with C-j / C-k).
        "C-j" => later,
        "C-k" => earlier,

        // Whitespace conversion inside the selections.
        "@" => convert_indent_to_spaces,
        "A-@" => convert_indent_to_tabs,

        // Empty lines around the cursor without leaving normal mode.
        "A-o" => add_newline_below,
        "A-O" => add_newline_above,

        // Merge overlapping selections (kakoune's `<a-+>`); `+` itself would
        // duplicate a selection onto itself, which zmax cannot hold — see the
        // module docs.
        "A-+" => merge_selections,
    });

    let select = keymap!({ "Select mode"
        "A-i" => select_textobject_inner,
        "A-a" => select_textobject_around,
        "A-k" => keep_selections,
        "A-K" => remove_selections,
        "space" => keep_primary_selection,
        "A-space" => remove_primary_selection,
        "Z" => save_selections_to_register,
        "z" => restore_selections_from_register,
        "A-z" => combine_selections_from_register,
        "$" => shell_keep_pipe,
    });

    // The view menus above are spelled inline in `normal`; these two exist so a
    // future caller (a `v` prefix in another mode) can reuse them.
    let _ = (view, view_sticky);

    hashmap! {
        Mode::Normal => normal,
        Mode::Select => select,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use zmax_view::input::KeyEvent;

    fn chord(keys: &HashMap<Mode, KeyTrie>, mode: Mode, presses: &[&str]) -> Option<String> {
        let events: Vec<KeyEvent> = presses.iter().map(|k| k.parse().unwrap()).collect();
        match keys[&mode].search(&events)? {
            KeyTrie::MappableCommand(cmd) => Some(cmd.name().to_string()),
            KeyTrie::Node(node) => Some(format!("node:{}", node.name)),
            KeyTrie::Sequence(_) => Some("sequence".to_string()),
        }
    }

    #[test]
    fn kakoune_keys_land_where_kakoune_documents_them() {
        let keys = default();
        // Selection registers — the reason this preset exists (keys.asciidoc
        // "Marks": Z saves, z restores, <a-z> combines).
        assert_eq!(
            chord(&keys, Mode::Normal, &["Z"]).as_deref(),
            Some("save_selections_to_register")
        );
        assert_eq!(
            chord(&keys, Mode::Normal, &["z"]).as_deref(),
            Some("restore_selections_from_register")
        );
        assert_eq!(
            chord(&keys, Mode::Normal, &["A-z"]).as_deref(),
            Some("combine_selections_from_register")
        );
        // View commands moved to v/V to make room for them.
        assert_eq!(
            chord(&keys, Mode::Normal, &["v", "t"]).as_deref(),
            Some("align_view_top")
        );
        assert_eq!(
            chord(&keys, Mode::Normal, &["V", "b"]).as_deref(),
            Some("align_view_bottom")
        );
        // Text objects on <a-i>/<a-a>, not behind `m`.
        assert_eq!(
            chord(&keys, Mode::Normal, &["A-i"]).as_deref(),
            Some("select_textobject_inner")
        );
        // <space> reduces to the primary selection (helix uses `,`).
        assert_eq!(
            chord(&keys, Mode::Normal, &["space"]).as_deref(),
            Some("keep_primary_selection")
        );
        // Macro recording is Q, playback q — as in kakoune (and as helix
        // already had it), so the base binding must survive the merge.
        assert_eq!(
            chord(&keys, Mode::Normal, &["Q"]).as_deref(),
            Some("record_macro")
        );
        assert_eq!(
            chord(&keys, Mode::Normal, &["q"]).as_deref(),
            Some("replay_macro")
        );
    }

    #[test]
    fn the_helix_base_still_shows_through() {
        let keys = default();
        // Movement and the goto menu are kakoune's own and are inherited
        // unchanged from the selection-first base.
        assert_eq!(
            chord(&keys, Mode::Normal, &["w"]).as_deref(),
            Some("move_next_word_start")
        );
        assert_eq!(
            chord(&keys, Mode::Normal, &["g", "h"]).as_deref(),
            Some("goto_line_start")
        );
        assert_eq!(
            chord(&keys, Mode::Normal, &["%"]).as_deref(),
            Some("select_all")
        );
        // Select mode keeps the overrides too.
        assert_eq!(
            chord(&keys, Mode::Select, &["A-k"]).as_deref(),
            Some("keep_selections")
        );
    }
}
