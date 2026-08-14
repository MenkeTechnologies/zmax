//! Inline help system — a searchable, scrollable Help browser over **every**
//! command (static + `:`-typable, with their live keybindings) plus curated topic
//! pages. Fuzzy filter on the left, full doc + key + aliases on the right.
//!
//! Open: `SPC h` · `:help` · `?`. Type to search · ↑/↓ or C-n/C-p move ·
//! →/← cycle category · Esc closes.
//!
//! Every list row is a button, so Help mode's `button-buffer-map` keys apply:
//! `TAB` / `S-TAB` are `forward-button` / `backward-button` (they wrap round the
//! ends, unlike ↑/↓), and a click — `mouse-1` via `follow-link`, or `mouse-2` —
//! is `push-button`.
//!
//! `RET` visits the entry at point — the cross-reference follow (`help-follow`)
//! that pushes onto the help history. While a single entry is displayed (the
//! read-only `*Help*` buffer), Emacs's Help-mode keys are live: `l` / `C-c C-b`
//! go back, `r` / `C-c C-f` go forward, `n` / `p` scroll to the next / previous
//! page of the topic, and `s` (`help-view-source`) jumps to the source that
//! defines the command. Any other character leaves the topic and searches again.

use std::collections::HashMap;
use std::path::PathBuf;

use tui::buffer::Buffer as Surface;
use zmax_core::Selection;
use zmax_view::{
    editor::Action,
    graphics::Rect,
    input::{KeyCode, KeyEvent, MouseButton, MouseEventKind},
};

use crate::{
    commands::MappableCommand,
    compositor::{Component, Compositor, Context, Event, EventResult},
    ctrl, key, shift,
};

#[derive(Clone, Copy, PartialEq)]
enum Cat {
    All,
    Commands,
    Keys,
    Topics,
}
const CATS: [(Cat, &str); 4] = [
    (Cat::All, "All"),
    (Cat::Commands, "Commands"),
    (Cat::Keys, "Keybindings"),
    (Cat::Topics, "Topics"),
];

struct Entry {
    cat: Cat,
    title: String,
    keys: Vec<String>,
    aliases: Vec<String>,
    doc: String,
}

/// command name → ["normal: d d", "select: x", …]
fn key_index() -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let km = crate::keymap::default();
    for (mode, trie) in &km {
        let short = match *mode {
            zmax_view::document::Mode::Normal => "n",
            zmax_view::document::Mode::Select => "v",
            zmax_view::document::Mode::Insert => "i",
        };
        for (cmd, chords) in trie.reverse_map() {
            let e = out.entry(cmd).or_default();
            for chord in chords {
                let s = chord
                    .iter()
                    .map(|k| k.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                e.push(format!("{short}: {s}"));
            }
        }
    }
    for v in out.values_mut() {
        v.sort();
        v.dedup();
    }
    out
}

