//! Vim default keymap for zmax.
//!
//! zmax targets vim/emacs semantics rather than Zmax's selection-first
//! model: the keys you press are the keys vim binds. Where vim is verb-noun
//! (operator-pending: `d{motion}`, `c{motion}`, `y{motion}`), we emulate it
//! with nested submaps whose motions run `[collapse_selection, extend-motion,
//! operate]` command sequences. zmax runs on the Zmax engine, so each
//! operator first collapses to the cursor, extends the selection over the
//! motion, then acts — reproducing vim's "operate over the motion" behavior.
//!
//! Numeric counts (`3w`, `d2w`) work for free: the engine consumes a numeric
//! prefix and applies it to the next command.
//!
//! The list of gaps that used to live here is gone: every item on it —
//! operator + find-char (`df<c>`, `ct<c>`), operator + text object (`ciw`,
//! `di(`), `.` repeat-last-change, the `q`/`@` macros, marks, and Replace mode —
//! works, and each was checked against neovim by driving both editors on the
//! same keys. The note outlasted the work it described.
//!
//! What is actually missing is narrower and lives in `port/mapping.json`, which
//! is regenerated from this file and carries the evidence per key. The shape of
//! it: there is no operator-pending mode, so every operator spells its motions
//! out in a submap (`d` has `w`/`i`/`a`/`` ` ``/… listed by hand). Motions no
//! submap lists — `d/pat`, `dn`, `d%` — do nothing, and each new operator has to
//! be taught the whole set, which is how `gUiw` came to be dead while `diw`
//! worked.

use std::collections::HashMap;

use super::macros::keymap;
use super::{KeyTrie, KeyTrieNode, MappableCommand, Mode};
use indexmap::IndexMap;
use zmax_core::hashmap;
use zmax_view::input::KeyEvent;

/// spacemacs SPC bindings that resolve to typable (`:`) commands. The keymap
/// macro can only express static commands, so these are inserted after macro
/// construction. Format: (chord, submap label, command). The chord uses the
/// same space-joined notation the port report parses, so coverage stays honest.
#[rustfmt::skip]
const SPACEMACS_TYPABLE: &[(&str, &str, &str)] = &[
    // These read their argument from a prompt when a key passes none, which is
    // what makes them bindable at all (they used to require the argument).
    ("A-:", "Eval", ":eval-expression"), // M-: eval-expression
    ("space f v f", "File vars", ":add-file-local-variable"), // SPC f v f
    ("space f v p", "File vars", ":add-file-local-variable-prop-line"), // SPC f v p
    // SPC f v d: add a *directory* variable — the `.dir-locals.el` sibling of the
    // two above, which applies to the whole tree rather than to one file.
    ("space f v d", "File vars", "add_dir_local_variable"), // SPC f v d
    // fzf.vim commands under SPC F (external fzf binary; honors $FZF_* env).
    ("space F f", "fzf", ":Files"),      // SPC F f : files
    ("space F g", "fzf", ":GFiles"),     // SPC F g : git files
    ("space F b", "fzf", ":Buffers"),    // SPC F b : buffers
    ("space F c", "fzf", ":Colors"),     // SPC F c : colorschemes
    ("space F r", "fzf", ":Rg"),         // SPC F r : ripgrep
    ("space F a", "fzf", ":Ag"),         // SPC F a : ag (rg backend)
    ("space F l", "fzf", ":BLines"),     // SPC F l : lines in buffer
    ("space F o", "fzf", ":Locate"),     // SPC F o : locate
    ("space F C", "fzf", ":Commands"),   // SPC F C : commands
    ("space F h", "fzf", ":History"),    // SPC F h : recent files
    ("space F L", "fzf", ":Lines"),      // SPC F L : lines (all buffers)
    ("space F t", "fzf", ":Filetypes"),  // SPC F t : filetypes
    ("space F v", "fzf", ":Commits"),    // SPC F v : commits
    ("space F V", "fzf", ":BCommits"),   // SPC F V : buffer commits
    ("space F j", "fzf", ":Jumps"),      // SPC F j : jumplist
    ("space F w", "fzf", ":Windows"),    // SPC F w : windows
    ("space F m", "fzf", ":Marks"),      // SPC F m : marks
    ("space F T", "fzf", ":Tags"),       // SPC F T : tags (whole tree)
    ("space F B", "fzf", ":BTags"),      // SPC F B : buffer tags
    ("space F s", "fzf", ":Snippets"),   // SPC F s : snippets
    ("space F M", "fzf", ":Maps"),       // SPC F M : keymaps
    ("space F H", "fzf", ":Helptags"),   // SPC F H : help tags
    ("space F d", "fzf", ":Todo"),       // SPC F d : TODO/FIXME tool window
    ("space p T", "fzf", ":Todo"),       // SPC p T : project TODOs
    ("space p r", "Projects", ":project-replace"),   // SPC p r : Replace in Path (JetBrains)
    ("space r t", "Bookmarks", "bookmark_toggle"),   // SPC r t : toggle bookmark (JetBrains F11)
    ("space r n", "Bookmarks", "bookmark_next"),     // SPC r n : next bookmark
    ("space r N", "Bookmarks", "bookmark_prev"),     // SPC r N : previous bookmark
    ("space r j", "Bookmarks", "bookmark_jump"),     // SPC r j : jump to a bookmark (picker)
    ("space b r", "Buffers", "recent_files_switcher"), // SPC b r : Recent Files switcher (JetBrains Recent Files)
    ("space g I", "Git", "toggle_inline_blame"),     // SPC g I : toggle GitLens-style inline blame
    ("space g B", "Git", "toggle_blame_annotate"),   // SPC g B : toggle blame annotate gutter (JetBrains Annotate)
    ("space f H", "Files", ":LocalHistory"),         // SPC f H : Local History snapshots for this file
    ("space f E", "Files", ":RevealInFinder"),       // SPC f E : reveal current file in Finder
    ("space b S", "Buffers", ":Scratch"),            // SPC b S : new scratch buffer (JetBrains Scratch File)
    ("space j R", "Jump", ":RecentLocations"),       // SPC j R : Recent Locations (JetBrains Recent Locations)
    ("space f s", "Files",   ":write"),            // SPC f s : save
    ("space f S", "Files",   ":write-all"),        // SPC f S : save all
    ("space a c", "Applications", "calc_dispatch"), // SPC a c : calc-dispatch (open the RPN calculator)
    ("space a =", "Applications", ":calc"),         // SPC a = : quick infix eval of the region/args
    ("space a m", "Applications", ":compose-mail"), // SPC a m : compose-mail (message-mode draft)
    ("space a r", "Applications", "rmail"),         // SPC a r : rmail (open the mail reader)
    ("space m c", "Mail", ":message-send-and-exit"),   // SPC m c : send and kill (C-c C-c)
    ("space m s", "Mail", ":message-send"),            // SPC m s : queue draft (C-c C-s)
    ("space m k", "Mail", ":message-kill-buffer"),     // SPC m k : kill draft (C-c C-k)
    ("space m w", "Mail", ":message-insert-signature"),// SPC m w : insert signature (C-c C-w)
    ("space m t", "Mail", ":message-goto-to"),         // SPC m t : goto To:
    ("space m b", "Mail", ":message-goto-body"),       // SPC m b : goto body (mail-text)
    // Embedded / single-board (Arduino IDE + PlatformIO): SPC a v ("Verify").
    ("space a v v", "Embedded", ":arduino-compile"),   // SPC a v v : Arduino compile/Verify
    ("space a v u", "Embedded", ":arduino-upload"),    // SPC a v u : Arduino upload
    ("space a v m", "Embedded", ":arduino-monitor"),   // SPC a v m : serial monitor
    ("space a v b", "Embedded", ":arduino-boards"),    // SPC a v b : pick board (FQBN)
    ("space a v p", "Embedded", ":arduino-ports"),     // SPC a v p : pick serial port
    ("space a v l", "Embedded", ":arduino-lib-search"),// SPC a v l : library manager
    ("space a v c", "Embedded", ":arduino-core-install"), // SPC a v c : install a board core
    ("space a v g", "Embedded", ":arduino-plotter"),   // SPC a v g : serial plotter (graph)
    ("space a v n", "Embedded", ":arduino-new-sketch"),// SPC a v n : new sketch
    ("space a v B", "Embedded", ":pio-build"),         // SPC a v B : PlatformIO build
    ("space a v U", "Embedded", ":pio-upload"),        // SPC a v U : PlatformIO upload
    ("space a v M", "Embedded", ":pio-monitor"),       // SPC a v M : PlatformIO serial monitor
    ("space a v d", "Embedded", ":pio-devices"),       // SPC a v d : PlatformIO device list
    ("space a v e", "Embedded", ":arduino-compile-export"), // SPC a v e : export compiled binary
    ("space a v o", "Embedded", ":arduino-burn-bootloader"),// SPC a v o : burn bootloader
    ("space a v i", "Embedded", ":arduino-board-info"), // SPC a v i : board info
    ("space a v r", "Embedded", ":arduino-core-search"),// SPC a v r : Boards Manager search
    ("space a v C", "Embedded", ":arduino-core-list"),  // SPC a v C : installed cores
    ("space a v L", "Embedded", ":arduino-lib-list"),   // SPC a v L : installed libraries
    ("space a v k", "Embedded", ":arduino-lib-examples"),// SPC a v k : library examples
    ("space a v K", "Embedded", ":pio-clean"),          // SPC a v K : PlatformIO clean
    ("space a v T", "Embedded", ":pio-test"),           // SPC a v T : PlatformIO test
    ("space a v H", "Embedded", ":pio-check"),          // SPC a v H : PlatformIO check
    ("space a v P", "Embedded", ":pio-lib-list"),       // SPC a v P : PlatformIO packages
    ("space a v s", "Embedded", ":pio-lib-search"),     // SPC a v s : PlatformIO registry search
    ("space a v O", "Embedded", ":arduino-outdated"),   // SPC a v O : outdated cores/libraries
    ("space a v x", "Embedded", ":arduino-debug"),      // SPC a v x : Arduino debugger
    ("space a v X", "Embedded", ":pio-debug"),          // SPC a v X : PlatformIO debugger
    ("space f R", "Files",   ":move"),             // SPC f R : rename file
    ("space f D", "Files",   ":delete-file"),      // SPC f D : delete file + buffer
    ("space b M", "Buffers", "buffer_menu"),       // SPC b M : Buffer Menu (emacs buffer-menu)
    ("space b d", "Buffers", ":buffer-close"),     // SPC b d : kill buffer
    ("space b X", "Buffers", ":buffer-close!"),    // SPC b X : FORCE kill buffer (discard unsaved)
    ("space b K", "Buffers", ":buffer-close-all!"), // SPC b K : FORCE kill ALL buffers
    ("space b D", "Buffers", ":buffer-close-others"), // SPC b C-d / others
    ("space b . C-d", "Buffers", "bury_buffer"),   // SPC b . C-d : bury current buffer
    ("space b R", "Buffers", ":reload"),           // SPC b R : revert
    ("space b N n", "Buffers", ":new"),            // SPC b N n : new buffer, current window
    ("space b N i", "Buffers", "clone_indirect_buffer"), // SPC b N i : indirect clone (shared-doc split)
    ("space b N I", "Buffers", "clone_indirect_buffer"), // SPC b N I : indirect clone of current buffer
    ("space b C-D", "Buffers", "kill_buffers_by_regex"), // SPC b C-D : kill buffers matching a regex
    ("space b N C-i", "Buffers", "clone_indirect_from_buffer"), // SPC b N C-i : indirect from existing buffer (shared-doc split)
    ("space q q", "Quit",    ":quit-all"),         // SPC q q : quit
    ("space q Q", "Quit",    ":quit-all!"),        // SPC q Q : force quit
    ("space q s", "Quit",    ":write-quit-all"),   // SPC q s : save and quit
    ("space q r", "Quit",    "restart_editor"),    // SPC q r : restart zmax (restart-emacs)
    ("space f T", "Files",   ":theme"),            // SPC T n / theme
    ("space x l s", "Text",  ":sort"),             // SPC x l s : sort lines
    ("space x g t", "Translate", ":translate"),           // SPC x g t : translate word (google-translate-at-point)
    ("space x g l", "Translate", ":translate-set-languages"), // SPC x g l : set translate languages
    ("space x g T", "Translate", ":translate-reverse"),   // SPC x g T : reverse translate languages
    // SPC t toggles -> existing :toggle substrate (config options).
    ("space t z", "Toggles", "toggle_column_indexing"), // SPC t z : toggle 0/1-based column indexing
    ("space t n r", "Toggles", ":toggle line-number absolute relative"), // relative nums
    ("space t n a", "Toggles", ":toggle line-number relative absolute"), // absolute nums
    ("space t n n", "Toggles", ":toggle line-number absolute relative"), // SPC t n n : toggle line numbers
    ("space t C-w", "Toggles", ":toggle whitespace.render all none"),    // SPC t C-w : global whitespace
    ("space t i",   "Toggles", ":toggle indent-guides.render"),          // indent guides
    ("space t t",   "Toggles", ":theme-toggle"),                         // SPC t t : light/dark toggle
    ("space t a",   "Toggles", ":toggle auto-completion"),               // auto-complete
    ("space t h h", "Toggles", ":toggle cursorline"),                    // highlight line
    ("space t w",   "Toggles", ":toggle whitespace.render all none"),    // whitespace
    ("space t c",   "Toggles", "toggle_subword"),                        // SPC t c : sub-word motion
    ("space t C",   "Toggles", "toggle_superword"),                      // SPC t C : super-word motion (emacs superword-mode)
    ("space t F",   "Toggles", "toggle_auto_fill"),                      // SPC t F : auto-fill mode
    ("space x d w", "Text",    ":delete-trailing-whitespace"),           // SPC x d w
    ("space x l d", "Text",    ":duplicate-line"),                       // SPC x l d
    ("space x o",   "Text",    "select_all_occurrences"),               // SPC x o : select all occurrences of selection (JetBrains Select All Occurrences)
    ("space x A",   "Text",    "open_all_buffer_links"),                // SPC x A : open every link in the buffer (link-hint-open-all-links)
    ("space x O",   "Text",    "link_hint_open_link"),                  // SPC x O : pick a link and open it (link-hint-open-link)
    ("space x y",   "Text",    "link_hint_copy_link"),                  // SPC x y : pick a link and copy it (link-hint-copy-link)
    ("space x Y",   "Text",    "copy_all_buffer_links"),                // SPC x Y : copy every link in the buffer (link-hint-copy-all-links)
    ("space x >",   "Text",    "move_element_right"),                   // SPC x > : swap syntax node with next sibling (JetBrains Move Element Right)
    ("space x <",   "Text",    "move_element_left"),                    // SPC x < : swap syntax node with prev sibling (JetBrains Move Element Left)
    ("space x J",   "Text",    ":move-line-down"),                       // SPC x J : drag down
    ("space x K",   "Text",    ":move-line-up"),                         // SPC x K : drag up
    ("space x t c", "Text",    ":transpose-chars"),                      // SPC x t c
    ("space x t l", "Text",    ":move-line-up"),                         // SPC x t l : transpose lines
    ("space x t w", "Text",    ":transpose-words"),                      // SPC x t w
    ("space x s",   "Text",    ":Thesaurus"),                            // SPC x s : synonyms for word under cursor
    ("space x r '", "Text",    "regexp_generate_strings"),               // SPC x r ' : generate strings from a finite regexp
    ("space x r p '", "Text",  "regexp_generate_strings"),               // SPC x r p ' : generate strings from a finite PCRE regexp
    ("space x r e '", "Text",  "regexp_generate_strings_emacs"),         // SPC x r e ' : generate strings from a finite Emacs regexp
    ("space x r c", "Text",    "regex_convert_form"),                    // SPC x r c : convert regex PCRE <-> Emacs
    ("space x r x", "Text",    "regex_pcre_to_rx_replace"),              // SPC x r x : regex around point -> rx form
    ("space x r /", "Text",    "regex_pcre_to_rx_explain"),              // SPC x r / : explain regex as rx
    ("space x r e x", "Text",  "regex_emacs_to_rx_replace"),            // SPC x r e x : Emacs regex -> rx form
    ("space x r e t", "Text",  "regex_emacs_to_rx_replace"),            // SPC x r e t : replace Emacs regex by rx form
    ("space x r e /", "Text",  "regex_emacs_to_rx_explain"),            // SPC x r e / : explain Emacs regex as rx
    ("space x r e p", "Text",  "regex_convert_form"),                    // SPC x r e p : Emacs regex -> PCRE
    ("space x r p e", "Text",  "regex_convert_form"),                    // SPC x r p e : PCRE -> Emacs regex
    ("space x r p x", "Text",  "regex_pcre_to_rx_replace"),              // SPC x r p x : PCRE regex -> rx form
    ("space x r p /", "Text",  "regex_pcre_to_rx_explain"),              // SPC x r p / : explain PCRE regex as rx
    ("space x r t", "Text",    "regex_pcre_to_rx_replace"),              // SPC x r t : replace regexp around point by rx
    ("space x t p", "Text",    "transpose_paragraph"),                   // SPC x t p : swap current/previous paragraph
    ("space x t e", "Text",    "transpose_sexp"),                        // SPC x t e : swap current/previous sexp
    ("space x t s", "Text",    "transpose_sentence"),                    // SPC x t s : swap current/previous sentence
    ("space x l u", "Text",    ":uniquify-lines"),                       // SPC x l u
    ("space x d l", "Text",    ":delete-blank-lines"),                   // SPC x d l
    ("space x d space", "Text", ":just-one-space"),                      // SPC x d SPC
    ("space x d f", "Text",    "c_hungry_delete_forward"),               // SPC x d f : hungry-delete forward (emacs c-hungry-delete-forward)
    ("space x d b", "Text",    "c_hungry_delete_backwards"),             // SPC x d b : hungry-delete backward (emacs c-hungry-delete-backwards)
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
    ("space g i",   "Git",     "git_init"),                             // SPC g i : initialize a new git repo
    ("space g f d", "Git",     "git_diff"),                             // SPC g f d : diff current file vs HEAD
    ("space g f m", "Git",     "git_file_dispatch"),                    // SPC g f m : magit file-operations dispatch
    ("space g f f", "Git",     "view_file_at_rev"),                     // SPC g f f : view current file at a branch/commit
    ("space g P",   "Git",     "git_push"),                             // SPC g P : push current branch (JetBrains Push)
    ("space g u",   "Git",     "git_pull"),                             // SPC g u : fast-forward pull (JetBrains Update Project)
    ("space g F",   "Git",     "git_fetch"),                            // SPC g F : fetch all remotes
    ("space f e d", "Files",   ":config-open"),                          // SPC f e d : open dotfile/config
    ("space q f",   "Quit",    ":quit"),                                 // SPC q f : kill frame
    ("space b s",   "Buffers", ":new"),                                  // SPC b s : scratch buffer
    ("space h t",   "Help",    ":tutor"),                                // SPC h t : start the tutor
    ("space h P s", "Help",    "profiler_start"),                        // SPC h P s : start the command profiler
    ("space h P k", "Help",    "profiler_stop"),                         // SPC h P k : stop the command profiler
    ("space h P r", "Help",    "profiler_report"),                       // SPC h P r : display the profiler report
    ("space h P w", "Help",    "profiler_write_report"),                 // SPC h P w : write the profiler report to a file
    ("space q a",   "Quit",    ":quit-all"),                             // SPC q a : quit all
    ("space q w",   "Quit",    ":write-quit"),                           // SPC q w : write & quit window
    ("space b C-d", "Buffers", ":buffer-close-others"),                  // SPC b C-d : kill other buffers
    ("space b x",   "Buffers", ":buffer-close"),                         // SPC b x : kill buffer & window
    ("space b e",   "Buffers", ":reload"),                              // SPC b e : revert/erase to disk
    ("space p k",   "Project", ":buffer-close-all"),                    // SPC p k : kill all project buffers
    ("space t l",   "Toggles", ":toggle soft-wrap.enable"),            // SPC t l : truncate/wrap lines
    ("space t L",   "Toggles", ":toggle soft-wrap.enable"),            // SPC t L : toggle visual (wrapped) lines
    ("space t W",   "Toggles", ":toggle trim-trailing-whitespace"),    // SPC t W : auto whitespace cleanup on save
    ("space t g",   "Toggles", "golden_ratio_resize"),                  // SPC t g : golden-ratio window resize
    ("space t m p", "Toggles", "toggle_modeline_position"),            // SPC t m p : toggle point position in mode line
    ("space t m v", "Toggles", "toggle_modeline_vcs"),                 // SPC t m v : toggle VC info in mode line
    ("space t m T", "Toggles", ":toggle render-statusline"),           // SPC t m T : toggle the mode line itself
    ("space T T",   "Toggles", ":toggle transparent-background"),      // SPC T T : toggle background transparency
    ("space T B",   "Toggles", ":toggle transparent-background"),      // SPC T B : toggle background transparency
    ("space T f",   "Toggles", "toggle_fringe"),                       // SPC T f : toggle fringe (gutter) display
    ("space t m b", "Toggles", "display_battery_mode"),                // SPC t m b : toggle the battery status
    ("space t m t", "Toggles", "display_time"),                        // SPC t m t : toggle the time
    ("space t -",   "Toggles", "toggle_centered_cursor"),             // SPC t - : centered-cursor mode
    ("space t f",   "Toggles", "toggle_fill_column"),                 // SPC t f : show fill-column ruler
    ("space t 8",   "Toggles", "toggle_long_line_marker"),            // SPC t 8 : highlight 80th column
    ("space t C-8", "Toggles", "toggle_long_line_marker"),            // SPC t C-8 : global 80-col highlight
    ("space t C-W", "Toggles", ":toggle trim-trailing-whitespace"),    // SPC t C-W : global whitespace cleanup
    ("space D f v", "Diff",    "git_diff"),                            // SPC D f v : ediff file versions (vs HEAD)
    ("space D f f", "Diff",    "ediff_file"),                          // SPC D f f : ediff current buffer vs a picked file
    ("space D f 3", "Diff",    "ediff_3_files"),                       // SPC D f 3 : 3-way diff of three files (read-only)
    ("space D d d", "Diff",    "ediff_directories"),                   // SPC D d d : ediff two directories (same-name files)
    ("space D d 3", "Diff",    "ediff_directories3"),                  // SPC D d 3 : ediff three directories (same-name files)
    ("space D r l", "Diff",    "ediff_regions"),                      // SPC D r l : ediff two regions linewise
    ("space D m f f", "Diff",  "ediff_merge_file"),                   // SPC D m f f : merge a file into current buffer
    ("space D b 3", "Diff",    "ediff_3_buffers"),                     // SPC D b 3 : 3-way diff of three buffers (read-only)
    ("space D b b", "Diff",    "ediff_buffer"),                        // SPC D b b : ediff two buffers (current vs picked)
    ("space D c",   "Diff",    "compare_with_clipboard"),              // SPC D c : diff current buffer vs clipboard (JetBrains Compare with Clipboard)
    ("space D w w", "Diff",    "ediff_windows"),                       // SPC D w w : compare the two windows (wordwise)
    ("space D w l", "Diff",    "ediff_windows"),                       // SPC D w l : compare the two windows (linewise)
    ("space t V",   "Toggles", ":toggle line-number absolute relative"), // SPC t V : visual line numbers
    ("space t h i", "Toggles", ":toggle indent-guides.render"),        // SPC t h i : highlight indentation
    ("space t C-i", "Toggles", ":toggle indent-guides.render"),        // SPC t C-i : global indent guide
    ("space t h c", "Toggles", ":toggle cursorcolumn"),                // SPC t h c : highlight current column
    ("space t h s", "Toggles", "toggle_syntax_highlighting"),          // SPC t h s : toggle syntax highlighting
    ("space t h a", "Toggles", "toggle_auto_highlight"),               // SPC t h a : toggle auto symbol highlight
    ("space t s",   "Toggles", "toggle_diagnostics"),                  // SPC t s : toggle diagnostics (flycheck)
    ("space t C-S-l", "Toggles", ":toggle soft-wrap.enable"),          // SPC t C-S-l : visual line navigation
    ("space t K", "Toggles", ":toggle auto-info"),                     // SPC t K : which-key (auto-info) mode
    ("space t k k", "Toggles", ":toggle auto-info"),                   // SPC t k k : which-key persistent state
    ("space t p", "Toggles", ":toggle auto-pairs"),                    // SPC t p : smartparens (auto-pairs)
    ("space t C-p", "Toggles", ":toggle auto-pairs"),                  // SPC t C-p : global smartparens
    // SPC T c (theme_picker) is a static command, bound in the macro keymap below.
    ("space T s", "Themes", ":theme"),                                 // SPC T s : select theme
    ("space T n", "Themes", ":theme-next"),                            // SPC T n : next theme
    ("space T p", "Themes", ":theme-prev"),                            // SPC T p : previous theme
    ("space h T v", "Help", ":tutor"),                                 // SPC h T v : evil tutor
    ("space h d c", "Help",    ":character-info"),                     // SPC h d c : describe char under point
    ("space p e",   "Project", "edit_project_config"),                // SPC p e : edit project-local .zmax/config.toml (dir-locals)
    ("space f e i", "Files",   ":config-open"),                       // SPC f e i : open init/config
    ("space f e e", "Files",   "show_environment"),                   // SPC f e e : show editor environment variables
    ("space f e E", "Files",   "reimport_shell_env"),                 // SPC f e E : reload env from the shell
    ("space f e v", "Files",   "copy_version"),                       // SPC f e v : display and copy the version
    ("space f e R", "Files",   ":config-reload"),                     // SPC f e R : resync the dotfile
    ("space f e C-e", "Files",  "reimport_shell_env"),                // SPC f e C-e : re-import shell environment
    ("space f C d", "Files",   ":line-ending crlf"),                  // SPC f C d : unix -> dos line endings
    ("space f C u", "Files",   ":line-ending lf"),                    // SPC f C u : dos -> unix line endings
    ("space e y",   "Errors",  ":yank-diagnostic"),                   // SPC e y : copy error at point
    ("space x x",   "Text",    ":run-shell-command"),                 // SPC x x : quickrun (run a command)
    ("space u space b d", "Universal", ":buffer-close"),              // SPC u SPC b d : kill buffer + window
    ("space u space b D", "Universal", "delete_window_and_buffer"),   // SPC u SPC b D : kill visible buffer + window
    ("space u space b m", "Universal", ":buffer-close-others"),       // SPC u SPC b m : kill other buffers
    ("space b . d", "Buffers", ":buffer-close"),                     // SPC b . d : kill current buffer
    ("space b . x", "Buffers", ":buffer-close"),                     // SPC b . x : kill buffer and window

    // Keyboard macros (SPC K) — the leader half of the emacs `C-x C-k` map.
    ("space K K",   "Macros",     "kmacro_end_or_call_macro"),   // SPC K K : stop recording, else run the last macro
    ("space K v",   "Macros",     "kmacro_ring_view"),           // SPC K v : view the last macro string
    ("space K c C", "Counter",    "kmacro_set_counter"),         // SPC K c C : set the macro counter
    ("space K c f", "Counter",    "kmacro_set_format"),          // SPC K c f : set the counter display format
    ("space K e b", "Edit macro", "kmacro_bind_to_key"),         // SPC K e b : bind the last macro to a key
    ("space K e e", "Edit macro", "kmacro_edit_macro"),          // SPC K e e : edit the last macro in a buffer
    ("space K e l", "Edit macro", "kmacro_edit_lossage"),        // SPC K e l : edit a macro from lossage
    ("space K e s", "Edit macro", "kmacro_step_edit_macro"),     // SPC K e s : step-edit the last macro

    // Toggles / help.
    ("space t S",   "Toggles", "flyspell_mode"),      // SPC t S : toggle spell checking (flyspell)
    ("space t k m", "Toggles", "describe_keymap"),    // SPC t k m : show the major-mode keymap
    ("space t k t", "Toggles", "describe_bindings"),  // SPC t k t : show the top-level keymap
    ("space h d K", "Help",    "describe_keymap"),    // SPC h d K : describe a keymap

    // Goto / project.
    ("space m g G", "Goto",    "xref_find_definitions_other_window"), // SPC m g G : definition in another window
    ("space p !",   "Project", "project_shell_command"),              // SPC p ! : shell command in the project root
    ("space p &",   "Project", "project_async_shell_command"),        // SPC p & : async shell command in the project root
    ("space p F",   "Project", "goto_file"),                          // SPC p F : find file from the path around point
    ("space p E",   "Project", "xref_find_references"),               // SPC p E : find references
    ("space p R",   "Project", ":project-replace"),                   // SPC p R : replace a string across the project
    ("space p D",   "Project", "dired"),                              // SPC p D : open the project root in dired

    // Keyboard macros (SPC K). `SPC K K` (stop/run) is in the macro map; the
    // start/insert-counter half is the emacs F3 command.
    ("space K k",   "Macro", "kmacro_start_macro_or_insert_counter"), // SPC K k : start recording / insert the counter

    // Editing style (SPC t E) — Spacemacs switches evil/holy/hybrid at runtime;
    // zmax switches the whole keymap preset, which is the same knob.
    ("space t E e", "Editing style", ":keymap emacs"),      // SPC t E e : emacs editing style (holy mode)
    ("space t E h", "Editing style", ":keymap spacemacs"),  // SPC t E h : hybrid (vim keys + emacs prefixes in insert)
    ("space t k M", "Toggles", "describe_keymap"),          // SPC t k M : full major-mode keymap

    // Text / lookup.
    ("space x w d", "Text", ":dictionary-search"),          // SPC x w d : define the word at point

    // ediff (SPC D): the region-wise and backup-file sessions.
    ("space D r w", "Diff", "ediff_regions"),               // SPC D r w : ediff two regions
    ("space D B",   "Diff", "diff_backup"),                 // SPC D B   : diff this file against its backup
    // `ediff-patch-file`: apply the patch in the current buffer to its target and
    // review the result side by side. Spacemacs prompts for the patch buffer; the
    // zmax port takes the patch from the buffer you run it in.
    ("space D f p", "Diff", "diff_ediff_patch"),            // SPC D f p : ediff-patch-file

    // ediff merge sessions (SPC D m). `SPC D m f f` (merge a file into the
    // buffer) is above; these are the three-way / two-buffer merges, which the
    // emerge ports implement (emerge is emacs's other merge engine — same verb:
    // produce a merge buffer from two sources, optionally against an ancestor).
    ("space D m b b", "Diff", "emerge_buffers"),               // SPC D m b b : merge two buffers
    ("space D m b 3", "Diff", "emerge_buffers_with_ancestor"), // SPC D m b 3 : merge two buffers + ancestor
    ("space D m f 3", "Diff", "emerge_files_with_ancestor"),   // SPC D m f 3 : merge two files + ancestor

    // Help.
    ("space h d T", "Help", ":describe-theme"),             // SPC h d T : describe a theme
    // SPC h d P : describe a package — the same picker `C-h P` (describe-package)
    // opens, over the language-support packages zmax knows about.
    ("space h d P", "Help", "package_search"),
    // Help-buffer navigation. Spacemacs binds `g b` / `g f` in help mode; `g f`
    // is vim's goto-file here, so only the back half can take the vim key. The
    // command is a no-op (it reports an error) unless the *Help* window is open,
    // which is exactly the mode-local behaviour spacemacs gives it.
    ("g b", "Help", "help_go_back"),                        // g b : help-go-back

    // NOTE: `[w` / `]w` (previous/next window) live in the `[` / `]` submaps of
    // the keymap! macro above — they are real static commands and belong there.
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
    // `do` / `dp` are vim's "same as :diffget" / "same as :diffput" (index.txt) —
    // obtain the other side's version of the change under the cursor, or write
    // ours into the base. Both typables exist, so both keys are bound.
    ("d o", "Diff", ":diffget"),      // do: :diffget on the change under the cursor
    ("d p", "Diff", ":diffput"),      // dp: :diffput the change under the cursor
];

