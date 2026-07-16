//! **Major-mode key overlays** — Emacs's per-major-mode keymaps, ported.
//!
//! Emacs reuses the same chords in every major mode and disambiguates them by
//! the buffer's mode: `C-c C-t` is `org-todo` in Org, `sgml-tag` in HTML,
//! `c-toggle-hungry-state` territory in C, … A single global keymap cannot hold
//! them all — they collide with each other and with the global map.
//!
//! zmax knows every buffer's major mode:
//! [`zmax_view::Document::major_mode`]. For most buffers that *is* the
//! document's language — the Emacs `M-x <lang>-mode` commands are ported as
//! exactly that: `org-mode`, `latex-mode`, `sgml-mode`, `html-mode`,
//! `fortran-mode`, `f90-mode` all set the current document's language
//! (`commands::enter_language_mode` / `set_major_mode`). So a language-scoped
//! overlay is not an approximation of Emacs's major-mode map; it is the same
//! mechanism under zmax's name for it.
//!
//! Emacs's major modes are a *superset* of zmax's languages, though.
//! `outline-mode`, `text-mode`, `enriched-mode`, `view-mode` and `nroff-mode`
//! all have keymaps, but they name no grammar and have no `languages.toml`
//! entry, so a document's language can never be `outline` and those maps could
//! never be reached. [`zmax_view::Document`] therefore carries an explicit
//! `major_mode: Option<String>` alongside its language, which those mode
//! commands set and which `Document::major_mode()` returns in preference to the
//! language. The overlay key is that string either way — a language name for a
//! file mode, a bare mode name for a language-less one.
//!
//! [`Keymaps::get_with_language`](super::Keymaps::get_with_language) consults
//! the overlay for the focused document's major mode *before* the base keymap, so
//! a major-mode chord shadows the global one exactly like Emacs's mode map
//! shadows the global map. Two guard rails keep that from breaking the modal
//! presets and ordinary typing:
//!
//! 1. **A major-mode prefix never opens on a base *leaf*.** In the `vim` preset
//!    `C-c` is `normal_mode` (escape); in `helix` it is `toggle_comments`. The
//!    overlay only turns `C-c` into a prefix where the base map already has a
//!    prefix there (the shipped `spacemacs` preset, and `emacs`). Enforced at
//!    lookup time, tested by `vim_preset_keeps_ctrl_c_escape`.
//! 2. **Each row names the modes it is live in** (`n`ormal / `s`elect /
//!    `i`nsert). Bare keys that mean something while you type keep their base
//!    meaning in Insert (Org's `TAB` does not swallow indentation), and the two
//!    chords whose Emacs meaning *is* a typing action (TeX's `"`, C's
//!    `C-c C-d` hungry-delete) are live only in Insert.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::{spacemacs::add_chord, KeyTrie, KeyTrieNode, Mode};

