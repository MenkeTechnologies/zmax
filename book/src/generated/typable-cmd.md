| Name | Description |
| --- | --- |
| `:terminal`, `:term` | Open an integrated terminal (PTY shell) running $SHELL. |
| `:ide`, `:workbench` | Enter IDE mode (file-tree sidebar + panels, like `--ide` / F2). |
| `:diff`, `:gdiff` | Open a read-only side-by-side diff of the buffer vs. its git HEAD version. |
| `:merge`, `:resolve` | Resolve the buffer's git merge conflicts in a 3-pane (ours/result/theirs) view. |
| `:magit`, `:git`, `:gst` | Open the Magit-style git status (stage/unstage/discard/commit changes by section). |
| `:hex`, `:hexview`, `:hexedit` | Open a read-only xxd-style hex viewer of a file's raw bytes (optional path; defaults to the buffer's file). |
| `:snippets`, `:snip` | Open the user snippet library editor (create/edit/delete reusable snippets). |
| `:org-cycle`, `:org-fold` | Toggle a fold over the current org heading's subtree (TAB-style outline cycling). |
| `:org-todo` | Cycle the current org heading's TODO keyword: none -> TODO -> DONE -> none. |
| `:org-promote` | Promote the current org heading one level (remove a leading star). |
| `:org-demote` | Demote the current org heading one level (add a leading star). |
| `:org-next-heading` | Move the cursor to the next org heading line. |
| `:org-prev-heading` | Move the cursor to the previous org heading line. |
| `:org-fold-all` | Fold every org heading subtree in the buffer. |
| `:org-unfold-all` | Unfold every fold in the buffer. |
| `:org-agenda`, `:agenda` | Open the org agenda: TODO/DONE items across open .org buffers and *.org files under the working directory, grouped by scheduled/deadline date. |
| `:org-priority` | Cycle the current org heading's priority cookie: none -> [#A] -> [#B] -> [#C] -> none. |
| `:org-capture`, `:capture` | Prompt for a line of text and append it as a '* TODO <text>' entry to an inbox org file (default <working-dir>/inbox.org, or an explicit path argument). |
| `:exit`, `:x`, `:xit` | Write changes to disk if the buffer is modified and then quit. Accepts an optional path (:exit some/path.txt). |
| `:exit!`, `:x!`, `:xit!` | Force write changes to disk, creating necessary subdirectories, if the buffer is modified and then quit. Accepts an optional path (:exit! some/path.txt). |
| `:quit`, `:q` | Close the current view. |
| `:help`, `:h` | Open the inline Help browser (searchable: commands, keybindings, topics). |
| `:wc`, `:words`, `:count` | Show document line/word/char counts (and selection stats). |
| `:blame` | Show git blame for the current line in the status bar. |
| `:reopen`, `:reopen-closed` | Reopen the most recently closed file. |
| `:zen` | Toggle the IDE workbench (Zen / focus mode). |
| `:emmet`, `:zencode` | Expand the emmet/zen HTML abbreviation before the cursor. |
| `:quit!`, `:q!` | Force close the current view, ignoring unsaved changes. |
| `:open`, `:o`, `:edit`, `:e` | Open a file from disk into the current view. |
| `:buffer-close`, `:bc`, `:bclose` | Close the current buffer. |
| `:buffer-close!`, `:bc!`, `:bclose!` | Close the current buffer forcefully, ignoring unsaved changes. |
| `:buffer-close-others`, `:bco`, `:bcloseother` | Close all buffers but the currently focused one. |
| `:buffer-close-others!`, `:bco!`, `:bcloseother!` | Force close all buffers but the currently focused one. |
| `:buffer-close-all`, `:bca`, `:bcloseall` | Close all buffers without quitting. |
| `:buffer-close-all!`, `:bca!`, `:bcloseall!` | Force close all buffers ignoring unsaved changes without quitting. |
| `:buffer-next`, `:bn`, `:bnext` | Goto next buffer. |
| `:buffer-previous`, `:bp`, `:bprev` | Goto previous buffer. |
| `:write`, `:w` | Write changes to disk. Accepts an optional path (:write some/path.txt) |
| `:write!`, `:w!` | Force write changes to disk creating necessary subdirectories. Accepts an optional path (:write! some/path.txt) |
| `:write-buffer-close`, `:wbc` | Write changes to disk and closes the buffer. Accepts an optional path (:write-buffer-close some/path.txt) |
| `:write-buffer-close!`, `:wbc!` | Force write changes to disk creating necessary subdirectories and closes the buffer. Accepts an optional path (:write-buffer-close! some/path.txt) |
| `:new`, `:n` | Create a new scratch buffer. |
| `:format`, `:fmt` | Format the file using an external formatter or language server. |
| `:indent-style` | Set the indentation style for editing. ('t' for tabs or 1-16 for number of spaces.) |
| `:line-ending` | Set the document's default line ending. Options: crlf, lf. |
| `:earlier`, `:ear` | Jump back to an earlier point in edit history. Accepts a number of steps or a time span. |
| `:later`, `:lat` | Jump to a later point in edit history. Accepts a number of steps or a time span. |
| `:write-quit`, `:wq` | Write changes to disk and close the current view. Accepts an optional path (:wq some/path.txt) |
| `:write-quit!`, `:wq!` | Write changes to disk and close the current view forcefully. Accepts an optional path (:wq! some/path.txt) |
| `:write-all`, `:wa` | Write changes from all buffers to disk. |
| `:write-all!`, `:wa!` | Forcefully write changes from all buffers to disk creating necessary subdirectories. |
| `:write-quit-all`, `:wqa`, `:xa` | Write changes from all buffers to disk and close all views. |
| `:write-quit-all!`, `:wqa!`, `:xa!` | Forcefully write changes from all buffers to disk, creating necessary subdirectories, and close all views (ignoring unsaved changes). |
| `:quit-all`, `:qa` | Close all views. |
| `:quit-all!`, `:qa!` | Force close all views ignoring unsaved changes. |
| `:cquit`, `:cq` | Quit with exit code (default 1). Accepts an optional integer exit code (:cq 2). |
| `:cquit!`, `:cq!` | Force quit with exit code (default 1) ignoring unsaved changes. Accepts an optional integer exit code (:cq! 2). |
| `:theme` | Change the editor theme (show current theme if no name specified). |
| `:hunk-reset`, `:reset-hunk`, `:hunk-undo` | Undo the git hunk under the cursor, restoring it from HEAD (gitsigns reset_hunk). |
| `:hunk-next`, `:next-hunk` | Move the cursor to the next git hunk. |
| `:hunk-prev`, `:prev-hunk` | Move the cursor to the previous git hunk. |
| `:conflict-ours`, `:diffget-ours`, `:conflict-keep-ours` | Resolve the merge conflict at the cursor by keeping OUR side (HEAD). |
| `:conflict-theirs`, `:diffget-theirs`, `:conflict-keep-theirs` | Resolve the merge conflict at the cursor by keeping THEIR side (incoming). |
| `:conflict-both`, `:conflict-keep-both` | Resolve the merge conflict at the cursor by keeping BOTH sides. |
| `:conflict-next` | Jump to the next merge-conflict marker. |
| `:conflict-prev` | Jump to the previous merge-conflict marker. |
| `:theme-toggle`, `:toggle-theme`, `:light-dark` | Toggle between a dark and light theme (`:theme-toggle [dark] [light]`). |
| `:theme-next` | Switch to the next theme (alphabetical). |
| `:theme-prev` | Switch to the previous theme (alphabetical). |
| `:run`, `:r!` | Run a command in the IDE Run tool window (defaults to `cargo run`). |
| `:grep`, `:rg`, `:search-project` | Search the project (ripgrep) and show jumpable results in the Run console. |
| `:shell-quote`, `:sh-quote` | Wrap the selection in safe shell single-quotes. |
| `:wrap-tag`, `:tag` | Wrap each selection in <tag>…</tag>. |
| `:csv-column`, `:csv-col` | Replace the selected CSV/TSV with just its Nth column (1-based). |
| `:code-fence`, `:fence` | Wrap the selection in a fenced Markdown code block with optional language. |
| `:md-table`, `:table-fmt` | Align the selected Markdown pipe table (pad columns, rebuild separator row). |
| `:json-query`, `:json-get` | Replace the selected JSON with the value at a dot-path (e.g. users.0.name). |
| `:json-flatten`, `:json-paths` | Flatten the selected JSON into greppable `path = value` lines. |
| `:json-to-csv`, `:json-csv` | Convert the selected JSON array of objects to CSV (sorted header union). |
| `:json-unflatten`, `:json-unpaths` | Rebuild nested JSON from `path = value` lines (inverse of :json-flatten). |
| `:toml-to-json`, `:toml-json` | Convert the selected TOML to pretty-printed JSON. |
| `:json-to-toml`, `:json-toml` | Convert the selected JSON to pretty-printed TOML. |
| `:json-sort`, `:json-sort-array` | Sort the selected JSON array (optionally by an object field: :json-sort name). |
| `:json-pick`, `:json-select` | Keep only the named keys in the selected JSON object/array (e.g. :json-pick name age). |
| `:json-omit`, `:json-drop` | Drop the named keys from the selected JSON object/array (e.g. :json-omit password). |
| `:json-unique`, `:json-uniq` | Remove duplicate elements from the selected JSON array (optionally by a field). |
| `:json-group-by`, `:json-group` | Group the selected JSON array of objects by a field (e.g. :json-group-by city). |
| `:extract`, `:matches` | Replace the selection with every regex match, one per line (group 1 if present). |
| `:filter`, `:keep-lines` | Keep only the selected lines matching a regex (in-buffer grep). |
| `:reject`, `:remove-lines` | Drop the selected lines matching a regex (in-buffer grep -v). |
| `:count-matches`, `:count-regex` | Report how many regex matches (and matching lines) are in the selection. |
| `:uniq-count`, `:frequency` | Collapse the selected lines to `count line`, sorted by frequency (uniq -c | sort -rn). |
| `:stats`, `:describe` | Show count/sum/mean/min/max of the numbers in the selection (non-destructive). |
| `:seq`, `:sequence` | Insert an integer sequence, one per line: :seq <start> <end> [step]. |
| `:field`, `:cut` | Keep only the Nth whitespace field of each selected line (awk '{print $N}'). |
| `:running-total`, `:cumsum` | Replace each numeric line with the cumulative total so far. |
| `:diff-lines`, `:deltas` | Replace each numeric line with its delta from the previous (inverse of running-total). |
| `:sum-column`, `:sumcol` | Sum the Nth whitespace field across the selected lines (non-destructive). |
| `:shuffle`, `:shuf` | Randomly reorder the selected lines (Fisher-Yates). |
| `:sample`, `:random-lines` | Keep N random lines from the selection, preserving order (:sample 10). |
| `:jsonl-to-json`, `:jsonl-json` | Convert the selected JSONL/NDJSON (one value per line) to a JSON array. |
| `:json-to-jsonl`, `:json-jsonl` | Convert the selected JSON array to JSONL (one compact value per line). |
| `:head`, `:first-lines` | Keep only the first N lines of the selection (:head 10). |
| `:tail`, `:last-lines` | Keep only the last N lines of the selection (:tail 10). |
| `:rev`, `:reverse-each-line` | Reverse the characters of each selected line independently (Unix rev). |
| `:json-table`, `:json-tbl` | Render the selected JSON array of objects as an aligned plain-text table. |
| `:hexdump`, `:xxd` | Render the selection as an xxd-style hex dump (offset, hex bytes, ASCII). |
| `:dedup`, `:unique-lines` | Remove all duplicate lines globally, keeping first occurrence and order. |
| `:caesar`, `:shift-letters` | Caesar-shift the selection's letters by N (e.g. :caesar 13 = ROT13; N may be negative). |
| `:base32-encode`, `:base32` | Base32-encode the selection (RFC 4648). |
| `:base32-decode`, `:unbase32` | Base32-decode the selection (RFC 4648). |
| `:crc32`, `:checksum` | Show the CRC32 (IEEE) checksum of the selection in hex and decimal (non-destructive). |
| `:rot47`, `:rot-47` | Apply ROT47 to the selection (rotates all printable ASCII; self-inverse). |
| `:morse-encode`, `:morse` | Encode the selection (A-Z, 0-9) to Morse code (words separated by /). |
| `:morse-decode`, `:unmorse` | Decode Morse code in the selection back to text. |
| `:human-bytes`, `:humanize-size` | Convert each numeric line (a byte count) to a human-readable size like 1.5 KiB. |
| `:ordinal`, `:ordinalize` | Convert each numeric line to its ordinal (1 → 1st, 22 → 22nd). |
| `:to-snake`, `:snake-case` | Convert the selected identifier to snake_case. |
| `:to-kebab`, `:kebab-case` | Convert the selected identifier to kebab-case. |
| `:to-camel`, `:camel-case` | Convert the selected identifier to camelCase. |
| `:to-pascal`, `:pascal-case` | Convert the selected identifier to PascalCase. |
| `:to-constant`, `:screaming-snake`, `:upper-snake` | Convert the selected identifier to CONSTANT_CASE. |
| `:to-binary`, `:text-to-binary` | Convert the selection to space-separated 8-bit binary. |
| `:from-binary`, `:binary-to-text` | Convert space-separated binary in the selection back to text. |
| `:natural-sort`, `:sort-natural` | Sort the selected lines in natural order (file2 before file10). |
| `:pad-right`, `:ljust` | Left-justify each selected line, padding with spaces to a minimum width. |
| `:pad-left`, `:rjust` | Right-justify each selected line, padding with spaces to a minimum width. |
| `:json-keys`, `:json-fields` | List the keys of the selected JSON object (or union across an array of objects). |
| `:json-type`, `:json-describe` | Show the JSON type and size of the selection in the status line (non-destructive). |
| `:after`, `:cut-after` | Keep the text after the first <delimiter> on each selected line. |
| `:before`, `:cut-before` | Keep the text before the first <delimiter> on each selected line. |
| `:swapcase`, `:invert-case` | Invert the case of each character in the selection (Hello → hELLO). |
| `:strip-invisible`, `:strip-zero-width` | Remove zero-width / invisible Unicode characters from the selection. |
| `:lines-to-json`, `:lines-to-array` | Wrap the selected lines into a JSON array of strings. |
| `:json-to-lines`, `:array-to-lines` | Unwrap a JSON array in the selection into one line per element. |
| `:checkbox-list`, `:task-list` | Turn the selected lines into a Markdown task list (- [ ] item). |
| `:unwrap-paragraphs`, `:unhardwrap` | Join hard-wrapped lines within each paragraph into single lines. |
| `:sql-in`, `:sql-in-list` | Build a SQL IN-list ('a', 'b', 'c') from the selected lines. |
| `:dec-to-hex`, `:to-hex-num` | Convert each decimal number line to hexadecimal. |
| `:hex-to-dec`, `:from-hex-num` | Convert each hexadecimal number line to decimal. |
| `:unicode-escape`, `:u-escape` | Escape non-ASCII characters in the selection as \u{XXXX}. |
| `:unicode-unescape`, `:u-unescape` | Decode \u{XXXX} and \uXXXX escapes in the selection back to characters. |
| `:sort-by-length`, `:sortlen` | Sort the selected lines by length (shortest first). |
| `:count-unique`, `:distinct-count` | Report the number of distinct vs total selected lines (non-destructive). |
| `:rotate-lines`, `:rotate` | Cyclically rotate the selected lines by N (negative rotates the other way). |
| `:unquote-lines`, `:strip-quotes-lines` | Remove surrounding quotes from each selected line independently. |
| `:quote-lines`, `:quote-each` | Wrap each selected line in double quotes (escaping \ and "). |
| `:repeat`, `:repeat-text` | Repeat the selected text N times (:repeat 3). |
| `:capitalize-lines`, `:capitalize` | Uppercase the first letter of each selected line. |
| `:remove-blank-lines`, `:remove-empty` | Remove all blank (whitespace-only) lines from the selection. |
| `:trim-lines`, `:trim` | Trim leading and trailing whitespace from each selected line. |
| `:kv-to-json`, `:env-to-json` | Convert key=value / key: value lines in the selection to a JSON object. |
| `:json-to-kv`, `:json-to-env` | Convert the selected JSON object to key=value lines. |
| `:json-pluck`, `:json-values-of` | Extract one field's value from each object in a JSON array, one per line. |
| `:to-html-list`, `:html-list` | Convert the selected lines into an HTML <ul> list. |
| `:from-html-list`, `:html-list-to-lines` | Extract <li> item text from an HTML list in the selection, one per line. |
| `:csv-to-html-table`, `:csv-to-html` | Convert the selected CSV/TSV (first row = headers) to an HTML <table>. |
| `:slugify-lines`, `:slug-lines` | Slugify each selected line independently (URL-friendly). |
| `:lines-to-csv-row`, `:join-csv` | Join the selected lines into one CSV row (RFC-4180 quoting). |
| `:csv-row-to-lines`, `:split-csv` | Split a CSV row in the selection into one field per line (quote-aware). |
| `:deslugify`, `:unslugify` | Turn a slug back into a Title Cased phrase (hyphens/underscores to spaces). |
| `:csv-to-tsv`, `:csv-tsv` | Convert the selected CSV to tab-separated values (quote-aware). |
| `:tsv-to-csv`, `:tsv-csv` | Convert the selected TSV to CSV (RFC-4180 quoting). |
| `:strip-line-numbers`, `:unnumber` | Remove a leading line number (and separator) from each selected line. |
| `:markdown-link`, `:md-link` | Wrap the selected text as a Markdown link [text](url). |
| `:extract-urls`, `:urls` | Replace the selection with the http(s) URLs found in it, one per line. |
| `:extract-emails`, `:emails` | Replace the selection with the email addresses found in it, one per line. |
| `:extract-ips`, `:ips` | Replace the selection with the IPv4 addresses found in it, one per line. |
| `:extract-quoted`, `:quoted-strings` | Replace the selection with the contents of double-quoted strings, one per line. |
| `:extract-between`, `:between` | Extract substrings between <start> and <end> delimiters, one per line. |
| `:wrap-with`, `:surround-with` | Wrap the selection with the given text on both sides (:wrap-with **). |
| `:extract-numbers`, `:numbers` | Replace the selection with the numbers found in it, one per line. |
| `:json-validate`, `:json-check` | Report whether the selection is valid JSON (with error location) — non-destructive. |
| `:csv-validate`, `:csv-check` | Check all CSV rows have the same field count (non-destructive). |
| `:ordered-list`, `:numbered-list` | Turn the selected lines into a Markdown ordered list (1. 2. 3.). |
| `:strip-list-markers`, `:unlist` | Strip leading bullet/number/checkbox list markers from each selected line. |
| `:sort-words`, `:sort-fields` | Sort the whitespace-separated words within each selected line. |
| `:unique-words`, `:dedup-words` | Remove duplicate words within each selected line (first occurrence kept). |
| `:sum-fields`, `:row-sum` | Replace each line with the sum of its numeric fields (row total). |
| `:avg-fields`, `:row-avg` | Replace each line with the mean of its numeric fields. |
| `:max-fields`, `:row-max` | Replace each line with the maximum of its numeric fields. |
| `:min-fields`, `:row-min` | Replace each line with the minimum of its numeric fields. |
| `:range-fields`, `:row-range` | Replace each line with the range (max - min) of its numeric fields. |
| `:to-env-export`, `:export-vars` | Prefix each KEY=value line with `export ` (turn a .env into shell exports). |
| `:strip-export`, `:unexport` | Remove a leading `export ` from each selected line. |
| `:dos2unix`, `:crlf-to-lf` | Convert CRLF/CR line endings in the selection to LF. |
| `:unix2dos`, `:lf-to-crlf` | Convert LF line endings in the selection to CRLF. |
| `:percent-of-total`, `:percentages` | Replace each numeric line with its percentage of the column total. |
| `:running-max`, `:cummax` | Replace each numeric line with the running maximum so far. |
| `:running-min`, `:cummin` | Replace each numeric line with the running minimum so far. |
| `:to-fixed`, `:round-to` | Format each numeric line to N decimal places (:to-fixed 2). |
| `:clamp`, `:clip` | Clamp each numeric line to the [min, max] range (:clamp 0 100). |
| `:scale`, `:multiply-by` | Multiply each numeric line by a factor (:scale 1.5). |
| `:offset`, `:add-to-each` | Add N to each numeric line (:offset 10; negative subtracts). |
| `:abs`, `:absolute-value` | Replace each numeric line with its absolute value. |
| `:linkify`, `:auto-link` | Wrap bare URLs in the selection with Markdown link syntax [url](url). |
| `:strip-markdown-links`, `:unlink` | Replace [text](url) Markdown links with just their text. |
| `:strip-emphasis`, `:strip-md-emphasis` | Remove Markdown bold/italic/code emphasis markers from the selection. |
| `:strip-html-comments`, `:strip-comments-html` | Remove <!-- ... --> HTML/Markdown comments from the selection. |
| `:remove-trailing-commas`, `:fix-trailing-commas` | Remove trailing commas before } or ] (JSON5/JS to strict JSON). |
| `:add-trailing-commas`, `:trailing-commas` | Add trailing commas before } or ] (cleaner JS/JSON5 diffs). |
| `:smart-quotes`, `:curly-quotes` | Convert straight quotes to typographic curly quotes (context-aware). |
| `:typographic-dashes`, `:em-dash` | Convert --- to em dash, -- to en dash, ... to ellipsis. |
| `:de-typography`, `:ascii-punctuation` | Normalize curly quotes/dashes/ellipsis back to ASCII punctuation. |
| `:to-ascii`, `:transliterate` | Transliterate accented Latin characters to ASCII (café → cafe). |
| `:nato`, `:phonetic` | Spell the selection in the NATO phonetic alphabet (A → Alfa). |
| `:transpose-grid`, `:transpose-ws` | Transpose a whitespace-separated grid (rows become columns). |
| `:repeat-lines`, `:duplicate-each` | Repeat each selected line N times (:repeat-lines 3). |
| `:rename-word`, `:rename-local` | Rename every whole-word occurrence of the symbol under the cursor in this buffer. |
| `:grep-word`, `:gw`, `:find-references` | Search the project for the whole word under the cursor (jumpable in Run). |
| `:todos`, `:project-todos`, `:fixme` | Scan the whole project for TODO/FIXME/HACK/XXX/BUG/NOTE markers (jumpable in Run). |
| `:registers`, `:reg`, `:display` | Show the contents of all registers. |
| `:yank-join` | Yank joined selections. A separator can be provided as first argument. Default value is newline. |
| `:clipboard-yank` | Yank main selection into system clipboard. |
| `:clipboard-yank-join` | Yank joined selections into system clipboard. A separator can be provided as first argument. Default value is newline. |
| `:primary-clipboard-yank` | Yank main selection into system primary clipboard. |
| `:primary-clipboard-yank-join` | Yank joined selections into system primary clipboard. A separator can be provided as first argument. Default value is newline. |
| `:clipboard-paste-after` | Paste system clipboard after selections. |
| `:clipboard-paste-before` | Paste system clipboard before selections. |
| `:clipboard-paste-replace` | Replace selections with content of system clipboard. |
| `:primary-clipboard-paste-after` | Paste primary clipboard after selections. |
| `:primary-clipboard-paste-before` | Paste primary clipboard before selections. |
| `:primary-clipboard-paste-replace` | Replace selections with content of system primary clipboard. |
| `:show-clipboard-provider` | Show clipboard provider name in status bar. |
| `:change-current-directory`, `:cd` | Change the current working directory. |
| `:show-directory-stack` | Show the directory stack as a <space> delimited string. |
| `:push-directory`, `:pushd` | Save and then change the current directory. |
| `:pop-directory`, `:popd` | Remove the top entry from the directory stack, and cd to the new top directory.. |
| `:show-directory`, `:pwd` | Show the current working directory. |
| `:encoding` | Set encoding. Based on `https://encoding.spec.whatwg.org`. |
| `:character-info`, `:char` | Get info about the character under the primary cursor. |
| `:reload`, `:rl` | Discard changes and reload from the source file. |
| `:reload-all`, `:rla` | Discard changes and reload all documents from the source files. |
| `:git-stage`, `:stage`, `:git-add` | Stage the current buffer's file (git add). |
| `:git-unstage`, `:unstage` | Unstage the current buffer's file (git reset HEAD). |
| `:stash`, `:git-stash` | git stash the working-tree changes (then reload open buffers). |
| `:stash-pop`, `:git-stash-pop` | git stash pop the most recent stash (then reload open buffers). |
| `:update`, `:u` | Write changes only if the file has been modified. |
| `:lsp-workspace-command` | Open workspace command picker |
| `:lsp-restart` | Restarts the given language servers, or all language servers that are used by the current file if no arguments are supplied |
| `:set`, `:se` | Set options with vim syntax (:set nu, :set nowrap, :set tw=80, :set cursorline) or native :set key value. |
| `:lsp-stop` | Stops the given language servers, or all language servers that are used by the current file if no arguments are supplied |
| `:tree-sitter-scopes` | Display tree sitter scopes, primarily for theming and development. |
| `:tree-sitter-highlight-name` | Display name of tree-sitter highlight scope under the cursor. |
| `:tree-sitter-layers` | Display language names of tree-sitter injection layers under the cursor. |
| `:debug-start`, `:dbg` | Start a debug session from a given template with given parameters. |
| `:debug-remote`, `:dbg-tcp` | Connect to a debug adapter by TCP address and start a debugging session from a given template with given parameters. |
| `:debug-eval` | Evaluate expression in current debug context. |
| `:vsplit`, `:vs` | Open the file in a vertical split. |
| `:vsplit-new`, `:vnew` | Open a scratch buffer in a vertical split. |
| `:hsplit`, `:hs`, `:sp` | Open the file in a horizontal split. |
| `:hsplit-new`, `:hnew` | Open a scratch buffer in a horizontal split. |
| `:tutor` | Open the tutorial. |
| `:goto`, `:g` | Goto line number. |
| `:set-language`, `:lang` | Set the language of current buffer (show current language if no value specified). |
| `:set-option` | Set a config option at runtime.<br>For example to disable smart case search, use `:set-option search.smart-case false`. |
| `:toggle-option`, `:toggle` | Toggle a config option at runtime.<br>For example to toggle smart case search, use `:toggle search.smart-case`. |
| `:get-option`, `:get` | Get the current value of a config option. |
| `:move-line-down` | Move the current line down by one (drag down). |
| `:move-line-up` | Move the current line up by one (drag up). |
| `:cycle-case` | Cycle the case style of the symbol under the cursor. |
| `:change-case` | Change the symbol under the cursor to camel|snake|kebab|pascal case. |
| `:left`, `:le` | Left-align line(s), setting leading indent to {n} (default 0) — vim :left. |
| `:right`, `:ri` | Right-align line(s) to width {n} (default 80) — vim :right. |
| `:center`, `:ce` | Center line(s) within width {n} (default 80) — vim :center. |
| `:undo` | Undo the last change (vim :undo). |
| `:redo`, `:red` | Redo the last undone change (vim :redo). |
| `:retab` | Replace tabs with spaces (tab-width per buffer) — vim :retab. |
| `:join`, `:j` | Join the current line(s) with the next, separated by a space (vim :j). |
| `:join!`, `:j!` | Join the current line(s) with the next, no separating space (vim :j!). |
| `:put`, `:pu` | Put (paste) a register's contents as new line(s) below the cursor (vim :put). |
| `:put!`, `:pu!` | Put (paste) a register's contents as new line(s) above the cursor (vim :put!). |
| `:delete-lines`, `:d`, `:del`, `:delete` | Delete the current line(s) into the unnamed register (vim :d). |
| `:yank-lines`, `:y`, `:ya`, `:yank` | Yank the current line(s) into the unnamed register (vim :y). |
| `:indent-lines` | Indent the current line(s) by one shiftwidth (vim :>). |
| `:dedent-lines` | Dedent the current line(s) by one shiftwidth (vim :<). |
| `:move-lines`, `:m` | Move the current line to after line {address}: :m{addr} (e.g. :m0, :m$, :m.+2). |
| `:copy-lines`, `:t`, `:co`, `:copy` | Copy the current line to after line {address}: :t{addr} (e.g. :t0, :t$). |
| `:global` | Run a command on matching lines: :g/pattern/d (delete). Also :g!/pat/d. |
| `:vglobal` | Run a command on non-matching lines: :v/pattern/d (delete). |
| `:substitute`, `:s` | Substitute: :s/pattern/replacement/[flags]. Also :%s/.../.../g for the whole file. |
| `:split-line` | Split the current line at the cursor, keeping the cursor in place. |
| `:just-one-space` | Collapse spaces and tabs around the cursor to a single space. |
| `:delete-blank-lines` | Collapse consecutive blank lines down to a single blank line. |
| `:uniquify-lines`, `:uniq` | Delete duplicate lines, keeping the first occurrence. |
| `:reverse`, `:reverse-lines`, `:tac` | Reverse the order of the selected lines (or the whole buffer). |
| `:uuid`, `:guid` | Insert a random UUID v4 at each cursor (replaces any selection). |
| `:goto-offset`, `:goto-char` | Move the cursor to an absolute character offset. |
| `:pad-numbers`, `:zero-pad` | Zero-pad every integer in the selection to <width> digits. |
| `:increment-numbers`, `:incr-numbers` | Add N (default 1; negative to decrement) to every integer in the selection. |
| `:bases`, `:base-info` | Show the selected integer in decimal, hex, octal, and binary. |
| `:lorem`, `:lipsum` | Insert N words (default 30) of lorem-ipsum placeholder text. |
| `:date` | Insert the current UTC date (YYYY-MM-DD) at each cursor. |
| `:datetime`, `:now` | Insert the current UTC date and time (YYYY-MM-DD HH:MM:SS) at each cursor. |
| `:timestamp`, `:epoch` | Insert the current Unix epoch (seconds) at each cursor. |
| `:sum`, `:total` | Sum the numbers in the selection; reports sum/avg/min/max/count in the status line. |
| `:calc`, `:eval-math` | Evaluate an arithmetic expression (+ - * / % ^), or each selection in place. |
| `:join-with`, `:joinw` | Join the selected lines into one with a separator (default ", "). |
| `:split-on`, `:splito` | Split the selected line(s) on a separator (default ",") into one item per line. |
| `:squeeze-blank-lines`, `:squeeze` | Collapse consecutive blank lines in the selection to one (cat -s). |
| `:dedup-adjacent`, `:uniq-adjacent` | Collapse consecutive duplicate lines in the selection (Unix uniq). |
| `:number-lines`, `:nl` | Prepend line numbers to the selected lines (optional start, default 1). |
| `:align`, `:tabularize` | Align the selected lines on a delimiter (default `=`) so it shares a column. |
| `:sort-by-field`, `:sortf` | Sort the selected lines by their Nth whitespace field (default 1). |
| `:sort-lines`, `:sortl` | Sort the selected lines (or the whole buffer) — vim-style line sort. |
| `:transpose-words` | Transpose the word before the cursor with the word after it. |
| `:transpose-chars` | Transpose the two characters around the cursor. |
| `:duplicate-line`, `:dup` | Duplicate the current line below. |
| `:delete-trailing-whitespace`, `:dtw` | Delete trailing whitespace from every line in the buffer. |
| `:sort` | Sort ranges in selection. |
| `:reflow` | Hard-wrap the current selection of lines to a given width. |
| `:tree-sitter-subtree`, `:ts-subtree` | Display the smallest tree-sitter subtree that spans the primary selection, primarily for debugging queries. |
| `:config-reload` | Refresh user config. |
| `:keymap` | Switch the active keymap preset: vim, helix, or emacs. |
| `:config-open` | Open the user config.toml file. |
| `:config-open-workspace` | Open the workspace config.toml file. |
| `:log-open` | Open the zemacs log file. |
| `:insert-output` | Run shell command, inserting output before each selection. |
| `:append-output` | Run shell command, appending output after each selection. |
| `:pipe`, `:\|` | Pipe each selection to the shell command. |
| `:pipe-to` | Pipe each selection to the shell command, ignoring output. |
| `:run-shell-command`, `:sh`, `:!` | Run a shell command |
| `:elisp`, `:eval-expression`, `:el` | Evaluate an Emacs Lisp expression against the editor (embedded elisprs). |
| `:vim`, `:viml`, `:vimscript` | Evaluate a Vimscript (VimL) expression via the embedded vimlrs interpreter. |
| `:awk`, `:awk-filter` | Filter the selection (or whole buffer) through an awk program (embedded awkrs). |
| `:zsh`, `:zshell` | Run a command in the embedded zsh shell (state persists); output shown in a popup. |
| `:stryke`, `:st` | Evaluate stryke (strykelang) source via the embedded interpreter (state persists). |
| `:repl` | Open the embedded-language REPL (elisp/viml/stryke/awk/zsh); optional starting language. |
| `:reset-diff-change`, `:diffget`, `:diffg` | Reset the diff change at the cursor position. |
| `:clear-register` | Clear given register. If no argument is provided, clear all registers. |
| `:set-register` | Set contents of the given register. |
| `:redraw` | Clear and re-render the whole UI |
| `:move`, `:mv` | Move the current buffer and its corresponding file to a different path |
| `:move!`, `:mv!` | Move the current buffer and its corresponding file to a different path creating necessary subdirectories |
| `:delete-file`, `:remove-file` | Delete the current buffer's file from disk and close the buffer (vim-eunuch :Delete). |
| `:chmod-x`, `:chmodx`, `:make-executable` | Make the current file executable (chmod a+x). Unix only. |
| `:mkdir` | Create a directory and any missing parents; with no arg, the current file's parent. |
| `:yank-diagnostic` | Yank diagnostic(s) under primary cursor to register, or clipboard by default |
| `:read`, `:r` | Load a file into buffer |
| `:echo` | Prints the given arguments to the statusline. |
| `:noop` | Does nothing. |
| `:workspace-trust` | Allow language servers and local config for the current workspace. |
| `:workspace-untrust` | Revoke the current workspace's trust grant or exclusion. |
| `:workspace-exclude` | Mark the current workspace as never-prompt. Never prompts for trust again. |
