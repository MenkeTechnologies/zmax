//! Vim default keymap for zemacs.
//!
//! zemacs targets vim/emacs semantics rather than Zemacs's selection-first
//! model: the keys you press are the keys vim binds. Where vim is verb-noun
//! (operator-pending: `d{motion}`, `c{motion}`, `y{motion}`), we emulate it
//! with nested submaps whose motions run `[collapse_selection, extend-motion,
//! operate]` command sequences. zemacs runs on the Zemacs engine, so each
//! operator first collapses to the cursor, extends the selection over the
//! motion, then acts — reproducing vim's "operate over the motion" behavior.
//!
//! Numeric counts (`3w`, `d2w`) work for free: the engine consumes a numeric
//! prefix and applies it to the next command.
//!
//! This is the first-step keymap. Known gaps tracked for later passes:
//!   - operator + find-char (`df<c>`, `ct<c>`): the find motion is interactive
//!     and cannot be chained inside a static sequence yet.
//!   - operator + text object (`ciw`, `di(`): needs the text-object pending
//!     state; `mi`/`ma` from the Zemacs base remain available meanwhile.
//!   - `.` repeat-last-change, vim macros `q`/`@`, marks, and Replace mode.

use std::collections::HashMap;

use super::macros::keymap;
use super::{KeyTrie, KeyTrieNode, MappableCommand, Mode};
use zemacs_core::hashmap;
use zemacs_view::input::KeyEvent;
use indexmap::IndexMap;

/// spacemacs SPC bindings that resolve to typable (`:`) commands. The keymap
/// macro can only express static commands, so these are inserted after macro
/// construction. Format: (chord, submap label, command). The chord uses the
/// same space-joined notation the port report parses, so coverage stays honest.
#[rustfmt::skip]
const SPACEMACS_TYPABLE: &[(&str, &str, &str)] = &[
    ("space f s", "Files",   ":write"),            // SPC f s : save
    ("space f S", "Files",   ":write-all"),        // SPC f S : save all
    ("space f R", "Files",   ":move"),             // SPC f R : rename file
    ("space b d", "Buffers", ":buffer-close"),     // SPC b d : kill buffer
    ("space b D", "Buffers", ":buffer-close-others"), // SPC b C-d / others
    ("space b R", "Buffers", ":reload"),           // SPC b R : revert
    ("space b N", "Buffers", ":new"),              // SPC b N : new buffer
    ("space q q", "Quit",    ":quit-all"),         // SPC q q : quit
    ("space q Q", "Quit",    ":quit-all!"),        // SPC q Q : force quit
    ("space q s", "Quit",    ":write-quit-all"),   // SPC q s : save and quit
    ("space f T", "Files",   ":theme"),            // SPC T n / theme
    ("space x l s", "Text",  ":sort"),             // SPC x l s : sort lines
    // SPC t toggles -> existing :toggle substrate (config options).
    ("space t n r", "Toggles", ":toggle line-number absolute relative"), // relative nums
    ("space t n a", "Toggles", ":toggle line-number relative absolute"), // absolute nums
    ("space t i",   "Toggles", ":toggle indent-guides.render"),          // indent guides
    ("space t a",   "Toggles", ":toggle auto-completion"),               // auto-complete
    ("space t h h", "Toggles", ":toggle cursorline"),                    // highlight line
    ("space t w",   "Toggles", ":toggle whitespace.render all none"),    // whitespace
    ("space x d w", "Text",    ":delete-trailing-whitespace"),           // SPC x d w
    ("space x l d", "Text",    ":duplicate-line"),                       // SPC x l d
    ("space x J",   "Text",    ":move-line-down"),                       // SPC x J : drag down
    ("space x K",   "Text",    ":move-line-up"),                         // SPC x K : drag up
    ("space x t c", "Text",    ":transpose-chars"),                      // SPC x t c
    ("space x t l", "Text",    ":move-line-up"),                         // SPC x t l : transpose lines
    ("space x t w", "Text",    ":transpose-words"),                      // SPC x t w
    ("space x l u", "Text",    ":uniquify-lines"),                       // SPC x l u
    ("space x d l", "Text",    ":delete-blank-lines"),                   // SPC x d l
    ("space x d space", "Text", ":just-one-space"),                      // SPC x d SPC
    ("space x i c", "Text",    ":change-case camel"),                    // SPC x i c
    ("space x i u", "Text",    ":change-case snake"),                    // SPC x i u
    ("space x i k", "Text",    ":change-case kebab"),                    // SPC x i k
    ("space x i -", "Text",    ":change-case kebab"),                    // SPC x i - : kebab-case
    ("space x i p", "Text",    ":change-case pascal"),                   // PascalCase
    ("space x i i", "Text",    ":cycle-case"),                           // SPC x i i : cycle
    ("space j n",   "Jump",    ":split-line"),                           // SPC j n : split line
    ("space j o",   "Jump",    ":split-line"),                           // SPC j o : split line, keep point
    ("space f e d", "Files",   ":config-open"),                          // SPC f e d : open dotfile/config
    ("space q f",   "Quit",    ":quit"),                                 // SPC q f : kill frame
    ("space b s",   "Buffers", ":new"),                                  // SPC b s : scratch buffer
    ("space h t",   "Help",    ":tutor"),                                // SPC h t : start the tutor
    ("space q a",   "Quit",    ":quit-all"),                             // SPC q a : quit all
    ("space q w",   "Quit",    ":write-quit"),                           // SPC q w : write & quit window
    ("space b C-d", "Buffers", ":buffer-close-others"),                  // SPC b C-d : kill other buffers
    ("space b x",   "Buffers", ":buffer-close"),                         // SPC b x : kill buffer & window
    ("space b e",   "Buffers", ":reload"),                              // SPC b e : revert/erase to disk
    ("space p k",   "Project", ":buffer-close-all"),                    // SPC p k : kill all project buffers
    ("space t l",   "Toggles", ":toggle soft-wrap.enable"),            // SPC t l : truncate/wrap lines
    ("space t V",   "Toggles", ":toggle line-number absolute relative"), // SPC t V : visual line numbers
];

/// Insert `cmd` at `path` under `root`, creating intermediate submap nodes
/// (labelled `label`) as needed. `cmd` may be a `:typable` or static command.
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
    s.split(' ').map(|k| k.parse().expect("valid key")).collect()
}

/// vim normal-mode chords that resolve to typable commands (not expressible in
/// the keymap macro). Inserted after macro construction.
#[rustfmt::skip]
const VIM_TYPABLE: &[(&str, &str, &str)] = &[
    ("Z Z", "Quit", ":write-quit"),   // ZZ: write if changed and close
    ("Z Q", "Quit", ":quit!"),        // ZQ: close without writing
    ("g J", "Goto", ":join!"),        // gJ: join lines without a space
    ("g a", "Ascii", ":character-info"), // ga: print value of char under cursor
    ("g 8", "Ascii", ":character-info"), // g8: print hex value of char under cursor
];

