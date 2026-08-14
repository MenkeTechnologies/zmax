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
        assert!(topics >= 8);
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
}