const TOPICS: &[(&str, &str)] = &[
    (
        "Welcome to zmax",
        "zmax is a hackable modal editor with a full IDE shell (project tree, structure, \
      problems, run window, git, minimap) and a vim-faithful keymap.\n\n\
      • Eight keymap presets: spacemacs (default), vim, helix, kakoune, micro,\n\
        nano, emacs, cua.  Switch with :keymap <name> or in Preferences ▸ Keymap.\n\
      • Press SPC (space) for the leader menu; press C-x for the emacs prefix.\n\
      • SPC , opens Preferences (Settings, Keymap, Color Scheme, Run Configs).\n\
      • SPC h opens this Help.\n\
      • : opens the command line; SPC SPC is the command palette (M-x).",
    ),
    (
        "Editing modes",
        "Normal — move and operate (h j k l, w b e, d c y, etc.).\n\
      Insert — type text; i a o, I A O, cw, s.  Esc / C-c returns to Normal.\n\
      Visual (Select) — v / V / C-v to select, then an operator (d y c > <).\n\
      gv reselects the last visual area.  Counts: 3dd, 5j, etc.",
    ),
    (
        "Search & replace",
        "/  search forward, ?  search backward, n / N  next / previous.\n\
      *  / #  search the word under the cursor.\n\
      :%s/old/new/g  substitute in the file;  &  repeats the last substitute.\n\
      :replace-word NEW  global-replace the word under the cursor with NEW.\n\
      gn selects the next match (then c / d operates on it).",
    ),
    (
        "Windows, splits & buffers",
        "C-w s / C-w v  horizontal / vertical split.  C-w h/j/k/l move between them.\n\
      C-w > / C-w <  resize width;  C-w + / C-w -  resize height;  C-w =  equalize.\n\
      C-w q  close;  C-w o  only.  ]b / [b  next / previous buffer.",
    ),
    (
        "Folds",
        "za toggle, zo open, zc close, zR open all, zM close all.\n\
      zj / zk move between folds.  zf{motion} creates a fold.",
    ),
    (
        "Spell checking",
        "]s / [s  next / previous misspelled word.\n\
      z=  suggestions (press a number to apply).\n\
      zg  mark good, zw  mark wrong, zug / zuw  undo.  Uses the system dictionary.",
    ),
    (
        "Run configurations",
        "SPC R c  opens the Run/Debug Configurations manager (add/edit/delete named \
      configs).  SPC R r  runs the active config.  The ▶ Run toolbar button runs it too. \
      Configs persist to <workspace>/.zmax/run-configs.toml.",
    ),
    (
        "Preferences & settings",
        "SPC ,  opens the unified Preferences window:\n\
      • Settings — every editor option, searchable, applied live (no restart);\n\
        C-x C-s saves the session's changes to config.toml.\n\
      • Keymap — add/edit your own [keys.*] bindings.\n\
      • Color Scheme — edit theme colors and save a custom theme.\n\
      • Run Configs — manage run configurations.\n\
      Ctrl-Tab cycles tabs.  Edits live-reload immediately.",
    ),
    (
        "Digraphs & special insert",
        "In insert mode: C-k {c1}{c2} inserts a digraph (e.g. C-k a' → á, C-k -> → →).\n\
      C-v / C-q insert the next key literally.  C-r inserts a register.\n\
      C-e / C-y copy the character below / above the cursor.",
    ),
    (
        "The leader (SPC) menu",
        "In the spacemacs keymap (default) SPC is the leader.  A which-key popup shows\n\
      the next keys.  SPC f  files, SPC b  buffers, SPC s  search, SPC g  git,\n\
      SPC p  project, SPC w  windows, SPC R  run, SPC ,  preferences, SPC h  help.\n\
      (The pure vim keymap has no SPC leader and shows no which-key popup.)",
    ),
    (
        "The C-x prefix (emacs/spacemacs)",
        "In the spacemacs (default) and emacs keymaps, C-x is the Emacs command prefix\n\
      and opens a which-key popup.  C-x C-s save, C-x C-f find-file, C-x b buffer,\n\
      C-x k kill-buffer, C-x o other-window, C-x 0/1/2/3 windows, C-x r registers/\n\
      rectangles/bookmarks, C-x C-c quit.  (In the vim keymap C-x is decrement.)",
    ),
    (
        "Preferences (SPC ,)",
        "A full-screen tabbed page — Ctrl-Tab cycles tabs, Esc closes, everything is\n\
      mouse + keyboard and applies live (no restart):\n\
      • Settings — every editor option, searchable.\n\
      • Keymap — your [keys.*] overrides + a browse-all-bindings reference.\n\
      • Color Scheme — theme picker + per-scope color/style editor.\n\
      • Run Configs — named run/debug configurations.\n\
      • Help — this browser.",
    ),
    (
        "Settings tab",
        "Every [editor] option is listed automatically (so nothing is ever missing),\n\
      grouped into sections and searchable with /.\n\
      • Booleans toggle with Space/⏎/click.\n\
      • Enums (line-number, cursor-shape, …) cycle through valid values.\n\
      • Numbers/strings are typed; arrays edit as a TOML literal.\n\
      • Setting a value applies it live for THIS SESSION and marks it * (unsaved).\n\
        C-x C-s (Custom-save, the [Apply and Save] button) writes every such value\n\
        to config.toml; C-c C-c (Custom-set, [Apply]) sets without saving.\n\
      • ● marks a value changed from its default; press r to reset it.\n\
      • Tab / S-Tab step over the buttons and fields; M-TAB completes a value.\n\
      • o opens the raw config.toml.",
    ),
    (
        "Theme studio (Color Scheme)",
        "Left pane: every installed theme — ⏎ enables it for this session (● = active);\n\
      C-x C-s (or [Save Theme Settings]) saves that choice to config.toml.\n\
      Right pane: per-scope editor.  f / b switch foreground / background,\n\
      type a #rrggbb hex; 1/2/3 toggle bold / italic / dim.  A live preview row\n\
      shows a sample styled with your edits.  n names the theme, s saves it to\n\
      ~/.zmax/themes/<name>.toml and selects it in the picker.",
    ),
    (
        "Keymap editor",
        "Tab toggles between your overrides and a searchable list of ALL bindings.\n\
      In overrides: a add, d delete, e/⏎ edit (mode · chord · command).\n\
      ⌨ Capture key records a chord by pressing the actual keys (e.g. Ctrl-W H).\n\
      Saves to [keys.<mode>] in config.toml and reloads live.",
    ),
    (
        "Run configurations",
        "SPC R c opens the manager: a add, c copy, d delete, e edit, r run.\n\
      Each config has a name, command, working dir, and KEY=VAL env.\n\
      The active one runs from the ▶ toolbar button or SPC R r, and shows in\n\
      the Run tool window.  Stored in <workspace>/.zmax/run-configs.toml.",
    ),
    (
        "Marks & jumps",
        "m{a-z} sets a mark, `{a-z} / '{a-z} jumps to it.  `` / '' return to the\n\
      previous jump.  C-o / C-i move back / forward in the jumplist.\n\
      gd goto definition, gr references, gi goto implementation (LSP).",
    ),
    (
        "Macros & registers",
        "q{reg} records a macro, q stops, @{reg} replays, @@ repeats.\n\
      \"{reg} selects a register before y/d/p.  C-r {reg} pastes it in insert.\n\
      The Registers (LOTR) tool window shows every register live.",
    ),
    (
        "Text objects & operators",
        "Operators d c y > < =  combine with motions and text objects:\n\
      diw / ciw word, di( / ci\" inside pair, dap paragraph, dat tag.\n\
      i = inside, a = around.  Counts repeat: 2daw, 3dd.  . repeats the change.",
    ),
    (
        "Keymap presets",
        "zmax ships eight presets; :keymap <name> switches at runtime, keymap = \"name\"\n\
      in config.toml picks the one you start with.\n\n\
      Modal:    spacemacs (default, vim keys + SPC leader + C-x), vim (pure),\n\
                helix (selection → action), kakoune (helix keys placed where\n\
                kakoune puts them: v/V view, A-i/A-a objects, Z/z/A-z registers).\n\
      Modeless: emacs, cua (emacs + C-x cut / C-c copy / C-v paste),\n\
                micro (C-s save, C-q quit, C-e command bar),\n\
                nano (^O write out, ^W where is, ^K cut, ^U paste, ^X exit).",
    ),
    (
        "Multiple selections",
        "A cursor is a one-character selection, and most commands act on every one.\n\n\
      C / A-C     add a cursor on the next / previous line\n\
      s           select every regex match inside the selections\n\
      S           split the selections on a regex\n\
      A-k / A-K   keep / drop the selections matching a regex\n\
      ,           reduce to the primary selection (space in the kakoune preset)\n\
      A-,         drop the primary selection\n\
      )  (        rotate which selection is primary; A-) A-( rotate the contents",
    ),
    (
        "Structural regular expressions",
        "sam and vis's x/y loops, over selections instead of a loop.\n\n\
      :sx /re/ command          run the command over every match of re\n\
      :structural-y /re/ cmd    …over the stretches *between* matches\n\
      :sX /name-re/ command     run it in every file whose name matches\n\
      :structural-Y /re/ cmd    …in every file whose name does not\n\n\
      With no command the pieces are simply selected, which is how the loops get\n\
      used interactively.  The pattern is ERE (\\w+ is a word), not vim magic.",
    ),
    (
        "Selection registers & history",
        "kakoune's selections-as-data, in the kakoune preset (and by command name in\n\
      any preset).\n\n\
      Z / z       save the selections into a register / restore them\n\
      A-z         combine the register's selections with the current ones:\n\
                  a append · u union · i intersection · < leftmost · > rightmost\n\
                  + longest · - shortest\n\
      A-u / A-U   undo / redo a *selection* change, leaving the text alone\n\
      A-S         keep the first and last character of each selection\n\
      A-& copy the main selection's indent onto the other selected lines",
    ),
    (
        "Bookmarks & marks",
        "Numbered bookmarks (ne / mcedit): ten per document, holding a line.\n\
      Bind or run set_numbered_bookmark, then press 0–9; goto_numbered_bookmark\n\
      returns to one and pushes the jumplist, so C-o comes back.\n\n\
      vim marks: m{a-z} sets, '{a-z} jumps, :marks lists, :delmarks removes.\n\
      Emacs bookmarks are the named, saved kind — see the bookmark_* commands.",
    ),
    (
        "Key mapping at runtime",
        ":map / :nmap / :imap / :vmap / :xmap / :smap / :omap and their noremap forms\n\
      bind a key without restarting; :unmap and the mapclear family remove them.\n\
      A map command with no right-hand side lists what is bound.\n\n\
      :cmap  binds a key on the command line\n\
      :lmap  binds a language ('keymap') key — :set keymap=<name> fills the same table\n\
      :tmap  binds a key in the terminal panel\n\n\
      Startup bindings live under [keys.<mode>] in config.toml, or in\n\
      Preferences ▸ Keymap.",
    ),
    (
        "Menus",
        "vim's menu family is here: :menu File.Save :write<CR> defines an item,\n\
      :emenu File.Save runs one by name, :popup File opens a subtree at the\n\
      cursor, and :unmenu removes (\":unmenu *\" removes everything).  The\n\
      mode-prefixed forms (:nmenu, :imenu, :amenu …) and :tmenu tooltips work\n\
      as vim documents them.",
    ),
    (
        "Language servers",
        "Diagnostics, completion, hover and navigation come from the language server\n\
      for the buffer's language — no plugin needed.\n\n\
      gd goto definition · gr references · gi implementation · gy type definition\n\
      K  hover · SPC k signature help · SPC a code actions · SPC r rename\n\
      SPC d document diagnostics · SPC D workspace diagnostics\n\
      :lsp-restart restarts a server, :lsp-stop stops one.",
    ),
    (
        "Debugging",
        "A debug adapter (DAP) session runs inside the editor.  :debug-start runs a\n\
      template with its parameters and :debug-remote attaches to an adapter over\n\
      TCP; :debug-eval evaluates an expression in the stopped frame.\n\n\
      F9 (SPC d b) toggles a breakpoint and SPC d B opens the breakpoint picker;\n\
      S-F5 (SPC d d) launches and SPC d c continues.  The IDE workbench's Debug\n\
      tab shows frames, variables and the console, and run/debug configurations\n\
      are edited in Preferences ▸ Run Configs.\n\n\
      :debug on its own is vim's script debugger, not the adapter.",
    ),
    (
        "Git",
        ":magit opens the magit-style status buffer — stage, unstage, commit, push,\n\
      pull, stash and branch from one place.  :blame annotates the current line,\n\
      :diff shows the working-tree diff, and the gutter marks added, changed and\n\
      removed lines as you type.  The IDE workbench has a Git tab with the same\n\
      status list.",
    ),
    (
        "Shell & terminal",
        ":terminal opens a real PTY inside the editor.\n\n\
      |    pipe each selection through a command and replace it with the output\n\
      A-|  pipe each selection and ignore the output\n\
      !    insert a command's output before the selection; A-! appends it\n\
      $    keep the selections a command exits zero on\n\
      :run-shell-command runs one without touching the buffer.",
    ),
    (
        "Embedded scripting",
        "Twelve interpreters are compiled in — no external runtime:\n\
      :elisp :vim :awk :zsh :stryke :ruby :php :python :node :arb :tcl :rlang\n\n\
      Each keeps state between calls, and :repl opens a panel fronting all of\n\
      them.  Selections can be filtered through a script the same way they can\n\
      through a shell command.",
    ),
    (
        "Pickers & fuzzy finding",
        "SPC f files · SPC b buffers · SPC / grep · SPC s symbols · SPC ? commands.\n\n\
      The fzf.vim command surface is here too: :Files, :Buffers, :Rg, :Lines,\n\
      :BLines, :Tags, :BTags, :Commits, :Maps, :Helptags, :Snippets.  Type to\n\
      filter, Enter opens, and the preview pane follows the highlighted row.",
    ),
    (
        "Snippets",
        "Snippets live in ~/.zmax/snippets.toml as trigger, scope and body.  Type a\n\
      trigger and expand it in insert mode, or pick one with :Snippets.  A body\n\
      may carry tabstops (${1:name}, $0) and Tab moves between them once expanded.\n\
      Language servers' own snippets arrive through completion.",
    ),
    (
        "Formatting & linting",
        "= reindents the selection and :format runs the language server's formatter.\n\
      :lint runs an external checker over the file — set it with\n\
      :set linter=<program> (% is replaced by the file's path) — and its messages\n\
      land in the location list, so :lnext and :lopen walk them.\n\
      :make builds and fills the compilation list; :cnext walks that one.",
    ),
    (
        "The IDE workbench",
        ":ide opens the workbench: project tree, structure outline, problems, run\n\
      console, git, debug, registers, TODO, marks, jumps, recent files and a\n\
      minimap stripe, with a powerline status bar across the bottom.\n\
      F2 toggles it, Tab cycles focus, Esc returns to the editor.",
    ),
    (
        "Undo, redo & history",
        "u undoes, U (or C-r in the vim presets) redoes.  :undotree opens the tree of\n\
      states — undo in zmax is a tree, so nothing is lost by undoing and typing.\n\
      :earlier and :later move through the file's states by count or by time,\n\
      and the history survives a restart when persistent undo is on.",
    ),
    (
        "Tabs, splits & the dashboard",
        ":tabnew opens a tab, :tabclose closes one, and the tabline across the top\n\
      shows what is open.  Splits: C-w s / C-w v, C-w hjkl to move, C-w q to\n\
      close.  Preferences ▸ Dashboard is a live system view — CPU, memory,\n\
      processes, network, disks — rendered in the editor.",
    ),
    (
        "Packages",
        ":zmax-native add owner/repo installs a compiled native plugin into\n\
      ~/.zmax/pkg and loads it without recompiling the editor; get, sync,\n\
      remove, update, registry, info and gc manage the rest.  Plugins are\n\
      SHA-256 pinned in installed.toml.",
    ),
    (
        "Learning zmax",
        ":tutor opens the built-in tutorial.  SPC h is this Help, and the Topics list\n\
      you are reading indexes it.  Every command has a one-line description in\n\
      the Commands and Static commands sections here, with the keys that run it.\n\
      The book (book/src) covers the same ground in long form.",
    ),
    (
        "Case & notation",
        "Rewrites of the selection, one line or one word at a time.\n\n\
      :to-snake :to-kebab :to-camel :to-pascal :to-constant   identifier case\n\
      :to-ascii        fold accents and smart punctuation down to ASCII\n\
      :to-binary       numbers to base 2; :dec-to-hex and :hex-to-dec convert\n\
                       between decimal and hex\n\
      :to-fixed N      format each numeric line to N decimal places\n\
      :to-env-export   prefix each KEY=value line with 'export '\n\
      :to-html-list   wrap the selected lines in an HTML <ul>",
    ),
    (
        "Sorting text",
        "Every one of these acts on the selected lines and replaces them in place.\n\n\
      :sort-lines         plain lexicographic sort\n\
      :sort-words         sort the words within the selection\n\
      :sort-by-length     shortest line first\n\
      :sort-by-field N    sort on the Nth whitespace field (default 1)\n\
      :sort-numeric-fields  numeric sort on a field\n\
      :sort-columns       sort by a column range rather than a field\n\
      :sort-paragraphs :sort-pages  sort larger blocks, not lines\n\n\
      :sort takes the flags, so it is the one to reach for when the rest do not\n\
      fit.  :uniquify-lines drops duplicates, :uniq-count collapses them to counts by\n\
      frequency, and :reverse turns the order round.",
    ),
    (
        "Pulling things out of text",
        "The extract family scans the selection and leaves one match per line.\n\n\
      :extract-urls :extract-emails :extract-ips :extract-numbers\n\
      :extract-quoted            everything inside quotes\n\
      :extract-between A B       the substrings between two delimiters\n\
      :extract <regex>           the general form; capture groups are kept\n\n\
      For finding rather than rewriting, :multi-occur lists matches for a\n\
      regexp across every buffer whose name matches a second regexp.",
    ),
    (
        "Cleaning text up",
        ":strip-invisible          drop zero-width and invisible Unicode\n\
      :strip-line-numbers      remove pasted line numbers\n\
      :strip-list-markers      remove bullet / numbered list markers\n\
      :strip-markdown-links    keep the link text, drop the target\n\
      :strip-emphasis          remove markdown * and _ emphasis\n\
      :strip-html-comments     remove <!-- … -->\n\
      :strip-export            remove 'export ' from shell assignments\n\n\
      :delete-trailing-whitespace, :delete-blank-lines and\n\
      :delete-horizontal-space handle whitespace itself.",
    ),
    (
        "JSON",
        "A whole toolkit that works on the selection, so it composes with multiple\n\
      selections and with the shell pipe.\n\n\
      :json-query users.0.name    replace the JSON with the value at a dot-path\n\
      :json-flatten               flatten to greppable 'path = value' lines\n\
      :json-pick / :json-omit     keep or drop fields\n\
      :json-group-by city         group an array of objects by a field\n\
      :json-sort :json-unique :json-keys :json-type :json-pluck\n\
      :json-to-csv :json-to-toml :json-to-kv :json-to-lines\n\
      :jsonl-to-json / :json-to-jsonl   between JSON and JSON Lines\n\
      :json-validate              parse and report the first error\n\
      :json-table                 render an array of objects as a table",
    ),
    (
        "CSV & TSV",
        ":csv-column N        keep only the Nth column (1-based)\n\
      :csv-validate        check every row has the same field count\n\
      :csv-to-tsv / :tsv-to-csv   change the delimiter\n\
      :csv-to-html-table   first row becomes the header of an HTML table\n\
      :lines-to-csv-row    join the selected lines into one RFC-4180 row\n\
      :csv-row-to-lines    the inverse\n\
      :json-to-csv         and back through :json-query for the other direction",
    ),
    (
        "Numbers in a buffer",
        ":calc <expr>          evaluate arithmetic, or every selection in place\n\
      :percent-of-total     each numeric line as a percentage of the column total\n\
      :diff-lines           each numeric line replaced by its delta from the last\n\
      :running-total        the inverse of that\n\
      :hexdump              render the selection as an xxd-style dump\n\
      :hex                  the same in one direction only\n\
      :rectangle-number-lines   number the lines of a rectangle, from N",
    ),
    (
        "Rectangles",
        "Column-shaped edits, as emacs's C-x r family does them.\n\n\
      :string-rectangle          replace the rectangle's column span on every\n\
                                 line with a string\n\
      :string-insert-rectangle   insert it instead of replacing\n\
      :rectangle-number-lines    number the lines, optionally from N with a\n\
                                 format string\n\n\
      Multiple cursors reach the same result: C on each line, then type.",
    ),
    (
        "Abbreviations",
        "Typed-word expansion, from both lineages.\n\n\
      :iabbrev lhs rhs        vim's insert-mode abbreviation (:cabbrev for the\n\
                              command line, :noreabbrev / :inoreabbrev for the\n\
                              non-recursive forms)\n\
      :unabbreviate           remove one; :abbreviate lists them\n\
      :define-global-abbrev NAME EXPANSION   emacs's global table\n\
      :define-mode-abbrev     the same, for this language only\n\
      :abbrev-mode on|off     toggle expansion on a word separator\n\
      :list-abbrevs :write-abbrev-file :read-abbrev-file :kill-all-abbrevs",
    ),
    (
        "Comment boxes",
        ":rebox redraws the selection as a comment box in the buffer's comment\n\
      syntax; a style number picks the box.  :rebox-next and :rebox-prev cycle\n\
      through the styles, :rebox-unbox takes the box away again, and\n\
      :rebox-left / :rebox-center / :rebox-right set how the text sits inside.",
    ),
    (
        "Highlighting",
        "Highlights that stay until removed, over and above the syntax colours.\n\n\
      :highlight-regexp <re>              add a persistent highlight\n\
      :highlight-phrase                   the same, tolerant of line breaks\n\
      :highlight-lines-matching-regexp    highlight whole lines\n\
      :highlight-symbol-at-point          every whole-word use of this symbol\n\
      :highlight-changes-mode             mark what has changed since opening\n\n\
      :highlight defines a theme face directly, which is what the highlights\n\
      above and the theme studio both write to.",
    ),
    (
        "Fill, margins & justification",
        ":set-fill-column N        wrap width (the cursor's column when N is left off)\n\
      :set-left-margin / :set-right-margin\n\
      :set-justification-left / -right / -center / -full / -none\n\
      :fill-individual-paragraphs   fill each paragraph separately, splitting on\n\
                                    indentation changes\n\
      :fill-nonuniform-paragraphs   the same, splitting only on blank lines\n\n\
      :set-face-foreground and :set-face-background change a face for the\n\
      session without editing the theme.",
    ),
    (
        "Org mode",
        "Outlines, TODOs and an agenda over .org files.\n\n\
      :org-cycle              fold or unfold this heading's subtree\n\
      :org-fold-all / :org-unfold-all\n\
      :org-todo               cycle none -> TODO -> DONE\n\
      :org-priority           set the [#A] priority cookie\n\
      :org-promote / :org-demote          change heading level\n\
      :org-move-subtree-up / -down        move a whole subtree\n\
      :org-schedule / :org-deadline       stamp a date on the heading\n\
      :org-next-heading / :org-prev-heading\n\
      :org-agenda             TODOs across open buffers and *.org files here,\n\
                              grouped by scheduled and deadline date\n\
      :org-capture            append a '* TODO <text>' line to inbox.org\n\
      :org-agenda-file-to-front / :org-remove-file   manage the agenda list\n\
      :org-export             export the buffer to Markdown",
    ),
    (
        "Images",
        ":image-mode draws the current image file in the terminal.\n\n\
      :image-rotate :image-flip-horizontally :image-flip-vertically\n\
      :image-increase-size / :image-decrease-size\n\
      :image-transform-fit-to-window :image-transform-set-scale\n\
      :image-transform-set-percent :image-transform-reset-to-original\n\
      :image-next-file / :image-previous-file   walk the directory\n\n\
      Animations: :image-next-frame :image-goto-frame :image-increase-speed\n\
      :image-reverse-speed :image-reset-speed.  :image-save writes the result.",
    ),
    (
        "Documents",
        ":doc-view-mode renders a PDF, PostScript or DjVu page in the terminal.\n\n\
      :doc-view-next-page / -previous-page / -first-page / -last-page\n\
      :doc-view-goto-page N\n\
      :doc-view-enlarge / :doc-view-shrink\n\
      :doc-view-set-slice X Y W H    crop the page; :doc-view-reset-slice undoes\n\
      :doc-view-search               search the extracted text\n\
      :doc-view-open-text            open that text as a buffer instead",
    ),
    (
        "The web",
        ":eww <url> fetches a page and renders the HTML to text in a buffer;\n\
      :eww-open-file does the same for a local file and :eww-search-words\n\
      searches with the configured engine.\n\n\
      :browse-url hands a URL to the system browser instead, and\n\
      :xwidget-webkit-browse-url opens a real WebKit view where one is\n\
      available (:xwidget-webkit-browse-history walks it).\n\n\
      :quickurl-add name url stores a URL, :quickurl recalls one by name and\n\
      :quickurl-list shows the table.",
    ),
    (
        "Feeds, mail & news",
        ":elfeed reads the RSS/Atom feeds listed in the elfeed feed file;\n\
      :elfeed-add <url> [tags] adds one and :elfeed-feeds lists them.\n\n\
      :compose-mail [to] [subject] opens a draft in message mode, where\n\
      :message-goto-to :message-goto-subject :message-goto-cc :message-goto-bcc\n\
      :message-goto-body and :message-insert-signature move around it, and\n\
      :message-send / :message-send-and-exit deliver it.\n\n\
      :gnus [server] opens the newsreader against an NNTP host or a spool.",
    ),
    (
        "Chat",
        ":irc-connect <host[:port]> <nick> registers with an IRC server;\n\
      :irc-join joins a channel, :irc-say <target> <text> sends a message,\n\
      :irc-view shows the traffic and :irc-quit disconnects.\n\n\
      :slack-start [token] authenticates (or reads $SLACK_TOKEN),\n\
      :slack-select-rooms picks the channels, :slack-buffer opens one,\n\
      :slack-message posts and :slack-quit ends the session.",
    ),
    (
        "Version control beyond git",
        "The perforce commands shell out to p4: :p4-edit :p4-add :p4-delete\n\
      :p4-revert :p4-submit :p4-shelve :p4-unshelve :p4-sync :p4-opened\n\
      :p4-changes :p4-describe :p4-filelog :p4-diff :p4-blame :p4-resolve\n\
      :p4-reconcile :p4-branches :p4-clients :p4-labels :p4-jobs :p4-users\n\
      :p4-print :p4-where :p4-info, and :p4 runs anything else.\n\n\
      :vc-root-version-diff diffs the tree against a revision for whichever\n\
      backend the project uses.",
    ),
    (
        "Diffing buffers",
        ":diffthis shows the buffer's changes side by side against git HEAD, and\n\
      :diffsplit opens another file beside it in diff mode.\n\
      :diffupdate recomputes, :diffoff leaves diff mode.\n\n\
      :diffput writes the hunk under the selection into the diff base, and\n\
      :diffget (:reset-diff-change) resets the change under the cursor, so a\n\
      difference can be settled by moving hunks rather than editing text.\n\
      :diff-buffer-with-file compares what is on screen with what is on disk,\n\
      and :diffpatch applies a patch file to the buffer.",
    ),
    (
        "Compiling & the error list",
        ":compile <command> runs it and collects the errors; :recompile repeats the\n\
      last one.  :make and :lmake do the same through the make program, filling\n\
      the quickfix and location lists respectively.\n\n\
      :cnext :cprevious :copen :cclose walk and show the quickfix list;\n\
      :lnext :lprevious :lopen :lclose do it for the location list.\n\
      :caddbuffer and :laddbuffer read errors out of a buffer you already have.",
    ),
    (
        "Tags",
        ":regenerate-tags rebuilds the project's TAGS file with ctags -Re and visits\n\
      it.  :tags shows the tag stack, :Tags and :BTags pick a tag with the fuzzy\n\
      finder (project-wide and buffer-local), and :Helptags does the same for\n\
      help topics.\n\n\
      Language servers cover the same ground without an index — see the\n\
      Language servers topic — but tags still work where no server exists.",
    ),
    (
        "Embedded & remote hardware",
        "PlatformIO: :pio-init scaffolds a project for a board, :pio-build compiles\n\
      it into the compilation list, :pio-upload flashes it, :pio-lib-install\n\
      adds a library and :pio-monitor opens the serial monitor.  The :pio-remote-* commands drive boards attached to another\n\
      machine.  :pio passes anything else straight through.\n\n\
      Arduino: :arduino-compile builds the sketch and :arduino-upload flashes\n\
      it, both live in a terminal panel; the arduino-profile-* commands manage\n\
      build profiles.\n\n\
      :serial-term <port> [speed] opens a plain serial terminal on any device.",
    ),
    (
        ".NET",
        ":dotnet-build :dotnet-clean :dotnet-restore :dotnet-publish :dotnet-test\n\
      run the SDK and collect errors into the compilation list.\n\
      :dotnet-run and :dotnet-run-with-args run the project.\n\
      :dotnet-add-package / :dotnet-add-reference change the project file,\n\
      :dotnet-new scaffolds, and :dotnet-sln-new / -add / -remove / -list keep\n\
      the solution in order.  :dotnet-goto-sln :dotnet-goto-csproj\n\
      :dotnet-goto-fsproj open those files without searching for them.",
    ),
    (
        "Machines & services",
        ":vagrant-up :vagrant-halt :vagrant-suspend :vagrant-resume :vagrant-reload\n\
      :vagrant-provision :vagrant-status :vagrant-ssh :vagrant-destroy drive a\n\
      Vagrant box from the editor.\n\n\
      :prodigy lists the services declared in prodigy.json with their state;\n\
      :prodigy-start :prodigy-stop :prodigy-restart :prodigy-browse manage them.\n\n\
      :zwire-host :zwire-sysinfo :zwire-hostinfo :zwire-exec :zwire-job\n\
      :zwire-jobs :zwire-job-output reach machines over zwire, and :zwire-crawl\n\
      inserts matching paths from a remote crawl at the cursor.",
    ),
    (
        "Torrents",
        ":transmission lists what a transmission-daemon is carrying, over its RPC\n\
      API.  :transmission-add takes a magnet link, URL or file path.\n\
      :transmission-start / -stop / -verify act on a torrent,\n\
      :transmission-remove and :transmission-remove-delete take it away (with\n\
      or without the data), :transmission-move relocates it, and\n\
      :transmission-files / :transmission-peers show the detail.\n\
      :transmission-limit-down :transmission-limit-up :transmission-turtle\n\
      control the speed.",
    ),
    (
        "Passwords & encryption",
        ":pass-list shows the password-store tree; :pass-show prints an entry and\n\
      :pass-copy puts it on the clipboard.  :pass-generate makes a new one,\n\
      :pass-insert and :pass-edit change one, :pass-rename and :pass-remove\n\
      move it, and :pass-init sets the store up.\n\
      :pass-otp copies a one-time token; :pass-otp-uri and :pass-otp-insert\n\
      add the secret.\n\n\
      :encrypt encrypts the selection (or the buffer) with an age passphrase,\n\
      replacing it in place with ASCII-armored ciphertext.",
    ),
    (
        "Notes",
        ":geeknote-find searches Evernote through the geeknote CLI,\n\
      :geeknote-show opens a note, :geeknote-create writes one,\n\
      :geeknote-move files it elsewhere, :geeknote-remove deletes it and\n\
      :geeknote-notebooks lists the notebooks.\n\n\
      For plain-text notes, org capture (see the Org mode topic) appends to an\n\
      inbox file without leaving the buffer you are in.",
    ),
    (
        "Music & sound",
        ":alda-server-start starts the Alda server; :alda-play-buffer\n\
      :alda-play-region :alda-play-block :alda-play-line play what you have\n\
      written, and :alda-server-status checks on it.\n\n\
      :extempore-connect attaches to a running Extempore process over TCP, then\n\
      :extempore-send-definition :extempore-send-region :extempore-send-buffer\n\
      evaluate live.  :tidal-run and :tidal-run-orbit do the same for TidalCycles\n\
      (:tidal-hush stops everything).\n\n\
      Players: :spotify-play-pause :spotify-next :spotify-search-track and the\n\
      rest, and :pianobar-play-pause :pianobar-next :pianobar-love\n\
      :pianobar-station.",
    ),
    (
        "Numerical & statistical work",
        ":octave-eval evaluates Octave code in a batch interpreter;\n\
      :octave-run-file runs a script, :octave-send-buffer :octave-send-region\n\
      :octave-send-line push what is on screen through it, and :octave-help /\n\
      :octave-lookfor search the documentation.\n\n\
      :rlang evaluates R, and :calc handles arithmetic without leaving the\n\
      buffer.  See Embedded scripting for the twelve compiled-in interpreters.",
    ),
    (
        "Sessions & desktops",
        ":desktop-save records the file-visiting buffers and the cursor positions;\n\
      :desktop-read reopens them, :desktop-revert rereads the file,\n\
      :desktop-change-dir switches to another one and :desktop-clear closes the\n\
      lot.\n\n\
      Filesets are the named-group version: :filesets-define-pattern defines\n\
      one as a regexp over a directory, :filesets-add-buffer and\n\
      :filesets-remove-buffer maintain it by hand, :filesets-open visits every\n\
      file in it, :filesets-close closes them, :filesets-run-cmd runs a command\n\
      over each and :filesets-list shows what exists.",
    ),
    (
        "Finding files fast",
        ":file-cache-add-directory adds a directory's file names to the cache and\n\
      :file-cache-add-directory-using-find walks a tree into it;\n\
      :file-cache-add-file adds one, :file-cache-display shows the cache and\n\
      :file-cache-clear-cache empties it.  Cached names complete anywhere a\n\
      file name is asked for.\n\n\
      :oldfiles picks from the files edited lately and :RecentLocations walks the\n\
      jump ring newest-first with context, while the file picker (SPC f)\n\
      searches the working tree directly.",
    ),
    (
        "Mirrored files",
        "Shadow copies keep a file in step across machines or directories.\n\
      :shadow-define-cluster names a site, :shadow-define-literal-group ties\n\
      specific files together and :shadow-define-regexp-group does it by\n\
      pattern.  :shadow-shadows lists what is pending, :shadow-copy-files\n\
      writes the copies and :shadow-cancel drops them.\n\
      :shadow-initialize turns the whole mechanism on.",
    ),
    (
        "Self-documentation",
        ":apropos <re> lists commands and config variables matching a regexp;\n\
      :apropos-command narrows it to commands, :apropos-variable and\n\
      :apropos-user-option to settings, :apropos-value searches what those\n\
      settings currently hold, and :apropos-documentation searches the\n\
      descriptions rather than the names.\n\n\
      This Help panel indexes the same data, and :help <name> jumps straight to\n\
      an entry.",
    ),
    (
        "Diversions",
        ":xkcd fetches a strip, draws it in the terminal and prints its alt text;\n\
      :xkcd-random :xkcd-next :xkcd-prev walk the archive, :xkcd-open opens the\n\
      page and :xkcd-explain opens the explainer wiki.\n\n\
      :tutor is the more useful place to spend the same five minutes.",
    ),
    (
        "Safety & measurement",
        ":sandbox <cmd> runs a command with shelling out and file writes refused,\n\
      which is what to use for something pasted in from elsewhere.\n\n\
      :profile start / func / file / pause / stop / dump measures where time\n\
      goes in scripts and functions, as vim's profiler does.\n\
      :browse <cmd> runs a command that wants a file name and picks the file\n\
      with the file picker.",
    ),
    (
        "Text statistics",
        "Measurements that report rather than rewrite, so they are safe to run on\n\
      anything.\n\n\
      :wc            line, word and character counts, and the selection's\n\
      :stats         count, sum, mean, min and max of the numbers selected\n\
      :sum-column N  sum the Nth whitespace field down the selection\n\
      :count-matches how many matches (and matching lines) a regex has\n\
      :uniq-count    the lines collapsed to 'count line', by frequency\n\
      :count-unique :unique-words :bases :crc32 :character-info",
    ),
    (
        "Arithmetic over lines",
        "Each of these treats every selected line as a number.\n\n\
      :offset N     add N (negative subtracts)\n\
      :scale F      multiply by a factor\n\
      :clamp A B    hold each value inside a range\n\
      :abs          absolute value\n\
      :to-fixed N   round to N decimal places\n\
      :running-total :running-max :running-min   cumulative down the column\n\
      :increment-numbers N   add N to every integer in the selection instead\n\
      :pad-numbers W         zero-pad every integer to W digits",
    ),
    (
        "Fields & columns",
        "Whitespace-separated data without leaving the buffer.\n\n\
      :field N        keep the Nth field of each line (awk '{print $N}')\n\
      :sum-fields     replace each line with its row total\n\
      :avg-fields :max-fields :min-fields :range-fields   the other row summaries\n\
      :transpose-grid rows become columns\n\
      :align          align on a delimiter (or /regex/) so it shares a column\n\
      :sort-by-field :sort-numeric-fields :sort-columns   see Sorting text",
    ),
    (
        "Line surgery",
        ":head N / :tail N     keep the first or last N lines of the selection\n\
      :sample N            keep N random lines, in order\n\
      :shuffle             reorder them randomly\n\
      :dedup               drop every duplicate, keeping the first\n\
      :dedup-adjacent      collapse runs of identical lines (uniq)\n\
      :rotate-lines N      rotate the block by N\n\
      :repeat-lines N      repeat each line N times\n\
      :number-lines        prepend line numbers\n\
      :seq A B [step]      insert a sequence instead of editing one",
    ),
    (
        "Prose & typography",
        ":smart-quotes          straight quotes to curly ones, in context\n\
      :typographic-dashes   --- to an em dash, -- to an en dash, ... to an ellipsis\n\
      :de-typography        all of that back to ASCII punctuation\n\
      :unwrap-paragraphs    undo hard wrapping within each paragraph\n\
      :reflow               hard-wrap the selection to a width\n\
      :capitalize-lines :swapcase :cycle-case   letter case\n\
      :quote-lines / :unquote-lines             quote each line",
    ),
    (
        "Markdown authoring",
        ":md-table            realign a pipe table and rebuild its separator row\n\
      :code-fence [lang]  wrap the selection in a fenced code block\n\
      :checkbox-list      turn the lines into a '- [ ]' task list\n\
      :ordered-list       number them instead\n\
      :markdown-link      make a link out of the selection\n\
      :linkify            wrap bare URLs in link syntax\n\
      :strip-markdown-links :strip-emphasis     the inverse\n\
      :slugify-lines / :deslugify               headings to anchors and back",
    ),
    (
        "HTML & templates",
        ":emmet expands the emmet abbreviation before the cursor, which is the\n\
      fastest way to write a block of markup.\n\n\
      :wrap-tag <tag>     wrap each selection in a tag pair\n\
      :wrap-with <text>   the general form, for anything else\n\
      :to-html-list       lines to a <ul>; :from-html-list takes it back\n\
      :csv-to-html-table  a table from CSV\n\
      :strip-html-comments :unicode-escape :de-typography  tidy the result",
    ),
    (
        "Encoding & escapes",
        ":encoding              the buffer's character encoding\n\
      :line-ending          LF or CRLF; :dos2unix and :unix2dos convert\n\
      :base32-encode / :base32-decode\n\
      :unicode-escape / :unicode-unescape   non-ASCII to \\u{…} and back\n\
      :to-binary / :from-binary\n\
      :dec-to-hex / :hex-to-dec / :bases\n\
      :human-bytes          a byte count as 1.5 KiB\n\
      :ordinal              1 to 1st, 22 to 22nd",
    ),
    (
        "Ciphers & checksums",
        ":caesar N       shift the letters by N (13 is ROT13, negatives go back)\n\
      :rot47         rotate every printable ASCII character; self-inverse\n\
      :morse-encode / :morse-decode\n\
      :nato          spell the selection in the NATO alphabet\n\
      :crc32         the checksum of the selection, in hex and decimal\n\
      :encrypt / :decrypt   real encryption, with an age passphrase",
    ),
    (
        "Inserting boilerplate",
        ":uuid              a random UUID v4 at each cursor\n\
      :lorem [N]        N words of placeholder text\n\
      :date :datetime :timestamp   today, now, and the epoch\n\
      :seq A B          a run of integers\n\
      :insert-char      a character by name or code point\n\
      :insert-file / :insert-file-literally / :insert-buffer\n\
      :insert-output    a command's output before each selection\n\
      :bat-template     a minimal batch-file skeleton",
    ),
    (
        "Aligning & whitespace",
        ":align [/re/]        line the selection up on a delimiter\n\
      :retab :tabify :untabify    between tabs and spaces\n\
      :indent-style        what this buffer uses\n\
      :just-one-space :fixup-whitespace :cycle-spacing\n\
      :squeeze-blank-lines :remove-blank-lines :trim-lines\n\
      :pad-left / :pad-right N    pad to a width\n\
      :center-region :reflow      centre and wrap",
    ),
    (
        "Keeping & dropping lines",
        ":filter <re>         keep only the lines matching (emacs keep-lines)\n\
      :reject <re>        drop them instead (flush-lines)\n\
      :copy-matching-lines / :kill-matching-lines   to the kill ring\n\
      :count-matches      how many there are, without touching anything\n\
      :global / :vglobal  run an Ex command on matching / non-matching lines\n\
      :multi-occur        list matches across buffers",
    ),
    (
        "Renaming & finding references",
        ":rename-word     rename every whole-word use of the symbol here, in this\n\
                       buffer, without a language server\n\
      :grep-word      search the project for the word under the cursor\n\
      :search-project search the project with ripgrep, jumpable in Run\n\
      :project-replace  regex replace across every matching workspace file\n\
      :Subvert        case-preserving substitute — foo/Foo/FOO all at once\n\
      :todos          every TODO, FIXME, HACK, XXX, BUG and NOTE in the tree",
    ),
    (
        "Transposing",
        ":transpose-chars    swap the two characters around the cursor\n\
      :transpose-words   the two words\n\
      :transpose-regions the two selections\n\
      :transpose-grid    a whitespace grid, rows for columns\n\
      :rev               reverse the characters of each line\n\
      :reverse           reverse the order of the lines",
    ),
    (
        "The argument list",
        "vim's list of files to work through.\n\n\
      :args         show it, or set it from a glob\n\
      :argadd :argedit :argdelete :argdedupe   change it\n\
      :next :previous :first :last :argument   move through it\n\
      :wnext :wprevious                        write, then move\n\
      :argdo <cmd>  run a command over every file in it\n\
      :snext :sprevious :srewind :slast :sargument  the split versions\n\
      :all          open a window for each",
    ),
    (
        "The buffer list",
        ":buffers          list them (:ls and :files are the same command)\n\
      :buffer N         go to one by number or name\n\
      :bfirst :blast :bmodified :balt   go by position or state\n\
      :badd :ball       add one, or open a window for each\n\
      :bufdo <cmd>      run a command in every buffer\n\
      :sbuffer :sbnext :sbprevious :sbfirst :sblast :sbmodified   in a split\n\
      :buffer-close :buffer-close-others :buffer-close-all",
    ),
    (
        "The quickfix list",
        "Filled by :make, :compile, :grep and friends.\n\n\
      :cnext :cprevious :cfirst :clast :cc N     move through it\n\
      :cnfile :cpfile :cabove :cbelow :cbefore :cafter   by file and position\n\
      :copen :cclose :cbottom :clist             the window and the listing\n\
      :cdo / :cfdo <cmd>    run a command at every entry, or once per file\n\
      :cbuffer :cgetbuffer :cexpr :cgetexpr :caddexpr :cfile :cgetfile\n\
      :caddfile             fill it from a buffer, an expression or a file\n\
      :colder :cnewer :chistory   the older lists are kept",
    ),
    (
        "The location list",
        "The same commands, per window: :lnext :lprevious :lfirst :llast :ll\n\
      :lnfile :lNfile :lpfile :labove :lbelow :lbefore :lafter\n\
      :lopen :lclose :lbottom :llist\n\
      :ldo :lfdo\n\
      :lbuffer :lgetbuffer :lexpr :lgetexpr :laddexpr :lfile :lgetfile\n\
      :laddfile\n\
      :lolder :lnewer :lhistory\n\n\
      :lmake and :lgrep fill it where :make and :grep fill the quickfix list.",
    ),
    (
        "Grep commands",
        ":grep / :grepadd       run the external grep program into the quickfix list\n\
      :lgrep / :lgrepadd    the same, into the location list\n\
      :vimgrep / :vimgrepadd   search with the editor's own engine\n\
      :lvimgrep / :lvimgrepadd\n\
      :helpgrep / :lhelpgrep   search the help itself\n\
      :search-project        ripgrep, straight into the Run console\n\
      :grep-word             the word under the cursor, without typing it",
    ),
    (
        "The tag stack",
        ":tag <name>     jump to a tag; :tselect and :tjump choose between matches\n\
      :tnext :tprevious :tfirst :tlast   walk the matches\n\
      :pop            back up the stack; :tags shows how deep it is\n\
      :stag :stselect :stjump            the same, in a split\n\
      :ptag :ptselect :ptjump :ptnext :ptprevious :ptfirst :ptlast   in the\n\
                     preview window (:pclose shuts it)\n\
      :ltag           put the matches in the location list\n\
      :regenerate-tags rebuilds the index",
    ),
    (
        "Searching includes & definitions",
        "vim's search through included files, useful in C-like trees.\n\n\
      :ilist / :isearch    list or search identifiers in this file and its\n\
                           includes; :ijump jumps to one, :isplit opens it in\n\
                           a split\n\
      :dlist / :dsearch / :djump / :dsplit   the same for macro definitions\n\
      :checkpath           report which included files could not be found",
    ),
    (
        "Tabs",
        ":tabnew :tabclose :tabonly           open, close, keep one\n\
      :tabnext :tabprevious :tabfirst :tablast :tab-switch   move between them\n\
      :tabmove :tab-rename                 reorder and name\n\
      :tabs                                list them\n\
      :tabdo <cmd>                         run a command in each\n\
      :tabfind                             open a file from the path in a tab\n\
      :tab-undo :tab-recent :tab-bar-history-back :tab-bar-history-forward\n\
                                           the tab bar's own history",
    ),
    (
        "The preview window",
        ":pedit <file>   open a file in the preview window without leaving this one\n\
      :pbuffer        preview a buffer instead\n\
      :psearch        preview the first match for a pattern in the include path\n\
      :ptag :ptjump :ptselect :ptnext :ptprevious :ptfirst :ptlast :ppop\n\
                      the tag commands, aimed at the preview window\n\
      :pclose         shut it",
    ),
    (
        "Window commands",
        ":vsplit :hsplit :vsplit-new :hsplit-new   split, with or without a file\n\
      :close :only                             close this one, or the rest\n\
      :wincmd <key>                            any C-w command by name\n\
      :windo <cmd>                             run a command in every window\n\
      :resize :winsize :winpos                 size and position\n\
      :aboveleft :belowright :leftabove :rightbelow :topleft :botright\n\
                     modifiers that place the next split\n\
      :vertical :horizontal :tab               and how it is oriented",
    ),
    (
        "Registers & the clipboard",
        ":registers          what every register holds\n\
      :set-register / :clear-register\n\
      :execute-register  run a register as a macro\n\
      :kbd <keys> [reg]  put the key sequence a description denotes into a\n\
                         register, so @reg replays it\n\
      :clipboard-yank :clipboard-paste-after :clipboard-paste-before\n\
      :clipboard-paste-replace         the system clipboard\n\
      :primary-clipboard-yank and the primary-* family   X11's primary selection\n\
      :yank-join :clipboard-yank-join  yank the lines joined into one\n\
      :show-clipboard-provider         which tool is doing the work",
    ),
    (
        "Pipes & embedded pipelines",
        ":pipe and :pipe-to send each selection through a shell command, replacing\n\
      it or ignoring the output.  :insert-output and :append-output put a\n\
      command's output around the selection instead.\n\n\
      The xpipe family does the same through the compiled-in interpreters, in\n\
      process, with no subprocess at all: :xpipe filters each selection through\n\
      a chain of stages, :xpipe-to ignores the output, and :xpipe-insert /\n\
      :xpipe-append run a stage with no input and place what it prints.\n\
      :shell-quote quotes a string safely before any of that.",
    ),
    (
        "Vim script",
        ":let :const :unlet :lockvar :unlockvar     variables\n\
      :echo :echomsg :echoerr :echohl           output\n\
      :eval :call :execute :defer               evaluation\n\
      :source :runtime :scriptnames :scriptencoding   files of script\n\
      :command :delcommand :comclear            user commands\n\
      :redir                                    capture messages to a register\n\
                                                or a file\n\
      :messages                                 what has been printed\n\n\
      :vim runs a line through the embedded vimscript interpreter directly.",
    ),
    (
        "Script debugging & profiling",
        ":debug <cmd>    run an Ex command under the script debugger\n\
      :breakadd :breakdel :breaklist   breakpoints in script\n\
      :debuggreedy    take debugger commands from the script itself\n\
      :profile        start, func, file, pause, stop, dump\n\
      :profdel        stop profiling something\n\
      :syntime        profile syntax highlighting: on, off, clear, report\n\
      :checkhealth    the health checks — clipboard, servers, grammars\n\
      :log            zmax's own log file, read-only",
    ),
    (
        "Autocommands",
        ":autocmd defines one, :augroup groups them so they can be cleared as a\n\
      set, and :doautocmd / :doautoall fire them by hand.  :noautocmd runs a\n\
      command with none of them firing, which is the way to avoid a loop.\n\n\
      :editorconfig applies the nearest .editorconfig to the buffer — the same\n\
      job, done by a file instead of a script.",
    ),
    (
        "Command modifiers",
        "Words that go before another Ex command and change how it runs.\n\n\
      :silent :unsilent :verbose      how much it says\n\
      :confirm                        ask before anything is lost\n\
      :hide                           do not warn about an unwritten buffer\n\
      :keepalt :keepjumps :keepmarks :keeppatterns   leave the lists alone\n\
      :lockmarks                      keep marks where they are\n\
      :noswapfile                     no swap file for this one\n\
      :browse                         pick the file with the file picker\n\
      :sandbox                        refuse shelling out and writing files",
    ),
    (
        "Ex line commands",
        "The line-range commands, spelled out.\n\n\
      :print :number :list :print-line-number   show lines\n\
      :delete-lines :yank-lines :copy-lines :move-lines\n\
      :indent-lines :dedent-lines\n\
      :put :iput      put a register back\n\
      :join           join the lines; :join-with uses a separator\n\
      :normal <keys>  run normal-mode keys over the range\n\
      :substitute     the classic; :smagic and :snomagic set the pattern rules",
    ),
    (
        "Spelling word lists",
        ":spellgood adds a word to the personal dictionary and :spellwrong marks it\n\
      wrong; :spellrare marks it rare and :spellundo takes an entry back.\n\
      :spelldump lists the whole dictionary, :spellinfo says where the files\n\
      came from, and :mkspell compiles a word list into one.\n\
      :spellrepall repeats the last correction everywhere in the buffer.",
    ),
    (
        "Dictionaries & translation",
        ":dictionary-search <word>    look a word up\n\
      :dictionary-tooltip-mode    look up what is under the cursor as you move\n\
      :Thesaurus                  pick a synonym\n\
      :translate                  translate the word under the cursor, or given\n\
                                  text, between the configured languages\n\
      :translate-set-languages :translate-reverse   which way round\n\
      :youdao-lookup              the Youdao dictionary",
    ),
    (
        "Japanese & Chinese text",
        ":migemo-search        turn romaji into a regex that matches Japanese\n\
      :romaji-to-kana      romaji to hiragana\n\
      :kana-to-katakana / :katakana-to-kana\n\
      :chinese-conv-simplified / :chinese-conv-traditional\n\
      :chinese-pinyin      the pinyin reading of the region",
    ),
    (
        "Input methods",
        ":list-input-methods and :describe-input-method say what is available;\n\
      :activate-transient-input-method turns one on for a single insertion.\n\
      :quail-translation-keymap and :quail-show-key explain what a key will\n\
      produce in the active method.\n\n\
      :modify-category-entry :standard-display-8bit :visual-order-cursor-movement\n\
      cover the older, more awkward corners of the same problem.",
    ),
    (
        "Characters & fonts",
        ":character-info      everything about the character under the cursor\n\
      :insert-char        insert one by name or code point\n\
      :digraphs           vim's two-key sequences\n\
      :unicode-fonts      a block sample sheet, to see what the terminal font\n\
                          actually covers\n\
      :unicode-fonts-char one character in every font that has it\n\
      :strip-invisible    remove what should not have been there",
    ),
    (
        "Files on disk",
        ":write-region and :append-to-file write part of the buffer somewhere else;\n\
      :read and :insert-file bring a file in (:insert-file-literally without\n\
      any decoding).\n\n\
      :delete-file :mkdir :copy-directory :list-directory\n\
      :chmod-x :set-file-modes :make-symbolic-link :add-name-to-file\n\
      :set-visited-file-name    keep the text, change what it writes to\n\
      :sudo-edit / :sudo-write  when the file belongs to root\n\
      :RevealInFinder           show it in the OS file manager",
    ),
    (
        "Directories & the environment",
        ":change-current-directory moves the editor; :show-directory says where it\n\
      is.  :push-directory :pop-directory :show-directory-stack keep a stack,\n\
      as the shell does.\n\n\
      :dirs asks the shell where it actually is and resynchronises, and\n\
      :shell-dirtrack-mode / :dirtrack-mode keep that automatic.\n\
      :getenv / :setenv read and set variables for the editor and everything\n\
      it starts.",
    ),
    (
        "Scratch buffers",
        ":Scratch opens an unnamed buffer, optionally in a language.  :new does the\n\
      same in a split.\n\n\
      :append-to-buffer :prepend-to-buffer :copy-to-buffer   move text into\n\
      another buffer without a register\n\
      :insert-buffer      the other direction\n\
      :rename-buffer :rename-uniquely   naming\n\
      :write-buffer-close write it and close it in one step",
    ),
    (
        "Swap files & recovery",
        ":preserve flushes the buffer to its swap file now; :recover replaces the\n\
      buffer with what the swap file holds after a crash, and :swapname says\n\
      which file that is.  :noswapfile runs one command without one.\n\n\
      :checktime notices that a file changed underneath you, :reload rereads\n\
      this buffer and :reload-all rereads every buffer.\n\
      :ask-user-about-lock decides what happens when someone else has it.",
    ),
    (
        "Sessions, views & state",
        ":mksession writes the working directory and the buffer list to a file that\n\
      :source restores; :mkview and :loadview do the same for one window's\n\
      cursor and folds.  :mkvimrc and :mkexrc write the settings out.\n\n\
      :wshada / :rshada carry the registers and every buffer's marks between\n\
      runs, and :wundo / :rundo do it for the undo tree.\n\
      :syncbind resynchronises scroll-bound windows.",
    ),
    (
        "Local history",
        ":LocalHistory picks a saved snapshot of this file and opens it — the\n\
      editor keeps them itself, so it works on files that were never committed.\n\n\
      :undolist lists the undo states, :undojoin makes the next change part of\n\
      the last one, and :changes shows what changed where.\n\
      :jumps and :clearjumps are the jump list; :RecentLocations is the same\n\
      ring newest-first with context, and :history is the command line's.",
    ),
    (
        "Git hunks & conflicts",
        ":hunk-next :hunk-prev move between the gutter's hunks and :hunk-reset\n\
      throws one away.\n\n\
      :merge opens a three-pane view — ours, result, theirs — over a conflicted\n\
      file.  :conflict-next / :conflict-prev walk the conflicts and\n\
      :conflict-ours :conflict-theirs :conflict-both settle one.\n\n\
      :git-stage / :git-unstage, :stash / :stash-pop, :update (write only if\n\
      modified), :compare-ref (diff against any ref) and\n\
      :vc-revision-other-window round it out.",
    ),
    (
        "GDB & the gud layer",
        ":gdb-breakpoints-buffer :gdb-threads-buffer :gdb-watch-buffer open the\n\
      classic gud windows; :gdb-watch adds an expression and :gdb-var-delete\n\
      removes one.  :gdb-save-window-configuration and\n\
      :gdb-load-window-configuration keep the layout.\n\n\
      :gud-def and :gud-call reach the underlying debugger directly, and\n\
      :gud-tooltip-mode shows a value when the cursor rests on a name.\n\
      :next-error :previous-error :first-error walk whatever produced the\n\
      errors, with :next-error-follow-minor-mode and :next-error-select-buffer\n\
      controlling which list that is.",
    ),
    (
        "Language injection",
        "A string can hold another language — SQL in Rust, HTML in JavaScript.\n\n\
      :injections       the rules in force, defaults plus injections.toml\n\
      :injection-info   what language the cursor is really in\n\
      :inject-language  inject one into the string at point with a hint comment\n\
      :edit-fragment    open the fragment in its own buffer, with its own\n\
                        highlighting and its own language server\n\
      :apply-fragment   write it back into the host string",
    ),
    (
        "Tree-sitter inspection",
        ":tree-sitter-scopes shows the scopes under the cursor, which is what theme\n\
      work needs; :tree-sitter-highlight-name gives the one that decided the\n\
      colour.  :tree-sitter-subtree prints the smallest subtree spanning the\n\
      selection, and :tree-sitter-layers lists the parsers layered over the\n\
      buffer.  :syntax reports what the buffer is being parsed as.",
    ),
    (
        "Workspace trust",
        "Language servers and workspace config do not run in an untrusted tree.\n\
      :trust trusts this one, :workspace-trust does the same by name,\n\
      :workspace-untrust revokes it and :workspace-exclude marks a tree that\n\
      should never be asked about again.\n\n\
      :lsp-health reports which servers are ready, initializing or absent and\n\
      what each supports; :lsp-workspace-command runs a server's own command;\n\
      :checkhealth checks the rest of the editor.",
    ),
    (
        "Configuration files",
        ":config-open opens config.toml and :config-open-workspace the one for this\n\
      tree; :init-open opens the init script and :log-open the log.\n\
      :config-reload applies changes without restarting.\n\n\
      :options opens Preferences at the Settings tab, :set / :setlocal /\n\
      :setglobal change one option, and :set-option :get-option :toggle-option\n\
      do it by name.  :customize :customize-variable :customize-group\n\
      :customize-apropos :customize-unsaved browse the same settings.",
    ),
    (
        "Themes from the command line",
        ":theme <name>       switch; :theme-next :theme-prev :theme-toggle cycle\n\
      :Colors             pick one with the fuzzy finder\n\
      :describe-theme     what a theme sets\n\
      :disable-theme      back to the default\n\
      :customize-face :set-face-foreground :set-face-background  one face\n\
      :highlight          define a face outright\n\
      :set-fringe-style :text-scale-pinch   the frame around the text\n\n\
      Preferences ▸ Color Scheme does the same with a preview.",
    ),
    (
        "Minor modes worth knowing",
        ":nav-flash-mode           flash the cursor line after a jump\n\
      :vim-empty-lines-mode    draw vim's ~ past the end of the buffer\n\
      :blink-cursor-mode :selectric-mode   a blinking cursor, a typewriter click\n\
      :repeat-mode             repeat a multi-key command with its last key\n\
      :eldoc-mode              signature hints where the cursor is\n\
      :completion-preview-mode the top candidate shown inline as you type\n\
      :icomplete-mode :fido-mode   candidates on the prompt line while typing\n\
      :rainbow-mode            paint colour literals in the colour they name\n\
      :color-identifiers-mode  colour only what the grammar calls a variable\n\
      :cua-mode                C-x cut, C-c copy, C-v paste",
    ),
    (
        "Lisp editing",
        "parinfer keeps parentheses and indentation in agreement, so one of them can\n\
      be edited and the other follows.\n\n\
      :parinfer-smart-mode   indentation and parens both, which is the default\n\
      :parinfer-indent-mode  indentation decides the parens\n\
      :parinfer-paren-mode   parens decide the indentation\n\
      :parinfer-mode         toggle between smart and paren\n\
      :parinfer-off          leave the text entirely alone",
    ),
    (
        "The fzf.vim surface",
        "Beyond :Files :Buffers :Rg :Lines :BLines :Tags :BTags :Commits :Maps\n\
      :Helptags and :Snippets:\n\n\
      :GFiles      git-tracked files only\n\
      :Locate      locate(1)\n\
      :History     recently opened files\n\
      :Filetypes   pick a language and set the buffer's\n\
      :BCommits    commits touching this file\n\
      :Jumps :Marks :Windows   the jumplist, the marks, the open windows\n\
      :Colors :Commands        themes and every : command\n\
      :Todo        TODO markers across the tree\n\
      :LocalHistory  the snapshots of this file",
    ),
    (
        "Jupyter notebooks",
        ":ein-notebooks lists a Jupyter server's notebooks over its REST API and\n\
      :ein-open opens one.  :ein-kernels lists the running kernels, with\n\
      :ein-kernel-start and :ein-kernel-stop controlling them.",
    ),
    (
        "Language runtimes",
        ":nvm-list and :nvm-use put an nvm-installed node on PATH for this editor\n\
      session and everything it starts; :npm-scripts lists what package.json\n\
      defines and :npm-run runs one.\n\n\
      :conda-env-list :conda-activate :conda-deactivate :conda-env-current do\n\
      the same for conda environments.",
    ),
    (
        "Elasticsearch",
        ":es-health reports cluster health, :es-indices and :es-nodes list what is\n\
      there, :es-search runs a Lucene query against an index and :es-request\n\
      sends a raw method, path and body.  The URL comes from ES_URL, or\n\
      localhost:9200.",
    ),
    (
        "Reference lookup",
        ":Man <page>      a man page in the run console\n\
      :dash-at-point   look the term up in Dash or Zeal\n\
      :dash-at-point-with-docset / :dash-docsets   pick which docset\n\
      :ietf-docs-open  an RFC or draft by name, or the word under the cursor\n\
      :bat-cmd-help    help for a batch-file command\n\
      :apropos         the editor's own commands and settings\n\
      :exusage :viusage  vim's summary of Ex and Normal mode\n\
      :helptags :helpclose   build the help index, shut the help window",
    ),
    (
        "News & aggregators",
        ":hackernews [feed]   top, new, best, ask, show or job, over the HN API\n\
      :lobsters            the lobste.rs front page\n\
      :reddit <sub>        a subreddit over the public JSON API;\n\
                           :reddit-comments opens a thread\n\
      :twitch-streams :twitch-search :twitch-open   live streams\n\
      :streamlink          open a stream in a player\n\
      :search-engine       search with a configured engine",
    ),
    (
        "Ambient information",
        ":weather and :weather-quick  a forecast, from OpenWeatherMap with a key or\n\
                                   wttr.in without one\n\
      :sun-times                  sunrise, sunset, solar noon and day length\n\
      :uptime                     how long this editor has been running\n\
      :wakatime                   today's coding time from the wakatime CLI\n\
      :wakatime-summary :wakatime-dashboard :wakatime-heartbeat",
    ),
    (
        "Dictation",
        ":whisper-record records from the microphone and transcribes it with\n\
      whisper.cpp; :whisper-file does the same for a file already on disk.\n\
      :whisper-model selects or lists the model and :whisper-language sets the\n\
      language it should expect.",
    ),
    (
        "The browser bridge",
        ":edit-server-start runs the Edit with Emacs server, so a textarea in the\n\
      browser can be edited here.  :edit-server-pending lists the requests\n\
      waiting, :edit-server-take claims one, :edit-server-finish sends the text\n\
      back and :edit-server-stop shuts the server down.",
    ),
    (
        "Timestamped notes",
        ":denote creates an IDENTIFIER--title__keywords.org note and opens it, which\n\
      makes the file name carry the metadata.  :denote-link inserts a link to\n\
      another note, or lists them when given no argument.",
    ),
    (
        "Thumbnails & media",
        ":thumbs-mode shows a directory's images as a grid of labelled thumbnails.\n\
      :image-mode-mark-file and :image-mode-unmark-file mark the ones worth\n\
      keeping, and :image-mode-copy-file-name-as-kill puts a name on the kill\n\
      ring.  :yank-media saves the clipboard's image next to the buffer and\n\
      inserts the reference the language uses for it.",
    ),
    (
        "Language modes",
        ":set-language sets what the buffer is parsed and completed as, and\n\
      :filetype reports it; :Filetypes picks one with the fuzzy finder.\n\n\
      Some languages have their own command: :apache-mode :cfengine-mode\n\
      :jr-mode :kivy-mode, and :text-mode / :normal-mode for the plain cases.",
    ),
    (
        "More language runners",
        ":factor-eval :factor-run-file :factor-listener :factor-vocab-words   Factor\n\
      :mercury-compile :mercury-run                                       Mercury\n\
      :powershell-run :powershell-eval :powershell-regexp-to-regex        PowerShell\n\
      :bat-run :bat-labels :bat-template                                  batch files\n\
      :sailfish-build :sailfish-install :sailfish-deploy                  Sailfish OS\n\n\
      The compiled-in interpreters are a separate topic — see Embedded\n\
      scripting — as are :perldo :rubydo :luado :pydo :py3do, which run a line\n\
      of their language over every line of the buffer.",
    ),
    (
        "Printing",
        ":lpr-buffer sends the buffer to the printer and :lpr-region sends only the\n\
      selection.  :print :number :list write lines to the message area instead,\n\
      which is what to use when the destination is a terminal recording.",
    ),
    (
        "Leaving & coming back",
        ":quit :quit-all :write-quit :write-all :write-quit-all   the usual ways out\n\
      :cquit           exit with a failing status, for git and friends\n\
      :exit            write if modified, then quit\n\
      :detach          leave the TUI and return to the shell, with the editor\n\
                       still running in the background\n\
      :reopen          reopen the file that was just closed\n\
      :restart         restart the editor in place\n\n\
      C-z (C-x C-z) suspends and hands the terminal back to the shell; fg\n\
      brings it round again.",
    ),
    (
        "The frame",
        ":zen toggles the IDE workbench, which is the focus mode.\n\n\
      :redraw :redrawstatus :redrawtabline :mode   force a repaint\n\
      :winsize :winpos                             the editor area, in cells\n\
      :text-scale-pinch                            zoom\n\
      :sleep :noop                                 wait, and do nothing, which\n\
                                                   scripts occasionally need\n\
      :gui                                         fails; zmax is a TUI",
    ),
    (
        "Vim packages & runtime files",
        ":packadd loads a package from the pack directory now and :packloadall loads\n\
      every one; :packdel removes one and :packupdate updates it.\n\
      :package-menu-filter-upgradable narrows the list to what has an update.\n\n\
      :runtime sources a file from every directory on the runtime path and\n\
      :source runs one by name; :scriptnames lists what has been sourced.\n\
      zmax's own native plugins are :zmax-native — see the Packages topic.",
    ),
    (
        "PlatformIO: building",
        ":pio-build compiles the project and routes errors to the compilation list;\n\
      the flag variants carry the flag in the name.\n\n\
      :pio-build-verbose :pio-build-silent :pio-build-no-auto-clean\n\
      :pio-run-jobs N      build with N parallel jobs\n\
      :pio-target <name>   any build target, live in a terminal panel\n\
      :pio-list-targets    what those targets are\n\
      :pio-nobuild :pio-clean :pio-cleanall\n\
      :pio-size            the program size report\n\
      :pio-envdump         the resolved build environment\n\
      :pio-env :pio-build-conf   which environment and config file to use\n\
      :pio-exec            build and run the native program\n\
      :pio-compiledb       a compile_commands.json for other tools\n\
      :pio-ci              build a standalone tree in an isolated project",
    ),
    (
        "PlatformIO: uploading & the monitor",
        ":pio-upload flashes the board and :pio-upload-monitor opens the monitor\n\
      straight after.  :pio-upload-to picks the port.\n\n\
      :pio-buildfs :pio-uploadfs :pio-uploadeep   filesystem and EEPROM images\n\
      :pio-bootloader :pio-fuses                  the parts below the sketch\n\
      :pio-monitor                                the serial monitor\n\
      :pio-monitor-filter :pio-monitor-filters-clear :pio-monitor-eol\n\
      :pio-monitor-parity :pio-monitor-rts :pio-monitor-dtr :pio-monitor-echo\n\
      :pio-monitor-raw :pio-monitor-encoding :pio-monitor-flow\n\
      :pio-monitor-reconnect :pio-monitor-quiet :pio-monitor-exit-char\n\
      :pio-monitor-menu-char                      how it behaves\n\
      :pio-plotter                                graph the monitor's numbers\n\
      :embedded-baud                              the project's baud rate",
    ),
    (
        "PlatformIO: boards & projects",
        ":pio-boards searches the board database and :pio-boards-json gives the\n\
      machine-readable form; :pio-boards-installed lists what is already here.\n\n\
      :pio-init                scaffold a project for a board\n\
      :pio-init-no-deps :pio-init-ide :pio-init-sample :pio-init-option\n\
      :pio-init-env-prefix     how that scaffolding is shaped\n\
      :pio-devices             connected devices; :pio-device-serial,\n\
                               :pio-device-logical and :pio-device-mdns split\n\
                               them by kind\n\
      :pio-home                the PlatformIO Home GUI",
    ),
    (
        "PlatformIO: libraries & packages",
        ":pio-lib-install adds a library; :pio-lib-install-nosave leaves the project\n\
      file alone.  :pio-lib-list :pio-lib-show :pio-lib-search :pio-lib-outdated\n\
      :pio-lib-update :pio-lib-uninstall cover the rest.\n\n\
      Packages are the general form: :pio-pkg-install-force\n\
      :pio-pkg-install-global :pio-pkg-install-skip-deps\n\
      :pio-pkg-list-libraries :pio-pkg-list-platforms :pio-pkg-list-tools\n\
      :pio-pkg-list-global :pio-pkg-update-global\n\
      :pio-pkg-search-page :pio-pkg-search-sort :pio-pkg-show-type\n\
      :pio-pkg-exec :pio-pkg-exec-pkg :pio-pkg-exec-call   run a packaged tool\n\
      :pio-pkg-pack :pio-pkg-publish :pio-pkg-unpublish :pio-pkg-unpublish-undo\n\
      :pio-platform-install :pio-tool-install",
    ),
    (
        "PlatformIO: testing & analysis",
        ":pio-test runs the unit tests; the variants set one flag each.\n\n\
      :pio-test-filter :pio-test-ignore :pio-test-verbose :pio-test-no-reset\n\
      :pio-test-without-building :pio-test-without-uploading\n\
      :pio-test-without-testing\n\
      :pio-test-port :pio-test-upload-port :pio-test-monitor-dtr\n\
      :pio-test-monitor-rts :pio-test-conf\n\
      :pio-test-json :pio-test-json-path :pio-test-junit   machine-readable output\n\
      :pio-list-tests\n\n\
      :pio-check is static analysis into the compilation list, with\n\
      :pio-check-severity :pio-check-flags :pio-check-fail-on\n\
      :pio-check-skip-packages :pio-check-src-filters :pio-check-silent\n\
      :pio-check-verbose :pio-check-json :pio-check-conf.",
    ),
    (
        "PlatformIO: debugging & maintenance",
        ":pio-debug starts a debug session; :pio-debug-interface :pio-debug-load-mode\n\
      :pio-debug-verbose :pio-debug-conf tune it.\n\n\
      :pio-project-config :pio-project-config-json :pio-project-config-lint\n\
      :pio-project-metadata :pio-project-metadata-json :pio-project-metadata-path\n\
      :pio-system-info :pio-system-info-json :pio-system-completion\n\
      :pio-system-prune :pio-prune-cache :pio-prune-core :pio-prune-platform\n\
      :pio-prune-dry-run\n\
      :pio-settings-get :pio-settings-set :pio-settings-reset\n\
      :pio-upgrade :pio-upgrade-dev :pio-upgrade-deps-only",
    ),
    (
        "PlatformIO: remote devices",
        "A board attached to another machine, driven from here.\n\n\
      :pio-remote-agent-start :pio-remote-agent-start-named :pio-remote-agent-list\n\
      :pio-remote-devices    what those agents can see\n\
      :pio-remote-run :pio-remote-run-force\n\
      :pio-remote-test :pio-remote-update\n\
      :pio-remote-monitor    the serial monitor, over the network",
    ),
    (
        "PlatformIO: accounts & teams",
        ":pio-account-login :pio-account-logout :pio-account-show :pio-account-token\n\
      :pio-account-register :pio-account-password :pio-account-update\n\
      :pio-account-forgot :pio-account-destroy\n\n\
      :pio-org-list :pio-org-create :pio-org-add :pio-org-remove\n\
      :pio-org-update :pio-org-destroy\n\
      :pio-team-list :pio-team-create :pio-team-add :pio-team-remove\n\
      :pio-team-update :pio-team-destroy\n\
      :pio-access-list :pio-access-grant :pio-access-revoke\n\
      :pio-access-public :pio-access-private",
    ),
    (
        "Arduino: compiling",
        ":arduino-compile builds the sketch live in a terminal panel; each variant\n\
      adds one arduino-cli flag.\n\n\
      :arduino-compile-verbose :arduino-compile-quiet :arduino-compile-warnings\n\
      :arduino-compile-clean :arduino-compile-jobs N\n\
      :arduino-compile-properties :arduino-compile-build-property\n\
      :arduino-compile-board-options :arduino-compile-output-dir\n\
      :arduino-compile-preprocess :arduino-compile-debug-opt\n\
      :arduino-compile-profile :arduino-compile-dump-profile\n\
      :arduino-compile-export\n\
      :arduino-compiledb   a compile_commands.json for other tools",
    ),
    (
        "Arduino: uploading, monitor & debug",
        ":arduino-upload compiles and flashes; :arduino-upload-verify checks what\n\
      landed.  :arduino-upload-programmer :arduino-upload-dir\n\
      :arduino-upload-file :arduino-upload-verbose pick how.\n\n\
      :arduino-monitor      the serial monitor, with :arduino-monitor-raw\n\
                            :arduino-monitor-timestamp :arduino-monitor-quiet\n\
                            :arduino-monitor-describe\n\
      :arduino-plotter      graph what it prints\n\
      :arduino-debug :arduino-debug-info :arduino-debug-programmer\n\
      :arduino-burn-bootloader",
    ),
    (
        "Arduino: boards, ports & sketches",
        ":arduino-boards lists the known boards and :arduino-boards-hidden includes\n\
      the ones normally kept back; :arduino-board-list-watch keeps watching as\n\
      boards come and go.\n\n\
      :arduino-board-info :arduino-board-details-full :arduino-board-search\n\
      :arduino-board-programmers :arduino-board-attach\n\
      :arduino-ports        pick the serial port from what is connected\n\
      :arduino-new-sketch\n\
      :arduino-sketch-archive :arduino-sketch-archive-full   zip the sketch",
    ),
    (
        "Arduino: cores & libraries",
        "Cores are board support packages: :arduino-core-list :arduino-core-list-all\n\
      :arduino-core-list-updatable :arduino-core-search :arduino-core-install\n\
      :arduino-core-uninstall :arduino-core-upgrade :arduino-core-download\n\
      :arduino-core-update-index.\n\n\
      Libraries: :arduino-lib-list :arduino-lib-list-all\n\
      :arduino-lib-list-updatable :arduino-lib-search :arduino-lib-search-names\n\
      :arduino-lib-install :arduino-lib-install-git :arduino-lib-install-zip\n\
      :arduino-lib-install-no-deps :arduino-lib-uninstall :arduino-lib-upgrade\n\
      :arduino-lib-deps :arduino-lib-examples :arduino-lib-download\n\
      :arduino-lib-update-index.\n\n\
      :arduino-outdated :arduino-update :arduino-update-outdated :arduino-upgrade\n\
      cover both at once.",
    ),
    (
        "Arduino: configuration & the CLI",
        ":arduino-config shows the configuration; :arduino-config-get\n\
      :arduino-config-set :arduino-config-add :arduino-config-remove\n\
      :arduino-config-delete :arduino-config-init change it.\n\n\
      Build profiles: :arduino-profile-create :arduino-profile-set-default\n\
      :arduino-profile-lib-add :arduino-profile-lib-remove\n\n\
      :arduino-cli runs any arduino-cli command, :arduino-daemon runs it as a\n\
      gRPC daemon, :arduino-completion prints a completion script,\n\
      :arduino-cache-clean empties the download cache and :arduino-version says\n\
      which CLI is being used.",
    ),
    (
        "Key mapping: the whole table",
        "Beyond :map and :unmap, every mode has its own three commands.\n\n\
      :noremap :nnoremap :inoremap :vnoremap :xnoremap :snoremap :onoremap\n\
                     non-recursive, per mode\n\
      :nunmap :iunmap :vunmap :xunmap :sunmap :ounmap :cunmap :lunmap :tunmap\n\
                     remove one\n\
      :mapclear :nmapclear :imapclear :vmapclear :xmapclear :smapclear\n\
      :omapclear :cmapclear :lmapclear :tmapclear   remove all of them\n\
      :loadkeymap    load a keymap file's worth, in a sourced script",
    ),
    (
        "Menus: the whole table",
        ":menu and :emenu are the common ones; the rest exist so a menu item can be\n\
      defined for one mode only, or defined without recursion.\n\n\
      :nmenu :imenu :vmenu :xmenu :smenu :omenu :cmenu :tlmenu :amenu\n\
      :nnoremenu :inoremenu :vnoremenu :xnoremenu :snoremenu :onoremenu\n\
      :cnoremenu :tlnoremenu :anoremenu :noremenu\n\
      :nunmenu :iunmenu :vunmenu :xunmenu :sunmenu :ounmenu :cunmenu\n\
      :tlunmenu :tunmenu :aunmenu\n\
      :menutranslate   translate a menu path for display",
    ),
    (
        "Clearing abbreviations",
        ":cnoreabbrev defines a non-recursive command-line abbreviation, matching\n\
      :inoreabbrev and :noreabbrev.\n\n\
      :iunabbreviate :cunabbreviate remove one from insert or command mode, and\n\
      :abclear :iabclear :cabclear clear the whole table for a mode.\n\
      :expand-region-abbrevs expands every abbrev in the selection at once.",
    ),
    (
        "Search highlighting & signs",
        ":nohlsearch clears the search highlight until the next search.\n\n\
      :match highlights a pattern in its own group (:match none clears it), and\n\
      :2match / :3match are the second and third groups.\n\
      :unhighlight-regexp removes a Hi-Lock highlight,\n\
      :hi-lock-find-patterns activates the ones written in a comment at the top\n\
      of the file and :hi-lock-write-interactive-patterns writes the current\n\
      ones out in that form.\n\n\
      :sign defines, places, unplaces, lists and jumps to gutter signs.",
    ),
    (
        "Folding by command",
        ":fold folds a range, :foldopen and :foldclose open and close one, and\n\
      :folddoopen / :folddoclosed run a command on the open or closed lines.\n\n\
      :outline-hide-by-heading-regexp folds every heading matching a regexp and\n\
      :outline-show-by-heading-regexp unfolds them, which is the fastest way\n\
      through a large outline.  :org-cycle does the same for org headings.",
    ),
    (
        "Running a script file",
        "Vim's interpreter commands run a snippet, and the -file forms run a file.\n\n\
      :lua / :luafile        :perl / :perlfile\n\
      :python / :pyfile      :py3 / :py3file      :pyx / :pyxfile\n\
      :ruby / :rubyfile\n\n\
      :perldo :rubydo :luado :pydo :py3do run their language once per line over\n\
      the buffer.  The compiled-in interpreters are separate — see Embedded\n\
      scripting — and do not need the language installed.",
    ),
    (
        "File-local variables",
        ":add-file-local-variable writes a setting into the file's Local Variables\n\
      block, so it travels with the file; :delete-file-local-variable takes it\n\
      out.  :add-file-local-variable-prop-line and\n\
      :delete-file-local-variable-prop-line use the first line instead, which\n\
      is where a mode line usually goes.\n\n\
      :editorconfig applies the nearest .editorconfig, and :setlocal changes a\n\
      setting for this buffer without writing anything.",
    ),
    (
        "Opening a file",
        ":open takes a path; :find searches the path option for it and :sfind does\n\
      that in a split.  :view and :sview open read-only.\n\
      :drop jumps to the window already editing a file, opening it only if no\n\
      window has it, which is what an external tool should call.\n\
      :tabfind opens the found file in a tab, and :visual leaves Ex mode.",
    ),
    (
        "Editing by command",
        "Line edits that need no keystrokes, so they work from a script.\n\n\
      :append :insert :change   type lines in, ending with a lone '.'\n\
      :duplicate-line :dl       duplicate, delete\n\
      :move-line-down :move-line-up\n\
      :left :right :center      align lines to a width\n\
      :change-case              the symbol under the cursor, to any case\n\
      :split-line :comment-box :repeat\n\
      :undo :redo               and :earlier / :later by time",
    ),
    (
        "Going to a place",
        ":goto <n> and :goto-line-relative <n> take a line number, absolute or from\n\
      the start of the narrowed region.  :goto-offset and :goto-byte take a\n\
      character or byte position instead.\n\n\
      :cc and :ll jump to the numbered quickfix or location entry, :mark sets a\n\
      mark by name, and :z prints a window of lines around the cursor into a\n\
      scratch buffer.",
    ),
    (
        "More text conversions",
        ":json-unflatten     rebuild nested JSON from 'path = value' lines\n\
      :toml-to-json      TOML to pretty JSON\n\
      :lines-to-json     the lines as a JSON array of strings\n\
      :kv-to-json        key=value or key: value lines as an object\n\
      :sql-in            a SQL IN-list from the lines\n\
      :remove-trailing-commas / :add-trailing-commas   between JSON5 and JSON\n\
      :after <delim> / :before <delim>   keep one side of each line\n\
      :natural-sort      file2 before file10\n\
      :sum               sum, mean, min, max and count, in the status line",
    ),
    (
        "The primary selection",
        "X11 keeps a second clipboard holding whatever was last selected with the\n\
      mouse.  :primary-clipboard-yank and :primary-clipboard-yank-join put text\n\
      there, and :primary-clipboard-paste-after :primary-clipboard-paste-before\n\
      :primary-clipboard-paste-replace read it back.\n\
      :show-clipboard-provider says which external tool is being used for both.",
    ),
    (
        "Projects",
        ":projectile-test-project runs the project's test command in its root and\n\
      :test-buffer runs only this file's tests; :test-function runs the one test\n\
      named, defaulting to the identifier under the cursor.\n\n\
      :projectile-invalidate-cache makes the next file search rescan the tree,\n\
      and :projectile-regenerate-tags rebuilds the tag index.\n\
      :run runs the project in the Run tool window.",
    ),
    (
        "Media players",
        ":spotify-play-pause :spotify-next :spotify-previous :spotify-status\n\
      :spotify-search-track :spotify-search-album :spotify-search-artist\n\
      :spotify-play-uri :spotify-quit\n\n\
      :pianobar :pianobar-play-pause :pianobar-next :pianobar-love\n\
      :pianobar-ban :pianobar-tired :pianobar-station :pianobar-info\n\
      :pianobar-output :pianobar-quit\n\n\
      :tidal-start :tidal-run :tidal-run-orbit :tidal-hush :tidal-stop-orbit\n\
      :tidal-output :tidal-quit\n\
      :streamlink :streamlink-qualities   for video",
    ),
    (
        "Wikis & the social web",
        ":confluence-page fetches a page by space and title over the REST API;\n\
      :confluence-search searches and :confluence-export exports.\n\n\
      :twitter shows the timeline, with :twitter-user :twitter-search and\n\
      :twitter-post.\n\
      :jabber-send sends an XMPP message to a JID, :jabber-send-muc posts to a\n\
      room and :jabber-accounts lists the configured accounts.",
    ),
    (
        "DjVu documents",
        ":djvu-text extracts a DjVu document's text, :djvu-pages lists the pages and\n\
      :djvu-outline the outline.  :djvu-occur searches the extracted text and\n\
      :djvu-export-page writes one page out.\n\n\
      The doc-view commands render the pages themselves — see Documents — and\n\
      :doc-view-clear-cache throws the rendered images away.",
    ),
    (
        "Colouring identifiers",
        ":rainbow-mode paints a colour literal in the colour it names.\n\n\
      :rainbow-identifiers-mode gives every identifier a colour derived from\n\
      its name, so the same name is always the same colour;\n\
      :global-rainbow-identifiers-mode does it everywhere.\n\
      :color-identifiers-mode limits that to what the grammar calls a variable,\n\
      with :global-color-identifiers-mode for every buffer.\n\
      :rainbow-identifiers-saturation and :rainbow-identifiers-lightness tune\n\
      how strong the colours are.",
    ),
    (
        "The minibuffer & prompts",
        ":icomplete-mode and :icomplete-vertical-mode show candidates while typing;\n\
      :fido-mode adds ido's habit of taking the top candidate on Enter.\n\
      :global-completion-preview-mode previews the top candidate inline in\n\
      every buffer.\n\n\
      :minibuffer-electric-default-mode hides the default once you type,\n\
      :minibuffer-depth-indicate-mode shows recursive prompts, and\n\
      :file-name-shadow-mode dims the part of a path that later components\n\
      override.\n\
      :temp-buffer-resize-mode sizes a temporary window to its contents.",
    ),
    (
        "Paragraph layout",
        ":set-justification-left :set-justification-right :set-justification-center\n\
      :set-justification-full :set-justification-none\n\
      :increase-left-margin :decrease-left-margin\n\
      :use-hard-newlines   mark typed newlines as hard, so filling keeps them\n\
      :paragraph-indent-minor-mode :paragraph-indent-text-mode\n\
                           treat an indented line as a new paragraph",
    ),
    (
        "Buffer menus",
        ":bs-show lists the buffers under the bs settings and :bs-customize changes\n\
      them; :msb-mode groups the mouse buffer menu by language.\n\
      :buffer-next and :buffer-previous cycle without the list.\n\
      :auto-compression-mode opens compressed files as though they were plain.",
    ),
    (
        "Bookmark files",
        ":bookmark-write saves the named bookmarks to a file and :bookmark-load\n\
      reads one back, so a set of positions can be kept per project or shared.\n\
      :multi-occur-in-matching-buffers lists matches across every buffer whose\n\
      name matches, which is the fastest way to survey what a bookmark set\n\
      points at.",
    ),
    (
        "Directory stack & filesets",
        ":shell-pushd-tohome :shell-pushd-dextract :shell-pushd-dunique change how\n\
      the shell's pushd behaves inside the editor's terminals.\n\
      :filesets-init loads the fileset definitions at startup and\n\
      :filesets-delete removes one.",
    ),
    (
        "Mail attachments",
        ":compose-mail-other-window opens the draft in a split instead of the\n\
      current window.  :mml-attach-file attaches a file as a MIME part, and\n\
      :message-kill-buffer throws the draft away without sending it.",
    ),
    (
        "Web widgets",
        ":xwidget-webkit-mode opens a real WebKit view where one is available, with\n\
      :xwidget-webkit-edit-mode for typing into the page and\n\
      :xwidget-webkit-isearch-mode for searching it.\n\n\
      :quickurl-browse shows a stored URL so it can be opened,\n\
      :search-engines lists the configured engines and their templates,\n\
      :bug-reference-url-format sets the tracker a bug number points at and\n\
      :debbugs-browse-mode points them at GNU debbugs.\n\
      :elfeed-show fetches one feed entry's page as text, and\n\
      :hackernews-item shows a story with its comments; :reddit-main opens the\n\
      front page.",
    ),
    (
        "Media playback in the buffer",
        ":image-previous-frame steps an animation back and :image-decrease-speed\n\
      slows it down, with :image-next-frame :image-goto-frame\n\
      :image-increase-speed :image-reverse-speed :image-reset-speed for the\n\
      rest.\n\n\
      :doc-view-previous-page :doc-view-first-page :doc-view-last-page walk a\n\
      document, :doc-view-show-tooltip explains what is under the cursor and\n\
      :doc-view-set-slice-using-mouse crops by dragging.",
    ),
    (
        "Man pages",
        ":Man opens a page in the run console.  :Man-next-manpage and\n\
      :Man-previous-manpage move between the pages a search matched, which is\n\
      how to walk section 1 and section 3 versions of the same name.",
    ),
    (
        "Perforce housekeeping",
        "Beyond the everyday :p4-edit and :p4-submit: :p4-refresh re-reads a file\n\
      from the depot, throwing local changes away, and :p4-files lists what the\n\
      depot holds.  :p4 runs any other p4 command.",
    ),
    (
        "Replace modes",
        ":startreplace enters Replace mode, where typing overwrites; :startgreplace\n\
      enters Virtual Replace mode, which keeps the layout of tabs.\n\
      R does the same from the keyboard, and :substitute is the version that\n\
      takes a pattern instead.",
    ),
    (
        "Diagnostics",
        ":yank-diagnostic copies the diagnostic under the cursor to a register or\n\
      the clipboard, which is what to use before searching for an error.\n\
      :lsp reports the language servers for this buffer and :lsp-health the\n\
      state of all of them.\n\
      :toggle-debug-on-error makes a failing command open a backtrace instead\n\
      of printing one line.",
    ),
    (
        "The build program",
        ":compiler selects what :make runs by setting makeprg — :compiler cargo, and\n\
      the error format follows.  :make then fills the quickfix list and\n\
      :lmake the location list.\n\
      :run runs the project in the Run tool window instead, which keeps the\n\
      output live rather than parsed.",
    ),
    (
        "Floating windows",
        ":fclose closes the topmost floating window — a picker, a popup or a panel —\n\
      without touching the buffer underneath, which is the reliable way out of\n\
      a stack of them.  Esc does the same from the keyboard.",
    ),
    (
        "Tab keys",
        ":tab-bar-select-tab-modifiers binds a modifier with 0 to 9 to select tabs\n\
      by number, so Alt-3 goes to the third tab; passing nil unbinds them.\n\
      :tab-switch takes a name or number, and :tabs lists what is open.",
    ),
    (
        "The snippet library",
        ":snippets opens the library editor, where a snippet's trigger, scope and\n\
      body are created, changed and deleted; the file behind it is\n\
      snippets.toml.  :Snippets picks one with the fuzzy finder and inserts it.",
    ),
    (
        "The calendar",
        ":calendar-hebrew-list-yahrzeits lists the Gregorian dates a Hebrew death\n\
      date falls on over a span of years.  Org's agenda covers the ordinary\n\
      calendar — see Org mode — and :date :datetime :timestamp insert today.",
    ),
    (
        "Version & intro",
        ":version shows the version and the compiled feature summary, which is what\n\
      a bug report needs; :emacs-version gives the emacs compatibility level\n\
      that the emacs commands report.\n\
      :intro shows the introductory message, and :log opens the log file.",
    ),
    (
        "Reviewing history",
        ":log-view-toggle-entry-display expands a commit in the log view to its full\n\
      message, and :vc-edit-next-command makes the next version-control command\n\
      prompt for its arguments instead of assuming them.\n\
      :BCommits picks a commit touching this file, and :compare-ref diffs the\n\
      buffer against any ref.",
    ),
    (
        "Structural regex commands",
        ":structural-x runs a command over every match of a pattern and\n\
      :structural-y over the stretches between them; :structural-X and\n\
      :structural-Y do the same across files, by file name.\n\
      :sx and :sX are the short names.  With no command the pieces are simply\n\
      selected — see Structural regular expressions for what that is for.",
    ),
    (
        "Where zmax keeps its files",
        "Everything lives under one dotted home directory rather than being spread\n\
      across XDG locations.\n\n\
      ~/.zmax/config.toml      settings, keys, themes\n\
      ~/.zmax/languages.toml   language and language-server configuration\n\
      ~/.zmax/snippets.toml    the snippet library\n\
      ~/.zmax/zmax.log         the log\n\
      ~/.zmax/runtime/         an overlay over the shipped runtime — themes,\n\
                               queries, grammars\n\n\
      A project can override both files from .zmax/config.toml and\n\
      .zmax/languages.toml in its own root.\n\
      :config-open :config-open-workspace :init-open :log-open open them, and\n\
      :config-reload applies a change without restarting.",
    ),
    (
        "Global, workspace & buffer settings",
        "Three levels, narrowest wins.\n\n\
      :set / :setglobal    everything, from now on\n\
      :setlocal            this buffer only\n\
      .zmax/config.toml    this project, every time it is opened\n\
      a Local Variables block   this file, wherever it is opened from —\n\
                                :add-file-local-variable writes one\n\n\
      A project's config and its language servers do not run until the tree is\n\
      trusted: :trust, or :workspace-exclude to stop being asked.",
    ),
    (
        "Coming from vim",
        "Set keymap = \"vim\" in config.toml, or run :keymap vim, and the keys are\n\
      vim's with nothing added.  The spacemacs default is the same keys plus a\n\
      SPC leader and the C-x prefix, so it is worth trying first.\n\n\
      What carries over: Ex commands and their ranges, registers and marks,\n\
      macros, text objects, :map and friends, autocommands, vimscript through\n\
      :vim, and the quickfix, location, tag, argument and buffer lists.\n\n\
      What is different: selections come first, so a cursor is a one-character\n\
      selection and every command acts on all of them; undo is a tree, not a\n\
      line; language servers do what a plugin used to.",
    ),
    (
        "Coming from emacs",
        "Set keymap = \"emacs\" for the modeless keys, or \"cua\" to get C-x cut, C-c\n\
      copy and C-v paste on top of them.\n\n\
      The command names are the emacs ones — :apropos :customize :bookmark-load\n\
      :transpose-words :fill-individual-paragraphs :string-rectangle :multi-occur\n\
      :abbrev-mode :desktop-save :shadow-copy-files — and the emacs minor modes\n\
      are here as commands too, listed under Minor modes worth knowing.\n\
      :elisp runs elisp in a compiled-in interpreter, with no emacs installed.\n\
      :emacs-version reports the compatibility level the emacs commands claim.",
    ),
    (
        "Coming from helix or kakoune",
        "keymap = \"helix\" is helix's own layout; keymap = \"kakoune\" is that model\n\
      with kakoune's key placement — v and V for the view, A-i and A-a for text\n\
      objects, Z z A-z for the selection registers.\n\n\
      zmax's editing core is the same selection-first model, so the muscle\n\
      memory transfers directly.  What is added: the whole Ex command surface,\n\
      the IDE workbench, the embedded interpreters, and structural regexes.",
    ),
    (
        "Coming from nano or micro",
        "keymap = \"nano\" gives ^O write out, ^W where is, ^K cut, ^U paste and ^X\n\
      exit; keymap = \"micro\" gives C-s save, C-q quit and C-e for the command\n\
      bar.  Neither has modes, so typing simply types.\n\n\
      Everything else in this Help is still reachable by name from the command\n\
      bar, which is the point of starting here: the keys stay familiar while\n\
      the commands are learned one at a time.",
    ),
    (
        "Coming from an IDE",
        "The commands with JetBrains names do what those menu items do.\n\n\
      :ide / :zen            the workbench, and focus mode\n\
      :RecentLocations       the jump ring, newest first, with context\n\
      :LocalHistory          snapshots of this file, kept by the editor\n\
      :Todo                  TODO markers across the tree\n\
      :project-replace       replace in files\n\
      :compare-ref           compare with branch\n\
      :edit-fragment / :inject-language   edit an injected language\n\
      :RevealInFinder        reveal in file manager\n\
      :run / :debug-start    the run and debug configurations, which\n\
                             Preferences ▸ Run Configs edits",
    ),
    (
        "Reporting a problem",
        ":version         the version and the compiled feature summary\n\
      :checkhealth     clipboard, language servers, grammars\n\
      :lsp-health      which servers are ready, and what they support\n\
      :log             the log file, read-only\n\
      :toggle-debug-on-error   make a failing command show its backtrace\n\
      :profile / :syntime      when the problem is that something is slow\n\n\
      A report is most useful with the :version output, what was typed, and the\n\
      last lines of the log.",
    ),
    (
        "Ranges & counts",
        "Every Ex command takes a range: :10,20delete-lines, :%format,\n\
      :.,+5indent-lines, and 'a,'b for a marked span.  % is the whole buffer\n\
      and . is the cursor's line.\n\n\
      :global and :vglobal turn a pattern into the range instead, and\n\
      :structural-x turns a pattern into many ranges — see Structural regular\n\
      expressions.\n\
      From the keyboard, a count goes before the operator: 3dd, 5j, 2ciw.",
    ),
    (
        "In process or out",
        "Anything that shells out costs a process; zmax has an in-process route for\n\
      most of it.\n\n\
      | and :pipe run a shell command per selection\n\
      :xpipe runs a chain of compiled-in interpreter stages instead, with no\n\
      process at all — the awk, ruby, php, python and stryke engines are\n\
      linked in\n\
      :sandbox runs something that should not be trusted with either\n\n\
      The text tools in this Help — sorting, JSON, fields, encodings — are\n\
      native Rust, so they are the cheapest option of the three.",
    ),
    (
        "Live coding",
        ":extempore-run starts the Extempore binary and :extempore-connect attaches\n\
      to one already running over TCP; :extempore-send-definition\n\
      :extempore-send-region :extempore-send-buffer evaluate into the running\n\
      process and :extempore-disconnect lets go.\n\n\
      :tidal-start and :tidal-run do the same for TidalCycles patterns, and\n\
      :alda-play-buffer for written scores.",
    ),
    (
        "Locale & the editor process",
        ":language sets the locale for the editor and every process it starts, and\n\
      reports it when given no argument; :encoding is the buffer's encoding\n\
      rather than the process's.\n\n\
      :getenv and :setenv change any other variable the same way, which is how\n\
      :conda-activate and :nvm-use work.\n\
      :normal-erase-is-backspace-mode swaps which of Backspace and Delete\n\
      erases backwards, when the terminal disagrees with the keyboard.",
    ),
    (
        "Sharing a link to the code",
        ":reveal opens this repository's page on GitHub, GitLab, Bitbucket or\n\
      wherever its remote points, which is the fastest way to hand someone a\n\
      link.  :browse-url opens any other URL, :quickurl-add stores one under a\n\
      name and :quickurl recalls it.\n\
      :move moves the buffer and its file to another path, keeping the buffer\n\
      open on it.",
    ),
    (
        "Odds & ends",
        "Commands that fit nowhere else but are worth knowing.\n\n\
      :split-on <sep>      split the selected lines into one item per separator\n\
      :org-move-subtree-down / :org-move-subtree-up\n\
      :dotnet-sln-add :dotnet-sln-remove :dotnet-sln-list   solution membership\n\
      :transmission-stop :transmission-verify\n\
      :0debuggreedy        undo :debuggreedy\n\
      :2match :3match      the second and third highlight groups\n\
      :tutor               the tutorial, which is the one to run first",
    ),
    (
        "AI assistance",
        "The AI commands work on the selection, or the buffer when nothing is\n\
      selected.  ai_chat asks about it, ai_inline_edit rewrites it in place with\n\
      ai_accept_edit confirming the preview, ai_explain explains it and\n\
      ai_generate_tests writes tests for it.  ai_fix takes the diagnostics on\n\
      the line, ai_commit_message writes the commit, ai_terminal_command turns a\n\
      sentence into a shell command, and ai_agent runs a task on its own.\n\
      The context commands add material for the model to read.\n\n\
      The whole family, under SPC a:\nai_accept_edit ai_add_file_context ai_agent ai_agent_review ai_apply_block\n\
      ai_chat ai_chat_panel ai_codebase_context ai_commit_message ai_complete\n\
      ai_docs_context ai_explain ai_fix ai_generate_tests ai_inline_edit\n\
      ai_inline_edit_preview ai_model_picker ai_revert_agent ai_symbol_context\n\
      ai_terminal_command ai_web_context",
    ),
    (
        "Keyboard macros",
        "Recording and replaying keystrokes, with a ring of the recent macros rather\n\
      than one slot.  kmacro_end_or_call_macro (F4) ends the recording, or calls\n\
      the last macro when nothing is being recorded, and kmacro_bind_to_key\n\
      reports the config line that would bind it permanently.\n\nkmacro_add_counter kmacro_bind_to_key kmacro_call_ring_2nd\n\
      kmacro_edit_lossage kmacro_edit_macro kmacro_end_macro\n\
      kmacro_end_or_call_macro kmacro_end_or_call_macro_repeat\n\
      kmacro_insert_counter kmacro_name_last_macro kmacro_redisplay\n\
      kmacro_ring_delete kmacro_ring_next kmacro_ring_prev kmacro_ring_swap\n\
      kmacro_ring_view kmacro_set_counter kmacro_set_format\n\
      kmacro_start_macro_or_insert_counter kmacro_step_edit_macro\n\
      kmacro_to_register",
    ),
    (
        "The macro ring",
        "kmacro_menu lists the recorded macros so they can be marked, deleted,\n\
      copied and edited like any other list.\n\nkmacro_menu kmacro_menu_do_copy kmacro_menu_do_delete\n\
      kmacro_menu_do_flagged_delete kmacro_menu_edit_column\n\
      kmacro_menu_edit_counter kmacro_menu_edit_format kmacro_menu_edit_keys\n\
      kmacro_menu_edit_position kmacro_menu_flag_for_deletion kmacro_menu_mark\n\
      kmacro_menu_transpose kmacro_menu_unmark kmacro_menu_unmark_all\n\
      kmacro_menu_unmark_backward",
    ),
    (
        "Incremental search",
        "Emacs-style search that narrows as you type, with the search string itself\n\
      as the thing being edited.  isearch_occur turns the current pattern into a\n\
      list of every match, and isearch_query_replace hands it to a replace,\n\
      which is why the search is worth finishing before deciding what to do.\n\nisearch_abort isearch_cancel isearch_char_by_name isearch_complete\n\
      isearch_del_char isearch_delete_char isearch_edit_string\n\
      isearch_emoji_by_name isearch_exit isearch_forward_symbol\n\
      isearch_forward_symbol_at_point isearch_forward_thing_at_point\n\
      isearch_forward_word isearch_help_map\n\
      isearch_highlight_lines_matching_regexp isearch_highlight_regexp\n\
      isearch_occur isearch_query_replace isearch_query_replace_regexp\n\
      isearch_quote_char isearch_ring_advance isearch_ring_retreat\n\
      isearch_toggle_case_fold isearch_toggle_char_fold\n\
      isearch_toggle_input_method isearch_toggle_invisible\n\
      isearch_toggle_lax_whitespace isearch_toggle_regexp\n\
      isearch_toggle_specified_input_method isearch_toggle_symbol\n\
      isearch_toggle_word isearch_transient_input_method isearch_yank_char\n\
      isearch_yank_kill isearch_yank_line isearch_yank_pop isearch_yank_symbol\n\
      isearch_yank_symbol_or_char isearch_yank_until_char isearch_yank_word\n\
      isearch_yank_word_or_char isearch_yank_x_selection",
    ),
    (
        "Drawing with picture mode",
        "picture_mode makes the buffer behave like a drawing surface: typing\n\
      overwrites rather than inserts, and the movement commands set which way\n\
      the cursor travels after each character — including the four diagonals,\n\
      which is what makes box and arrow drawing practical.\n\npicture_backward_clear_column picture_clear_column picture_clear_line\n\
      picture_clear_rectangle_to_register picture_mode picture_motion\n\
      picture_motion_reverse picture_movement_down picture_movement_left\n\
      picture_movement_ne picture_movement_nw picture_movement_right\n\
      picture_movement_se picture_movement_sw picture_movement_up\n\
      picture_open_line picture_set_tab_stops picture_tab picture_tab_search\n\
      picture_yank_rectangle picture_yank_rectangle_from_register",
    ),
    (
        "ASCII tables",
        "table_recognize finds the table drawn at the cursor and treats it as a\n\
      table from then on, so cells reflow and split as they are edited;\n\
      table_capture turns plain text into one, and table_release turns it back.\n\ntable_capture table_fixed_width_mode table_generate_source\n\
      table_heighten_cell table_insert_sequence table_justify table_narrow_cell\n\
      table_query_dimension table_recognize table_recognize_cell\n\
      table_recognize_region table_recognize_table table_release\n\
      table_shorten_cell table_span_cell table_split_cell\n\
      table_split_cell_horizontally table_split_cell_vertically\n\
      table_to_csv_selection table_unrecognize table_unrecognize_cell\n\
      table_unrecognize_region table_unrecognize_table table_widen_cell",
    ),
    (
        "Aligning at a character",
        "align_current aligns the region into columns section by section, and the\n\
      align_at_* commands align on one specific character without asking.\n\
      align_at_regex takes any pattern; the typable :align does the same from\n\
      the command line.\n\nalign_at_ampersand align_at_arithmetic align_at_bar align_at_colon\n\
      align_at_comma align_at_dot align_at_equals align_at_lbrace\n\
      align_at_lbracket align_at_lparen align_at_rbrace align_at_rbracket\n\
      align_at_regex align_at_rparen align_at_semicolon align_current\n\
      align_entire align_highlight_rule align_left_at_char align_right_at_char\n\
      align_selections align_unhighlight_rule",
    ),
    (
        "Process buffers",
        "comint is the machinery behind every buffer with a program running in it —\n\
      a shell, a REPL, a debugger.  comint_run starts one, and the rest are the\n\
      keys that make such a buffer usable: history, prompts, signals, and\n\
      getting at what was typed or printed before.\n\ncomint_bol_or_process_mark comint_completion_at_point\n\
      comint_continue_subjob comint_copy_old_input comint_delchar_or_maybe_eof\n\
      comint_delete_output comint_dynamic_list_filename\n\
      comint_dynamic_list_input_ring comint_get_next_from_history\n\
      comint_history_isearch_backward_regexp comint_insert_previous_argument\n\
      comint_interrupt_subjob comint_kill_input comint_kill_subjob\n\
      comint_magic_space comint_next_prompt comint_previous_prompt\n\
      comint_quit_subjob comint_run comint_send_invisible comint_shell\n\
      comint_show_maximum_output comint_show_output comint_stop_subjob\n\
      comint_strip_ctrl_m comint_truncate_buffer comint_write_output",
    ),
    (
        "The calendar",
        "A calendar that moves by day, week, month and year, and knows about the\n\
      other calendars in use around the world — Hebrew, Islamic, Chinese,\n\
      Bahai, Coptic, Ethiopic, French Revolutionary, Julian, Mayan, Persian and\n\
      ISO — with a goto and a print command for each.\n\ncalendar_bahai_goto_date calendar_bahai_print_date\n\
      calendar_chinese_goto_date calendar_chinese_print_date\n\
      calendar_coptic_goto_date calendar_coptic_print_date\n\
      calendar_count_days_region calendar_day_of_year\n\
      calendar_ethiopic_goto_date calendar_ethiopic_print_date\n\
      calendar_french_goto_date calendar_french_print_date\n\
      calendar_goto_day_of_year calendar_hebrew_goto_date\n\
      calendar_hebrew_print_date calendar_islamic_goto_date\n\
      calendar_islamic_print_date calendar_iso_goto_week calendar_iso_print_date\n\
      calendar_julian_goto_date calendar_julian_print_date calendar_mark_today\n\
      calendar_mayan_goto_long_count calendar_mayan_print_date\n\
      calendar_other_month calendar_persian_goto_date\n\
      calendar_persian_print_date calendar_print_other_dates calendar_redraw\n\
      calendar_scroll_left calendar_scroll_right calendar_set_date_style\n\
      calendar_star_date calendar_unmark",
    ),
    (
        "Holidays & the sky",
        "The holiday commands list what falls in a range and mark it on the\n\
      calendar; the astronomical ones answer the questions a calendar cannot —\n\
      sunrise, sunset, the phases of the moon, the equinoxes and solstices.\n\ncalendar_astro_goto_day_number calendar_astro_print_day_number\n\
      calendar_list_holidays calendar_lunar_phases calendar_sunrise_sunset\n\
      diary_astro_day_number diary_lunar_phases diary_sunrise_sunset\n\
      holiday_list",
    ),
    (
        "Diary entries",
        "A diary is a plain text file of dated lines; these commands insert an entry\n\
      for the day, the week, the month, the year or an anniversary, in whichever\n\
      calendar the date belongs to.\n\ndiary_bahai_insert_anniversary_entry diary_bahai_insert_entry\n\
      diary_bahai_insert_monthly_entry diary_bahai_insert_yearly_entry\n\
      diary_chinese_insert_anniversary_entry diary_chinese_insert_entry\n\
      diary_chinese_insert_monthly_entry diary_chinese_insert_yearly_entry\n\
      diary_hebrew_insert_anniversary_entry diary_hebrew_insert_entry\n\
      diary_hebrew_insert_monthly_entry diary_hebrew_insert_yearly_entry\n\
      diary_insert_anniversary_entry diary_insert_block_entry\n\
      diary_insert_cyclic_entry diary_insert_entry diary_insert_monthly_entry\n\
      diary_insert_weekly_entry diary_insert_yearly_entry\n\
      diary_islamic_insert_anniversary_entry diary_islamic_insert_entry\n\
      diary_islamic_insert_monthly_entry diary_islamic_insert_yearly_entry",
    ),
    (
        "Reading the diary",
        "The listing and marking commands pull the day's entries out of the file and\n\
      show them next to the calendar; diary_fancy_display is the readable form.\n\ndiary_bahai_date diary_bahai_list_entries diary_bahai_mark_entries\n\
      diary_chinese_date diary_chinese_list_entries diary_chinese_mark_entries\n\
      diary_coptic_date diary_day_of_year diary_ethiopic_date\n\
      diary_fancy_display diary_french_date diary_hebrew_birthday\n\
      diary_hebrew_date diary_hebrew_list_entries diary_hebrew_mark_entries\n\
      diary_hebrew_omer diary_hebrew_parasha diary_hebrew_rosh_hodesh\n\
      diary_hebrew_sabbath_candles diary_hebrew_yahrzeit\n\
      diary_include_other_diary_files diary_islamic_date\n\
      diary_islamic_list_entries diary_islamic_mark_entries diary_list_entries\n\
      diary_mail_entries diary_mark_entries diary_mark_included_diary_files\n\
      diary_print_entries diary_show_all_entries diary_simple_display\n\
      diary_sort_entries diary_view_entries",
    ),
    (
        "Version control, the emacs way",
        "vc_next_action is the whole of version control in one key: it does whatever\n\
      the file's state calls for next — register it, stage it, commit it.  The\n\
      rest are the commands for when that is not what you want.\n\nvc_annotate vc_create_branch vc_create_tag vc_delete_file vc_dir\n\
      vc_dir_mark vc_dir_mark_all_files vc_dir_mark_by_regexp\n\
      vc_dir_mark_registered_files vc_ediff vc_ignore vc_insert_headers\n\
      vc_log_incoming vc_log_outgoing vc_log_search vc_next_action\n\
      vc_prepare_patch vc_print_branch_log vc_print_log vc_print_root_log\n\
      vc_pull vc_push vc_refresh_state vc_region_history vc_register\n\
      vc_rename_file vc_retrieve_tag vc_revert vc_root_diff vc_state_refresh\n\
      vc_switch_branch vc_update_change_log",
    ),
    (
        "The package menu",
        "The package list, with the marking-and-executing habit the rest of emacs\n\
      uses: mark what should be installed or deleted, then execute the marks.\n\npackage_activate_all package_browse_url package_delete package_install\n\
      package_install_file package_menu_describe_package package_menu_execute\n\
      package_menu_filter_by_archive package_menu_filter_by_description\n\
      package_menu_filter_by_keyword package_menu_filter_by_name\n\
      package_menu_filter_by_name_or_description package_menu_filter_by_regexp\n\
      package_menu_filter_by_status package_menu_filter_by_version\n\
      package_menu_filter_clear package_menu_filter_marked\n\
      package_menu_filter_upgradable package_menu_hide_package\n\
      package_menu_mark_delete package_menu_mark_install\n\
      package_menu_mark_obsolete_for_deletion package_menu_mark_unmark\n\
      package_menu_mark_upgrades package_menu_quick_help\n\
      package_menu_sort_by_name package_menu_sort_by_stars\n\
      package_menu_sort_by_status package_menu_toggle_hiding\n\
      package_quickstart_refresh package_recompile package_recompile_all\n\
      package_refresh_contents package_report_bug package_search package_upgrade\n\
      package_upgrade_all package_vc_checkout package_vc_install\n\
      package_vc_install_from_checkout package_vc_prepare_patch\n\
      package_vc_rebuild",
    ),
    (
        "Ediff",
        "A two- or three-way diff driven from a control panel rather than a buffer,\n\
      moving hunk by hunk and copying between the sides.\n\nediff_3_buffers ediff_3_files ediff_buffer ediff_directories\n\
      ediff_directories3 ediff_directory_revisions ediff_documentation\n\
      ediff_dotfile_and_template ediff_file ediff_merge_directories\n\
      ediff_merge_directories_with_ancestor ediff_merge_file\n\
      ediff_merge_revisions ediff_merge_revisions_with_ancestor\n\
      ediff_patch_buffer ediff_regions ediff_regions_wordwise\n\
      ediff_show_registry ediff_windows",
    ),
    (
        "Debug adapter commands",
        "The DAP session's own commands, which are what the Debug tab and the F-keys\n\
      call.  dap_launch starts the target and dap_toggle_breakpoint is on F9.\n\ndap_breakpoints_picker dap_continue dap_disable_exceptions\n\
      dap_edit_condition dap_edit_log dap_enable_exceptions dap_launch dap_next\n\
      dap_pause dap_remove_breakpoint dap_restart dap_run_to_cursor dap_step_in\n\
      dap_step_out dap_switch_stack_frame dap_switch_thread dap_terminate\n\
      dap_toggle_breakpoint dap_variables",
    ),
    (
        "Gnus commands",
        "The newsreader's group, summary and article commands.\n\ngnus_group_exit gnus_group_kill_group gnus_group_list_all_groups\n\
      gnus_group_list_groups gnus_group_list_killed gnus_group_list_zombies\n\
      gnus_group_next_unread_group gnus_group_prev_unread_group\n\
      gnus_group_read_group gnus_group_toggle_subscription_at_point\n\
      gnus_summary_exit gnus_summary_isearch_article gnus_summary_next_page\n\
      gnus_summary_next_unread_article gnus_summary_prev_page\n\
      gnus_summary_prev_unread_article gnus_summary_search_article_backward\n\
      gnus_summary_search_article_forward",
    ),
    (
        "Moving between windows",
        "windmove picks the window by direction rather than by cycling, so the\n\
      window above is always the same key regardless of how many are open.\n\nwindmove_default_keybindings windmove_delete_default_keybindings\n\
      windmove_delete_down windmove_delete_left windmove_delete_right\n\
      windmove_delete_up windmove_display_default_keybindings\n\
      windmove_display_down windmove_display_left windmove_display_new_frame\n\
      windmove_display_new_tab windmove_display_right\n\
      windmove_display_same_window windmove_display_up\n\
      windmove_swap_states_default_keybindings",
    ),
    (
        "Window layouts",
        "A layout is the arrangement of windows and what is in them.  layout_save\n\
      writes them to disk, so a project's arrangement survives a restart.\n\nlayout_add_buffers layout_create layout_default layout_delete\n\
      layout_goto_1 layout_goto_2 layout_goto_3 layout_goto_4 layout_goto_5\n\
      layout_goto_6 layout_goto_7 layout_goto_8 layout_goto_9 layout_last\n\
      layout_load layout_next layout_prev layout_rename layout_save",
    ),
    (
        "Outline mode",
        "Headings and bodies in any language, folded by level.  outline_show_all\n\
      reveals everything; the rest hide and show by level, by subtree, or by\n\
      what is at the cursor.\n\noutline_backward_same_level outline_cycle outline_cycle_buffer\n\
      outline_forward_same_level outline_hide_body\n\
      outline_hide_by_heading_regexp outline_hide_entry outline_hide_leaves\n\
      outline_hide_other outline_hide_sublevels outline_hide_subtree\n\
      outline_minor_mode outline_mode outline_next_visible_heading\n\
      outline_previous_visible_heading outline_show_all outline_show_branches\n\
      outline_show_by_heading_regexp outline_show_children outline_show_entry\n\
      outline_show_subtree outline_up_heading",
    ),
    (
        "The mouse",
        "The mouse is fully wired, in the emacs sense rather than the IDE sense:\n\
      mouse_save_then_kill extends the region to the click and copies it, and\n\
      clicking again kills it.\n\nmouse_avoidance_mode mouse_buffer_menu mouse_goto_tag mouse_paste_after\n\
      mouse_paste_before mouse_pop_tag mouse_save_then_kill mouse_scroll_left\n\
      mouse_scroll_page_down mouse_scroll_page_left mouse_scroll_page_right\n\
      mouse_scroll_page_up mouse_scroll_right mouse_search_word_backward\n\
      mouse_search_word_forward mouse_secondary_save_then_kill\n\
      mouse_select_window mouse_set_point mouse_set_region mouse_set_secondary\n\
      mouse_split_window_horizontally mouse_split_window_vertically\n\
      mouse_start_secondary mouse_wheel_mode mouse_wheel_text_scale\n\
      mouse_yank_at_click mouse_yank_primary mouse_yank_secondary",
    ),
    (
        "Describing what is bound",
        "describe_key (C-h k) takes a key and reports the command it runs with its\n\
      documentation, which is the fastest way to answer 'what did I just press'.\n\
      The rest describe the other things a key could have been.\n\ndescribe_bindings describe_categories describe_char describe_character_set\n\
      describe_coding_system describe_command describe_copying\n\
      describe_current_modes describe_diagnostics_checker describe_distribution\n\
      describe_face describe_fontset describe_function describe_gnu_project\n\
      describe_input_method describe_key describe_key_briefly describe_keymap\n\
      describe_language_environment describe_language_package\n\
      describe_no_warranty describe_package describe_prefix_bindings\n\
      describe_repeat_maps describe_symbol describe_syntax\n\
      describe_text_properties describe_variable",
    ),
    (
        "Going to a place in the file",
        "The goto commands that take a position rather than a kind of thing.\n\ngoto_address_mode goto_buffer_window goto_byte goto_char goto_column\n\
      goto_define_from_cursor goto_define_from_start goto_file goto_file_end\n\
      goto_file_hsplit goto_file_new_tab goto_file_other_frame\n\
      goto_file_readonly goto_file_start goto_file_vsplit goto_followup_to\n\
      goto_keyword_line_from_cursor goto_keyword_line_from_start goto_line\n\
      goto_line_end goto_line_end_newline goto_line_middle goto_line_start\n\
      goto_mark goto_mark_line goto_mark_line_nojump goto_mark_nojump\n\
      goto_newer_change goto_numbered_bookmark goto_older_change\n\
      goto_preview_window goto_reply_to goto_visual_line_end\n\
      goto_visual_line_start goto_window_1 goto_window_2 goto_window_3\n\
      goto_window_4 goto_window_5 goto_window_6 goto_window_7 goto_window_8\n\
      goto_window_9 goto_window_bottom goto_window_center goto_window_top\n\
      goto_word",
    ),
    (
        "Going to the next thing",
        "Every one of these takes a kind of syntactic object and moves to the next\n\
      or previous one, so they compose with counts the way w and b do.\n\ngoto_next_buffer goto_next_change goto_next_class goto_next_close_paren\n\
      goto_next_comment goto_next_conflict goto_next_diag goto_next_entry\n\
      goto_next_function goto_next_mark goto_next_mark_line goto_next_paragraph\n\
      goto_next_parameter goto_next_preproc goto_next_section\n\
      goto_next_spell_error goto_next_tabpage goto_next_tabstop goto_next_test\n\
      goto_next_unmatched_brace goto_next_unmatched_paren goto_next_xml_element\n\
      goto_prev_change goto_prev_class goto_prev_comment goto_prev_conflict\n\
      goto_prev_diag goto_prev_entry goto_prev_function goto_prev_mark\n\
      goto_prev_mark_line goto_prev_open_paren goto_prev_paragraph\n\
      goto_prev_parameter goto_prev_preproc goto_prev_section\n\
      goto_prev_spell_error goto_prev_tabstop goto_prev_test\n\
      goto_prev_unmatched_brace goto_prev_unmatched_paren goto_prev_xml_element\n\
      goto_previous_buffer goto_previous_tabpage",
    ),
    (
        "Going to the definition",
        "What a language server answers, plus the first and last of the things the\n\
      editor tracks itself — changes, diagnostics, tab pages, modified files.\n\ngoto_declaration goto_definition goto_first_change goto_first_diag\n\
      goto_first_nonwhitespace goto_first_nonwhitespace_down goto_first_tabpage\n\
      goto_implementation goto_last_accessed_file goto_last_change\n\
      goto_last_diag goto_last_line goto_last_modification\n\
      goto_last_modified_file goto_last_tabpage goto_line_last_nonblank\n\
      goto_reference goto_type_definition goto_visual_first_nonwhitespace",
    ),
    (
        "Selecting things",
        "A selection is the unit every command acts on, so most of the work is in\n\
      making the right one.\n\nselect_all select_all_children select_all_instances select_all_occurrences\n\
      select_all_siblings select_down_key select_end_key select_first_last_chars\n\
      select_frame_by_name select_gn_match select_gn_match_prev select_home_key\n\
      select_in select_left_key select_line_above select_line_below select_mode\n\
      select_next_sibling select_page_down_key select_page_up_key\n\
      select_paragraph_backward_vim select_paragraph_backward_vim_linewise\n\
      select_paragraph_forward_vim select_paragraph_forward_vim_linewise\n\
      select_pasted_text select_prev_sibling\n\
      select_references_to_symbol_under_cursor select_regex select_register\n\
      select_right_key select_textobject_around select_textobject_inner\n\
      select_up_key",
    ),
    (
        "Extending a selection",
        "Every movement has an extend form that keeps the anchor where it is, which\n\
      is how a selection grows without leaving normal mode.\n\nextend_backward_exclusive_vim extend_char_left extend_char_right\n\
      extend_chars_left_vim extend_chars_right_vim extend_forward_exclusive_vim\n\
      extend_line extend_line_above extend_line_above_linewise extend_line_below\n\
      extend_line_below_linewise extend_line_down extend_line_up\n\
      extend_next_char extend_next_long_word_end extend_next_long_word_start\n\
      extend_next_paragraph extend_next_sub_word_end extend_next_sub_word_start\n\
      extend_next_word_end extend_next_word_start extend_page_down\n\
      extend_page_up extend_parent_node_end extend_parent_node_start\n\
      extend_prev_char extend_prev_long_word_end extend_prev_long_word_start\n\
      extend_prev_paragraph extend_prev_sub_word_end extend_prev_sub_word_start\n\
      extend_prev_word_end extend_prev_word_start extend_search_next\n\
      extend_search_next_vim extend_search_prev extend_search_prev_vim\n\
      extend_till_char extend_till_prev_char extend_to_char extend_to_column\n\
      extend_to_file_end extend_to_file_start extend_to_first_nonwhitespace\n\
      extend_to_last_line extend_to_line_bounds extend_to_line_end\n\
      extend_to_line_end_newline extend_to_line_start extend_to_visual_line_end\n\
      extend_to_visual_line_start extend_to_word extend_visual_line_down\n\
      extend_visual_line_up",
    ),
    (
        "Moving the cursor",
        "The movement primitives themselves, by character, word, line, paragraph and\n\
      visual line.\n\nmove_char_left move_char_right move_element_left move_element_right\n\
      move_file_refactor move_file_to_trash move_line_down move_line_up\n\
      move_next_long_word_end move_next_long_word_start move_next_sub_word_end\n\
      move_next_sub_word_start move_next_word_end move_next_word_start\n\
      move_parent_node_end move_parent_node_start move_prev_long_word_end\n\
      move_prev_long_word_start move_prev_sub_word_end move_prev_sub_word_start\n\
      move_prev_word_end move_prev_word_start move_sentence_backward\n\
      move_sentence_forward move_text_line_down move_text_line_up\n\
      move_to_opposite_group move_to_window_line_top_bottom\n\
      move_visual_line_down move_visual_line_up",
    ),
    (
        "Copying & killing",
        "Copying to a register, to the clipboard, and to the emacs kill ring, which\n\
      are three different places with three sets of commands.\n\ncopy_all_buffer_links copy_as_format copy_as_format_asciidoc\n\
      copy_as_format_bitbucket copy_as_format_disqus copy_as_format_github\n\
      copy_as_format_gitlab copy_as_format_hipchat copy_as_format_html\n\
      copy_as_format_jira copy_as_format_markdown copy_as_format_mediawiki\n\
      copy_as_format_org_mode copy_as_format_pod copy_as_format_rst\n\
      copy_as_format_slack copy_as_format_telegram copy_as_format_whatsapp\n\
      copy_between_registers copy_char_above copy_char_below copy_diagnostic\n\
      copy_dir_locals_to_file_locals copy_dir_locals_to_file_locals_prop_line\n\
      copy_file copy_file_locals_to_dir_locals copy_indent copy_last_keys\n\
      copy_rectangle_as_kill copy_rectangle_to_register copy_reference\n\
      copy_region_as_kill copy_remote_url copy_selection_on_next_line\n\
      copy_selection_on_prev_line copy_system_info copy_to_register copy_version\n\
      kill_buffers_by_regex kill_compilation kill_local_variable kill_rectangle\n\
      kill_ring_deindent_mode kill_sentence kill_sexp kill_some_buffers\n\
      kill_to_line_end kill_to_line_start kill_whole_line yank_file_dir\n\
      yank_file_name yank_file_path yank_file_path_with_line\n\
      yank_file_path_with_line_col yank_find_char_backward\n\
      yank_find_char_forward yank_from_kill_ring yank_joined\n\
      yank_joined_to_clipboard yank_joined_to_primary_clipboard\n\
      yank_main_selection_to_clipboard yank_main_selection_to_primary_clipboard\n\
      yank_no_trailing_whitespace yank_pop yank_rectangle yank_textobject\n\
      yank_textobject_around yank_textobject_inner yank_till_char_backward\n\
      yank_till_char_forward yank_to_clipboard yank_to_mark yank_to_mark_line\n\
      yank_to_primary_clipboard yank_to_search_backward yank_to_search_forward",
    ),
    (
        "Deleting",
        "Deleting by object, by direction and by what surrounds the cursor.\n\ndelete_char_backward delete_char_forward delete_chars_backward_vim\n\
      delete_chars_forward_vim delete_dir_local_variable delete_file\n\
      delete_find_char_backward delete_find_char_forward delete_frame\n\
      delete_other_frames delete_rectangle delete_selection\n\
      delete_selection_linewise delete_selection_mode delete_selection_noyank\n\
      delete_textobject_around delete_textobject_inner delete_till_char_backward\n\
      delete_till_char_forward delete_to_mark delete_to_mark_line\n\
      delete_to_search_backward delete_to_search_forward\n\
      delete_whitespace_rectangle delete_window_and_buffer delete_word_backward\n\
      delete_word_forward",
    ),
    (
        "Toggles",
        "Everything that is either on or off, one command each — there is no\n\
      settings dialog to hunt through, and each of these is bindable.\n\ntoggle_abbrev_mode toggle_ai_autocomplete toggle_ai_privacy\n\
      toggle_auto_completion toggle_auto_fill toggle_auto_highlight\n\
      toggle_auto_reveal toggle_auto_revert toggle_blame_annotate\n\
      toggle_block_comments toggle_bottom_zoom toggle_centered_cursor\n\
      toggle_column_indexing toggle_comments toggle_diagnostics\n\
      toggle_drawer_mid toggle_electric_pair toggle_fill_column\n\
      toggle_follow_mode toggle_frame_fullscreen toggle_frame_maximized\n\
      toggle_frame_tab_bar toggle_fringe toggle_hl_line toggle_ide\n\
      toggle_indent_guides toggle_inlay_hints toggle_inline_blame\n\
      toggle_input_method toggle_lang_keymap toggle_line_comments\n\
      toggle_line_numbers toggle_long_line_marker toggle_modeline_major_mode\n\
      toggle_modeline_minor_modes toggle_modeline_new_version\n\
      toggle_modeline_org_clock toggle_modeline_position\n\
      toggle_modeline_responsive toggle_modeline_vcs toggle_readonly\n\
      toggle_replace_mode toggle_revins toggle_scroll_bar\n\
      toggle_smooth_scrolling toggle_soft_wrap toggle_subword toggle_superword\n\
      toggle_syntax_highlighting toggle_system_monitor toggle_test_file\n\
      toggle_tilde_fringe toggle_value_selection toggle_whitespace_render\n\
      toggle_window_dedication",
    ),
    (
        "Folding commands",
        "Folds by level, by syntax and by hand.\n\nfold_close fold_close_all fold_close_recursive fold_comments fold_create\n\
      fold_delete fold_delete_all fold_less fold_more fold_next fold_open\n\
      fold_open_all fold_open_recursive fold_prev fold_toggle",
    ),
    (
        "C and C++ helpers",
        "The cc-mode commands: indentation by syntactic context, moving by\n\
      preprocessor conditional, and the electric characters that reindent as\n\
      they are typed.\n\nc_backslash_region c_backward_conditional c_beginning_of_defun\n\
      c_beginning_of_statement c_context_line_break c_end_of_defun\n\
      c_end_of_statement c_fill_paragraph c_forward_conditional c_guess\n\
      c_guess_install c_hungry_delete_backwards c_hungry_delete_forward\n\
      c_indent_defun c_indent_exp c_indent_line_or_region c_macro_expand\n\
      c_mark_function c_set_style c_show_syntactic_information\n\
      c_toggle_auto_newline c_toggle_electric_state c_toggle_hungry_state\n\
      c_ts_mode_indent_defun c_ts_mode_set_style c_up_conditional",
    ),
    (
        "TeX helpers",
        "Environments, sections, macros and the compile-view cycle.\n\nlatex_close_block latex_electric_env_pair_mode latex_insert_block\n\
      latex_mode tex_bibtex_file tex_buffer tex_compile tex_file\n\
      tex_insert_braces tex_insert_quote tex_kill_job tex_mode tex_print\n\
      tex_recenter_output_buffer tex_region tex_terminate_paragraph tex_validate\n\
      tex_view",
    ),
    (
        "Org commands",
        "The org commands as keys rather than as : commands — the same operations\n\
      the Org mode topic lists, bound where org binds them.\n\norg_agenda org_capture org_clock_toggle org_cycle org_deadline org_demote\n\
      org_fold_all org_metadown org_metaleft org_metaright org_metaup org_mode\n\
      org_next_heading org_prev_heading org_priority org_promote org_schedule\n\
      org_shifttab org_todo org_unfold_all",
    ),
    (
        "Inserting",
        "Insert mode's own commands, and the ones that insert something computed.\n\ninsert_abbrevs insert_at_last_insert insert_at_line_end\n\
      insert_at_line_start insert_char_by_code insert_char_interactive\n\
      insert_command_normal insert_digraph insert_kbd_macro\n\
      insert_kill_entered_vim insert_last_inserted_and_stop\n\
      insert_last_inserted_text insert_lorem_list insert_lorem_paragraph\n\
      insert_lorem_sentence insert_mode insert_newline insert_password_numerical\n\
      insert_password_paranoid insert_password_phonetic insert_password_simple\n\
      insert_password_strong insert_register insert_spell_suggest insert_tab\n\
      insert_toc insert_unindent insert_uuid_v1 insert_uuid_v4",
    ),
    (
        "Scrolling & the view",
        "Moving the text under the cursor rather than the cursor through the text.\n\nalign_view_bottom align_view_center align_view_middle align_view_top\n\
      scroll_bar_drag scroll_bar_mode scroll_column_left scroll_column_right\n\
      scroll_cursor_to_left_edge scroll_cursor_to_right_edge scroll_down\n\
      scroll_half_column_left scroll_half_column_right scroll_line_above_window\n\
      scroll_line_below_window scroll_other_window scroll_other_window_down\n\
      scroll_up view_buffer view_buffer_other_window view_echo_area_messages\n\
      view_emacs_debugging view_emacs_faq view_emacs_problems view_emacs_todo\n\
      view_exit view_external_packages view_file view_file_at_rev\n\
      view_hello_file view_lossage view_mode view_order_manuals view_quit\n\
      view_register",
    ),
    (
        "Buffer commands",
        "Switching, listing, killing and reverting buffers, and the ones that act on\n\
      every buffer at once.\n\nbuffer_line_picker buffer_menu buffer_picker buffer_swap_window_1\n\
      buffer_swap_window_2 buffer_swap_window_3 buffer_swap_window_4\n\
      buffer_swap_window_5 buffer_swap_window_6 buffer_swap_window_7\n\
      buffer_swap_window_8 buffer_swap_window_9 buffer_to_window_1\n\
      buffer_to_window_2 buffer_to_window_3 buffer_to_window_4\n\
      buffer_to_window_5 buffer_to_window_6 buffer_to_window_7\n\
      buffer_to_window_8 buffer_to_window_9 kill_buffers_by_regex",
    ),
    (
        "Diff commands",
        "Hunks in the gutter, and the three-way merge.\n\nconflict_take_all_ours conflict_take_all_theirs\n\
      diff_add_change_log_entries_other_window\n\
      diff_add_change_log_entry_other_window diff_apply_buffer diff_apply_hunk\n\
      diff_backup diff_buffers diff_context_to_unified\n\
      diff_delete_trailing_whitespace diff_ediff_patch diff_file_kill\n\
      diff_hunk_kill diff_ignore_whitespace_hunk diff_refresh_hunk\n\
      diff_restrict_view diff_reverse_direction diff_split_hunk\n\
      diff_unified_to_context",
    ),
    (
        "Vim compatibility commands",
        "The vim behaviours that have no emacs or helix equivalent, kept under their\n\
      own names so the vim keymap can bind them.\n\nvim_change_line vim_find_next_char vim_find_prev_char vim_find_till_char\n\
      vim_move_next_long_word_end vim_move_next_long_word_start\n\
      vim_move_next_word_end vim_move_next_word_start\n\
      vim_move_prev_long_word_end vim_move_prev_long_word_start\n\
      vim_move_prev_word_end vim_move_prev_word_start vim_record_macro\n\
      vim_replay_macro vim_sleep vim_till_prev_char",
    ),
    (
        "Changing text",
        "Replacing what is selected, and the case and increment operators.\n\nchange_find_char_backward change_find_char_forward change_log_goto_source\n\
      change_log_merge change_log_mode change_selection change_selection_noyank\n\
      change_signature change_textobject_around change_textobject_inner\n\
      change_till_char_backward change_till_char_forward change_to_mark\n\
      change_to_mark_line change_to_search_backward change_to_search_forward\n\
      decrement decrement_sequential increment increment_register\n\
      increment_sequential replace_chars_vim replace_mode\n\
      replace_selections_with_clipboard\n\
      replace_selections_with_primary_clipboard replace_with_yanked",
    ),
    (
        "Finding",
        "Character search, the pickers, and the search-under-cursor commands.\n\nfind_char_backward_label find_char_forward_label find_file_at_point\n\
      find_file_other_frame find_file_other_tab find_file_other_window\n\
      find_file_read_only find_file_read_only_other_frame\n\
      find_file_replace_buffer find_grep find_next_char find_prev_char\n\
      find_sibling_file find_tag_other_window find_till_char search_everywhere\n\
      search_history_picker search_in_files search_next search_next_vim\n\
      search_prev search_prev_vim search_selection\n\
      search_selection_detect_word_boundaries",
    ),
    (
        "Setting things",
        "The commands that set a value rather than toggling it.\n\nset_buffer_file_coding_system set_buffer_process_coding_system\n\
      set_file_name_coding_system set_fill_prefix set_fontset_font\n\
      set_frame_name set_goal_column set_input_method set_keyboard_coding_system\n\
      set_language_environment set_locale_environment set_mark set_mark_command\n\
      set_next_selection_coding_system set_numbered_bookmark\n\
      set_selection_coding_system set_selective_display\n\
      set_terminal_coding_system",
    ),
    (
        "Languages: what is supported",
        "zmax parses 361 languages with tree-sitter and ships a default language\n\
      server for 206 of them, so most files work with nothing installed but the\n\
      server itself.\n\n\
      346 have syntax highlighting\n\
      150 have tree-sitter textobjects — the m menu's function, class and\n\
          parameter objects\n\
      142 have automatic indentation\n\
      71 have code-navigation tags, for jumping without a server\n\
      88 have rainbow brackets\n\n\
      :set-language changes what a buffer is treated as, :filetype reports it,\n\
      and languages.toml overrides any of it — globally in the config directory\n\
      or per project.",
    ),
    (
        "Languages with a language server",
        "The server named is the default; languages.toml can point at another, or at\n\
      several.  :lsp-health says which are actually running.\n\nada (ada_language_server) agda (als) amber (amber-lsp)\n\
      asciidoc (ltex-ls-plus) astro (astro-ls) autohotkey (autohotkey_lsp)\n\
      awk (awk-language-server) bash (zshrs) bass (bass)\n\
      beancount (beancount-language-server) bibtex (texlab)\n\
      bicep (bicep-langserver) bitbake (bitbake-language-server)\n\
      blueprint (blueprint-compiler) c (clangd) c-sharp (roslyn-language-server)\n\
      c3 (c3-lsp) cabal (haskell-language-server-wrapper)\n\
      cairo (cairo-language-server) circom (circom-lsp) clarity (clarinet)\n\
      clojure (clojure-lsp) cmake (neocmakelsp) codeql (codeql)\n\
      coffeescript (coffeesense-language-server) common-lisp (cl-lsp)\n\
      coq (coq-lsp) cpp (clangd) cross-config (taplo) crystal (crystalline)\n\
      css (vscode-css-language-server) cue (cue) d (serve-d) dart (dart)\n\
      debian (debian-lsp) devicetree (dts-lsp) dhall (dhall-lsp-server)\n\
      docker-bake (docker-language-server)\n\
      docker-compose (docker-compose-langserver) dockerfile (docker-langserver)\n\
      dot (dot-language-server) drools (drools-lsp) earthfile (earthlyls)\n\
      ebnf (ebnfer) eiffel (eiffel-language-server) elixir (elixir-ls)\n\
      elm (elm-language-server) elvish (elvish) erlang (erlang_ls)\n\
      faust (faustlsp) fennel (fennel-ls) fish (fish-lsp) forth (forth-lsp)\n\
      fortran (fortls) fountain (fountain-lsp-server) fsharp (fsautocomplete)\n\
      gas (asm-lsp) git-cliff-config (taplo) git-commit (commit-lsp)\n\
      github-action (actions-languageserver) gitlab-ci (yaml-language-server)\n\
      gjs (typescript-language-server) gleam (gleam)\n\
      glimmer (ember-language-server) glsl (glsl_analyzer) go (gopls)\n\
      gomod (gopls) gotmpl (gopls) gowork (gopls) gpr (ada_language_server)\n\
      graphql (graphql-lsp) groovy (groovy-language-server)\n\
      gts (typescript-language-server) hare (hare-lsp)\n\
      haskell (haskell-language-server-wrapper)\n\
      haskell-literate (haskell-language-server-wrapper) hcl (terraform-ls)\n\
      hdl (hdls) heex (elixir-ls) helm (helm_ls)\n\
      html (vscode-html-language-server) htmldjango (djlsp) http (kulala-ls)\n\
      hy (hyuga) hyprlang (hyprls) idris (idris2-lsp) java (jdtls)\n\
      javascript (typescript-language-server) jjconfig (taplo) jq (jq-lsp)\n\
      json (vscode-json-language-server) json-ld (vscode-json-language-server)\n\
      jsonc (vscode-json-language-server) jsonnet (jsonnet-language-server)\n\
      jsx (typescript-language-server) julia (julia) just (just-lsp)\n\
      kcl (kcl-language-server) koka (koka) kotlin (kotlin-language-server)\n\
      koto (koto-ls) latex (texlab) lean (lake)\n\
      less (vscode-css-language-server) lua (lua-language-server)\n\
      luau (luau-lsp) markdoc (markdoc-ls) markdown (marksman)\n\
      mermaid (merman-lsp) meson (mesonlsp) mint (mint) miseconfig (taplo)\n\
      mojo (pixi) nasm (asm-lsp) nginx (nginx-language-server) nickel (nls)\n\
      nim (nimlangserver) nix (nil) nu (nu) ocaml (ocamllsp)\n\
      ocaml-interface (ocamllsp) octave (octave-lsp) odin (ols) opencl (clangd)\n\
      openscad (openscad-lsp) org (ltex-ls-plus) pact (pact-lsp) pascal (pasls)\n\
      perl (perlnavigator) pest (pest-language-server) php (intelephense)\n\
      pkgbuild (termux-language-server) pkl (pkl-lsp) plantuml (plantuml-lsp)\n\
      ponylang (pony-lsp) powershell (pwsh) prisma (prisma-language-server)\n\
      prolog (swipl) protobuf (buf) puppet (puppet-languageserver)\n\
      purescript (purescript-language-server) python (ty) qml (qmlls)\n\
      quint (quint-language-server) r (R) racket (racket)\n\
      raku (raku-language-server) reason (reason-language-server) rego (regols)\n\
      rescript (rescript-language-server) ripple (ripple-language-server)\n\
      rmarkdown (R) robot (robotcode) ron (ron-lsp) rshtml (rshtml-analyzer)\n\
      rst (python3) ruby (ruby-lsp) rust (rust-analyzer) scala (metals)\n\
      scheme (guile-lsp-server) scss (vscode-css-language-server) slang (slangd)\n\
      slint (slint-lsp) sls (salt_lsp_server) smali (smalisp) smithy (cs)\n\
      sml (millet-ls) snakemake (pylsp) solidity (solc)\n\
      sourcepawn (sourcepawn-studio) spade (swim)\n\
      sparql (sparql-language-server) sql (sqls) starlark (starpls)\n\
      stryke (stryke) styx (styx) svelte (svelteserver) sway (forc)\n\
      swift (sourcekit-lsp) systemd (systemd-lsp) systemverilog (svlangserver)\n\
      teal (teal-language-server) templ (templ) text (ltex-ls-plus)\n\
      tfvars (terraform-ls) tilt (tilt) toml (taplo) tsq (ts_query_ls)\n\
      tsx (typescript-language-server) turtle (turtle-language-server)\n\
      typescript (typescript-language-server) typespec (tsp-server)\n\
      typst (tinymist) v (v-analyzer) vala (vala-language-server)\n\
      verilog (verible-verilog-ls) vhdl (vhdl_ls) vue (vue-language-server)\n\
      wat (wat_server) wgsl (wgsl-analyzer) wikitext (wikitext-lsp)\n\
      woodpecker-ci (yaml-language-server) yaml (yaml-language-server)\n\
      yang (yang-language-server) yara (yls) zig (zls)",
    ),
    (
        "Languages with textobjects",
        "These are the languages where the m menu's function, class, parameter,\n\
      comment and test objects work, because the grammar has the queries for\n\
      them.\n\nada adl amber awk bash basic blade c c-sharp caddyfile cairo clojure cmake\n\
      codeql common-lisp concerto cpp cross-config crystal css cylc d dart dhall\n\
      docker-bake docker-compose dockerfile doxyfile earthfile eiffel elixir elm\n\
      env erlang fga fish freebasic gas gdscript git-cliff-config git-commit\n\
      git-config github-action gitlab-ci gjs gleam glsl go godot-resource\n\
      graphql gren gts hare haskell hcl heex hocon html hurl inko java\n\
      javascript jjconfig jq json json-ld json5 jsonc jsx julia just kdl kotlin\n\
      koto latex llvm llvm-mir lua luau mail matlab miseconfig mojo nasm\n\
      nestedtext nim nix nu ocaml odin ohm opencl pascal penrose perl pest php\n\
      picat pkgbuild po ponylang prisma properties protobuf purescript python\n\
      qml r raku rescript robots.txt rshtml ruby rust rust-format-args-macro\n\
      sage scala scheme shellcheckrc slang slint solidity sourcepawn sql\n\
      starlark styx svelte sway swift tablegen tact textproto tilt toml tsx\n\
      typescript typespec typst unison v vala verilog vue wesl wgsl\n\
      woodpecker-ci wren xml yaml zig",
    ),
    (
        "Languages with auto-indent",
        "Where a new line indents itself from the syntax rather than from the\n\
      previous line.\n\nadl amber bash basic c c-sharp caddyfile cairo capnp clojure cmake\n\
      concerto cpon cpp crystal css cylc cython d dart docker-bake\n\
      docker-compose doxyfile earthfile eiffel elixir erlang fga fish fortran\n\
      freebasic gdscript github-action gitlab-ci gjs glsl go gts hcl hocon html\n\
      hurl hyprlang inko janet java javascript jjconfig json json-ld json5 jsonc\n\
      jsx julia just kconfig kdl koka kotlin koto latex ld less llvm llvm-mir\n\
      llvm-mir-yaml lua luau make matlab meson miseconfig mojo move msbuild\n\
      nestedtext nickel nim nix nu ocaml odin ohm opencl perl pest php picat\n\
      pkgbuild pkl ponylang prolog protobuf ptx python qml quarto r racket raku\n\
      rmarkdown robot ron ruby rust rust-format-args-macro scala scheme scss\n\
      slang slint smali snakemake spade starlark styx svelte sway swift tablegen\n\
      tact tcl textproto tfvars tilt tolk toml tql tsx typescript typespec\n\
      unison v vue wgsl wit woodpecker-ci wren xml yaml yuck zig",
    ),
    (
        "Languages with navigation tags",
        "Jump to a definition without a language server running.\n\namber bash basic c c-sharp clojure common-lisp cpp crystal cython dart\n\
      docker-bake doxyfile elisp elixir elm erlang freebasic gdscript git-config\n\
      gitlab-ci gjs glsl go godot-resource gts haskell haxe hyprlang ini inko\n\
      java javascript jsx julia just kdl kotlin lua markdown nix perl php\n\
      php-only picat protobuf python r raku ripple robots.txt ron rst ruby rust\n\
      scala scheme slisp spicedb strictdoc svelte swift systemd toml tsx\n\
      typescript typst unison wgsl woodpecker-ci zig",
    ),
    (
        "Languages with rainbow brackets",
        "Nested brackets coloured by depth.\n\namber bash blade c c-sharp clojure cmake common-lisp cpp cross-config\n\
      crystal css dart docker-bake elixir elm erlang fennel fsharp gdscript\n\
      git-cliff-config github-action gitlab-ci gleam glsl go go-format-string\n\
      graphql groovy haskell hcl html janet java javascript json json-ld json5\n\
      jsonc jsx julia kdl kotlin koto less lua mail nearley nim nix ocaml perl\n\
      php picat powershell python r racket raku regex ripple ron ruby rust\n\
      rust-format-args-macro scala scheme scss solidity spade sql starlark styx\n\
      svelte swift tilt toml tsq tsx typescript unison vim wgsl woodpecker-ci\n\
      xml yaml yuck zig",
    ),
    (
        "Every language: A to F",
        "The full list, as :set-language spells them.\n\nada adl agda alda alloy amber apache asciidoc astro autohotkey awk bash\n\
      basic bass batch beancount bibtex bicep bitbake blade blueprint bovex c\n\
      c-sharp c3 cabal caddyfile cairo capnp cel cfengine chuck circom clarity\n\
      clojure cmake codeql coffeescript comment common-lisp concerto coq cpon\n\
      cpp cross-config crystal css csv cue cylc cython d dart dbml debian\n\
      devicetree dhall diff djot docker-bake docker-compose dockerfile dot\n\
      doxyfile drools dtd dune dunstrc earthfile ebnf edoc eex eiffel ejs elisp\n\
      elixir elm elvish embedded-perl env erb erlang esdl factor faust fennel\n\
      fga fidl fish flatbuffers forth fortran fountain freebasic fsharp",
    ),
    (
        "Every language: G to M",
        "The full list, continued.\n\ngas gdscript gemini gherkin ghostty git-attributes git-cliff-config\n\
      git-commit git-config git-ignore git-notes git-rebase github-action\n\
      gitlab-ci gjs gleam glimmer glsl gn gnuplot go go-format-string\n\
      godot-resource gomod gotmpl gowork gpr graphql gren groovy gts hare\n\
      haskell haskell-literate haskell-persistent haxe hcl hdl heex helm hocon\n\
      hoon hosts html htmldjango http hurl hy hyprlang idris iex ini ink inko\n\
      janet java javascript jinja jjconfig jjdescription jjrevset jjtemplate jq\n\
      jr jsdoc json json-ld json5 jsonc jsonnet jsx julia just kcl kconfig kdl\n\
      kivy klog koka kotlin koto latex ld ldif lean ledger less llvm llvm-mir\n\
      llvm-mir-yaml log lpf lua lua-format-string luap luau mail make markdoc\n\
      markdown markdown-rustdoc markdown.inline matlab mercury mermaid meson\n\
      mint miseconfig mojo move msbuild",
    ),
    (
        "Every language: N to S",
        "The full list, continued.\n\nnasm nearley nestedtext nginx nickel nim nix nu nunjucks ocaml\n\
      ocaml-interface octave odin ohm opencl openscad org pact pascal passwd pem\n\
      penrose perl pest php php-only picat pip-requirements pkgbuild pkl\n\
      plantuml po pod ponylang powershell prisma prolog properties protobuf\n\
      proverif prql ptx pug puppet purescript python qml qmv quarto quint r\n\
      racket raku reason regex rego rescript ripple rmarkdown robot robots.txt\n\
      ron rpmspec rshtml rst ruby rust rust-format-args rust-format-args-macro\n\
      sage scala scfg scheme scss shellcheckrc slang slint slisp sls smali\n\
      smithy sml snakemake solidity sourcepawn spade sparql spicedb sql\n\
      ssh_client_config starlark strace strictdoc stryke styx supercollider\n\
      svelte sway swift systemd systemverilog",
    ),
    (
        "Every language: T to Z",
        "The full list, concluded.\n\nt32 tablegen tact task tcl teal templ tera text textproto tfvars thrift\n\
      tilt tlaplus todotxt tolk toml tql tsq tsx turtle twig typescript typespec\n\
      typst ungrammar unison uxntal v vala vento verilog vhdl vhs vim vue wast\n\
      wat webc werk wesl wgsl wikitext wit woodpecker-ci wren xit xml xtc yaml\n\
      yang yara yuck zig",
    ),
];