/// The Emacs major-mode keymaps, as `(languages, modes, chord, label, command)`:
///
/// * `languages` — space-separated major-mode names (what
///   `Document::major_mode()` returns): a `languages.toml` language name for a
///   mode that is one (`c`, `org`, `latex`), or the bare Emacs mode name for a
///   language-less mode (`outline`, `text`, `enriched`, `view`, `nroff`) that
///   only `Document::set_major_mode` can put there. One Emacs major mode can
///   cover several (`c cpp`, `html xml`).
/// * `modes` — the editor modes the chord is live in: `n`ormal, `s`elect,
///   `i`nsert.
/// * `chord` — space-separated keys, same vocabulary as the `keymap!` macro.
/// * `label` — which-key label for the intermediate submap nodes.
/// * `command` — a static command, or a typable one written `":name"`.
///
/// Every row is verified by `every_major_mode_chord_resolves` (the command name
/// exists and the chord parses and resolves inside the overlay).
#[rustfmt::skip]
pub const MAJOR_MODE_KEYS: &[(&str, &str, &str, &str, &str)] = &[
    // -- Org mode (org-mode) -------------------------------------------------
    // TAB / S-TAB are Normal+Select only: `org_cycle` folds a heading and has no
    // "indent when not on a heading" fallback the way Emacs's org-cycle does, so
    // binding it in Insert would swallow indentation in body text.
    ("org", "ns",  "tab",     "Org", "org_cycle"),      // org-cycle
    ("org", "ns",  "S-tab",   "Org", "org_shifttab"),   // org-shifttab (global cycle)
    ("org", "nsi", "A-up",    "Org", "org_metaup"),     // M-<up>:    org-metaup
    ("org", "nsi", "A-down",  "Org", "org_metadown"),   // M-<down>:  org-metadown
    ("org", "nsi", "A-left",  "Org", "org_metaleft"),   // M-<left>:  org-metaleft (promote)
    ("org", "nsi", "A-right", "Org", "org_metaright"),  // M-<right>: org-metaright (demote)
    ("org", "nsi", "C-c C-t", "Org", "org_todo"),       // org-todo
    // org-schedule / org-deadline read a date. They used to *require* it as an
    // argument (so a key press could only ever error); they prompt now, which is
    // what makes the two chords bindable.
    ("org", "nsi", "C-c C-s", "Org", "org_schedule"),   // org-schedule (SCHEDULED:)
    ("org", "nsi", "C-c C-d", "Org", "org_deadline"),   // org-deadline  (DEADLINE:)

    // -- C / C++ (c-mode, c++-mode) ------------------------------------------
    // `C-c .` is c-set-style in Normal/Select only: Insert keeps the base
    // `C-c .` (postfix completion), which is a typing action.
    ("c cpp", "ns",  "C-c .",         "C mode", "c_set_style"),                 // c-set-style
    ("c cpp", "nsi", "C-c C-a",       "C mode", "c_toggle_auto_newline"),       // c-toggle-auto-newline
    ("c cpp", "nsi", "C-c C-l",       "C mode", "c_toggle_electric_state"),     // c-toggle-electric-state
    ("c cpp", "nsi", "C-c C-e",       "C mode", "c_macro_expand"),              // c-macro-expand
    ("c cpp", "nsi", "C-c C-s",       "C mode", "c_show_syntactic_information"),// c-show-syntactic-information
    ("c cpp", "nsi", "C-c C-q",       "C mode", "c_indent_defun"),              // c-indent-defun
    ("c cpp", "nsi", "C-c C-\\",      "C mode", "c_backslash_region"),          // c-backslash-region
    ("c cpp", "nsi", "C-c C-n",       "C mode", "c_forward_conditional"),       // c-forward-conditional
    ("c cpp", "nsi", "C-c C-p",       "C mode", "c_backward_conditional"),      // c-backward-conditional
    ("c cpp", "nsi", "C-c C-u",       "C mode", "c_up_conditional"),            // c-up-conditional
    // Hungry delete. C-c C-d is Insert-only: in Normal it would shadow the
    // global C-c C-d (start a debug session), and hungry-delete is a typing act.
    ("c cpp", "i",   "C-c C-d",       "C mode", "c_hungry_delete_forward"),     // c-hungry-delete-forward
    ("c cpp", "nsi", "C-c del",       "C mode", "c_hungry_delete_forward"),     // C-c <Delete>
    ("c cpp", "nsi", "C-c C-del",     "C mode", "c_hungry_delete_forward"),     // C-c C-<Delete>
    ("c cpp", "nsi", "C-c backspace", "C mode", "c_hungry_delete_backwards"),   // C-c DEL
    ("c cpp", "nsi", "C-c C-backspace","C mode","c_hungry_delete_backwards"),   // C-c C-DEL
    ("c cpp", "nsi", "A-C-q",         "C mode", "c_indent_exp"),                // C-M-q: c-indent-exp
    ("c cpp", "nsi", "A-C-h",         "C mode", "c_mark_function"),             // C-M-h: c-mark-function
    ("c cpp", "nsi", "A-a",           "C mode", "c_beginning_of_statement"),    // M-a: c-beginning-of-statement
    ("c cpp", "nsi", "A-e",           "C mode", "c_end_of_statement"),          // M-e: c-end-of-statement
    ("c cpp", "nsi", "A-q",           "C mode", "c_fill_paragraph"),            // M-q: c-fill-paragraph
    // The two keys the Emacs manual's "Program Modes" / "Basic Indent" nodes give a
    // programming mode: "typing TAB updates the indentation of the current line"
    // (c-indent-line-or-region) and "DEL is usually bound to
    // backward-delete-char-untabify … so that you can delete one column of
    // indentation without worrying whether the whitespace consists of spaces or
    // tabs" (cc-mode reaches it through `c-backspace-function`). Both are typing
    // actions, so both are Insert-only — like Text mode's TAB above: in Normal,
    // `tab` is vim's jump_forward and `backspace` its motion, and they stay that.
    ("c cpp", "i",   "tab",           "C mode", "c_indent_line_or_region"),      // TAB: c-indent-line-or-region
    ("c cpp", "i",   "backspace",     "C mode", "backward_delete_char_untabify"),// DEL: backward-delete-char-untabify

    // -- TeX / LaTeX (tex-mode, latex-mode) ----------------------------------
    // `"` is Insert-only — in Normal it is vim's register prefix.
    ("latex", "i",   "\"",      "TeX", "tex_insert_quote"),           // tex-insert-quote
    ("latex", "nsi", "C-j",     "TeX", "tex_terminate_paragraph"),    // tex-terminate-paragraph
    ("latex", "nsi", "C-c {",   "TeX", "tex_insert_braces"),          // tex-insert-braces
    ("latex", "nsi", "C-c }",   "TeX", "up_list"),                    // up-list
    ("latex", "nsi", "C-c C-b", "TeX", "tex_buffer"),                 // tex-buffer
    ("latex", "nsi", "C-c C-r", "TeX", "tex_region"),                 // tex-region
    ("latex", "nsi", "C-c C-f", "TeX", "tex_file"),                   // tex-file
    ("latex", "nsi", "C-c C-v", "TeX", "tex_view"),                   // tex-view
    ("latex", "nsi", "C-c C-p", "TeX", "tex_print"),                  // tex-print
    ("latex", "nsi", "C-c C-l", "TeX", "tex_recenter_output_buffer"), // tex-recenter-output-buffer
    ("latex", "nsi", "C-c tab", "TeX", "tex_bibtex_file"),            // C-c TAB: tex-bibtex-file
    ("latex", "nsi", "C-c C-c", "TeX", "tex_compile"),                // tex-compile (the major-mode compile)
    ("latex", "nsi", "C-c C-o", "TeX", "latex_insert_block"),         // latex-insert-block
    ("latex", "nsi", "C-c C-e", "TeX", "latex_close_block"),          // latex-close-block

    // -- SGML / HTML (sgml-mode, html-mode) ----------------------------------
    ("html xml", "nsi", "C-c C-n", "SGML", "sgml_name_char"),          // sgml-name-char
    ("html xml", "nsi", "C-c C-t", "SGML", "sgml_tag"),                // sgml-tag
    ("html xml", "nsi", "C-c C-a", "SGML", "sgml_attributes"),         // sgml-attributes
    ("html xml", "nsi", "C-c C-f", "SGML", "sgml_skip_tag_forward"),   // sgml-skip-tag-forward
    ("html xml", "nsi", "C-c C-b", "SGML", "sgml_skip_tag_backward"),  // sgml-skip-tag-backward
    ("html xml", "nsi", "C-c C-d", "SGML", "sgml_delete_tag"),         // sgml-delete-tag
    ("html xml", "nsi", "C-c ?",   "SGML", "sgml_tag_help"),           // sgml-tag-help
    ("html xml", "nsi", "C-c /",   "SGML", "sgml_close_tag"),          // sgml-close-tag
    ("html xml", "nsi", "C-c 8",   "SGML", "sgml_name_8bit_mode"),     // sgml-name-8bit-mode
    ("html xml", "nsi", "C-c C-v", "SGML", "sgml_validate"),           // sgml-validate
    // "C-c TAB — Toggle the visibility of existing tags in the buffer. This can
    // be used as a cheap preview" (Emacs manual, HTML Mode). The real port: the
    // tags stay in the buffer, they stop being drawn.
    ("html xml", "nsi", "C-c tab", "SGML", "sgml_tags_invisible"),     // C-c TAB: sgml-tags-invisible

    // -- Fortran / F90 (fortran-mode, f90-mode) ------------------------------
    // `C-c ;` is Normal+Select only: Insert keeps the base `C-c ;`
    // (complete-current-statement), a typing action.
    ("fortran", "ns",  "C-c ;",   "Fortran", "fortran_comment_region"),    // fortran-comment-region
    ("fortran", "nsi", "C-c C-n", "Fortran", "fortran_next_statement"),    // fortran-next-statement
    ("fortran", "nsi", "C-c C-p", "Fortran", "fortran_previous_statement"),// fortran-previous-statement
    ("fortran", "nsi", "C-c C-d", "Fortran", "fortran_join_line"),         // fortran-join-line (= M-^)
    ("fortran", "nsi", "C-c C-r", "Fortran", "fortran_column_ruler"),      // fortran-column-ruler
    // Emacs binds C-c C-w to fortran-window-create-momentarily and C-u C-c C-w to
    // fortran-window-create (Fortran Columns). zmax has no universal argument,
    // so the bare chord is the momentary one — the command the chord names.
    // `fortran_window_create` stays reachable by name (M-x / `:`).
    ("fortran", "nsi", "C-c C-w", "Fortran", "fortran_window_create_momentarily"), // C-c C-w
    ("fortran", "nsi", "C-c C-e", "Fortran", "f90_next_block"),            // f90-next-block
    ("fortran", "nsi", "C-c C-a", "Fortran", "f90_previous_block"),        // f90-previous-block
    ("fortran", "nsi", "A-C-n",   "Fortran", "fortran_end_of_block"),      // C-M-n: fortran-end-of-block
    ("fortran", "nsi", "A-C-p",   "Fortran", "fortran_beginning_of_block"),// C-M-p: fortran-beginning-of-block
    ("fortran", "nsi", "A-C-j",   "Fortran", "fortran_split_line"),        // C-M-j: fortran-split-line
    ("fortran", "nsi", "A-C-q",   "Fortran", "fortran_indent_subprogram"), // C-M-q: fortran-indent-subprogram
    ("fortran", "nsi", "A-^",     "Fortran", "fortran_join_line"),         // M-^: fortran-join-line

    // -- Emacs Lisp (emacs-lisp-mode) ----------------------------------------
    ("elisp", "nsi", "A-C-x", "Emacs Lisp", "eval_elisp_defun"),           // C-M-x: eval-defun
    // Lisp Interaction mode's `C-j` (eval-print-last-sexp). zmax's
    // `lisp-interaction-mode` is an elisp *scratch buffer* (commands.rs:
    // `lisp_interaction_mode`) and the overlay key is the language, so the chord
    // is live in every elisp buffer. Normal-only: `C-j` in Insert stays the
    // newline it is in emacs-lisp-mode.
    ("elisp", "n", "C-j", "Emacs Lisp", "eval_print_last_sexp"),           // C-j: eval-print-last-sexp

    // -- Common Lisp / Scheme (lisp-mode, scheme-mode) -----------------------
    // Both send the top-level form at point to the inferior Lisp started by
    // `run-lisp` — emacs binds `C-M-x` to `lisp-eval-defun` in both modes.
    ("commonlisp scheme", "nsi", "A-C-x", "Lisp", "lisp_eval_defun"),      // C-M-x: lisp-eval-defun

    // -- Message / mail composition (message-mode) ---------------------------
    // The `mail` language is what `C-x m` (compose-mail) opens, i.e. Emacs's
    // message-mode buffer, so message-mode's map is this language's overlay.
    // `C-c C-f` is a *sub-prefix* here (the header map) even though the base map
    // has a leaf on it (gud-finish): a major-mode prefix two keys deep is allowed
    // — only a base leaf on the FIRST key (vim's `C-c` = escape) blocks it.
    ("mail", "nsi", "C-c C-c",     "Message", ":message-send-and-exit"),   // message-send-and-exit
    ("mail", "nsi", "C-c C-s",     "Message", ":message-send"),            // message-send
    ("mail", "nsi", "C-c C-k",     "Message", ":message-kill-buffer"),     // message-kill-buffer
    ("mail", "nsi", "C-c C-b",     "Message", ":message-goto-body"),       // message-goto-body
    ("mail", "nsi", "C-c C-w",     "Message", ":message-insert-signature"),// message-insert-signature
    ("mail", "nsi", "C-c C-f C-t", "Message", ":message-goto-to"),         // message-goto-to
    ("mail", "nsi", "C-c C-f C-s", "Message", ":message-goto-subject"),    // message-goto-subject
    ("mail", "nsi", "C-c C-f C-c", "Message", ":message-goto-cc"),         // message-goto-cc
    ("mail", "nsi", "C-c C-f C-b", "Message", ":message-goto-bcc"),        // message-goto-bcc
    // The rest of the header map (Emacs manual, "Header Editing"): C-c C-f C-f is
    // Mail-Followup-To, C-c C-f C-w is Fcc, C-c C-f C-r is Reply-To. Each command
    // creates the header when the draft has none, which is what the emacs ones do.
    // (The three ports exist under `goto_followup_to` / `message_goto_fcc` /
    // `goto_reply_to`; the keys are the manual's, which is what the chords must be.)
    ("mail", "nsi", "C-c C-f C-f", "Message", "goto_followup_to"),         // message-goto-followup-to
    ("mail", "nsi", "C-c C-f C-w", "Message", "message_goto_fcc"),         // message-goto-fcc
    ("mail", "nsi", "C-c C-f C-r", "Message", "goto_reply_to"),            // message-goto-reply-to
    // message-tab: in a header line it completes the mail alias before point, and
    // anywhere else it is an ordinary indent — a typing action, so Insert only (in
    // Normal, `tab` is vim's jump_forward and must stay it).
    ("mail", "i",   "tab",         "Message", "message_tab"),              // TAB: message-tab
    ("mail", "nsi", "C-c C-y",     "Message", "message_yank_original"),    // message-yank-original (cite the reply)
    ("mail", "nsi", "C-c C-q",     "Message", "mail_fill_yanked_message"), // message-fill-yanked-message
    ("mail", "nsi", "C-c C-a",     "Message", ":mml-attach-file"),         // mml-attach-file

    // -- Outline (outline-mode) ----------------------------------------------
    // The first *language-less* major mode: `outline` is no grammar and no
    // `languages.toml` entry, so it only ever arrives via
    // `Document::set_major_mode(Some("outline"))` (M-x outline-mode).
    //
    // Every one of these commands is already reachable on `C-c @ <k>` — Emacs's
    // outline-MINOR-mode prefix, which is where zmax had to park them while a
    // major mode could only be a language. These are the same commands on the
    // real outline-MAJOR-mode keys, which drop the `@`.
    //
    // TAB / S-TAB are Normal+Select only, like Org's: `outline_cycle` folds a
    // heading and has no "indent when not on a heading" fallback, so binding it
    // in Insert would swallow indentation in body text.
    ("outline", "ns",  "tab",     "Outline", "outline_cycle"),                 // outline-cycle
    ("outline", "ns",  "S-tab",   "Outline", "outline_cycle_buffer"),          // outline-cycle-buffer
    // Motion (Outline Motion Commands).
    ("outline", "nsi", "C-c C-n", "Outline", "outline_next_visible_heading"),  // outline-next-visible-heading
    ("outline", "nsi", "C-c C-p", "Outline", "outline_previous_visible_heading"),// outline-previous-visible-heading
    ("outline", "nsi", "C-c C-f", "Outline", "outline_forward_same_level"),    // outline-forward-same-level
    ("outline", "nsi", "C-c C-b", "Outline", "outline_backward_same_level"),   // outline-backward-same-level
    ("outline", "nsi", "C-c C-u", "Outline", "outline_up_heading"),            // outline-up-heading
    // Visibility (Outline Visibility Commands).
    ("outline", "nsi", "C-c C-c", "Outline", "outline_hide_entry"),            // outline-hide-entry
    ("outline", "nsi", "C-c C-e", "Outline", "outline_show_entry"),            // outline-show-entry
    ("outline", "nsi", "C-c C-d", "Outline", "outline_hide_subtree"),          // outline-hide-subtree
    ("outline", "nsi", "C-c C-s", "Outline", "outline_show_subtree"),          // outline-show-subtree
    ("outline", "nsi", "C-c C-l", "Outline", "outline_hide_leaves"),           // outline-hide-leaves
    ("outline", "nsi", "C-c C-k", "Outline", "outline_show_branches"),         // outline-show-branches
    ("outline", "nsi", "C-c C-i", "Outline", "outline_show_children"),         // outline-show-children
    ("outline", "nsi", "C-c C-t", "Outline", "outline_hide_body"),             // outline-hide-body
    ("outline", "nsi", "C-c C-a", "Outline", "outline_show_all"),              // outline-show-all
    ("outline", "nsi", "C-c C-q", "Outline", "outline_hide_sublevels"),        // outline-hide-sublevels
    ("outline", "nsi", "C-c C-o", "Outline", "outline_hide_other"),            // outline-hide-other
    // The two regexp commands (Outline Visibility). Like org-schedule, they read
    // their regexp interactively now instead of demanding an argument, so the
    // `C-c /` sub-prefix they live on is finally bindable.
    ("outline", "nsi", "C-c / h", "Outline", "outline_hide_by_heading_regexp"), // outline-hide-by-heading-regexp
    ("outline", "nsi", "C-c / s", "Outline", "outline_show_by_heading_regexp"), // outline-show-by-heading-regexp

    // -- Text (text-mode) ----------------------------------------------------
    // Both keys are typing actions, so both are Insert-only: in Normal, `tab` is
    // vim's jump_forward and must stay it.
    // Emacs: "In Text mode, the TAB (indent-for-tab-command) command usually
    // inserts whitespace up to the next tab stop, instead of indenting the
    // current line" — that is `insert_tab`, not the base map's `smart_tab`
    // (which stops inserting once there is a non-blank to the left).
    ("text", "i", "tab",   "Text", "insert_tab"),  // TAB:   indent-for-tab-command
    ("text", "i", "A-tab", "Text", "completion"),  // M-TAB: completion-at-point

    // -- Nroff (nroff-mode) --------------------------------------------------
    // `nroff` is language-less too (M-x nroff-mode). M-? shadows the base
    // `A-?` (xref-find-references) in an nroff buffer — which is exactly what
    // Emacs's nroff-mode map does to the global one.
    ("nroff", "nsi", "A-n", "Nroff", "nroff_forward_text_line"),  // M-n: nroff-forward-text-line
    ("nroff", "nsi", "A-p", "Nroff", "nroff_backward_text_line"), // M-p: nroff-backward-text-line
    ("nroff", "nsi", "A-?", "Nroff", "nroff_count_text_lines"),   // M-?: nroff-count-text-lines

    // -- Enriched text (enriched-mode) ---------------------------------------
    // The `M-j` justification prefix is Normal+Select only: in Insert the base
    // map has a *leaf* on `A-j` (default-indent-new-line), and a major-mode
    // prefix never opens on a base leaf (guard rail 1) — so an `M-j …` chord
    // would be dead there anyway, and it is a typing action besides.
    ("enriched", "nsi", "C-c [",   "Enriched", ":set-left-margin"),        // C-c [: set-left-margin
    ("enriched", "nsi", "C-c ]",   "Enriched", ":set-right-margin"),       // C-c ]: set-right-margin
    ("enriched", "nsi", "C-x tab", "Enriched", ":increase-left-margin"),   // C-x TAB: increase-left-margin
    ("enriched", "ns",  "A-j l",   "Enriched", ":set-justification-left"), // M-j l: set-justification-left
    ("enriched", "ns",  "A-j r",   "Enriched", ":set-justification-right"),// M-j r: set-justification-right
    ("enriched", "ns",  "A-j b",   "Enriched", ":set-justification-full"), // M-j b: set-justification-full
    ("enriched", "ns",  "A-j c",   "Enriched", ":set-justification-center"),// M-j c: set-justification-center
    ("enriched", "ns",  "A-j u",   "Enriched", ":set-justification-none"), // M-j u: set-justification-none
    ("enriched", "ns",  "A-S",     "Enriched", ":set-justification-center"),// M-S:   set-justification-center
    // The `M-o` face map (`facemenu-keymap`), which enriched-mode is the reason
    // to have: each key puts a face on the region (or on what you type next) as a
    // text property, and enriched-mode's save path writes those faces back out as
    // `text/enriched`. The six keys are the six the Emacs manual's "Enriched
    // Faces" node lists, each on its own facemenu-* port — these could not be
    // bound at all until the facemenu setters existed.
    ("enriched", "nsi", "A-o d",   "Face",     "facemenu_set_default"),     // M-o d: facemenu-set-default
    ("enriched", "nsi", "A-o b",   "Face",     "facemenu_set_bold"),        // M-o b: facemenu-set-bold
    ("enriched", "nsi", "A-o i",   "Face",     "facemenu_set_italic"),      // M-o i: facemenu-set-italic
    ("enriched", "nsi", "A-o l",   "Face",     "facemenu_set_bold_italic"), // M-o l: facemenu-set-bold-italic
    ("enriched", "nsi", "A-o u",   "Face",     "facemenu_set_underline"),   // M-o u: facemenu-set-underline
    ("enriched", "nsi", "A-o o",   "Face",     "facemenu_set_face"),        // M-o o: facemenu-set-face (prompts)

    // -- View (view-mode) ----------------------------------------------------
    // Normal-only: View mode is for reading, and every one of these is a bare
    // typing key that must keep its meaning in Insert.
    //
    // `space` deliberately shadows the leader in a view buffer — Emacs's View
    // mode does exactly that (SPC scrolls a windowful), and it is the one key
    // the mode exists for. `:view-mode` (typed on `:`, which the overlay does
    // not touch) is the way back out.
    ("view", "n", "space",     "View", "page_down"), // SPC: View-scroll-page-forward
    ("view", "n", "backspace", "View", "page_up"),   // DEL: View-scroll-page-backward
    ("view", "n", "s",         "View", "search"),    // s:   incremental search
    // The two ways out. `view_mode` toggles, so on a view buffer it *is*
    // View-exit; View-quit additionally buries the buffer, which zmax does not
    // do (the buffer stays where it is, no longer in view-mode).
    ("view", "n", "e",         "View", "view_mode"), // e:   View-exit
    ("view", "n", "q",         "View", "view_mode"), // q:   View-quit (no bury)
];

