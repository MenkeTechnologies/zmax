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
            "C-c" => run_active_config,          // C-c C-c: execute / compile (major-mode action)
            "C-r" => rerun_last_run,             // C-c C-r: re-run
            "C-d" => dap_launch,                 // C-c C-d: debug (DAP session for the chosen config)
            ";" => complete_current_statement,   // C-c ;: complete current statement (JetBrains Cmd-Shift-Enter); works in insert mode
            "." => postfix_expand,               // C-c .: postfix completion (expr.if/.for/... ; works in insert mode)
        },
    })
}

/// The Emacs `C-h` help prefix, routed to zemacs's help / discovery commands.
#[rustfmt::skip]
fn ch_prefix() -> KeyTrie {
    keymap!({ "overlay"
        "C-h" => { "Help"
            "C-h" => help,                        // help-for-help
            "?" => help,                          // help-for-help
            "f" => describe_command,              // describe-function
            "x" => describe_command,              // describe-command
            "a" => describe_command,              // apropos-command
            "d" => describe_command,              // apropos-documentation
            "o" => describe_command,              // describe-symbol
            "k" => describe_key,                  // describe-key
            "c" => describe_key,                  // describe-key-briefly
            "w" => where_is,                      // where-is
            "b" => describe_bindings,             // describe-bindings
            "v" => config_variable_search,        // describe-variable
            "m" => describe_current_modes,        // describe-mode
            "s" => describe_syntax,               // describe-syntax
            "C" => describe_coding_system,        // describe-coding-system
            "L" => describe_language_environment, // describe-language-environment
            "l" => view_lossage,                  // view-lossage
            "p" => package_search,                // finder-by-keyword
            "P" => package_search,                // describe-package
            "." => hover,                         // display-local-help (LSP hover)
            "i" => info_search,                   // info
            "S" => man_page_search,               // info-lookup-symbol
            // Manual / info / tutorial navigation → the zemacs help browser.
            "r" => help,                          // info-emacs-manual
            "F" => help,                          // Info-goto-emacs-command-node
            "K" => help,                          // Info-goto-emacs-key-command-node
            "t" => help,                          // help-with-tutorial
            "n" => help,                          // view-emacs-news
            "g" => help,                          // describe-gnu-project
            "h" => help,                          // view-hello-file
            "e" => help,                          // view-echo-area-messages
            "I" => help,                          // describe-input-method
        },
    })
}

