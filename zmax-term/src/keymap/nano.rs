//! The **nano** keymap.
//!
//! nano is modeless, so this preset starts from the emacs base (zmax's modeless
//! keymap) and lays nano's *classic* scheme over it — the one the help bar at
//! the bottom of nano advertises: `^O` write out, `^W` where is, `^K` cut,
//! `^U` paste, `^G` help, `^X` exit.
//!
//! nano's own `src/global.c` registers two schemes: the classic chords above and
//! the modern `^S`/`^F`/`^Q` set (`--modernbindings`). Both are in
//! `port/data/nano.json`, since both are in the source; the classic scheme is
//! what an unadorned `nano` gives you, so it is what this preset binds. The
//! modern chords that do not collide are bound as well, which is what nano does
//! when both are available.

use std::collections::HashMap;

use indexmap::IndexMap;

use super::macros::keymap;
use super::{emacs, merge_keys, KeyTrie, KeyTrieNode, MappableCommand, Mode};
use zmax_core::hashmap;
use zmax_view::input::KeyEvent;

/// nano chords whose function is a zmax `:` command rather than a static one.
#[rustfmt::skip]
const NANO_TYPABLE: &[(&str, &str, &str)] = &[
    ("C-o", "File",  ":write"),   // ^O write out
    ("C-x", "File",  ":quit"),    // ^X exit
    ("C-g", "Help",  ":help"),    // ^G help
    ("C-r", "File",  ":read"),    // ^R insert another file
    ("A-r", "Edit",  ":substitute"), // M-R replace
    ("C-t", "Shell", ":run-shell-command"), // ^T execute
];

pub fn default() -> HashMap<Mode, KeyTrie> {
    let mut keys = emacs::default();
    merge_keys(&mut keys, overrides());
    for mode in [Mode::Insert, Mode::Normal] {
        if let Some(KeyTrie::Node(root)) = keys.get_mut(&mode) {
            for (chord, label, cmd) in NANO_TYPABLE {
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
    s.split(' ')
        .map(|k| k.parse().expect("valid key"))
        .collect()
}

fn overrides() -> HashMap<Mode, KeyTrie> {
    macro_rules! nano_keys {
        () => {
            keymap!({ "nano"
                // The classic help-bar row.
                "C-w" => search,                    // ^W where is
                "C-q" => rsearch,                   // ^Q where was (backwards)
                "C-k" => kill_to_line_end,          // ^K cut
                "C-u" => paste_clipboard_before,    // ^U paste
                "C-j" => format_selections,         // ^J justify
                "C-c" => what_cursor_position,      // ^C location
                "C-_" => goto_line,                 // ^_ go to line
                "C-y" => page_up,                   // ^Y previous page
                "C-v" => page_down,                 // ^V next page
                "C-p" => move_visual_line_up,       // ^P previous line
                "C-n" => move_visual_line_down,     // ^N next line
                "C-a" => goto_line_start,           // ^A beginning of line
                "C-e" => goto_line_end,             // ^E end of line
                "C-d" => delete_char_forward,       // ^D delete
                "C-h" => delete_char_backward,      // ^H backspace
                "C-b" => move_char_left,            // ^B back
                "C-f" => move_char_right,           // ^F forward

                // Meta chords nano's second help row advertises.
                "A-u" => undo,                      // M-U undo
                "A-e" => redo,                      // M-E redo
                "A-6" => yank_to_clipboard,         // M-6 copy
                "A-a" => select_mode,               // M-A set mark
                "A-w" => search_next,               // M-W find next
                "A-g" => goto_line,                 // M-G go to line
                "A-\\" => goto_file_start,          // M-\ first line
                "A-/" => goto_last_line,            // M-/ last line
                "A-]" => match_brackets,            // M-] to matching bracket
                "A-3" => toggle_comments,           // M-3 comment/uncomment
                "A-d" => count_words_region,        // M-D word count
                "A-x" => flyspell_buffer,           // spell check
            })
        };
    }

    hashmap! {
        Mode::Insert => nano_keys!(),
        Mode::Normal => nano_keys!(),
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
    fn the_help_bar_chords_do_what_the_help_bar_says() {
        let keys = default();
        for (press, expected) in [
            ("C-w", "search"),                 // ^W Where Is
            ("C-k", "kill_to_line_end"),       // ^K Cut
            ("C-u", "paste_clipboard_before"), // ^U Paste
            ("C-j", "format_selections"),      // ^J Justify
            ("C-y", "page_up"),                // ^Y Prev Page
            ("C-v", "page_down"),              // ^V Next Page
            ("A-u", "undo"),                   // M-U Undo
            ("A-e", "redo"),                   // M-E Redo
        ] {
            assert_eq!(
                cmd(&keys, Mode::Insert, press).as_deref(),
                Some(expected),
                "insert-mode {press}"
            );
        }
    }

    #[test]
    fn write_out_read_and_exit_are_grafted_typables() {
        let keys = default();
        for mode in [Mode::Insert, Mode::Normal] {
            assert_eq!(
                cmd(&keys, mode, "C-o").as_deref(),
                Some("write"),
                "{mode:?}"
            );
            assert_eq!(cmd(&keys, mode, "C-x").as_deref(), Some("quit"), "{mode:?}");
            assert_eq!(cmd(&keys, mode, "C-r").as_deref(), Some("read"), "{mode:?}");
        }
    }

    #[test]
    fn it_starts_modeless_like_nano() {
        assert_eq!(super::super::default_mode("nano"), Mode::Insert);
    }
}