pub struct HelpPanel {
    entries: Vec<Entry>,
    cat: Cat,
    filter: String,
    sel: usize, // index into the filtered view
    top: usize,
    detail_scroll: u16,
    cat_hits: Vec<(u16, u16, u16, usize)>,
    row_hits: Vec<(u16, u16, u16, usize)>, // maps screen row -> filtered index
    /// The entry being *displayed on its own* (Emacs's one-topic `*Help*`
    /// buffer): while set, the list shows only it. Enter visits the selected
    /// entry; typing or Backspace goes back to browsing.
    visiting: Option<usize>,
    /// Visit history — the entry indices `help-go-back` / `help-go-forward` walk,
    /// oldest first, with `hpos` the position in it.
    history: Vec<usize>,
    hpos: usize,
    /// Height of the detail pane at the last render, so a page scroll moves by a
    /// screenful (Emacs `help-goto-next-page`).
    page: u16,
    /// `C-c` was typed: the panel is waiting for the second key of Emacs's
    /// `C-c C-b` (`help-go-back`) / `C-c C-f` (`help-go-forward`) chords.
    pending_ctrl_c: bool,
}

impl Default for HelpPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpPanel {
    pub fn new() -> Self {
        let keys = key_index();
        let mut entries = Vec::new();
        for c in MappableCommand::STATIC_COMMAND_LIST {
            let name = c.name().to_string();
            entries.push(Entry {
                cat: Cat::Commands,
                keys: keys.get(&name).cloned().unwrap_or_default(),
                title: name,
                aliases: Vec::new(),
                doc: c.doc().to_string(),
            });
        }
        for t in crate::commands::typed::TYPABLE_COMMAND_LIST {
            entries.push(Entry {
                cat: Cat::Commands,
                title: format!(":{}", t.name),
                keys: Vec::new(),
                aliases: t.aliases.iter().map(|a| format!(":{a}")).collect(),
                doc: t.doc.to_string(),
            });
        }
        for (title, body) in TOPICS {
            entries.push(Entry {
                cat: Cat::Topics,
                title: title.to_string(),
                keys: Vec::new(),
                aliases: Vec::new(),
                doc: body.to_string(),
            });
        }
        entries.sort_by_key(|a| a.title.to_lowercase());
        Self {
            entries,
            cat: Cat::All,
            filter: String::new(),
            sel: 0,
            top: 0,
            detail_scroll: 0,
            cat_hits: Vec::new(),
            row_hits: Vec::new(),
            visiting: None,
            history: Vec::new(),
            hpos: 0,
            page: 5,
            pending_ctrl_c: false,
        }
    }

