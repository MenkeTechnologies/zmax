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
use super::{KeyTrie, KeyTrieNode, MappableCommand, Mode};
use helix_core::hashmap;
use helix_view::input::KeyEvent;
use indexmap::IndexMap;

/// spacemacs SPC bindings that resolve to typable (`:`) commands. The keymap
/// macro can only express static commands, so these are inserted after macro
/// construction. Format: (chord, submap label, command). The chord uses the
/// same space-joined notation the port report parses, so coverage stays honest.
#[rustfmt::skip]
const SPACEMACS_TYPABLE: &[(&str, &str, &str)] = &[
    ("space f s", "Files",   ":write"),            // SPC f s : save
    ("space f S", "Files",   ":write-all"),        // SPC f S : save all
    ("space f R", "Files",   ":move"),             // SPC f R : rename file
    ("space b d", "Buffers", ":buffer-close"),     // SPC b d : kill buffer
    ("space b D", "Buffers", ":buffer-close-others"), // SPC b C-d / others
    ("space b R", "Buffers", ":reload"),           // SPC b R : revert
    ("space b N", "Buffers", ":new"),              // SPC b N : new buffer
    ("space q q", "Quit",    ":quit-all"),         // SPC q q : quit
    ("space q Q", "Quit",    ":quit-all!"),        // SPC q Q : force quit
    ("space q s", "Quit",    ":write-quit-all"),   // SPC q s : save and quit
    ("space f T", "Files",   ":theme"),            // SPC T n / theme
    ("space x l s", "Text",  ":sort"),             // SPC x l s : sort lines
    // SPC t toggles -> existing :toggle substrate (config options).
    ("space t n r", "Toggles", ":toggle line-number absolute relative"), // relative nums
    ("space t n a", "Toggles", ":toggle line-number relative absolute"), // absolute nums
    ("space t i",   "Toggles", ":toggle indent-guides.render"),          // indent guides
    ("space t a",   "Toggles", ":toggle auto-completion"),               // auto-complete
    ("space t h h", "Toggles", ":toggle cursorline"),                    // highlight line
    ("space t w",   "Toggles", ":toggle whitespace.render all none"),    // whitespace
    ("space x d w", "Text",    ":delete-trailing-whitespace"),           // SPC x d w
    ("space x l d", "Text",    ":duplicate-line"),                       // SPC x l d
    ("space x J",   "Text",    ":move-line-down"),                       // SPC x J : drag down
    ("space x K",   "Text",    ":move-line-up"),                         // SPC x K : drag up
    ("space x t c", "Text",    ":transpose-chars"),                      // SPC x t c
    ("space x t l", "Text",    ":move-line-up"),                         // SPC x t l : transpose lines
    ("space x t w", "Text",    ":transpose-words"),                      // SPC x t w
    ("space x l u", "Text",    ":uniquify-lines"),                       // SPC x l u
    ("space x d l", "Text",    ":delete-blank-lines"),                   // SPC x d l
    ("space x d space", "Text", ":just-one-space"),                      // SPC x d SPC
    ("space x i c", "Text",    ":change-case camel"),                    // SPC x i c
    ("space x i u", "Text",    ":change-case snake"),                    // SPC x i u
    ("space x i k", "Text",    ":change-case kebab"),                    // SPC x i k
    ("space x i p", "Text",    ":change-case pascal"),                   // PascalCase
    ("space x i i", "Text",    ":cycle-case"),                           // SPC x i i : cycle
    ("space j n",   "Jump",    ":split-line"),                           // SPC j n : split line
    ("space b s",   "Buffers", ":new"),                                  // SPC b s : scratch buffer
];

/// Insert `cmd` at `path` under `root`, creating intermediate submap nodes
/// (labelled `label`) as needed. `cmd` may be a `:typable` or static command.
fn add_command(root: &mut KeyTrieNode, path: &[KeyEvent], label: &str, cmd: &str) {
    let (head, rest) = path.split_first().expect("non-empty key path");
    if rest.is_empty() {
        root.insert(
            *head,
            KeyTrie::MappableCommand(cmd.parse::<MappableCommand>().expect("valid command")),
        );
        return;
    }
    let child = root
        .entry(*head)
        .or_insert_with(|| KeyTrie::Node(KeyTrieNode::new(label, IndexMap::new())));
    if let KeyTrie::Node(child_node) = child {
        add_command(child_node, rest, label, cmd);
    }
}

