<!--
# YY.0M (YYYY-0M-0D)

Breaking changes:

Features:

Commands:

Usability improvements:

Fixes:

* Git gutters no longer keep showing pre-commit hunks after a commit made
  outside the editor. The gutter diffs each buffer against HEAD's blob, and the
  in-editor git commands re-fetch that base after they move HEAD — but a `git
  commit` run in another terminal writes only inside `.git`, leaving the working
  tree byte-identical, and the filesystem watcher discarded every `.git` path
  before looking at it. The watcher now also watches the repository's git
  directory (found even when zmax was launched from a subdirectory, and
  including the common directory of a linked worktree) and re-fetches every open
  buffer's diff base when HEAD, `ORIG_HEAD`, `packed-refs` or a `refs/heads/…`
  tip is written — i.e. on an external commit, checkout, reset, rebase or pull.
  Staging (`.git/index`) and fetched remote tips deliberately do not fire it: the
  base is HEAD, and neither moves HEAD. Only the base is re-read, never the
  buffer text, so unsaved edits are untouched.
* Vim `zM` / `za` / `zc` / `zj` / `zk` no longer do nothing on a freshly opened
  buffer. The fold set was only ever populated by `:set foldmethod=…`, and the
  default `foldmethod=manual` computes no folds, so "close all folds" iterated an
  empty set. The `z` fold family now builds the folds on demand from the current
  `foldmethod`, falling back to the tree-sitter function/class regions and then
  to indentation when the method computes none (`manual`, `expr`, `diff`). Folds
  made by hand with `zf` are left alone. Tests added driving `zM`/`zR`/`za` as
  keystrokes through the shipped keymap.
* Vim `foldlevel` is a real fold level now, and `zm` / `zr` are real commands.
  Both used to be aliases for `zM` / `zR` (close/open *everything*), and the fold
  model carried no level at all. Folds now have vim's level (an outermost fold is
  level 1, each containing fold adds one) and the buffer carries a `foldlevel`:
  `zm` closes one more level of nesting, `zr` opens one, `zM` drops the level to
  0, `zR` raises it to the deepest fold, and `:set foldlevel=N` / `foldlevelstart`
  set it directly instead of the old "N > 0 ? open everything : close everything".
* Emacs calendar-date diary sexps — `%%(diary-julian-date)`, `%%(diary-iso-date)`,
  `%%(diary-mayan-date)` and `%%(diary-persian-date)` are recognized; they apply
  every day and display the date rendered in that calendar (a new `CalendarDate`
  diary spec with dynamic `Entry::display_text`).
* Emacs Mayan calendar navigation — in the calendar overlay, `m` jumps to a Mayan
  long-count date (`b.k.t.u.kin`), `H`/`B` jump to the next/previous date with a
  given haab, `T`/`Y` to the next/previous tzolkin, and `R` to the next calendar
  round (haab + tzolkin) — `calendar-mayan-goto-long-count-date` /
  `-next-haab-date` / `-previous-haab-date` / `-next-tzolkin-date` /
  `-previous-tzolkin-date` / `-next-calendar-round-date`, on the existing
  `cal-mayan` conversions.
* Emacs doc-view — read a PDF/PS/DVI/EPUB in the terminal: `:doc-view-mode` renders
  the current page (pdftocairo / mutool / magick / pdftoppm → terminal image viewer,
  via the tty handoff); `:doc-view-next-page`/`-previous-page`/`-first-page`/
  `-last-page`/`-goto-page` navigate; `:doc-view-enlarge`/`-shrink` change the
  render resolution; `:doc-view-open-text` extracts the text (pdftotext) and
  `:doc-view-search` greps it; `:doc-view-clear-cache` resets the render state.
* Emacs image-mode — view and transform an image file in the terminal:
  `:image-mode`/`:image-toggle-display` display it (via the tty-handoff image
  viewer), `:image-rotate` / `:image-flip-horizontally` / `:image-flip-vertically`
  transform it with ImageMagick and redisplay (accumulating per image),
  `:image-next-file`/`:image-previous-file` walk the directory's images, and
  `:image-mode-copy-file-name-as-kill` copies the path. Shares the image-display
  substrate with Dired.