/// The complete remaining Emacs `C-x` / `C-c` / `C-h` chords not covered by the
/// curated prefixes above, each mapped to the nearest zemacs command (generated
/// from the Emacs Key Index). `command_palette` is the fallback where zemacs has
/// no analogue. Format: (chord, submap-label, command).
#[rustfmt::skip]
const CXCH_FULL: &[(&str, &str, &str)] = &[
    ("C-c , j", "C-c ,", "goto_definition"),
    ("C-c , J", "C-c ,", "workspace_symbol_picker"),
    ("C-c , l", "C-c ,", "symbol_picker"),
    ("C-c , space", "C-c ,", "completion"),
    ("C-c @ C-c", "Outline", "fold_toggle"),
    ("C-c @ C-h", "Outline", "fold_close"),
    ("C-c @ C-l", "Outline", "fold_close_recursive"),
    ("C-c @ C-r", "Outline", "fold_open"),
    ("C-c @ C-s", "Outline", "fold_open"),
    ("C-c C-x", "C-c C-x", "fold_close"),
    ("C-c C-z", "C-c C-z", "fold_open"),
    ("C-h .", "C-h .", "hover"),
    ("C-h 4 i", "Other window", "info_search"),
    ("C-h 4 s", "Other window", "help"),
    ("C-h C", "C-h C", "help"),
    ("C-h C-c", "C-h C-c", "help"),
    ("C-h C-d", "C-h C-d", "help"),
    ("C-h C-e", "C-h C-e", "package_search"),
    ("C-h C-f", "C-h C-f", "browse_faq"),
    ("C-h C-m", "C-h C-m", "help"),
    ("C-h C-n", "C-h C-n", "browse_news"),
    ("C-h C-o", "C-h C-o", "help"),
    ("C-h C-p", "C-h C-p", "browse_faq"),
    ("C-h C-q", "C-h C-q", "help"),
    ("C-h C-t", "C-h C-t", "help"),
    ("C-h C-w", "C-h C-w", "help"),
    ("C-h g", "C-h g", "help"),
    ("C-h I", "C-h I", "unicode_picker"),
    ("C-x #", "C-x #", "command_palette"),
    ("C-x $", "C-x $", "fold_close_all"),
    ("C-x )", "C-x )", "command_palette"),
    ("C-x +", "C-x +", "resize_view_equalize"),
    ("C-x -", "C-x -", "resize_view_shorter"),
    ("C-x .", "C-x .", "command_palette"),
    ("C-x 4", "Other window", "hsplit"),
    ("C-x 4 4", "Other window", "hsplit"),
    ("C-x 4 a", "Other window", "command_palette"),
    ("C-x 4 c", "Other window", "clone_indirect_buffer"),
    ("C-x 4 C-j", "Other window", "file_explorer"),
    ("C-x 4 C-o", "Other window", "buffer_picker"),
    ("C-x 4 d", "Other window", "file_explorer"),
    ("C-x 4 m", "Other window", "command_palette"),
    ("C-x 5", "Frame", "vsplit"),
    ("C-x 5 .", "Frame", "goto_definition"),
    ("C-x 5 0", "Frame", "wclose"),
    ("C-x 5 1", "Frame", "wonly"),
    ("C-x 5 2", "Frame", "vsplit"),
    ("C-x 5 5", "Frame", "vsplit"),
    ("C-x 5 b", "Frame", "buffer_picker"),
    ("C-x 5 c", "Frame", "vsplit"),
    ("C-x 5 d", "Frame", "file_explorer"),
    ("C-x 5 f", "Frame", "file_picker"),
    ("C-x 5 m", "Frame", "command_palette"),
    ("C-x 5 o", "Frame", "rotate_view"),
    ("C-x 5 r", "Frame", "file_picker"),
    ("C-x 5 u", "Frame", "reopen_last_closed"),
    ("C-x 6 1", "Two-column", "wonly"),
    ("C-x 6 2", "Two-column", "vsplit"),
    ("C-x 6 b", "Two-column", "buffer_picker"),
    ("C-x 6 d", "Two-column", "wclose"),
    ("C-x 6 ret", "Two-column", "insert_newline"),
    ("C-x 6 s", "Two-column", "vsplit"),
    ("C-x 8", "Unicode", "unicode_picker"),
    ("C-x 8 e", "Unicode", "unicode_picker"),
    ("C-x 8 ret", "Unicode", "unicode_picker"),
    ("C-x ;", "C-x ;", "command_palette"),
    ("C-x <", "C-x <", "scroll_half_column_right"),
    ("C-x >", "C-x >", "scroll_half_column_left"),
    ("C-x a i g", "Abbrev", "define_abbrev"),
    ("C-x a i l", "Abbrev", "define_abbrev"),
    ("C-x a l", "Abbrev", "define_abbrev"),
    ("C-x C-+", "C-x C-+", "command_palette"),
    ("C-x C--", "C-x C--", "command_palette"),
    ("C-x C-0", "C-x C-0", "command_palette"),
    ("C-x C-=", "C-x C-=", "command_palette"),
    ("C-x C-a C-b", "C-x C-a", "command_palette"),
    ("C-x C-e", "C-x C-e", "eval_elisp_line"),
    ("C-x C-k b", "Macro", "kmacro_to_register"),
    ("C-x C-k C-a", "Macro", "kmacro_add_counter"),
    ("C-x C-k C-c", "Macro", "kmacro_add_counter"),
    ("C-x C-k C-e", "Macro", "kmacro_ring_view"),
    ("C-x C-k C-f", "Macro", "kmacro_add_counter"),
    ("C-x C-k C-i", "Macro", "kmacro_insert_counter"),
    ("C-x C-k C-k", "Macro", "kmacro_ring_next"),
    ("C-x C-k C-n", "Macro", "kmacro_ring_next"),
    ("C-x C-k C-p", "Macro", "kmacro_ring_prev"),
    ("C-x C-k d", "Macro", "kmacro_ring_delete"),
    ("C-x C-k e", "Macro", "kmacro_ring_view"),
    ("C-x C-k l", "Macro", "kmacro_ring_view"),
    ("C-x C-k n", "Macro", "kmacro_to_register"),
    ("C-x C-k r", "Macro", "command_palette"),
    ("C-x C-k ret", "Macro", "kmacro_ring_view"),
    ("C-x C-k space", "Macro", "kmacro_ring_view"),
    ("C-x C-k x", "Macro", "kmacro_to_register"),
    ("C-x C-n", "C-x C-n", "command_palette"),
    ("C-x C-o", "C-x C-o", "command_palette"),
    ("C-x C-p", "C-x C-p", "select_all"),
    ("C-x C-space", "C-x C-space", "pop_to_mark"),
    ("C-x C-t", "C-x C-t", "drag_line_down"),
    ("C-x C-z", "C-x C-z", "command_palette"),
    ("C-x backspace", "C-x backspace", "delete_word_backward"),
    ("C-x e", "C-x e", "command_palette"),
    ("C-x esc esc", "C-x esc", "command_history_picker"),
    ("C-x f", "C-x f", "toggle_fill_column"),
    ("C-x i", "C-x i", "command_palette"),
    ("C-x l", "C-x l", "document_stats"),
    ("C-x m", "C-x m", "command_palette"),
    ("C-x q", "C-x q", "command_palette"),
    ("C-x r f", "Registers", "layout_create"),
    ("C-x r M", "Registers", "bookmark_set"),
    ("C-x r A-w", "Registers", "copy_rectangle_as_kill"),
    ("C-x r N", "Registers", "command_palette"),
    ("C-x r o", "Registers", "clear_rectangle"),
    ("C-x r r", "Registers", "copy_rectangle_as_kill"),
    ("C-x r s", "Registers", "point_to_register"),
    ("C-x r t", "Registers", "clear_rectangle"),
    ("C-x r w", "Registers", "layout_create"),
    ("C-x ret", "Coding", "command_palette"),
    ("C-x ret c", "Coding", "command_palette"),
    ("C-x ret f", "Coding", "command_palette"),
    ("C-x ret F", "Coding", "command_palette"),
    ("C-x ret k", "Coding", "command_palette"),
    ("C-x ret p", "Coding", "command_palette"),
    ("C-x ret r", "Coding", "command_palette"),
    ("C-x ret t", "Coding", "command_palette"),
    ("C-x ret x", "Coding", "command_palette"),
    ("C-x ret X", "Coding", "command_palette"),
    ("C-x t", "Tab", "new_tab"),
    ("C-x t 0", "Tab", "close_tab"),
    ("C-x t 1", "Tab", "tab_only"),
    ("C-x t 2", "Tab", "new_tab"),
    ("C-x t b", "Tab", "buffer_picker"),
    ("C-x t d", "Tab", "file_explorer"),
    ("C-x t f", "Tab", "file_picker"),
    ("C-x t m", "Tab", "move_to_opposite_group"),
    ("C-x t o", "Tab", "goto_next_tabpage"),
    ("C-x t r", "Tab", "command_palette"),
    ("C-x t ret", "Tab", "goto_next_tabpage"),
    ("C-x t t", "Tab", "goto_next_tabpage"),
    ("C-x v !", "VCS", "git_status"),
    ("C-x v +", "VCS", "git_pull"),
    ("C-x v =", "VCS", "git_diff"),
    ("C-x v a", "VCS", "git_file_log_picker"),
    ("C-x v b c", "VCS", "git_branch_picker"),
    ("C-x v b l", "VCS", "git_repo_log_picker"),
    ("C-x v b s", "VCS", "git_branch_picker"),
    ("C-x v D", "VCS", "git_diff"),
    ("C-x v d", "VCS", "git_status"),
    ("C-x v g", "VCS", "git_blame_line"),
    ("C-x v G", "VCS", "git_file_dispatch"),
    ("C-x v h", "VCS", "git_file_log_picker"),
    ("C-x v i", "VCS", "git_file_dispatch"),
    ("C-x v I", "VCS", "git_repo_log_picker"),
    ("C-x v l", "VCS", "git_file_log_picker"),
    ("C-x v L", "VCS", "git_repo_log_picker"),
    ("C-x v O", "VCS", "git_repo_log_picker"),
    ("C-x v P", "VCS", "git_push"),
    ("C-x v r", "VCS", "git_branch_picker"),
    ("C-x v s", "VCS", "git_branch_picker"),
    ("C-x v u", "VCS", "git_file_dispatch"),
    ("C-x v v", "VCS", "git_status"),
    ("C-x v ~", "VCS", "view_file_at_rev"),
    ("C-x w .", "Highlight", "toggle_auto_highlight"),
    ("C-x w b", "Highlight", "toggle_auto_highlight"),
    ("C-x w d", "Highlight", "buffer_picker"),
    ("C-x w h", "Highlight", "select_regex"),
    ("C-x w i", "Highlight", "select_regex"),
    ("C-x w l", "Highlight", "select_regex"),
    ("C-x w p", "Highlight", "select_regex"),
    ("C-x w r", "Highlight", "clear_search_highlight"),
    ("C-x x g", "C-x x", "command_palette"),
    ("C-x x i", "C-x x", "buffer_picker"),
    ("C-x x r", "C-x x", "command_palette"),
    ("C-x x t", "C-x x", "toggle_soft_wrap"),
    ("C-x x u", "C-x x", "command_palette"),
    ("C-x z", "C-x z", "repeat_last_motion"),
    ("C-x [", "C-x [", "page_up"),
    ("C-x ]", "C-x ]", "page_down"),
    ("C-x ^", "C-x ^", "resize_view_taller"),
    ("C-x `", "C-x `", "run_next_error"),
    ("C-x }", "C-x }", "resize_view_wider"),
    ("C-x *", "C-x *", "command_palette"),
];

