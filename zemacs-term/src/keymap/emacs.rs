//! Emacs keymap.
//!
//! Emacs is modeless — you are always "inserting" — but zemacs runs on a modal
//! engine where self-inserting printable keys only happens in Insert mode. So
//! this keymap puts the real emacs bindings in **Insert mode** and the editor
//! starts in Insert mode when the emacs keymap is selected (see
//! `keymap::default_mode` + `Application::new`). Normal and Select modes are
//! kept usable (movement + an escape hatch back to inserting) but are not where
//! an emacs user normally lives.
//!
//! Region commands (`C-w` kill, `M-w` copy) operate on a selection; `C-space`
//! sets the mark by entering Select mode, and `C-g` collapses it (keyboard-quit).

use std::collections::HashMap;

use indexmap::IndexMap;

use super::macros::keymap;
use super::{KeyTrie, KeyTrieNode, MappableCommand, Mode};
use zemacs_core::hashmap;
use zemacs_view::input::KeyEvent;

/// Insert `cmd` at `path` under `root`, creating intermediate submap nodes
/// (labelled `label`) as needed. `cmd` may be a `:typable` or static command.
/// Used for bindings that resolve to typable commands (`:write`, `:quit-all`),
/// which the `keymap!` macro cannot express.
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
    s.split(' ')
        .map(|k| k.parse().expect("valid key"))
        .collect()
}

/// Emacs chords that resolve to typable (`:`) commands. Applied after macro
/// construction (the macro only expresses static commands).
#[rustfmt::skip]
const EMACS_TYPABLE: &[(&str, &str, &str)] = &[
    ("C-x C-s", "File",   ":write"),         // save-buffer
    ("C-x s",   "File",   ":write-all"),     // save-some-buffers
    ("C-x C-w", "File",   ":write"),         // write-file (approx)
    ("C-x C-c", "Quit",   ":write-quit-all"),// save-buffers-kill-terminal
    ("C-x k",   "Buffer", ":buffer-close"),  // kill-buffer
];

fn add_typables(mode: &mut KeyTrie) {
    if let KeyTrie::Node(root) = mode {
        for (keys, label, cmd) in EMACS_TYPABLE {
            add_command(root, &chord(keys), label, cmd);
        }
    }
}

