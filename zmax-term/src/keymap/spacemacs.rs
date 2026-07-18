//! The **spacemacs** keymap — zmax's default preset.
//!
//! Spacemacs is evil-mode (vim keys) with the `SPC` leader *and* the standard
//! Emacs command prefixes `C-x`, `C-c` and `C-h`. This keymap is the shared vim
//! base (which already carries the `SPC` leader) with those three Emacs prefixes
//! overlaid onto Normal and Select modes (replacing vim's `C-x = decrement` and
//! `C-h = move-left` there). Pressing `C-x` / `C-c` / `C-h` opens a which-key
//! popup of the Emacs map, exactly like Spacemacs. Bindings resolve to the
//! nearest zmax command; Emacs features with no zmax analogue (Calc, frames,
//! VC, coding-system menus, keyboard macros) are intentionally left unbound.
//!
//! The `vim` preset is the same base with the `SPC` leader stripped instead (see
//! [`super::vim::default`]); `decrement` stays reachable via `g C-x` in both.

use std::collections::HashMap;

use indexmap::IndexMap;

use super::macros::keymap;
use super::{vim, KeyTrie, KeyTrieNode, MappableCommand, Mode};
use zmax_core::hashmap;
use zmax_view::input::KeyEvent;

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
    ("C-o", "Edit",   ":delete-blank-lines"), // C-x C-o: delete-blank-lines
    // C-x z (`repeat`) is NOT `:repeat` — that typable repeats the *selected text*
    // N times, which is a different command entirely. C-x z falls through to
    // `repeat_last_motion` in CXCH_FULL below (the closest zmax analogue).
    ("r t", "Rect",   ":string-rectangle"),// C-x r t: string-rectangle
];

/// Insert `cmd` at `path` under `root`, creating intermediate submap nodes
/// labelled `label` as needed. Mirrors the helper in the emacs keymap.
pub(super) fn add_command(root: &mut KeyTrieNode, path: &[KeyEvent], label: &str, cmd: &str) {
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
            // find-alternate-file loads the chosen file *in place of* this buffer,
            // closing it — `file_picker` (the old binding) opened it alongside.
            "C-v" => find_file_replace_buffer, // C-x C-v: find-alternate-file
            "C-r" => file_picker,           // C-x C-r: find-file-read-only
            "C-q" => toggle_readonly,       // C-x C-q: read-only-mode
            "b" => buffer_picker,           // C-x b: switch-to-buffer
            // C-x C-b is list-buffers: the *Buffer List* — the Buffer Menu, with its
            // own keys (select / mark / delete / save). `buffer_picker` (the old
            // binding) is C-x b's fuzzy switcher, a different command.
            "C-b" => list_buffers,          // C-x C-b: list-buffers
            "left" => goto_previous_buffer, // C-x <left>: previous-buffer
            "right" => goto_next_buffer,    // C-x <right>: next-buffer
            // C-x d is `dired` — the Dired directory editor itself, not a fuzzy file
            // picker seeded with the directory (which is what it used to run).
            "d" => dired,                   // C-x d: dired
            "C-d" => file_picker_in_current_directory,      // C-x C-d: list-directory
            // C-x C-j is dired-jump: Dired on the current buffer's directory, with
            // the real Dired overlay (it used to open a file picker there).
            "C-j" => dired_jump,            // C-x C-j: dired-jump
            "o" => rotate_view,             // C-x o: other-window
            "1" => wonly,                   // C-x 1: delete-other-windows
            "0" => wclose,                  // C-x 0: delete-window
            "2" => hsplit,                  // C-x 2: split-window-below
            "3" => vsplit,                  // C-x 3: split-window-right
            "{" => resize_view_narrower,    // C-x {: shrink-window-horizontally
            "}" => resize_view_wider,       // C-x }: enlarge-window-horizontally
            "^" => resize_view_taller,      // C-x ^: enlarge-window
            "+" => resize_view_equalize,    // C-x +: balance-windows
            "C-t" => transpose_line,        // C-x C-t: transpose-lines
            "4" => { "Other window"
                // find-file-other-window: the file *picker* (open a chosen file), not
                // `goto_file` (open the path under the cursor — a different command).
                "f" => find_file_other_window, // C-x 4 f: find-file-other-window
                // switch-to-buffer-other-window really splits and shows the buffer in
                // the other window; `buffer_picker` (the old binding) is C-x b.
                "b" => switch_to_buffer_other_window, // C-x 4 b: switch-to-buffer-other-window
                // kill-buffer-and-window closes the window *and* kills the buffer in
                // it; `wclose` alone left the buffer open.
                "0" => delete_window_and_buffer, // C-x 4 0: kill-buffer-and-window
                "." => xref_find_definitions_other_window, // C-x 4 .: xref-find-definitions-other-window
            },
            "C-space" => pop_to_mark,       // C-x C-SPC: pop-to-mark
            "C-x" => flip_selections,       // C-x C-x: exchange-point-and-mark
            // rectangle-mark-mode is its own command (it makes the *region*
            // rectangular); `visual_block_mode` was the nearest vim mode switch.
            "space" => rectangle_mark_mode, // C-x SPC: rectangle-mark-mode
            "h" => select_all,              // C-x h: mark-whole-buffer
            "C-l" => switch_to_lowercase,   // C-x C-l: downcase-region
            "C-u" => switch_to_uppercase,   // C-x C-u: upcase-region
            "C-;" => toggle_comments,       // C-x C-;: comment-line
            // C-x TAB is indent-rigidly: shift the region N columns *as a block*,
            // preserving relative indentation. `indent` (the old binding) re-indents
            // each line by the language's rules, which is a different command.
            "tab" => indent_rigidly,        // C-x TAB: indent-rigidly
            "=" => what_cursor_position,    // C-x =: what-cursor-position
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
                "l" => list_bookmarks,           // C-x r l: list-bookmarks
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
            ";" => complete_current_statement,   // C-c ;: complete current statement (JetBrains Complete Current Statement); works in insert mode
            "." => postfix_expand,               // C-c .: postfix completion (expr.if/.for/... ; works in insert mode)
        },
    })
}

