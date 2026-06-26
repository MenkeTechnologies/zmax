//! Vim default keymap for zemacs.
//!
//! zemacs targets vim/emacs semantics rather than Helix's selection-first
//! model: the keys you press are the keys vim binds. Where vim is verb-noun
//! (operator-pending: `d{motion}`, `c{motion}`, `y{motion}`), we emulate it
//! with nested submaps whose motions run `[collapse_selection, extend-motion,
//! operate]` command sequences. zemacs runs on the Helix engine, so each
//! operator first collapses to the cursor, extends the selection over the
//! motion, then acts — reproducing vim's "operate over the motion" behavior.
//!
//! Numeric counts (`3w`, `d2w`) work for free: the engine consumes a numeric
//! prefix and applies it to the next command.
//!
//! This is the first-step keymap. Known gaps tracked for later passes:
//!   - operator + find-char (`df<c>`, `ct<c>`): the find motion is interactive
//!     and cannot be chained inside a static sequence yet.
//!   - operator + text object (`ciw`, `di(`): needs the text-object pending
//!     state; `mi`/`ma` from the Helix base remain available meanwhile.
//!   - `.` repeat-last-change, vim macros `q`/`@`, marks, and Replace mode.

use std::collections::HashMap;

use super::macros::keymap;
use super::{KeyTrie, Mode};
use helix_core::hashmap;