/// Emacs global chords whose port is a typable (`:`) command, so the keymap macro
/// cannot express them. A keybinding runs the command with no arguments, so a
/// typable that *requires* one (`:eval-expression <sexp>`) would only ever raise
/// a "wrong number of arguments" error; those stay unbound rather than broken.
#[rustfmt::skip]
const EMACS_TYPABLE: &[(&str, &str, &str)] = &[
    ("A-t",     "Emacs", ":transpose-words"),          // M-t transpose-words
    ("A-space", "Emacs", ":just-one-space"),           // M-SPC just-one-space
    ("A-\\",    "Emacs", ":delete-horizontal-space"),  // M-\ delete-horizontal-space
    ("A-C-o",   "Emacs", ":split-line"),               // C-M-o split-line
    ("C-S-backspace", "Emacs", ":kill-whole-line"),    // C-S-DEL kill-whole-line
    // M-s h: the hi-lock map, the same commands the `C-x w` map runs. The three
    // chords that read a regexp (`M-s h r` / `l` / `p`) *prompt* for it when
    // given no argument (see `highlight_regexp` in commands/typed.rs), so they
    // are bindable — they used to be left out back when they hard-failed.
    ("A-s h r", "Highlight", ":highlight-regexp"),                 // M-s h r highlight-regexp
    ("A-s h l", "Highlight", ":highlight-lines-matching-regexp"),  // M-s h l highlight-lines-matching-regexp
    ("A-s h p", "Highlight", ":highlight-phrase"),                 // M-s h p highlight-phrase
    ("A-s h .", "Highlight", ":highlight-symbol-at-point"),        // M-s h . highlight-symbol-at-point
    ("A-s h u", "Highlight", ":unhighlight-regexp"),               // M-s h u unhighlight-regexp (no arg = all)
    ("A-s h f", "Highlight", ":hi-lock-find-patterns"),            // M-s h f hi-lock-find-patterns
    ("A-s h w", "Highlight", ":hi-lock-write-interactive-patterns"), // M-s h w hi-lock-write-interactive-patterns
];

fn add_spacemacs_typables(normal: &mut KeyTrie) {
    if let KeyTrie::Node(root) = normal {
        for (ch, label, cmd) in SPACEMACS_TYPABLE
            .iter()
            .chain(VIM_TYPABLE)
            .chain(EMACS_TYPABLE)
        {
            add_command(root, &chord(ch), label, cmd);
        }
    }
}

/// Walk `path` from `root`, returning the submap it names.
fn node_at<'a>(root: &'a mut KeyTrieNode, path: &[KeyEvent]) -> Option<&'a mut KeyTrieNode> {
    let mut cur = root;
    for key in path {
        match cur.get_mut(key)? {
            KeyTrie::Node(node) => cur = node,
            _ => return None,
        }
    }
    Some(cur)
}

/// Bind `keys` under the submap at `parent` to the transient state `ts`, each
/// with its own opening command: the key performs the command *and* latches the
/// state, which is how Spacemacs's `SPC w [` shrinks a window and leaves you in
/// the window transient state, where a bare `[` shrinks again.
fn add_transient_entries(
    root: &mut KeyTrieNode,
    parent: &str,
    ts: &KeyTrieNode,
    keys: &[(&str, MappableCommand)],
) {
    let Some(parent) = node_at(root, &chord(parent)) else {
        return;
    };
    for (key, cmd) in keys {
        parent.insert(
            key.parse::<KeyEvent>().expect("valid key"),
            KeyTrie::Node(ts.transient_entry(cmd.clone())),
        );
    }
}

/// The transient states whose entry key also acts (see [`add_transient_entries`]).
/// The states entered by a bare prefix (`SPC w .`, `SPC b .`, `SPC l w`,
/// `SPC x .`, `SPC z x`, `SPC z f`) are declared `sticky=true` in [`base`]
/// directly; these are the ones Spacemacs also reaches through an acting key.
fn add_transient_states(normal: &mut KeyTrie) {
    let Some(root) = normal.node_mut() else {
        return;
    };

    // Window transient state — the body already exists at `SPC w .`. Spacemacs
    // also enters it from the resize keys, which is why `SPC w [` was a one-shot
    // shrink here before.
    // Spacemacs enters it with `SPC w [` / `SPC w {` (shrink and stay); the other
    // resize keys (`+ - < >`) stay one-shot because vim binds them under `C-w`,
    // which mirrors this map (see `aliased_modes_are_same_in_default_keymap`) and
    // must keep pure-vim semantics.
    if let Some(window_ts) = node_at(root, &chord("space w .")).cloned() {
        let entries: &[(&str, MappableCommand)] = &[
            ("[", MappableCommand::resize_view_narrower),
            ("{", MappableCommand::resize_view_shorter),
        ];
        add_transient_entries(root, "space w", &window_ts, entries);
        // `C-w` is the vim-side alias of the same window menu and must stay a
        // superset of it (`aliased_modes_are_same_in_default_keymap`), so it gets
        // the same entries — and the same state, rather than its own copy of the
        // body.
        if let Some(ctrl_w) = node_at(root, &chord("C-w")) {
            ctrl_w.insert(chord(".")[0], KeyTrie::Node(window_ts.clone()));
        }
        add_transient_entries(root, "C-w", &window_ts, entries);
    }

    // Numbers transient state (`SPC n +` / `SPC n -`): keep incrementing the
    // number under the cursor with bare `+`/`-`.
    if let KeyTrie::Node(numbers_ts) = keymap!({ "Numbers transient" sticky=true
        "+" | "=" => increment,
        "-" | "_" => decrement,
        "q" => exit_transient_state,
    }) {
        add_transient_entries(
            root,
            "space n",
            &numbers_ts,
            &[
                ("+", MappableCommand::increment),
                ("=", MappableCommand::increment),
                ("-", MappableCommand::decrement),
                ("_", MappableCommand::decrement),
            ],
        );
    }

    // Errors transient state (`SPC e n` / `SPC e p`): walk diagnostics with bare
    // `n`/`p` until you leave.
    if let KeyTrie::Node(errors_ts) = keymap!({ "Errors transient" sticky=true
        "n" | "j" => goto_next_diag,
        "p" | "k" => goto_prev_diag,
        "f" => goto_first_diag,
        "l" => diagnostics_picker,
        "q" => exit_transient_state,
    }) {
        add_transient_entries(
            root,
            "space e",
            &errors_ts,
            &[
                ("n", MappableCommand::goto_next_diag),
                ("p", MappableCommand::goto_prev_diag),
            ],
        );
    }
}

