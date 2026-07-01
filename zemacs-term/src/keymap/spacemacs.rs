//! The **spacemacs** keymap — zemacs's default preset.
//!
//! Spacemacs is evil-mode (vim keys) with the `SPC` leader *and* the standard
//! Emacs command prefixes `C-x`, `C-c` and `C-h`. This keymap is the shared vim
//! base (which already carries the `SPC` leader) with those three Emacs prefixes
//! overlaid onto Normal and Select modes (replacing vim's `C-x = decrement` and
//! `C-h = move-left` there). Pressing `C-x` / `C-c` / `C-h` opens a which-key
//! popup of the Emacs map, exactly like Spacemacs. Bindings resolve to the
//! nearest zemacs command; Emacs features with no zemacs analogue (Calc, frames,
//! VC, coding-system menus, keyboard macros) are intentionally left unbound.
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

/// The Emacs `C-x` prefix (static commands). Wrapped in a throwaway node so it
/// can be merged into the base keymap with [`KeyTrie::merge_nodes`].
#[rustfmt::skip]
fn cx_prefix() -> KeyTrie {
    keymap!({ "overlay"
        "C-x" => { "C-x"
            "u" => undo,                    // C-x u: undo
            "C-f" => file_picker,           // C-x C-f: find-file
            "C-v" => file_picker,           // C-x C-v: find-alternate-file
            "C-r" => file_picker,           // C-x C-r: find-file-read-only
            "C-q" => toggle_readonly,       // C-x C-q: read-only-mode
            "b" => buffer_picker,           // C-x b: switch-to-buffer
            "C-b" => buffer_picker,         // C-x C-b: list-buffers
            "left" => goto_previous_buffer, // C-x <left>: previous-buffer
            "right" => goto_next_buffer,    // C-x <right>: next-buffer
            "d" => file_picker_in_current_directory,        // C-x d: dired
            "C-d" => file_picker_in_current_directory,      // C-x C-d: list-directory
            "C-j" => file_picker_in_current_buffer_directory, // C-x C-j: dired-jump
            "o" => rotate_view,             // C-x o: other-window
            "1" => wonly,                   // C-x 1: delete-other-windows
            "0" => wclose,                  // C-x 0: delete-window
            "2" => hsplit,                  // C-x 2: split-window-below
            "3" => vsplit,                  // C-x 3: split-window-right
            "{" => resize_view_narrower,    // C-x {: shrink-window-horizontally
            "4" => { "Other window"
                "f" => goto_file,           // C-x 4 f: find-file-other-window
                "b" => buffer_picker,       // C-x 4 b: switch-to-buffer-other-window
                "0" => wclose,              // C-x 4 0: kill-buffer-and-window
                "." => goto_definition,     // C-x 4 .: find-tag-other-window
            },
            "C-space" => pop_to_mark,       // C-x C-SPC: pop-to-mark
            "C-x" => flip_selections,       // C-x C-x: exchange-point-and-mark
            "space" => visual_block_mode,   // C-x SPC: rectangle-mark-mode
            "h" => select_all,              // C-x h: mark-whole-buffer
            "C-l" => switch_to_lowercase,   // C-x C-l: downcase-region
            "C-u" => switch_to_uppercase,   // C-x C-u: upcase-region
            "C-;" => toggle_comments,       // C-x C-;: comment-line
            "tab" => indent,                // C-x TAB: indent-rigidly
            "=" => file_info,               // C-x =: what-cursor-position
            "n" => { "Narrow"
                "n" => narrow_to_region,    // C-x n n: narrow-to-region
                "w" => widen,               // C-x n w: widen
                "d" => narrow_to_function,  // C-x n d: narrow-to-defun
                "p" => narrow_to_page,      // C-x n p: narrow-to-page
            },
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

/// The Emacs `C-c` prefix. Globally this is the mode/user prefix (almost empty by
/// default); `C-c C-c` is the near-universal "execute / compile" action.
#[rustfmt::skip]
fn cc_prefix() -> KeyTrie {
    keymap!({ "overlay"
        "C-c" => { "C-c (mode prefix)"
            "C-c" => run_active_config,     // C-c C-c: execute / compile (major-mode action)
            "C-r" => rerun_last_run,        // C-c C-r: re-run
        },
    })
}

/// The Emacs `C-h` help prefix, routed to zemacs's help / discovery commands.
#[rustfmt::skip]
fn ch_prefix() -> KeyTrie {
    keymap!({ "overlay"
        "C-h" => { "Help"
            "C-h" => help,                  // C-h C-h: help-for-help
            "?" => help,
            "k" => help,                    // C-h k: describe-key
            "K" => help,
            "c" => help,                    // C-h c: describe-key-briefly
            "w" => help,                    // C-h w: where-is
            "b" => help,                    // C-h b: describe-bindings
            "t" => help,                    // C-h t: help-with-tutorial
            "l" => help,                    // C-h l: view-lossage
            "e" => help,                    // C-h e: view-echo-area-messages
            "s" => help,                    // C-h s: describe-syntax
            "h" => help,                    // C-h h: describe international chars
            "f" => command_palette,         // C-h f: describe-function
            "F" => command_palette,
            "o" => command_palette,         // C-h o: describe-symbol
            "x" => command_palette,         // C-h x: describe-command
            "a" => command_palette,         // C-h a: apropos-command
            "d" => command_palette,         // C-h d: apropos-documentation
            "m" => describe_current_modes,  // C-h m: describe-mode
            "i" => info_search,             // C-h i: info
            "v" => config_variable_search,  // C-h v: describe-variable
            "p" => package_search,          // C-h p: finder-by-keyword
            "P" => package_search,          // C-h P: describe-package
            "L" => layer_search,            // C-h L: spacemacs layers
            "S" => man_page_search,         // C-h S: info-lookup-symbol (approx)
        },
    })
}

/// The spacemacs keymap: the vim base plus the Emacs `C-x` / `C-c` / `C-h`
/// prefixes, active in Normal, Select **and** Insert — exactly like Spacemacs.
pub fn default() -> HashMap<Mode, KeyTrie> {
    let mut keymap = vim::base();
    let cx_key = "C-x".parse::<KeyEvent>().expect("valid key");
    let prefix_keys: [KeyEvent; 3] = [
        "C-x".parse().expect("valid key"),
        "C-c".parse().expect("valid key"),
        "C-h".parse().expect("valid key"),
    ];

    for mode in [Mode::Normal, Mode::Select, Mode::Insert] {
        let Some(trie) = keymap.get_mut(&mode) else {
            continue;
        };
        // Drop any existing binding on these keys first (vim's insert-mode C-x
        // completion, C-h backspace, decrement, …) so the Emacs prefix replaces
        // them cleanly instead of recursively merging into a hybrid node.
        if let Some(node) = trie.node_mut() {
            for k in &prefix_keys {
                node.shift_remove(k);
            }
        }
        trie.merge_nodes(cx_prefix());
        trie.merge_nodes(cc_prefix());
        trie.merge_nodes(ch_prefix());
        // Graft the typable `C-x` bindings (save / write-all / quit / kill-buffer)
        // the `keymap!` macro can't express, under the freshly-merged `C-x` node.
        if let Some(KeyTrie::Node(cx)) = trie.node_mut().and_then(|n| n.get_mut(&cx_key)) {
            for (key, label, cmd) in CX_TYPABLE {
                let event = key.parse::<KeyEvent>().expect("valid key");
                add_command(cx, &[event], label, cmd);
            }
        }
    }

    keymap
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search<'a>(km: &'a HashMap<Mode, KeyTrie>, mode: Mode, chord: &str) -> Option<&'a KeyTrie> {
        let keys: Vec<KeyEvent> = chord.split(' ').map(|k| k.parse().unwrap()).collect();
        km[&mode].search(&keys)
    }
    fn is_prefix(km: &HashMap<Mode, KeyTrie>, mode: Mode, chord: &str) -> bool {
        matches!(search(km, mode, chord), Some(KeyTrie::Node(_)))
    }
    fn cmd(km: &HashMap<Mode, KeyTrie>, mode: Mode, chord: &str) -> Option<String> {
        match search(km, mode, chord) {
            Some(KeyTrie::MappableCommand(c)) => Some(c.name().to_string()),
            _ => None,
        }
    }

    #[test]
    fn emacs_prefixes_active_in_all_modes() {
        let km = default();
        // C-x / C-c / C-h are real Emacs prefixes in Normal, Select AND Insert
        // (Spacemacs behaviour) — not vim's decrement / completion / move-left.
        for mode in [Mode::Normal, Mode::Select, Mode::Insert] {
            assert!(
                is_prefix(&km, mode, "C-x"),
                "C-x must be a prefix in {mode}"
            );
            assert!(
                is_prefix(&km, mode, "C-c"),
                "C-c must be a prefix in {mode}"
            );
            assert!(
                is_prefix(&km, mode, "C-h"),
                "C-h must be a prefix in {mode}"
            );
        }
        // Insert-mode C-x is the Emacs prefix now, not vim's i_CTRL-X completion.
        assert_eq!(
            cmd(&km, Mode::Insert, "C-x C-f").as_deref(),
            Some("file_picker")
        );
        assert!(cmd(&km, Mode::Insert, "C-x C-s").is_some(), "C-x C-s save");
        assert_eq!(
            cmd(&km, Mode::Normal, "C-h m").as_deref(),
            Some("describe_current_modes")
        );
    }
}