    /// Display `entry` on its own and record the visit, so `help-go-back` can
    /// return to whatever was shown before it.
    fn visit(&mut self, entry: usize) {
        if self.visiting == Some(entry) {
            return;
        }
        // A new visit truncates the forward history, exactly as a browser does.
        if !self.history.is_empty() && self.hpos + 1 < self.history.len() {
            self.history.truncate(self.hpos + 1);
        }
        if self.history.last() != Some(&entry) {
            self.history.push(entry);
        }
        self.hpos = self.history.len() - 1;
        self.visiting = Some(entry);
        self.detail_scroll = 0;
    }

    /// Emacs `help-go-back` (`C-c C-b` / `l` in `*Help*`): show the previously
    /// visited help entry. `false` when there is none.
    pub fn go_back(&mut self) -> bool {
        if self.hpos == 0 || self.history.is_empty() {
            return false;
        }
        self.hpos -= 1;
        self.visiting = Some(self.history[self.hpos]);
        self.detail_scroll = 0;
        true
    }

    /// Emacs `help-go-forward` (`C-c C-f` / `r` in `*Help*`): the counterpart of
    /// [`Self::go_back`]. `false` when there is nothing ahead.
    pub fn go_forward(&mut self) -> bool {
        if self.hpos + 1 >= self.history.len() {
            return false;
        }
        self.hpos += 1;
        self.visiting = Some(self.history[self.hpos]);
        self.detail_scroll = 0;
        true
    }