* Emacs Dired inline image display — `M-i` shows the marked images (or the one at
  point) in the terminal via the first available viewer (chafa / kitty +kitten
  icat / imgcat / viu / timg / catimg), using a new full-screen tty handoff
  (`Editor::pending_tty_command` + `Application::drain_tty_command`, mirroring the
  fzf handoff): zmax leaves the TUI, the viewer renders the image, and Enter
  returns. Covers `image-dired-dired-display-image`/`display-this`/`display-thumbs`
  /`toggle-marked-thumbs`; `M-o` still opens in the OS external viewer.
* Emacs Dired image-dired comments/tags — `M-a` sets a comment on marked images
  (`image-dired-dired-comment-files`), `M-b` sets `comment;tags` on the file at
  point (`image-dired-dired-edit-comment-and-tags`), `M-h` sets its description
  (`image-dired-thumbnail-set-image-description`), stored in an image-dired db.
  `image-dired-dired-display-image`/`display-this` show the image via the external
  viewer (`M-o`).
* Emacs Dired epa/wdired/find-replace/locate/image — `M-e`/`M-k`/`M-z`/`M-v` run
  gpg encrypt/decrypt/sign/verify on marked files (`epa-dired-do-*`), `M-w` opens
  an editable name listing and `:wdired-finish-edit` applies the renames (wdired),
  `M-q` does a bulk regexp find-and-replace across marked files
  (`dired-do-find-regexp-and-replace`), `M-c` runs `locate` filtered to the dir,
  and `M-o` opens the image at point in the external viewer.
* Emacs Dired man/print/find/other-tab/tree — `M-m` shows a file's man page in a
  scratch buffer (`dired-do-man`), `M-r` prints marked files via lpr/lp
  (`dired-do-print`), `M-t` opens the file at point in a new tabpage
  (`dired-other-tab`), `M-f`/`M-g` list `find -name` / `find … grep` matches
  (`find-name-dired`/`find-grep-dired`), `M-j` jumps to a named subdir section
  (`dired-goto-subdir`), and `M-u`/`M-y` move to the top / first subdir section
  (`dired-tree-up`/`dired-tree-down`).
* Emacs Dired elisp file ops + subdir motion — `M-l` evaluates marked `.el` files
  through the embedded elisp (`dired-do-load`), `b` validates them
  (`dired-do-byte-compile`; interpreted, no `.elc`), and `M-n`/`M-p` move between
  inserted subdir sections (`dired-next-subdir`/`dired-prev-subdir`).
* Emacs Dired subdirectory insertion — `i` on a directory inserts its listing as
  an inline section (entries carry a `reldir/` prefix so file ops still resolve),
  `$` collapses the section at point, and `M-$` toggles all sections
  (`dired-maybe-insert-subdir` / `dired-hide-subdir` / `dired-hide-all`). `i` on a
  file still starts filename isearch.
* Emacs tab-bar — real tab substrate: named tabs (`tab-rename`, `tab-switch` by
  name/number), a closed-tab stack (`tab-undo` reopens the last closed tab),
  visited-tab history (`tab-bar-history-back`/`-forward`, `tab-bar-history-mode`),
  and `tab-bar-mode` to toggle the tab bar. Adds a `name` field to `TabPage` plus
  closed-tab/history state on the editor.
* Emacs timeclock (`timeclock.el`) — real time-tracking substrate: a pure
  `zmax-core::timeclock` model (file-backed timelog, interval math in Unix
  seconds, unit-tested) plus commands `timeclock-in`/`-out`/`-change`,
  `-workday-remaining`, `-when-to-leave`, `-reread-log`, `-mode-line-display`.
