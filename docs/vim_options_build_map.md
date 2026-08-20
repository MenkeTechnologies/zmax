# Vim `:set` option build map

Goal: zmax genuinely honors every vim option (`:set …` has real effect), not
just stores/round-trips it. `:set` lives in `zmax-term/src/commands/typed.rs`
(`fn vim_set`, VIM_OPTIONS table + special arms); the recognized-name/round-trip
table is `zmax-term/src/commands/vim_options_data.rs`; `EditorConfig` is
`zmax-view/src/editor.rs` (`struct Config`).

Status is tracked honestly in `port/mapping.json` (ported = real effect; partial
= accepted + `:set opt?` round-trips but no behavior yet; the store gives the
partial). Derived from a 4-agent source audit 2026-07-06.

## Done (real effect)
Table/arms: number, relativenumber, wrap, linebreak, ignorecase, smartcase,
wrapscan, hlsearch, cursorline, cursorcolumn, scrolloff, textwidth, termguicolors,
mouse, list, autoread, endofline, fixendofline, splitright, splitbelow, signcolumn,
showtabline, foldenable, foldlevel, laststatus, shell, listchars, showbreak,
colorcolumn, fileformat, expandtab, tabstop, shiftwidth, softtabstop, readonly,
modifiable, clipboard, gdefault, updatetime, smarttab, filetype, syntax,
guicursor. Always-on-faithful (credited): hidden, autoindent, backspace, casemap,
belloff, wildchar, wildmenu, encoding, fileformats, ttyfast, wildignorecase,
window, ruler, showmode.

Folding (ported from neovim `fold.c`, so the fold tree nests rather than being
approximated): foldmethod — which owns the buffer, with manual/marker/indent/
expr/syntax all real and no fallback chain between them — plus foldexpr,
foldmarker, foldlevelstart (applied wherever folds are built, not only on
`:set foldmethod`) and foldcolumn (the fold column is in the default gutter
layout). `foldenable`/`foldlevel` were already listed above.

## Cheap wires — new `Config`/`Document` field + one consumer (next batches)
Each = add field + wire the named call-site + a unit test.
- whichwrap -> move_horizontally (allow h/l/arrows to cross line ends) [DONE]
- shiftround -> indent/dedent ops (round to multiple of shiftwidth) [DONE]
- joinspaces -> join_lines_below_vim (2 spaces after sentence punctuation) [DONE]
- nrformats -> increment/decrement (commands.rs ~1764): which of bin/hex/oct/alpha [DONE]
- matchpairs -> match_brackets::get_pair (configurable % pairs) [DONE]
- startofline -> G/gg/{count}G cursor-column placement (startofline_pos) [DONE; dd/C-d still first-non-blank]
- commentstring -> per-buffer comment-token override for toggle_comments (apply_comment_transaction) [DONE]
- sidescroll / sidescrolloff / scrolloffpad / scrolljump -> view horizontal/vertical scroll math
- scroll -> C-d/C-u page distance. Accepted and round-trips as `scroll-lines`, but the only reader is the mouse-wheel handler (ui/editor.rs); `page_cursor_half_down`/`_up` hardcode `inner_height()/2`
- title / titlestring / titlelen / titleold / icon -> emit OSC window-title from render loop
- makeprg / grepprg / grepformat -> :make/:grep program (hardcoded today; `errorformat` itself is wired — `qf_entry_from_line` tries the configured patterns before its built-in shape, commands.rs)
- formatprg / equalprg -> external filter path for gq / `=`
- keywordprg -> `K` keyword lookup program
- tags / tagcase / tagbsearch / taglength / tagrelative -> find_tags_file (hardcoded ./tags,tags)
- isident / isfname -> gf / identifier char classes (currently fixed char_is_word). `iskeyword` is DONE: parsed by `parse_iskeyword` and applied to word motions and text objects (commands/typed.rs)
- pumheight / pumwidth / pummaxwidth / completeitemalign -> completion popup render caps
- confirm -> default-on the existing `:confirm` behavior on quit/edit
- autowrite / autowriteall / write / writeany -> save-before-switch / write guards
- cmdheight -> command-line area layout
- winheight/winwidth/winminheight/winminwidth/winfixheight/winfixwidth -> tree split layout
- errorbells / visualbell -> BEL / screen-flash on error status
- mousemodel / mousetime / mousescroll / mousefocus -> mouse event handler
- timeoutlen / ttimeoutlen / timeout / maxmapdepth -> keymap pending-chord timeout + recursion cap
- fileignorecase / infercase -> path/word completion casing
- switchbuf / jumpoptions -> buffer-jump target + jumplist push semantics
- report -> "N lines changed" status threshold
- shellcmdflag / shellredir / shellquote / shellpipe / shelltemp -> :! shell runner
- exrc -> source project-local rc on load (like editorconfig loader)
- runtimepath / packpath / loadplugins -> runtime/plugin search + load toggle
- verbose / verbosefile / debug / shortmess / more / messagesopt -> logging/message layer
- synmaxcol / redrawtime / lazyredraw / smoothscroll / display / isprint / fillchars / termsync -> render loop
- background -> pick light/dark theme variant on theme-load
- bomb / binary / fsync -> Document write path
- undolevels -> cap History ring (history.rs)
- selection / selectmode / keymodel -> visual/select-mode extension semantics
- virtualedit -> cursor clamp/positioning (block mode already exists)
- inccommand -> live :s preview overlay
- spelllang / spell / spelloptions / spellsuggest -> spell.rs already has is_misspelled + nav; needs live underline render + toggle
- foldnestmax / foldminlines / foldopen / foldclose / foldtext / foldignore -> remaining folding subsystem knobs
  (foldcolumn and foldlevelstart shipped — see "Done")

## Subsystems (large; need buy-in)
- spell-check LIVE render + toggle (engine + nav already exist in spell.rs — highest value, nearly done)
- conceal (conceallevel/concealcursor) — syntax-driven conceal ranges + reveal-on-cursor
- backup files (backup/writebackup/backupdir/backupext/backupcopy/backupskip/patchmode)
- persistent undo (undofile/undodir/undoreload)
- swap-file recovery (swapfile/directory/updatecount)
- multi-encoding I/O (encoding/fileencoding(s)/fileencodings/charconvert/makeencoding)
- autocommands (eventignore/eventignorewin/modeline(expr)/viewoptions/mkview)
- bidi / RTL (arabic/arabicshape/rightleft/rightleftcmd/revins/allowrevins/termbidi)
- langmap/keymap key-translation layer
- cmdline-window (cedit/cmdwinheight)
- diff-mode (diff/diffopt/diffexpr/diffanchors)

## Won't build — inapplicable to a terminal modal editor
columns, lines, guifont, guifontwide, linespace, winblend, pumblend, mousehide,
winaltkeys, langmenu, menuitems, shellslash, completeslash, iconstring, helplang,
maxmempattern, pyxversion, channel, imsearch, iminsert, redrawdebug, mousemoveevent(GUI).