    /// Emacs `help-goto-next-page`: scroll the displayed help text down one
    /// screenful.
    pub fn goto_next_page(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(self.page.max(1));
    }

    /// Emacs `help-goto-previous-page`: scroll it up one screenful.
    pub fn goto_previous_page(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(self.page.max(1));
    }

    /// Emacs `forward-button` (`TAB` in Help mode, inherited from
    /// `button-buffer-map`): move point to the `n`th next button. Every row of
    /// the list is a cross-reference button — `RET` follows it — so this steps
    /// the selection. Called interactively its `wrap` argument is non-nil, so
    /// moving past either end continues from the other; that wrap is what
    /// separates it from `↓`, which stops at the last row.
    pub fn forward_button(&mut self, n: isize) -> bool {
        let len = self.matches().len();
        if len == 0 {
            return false;
        }
        let cur = self.sel.min(len - 1) as isize;
        self.sel = (cur + n).rem_euclid(len as isize) as usize;
        self.detail_scroll = 0;
        true
    }

    /// Emacs `backward-button` (`S-TAB` / `<backtab>`), defined there as
    /// `forward-button` with a negated count.
    pub fn backward_button(&mut self, n: isize) -> bool {
        self.forward_button(-n)
    }

    /// Emacs `help-follow` (`RET` in Help mode): follow the cross-reference at
    /// point. Every list row is a cross-reference to its entry, so this visits the
    /// selected one — showing it on its own and recording it in the history
    /// `help-go-back` / `help-go-forward` walk. `false` when point is on no
    /// cross-reference, which is the "No cross-reference here" case.
    pub fn follow(&mut self) -> bool {
        match self.matches().get(self.sel) {
            Some(&e) => {
                self.visit(e);
                self.sel = 0;
                true
            }
            None => false,
        }
    }