/// Add a full space-separated `chord` -> `cmd` binding, creating intermediate
/// submaps labelled `label`. A chord with a key zemacs can't parse is skipped
/// (rather than panicking), so unrepresentable Emacs chords are simply omitted.
fn add_chord(root: &mut KeyTrieNode, chord: &str, label: &str, cmd: &str) {
    let mut path = Vec::new();
    for tok in chord.split(' ') {
        match tok.parse::<KeyEvent>() {
            Ok(ev) => path.push(ev),
            Err(_) => return,
        }
    }
    if !path.is_empty() {
        add_command(root, &path, label, cmd);
    }
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
        // completion, C-h backspace, decrement, …) so the Emacs prefixes replace
        // them cleanly instead of recursively merging into a hybrid node.
        if let Some(node) = trie.node_mut() {
            for k in &prefix_keys {
                node.shift_remove(k);
            }
        }
        // 1. Lay down the full remaining Emacs C-x/C-c/C-h map first (approximate
        //    where zemacs has no faithful analogue), so every documented chord is
        //    bound in every mode.
        if let Some(node) = trie.node_mut() {
            for (chord, label, cmd) in CXCH_FULL {
                add_chord(node, chord, label, cmd);
            }
        }
        // 2. Overlay the curated prefixes on top. `merge_nodes` recurses into the
        //    C-x/C-c/C-h nodes, so the hand-written real bindings WIN over the
        //    generated fallbacks on any collision, while the non-colliding
        //    fallbacks survive.
        trie.merge_nodes(cx_prefix());
        trie.merge_nodes(cc_prefix());
        trie.merge_nodes(ch_prefix());
        // 3. Graft the typable `C-x` bindings (save / write-all / quit / kill-buffer)
        //    the `keymap!` macro can't express, under the C-x node.
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

    #[test]
    fn complete_statement_bound_under_cc_in_insert() {
        let km = default();
        // JetBrains "Complete Current Statement" lives under the Emacs major-mode
        // prefix C-c and must fire while typing → active in Insert (and Normal).
        for mode in [Mode::Insert, Mode::Normal, Mode::Select] {
            assert_eq!(
                cmd(&km, mode, "C-c ;").as_deref(),
                Some("complete_current_statement"),
                "C-c ; must complete the statement in {mode}"
            );
        }
        // …without clobbering the existing C-c C-c major-mode action.
        assert_eq!(
            cmd(&km, Mode::Insert, "C-c C-c").as_deref(),
            Some("run_active_config")
        );
    }

    #[test]
    fn ch_help_maps_to_real_distinct_functions() {
        let km = default();
        // The curated real help functions must win over the generated fallbacks.
        for (chord, want) in [
            ("C-h f", "describe_command"),
            ("C-h x", "describe_command"),
            ("C-h a", "describe_command"),
            ("C-h k", "describe_key"),
            ("C-h w", "where_is"),
            ("C-h b", "describe_bindings"),
            ("C-h v", "config_variable_search"),
            ("C-h m", "describe_current_modes"),
            ("C-h s", "describe_syntax"),
            ("C-h C", "describe_coding_system"),
            ("C-h L", "describe_language_environment"),
            ("C-h l", "view_lossage"),
            ("C-h p", "package_search"),
            ("C-h .", "hover"),
        ] {
            assert_eq!(
                cmd(&km, Mode::Normal, chord).as_deref(),
                Some(want),
                "{chord} should map to {want}"
            );
        }
        // A non-colliding generated fallback still survives under C-h.
        assert_eq!(cmd(&km, Mode::Normal, "C-h C-f").as_deref(), Some("browse_faq"));
    }
}