/// The overlay tries, built once: mode -> language -> the major-mode [`KeyTrie`].
fn overlays() -> &'static HashMap<Mode, HashMap<String, KeyTrie>> {
    static OVERLAYS: OnceLock<HashMap<Mode, HashMap<String, KeyTrie>>> = OnceLock::new();
    OVERLAYS.get_or_init(|| {
        let mut out: HashMap<Mode, HashMap<String, KeyTrie>> = HashMap::new();
        for (languages, modes, chord, label, cmd) in MAJOR_MODE_KEYS {
            for (flag, mode) in [
                ('n', Mode::Normal),
                ('s', Mode::Select),
                ('i', Mode::Insert),
            ] {
                if !modes.contains(flag) {
                    continue;
                }
                for language in languages.split(' ') {
                    let trie = out
                        .entry(mode)
                        .or_default()
                        .entry(language.to_string())
                        .or_insert_with(|| {
                            KeyTrie::Node(KeyTrieNode::new(label, Default::default()))
                        });
                    if let KeyTrie::Node(root) = trie {
                        add_chord(root, chord, label, cmd);
                    }
                }
            }
        }
        out
    })
}

/// The major-mode overlay for `language` in `mode`, if that language has one.
pub fn overlay(language: &str, mode: Mode) -> Option<&'static KeyTrie> {
    overlays().get(&mode)?.get(language)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{preset, KeymapResult, Keymaps, MappableCommand};
    use zmax_view::input::KeyEvent;

    fn cmd_at(trie: &KeyTrie, chord: &str) -> Option<String> {
        let keys: Vec<KeyEvent> = chord.split(' ').map(|k| k.parse().unwrap()).collect();
        match trie.search(&keys) {
            Some(KeyTrie::MappableCommand(c)) => Some(c.name().to_string()),
            _ => None,
        }
    }

    /// Every row must name a real command, parse as a chord, and still resolve to
    /// a leaf inside its own overlay — a typo'd command name compiles (these are
    /// strings resolved at runtime) and a chord sitting on another chord's prefix
    /// key silently swallows it.
    #[test]
    fn every_major_mode_chord_resolves() {
        for (languages, modes, chord, _, name) in MAJOR_MODE_KEYS {
            assert!(
                name.parse::<MappableCommand>().is_ok(),
                "`{chord}` is bound to `{name}`, which is not a real command"
            );
            for key in chord.split(' ') {
                assert!(
                    key.parse::<KeyEvent>().is_ok(),
                    "`{chord}`: `{key}` is not a parseable key"
                );
            }
            assert!(
                !modes.is_empty() && modes.chars().all(|c| "nsi".contains(c)),
                "`{chord}`: `{modes}` is not a mode set (n/s/i)"
            );
            for (flag, mode) in [
                ('n', Mode::Normal),
                ('s', Mode::Select),
                ('i', Mode::Insert),
            ] {
                if !modes.contains(flag) {
                    continue;
                }
                for language in languages.split(' ') {
                    let trie = overlay(language, mode)
                        .unwrap_or_else(|| panic!("no `{language}` overlay in {mode}"));
                    // A typable row is written `":name"`; `MappableCommand::name()`
                    // reports it without the `:` sigil (which is syntax, not part of
                    // the name — the report generator strips it the same way). The
                    // identity check is unchanged: the chord must land on exactly the
                    // command the row names.
                    assert_eq!(
                        cmd_at(trie, chord).as_deref(),
                        Some(name.trim_start_matches(':')),
                        "{language}/{mode}: `{chord}` does not resolve to `{name}` — \
                         another chord's prefix is shadowing it"
                    );
                }
            }
        }
    }

    /// No two rows may claim the same (language, mode, chord): `add_chord`
    /// overwrites silently, so a collision would quietly drop one binding.
    #[test]
    fn no_duplicate_major_mode_chords() {
        let mut seen = std::collections::HashSet::new();
        for (languages, modes, chord, _, _) in MAJOR_MODE_KEYS {
            for language in languages.split(' ') {
                for m in modes.chars() {
                    assert!(
                        seen.insert((language, m, *chord)),
                        "duplicate binding for `{chord}` in {language}/{m}"
                    );
                }
            }
        }
    }

    /// The shipped keymap: a major-mode chord shadows the global one in its own
    /// language, and leaves every other language alone.
    #[test]
    fn overlay_shadows_the_global_chord_only_in_its_language() {
        let mut keymaps = Keymaps::new(Box::new(arc_swap::access::Constant(
            preset("spacemacs").unwrap(),
        )));
        let key = |k: &str| k.parse::<KeyEvent>().unwrap();
        let mut run = |lang: Option<&str>, chord: &[&str]| -> KeymapResult {
            let mut res = KeymapResult::NotFound;
            for k in chord {
                res = keymaps.get_with_language(Mode::Normal, key(k), lang);
            }
            res
        };
        let matched = |res: KeymapResult| match res {
            KeymapResult::Matched(c) => c.name().to_string(),
            other => panic!("expected a command, got {other:?}"),
        };
        // C-c C-t: org-todo in Org, sgml-tag in HTML, the global doc-view text
        // command in a buffer with no major-mode map (and in plain Rust).
        assert_eq!(matched(run(Some("org"), &["C-c", "C-t"])), "org_todo");
        assert_eq!(matched(run(Some("html"), &["C-c", "C-t"])), "sgml_tag");
        assert_eq!(
            matched(run(Some("rust"), &["C-c", "C-t"])),
            "doc-view-open-text"
        );
        assert_eq!(matched(run(None, &["C-c", "C-t"])), "doc-view-open-text");
        // A global chord the overlay does not touch still works in an org buffer.
        assert_eq!(
            matched(run(Some("org"), &["C-c", "C-c"])),
            "run_active_config"
        );
        // Org's TAB shadows vim's jump_forward — but only in org.
        assert_eq!(matched(run(Some("org"), &["tab"])), "org_cycle");
        assert_eq!(matched(run(Some("rust"), &["tab"])), "jump_forward");
    }

    /// Guard rail 1: a major-mode prefix never opens on a base *leaf*. In the
    /// `vim` preset `C-c` is escape-to-normal; the C/Org/TeX overlays must not
    /// turn it into a dead prefix there.
    #[test]
    fn vim_preset_keeps_ctrl_c_escape() {
        let mut keymaps =
            Keymaps::new(Box::new(arc_swap::access::Constant(preset("vim").unwrap())));
        let c_c = "C-c".parse::<KeyEvent>().unwrap();
        // vim's insert-mode C-c is a sequence (leave insert, then Normal), i.e. a
        // base *leaf* — the C overlay's `C-c …` chords must not turn it into a
        // prefix, or the vim preset would lose escape in every C buffer.
        let names = match keymaps.get_with_language(Mode::Insert, c_c, Some("c")) {
            KeymapResult::MatchedSequence(cmds) => cmds
                .iter()
                .map(|c| c.name().to_string())
                .collect::<Vec<_>>(),
            other => panic!("vim's insert-mode C-c must still run a command, got {other:?}"),
        };
        assert_eq!(
            names.last().map(String::as_str),
            Some("normal_mode"),
            "vim's insert-mode C-c must stay escape-to-normal in a C buffer"
        );
    }

    /// The language-less major modes — the whole point of
    /// `Document::major_mode` being its own field rather than the language.
    /// `outline` is not a grammar and never will be, so these chords are only
    /// reachable because the overlay key is an opaque mode name.
    #[test]
    fn language_less_modes_are_reachable_and_shadow_the_base_map() {
        let mut keymaps = Keymaps::new(Box::new(arc_swap::access::Constant(
            preset("spacemacs").unwrap(),
        )));
        let key = |k: &str| k.parse::<KeyEvent>().unwrap();
        let mut run = |mode: Mode, lang: Option<&str>, chord: &[&str]| -> KeymapResult {
            let mut res = KeymapResult::NotFound;
            for k in chord {
                res = keymaps.get_with_language(mode, key(k), lang);
            }
            res
        };
        let matched = |res: KeymapResult| match res {
            KeymapResult::Matched(c) => c.name().to_string(),
            other => panic!("expected a command, got {other:?}"),
        };

        // Outline mode's real keys, which drop the `@` of the minor-mode prefix.
        assert_eq!(
            matched(run(Mode::Normal, Some("outline"), &["C-c", "C-t"])),
            "outline_hide_body"
        );
        // …and the same chord in a plain buffer is still the global command.
        assert_eq!(
            matched(run(Mode::Normal, Some("rust"), &["C-c", "C-t"])),
            "doc-view-open-text"
        );
        // Nroff's M-? shadows the base xref-find-references, in nroff only.
        assert_eq!(
            matched(run(Mode::Normal, Some("nroff"), &["A-?"])),
            "nroff_count_text_lines"
        );
        assert_ne!(
            matched(run(Mode::Normal, Some("rust"), &["A-?"])),
            "nroff_count_text_lines"
        );
        // Enriched's `M-j` opens as a prefix in Normal (the base map has no leaf
        // there) and lands on a typable.
        assert_eq!(
            matched(run(Mode::Normal, Some("enriched"), &["A-j", "c"])),
            "set-justification-center"
        );
        // View mode's SPC scrolls instead of opening the leader — the one chord
        // in this table that deliberately shadows a base *prefix*.
        assert_eq!(
            matched(run(Mode::Normal, Some("view"), &["space"])),
            "page_down"
        );
        // …and SPC is still the leader everywhere else.
        assert!(
            matches!(
                run(Mode::Normal, None, &["space"]),
                KeymapResult::Pending(_)
            ),
            "SPC must stay the leader in an ordinary buffer"
        );
    }

    /// The language-less modes must not steal keys from a buffer that is typing:
    /// `s`, `space` and `backspace` are View mode's, and they stay plain input
    /// in Insert even in a view buffer.
    #[test]
    fn view_mode_keys_are_not_live_in_insert() {
        let new = || {
            Keymaps::new(Box::new(arc_swap::access::Constant(
                preset("spacemacs").unwrap(),
            )))
        };
        let (mut in_view, mut plain) = (new(), new());
        for k in ["s", "space", "backspace"] {
            let key = k.parse::<KeyEvent>().unwrap();
            assert_eq!(
                in_view.get_with_language(Mode::Insert, key, Some("view")),
                plain.get_with_language(Mode::Insert, key, None),
                "`{k}` in Insert must keep its base meaning in a view buffer"
            );
        }
    }

    /// Guard rail 2: the mode set is honoured — TeX's `\"` self-inserts in
    /// Normal (it is vim's register prefix there) and only runs
    /// `tex-insert-quote` while typing.
    #[test]
    fn mode_scoped_rows_are_only_live_in_their_modes() {
        let mut keymaps = Keymaps::new(Box::new(arc_swap::access::Constant(
            preset("spacemacs").unwrap(),
        )));
        let quote = "\"".parse::<KeyEvent>().unwrap();
        assert_eq!(
            keymaps.get_with_language(Mode::Insert, quote, Some("latex")),
            KeymapResult::Matched(MappableCommand::tex_insert_quote)
        );
        assert_ne!(
            keymaps.get_with_language(Mode::Normal, quote, Some("latex")),
            KeymapResult::Matched(MappableCommand::tex_insert_quote),
            "`\"` in Normal stays vim's register prefix, even in a TeX buffer"
        );
    }
}
