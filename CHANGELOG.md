<!--
# YY.0M (YYYY-0M-0D)

Breaking changes:

Features:

Commands:

Usability improvements:

Fixes:

Themes:

New languages:

Updated languages and queries:

Packaging:
-->

# zemacs (unreleased)

Changes in zemacs.

Features:

* Transform library: 200+ selection-transform `:` commands — JSON/CSV/TOML
  reshaping (`:json-query`, `:json-to-csv`, `:json-flatten`, `:json-group-by`,
  `:csv-column`, `:md-table`, …), number/stats ops (`:stats`, `:running-total`,
  `:percent-of-total`, `:scale`, `:sum-fields`, `:bases`), identifier case
  (`:to-snake`/`:to-camel`/`:to-kebab`/`:to-pascal`/`:to-constant`), encoders
  (`:base32`, `:caesar`, `:morse`, `:crc32`, `:nato`, plus `SPC x` URL/Base64/
  HTML/JWT/roman/colour), extraction (`:extract-urls`/`-emails`/`-numbers`),
  Markdown/typography, line ops (`:align`, `:reflow`, `:dedup`, `:shuffle`,
  `:sort-by-field`), and generators (`:uuid`, `:lorem`, `:date`, `:seq`). They
  run on the selection (or whole buffer) as one undoable change.
* Org-mode: outline folding, `TODO`/priority cycling, promote/demote, capture,
  and a date-aware agenda across `.org` files (`SPC m …`, `SPC a o …`).
* Snippet library: a `:snippets` editor TUI for reusable snippets (trigger,
  scope, description, LSP-syntax body), persisted to `snippets.toml`. Typing a
  trigger word and pressing `Tab` expands the body with live tab stops; user
  triggers take priority over emmet and are scoped per language.
* Hex editor: a byte-faithful, xxd-style viewer/editor (`:hex`). Binary files
  now open in it automatically instead of being rejected, and it round-trips
  raw bytes exactly on save.
* Merge-conflict resolution: a JetBrains-style 3-pane (ours/result/theirs)
  resolver with a diff3 base pane, inline character highlighting, and
  horizontal scroll (`:merge`), plus a read-only side-by-side diff against git
  `HEAD` (`:diff`). The IDE Git tab lists conflicted files.
* Magit-style git TUI: interactive rebase, per-hunk staging, and branch and
  stash menus.
* Wildfire: `<ret>` selects/expands to the closest text object and grows to the
  next enclosing one; `<backspace>` shrinks back.
* REPL panel (`:repl`, `SPC a r`) fronting all five embedded languages.
* Run-configuration manager and a unified Preferences window
  (Settings/Keymap/Theme/Run-Configs).
* Three live keymaps on one engine: a vim default with the operator-pending
  grammar emulated on the selection engine (`d`/`c`/`y` with motions, `ciw`/`di(`
  text objects, `df,`/`ct)` finds, `.` dot-repeat, `q`/`@` macros, named marks,
  Replace mode), an emacs layer, and a Spacemacs `SPC` leader; switch presets at
  runtime with `:keymap`.
* Spacemacs-style discoverable leader: a labelled `SPC` command tree with
  which-key popups (`auto-info`, tunable per-prefix via `auto-info-exclude`) and
  hundreds of ported bindings across files/buffers/windows/search/git/help/text.
* IDE mode / workbench (`:ide`, `:workbench`, `F2`): a project file-tree, a
  tree-sitter structure outline, problems/run panels, and an error-stripe
  minimap, with the full layout persisted to appdata.
* Integrated terminal (`:terminal`/`:term`): a PTY shell in a pane, with a `C-\`
  window leader for split/focus and click-to-focus across panes.
* Startify-style start screen on launch: recent files ranked by frecency and by
  MRU, under a fortune/cowsay header.
* Mouse support: click to focus a pane, scroll, click tabs and the gutter, and
  drag the split divider to resize windows.
* Searchable Help browser (`:help`/`SPC h h`) over every command, key, and
  topic; the `SPC h` describe-* family routes symbol lookups through LSP hover.

Commands:

* `:ide`/`:workbench` (`F2`, `SPC z`), `:terminal`/`:term` (`SPC p '`), `:repl`
  (`SPC a r`), `:preferences` (`SPC ,`), `:help`/`:h` (`SPC h h`), `:keymap`

