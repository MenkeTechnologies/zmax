//! The **spacemacs** keymap — zemacs's default preset.
//!
//! Spacemacs is evil-mode (vim keys) with the `SPC` leader *and* the full Emacs
//! `C-x` command prefix. This keymap is the shared vim base (which already
//! carries the `SPC` leader) with that `C-x` prefix overlaid onto Normal and
//! Select modes, replacing vim's `C-x = decrement` there. Pressing `C-x` opens
//! a which-key popup of the Emacs `C-x` map, exactly like Spacemacs.
//!
//! The `vim` preset is the same base with the `SPC` leader stripped instead (see
//! [`super::vim::default`]); `decrement` stays reachable via `g C-x` in both.

use std::collections::HashMap;

use indexmap::IndexMap;

use super::macros::keymap;
use super::{vim, KeyTrie, KeyTrieNode, MappableCommand, Mode};
use zemacs_core::hashmap;
use zemacs_view::input::KeyEvent;

/// Emacs `C-x` chords that resolve to typable (`:`) commands — the `keymap!`
/// macro only expresses static commands, so these are grafted under the `C-x`
/// node afterward. Keys are relative to `C-x` (e.g. `"C-s"` means `C-x C-s`).
#[rustfmt::skip]
const CX_TYPABLE: &[(&str, &str, &str)] = &[
    ("C-s", "File",   ":write"),           // C-x C-s: save-buffer
    ("s",   "File",   ":write-all"),       // C-x s:   save-some-buffers
    ("C-w", "File",   ":write"),           // C-x C-w: write-file (approx)
    ("C-c", "Quit",   ":write-quit-all"),  // C-x C-c: save-buffers-kill-terminal
    ("k",   "Buffer", ":buffer-close"),    // C-x k:   kill-buffer
];

/// Insert `cmd` at `path` under `root`, creating intermediate submap nodes
/// labelled `label` as needed. Mirrors the helper in the emacs keymap.
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

/// The Emacs `C-x` prefix (static commands), wrapped in a throwaway Normal-mode
/// node so it can be merged into the base keymap with [`KeyTrie::merge_nodes`].
#[rustfmt::skip]
fn cx_prefix() -> KeyTrie {
    keymap!({ "Normal mode"
        "C-x" => { "C-x"
            "u" => undo,                    // C-x u: undo
            "C-f" => file_picker,           // C-x C-f: find-file
            "b" => buffer_picker,           // C-x b: switch-to-buffer
            "C-b" => buffer_picker,         // C-x C-b: list-buffers (approx)
            "o" => rotate_view,             // C-x o: other-window
            "1" => wonly,                   // C-x 1: delete-other-windows
            "0" => wclose,                  // C-x 0: delete-window
            "2" => hsplit,                  // C-x 2: split-window-below
            "3" => vsplit,                  // C-x 3: split-window-right
            "C-space" => pop_to_mark,       // C-x C-SPC: pop-to-mark
            "C-x" => flip_selections,       // C-x C-x: exchange-point-and-mark
            "r" => { "Registers / rectangles / bookmarks"
                "space" => point_to_register,    // C-x r SPC: point-to-register
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
    })
}

/// The spacemacs keymap: the vim base plus the Emacs `C-x` prefix.
pub fn default() -> HashMap<Mode, KeyTrie> {
    let mut keymap = vim::base();
    let cx_key = "C-x".parse::<KeyEvent>().expect("valid key");

    // Overlay `C-x` onto every modal mode the leader lives in. `merge_nodes`
    // replaces vim's `C-x = decrement` leaf with the prefix node.
    for mode in [Mode::Normal, Mode::Select] {
        let Some(trie) = keymap.get_mut(&mode) else {
            continue;
        };
        trie.merge_nodes(cx_prefix());
        // Graft the typable `C-x` bindings (save / write-all / quit / kill-buffer)
        // directly under the freshly-merged `C-x` node.
        if let Some(KeyTrie::Node(cx)) = trie.node_mut().and_then(|n| n.get_mut(&cx_key)) {
            for (key, label, cmd) in CX_TYPABLE {
                let event = key.parse::<KeyEvent>().expect("valid key");
                add_command(cx, &[event], label, cmd);
            }
        }
    }

    keymap
}
