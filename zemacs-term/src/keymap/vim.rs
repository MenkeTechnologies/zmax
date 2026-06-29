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
use indexmap::IndexMap;
use zemacs_core::hashmap;
use zemacs_view::input::KeyEvent;

/// spacemacs SPC bindings that resolve to typable (`:`) commands. The keymap
/// macro can only express static commands, so these are inserted after macro
/// construction. Format: (chord, submap label, command). The chord uses the
/// same space-joined notation the port report parses, so coverage stays honest.
#[rustfmt::skip]
const SPACEMACS_TYPABLE: &[(&str, &str, &str)] = &[
    ("space f s", "Files",   ":write"),            // SPC f s : save
    ("space f S", "Files",   ":write-all"),        // SPC f S : save all
    ("space a c", "Applications", ":calc"),        // SPC a c : calc-dispatch
    ("space f R", "Files",   ":move"),             // SPC f R : rename file
    ("space f D", "Files",   ":delete-file"),      // SPC f D : delete file + buffer
    ("space b d", "Buffers", ":buffer-close"),     // SPC b d : kill buffer
    ("space b D", "Buffers", ":buffer-close-others"), // SPC b C-d / others
    ("space b R", "Buffers", ":reload"),           // SPC b R : revert
    ("space b N n", "Buffers", ":new"),            // SPC b N n : new buffer, current window
    ("space b N i", "Buffers", ":new"),            // SPC b N i : indirect buffer (approx new)
    ("space b N C-i", "Buffers", ":new"),          // SPC b N C-i : indirect buffer (approx new)
    ("space q q", "Quit",    ":quit-all"),         // SPC q q : quit
    ("space q Q", "Quit",    ":quit-all!"),        // SPC q Q : force quit
    ("space q s", "Quit",    ":write-quit-all"),   // SPC q s : save and quit
    ("space f T", "Files",   ":theme"),            // SPC T n / theme
    ("space x l s", "Text",  ":sort"),             // SPC x l s : sort lines
    // SPC t toggles -> existing :toggle substrate (config options).
    ("space t n r", "Toggles", ":toggle line-number absolute relative"), // relative nums
    ("space t n a", "Toggles", ":toggle line-number relative absolute"), // absolute nums
    ("space t n n", "Toggles", ":toggle line-number absolute relative"), // SPC t n n : toggle line numbers
    ("space t C-w", "Toggles", ":toggle whitespace.render all none"),    // SPC t C-w : global whitespace
    ("space t i",   "Toggles", ":toggle indent-guides.render"),          // indent guides
    ("space t t",   "Toggles", ":theme-toggle"),                         // SPC t t : light/dark toggle
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
    // --- Git hunks (gitsigns / vim-gitgutter / JetBrains gutter) ---
    ("] c",         "Git",     ":hunk-next"),                            // next changed hunk
    ("[ c",         "Git",     ":hunk-prev"),                            // previous changed hunk
    ("space g r",   "Git",     ":hunk-reset"),                           // SPC g r : reset/undo hunk
    ("space g n",   "Git",     ":hunk-next"),                            // SPC g n : next hunk
    ("space g p",   "Git",     ":hunk-prev"),                            // SPC g p : prev hunk
    ("space g c o", "Git",     ":conflict-ours"),                        // SPC g c o : keep ours
    ("space g c t", "Git",     ":conflict-theirs"),                      // SPC g c t : keep theirs
    ("space g c b", "Git",     ":conflict-both"),                        // SPC g c b : keep both
    ("] x",         "Git",     ":conflict-next"),                        // next conflict
    ("[ x",         "Git",     ":conflict-prev"),                        // previous conflict
    ("space g f l", "Git",     "git_file_log_picker"),                   // SPC g f l : commits log for current file (:BCommits)
    ("space g L",   "Git",     "git_repo_log_picker"),                   // SPC g L : repo commit log (:Commits)
    ("space g S",   "Git",     ":git-stage"),                           // SPC g S : stage current file
    ("space g U",   "Git",     ":git-unstage"),                         // SPC g U : unstage current file
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
    ("space t h i", "Toggles", ":toggle indent-guides.render"),        // SPC t h i : highlight indentation
    ("space t C-i", "Toggles", ":toggle indent-guides.render"),        // SPC t C-i : global indent guide
    ("space t h c", "Toggles", ":toggle cursorcolumn"),                // SPC t h c : highlight current column
    ("space t C-S-l", "Toggles", ":toggle soft-wrap.enable"),          // SPC t C-S-l : visual line navigation
    ("space t K", "Toggles", ":toggle auto-info"),                     // SPC t K : which-key (auto-info) mode
    ("space t p", "Toggles", ":toggle auto-pairs"),                    // SPC t p : smartparens (auto-pairs)
    ("space t C-p", "Toggles", ":toggle auto-pairs"),                  // SPC t C-p : global smartparens
    // SPC T c (theme_picker) is a static command, bound in the macro keymap below.
    ("space T s", "Themes", ":theme"),                                 // SPC T s : select theme
    ("space T n", "Themes", ":theme-next"),                            // SPC T n : next theme
    ("space T p", "Themes", ":theme-prev"),                            // SPC T p : previous theme
    ("space h T v", "Help", ":tutor"),                                 // SPC h T v : evil tutor
    ("space h d c", "Help",    ":character-info"),                     // SPC h d c : describe char under point
    ("space p e",   "Project", ":config-open"),                       // SPC p e : edit dir-locals/config
    ("space f e i", "Files",   ":config-open"),                       // SPC f e i : open init/config
    ("space f e e", "Files",   ":config-open"),                       // SPC f e e : open env/config
    ("space f e R", "Files",   ":config-reload"),                     // SPC f e R : resync the dotfile
    ("space f e C-e", "Files",  ":config-reload"),                    // SPC f e C-e : reinit env
    ("space f C d", "Files",   ":line-ending crlf"),                  // SPC f C d : unix -> dos line endings
    ("space f C u", "Files",   ":line-ending lf"),                    // SPC f C u : dos -> unix line endings
    ("space e y",   "Errors",  ":yank-diagnostic"),                   // SPC e y : copy error at point
    ("space x x",   "Text",    ":run-shell-command"),                 // SPC x x : quickrun (run a command)
    ("space u space b d", "Universal", ":buffer-close"),              // SPC u SPC b d : kill buffer + window
    ("space u space b m", "Universal", ":buffer-close-others"),       // SPC u SPC b m : kill other buffers
    ("space b . d", "Buffers", ":buffer-close"),                     // SPC b . d : kill current buffer
    ("space b . x", "Buffers", ":buffer-close"),                     // SPC b . x : kill buffer and window
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
    s.split(' ')
        .map(|k| k.parse().expect("valid key"))
        .collect()
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
    ("g -", "Undo", ":earlier"),      // g-: go to older text state (undo-tree)
    ("g +", "Undo", ":later"),        // g+: go to newer text state (undo-tree)
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
        // vim caret semantics: land *on* the target char, not Helix's
        // off-by-one block-cursor position. See `move_word_vim_impl`.
        "w" => vim_move_next_word_start,
        "b" => vim_move_prev_word_start,
        "e" => vim_move_next_word_end,
        "W" => vim_move_next_long_word_start,
        "B" => vim_move_prev_long_word_start,
        "E" => vim_move_next_long_word_end,

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
        "del" => delete_selection,          // <Del> = x (delete char under cursor)
        "X" => delete_char_backward,        // delete char before cursor
        "D" => [extend_to_line_end, delete_selection],
        "C" => [extend_to_line_end, change_selection],
        "Y" => [extend_to_line_bounds, yank, collapse_selection],
        "s" => sneak_or_substitute_char,    // vim-sneak (editor.vim-sneak=true) else substitute char
        "S" => sneak_or_substitute_line,    // vim-sneak backward, else substitute line
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
            "g" => { "Delete to top"
                "g" => [collapse_selection, extend_to_file_start, delete_selection], // dgg
            },
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

        // --- indent operators (vim >>, <<, >{motion}, <{motion}) -----------
        ">" => { "Indent"
            ">" => indent,                       // >> indent current line
            "j" => [extend_to_line_bounds, extend_line_below, indent, flip_selections, collapse_selection, goto_first_nonwhitespace],
            "k" => [extend_to_line_bounds, extend_line_up, indent, flip_selections, collapse_selection, goto_first_nonwhitespace],
            "G" => [extend_to_last_line, indent, collapse_selection],
            "g" => { "Indent to top"
                "g" => [extend_to_file_start, indent, collapse_selection],
            },
        },
        "<" => { "Unindent"
            "<" => unindent,                       // << unindent current line
            "j" => [extend_to_line_bounds, extend_line_below, unindent, flip_selections, collapse_selection, goto_first_nonwhitespace],
            "k" => [extend_to_line_bounds, extend_line_up, unindent, flip_selections, collapse_selection, goto_first_nonwhitespace],
            "G" => [extend_to_last_line, unindent, collapse_selection],
            "g" => { "Unindent to top"
                "g" => [extend_to_file_start, unindent, collapse_selection],
            },
        },

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
            // g?{motion} / g?? / g?g?: ROT13-encode text (vim operator).
            "?" => { "Rot13"
                "?" => [extend_to_line_bounds, rot13, collapse_selection],          // g?? current line
                "j" => [extend_to_line_bounds, extend_line_below, rot13, flip_selections, collapse_selection, goto_first_nonwhitespace],
                "k" => [extend_to_line_bounds, extend_line_up, rot13, flip_selections, collapse_selection, goto_first_nonwhitespace],
                "w" => [collapse_selection, extend_next_word_start, rot13, collapse_selection],
                "e" => [collapse_selection, extend_next_word_end, rot13, collapse_selection],
                "b" => [collapse_selection, extend_prev_word_start, rot13, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, rot13, collapse_selection],
                "G" => [extend_to_last_line, rot13, collapse_selection],
                "g" => { "Rot13 line"
                    "?" => [extend_to_line_bounds, rot13, collapse_selection],      // g?g? current line
                },
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
            "E" => vim_move_prev_long_word_end, // gE back to end of previous WORD
            "e" => vim_move_prev_word_end,      // ge back to end of previous word
            "j" => move_line_down,
            "k" => move_line_up,
            "h" => select_mode,                // gh: start Select mode (vim); g0/g^ cover line start
            "l" => goto_line_end,
            "0" => goto_line_start,            // g0 leftmost (screen line)
            "$" => goto_line_end,              // g$ rightmost (screen line)
            "^" => goto_first_nonwhitespace,   // g^ first non-blank (screen line)
            "_" => goto_line_last_nonblank,    // g_ last non-blank char of line
            "M" => goto_line_middle,           // gM middle of the text line
            "o" => goto_byte,                  // go to byte {count} in buffer
            "I" => insert_at_line_start,       // gI insert at column 1
            "d" => goto_definition,
            "D" => goto_declaration,
            "y" => goto_type_definition,
            "r" => goto_reference,
            "i" => insert_at_last_insert,      // gi insert at last insert position
            "R" => replace_mode,               // gR virtual replace ≈ replace mode
            "v" => reselect_visual,            // gv reselect last visual area
            "f" => goto_file,
            "x" => goto_file,                 // gx: open file/URL under cursor (goto_file opens URLs externally)
            // ga (print char ascii/unicode value) is bound via VIM_TYPABLE to
            // :character-info — vim's ga, not zemacs's goto-last-accessed-file.
            "m" => goto_line_middle,          // gm: go to middle of the screen line (vim, not last-modified)
            "C-g" => document_stats,          // g CTRL-G: line/word/char counts (+ selection)
            "t" => goto_next_buffer,           // gt: next tabpage -> next buffer
            "T" => goto_previous_buffer,       // gT: previous tabpage -> previous buffer
            "p" => paste_after,                // gp: paste after (vim leaves cursor after)
            "P" => paste_before,               // gP: paste before
            "n" => search_next,                // gn: select the next search match
            "N" => search_prev,                // gN: select the previous search match
            "." => goto_last_modification,
            "'" => goto_mark_line,             // g'{mark}: like ' but keep jumplist
            "`" => goto_mark,                  // g`{mark}: like ` but keep jumplist
            "down" => move_line_down,          // g<Down>: like gj (display line down)
            "up"   => move_line_up,            // g<Up>: like gk (display line up)
            "home" => goto_line_start,         // g<Home>: like g0
            "end"  => goto_line_end,           // g<End>: like g$
            "#" => [search_selection, search_prev], // g#: search word backward (no \<\> bounds)
            "*" => [search_selection, search_next], // g*: search word forward (no \<\> bounds)
            "H" => [extend_to_line_bounds, select_mode], // gH: start linewise Select mode
            "C-h" => select_mode,              // g CTRL-H: start blockwise Select mode (emulated)
            "]" => goto_definition,            // g]: :tselect tag under cursor
            "C-]" => goto_definition,          // g CTRL-]: :tjump tag under cursor
            "tab" => goto_last_accessed_file,  // g<Tab>: go to last accessed tabpage
            "," => goto_last_modification,     // g,: newer change-list position (approx last change)
            "Q" => command_mode,               // gQ: Ex mode -> open command line
        },

        // --- z submap (view + folds) ---------------------------------------
        "z" => { "View"
            "z" => align_view_center,
            "t" => align_view_top,
            "b" => align_view_bottom,
            "." => [align_view_center, goto_first_nonwhitespace], // z. center + first non-blank
            "-" => [align_view_bottom, goto_first_nonwhitespace], // z- bottom + first non-blank
            "ret" => [align_view_top, goto_first_nonwhitespace],  // z<CR> top + first non-blank
            "+" => page_down,         // z+ cursor on line below window (approx page down)
            "^" => page_up,           // z^ cursor on line above window (approx page up)

            // horizontal scroll (vim 'nowrap' z h / z l / z H / z L)
            "h" => scroll_column_left,         // zh scroll left one column
            "l" => scroll_column_right,        // zl scroll right one column
            "left"  => scroll_column_left,     // z<Left> = zh
            "right" => scroll_column_right,    // z<Right> = zl
            "H" => scroll_half_column_left,    // zH scroll left half a screen
            "L" => scroll_half_column_right,   // zL scroll right half a screen
            "e" => scroll_half_column_left,    // ze scroll so cursor is near the right edge (approx)
            "s" => scroll_half_column_right,   // zs scroll so cursor is near the left edge (approx)
            "x" => fold_open,                  // zx re-apply foldlevel and open enough to see cursor (approx)

            // spell checking (vim z= / zg / zw / zG / zW / zug …)
            "=" => spell_suggest,              // z= spelling suggestions for word under cursor
            "g" => spell_add_good,             // zg mark word as correctly spelled
            "w" => spell_add_bad,              // zw mark word as misspelled
            "G" => spell_add_good,             // zG temporarily good (approx: persisted)
            "W" => spell_add_bad,              // zW temporarily bad (approx)
            "u" => { "Undo spell"
                "g" => spell_undo,             // zug undo zg
                "w" => spell_undo,             // zuw undo zw
                "G" => spell_undo,             // zuG undo zG
                "W" => spell_undo,             // zuW undo zW
            },

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
            "D" => fold_delete,       // zD delete folds recursively (approx: at cursor)
            "E" => fold_delete_all,   // zE eliminate all folds
            "A" => fold_toggle,       // zA toggle fold recursively (approx: at cursor)
            "i" => fold_toggle,       // zi toggle foldenable (approx: fold at cursor)
            "m" => fold_close_all,    // zm fold more (decrease foldlevel)
            "r" => fold_open_all,     // zr fold reduce (increase foldlevel)
            "n" => fold_open_all,     // zn foldenable off (show all text)
            "N" => fold_close_all,    // zN set foldenable (close to foldlevel, approx)
            "X" => fold_open_all,     // zX re-apply foldlevel (approx open all)
            "F" => [extend_to_line_bounds, fold_create], // zF create a fold for N lines
            "p" => paste_after,       // zp block paste without trailing spaces (approx)
            "P" => paste_before,      // zP block paste without trailing spaces (approx)
            "y" => [yank, collapse_selection], // zy yank without trailing spaces (approx)
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
            "x" => goto_prev_conflict,    // [x previous merge-conflict marker
            "n" => goto_prev_conflict,    // [n previous conflict (vim-unimpaired style)
            "f" => goto_file,             // [f same as gf: open file under cursor
            "m" => goto_prev_function,    // [m back to start of member/function
            "b" => goto_previous_buffer,  // [b previous buffer (unimpaired-style)
            "/" => goto_prev_comment,     // [/ previous comment
            "p" => paste_before,          // [p paste before (linewise, adjust indent)
            "P" => paste_before,          // [P same as [p
            "*" => goto_prev_comment,     // [* same as [/ : previous comment
            "]" => goto_prev_function,    // [] N sections backward (member/function)
            "z" => fold_prev,             // [z move to start of open fold
            // word-under-cursor / #define navigation (vim [i [I [d [D [CTRL-I [CTRL-D).
            // Approximated with a current-buffer word search (vim also scans included files).
            "i"   => [search_selection_detect_word_boundaries, search_prev], // [i: prev line containing the word
            "I"   => [search_selection_detect_word_boundaries, search_prev], // [I: list occurrences (approx: jump prev)
            "D"   => [search_selection_detect_word_boundaries, search_prev], // [D: list #defines (approx)
            "C-i" => [search_selection_detect_word_boundaries, search_prev], // [CTRL-I: word in included files (approx)
            "C-d" => goto_declaration,        // [CTRL-D: jump to first #define (approx: declaration)
            "s" => goto_prev_spell_error,     // [s: previous misspelled word
            "(" => goto_prev_unmatched_paren, // [( previous unmatched (
            "{" => goto_prev_unmatched_brace, // [{ previous unmatched {
            "`" => goto_prev_mark,            // [` previous lowercase mark
            "'" => goto_prev_mark_line,       // ['  previous lowercase mark (line)
        },
        "]" => { "Next"
            "]" => goto_next_paragraph,
            "d" => goto_next_diag,
            "g" => goto_next_change,
            "c" => goto_next_change,      // ]c forward to start of next change (diff hunk)
            "x" => goto_next_conflict,    // ]x next merge-conflict marker
            "n" => goto_next_conflict,    // ]n next conflict (vim-unimpaired style)
            "f" => goto_file,             // ]f same as gf: open file under cursor
            "m" => goto_next_function,    // ]m forward to next member/function
            "b" => goto_next_buffer,      // ]b next buffer (unimpaired-style)
            "/" => goto_next_comment,     // ]/ next comment
            "p" => paste_after,           // ]p paste after (linewise, adjust indent)
            "P" => paste_before,          // ]P same as [p
            "*" => goto_next_comment,     // ]* same as ]/ : next comment
            "[" => goto_next_function,    // ][ N sections forward (member/function)
            "z" => fold_next,             // ]z move to end of open fold
            // word-under-cursor / #define navigation (vim ]i ]I ]d ]D ]CTRL-I ]CTRL-D).
            "i"   => [search_selection_detect_word_boundaries, search_next], // ]i: next line containing the word
            "I"   => [search_selection_detect_word_boundaries, search_next], // ]I: list occurrences (approx: jump next)
            "D"   => [search_selection_detect_word_boundaries, search_next], // ]D: list #defines (approx)
            "C-i" => [search_selection_detect_word_boundaries, search_next], // ]CTRL-I: word in included files (approx)
            "C-d" => goto_definition,         // ]CTRL-D: jump to first #define (approx: definition)
            "s" => goto_next_spell_error,     // ]s: next misspelled word
            ")" => goto_next_unmatched_paren, // ]) next unmatched )
            "}" => goto_next_unmatched_brace, // ]} next unmatched }
            "`" => goto_next_mark,            // ]` next lowercase mark
            "'" => goto_next_mark_line,       // ]'  next lowercase mark (line)
        },

        // --- window commands (C-w) -----------------------------------------
        "C-w" => { "Window"
            "s" | "C-s" => hsplit,
            "v" | "C-v" => vsplit,
            "w" | "C-w" => rotate_view,
            "r" | "C-r" => rotate_view,       // C-w r / C-w C-r: rotate windows downwards
            "tab" => rotate_view,             // SPC w TAB: switch to alternate window
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
            ">" => resize_view_wider,         // C-w >: increase window width N columns
            "<" => resize_view_narrower,      // C-w <: decrease window width N columns
            "x" | "C-x" => transpose_view,    // C-w x: exchange current window with next
            "f" | "C-f" => goto_file_hsplit,  // C-w f / C-w C-f: split + edit file under cursor
            "F" => goto_file_hsplit,          // C-w F: split + edit file (with line number)
            "]" | "C-]" => goto_definition,   // C-w ] / C-w C-]: jump to tag/definition (no split)
            "^" | "C-^" => goto_last_accessed_file, // C-w ^ / C-w C-^: edit alternate file
            "i" | "C-i" => goto_declaration,  // C-w i / C-w C-i: split + jump to declaration (no split)
            "p" | "C-p" => rotate_view,       // C-w p: go to previous (last accessed) window
            "t" | "C-t" => jump_view_up,      // C-w t: go to top window
            "b" | "C-b" => jump_view_down,    // C-w b: go to bottom window
            "W" => rotate_view_reverse,       // C-w W: go to previous window (wrap)
            "}" => hover,                     // C-w }: show tag under cursor in preview (hover)
            // CTRL-W g ...: tab/file/tag variants (vim's window-goto sub-prefix)
            "g" => { "Window goto"
                "t" => goto_next_buffer,      // C-w g t: next tabpage -> next buffer
                "T" => goto_previous_buffer,  // C-w g T: prev tabpage -> previous buffer
                "f" => goto_file,             // C-w g f: edit file under cursor (new tab approx)
                "F" => goto_file,             // C-w g F: edit file under cursor (new tab approx)
                "]" | "C-]" => goto_definition, // C-w g ] / g C-]: tag jump (:tselect/:tjump)
                "}" => hover,                 // C-w g }: preview tag under cursor
                "tab" => goto_last_accessed_file, // C-w g <Tab>: last accessed tab -> alt file
            },
            "n" | "C-n" => hsplit_new,        // C-w n: open new window
            "/" => vsplit,                    // spacemacs SPC w / : split vertically
            // vim window height resize (horizontal split stays on s / C-s)
            "+" => resize_view_taller,        // C-w +: increase window height N lines
            "-" => resize_view_shorter,       // C-w -: decrease window height N lines
            "=" => resize_view_equalize,      // C-w =: make all windows equal size
            "c" => wclose,                    // spacemacs SPC w c : close window
            "m" => wonly,                     // spacemacs SPC w m : maximize (only)
            "S" => hsplit,                    // spacemacs SPC w S / vim C-w S : split & focus
            "V" => vsplit,                    // spacemacs SPC w V : vsplit & focus
            "|" => wonly,                     // spacemacs SPC w | : maximize window (only)
            "1" => wonly,                     // SPC w 1 : single-window layout
            "2" => vsplit,                    // SPC w 2 : two-window layout (split)
            "3" => vsplit,                    // SPC w 3 : three-window layout (split)
            "4" => vsplit,                    // SPC w 4 : four-window layout (split)
            "_" => wonly,                     // SPC w _ : maximize window horizontally
            "D" => wclose,                    // SPC w D : delete another window
            "M" => transpose_view,            // SPC w M : swap windows
            "." => { "Window transient"
                "h" => jump_view_left,        // SPC w . h : go to window left
                "j" => jump_view_down,        // SPC w . j : go to window below
                "k" => jump_view_up,          // SPC w . k : go to window above
                "l" => jump_view_right,       // SPC w . l : go to window right
                "/" => vsplit,                // SPC w . / : vertical split
                "-" => hsplit,                // SPC w . - : horizontal split
                "d" => wclose,                // SPC w . d : delete window
                "D" => wonly,                 // SPC w . D : delete other windows
                "o" => rotate_view,           // SPC w . o : cycle windows
                "z" => align_view_center,     // SPC w . z : recenter
            },
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
        "C-^"     => goto_last_accessed_file, // CTRL-^ = edit alternate file
        "S-ret"   => page_down,          // <S-CR> = CTRL-F (page down)
        "S-+"     => page_down,          // <S-+> = CTRL-F (page down)
        "S-minus" => page_up,            // <S--> = CTRL-B (page up)
        "U"       => undo,               // U: undo latest changes on one line (approx: undo)
        "F1"      => command_palette,     // <F1>: help -> command palette (commands/help list)
        "C-t"     => jump_backward,      // CTRL-T = pop tag stack (≈ jump back)
        "C-tab"   => goto_last_accessed_file, // CTRL-<Tab> = go to last accessed tab

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
        "C-g"     => file_info,           // vim CTRL-G: show file name + position (Esc still collapses)
        "C-l"     => align_view_center,   // C-l recenter
        "C-s"     => search,              // C-s isearch-forward
        "C-/"     => undo,                // C-/ undo
        "C-_"     => undo,                // C-_ undo
        "A-;"     => toggle_comments,     // M-; comment-dwim
        "A-m"     => goto_first_nonwhitespace, // M-m back-to-indentation
        "A-q"     => format_selections,   // M-q fill/reformat (approx)
        "A-^"     => join_selections,     // M-^ join to previous line (approx)

        // vim CTRL-C / CTRL-\ CTRL-N / CTRL-\ CTRL-G: ensure/return to Normal mode.
        "C-c"     => normal_mode,
        "C-\\" => { "Normal"
            "C-n" => normal_mode,            // CTRL-\ CTRL-N: go to Normal mode
            "C-g" => normal_mode,            // CTRL-\ CTRL-G: go to Normal mode
        },

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
        "," => repeat_find_char_reverse,   // vim , : repeat last f/t/F/T reversed
        // Run / Debug function keys (IDE-style)
        "F5" => run_active_config,             // F5      : run the active configuration
        "S-F5" => dap_launch,                  // Shift-F5: start debugging
        "F9" => dap_toggle_breakpoint,         // F9      : toggle breakpoint
        "F10" => dap_next,                     // F10     : step over
        "F11" => dap_step_in,                  // F11     : step in
        "S-F11" => dap_step_out,               // Shift-F11: step out

        "space" => { "Leader (spacemacs SPC)"
            "space" => command_palette,            // SPC SPC : M-x
            "tab"   => goto_last_accessed_file,    // SPC TAB : alternate buffer
            ":"     => command_mode,               // SPC :   : Ex command
            "/"     => global_search,              // SPC /   : search project
            "?"     => command_palette,            // SPC ?   : commands
            "'"     => last_picker,                // SPC '   : resume picker
            ";"     => toggle_comments,            // SPC ;   : comment operator

            "a" => { "Applications"
                "r" => repl,                       // SPC a r : embedded-language REPL (elisp/viml/stryke/awk/zsh)
                "d" => file_explorer,              // SPC a d : dired (file manager)
                "f" => file_explorer,              // SPC a f : file tree
                "t" => { "Ranger"
                    "r" => { "deer/dirvish"
                        "r" => file_explorer,      // SPC a t r r : ranger full layout
                        "d" => file_explorer,      // SPC a t r d : deer single column
                    },
                },
            },
            "K" => { "Macros"
                "c" => { "Counter"
                    "a" => kmacro_add_counter,     // SPC K c a : increment macro counter
                    "c" => kmacro_insert_counter,  // SPC K c c : insert counter value, then increment
                },
            },

            "T" => { "Themes"
                "c" => theme_picker,               // SPC T c : fzf theme picker w/ live preview (:Colors)
            },

            "f" => { "Files"
                "f" => file_picker,                            // SPC f f
                "l" => file_picker,                            // SPC f l : open file literally
                "A" => file_picker,                            // SPC f A : open file, replace buffer
                "o" => goto_file,                              // SPC f o : open with external program
                "F" => goto_file,                              // SPC f F : open file under point
                "L" => file_picker,                            // SPC f L : locate a file
                "b" => marks_picker,                           // SPC f b : go to file bookmarks (marks)
                "r" => frecent_file_picker,                    // SPC f r : recent files (z frecency)
                "u" => reopen_last_closed,                     // SPC f u : reopen last closed file
                "t" => file_explorer,                          // SPC f t
                "d" => file_explorer_in_current_buffer_directory, // SPC f d
                "j" => file_explorer_in_current_buffer_directory, // SPC f j : dired
                "y" => { "Yank path"
                    "y" => yank_file_path,            // SPC f y y : copy file path
                    "n" => yank_file_name,            // SPC f y n : copy file name
                    "l" => yank_file_path_with_line,  // SPC f y l : copy path:line
                    "c" => yank_file_path_with_line_col, // SPC f y c : copy path:line:col
                    "d" => yank_file_dir,             // SPC f y d : copy directory
                    "N" => yank_file_name,            // SPC f y N : copy file name (no ext, approx)
                    "C" => yank_file_path,            // SPC f y C : copy path relative to project
                    "D" => yank_file_dir,             // SPC f y D : copy directory relative to project
                    "L" => yank_file_path_with_line,  // SPC f y L : copy path:line relative
                    "Y" => yank_file_path,            // SPC f y Y : copy full file path
                },
            },
            "i" => { "Insert"
                "u" => unicode_picker,             // SPC i u : search unicode chars and insert (helm-unicode)
                "U" => { "UUID"
                    "1" => insert_uuid_v1,         // SPC i U 1 : time-based UUIDv1
                    "4" => insert_uuid_v4,         // SPC i U 4 : random UUIDv4
                    "U" => insert_uuid_v4,         // SPC i U U : random UUIDv4
                },
                "l" => { "Lorem ipsum"
                    "s" => insert_lorem_sentence,  // SPC i l s
                    "p" => insert_lorem_paragraph, // SPC i l p
                    "l" => insert_lorem_list,      // SPC i l l
                },
                "p" => { "Password"
                    "1" => insert_password_simple,    // SPC i p 1
                    "2" => insert_password_strong,    // SPC i p 2
                    "3" => insert_password_paranoid,  // SPC i p 3
                    "n" => insert_password_numerical, // SPC i p n
                    "p" => insert_password_phonetic,  // SPC i p p
                },
            },
            "b" => { "Buffers"
                "b" => buffer_picker,              // SPC b b
                "n" => goto_next_buffer,           // SPC b n
                "p" => goto_previous_buffer,       // SPC b p
                "m" => changed_file_picker,        // SPC b m
                "W" => buffer_picker,              // SPC b W : go to buffer (workspace/window)
                "N" => { "New buffer"
                    "h" => hsplit_new,             // SPC b N h : new buffer in window left
                    "j" => hsplit_new,             // SPC b N j : new buffer in window below
                    "k" => hsplit_new,             // SPC b N k : new buffer in window above
                    "l" => hsplit_new,             // SPC b N l : new buffer in window right
                    // SPC b N n / i / C-i -> :new via typable table
                },
                "." => { "Buffer transient"
                    "n" => goto_next_buffer,       // SPC b . n : next buffer
                    "N" => goto_previous_buffer,   // SPC b . N : previous buffer
                    "p" => goto_previous_buffer,   // SPC b . p : previous buffer
                    "b" => buffer_picker,          // SPC b . b : list buffers
                    "z" => align_view_center,      // SPC b . z : recenter buffer in window
                    "o" => rotate_view,            // SPC b . o : focus other window
                    // SPC b . d / x -> :buffer-close via typable table
                },
                "P" => [select_all, replace_with_yanked], // SPC b P : paste-replace buffer
                "Y" => [select_all, yank_to_clipboard, collapse_selection], // SPC b Y
                "h" => dashboard,                  // SPC b h : home buffer (dashboard)
                "H" => help,                       // SPC b H : *Help* buffer (inline Help browser)
                "u" => reopen_last_closed,         // SPC b u : reopen the most recently killed buffer
                "w" => toggle_readonly,            // SPC b w : toggle read-only (writable state)
            },
            // Kept identical to the `C-w` window submap (see aliased-modes test).
            "w" => { "Window"
                "s" | "C-s" => hsplit,
                "v" | "C-v" => vsplit,
                "w" | "C-w" => rotate_view,
                "r" | "C-r" => rotate_view,
                "tab" => rotate_view,
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
                ">" => resize_view_wider,         // C-w >: increase window width N columns
                "<" => resize_view_narrower,      // C-w <: decrease window width N columns
                "x" | "C-x" => transpose_view,
                "f" | "C-f" => goto_file_hsplit,
                "F" => goto_file_hsplit,
                "]" | "C-]" => goto_definition,
                "^" | "C-^" => goto_last_accessed_file,
                "i" | "C-i" => goto_declaration,
                "p" | "C-p" => rotate_view,
                "t" | "C-t" => jump_view_up,
                "b" | "C-b" => jump_view_down,
                "W" => rotate_view_reverse,
                "}" => hover,
                "g" => { "Window goto"
                    "t" => goto_next_buffer,
                    "T" => goto_previous_buffer,
                    "f" => goto_file,
                    "F" => goto_file,
                    "]" | "C-]" => goto_definition,
                    "}" => hover,
                    "tab" => goto_last_accessed_file,
                },
                "n" | "C-n" => hsplit_new,
                "/" => vsplit,
                "+" => resize_view_taller,
                "-" => resize_view_shorter,
                "[" => resize_view_narrower,      // SPC w [ : shrink window horizontally
                "{" => resize_view_shorter,       // SPC w { : shrink window vertically
                "=" => resize_view_equalize,
                "c" => wclose,
                "m" => wonly,
                "S" => hsplit,
                "V" => vsplit,
                "|" => wonly,
                "1" => wonly,
                "2" => vsplit,
                "3" => vsplit,
                "4" => vsplit,
                "_" => wonly,
                "D" => wclose,
                "M" => transpose_view,
                "." => { "Window transient"
                    "h" => jump_view_left,
                    "j" => jump_view_down,
                    "k" => jump_view_up,
                    "l" => jump_view_right,
                    "H" => swap_view_left,         // SPC w . H : move window left
                    "J" => swap_view_down,         // SPC w . J : move window down
                    "K" => swap_view_up,           // SPC w . K : move window up
                    "L" => swap_view_right,        // SPC w . L : move window right
                    "/" => vsplit,
                    "-" => hsplit,
                    "s" => hsplit,                 // SPC w . s : horizontal split
                    "S" => hsplit,                 // SPC w . S : horizontal split + focus
                    "v" => vsplit,                 // SPC w . v : vertical split
                    "V" => vsplit,                 // SPC w . V : vertical split + focus
                    "r" => rotate_view,            // SPC w . r : rotate windows forward
                    "R" => rotate_view_reverse,    // SPC w . R : rotate windows backward
                    "w" => rotate_view,            // SPC w . w : focus other window
                    "d" => wclose,
                    "D" => wonly,
                    "o" => rotate_view,
                    "z" => align_view_center,
                    "[" => resize_view_narrower,   // SPC w . [ : shrink horizontally
                    "]" => resize_view_wider,      // SPC w . ] : enlarge horizontally
                    "{" => resize_view_shorter,    // SPC w . { : shrink vertically
                    "}" => resize_view_taller,     // SPC w . } : enlarge vertically
                    "_" => wonly,                  // SPC w . _ : maximize horizontally
                    "|" => wonly,                  // SPC w . | : maximize vertically
                },
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
                "`" => jump_backward,              // SPC s ` : back to pre-jump location
                "P" => global_search,              // SPC s P : search in a project
                "d" => global_search,              // SPC s d : search current directory
                "D" => global_search,              // SPC s D : search current directory (alt tool)
                "B" => global_search,              // SPC s B : search all open buffers
                "F" => global_search,              // SPC s F : search files in a directory
                "l" => last_picker,                // SPC s l : resume last search
                "L" => buffer_line_picker,         // SPC s L : fuzzy lines in current buffer (:BLines)
                "H" => search_next,                // SPC s H : go to last search occurrence
                // ag / grep / ack / rg families all map to project-wide search;
                // uppercase variants are the "with default input" forms.
                "a" => { "ag"
                    "a" => global_search, "b" => global_search, "d" => global_search,
                    "f" => global_search, "p" => global_search,
                    "A" => global_search, "B" => global_search, "D" => global_search,
                    "F" => global_search, "P" => global_search,
                },
                "g" => { "grep"
                    "g" => global_search, "b" => global_search, "f" => global_search,
                    "d" => global_search, "p" => global_search,
                    "G" => global_search, "B" => global_search, "F" => global_search,
                },
                "k" => { "ack"
                    "b" => global_search, "d" => global_search,
                    "f" => global_search, "p" => global_search,
                    "B" => global_search, "D" => global_search,
                    "F" => global_search, "P" => global_search,
                },
                "r" => { "rg"
                    "r" => global_search, "b" => global_search, "f" => global_search,
                    "d" => global_search, "p" => global_search,
                    "R" => global_search, "B" => global_search, "F" => global_search,
                    "D" => global_search, "P" => global_search,
                },
            },
            "R" => { "Run"
                "r" => run_active_config,          // SPC R r : run the active configuration
                "R" => rerun_last_run,             // SPC R R : re-run the last command
                "c" => run_config_manager,         // SPC R c : manage run/debug configurations
                "e" => run_config_manager,         // SPC R e : edit configurations
                "l" => clear_run_output,           // SPC R l : clear the Run console output
                "x" => clear_run_output,           // SPC R x : clear the Run console output
                "n" => run_next_error,             // SPC R n : jump to next file:line in output
                "p" => run_prev_error,             // SPC R p : jump to previous file:line in output
            },
            "d" => { "Debug"
                "d" => dap_launch,                 // SPC d d : start debugging
                "b" => dap_toggle_breakpoint,      // SPC d b : toggle breakpoint
                "c" => dap_continue,               // SPC d c : continue
                "i" => dap_step_in,                // SPC d i : step in
                "o" => dap_step_out,               // SPC d o : step out
                "n" => dap_next,                   // SPC d n : step over
                "p" => dap_pause,                  // SPC d p : pause
                "r" => dap_restart,                // SPC d r : restart session
                "v" => dap_variables,              // SPC d v : list variables
                "q" => dap_terminate,              // SPC d q : end debug session
            },
            "S" => settings_page,                  // SPC S : Preferences → Settings tab
            "," => preferences,                    // SPC , : open the unified Preferences window
            "z" => toggle_ide,                     // SPC z : toggle IDE workbench (Zen / focus mode)
            "H" => { "Harpoon"
                "a" => harpoon_add,                // SPC H a : pin current file
                "l" => harpoon_menu,               // SPC H l : marks menu
                "h" => harpoon_menu,               // SPC H h : marks menu
                "d" => harpoon_remove,             // SPC H d : unpin current file
                "j" => harpoon_jump,               // SPC H j : jump to slot [count]
                "n" => harpoon_next,               // SPC H n : next mark (wraps)
                "p" => harpoon_prev,               // SPC H p : previous mark (wraps)
                "1" => harpoon_1,                  // SPC H 1
                "2" => harpoon_2,                  // SPC H 2
                "3" => harpoon_3,                  // SPC H 3
                "4" => harpoon_4,                  // SPC H 4
            },
            "W" => { "Workbench panels"
                "t" => focus_file_tree,            // SPC W t : focus project tree
                "p" => focus_file_tree,            // SPC W p : focus project tree
                "s" => focus_structure,            // SPC W s : focus structure/outline
                "o" => focus_structure,            // SPC W o : focus outline
                "e" => focus_problems,             // SPC W e : focus problems/errors
                "r" => focus_run_console,          // SPC W r : focus Run console (j/k scroll)
                "g" => focus_git_panel,            // SPC W g : focus Git changes (j/k select, Enter opens)
                "c" => focus_ci_panel,             // SPC W c : focus CI status (GitHub Actions; Enter opens)
                "f" => toggle_drawer_mid,          // SPC W f : fold / unfold the middle drawer column
                "m" => toggle_bottom_zoom,         // SPC W m : maximize / restore the bottom panel
            },
            "p" => { "Project"
                "f" => file_picker,                // SPC p f
                "p" => file_picker,                // SPC p p
                "b" => buffer_picker,              // SPC p b : project buffer
                "h" => file_picker,                // SPC p h : find file
                "s" => global_search,              // SPC p s
                "r" => goto_last_modified_file,    // SPC p r
                "t" => file_explorer,              // SPC p t : project tree (treemacs)
                "v" => reveal_in_tree,             // SPC p v : reveal current file in the tree
                "V" => toggle_auto_reveal,         // SPC p V : toggle always-select-opened-file
                "d" => file_explorer,              // SPC p d : find directory
                "g" => symbol_picker,              // SPC p g : find tags
                "o" => global_search,              // SPC p o : multi-occur
                "a" => goto_next_test,             // SPC p a : toggle implementation/test
                "'" => terminal,                   // SPC p ' : open a shell in the project root
                "c" => run_active_config,          // SPC p c : compile project (run active config)
                "u" => run_active_config,          // SPC p u : run project (run active config)
                "i" => run_config_manager,         // SPC p i : install project (manage run/build targets)
            },
            "e" => { "Errors"
                "l" => diagnostics_picker,             // SPC e l
                "L" => workspace_diagnostics_picker,   // SPC e L
                "n" => goto_next_diag,                 // SPC e n
                "p" => goto_prev_diag,                 // SPC e p
                "f" => goto_first_diag,                // SPC e f
                "y" => copy_diagnostic,                // SPC e y : copy diagnostic message(s)
                "h" => command_palette,                // SPC e h : describe checker
                "v" => command_palette,                // SPC e v : verify checker setup
                "." => goto_last_diag,
            },
            "c" => { "Comments"
                "l" => toggle_line_comments,       // SPC c l
                "c" => toggle_comments,            // SPC c c
                "b" => toggle_block_comments,      // SPC c b
                "p" => toggle_comments,            // SPC c p : comment paragraph
                "h" => toggle_comments,            // SPC c h : hide/show comments (toggle)
                "t" => toggle_comments,            // SPC c t : comment to line
                "y" => [yank, toggle_comments],    // SPC c y : comment and yank
                "d" => wclose,                     // SPC c d : close compilation window
                "L" => toggle_line_comments,       // SPC c L : invert/toggle comment lines
                "T" => toggle_comments,            // SPC c T : invert comment to line
                "Y" => [yank, toggle_comments],    // SPC c Y : invert comment and yank
                "P" => toggle_comments,            // SPC c P : invert comment paragraphs
                "C" => run_active_config,          // SPC c C : compile (run active config)
                "r" => rerun_last_run,             // SPC c r : recompile (re-run last)
                "m" => run_config_manager,         // SPC c m : pick a build/run target (helm-make)
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
                "w" => goto_word,                  // SPC j w : avy jump to word
                "l" => goto_word,                  // SPC j l : avy jump to line
                "f" => goto_definition,            // SPC j f : jump to elisp function def
                "v" => goto_definition,            // SPC j v : jump to elisp variable def
                "I" => workspace_symbol_picker,    // SPC j I : jump to def in any buffer (imenu)
                "=" => format_selections,          // SPC j = : format region/buffer
                "+" => format_selections,          // SPC j + : format region/buffer (alt)
                "(" => goto_prev_unmatched_paren,  // SPC j ( : jump to first unbalanced paren
                "D" => file_explorer_in_current_buffer_directory, // SPC j D : current directory listing
                "U" => goto_file,                  // SPC j U : select URL and follow
            },
            "F" => { "Frames"
                "n" => hsplit_new,                 // SPC F n : create a new frame (new window)
            },
            "n" => { "Numbers/Narrow"
                "+" => increment,                  // SPC n + : increase number under point
                "=" => increment,                  // SPC n = : increase number under point
                "-" => decrement,                  // SPC n - : decrease number under point
                "_" => decrement,                  // SPC n _ : decrease number under point
                "r" => narrow_to_region,           // SPC n r : narrow buffer to selection (fold outside)
                "w" => fold_open_all,              // SPC n w : widen (show whole buffer again)
            },
            "g" => { "Goto (LSP)"
                "d" => goto_definition,
                "D" => goto_declaration,
                "r" => goto_reference,
                "i" => goto_implementation,
                "y" => goto_type_definition,
                "b" => git_blame_line,             // SPC g b : git blame current line (spacemacs magit-blame)
                "s" => focus_git_panel,            // SPC g s : git status (focus Git changes panel)
                "m" => focus_git_panel,            // SPC g m : magit dispatch (focus Git panel)
                "t" => git_file_log_picker,        // SPC g t : git time machine (browse file history)
                "M" => git_blame_line,             // SPC g M : last commit message of current line
                "l" => { "Links"
                    "l" => open_remote_url,        // SPC g l l : browse to file at current line
                    "c" => open_remote_url,        // SPC g l c : browse to file at a commit
                    "p" => open_remote_url,        // SPC g l p : browse using permalink
                    "L" => copy_remote_url,        // SPC g l L : copy link to selected lines
                    "C" => copy_remote_url,        // SPC g l C : copy link at a commit
                    "P" => copy_remote_url,        // SPC g l P : copy permalink to lines
                },
                "c" => { "Conflict"
                    // o/t/b (single resolve) come from the pre-existing :conflict-*
                    // typables (space g c o/t/b); only the bulk ops are added here.
                    "O" => conflict_take_all_ours, // SPC g c O : keep ours everywhere
                    "T" => conflict_take_all_theirs, // SPC g c T : keep theirs everywhere
                },
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
                "c" => count_selection,            // SPC x c : count chars/words/lines
                "u" => switch_to_lowercase,        // SPC x u : lowercase
                "U" => switch_to_uppercase,        // SPC x U : uppercase the selection
                "o" => goto_file,                  // SPC x o : open link in frame (avy)
                "i" => { "Symbol style"
                    "C" => symbol_upper_camel,     // SPC x i C : UpperCamelCase
                    "U" => symbol_up_case,         // SPC x i U : UP_CASE
                    "_" => symbol_under_score,     // SPC x i _ : under_score
                },
                "l" => { "Lines"
                    "r" => randomize_lines_in_region, // SPC x l r : randomize lines
                },
                "w" => { "Words"
                    "c" => count_selection,        // SPC x w c : count occurrences per word
                    "r" => randomize_words_in_region, // SPC x w r : randomize words
                },
                "j" => { "Justify"
                    "l" => format_selections,      // SPC x j l : justify left (reflow)
                    "c" => format_selections,      // SPC x j c : justify center (reflow)
                    "f" => format_selections,      // SPC x j f : justify full (reflow)
                    "r" => format_selections,      // SPC x j r : justify right (reflow)
                    "n" => format_selections,      // SPC x j n : justify none (reflow)
                },
                "tab" => indent,                   // SPC x TAB : indent region
                "a" => { "Align"
                    "a" => align_selections,       // SPC x a a : align region
                    "&" => align_selections,       // SPC x a & : align at &
                    "c" => align_selections,       // SPC x a c : align indentation
                    "l" => align_selections,       // SPC x a l : left-align
                    "r" => align_selections,       // SPC x a r : align at regexp
                    "m" => align_selections,       // SPC x a m : align at math operators
                    "L" => align_selections,       // SPC x a L : right-align
                    "(" => align_selections,       // SPC x a ( : align at (
                    ")" => align_selections,       // SPC x a ) : align at )
                    "[" => align_selections,       // SPC x a [ : align at [
                    "]" => align_selections,       // SPC x a ] : align at ]
                    "{" => align_selections,       // SPC x a { : align at {
                    "}" => align_selections,       // SPC x a } : align at }
                    "," => align_selections,       // SPC x a , : align at ,
                    "." => align_selections,       // SPC x a . : align at . (numeric)
                    ":" => align_selections,       // SPC x a : : align at :
                    ";" => align_selections,       // SPC x a ; : align at ;
                    "=" => align_selections,       // SPC x a = : align at =
                },
            },
            "r" => { "Resume / registers"
                "l" => last_picker,                // SPC r l : resume picker
                "e" => register_picker,            // SPC r e : registers
                "r" => register_picker,            // SPC r r : show registers
                "m" => marks_picker,               // SPC r m : pick a mark and jump (:Marks)
                "y" => register_picker,            // SPC r y : kill ring
                ":" => command_history_picker,     // SPC r : : command-line history (:History:)
                "/" => search_history_picker,      // SPC r / : search history (:History/)
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
                "V" => extend_line,                // SPC k V : line-wise visual select
                "C-v" => select_mode,              // SPC k C-v : block-wise selection
                "C-r" => redo,                     // SPC k C-r : redo
                "u" => undo,                       // SPC k u : undo
                "w" => wrap_sexp,                  // SPC k w : wrap with parens
                "%" => match_brackets,             // SPC k % : go to other paren of the pair
                ":" => command_mode,               // SPC k : : ex command
                "i" => insert_mode,                // SPC k i : switch to insert state
                "p" => paste_after,                // SPC k p : paste after
                "P" => paste_before,               // SPC k P : paste before
                "L" => select_next_sibling,        // SPC k L : forward to next sexp
                "H" => select_prev_sibling,        // SPC k H : backward to previous sexp
                "J" => join_selections,            // SPC k J : join sexp (join lines)
                "s" => paredit_slurp_forward,      // SPC k s : slurp forward
                "b" => paredit_barf_forward,       // SPC k b : barf forward
                "S" => paredit_slurp_backward,     // SPC k S : slurp backward
                "B" => paredit_barf_backward,      // SPC k B : barf backward
                "W" => paredit_splice,             // SPC k W : unwrap (splice) sexp
                "r" => paredit_raise,              // SPC k r : raise sexp
                "t" => paredit_transpose,          // SPC k t : transpose sexps
                "e" => paredit_splice_kill_forward,  // SPC k e : splice, killing forward
                "E" => paredit_splice_kill_backward, // SPC k E : splice, killing backward
                "d" => { "Delete"
                    "x" => [expand_selection, delete_selection], // SPC k dx : delete sexp
                    "s" => [expand_selection, delete_selection], // SPC k ds : delete symbol
                    "w" => [collapse_selection, extend_next_word_start, delete_selection], // SPC k dw
                },
                "D" => { "Delete backward"
                    "x" => [expand_selection, delete_selection], // SPC k Dx : delete sexp backward
                    "s" => [expand_selection, delete_selection], // SPC k Ds : delete symbol backward
                    "w" => [collapse_selection, extend_prev_word_start, delete_selection], // SPC k Dw
                },
            },
            "h" => { "Help"
                "h" => help,                       // SPC h h : open the inline Help browser
                "k" => help,                       // SPC h k : describe key / commands
                "?" => help,                       // SPC h ? : list bindings
                "c" => help,                       // SPC h c : describe command
                "space" => command_palette,        // SPC h SPC : discover docs/layers
                "f" => command_palette,            // SPC h f : discover the FAQ
                "l" => command_palette,            // SPC h l : search layers
                "p" => command_palette,            // SPC h p : search packages
                "n" => command_palette,            // SPC h n : browse emacs news
                "r" => command_palette,            // SPC h r : search documentation files
                "." => command_palette,            // SPC h . : search dotfile variables
                "i" => command_palette,            // SPC h i : search info pages
                "m" => command_palette,            // SPC h m : search man pages
                "d" => { "Describe"
                    "b" => command_palette,        // SPC h d b : describe bindings
                    "f" => command_palette,        // SPC h d f : describe function
                    "k" => command_palette,        // SPC h d k : describe key
                    "v" => command_palette,        // SPC h d v : describe variable
                    "m" => command_palette,        // SPC h d m : describe modes
                    "a" => hover,                  // SPC h d a : describe expression under point
                    "p" => command_palette,        // SPC h d p : describe package
                    "t" => command_palette,        // SPC h d t : describe text properties
                    "x" => command_palette,        // SPC h d x : describe ex-command
                    "l" => command_palette,        // SPC h d l : copy last keys
                    "s" => command_palette,        // SPC h d s : copy system info
                    // SPC h d c (describe char) -> :character-info via typable table
                },
            },
            "m" => { "Major mode"
                "g" => { "Goto"
                    "g" => goto_definition,        // SPC m g g : go to definition
                },
                "h" => { "Help"
                    "h" => hover,                  // SPC m h h : describe thing at point
                },
            },
            // SPC u : universal-argument prefix. Only the window-layout variants
            // that map to a real command are bound; buffer variants are added via
            // the typable table (SPC u SPC b d / b m).
            "u" => { "Universal arg"
                "space" => { "C-u"
                    "w" => { "Windows"
                        "d" => wclose,             // SPC u SPC w d : delete window + buffer
                        "1" => wonly,              // SPC u SPC w 1 : single-window layout (force)
                        "2" => vsplit,             // SPC u SPC w 2 : two-window layout (force)
                        "3" => vsplit,             // SPC u SPC w 3 : three-window layout (force)
                        "4" => vsplit,             // SPC u SPC w 4 : four-window layout (force)
                    },
                },
            },
        },
    });

    // Visual / select mode: motions extend, operators act directly.
    let mut select = keymap!({ "Visual mode"
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
        "," => repeat_find_char_reverse,

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
            "g" => extend_to_file_start,             // vgg: extend selection to first line
            "C-g" => document_stats,                 // v g CTRL-G: count the selection
            "e" => extend_to_last_line,              // ge: extend to last line
            "h" => extend_to_first_nonwhitespace,    // extend to first non-blank
            "l" | "$" => extend_to_line_end,         // extend to line end
            "q" => [format_selections, normal_mode],
            "w" => [format_selections, normal_mode],
            "v" => reselect_visual,                  // gv: reselect previous highlighted area
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
        // CTRL-\ CTRL-N / CTRL-\ CTRL-G: stop Visual mode, go to Normal mode
        "C-\\" => { "To normal"
            "C-n" => [save_visual_selection, collapse_selection, normal_mode],
            "C-g" => [save_visual_selection, collapse_selection, normal_mode],
        },
        "esc" => [save_visual_selection, collapse_selection, normal_mode],
    });

    // Insert mode: vim-style editing keys.
    let insert = keymap!({ "Insert mode"
        "esc" => [mark_insert_exit, normal_mode],
        "C-c" => [mark_insert_exit, normal_mode],
        "C-[" => [mark_insert_exit, normal_mode],   // CTRL-[ = <Esc>
        "F1"  => [mark_insert_exit, normal_mode],   // i_<F1>: stop insert mode (help omitted)
        // CTRL-\ CTRL-N / CTRL-\ CTRL-G: leave insert for Normal mode
        "C-\\" => { "To normal"
            "C-n" => [mark_insert_exit, normal_mode],
            "C-g" => [mark_insert_exit, normal_mode],
        },

        "backspace" | "C-h" => delete_char_backward,
        "del"               => delete_char_forward,
        "C-w"               => delete_word_backward,
        "A-backspace"       => delete_word_backward,
        "A-d"               => delete_word_forward,
        "C-u"               => kill_to_line_start,
        "C-k"               => insert_digraph,   // vim i_CTRL-K: enter a digraph (was emacs kill-to-eol)

        // indent the current line (vim i_CTRL-T / i_CTRL-D)
        "C-t"   => indent,
        "C-d"   => unindent,

        // keyword/omni completion (vim i_CTRL-N / i_CTRL-P)
        "C-n"   => completion,
        "C-p"   => completion,
        // CTRL-X completion sub-mode: the keyword/identifier/omni variants all
        // map to zemacs's single (LSP + word) completion.
        "C-x" => { "Complete"
            "C-o" => completion,   // omni completion (LSP)
            "C-n" => completion,   // keyword completion, forward
            "C-p" => completion,   // keyword completion, backward
            "C-i" => completion,   // identifier completion
            // The remaining vim CTRL-X completion sub-modes (file names, whole
            // lines, dictionary, defined identifiers, completefunc, tags) all
            // route to zemacs's single LSP+word completion — same trigger, the
            // candidate source differs, so these are tracked as partial.
            "C-f" => completion,   // i_CTRL-X_CTRL-F: file-name completion
            "C-l" => completion,   // i_CTRL-X_CTRL-L: whole-line completion
            "C-k" => completion,   // i_CTRL-X_CTRL-K: dictionary completion
            "C-d" => completion,   // i_CTRL-X_CTRL-D: defined-identifier completion
            "C-u" => completion,   // i_CTRL-X_CTRL-U: 'completefunc' completion
            "C-]" => completion,   // i_CTRL-X_CTRL-]: tag completion
            "C-v" => completion,   // i_CTRL-X_CTRL-V: complete like in : command line
            "s"   => completion,   // i_CTRL-X_s: spelling suggestions
            "C-t" => completion,   // i_CTRL-X_CTRL-T: thesaurus completion
            "C-r" => completion,   // i_CTRL-X_CTRL-R: complete from registers
            "C-s" => completion,   // i_CTRL-X_CTRL-S: spelling suggestions
            "C-e" => scroll_down,  // i_CTRL-X_CTRL-E: scroll window up (view down)
            "C-y" => scroll_up,    // i_CTRL-X_CTRL-Y: scroll window down (view up)
        },
        // i_CTRL-G j/k (and CTRL-J/CTRL-K, <Down>/<Up>): move a display line
        // down/up, toward the column where insertion started.
        "C-g" => { "Insert motion"
            "j" | "C-j" | "down" => move_visual_line_down,
            "k" | "C-k" | "up"   => move_visual_line_up,
            "u" => commit_undo_checkpoint,   // i_CTRL-G_u: break undo so the next edit is a separate change
            "U" => commit_undo_checkpoint,   // i_CTRL-G_U: (approx) don't break undo on next cursor move
        },

        "ret"   => insert_newline,
        "C-j"   => insert_newline,
        "tab"   => insert_tab,

        "C-r"   => insert_register,
        "C-e"   => copy_char_below,         // vim i_CTRL-E: insert the character below the cursor
        "C-y"   => copy_char_above,         // vim i_CTRL-Y: insert the character above the cursor
        "ins"   => replace_mode,           // <Insert>: switch to Replace (overtype) mode

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
        "C-v"     => insert_char_interactive, // vim i_CTRL-V: insert the next key literally
        "C-q"     => insert_char_interactive, // vim i_CTRL-Q: same as CTRL-V (insert next key literally)
        "A-f"     => move_next_word_start, // M-f forward-word
        "A-b"     => move_prev_word_start, // M-b backward-word
        "A-v"     => page_up,              // M-v scroll-down
        "A-<"     => goto_file_start,      // M-< beginning of buffer
        "A->"     => goto_file_end,        // M-> end of buffer
        "A-/"     => completion,           // M-/ dynamic abbrev / completion
        "C-/"     => undo,                 // C-/ undo
        "C-_"     => undo,                 // C-_ undo

        "pageup"   => page_up,
        "pagedown" => page_down,
        "S-up"     => page_up,      // <S-Up> = <PageUp>
        "S-down"   => page_down,    // <S-Down> = <PageDown>
    });

    add_spacemacs_typables(&mut normal);

    // Make git hunk-reset work on a visual selection too: SPC g r in select mode
    // resets every hunk the selection touches (the leader is otherwise
    // normal-mode only). `]c`/`[c` also navigate hunks while selecting.
    if let KeyTrie::Node(sel) = &mut select {
        add_command(sel, &chord("space g r"), "Git", ":hunk-reset");
        add_command(sel, &chord("] c"), "Git", ":hunk-next");
        add_command(sel, &chord("[ c"), "Git", ":hunk-prev");
    }

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
        assert_eq!(
            cmd_name(resolve(n, "C-w H").unwrap()),
            Some("swap_view_left")
        );
        assert_eq!(
            cmd_name(resolve(n, "C-w J").unwrap()),
            Some("swap_view_down")
        );
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
            Some("vim_move_prev_word_end")
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
        assert_eq!(
            cmd_name(resolve(n, "space f f").unwrap()),
            Some("file_picker")
        );
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
            let leaf =
                resolve(n, chord_str).unwrap_or_else(|| panic!("{chord_str} did not resolve"));
            // The bound leaf must equal what the command string parses to, and
            // it must be a typable command.
            let expected =
                KeyTrie::MappableCommand(cmd.parse::<MappableCommand>().expect("valid command"));
            assert_eq!(leaf, &expected, "wrong command for {chord_str}");
            // Entries are mostly typable `:cmd`s, but `add_command` also accepts
            // bare static command names (e.g. `git_file_log_picker`), so allow both.
            assert!(
                matches!(leaf, KeyTrie::MappableCommand(_)),
                "{chord_str} should map to a command"
            );
        }
    }

    #[test]
    fn vim_case_operators_are_sequences() {
        let km = default();
        let n = &km[&Mode::Normal];
        for chord in ["g U U", "g U w", "g u u", "g u w", "g ~ ~", "g ~ w"] {
            let leaf = resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"));
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
            let leaf = resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert!(
                matches!(leaf, KeyTrie::Sequence(_)),
                "{chord} should be a command sequence"
            );
        }
        assert_eq!(cmd_name(resolve(n, "space x tab").unwrap()), Some("indent"));
    }

    #[test]
    fn vim_ported_motion_aliases_and_operators() {
        let km = default();
        let n = &km[&Mode::Normal];
        // ctrl/arrow motion aliases from index.txt
        assert_eq!(cmd_name(resolve(n, "C-h").unwrap()), Some("move_char_left"));
        assert_eq!(
            cmd_name(resolve(n, "C-left").unwrap()),
            Some("move_prev_word_start")
        );
        assert_eq!(
            cmd_name(resolve(n, "C-right").unwrap()),
            Some("move_next_word_start")
        );
        assert_eq!(
            cmd_name(resolve(n, "C-home").unwrap()),
            Some("goto_file_start")
        );
        assert_eq!(
            cmd_name(resolve(n, "C-end").unwrap()),
            Some("goto_last_line")
        );
        assert_eq!(cmd_name(resolve(n, "ins").unwrap()), Some("insert_mode"));
        assert_eq!(
            cmd_name(resolve(n, "C-]").unwrap()),
            Some("goto_definition")
        );
        // gt/gT navigate buffers (vim tabpages)
        assert_eq!(
            cmd_name(resolve(n, "g t").unwrap()),
            Some("goto_next_buffer")
        );
        assert_eq!(
            cmd_name(resolve(n, "g T").unwrap()),
            Some("goto_previous_buffer")
        );
        // = reindent operator is a sequence for motions, leaf for ==
        assert_eq!(cmd_name(resolve(n, "= =").unwrap()), Some("indent"));
        assert!(matches!(resolve(n, "= j").unwrap(), KeyTrie::Sequence(_)));

        // visual block + extras
        let s = &km[&Mode::Select];
        assert_eq!(
            cmd_name(resolve(s, "C-v").unwrap()),
            Some("copy_selection_on_next_line")
        );
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
        assert_eq!(
            cmd_name(resolve(n, "A-x").unwrap()),
            Some("command_palette")
        );
        assert_eq!(
            cmd_name(resolve(n, "A-f").unwrap()),
            Some("move_next_word_start")
        );
        assert_eq!(
            cmd_name(resolve(n, "A-<").unwrap()),
            Some("goto_file_start")
        );
        // vim CTRL-G (file info) wins over the emacs keyboard-quit on C-g; Esc still collapses.
        assert_eq!(cmd_name(resolve(n, "C-g").unwrap()), Some("file_info"));
        assert_eq!(
            cmd_name(resolve(n, "C-l").unwrap()),
            Some("align_view_center")
        );
        // readline motion in insert mode that does NOT clash with a vim insert key
        // (vim leaves C-f/C-b/M-f free in insert) stays emacs.
        assert_eq!(
            cmd_name(resolve(i, "C-f").unwrap()),
            Some("move_char_right")
        );
        assert_eq!(cmd_name(resolve(i, "C-b").unwrap()), Some("move_char_left"));
        assert_eq!(
            cmd_name(resolve(i, "A-f").unwrap()),
            Some("move_next_word_start")
        );
        // vim insert keys win where they conflict with the old emacs bindings.
        assert_eq!(
            cmd_name(resolve(i, "C-e").unwrap()),
            Some("copy_char_below")
        );
        assert_eq!(
            cmd_name(resolve(i, "C-y").unwrap()),
            Some("copy_char_above")
        );
        assert_eq!(cmd_name(resolve(i, "C-k").unwrap()), Some("insert_digraph"));
        assert_eq!(
            cmd_name(resolve(i, "C-v").unwrap()),
            Some("insert_char_interactive")
        );
    }

    #[test]
    fn vim_operator_pending_is_a_sequence() {
        let km = default();
        let n = &km[&Mode::Normal];
        // `dd`, `dw`, `cw`, `yy` resolve to multi-command sequences.
        for chord in ["d d", "d w", "c w", "y y", "d $"] {
            let leaf = resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert!(
                matches!(leaf, KeyTrie::Sequence(_)),
                "{chord} should be an operator sequence, got {leaf:?}"
            );
        }
    }
}