* `:snippets`/`:snip`, `snippet_expand`, `goto_next_tabstop`/`goto_prev_tabstop`
* `:hex`/`:hexview`/`:hexedit`
* `:diff`/`:gdiff`, `:merge`/`:resolve`, `:conflict-ours`/`:conflict-theirs`/
  `:conflict-both`, `:conflict-next`/`:conflict-prev`
* `goto_next_conflict`/`goto_prev_conflict` (`]n`/`[n`), `resolve_conflicts`
* Quickfix/location-list navigation on vim-unimpaired keys: `]q`/`[q`
  (`quickfix_next`/`quickfix_prev`) and `]l`/`[l` (`loclist_next`/`loclist_prev`).
* Vim command-name aliases: `:split` (→ `:hsplit`), `:b` (→ `:buffer`), and
  `:bd`/`:bdelete` (→ `:buffer-close`).
* `:close`/`:clo` (close the current window, refusing the last one) and
  `:only`/`:on` (close every other window).
  (`SPC g m`, `SPC g c r`), `conflict_take_all_ours`/`conflict_take_all_theirs`
  (`SPC g c O`/`SPC g c T`)
* `wildfire`/`wildfire_shrink` (`<ret>`/`<backspace>`)
* Vimscript passthrough: any `:` command zemacs does not define is run by the
  embedded vimlrs interpreter, so `:call`/`:execute`/`:if …|…|endif`/`:for`/
  `:while`/`:function`/`:return`/`:try`/`:throw`/`:break`/`:continue`/`:unlet`
  and other VimL statements work at the command prompt — Vim's `:` prompt is the
  Vimscript engine.
* `:source`/`:so <file>` — source a Vimscript file through vimlrs with script
  context (`s:` scope, `<SID>`, line continuations).
* `:windo <cmd>` — run an ex-command in each window; `:wincmd <key>` — run a
  window (CTRL-W) command by key.
* `:make [args]` — run make, collect errors into the quickfix list (shared with
  `:cnext`/`:cc`/`:copen`), and jump to the first error.
* `:messages` / `:mes` — a session message log (every status/error/warning
  shown), displayed newest-last like vim `:messages` / the emacs `*Messages*`
  buffer. Backed by a capped ring on the editor.
* `:redir @a` / `:redir > file` / `:redir >> file` … `:redir END` — capture the
  message output between start and END into a register or file (vim `:redir`).
* Neovim command names: `:Inspect` / `:InspectTree` (the tree-sitter highlight
  capture / syntax subtree under the cursor) and `:Man {topic}` (open a man page
  in the run console).
* `:undolist` — a terse text list of undo states (number, age, current marker),
  the textual companion to the visual `:undotree` (vim `:undolist`).
* Vim tag stack over a ctags `tags` file: `:tag {name}` jumps to a definition
  and pushes the stack, `:tnext`/`:tprevious`/`:tfirst`/`:tlast` cycle matches,
  `:pop` returns, `:tags` shows the stack, `:tselect`/`:tjump` pick among matches
  and `:stag` opens the definition in a split — LSP-independent navigation,
  distinct from the `:Tags`/`:BTags` fzf pickers.
* `safe_delete` — JetBrains-style Safe Delete: remove the symbol under the
  cursor only if it has no other references, else show the usages.

Usability improvements:

* IDE layout (drawer widths, hidden/closed panels, folds, minimap) and the
  active colorscheme persist to appdata and are restored on `:ide`.
* Auto-reload (vim `autoread`): externally changed files are reloaded, keeping
  local edits on conflict; on by default and configurable.

Fixes:

* Vim `:set foldlevel` relabelled ported (mapping.json was stale; the build map
  already listed it done): `foldlevel=0` closes every fold, a high value opens them,
  driving the folds from `foldmethod`. Test added.
* Vim `:set iskeyword` relabelled ported: it already feeds
  `zemacs_core::chars::set_extra_keyword_chars`, so `:set iskeyword=@,48-57,_,45`
  makes `w`/`b`/`e`/text-objects treat the named characters (here `-`) as word
  chars. Added tests pinning it plus the already-effective `foldmethod=indent`
  (fold recompute) and `bomb`/`nobomb` (document BOM toggle).