/// The Emacs `C-h` help prefix, routed to zmax's help / discovery commands.
#[rustfmt::skip]
fn ch_prefix() -> KeyTrie {
    keymap!({ "overlay"
        "C-h" => { "Help"
            "C-h" => help,                        // help-for-help
            "?" => help,                          // help-for-help
            // C-h f is describe-function and C-h x describe-command — two commands in
            // emacs, and two here now that `describe_function` exists.
            "f" => describe_function,             // describe-function
            "x" => describe_command,              // describe-command
            // C-h a (apropos-command) and C-h d (apropos-documentation) are the two
            // *apropos* keys: they search the command set / every doc string for a
            // pattern and list every hit. Both are typable commands, which this macro
            // cannot express, so they are bound in CXCH_FULL below — and must NOT be
            // listed here, or this map would shadow them with `describe_command`
            // (which describes one chosen command, i.e. C-h f, not an apropos search).
            "k" => describe_key,                  // describe-key
            // C-h c (describe-key-briefly) and C-h o (describe-symbol) have real
            // ports; they are bound in CXCH_FULL below (this map would shadow them).
            "w" => where_is,                      // where-is
            "b" => describe_bindings,             // describe-bindings
            "v" => describe_variable,             // describe-variable
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
            // Manual / info / tutorial navigation → the zmax help browser.
            "r" => help,                          // info-emacs-manual
            "F" => help,                          // Info-goto-emacs-command-node
            "K" => help,                          // Info-goto-emacs-key-command-node
            // C-h t (help-with-tutorial) is the `:tutor` typable, which this macro
            // cannot express — it is bound in CXCH_FULL above.
            "n" => browse_news,                   // view-emacs-news
            "g" => describe_gnu_project,          // describe-gnu-project
            "h" => view_hello_file,               // view-hello-file
            "e" => view_echo_area_messages,       // view-echo-area-messages
            "I" => help,                          // describe-input-method
        },
    })
}