/// The shared vim/evil base keymap, **including** the spacemacs `SPC` leader.
/// The `vim` preset ([`default`]) strips the leader from this; the `spacemacs`
/// preset ([`super::spacemacs::default`]) overlays the Emacs `C-x` prefix on it.
#[rustfmt::skip]
pub(crate) fn base() -> HashMap<Mode, KeyTrie> {
    let mut normal = keymap!({ "Normal mode"
        // --- left-hand motions ---------------------------------------------
        // The cursor keys are vim's "special keys" and answer to 'keymodel'
        // stopsel; h/j/k/l are not special and never stop Select mode, so the
        // pairs cannot share a binding.
        "h" => move_char_left,
        "left" => left_key,
        // vim `j`/`k` move by *text* line; the display-line moves are `gj`/`gk`
        // (`:h gj`). These two were the other way round, so on any wrapped line —
        // and soft-wrap is on by default — `j` walked to the next screen row
        // instead of the next line.
        "j" => move_line_down,
        "down" => down_key,
        "k" => move_line_up,
        "up" => up_key,
        "l" => move_char_right,
        "right" => right_key,
        // JetBrains Move Statement Up/Down (relocate the line/selection).
        "A-up"   => shift_line_up,
        "A-down" => shift_line_down,
        // Wildfire (vim plugin port): <BS> shrinks back to the previously
        // selected text object. Replaces vim's `move_char_left` on backspace.
        "backspace"   => wildfire_shrink,

        // --- word motions ---------------------------------------------------
        // vim caret semantics: land *on* the target char, not the selection
        // off-by-one block-cursor position. See `move_word_vim_impl`.
        "w" => subword_w,
        "b" => subword_b,
        "e" => subword_e,
        "W" => vim_move_next_long_word_start,
        "B" => vim_move_prev_long_word_start,
        "E" => vim_move_next_long_word_end,

        // --- line / column motions -----------------------------------------
        "0" => goto_line_start,
        "home" => home_key,
        "S-home" => shift_home_key,
        "^"          => goto_first_nonwhitespace,
        "$" => goto_line_end,
        "end" => end_key,
        "S-end" => shift_end_key,
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
        // easymotion: f/t/F/T label every visible target and jump by label.
        "f" => find_char_forward_label,
        "F" => find_char_backward_label,
        "t" => till_char_forward_label,
        "T" => till_char_backward_label,
        ";" => repeat_find_char,         // vim ; : repeat last f/t/F/T (same dir, across lines)

        // --- search ---------------------------------------------------------
        "/" => search,
        "?" => rsearch,
        "n" => search_next_vim,  // vim n: repeat in the last search direction (backward after ?)
        "N" => search_prev_vim,  // vim N: repeat in the opposite direction
        "*" => [search_selection_detect_word_boundaries, search_next],
        "#" => [search_selection_detect_word_boundaries, search_prev], // backward word search

        // --- line motions to first non-blank ------------------------------
        // Wildfire (vim plugin port): <CR> selects/expands to the closest
        // enclosing text object (N<CR> jumps to the Nth closest). This takes
        // over Enter in Normal mode; `+` keeps the original down + first
        // non-blank motion.
        "ret"       => wildfire,
        "+"         => [move_visual_line_down, goto_first_nonwhitespace],
        "-"         => [move_visual_line_up, goto_first_nonwhitespace],
        "_"         => goto_first_nonwhitespace_down, // vim _: honours count (N-1 lines down)

        // --- macros ---------------------------------------------------------
        "q" => vim_record_macro,  // q{reg} record (q again to stop)
        "@" => vim_replay_macro,  // @{reg} replay
        "Q" => replay_macro,      // Q replay last/default register

        // --- misc ----------------------------------------------------------
        "K" => keyword_lookup, // vim K: keywordprg on the word, else LSP hover

        // --- insert entry ---------------------------------------------------
        "i" => insert_mode,
        "I" => insert_at_line_start,
        "a" => append_mode,
        "A" => insert_at_line_end,
        "o" => open_below,
        "O" => open_above,

        // --- single-key edits ----------------------------------------------
        "x" => delete_chars_forward_vim,    // delete char(s) under cursor (count, line-bounded)
        "del" => delete_chars_forward_vim,  // <Del> = x
        "X" => delete_chars_backward_vim,   // delete char(s) before cursor (no line join)
        "D" => [extend_to_line_end, delete_selection],
        "C" => [extend_to_line_end, change_selection],
        "Y" => [extend_to_line_bounds, yank, collapse_selection],
        "s" => sneak_or_substitute_char,    // vim-sneak (editor.vim-sneak=true) else substitute char
        "S" => sneak_or_substitute_line,    // vim-sneak backward, else substitute line
        "r" => replace_chars_vim,          // replace count char(s), line-bounded (vim r)
        "R" => replace_mode,                // enter Replace mode (overtype)
        "J" => join_lines_vim,              // join line(s) with a space, cursor at join
        "~" => switch_case_forward,         // toggle case and advance cursor
        "p" => paste_after,
        "P" => paste_before,
        "u" => undo,
        "C-r" => redo,

        // --- operator-pending: delete --------------------------------------
        "d" => { "delete"
            "d" => [collapse_selection, extend_to_line_bounds, delete_selection_linewise, goto_first_nonwhitespace],
            // dj/dk: linewise, current line + count lines below/above (vim `dj` = 2 lines).
            "j" => [collapse_selection, extend_line_below_linewise, delete_selection_linewise, goto_first_nonwhitespace],
            "k" => [collapse_selection, extend_line_above_linewise, delete_selection_linewise, goto_first_nonwhitespace],
            "w" => [collapse_selection, subword_extend_w, delete_selection],
            "W" => [collapse_selection, extend_next_long_word_start, delete_selection],
            "e" => [collapse_selection, subword_extend_e, delete_selection],
            "E" => [collapse_selection, extend_next_long_word_end, delete_selection],
            "b" => [collapse_selection, subword_extend_b, extend_backward_exclusive_vim, delete_selection],
            "B" => [collapse_selection, extend_prev_long_word_start, extend_backward_exclusive_vim, delete_selection],
            "h" => [extend_chars_left_vim, delete_selection],   // dh: count chars left
            "l" => [extend_chars_right_vim, delete_selection],  // dl: count chars right (like x)
            "space" => [extend_chars_right_vim, delete_selection], // d<space>: like dl
            "$" => [collapse_selection, extend_to_line_end, delete_selection],
            // vim backward motions are exclusive: the char under the cursor stays.
            "0" => [collapse_selection, extend_to_line_start, extend_backward_exclusive_vim, delete_selection],
            "^" => [collapse_selection, extend_to_first_nonwhitespace, extend_backward_exclusive_vim, delete_selection],
            "}" => [collapse_selection, select_paragraph_forward_vim, delete_selection], // d} (exclusive→linewise)
            "{" => [collapse_selection, select_paragraph_backward_vim, delete_selection], // d{
            // dG: snap the span to whole lines before deleting - vim's G is a
            // linewise motion. delete_selection_linewise only skips the
            // selection=exclusive adjustment; it does not widen the range, so
            // without this the last line is deleted from its first column
            // rather than entirely (matching cG/cgg, which already do this).
            "G" => [collapse_selection, extend_to_last_line, extend_to_line_bounds, delete_selection_linewise],
            "g" => { "Delete to top"
                "g" => [collapse_selection, extend_to_file_start, extend_to_line_bounds, delete_selection_linewise], // dgg
                "n" => [select_gn_match, delete_selection],      // dgn: delete the match at/after cursor
                "N" => [select_gn_match_prev, delete_selection], // dgN
            },
            // vim forced charwise (`dvj`/`dvk`): force a normally-linewise vertical
            // motion to delete charwise (from the cursor to the same column).
            "v" => { "Force charwise"
                "j" => [extend_line_down, delete_selection],
                "k" => [extend_line_up, delete_selection],
            },
            "V" => { "Force linewise"
                "}" => [collapse_selection, select_paragraph_forward_vim_linewise, delete_selection],
                "{" => [collapse_selection, select_paragraph_backward_vim_linewise, delete_selection],
            },
            "%" => [match_brackets, delete_selection],
            "i" => delete_textobject_inner,   // diw, di(, dip, ...
            "a" => delete_textobject_around,  // daw, da(, ...
            // vim `{motion}` takes a mark: d`a to the mark, d'a whole lines.
            "`" => delete_to_mark,            // d`{mark}
            "'" => delete_to_mark_line,       // d'{mark}
            "f" => delete_find_char_forward,  // df<c>
            "t" => delete_till_char_forward,  // dt<c>
            "F" => delete_find_char_backward, // dF<c>
            "T" => delete_till_char_backward, // dT<c>
        },

        // --- operator-pending: change --------------------------------------
        "c" => { "change"
            "c" => vim_change_line,   // cc: change count lines, keep indent
            // cj/ck: linewise change of the current line + count lines below/above.
            "j" => [collapse_selection, extend_line_below_linewise, change_selection],
            "k" => [collapse_selection, extend_line_above_linewise, change_selection],
            // cw/cW act like ce/cE in vim (change stops at word end, not next word start).
            "w" => [collapse_selection, subword_extend_e, change_selection],
            "W" => [collapse_selection, extend_next_long_word_end, change_selection],
            "e" => [collapse_selection, subword_extend_e, change_selection],
            "E" => [collapse_selection, extend_next_long_word_end, change_selection],
            "b" => [collapse_selection, subword_extend_b, change_selection],
            "B" => [collapse_selection, extend_prev_long_word_start, change_selection],
            "h" => [extend_chars_left_vim, change_selection],   // ch
            "l" => [extend_chars_right_vim, change_selection],  // cl (like s)
            "space" => [extend_chars_right_vim, change_selection], // c<space>
            "$" => [collapse_selection, extend_to_line_end, change_selection],
            "0" => [collapse_selection, extend_to_line_start, change_selection],
            "^" => [collapse_selection, extend_to_first_nonwhitespace, change_selection],
            "}" => [collapse_selection, select_paragraph_forward_vim, change_selection], // c}
            "{" => [collapse_selection, select_paragraph_backward_vim, change_selection], // c{
            // cG: linewise change from the current line to the last line.
            // extend_to_last_line first, then snap the multi-line span to full
            // line bounds so change_selection removes whole lines (mirrors dG).
            "G" => [collapse_selection, extend_to_last_line, extend_to_line_bounds, change_selection],
            "g" => { "Change to top"
                "g" => [collapse_selection, extend_to_file_start, extend_to_line_bounds, change_selection], // cgg
                "n" => [select_gn_match, change_selection],      // cgn: change the match, repeat with `.`
                "N" => [select_gn_match_prev, change_selection], // cgN
            },
            "v" => { "Force charwise"
                "j" => [extend_line_down, change_selection], // cvj
                "k" => [extend_line_up, change_selection],   // cvk
            },
            "V" => { "Force linewise"
                "}" => [collapse_selection, select_paragraph_forward_vim_linewise, change_selection],
                "{" => [collapse_selection, select_paragraph_backward_vim_linewise, change_selection],
            },
            "%" => [match_brackets, change_selection],  // c% change to matching bracket
            "i" => change_textobject_inner,   // ciw, ci(, cip, ...
            "a" => change_textobject_around,  // caw, ca(, ...
            "`" => change_to_mark,            // c`{mark}
            "'" => change_to_mark_line,       // c'{mark}
            "f" => change_find_char_forward,  // cf<c>
            "t" => change_till_char_forward,  // ct<c>
            "F" => change_find_char_backward, // cF<c>
            "T" => change_till_char_backward, // cT<c>
        },

        // --- operator-pending: yank ----------------------------------------
        "y" => { "yank"
            "y" => [collapse_selection, extend_to_line_bounds, yank, collapse_selection],
            // yj/yk: linewise yank of the current line + count lines below/above.
            "j" => [collapse_selection, extend_line_below_linewise, yank, collapse_selection],
            "k" => [collapse_selection, extend_line_above_linewise, yank, collapse_selection],
            "w" => [collapse_selection, subword_extend_w, yank, collapse_selection],
            "W" => [collapse_selection, extend_next_long_word_start, yank, collapse_selection],
            "e" => [collapse_selection, subword_extend_e, yank, collapse_selection],
            "E" => [collapse_selection, extend_next_long_word_end, yank, collapse_selection],
            "b" => [collapse_selection, subword_extend_b, yank, collapse_selection],
            "B" => [collapse_selection, extend_prev_long_word_start, yank, collapse_selection],
            "h" => [extend_chars_left_vim, yank, collapse_selection],   // yh
            "l" => [extend_chars_right_vim, yank, collapse_selection],  // yl
            "space" => [extend_chars_right_vim, yank, collapse_selection], // y<space>
            "$" => [collapse_selection, extend_to_line_end, yank, collapse_selection],
            "0" => [collapse_selection, extend_to_line_start, yank, collapse_selection],
            "^" => [collapse_selection, extend_to_first_nonwhitespace, yank, collapse_selection],
            "}" => [collapse_selection, select_paragraph_forward_vim, yank, collapse_selection], // y}
            "{" => [collapse_selection, select_paragraph_backward_vim, yank, collapse_selection], // y{
            "G" => [collapse_selection, extend_to_last_line, extend_to_line_bounds, yank, collapse_selection],
            "g" => { "Yank to top"
                "g" => [collapse_selection, extend_to_file_start, extend_to_line_bounds, yank, collapse_selection], // ygg
                "n" => [select_gn_match, yank],      // ygn: yank the match at/after cursor
                "N" => [select_gn_match_prev, yank], // ygN
            },
            "v" => { "Force charwise"
                "j" => [extend_line_down, yank, collapse_selection], // yvj
                "k" => [extend_line_up, yank, collapse_selection],   // yvk
            },
            "V" => { "Force linewise"
                "}" => [collapse_selection, select_paragraph_forward_vim_linewise, yank, collapse_selection],
                "{" => [collapse_selection, select_paragraph_backward_vim_linewise, yank, collapse_selection],
            },
            "%" => [match_brackets, yank, collapse_selection],  // y% matching bracket
            "i" => yank_textobject_inner,     // yiw, yi(, yip, ...
            "a" => yank_textobject_around,    // yaw, ya(, ...
            "`" => yank_to_mark,              // y`{mark}
            "'" => yank_to_mark_line,         // y'{mark}
            "f" => yank_find_char_forward,    // yf<c>
            "t" => yank_till_char_forward,    // yt<c>
            "F" => yank_find_char_backward,   // yF<c>
            "T" => yank_till_char_backward,   // yT<c>
        },

        // --- indent operators (vim >>, <<, >{motion}, <{motion}) -----------
        ">" => { "Indent"
            ">" => [indent, goto_first_nonwhitespace],       // >> indent, cursor to first non-blank (vim)
            "j" => [extend_to_line_bounds, extend_line_below, indent, flip_selections, collapse_selection, goto_first_nonwhitespace],
            "k" => [extend_to_line_bounds, extend_line_up, indent, flip_selections, collapse_selection, goto_first_nonwhitespace],
            "G" => [extend_to_last_line, indent, collapse_selection],
            "g" => { "Indent to top"
                "g" => [extend_to_file_start, indent, collapse_selection],
            },
        },
        "<" => { "Unindent"
            "<" => [unindent, goto_first_nonwhitespace],     // << unindent, cursor to first non-blank (vim)
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
        // V: vim visual-LINE. Enters Select with a fixed anchor line; motions
        // grow a whole-line span (via line_reproject) so both boundary lines
        // stay fully selected. See `visual_line_mode`.
        "V" => visual_line_mode,
        // C-v: true vim visual-block. Enters Select with a rectangular-block
        // anchor; motions grow the rectangle (one range per row), I/A
        // block-insert/append at the left/right edge, o/O switch corners, $
        // gives ragged-right. See `visual_block_mode` / `block_reproject`.
        "C-v" => visual_block_mode,

        // --- g submap -------------------------------------------------------
        "g" => { "Goto"
            // g CTRL-A / g CTRL-X: increment / decrement. Also mirrored on the
            // top-level C-a/C-x, but kept here so decrement stays reachable in
            // the spacemacs preset (where top-level C-x becomes the emacs prefix).
            "C-a" => increment,
            "C-x" => decrement,
            // undo-tree time-travel: g- older text state, g+ newer (chronological).
            "-" => earlier,
            "+" => later,
            // g@: call 'operatorfunc' on the selection. vim applies it to the
            // region a following motion covers; zmax already has that region.
            "@" => operator_func,
            // case-change operators (gU / gu / g~ + motion)
            "U" => { "Uppercase"
                "U" => [extend_to_line_bounds, switch_to_uppercase, collapse_selection],
                "w" => [collapse_selection, subword_extend_w, switch_to_uppercase, collapse_selection],
                "W" => [collapse_selection, extend_next_long_word_start, switch_to_uppercase, collapse_selection],
                "e" => [collapse_selection, subword_extend_e, switch_to_uppercase, collapse_selection],
                "b" => [collapse_selection, subword_extend_b, switch_to_uppercase, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, switch_to_uppercase, collapse_selection],
                "^" => [collapse_selection, extend_to_first_nonwhitespace, switch_to_uppercase, collapse_selection],
                // vim `gU{motion}` takes any motion, text objects included.
                "i" => uppercase_textobject_inner,   // gUiw, gUi(, gUip, ...
                "a" => uppercase_textobject_around,  // gUaw, gUa(, ...
            },
            "u" => { "Lowercase"
                "u" => [extend_to_line_bounds, switch_to_lowercase, collapse_selection],
                "w" => [collapse_selection, subword_extend_w, switch_to_lowercase, collapse_selection],
                "W" => [collapse_selection, extend_next_long_word_start, switch_to_lowercase, collapse_selection],
                "e" => [collapse_selection, subword_extend_e, switch_to_lowercase, collapse_selection],
                "b" => [collapse_selection, subword_extend_b, switch_to_lowercase, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, switch_to_lowercase, collapse_selection],
                "^" => [collapse_selection, extend_to_first_nonwhitespace, switch_to_lowercase, collapse_selection],
                "i" => lowercase_textobject_inner,   // guiw, gui(, ...
                "a" => lowercase_textobject_around,  // guaw, gua(, ...
            },
            "~" => { "Toggle case"
                "~" => [extend_to_line_bounds, switch_case, collapse_selection],
                "w" => [collapse_selection, subword_extend_w, switch_case, collapse_selection],
                "W" => [collapse_selection, extend_next_long_word_start, switch_case, collapse_selection],
                "e" => [collapse_selection, subword_extend_e, switch_case, collapse_selection],
                "b" => [collapse_selection, subword_extend_b, switch_case, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, switch_case, collapse_selection],
                "^" => [collapse_selection, extend_to_first_nonwhitespace, switch_case, collapse_selection],
                "i" => togglecase_textobject_inner,  // g~iw, g~i(, ...
                "a" => togglecase_textobject_around, // g~aw, g~a(, ...
            },
            // g?{motion} / g?? / g?g?: ROT13-encode text (vim operator).
            "?" => { "Rot13"
                "?" => [extend_to_line_bounds, rot13, collapse_selection],          // g?? current line
                "j" => [extend_to_line_bounds, extend_line_below, rot13, flip_selections, collapse_selection, goto_first_nonwhitespace],
                "k" => [extend_to_line_bounds, extend_line_up, rot13, flip_selections, collapse_selection, goto_first_nonwhitespace],
                "w" => [collapse_selection, subword_extend_w, rot13, collapse_selection],
                "e" => [collapse_selection, subword_extend_e, rot13, collapse_selection],
                "b" => [collapse_selection, subword_extend_b, rot13, collapse_selection],
                "$" => [collapse_selection, extend_to_line_end, rot13, collapse_selection],
                "G" => [extend_to_last_line, rot13, collapse_selection],
                "g" => { "Rot13 line"
                    "?" => [extend_to_line_bounds, rot13, collapse_selection],      // g?g? current line
                },
            },
            // gq{motion} / gw{motion}: reflow text to 'text-width' (vim gq/gw).
            // gq leaves the cursor at the end of the reflowed text; gw restores it
            // to the start.
            // gq is always linewise, so every motion snaps to line bounds first.
            "q" => { "Reflow"
                "q" => [extend_to_line_bounds, reflow_selections, collapse_selection],
                "j" => [extend_line_below, extend_to_line_bounds, reflow_selections, collapse_selection],
                "k" => [extend_line_up, extend_to_line_bounds, reflow_selections, collapse_selection],
                "G" => [extend_to_last_line, extend_to_line_bounds, reflow_selections, collapse_selection],
                "}" => [extend_to_line_bounds, extend_next_paragraph, reflow_selections, collapse_selection],
                "{" => [extend_to_line_bounds, extend_prev_paragraph, reflow_selections, collapse_selection],
                "g" => { "Reflow to top"
                    "q" => [extend_to_line_bounds, reflow_selections, collapse_selection], // gqgq = gqq
                    "g" => [extend_to_file_start, extend_to_line_bounds, reflow_selections, collapse_selection],
                },
            },
            "w" => { "Reflow"
                "w" => [extend_to_line_bounds, reflow_selections_keep_cursor],
                "j" => [extend_line_below, extend_to_line_bounds, reflow_selections_keep_cursor],
                "k" => [extend_line_up, extend_to_line_bounds, reflow_selections_keep_cursor],
                "G" => [extend_to_last_line, extend_to_line_bounds, reflow_selections_keep_cursor],
                "}" => [extend_to_line_bounds, extend_next_paragraph, reflow_selections_keep_cursor],
                "{" => [extend_to_line_bounds, extend_prev_paragraph, reflow_selections_keep_cursor],
                "g" => { "Reflow to top"
                    "w" => [extend_to_line_bounds, reflow_selections_keep_cursor], // gwgw = gww
                    "g" => [extend_to_file_start, extend_to_line_bounds, reflow_selections_keep_cursor],
                },
            },

            "g" => goto_file_start,
            "&" => repeat_substitute_global,   // g& repeat last :s whole file
            ";" => goto_older_change,          // g; walk to an older change-list position
            "E" => vim_move_prev_long_word_end, // gE back to end of previous WORD
            "e" => vim_move_prev_word_end,      // ge back to end of previous word
            "j" => move_visual_line_down,      // gj: down one *display* line
            "k" => move_visual_line_up,        // gk: up one display line
            "h" => select_mode,                // gh: start Select mode (vim); g0/g^ cover line start
            "l" => goto_line_end,
            // The soft-wrap-aware commands, which is what these mean: `g0`/`g$` are
            // the ends of the *screen* line (`:h g0`). They were bound to the
            // text-line commands, so on a wrapped line `g$` was just `$`.
            "0" => goto_visual_line_start,     // g0 leftmost (screen line)
            "$" => goto_visual_line_end,       // g$ rightmost (screen line)
            "^" => goto_visual_first_nonwhitespace, // g^ first non-blank (screen line)
            "_" => goto_line_last_nonblank,    // g_ last non-blank char of line
            "M" => goto_line_middle,           // gM middle of the text line
            "o" => goto_byte,                  // go to byte {count} in buffer
            "O" => symbol_picker,              // gO: document symbols (nvim 0.11 default)
            // gc: comment operator (nvim built-in). gcc toggles the current line;
            // gc{motion} comments the motion's lines. Textobject forms (gcip) need
            // a comment-textobject command that doesn't exist yet, so are omitted.
            "c" => { "comment"
                "c" => toggle_line_comments,   // gcc: toggle the current line
                "j" => [collapse_selection, extend_line_below_linewise, toggle_comments, normal_mode], // gcj
                "k" => [collapse_selection, extend_line_above_linewise, toggle_comments, normal_mode], // gck
                "}" => [collapse_selection, select_paragraph_forward_vim, toggle_comments, normal_mode], // gc}
                "{" => [collapse_selection, select_paragraph_backward_vim, toggle_comments, normal_mode], // gc{
                "G" => [collapse_selection, extend_to_last_line, extend_to_line_bounds, toggle_comments, normal_mode], // gcG
            },
            "I" => insert_at_line_start,       // gI insert at column 1
            "J" => join_lines_vim_nospace,     // gJ: join lines without a space
            "d" => goto_definition,
            "D" => goto_declaration,
            "y" => goto_type_definition,
            "r" => goto_reference,
            "i" => insert_at_last_insert,      // gi insert at last insert position
            "R" => virtual_replace_mode,       // gR: Virtual Replace mode
            "v" => reselect_visual,            // gv reselect last visual area
            "V" => no_op,                      // gV: don't reselect the previous Visual area (no-op)
            "s" => vim_sleep,                  // gs: sleep for {count} seconds
            "<" => view_echo_area_messages,    // g<: display previous command output (echo-area log)
            "f" => goto_file,
            "x" => goto_file,                 // gx: open file/URL under cursor (goto_file opens URLs externally)
            // ga (print char ascii/unicode value) is bound via VIM_TYPABLE to
            // :character-info — vim's ga, not zmax's goto-last-accessed-file.
            "m" => goto_line_middle,          // gm: go to middle of the screen line (vim, not last-modified)
            "C-g" => document_stats,          // g CTRL-G: line/word/char counts (+ selection)
            "t" => goto_next_tabpage,          // gt: next tabpage
            "T" => goto_previous_tabpage,      // gT: previous tabpage
            "p" => paste_after_cursor_after,   // gp: like p, but cursor rests after the pasted text
            "P" => paste_before_cursor_after,  // gP: like P, but cursor rests after the pasted text
            "n" => [select_gn_match, select_mode],      // gn: visually select the match at/after cursor
            "N" => [select_gn_match_prev, select_mode], // gN: visually select the previous match
            "." => goto_last_modification,
            "'" => goto_mark_line_nojump,      // g'{mark}: like ' but keep jumplist unchanged
            "`" => goto_mark_nojump,           // g`{mark}: like ` but keep jumplist unchanged
            "down" => move_line_down,          // g<Down>: like gj (display line down)
            "up"   => move_line_up,            // g<Up>: like gk (display line up)
            "home" => goto_line_start,         // g<Home>: like g0
            "end"  => goto_line_end,           // g<End>: like g$
            "#" => [search_selection, search_prev], // g#: search word backward (no \<\> bounds)
            "*" => [search_selection, search_next], // g*: search word forward (no \<\> bounds)
            "H" => [extend_to_line_bounds, select_mode], // gH: start linewise Select mode
            "C-h" => visual_block_mode,        // g CTRL-H: start blockwise (visual-block) Select mode
            "]" => goto_definition,            // g]: :tselect tag under cursor
            "C-]" => goto_definition,          // g CTRL-]: :tjump tag under cursor
            "tab" => goto_last_accessed_file,  // g<Tab>: go to last accessed tabpage
            "," => goto_newer_change,          // g,: walk to a newer change-list position
            "Q" => ex_mode,                    // gQ: Ex mode (:visual leaves it)
        },

        // --- z submap (view + folds) ---------------------------------------
        "z" => { "View"
            "z" => align_view_center,
            "t" => align_view_top,
            "b" => align_view_bottom,
            "." => [align_view_center, goto_first_nonwhitespace], // z. center + first non-blank
            "-" => [align_view_bottom, goto_first_nonwhitespace], // z- bottom + first non-blank
            "ret" => [align_view_top, goto_first_nonwhitespace],  // z<CR> top + first non-blank
            // z+ / z^: page by a *whole* screenful — the line below the window goes
            // to the top of it (z+), the line above to the bottom (z^), cursor on
            // that line's first non-blank. A count names the line instead.
            "+" => [scroll_line_below_window, goto_first_nonwhitespace],
            "^" => [scroll_line_above_window, goto_first_nonwhitespace],

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
            "R" => fold_open_all,     // zR open all folds (foldlevel -> deepest)
            "M" => fold_close_all,    // zM close all folds (foldlevel -> 0)
            "d" => fold_delete,       // zd delete fold under cursor
            "D" => fold_delete,       // zD delete folds recursively (approx: at cursor)
            "E" => fold_delete_all,   // zE eliminate all folds
            "A" => fold_toggle,       // zA toggle fold recursively (approx: at cursor)
            "i" => fold_toggle,       // zi toggle foldenable (approx: fold at cursor)
            "m" => fold_more,         // zm fold more (decrease foldlevel by one)
            "r" => fold_less,         // zr fold reduce (increase foldlevel by one)
            "n" => fold_open_all,     // zn foldenable off (show all text)
            "N" => fold_close_all,    // zN set foldenable (close to foldlevel, approx)
            "X" => fold_open_all,     // zX re-apply foldlevel (approx open all)
            "F" => [extend_to_line_bounds, fold_create], // zF create a fold for N lines
            // zp/zP/zy: block paste/yank without the trailing padding a blockwise
            // register carries (see `strip_trailing_whitespace_lines`).
            "p" => paste_after_no_trailing_whitespace,
            "P" => paste_before_no_trailing_whitespace,
            "y" => yank_no_trailing_whitespace,
            "j" => fold_next,         // zj move to next fold
            "k" => fold_prev,         // zk move to previous fold
            // zf{motion}: create a fold over the motion (vim operator)
            "f" => { "Create fold"
                // extend_to_line_bounds first: extend_line_below/up only step to
                // the next line once the current one is *fully* selected (it is
                // helix's `x` — from a bare cursor it selects the current line and
                // stops). Running it first left `zfj` with a one-line range, and
                // Folds::create rejects those, so no fold was ever made.
                "j" => [extend_to_line_bounds, extend_line_below, fold_create],
                "k" => [extend_to_line_bounds, extend_line_up, fold_create],
                "G" => [extend_to_last_line, fold_create],
                "}" => [goto_next_paragraph, fold_create],
                "f" => [extend_to_line_bounds, fold_create],
            },
        },

        // --- bracket submaps (vim unimpaired-ish) --------------------------
        "[" => { "Prev"
            "[" => goto_prev_section,   // [[ back to the previous section ('sections')
            "d" => goto_prev_diag,
            "g" => goto_prev_change,
            "c" => goto_prev_change,      // [c back to start of prev change (diff hunk)
            "x" => goto_prev_conflict,    // [x previous merge-conflict marker
            "n" => goto_prev_conflict,    // [n previous conflict (vim-unimpaired style)
            "f" => goto_file,             // [f same as gf: open file under cursor
            "m" => goto_prev_function,    // [m back to start of member/function
            "b" => goto_previous_buffer,  // [b previous buffer (unimpaired-style)
            "q" => quickfix_prev,         // [q previous quickfix entry (:cprev, unimpaired-style)
            "l" => loclist_prev,          // [l previous location-list entry (:lprev, unimpaired-style)
            "/" => goto_prev_comment,     // [/ previous comment
            "p" => paste_before,          // [p paste before (linewise, adjust indent)
            "P" => paste_before,          // [P same as [p
            "*" => goto_prev_comment,     // [* same as [/ : previous comment
            "]" => goto_prev_function,    // [] N sections backward (member/function)
            "z" => fold_prev,             // [z move to start of open fold
            // word-under-cursor / #define navigation (vim [i [I [D [CTRL-I [CTRL-D).
            // `[` scans from the top of the file; the lowercase form only *shows*
            // the line, the uppercase form lists them all, CTRL jumps. The current
            // buffer is scanned (vim also walks the files it includes).
            // ([d/]d stay on the diagnostics, which own them here.)
            "i"   => show_keyword_line_from_start,  // [i: show the first line with the word
            "I"   => list_keyword_lines_from_start, // [I: list every line with the word
            "D"   => list_defines_from_start,       // [D: list every #define of the word
            "C-i" => goto_keyword_line_from_start,  // [CTRL-I: jump to the first such line
            "C-d" => goto_define_from_start,        // [CTRL-D: jump to the first #define
            "s" => goto_prev_spell_error,     // [s: previous misspelled word
            "w" => rotate_view_reverse,       // [w: go to the previous window (spacemacs)
            // [t: spacemacs's "go to the previous frame". zmax has no frames, so
            // it cycles windows — the same substitution `C-x 5 o` (other-frame)
            // makes, which lands it on the same command as [w.
            "t" => rotate_view_reverse,       // [t: previous frame (= previous window)
            "(" => goto_prev_unmatched_paren, // [( previous unmatched (
            "{" => goto_prev_unmatched_brace, // [{ previous unmatched {
            "#" => goto_prev_preproc,         // [# previous unmatched #if/#else
            "`" => goto_prev_mark,            // [` previous lowercase mark
            "'" => goto_prev_mark_line,       // ['  previous lowercase mark (line)
        },
        "]" => { "Next"
            "]" => goto_next_section,   // ]] forward to the next section ('sections')
            "d" => goto_next_diag,
            "g" => goto_next_change,
            "c" => goto_next_change,      // ]c forward to start of next change (diff hunk)
            "x" => goto_next_conflict,    // ]x next merge-conflict marker
            "n" => goto_next_conflict,    // ]n next conflict (vim-unimpaired style)
            "f" => goto_file,             // ]f same as gf: open file under cursor
            "m" => goto_next_function,    // ]m forward to next member/function
            "b" => goto_next_buffer,      // ]b next buffer (unimpaired-style)
            "q" => quickfix_next,         // ]q next quickfix entry (:cnext, unimpaired-style)
            "l" => loclist_next,          // ]l next location-list entry (:lnext, unimpaired-style)
            "/" => goto_next_comment,     // ]/ next comment
            "p" => paste_after,           // ]p paste after (linewise, adjust indent)
            "P" => paste_before,          // ]P same as [p
            "*" => goto_next_comment,     // ]* same as ]/ : next comment
            "[" => goto_next_function,    // ][ N sections forward (member/function)
            "z" => fold_next,             // ]z move to end of open fold
            // word-under-cursor / #define navigation (vim ]i ]I ]D ]CTRL-I ]CTRL-D):
            // the same four commands as `[`, scanning from below the cursor instead
            // of from the top of the file.
            "i"   => show_keyword_line_from_cursor,  // ]i: show the next line with the word
            "I"   => list_keyword_lines_from_cursor, // ]I: list the lines below with the word
            "D"   => list_defines_from_cursor,       // ]D: list the #defines below
            "C-i" => goto_keyword_line_from_cursor,  // ]CTRL-I: jump to the next such line
            "C-d" => goto_define_from_cursor,        // ]CTRL-D: jump to the next #define
            "s" => goto_next_spell_error,     // ]s: next misspelled word
            "w" => rotate_view,               // ]w: go to the next window (spacemacs)
            // ]t: go to the next frame. zmax has real frames now, so this is
            // `other_frame` (it wraps, exactly like emacs's) rather than the window
            // rotation it had to stand on. `[t` (the previous frame) has no port yet:
            // `other_frame` only ever steps forward.
            "t" => other_frame,               // ]t: next frame
            ")" => goto_next_unmatched_paren, // ]) next unmatched )
            "}" => goto_next_unmatched_brace, // ]} next unmatched }
            "#" => goto_next_preproc,         // ]# next unmatched #endif/#else
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
            // vim `C-w d` splits and jumps to the definition under the cursor;
            // spacemacs `SPC w d` deletes the window. The two disagree, so each
            // side keeps its own meaning (see VIM_OWNS in keymap.rs). `C-w C-d`
            // stays close-window, as vim has no C-w C-d.
            "d" => xref_find_definitions_other_window,
            "C-d" => wclose,
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
            "C-f" => goto_file_hsplit,        // C-w C-f: split + edit file under cursor
            "f" => toggle_follow_mode,        // C-w f / SPC w f: toggle follow mode (spacemacs)
            "F" => goto_file_hsplit,          // C-w F: split + edit file (with line number)
            "]" | "C-]" => goto_definition,   // C-w ] / C-w C-]: jump to tag/definition (no split)
            "^" | "C-^" => goto_last_accessed_file, // C-w ^ / C-w C-^: edit alternate file
            "i" | "C-i" => goto_declaration,  // C-w i / C-w C-i: split + jump to declaration (no split)
            "p" | "C-p" => rotate_view,       // C-w p: go to previous (last accessed) window
            "z" => recenter_other_window,     // C-w z: recenter point in the other window (emacs recenter-other-window)
            "C-z" => recenter_other_window,   // C-w C-z: same as C-w z
            "C-c" => no_op,                   // C-w C-c: no-op (vim)
            "P" => goto_preview_window,       // C-w P: go to the preview window
            "C-t" => jump_view_up,            // C-w C-t: go to top window
            "t" => toggle_window_dedication,  // C-w t / SPC w t: toggle window dedication (spacemacs)
            "T" => window_to_new_tab,         // C-w T: move current window to a new tabpage (vim)
            "b" | "C-b" => jump_view_down,    // C-w b: go to bottom window
            "W" => rotate_view_reverse,       // C-w W: go to previous window (wrap)
            "u" => winner_undo,               // SPC w u : winner-undo (undo window layout)
            "}" => preview_tag,               // C-w }: :ptag the tag under the cursor
            // CTRL-W g ...: tab/file/tag variants (vim's window-goto sub-prefix)
            "g" => { "Window goto"
                "t" => goto_next_tabpage,     // C-w g t: next tabpage
                "T" => goto_previous_tabpage, // C-w g T: prev tabpage
                "f" => goto_file,             // C-w g f: edit file under cursor (new tab approx)
                "F" => goto_file,             // C-w g F: edit file under cursor (new tab approx)
                "]" | "C-]" => goto_definition, // C-w g ] / g C-]: tag jump (:tselect/:tjump)
                "}" => preview_tjump,         // C-w g }: :ptjump the tag under the cursor
                "tab" => goto_last_accessed_file, // C-w g <Tab>: last accessed tab -> alt file
            },
            "n" | "C-n" => hsplit_new,        // C-w n: open new window
            "/" => vsplit,                    // spacemacs SPC w / : split vertically
            // vim window height resize (horizontal split stays on s / C-s)
            "+" => resize_view_taller,        // C-w +: increase window height N lines
            "-" => resize_view_shorter,       // C-w -: decrease window height N lines
            "[" => resize_view_narrower,      // C-w [ : shrink window horizontally (matches SPC w)
            "{" => resize_view_shorter,       // C-w { : shrink window vertically (matches SPC w)
            "=" => resize_view_equalize,      // C-w =: make all windows equal size
            "c" => wclose,                    // spacemacs SPC w c : close window
            "m" => wonly,                     // spacemacs SPC w m : maximize (only)
            "S" => hsplit,                    // spacemacs SPC w S / vim C-w S : split & focus
            "V" => vsplit,                    // spacemacs SPC w V : vsplit & focus
            "|" => wonly,                     // spacemacs SPC w | : maximize window (only)
            "1" => wonly,                     // SPC w 1 : single-window layout
            "2" => vsplit,                    // SPC w 2 : two-window layout (split)
            "3" => make_3_windows,            // SPC w 3 : three-window layout
            "4" => make_4_windows,            // SPC w 4 : 2x2 window grid
            "_" | "C-_" => wonly,             // SPC w _ / vim C-w _ / C-w C-_: maximize window horizontally
            "D" => wclose,                    // SPC w D : delete another window
            "M" => transpose_view,            // SPC w M : swap windows
            // `C-w .` is the same window transient state as `SPC w .` — the one
            // definition lives under the leader and `add_transient_states` grafts
            // it here, so the two can no longer drift apart.
        },

        // --- scrolling / jumps ---------------------------------------------
        "C-d" => page_cursor_half_down,
        "C-u" => page_cursor_half_up,
        "C-f" => page_down,
        "pagedown" => page_down_key,
        "C-b" => page_up,
        "pageup" => page_up_key,
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
        // The shifted keys answer to 'keymodel': with `startsel` they select,
        // otherwise they keep vim's default meaning (word move / page).
        "S-left"  => shift_left_key,
        "C-right" => move_next_word_start,  // <C-Right>/<S-Right> = w
        "S-right" => shift_right_key,
        "C-home"  => goto_file_start,    // <C-Home> = gg
        "C-end"   => goto_last_line,     // <C-End> = G
        "S-down"  => shift_down_key,     // <S-Down> = CTRL-F
        "S-up"    => shift_up_key,       // <S-Up> = CTRL-B
        "ins"     => insert_mode,        // <Insert> = i
        "C-]"     => tag_jump,           // CTRL-] = :ta (jump to tag, pushing the tag stack)
        "C-^"     => goto_last_accessed_file, // CTRL-^ = edit alternate file
        "S-ret"   => page_down,          // <S-CR> = CTRL-F (page down)
        "S-+"     => page_down,          // <S-+> = CTRL-F (page down)
        "S-minus" => page_up,            // <S--> = CTRL-B (page up)
        "U"       => undo_line,          // U: undo all latest changes on one line
        "F1"      => help,               // <F1> = <Help>: open the Help browser
        "C-t"     => tag_pop,            // CTRL-T = pop the tag stack (:pop)
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
        // M-q is fill-paragraph — reflow to the fill column, which is what
        // `reflow_selections` does (the emacs preset binds the same command).
        // `format_selections` is the LSP formatter, a different command entirely.
        "A-q"     => reflow_selections,   // M-q fill-paragraph
        "A-^"     => join_selections,     // M-^ join to previous line (approx)

        // --- emacs global map: the rest of the Meta keys --------------------
        // Sentences / paragraphs / words / case (Emacs Key Index). Each of these
        // runs the faithful port named in the command's own doc string.
        "A-a"     => move_sentence_backward, // M-a backward-sentence
        "A-e"     => move_sentence_forward,  // M-e forward-sentence
        "A-k"     => kill_sentence,          // M-k kill-sentence
        "A-h"     => mark_paragraph,         // M-h mark-paragraph
        "A-{"     => goto_prev_paragraph,    // M-{ backward-paragraph
        "A-}"     => goto_next_paragraph,    // M-} forward-paragraph
        "A-c"     => capitalize_word,        // M-c capitalize-word
        "A-l"     => downcase_word,          // M-l downcase-word
        "A-u"     => upcase_word,            // M-u upcase-word
        "A-@"     => mark_word,              // M-@ mark-word
        "A-y"     => yank_pop,               // M-y yank-pop
        "A-z"     => zap_to_char,            // M-z zap-to-char
        "A-="     => count_selection,        // M-= count-words-region
        "A-'"     => expand_abbrev,          // M-' expand-abbrev
        "A-$"     => ispell_word,            // M-$ ispell-word
        "A-~"     => not_modified,           // M-~ not-modified
        "A-."     => goto_definition,        // M-. xref-find-definitions
        "A-?"     => xref_find_references,   // M-? xref-find-references
        "A-X"     => command_palette,        // M-X / M-S-x execute-extended-command-for-buffer
        "A-tab"   => completion,             // M-TAB complete-symbol
        "A-left"  => move_prev_word_start,   // M-<left> left-word
        "A-right" => move_next_word_start,   // M-<right> right-word
        "A-!"     => shell_insert_output,    // M-! shell-command (output is inserted here)
        "A-|"     => shell_pipe,             // M-| shell-command-on-region
        // M-r move-to-window-line-top-bottom: centre, then top, then bottom on
        // successive presses. `goto_window_center` (the old binding) only ever
        // centred — the cycling port exists now, so the chord runs it.
        "A-r"     => move_to_window_line_top_bottom, // M-r move-to-window-line-top-bottom
        "C-@"     => set_mark_command,       // C-@ set-mark-command (= C-SPC)
        "C-S-tab" => goto_previous_tabpage,  // S-C-TAB tab-bar-switch-to-prev-tab
        "F3"      => kmacro_start_macro_or_insert_counter, // F3 kmacro-start-macro-or-insert-counter

        // C-M- map: s-expressions, defuns, lists, the other window (Emacs binds
        // the structural-editing commands here).
        "A-C-f"     => forward_sexp,             // C-M-f forward-sexp
        "A-C-b"     => backward_sexp,            // C-M-b backward-sexp
        "A-C-k"     => kill_sexp,                // C-M-k kill-sexp
        "A-C-t"     => transpose_sexp,           // C-M-t transpose-sexps
        "A-C-space" => mark_sexp,                // C-M-SPC mark-sexp
        "A-C-@"     => mark_sexp,                // C-M-@ mark-sexp
        "A-C-h"     => mark_defun,               // C-M-h mark-defun
        "A-C-a"     => c_beginning_of_defun,     // C-M-a beginning-of-defun
        "A-C-e"     => c_end_of_defun,           // C-M-e end-of-defun
        "A-C-d"     => down_list,                // C-M-d down-list
        "A-C-u"     => backward_up_list,         // C-M-u backward-up-list
        "A-C-n"     => forward_list,             // C-M-n forward-list
        "A-C-p"     => backward_list,            // C-M-p backward-list
        "A-C-v"     => scroll_other_window,      // C-M-v scroll-other-window
        "A-C-S-v"   => scroll_other_window_down, // C-M-S-v scroll-other-window-down
        "A-C-S-l"   => recenter_other_window,    // C-M-S-l recenter-other-window
        "A-C-q"     => prog_indent_sexp,         // C-M-q indent-sexp / indent-pp-sexp
        "A-C-\\"    => indent,                   // C-M-\ indent-region
        "A-C-j"     => default_indent_new_line,  // C-M-j default-indent-new-line
        "A-C-i"     => completion,               // C-M-i completion-at-point
        "A-C-/"     => dabbrev_completion,       // C-M-/ dabbrev-completion
        "A-C-."     => workspace_symbol_picker,  // C-M-. xref-find-apropos
        "A-C-s"     => search,                   // C-M-s isearch-forward-regexp
        "A-C-r"     => rsearch,                  // C-M-r isearch-backward-regexp
        "A-C-w"     => append_next_kill,         // C-M-w append-next-kill

        // M-g: the emacs goto map.
        "A-g" => { "Goto (M-g)"
            "g"   => goto_line,        // M-g g   goto-line
            "A-g" => goto_line,        // M-g M-g goto-line
            "c"   => goto_char,        // M-g c   goto-char
            "tab" => goto_column,      // M-g TAB move-to-column
            "n"   => run_next_error,   // M-g n   next-error
            "A-n" => run_next_error,   // M-g M-n next-error
            "p"   => run_prev_error,   // M-g p   previous-error
            "A-p" => run_prev_error,   // M-g M-p previous-error
        },

        // M-- : emacs negative argument. The only negative-arg keys with a
        // distinct action are the word-case commands, which operate on the
        // *previous* word instead of the next (M-- M-u / M-l / M-c). Both the
        // Alt-letter and bare-letter continuations are accepted.
        "A-minus" => { "Negative arg (M--)"
            "A-u" => upcase_prev_word,      // M-- M-u  upcase previous word
            "u"   => upcase_prev_word,
            "A-l" => downcase_prev_word,    // M-- M-l  downcase previous word
            "l"   => downcase_prev_word,
            "A-c" => capitalize_prev_word,  // M-- M-c  capitalize previous word
            "c"   => capitalize_prev_word,
        },

        // M-s: the emacs search map (occur / word / symbol isearch). The `M-s h`
        // hi-lock chords are typables, grafted on in EMACS_TYPABLE.
        "A-s" => { "Search (M-s)"
            "o"   => occur,                            // M-s o   occur
            "w"   => isearch_forward_word,             // M-s w   isearch-forward-word
            "A-w" => isearch_forward_word,             // M-s M-w word-search-forward
            "."   => isearch_forward_symbol_at_point,  // M-s .   isearch-forward-symbol-at-point
            "A-." => isearch_forward_symbol_at_point,  // M-s M-. isearch-forward-symbol-at-point
            "_"   => isearch_forward_symbol,           // M-s _   isearch-forward-symbol
        },

        // F2: the emacs two-column map (same commands as C-x 6).
        "F2" => { "Two-column"
            "1"   => twocol_merge,            // F2 1   2C-merge
            "2"   => twocol_two_columns,      // F2 2   2C-two-columns
            "b"   => twocol_associate_buffer, // F2 b   2C-associate-buffer
            "d"   => twocol_dissociate,       // F2 d   2C-dissociate
            "s"   => twocol_split,            // F2 s   2C-split
            "ret" => twocol_newline,          // F2 RET 2C-newline
        },

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
            "r" => indent_code_rigidly,                 // = r : shift region lines by [count] cols, skip string-interior lines (emacs indent-code-rigidly)
            "s" => prog_indent_sexp,                    // = s : reindent the s-expression after point, or the enclosing defun with a count (emacs prog-indent-sexp, C-M-q)
        },

        // --- increment / decrement -----------------------------------------
        "C-a" => increment,
        "C-x" => decrement,

        // --- misc -----------------------------------------------------------
        // `{count}:` pre-fills the Ex line with the range the count names (`3:`
        // gives `:.,.+2`); a bare `:` opens it empty.
        ":" => command_mode_count,
        "C-z" => suspend,
        // vim never keeps you in a multi-cursor state: Esc in Normal drops every
        // extra cursor (from visual-block, select-all, etc.) back to a single one.
        "esc" => [keep_primary_selection, collapse_selection],

        // --- leader (space) — kept for pickers / LSP / commands ------------
        // --- leader (space): spacemacs SPC tree ----------------------------
        // Structured to mirror spacemacs' SPC keybinding tree. Only bindings
        // that map to a real zmax static command are present; spacemacs
        // bindings needing a typable (`:w` save, `:q` quit, `:bd`) are not yet
        // expressible in the keymap macro and remain tracked as absent.
        "," => repeat_find_char_reverse,   // vim , : repeat last f/t/F/T reversed
        // Run / Debug function keys (IDE-style)
        "F4" => kmacro_end_or_call_macro,      // F4      : end macro if recording, else call last macro (emacs kmacro-end-or-call-macro)
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
                "a" => ai_agent,                   // SPC a a : autonomous AI agent (reads/writes files, tool-use)
                "R" => ai_agent_review,            // SPC a R : toggle agent review (dry-run) mode
                "tab" => ai_complete,              // SPC a TAB : AI code completion at the cursor
                "z" => ai_revert_agent,            // SPC a z : revert to the agent's pre-run checkpoint
                "i" => ai_chat,                    // SPC a i : ask the AI provider (Cursor-style assistant)
                "p" => ai_chat_panel,              // SPC a p : streaming AI chat drawer (on-the-fly generation)
                "e" => ai_inline_edit,             // SPC a e : AI inline edit/generate
                "E" => ai_inline_edit_preview,     // SPC a E : AI inline edit with diff preview
                "." => ai_accept_edit,             // SPC a . : accept the pending AI edit preview
                "x" => ai_explain,                 // SPC a x : AI explain selection
                "F" => ai_fix,                     // SPC a F : AI fix diagnostic on current line
                "m" => ai_model_picker,            // SPC a m : pick AI model
                "P" => toggle_ai_privacy,          // SPC a P : toggle AI privacy mode
                "y" => ai_apply_block,             // SPC a y : apply last AI code block
                "@" => ai_add_file_context,        // SPC a @ : add @file context for next chat
                "b" => ai_codebase_context,        // SPC a b : @codebase keyword-search context
                "s" => ai_symbol_context,          // SPC a s : @symbol (definition) context
                "D" => ai_docs_context,            // SPC a D : @docs keyword search over docs/ dir
                "w" => ai_web_context,             // SPC a w : @web live web-search context
                "g" => toggle_ai_autocomplete,     // SPC a g : toggle real-time ghost-text autocomplete
                "k" => ai_terminal_command,        // SPC a k : generate a shell command
                "u" => ai_generate_tests,          // SPC a u : AI generate unit tests
                "U" => undo_tree,                  // SPC a U : browse branching undo history (vim undotree)
                "c" => ai_commit_message,          // SPC a c : AI git commit message
                "r" => repl,                       // SPC a r : embedded-language REPL (elisp/viml/stryke/awk/zsh)
                "d" => file_explorer,              // SPC a d : dired (file manager)
                "f" => file_explorer,              // SPC a f : file tree
                "o" => { "Org"
                    "a" => org_agenda,             // SPC a o a : org agenda (Spacemacs org-agenda)
                    "c" => org_capture,            // SPC a o c : org capture (Spacemacs org-capture)
                },
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
                "r" => { "Macro ring"
                    "n" => kmacro_ring_next,       // SPC K r n : cycle to next macro in ring
                    "p" => kmacro_ring_prev,       // SPC K r p : cycle to previous macro in ring
                    "N" => kmacro_ring_prev,       // SPC K r N : cycle to previous macro in ring
                    "d" => kmacro_ring_delete,     // SPC K r d : delete head macro in ring
                    "s" => kmacro_ring_swap,       // SPC K r s : swap first two macros in ring
                    "L" => kmacro_ring_view,       // SPC K r L : view head macro in ring
                },
                "e" => { "Edit macro"
                    "r" => kmacro_to_register,     // SPC K e r : write last macro to a register
                    "n" => kmacro_to_register,     // SPC K e n : name the last macro (to a register)
                },
            },

            "T" => { "Themes"
                "c" => theme_picker,               // SPC T c : fzf theme picker w/ live preview (:Colors)
            },

            "f" => { "Files"
                "f" => file_picker,                            // SPC f f
                "l" => open_file_literally,                    // SPC f l : open file with no syntax (fundamental mode)
                "A" => find_file_replace_buffer,               // SPC f A : open file, replace + close current buffer
                "o" => open_file_external,                     // SPC f o : open with external program
                "F" => goto_file,                              // SPC f F : open file under point
                "h" => open_hex,                               // SPC f h : open binary file in hex editor (hexl)
                "c" => copy_file,                              // SPC f c : copy file to a different location
                "J" => open_junk_file,                         // SPC f J : open a junk file
                "L" => locate_file,                            // SPC f L : locate a file (system locate/mdfind)
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
                "M" => kill_buffers_by_regex,      // SPC b M : kill buffers matching a regexp
                "W" => goto_buffer_window,         // SPC b W : focus the window already showing a chosen buffer
                "N" => { "New buffer"
                    "h" => vsplit_new,             // SPC b N h : new buffer in window left (vertical split)
                    "j" => hsplit_new,             // SPC b N j : new buffer in window below
                    "k" => hsplit_new,             // SPC b N k : new buffer in window above
                    "l" => vsplit_new,             // SPC b N l : new buffer in window right (vertical split)
                    // SPC b N n / i / C-i -> :new via typable table
                },
                "." => { "Buffer transient" sticky=true
                    "n" => goto_next_buffer,       // SPC b . n : next buffer
                    "N" => goto_previous_buffer,   // SPC b . N : previous buffer
                    "p" => goto_previous_buffer,   // SPC b . p : previous buffer
                    "b" => buffer_picker,          // SPC b . b : list buffers
                    "z" => align_view_center,      // SPC b . z : recenter buffer in window
                    "o" => rotate_view,            // SPC b . o : focus other window
                    // move current buffer to window N
                    "1" => buffer_to_window_1, "2" => buffer_to_window_2, "3" => buffer_to_window_3,
                    "4" => buffer_to_window_4, "5" => buffer_to_window_5, "6" => buffer_to_window_6,
                    "7" => buffer_to_window_7, "8" => buffer_to_window_8, "9" => buffer_to_window_9,
                    // switch focus to window N
                    "C-1" => goto_window_1, "C-2" => goto_window_2, "C-3" => goto_window_3,
                    "C-4" => goto_window_4, "C-5" => goto_window_5, "C-6" => goto_window_6,
                    "C-7" => goto_window_7, "C-8" => goto_window_8, "C-9" => goto_window_9,
                    // swap current buffer with window N (Meta = Alt)
                    "A-1" => buffer_swap_window_1, "A-2" => buffer_swap_window_2, "A-3" => buffer_swap_window_3,
                    "A-4" => buffer_swap_window_4, "A-5" => buffer_swap_window_5, "A-6" => buffer_swap_window_6,
                    "A-7" => buffer_swap_window_7, "A-8" => buffer_swap_window_8, "A-9" => buffer_swap_window_9,
                    "q" => exit_transient_state,   // SPC b . q : quit the transient state
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
                "C-f" => goto_file_hsplit,
                "f" => toggle_follow_mode,        // SPC w f : toggle follow mode (spacemacs)
                "F" => goto_file_hsplit,
                "]" | "C-]" => goto_definition,
                "^" | "C-^" => goto_last_accessed_file,
                "i" | "C-i" => goto_declaration,
                "p" | "C-p" => rotate_view,
                "z" => recenter_other_window,     // SPC w z : recenter point in the other window (emacs recenter-other-window)
                "C-z" => recenter_other_window,   // SPC w C-z: same as SPC w z (parity with C-w)
                "C-c" => no_op,                   // SPC w C-c: no-op (parity with C-w)
                "P" => goto_preview_window,       // SPC w P: go to the preview window (parity with C-w)
                "C-t" => jump_view_up,
                "t" => toggle_window_dedication,  // SPC w t : toggle window dedication (spacemacs)
                "b" | "C-b" => jump_view_down,
                "W" => rotate_view_reverse,
                "u" => winner_undo,               // SPC w u : winner-undo (undo window layout)
                "}" => preview_tag,              // parity with C-w }
                "g" => { "Window goto"
                    "t" => goto_next_tabpage,
                    "T" => goto_previous_tabpage,
                    "f" => goto_file,
                    "F" => goto_file,
                    "]" | "C-]" => goto_definition,
                    "}" => preview_tjump,        // parity with C-w g }
                    "tab" => goto_last_accessed_file,
                },
                "n" | "C-n" => hsplit_new,
                "/" => vsplit,
                "+" => resize_view_taller,
                "-" => resize_view_shorter,
                "[" => resize_view_narrower,      // SPC w [ : shrink window horizontally
                "{" => resize_view_shorter,       // SPC w { : shrink window vertically
                "=" => resize_view_equalize,
                // Spacemacs `SPC w c` is the centered-cursor prefix, not
                // close-window (that is `SPC w d` / `SPC w x`). vim's `C-w c` keeps
                // meaning close-window — the two disagree on this key, and the
                // aliased-modes test allows exactly that divergence.
                "c" => { "Centering"
                    "c" => toggle_centered_cursor,   // SPC w c c : centered-cursor mode
                    "." => align_view_center,        // SPC w c . : center this buffer now
                    "C" => toggle_centered_cursor,   // SPC w c C : same, spacemacs alias
                },
                "m" => wonly,
                "S" => hsplit,
                "V" => vsplit,
                "|" => wonly,
                "1" => wonly,
                "2" => vsplit,
                "3" => make_3_windows,
                "4" => make_4_windows,
                "_" => wonly,
                "D" => wclose,
                "M" => transpose_view,
                "." => { "Window transient" sticky=true
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
                    "m" => wonly,                  // SPC w . m : maximize current window
                    "x" => delete_window_and_buffer, // SPC w . x : delete window + kill buffer
                    "a" => ace_window,             // SPC w . a : ace-window (jump to window by number)
                "u" => winner_undo,            // SPC w . u : winner-undo (undo window layout)
                "U" => winner_redo,            // SPC w . U : winner-redo (redo window layout)
                    "g" => golden_ratio_resize,    // SPC w . g : golden-ratio resize
                    // (`-` and `/` are splits in this state, as in spacemacs;
                    // resizing is `[ ] { }` plus these two.)
                    "<" => resize_view_narrower,   // SPC w . < : shrink horizontally
                    ">" => resize_view_wider,      // SPC w . > : enlarge horizontally
                    "=" => resize_view_equalize,   // SPC w . = : balance windows
                    "q" => exit_transient_state,   // SPC w . q : quit the transient state
                    "1" => goto_window_1, "2" => goto_window_2, "3" => goto_window_3,
                    "4" => goto_window_4, "5" => goto_window_5, "6" => goto_window_6,
                    "7" => goto_window_7, "8" => goto_window_8, "9" => goto_window_9,
                },
            },
            "s" => { "Search"
                "s" => global_search,              // SPC s s
                "E" => search_everywhere,          // SPC s E : Search Everywhere (JetBrains double-shift)
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
                "c" => clear_search_highlight,     // SPC s c : clear persistent search highlight
                // uppercase variants are the "with default input" forms: seeded
                // with the symbol under the cursor.
                "D" => global_search_symbol,       // SPC s D : search current directory (default input)
                "B" => global_search_symbol,       // SPC s B : search all open buffers (default input)
                "F" => global_search_symbol,       // SPC s F : search files in a directory (default input)
                "l" => last_picker,                // SPC s l : resume last search
                "L" => buffer_line_picker,         // SPC s L : fuzzy lines in current buffer (:BLines)
                "H" => search_next,                // SPC s H : go to last search occurrence
                // ag / grep / ack / rg families all map to project-wide search;
                // uppercase variants are the "with default input" forms.
                "a" => { "ag"
                    "a" => global_search, "b" => global_search, "d" => global_search,
                    "f" => global_search, "p" => global_search,
                    "A" => global_search_symbol, "B" => global_search_symbol, "D" => global_search_symbol,
                    "F" => global_search_symbol, "P" => global_search_symbol,
                },
                "g" => { "grep"
                    "g" => global_search, "b" => global_search, "f" => global_search,
                    "d" => global_search, "p" => global_search,
                    "G" => global_search_symbol, "B" => global_search_symbol, "F" => global_search_symbol,
                },
                "k" => { "ack"
                    "b" => global_search, "d" => global_search,
                    "f" => global_search, "p" => global_search,
                    "B" => global_search_symbol, "D" => global_search_symbol,
                    "F" => global_search_symbol, "P" => global_search_symbol,
                },
                "r" => { "rg"
                    "r" => global_search, "b" => global_search, "f" => global_search,
                    "d" => global_search, "p" => global_search,
                    "R" => global_search_symbol, "B" => global_search_symbol, "F" => global_search_symbol,
                    "D" => global_search_symbol, "P" => global_search_symbol,
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
                "B" => dap_breakpoints_picker,     // SPC d B : view all breakpoints (JetBrains View Breakpoints)
                "c" => dap_continue,               // SPC d c : continue
                "C" => dap_run_to_cursor,          // SPC d C : run to cursor (JetBrains Run To Cursor)
                "i" => dap_step_in,                // SPC d i : step in
                "o" => dap_step_out,               // SPC d o : step out
                "n" => dap_next,                   // SPC d n : step over
                "p" => dap_pause,                  // SPC d p : pause
                "r" => dap_restart,                // SPC d r : restart session
                "v" => dap_variables,              // SPC d v : list variables
                "q" => dap_terminate,              // SPC d q : end debug session
                "g" => { "GDB data buffers"
                    "l" => gdb_display_locals_buffer,      // SPC d g l : locals buffer
                    "r" => gdb_display_registers_buffer,   // SPC d g r : registers buffer
                    "s" => gdb_display_stack_for_thread,   // SPC d g s : stack for thread
                    "d" => gdb_display_disassembly_buffer, // SPC d g d : disassembly buffer
                    "m" => gdb_display_memory_buffer,      // SPC d g m : memory buffer
                    "i" => gdb_display_io_buffer,          // SPC d g i : inferior IO buffer
                    "e" => gdb_edit_value,                 // SPC d g e : edit value
                    "k" => gdb_delete_breakpoint,          // SPC d g k : delete breakpoint
                    "t" => dap_switch_thread,              // SPC d g t : select thread
                    "f" => dap_switch_stack_frame,         // SPC d g f : select frame
                    "w" => gdb_many_windows,               // SPC d g w : many windows
                },
            },
            "*" => global_search_symbol,           // SPC * : search with default input (symbol at point)
            // Window-by-number (window-numbering-mode): SPC 1..9 jump to window N.
            "1" => goto_window_1,
            "2" => goto_window_2,
            "3" => goto_window_3,
            "4" => goto_window_4,
            "5" => goto_window_5,
            "6" => goto_window_6,
            "7" => goto_window_7,
            "8" => goto_window_8,
            "9" => goto_window_9,
            "S" => settings_page,                  // SPC S : Preferences → Settings tab
            "," => preferences,                    // SPC , : open the unified Preferences window
            // Spacemacs `SPC z` is the zoom prefix; both scaling maps are
            // transient states (sticky), so `+`/`-`/`0` keep repeating.
            "z" => { "Zoom"
                "x" => { "Font scaling" sticky=true
                    "+" | "=" | "k" => text_scale_increase, // SPC z x + / = / k : scale text up
                    "-" | "_" | "j" => text_scale_decrease, // SPC z x - / _ / j : scale text down
                    "0" => text_scale_reset,           // SPC z x 0 : reset text scale
                    "q" => exit_transient_state,       // SPC z x q : leave the transient state
                },
                "f" => { "Frame scaling" sticky=true
                    "+" | "=" | "k" => frame_zoom_in,  // SPC z f + / = / k : zoom frame in
                    "-" | "_" | "j" => frame_zoom_out, // SPC z f - / _ / j : zoom frame out
                    "0" => frame_zoom_reset,           // SPC z f 0 : reset frame zoom
                    "q" => exit_transient_state,       // SPC z f q : leave the transient state
                },
                "z" => toggle_ide,                     // SPC z z : toggle IDE workbench (Zen / focus mode)
            },
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
                "q" => hide_active_tool_window,    // SPC W q : hide active tool window (JetBrains Shift-Esc)
                "tab" => jump_to_last_tool_window, // SPC W TAB : jump to last tool window (JetBrains F12)
                "z" => toggle_ide,                 // SPC W z : hide all tool windows (Zen)
                "b" => focus_bookmarks,            // SPC W b : Bookmarks tool window
                "k" => focus_marks_panel,          // SPC W k : Marks tool window
                "R" => focus_registers_panel,      // SPC W R : Registers tool window
                "j" => focus_jumplist_panel,       // SPC W j : Jumplist tool window
                "u" => focus_recent_panel,         // SPC W u : Recent Files tool window
                "d" => focus_todo_panel,           // SPC W d : TODO tool window
            },
            "p" => { "Project"
                "f" => file_picker,                // SPC p f
                "p" => file_picker,                // SPC p p
                "b" => buffer_picker,              // SPC p b : project buffer
                "h" => file_picker,                // SPC p h : find file
                "s" => global_search,              // SPC p s
                "r" => goto_last_modified_file,    // SPC p r
                "t" => file_explorer,              // SPC p t : project tree (treemacs)
                "v" => git_status,                 // SPC p v : open project VC (magit status)
                "V" => toggle_auto_reveal,         // SPC p V : toggle always-select-opened-file
                "d" => file_explorer,              // SPC p d : find directory
                "g" => symbol_picker,              // SPC p g : find tags
                "o" => global_search,              // SPC p o : multi-occur
                "a" => toggle_test_file,           // SPC p a : toggle implementation/test file
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
                "h" => describe_diagnostics_checker,   // SPC e h : describe checker (language servers)
                "v" => diagnostics_verify_setup,       // SPC e v : verify diagnostics/LSP setup
                "c" => clear_diagnostics,              // SPC e c : clear all diagnostics
                "." => goto_last_diag,
            },
            "c" => { "Comments"
                "l" => toggle_line_comments,       // SPC c l
                "c" => toggle_comments,            // SPC c c
                "b" => toggle_block_comments,      // SPC c b
                "p" => toggle_comments,            // SPC c p : comment paragraph
                "h" => fold_comments,              // SPC c h : hide comments (fold comment blocks)
                "t" => comment_to_line,            // SPC c t : comment/uncomment to a prompted line
                "y" => [yank, toggle_comments],    // SPC c y : comment and yank
                "d" => wclose,                     // SPC c d : close compilation window
                "L" => toggle_line_comments,       // SPC c L : invert/toggle comment lines
                "T" => invert_comment_to_line,     // SPC c T : invert (per-line) comment to a prompted line
                "Y" => [yank, toggle_comments],    // SPC c Y : invert comment and yank
                "P" => toggle_comments,            // SPC c P : invert comment paragraphs
                "C" => run_active_config,          // SPC c C : compile (run active config)
                "r" => rerun_last_run,             // SPC c r : recompile (re-run last)
                "m" => run_config_manager,         // SPC c m : pick a build/run target (helm-make)
                "k" => clear_run_output,           // SPC c k : kill compilation (clear run output)
                "x" => comment_kill,               // SPC c x : kill the comment on the current line (emacs comment-kill)
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
                "e" => goto_char,                  // SPC j e : easymotion — label & jump to a char
                "a" => goto_char,                  // SPC j a : avy-goto-char (alias)
                "f" => goto_definition,            // SPC j f : jump to elisp function def
                "v" => goto_definition,            // SPC j v : jump to elisp variable def
                "I" => workspace_symbol_picker,    // SPC j I : jump to def in any buffer (imenu)
                "=" => format_selections,          // SPC j = : format region/buffer
                "+" => format_selections,          // SPC j + : format region/buffer (alt)
                "(" => goto_prev_unmatched_paren,  // SPC j ( : jump to first unbalanced paren
                "D" => file_explorer_in_current_buffer_directory, // SPC j D : current directory listing
                "U" => goto_file,                  // SPC j U : select URL and follow
                "s" => paredit_split,              // SPC j s : split sexp/string at point
                "S" => paredit_split,              // SPC j S : split sexp, newline (approx: split)
            },
            // Spacemacs's frame map. zmax has real frames now, so these run the
            // frame commands rather than the layout/window stand-ins they used to.
            // Only the chords fzf.vim does not already hold are here: SPC F f / d /
            // b / B / o are the fzf pickers (:Files / :Todo / :Buffers / :BTags /
            // :Locate) in zmax, so spacemacs's frame keys on those five letters
            // have no slot — the C-x 5 map is the whole frame map, unshadowed.
            "F" => { "Frames"
                "n" => make_frame_command,         // SPC F n : create a new frame
                "D" => delete_other_frames,        // SPC F D : delete all other frames
                "O" => dired_other_frame,          // SPC F O : open dired in another frame
            },
            "o" => { "Org / user"
                "c" => org_capture,                // SPC o c : org-mode capture
            },
            "n" => { "Numbers/Narrow"
                "+" => increment,                  // SPC n + : increase number under point
                "=" => increment,                  // SPC n = : increase number under point
                "-" => decrement,                  // SPC n - : decrease number under point
                "_" => decrement,                  // SPC n _ : decrease number under point
                "r" => narrow_to_region,           // SPC n r : narrow buffer to selection (fold outside)
                "w" => widen,                      // SPC n w : widen (remove narrowing, show whole buffer)
                "f" => narrow_to_function,         // SPC n f : narrow to the enclosing function
                "F" => narrow_to_function_indirect, // SPC n F : narrow to function in an indirect (split) view
                "R" => narrow_region_indirect,     // SPC n R : narrow to selection in an indirect (split) view
                "p" => narrow_to_page,             // SPC n p : narrow to the current page
                "P" => narrow_to_page_indirect,    // SPC n P : narrow to page in an indirect (split) view
            },
            "g" => { "Goto (LSP)"
                "d" => goto_definition,
                "D" => goto_declaration,
                "r" => goto_reference,
                "i" => goto_implementation,
                "y" => goto_type_definition,
                "h" => call_hierarchy_incoming_calls, // SPC g h : call hierarchy — who calls this (JetBrains Ctrl-Alt-H)
                "H" => call_hierarchy_outgoing_calls, // SPC g H : call hierarchy — what this calls
                "T" => type_hierarchy_supertypes,  // SPC g T : type hierarchy — supertypes (JetBrains Ctrl-H)
                "U" => type_hierarchy_subtypes,    // SPC g U : type hierarchy — subtypes
                "b" => git_blame_line,             // SPC g b : git blame current line (spacemacs magit-blame)
                "s" => git_status,                 // SPC g s : magit status porcelain (Spacemacs magit-status)
                "m" => resolve_conflicts,          // SPC g m : open 3-way merge-conflict resolver
                "G" => focus_git_panel,            // SPC g G : focus zmax Git changes panel
                "t" => git_file_log_picker,        // SPC g t : git time machine (browse file history)
                "M" => git_blame_line,             // SPC g M : last commit message of current line
                "=" => git_diff,                   // SPC g = : side-by-side diff vs HEAD (SPC g d is goto_definition)
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
                    "r" => resolve_conflicts,      // SPC g c r : open 3-way merge resolver
                },
            },
            "l" => { "Layouts / LSP"
                // LSP actions (zmax); the rest are Spacemacs layout/workspace keys.
                "r" => rename_symbol,              // SPC l r : rename symbol
                "a" => code_action,                // SPC l a : code action
                "O" => organize_imports,           // SPC l O : optimize/organize imports (JetBrains Ctrl-Alt-O)
                "i" => implement_methods,          // SPC l i : implement interface/trait members (JetBrains Ctrl-I)
                "v" => override_methods,           // SPC l v : override inherited members (JetBrains Ctrl-O)
                "g" => generate_code,              // SPC l g : generate code — getters/constructors/impls (JetBrains Generate)
                "k" => hover,                      // SPC l k : hover
                "q" => peek_definition,            // SPC l q : peek definition in a popup (JetBrains Quick Definition)
                "s" => signature_help,             // SPC l s : signature help
                "f" => format_selections,          // SPC l f : format
                // --- layouts (named window configurations) ---
                "l" => layout_create,              // SPC l l : create/select a layout
                "n" => layout_next,                // SPC l n : next layout
                "C-l" => layout_next,              // SPC l C-l : next layout (transient)
                "p" => layout_prev,                // SPC l p : previous layout
                "N" => layout_prev,                // SPC l N : previous layout
                "C-h" => layout_prev,              // SPC l C-h : previous layout (transient)
                "tab" => layout_last,              // SPC l TAB : last used layout
                "h" => layout_default,             // SPC l h : default layout
                "d" => layout_delete,              // SPC l d : delete layout (keep buffers)
                "x" => layout_delete,              // SPC l x : kill layout
                "X" => layout_delete,              // SPC l X : kill other layouts (approx)
                "D" => layout_delete,              // SPC l D : delete other layouts (approx)
                "S" => layout_save,                // SPC l S : save layouts to file (s is signature-help)
                "L" => layout_load,                // SPC l L : load layouts from file
                "o" => layout_load,                // SPC l o : open a custom layout (load from file)
                "R" => layout_rename,              // SPC l R : rename current layout
                "b" => buffer_picker,              // SPC l b : select a buffer in the layout
                "t" => buffer_picker,              // SPC l t : display a buffer
                "1" => layout_goto_1, "2" => layout_goto_2, "3" => layout_goto_3,
                "4" => layout_goto_4, "5" => layout_goto_5, "6" => layout_goto_6,
                "7" => layout_goto_7, "8" => layout_goto_8, "9" => layout_goto_9,
                "A" => layout_add_buffers,         // SPC l A : add another layout's buffers
                "C-1" => layout_goto_1, "C-2" => layout_goto_2, "C-3" => layout_goto_3,
                "C-4" => layout_goto_4, "C-5" => layout_goto_5, "C-6" => layout_goto_6,
                "C-7" => layout_goto_7, "C-8" => layout_goto_8, "C-9" => layout_goto_9,
                // Workspaces (eyebrowse) tier — approximated by the same layout
                // ring. `SPC l w` initiates the workspaces transient state.
                "w" => { "Workspaces" sticky=true
                    "q" => exit_transient_state,   // SPC l w q : quit the transient state
                    "w" => layout_create,          // SPC l w w : tagged workspace
                    "n" => layout_next,            // SPC l w n : next workspace
                    "l" => layout_next,            // SPC l w l : next workspace
                    "p" => layout_prev,            // SPC l w p : previous workspace
                    "N" => layout_prev,            // SPC l w N : previous workspace
                    "h" => layout_prev,            // SPC l w h : previous workspace
                    "d" => layout_delete,          // SPC l w d : close workspace
                    "R" => layout_rename,          // SPC l w R : rename current workspace
                    "tab" => layout_last,          // SPC l w TAB : last workspace
                    "1" => layout_goto_1, "2" => layout_goto_2, "3" => layout_goto_3,
                    "4" => layout_goto_4, "5" => layout_goto_5, "6" => layout_goto_6,
                    "7" => layout_goto_7, "8" => layout_goto_8, "9" => layout_goto_9,
                    "C-1" => layout_goto_1, "C-2" => layout_goto_2, "C-3" => layout_goto_3,
                    "C-4" => layout_goto_4, "C-5" => layout_goto_5, "C-6" => layout_goto_6,
                    "C-7" => layout_goto_7, "C-8" => layout_goto_8, "C-9" => layout_goto_9,
                },
            },
            "v" => expand_selection,               // SPC v : expand region
            "x" => { "Text"
                // drag-stuff transient state: j/k keep dragging until q/ESC.
                "." => { "Drag" sticky=true
                    "j" | "down" => drag_line_down,    // SPC x . j : drag line down
                    "k" | "up" => drag_line_up,        // SPC x . k : drag line up
                    "J" => drag_line_down,
                    "K" => drag_line_up,
                    "q" => exit_transient_state,       // SPC x . q : quit the transient state
                },
                "c" => count_selection,            // SPC x c : count chars/words/lines
                "e" => { "Abbrev"
                    "u" => unexpand_abbrev,        // SPC x e u : undo last abbrev expansion (emacs unexpand-abbrev)
                    "m" => abbrev_prefix_mark,     // SPC x e m : mark abbrev prefix boundary (emacs abbrev-prefix-mark, M-')
                },
                "u" => switch_to_lowercase,        // SPC x u : lowercase
                "U" => switch_to_uppercase,        // SPC x U : uppercase the selection
                "o" => goto_file,                  // SPC x o : open link in frame (avy)
                "i" => { "Symbol style"
                    "C" => symbol_upper_camel,     // SPC x i C : UpperCamelCase
                    "U" => symbol_up_case,         // SPC x i U : UP_CASE
                    "_" => symbol_under_score,     // SPC x i _ : under_score
                    "c" => symbol_lower_camel,     // SPC x i c : camelCase (vim-abolish crc)
                    "-" => symbol_kebab,           // SPC x i - : kebab-case (vim-abolish cr-)
                    "." => symbol_dot,             // SPC x i . : dot.case (vim-abolish cr.)
                },
                "l" => { "Lines"
                    "r" => randomize_lines_in_region, // SPC x l r : randomize lines
                },
                "w" => { "Words"
                    "c" => count_words_region,     // SPC x w c : count occurrences per word
                    "r" => randomize_words_in_region, // SPC x w r : randomize words
                },
                "j" => { "Justify"
                    "l" => justify_left,           // SPC x j l : justify left (fill)
                    "c" => justify_center,         // SPC x j c : justify center
                    "f" => justify_full,           // SPC x j f : justify full
                    "r" => justify_right,          // SPC x j r : justify right
                    "n" => justify_none,           // SPC x j n : justify none (left-fill)
                },
                "tab" => indent,                   // SPC x TAB : indent region
                "a" => { "Align"
                    "a" => align_selections,       // SPC x a a : align cursors
                    "&" => align_at_ampersand,     // SPC x a & : align at &
                    "c" => indent,                 // SPC x a c : align indentation (reindent)
                    "l" => align_left_at_char,     // SPC x a l : left-align at typed char
                    "r" => align_at_regex,         // SPC x a r : align region at a typed regexp
                    "m" => align_at_arithmetic,    // SPC x a m : align at math operators
                    "L" => align_right_at_char,    // SPC x a L : right-align at typed char
                    "(" => align_at_lparen,        // SPC x a ( : align at (
                    ")" => align_at_rparen,        // SPC x a ) : align at )
                    "[" => align_at_lbracket,      // SPC x a [ : align at [
                    "]" => align_at_rbracket,      // SPC x a ] : align at ]
                    "{" => align_at_lbrace,        // SPC x a { : align at {
                    "}" => align_at_rbrace,        // SPC x a } : align at }
                    "," => align_at_comma,         // SPC x a , : align at ,
                    "." => align_at_dot,           // SPC x a . : align at . (numeric)
                    ":" => align_at_colon,         // SPC x a : : align at :
                    ";" => align_at_semicolon,     // SPC x a ; : align at ;
                    "=" => align_at_equals,        // SPC x a = : align at =
                },
            },
            "r" => { "Resume / registers"
                "l" => last_picker,                // SPC r l : resume picker
                "e" => register_picker,            // SPC r e : registers
                "r" => register_picker,            // SPC r r : show registers
                "m" => marks_picker,               // SPC r m : pick a mark and jump (:Marks)
                "y" => register_picker,            // SPC r y : kill ring
                "a" => append_next_kill,           // SPC r a : append next kill to last (emacs C-M-w)
                ":" => command_history_picker,     // SPC r : : command-line history (:History:)
                "/" => search_history_picker,      // SPC r / : search history (:History/)
                "s" => last_picker,                // SPC r s : resume last search/picker buffer
            },
            "k" => { "Lisp (sexp)"
                // navigation maps onto the tree-sitter node commands
                "0" => move_parent_node_start,     // SPC k 0 : beginning of sexp
                "$" => move_parent_node_end,       // SPC k $ : end of sexp
                "U" => expand_selection,           // SPC k U : up to parent sexp
                "I" => [move_parent_node_start, insert_mode], // SPC k I : begin + insert
                "h" => select_prev_sibling,        // SPC k h : previous symbol
                "l" => select_next_sibling,        // SPC k l : next symbol
                "j" => goto_next_close_paren,      // SPC k j : forward to next closing paren
                "k" => goto_prev_open_paren,       // SPC k k : backward to previous opening paren
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
                "a" => paredit_absorb,             // SPC k a : absorb previous sexp into form
                "c" => paredit_convolute,          // SPC k c : convolute sexp
                "e" => paredit_splice_kill_forward,  // SPC k e : splice, killing forward
                "E" => paredit_splice_kill_backward, // SPC k E : splice, killing backward
                "(" => paredit_insert_sexp_before, // SPC k ( : insert sexp before current
                ")" => paredit_insert_sexp_after,  // SPC k ) : insert sexp after current
                "`" => { "Hybrid"
                    "s" => paredit_slurp_forward,  // SPC k ` s : hybrid slurp forward
                    "t" => paredit_transpose,      // SPC k ` t : hybrid transpose
                    "p" => paredit_transpose,      // SPC k ` p : hybrid push (swap)
                    "k" => [expand_selection, delete_selection], // SPC k ` k : hybrid delete sexp
                },
                "d" => { "Delete"
                    "x" => [expand_selection, delete_selection], // SPC k dx : delete sexp
                    "s" => [expand_selection, delete_selection], // SPC k ds : delete symbol
                    "w" => [collapse_selection, subword_extend_w, delete_selection], // SPC k dw
                },
                "D" => { "Delete backward"
                    "x" => [expand_selection, delete_selection], // SPC k Dx : delete sexp backward
                    "s" => [expand_selection, delete_selection], // SPC k Ds : delete symbol backward
                    "w" => [collapse_selection, subword_extend_b, delete_selection], // SPC k Dw
                },
            },
            "h" => { "Help"
                "h" => help,                       // SPC h h : open the inline Help browser
                "k" => help,                       // SPC h k : describe key / commands
                "?" => help,                       // SPC h ? : list bindings
                "c" => help,                       // SPC h c : describe command
                "space" => help,                   // SPC h SPC : discover docs (Help browser)
                "f" => browse_faq,                 // SPC h f : discover the FAQ (browse FAQ.md)
                "l" => layer_search,               // SPC h l : search capability areas (zmax "layers")
                "p" => package_search,             // SPC h p : search language packages
                "n" => browse_news,                // SPC h n : browse zmax release notes (NEWS)
                "r" => help,                       // SPC h r : search documentation files (Help browser)
                "." => config_variable_search,     // SPC h . : search config variables (dotfile vars)
                "i" => info_search,                // SPC h i : search info manuals (apropos, seeded at point)
                "m" => man_page_search,            // SPC h m : search man pages (apropos picker)
                "d" => { "Describe"
                    "b" => help,                   // SPC h d b : describe bindings (Help browser: keys)
                    "f" => hover,                  // SPC h d f : describe function (LSP hover at point)
                    "k" => help,                   // SPC h d k : describe key (Help browser)
                    "v" => hover,                  // SPC h d v : describe variable (LSP hover at point)
                    "m" => describe_current_modes, // SPC h d m : describe current modes
                    "a" => hover,                  // SPC h d a : describe expression under point
                    "p" => describe_language_package, // SPC h d p : describe language-support package
                    "t" => describe_text_properties, // SPC h d t : describe text properties (syntax node stack)
                    "x" => help,                   // SPC h d x : describe ex-command (Help browser)
                    "l" => copy_last_keys,         // SPC h d l : copy last pressed keys to clipboard
                    "s" => copy_system_info,       // SPC h d s : copy system info to clipboard
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
                "e" => { "Eval (elisp)"
                    "e" => eval_elisp_line,        // SPC m e e : eval last sexp (current line)
                    "$" => eval_elisp_line,        // SPC m e $ : goto eol + eval last sexp
                    "l" => eval_elisp_line,        // SPC m e l : goto eol + eval last sexp
                    "r" => eval_elisp_region,      // SPC m e r : eval region
                    "b" => eval_elisp_buffer,      // SPC m e b : eval buffer
                    "f" => eval_elisp_defun,       // SPC m e f : eval current defun
                    "c" => eval_elisp_defun,       // SPC m e c : eval current form
                },
                // Org-mode editing (zmax has no per-language keymaps, so the org
                // commands live in the global Major-mode menu; they no-op politely
                // off org buffers). Promote uses `H` because lowercase `h` is the
                // Help submenu above.
                "t" => org_todo,                   // SPC m t : cycle TODO keyword
                "p" => org_priority,               // SPC m p : cycle priority cookie
                "tab" => org_cycle,                // SPC m TAB : toggle subtree fold
                "z" => org_cycle,                  // SPC m z : toggle subtree fold (TAB alias)
                "H" => org_promote,                // SPC m H : promote heading (h is Help submenu)
                "l" => org_demote,                 // SPC m l : demote heading
                "j" => org_next_heading,           // SPC m j : next heading
                "k" => org_prev_heading,           // SPC m k : previous heading
                "a" => org_fold_all,               // SPC m a : fold all headings
                "A" => org_unfold_all,             // SPC m A : unfold all
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
                        "3" => make_3_windows,     // SPC u SPC w 3 : three-window layout (force)
                        "4" => make_4_windows,     // SPC u SPC w 4 : four-window layout (force)
                        "D" => delete_window_and_buffer, // SPC u SPC w D : delete window + its buffer
                    },
                },
            },
        },
    });

    // Visual / select mode: motions extend, operators act directly.
    let mut select = keymap!({ "Visual mode"
        // Each motion appends `block_reproject`: in visual-block mode it rebuilds
        // the rectangle from the anchor to the new cursor; otherwise it is a no-op.
        "h" | "left"  => [extend_char_left, block_reproject],
        "j" | "down"  => [extend_visual_line_down, block_reproject],
        "k" | "up"    => [extend_visual_line_up, block_reproject],
        "l" | "right" => [extend_char_right, block_reproject],

        "w" => [subword_extend_w, block_reproject],
        "b" => [subword_extend_b, block_reproject],
        "e" => [subword_extend_e, block_reproject],
        "W" => [extend_next_long_word_start, block_reproject],
        "B" => [extend_prev_long_word_start, block_reproject],
        "E" => [extend_next_long_word_end, block_reproject],

        "0" | "home" => [extend_to_line_start, block_reproject],
        "^"          => [extend_to_first_nonwhitespace, block_reproject],
        "$" | "end"  => block_dollar,         // visual-block: ragged right per row
        "G"          => [extend_to_last_line, block_reproject],
        "%"          => match_brackets_or_goto_percent,
        "{"          => goto_prev_paragraph,
        "}"          => goto_next_paragraph,
        "("          => move_sentence_backward,
        ")"          => move_sentence_forward,

        "f" => [extend_next_char, block_reproject],
        "F" => [extend_prev_char, block_reproject],
        "t" => [extend_till_char, block_reproject],
        "T" => [extend_till_prev_char, block_reproject],
        ";" => repeat_find_char,
        "," => repeat_find_char_reverse,

        // search in Visual mode extends the selection to the match (vim v_/, v_n)
        "/" => search,
        "?" => rsearch,
        "n" => extend_search_next_vim,  // vim n (visual): repeat in the last search direction
        "N" => extend_search_prev_vim,  // vim N (visual): repeat in the opposite direction
        "*" => [search_selection_detect_word_boundaries, extend_search_next],
        "#" => [search_selection_detect_word_boundaries, extend_search_prev],

        "i" => select_textobject_inner,
        "a" => select_textobject_around,

        // Save before the delete: it collapses the selection, and the trailing
        // normal_mode is what records the area for `gv`. The first save of an
        // exit wins (see VISUAL_SAVE_PENDING), so this one sticks.
        "d" | "x" => [save_visual_selection, delete_selection, normal_mode],
        "c" | "s" => [save_visual_selection, change_selection], // gv reselects the changed area
        "\"" => select_register, // "{reg} in Visual: pick the register for the next y/d/p (e.g. "+y)
        "y"       => [save_visual_selection, yank_textobject, normal_mode],
        "p"       => replace_with_yanked,
        "r"       => replace,
        "J"       => [save_visual_selection, join_selections, normal_mode],
        "~"       => switch_case,
        "u"       => [switch_to_lowercase, normal_mode],
        "U"       => [switch_to_uppercase, normal_mode],
        ">"       => [indent, normal_mode],
        "<"       => [unindent, normal_mode],
        // o/O: in charwise/linewise visual these flip the cursor end; in
        // visual-block, o jumps to the opposite corner and O to the other
        // column edge on the same row (vim's block o/O). The commands fall back
        // to flip_selections when not in block mode.
        "o"       => block_swap_corners,
        "O"       => block_swap_columns,

        // --- visual block (CTRL-V) -----------------------------------------
        // C-v toggles a true rectangular block: the selection projects to one
        // range per row spanning the same columns. Motions grow the rectangle
        // (via block_reproject); I/A block-insert/append at the left/right edge
        // of every row, c/d/x/y act on the whole block.
        "C-v"     => visual_block_mode,
        "I"       => block_insert,  // block-insert at left column (pads/skips per vim)
        "A"       => block_append,  // block-append at right column, padding short rows
        "V"       => visual_line_mode,  // toggle visual-line off (vim V in Visual leaves)
        "P"       => replace_with_yanked,      // replace the highlighted area with a register
        // v_=: filter the highlighted lines through 'equalprg' when it is set,
        // else reindent them (vim's own behaviour with an empty 'equalprg').
        "=" => [save_visual_selection, filter_equalprg, normal_mode],

        // filter highlighted text through an external command (vim visual !)
        "!"       => [save_visual_selection, shell_pipe, normal_mode],

        // linewise visual operators: extend to whole lines, then act
        "D" | "X" => [extend_to_line_bounds, delete_selection_linewise, normal_mode],
        "Y"       => [extend_to_line_bounds, yank, collapse_selection, normal_mode],
        "C" | "S" | "R" => [save_visual_selection, extend_to_line_bounds, change_selection],

        // zf: create a fold over the highlighted lines (vim visual zf)
        "z" => { "Fold"
            "f" => [fold_create, normal_mode],
        },

        // gq / gw: reformat the highlighted lines (LSP formatter)
        "g" => { "Goto"
            "g" => [extend_to_file_start, block_reproject],   // vgg: extend selection to first line
            "C-g" => document_stats,                 // v g CTRL-G: count the selection
            "e" => [extend_to_last_line, block_reproject],    // ge: extend to last line
            "h" => [extend_to_first_nonwhitespace, block_reproject], // extend to first non-blank
            "l" | "$" => [extend_to_line_end, block_reproject],      // extend to line end
            // v_gq / v_gw: reflow the highlighted lines to 'textwidth' — the same
            // operator normal-mode `gq{motion}` runs, not the LSP formatter (`=`).
            // gw keeps the cursor where it was.
            "q" => [save_visual_selection, reflow_selections, normal_mode],
            "w" => [save_visual_selection, reflow_selections_keep_cursor, normal_mode],
            "v" => reselect_visual,                  // gv: reselect previous highlighted area
            "J" => [save_visual_selection, join_selections, normal_mode],   // gJ: join lines, no space (approx)
            "c" => [toggle_comments, normal_mode],   // gc: comment the highlighted lines
            "?" => [rot13, normal_mode],             // g?: ROT13 the selection
            "u" => [switch_to_lowercase, normal_mode], // gu: lowercase (vim also allows plain u)
            "U" => [switch_to_uppercase, normal_mode], // gU: uppercase (vim also allows plain U)
            "~" => [switch_case, normal_mode],       // g~: toggle case (vim also allows plain ~)
            "C-a" => increment_sequential,          // g CTRL-A: bump each line by a growing amount
            "C-x" => decrement_sequential,          // g CTRL-X: decrement each line likewise
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
        // Leaving Visual/visual-block collapses to a single cursor (like vim), not
        // one cursor per line — keep_primary drops the block's extra cursors.
        "esc" => [save_visual_selection, keep_primary_selection, collapse_selection, normal_mode],
    });

    // Insert mode: vim-style editing keys.
    let insert = keymap!({ "Insert mode"
        "esc" => [mark_insert_exit, normal_mode],
        "C-c" => [mark_insert_exit, normal_mode],
        "C-[" => [mark_insert_exit, normal_mode],   // CTRL-[ = <Esc>
        "F1"  => [mark_insert_exit, normal_mode, help], // i_<F1> = <Help>: stop insert, open Help
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
        "C-d"   => insert_unindent,        // i_CTRL-D; 0/^ CTRL-D deletes all indent

        // i_CTRL-^: turn the `:lmap` language keymap off/on ('iminsert').
        "C-^"   => toggle_lang_keymap,

        // keyword/omni completion (vim i_CTRL-N / i_CTRL-P)
        "C-n"   => completion,
        "C-p"   => completion,
        // CTRL-X completion sub-mode: the keyword/identifier/omni variants all
        // map to zmax's single (LSP + word) completion.
        "C-x" => { "Complete"
            // i_CTRL-X_CTRL-O / i_CTRL-X_CTRL-U: vim's 'omnifunc' / 'completefunc'.
            // With the option unset both fall back to zmax's LSP+word completion,
            // which is what the keys did before the two options had a consumer.
            "C-o" => complete_omni_func,   // i_CTRL-X_CTRL-O: 'omnifunc'
            "C-u" => complete_user_func,   // i_CTRL-X_CTRL-U: 'completefunc'
            "C-n" => completion,   // keyword completion, forward
            "C-p" => completion,   // keyword completion, backward
            "C-i" => completion,   // identifier completion
            // The sub-modes whose *source* is not zmax's LSP+word completion get
            // their own: each gathers its candidates (buffer lines, the directory,
            // the 'dictionary'/'thesaurus' files, the registers, the buffer's
            // #defines) and inserts the one you pick.
            "C-f" => complete_filename,       // i_CTRL-X_CTRL-F: file names
            "C-l" => complete_line,           // i_CTRL-X_CTRL-L: whole lines
            "C-k" => complete_dictionary,     // i_CTRL-X_CTRL-K: 'dictionary' words
            "C-d" => complete_define,         // i_CTRL-X_CTRL-D: defined identifiers
            "C-t" => complete_thesaurus,      // i_CTRL-X_CTRL-T: 'thesaurus' words
            "C-r" => complete_register_word,  // i_CTRL-X_CTRL-R: words in the registers
            "s"   => insert_spell_suggest,    // i_CTRL-X_s: spelling suggestions
            "C-s" => insert_spell_suggest,    // i_CTRL-X_CTRL-S: same
            // Tag completion (CTRL-]) has no source here, so it falls back to the
            // LSP+word completion, as does the `:`-line one.
            "C-]" => complete_tag, // i_CTRL-X_CTRL-]: tag completion
            "C-v" => complete_cmdline, // i_CTRL-X_CTRL-V: complete like in : command line
            "C-e" => scroll_down,  // i_CTRL-X_CTRL-E: scroll window up (view down)
            "C-y" => scroll_up,    // i_CTRL-X_CTRL-Y: scroll window down (view up)
            "C-z" => no_op,        // i_CTRL-X_CTRL-Z: stop completion, leave text unchanged
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
        "tab"   => ghost_text_accept,       // accept AI ghost-text suggestion, else expand emmet/Tab
        "A-right" => ghost_text_accept_word, // partial-accept: take the next word of the suggestion

        "C-r"   => insert_register,
        "C-o"   => insert_command_normal,         // i_CTRL-O: run one Normal command, return to Insert
        "C-]"   => expand_abbrev,                 // i_CTRL-]: trigger abbreviation before point
        "C-a"   => insert_last_inserted_text,    // i_CTRL-A: insert previously inserted text
        "C-@"   => insert_last_inserted_and_stop, // i_CTRL-@: insert previously inserted text, stop insert
        "C-e"   => copy_char_below,         // vim i_CTRL-E: insert the character below the cursor
        "C-y"   => copy_char_above,         // vim i_CTRL-Y: insert the character above the cursor
        "ins"   => toggle_replace_mode,    // <Insert>: toggle insert/overtype (Replace)

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
        // i_CTRL-SHIFT-V / i_CTRL-SHIFT-Q: the modifyOtherKeys spellings of the two
        // above — a terminal that distinguishes them sends these instead.
        "C-S-v"   => insert_char_interactive,
        "C-S-q"   => insert_char_interactive,
        "A-f"     => move_next_word_start, // M-f forward-word
        "A-b"     => move_prev_word_start, // M-b backward-word
        "A-j"     => default_indent_new_line, // M-j break line + continue comment (default-indent-new-line)
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
    add_transient_states(&mut normal);

    // Visual mode gets the whole SPC leader too. Spacemacs exposes the `SPC`
    // menu in visual state, and zmax previously only had it in Normal — so
    // pressing space while a `v`-selection was active did nothing (mouse-drag
    // selections stay in Normal mode, which is why the leader worked there but
    // not after `v`). Graft Normal's leader subtree into Select; the
    // visual-specific overrides below then win where they differ.
    let space_key = chord("space")[0];
    if let Some(leader) = normal.search(&[space_key]).cloned() {
        if let Some(sel) = select.node_mut() {
            sel.insert(space_key, leader);
        }
    }

    // Make git hunk-reset work on a visual selection too: SPC g r in select mode
    // resets every hunk the selection touches. `]c`/`[c` also navigate hunks
    // while selecting.
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

/// Emacs/readline convenience chords that `base()` layers in for the
/// spacemacs/emacs-flavored experience but that vim does not bind. The pure
/// `vim` preset strips them so its keys match vim's per-mode index. (Note: vim's
/// own `C-g` file-info, `C-v`/`C-q` insert-literal, and the `C-w`/`C-u`/`C-t`/
/// `C-d`/`C-k`/`C-r` insert keys are real vim and are NOT in these lists.)
#[rustfmt::skip]
const NON_VIM_NORMAL: &[&str] = &[
    "A-x", "A-<", "A->", "A-f", "A-b", "A-d", "A-w", "A-v",
    "C-space", "C-l", "C-s", "C-/", "C-_", "A-;", "A-m", "A-q", "A-^",
    // The rest of the emacs global map (see the "emacs global map" block in
    // `base`). Stripping the top-level chord drops the whole submap with it,
    // which is what removes `M-g …`, `M-s …` and the `F2` two-column map.
    "A-a", "A-e", "A-k", "A-h", "A-{", "A-}", "A-c", "A-l", "A-u", "A-@",
    "A-y", "A-z", "A-=", "A-'", "A-$", "A-~", "A-.", "A-?", "A-X", "A-tab",
    "A-left", "A-right", "A-!", "A-|", "A-r", "A-t", "A-space", "A-\\",
    "C-@", "C-S-tab", "F2", "F3", "A-g", "A-s",
    "A-C-f", "A-C-b", "A-C-k", "A-C-t", "A-C-space", "A-C-@", "A-C-h",
    "A-C-a", "A-C-e", "A-C-d", "A-C-u", "A-C-n", "A-C-p", "A-C-v",
    "A-C-S-v", "A-C-S-l", "A-C-q", "A-C-\\", "A-C-j", "A-C-i", "A-C-/",
    "A-C-.", "A-C-s", "A-C-r", "A-C-w", "A-C-o",
];
#[rustfmt::skip]
const NON_VIM_INSERT: &[&str] = &["C-f", "C-b", "A-f", "A-b", "A-v", "A-<", "A->", "A-/", "C-/", "C-_"];

/// Remove `keys` (top-level chords) from `mode` if present. `shift_remove`
/// preserves the order of the surviving keys and no-ops on absent ones.
fn strip_keys(keymap: &mut HashMap<Mode, KeyTrie>, mode: Mode, keys: &[&str]) {
    if let Some(node) = keymap.get_mut(&mode).and_then(KeyTrie::node_mut) {
        for k in keys {
            node.shift_remove(&k.parse::<KeyEvent>().expect("valid key"));
        }
    }
}

/// The **vim** preset: pure vim, with no spacemacs/emacs layer. It is [`base`]
/// with the `SPC` leader removed from Normal and Select (so vim shows no
/// which-key popup and `<Space>` reverts to vim's "move one char right" / extend
/// in visual) and the non-vim emacs/readline chords stripped. `C-x` stays vim's
/// `decrement`.
pub fn default() -> HashMap<Mode, KeyTrie> {
    let mut keymap = base();
    let space = chord("space")[0];
    if let Some(node) = keymap.get_mut(&Mode::Normal).and_then(KeyTrie::node_mut) {
        node.shift_remove(&space);
        node.insert(
            space,
            KeyTrie::MappableCommand(MappableCommand::move_char_right),
        );
    }
    if let Some(node) = keymap.get_mut(&Mode::Select).and_then(KeyTrie::node_mut) {
        node.shift_remove(&space);
        node.insert(
            space,
            KeyTrie::MappableCommand(MappableCommand::extend_char_right),
        );
    }
    strip_keys(&mut keymap, Mode::Normal, NON_VIM_NORMAL);
    strip_keys(&mut keymap, Mode::Insert, NON_VIM_INSERT);
    // vim `i_CTRL-_`: toggle 'revins'. Only the vim preset gets this — the strip
    // above just freed `C-_`, which the shipped spacemacs default keeps bound to
    // emacs `undo`. The key is inert unless 'allowrevins' is set, exactly as vim
    // gates it.
    if let Some(node) = keymap.get_mut(&Mode::Insert).and_then(KeyTrie::node_mut) {
        node.insert(
            chord("C-_")[0],
            KeyTrie::MappableCommand(MappableCommand::toggle_revins),
        );
    }
    keymap
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{KeyTrie, MappableCommand};
    use zmax_view::input::KeyEvent;

    fn cmd_name(trie: &KeyTrie) -> Option<&str> {
        match trie {
            KeyTrie::MappableCommand(MappableCommand::Static { name, .. }) => Some(name),
            _ => None,
        }
    }

    /// The first static command of a leaf — its own name if it's a single command,
    /// or the first command of a `[a, b]` sequence.
    fn seq_first(trie: &KeyTrie) -> Option<&str> {
        match trie {
            KeyTrie::Sequence(cmds) => match cmds.first() {
                Some(MappableCommand::Static { name, .. }) => Some(name),
                _ => None,
            },
            _ => cmd_name(trie),
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
        assert_eq!(
            cmd_name(resolve(n, "x").unwrap()),
            Some("delete_chars_forward_vim")
        );
        assert_eq!(cmd_name(resolve(n, "i").unwrap()), Some("insert_mode"));
        assert_eq!(cmd_name(resolve(n, "a").unwrap()), Some("append_mode"));
    }

    #[test]
    fn vim_window_family_bound() {
        let km = default();
        let n = &km[&Mode::Normal];
        // vim CTRL-W window moves map onto zmax's view commands.
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
    fn vim_g_prefix_is_vim() {
        let km = default();
        let n = &km[&Mode::Normal];
        // ge/gn/gN carry vim meaning, not the zmax bindings they collided with.
        assert_eq!(
            cmd_name(resolve(n, "g e").unwrap()),
            Some("vim_move_prev_word_end")
        );
        // gn/gN now visually select the match (select_gn_match{,_prev} + select_mode).
        assert_eq!(
            seq_first(resolve(n, "g n").unwrap()),
            Some("select_gn_match")
        );
        assert_eq!(
            seq_first(resolve(n, "g N").unwrap()),
            Some("select_gn_match_prev")
        );
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
        // The SPC leader lives in the shared base (the spacemacs preset); the
        // pure `vim` preset strips it.
        let km = base();
        let n = &km[&Mode::Normal];
        // spacemacs SPC tree resolves to the expected zmax commands.
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
        // `SPC e n` is the errors transient state's entry key: it runs
        // goto_next_diag *and* latches the state (see transient_states_latch).
        assert_eq!(
            enter_cmd(resolve(n, "space e n").unwrap()),
            Some("goto_next_diag")
        );
        assert_eq!(
            cmd_name(resolve(n, "space s s").unwrap()),
            Some("global_search")
        );
    }

    /// The name of a sticky node's opening command, if it has one.
    fn enter_cmd(trie: &KeyTrie) -> Option<&str> {
        match trie {
            KeyTrie::Node(node) => match node.on_enter.as_ref()? {
                MappableCommand::Static { name, .. } => Some(name),
                _ => None,
            },
            _ => None,
        }
    }

    fn is_sticky(trie: &KeyTrie) -> bool {
        matches!(trie, KeyTrie::Node(node) if node.is_sticky)
    }

    /// Spacemacs transient states: entering one latches it, so the state's keys
    /// keep firing until `q`/ESC. Every one of them must be a sticky node, must
    /// bind `q` to `exit_transient_state`, and the states Spacemacs enters with
    /// an acting key must run that command on entry.
    #[test]
    fn transient_states_latch() {
        let km = base();
        let n = &km[&Mode::Normal];

        for prefix in [
            "space w .",
            "space b .",
            "space x .",
            "space l w",
            "space z x",
            "space z f",
        ] {
            let node = resolve(n, prefix).unwrap_or_else(|| panic!("{prefix} is bound"));
            assert!(
                is_sticky(node),
                "{prefix} must be a transient (sticky) state"
            );
            assert_eq!(
                cmd_name(resolve(n, &format!("{prefix} q")).unwrap()),
                Some("exit_transient_state"),
                "{prefix} q must leave the transient state"
            );
        }

        // Entry keys that act *and* latch.
        for (chord, cmd) in [
            ("space w [", "resize_view_narrower"),
            ("space w {", "resize_view_shorter"),
            ("C-w [", "resize_view_narrower"),
            ("space n +", "increment"),
            ("space n -", "decrement"),
            ("space e n", "goto_next_diag"),
            ("space e p", "goto_prev_diag"),
        ] {
            let node = resolve(n, chord).unwrap_or_else(|| panic!("{chord} is bound"));
            assert!(is_sticky(node), "{chord} must latch a transient state");
            assert_eq!(
                enter_cmd(node),
                Some(cmd),
                "{chord} must run {cmd} on entry"
            );
        }

        // Inside the window transient state a bare resize key repeats.
        assert_eq!(
            cmd_name(resolve(n, "space w [ ]").unwrap()),
            Some("resize_view_wider")
        );
        // Text/frame scaling states.
        assert_eq!(
            cmd_name(resolve(n, "space z x +").unwrap()),
            Some("text_scale_increase")
        );
        assert_eq!(
            cmd_name(resolve(n, "space z f 0").unwrap()),
            Some("frame_zoom_reset")
        );
        assert_eq!(
            cmd_name(resolve(n, "space z z").unwrap()),
            Some("toggle_ide")
        );
    }

    #[test]
    fn visual_mode_has_spc_leader() {
        // Regression: pressing SPC in Visual (Select) mode must open the same
        // leader tree as Normal mode — it was previously Normal-only, so a
        // `v`-selection couldn't reach the SPC menu (mouse-drag could, because
        // it stays in Normal mode).
        let km = base();
        let s = &km[&Mode::Select];
        assert_eq!(
            cmd_name(resolve(s, "space f f").unwrap()),
            Some("file_picker")
        );
        assert_eq!(
            cmd_name(resolve(s, "space space").unwrap()),
            Some("command_palette")
        );
        assert_eq!(cmd_name(resolve(s, "space w s").unwrap()), Some("hsplit"));
        // the visual-specific override (:hunk-reset, a typable) still wins over
        // the grafted Normal leaf (goto_reference): the leaf is no longer it.
        assert_ne!(
            cmd_name(resolve(s, "space g r").unwrap()),
            Some("goto_reference")
        );
    }

    /// Spacemacs leader chords closed against the gap list, pinned to the exact
    /// command each must run. `SPC K` is the leader half of the emacs `C-x C-k`
    /// keyboard-macro map, so the two must not drift apart.
    #[test]
    fn spacemacs_leader_gap_chords_bound() {
        let km = base();
        let n = &km[&Mode::Normal];
        for (chord, want) in [
            ("space K K", "kmacro_end_or_call_macro"),
            ("space K v", "kmacro_ring_view"),
            ("space K c C", "kmacro_set_counter"),
            ("space K c f", "kmacro_set_format"),
            ("space K e b", "kmacro_bind_to_key"),
            ("space K e e", "kmacro_edit_macro"),
            ("space K e l", "kmacro_edit_lossage"),
            ("space K e s", "kmacro_step_edit_macro"),
            ("space t S", "flyspell_mode"),
            ("space t k m", "describe_keymap"),
            ("space t k t", "describe_bindings"),
            ("space h d K", "describe_keymap"),
            ("space m g G", "xref_find_definitions_other_window"),
            ("space p !", "project_shell_command"),
            ("space p &", "project_async_shell_command"),
            ("space p F", "goto_file"),
            ("space p E", "xref_find_references"),
            ("[ w", "rotate_view_reverse"),
            ("] w", "rotate_view"),
        ] {
            let leaf = resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert_eq!(cmd_name(leaf), Some(want), "wrong command for {chord}");
        }
        // The existing SPC K bindings these were grafted alongside must survive.
        for (chord, want) in [
            ("space K c a", "kmacro_add_counter"),
            ("space K c c", "kmacro_insert_counter"),
            ("space K e r", "kmacro_to_register"),
            ("space K r s", "kmacro_ring_swap"),
        ] {
            let leaf = resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"));
            assert_eq!(cmd_name(leaf), Some(want), "{chord} regressed");
        }
    }

    #[test]
    fn spacemacs_typable_bindings_inserted() {
        let km = base();
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
        let km = base();
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
        // CTRL-] is vim's `:ta`, so it goes through tag_jump (goto_definition plus
        // the tag-stack push) rather than goto_definition alone — that push is what
        // gives CTRL-T something to pop.
        assert_eq!(cmd_name(resolve(n, "C-]").unwrap()), Some("tag_jump"));
        assert_eq!(cmd_name(resolve(n, "C-t").unwrap()), Some("tag_pop"));
        // gt/gT navigate vim tabpages
        assert_eq!(
            cmd_name(resolve(n, "g t").unwrap()),
            Some("goto_next_tabpage")
        );
        assert_eq!(
            cmd_name(resolve(n, "g T").unwrap()),
            Some("goto_previous_tabpage")
        );
        // = reindent operator is a sequence for motions, leaf for ==
        assert_eq!(cmd_name(resolve(n, "= =").unwrap()), Some("indent"));
        assert!(matches!(resolve(n, "= j").unwrap(), KeyTrie::Sequence(_)));

        // visual block + extras: C-v now enters true rectangular block mode,
        // and o/O switch block corners (falling back to flip outside block).
        let s = &km[&Mode::Select];
        assert_eq!(
            cmd_name(resolve(s, "C-v").unwrap()),
            Some("visual_block_mode")
        );
        assert_eq!(
            cmd_name(resolve(s, "o").unwrap()),
            Some("block_swap_corners")
        );
        assert_eq!(
            cmd_name(resolve(s, "O").unwrap()),
            Some("block_swap_columns")
        );
        assert_eq!(cmd_name(resolve(s, "K").unwrap()), Some("hover"));
        // g CTRL-A is vim's *sequential* increment (1/2/3 down a selection),
        // distinct from CTRL-A which bumps every line by the same amount.
        assert_eq!(
            cmd_name(resolve(s, "g C-a").unwrap()),
            Some("increment_sequential")
        );

        // insert-mode indent + completion
        let i = &km[&Mode::Insert];
        assert_eq!(cmd_name(resolve(i, "C-t").unwrap()), Some("indent"));
        assert_eq!(
            cmd_name(resolve(i, "C-d").unwrap()),
            Some("insert_unindent")
        );
        assert_eq!(cmd_name(resolve(i, "C-n").unwrap()), Some("completion"));
    }

    #[test]
    fn emacs_readline_keys_bound() {
        // The emacs/readline convenience chords live in the shared base (the
        // spacemacs preset); the pure `vim` preset strips them — see
        // `pure_vim_strips_non_vim_chords`.
        let km = base();
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
    fn pure_vim_strips_non_vim_chords() {
        // The pure `vim` preset must NOT carry the emacs/readline chords that the
        // shared base layers in for spacemacs — they aren't vim bindings.
        let km = default();
        let n = &km[&Mode::Normal];
        let i = &km[&Mode::Insert];
        for chord in [
            "A-x", "A-<", "A->", "A-f", "A-b", "A-d", "A-w", "A-v", "C-space", "C-l",
        ] {
            assert!(
                resolve(n, chord).is_none(),
                "vim Normal should not bind emacs chord {chord}"
            );
        }
        for chord in ["C-f", "C-b", "A-f", "A-b", "A-v", "A-/"] {
            assert!(
                resolve(i, chord).is_none(),
                "vim Insert should not bind emacs chord {chord}"
            );
        }
        // But real vim keys survive: C-g (file info) in Normal, the insert-literal
        // and edit keys in Insert.
        assert_eq!(cmd_name(resolve(n, "C-g").unwrap()), Some("file_info"));
        assert_eq!(
            cmd_name(resolve(i, "C-v").unwrap()),
            Some("insert_char_interactive")
        );
        assert_eq!(
            cmd_name(resolve(i, "C-w").unwrap()),
            Some("delete_word_backward")
        );
        // And <Space> reverts to vim's move-right (no SPC leader).
        assert_eq!(
            cmd_name(resolve(n, "space").unwrap()),
            Some("move_char_right")
        );
    }

    /// vim's keyword/`#define` navigation (`[i` `]i` `[I` `]I` `[D` `]D` and the
    /// CTRL forms). The lowercase key only *shows* the line, the uppercase one
    /// lists every hit and CTRL jumps to the first — three different commands, so
    /// a regression that collapses them back onto a plain word search is caught.
    /// `[d`/`]d` must keep navigating diagnostics, which own them here.
    #[test]
    fn vim_keyword_and_define_navigation() {
        let km = default();
        let n = &km[&Mode::Normal];
        for (chord, cmd) in [
            ("[ i", "show_keyword_line_from_start"),
            ("] i", "show_keyword_line_from_cursor"),
            ("[ I", "list_keyword_lines_from_start"),
            ("] I", "list_keyword_lines_from_cursor"),
            ("[ D", "list_defines_from_start"),
            ("] D", "list_defines_from_cursor"),
            ("[ C-i", "goto_keyword_line_from_start"),
            ("] C-i", "goto_keyword_line_from_cursor"),
            ("[ C-d", "goto_define_from_start"),
            ("] C-d", "goto_define_from_cursor"),
            // the diagnostics keep [d/]d
            ("[ d", "goto_prev_diag"),
            ("] d", "goto_next_diag"),
        ] {
            assert_eq!(
                cmd_name(resolve(n, chord).unwrap_or_else(|| panic!("{chord} did not resolve"))),
                Some(cmd),
                "{chord} must run {cmd}"
            );
        }
    }

    /// The `z` keys that act on a whole screenful or on a block's padding, and
    /// `{count}:`. These all replaced approximations (page_down for `z+`, a plain
    /// paste for `zp`, …), so pin the real commands.
    #[test]
    fn vim_z_scroll_block_and_count_colon() {
        let km = default();
        let n = &km[&Mode::Normal];
        // z+ / z^ scroll a whole window, then land on the first non-blank.
        assert_eq!(
            seq_first(resolve(n, "z +").unwrap()),
            Some("scroll_line_below_window")
        );
        assert_eq!(
            seq_first(resolve(n, "z ^").unwrap()),
            Some("scroll_line_above_window")
        );
        // zy/zp/zP drop the trailing padding of a block.
        assert_eq!(
            cmd_name(resolve(n, "z y").unwrap()),
            Some("yank_no_trailing_whitespace")
        );
        assert_eq!(
            cmd_name(resolve(n, "z p").unwrap()),
            Some("paste_after_no_trailing_whitespace")
        );
        assert_eq!(
            cmd_name(resolve(n, "z P").unwrap()),
            Some("paste_before_no_trailing_whitespace")
        );
        // `{count}:` opens the Ex line with the count's range already in it.
        assert_eq!(
            cmd_name(resolve(n, ":").unwrap()),
            Some("command_mode_count")
        );
        // <F1> is vim's <Help>, not the command palette.
        assert_eq!(cmd_name(resolve(n, "F1").unwrap()), Some("help"));
    }

    /// The insert-mode `CTRL-X` sub-modes whose candidate source is not zmax's
    /// LSP+word completion each have their own command now.
    #[test]
    fn vim_insert_completion_submodes() {
        let km = default();
        let i = &km[&Mode::Insert];
        for (chord, cmd) in [
            ("C-x C-l", "complete_line"),
            ("C-x C-f", "complete_filename"),
            ("C-x C-k", "complete_dictionary"),
            ("C-x C-t", "complete_thesaurus"),
            ("C-x C-r", "complete_register_word"),
            ("C-x C-d", "complete_define"),
            ("C-x s", "insert_spell_suggest"),
            ("C-x C-s", "insert_spell_suggest"),
            // i_CTRL-X CTRL-O / CTRL-U drive 'omnifunc' / 'completefunc', and fall
            // back to zmax's own completion when the option is unset.
            ("C-x C-o", "complete_omni_func"),
            ("C-x C-u", "complete_user_func"),
            ("C-x C-n", "completion"),
        ] {
            assert_eq!(
                cmd_name(resolve(i, chord).unwrap_or_else(|| panic!("{chord} did not resolve"))),
                Some(cmd),
                "{chord} must run {cmd}"
            );
        }
        // i_CTRL-SHIFT-V / i_CTRL-SHIFT-Q are the modifyOtherKeys spellings of the
        // insert-literal keys.
        assert_eq!(
            cmd_name(resolve(i, "C-S-v").unwrap()),
            Some("insert_char_interactive")
        );
        assert_eq!(
            cmd_name(resolve(i, "C-S-q").unwrap()),
            Some("insert_char_interactive")
        );
    }

    /// Visual `=` filters through 'equalprg' and visual `gq`/`gw` reflow the lines
    /// (they used to run the LSP formatter, which is what `=` falls back to).
    #[test]
    fn visual_format_keys_are_vim() {
        let km = default();
        let s = &km[&Mode::Select];
        assert_eq!(resolve(s, "=").map(seq_nth1), Some(Some("filter_equalprg")));
        assert_eq!(
            resolve(s, "g q").map(seq_nth1),
            Some(Some("reflow_selections"))
        );
        assert_eq!(
            resolve(s, "g w").map(seq_nth1),
            Some(Some("reflow_selections_keep_cursor"))
        );
        // gq{motion} in Normal now covers the k/{ /gg motions too.
        let n = &km[&Mode::Normal];
        for chord in ["g q k", "g q {", "g q g g", "g w k", "g w {", "g w g g"] {
            assert!(
                matches!(resolve(n, chord), Some(KeyTrie::Sequence(_))),
                "{chord} should reflow"
            );
        }
    }

    /// The second static command of a sequence — the operators in Select mode all
    /// save the visual area first, so the command under test is the one after it.
    fn seq_nth1(trie: &KeyTrie) -> Option<&str> {
        match trie {
            KeyTrie::Sequence(cmds) => match cmds.get(1) {
                Some(MappableCommand::Static { name, .. }) => Some(name),
                _ => None,
            },
            _ => None,
        }
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

    /// The Emacs global map (the Meta and Ctrl-Meta keys outside the `C-x`/`C-c`/
    /// `C-h` prefixes) is part of the shared base, so the shipped spacemacs preset
    /// carries it. Each chord must reach the *faithful* port named by the Emacs
    /// Key Index — the command name is a string resolved at runtime, so a typo
    /// still compiles and only this assertion catches it.
    #[test]
    fn emacs_global_map_bound() {
        let km = base();
        let n = &km[&Mode::Normal];
        for (chord, want) in [
            // Sentences, paragraphs, words, case.
            ("A-a", "move_sentence_backward"),
            ("A-e", "move_sentence_forward"),
            ("A-k", "kill_sentence"),
            ("A-h", "mark_paragraph"),
            ("A-{", "goto_prev_paragraph"),
            ("A-}", "goto_next_paragraph"),
            ("A-c", "capitalize_word"),
            ("A-l", "downcase_word"),
            ("A-u", "upcase_word"),
            ("A-@", "mark_word"),
            ("A-y", "yank_pop"),
            ("A-z", "zap_to_char"),
            ("A-=", "count_selection"),
            ("A-'", "expand_abbrev"),
            ("A-$", "ispell_word"),
            ("A-~", "not_modified"),
            ("A-.", "goto_definition"),
            ("A-?", "xref_find_references"),
            ("A-X", "command_palette"),
            ("A-tab", "completion"),
            ("A-left", "move_prev_word_start"),
            ("A-right", "move_next_word_start"),
            ("A-!", "shell_insert_output"),
            ("A-|", "shell_pipe"),
            ("C-@", "set_mark_command"),
            ("C-S-tab", "goto_previous_tabpage"),
            ("F3", "kmacro_start_macro_or_insert_counter"),
            // C-M-: s-expressions, defuns, lists, the other window.
            ("A-C-f", "forward_sexp"),
            ("A-C-b", "backward_sexp"),
            ("A-C-k", "kill_sexp"),
            ("A-C-t", "transpose_sexp"),
            ("A-C-space", "mark_sexp"),
            ("A-C-@", "mark_sexp"),
            ("A-C-h", "mark_defun"),
            ("A-C-a", "c_beginning_of_defun"),
            ("A-C-e", "c_end_of_defun"),
            ("A-C-d", "down_list"),
            ("A-C-u", "backward_up_list"),
            ("A-C-n", "forward_list"),
            ("A-C-p", "backward_list"),
            ("A-C-v", "scroll_other_window"),
            ("A-C-S-v", "scroll_other_window_down"),
            ("A-C-S-l", "recenter_other_window"),
            ("A-C-q", "prog_indent_sexp"),
            ("A-C-\\", "indent"),
            ("A-C-j", "default_indent_new_line"),
            ("A-C-i", "completion"),
            ("A-C-.", "workspace_symbol_picker"),
            ("A-C-s", "search"),
            ("A-C-r", "rsearch"),
            ("A-C-w", "append_next_kill"),
            // M-g goto map and M-s search map.
            ("A-g g", "goto_line"),
            ("A-g A-g", "goto_line"),
            ("A-g tab", "goto_column"),
            ("A-g n", "run_next_error"),
            ("A-g p", "run_prev_error"),
            ("A-s o", "occur"),
            ("A-s w", "isearch_forward_word"),
            ("A-s .", "isearch_forward_symbol_at_point"),
            ("A-s _", "isearch_forward_symbol"),
            // F2 two-column map (the C-x 6 aliases).
            ("F2 1", "twocol_merge"),
            ("F2 2", "twocol_two_columns"),
            ("F2 b", "twocol_associate_buffer"),
            ("F2 d", "twocol_dissociate"),
            ("F2 s", "twocol_split"),
            ("F2 ret", "twocol_newline"),
        ] {
            let leaf = resolve(n, chord).unwrap_or_else(|| panic!("{chord} is not bound"));
            assert_eq!(cmd_name(leaf), Some(want), "{chord}");
        }
    }

    /// The Emacs global chords whose port is a typable command. Only zero-argument
    /// typables may be bound (a keybinding passes no arguments), so this also pins
    /// that every one of them resolves to the command the Emacs manual names.
    #[test]
    fn emacs_global_typables_bound() {
        let km = base();
        let n = &km[&Mode::Normal];
        for (chord, want) in [
            ("A-t", "transpose-words"),
            ("A-space", "just-one-space"),
            ("A-\\", "delete-horizontal-space"),
            ("A-C-o", "split-line"),
            ("A-s h .", "highlight-symbol-at-point"),
            ("A-s h u", "unhighlight-regexp"),
            ("A-s h f", "hi-lock-find-patterns"),
            ("A-s h w", "hi-lock-write-interactive-patterns"),
        ] {
            let leaf = resolve(n, chord).unwrap_or_else(|| panic!("{chord} is not bound"));
            match leaf {
                KeyTrie::MappableCommand(MappableCommand::Typable { name, args, .. }) => {
                    assert_eq!(name, want, "{chord}");
                    assert!(args.is_empty(), "{chord}: keybound typables take no args");
                }
                other => panic!("{chord} should be a typable command, got {other:?}"),
            }
        }
    }

    /// The emacs global map is *not* vim: the pure `vim` preset must strip every
    /// chord of it, submaps included. A chord left behind would shadow a vim key
    /// (or invent one), which is exactly what `NON_VIM_NORMAL` exists to prevent.
    #[test]
    fn pure_vim_strips_the_emacs_global_map() {
        let km = default();
        let n = &km[&Mode::Normal];
        for chord in [
            "A-a", "A-e", "A-k", "A-h", "A-c", "A-l", "A-u", "A-y", "A-z", "A-t", "A-.", "A-?",
            "A-g", "A-s", "A-C-f", "A-C-b", "A-C-k", "A-C-h", "A-C-v", "C-@", "F2", "F3",
        ] {
            assert!(
                resolve(n, chord).is_none(),
                "vim Normal must not bind the emacs chord {chord}"
            );
        }
        // …while the vim keys the emacs map sits next to survive.
        assert_eq!(cmd_name(resolve(n, "C-a").unwrap()), Some("increment"));
        assert_eq!(cmd_name(resolve(n, "C-x").unwrap()), Some("decrement"));
        assert_eq!(cmd_name(resolve(n, "F1").unwrap()), Some("help"));
    }

    /// Window motion under `[` / `]` (spacemacs `[w` / `]w`). These used to live in
    /// the typable table, where the port report could not see them and a claim of
    /// "ported" could not be checked against the source.
    #[test]
    fn unimpaired_window_motion_bound() {
        let km = base();
        let n = &km[&Mode::Normal];
        assert_eq!(
            cmd_name(resolve(n, "[ w").unwrap()),
            Some("rotate_view_reverse")
        );
        assert_eq!(cmd_name(resolve(n, "] w").unwrap()), Some("rotate_view"));
        // The `[` / `]` submaps keep their existing motions.
        assert_eq!(
            cmd_name(resolve(n, "[ b").unwrap()),
            Some("goto_previous_buffer")
        );
        assert_eq!(
            cmd_name(resolve(n, "] b").unwrap()),
            Some("goto_next_buffer")
        );
    }
}