    /// Emacs `push-button` — what `mouse-2` runs from `button-map` and what
    /// `mouse-1` runs on a help xref, whose `follow-link` property makes a left
    /// click act as `mouse-2`. On a help cross-reference that is `help-follow`,
    /// so this is the click-driven twin of the `⏎` arm.
    fn push_button(&mut self, pos: usize) {
        self.sel = pos;
        if let Some(&e) = self.matches().get(pos) {
            self.visit(e);
            self.sel = 0;
        }
    }

    /// Emacs `help-view-source` (`s` in Help mode): "View the source of the
    /// current help item." Emacs reads the `:file` that `load-history` recorded
    /// for the symbol and jumps to its definition, erroring when that file is
    /// unknown. zmax keeps no load-history, but the `static_commands!` macro
    /// stringifies each command's Rust `fn` name into its command name, so the
    /// definition site is `fn <name>(` in the crate sources. Locate it the way
    /// `find-function-search-for-symbol` scans the source — walk the workspace
    /// (honouring `.gitignore`, like Find-in-Files) for that definition and open
    /// the file there. Like Emacs, error when the source can't be found; only a
    /// real command (name == fn name) has one — typable `:commands`, aliases and
    /// topic pages do not.
    fn view_source(&self) -> EventResult {
        match self.source_location() {
            Some((path, line)) => open_source_at(path, line),
            None => source_not_found(),
        }
    }