/// The complete remaining Emacs `C-x` / `C-c` / `C-h` chords not covered by the
/// curated prefixes above, each mapped to the nearest zmax command (generated
/// from the Emacs Key Index). `command_palette` is the fallback where zmax has
/// no analogue. Format: (chord, submap-label, command).
///
/// Invariant: never give a *prefix* chord (`C-x 4`, `C-x 5`, `C-x 8`, `C-x t`,
/// `C-x RET`) a command of its own. [`add_command`] cannot descend through a
/// leaf, so a leaf on a prefix silently swallows every chord beneath it — that
/// bug is what kept the whole `C-x 5`, `C-x t` and `C-x RET` maps unreachable.
/// In Emacs these keys are prefixes with no command, which is what they are here.
#[rustfmt::skip]
const CXCH_FULL: &[(&str, &str, &str)] = &[
    ("C-c , j", "C-c ,", "goto_definition"),
    ("C-c , J", "C-c ,", "workspace_symbol_picker"),
    // semantic-analyze-possible-completions: the completion list for the symbol at
    // point (Emacs shows it in another window), not the document-symbol picker.
    ("C-c , l", "C-c ,", "completion"),
    ("C-c , space", "C-c ,", "completion"),
    // `C-c @` is the prefix BOTH hideshow (hs-minor-mode) and Outline minor mode
    // hang their maps on in Emacs — the two collide there as well, and only the
    // enabled minor mode's binding is reachable. zmax has one map, so the four
    // chords they both claim (C-c @ C-c, C-c @ C-h, C-c @ C-l, C-c @ C-s) stay on
    // hideshow, and the rest of the prefix is the outline-mode-prefix-map: the
    // Outline-mode command set, reachable in *any* buffer (zmax has no outline
    // *major* mode — a major mode here is a document language, and no language is
    // "outline").
    ("C-c @ C-c", "Outline", "fold_toggle"),
    ("C-c @ C-h", "Outline", "fold_close"),
    // hs-hide-level: fold every block at a named nesting depth — the real port,
    // not "close this fold and its children" (fold_close_recursive).
    ("C-c @ C-l", "Outline", "hs_hide_level"),
    ("C-c @ C-r", "Outline", "fold_open"),
    ("C-c @ C-s", "Outline", "fold_open"),
    ("C-c @ A-C-h", "Outline", "fold_close_all"), // C-c @ C-M-h: hs-hide-all
    ("C-c @ A-C-s", "Outline", "fold_open_all"),  // C-c @ C-M-s: hs-show-all
    // Outline motion (Outline-mode's C-c C-n / C-p / C-f / C-b / C-u).
    ("C-c @ C-n", "Outline", "outline_next_visible_heading"),     // outline-next-visible-heading
    ("C-c @ C-p", "Outline", "outline_previous_visible_heading"), // outline-previous-visible-heading
    ("C-c @ C-f", "Outline", "outline_forward_same_level"),       // outline-forward-same-level
    ("C-c @ C-b", "Outline", "outline_backward_same_level"),      // outline-backward-same-level
    ("C-c @ C-u", "Outline", "outline_up_heading"),               // outline-up-heading
    // Outline visibility (Outline-mode's C-c C-a / C-t / C-d / C-e / C-k / C-i /
    // C-q / C-o). outline-hide-entry, -show-subtree and -hide-leaves have no slot:
    // hideshow owns C-c @ C-c, C-c @ C-s and C-c @ C-l above.
    ("C-c @ C-a", "Outline", "outline_show_all"),      // outline-show-all
    ("C-c @ C-t", "Outline", "outline_hide_body"),     // outline-hide-body
    ("C-c @ C-d", "Outline", "outline_hide_subtree"),  // outline-hide-subtree
    ("C-c @ C-e", "Outline", "outline_show_entry"),    // outline-show-entry
    ("C-c @ C-k", "Outline", "outline_show_branches"), // outline-show-branches
    ("C-c @ C-i", "Outline", "outline_show_children"), // outline-show-children
    ("C-c @ C-q", "Outline", "outline_hide_sublevels"),// outline-hide-sublevels (count = level)
    ("C-c @ C-o", "Outline", "outline_hide_other"),    // outline-hide-other
    // Foldout (the outline zoom minor mode): `C-c C-z` narrows the buffer to the
    // subtree at point and `C-c C-x` widens back out to the heading it came from.
    // Both are real ports now — they used to be the nearest fold_open / fold_close,
    // which only toggle a fold's visibility and never narrow.
    ("C-c C-x", "Foldout", "foldout_exit_fold"),    // foldout-exit-fold
    ("C-c C-z", "Foldout", "foldout_zoom_subtree"), // foldout-zoom-subtree
    // GUD (the debugger keys emacs binds under C-c, alongside the C-x C-a global
    // map — C-x C-a C-b/gud-break is already below). Each runs the DAP command the
    // Emacs manual's "Commands of GUD" names for the chord. gud-cont (C-c C-r),
    // gud-print (C-c C-p) and gud-up/down (C-c < / >) are not here: C-c C-r is
    // already zmax's re-run, and there is no print-expression-at-point or
    // single-frame-up/down DAP command to bind the others to.
    ("C-c C-n", "Debug (GUD)", "dap_next"),          // gud-next: step over
    ("C-c C-s", "Debug (GUD)", "dap_step_in"),       // gud-step: step into
    ("C-c C-f", "Debug (GUD)", "dap_step_out"),      // gud-finish: run until this frame returns
    ("C-c C-u", "Debug (GUD)", "dap_run_to_cursor"), // gud-until: continue to the current line
    // The rest of the GUD map, each on the command the Emacs manual's "Commands of
    // GUD" names for the chord — these are the real `gud-*` ports, which did not
    // exist when the four above were bound. gud-cont (C-c C-r) and gud-remove
    // (C-c C-d) still have no slot: those two chords are zmax's re-run and
    // debug-launch, and a leaf cannot hold two commands.
    ("C-c C-p", "Debug (GUD)", "gud_print"),         // gud-print: print the expression at point
    ("C-c C-i", "Debug (GUD)", "gud_stepi"),         // gud-stepi: step one machine instruction
    ("C-c C-l", "Debug (GUD)", "gud_refresh"),       // gud-refresh: redisplay, scroll to newest output
    ("C-c <",   "Debug (GUD)", "gud_up"),            // gud-up:   select the frame one level up
    ("C-c >",   "Debug (GUD)", "gud_down"),          // gud-down: select the frame one level down
    // doc-view / image mode's C-c C-t: extract the document's text into a
    // buffer. (Its sibling C-c C-c — toggle text/image display — cannot be
    // bound: C-c C-c is the major-mode execute/compile action.)
    ("C-c C-t", "C-c C-t", ":doc-view-open-text"),
    // Term mode's three C-c keys (Emacs manual, "Term Mode"), each on its real
    // port. The terminal panel swallows every key while it is the focused pane
    // (that is what char mode *means*), so these run from the editor pane and act
    // on the open panel — `term_action` looks it up and says so when there is none.
    ("C-c C-j", "Term", "term_line_mode"),   // C-c C-j: term-line-mode
    ("C-c C-k", "Term", "term_char_mode"),   // C-c C-k: term-char-mode
    ("C-c C-q", "Term", "term_pager_toggle"),// C-c C-q: term-pager-toggle
    // goto-address-mode's one key: open the URL/e-mail at point in the browser.
    ("C-c ret", "Goto Address", "open_url_under_cursor"), // C-c RET: goto-address-at-point
    // C-h a / C-h d are the apropos keys — a *pattern search* over the command set
    // and over every doc string, listing every hit. They are typable commands, so
    // they live here rather than in `ch_prefix` (which cannot express a typable).
    ("C-h a", "Help", ":apropos-command"),        // C-h a: apropos-command
    ("C-h d", "Help", ":apropos-documentation"),  // C-h d: apropos-documentation
    ("C-h .", "C-h .", "hover"),
    ("C-h 4 i", "Other window", "info_search_other_window"),
    ("C-h 4 s", "Other window", "help"),
    ("C-h C", "C-h C", "help"),
    ("C-h c", "C-h c", "describe_key_briefly"),   // C-h c: describe-key-briefly
    ("C-h o", "C-h o", "describe_symbol"),        // C-h o: describe-symbol
    ("C-h C-c", "C-h C-c", "describe_copying"),
    ("C-h C-d", "C-h C-d", "describe_distribution"),
    ("C-h C-e", "C-h C-e", "view_external_packages"), // C-h C-e: view-external-packages
    ("C-h C-f", "C-h C-f", "view_emacs_faq"),
    ("C-h C-m", "C-h C-m", "view_order_manuals"), // C-h C-m: view-order-manuals
    ("C-h C-n", "C-h C-n", "browse_news"),
    ("C-h C-o", "C-h C-o", "describe_distribution"), // C-h C-o: describe-distribution
    ("C-h C-p", "C-h C-p", "view_emacs_problems"),
    ("C-h C-q", "C-h C-q", "help_quick_toggle"), // C-h C-q: help-quick-toggle (the cheat-sheet)
    ("C-h C-t", "C-h C-t", "view_emacs_todo"),    // C-h C-t: view-emacs-todo
    ("C-h C-w", "C-h C-w", "describe_no_warranty"),
    ("C-h g", "C-h g", "describe_gnu_project"),
    ("C-h I", "C-h I", "unicode_picker"),
    ("C-h t", "Help", ":tutor"),                          // C-h t: help-with-tutorial
    // C-x #: server-edit — tell the waiting `emacsclient` this buffer is done, so
    // the client's process returns. A real port now (there is an emacs server);
    // it used to be the command_palette fallback.
    ("C-x #", "C-x #", "server_edit"),
    // C-x $: set-selective-display (hide lines indented past a column), which has
    // a real port now — it is not "close every fold" (fold_close_all).
    ("C-x $", "C-x $", "set_selective_display"),
    // Basic keyboard macros. `record_macro` toggles recording, so it stands in for
    // kmacro-start-macro (C-x () — but C-x ) is kmacro-end-macro, which is its own
    // command now (it ends the definition and errors when none is being recorded).
    ("C-x (", "C-x (", "record_macro"),
    ("C-x )", "C-x )", "kmacro_end_macro"),   // C-x ): kmacro-end-macro
    ("C-x +", "C-x +", "resize_view_equalize"),
    // C-x - is shrink-window-if-larger-than-buffer: fit the window to its buffer's
    // text. `resize_view_shorter` (the old binding) just shrank it one step.
    ("C-x -", "C-x -", "shrink_window_if_larger_than_buffer"),
    ("C-x .", "C-x .", "set_fill_prefix"),               // C-x .: set-fill-prefix
    // C-x 4 4 is other-window-prefix: it does not split — it redirects the buffer
    // the NEXT command displays into another window. The real port exists now.
    ("C-x 4 4", "Other window", "other_window_prefix"),       // C-x 4 4: other-window-prefix
    ("C-x 4 a", "Other window", "add_change_log_entry_other_window"), // C-x 4 a: add-change-log-entry-other-window
    ("C-x 4 c", "Other window", "clone_indirect_buffer"),
    ("C-x 4 C-j", "Other window", "dired_jump_other_window"), // C-x 4 C-j: dired-jump-other-window
    // display-buffer shows a buffer in another window WITHOUT selecting it —
    // `buffer_picker` (the old binding) switches to it, which is C-x b.
    ("C-x 4 C-o", "Other window", "display_buffer"),          // C-x 4 C-o: display-buffer
    ("C-x 4 d", "Other window", "dired_other_window"),        // C-x 4 d: dired-other-window
    ("C-x 4 m", "Other window", ":compose-mail"),        // C-x 4 m: compose-mail-other-window (same window here)
    // The C-x 5 frame map. zmax has real frames now (`Editor::new_frame` /
    // `switch_frame` / `delete_frame`), so every chord runs the `*-frame` command
    // the Emacs manual names for it instead of the window-split stand-in it had to
    // sit on while there were no frames to make.
    // C-x 5 . (xref-find-definitions-other-frame) keeps its current-window port:
    // there is no other-frame variant of the xref jump. C-x 5 m has one
    // (`compose_mail_other_frame`) and runs it.
    ("C-x 5 .", "Frame", "goto_definition"),
    ("C-x 5 0", "Frame", "delete_frame"),                // C-x 5 0: delete-frame
    ("C-x 5 1", "Frame", "delete_other_frames"),         // C-x 5 1: delete-other-frames
    ("C-x 5 2", "Frame", "make_frame_command"),          // C-x 5 2: make-frame-command
    ("C-x 5 5", "Frame", "other_frame_prefix"),          // C-x 5 5: other-frame-prefix
    ("C-x 5 b", "Frame", "switch_to_buffer_other_frame"),// C-x 5 b: switch-to-buffer-other-frame
    ("C-x 5 c", "Frame", "clone_frame"),                 // C-x 5 c: clone-frame
    ("C-x 5 d", "Frame", "dired_other_frame"),           // C-x 5 d: dired-other-frame
    ("C-x 5 f", "Frame", "find_file_other_frame"),       // C-x 5 f: find-file-other-frame
    ("C-x 5 m", "Frame", "compose_mail_other_frame"),    // C-x 5 m: compose-mail-other-frame
    ("C-x 5 o", "Frame", "other_frame"),                 // C-x 5 o: other-frame
    ("C-x 5 r", "Frame", "find_file_read_only_other_frame"), // C-x 5 r: find-file-read-only-other-frame
    ("C-x 5 u", "Frame", "undelete_frame"),              // C-x 5 u: undelete-frame
    // Two-column mode (`C-x 6`, aliased to `F2`): the real 2C-* ports, not the
    // window-split approximations they used to be.
    ("C-x 6 1", "Two-column", "twocol_merge"),            // 2C-merge
    ("C-x 6 2", "Two-column", "twocol_two_columns"),      // 2C-two-columns
    ("C-x 6 b", "Two-column", "twocol_associate_buffer"), // 2C-associate-buffer
    ("C-x 6 d", "Two-column", "twocol_dissociate"),       // 2C-dissociate
    ("C-x 6 ret", "Two-column", "twocol_newline"),        // 2C-newline
    ("C-x 6 s", "Two-column", "twocol_split"),            // 2C-split
    ("C-x 8 e", "Unicode", "unicode_picker"),
    ("C-x 8 ret", "Unicode", "unicode_picker"),
    ("C-x ;", "C-x ;", "command_palette"),
    ("C-x <", "C-x <", "scroll_half_column_right"),
    ("C-x >", "C-x >", "scroll_half_column_left"),
    ("C-x a i g", "Abbrev", "inverse_add_global_abbrev"),
    // The `l` chords are the mode-local halves: they write the major mode's own
    // abbrev table (`emacs_abbrev::define_mode`), which the expansion lookup
    // consults before the global one. `C-x a g` keeps `define_abbrev` (global).
    ("C-x a i l", "Abbrev", "inverse_add_mode_abbrev"),
    ("C-x a l", "Abbrev", "add_mode_abbrev"),
    // Text scale (C-x C-+ / C-- / C-0 / C-=) and its global C-M- variants. `C--`
    // is not a legal key string (the parser wants `C-minus`), which is why the
    // old `C-x C--` entry silently never bound.
    ("C-x C-+", "Text scale", "text_scale_increase"),
    ("C-x C-=", "Text scale", "text_scale_increase"),
    ("C-x C-minus", "Text scale", "text_scale_decrease"),
    ("C-x C-0", "Text scale", "text_scale_reset"),
    ("C-x A-C-+", "Text scale", "text_scale_increase"),
    ("C-x A-C-=", "Text scale", "text_scale_increase"),
    ("C-x A-C-minus", "Text scale", "text_scale_decrease"),
    ("C-x A-C-0", "Text scale", "text_scale_reset"),
    // The GUD map. Emacs gives every GUD command TWO keys: a `C-c` one (live only
    // in the source buffer GUD is debugging) and a global `C-x C-a` alias — the
    // manual's "Commands of GUD" table lists both on one line. zmax binds the
    // `C-c` half where the chord is free (C-c C-n / C-s / C-f / C-u / C-p / C-i /
    // C-l / < / >, above), and the whole map here on the `C-x C-a` alias, which is
    // the only key the three chords whose `C-c` half is already taken can have:
    // C-c C-r is zmax's re-run, C-c C-d its debug-launch, C-c C-t doc-view's
    // open-text. Those three are reachable as C-x C-a C-r / C-d / C-t.
    ("C-x C-a C-b", "C-x C-a", "dap_toggle_breakpoint"), // C-x C-a C-b: gud-break (toggles here)
    ("C-x C-a C-j", "C-x C-a", "gud_jump"),              // C-x C-a C-j: gud-jump (set the execution point here)
    ("C-x C-a C-w", "C-x C-a", "gud_watch"),             // C-x C-a C-w: gud-watch (watch the expression at point)
    ("C-x C-a C-r", "C-x C-a", "dap_continue"),          // C-x C-a C-r: gud-cont
    ("C-x C-a C-t", "C-x C-a", "gud_tbreak"),            // C-x C-a C-t: gud-tbreak (temporary breakpoint)
    ("C-x C-a C-n", "C-x C-a", "dap_next"),              // C-x C-a C-n: gud-next
    ("C-x C-a C-s", "C-x C-a", "dap_step_in"),           // C-x C-a C-s: gud-step
    ("C-x C-a C-f", "C-x C-a", "dap_step_out"),          // C-x C-a C-f: gud-finish
    ("C-x C-a C-u", "C-x C-a", "dap_run_to_cursor"),     // C-x C-a C-u: gud-until
    ("C-x C-a C-p", "C-x C-a", "gud_print"),             // C-x C-a C-p: gud-print
    ("C-x C-a C-i", "C-x C-a", "gud_stepi"),             // C-x C-a C-i: gud-stepi
    ("C-x C-a C-l", "C-x C-a", "gud_refresh"),           // C-x C-a C-l: gud-refresh
    ("C-x C-a <", "C-x C-a", "gud_up"),                  // C-x C-a <: gud-up
    ("C-x C-a >", "C-x C-a", "gud_down"),                // C-x C-a >: gud-down
    ("C-x C-e", "C-x C-e", "eval_last_sexp"),
    // The C-x C-k keyboard-macro map, each chord on the command the Emacs manual
    // names for it (Keyboard-Macro-{Counter,Ring,Registers,Step-Edit},
    // {Basic,Edit,Save}-Keyboard-Macro). These used to be approximations because
    // the kmacro ports did not exist yet; they do now.
    ("C-x C-k b", "Macro", "kmacro_bind_to_key"),       // kmacro-bind-to-key
    ("C-x C-k C-a", "Macro", "kmacro_add_counter"),     // kmacro-add-counter
    ("C-x C-k C-c", "Macro", "kmacro_set_counter"),     // kmacro-set-counter
    ("C-x C-k C-e", "Macro", "kmacro_edit_macro"),      // kmacro-edit-macro-repeat
    ("C-x C-k C-f", "Macro", "kmacro_set_format"),      // kmacro-set-format
    ("C-x C-k C-i", "Macro", "kmacro_insert_counter"),  // kmacro-insert-counter
    ("C-x C-k C-k", "Macro", "kmacro_end_or_call_macro_repeat"), // kmacro-end-or-call-macro-repeat
    ("C-x C-k C-n", "Macro", "kmacro_ring_next"),       // kmacro-cycle-ring-next
    ("C-x C-k C-p", "Macro", "kmacro_ring_prev"),       // kmacro-cycle-ring-previous
    ("C-x C-k C-t", "Macro", "kmacro_ring_swap"),       // kmacro-swap-ring
    ("C-x C-k d", "Macro", "kmacro_ring_delete"),       // kmacro-delete-ring-head
    ("C-x C-k e", "Macro", "kmacro_edit_macro"),        // edit-kbd-macro
    ("C-x C-k l", "Macro", "kmacro_edit_lossage"),      // kmacro-edit-lossage
    ("C-x C-k n", "Macro", "kmacro_name_last_macro"),   // kmacro-name-last-macro
    ("C-x C-k r", "Macro", "apply_macro_to_region_lines"), // apply-macro-to-region-lines
    ("C-x C-k ret", "Macro", "kmacro_edit_macro"),      // kmacro-edit-macro
    ("C-x C-k space", "Macro", "kmacro_step_edit_macro"), // kmacro-step-edit-macro
    ("C-x C-k x", "Macro", "kmacro_to_register"),       // kmacro-to-register
    ("C-x C-n", "C-x C-n", "set_goal_column"),           // C-x C-n: set-goal-column
    ("C-x C-o", "C-x C-o", "command_palette"),
    ("C-x C-p", "C-x C-p", "mark_page"), // C-x C-p: mark-page (form-feed page, not the buffer)
    ("C-x C-space", "C-x C-space", "pop_to_mark"),
    ("C-x C-t", "C-x C-t", "drag_line_down"),
    ("C-x C-z", "C-x C-z", "suspend"),
    ("C-x backspace", "C-x backspace", "backward_kill_sentence"), // C-x DEL: backward-kill-sentence
    ("C-x e", "C-x e", "kmacro_end_or_call_macro"), // C-x e: kmacro-end-and-call-macro
    ("C-x esc esc", "C-x esc", "command_history_picker"),
    ("C-x f", "C-x f", ":set-fill-column"), // C-x f: set-fill-column (0 args = cursor column)
    ("C-x i", "C-x i", ":insert-file"), // C-x i: insert-file (prompts for the file)
    ("C-x l", "C-x l", "count_lines_page"), // C-x l: count-lines-page
    ("C-x m", "C-x m", ":compose-mail"),                 // C-x m: compose-mail
    ("C-x q", "C-x q", "command_palette"),
    // The configuration registers: both save the window layout *into a named
    // register* (restored with C-x r j), which is what the emacs commands do —
    // `layout_create`, the old binding, saved a layout but never into a register.
    ("C-x r f", "Registers", "frameset_to_register"),
    ("C-x r M", "Registers", "bookmark_set_no_overwrite"),
    ("C-x r A-w", "Registers", "copy_rectangle_as_kill"),
    // rectangle-number-lines: insert an incrementing number in front of each line
    // the region covers. `:number-lines` (0 args = start at 1) is the port; it
    // numbers the selected lines rather than the rectangle's left edge.
    ("C-x r N", "Registers", ":number-lines"),
    ("C-x r o", "Registers", "open_rectangle"),          // C-x r o: open-rectangle (insert blanks, shift text right)
    // The register commands, not the kill-ring ones: C-x r r copies the rectangle
    // *into a register*, C-x r s copies the region *into a register* (C-x r SPC is
    // point-to-register, bound in cx_prefix).
    ("C-x r r", "Registers", "copy_rectangle_to_register"),
    ("C-x r s", "Registers", "copy_to_register"),
    ("C-x r t", "Registers", "clear_rectangle"),
    ("C-x r w", "Registers", "window_configuration_to_register"),
    // The C-x RET coding-system map. Every chord runs the setter the Emacs manual
    // names for it — they were the command_palette fallback while no coding-system
    // command existed.
    ("C-x ret c", "Coding", "universal_coding_system_argument"),  // universal-coding-system-argument
    ("C-x ret f", "Coding", "set_buffer_file_coding_system"),     // set-buffer-file-coding-system
    ("C-x ret F", "Coding", "set_file_name_coding_system"),       // set-file-name-coding-system
    ("C-x ret k", "Coding", "set_keyboard_coding_system"),        // set-keyboard-coding-system
    ("C-x ret p", "Coding", "set_buffer_process_coding_system"),  // set-buffer-process-coding-system
    ("C-x ret r", "Coding", "revert_buffer_with_coding_system"),  // revert-buffer-with-coding-system
    ("C-x ret t", "Coding", "set_terminal_coding_system"),        // set-terminal-coding-system
    ("C-x ret x", "Coding", "set_selection_coding_system"),       // set-selection-coding-system
    ("C-x ret X", "Coding", "set_next_selection_coding_system"),  // set-next-selection-coding-system
    ("C-x t 0", "Tab", "close_tab"),
    ("C-x t 1", "Tab", "tab_only"),
    ("C-x t 2", "Tab", "new_tab"),
    // C-x t b / C-x t d / C-x t f are the *-other-tab commands: they show the chosen
    // buffer / directory / file in a NEW TAB. The plain picker (the old binding)
    // opened it in the current one, which is C-x b / C-x d / C-x C-f.
    ("C-x t b", "Tab", "switch_to_buffer_other_tab"),    // C-x t b: switch-to-buffer-other-tab
    ("C-x t d", "Tab", "dired_other_tab"),                // C-x t d: dired-other-tab
    ("C-x t f", "Tab", "find_file_other_tab"),           // C-x t f: find-file-other-tab
    // C-x t m is tab-move: it moves the *tab* inside the tab bar. It used to run
    // `move_to_opposite_group` — the JetBrains "move this editor to the other
    // split group" action, which moves a buffer between splits and never touches a
    // tab. `:tabmove` moves the tab (to the last position with no count; emacs
    // moves it one to the right).
    ("C-x t m", "Tab", ":tabmove"),                      // C-x t m: tab-move
    ("C-x t o", "Tab", "goto_next_tabpage"),
    ("C-x t r", "Tab", "tab_rename"),                    // C-x t r: tab-rename
    // C-x t RET is tab-switch: pick the tab by name. `tab_switch` prompts, which is
    // what the chord had been waiting for — it sat on "go to the next tab" until
    // the real port existed.
    ("C-x t ret", "Tab", "tab_switch"),                  // C-x t RET: tab-switch
    // C-x t t is other-tab-prefix: it redirects the buffer the NEXT command
    // displays into a new tab; it does not "go to the next tab" (that is C-x t o).
    ("C-x t t", "Tab", "other_tab_prefix"),              // C-x t t: other-tab-prefix
    // The `vc-*` ports exist, so every C-x v chord runs the command the Emacs manual
    // names for it instead of the nearest git_* approximation (a magit dispatch menu
    // is not `vc-revert`, and a branch picker is not `vc-create-tag`).
    ("C-x v !", "VCS", "git_status"),
    ("C-x v +", "VCS", "git_pull"),
    ("C-x v =", "VCS", "git_diff"),
    ("C-x v a", "VCS", "vc_update_change_log"), // vc-update-change-log
    ("C-x v b c", "VCS", "vc_create_branch"),   // vc-create-branch
    ("C-x v b l", "VCS", "vc_print_branch_log"),// vc-print-branch-log
    ("C-x v b s", "VCS", "vc_switch_branch"),   // vc-switch-branch
    ("C-x v c", "VCS", "git_acp"),
    ("C-x v D", "VCS", "vc_root_diff"),         // vc-root-diff (whole tree)
    ("C-x v d", "VCS", "git_status"),
    ("C-x v g", "VCS", "git_blame_line"),
    ("C-x v G", "VCS", "vc_ignore"),            // vc-ignore
    ("C-x v h", "VCS", "vc_region_history"),    // vc-region-history
    ("C-x v i", "VCS", "vc_register"),          // vc-register
    ("C-x v I", "VCS", "vc_log_incoming"),      // vc-log-incoming
    ("C-x v l", "VCS", "vc_print_log"),         // vc-print-log
    ("C-x v L", "VCS", "vc_print_root_log"),    // vc-print-root-log
    ("C-x v O", "VCS", "vc_log_outgoing"),      // vc-log-outgoing
    ("C-x v P", "VCS", "git_push"),
    ("C-x v r", "VCS", "vc_retrieve_tag"),      // vc-retrieve-tag
    ("C-x v s", "VCS", "vc_create_tag"),        // vc-create-tag
    ("C-x v u", "VCS", "vc_revert"),            // vc-revert
    ("C-x v v", "VCS", "vc_next_action"),       // vc-next-action
    ("C-x v ~", "VCS", "view_file_at_rev"),
    // hi-lock (C-x w): the real highlight-* / hi-lock-* ports. The three that read a
    // regexp now prompt for it when given no argument, so they are bindable.
    ("C-x w .", "Highlight", ":highlight-symbol-at-point"), // highlight-symbol-at-point
    ("C-x w b", "Highlight", ":hi-lock-write-interactive-patterns"),
    // C-x w d is toggle-window-dedicated (Displaying Buffers), not a hi-lock chord:
    // Emacs 29 moved hi-lock to M-s h and reused C-x w for window commands.
    ("C-x w d", "Highlight", "toggle_window_dedication"),
    ("C-x w h", "Highlight", ":highlight-regexp"), // highlight-regexp
    ("C-x w i", "Highlight", ":hi-lock-find-patterns"),
    ("C-x w l", "Highlight", ":highlight-lines-matching-regexp"),
    ("C-x w p", "Highlight", ":highlight-phrase"),
    ("C-x w r", "Highlight", ":unhighlight-regexp"),        // unhighlight-regexp
    // revert-buffer-quick is its own command (revert with no confirmation prompt);
    // `:reload` is the vim `:e!` verb it used to stand on.
    ("C-x x g", "C-x x", "revert_buffer_quick"),         // C-x x g: revert-buffer-quick
    // C-x x i is insert-buffer (paste another buffer's text here), not a buffer
    // *switch*; C-x x r is rename-buffer, which prompts for the new name now, so
    // it no longer has to sit on the command_palette fallback.
    ("C-x x i", "C-x x", ":insert-buffer"),              // C-x x i: insert-buffer
    ("C-x x r", "C-x x", ":rename-buffer"),              // C-x x r: rename-buffer
    ("C-x x t", "C-x x", "toggle_soft_wrap"),
    ("C-x x u", "C-x x", ":rename-uniquely"),            // C-x x u: rename-uniquely
    ("C-x z", "C-x z", "repeat_last_motion"),
    // C-x [ / C-x ] move over form-feed *pages*, they do not scroll a screenful.
    ("C-x [", "C-x [", "backward_page"),
    ("C-x ]", "C-x ]", "forward_page"),
    ("C-x ^", "C-x ^", "resize_view_taller"),
    ("C-x `", "C-x `", "run_next_error"),
    ("C-x }", "C-x }", "resize_view_wider"),
    ("C-x *", "C-x *", "command_palette"),
];