* Vim `:set` options — genuine behaviour (not just round-trip): `commentstring`
  (e.g. `:set commentstring=#%s`) now overrides the comment operator's line-comment
  token; `startofline` is honoured by bare `G`/`gg` (and `{count}G`) — `nostartofline`
  keeps the cursor column instead of jumping to the first non-blank; `nrformats`'s
  real effect on `CTRL-A`/`CTRL-X` is confirmed. The remaining unported options are
  subsystem-scale (folds, live spell render, diff, backup, persistent undo, encoding,
  windows, tags) and tracked honestly in `port/mapping.json`.
* Keybinding coverage: filled missing canonical default bindings across presets
  (each maps to an already-existing command). **Emacs**: `M-m` (back-to-indent),
  `M-c`/`M-u`/`M-l` (capitalize/upcase/downcase-word), `M-z` (zap-to-char), `M-h`
  (mark-paragraph), `M-{`/`M-}` (paragraph), `M-.`/`M-,`/`M-?` (xref), `M-g g/n/p`
  (goto-line / next- & previous-error), `C-M-a`/`C-M-e` (defun), `C-M-\` (indent),
  `C-M-s`/`C-M-r` (regexp isearch), `C-t`/`M-t` (transpose chars/words), `M-\`/
  `M-SPC` (delete-horizontal-space / just-one-space), `C-M-o` (split-line), and
  under `C-x`: `}`/`{`/`^`/`+` (window resize/balance), `<left>`/`<right>`
  (prev/next-buffer), `C-o` (delete-blank-lines), `C-;` (comment-line), `r t`
  (string-rectangle), `z` (repeat). **Vim/spacemacs**: `gcc` and visual `gc`
  (comment operator), `gO` (document symbols), visual `g?` (ROT13) and visual
  `gu`/`gU`/`g~` (case). **Spacemacs** `C-x` overlay gains the same window-resize
  and transpose/rectangle/repeat siblings.
* Incremental search cycling (`C-g`/`C-t`): while typing a `/` or `?` search,
  `C-g` advances the preview to the next match and `C-t` retreats to the previous
  one; pressing Enter commits to the cycled match instead of snapping back to the
  first hit, and editing the pattern resets the cycle. Vim/spacemacs presets only.
* Confirmed substitute (`:s/pat/rep/c`): the `c` flag prompts at each match with
  `y` (replace), `n` (skip), `a` (replace this + all remaining), `l` (replace this
  then stop), `q`/`Esc` (stop). The current match is highlighted while prompting
  and all accepted replacements commit as one undo step. Vim/spacemacs presets only.
* Emacs keymap: added standard bindings that were missing — `C-o` (open-line),
  `M-;` (comment), `M-^` (join line), `M-q` (fill-paragraph), and in the `C-x`
  prefix `C-x C-t` (transpose lines), `C-x h` (mark whole buffer), `C-x C-l`/`C-x
  C-u` (downcase/upcase region). Vim-specific commands are pinned out of the
  `emacs`/`helix` presets by a keymap test so vim bindings can't leak into them.
* Last-position restore (vim `` `" ``): reopening a file returns the cursor to
  where it was when the buffer was last closed — within a session and **across
  sessions** (seeded at startup from the `.zemacsinfo` numbered file marks). A bare
  `:e file` restores it; `:e file:line` still jumps to the explicit position.
* Visual-block `I`/`A`: `I` now inserts at the block's left column on every row
  (not the active cursor column), and `A` appends at the right column + 1, padding
  rows shorter than that column with spaces so the append lands correctly (vim's
  virtual-space behavior) instead of skipping them.
* Forced charwise motion `dvj`/`dvk` (and `cvj`/`cvk`/`yvj`/`yvk`): `v` after an
  operator forces the normally-linewise vertical motion to operate charwise (from
  the cursor to the same column on the target line).
* `cgn`/`dgn`/`ygn` (and `cgN`/`dgN`/`ygN`): operate on the last-search match at or
  after the cursor, so `cgnNEW<Esc>` then `.` walks and changes successive matches
  (vim's change-next-match workflow).
* `d}`/`c}`/`y}` and `d{`/`c{`/`y{` follow vim's exclusive→linewise promotion: at
  column 0 they take whole lines and stop before the blank separator (previously
  they deleted charwise through the blank line).
* `:{range}normal` and `:g/pat/normal {keys}` replay a key sequence over each line
  in a range / each matching line (`:%normal A;`, `:2,3normal I#`,
  `:g/TODO/normal A!`, `:g/x/normal dd`). Lines are processed bottom-up so edits
  don't renumber the rest, and an implicit `<Esc>` returns to Normal between lines.
* Ex line ranges for `:sort` and `:move`/`:copy`: `:%sort`, `:2,5sort n`,
  `:'<,'>sort!`, and source ranges for move/copy — `:1,5m$`, `:'<,'>t0`, `:.co$`.
  Ranges accept `%`, `.`, `$`, line numbers, `N,M`, `'<,'>`, address arithmetic
  (`.+3`, `$-1`, `5+2`, `.,+2`), named-mark addresses (`:'a,'bsort`), and pattern
  addresses (`:/foo/,$sort`, `:/start/,/end/normal …`) — shared by
  `:sort`/`:normal`/`:m`/`:t`/`:!` ranges.
* `nrformats+=alpha`: with `alpha` in `nrformats`, `CTRL-A`/`CTRL-X` step a lone
  letter (`a`→`b`), clamping at the `a`/`z` boundary. Off by default (numbers and
  dates only), matching vim.
* Vim `:sort` line sort with bare-letter flags: `:sort`, `:sort!` (reverse),
  `:sort n` (numeric), `:sort i` (ignore case), `:sort u` (unique), and
  combinations like `:sort! ni`. Sorts the whole buffer (or the visual selection's
  lines). `vim`/`spacemacs` presets; `helix` keeps its selection-based `:sort`.
* `gq`/`gw` reflow to `text-width` (vim), hard-wrapping the motion's lines instead
  of running the LSP formatter. `gq` leaves the cursor at the end of the reflowed
  text, `gw` restores it to the start; both take motions (`gqq`, `gqj`, `gqG`,
  `gq}`, and the `gw` equivalents).
* Vim range filters: `:%!cmd`, `:.!cmd`, `:N,M!cmd`, `:'<,'>!cmd` pipe the range's
  lines through a shell command and replace them with its output (e.g. `:%!sort`,
  `:.!tr a-z A-Z`). Bang commands like `:w!`/`:q!` are unaffected.
* Real changelist for `g;`/`g,`: they now walk the per-buffer list of edit
  positions (older/newer) with a count (`3g;`), instead of both jumping to the
  single last change. Edits update one entry per line; positions track the text
  through later edits. Added `:changes` to pick from the list.
* `:s` replacement case folding: `\U`/`\L` uppercase/lowercase the following text
  until `\E`, and `\u`/`\l` fold the next character — e.g. `:s/\(\w\+\)/\u\1/`
  title-cases a word. Backreferences (`\0`-`\9`, `&`) and `\n`/`\t`/`\r` escapes
  keep working.
* Search offsets: `/pat/e` (end of match), `/pat/s`/`/pat/b` (start), `/pat/e-1`,
  `/pat/s+2`, and line offsets `/pat/+2`/`/pat/-1` (a line below/above at the first
  non-blank). `vim`/`spacemacs` presets; escape a literal `/` in the pattern as `\/`.
* `[count]/pat` / `[count]?pat`: a count before a search jumps to the count-th
  match (previously the count was ignored and it went to the first).
* `n`/`N` respect the last search direction: after a backward `?pat`, `n` now
  repeats backward and `N` forward (previously `n` was always forward). `/` and
  `*` set the direction forward, `?` and `#` backward. `vim`/`spacemacs` presets.
* Vim "magic" regex is translated to the engine's syntax in `/`, `?`, `n`, `N`
  search **and** in `:s`/`:g`/`:v` patterns, so vim muscle-memory works:
  `\(foo\|bar\)\+` is now a group + alternation + quantifier (not a literal-text
  search), `a\{2,3}` a counted quantifier, and a bare `(`/`|`/`+` a literal.
  Honors `\v`/`\m`/`\M`/`\V` magic levels, `\c`/`\C` inline case, and
  `\a`/`\l`/`\u`/`\x`/`\h` character-class aliases. Applies to `vim`/`spacemacs`
  presets only; `helix`/`emacs` keep native Rust-regex syntax. Note: a `:s`
  pattern is now vim-magic under these presets, so a bare `(o+)` matches the
  literal text — use `\(o\+\)` for a group (as in vim).
* Operator + motion counts now multiply like vim: `2d3w` deletes `2 * 3 = 6`
  words instead of concatenating the digits into a `23`-word delete (a silent
  over-deletion). Applies to `vim`/`spacemacs` presets only.
* `cit` changes inside the surrounding (X)HTML tag rather than the tree-sitter
  class.
* Guard against an autosave path that could truncate a file after an undo.