    /// The file and 1-based line that define the entry currently displayed on its
    /// own, or `None` when it has no source. Shared by the `s` key
    /// (`help-view-source`) and the `help-find-source` command, which runs the
    /// same lookup from outside the buffer.
    pub fn source_location(&self) -> Option<(PathBuf, usize)> {
        let i = self.visiting?;
        let e = &self.entries[i];
        if e.cat != Cat::Commands || e.title.starts_with(':') {
            return None;
        }
        locate_definition(&format!("fn {}(", e.title))
    }

    /// Step the category filter — zmax's own affordance, on `→` / `←` because
    /// Help mode owns `TAB` / `S-TAB` for button navigation.
    fn cycle_cat(&mut self, forward: bool) {
        let i = CATS.iter().position(|(c, _)| *c == self.cat).unwrap_or(0);
        let step = if forward { 1 } else { CATS.len() - 1 };
        self.cat = CATS[(i + step) % CATS.len()].0;
        self.sel = 0;
        self.top = 0;
    }

    /// The title of the entry currently shown on its own, if any — so a command
    /// can report what it moved to.
    pub fn current_title(&self) -> Option<&str> {
        self.visiting.map(|i| self.entries[i].title.as_str())
    }

    /// Construct the browser pre-filtered to `filter` — used by `:Helptags` to
    /// land on the fuzzy-picked entry.
    pub fn with_filter(filter: String) -> Self {
        let mut p = Self::new();
        p.filter = filter;
        p
    }