/// Add a full space-separated `chord` -> `cmd` binding, creating intermediate
/// submaps labelled `label`. A chord with a key zmax can't parse is skipped
/// (rather than panicking), so unrepresentable Emacs chords are simply omitted.
pub(super) fn add_chord(root: &mut KeyTrieNode, chord: &str, label: &str, cmd: &str) {
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
        //    where zmax has no faithful analogue), so every documented chord is
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
            for (keys, label, cmd) in CX_TYPABLE {
                // Keys may be a multi-chord path relative to C-x (e.g. "r t"),
                // so split on spaces into a KeyEvent path like the emacs keymap.
                let path: Vec<KeyEvent> = keys
                    .split(' ')
                    .map(|k| k.parse::<KeyEvent>().expect("valid key"))
                    .collect();
                add_command(cx, &path, label, cmd);
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

    // The shipped keymap (`keymap::default` re-exports this one) must keep vim's
    // whole `z` fold family reachable — the Emacs `C-x`/`C-c`/`C-h` overlay must
    // not shadow or drop any of it.
    #[test]
    fn shipped_keymap_keeps_the_vim_fold_family() {
        let km = default();
        for (chord, want) in [
            ("z M", "fold_close_all"),
            ("z R", "fold_open_all"),
            // zm/zr step the foldlevel by one; only zM/zR go all the way.
            ("z m", "fold_more"),
            ("z r", "fold_less"),
            ("z a", "fold_toggle"),
            ("z o", "fold_open"),
            ("z c", "fold_close"),
        ] {
            assert_eq!(
                cmd(&km, Mode::Normal, chord).as_deref(),
                Some(want),
                "{chord} must be {want} in the shipped keymap"
            );
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
            // C-h f is describe-function, C-h x describe-command. The two chords used
            // to share `describe_command` for want of a `describe_function` port; it
            // exists now, so C-h f runs it and this expectation moved with the binding.
            ("C-h f", "describe_function"),
            ("C-h x", "describe_command"),
            // C-h a / C-h d are apropos, not describe: they take a PATTERN and list
            // every command / doc string that matches. Both used to run
            // `describe_command` (describe one chosen command — that is C-h f) for
            // want of an apropos port; the typables exist now, so the two chords run
            // them and this expectation moved with the binding.
            ("C-h a", "apropos-command"),
            ("C-h d", "apropos-documentation"),
            ("C-h k", "describe_key"),
            ("C-h w", "where_is"),
            ("C-h b", "describe_bindings"),
            // emacs `C-h v` is describe-variable, which now has a real port; it
            // used to open the config-variable *search* picker, a different thing.
            ("C-h v", "describe_variable"),
            ("C-h m", "describe_current_modes"),
            ("C-h s", "describe_syntax"),
            ("C-h C", "describe_coding_system"),
            ("C-h L", "describe_language_environment"),
            ("C-h l", "view_lossage"),
            ("C-h p", "package_search"),
            ("C-h .", "hover"),
            // GNU help/doc commands wired to their faithful zmax ports.
            ("C-h h", "view_hello_file"),
            ("C-h e", "view_echo_area_messages"),
            ("C-h g", "describe_gnu_project"),
            ("C-h n", "browse_news"),
            ("C-h C-c", "describe_copying"),
            ("C-h C-d", "describe_distribution"),
            ("C-h C-f", "view_emacs_faq"),
            ("C-h C-w", "describe_no_warranty"),
        ] {
            assert_eq!(
                cmd(&km, Mode::Normal, chord).as_deref(),
                Some(want),
                "{chord} should map to {want}"
            );
        }
        // emacs `C-h C-p` is view-emacs-problems (the PROBLEMS file), not the FAQ.
        assert_eq!(
            cmd(&km, Mode::Normal, "C-h C-p").as_deref(),
            Some("view_emacs_problems")
        );
    }

    /// Every chord in the generated table must name a command that actually
    /// exists **and** must still be reachable once the curated prefixes are
    /// merged on top. A typo'd command name compiles fine (these are strings
    /// resolved at runtime), and a leaf sitting on a prefix key silently
    /// swallows everything beneath it — this test is what catches both.
    #[test]
    fn every_generated_emacs_chord_resolves() {
        let km = default();
        for (chord, _, name) in CXCH_FULL {
            assert!(
                name.parse::<MappableCommand>().is_ok(),
                "CXCH_FULL binds `{chord}` to `{name}`, which is not a real command"
            );
            assert!(
                cmd(&km, Mode::Normal, chord).is_some(),
                "`{chord}` does not resolve to a command — a leaf is shadowing it"
            );
        }
    }

    /// `C-x 4/5/8/t/RET` are *prefixes* in Emacs, never commands. Giving any of
    /// them a command of its own makes `add_command` drop every chord underneath
    /// (it cannot descend through a leaf), which is exactly how the whole
    /// `C-x 5`, `C-x t` and `C-x RET` maps used to be dead at runtime.
    #[test]
    fn emacs_prefix_keys_stay_prefixes() {
        let km = default();
        for chord in ["C-x 4", "C-x 5", "C-x 6", "C-x 8", "C-x t", "C-x ret"] {
            assert!(
                is_prefix(&km, Mode::Normal, chord),
                "{chord} must stay a prefix, not a command"
            );
        }
        // …and the children the leaves used to swallow are reachable again.
        for (chord, want) in [
            // The C-x 5 map runs the real frame commands now (zmax grew frames);
            // these three used to be wclose / vsplit / file_picker, and the
            // expectations moved with the bindings.
            ("C-x 5 0", "delete_frame"),
            ("C-x 5 2", "make_frame_command"),
            ("C-x 5 f", "find_file_other_frame"),
            ("C-x t 1", "tab_only"),
            // C-x t f is find-file-other-tab: the file opens in a NEW TAB. It sat on
            // the plain `file_picker` (which opens it in the current tab — that is
            // C-x C-f) until the real port existed; the expectation moved with the
            // binding.
            ("C-x t f", "find_file_other_tab"),
            ("C-x 8 ret", "unicode_picker"),
            // C-x RET f is set-buffer-file-coding-system, which has a real port;
            // it sat on the command_palette fallback until that port existed.
            ("C-x ret f", "set_buffer_file_coding_system"),
            ("C-x 4 c", "clone_indirect_buffer"),
        ] {
            assert_eq!(
                cmd(&km, Mode::Normal, chord).as_deref(),
                Some(want),
                "{chord}"
            );
        }
    }

    /// The Emacs prefix chords that used to hold approximations (because the
    /// real ports did not exist yet) now run the command the Emacs manual names
    /// for them.
    #[test]
    fn emacs_prefix_chords_run_their_real_ports() {
        let km = default();
        for (chord, want) in [
            // C-x C-k keyboard-macro map.
            ("C-x C-k b", "kmacro_bind_to_key"),
            ("C-x C-k C-c", "kmacro_set_counter"),
            ("C-x C-k C-f", "kmacro_set_format"),
            ("C-x C-k C-k", "kmacro_end_or_call_macro_repeat"),
            ("C-x C-k C-t", "kmacro_ring_swap"),
            ("C-x C-k e", "kmacro_edit_macro"),
            ("C-x C-k ret", "kmacro_edit_macro"),
            ("C-x C-k l", "kmacro_edit_lossage"),
            ("C-x C-k n", "kmacro_name_last_macro"),
            ("C-x C-k r", "apply_macro_to_region_lines"),
            ("C-x C-k space", "kmacro_step_edit_macro"),
            // Basic keyboard macros. C-x ) is kmacro-end-macro — its own command now,
            // not the `record_macro` toggle that used to serve both halves.
            ("C-x (", "record_macro"),
            ("C-x )", "kmacro_end_macro"),
            ("C-x e", "kmacro_end_or_call_macro"),
            // Text scale — `C-x C--` never parsed (`C--` is not a legal key).
            ("C-x C-minus", "text_scale_decrease"),
            ("C-x C-+", "text_scale_increase"),
            ("C-x C-0", "text_scale_reset"),
            ("C-x A-C-minus", "text_scale_decrease"),
            // Other-window / hideshow / sentences.
            ("C-x 4 .", "xref_find_definitions_other_window"),
            ("C-x 4 d", "dired_other_window"),
            ("C-x 4 C-j", "dired_jump_other_window"),
            ("C-x backspace", "backward_kill_sentence"),
            ("C-c @ A-C-h", "fold_close_all"),
            ("C-c @ A-C-s", "fold_open_all"),
            // C-x TAB is indent-rigidly (shift the block N columns), not `indent`
            // (re-indent each line by the language's rules) — a different command,
            // and the expectation moved with the binding.
            ("C-x tab", "indent_rigidly"),
            // Term mode's C-c keys, on their real ports.
            ("C-c C-j", "term_line_mode"),
            ("C-c C-k", "term_char_mode"),
            ("C-c C-q", "term_pager_toggle"),
            // goto-address-mode's C-c RET.
            ("C-c ret", "open_url_under_cursor"),
            // The C-x C-a half of the GUD map — the only key the three chords whose
            // C-c half is taken (re-run, debug-launch, doc-view open-text) can have.
            ("C-x C-a C-r", "dap_continue"),
            ("C-x C-a C-t", "gud_tbreak"),
            ("C-x C-a C-p", "gud_print"),
            ("C-x C-a <", "gud_up"),
            // Chords that sat on a near neighbour until the real port existed. Each
            // now runs the command the Emacs manual names for it: the Buffer Menu
            // (not the fuzzy switcher), Dired itself (not a file picker seeded with
            // the directory), the rectangular region (not vim's visual-block mode),
            // the other-window / other-tab / other-frame displays, and the
            // fit-to-buffer window shrink.
            ("C-x C-b", "list_buffers"),
            ("C-x d", "dired"),
            ("C-x C-j", "dired_jump"),
            ("C-x C-v", "find_file_replace_buffer"),
            ("C-x space", "rectangle_mark_mode"),
            ("C-x 4 b", "switch_to_buffer_other_window"),
            ("C-x t b", "switch_to_buffer_other_tab"),
            ("C-x t f", "find_file_other_tab"),
            ("C-x 5 m", "compose_mail_other_frame"),
            ("C-x -", "shrink_window_if_larger_than_buffer"),
            ("C-x x g", "revert_buffer_quick"),
        ] {
            assert_eq!(
                cmd(&km, Mode::Normal, chord).as_deref(),
                Some(want),
                "{chord}"
            );
        }
    }

    #[test]
    fn cx_v_c_is_add_commit_push() {
        // C-x v c under the emacs VCS prefix is the one-shot add-commit-push
        // (git_acp); the VCS node it lives under must stay a real prefix.
        let km = default();
        assert!(
            is_prefix(&km, Mode::Normal, "C-x v"),
            "C-x v is the VCS prefix"
        );
        assert_eq!(
            cmd(&km, Mode::Normal, "C-x v c").as_deref(),
            Some("git_acp"),
            "C-x v c must stage-all, commit and push"
        );
    }
}