fn chord(s: &str) -> Vec<KeyEvent> {
    s.split(' ').map(|k| k.parse().expect("valid key")).collect()
}

fn add_spacemacs_typables(normal: &mut KeyTrie) {
    if let KeyTrie::Node(root) = normal {
        for (ch, label, cmd) in SPACEMACS_TYPABLE {
            add_command(root, &chord(ch), label, cmd);
        }
    }
}

#[rustfmt::skip]
pub fn default() -> HashMap<Mode, KeyTrie> {
    let mut normal = keymap!({ "Normal mode"
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

        // --- marks ----------------------------------------------------------
        "m"  => set_mark,        // m{a-z} set mark
        "`"  => goto_mark,       // `{a-z} jump to mark (exact)
        "'"  => goto_mark_line,  // '{a-z} jump to mark line

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
            "i" => delete_textobject_inner,   // diw, di(, dip, ...
            "a" => delete_textobject_around,  // daw, da(, ...
            "f" => delete_find_char_forward,  // df<c>
            "t" => delete_till_char_forward,  // dt<c>
            "F" => delete_find_char_backward, // dF<c>
            "T" => delete_till_char_backward, // dT<c>
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
            "i" => change_textobject_inner,   // ciw, ci(, cip, ...
            "a" => change_textobject_around,  // caw, ca(, ...
            "f" => change_find_char_forward,  // cf<c>
            "t" => change_till_char_forward,  // ct<c>
            "F" => change_find_char_backward, // cF<c>
            "T" => change_till_char_backward, // cT<c>
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
            "i" => yank_textobject_inner,     // yiw, yi(, yip, ...
            "a" => yank_textobject_around,    // yaw, ya(, ...
            "f" => yank_find_char_forward,    // yf<c>
            "t" => yank_till_char_forward,    // yt<c>
            "F" => yank_find_char_backward,   // yF<c>
            "T" => yank_till_char_backward,   // yT<c>
        },

        // --- indent operators ----------------------------------------------
        ">" => indent,
        "<" => unindent,

        // --- visual mode ----------------------------------------------------
        "v" => select_mode,
        "V" => [extend_to_line_bounds, select_mode],

        // --- g submap -------------------------------------------------------
        "g" => { "Goto"
            // case-change operators (gU / gu / g~ + motion)
            "U" => { "Uppercase"
                "U" => [extend_to_line_bounds, switch_to_uppercase, collapse_selection],
                "w" => [collapse_selection, extend_next_word_start, switch_to_uppercase, collapse_selection],
                "W" => [collapse_selection, extend_next_long_word_start, switch_to_uppercase, collapse_selection],
                "e" => [collapse_selection, extend_next_word_end, switch_to_uppercase, collapse_selection],
                "b" => [collapse_selection, extend_prev_word_start, switch_to_uppercase, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, switch_to_uppercase, collapse_selection],
                "^" => [collapse_selection, extend_to_first_nonwhitespace, switch_to_uppercase, collapse_selection],
            },
            "u" => { "Lowercase"
                "u" => [extend_to_line_bounds, switch_to_lowercase, collapse_selection],
                "w" => [collapse_selection, extend_next_word_start, switch_to_lowercase, collapse_selection],
                "W" => [collapse_selection, extend_next_long_word_start, switch_to_lowercase, collapse_selection],
                "e" => [collapse_selection, extend_next_word_end, switch_to_lowercase, collapse_selection],
                "b" => [collapse_selection, extend_prev_word_start, switch_to_lowercase, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, switch_to_lowercase, collapse_selection],
                "^" => [collapse_selection, extend_to_first_nonwhitespace, switch_to_lowercase, collapse_selection],
            },
            "~" => { "Toggle case"
                "~" => [extend_to_line_bounds, switch_case, collapse_selection],
                "w" => [collapse_selection, extend_next_word_start, switch_case, collapse_selection],
                "W" => [collapse_selection, extend_next_long_word_start, switch_case, collapse_selection],
                "e" => [collapse_selection, extend_next_word_end, switch_case, collapse_selection],
                "b" => [collapse_selection, extend_prev_word_start, switch_case, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, switch_case, collapse_selection],
                "^" => [collapse_selection, extend_to_first_nonwhitespace, switch_case, collapse_selection],
            },

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
            "r" => rotate_view,
            "q" | "C-q" => wclose,
            "d" | "C-d" => wclose,
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
        // --- leader (space): spacemacs SPC tree ----------------------------
        // Structured to mirror spacemacs' SPC keybinding tree. Only bindings
        // that map to a real zemacs static command are present; spacemacs
        // bindings needing a typable (`:w` save, `:q` quit, `:bd`) are not yet
        // expressible in the keymap macro and remain tracked as absent.
        "," => keep_primary_selection,
        "space" => { "Leader (spacemacs SPC)"
            "space" => command_palette,            // SPC SPC : M-x
            "tab"   => goto_last_accessed_file,    // SPC TAB : alternate buffer
            ":"     => command_mode,               // SPC :   : Ex command
            "/"     => global_search,              // SPC /   : search project
            "?"     => command_palette,            // SPC ?   : commands
            "'"     => last_picker,                // SPC '   : resume picker
            ";"     => toggle_comments,            // SPC ;   : comment operator

            "f" => { "Files"
                "f" => file_picker,                            // SPC f f
                "r" => goto_last_modified_file,                // SPC f r
                "t" => file_explorer,                          // SPC f t
                "d" => file_explorer_in_current_buffer_directory, // SPC f d
                "j" => file_explorer_in_current_buffer_directory, // SPC f j : dired
            },
            "b" => { "Buffers"
                "b" => buffer_picker,              // SPC b b
                "n" => goto_next_buffer,           // SPC b n
                "p" => goto_previous_buffer,       // SPC b p
                "m" => changed_file_picker,        // SPC b m
                "Y" => [select_all, yank_to_clipboard, collapse_selection], // SPC b Y
            },
            // Kept identical to the `C-w` window submap (see aliased-modes test).
            "w" => { "Window"
                "s" | "C-s" => hsplit,
                "v" | "C-v" => vsplit,
                "w" | "C-w" => rotate_view,
                "r" => rotate_view,
                "q" | "C-q" => wclose,
                "d" | "C-d" => wclose,
                "o" | "C-o" => wonly,
                "h" | "C-h" => jump_view_left,
                "j" | "C-j" => jump_view_down,
                "k" | "C-k" => jump_view_up,
                "l" | "C-l" => jump_view_right,
            },
            "s" => { "Search"
                "s" => global_search,              // SPC s s
                "f" => global_search,              // SPC s f
                "b" => global_search,              // SPC s b
                "p" => global_search,              // SPC s p
                "j" => symbol_picker,              // SPC s j
                "e" => select_references_to_symbol_under_cursor, // SPC s e : edit occurrences
                "S" => workspace_symbol_picker,
            },
            "p" => { "Project"
                "f" => file_picker,                // SPC p f
                "p" => file_picker,                // SPC p p
                "b" => buffer_picker,              // SPC p b : project buffer
                "h" => file_picker,                // SPC p h : find file
                "s" => global_search,              // SPC p s
                "r" => goto_last_modified_file,    // SPC p r
            },
            "e" => { "Errors"
                "l" => diagnostics_picker,             // SPC e l
                "L" => workspace_diagnostics_picker,   // SPC e L
                "n" => goto_next_diag,                 // SPC e n
                "p" => goto_prev_diag,                 // SPC e p
                "f" => goto_first_diag,                // SPC e f
                "." => goto_last_diag,
            },
            "c" => { "Comments"
                "l" => toggle_line_comments,       // SPC c l
                "c" => toggle_comments,            // SPC c c
                "b" => toggle_block_comments,      // SPC c b
            },
            "j" => { "Jump"
                "i" => symbol_picker,              // SPC j i
                "j" => jumplist_picker,            // SPC j j
                "0" => goto_line_start,            // SPC j 0
                "$" => goto_line_end,              // SPC j $
                "b" => jump_backward,              // SPC j b : back to prev location
                "d" => file_explorer_in_current_buffer_directory, // SPC j d : dir listing
                "c" => goto_last_change,           // SPC j c : go to last change
                "k" => [move_visual_line_down, indent], // SPC j k : next line + indent
            },
            "g" => { "Goto (LSP)"
                "d" => goto_definition,
                "D" => goto_declaration,
                "r" => goto_reference,
                "i" => goto_implementation,
                "y" => goto_type_definition,
            },
            "l" => { "LSP"
                "r" => rename_symbol,              // SPC l r
                "a" => code_action,                // SPC l a
                "k" => hover,                      // SPC l k
                "s" => signature_help,             // SPC l s
                "f" => format_selections,          // SPC l f
            },
            "v" => expand_selection,               // SPC v : expand region
            "x" => { "Text"
                "u" => switch_to_lowercase,        // SPC x u : lowercase
                "tab" => indent,                   // SPC x TAB : indent region
                "a" => { "Align"
                    "a" => align_selections,       // SPC x a a : align region
                },
            },
            "r" => { "Resume / registers"
                "l" => last_picker,                // SPC r l : resume picker
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

    add_spacemacs_typables(&mut normal);

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
    fn spacemacs_leader_tree_bound() {
        let km = default();
        let n = &km[&Mode::Normal];
        // spacemacs SPC tree resolves to the expected zemacs commands.
        assert_eq!(cmd_name(resolve(n, "space f f").unwrap()), Some("file_picker"));
        assert_eq!(
            cmd_name(resolve(n, "space b b").unwrap()),
            Some("buffer_picker")
        );
        assert_eq!(
            cmd_name(resolve(n, "space space").unwrap()),
            Some("command_palette")
        );
        assert_eq!(
            cmd_name(resolve(n, "space e n").unwrap()),
            Some("goto_next_diag")
        );
        assert_eq!(
            cmd_name(resolve(n, "space s s").unwrap()),
            Some("global_search")
        );
    }

    #[test]
    fn spacemacs_typable_bindings_inserted() {
        let km = default();
        let n = &km[&Mode::Normal];
        // SPC f s / SPC q q etc. resolve to typable commands inserted post-macro.
        for (chord_str, _, cmd) in SPACEMACS_TYPABLE {
            let leaf = resolve(n, chord_str)
                .unwrap_or_else(|| panic!("{chord_str} did not resolve"));
            // The bound leaf must equal what the command string parses to, and
            // it must be a typable command.
            let expected = KeyTrie::MappableCommand(
                cmd.parse::<MappableCommand>().expect("valid command"),
            );
            assert_eq!(leaf, &expected, "wrong command for {chord_str}");
            assert!(
                matches!(leaf, KeyTrie::MappableCommand(MappableCommand::Typable { .. })),
                "{chord_str} should be a typable command"
            );
        }
    }

    #[test]
    fn vim_case_operators_are_sequences() {
        let km = default();
        let n = &km[&Mode::Normal];
        for chord in ["g U U", "g U w", "g u u", "g u w", "g ~ ~", "g ~ w"] {
            let leaf =
                resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert!(
                matches!(leaf, KeyTrie::Sequence(_)),
                "{chord} should be a case-operator sequence"
            );
        }
    }

    #[test]
    fn spacemacs_composite_bindings_are_sequences() {
        let km = default();
        let n = &km[&Mode::Normal];
        for chord in ["space j k", "space b Y"] {
            let leaf =
                resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert!(
                matches!(leaf, KeyTrie::Sequence(_)),
                "{chord} should be a command sequence"
            );
        }
        assert_eq!(
            cmd_name(resolve(n, "space x tab").unwrap()),
            Some("indent")
        );
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
