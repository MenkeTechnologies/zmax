//! Emacs keymap.
//!
//! Emacs is modeless — you are always "inserting" — but zmax runs on a modal
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
use zmax_core::hashmap;
use zmax_view::input::KeyEvent;

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
    // Editing verbs that only exist as typable (`:`) commands.
    ("C-t",     "Edit",   ":transpose-chars"),        // transpose-chars
    ("A-t",     "Edit",   ":transpose-words"),        // M-t: transpose-words
    ("A-\\",    "Edit",   ":delete-horizontal-space"),// M-\: delete-horizontal-space
    ("A-space", "Edit",   ":just-one-space"),         // M-SPC: just-one-space
    ("C-A-o",   "Edit",   ":split-line"),             // C-M-o: split-line
    ("C-x C-o", "Edit",   ":delete-blank-lines"),     // C-x C-o: delete-blank-lines
    ("C-x r t", "Rect",   ":string-rectangle"),       // C-x r t: string-rectangle
    ("C-x r N", "Rect",   ":number-lines"),           // C-x r N: rectangle-number-lines
    ("C-x z",   "Edit",   ":repeat"),                 // C-x z: repeat last command
    // GUD. Emacs binds `C-c C-d` (gud-remove) in the debugger's source buffer;
    // this preset has no other claim on `C-c C-d`, unlike the spacemacs default
    // where it is the debug-launch key (see keymap/spacemacs.rs, where the GUD
    // map lives on the `C-x C-a` alias for exactly that reason).
    ("C-c C-d", "Debug (GUD)", "dap_remove_breakpoint"), // gud-remove
    // Tab bar. `tab-bar-select-tab-modifiers` names the modifier the digit keys
    // carry; Control is the one that is free here — Meta digits are emacs's
    // `digit-argument`, which zmax reads in `EditorView::handle_prefix_key`, and
    // `tab-bar-select-tab-modifiers` can move the digits onto Meta at runtime.
    // 1–8 select that tab, 9 selects the last one and 0 the most recent, exactly
    // as `tab-bar.el` binds them.
    ("C-1",     "Tab",    ":tab-switch 1"),           // C-1: tab-select
    ("C-2",     "Tab",    ":tab-switch 2"),
    ("C-3",     "Tab",    ":tab-switch 3"),
    ("C-4",     "Tab",    ":tab-switch 4"),
    ("C-5",     "Tab",    ":tab-switch 5"),
    ("C-6",     "Tab",    ":tab-switch 6"),
    ("C-7",     "Tab",    ":tab-switch 7"),
    ("C-8",     "Tab",    ":tab-switch 8"),
    ("C-9",     "Tab",    ":tablast"),                // C-9: tab-last
    ("C-0",     "Tab",    "tab_recent"),              // C-0: tab-recent
    // Frames (`C-x 5`), the same map the spacemacs preset overlays.
    ("C-x 5 .", "Frame",  "xref_find_definitions_other_frame"), // C-x 5 .: xref-find-definitions-other-frame
    ("C-x 5 0", "Frame",  "delete_frame"),            // C-x 5 0: delete-frame
    ("C-x 5 1", "Frame",  "delete_other_frames"),     // C-x 5 1: delete-other-frames
    ("C-x 5 2", "Frame",  "make_frame_command"),      // C-x 5 2: make-frame-command
    ("C-x 5 b", "Frame",  "switch_to_buffer_other_frame"), // C-x 5 b
    ("C-x 5 d", "Frame",  "dired_other_frame"),       // C-x 5 d: dired-other-frame
    ("C-x 5 f", "Frame",  "find_file_other_frame"),   // C-x 5 f: find-file-other-frame
    ("C-x 5 o", "Frame",  "other_frame"),             // C-x 5 o: other-frame
    ("C-x 5 r", "Frame",  "find_file_read_only_other_frame"), // C-x 5 r
    ("C-x 5 u", "Frame",  "undelete_frame"),          // C-x 5 u: undelete-frame
    // The frame's own window, under a window system.
    ("F11",     "Frame",  "toggle_frame_fullscreen"), // F11: toggle-frame-fullscreen
    ("A-F10",   "Frame",  "toggle_frame_maximized"),  // M-F10: toggle-frame-maximized
    ("C-z",     "Frame",  "iconify_or_deiconify_frame"), // C-z (X): iconify-or-deiconify-frame
];

