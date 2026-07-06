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

* `cit` changes inside the surrounding (X)HTML tag rather than the tree-sitter
  class.
* Guard against an autosave path that could truncate a file after an undo.