#[rustfmt::skip]
pub fn default() -> HashMap<Mode, KeyTrie> {
    // Insert mode is where emacs lives: self-inserting text plus C-/M- chords.
    let mut insert = keymap!({ "Insert mode"
        // movement
        "C-f" => move_char_right,
        "C-b" => move_char_left,
        "C-n" => move_visual_line_down,
        "C-p" => move_visual_line_up,
        "C-a" => goto_line_start,
        "C-e" => goto_line_end,             // move-end-of-line (stops before the newline)
        "A-f" => move_next_word_end,        // M-f: forward-word
        "A-b" => move_prev_word_start,      // M-b: backward-word
        "A-<" => goto_file_start,           // M-<: beginning-of-buffer
        "A->" => goto_last_line,            // M->: end-of-buffer
        "C-v" => page_down,
        "A-v" => page_up,
        "C-l" => align_view_center,         // recenter
        "left" => move_char_left,
        "right" => move_char_right,
        "up" => move_visual_line_up,
        "down" => move_visual_line_down,
        "home" => goto_line_start,
        "end" => goto_line_end,
        "pageup" => page_up,
        "pagedown" => page_down,

        // mark / region
        "C-space" => set_mark_command,      // set-mark-command (pushes mark ring)
        "C-g" => collapse_selection,        // keyboard-quit

        // editing
        "C-d" | "del" => delete_char_forward,
        "backspace" | "C-h" => delete_char_backward,
        "C-k" => kill_to_line_end,          // kill-line
        "A-d" => delete_word_forward,       // M-d: kill-word
        "A-backspace" | "C-w" => delete_word_backward, // C-w/M-DEL approx (no region: kill prev word)
        "A-w" => [yank, collapse_selection],// M-w: kill-ring-save (copy)
        "C-y" => yank_from_kill_ring,       // C-y: yank latest kill-ring entry
        "A-y" => yank_pop,                  // M-y: yank-pop, cycle to older kill
        "C-u" => kill_to_line_start,
        "C-_" | "C-/" => undo,              // undo
        "A-/" => completion,                // M-/: dabbrev-expand (dynamic completion)
        "ret" | "C-j" => insert_newline,
        "tab" => emmet_expand,

        // commands / search / files / buffers
        "A-x" => command_palette,           // M-x: execute-extended-command
        "C-s" => search,                    // isearch-forward (approx)
        "C-r" => rsearch,                   // isearch-backward (approx)
        "C-x" => { "C-x"
            "u" => undo,                    // C-x u: undo
            "C-f" => file_picker,           // find-file
            "b" => buffer_picker,           // switch-to-buffer
            "C-b" => buffer_picker,         // list-buffers (approx)
            "o" => rotate_view,             // other-window
            "1" => wonly,                   // delete-other-windows
            "0" => wclose,                  // delete-window
            "2" => hsplit,                  // split-window-below
            "3" => vsplit,                  // split-window-right
            "C-space" => pop_to_mark,       // C-x C-SPC: pop-to-mark
            "C-x" => flip_selections,       // C-x C-x: exchange-point-and-mark
            "r" => { "Registers"
                "space" => point_to_register,   // C-x r SPC: point-to-register
                "j" => jump_to_register,         // C-x r j: jump-to-register
                "n" => number_to_register,       // C-x r n: number-to-register
                "+" => increment_register,       // C-x r +: increment-register
                "i" => emacs_insert_register,    // C-x r i: insert-register
                "k" => kill_rectangle,           // C-x r k: kill-rectangle
                "d" => delete_rectangle,         // C-x r d: delete-rectangle
                "c" => clear_rectangle,          // C-x r c: clear-rectangle
                "y" => yank_rectangle,           // C-x r y: yank-rectangle
                "A-w" => copy_rectangle_as_kill, // C-x r M-w: copy-rectangle-as-kill
                "m" => bookmark_set,             // C-x r m: bookmark-set
                "b" => bookmark_jump,            // C-x r b: bookmark-jump
                "l" => bookmark_jump,            // C-x r l: list-bookmarks
            },
            "'" => expand_abbrev,           // C-x ': expand-abbrev
            "a" => { "Abbrev"
                "g" => define_abbrev,       // C-x a g: add-global-abbrev
            },
        },
    });

    // Select mode = region active after C-space; movement extends, then act.
    let mut select = keymap!({ "Select (region) mode"
        "C-f" => extend_char_right,
        "C-b" => extend_char_left,
        "C-n" => extend_visual_line_down,
        "C-p" => extend_visual_line_up,
        "C-a" => goto_line_start,
        "C-e" => goto_line_end,
        "A-f" => extend_next_word_end,
        "A-b" => extend_prev_word_start,
        "left" => extend_char_left,
        "right" => extend_char_right,
        "up" => extend_visual_line_up,
        "down" => extend_visual_line_down,
        "C-w" => [delete_selection, normal_mode, insert_mode], // kill-region, back to inserting
        "A-w" => [yank, collapse_selection, normal_mode, insert_mode], // copy-region
        "C-g" => [collapse_selection, normal_mode, insert_mode],       // keyboard-quit
        "esc" => [collapse_selection, normal_mode, insert_mode],
    });

    // Normal mode is rarely used in emacs; keep movement working and offer an
    // escape hatch back to inserting. `i`/`a` and most chords re-enter insert.
    let mut normal = keymap!({ "Normal mode"
        "i" | "a" => insert_mode,
        "C-f" | "right" => move_char_right,
        "C-b" | "left"  => move_char_left,
        "C-n" | "down"  => move_visual_line_down,
        "C-p" | "up"    => move_visual_line_up,
        "C-a" | "home"  => goto_line_start,
        "C-e" | "end"   => goto_line_end,
        "A-f" => move_next_word_end,
        "A-b" => move_prev_word_start,
        "C-v" | "pagedown" => page_down,
        "A-v" | "pageup"   => page_up,
        "C-space" => select_mode,
        "C-g" => collapse_selection,
        "C-d" => delete_char_forward,
        "C-k" => kill_to_line_end,
        "C-_" | "C-/" => undo,
        "A-/" => completion,                // M-/: dabbrev-expand
        "C-y" => yank_from_kill_ring,
        "A-y" => yank_pop,
        "A-x" => command_palette,           // M-x: execute-extended-command
        "C-s" => search,
        "C-r" => rsearch,
        "C-x" => { "C-x"
            "u" => undo,
            "C-f" => file_picker,
            "b" => buffer_picker,
            "o" => rotate_view,
            "1" => wonly,
            "0" => wclose,
            "2" => hsplit,
            "3" => vsplit,
        },
    });

    add_typables(&mut insert);
    add_typables(&mut normal);
    // Region kill/copy in select mode also wants C-x save etc.
    add_typables(&mut select);

    hashmap!(
        Mode::Normal => normal,
        Mode::Select => select,
        Mode::Insert => insert,
    )
}