fn add_spacemacs_typables(normal: &mut KeyTrie) {
    if let KeyTrie::Node(root) = normal {
        for (ch, label, cmd) in SPACEMACS_TYPABLE.iter().chain(VIM_TYPABLE) {
            add_command(root, &chord(ch), label, cmd);
        }
    }
}

#[rustfmt::skip]
pub fn default() -> HashMap<Mode, KeyTrie> {
    let mut normal = keymap!({ "Normal mode"
        // --- left-hand motions ---------------------------------------------
        "h" | "left"  => move_char_left,
        "j" | "down"  => move_visual_line_down,
        "k" | "up"    => move_visual_line_up,
        "l" | "right" => move_char_right,
        "backspace"   => move_char_left,

        // --- word motions ---------------------------------------------------
        "w" => move_next_word_start,
        "b" => move_prev_word_start,
        "e" => move_next_word_end,
        "W" => move_next_long_word_start,
        "B" => move_prev_long_word_start,
        "E" => move_next_long_word_end,

        // --- line / column motions -----------------------------------------
        "0" | "home" => goto_line_start,
        "^"          => goto_first_nonwhitespace,
        "$" | "end"  => goto_line_end,
        "|"          => goto_column,
        "G"          => goto_last_line,
        "%"          => match_brackets_or_goto_percent,

        // --- marks ----------------------------------------------------------
        "m"  => set_mark,        // m{a-z} set mark
        "`"  => goto_mark,       // `{a-z} jump to mark (exact)
        "'"  => goto_mark_line,  // '{a-z} jump to mark line

        // --- registers ------------------------------------------------------
        "\"" => select_register, // "{reg} select register for next y/d/p

        // --- repeat last substitute -----------------------------------------
        "&" => repeat_substitute, // & repeat last :s on current line

        // --- screen motions -------------------------------------------------
        "H" => goto_window_top,
        "M" => goto_window_center,
        "L" => goto_window_bottom,

        // --- paragraph motions ----------------------------------------------
        "{" => goto_prev_paragraph,
        "}" => goto_next_paragraph,
        "(" => move_sentence_backward,   // ( back to start of sentence
        ")" => move_sentence_forward,    // ) forward to next sentence

        // --- find char ------------------------------------------------------
        "f" => find_next_char,
        "F" => find_prev_char,
        "t" => find_till_char,
        "T" => till_prev_char,
        ";" => repeat_last_motion,

        // --- search ---------------------------------------------------------
        "/" => search,
        "?" => rsearch,
        "n" => search_next,
        "N" => search_prev,
        "*" => [search_selection_detect_word_boundaries, search_next],
        "#" => [search_selection_detect_word_boundaries, search_prev], // backward word search

        // --- line motions to first non-blank ------------------------------
        "+" | "ret" => [move_visual_line_down, goto_first_nonwhitespace],
        "-"         => [move_visual_line_up, goto_first_nonwhitespace],
        "_"         => goto_first_nonwhitespace,

        // --- macros ---------------------------------------------------------
        "q" => vim_record_macro,  // q{reg} record (q again to stop)
        "@" => vim_replay_macro,  // @{reg} replay
        "Q" => replay_macro,      // Q replay last/default register

        // --- misc ----------------------------------------------------------
        "K" => hover,   // keyword lookup (LSP hover)

        // --- insert entry ---------------------------------------------------
        "i" => insert_mode,
        "I" => insert_at_line_start,
        "a" => append_mode,
        "A" => insert_at_line_end,
        "o" => open_below,
        "O" => open_above,

        // --- single-key edits ----------------------------------------------
        "x" => delete_selection,            // delete char under cursor
        "X" => delete_char_backward,        // delete char before cursor
        "D" => [extend_to_line_end, delete_selection],
        "C" => [extend_to_line_end, change_selection],
        "Y" => [extend_to_line_bounds, yank, collapse_selection],
        "s" => change_selection,            // substitute char
        "S" => [extend_to_line_bounds, change_selection],
        "r" => replace,
        "R" => replace_mode,                // enter Replace mode (overtype)
        "J" => join_selections,
        "~" => switch_case,
        "p" => paste_after,
        "P" => paste_before,
        "u" => undo,
        "C-r" => redo,

        // --- operator-pending: delete --------------------------------------
        "d" => { "delete"
            "d" => [collapse_selection, extend_to_line_bounds, delete_selection],
            "w" => [collapse_selection, extend_next_word_start, delete_selection],
            "W" => [collapse_selection, extend_next_long_word_start, delete_selection],
            "e" => [collapse_selection, extend_next_word_end, delete_selection],
            "E" => [collapse_selection, extend_next_long_word_end, delete_selection],
            "b" => [collapse_selection, extend_prev_word_start, delete_selection],
            "B" => [collapse_selection, extend_prev_long_word_start, delete_selection],
            "$" => [collapse_selection, extend_to_line_end, delete_selection],
            "0" => [collapse_selection, extend_to_line_start, delete_selection],
            "^" => [collapse_selection, extend_to_first_nonwhitespace, delete_selection],
            "G" => [collapse_selection, extend_to_last_line, delete_selection],
            "%" => [match_brackets, delete_selection],
            "i" => delete_textobject_inner,   // diw, di(, dip, ...
            "a" => delete_textobject_around,  // daw, da(, ...
            "f" => delete_find_char_forward,  // df<c>
            "t" => delete_till_char_forward,  // dt<c>
            "F" => delete_find_char_backward, // dF<c>
            "T" => delete_till_char_backward, // dT<c>
        },

        // --- operator-pending: change --------------------------------------
        "c" => { "change"
            "c" => [collapse_selection, extend_to_line_bounds, change_selection],
            "w" => [collapse_selection, extend_next_word_end, change_selection],
            "W" => [collapse_selection, extend_next_long_word_end, change_selection],
            "e" => [collapse_selection, extend_next_word_end, change_selection],
            "E" => [collapse_selection, extend_next_long_word_end, change_selection],
            "b" => [collapse_selection, extend_prev_word_start, change_selection],
            "B" => [collapse_selection, extend_prev_long_word_start, change_selection],
            "$" => [collapse_selection, extend_to_line_end, change_selection],
            "^" => [collapse_selection, extend_to_first_nonwhitespace, change_selection],
            "i" => change_textobject_inner,   // ciw, ci(, cip, ...
            "a" => change_textobject_around,  // caw, ca(, ...
            "f" => change_find_char_forward,  // cf<c>
            "t" => change_till_char_forward,  // ct<c>
            "F" => change_find_char_backward, // cF<c>
            "T" => change_till_char_backward, // cT<c>
        },

        // --- operator-pending: yank ----------------------------------------
        "y" => { "yank"
            "y" => [collapse_selection, extend_to_line_bounds, yank, collapse_selection],
            "w" => [collapse_selection, extend_next_word_start, yank, collapse_selection],
            "W" => [collapse_selection, extend_next_long_word_start, yank, collapse_selection],
            "e" => [collapse_selection, extend_next_word_end, yank, collapse_selection],
            "b" => [collapse_selection, extend_prev_word_start, yank, collapse_selection],
            "$" => [collapse_selection, extend_to_line_end, yank, collapse_selection],
            "0" => [collapse_selection, extend_to_line_start, yank, collapse_selection],
            "^" => [collapse_selection, extend_to_first_nonwhitespace, yank, collapse_selection],
            "G" => [collapse_selection, extend_to_last_line, yank, collapse_selection],
            "i" => yank_textobject_inner,     // yiw, yi(, yip, ...
            "a" => yank_textobject_around,    // yaw, ya(, ...
            "f" => yank_find_char_forward,    // yf<c>
            "t" => yank_till_char_forward,    // yt<c>
            "F" => yank_find_char_backward,   // yF<c>
            "T" => yank_till_char_backward,   // yT<c>
        },

        // --- indent operators ----------------------------------------------
        ">" => indent,
        "<" => unindent,

        // --- filter operator (vim !{motion}{cmd}, !!{cmd}) -----------------
        // vim `!` is always linewise: it selects the lines covered by the
        // motion, then pipes them through an external command. shell_pipe
        // prompts for the command and replaces the selection with its output.
        "!" => { "filter"
            "!" => [extend_to_line_bounds, shell_pipe],              // !! current line
            "j" => [extend_line_below, extend_to_line_bounds, shell_pipe],
            "k" => [extend_line_up, extend_to_line_bounds, shell_pipe],
            "G" => [extend_to_last_line, extend_to_line_bounds, shell_pipe],
        },

        // --- visual mode ----------------------------------------------------
        "v" => select_mode,
        "V" => [extend_to_line_bounds, select_mode],
        // C-v: visual block. zemacs runs on the Zemacs engine, which has no
        // rectangular-selection mode, so block editing is emulated with
        // multi-cursor: enter Visual, set the column width with l/e/$, then
        // grow the block downward with C-v (or C) — each press copies the
        // current selection onto the next line. I/A block-insert, c/d/x act
        // per cursor (see the Visual-mode bindings).
        "C-v" => select_mode,

        // --- g submap -------------------------------------------------------
        "g" => { "Goto"
            // case-change operators (gU / gu / g~ + motion)
            "U" => { "Uppercase"
                "U" => [extend_to_line_bounds, switch_to_uppercase, collapse_selection],
                "w" => [collapse_selection, extend_next_word_start, switch_to_uppercase, collapse_selection],
                "W" => [collapse_selection, extend_next_long_word_start, switch_to_uppercase, collapse_selection],
                "e" => [collapse_selection, extend_next_word_end, switch_to_uppercase, collapse_selection],
                "b" => [collapse_selection, extend_prev_word_start, switch_to_uppercase, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, switch_to_uppercase, collapse_selection],
                "^" => [collapse_selection, extend_to_first_nonwhitespace, switch_to_uppercase, collapse_selection],
            },
            "u" => { "Lowercase"
                "u" => [extend_to_line_bounds, switch_to_lowercase, collapse_selection],
                "w" => [collapse_selection, extend_next_word_start, switch_to_lowercase, collapse_selection],
                "W" => [collapse_selection, extend_next_long_word_start, switch_to_lowercase, collapse_selection],
                "e" => [collapse_selection, extend_next_word_end, switch_to_lowercase, collapse_selection],
                "b" => [collapse_selection, extend_prev_word_start, switch_to_lowercase, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, switch_to_lowercase, collapse_selection],
                "^" => [collapse_selection, extend_to_first_nonwhitespace, switch_to_lowercase, collapse_selection],
            },
            "~" => { "Toggle case"
                "~" => [extend_to_line_bounds, switch_case, collapse_selection],
                "w" => [collapse_selection, extend_next_word_start, switch_case, collapse_selection],
                "W" => [collapse_selection, extend_next_long_word_start, switch_case, collapse_selection],
                "e" => [collapse_selection, extend_next_word_end, switch_case, collapse_selection],
                "b" => [collapse_selection, extend_prev_word_start, switch_case, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, switch_case, collapse_selection],
                "^" => [collapse_selection, extend_to_first_nonwhitespace, switch_case, collapse_selection],
            },
            // gq{motion} / gw{motion}: reformat text. zemacs reformats via the
            // LSP formatter (vim uses formatprg/textwidth) — partial but same intent.
            "q" => { "Format"
                "q" => [extend_to_line_bounds, format_selections, collapse_selection],
                "j" => [extend_line_below, extend_to_line_bounds, format_selections, collapse_selection],
                "G" => [extend_to_last_line, extend_to_line_bounds, format_selections, collapse_selection],
            },
            "w" => { "Format"
                "w" => [extend_to_line_bounds, format_selections, collapse_selection],
                "j" => [extend_line_below, extend_to_line_bounds, format_selections, collapse_selection],
                "G" => [extend_to_last_line, extend_to_line_bounds, format_selections, collapse_selection],
            },

            "g" => goto_file_start,
            "&" => repeat_substitute_global,   // g& repeat last :s whole file
            ";" => goto_last_modification,     // g; goto last change position
            "E" => move_prev_long_word_end,    // gE back to end of previous WORD
            "e" => move_prev_word_end,         // ge back to end of previous word
            "j" => move_line_down,
            "k" => move_line_up,
            "h" => goto_line_start,
            "l" => goto_line_end,
            "0" => goto_line_start,            // g0 leftmost (screen line)
            "$" => goto_line_end,              // g$ rightmost (screen line)
            "^" => goto_first_nonwhitespace,   // g^ first non-blank (screen line)
            "_" => goto_line_end,              // g_ last non-blank (approx)
            "I" => insert_at_line_start,       // gI insert at column 1
            "d" => goto_definition,
            "D" => goto_declaration,
            "y" => goto_type_definition,
            "r" => goto_reference,
            "i" => insert_at_last_insert,      // gi insert at last insert position
            "v" => reselect_visual,            // gv reselect last visual area
            "f" => goto_file,
            "x" => goto_file,                 // gx: open file/URL under cursor (goto_file opens URLs externally)
            // ga (print char ascii/unicode value) is bound via VIM_TYPABLE to
            // :character-info — vim's ga, not zemacs's goto-last-accessed-file.
            "m" => goto_last_modified_file,
            "t" => goto_next_buffer,           // gt: next tabpage -> next buffer
            "T" => goto_previous_buffer,       // gT: previous tabpage -> previous buffer
            "p" => paste_after,                // gp: paste after (vim leaves cursor after)
            "P" => paste_before,               // gP: paste before
            "n" => search_next,                // gn: select the next search match
            "N" => search_prev,                // gN: select the previous search match
            "." => goto_last_modification,
        },

        // --- z submap (view + folds) ---------------------------------------
        "z" => { "View"
            "z" => align_view_center,
            "t" => align_view_top,
            "b" => align_view_bottom,
            "." => [align_view_center, goto_first_nonwhitespace], // z. center + first non-blank
            "-" => [align_view_bottom, goto_first_nonwhitespace], // z- bottom + first non-blank
            "ret" => [align_view_top, goto_first_nonwhitespace],  // z<CR> top + first non-blank

            // folds (vim z* family)
            "a" => fold_toggle,       // za toggle fold under cursor
            "o" => fold_open,         // zo open fold
            "O" => fold_open,         // zO open folds recursively (approx: open at cursor)
            "c" => fold_close,        // zc close fold
            "C" => fold_close,        // zC close folds recursively (approx)
            "v" => fold_open,         // zv view cursor: open enough folds to see it
            "R" => fold_open_all,     // zR open all folds
            "M" => fold_close_all,    // zM close all folds
            "d" => fold_delete,       // zd delete fold under cursor
            "E" => fold_delete_all,   // zE eliminate all folds
            "j" => fold_next,         // zj move to next fold
            "k" => fold_prev,         // zk move to previous fold
            // zf{motion}: create a fold over the motion (vim operator)
            "f" => { "Create fold"
                "j" => [extend_line_below, extend_to_line_bounds, fold_create],
                "k" => [extend_line_up, extend_to_line_bounds, fold_create],
                "G" => [extend_to_last_line, fold_create],
                "}" => [goto_next_paragraph, fold_create],
                "f" => [extend_to_line_bounds, fold_create],
            },
        },

        // --- bracket submaps (vim unimpaired-ish) --------------------------
        "[" => { "Prev"
            "[" => goto_prev_paragraph,
            "d" => goto_prev_diag,
            "g" => goto_prev_change,
            "c" => goto_prev_change,      // [c back to start of prev change (diff hunk)
            "f" => goto_file,             // [f same as gf: open file under cursor
            "m" => goto_prev_function,    // [m back to start of member/function
            "b" => goto_previous_buffer,  // [b previous buffer (unimpaired-style)
            "/" => goto_prev_comment,     // [/ previous comment
            "p" => paste_before,          // [p paste before (linewise, adjust indent)
        },
        "]" => { "Next"
            "]" => goto_next_paragraph,
            "d" => goto_next_diag,
            "g" => goto_next_change,
            "c" => goto_next_change,      // ]c forward to start of next change (diff hunk)
            "f" => goto_file,             // ]f same as gf: open file under cursor
            "m" => goto_next_function,    // ]m forward to next member/function
            "b" => goto_next_buffer,      // ]b next buffer (unimpaired-style)
            "/" => goto_next_comment,     // ]/ next comment
            "p" => paste_after,           // ]p paste after (linewise, adjust indent)
        },

        // --- window commands (C-w) -----------------------------------------
        "C-w" => { "Window"
            "s" | "C-s" => hsplit,
            "v" | "C-v" => vsplit,
            "w" | "C-w" => rotate_view,
            "r" => rotate_view,
            "q" | "C-q" => wclose,
            "d" | "C-d" => wclose,
            "o" | "C-o" => wonly,
            "h" | "C-h" => jump_view_left,
            "j" | "C-j" => jump_view_down,
            "k" | "C-k" => jump_view_up,
            "l" | "C-l" => jump_view_right,
            "left"  => jump_view_left,
            "down"  => jump_view_down,
            "up"    => jump_view_up,
            "right" => jump_view_right,
            "H" => swap_view_left,            // C-w H: move window to the far left
            "J" => swap_view_down,            // C-w J: move window to the very bottom
            "K" => swap_view_up,              // C-w K: move window to the very top
            "L" => swap_view_right,           // C-w L: move window to the far right
            "R" => rotate_view_reverse,       // C-w R: rotate windows upwards
            "x" | "C-x" => transpose_view,    // C-w x: exchange current window with next
            "n" | "C-n" => hsplit_new,        // C-w n: open new window
            "/" => vsplit,                    // spacemacs SPC w / : split vertically
            "-" => hsplit,                    // spacemacs SPC w - : split horizontally
            "c" => wclose,                    // spacemacs SPC w c : close window
            "m" => wonly,                     // spacemacs SPC w m : maximize (only)
            "S" => hsplit,                    // spacemacs SPC w S / vim C-w S : split & focus
            "V" => vsplit,                    // spacemacs SPC w V : vsplit & focus
        },

        // --- scrolling / jumps ---------------------------------------------
        "C-d" => page_cursor_half_down,
        "C-u" => page_cursor_half_up,
        "C-f" | "pagedown" => page_down,
        "C-b" | "pageup"   => page_up,
        "C-o" => jump_backward,
        "C-i" | "tab" => jump_forward,
        "C-e" => scroll_down,
        "C-y" => scroll_up,

        // --- ctrl/arrow motion aliases (vim index.txt) ---------------------
        "C-h" => move_char_left,         // CTRL-H = h
        "C-j" => move_visual_line_down,  // CTRL-J = j
        "C-n" => move_visual_line_down,  // CTRL-N = j
        "C-p" => move_visual_line_up,    // CTRL-P = k
        "C-left"  => move_prev_word_start,  // <C-Left>/<S-Left> = b
        "S-left"  => move_prev_word_start,
        "C-right" => move_next_word_start,  // <C-Right>/<S-Right> = w
        "S-right" => move_next_word_start,
        "C-home"  => goto_file_start,    // <C-Home> = gg
        "C-end"   => goto_last_line,     // <C-End> = G
        "S-down"  => page_down,          // <S-Down> = CTRL-F
        "S-up"    => page_up,            // <S-Up> = CTRL-B
        "ins"     => insert_mode,        // <Insert> = i
        "C-]"     => goto_definition,    // CTRL-] = :ta (jump to tag)

        // --- emacs/readline keys (Meta space is free in the vim keymap) -----
        "A-x"     => command_palette,     // M-x
        "A-<"     => goto_file_start,     // M-< beginning of buffer
        "A->"     => goto_last_line,      // M-> end of buffer
        "A-f"     => move_next_word_start,// M-f forward-word
        "A-b"     => move_prev_word_start,// M-b backward-word
        "A-d"     => delete_word_forward, // M-d kill-word
        "A-w"     => yank,                // M-w kill-ring-save (copy)
        "A-v"     => page_up,             // M-v scroll-down
        "C-space" => select_mode,         // C-SPC set-mark
        "C-g"     => collapse_selection,  // C-g keyboard-quit
        "C-l"     => align_view_center,   // C-l recenter
        "C-s"     => search,              // C-s isearch-forward
        "C-/"     => undo,                // C-/ undo
        "C-_"     => undo,                // C-_ undo

        // --- = reindent operator (vim ==, ={motion}) -----------------------
        "=" => { "Indent"
            "=" => indent,                              // == reindent line
            "j" => [extend_line_below, indent],
            "k" => [extend_line_up, indent],
            "G" => [extend_to_last_line, indent],
        },

        // --- increment / decrement -----------------------------------------
        "C-a" => increment,
        "C-x" => decrement,

        // --- misc -----------------------------------------------------------
        ":" => command_mode,
        "C-z" => suspend,
        "esc" => collapse_selection,

        // --- leader (space) — kept for pickers / LSP / commands ------------
        // --- leader (space): spacemacs SPC tree ----------------------------
        // Structured to mirror spacemacs' SPC keybinding tree. Only bindings
        // that map to a real zemacs static command are present; spacemacs
        // bindings needing a typable (`:w` save, `:q` quit, `:bd`) are not yet
        // expressible in the keymap macro and remain tracked as absent.
        "," => keep_primary_selection,
        "space" => { "Leader (spacemacs SPC)"
            "space" => command_palette,            // SPC SPC : M-x
            "tab"   => goto_last_accessed_file,    // SPC TAB : alternate buffer
            ":"     => command_mode,               // SPC :   : Ex command
            "/"     => global_search,              // SPC /   : search project
            "?"     => command_palette,            // SPC ?   : commands
            "'"     => last_picker,                // SPC '   : resume picker
            ";"     => toggle_comments,            // SPC ;   : comment operator

            "f" => { "Files"
                "f" => file_picker,                            // SPC f f
                "r" => goto_last_modified_file,                // SPC f r
                "t" => file_explorer,                          // SPC f t
                "d" => file_explorer_in_current_buffer_directory, // SPC f d
                "j" => file_explorer_in_current_buffer_directory, // SPC f j : dired
            },
            "b" => { "Buffers"
                "b" => buffer_picker,              // SPC b b
                "n" => goto_next_buffer,           // SPC b n
                "p" => goto_previous_buffer,       // SPC b p
                "m" => changed_file_picker,        // SPC b m
                "Y" => [select_all, yank_to_clipboard, collapse_selection], // SPC b Y
            },
            // Kept identical to the `C-w` window submap (see aliased-modes test).
            "w" => { "Window"
                "s" | "C-s" => hsplit,
                "v" | "C-v" => vsplit,
                "w" | "C-w" => rotate_view,
                "r" => rotate_view,
                "q" | "C-q" => wclose,
                "d" | "C-d" => wclose,
                "o" | "C-o" => wonly,
                "h" | "C-h" => jump_view_left,
                "j" | "C-j" => jump_view_down,
                "k" | "C-k" => jump_view_up,
                "l" | "C-l" => jump_view_right,
                "left"  => jump_view_left,
                "down"  => jump_view_down,
                "up"    => jump_view_up,
                "right" => jump_view_right,
                "H" => swap_view_left,
                "J" => swap_view_down,
                "K" => swap_view_up,
                "L" => swap_view_right,
                "R" => rotate_view_reverse,
                "x" | "C-x" => transpose_view,
                "n" | "C-n" => hsplit_new,
                "/" => vsplit,
                "-" => hsplit,
                "c" => wclose,
                "m" => wonly,
                "S" => hsplit,
                "V" => vsplit,
            },
            "s" => { "Search"
                "s" => global_search,              // SPC s s
                "f" => global_search,              // SPC s f
                "b" => global_search,              // SPC s b
                "p" => global_search,              // SPC s p
                "j" => symbol_picker,              // SPC s j
                "e" => select_references_to_symbol_under_cursor, // SPC s e : edit occurrences
                "h" => select_references_to_symbol_under_cursor, // SPC s h : highlight symbol
                "S" => workspace_symbol_picker,
                // ag / grep / ack search families all map to project-wide search.
                "a" => { "ag"
                    "a" => global_search, "b" => global_search, "d" => global_search,
                    "f" => global_search, "p" => global_search,
                },
                "g" => { "grep"
                    "g" => global_search, "b" => global_search, "f" => global_search,
                    "d" => global_search, "p" => global_search,
                },
                "k" => { "ack"
                    "b" => global_search, "d" => global_search,
                    "f" => global_search, "p" => global_search,
                },
                "r" => { "rg"
                    "r" => global_search, "b" => global_search, "f" => global_search,
                    "d" => global_search, "p" => global_search,
                },
            },
            "p" => { "Project"
                "f" => file_picker,                // SPC p f
                "p" => file_picker,                // SPC p p
                "b" => buffer_picker,              // SPC p b : project buffer
                "h" => file_picker,                // SPC p h : find file
                "s" => global_search,              // SPC p s
                "r" => goto_last_modified_file,    // SPC p r
                "t" => file_explorer,              // SPC p t : project tree (treemacs)
                "d" => file_explorer,              // SPC p d : find directory
            },
            "e" => { "Errors"
                "l" => diagnostics_picker,             // SPC e l
                "L" => workspace_diagnostics_picker,   // SPC e L
                "n" => goto_next_diag,                 // SPC e n
                "p" => goto_prev_diag,                 // SPC e p
                "f" => goto_first_diag,                // SPC e f
                "." => goto_last_diag,
            },
            "c" => { "Comments"
                "l" => toggle_line_comments,       // SPC c l
                "c" => toggle_comments,            // SPC c c
                "b" => toggle_block_comments,      // SPC c b
                "p" => toggle_comments,            // SPC c p : comment paragraph
                "h" => toggle_comments,            // SPC c h : hide/show comments (toggle)
            },
            "j" => { "Jump"
                "i" => symbol_picker,              // SPC j i
                "j" => jumplist_picker,            // SPC j j
                "0" => goto_line_start,            // SPC j 0
                "$" => goto_line_end,              // SPC j $
                "b" => jump_backward,              // SPC j b : back to prev location
                "d" => file_explorer_in_current_buffer_directory, // SPC j d : dir listing
                "c" => goto_last_change,           // SPC j c : go to last change
                "k" => [move_visual_line_down, indent], // SPC j k : next line + indent
                "u" => goto_file,                  // SPC j u : jump to URL/file under cursor
            },
            "g" => { "Goto (LSP)"
                "d" => goto_definition,
                "D" => goto_declaration,
                "r" => goto_reference,
                "i" => goto_implementation,
                "y" => goto_type_definition,
            },
            "l" => { "LSP"
                "r" => rename_symbol,              // SPC l r
                "a" => code_action,                // SPC l a
                "k" => hover,                      // SPC l k
                "s" => signature_help,             // SPC l s
                "f" => format_selections,          // SPC l f
            },
            "v" => expand_selection,               // SPC v : expand region
            "x" => { "Text"
                "u" => switch_to_lowercase,        // SPC x u : lowercase
                "tab" => indent,                   // SPC x TAB : indent region
                "a" => { "Align"
                    "a" => align_selections,       // SPC x a a : align region
                    "&" => align_selections,       // SPC x a & : align at &
                    "c" => align_selections,       // SPC x a c : align indentation
                    "l" => align_selections,       // SPC x a l : left-align
                    "r" => align_selections,       // SPC x a r : align at regexp
                    "m" => align_selections,       // SPC x a m : align at math operators
                },
            },
            "r" => { "Resume / registers"
                "l" => last_picker,                // SPC r l : resume picker
                "e" => register_picker,            // SPC r e : registers
                "r" => register_picker,            // SPC r r : show registers
                "y" => register_picker,            // SPC r y : kill ring
            },
            "a" => { "Applications"
                "d" => file_explorer,              // SPC a d : dired (file manager)
                "r" => file_explorer,              // SPC a r : ranger (file browser)
                "f" => file_explorer,              // SPC a f : file tree
            },
            "k" => { "Lisp (sexp)"
                // navigation maps onto the tree-sitter node commands
                "0" => move_parent_node_start,     // SPC k 0 : beginning of sexp
                "$" => move_parent_node_end,       // SPC k $ : end of sexp
                "U" => expand_selection,           // SPC k U : up to parent sexp
                "I" => [move_parent_node_start, insert_mode], // SPC k I : begin + insert
                "h" => select_prev_sibling,        // SPC k h : previous symbol
                "l" => select_next_sibling,        // SPC k l : next symbol
                "j" => shrink_selection,           // SPC k j : into child
                "k" => expand_selection,           // SPC k k : out to parent
                "y" => [expand_selection, yank, collapse_selection], // SPC k y : copy expression
                "v" => select_mode,                // SPC k v : visual select
                "C-v" => select_mode,              // SPC k C-v : block-wise selection
                "C-r" => redo,                     // SPC k C-r : redo
                "w" => wrap_sexp,                  // SPC k w : wrap with parens
                "d" => { "Delete"
                    "x" => [expand_selection, delete_selection], // SPC k dx : delete sexp
                    "s" => [expand_selection, delete_selection], // SPC k ds : delete symbol
                    "w" => [collapse_selection, extend_next_word_start, delete_selection], // SPC k dw
                },
            },
            "h" => { "Help"
                "k" => command_palette,            // SPC h k : describe key / commands
                "?" => command_palette,            // SPC h ? : list bindings
                "c" => command_palette,            // SPC h c : describe command
            },
        },
    });

    // Visual / select mode: motions extend, operators act directly.
    let select = keymap!({ "Visual mode"
        "h" | "left"  => extend_char_left,
        "j" | "down"  => extend_visual_line_down,
        "k" | "up"    => extend_visual_line_up,
        "l" | "right" => extend_char_right,

        "w" => extend_next_word_start,
        "b" => extend_prev_word_start,
        "e" => extend_next_word_end,
        "W" => extend_next_long_word_start,
        "B" => extend_prev_long_word_start,
        "E" => extend_next_long_word_end,

        "0" | "home" => extend_to_line_start,
        "^"          => extend_to_first_nonwhitespace,
        "$" | "end"  => extend_to_line_end,
        "G"          => extend_to_last_line,
        "%"          => match_brackets_or_goto_percent,
        "{"          => goto_prev_paragraph,
        "}"          => goto_next_paragraph,
        "("          => move_sentence_backward,
        ")"          => move_sentence_forward,

        "f" => extend_next_char,
        "F" => extend_prev_char,
        "t" => extend_till_char,
        "T" => extend_till_prev_char,
        ";" => repeat_last_motion,

        "i" => select_textobject_inner,
        "a" => select_textobject_around,

        "d" | "x" => [delete_selection, normal_mode],
        "c" | "s" => change_selection,
        "y"       => [yank, collapse_selection, normal_mode],
        "p"       => replace_with_yanked,
        "r"       => replace,
        "J"       => [join_selections, normal_mode],
        "~"       => switch_case,
        "u"       => [switch_to_lowercase, normal_mode],
        "U"       => [switch_to_uppercase, normal_mode],
        ">"       => [indent, normal_mode],
        "<"       => [unindent, normal_mode],
        "o"       => flip_selections,
        "O"       => flip_selections,          // move to the other corner/end of the selection

        // --- visual block (multi-cursor emulation) -------------------------
        // C-v grows the block: copy the current selection onto the next line,
        // building a vertical stack of cursors. I and A then block-insert at
        // the left edge and block-append at the right edge of every line in
        // the block (vim's CTRL-V I.../A...).
        "C-v"     => copy_selection_on_next_line,
        "I"       => [collapse_selection, insert_mode],
        "A"       => append_mode,
        "V"       => extend_to_line_bounds,
        "P"       => replace_with_yanked,      // replace the highlighted area with a register
        "=" => [format_selections, normal_mode], // reformat/reindent the highlighted lines

        // filter highlighted text through an external command (vim visual !)
        "!"       => [shell_pipe, normal_mode],

        // linewise visual operators: extend to whole lines, then act
        "D" | "X" => [extend_to_line_bounds, delete_selection, normal_mode],
        "Y"       => [extend_to_line_bounds, yank, collapse_selection, normal_mode],
        "C" | "S" | "R" => [extend_to_line_bounds, change_selection],

        // zf: create a fold over the highlighted lines (vim visual zf)
        "z" => { "Fold"
            "f" => [fold_create, normal_mode],
        },

        // gq / gw: reformat the highlighted lines (LSP formatter)
        "g" => { "Goto"
            "q" => [format_selections, normal_mode],
            "w" => [format_selections, normal_mode],
            "J" => [join_selections, normal_mode],   // gJ: join lines, no space (approx)
            "C-a" => increment,                      // g CTRL-A: increment in selection
            "C-x" => decrement,                      // g CTRL-X: decrement in selection
        },

        "C-a" => increment,
        "C-x" => decrement,

        // visual-mode extras (vim v_*)
        "K"   => hover,                              // run keywordprg on the area
        "C-]" => goto_definition,                    // jump to highlighted tag
        "v"   => [collapse_selection, normal_mode],  // v: stop Visual / back to charwise
        "backspace" | "C-h" => [delete_selection, normal_mode], // Select: delete area

        ":" => command_mode,
        "C-c" => [save_visual_selection, collapse_selection, normal_mode], // stop Visual mode
        "esc" => [save_visual_selection, collapse_selection, normal_mode],
    });

    // Insert mode: vim-style editing keys.
    let insert = keymap!({ "Insert mode"
        "esc" => [mark_insert_exit, normal_mode],
        "C-c" => [mark_insert_exit, normal_mode],
        "C-[" => [mark_insert_exit, normal_mode],   // CTRL-[ = <Esc>

        "backspace" | "C-h" => delete_char_backward,
        "del"               => delete_char_forward,
        "C-w"               => delete_word_backward,
        "A-backspace"       => delete_word_backward,
        "A-d"               => delete_word_forward,
        "C-u"               => kill_to_line_start,
        "C-k"               => kill_to_line_end,

        // indent the current line (vim i_CTRL-T / i_CTRL-D)
        "C-t"   => indent,
        "C-d"   => unindent,

        // keyword/omni completion (vim i_CTRL-N / i_CTRL-P)
        "C-n"   => completion,
        "C-p"   => completion,

        "ret"   => insert_newline,
        "C-j"   => insert_newline,
        "tab"   => insert_tab,

        "C-r"   => insert_register,
        "C-a"   => insert_at_line_start,
        "C-e"   => insert_at_line_end,

        "up"    => move_visual_line_up,
        "down"  => move_visual_line_down,
        "left"  => move_char_left,
        "right" => move_char_right,
        "home"  => goto_line_start,
        "end"   => goto_line_end_newline,

        // word/file motions with modifiers (vim i_<C-Left> etc.)
        "C-left"  => move_prev_word_start,
        "S-left"  => move_prev_word_start,
        "C-right" => move_next_word_start,
        "S-right" => move_next_word_start,
        "C-home"  => goto_file_start,
        "C-end"   => goto_file_end,

        // emacs/readline editing keys in insert mode
        "C-f"     => move_char_right,      // C-f forward-char
        "C-b"     => move_char_left,       // C-b backward-char
        "C-v"     => page_down,            // C-v scroll-up
        "A-f"     => move_next_word_start, // M-f forward-word
        "A-b"     => move_prev_word_start, // M-b backward-word
        "A-v"     => page_up,              // M-v scroll-down
        "A-<"     => goto_file_start,      // M-< beginning of buffer
        "A->"     => goto_file_end,        // M-> end of buffer
        "C-/"     => undo,                 // C-/ undo
        "C-_"     => undo,                 // C-_ undo

        "pageup"   => page_up,
        "pagedown" => page_down,
        "S-up"     => page_up,      // <S-Up> = <PageUp>
        "S-down"   => page_down,    // <S-Down> = <PageDown>
    });

    add_spacemacs_typables(&mut normal);

    hashmap!(
        Mode::Normal => normal,
        Mode::Select => select,
        Mode::Insert => insert,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{KeyTrie, MappableCommand};
    use zemacs_view::input::KeyEvent;

    fn cmd_name(trie: &KeyTrie) -> Option<&str> {
        match trie {
            KeyTrie::MappableCommand(MappableCommand::Static { name, .. }) => Some(name),
            _ => None,
        }
    }

    /// Walk a chord like "g l" or "d w" through the trie and return the leaf.
    fn resolve<'a>(root: &'a KeyTrie, chord: &str) -> Option<&'a KeyTrie> {
        let keys: Vec<KeyEvent> = chord.split(' ').map(|k| k.parse().unwrap()).collect();
        root.search(&keys)
    }

    #[test]
    fn vim_keymap_constructs() {
        // Panics here would mean a duplicate key within a node.
        let km = default();
        assert!(km.contains_key(&Mode::Normal));
        assert!(km.contains_key(&Mode::Select));
        assert!(km.contains_key(&Mode::Insert));
    }

    #[test]
    fn vim_direct_motions_bound_to_vim_keys() {
        let km = default();
        let n = &km[&Mode::Normal];
        // The keys vim users actually press now resolve to the right command.
        assert_eq!(cmd_name(resolve(n, "$").unwrap()), Some("goto_line_end"));
        assert_eq!(cmd_name(resolve(n, "0").unwrap()), Some("goto_line_start"));
        assert_eq!(
            cmd_name(resolve(n, "^").unwrap()),
            Some("goto_first_nonwhitespace")
        );
        assert_eq!(
            cmd_name(resolve(n, "%").unwrap()),
            Some("match_brackets_or_goto_percent")
        );
        assert_eq!(cmd_name(resolve(n, "G").unwrap()), Some("goto_last_line"));
        assert_eq!(cmd_name(resolve(n, "H").unwrap()), Some("goto_window_top"));
        assert_eq!(cmd_name(resolve(n, "x").unwrap()), Some("delete_selection"));
        assert_eq!(cmd_name(resolve(n, "i").unwrap()), Some("insert_mode"));
        assert_eq!(cmd_name(resolve(n, "a").unwrap()), Some("append_mode"));
    }

    #[test]
    fn vim_window_family_bound() {
        let km = default();
        let n = &km[&Mode::Normal];
        // vim CTRL-W window moves map onto zemacs's view commands.
        assert_eq!(cmd_name(resolve(n, "C-w H").unwrap()), Some("swap_view_left"));
        assert_eq!(cmd_name(resolve(n, "C-w J").unwrap()), Some("swap_view_down"));
        assert_eq!(cmd_name(resolve(n, "C-w K").unwrap()), Some("swap_view_up"));
        assert_eq!(
            cmd_name(resolve(n, "C-w L").unwrap()),
            Some("swap_view_right")
        );
        assert_eq!(
            cmd_name(resolve(n, "C-w R").unwrap()),
            Some("rotate_view_reverse")
        );
        assert_eq!(
            cmd_name(resolve(n, "C-w x").unwrap()),
            Some("transpose_view")
        );
        // CTRL-W + arrow navigates like CTRL-W h/j/k/l.
        assert_eq!(
            cmd_name(resolve(n, "C-w left").unwrap()),
            Some("jump_view_left")
        );
    }

    #[test]
    fn vim_g_prefix_is_vim_not_helix() {
        let km = default();
        let n = &km[&Mode::Normal];
        // ge/gn/gN carry vim meaning, not the zemacs bindings they collided with.
        assert_eq!(
            cmd_name(resolve(n, "g e").unwrap()),
            Some("move_prev_word_end")
        );
        assert_eq!(cmd_name(resolve(n, "g n").unwrap()), Some("search_next"));
        assert_eq!(cmd_name(resolve(n, "g N").unwrap()), Some("search_prev"));
        // buffer nav relocated to unimpaired-style [b / ]b.
        assert_eq!(
            cmd_name(resolve(n, "] b").unwrap()),
            Some("goto_next_buffer")
        );
        assert_eq!(
            cmd_name(resolve(n, "[ b").unwrap()),
            Some("goto_previous_buffer")
        );
    }

    #[test]
    fn spacemacs_leader_tree_bound() {
        let km = default();
        let n = &km[&Mode::Normal];
        // spacemacs SPC tree resolves to the expected zemacs commands.
        assert_eq!(cmd_name(resolve(n, "space f f").unwrap()), Some("file_picker"));
        assert_eq!(
            cmd_name(resolve(n, "space b b").unwrap()),
            Some("buffer_picker")
        );
        assert_eq!(
            cmd_name(resolve(n, "space space").unwrap()),
            Some("command_palette")
        );
        assert_eq!(
            cmd_name(resolve(n, "space e n").unwrap()),
            Some("goto_next_diag")
        );
        assert_eq!(
            cmd_name(resolve(n, "space s s").unwrap()),
            Some("global_search")
        );
    }

    #[test]
    fn spacemacs_typable_bindings_inserted() {
        let km = default();
        let n = &km[&Mode::Normal];
        // SPC f s / SPC q q etc. resolve to typable commands inserted post-macro.
        for (chord_str, _, cmd) in SPACEMACS_TYPABLE.iter().chain(VIM_TYPABLE) {
            let leaf = resolve(n, chord_str)
                .unwrap_or_else(|| panic!("{chord_str} did not resolve"));
            // The bound leaf must equal what the command string parses to, and
            // it must be a typable command.
            let expected = KeyTrie::MappableCommand(
                cmd.parse::<MappableCommand>().expect("valid command"),
            );
            assert_eq!(leaf, &expected, "wrong command for {chord_str}");
            assert!(
                matches!(leaf, KeyTrie::MappableCommand(MappableCommand::Typable { .. })),
                "{chord_str} should be a typable command"
            );
        }
    }

    #[test]
    fn vim_case_operators_are_sequences() {
        let km = default();
        let n = &km[&Mode::Normal];
        for chord in ["g U U", "g U w", "g u u", "g u w", "g ~ ~", "g ~ w"] {
            let leaf =
                resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert!(
                matches!(leaf, KeyTrie::Sequence(_)),
                "{chord} should be a case-operator sequence"
            );
        }
    }

    #[test]
    fn spacemacs_composite_bindings_are_sequences() {
        let km = default();
        let n = &km[&Mode::Normal];
        for chord in ["space j k", "space b Y"] {
            let leaf =
                resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert!(
                matches!(leaf, KeyTrie::Sequence(_)),
                "{chord} should be a command sequence"
            );
        }
        assert_eq!(
            cmd_name(resolve(n, "space x tab").unwrap()),
            Some("indent")
        );
    }

    #[test]
    fn vim_ported_motion_aliases_and_operators() {
        let km = default();
        let n = &km[&Mode::Normal];
        // ctrl/arrow motion aliases from index.txt
        assert_eq!(cmd_name(resolve(n, "C-h").unwrap()), Some("move_char_left"));
        assert_eq!(cmd_name(resolve(n, "C-left").unwrap()), Some("move_prev_word_start"));
        assert_eq!(cmd_name(resolve(n, "C-right").unwrap()), Some("move_next_word_start"));
        assert_eq!(cmd_name(resolve(n, "C-home").unwrap()), Some("goto_file_start"));
        assert_eq!(cmd_name(resolve(n, "C-end").unwrap()), Some("goto_last_line"));
        assert_eq!(cmd_name(resolve(n, "ins").unwrap()), Some("insert_mode"));
        assert_eq!(cmd_name(resolve(n, "C-]").unwrap()), Some("goto_definition"));
        // gt/gT navigate buffers (vim tabpages)
        assert_eq!(cmd_name(resolve(n, "g t").unwrap()), Some("goto_next_buffer"));
        assert_eq!(cmd_name(resolve(n, "g T").unwrap()), Some("goto_previous_buffer"));
        // = reindent operator is a sequence for motions, leaf for ==
        assert_eq!(cmd_name(resolve(n, "= =").unwrap()), Some("indent"));
        assert!(matches!(resolve(n, "= j").unwrap(), KeyTrie::Sequence(_)));

        // visual block + extras
        let s = &km[&Mode::Select];
        assert_eq!(cmd_name(resolve(s, "C-v").unwrap()), Some("copy_selection_on_next_line"));
        assert_eq!(cmd_name(resolve(s, "K").unwrap()), Some("hover"));
        assert_eq!(cmd_name(resolve(s, "g C-a").unwrap()), Some("increment"));

        // insert-mode indent + completion
        let i = &km[&Mode::Insert];
        assert_eq!(cmd_name(resolve(i, "C-t").unwrap()), Some("indent"));
        assert_eq!(cmd_name(resolve(i, "C-d").unwrap()), Some("unindent"));
        assert_eq!(cmd_name(resolve(i, "C-n").unwrap()), Some("completion"));
    }

    #[test]
    fn emacs_readline_keys_bound() {
        let km = default();
        let n = &km[&Mode::Normal];
        let i = &km[&Mode::Insert];
        // Meta keys in normal mode (M-x, M-f/b, M-w, M-</>)
        assert_eq!(cmd_name(resolve(n, "A-x").unwrap()), Some("command_palette"));
        assert_eq!(cmd_name(resolve(n, "A-f").unwrap()), Some("move_next_word_start"));
        assert_eq!(cmd_name(resolve(n, "A-<").unwrap()), Some("goto_file_start"));
        assert_eq!(cmd_name(resolve(n, "C-g").unwrap()), Some("collapse_selection"));
        assert_eq!(cmd_name(resolve(n, "C-l").unwrap()), Some("align_view_center"));
        // readline motion in insert mode (C-f/C-b, M-f/M-b)
        assert_eq!(cmd_name(resolve(i, "C-f").unwrap()), Some("move_char_right"));
        assert_eq!(cmd_name(resolve(i, "C-b").unwrap()), Some("move_char_left"));
        assert_eq!(cmd_name(resolve(i, "A-f").unwrap()), Some("move_next_word_start"));
        assert_eq!(cmd_name(resolve(i, "C-a").unwrap()), Some("insert_at_line_start"));
    }

    #[test]
    fn vim_operator_pending_is_a_sequence() {
        let km = default();
        let n = &km[&Mode::Normal];
        // `dd`, `dw`, `cw`, `yy` resolve to multi-command sequences.
        for chord in ["d d", "d w", "c w", "y y", "d $"] {
            let leaf = resolve(n, chord)
                .unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert!(
                matches!(leaf, KeyTrie::Sequence(_)),
                "{chord} should be an operator sequence, got {leaf:?}"
            );
        }
    }
}
