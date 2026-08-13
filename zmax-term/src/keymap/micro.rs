//! The **micro** keymap.
//!
//! micro is modeless — typing inserts, everything else is a control or alt
//! chord — so this preset starts from the emacs base (zmax's other modeless
//! keymap, where insert mode already carries the editing chords) and lays
//! micro's own defaults over it.
//!
//! The bindings come from micro's shipped `bindings.json` defaults, quoted in
//! `runtime/help/keybindings.md` and parsed into `port/data/micro.json`. Chords
//! whose micro action has no zmax command are left to the base rather than
//! bound to something that merely looks similar; those stay absent in the port
//! report instead of being claimed here.

use std::collections::HashMap;

use indexmap::IndexMap;

use super::macros::keymap;
use super::{emacs, merge_keys, KeyTrie, KeyTrieNode, MappableCommand, Mode};
use zmax_core::hashmap;
use zmax_view::input::KeyEvent;

/// micro chords whose action is a zmax `:` command rather than a static one.
/// The `keymap!` macro only expresses statics, so these are grafted after.
#[rustfmt::skip]
const MICRO_TYPABLE: &[(&str, &str, &str)] = &[
    ("C-s",  "File",  ":write"),        // Save
    ("F2",   "File",  ":write"),        // Save
    ("C-q",  "File",  ":quit"),         // Quit
    ("F4",   "File",  ":quit"),         // Quit
    ("F10",  "File",  ":quit"),         // Quit
    ("C-g",  "Help",  ":help"),         // ToggleHelp
    ("C-b",  "Shell", ":run-shell-command"), // ShellMode
    ("C-t",  "Tabs",  ":tabnew"),       // AddTab
];

pub fn default() -> HashMap<Mode, KeyTrie> {
    let mut keys = emacs::default();
    merge_keys(&mut keys, overrides());
    for mode in [Mode::Insert, Mode::Normal] {
        if let Some(KeyTrie::Node(root)) = keys.get_mut(&mode) {
            for (chord, label, cmd) in MICRO_TYPABLE {
                add_command(root, &keys_of(chord), label, cmd);
            }
        }
    }
    keys
}

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

fn keys_of(s: &str) -> Vec<KeyEvent> {
    s.split(' ').map(|k| k.parse().expect("valid key")).collect()
}

fn overrides() -> HashMap<Mode, KeyTrie> {
    macro_rules! micro_keys {
        () => {
            keymap!({ "micro"
                "C-o" => file_picker,               // OpenFile
                "C-f" => search,                    // Find
                "C-n" => search_next,               // FindNext
                "C-p" => search_prev,               // FindPrevious
                "F3" => search,                     // Find
                "F7" => search,                     // Find
                "C-z" => undo,                      // Undo
                "C-y" => redo,                      // Redo
                "C-c" => yank_to_clipboard,         // Copy
                "C-x" => cut_to_clipboard,          // Cut
                "C-v" => paste_clipboard_before,    // Paste
                "C-k" => kill_whole_line,           // CutLine
                "C-d" => duplicate_selection_down,  // Duplicate
                "C-a" => select_all,                // SelectAll
                "C-e" => command_mode,              // CommandMode
                "C-w" => rotate_view,               // NextSplit
                "C-u" => record_macro,              // ToggleMacro
                "C-j" => replay_macro,              // PlayMacro
                "ins" => overwrite_mode,            // ToggleOverwriteMode

                // Alt chords: word motion, line ends, paragraphs, tabs.
                "A-f" => move_next_word_start,      // WordRight
                "A-b" => move_prev_word_start,      // WordLeft
                "A-a" => goto_line_start,           // StartOfLine
                "A-e" => goto_line_end,             // EndOfLine
                "A-{" => goto_prev_paragraph,       // ParagraphPrevious
                "A-}" => goto_next_paragraph,       // ParagraphNext
                "A-," => goto_previous_buffer,      // PreviousTab
                "A-." => goto_next_buffer,          // NextTab

                // Multiple cursors — micro's own vocabulary.
                "A-n" => add_selection_to_next_match, // SpawnMultiCursor
                "A-c" => keep_primary_selection,      // RemoveAllMultiCursors
                "A-x" => remove_primary_selection,    // SkipMultiCursor
            })
        };
    }

    hashmap! {
        Mode::Insert => micro_keys!(),
        Mode::Normal => micro_keys!(),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn cmd(keys: &HashMap<Mode, KeyTrie>, mode: Mode, press: &str) -> Option<String> {
        let event: KeyEvent = press.parse().unwrap();
        match keys[&mode].search(&[event])? {
            KeyTrie::MappableCommand(cmd) => Some(cmd.name().to_string()),
            KeyTrie::Node(node) => Some(format!("node:{}", node.name)),
            KeyTrie::Sequence(_) => Some("sequence".to_string()),
        }
    }

    #[test]
    fn micro_chords_do_what_micro_does() {
        let keys = default();
        for (press, expected) in [
            ("C-f", "search"),
            ("C-z", "undo"),
            ("C-y", "redo"),
            ("C-a", "select_all"),
            ("C-e", "command_mode"),
            ("A-f", "move_next_word_start"),
            ("A-b", "move_prev_word_start"),
            ("A-n", "add_selection_to_next_match"),
            ("F3", "search"),
        ] {
            assert_eq!(
                cmd(&keys, Mode::Insert, press).as_deref(),
                Some(expected),
                "insert-mode {press}"
            );
        }
    }

    #[test]
    fn the_typable_chords_are_grafted_in_both_modes() {
        let keys = default();
        // Save and quit are `:` commands, so they arrive through the graft
        // rather than the macro — and micro's user presses them in either mode.
        for mode in [Mode::Insert, Mode::Normal] {
            assert_eq!(cmd(&keys, mode, "C-s").as_deref(), Some("write"), "{mode:?}");
            assert_eq!(cmd(&keys, mode, "C-q").as_deref(), Some("quit"), "{mode:?}");
            assert_eq!(cmd(&keys, mode, "F2").as_deref(), Some("write"), "{mode:?}");
        }
    }

    #[test]
    fn it_starts_modeless_like_micro() {
        assert_eq!(super::super::default_mode("micro"), Mode::Insert);
        assert_eq!(
            cmd(&default(), Mode::Insert, "C-v").as_deref(),
            Some("paste_clipboard_before")
        );
    }
}
