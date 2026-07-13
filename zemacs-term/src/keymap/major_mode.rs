//! **Major-mode key overlays** — Emacs's per-major-mode keymaps, ported.
//!
//! Emacs reuses the same chords in every major mode and disambiguates them by
//! the buffer's mode: `C-c C-t` is `org-todo` in Org, `sgml-tag` in HTML,
//! `c-toggle-hungry-state` territory in C, … A single global keymap cannot hold
//! them all — they collide with each other and with the global map.
//!
//! zemacs already knows every buffer's major mode: it is the document's
//! **language** ([`zemacs_view::Document::language_name`]). The Emacs
//! `M-x <lang>-mode` commands are ported as exactly that — `org-mode`,
//! `latex-mode`, `sgml-mode`, `html-mode`, `fortran-mode`, `f90-mode` all set
//! the current document's language (`commands::enter_language_mode` /
//! `set_major_mode`). So a *language-scoped overlay* is not an approximation of
//! Emacs's major-mode map; it is the same mechanism under zemacs's name for it.
//!
//! [`Keymaps::get_with_language`](super::Keymaps::get_with_language) consults
//! the overlay for the focused document's language *before* the base keymap, so
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
/// * `languages` — space-separated `languages.toml` names (what
///   `Document::language_name()` returns). One Emacs major mode can cover
///   several (`c cpp`, `html xml`).
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

    // -- Fortran / F90 (fortran-mode, f90-mode) ------------------------------
    // `C-c ;` is Normal+Select only: Insert keeps the base `C-c ;`
    // (complete-current-statement), a typing action.
    ("fortran", "ns",  "C-c ;",   "Fortran", "fortran_comment_region"),    // fortran-comment-region
    ("fortran", "nsi", "C-c C-n", "Fortran", "fortran_next_statement"),    // fortran-next-statement
    ("fortran", "nsi", "C-c C-p", "Fortran", "fortran_previous_statement"),// fortran-previous-statement
    ("fortran", "nsi", "C-c C-d", "Fortran", "fortran_join_line"),         // fortran-join-line (= M-^)
    ("fortran", "nsi", "C-c C-r", "Fortran", "fortran_column_ruler"),      // fortran-column-ruler
    ("fortran", "nsi", "C-c C-e", "Fortran", "f90_next_block"),            // f90-next-block
    ("fortran", "nsi", "C-c C-a", "Fortran", "f90_previous_block"),        // f90-previous-block
    ("fortran", "nsi", "A-C-n",   "Fortran", "fortran_end_of_block"),      // C-M-n: fortran-end-of-block
    ("fortran", "nsi", "A-C-p",   "Fortran", "fortran_beginning_of_block"),// C-M-p: fortran-beginning-of-block
    ("fortran", "nsi", "A-C-j",   "Fortran", "fortran_split_line"),        // C-M-j: fortran-split-line
    ("fortran", "nsi", "A-C-q",   "Fortran", "fortran_indent_subprogram"), // C-M-q: fortran-indent-subprogram
    ("fortran", "nsi", "A-^",     "Fortran", "fortran_join_line"),         // M-^: fortran-join-line

    // -- Emacs Lisp (emacs-lisp-mode) ----------------------------------------
    ("elisp", "nsi", "A-C-x", "Emacs Lisp", "eval_elisp_defun"),           // C-M-x: eval-defun

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
    use zemacs_view::input::KeyEvent;

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