* Vim `:append`/`:insert`/`:change` — real Ex line-input substrate: a new
  `ExInput` modal (`ui/ex_input.rs`) collects typed lines until a lone `.` and
  inserts them as one undo step — after the current line (`:append`), before it
  (`:insert`), or replacing it (`:change`). `Esc` aborts (nothing inserted or
  deleted); `Backspace` edits the current line.
* Emacs `abbrev-mode` — real auto-expansion substrate: a new `abbrev-mode` minor
  mode (`:abbrev-mode [on|off]`, off by default) hooks the Insert self-insert
  path so typing a word separator expands the abbrev before point, mirroring
  emacs's `self-insert-command` calling `expand-abbrev`. Zero overhead when off.
* Emacs mode-local abbrevs — real substrate: per-major-mode abbrev tables in
  `emacs_abbrev` (keyed by the buffer's language, `fundamental` when none).
  `expand-abbrev` now searches the buffer's mode table before the global one.
  New commands `define-mode-abbrev` (`:define-mode-abbrev NAME EXPANSION`),
  `add-mode-abbrev` (C-x a l), and `inverse-add-mode-abbrev` (C-x a i l).
* Vim `:undojoin` — real undo substrate: `History::merge_last_revision` composes
  the next committed change into the current undo revision (forward + inverse
  transactions), so a single undo reverts both. An undo/redo cancels a pending
  join (vim's E790).
* Vim `:sign` (define/undefine/place/unplace/list/jump) — real sign substrate:
  named definitions (`text=`/`texthl=`) placed on file lines by id/group/priority,
  drawn in a new sign gutter column that stays zero-width until a sign is placed
  (`signcolumn=auto`). `:sign unplace *` clears all; `:sign jump {id} file={f}`
  opens the file and moves to the sign.
* Vim `CTRL-W T` moves the current window to a new tabpage (refuses when it is
  the sole window of the tabpage, matching vim).
* Vim `:match`/`:2match`/`:3match` highlight a pattern in one of three
  independent match slots on the Hi-Lock overlay; `:{N}match none` clears a slot
  (colour is chosen by index rather than the named highlight group).
* Vim `:diffoff` turns diff mode off by removing the side-by-side diff overlay.
* Vim `:unhide`/`:sunhide` open a window for each loaded buffer (aliases of
  `:ball`; every zmax buffer is loaded).
* Vim `:helptags` is accepted as a no-op — zmax's help is indexed directly and
  needs no tag file.
* Vim `CTRL-W CTRL-_` (same as `CTRL-W _`) now maximizes the window height,
  aliased alongside `CTRL-W _`.
* Vim `:argglobal`/`:arglocal` dispatch as aliases of `:args` (the single global
  argument list approximates vim's global/window-local arg-list variants).
* Vim `:lnfile`/`:lNfile` — location-list "first entry in next file" / "last
  entry in previous file" (QfKind::Location variants of `:cnfile`/`:cpfile`).
* Vim `:cNfile` now correctly jumps to the last entry in the PREVIOUS file
  (`:cpfile`); it had been mis-aliased to `:cnfile` (next file).
* Vim `:lolder`/`:lnewer`/`:lhistory` — a per-window location-list history
  (new View state, pushed on each `:lexpr`-style replace), mirroring the quickfix
  `:colder`/`:cnewer`/`:chistory` history.
* Vim `:doautoall {event}` fires the event autocommands for every loaded buffer
  (not just the current one), reporting the buffer count.
* Vim `:saveas`, `:setglobal`/`:setlocal`, `:setfiletype`, `:lvimgrep`, `:ptag`,
  `:bunload`/`:bwipeout`, `:bNext`, `:cNfile`, and `:quitall` now dispatch as
  aliases of their zmax equivalents (previously they errored "no such command"
  despite being documented as ported).
* Vim `:colorscheme` (`:colo`), `:enew`, `:ascii`, and `:chdir` now dispatch as
  aliases of `:theme`/`:new`/`:character-info`/`:cd` (previously they errored
  "no such command" despite being documented).
* Vim `:lcd`/`:tcd`/`:lchdir`/`:tchdir` (window/tab-local cd variants) now
  dispatch as aliases of the global `:cd` (previously they errored "no such
  command").
* Vim `:balt` adds a file to the buffer list and sets it as the alternate file
  (the `CTRL-^` target) without switching to it.
Themes:

New languages:

Updated languages and queries:

Packaging:
-->

# zmax (unreleased)

Changes in zmax.

Features:

* `transparent-background` editor option — when set, zmax skips painting the
  `ui.background` fill across the editor surface, the gutter/sign column, and the
  IDE file-tree sidebar (and stops pushing a theme background to the terminal via
  OSC), so a translucent/blurred terminal window shows through. Themed chrome
  (statusline, breadcrumbs, panel status bars) keeps its colors. Defaults to
  `false`.
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

* Restart (`SPC q r`, restart-emacs): `restart_editor` refuses on unsaved
  buffers, else closes every view and re-execs zmax with the same arguments
  once the UI loop has exited and the terminal is restored.
* Frame/display toggles (`SPC T T`/`SPC T B` transparency, `SPC T f` fringe,
  `SPC t m T` mode line): background transparency and the mode line reuse the
  existing `:toggle` substrate (`transparent-background`, `render-statusline`);
  `toggle_fringe` hides/shows the whole gutter column strip, stashing and
  restoring the layout. These render in both the terminal and the zmax-gui
  window (which wraps the same editor in an embedded PTY).
* Command profiler (`SPC h P s/k/r/w`): `profiler_start`/`profiler_stop`
  instrument the single command-dispatch point (`MappableCommand::execute`) to
  time every command run; `profiler_report` shows per-command call count / total
  ms / avg µs (slowest first) and `profiler_write_report` writes it to a file —
  the Spacemacs `SPC h P` profiler tree.
* Regexp string generation (`SPC x r '`, `SPC x r p '`, `SPC x r e '`):
  `regexp_generate_strings` enumerates every string a *finite* regexp matches
  (literals, `( )`, `|`, `[ ]`/ranges, `?`, `{n,m}`; unbounded `*`/`+`/`.` are
  rejected), the `e '` variant accepting Emacs regexp syntax.
* Link hints (`SPC x A/O/y/Y`): `open_all_buffer_links` opens every URL in the
  buffer, `link_hint_open_link`/`link_hint_copy_link` pick one URL from the
  buffer to open or copy, and `copy_all_buffer_links` copies them all — the
  Spacemacs `link-hint-*` family, over the buffer's `http(s)://`/`www.` links.
* Directory ediff (`SPC D d d`, `SPC D d 3`): `ediff_directories` /
  `ediff_directories3` prompt for two or three directories and render the
  same-name files that differ (plus files unique to one side) into a scratch
  overview — the Emacs `ediff-directories` / `ediff-directories3` overview.
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
* Vimscript passthrough: any `:` command zmax does not define is run by the
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

* Vim `:echoerr` / `:echoe` prints its arguments to the statusline as an error.
* Vim `:{range}` with no command moves the cursor to the last line of the range
  (`:2,4` → line 4, `:$`, `:.+3`, `:'<,'>`), landing on its first non-blank.
* Vim `:ijump` / `:ij` jumps to the first whole-word occurrence of an identifier
  from the top of the buffer (a `/pat/` argument is a regex); records a jump.
  `:djump` / `:dj` does the same but only for a macro's `#define` line.
  `:isplit` / `:dsplit` split the window first, then jump. `:ilist` / `:dlist`
  list every matching line / `#define` line in a scratch buffer. `:digraphs` /
  `:dig` lists the digraph table in a scratch buffer. `:z` prints a window of
  lines from the cursor into a scratch buffer. `:checkpath` lists the
  files `#include`d by the current buffer. `:exusage` lists the
  available Ex commands in a scratch buffer. `:viusage` lists the
  Normal-mode commands. `:isearch` / `:dsearch`
  echo the first matching line without moving the cursor.
* Vim `:@{reg}` executes a register's contents as Ex command line(s) (also
  `:execute-register`; `:@:` re-runs the last command line). `:@@` repeats the
  last `:@`.
* Vim `:iput` / `:ip` puts a register below the cursor, re-indenting the block so
  its first line matches the current line's leading whitespace (later lines shift
  by the same amount; blank lines stay empty).
* Vim `:changes` (open the buffer changelist picker, the analog of `:jumps`) is
  now tracked as ported.
* Vim `:dl` (`:delete` with the `l` flag) deletes the current line(s) and
  echoes the deleted line in `:list` format (`$` marks the line end).
* Vim `:smagic` / `:snomagic` run `:substitute` forcing the 'magic' / 'nomagic'
  level on the pattern (e.g. `:snomagic/a.c/x/` treats `.` literally). Both the
  no-space form (`:%snomagic/…`) and the space form are supported.
* Vim `:=` echoes the last line number of the buffer (its line count) to the
  status line (also `:print-line-number`).
* Vim `''` (jump to the line before the latest jump) is now tracked as ported:
  `G`/search/`gg` and other jumps record the `` ` ``/`'` context marks, and `''`
  returns to that line's first non-blank character.
* Vim `{count}S` (substitute line, == `{count}cc`) now honors the count: it
  changes `count` lines into the optional register, collapsing their content to
  one empty line to insert on and keeping the trailing newline (previously `S`
  changed only the current line). Applies when vim-sneak is off.
* Vim `{count}s` (substitute char) now honors the count: it deletes `count`
  characters forward, bounded to the current line, into the optional register,
  then enters insert (previously `s` changed only the single cursor character).
  Applies when vim-sneak is off.
* Vim-abolish `:Subvert` (`:S`) now honors `gdefault` like `:s` — with
  `:set gdefault`, the `g` flag is implied so all matches on the line are
  replaced (previously `:S` always used the literal `g` flag and ignored the
  option).
* Vim `:goto` (`:go`) byte-offset motion now available as `:goto-byte` / `:go` /
  `:gob`: 1-based, byte-accurate (multi-byte characters count by UTF-8 length,
  a mid-character offset snaps to the character start). `:goto` itself stays
  line-based for helix compatibility.
* Vim `i_CTRL-R CTRL-R {reg}` / `CTRL-R CTRL-O {reg}` (literal / no-autoindent
  register insert) now work: `CTRL-R` in insert mode accepts a following `CTRL-R`,
  `CTRL-O`, or `CTRL-P` modifier and then the register (zmax already pastes
  register contents literally).
* Vim `0 CTRL-D` / `^ CTRL-D` in insert mode now delete the just-typed `0`/`^` and
  all of the line's indent (plain `i_CTRL-D` still removes one level).
* Vim `<Insert>` in insert mode now toggles between inserting and overtyping
  (Insert ↔ Replace) instead of only switching to Replace one-way.
* Vim `N@:` now repeats the last `:` command N times (previously `@:` ran it once
  regardless of the count).
* Vim `gn`/`gN` now visually select the next/previous search match (into Select
  mode) instead of just jumping like `n`/`N`, so an operator or extension can act
  on it (matching vim; the operator forms `cgn`/`dgn`/`ygn` already worked).
* Vim `_` now honours its count: `3_` jumps to the first non-blank two lines down
  (previously `_` ignored the count and behaved like `^`).
* Vim `:set nomodified` / `:set modified`: `nomodified` marks the buffer as saved
  (clears the modified flag) without writing to disk; `modified` forces the flag on.
* Vim `:set comments`: user-defined line-comment leaders (`{flags}:{leader}`,
  block `s`/`m`/`e` entries skipped) drive comment-leader continuation on `<Enter>`
  and `o`/`O`, taking precedence over the language token and working even in
  plaintext (e.g. `:set comments=:#` continues `#`-prefixed lines). `gq` comment
  reflow is not yet modelled.
* Vim `:set digraph`: `{char1}<BS>{char2}` enters a digraph in insert mode —
  `a<BS>:` yields `ä`, using the built-in digraph table. `<BS>` arms the entry
  (it no longer deletes) with the character before the cursor; the next character
  combines with it, and a half-entered digraph is discarded on `<Esc>`. (`CTRL-K`
  digraph entry keeps working regardless of the option.)
* Vim `:set smartindent`: in a buffer with no tree-sitter indent query
  (plaintext), a line ending in `{` indents the next line one level. Tree-sitter
  languages already do this via their indent query; dedent-before-`}` and
  `cinwords` are not modelled.
* Vim `:set copyindent`: a new line copies the current line's exact leading
  whitespace instead of recomputing the indent — so on `fn f() {` the automatic
  indent-after-`{` is suppressed and the previous line's indent characters are
  preserved verbatim.
* Vim `:set delcombine`: `x` on a composed character (base + combining marks)
  deletes only its last combining mark, leaving the base — e.g. `x` on `é`
  (`e` + U+0301) yields `e`. Default `x` still removes the whole grapheme.
* Vim `:set revins` (reverse insert): each typed character is inserted before the
  previous one, so typing `abc` yields `cba`. Off by default; `:set norevins`
  restores normal insertion.
* Vim `quoteescape` and escaped-quote text objects: `di"`/`ci"`/`i"`/`a"` now skip
  backslash-escaped quotes inside a string, so `di"` on `"a \"b\" c"` spans the
  whole string instead of stopping at the first `\"`. Default escape is `\` (vim's
  default); `:set quoteescape=…` overrides it and `:set quoteescape=` disables it.
* Vim `formatoptions` now honors its common flags: `r`/`o` gate comment-leader
  auto-continuation after `<Enter>` and `o`/`O`; `j` drops the joined comment
  leader on `J` (`// a` + `// b` → `// a b`); `t`/`c` auto-wrap the line past
  `text_width` while typing (via the existing auto-fill). Advanced flags `a`/`n`/`w`
  aren't modelled and `q` (allow `gq`) is effectively always on.
* Vim `:set errorformat` now drives `:make`/compile-output parsing into the
  quickfix list: the common conversion specs (`%f` `%l` `%c` `%m` `%t` `%n` `%%`)
  are compiled to a regex, comma-separated patterns are tried in order, and `\,`
  is a literal comma. Unset falls back to the built-in `file:line:col` heuristic.
* Vim `:set keywordprg` wired to `K`: with it set, `K` runs `<keywordprg> <word>`
  (or substitutes `$*`) on the word under the cursor and shows the output in a
  scratch buffer, matching vim; with no `keywordprg` it falls back to the LSP hover
  popup (the previous default).
* Vim `:set foldmethod=syntax` now computes folds from the tree-sitter
  `function`/`class` text-object captures (previously only `indent` and `marker`
  produced folds; `syntax` cleared them). Folds the enclosing regions of every
  function and class; `foldlevel`/`zM` then close them.
* Vim backup subsystem completed: `:set backupdir` redirects the backup copy into
  its first non-empty directory (auto-created), and `:set backupskip` takes glob
  patterns (`*`/`?`) whose match skips the backup — both honoured by the document
  save path via a new pure, unit-tested `backup_plan` (with a small `glob_match`).
  New `Config` fields `backup-dir`/`backup-skip`.
* Vim `:set foldlevel` relabelled ported (mapping.json was stale; the build map
  already listed it done): `foldlevel=0` closes every fold, a high value opens them,
  driving the folds from `foldmethod`. Test added.
* Vim `:set iskeyword` relabelled ported: it already feeds
  `zmax_core::chars::set_extra_keyword_chars`, so `:set iskeyword=@,48-57,_,45`
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
  sessions** (seeded at startup from the `.zmaxinfo` numbered file marks). A bare
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