#[rustfmt::skip]
pub fn default() -> HashMap<Mode, KeyTrie> {
    let normal = keymap!({ "Normal mode"
        // --- left-hand motions ---------------------------------------------
        "h" | "left"  => move_char_left,
        "j" | "down"  => move_visual_line_down,
        "k" | "up"    => move_visual_line_up,
        "l" | "right" => move_char_right,
        "backspace"   => move_char_left,

        // --- word motions ---------------------------------------------------
        "w" => move_next_word_start,
        "b" => move_prev_word_start,
        "e" => move_next_word_end,
        "W" => move_next_long_word_start,
        "B" => move_prev_long_word_start,
        "E" => move_next_long_word_end,

        // --- line / column motions -----------------------------------------
        "0" | "home" => goto_line_start,
        "^"          => goto_first_nonwhitespace,
        "$" | "end"  => goto_line_end,
        "|"          => goto_column,
        "G"          => goto_last_line,
        "%"          => match_brackets,

        // --- screen motions -------------------------------------------------
        "H" => goto_window_top,
        "M" => goto_window_center,
        "L" => goto_window_bottom,

        // --- paragraph motions ----------------------------------------------
        "{" => goto_prev_paragraph,
        "}" => goto_next_paragraph,

        // --- find char ------------------------------------------------------
        "f" => find_next_char,
        "F" => find_prev_char,
        "t" => find_till_char,
        "T" => till_prev_char,
        ";" => repeat_last_motion,

        // --- search ---------------------------------------------------------
        "/" => search,
        "?" => rsearch,
        "n" => search_next,
        "N" => search_prev,
        "*" => [search_selection_detect_word_boundaries, search_next],

        // --- insert entry ---------------------------------------------------
        "i" => insert_mode,
        "I" => insert_at_line_start,
        "a" => append_mode,
        "A" => insert_at_line_end,
        "o" => open_below,
        "O" => open_above,

        // --- single-key edits ----------------------------------------------
        "x" => delete_selection,            // delete char under cursor
        "X" => delete_char_backward,        // delete char before cursor
        "D" => [extend_to_line_end, delete_selection],
        "C" => [extend_to_line_end, change_selection],
        "Y" => [extend_to_line_bounds, yank, collapse_selection],
        "s" => change_selection,            // substitute char
        "S" => [extend_to_line_bounds, change_selection],
        "r" => replace,
        "J" => join_selections,
        "~" => switch_case,
        "p" => paste_after,
        "P" => paste_before,
        "u" => undo,
        "C-r" => redo,

        // --- operator-pending: delete --------------------------------------
        "d" => { "delete"
            "d" => [collapse_selection, extend_to_line_bounds, delete_selection],
            "w" => [collapse_selection, extend_next_word_start, delete_selection],
            "W" => [collapse_selection, extend_next_long_word_start, delete_selection],
            "e" => [collapse_selection, extend_next_word_end, delete_selection],
            "E" => [collapse_selection, extend_next_long_word_end, delete_selection],
            "b" => [collapse_selection, extend_prev_word_start, delete_selection],
            "B" => [collapse_selection, extend_prev_long_word_start, delete_selection],
            "$" => [collapse_selection, extend_to_line_end, delete_selection],
            "0" => [collapse_selection, extend_to_line_start, delete_selection],
            "^" => [collapse_selection, extend_to_first_nonwhitespace, delete_selection],
            "G" => [collapse_selection, extend_to_last_line, delete_selection],
            "%" => [match_brackets, delete_selection],
        },

        // --- operator-pending: change --------------------------------------
        "c" => { "change"
            "c" => [collapse_selection, extend_to_line_bounds, change_selection],
            "w" => [collapse_selection, extend_next_word_end, change_selection],
            "W" => [collapse_selection, extend_next_long_word_end, change_selection],
            "e" => [collapse_selection, extend_next_word_end, change_selection],
            "E" => [collapse_selection, extend_next_long_word_end, change_selection],
            "b" => [collapse_selection, extend_prev_word_start, change_selection],
            "B" => [collapse_selection, extend_prev_long_word_start, change_selection],
            "$" => [collapse_selection, extend_to_line_end, change_selection],
            "^" => [collapse_selection, extend_to_first_nonwhitespace, change_selection],
        },

        // --- operator-pending: yank ----------------------------------------
        "y" => { "yank"
            "y" => [collapse_selection, extend_to_line_bounds, yank, collapse_selection],
            "w" => [collapse_selection, extend_next_word_start, yank, collapse_selection],
            "W" => [collapse_selection, extend_next_long_word_start, yank, collapse_selection],
            "e" => [collapse_selection, extend_next_word_end, yank, collapse_selection],
            "b" => [collapse_selection, extend_prev_word_start, yank, collapse_selection],
            "$" => [collapse_selection, extend_to_line_end, yank, collapse_selection],
            "0" => [collapse_selection, extend_to_line_start, yank, collapse_selection],
            "^" => [collapse_selection, extend_to_first_nonwhitespace, yank, collapse_selection],
            "G" => [collapse_selection, extend_to_last_line, yank, collapse_selection],
        },

        // --- indent operators ----------------------------------------------
        ">" => indent,
        "<" => unindent,

        // --- visual mode ----------------------------------------------------
        "v" => select_mode,
        "V" => [extend_to_line_bounds, select_mode],

        // --- g submap -------------------------------------------------------
        "g" => { "Goto"
            "g" => goto_file_start,
            "e" => goto_last_line,
            "j" => move_line_down,
            "k" => move_line_up,
            "h" => goto_line_start,
            "l" => goto_line_end,
            "d" => goto_definition,
            "D" => goto_declaration,
            "y" => goto_type_definition,
            "r" => goto_reference,
            "i" => goto_implementation,
            "f" => goto_file,
            "a" => goto_last_accessed_file,
            "m" => goto_last_modified_file,
            "n" => goto_next_buffer,
            "p" => goto_previous_buffer,
            "." => goto_last_modification,
        },

        // --- z submap (view) -----------------------------------------------
        "z" => { "View"
            "z" => align_view_center,
            "t" => align_view_top,
            "b" => align_view_bottom,
            "c" => align_view_center,
        },

        // --- bracket submaps (vim unimpaired-ish) --------------------------
        "[" => { "Prev"
            "[" => goto_prev_paragraph,
            "d" => goto_prev_diag,
            "g" => goto_prev_change,
            "f" => goto_prev_function,
        },
        "]" => { "Next"
            "]" => goto_next_paragraph,
            "d" => goto_next_diag,
            "g" => goto_next_change,
            "f" => goto_next_function,
        },

        // --- window commands (C-w) -----------------------------------------
        "C-w" => { "Window"
            "s" | "C-s" => hsplit,
            "v" | "C-v" => vsplit,
            "w" | "C-w" => rotate_view,
            "q" | "C-q" => wclose,
            "o" | "C-o" => wonly,
            "h" | "C-h" => jump_view_left,
            "j" | "C-j" => jump_view_down,
            "k" | "C-k" => jump_view_up,
            "l" | "C-l" => jump_view_right,
        },

        // --- scrolling / jumps ---------------------------------------------
        "C-d" => page_cursor_half_down,
        "C-u" => page_cursor_half_up,
        "C-f" | "pagedown" => page_down,
        "C-b" | "pageup"   => page_up,
        "C-o" => jump_backward,
        "C-i" | "tab" => jump_forward,
        "C-e" => scroll_down,
        "C-y" => scroll_up,

        // --- increment / decrement -----------------------------------------
        "C-a" => increment,
        "C-x" => decrement,

        // --- misc -----------------------------------------------------------
        ":" => command_mode,
        "C-z" => suspend,
        "esc" => collapse_selection,

        // --- leader (space) — kept for pickers / LSP / commands ------------
        "," => keep_primary_selection,
        "space" => { "Leader"
            "f" => file_picker,
            "b" => buffer_picker,
            "j" => jumplist_picker,
            "s" => symbol_picker,
            "S" => workspace_symbol_picker,
            "d" => diagnostics_picker,
            "/" => global_search,
            "k" => hover,
            "r" => rename_symbol,
            "a" => code_action,
            "'" => last_picker,
            "y" => yank_to_clipboard,
            "p" => paste_clipboard_after,
            "P" => paste_clipboard_before,
            "?" => command_palette,
            "c" => toggle_comments,
            // Kept identical to the `C-w` window submap (see aliased-modes test).
            "w" => { "Window"
                "s" | "C-s" => hsplit,
                "v" | "C-v" => vsplit,
                "w" | "C-w" => rotate_view,
                "q" | "C-q" => wclose,
                "o" | "C-o" => wonly,
                "h" | "C-h" => jump_view_left,
                "j" | "C-j" => jump_view_down,
                "k" | "C-k" => jump_view_up,
                "l" | "C-l" => jump_view_right,
            },
        },
    });

    // Visual / select mode: motions extend, operators act directly.
    let select = keymap!({ "Visual mode"
        "h" | "left"  => extend_char_left,
        "j" | "down"  => extend_visual_line_down,
        "k" | "up"    => extend_visual_line_up,
        "l" | "right" => extend_char_right,

        "w" => extend_next_word_start,
        "b" => extend_prev_word_start,
        "e" => extend_next_word_end,
        "W" => extend_next_long_word_start,
        "B" => extend_prev_long_word_start,
        "E" => extend_next_long_word_end,

        "0" | "home" => extend_to_line_start,
        "^"          => extend_to_first_nonwhitespace,
        "$" | "end"  => extend_to_line_end,
        "G"          => extend_to_last_line,
        "%"          => match_brackets,
        "{"          => goto_prev_paragraph,
        "}"          => goto_next_paragraph,

        "f" => extend_next_char,
        "F" => extend_prev_char,
        "t" => extend_till_char,
        "T" => extend_till_prev_char,
        ";" => repeat_last_motion,

        "i" => select_textobject_inner,
        "a" => select_textobject_around,

        "d" | "x" => [delete_selection, normal_mode],
        "c" | "s" => change_selection,
        "y"       => [yank, collapse_selection, normal_mode],
        "p"       => replace_with_yanked,
        "r"       => replace,
        "J"       => [join_selections, normal_mode],
        "~"       => switch_case,
        "u"       => [switch_to_lowercase, normal_mode],
        "U"       => [switch_to_uppercase, normal_mode],
        ">"       => [indent, normal_mode],
        "<"       => [unindent, normal_mode],
        "o"       => flip_selections,
        "V"       => extend_to_line_bounds,

        "C-a" => increment,
        "C-x" => decrement,

        ":" => command_mode,
        "esc" => [collapse_selection, normal_mode],
    });

    // Insert mode: vim-style editing keys.
    let insert = keymap!({ "Insert mode"
        "esc" => normal_mode,
        "C-c" => normal_mode,

        "backspace" | "C-h" => delete_char_backward,
        "del"               => delete_char_forward,
        "C-w"               => delete_word_backward,
        "A-backspace"       => delete_word_backward,
        "A-d"               => delete_word_forward,
        "C-u"               => kill_to_line_start,
        "C-k"               => kill_to_line_end,

        "ret"   => insert_newline,
        "C-j"   => insert_newline,
        "tab"   => insert_tab,

        "C-r"   => insert_register,
        "C-a"   => insert_at_line_start,
        "C-e"   => insert_at_line_end,

        "up"    => move_visual_line_up,
        "down"  => move_visual_line_down,
        "left"  => move_char_left,
        "right" => move_char_right,
        "home"  => goto_line_start,
        "end"   => goto_line_end_newline,

        "pageup"   => page_up,
        "pagedown" => page_down,
    });

    hashmap!(
        Mode::Normal => normal,
        Mode::Select => select,
        Mode::Insert => insert,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{KeyTrie, MappableCommand};
    use helix_view::input::KeyEvent;

    fn cmd_name(trie: &KeyTrie) -> Option<&str> {
        match trie {
            KeyTrie::MappableCommand(MappableCommand::Static { name, .. }) => Some(name),
            _ => None,
        }
    }

    /// Walk a chord like "g l" or "d w" through the trie and return the leaf.
    fn resolve<'a>(root: &'a KeyTrie, chord: &str) -> Option<&'a KeyTrie> {
        let keys: Vec<KeyEvent> = chord.split(' ').map(|k| k.parse().unwrap()).collect();
        root.search(&keys)
    }

    #[test]
    fn vim_keymap_constructs() {
        // Panics here would mean a duplicate key within a node.
        let km = default();
        assert!(km.contains_key(&Mode::Normal));
        assert!(km.contains_key(&Mode::Select));
        assert!(km.contains_key(&Mode::Insert));
    }

    #[test]
    fn vim_direct_motions_bound_to_vim_keys() {
        let km = default();
        let n = &km[&Mode::Normal];
        // The keys vim users actually press now resolve to the right command.
        assert_eq!(cmd_name(resolve(n, "$").unwrap()), Some("goto_line_end"));
        assert_eq!(cmd_name(resolve(n, "0").unwrap()), Some("goto_line_start"));
        assert_eq!(
            cmd_name(resolve(n, "^").unwrap()),
            Some("goto_first_nonwhitespace")
        );
        assert_eq!(cmd_name(resolve(n, "%").unwrap()), Some("match_brackets"));
        assert_eq!(cmd_name(resolve(n, "G").unwrap()), Some("goto_last_line"));
        assert_eq!(cmd_name(resolve(n, "H").unwrap()), Some("goto_window_top"));
        assert_eq!(cmd_name(resolve(n, "x").unwrap()), Some("delete_selection"));
        assert_eq!(cmd_name(resolve(n, "i").unwrap()), Some("insert_mode"));
        assert_eq!(cmd_name(resolve(n, "a").unwrap()), Some("append_mode"));
    }

    #[test]
    fn vim_operator_pending_is_a_sequence() {
        let km = default();
        let n = &km[&Mode::Normal];
        // `dd`, `dw`, `cw`, `yy` resolve to multi-command sequences.
        for chord in ["d d", "d w", "c w", "y y", "d $"] {
            let leaf = resolve(n, chord)
                .unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert!(
                matches!(leaf, KeyTrie::Sequence(_)),
                "{chord} should be an operator sequence, got {leaf:?}"
            );
        }
    }
}