    /// Every entry title (static commands, `:typables`, and topics) — the source
    /// list for the `:Helptags` fzf picker.
    pub fn entry_titles(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.title.clone()).collect()
    }

    fn matches(&self) -> Vec<usize> {
        // While a single entry is being visited (Emacs's one-topic *Help*), the
        // view is exactly that entry — that is what go-back/go-forward move.
        if let Some(i) = self.visiting {
            return vec![i];
        }
        let f = self.filter.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let in_cat = match self.cat {
                    Cat::All => true,
                    Cat::Keys => !e.keys.is_empty(),
                    c => e.cat == c,
                };
                in_cat
                    && (f.is_empty()
                        || e.title.to_lowercase().contains(&f)
                        || e.doc.to_lowercase().contains(&f)
                        || e.keys.iter().any(|k| k.to_lowercase().contains(&f)))
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) -> EventResult {
        match kind {
            MouseEventKind::ScrollDown => {
                self.sel += 1;
                return EventResult::Consumed(None);
            }
            MouseEventKind::ScrollUp => {
                self.sel = self.sel.saturating_sub(1);
                return EventResult::Consumed(None);
            }
            // Both buttons push: `mouse-2` is `push-button` in `button-map`,
            // and `mouse-1` follows the same button through `follow-link`.
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Down(MouseButton::Middle) => {
            }
            _ => return EventResult::Consumed(None),
        }
        if let Some(&(_, _, _, ci)) = self
            .cat_hits
            .iter()
            .find(|&&(x0, x1, r, _)| row == r && col >= x0 && col < x1)
        {
            self.cat = CATS[ci].0;
            self.sel = 0;
            self.top = 0;
            return EventResult::Consumed(None);
        }
        if let Some(&(_, _, _, pos)) = self
            .row_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            self.push_button(pos);
        }
        EventResult::Consumed(None)
    }
}

/// Scan the workspace for the `fn <name>(` definition of a command — the zmax
/// analog of `find-function-search-for-symbol` reading the source. Returns the
/// file and 1-based line of the first match. `.gitignore` is honoured and huge
/// files are skipped, matching the Find-in-Files walker.
fn locate_definition(needle: &str) -> Option<(PathBuf, usize)> {
    locate_definition_in(zmax_stdx::env::current_working_dir(), needle)
}

fn locate_definition_in(root: PathBuf, needle: &str) -> Option<(PathBuf, usize)> {
    const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
    for entry in ignore::WalkBuilder::new(&root).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for (i, line) in content.lines().enumerate() {
            let t = line.trim_start();
            if (t.starts_with("fn ") || t.starts_with("pub fn ")) && t.contains(needle) {
                return Some((path.to_path_buf(), i + 1));
            }
        }
    }
    None
}

/// Open `path` at 1-based `line`, popping the Help panel — the jump `help-view-
/// source` performs once the definition is found (same pattern as Find-in-Files
/// opening a result).
fn open_source_at(path: PathBuf, line: usize) -> EventResult {
    EventResult::Consumed(Some(Box::new(
        move |c: &mut Compositor, cx: &mut Context| {
            c.pop();
            let scrolloff = cx.editor.config().scrolloff;
            match cx.editor.open(&path, Action::Replace) {
                Ok(_) => {
                    let (view, doc) = current!(cx.editor);
                    let text = doc.text();
                    let last = text.len_lines().saturating_sub(1);
                    let pos = text.line_to_char(line.saturating_sub(1).min(last));
                    doc.set_selection(view.id, Selection::point(pos));
                    view.ensure_cursor_in_view(doc, scrolloff);
                }
                Err(e) => cx.editor.set_error(format!("open failed: {e}")),
            }
        },
    )))
}

/// Emacs's error when `help-view-source` has no `:file` to visit — reported on
/// the status line while the Help panel stays open.
fn source_not_found() -> EventResult {
    EventResult::Consumed(Some(Box::new(|_c: &mut Compositor, cx: &mut Context| {
        cx.editor
            .set_error("Source file for the current help item is not defined");
    })))
}

impl Component for HelpPanel {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key: KeyEvent = match event {
            Event::Key(k) => *k,
            Event::Mouse(ev) => return self.handle_mouse(ev.column, ev.row, ev.kind),
            _ => return EventResult::Ignored(None),
        };
        let n = self.matches().len();
        // `C-c` armed the Emacs `C-c C-b` / `C-c C-f` chords: the next key either
        // completes one or drops the prefix, as an Emacs prefix key does.
        if std::mem::take(&mut self.pending_ctrl_c) {
            match key {
                ctrl!('b') => {
                    self.go_back();
                }
                ctrl!('f') => {
                    self.go_forward();
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        match key {
            key!(Esc) => {
                return EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                    c.pop();
                })))
            }
            ctrl!('c') => self.pending_ctrl_c = true,
            key!(Tab) => {
                self.forward_button(1);
            }
            shift!(Tab) => {
                self.backward_button(1);
            }
            key!(Right) => self.cycle_cat(true),
            key!(Left) => self.cycle_cat(false),
            key!(Down) | ctrl!('n') | ctrl!('j') => {
                if n > 0 {
                    self.sel = (self.sel + 1).min(n - 1);
                    self.detail_scroll = 0;
                }
            }
            key!(Up) | ctrl!('p') | ctrl!('k') => {
                self.sel = self.sel.saturating_sub(1);
                self.detail_scroll = 0;
            }
            key!(PageDown) => self.goto_next_page(),
            key!(PageUp) => self.goto_previous_page(),
            key!(Enter) => {
                // `help-follow`: follow the cross-reference at point — visit the
                // selected entry, showing it on its own and recording it in the
                // history that help-go-back / help-go-forward walk.
                self.follow();
            }
            key!(Backspace) => {
                self.visiting = None;
                self.filter.pop();
                self.sel = 0;
            }
            // Help-mode keys, live while a single topic is displayed on its own —
            // that state is the read-only `*Help*` buffer. While browsing, these
            // letters are search input (the fall-through arm below).
            key!('l') if self.visiting.is_some() => {
                self.go_back();
            }
            key!('r') if self.visiting.is_some() => {
                self.go_forward();
            }
            key!('n') if self.visiting.is_some() => self.goto_next_page(),
            key!('p') if self.visiting.is_some() => self.goto_previous_page(),
            // `help-view-source`: jump to the source that defines the visited
            // command. While browsing, `s` is search input (the arm below).
            key!('s') if self.visiting.is_some() => return self.view_source(),
            _ => {
                if let KeyCode::Char(c) = key.code {
                    self.visiting = None;
                    self.filter.push(c);
                    self.sel = 0;
                }
            }
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Paragraph, Wrap};

        self.cat_hits.clear();
        self.row_hits.clear();
        let matched = self.matches();
        if self.sel >= matched.len() {
            self.sel = matched.len().saturating_sub(1);
        }

        let theme = &ctx.editor.theme;
        let bg = to_rat_style(theme.get("ui.background"));
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let border = to_rat_style(theme.get("ui.window"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let keyc = to_rat_style(theme.get("keyword"));
        // `transparent-background`: drop the page fill so the terminal shows through.
        let mut page_bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            page_bg.bg = None;
        }
        surface.clear_with(area, page_bg);

        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        render(
            Paragraph::new(Span::styled(" Help ", accent)),
            Rect::new(area.x + 1, area.y, area.width.saturating_sub(1), 1),
            surface,
        );
        let _ = (border, bg);
        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(1),
        );
        if inner.width < 24 || inner.height < 6 {
            return;
        }

        // top: category buttons + search box
        let mut x = inner.x + 1;
        for (i, (c, name)) in CATS.iter().enumerate() {
            let lbl = format!(" {name} ");
            let w = lbl.chars().count() as u16;
            let st = if *c == self.cat {
                text.add_modifier(RMod::REVERSED)
            } else {
                dim
            };
            render(
                Paragraph::new(Span::styled(lbl, st)),
                Rect::new(x, inner.y, w, 1),
                surface,
            );
            self.cat_hits.push((x, x + w, inner.y, i));
            x += w + 1;
        }
        render(
            Paragraph::new(Span::styled(
                format!("  🔍 {}▏  ({} results)", self.filter, matched.len()),
                dim,
            )),
            Rect::new(x + 1, inner.y, inner.x + inner.width - x - 1, 1),
            surface,
        );

        // body split: list | detail
        let list_w = (inner.width * 2 / 5).clamp(16, 44);
        let body_y = inner.y + 2;
        let body_h = inner.height.saturating_sub(3);
        // Remember the detail height so help-goto-next-page scrolls a screenful.
        self.page = body_h.saturating_sub(1).max(1);
        // keep selection in view
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + body_h as usize {
            self.top = self.sel + 1 - body_h as usize;
        }
        let last = (self.top + body_h as usize).min(matched.len());
        for (pos, &m) in matched.iter().enumerate().take(last).skip(self.top) {
            let e = &self.entries[m];
            let y = body_y + (pos - self.top) as u16;
            let is_sel = pos == self.sel;
            if is_sel {
                surface.set_style(Rect::new(inner.x, y, list_w, 1), theme.get("ui.selection"));
            }
            let glyph = if e.cat == Cat::Topics {
                "📖 "
            } else {
                "› "
            };
            render(
                Paragraph::new(Span::styled(
                    format!("{glyph}{}", e.title),
                    if is_sel { accent } else { text },
                )),
                Rect::new(inner.x, y, list_w, 1),
                surface,
            );
            self.row_hits.push((y, inner.x, inner.x + list_w, pos));
        }

        // divider
        let dx = inner.x + list_w;
        for y in body_y..body_y + body_h {
            render(
                Paragraph::new(Span::styled("│", dim)),
                Rect::new(dx, y, 1, 1),
                surface,
            );
        }

        // detail
        if let Some(&ei) = matched.get(self.sel) {
            let e = &self.entries[ei];
            let detail_x = dx + 2;
            let detail_w = (inner.x + inner.width).saturating_sub(detail_x);
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(Span::styled(e.title.clone(), accent)));
            if !e.keys.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("keys: {}", e.keys.join("   ")),
                    keyc,
                )));
            }
            if !e.aliases.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("aliases: {}", e.aliases.join(", ")),
                    dim,
                )));
            }
            lines.push(Line::from(""));
            for para in e.doc.split('\n') {
                lines.push(Line::from(Span::styled(para.to_string(), text)));
            }
            let para = Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((self.detail_scroll, 0));
            render(para, Rect::new(detail_x, body_y, detail_w, body_h), surface);
        }

        render(
            Paragraph::new(Span::styled(
                if self.visiting.is_some() {
                    " l / C-c C-b back · r / C-c C-f forward · n / p page · s source · ⏎ visit · ⌫ back to search · Esc close"
                } else {
                    " type to search · ↑/↓ or C-n/C-p/C-j/C-k move · Tab/S-Tab button · ⏎ visit · →/← category · PgUp/PgDn scroll doc · Esc close"
                },
                dim,
            )),
            Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
            surface,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_history_walks_back_and_forward() {
        let mut p = HelpPanel::new();
        let first = p.entries[0].title.clone();
        let second = p.entries[1].title.clone();

        p.visit(0);
        p.visit(1);
        assert_eq!(p.current_title(), Some(second.as_str()));
        // Visiting shows exactly that entry, like Emacs's one-topic *Help*.
        assert_eq!(p.matches(), vec![1]);

        assert!(p.go_back());
        assert_eq!(p.current_title(), Some(first.as_str()));
        assert!(!p.go_back(), "nothing before the first visit");

        assert!(p.go_forward());
        assert_eq!(p.current_title(), Some(second.as_str()));
        assert!(!p.go_forward(), "nothing after the last visit");

        // A new visit from the middle truncates the forward history.
        p.go_back();
        p.visit(2);
        assert!(!p.go_forward());
        assert!(p.go_back());
        assert_eq!(p.current_title(), Some(first.as_str()));
    }

    #[test]
    fn help_paging_moves_by_a_screenful() {
        let mut p = HelpPanel::new();
        p.page = 20;
        p.goto_next_page();
        p.goto_next_page();
        assert_eq!(p.detail_scroll, 40);
        p.goto_previous_page();
        assert_eq!(p.detail_scroll, 20);
        p.goto_previous_page();
        p.goto_previous_page();
        assert_eq!(p.detail_scroll, 0, "scroll saturates at the top");
    }

    #[test]
    fn help_buttons_wrap_round_both_ends() {
        let mut p = HelpPanel::new();
        let n = p.matches().len();
        assert!(n > 1);

        assert!(p.forward_button(1));
        assert_eq!(p.sel, 1);
        assert!(p.backward_button(1));
        assert_eq!(p.sel, 0);
        // `forward-button`'s interactive WRAP argument is non-nil, so the ends
        // continue from the other end rather than stopping.
        assert!(p.backward_button(1));
        assert_eq!(
            p.sel,
            n - 1,
            "backward from the first button wraps to the last"
        );
        assert!(p.forward_button(1));
        assert_eq!(p.sel, 0, "forward from the last button wraps to the first");
    }

    #[test]
    fn help_push_button_follows_the_clicked_row() {
        let mut p = HelpPanel::new();
        let target = p.matches()[2];
        let title = p.entries[target].title.clone();
        p.push_button(2);
        assert_eq!(
            p.current_title(),
            Some(title.as_str()),
            "push-button on a help xref is help-follow"
        );
        assert_eq!(p.history, vec![target], "the visit is recorded");
        assert_eq!(p.sel, 0, "the one-topic view selects its single row");
    }

    #[test]
    fn help_follow_visits_the_cross_reference_at_point() {
        let mut p = HelpPanel::new();
        let target = p.matches()[3];
        let title = p.entries[target].title.clone();
        p.sel = 3;
        assert!(p.follow(), "a row is a cross-reference, so RET follows it");
        assert_eq!(p.current_title(), Some(title.as_str()));
        assert_eq!(p.history, vec![target]);
        // With nothing matching, there is no cross-reference to follow.
        let mut empty = HelpPanel::new();
        empty.filter = "zz-no-such-entry-zz".into();
        assert!(!empty.follow());
    }

    #[test]
    fn help_view_source_finds_the_defining_fn() {
        // `help-view-source` resolves a command to `fn <name>(` in the sources
        // (the zmax analog of find-function-search-for-symbol). Scanning this
        // crate must at least turn up its own resolver.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let hit = locate_definition_in(root, "fn locate_definition_in(");
        let (path, line) = hit.expect("should find its own definition in the crate tree");
        assert!(path.to_string_lossy().ends_with("help.rs"));
        assert!(line > 0);
        // A name with no matching `fn` yields no source, as Emacs errors when
        // `:file` is undefined.
        assert!(locate_definition_in(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            "fn zz_no_such_command_definition(",
        )
        .is_none());
    }

    #[test]
    fn help_indexes_commands_keys_topics() {
        let p = HelpPanel::new();
        let cmds = p.entries.iter().filter(|e| e.cat == Cat::Commands).count();
        let topics = p.entries.iter().filter(|e| e.cat == Cat::Topics).count();
        let with_keys = p.entries.iter().filter(|e| !e.keys.is_empty()).count();
        eprintln!("help: {cmds} commands, {topics} topics, {with_keys} with keybindings");
        assert!(cmds > 200, "expected the full command surface, got {cmds}");
        assert!(
            topics >= 140,
            "the topic list should index the command surface, got {topics}"
        );
        assert!(
            with_keys > 50,
            "expected many commands to show keys, got {with_keys}"
        );
    }

    /// A topic that names a command the editor does not have is worse than no
    /// topic: the reader types it and gets "no such command". This caught
    /// `:debug-breakpoint`, which was never registered — the breakpoint is
    /// toggled by a static command, not a typable one.
    #[test]
    fn every_command_a_topic_names_is_registered() {
        let known: std::collections::HashSet<&str> = crate::commands::typed::TYPABLE_COMMAND_LIST
            .iter()
            .flat_map(|c| std::iter::once(c.name).chain(c.aliases.iter().copied()))
            .collect();

        let mut missing: Vec<(&str, String)> = Vec::new();
        for (title, body) in TOPICS {
            for (i, tail) in body.match_indices(':').map(|(i, _)| (i, &body[i + 1..])) {
                // Skip prose that merely contains a colon — `http://`, and the
                // `<host[:port]>` placeholder. A command reference starts a word.
                if body[..i].ends_with(|c: char| c.is_alphanumeric() || c == '[') {
                    continue;
                }
                let name: String = tail
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                // A trailing `-` marks a family (`:pio-remote-*`), not a command.
                if name.len() < 3 || name.ends_with('-') {
                    continue;
                }
                if !known.contains(name.as_str()) {
                    missing.push((title, name));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "topics name commands that are not registered: {missing:?}"
        );
    }

    /// The same guarantee for the static commands, which topics name without a
    /// leading `:`. Any bare `snake_case` word in a topic body has to be a real
    /// static command — the alternative is a topic that tells the reader to
    /// press a key that runs nothing.
    #[test]
    fn every_static_command_a_topic_names_is_registered() {
        let known: std::collections::HashSet<&str> =
            crate::commands::MappableCommand::STATIC_COMMAND_LIST
                .iter()
                .map(|c| c.name())
                .collect();
        // Prose that happens to be snake_case: file names and shell variables.
        const NOT_COMMANDS: &[&str] = &["compile_commands", "title__keywords"];
        // Language names and language-server names are snake_case too, and the
        // language topics are generated from this table — so read the same table
        // rather than keeping a hand-written list of exceptions beside it.
        let lang_table = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../book/src/generated/lang-support.md"
        ))
        .expect("the generated language table should be in the repo");
        let from_table: std::collections::HashSet<String> = lang_table
            .lines()
            .filter(|l| l.starts_with("| "))
            .flat_map(|l| l.split(['|', '`', ',', ' ']))
            .map(|w| w.trim().to_string())
            .filter(|w| w.contains('_'))
            .collect();

        let mut missing: Vec<(&str, String)> = Vec::new();
        for (title, body) in TOPICS {
            for word in body.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                if !word.contains('_') || word.starts_with('_') || word.ends_with('_') {
                    continue;
                }
                if word.chars().next().is_none_or(|c| !c.is_ascii_lowercase()) {
                    continue;
                }
                if known.contains(word) || NOT_COMMANDS.contains(&word) || from_table.contains(word)
                {
                    continue;
                }
                missing.push((title, word.to_string()));
            }
        }
        assert!(
            missing.is_empty(),
            "topics name static commands that are not registered: {missing:?}"
        );
    }
}