fn add_typables(mode: &mut KeyTrie) {
    if let KeyTrie::Node(root) = mode {
        for (keys, label, cmd) in EMACS_TYPABLE {
            add_command(root, &chord(keys), label, cmd);
        }
    }
}

/// Graft emacs's `help-command` map onto `C-h` **and** `F1`.
///
/// `lisp/help.el` binds all three of `C-h` (`help-char`), `<help>` and `[f1]` in
/// `global-map` to the *same* keymap object:
///
/// ```elisp
/// (define-key global-map (char-to-string help-char) 'help-command)
/// (define-key global-map [help] 'help-command)
/// (define-key global-map [f1] 'help-command)
/// (fset 'help-command help-map)
/// ```
///
/// so `F1 k` and `C-h k` are one binding, not two — which is why the node is
/// built once and cloned onto `F1`. (`<help>` has no zmax key event, so that
/// alias is dropped.) The map itself is [`spacemacs::ch_prefix`], the same
/// curated `help-map` port the spacemacs preset overlays; the `C-h *` rows of
/// [`spacemacs::CXCH_FULL`] (`C-h a`, `C-h d`, `C-h 4 i`, `C-h 4 s`, the
/// `C-h C-*` GNU-documentation keys) go down first so the curated bindings win
/// on collision, exactly as they do there.
fn add_help_map(mode: &mut KeyTrie) {
    if let Some(root) = mode.node_mut() {
        for (chord, label, cmd) in super::spacemacs::CXCH_FULL
            .iter()
            .filter(|(chord, _, _)| chord.starts_with("C-h "))
        {
            super::spacemacs::add_chord(root, chord, label, cmd);
        }
    }
    mode.merge_nodes(super::spacemacs::ch_prefix());
    let help_key: KeyEvent = "C-h".parse().expect("valid key");
    let f1: KeyEvent = "F1".parse().expect("valid key");
    let Some(help_map) = mode.search(&[help_key]).cloned() else {
        return;
    };
    if let Some(root) = mode.node_mut() {
        root.insert(f1, help_map);
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
        "A-m" => goto_first_nonwhitespace,  // M-m: back-to-indentation
        "A-<" => goto_file_start,           // M-<: beginning-of-buffer
        "A->" => goto_last_line,            // M->: end-of-buffer
        "A-{" => goto_prev_paragraph,       // M-{: backward-paragraph
        "A-}" => goto_next_paragraph,       // M-}: forward-paragraph
        "C-A-a" => goto_prev_function,      // C-M-a: beginning-of-defun
        "C-A-e" => goto_next_function,      // C-M-e: end-of-defun
        "C-v" => page_down,
        "A-v" => page_up,
        "C-l" => align_view_center,         // recenter
        // M-g prefix: goto-line and next/previous-error.
        "A-g" => { "Goto"
            "g" => goto_line,               // M-g g / M-g M-g: goto-line (count-prefixed)
            "A-g" => goto_line,
            "n" => goto_next_diag,          // M-g n: next-error
            "A-n" => goto_next_diag,
            "p" => goto_prev_diag,          // M-g p: previous-error
            "A-p" => goto_prev_diag,
        },
        // Xref: find-definition / find-references / pop-marker.
        "A-." => goto_definition,           // M-.: xref-find-definitions
        "A-," => jump_backward,             // M-,: xref-pop-marker-stack
        "A-?" => goto_reference,            // M-?: xref-find-references
        // <left>/<right> are left-char/right-char, not backward-char/forward-char:
        // they move by *screen* direction, so in a right-to-left paragraph they
        // move the other way through the buffer (Emacs manual, "Bidirectional
        // Editing"). In left-to-right text they are backward-char/forward-char.
        "left" => left_char,                // <left>: left-char
        "right" => right_char,              // <right>: right-char
        "up" => move_visual_line_up,
        "down" => move_visual_line_down,
        "home" => goto_line_start,
        "end" => goto_line_end,
        "pageup" => page_up,
        "pagedown" => page_down,

        // mark / region
        "C-space" => set_mark_command,      // set-mark-command (pushes mark ring)
        "C-g" => collapse_selection,        // keyboard-quit
        // On MS-DOS C-Break is an extra quit character equivalent to C-g. The
        // Pause/Break key with Control reaches us as C-pause; dispatch it to
        // keyboard-quit so the DOS quit key behaves like C-g everywhere.
        "C-pause" => collapse_selection,    // C-Break (MS-DOS): keyboard-quit

        // editing
        "C-d" | "del" => delete_char_forward,
        // Only <backspace> deletes: `help-char` is C-h and `global-map` binds it
        // to `help-command` (lisp/help.el, `(define-key global-map (char-to-string
        // help-char) 'help-command)`), so C-h is the help prefix here — see
        // `add_help_map` below. C-h-as-DEL is what a user opts into with
        // `(keyboard-translate ?\C-h ?\C-?)`, it is not the stock binding.
        "backspace" => delete_char_backward,
        "C-k" => kill_to_line_end,          // kill-line
        "A-d" => delete_word_forward,       // M-d: kill-word
        "A-backspace" | "C-w" => delete_word_backward, // C-w/M-DEL approx (no region: kill prev word)
        "A-w" => [yank, collapse_selection],// M-w: kill-ring-save (copy)
        "C-y" => yank_from_kill_ring,       // C-y: yank latest kill-ring entry
        "A-y" => yank_pop,                  // M-y: yank-pop, cycle to older kill
        // C-u is emacs's universal-argument, not a kill (the old binding was a
        // readline-ism: emacs has no C-u kill). A bare C-u means 4, each further C-u
        // multiplies by 4, digits after it replace the number outright. The digits
        // and `M-1`…`M-9` / `M--` that continue an argument are read by
        // `EditorView::handle_prefix_key`; this key is what starts one.
        "C-u" => universal_argument,        // C-u: universal-argument
        "C-_" | "C-/" => undo,              // undo
        // M-/ is dabbrev-expand: expand the word before point from the words already
        // in the buffer, cycling on a repeat press. `completion` (the old binding) is
        // the LSP completion popup, a different command.
        "A-/" => dabbrev_expand,            // M-/: dabbrev-expand
        // M-TAB is complete-symbol: complete the symbol before point. (On
        // MS-Windows the WM grabs M-TAB, so emacs users reach it via ESC TAB /
        // C-M-i, but the underlying binding is this.) `completion` fires the
        // completion popup, emacs's completion-at-point machinery.
        "A-tab" => completion,              // M-TAB: complete-symbol
        "C-q" => quoted_insert,             // C-q: quoted-insert (next key inserts literally)
        "ret" | "C-j" => insert_newline,
        "tab" => emmet_expand,
        "C-o" => picture_open_line,         // C-o: open-line (split the line at point)
        "A-;" => toggle_comments,           // M-;: comment-dwim
        "A-^" => join_selections,           // M-^: delete-indentation (join line)
        "A-q" => reflow_selections,         // M-q: fill-paragraph
        "A-c" => capitalize_word,           // M-c: capitalize-word
        "A-u" => upcase_word,               // M-u: upcase-word
        "A-l" => downcase_word,             // M-l: downcase-word
        "A-z" => zap_to_char,               // M-z: zap-to-char
        "A-h" => mark_paragraph,            // M-h: mark-paragraph
        "C-A-backspace" => delete_word_backward, // C-M-DEL: backward-kill-word (approx)
        // indent-region modifies the buffer, which sets `deactivate-mark`, so
        // the region ends here. `indent` keeps the selection now (for repeated
        // `>` in the vim/helix presets), so this spells the exit out.
        "C-A-\\" => [indent, exit_select_mode], // C-M-\: indent-region

        // The menu bar. Both keys are emacs's own: F10 walks the menu with the
        // keyboard, M-` flattens the same tree into one list. They live only in
        // this keymap — in the vim base F10 is the debugger's step-over.
        "F10" => menu_bar_open,             // F10: menu-bar-open
        // S-F10 is the context menu (the one `down-mouse-3` pops up), at point.
        "S-F10" => context_menu_open,       // S-F10: context-menu-open
        "A-`" => tmm_menubar,               // M-`: tmm-menubar (the text menu bar)

        // commands / search / files / buffers
        "A-x" => command_palette,           // M-x: execute-extended-command
        "A-X" => command_palette,           // M-X / M-S-x: execute-extended-command-for-buffer
        "C-s" => search,                    // isearch-forward (approx)
        "C-r" => rsearch,                   // isearch-backward (approx)
        "C-A-s" => search,                  // C-M-s: isearch-forward-regexp
        "C-A-r" => rsearch,                 // C-M-r: isearch-backward-regexp
        // Query replace. The global M-% / C-M-% prompt for BOTH sides and ask at
        // every match — that is `query_replace` / `query_replace_regexp`. (The
        // isearch_* variants, which take the last search pattern as the "from" side,
        // are emacs's *inside-isearch* M-%, a different command; they used to sit
        // here for want of the real global ports.)
        "A-%" => query_replace,             // M-%: query-replace
        "C-A-%" => query_replace_regexp,    // C-M-%: query-replace-regexp
        "A-r" => move_to_window_line_top_bottom, // M-r: move-to-window-line-top-bottom
        "A-&" => async_shell_command,       // M-&: async-shell-command
        "C-A-," => jump_forward,            // C-M-,: xref-go-forward
        "C-A-l" => reposition_window,       // C-M-l: reposition-window
        // M-i pads to the next TAB STOP (a column), not by one indent unit —
        // `insert_tab`, the old binding, is what TAB does. The stop list is the
        // one `M-x edit-tab-stops` edits; empty means every `tab-width` columns.
        "A-i" => tab_to_tab_stop,           // M-i: tab-to-tab-stop
        // Quitting: C-] abandons one recursive editing level (or one modal
        // overlay, zmax's stand-in for minibuffer input) — it was
        // keyboard_escape_quit, which never touched the recursive stack, so no
        // key aborted a level. ESC ESC ESC is emacs's keyboard-escape-quit (get
        // out of whatever state point is in) and keeps that command.
        "C-]" => abort_recursive_edit,      // C-]: abort-recursive-edit
        "esc" => { "ESC"
            "esc" => { "ESC ESC"
                "esc" => keyboard_escape_quit, // ESC ESC ESC: keyboard-escape-quit
            },
        },
        "C-x" => { "C-x"
            "u" => undo,                    // C-x u: undo
            "C-f" => file_picker,           // find-file
            "C-v" => find_file_replace_buffer, // C-x C-v: find-alternate-file
            "C-r" => find_file_read_only,   // C-x C-r: find-file-read-only
            "b" => buffer_picker,           // switch-to-buffer
            "C-b" => list_buffers,          // C-x C-b: list-buffers (the Buffer Menu)
            "d" => dired,                   // C-x d: dired
            "C-j" => dired_jump,            // C-x C-j: dired-jump
            "space" => rectangle_mark_mode, // C-x SPC: rectangle-mark-mode
            "o" => rotate_view,             // other-window
            "1" => wonly,                   // delete-other-windows
            "0" => wclose,                  // delete-window
            "2" => hsplit,                  // split-window-below
            "3" => vsplit,                  // split-window-right
            "4" => { "Other window"
                // find-file-other-window is bound to both `f` and `C-f`; it
                // reuses an existing other window and splits only when this is
                // the sole one.
                "f" => find_file_other_window,   // C-x 4 f: find-file-other-window
                "C-f" => find_file_other_window, // C-x 4 C-f: find-file-other-window
                "b" => switch_to_buffer_other_window, // C-x 4 b: switch-to-buffer-other-window
                "0" => delete_window_and_buffer, // C-x 4 0: kill-buffer-and-window
                "." => xref_find_definitions_other_window, // C-x 4 .: xref-find-definitions-other-window
            },
            // The `C-x 5` frame map is grafted from EMACS_TYPABLE above.
            "t" => { "Other tab"
                "f" => find_file_other_tab,     // C-x t f: find-file-other-tab
                // The FFAP manual lists the other-tab visit under `C-x t C-f`.
                "C-f" => find_file_other_tab,   // C-x t C-f: ffap-other-tab
                "b" => switch_to_buffer_other_tab, // C-x t b: switch-to-buffer-other-tab
                // tab-bar.el: "Move the current tab ARG positions to the right",
                // wrapping — not vim's `:tabmove`, which goes to the last position.
                "m" => tab_move_right,          // C-x t m: tab-move
            },
            "}" => resize_view_wider,       // C-x }: enlarge-window-horizontally
            "{" => resize_view_narrower,    // C-x {: shrink-window-horizontally
            "^" => resize_view_taller,      // C-x ^: enlarge-window
            "+" => resize_view_equalize,    // C-x +: balance-windows
            "right" => goto_next_buffer,    // C-x <right>: next-buffer
            "left" => goto_previous_buffer, // C-x <left>: previous-buffer
            "C-;" => toggle_comments,       // C-x C-;: comment-line
            "C-space" => pop_to_mark,       // C-x C-SPC: pop-to-mark
            "C-x" => flip_selections,       // C-x C-x: exchange-point-and-mark
            "C-t" => transpose_line,        // C-x C-t: transpose-lines
            "h" => select_all,              // C-x h: mark-whole-buffer
            "C-l" => switch_to_lowercase,   // C-x C-l: downcase-region
            "C-u" => switch_to_uppercase,   // C-x C-u: upcase-region
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
            // Keyboard-macro definition. Both are emacs global bindings and
            // neither was reachable in this preset. `kmacro_start_macro` is not
            // the `record_macro` toggle: a second C-x ( errors with "Already
            // defining kbd macro", and C-u C-x ( appends to the last macro.
            "(" => kmacro_start_macro,      // C-x (: kmacro-start-macro
            ")" => kmacro_end_macro,        // C-x ): kmacro-end-macro
            "'" => expand_abbrev,           // C-x ': expand-abbrev
            "a" => { "Abbrev"
                "g" => define_abbrev,       // C-x a g: add-global-abbrev
                "l" => add_mode_abbrev,     // C-x a l: add-mode-abbrev
                "i" => { "Inverse abbrev"
                    "g" => inverse_add_global_abbrev, // C-x a i g: inverse-add-global-abbrev
                    "l" => inverse_add_mode_abbrev,   // C-x a i l: inverse-add-mode-abbrev
                },
            },
            // Input methods: C-x \ turns one on for a single character, and
            // C-x RET C-\ selects the one this buffer composes with.
            "\\" => activate_transient_input_method, // C-x \: activate-transient-input-method
            "ret" => { "Coding / input methods"
                "C-\\" => set_input_method, // C-x RET C-\: set-input-method
            },
        },
        // C-\: toggle-input-method — the key you press to start (and stop)
        // typing through the buffer's input method. Pressed twice between two
        // characters it also stops them combining.
        "C-\\" => toggle_input_method,
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
        "A-m" => extend_to_first_nonwhitespace, // M-m: back-to-indentation (extend)
        "A-<" => goto_file_start,
        "A->" => goto_last_line,
        "left" => extend_char_left,
        "right" => extend_char_right,
        "up" => extend_visual_line_up,
        "down" => extend_visual_line_down,
        // C-SPC C-SPC: set the mark, pushing it onto the mark ring, without
        // leaving it active. `set_mark_command` does the push (it drops a
        // consecutive duplicate, so the doubled chord adds one entry, not two)
        // and the rest deactivates the region.
        "C-space" => [set_mark_command, collapse_selection, normal_mode, insert_mode],
        "C-w" => [delete_selection, normal_mode, insert_mode], // kill-region, back to inserting
        "A-w" => [yank, collapse_selection, normal_mode, insert_mode], // copy-region
        "C-g" => [collapse_selection, normal_mode, insert_mode],       // keyboard-quit
        "C-pause" => [collapse_selection, normal_mode, insert_mode],   // C-Break (MS-DOS): keyboard-quit
        "esc" => [collapse_selection, normal_mode, insert_mode],
    });

    // Normal mode is rarely used in emacs; keep movement working and offer an
    // escape hatch back to inserting. `i`/`a` and most chords re-enter insert.
    let mut normal = keymap!({ "Normal mode"
        "i" | "a" => insert_mode,
        "C-f" => move_char_right,           // C-f: forward-char (logical)
        "C-b" => move_char_left,            // C-b: backward-char (logical)
        // The arrow keys are the *screen*-direction pair (left-char/right-char).
        "right" => right_char,
        "left"  => left_char,
        "C-\\" => toggle_input_method,      // C-\: toggle-input-method
        "C-n" | "down"  => move_visual_line_down,
        "C-p" | "up"    => move_visual_line_up,
        "C-a" | "home"  => goto_line_start,
        "C-e" | "end"   => goto_line_end,
        "A-f" => move_next_word_end,
        "A-b" => move_prev_word_start,
        "A-m" => goto_first_nonwhitespace,  // M-m: back-to-indentation
        "A-<" => goto_file_start,
        "A->" => goto_last_line,
        "A-{" => goto_prev_paragraph,       // M-{: backward-paragraph
        "A-}" => goto_next_paragraph,       // M-}: forward-paragraph
        "C-A-a" => goto_prev_function,      // C-M-a: beginning-of-defun
        "C-A-e" => goto_next_function,      // C-M-e: end-of-defun
        "A-." => goto_definition,           // M-.: xref-find-definitions
        "A-," => jump_backward,             // M-,: xref-pop-marker-stack
        "A-?" => goto_reference,            // M-?: xref-find-references
        "A-c" => capitalize_word,           // M-c: capitalize-word
        "A-u" => upcase_word,               // M-u: upcase-word
        "A-l" => downcase_word,             // M-l: downcase-word
        "A-z" => zap_to_char,               // M-z: zap-to-char
        "A-h" => mark_paragraph,            // M-h: mark-paragraph
        "A-;" => toggle_comments,           // M-;: comment-dwim
        "A-q" => reflow_selections,         // M-q: fill-paragraph
        "A-g" => { "Goto"
            "g" => goto_line,               // M-g g: goto-line
            "A-g" => goto_line,
            "n" => goto_next_diag,          // M-g n: next-error
            "p" => goto_prev_diag,          // M-g p: previous-error
        },
        "C-v" | "pagedown" => page_down,
        "A-v" | "pageup"   => page_up,
        "C-space" => select_mode,
        "C-g" => collapse_selection,
        "C-pause" => collapse_selection,    // C-Break (MS-DOS): keyboard-quit
        "A-tab" => completion,              // M-TAB: complete-symbol
        "C-u" => universal_argument,        // C-u: universal-argument
        "C-d" => delete_char_forward,
        "C-k" => kill_to_line_end,
        "C-_" | "C-/" => undo,
        "A-/" => dabbrev_expand,            // M-/: dabbrev-expand
        "A-r" => move_to_window_line_top_bottom, // M-r: move-to-window-line-top-bottom
        "C-y" => yank_from_kill_ring,
        "A-y" => yank_pop,
        "A-x" => command_palette,           // M-x: execute-extended-command
        "F10" => menu_bar_open,             // F10: menu-bar-open
        "A-`" => tmm_menubar,               // M-`: tmm-menubar
        "C-s" => search,
        "C-r" => rsearch,
        "C-A-s" => search,                  // C-M-s: isearch-forward-regexp
        "C-A-r" => rsearch,                 // C-M-r: isearch-backward-regexp
        "C-x" => { "C-x"
            "u" => undo,
            "C-f" => file_picker,
            "b" => buffer_picker,
            "C-b" => list_buffers,          // C-x C-b: list-buffers (the Buffer Menu)
            "o" => rotate_view,
            "1" => wonly,
            "0" => wclose,
            "2" => hsplit,
            "3" => vsplit,
            "}" => resize_view_wider,       // C-x }: enlarge-window-horizontally
            "{" => resize_view_narrower,    // C-x {: shrink-window-horizontally
            "^" => resize_view_taller,      // C-x ^: enlarge-window
            "+" => resize_view_equalize,    // C-x +: balance-windows
            "right" => goto_next_buffer,    // C-x <right>: next-buffer
            "left" => goto_previous_buffer, // C-x <left>: previous-buffer
            "h" => select_all,              // C-x h: mark-whole-buffer
            "C-x" => flip_selections,       // C-x C-x: exchange-point-and-mark
        },
    });

    add_typables(&mut insert);
    add_typables(&mut normal);
    // Region kill/copy in select mode also wants C-x save etc.
    add_typables(&mut select);

    // Emacs is modeless, so its help prefix is live wherever the user is —
    // including with a region active (asking for help does not end the region).
    add_help_map(&mut insert);
    add_help_map(&mut normal);
    add_help_map(&mut select);

    hashmap!(
        Mode::Normal => normal,
        Mode::Select => select,
        Mode::Insert => insert,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search<'a>(km: &'a HashMap<Mode, KeyTrie>, mode: Mode, chord: &str) -> Option<&'a KeyTrie> {
        let keys: Vec<KeyEvent> = chord.split(' ').map(|k| k.parse().unwrap()).collect();
        km[&mode].search(&keys)
    }
    fn cmd(km: &HashMap<Mode, KeyTrie>, mode: Mode, chord: &str) -> Option<String> {
        match search(km, mode, chord) {
            Some(KeyTrie::MappableCommand(c)) => Some(c.name().to_string()),
            _ => None,
        }
    }

    /// `global-map` binds C-h (`help-char`), `[help]` and `[f1]` to the one
    /// `help-command` keymap, so the help prefix is live in every mode of this
    /// modeless preset and F1 reaches the same map. The keys checked here are
    /// `help-map`'s own (lisp/help.el): k describe-key, f describe-function,
    /// p finder-by-keyword, P describe-package, a apropos-command, 4 s
    /// help-find-source.
    #[test]
    fn help_map_lives_on_c_h_and_f1() {
        let km = default();
        for mode in [Mode::Insert, Mode::Normal, Mode::Select] {
            for (chord, want) in [
                ("C-h k", "describe_key"),
                ("C-h f", "describe_function"),
                ("C-h p", "finder_by_keyword"),
                ("C-h P", "describe_package"),
                ("C-h a", "apropos-command"),
                ("C-h 4 s", "help_find_source"),
                ("C-h 4 i", "info_search_other_window"),
            ] {
                assert_eq!(
                    cmd(&km, mode, chord).as_deref(),
                    Some(want),
                    "{chord} must be {want} in {mode}"
                );
                // F1 IS help-command — the same map, not a second one.
                let via_f1 = chord.replacen("C-h", "F1", 1);
                assert_eq!(
                    cmd(&km, mode, &via_f1).as_deref(),
                    Some(want),
                    "{via_f1} must reach the same binding as {chord}"
                );
            }
            // Both spellings of help-for-help, from inside the map.
            assert_eq!(cmd(&km, mode, "C-h C-h").as_deref(), Some("help"));
            assert_eq!(cmd(&km, mode, "F1 F1").as_deref(), Some("help"));
        }
    }

    /// C-h is the help prefix, so it is no longer a second spelling of
    /// backspace — but <backspace> itself must still delete backward, which is
    /// the half of the old `"backspace" | "C-h"` arm that emacs really binds.
    #[test]
    fn backspace_still_deletes_after_c_h_became_help() {
        let km = default();
        assert_eq!(
            cmd(&km, Mode::Insert, "backspace").as_deref(),
            Some("delete_char_backward")
        );
        assert!(
            matches!(search(&km, Mode::Insert, "C-h"), Some(KeyTrie::Node(_))),
            "C-h is help-command, a prefix, not delete_char_backward"
        );
    }
}
